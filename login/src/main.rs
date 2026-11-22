//! Helbreath Login server, ported from the C++ `Login` project (LoginServer.cpp & co).
//!
//! usage: login [--config LServer.cfg] [--config-dir DIR] [--bind 0.0.0.0] [--ws-port N] [--tcp-port N]
//!              [--gs-port 5656] [--no-db-maintenance]
//!
//! Clients speak WebSocket (one binary message = one frame) on --ws-port (default: login-server-port
//! from the cfg) and, with --tcp-port, plain TCP. Game servers use plain TCP on --gs-port, the C++
//! "gate server" port (hard-coded 5656 there). Logs go to Events.log / Logs/ in the working directory.
//! stdin commands replace the window hotkeys: `shutdown` (F1+F4), `reload` (F1+F5).
mod db;
mod log;
mod msg;
mod party;
mod server;
mod util;

use futures_util::{SinkExt, StreamExt};
use proto::Splitter;
use server::{Cfg, Ev, Server};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use util::{atoi, l1, StrTok};

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

struct ProgramCfg {
    listen_port: u16,
    mysql_addr: String,
    mysql_user: String,
    mysql_pwd: String,
    mysql_port: u16,
    mysql_db: String,
    permitted: Vec<Vec<u8>>,
}

/// `ReadProgramConfigFile`
fn read_program_config(path: &PathBuf) -> Option<ProgramCfg> {
    let Ok(raw) = std::fs::read(path) else {
        log::log_lv("(ERROR) Cannot open configuration file.", log::WARN_MSG);
        return None;
    };
    let data = util::text_mode(&raw);
    let mut c = ProgramCfg { listen_port: 0, mysql_addr: String::new(), mysql_user: String::new(), mysql_pwd: String::new(), mysql_port: 0, mysql_db: String::new(), permitted: Vec::new() };
    let mut t = StrTok::new(&data, b"= \t\n");
    while let Some(tok) = t.get() {
        let mut val = || t.get().unwrap_or_default();
        match tok.as_slice() {
            b"login-server-port" => {
                c.listen_port = atoi(&val()) as u16;
                log::log(&format!("(*) Login server port : {}", c.listen_port));
            }
            b"external-address" => log::log(&format!("(*) server external address : {}", l1(&val()))),
            b"mysql-address" => {
                c.mysql_addr = l1(&val());
                log::log(&format!("(*) mySql server address : {}", c.mysql_addr));
            }
            b"mysql-user" => {
                c.mysql_user = l1(&val());
                log::log(&format!("(*) mySql server User : {}", c.mysql_user));
            }
            b"mysql-password" => {
                c.mysql_pwd = l1(&val());
                log::log(&format!("(*) mySql server password : {}", c.mysql_pwd));
            }
            b"mysql-server-port" => {
                c.mysql_port = atoi(&val()) as u16;
                log::log(&format!("(*) mySql Server port : {}", c.mysql_port));
            }
            b"mysql-database" => {
                c.mysql_db = l1(&val());
                log::log(&format!("(*) mySql server database : {}", c.mysql_db));
            }
            b"permitted-address" => {
                let v = val();
                if c.permitted.len() < msg::MAXGAMESERVERS {
                    log::log(&format!("(*) IP [{}] added to permitted addresses list!", l1(&v)));
                    c.permitted.push(v);
                }
            }
            _ => {}
        }
    }
    if c.listen_port == 0 || c.mysql_port == 0 || c.mysql_addr.is_empty() {
        log::log_lv("(ERROR) Info is missing in config file, unable to start server", log::WARN_MSG);
        return None;
    }
    Some(c)
}

/// One TCP connection (client or game server): frames in, frames out; dropping the sender closes it.
async fn tcp_conn(stream: TcpStream, gs: bool, ev: UnboundedSender<Ev>) {
    let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let _ = stream.set_nodelay(true);
    let conn = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = unbounded_channel::<Vec<u8>>();
    if ev.send(Ev::Open { gs, conn, peer, tx }).is_err() {
        return;
    }
    let (mut rd, mut wr) = stream.into_split();
    let mut split = Splitter::default();
    let mut buf = vec![0u8; 16384];
    loop {
        tokio::select! {
            biased;
            out = rx.recv() => match out {
                Some(f) => if wr.write_all(&f).await.is_err() { break },
                None => break,
            },
            n = rd.read(&mut buf) => {
                let Ok(n @ 1..) = n else { break };
                split.push(&buf[..n]);
                let mut bad = false;
                loop {
                    match split.next_frame() {
                        Ok(Some(frame)) => { let _ = ev.send(Ev::Frame { gs, conn, frame }); }
                        Ok(None) => break,
                        Err(_) => { bad = true; break; }
                    }
                }
                if bad { break }
            }
        }
    }
    let _ = wr.shutdown().await;
    let _ = ev.send(Ev::Closed { gs, conn });
}

/// A WebSocket client: each binary message carries whole frames.
async fn ws_conn(stream: TcpStream, ev: UnboundedSender<Ev>) {
    let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let _ = stream.set_nodelay(true);
    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else { return };
    let conn = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = unbounded_channel::<Vec<u8>>();
    if ev.send(Ev::Open { gs: false, conn, peer, tx }).is_err() {
        return;
    }
    let (mut ws_tx, mut ws_rx) = ws.split();
    let mut split = Splitter::default();
    loop {
        tokio::select! {
            biased;
            out = rx.recv() => match out {
                Some(f) => if ws_tx.send(Message::Binary(f.into())).await.is_err() { break },
                None => break,
            },
            m = ws_rx.next() => match m {
                Some(Ok(Message::Binary(b))) => {
                    split.push(&b);
                    let mut bad = false;
                    loop {
                        match split.next_frame() {
                            Ok(Some(frame)) => { let _ = ev.send(Ev::Frame { gs: false, conn, frame }); }
                            Ok(None) => break,
                            Err(_) => { bad = true; break; }
                        }
                    }
                    if bad { break }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
        }
    }
    let _ = ws_tx.close().await;
    let _ = ev.send(Ev::Closed { gs: false, conn });
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

async fn listen(addr: String) -> TcpListener {
    match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot listen on {addr}: {e}");
            std::process::exit(1);
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg_path = PathBuf::from(arg(&args, "--config").unwrap_or_else(|| "LServer.cfg".into()));
    let Some(pc) = read_program_config(&cfg_path) else { std::process::exit(1) };
    let config_dir = arg(&args, "--config-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| cfg_path.parent().unwrap_or(std::path::Path::new(".")).join("Config"));
    // the C++ binds to the host's first address (gethostbyname(gethostname()))
    let bind = arg(&args, "--bind").unwrap_or_else(|| "0.0.0.0".into());
    let port = |n: &str, d: u16| arg(&args, n).and_then(|v| v.parse().ok()).unwrap_or(d);
    let ws_port = port("--ws-port", pc.listen_port);
    let gs_port = port("--gs-port", 5656);
    let tcp_port = arg(&args, "--tcp-port").and_then(|v| v.parse::<u16>().ok());
    let listen_all = pc.permitted.is_empty();
    let cfg = Cfg {
        db: db::DbCfg { host: pc.mysql_addr, port: pc.mysql_port, user: pc.mysql_user, pwd: pc.mysql_pwd },
        db_name: pc.mysql_db,
        permitted: pc.permitted,
        listen_all,
        config_dir,
        db_maintenance: !args.iter().any(|a| a == "--no-db-maintenance"),
    };
    let mut srv = Server::new(cfg);
    if !srv.init().await {
        std::process::exit(1);
    }

    let (ev_tx, mut ev_rx) = unbounded_channel::<Ev>();
    let ws = listen(format!("{bind}:{ws_port}")).await;
    let gs = listen(format!("{bind}:{gs_port}")).await;
    log::log(&format!("(*) Listening: clients ws://{bind}:{ws_port}{}, game servers tcp {bind}:{gs_port}", tcp_port.map(|p| format!(" and tcp {bind}:{p}")).unwrap_or_default()));
    {
        let ev = ev_tx.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = ws.accept().await {
                tokio::spawn(ws_conn(s, ev.clone()));
            }
        });
    }
    {
        let ev = ev_tx.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = gs.accept().await {
                tokio::spawn(tcp_conn(s, true, ev.clone()));
            }
        });
    }
    if let Some(p) = tcp_port {
        let l = listen(format!("{bind}:{p}")).await;
        let ev = ev_tx.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = l.accept().await {
                tokio::spawn(tcp_conn(s, false, ev.clone()));
            }
        });
    }
    {
        let ev = ev_tx.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let _ = ev.send(Ev::Cmd(l));
            }
        });
    }
    drop(ev_tx);

    // the C++ timer (timeSetEvent, 300 ms) runs on its own thread; here it interleaves with the messages
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(msg::MAINTIMERSIZE));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            ev = ev_rx.recv() => match ev {
                Some(ev) => srv.handle(ev).await,
                None => break,
            },
            _ = tick.tick() => srv.on_timer().await,
        }
    }
}
