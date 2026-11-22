//! WebSocket <-> TCP bridge so the browser client can talk to the original C++ servers.
//! The client opens `ws://<bridge>/<host>:<port>`; one WS binary message carries exactly one frame.
//! With `--record FILE` every frame is appended, deciphered, as one JSON line.
//! `--tcp LISTEN=TARGET` adds a plain TCP proxy, so the original Windows client can be recorded too
//! (point its GM.cfg `log-server-address` at the proxy; in `game-server-mode = LAN` it reuses that
//! address for the game servers, so give one `--tcp` per login and game server port).
//!
//! usage: bridge [--listen 127.0.0.1:8000] [--allow-host 172.27.64.1 ...] [--tcp 127.0.0.1:4000=172.27.64.1:4000 ...] [--record trace.jsonl]
//! The WebSocket listener only starts when at least one --allow-host is given.

use futures_util::{SinkExt, StreamExt};
use proto::Splitter;
use std::fs::OpenOptions;
use std::io::{LineWriter, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;

type Recorder = Option<Arc<Mutex<LineWriter<std::fs::File>>>>;

struct Conn {
    id: u64,
    target: String,
    rec: Recorder,
}

impl Conn {
    fn record(&self, dir: &str, frame: &[u8]) {
        let Some(rec) = &self.rec else { return };
        let (key, body) = proto::decode(frame);
        let msgid = body.get(..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
        let line = format!(
            "{{\"t\":{t},\"conn\":{},\"target\":\"{}\",\"dir\":\"{dir}\",\"key\":{key},\"msgid\":\"0x{msgid:08x}\",\"body\":\"{hex}\"}}",
            self.id, self.target
        );
        let _ = writeln!(rec.lock().unwrap(), "{line}");
    }
}

/// `/host:port` -> `host:port` when host is allowed. The target is checked here because it comes from the browser.
fn target_from_path(path: &str, allowed: &[String]) -> Option<String> {
    let (host, port) = path.strip_prefix('/')?.rsplit_once(':')?;
    port.parse::<u16>().ok()?;
    allowed.iter().any(|h| h == host).then(|| format!("{host}:{port}"))
}

async fn handle(stream: TcpStream, allowed: Arc<Vec<String>>, id: u64, rec: Recorder) {
    let mut target = None;
    let ws = tokio_tungstenite::accept_hdr_async(stream, |req: &Request, resp: Response| {
        target = target_from_path(req.uri().path(), &allowed);
        if target.is_some() {
            Ok(resp)
        } else {
            let mut e = ErrorResponse::new(Some("target not allowed".into()));
            *e.status_mut() = StatusCode::FORBIDDEN;
            Err(e)
        }
    })
    .await;
    let (Ok(ws), Some(target)) = (ws, target) else { return };
    let tcp = match TcpStream::connect(&target).await {
        Ok(t) => t,
        Err(e) => return eprintln!("[{id}] connect {target}: {e}"),
    };
    let _ = tcp.set_nodelay(true);
    eprintln!("[{id}] open {target}");
    let conn = Conn { id, target, rec };
    let (mut tcp_rd, mut tcp_wr) = tcp.into_split();
    let (mut ws_tx, mut ws_rx) = ws.split();

    let c2s = async {
        let mut split = Splitter::default();
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Binary(data) => {
                    split.push(&data);
                    while let Ok(Some(f)) = split.next_frame() {
                        conn.record("c2s", &f);
                    }
                    if tcp_wr.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    };
    let s2c = async {
        let mut split = Splitter::default();
        let mut buf = vec![0u8; 65536];
        loop {
            let n = match tcp_rd.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            split.push(&buf[..n]);
            loop {
                match split.next_frame() {
                    Ok(Some(f)) => {
                        conn.record("s2c", &f);
                        if ws_tx.send(Message::Binary(f.into())).await.is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => return eprintln!("[{id}] bad frame size from server: {}", e.0),
                }
            }
        }
        let _ = ws_tx.send(Message::Close(None)).await;
    };
    tokio::select! { _ = c2s => {}, _ = s2c => {} }
    eprintln!("[{id}] closed {}", conn.target);
}

/// Copies one direction byte for byte, recording each whole frame that passes.
async fn copy_recording(
    mut rd: tokio::net::tcp::OwnedReadHalf,
    mut wr: tokio::net::tcp::OwnedWriteHalf,
    conn: &Conn,
    dir: &str,
) {
    let mut split = Splitter::default();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = match rd.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if wr.write_all(&buf[..n]).await.is_err() {
            return;
        }
        split.push(&buf[..n]);
        loop {
            match split.next_frame() {
                Ok(Some(f)) => conn.record(dir, &f),
                Ok(None) => break,
                // keep forwarding: the game decides what a bad frame means, we only stop recording it
                Err(_) => {
                    split = Splitter::default();
                    eprintln!("[{}] {dir}: bad frame size, recording resets", conn.id);
                    break;
                }
            }
        }
    }
}

async fn tcp_proxy(client: TcpStream, target: String, id: u64, rec: Recorder) {
    let server = match TcpStream::connect(&target).await {
        Ok(s) => s,
        Err(e) => return eprintln!("[{id}] connect {target}: {e}"),
    };
    let _ = client.set_nodelay(true);
    let _ = server.set_nodelay(true);
    eprintln!("[{id}] tcp open {target}");
    let conn = Conn { id, target, rec };
    let (crd, cwr) = client.into_split();
    let (srd, swr) = server.into_split();
    tokio::select! {
        _ = copy_recording(crd, swr, &conn, "c2s") => {},
        _ = copy_recording(srd, cwr, &conn, "s2c") => {},
    }
    eprintln!("[{id}] tcp closed {}", conn.target);
}

async fn serve_tcp(listen: String, target: String, next_id: Arc<AtomicU64>, rec: Recorder) -> std::io::Result<()> {
    let listener = TcpListener::bind(&listen).await.inspect_err(|e| eprintln!("tcp proxy {listen}: {e}"))?;
    eprintln!("tcp proxy {listen} -> {target}");
    loop {
        let (stream, _) = listener.accept().await?;
        let id = next_id.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(tcp_proxy(stream, target.clone(), id, rec.clone()));
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut listen = "127.0.0.1:8000".to_string();
    let mut allowed = Vec::new();
    let mut tcp = Vec::new();
    let mut record = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().expect("missing value after flag");
        match a.as_str() {
            "--listen" => listen = val(),
            "--allow-host" => allowed.push(val()),
            "--record" => record = Some(val()),
            "--tcp" => {
                let v = val();
                let (l, t) = v.split_once('=').expect("--tcp wants LISTEN=TARGET");
                tcp.push((l.to_string(), t.to_string()));
            }
            _ => panic!("unknown argument {a}"),
        }
    }
    assert!(!allowed.is_empty() || !tcp.is_empty(), "give --allow-host and/or --tcp");
    let rec: Recorder = match record {
        Some(p) => Some(Arc::new(Mutex::new(LineWriter::new(
            OpenOptions::new().create(true).append(true).open(p)?,
        )))),
        None => None,
    };
    let next_id = Arc::new(AtomicU64::new(1));
    let mut proxies = tokio::task::JoinSet::new();
    for (l, t) in tcp {
        proxies.spawn(serve_tcp(l, t, next_id.clone(), rec.clone()));
    }
    if allowed.is_empty() {
        // only TCP proxies: run until one of them fails (e.g. port in use)
        return proxies.join_next().await.map_or(Ok(()), |r| r.expect("proxy task panicked"));
    }
    let allowed = Arc::new(allowed);
    let listener = TcpListener::bind(&listen).await?;
    eprintln!("bridge on ws://{listen}/<host>:<port>, allowed hosts {allowed:?}");
    loop {
        let (stream, _) = listener.accept().await?;
        let id = next_id.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(handle(stream, allowed.clone(), id, rec.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::target_from_path;

    #[test]
    fn target_check() {
        let ok = vec!["172.27.64.1".to_string()];
        assert_eq!(target_from_path("/172.27.64.1:4000", &ok).as_deref(), Some("172.27.64.1:4000"));
        assert_eq!(target_from_path("/10.0.0.1:4000", &ok), None);
        assert_eq!(target_from_path("/172.27.64.1:99999", &ok), None);
        assert_eq!(target_from_path("/172.27.64.1", &ok), None);
    }
}
