//! Sockets (the C++ XSocket + the window-message plumbing). Every socket is a tokio task that only moves frames;
//! all state lives in the world loop, which gets `Ev`s and answers through the `tx` of each connection.
//! Client frames: `[key][u16 size][body]` (proto crate) over plain TCP or one WebSocket binary message per frame.
use futures_util::{SinkExt, StreamExt};
use proto::Splitter;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;

/// MSGBUFFERSIZE: a bigger frame is XSOCKEVENT_MSGSIZETOOLARGE (the connection is dropped).
pub const MSGBUFFERSIZE: usize = 30000;

pub type Tx = UnboundedSender<Vec<u8>>;

pub enum Ev {
    /// A client connected; frames for it go to `tx` (already enciphered), dropping `tx` closes it.
    Accept { conn: u64, ip: Vec<u8>, tx: Tx },
    Read { conn: u64, key: u8, body: Vec<u8> },
    Closed { conn: u64 },
    /// Login sub-socket `i` connected / got a frame / failed or closed.
    LsUp { i: usize, tx: Tx },
    LsRead { i: usize, body: Vec<u8> },
    LsDown { i: usize },
    Stdin(String),
}

static NEXT: AtomicU64 = AtomicU64::new(1);

/// A connection id for something that is not a socket (the bots).
pub fn new_conn_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn frame_ok(f: &[u8]) -> bool {
    f.len() >= proto::HEADER && f.len() - proto::HEADER <= MSGBUFFERSIZE
}

async fn tcp_conn(mut s: TcpStream, ev: UnboundedSender<Ev>, conn: u64, mut rx: UnboundedReceiver<Vec<u8>>) {
    let mut sp = Splitter::default();
    let mut buf = vec![0u8; 8192];
    loop {
        tokio::select! {
            out = rx.recv() => match out {
                Some(f) => if s.write_all(&f).await.is_err() { break },
                None => break,
            },
            n = s.read(&mut buf) => match n {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    sp.push(&buf[..n]);
                    loop {
                        match sp.next_frame() {
                            Ok(Some(f)) if frame_ok(&f) => {
                                let (key, body) = proto::decode(&f);
                                let _ = ev.send(Ev::Read { conn, key, body });
                            }
                            Ok(None) => break,
                            _ => {
                                let _ = ev.send(Ev::Closed { conn });
                                return;
                            }
                        }
                    }
                }
            },
        }
    }
    let _ = ev.send(Ev::Closed { conn });
}

async fn ws_conn(s: TcpStream, ev: UnboundedSender<Ev>, conn: u64, mut rx: UnboundedReceiver<Vec<u8>>) {
    let Ok(ws) = tokio_tungstenite::accept_async(s).await else {
        let _ = ev.send(Ev::Closed { conn });
        return;
    };
    let (mut w, mut r) = ws.split();
    loop {
        tokio::select! {
            out = rx.recv() => match out {
                Some(f) => if w.send(Message::Binary(f.into())).await.is_err() { break },
                None => break,
            },
            m = r.next() => match m {
                Some(Ok(Message::Binary(f))) => {
                    // one message = one frame; the size field must match
                    if !frame_ok(&f) || u16::from_le_bytes([f[1], f[2]]) as usize != f.len() {
                        break;
                    }
                    let (key, body) = proto::decode(&f);
                    let _ = ev.send(Ev::Read { conn, key, body });
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
        }
    }
    let _ = w.close().await;
    let _ = ev.send(Ev::Closed { conn });
}

/// Client listener (`bListen` + `bAccept`); `ws` picks the WebSocket flavour.
pub async fn listen(addr: String, ws: bool, ev: UnboundedSender<Ev>) {
    let l = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            crate::log::log(&format!("(!!!) Cannot listen on {addr}: {e}"));
            std::process::exit(1);
        }
    };
    crate::log::log(&format!("(*) Listening for {} clients on {addr}", if ws { "WebSocket" } else { "TCP" }));
    loop {
        let Ok((s, peer)) = l.accept().await else { continue };
        let _ = s.set_nodelay(true);
        let conn = NEXT.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = unbounded_channel();
        let _ = ev.send(Ev::Accept { conn, ip: peer.ip().to_string().into_bytes(), tx });
        let ev = ev.clone();
        if ws {
            tokio::spawn(ws_conn(s, ev, conn, rx));
        } else {
            tokio::spawn(tcp_conn(s, ev, conn, rx));
        }
    }
}

/// One Login sub-socket (`m_pSubLogSock[i]->bConnect`). Reports LsUp/LsRead/LsDown; the world reconnects.
pub async fn login_socket(addr: String, i: usize, ev: UnboundedSender<Ev>) {
    let Ok(mut s) = TcpStream::connect(&addr).await else {
        let _ = ev.send(Ev::LsDown { i });
        return;
    };
    let _ = s.set_nodelay(true);
    let (tx, mut rx) = unbounded_channel::<Vec<u8>>();
    let _ = ev.send(Ev::LsUp { i, tx });
    let mut sp = Splitter::default();
    let mut buf = vec![0u8; 65536];
    loop {
        tokio::select! {
            out = rx.recv() => match out {
                Some(f) => if s.write_all(&f).await.is_err() { break },
                None => break,
            },
            n = s.read(&mut buf) => match n {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    sp.push(&buf[..n]);
                    while let Ok(Some(f)) = sp.next_frame() {
                        let _ = ev.send(Ev::LsRead { i, body: proto::decode(&f).1 });
                    }
                }
            },
        }
    }
    let _ = ev.send(Ev::LsDown { i });
}
