//! Headless test client for the Login server (Rust port or the C++ original).
//!
//!   probe seq HOST:PORT [--ws] [--gs HOST:PORT]
//!   probe replay TRACE.jsonl LOGIN_HOST:PORT GS_HOST:PORT
//!
//! Runs a fixed request sequence with `rt`-prefixed names and prints one line per step:
//! `step | reply body hex` (or CLOSED / TIMEOUT). Requests are enciphered with key 47, as a client
//! would; replies are printed deciphered. With --gs it also plays a fake game server (5 sockets, map
//! "default" in WS1) to exercise enter-game, player data load/save and logout.
//! Without --gs the enter-game step is skipped (the C++ Login has a real HG registered).
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::{self, Message};

const KEY: u8 = 47;
const RESPONSE_LOG: u32 = 0x0AC94203;
const REQUEST_LOGIN: u32 = 0x0AC94201;
const CREATENEWACCOUNT: u32 = 0x0FC94209;
const CREATENEWCHARACTER: u32 = 0x0FC9420B;
const ENTERGAME: u32 = 0x0FC9420C;
const DELETECHARACTER: u32 = 0x0FC9420E;
const CHANGEPASSWORD: u32 = 0x0FC94217;
const GS_REGISTER: u32 = 0x0512A3F9;
const GS_REGISTER_SOCKET: u32 = 0x0512A3FB;
const GS_PLAYERDATA: u32 = 0x0C152219;
const GS_RESPONSE_PLAYERDATA: u32 = 0x0C15221A;
const GS_SAVEPLAYERDATA: u32 = 0x0DF30778;
const GS_NOSAVELOGOUT: u32 = 0x0DF30759;
const GS_ENTERGAMECONFIRM: u32 = 0x12A01009;
const GS_CONFIRMEDIP: u32 = 0x12A0100B;
const GS_RESPONSE_SAVEPLAYERDATA_REPLY: u32 = 0x0C15221B;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// msgid + msgtype + fields, each field zero padded to its width.
fn body(msgid: u32, msgtype: u16, fields: &[(&[u8], usize)], total: usize) -> Vec<u8> {
    let mut b = msgid.to_le_bytes().to_vec();
    b.extend_from_slice(&msgtype.to_le_bytes());
    for (v, n) in fields {
        let mut f = v.to_vec();
        f.resize(*n, 0);
        b.extend_from_slice(&f);
    }
    b.resize(total, 0);
    b
}

enum Conn {
    Tcp(TcpStream, proto::Splitter),
    Ws(tungstenite::WebSocket<TcpStream>),
}

impl Conn {
    fn open(addr: &str, ws: bool) -> Conn {
        let s = TcpStream::connect(addr).expect("connect");
        s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        if ws {
            let (w, _) = tungstenite::client::client(format!("ws://{addr}/"), s).expect("ws handshake");
            Conn::Ws(w)
        } else {
            Conn::Tcp(s, proto::Splitter::default())
        }
    }
    fn send(&mut self, body: &[u8]) {
        self.write_raw(&proto::encode(KEY, body));
    }
    fn write_raw(&mut self, f: &[u8]) {
        let f = f.to_vec();
        match self {
            Conn::Tcp(s, _) => s.write_all(&f).unwrap(),
            Conn::Ws(w) => w.send(Message::Binary(f.into())).unwrap(),
        }
    }
    /// Next reply body; Err("CLOSED") / Err("TIMEOUT").
    fn recv(&mut self, wait: Duration) -> Result<Vec<u8>, &'static str> {
        let end = Instant::now() + wait;
        loop {
            match self {
                Conn::Tcp(s, sp) => {
                    if let Ok(Some(f)) = sp.next_frame() {
                        return Ok(proto::decode(&f).1);
                    }
                    let mut buf = [0u8; 4096];
                    match s.read(&mut buf) {
                        Ok(0) => return Err("CLOSED"),
                        Ok(n) => sp.push(&buf[..n]),
                        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                        Err(_) => return Err("CLOSED"),
                    }
                }
                Conn::Ws(w) => match w.read() {
                    Ok(Message::Binary(b)) => return Ok(proto::decode(&b).1),
                    Ok(Message::Close(_)) => return Err("CLOSED"),
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                    Err(_) => return Err("CLOSED"),
                },
            }
            if Instant::now() > end {
                return Err("TIMEOUT");
            }
        }
    }
    /// Next body with this message id, skipping others (game-server side traffic).
    fn recv_id(&mut self, id: u32, wait: Duration) -> Result<Vec<u8>, &'static str> {
        let end = Instant::now() + wait;
        loop {
            let b = self.recv(end.saturating_duration_since(Instant::now()))?;
            if b.get(..4) == Some(&id.to_le_bytes()) {
                return Ok(b);
            }
        }
    }
}

fn step(addr: &str, ws: bool, name: &str, req: Vec<u8>, expect_close: bool) -> Vec<u8> {
    let mut c = Conn::open(addr, ws);
    c.send(&req);
    let r = c.recv(Duration::from_secs(15));
    let mut line = match &r {
        Ok(b) => hex(b),
        Err(e) => e.to_string(),
    };
    if expect_close && r.is_ok() {
        line += &format!(" then {}", c.recv(Duration::from_secs(3)).err().unwrap_or("MORE"));
    }
    println!("{name:<22}| {line}");
    r.unwrap_or_default()
}

fn login_req(acc: &[u8], pwd: &[u8], world: &[u8]) -> Vec<u8> {
    login(acc, pwd, world)
}

fn login(acc: &[u8], pwd: &[u8], world: &[u8]) -> Vec<u8> {
    body(REQUEST_LOGIN, 0, &[(acc, 10), (pwd, 10), (world, 10)], 36)
}

fn create_account(acc: &[u8], pwd: &[u8]) -> Vec<u8> {
    let f: &[(&[u8], usize)] = &[
        (acc, 10),
        (pwd, 10),
        (b"rt@example.invalid", 50),
        (b" ", 10),
        (b"30", 10),
        (b" ", 4),
        (b" ", 2),
        (b" ", 2),
        (b"Nowhere", 17),
        (b"", 28),
        (b"rt quiz?", 45),
        (b"rt answer", 20),
        (b"", 50),
    ];
    body(CREATENEWACCOUNT, 0, f, 264)
}

fn create_char(ch: &[u8], acc: &[u8], pwd: &[u8], stats: [u8; 6]) -> Vec<u8> {
    let mut b = body(CREATENEWCHARACTER, 0, &[(ch, 10), (acc, 10), (pwd, 10), (b"WS1", 30)], 66);
    b.extend_from_slice(&[1, 2, 3, 4, 5]); // gender, skin, hair style, hair colour, underwear
    b.extend_from_slice(&stats);
    b
}

fn enter_game(ch: &[u8], acc: &[u8], pwd: &[u8]) -> Vec<u8> {
    let mut b = body(ENTERGAME, 0x0F1F, &[(ch, 10), (b"", 10), (acc, 10), (pwd, 10), (b"WS1", 10)], 56);
    b.extend_from_slice(&1i32.to_le_bytes());
    b.resize(200, 0);
    b
}

fn fake_gs(addr: &str, ip: &str, port: u16, maps: &[&str]) -> Vec<Conn> {
    let mut s0 = Conn::open(addr, false);
    let mut reg = body(GS_REGISTER, 0, &[(b"rtgs", 10), (b"WS1", 10), (ip.as_bytes(), 16)], 42);
    reg.resize(48, 0);
    reg[42..44].copy_from_slice(&port.to_le_bytes());
    reg[44] = 0; // received config: no
    reg[45] = maps.len() as u8;
    for m in maps {
        let mut map = m.as_bytes().to_vec();
        map.resize(11, 0);
        reg.extend_from_slice(&map);
    }
    s0.send(&reg);
    let mut cfgs = 0;
    let resp = loop {
        let b = s0.recv(Duration::from_secs(5)).expect("gs register reply");
        if b[..4] == 0x0512A3FAu32.to_le_bytes() {
            break b;
        }
        cfgs += 1;
    };
    println!("{:<22}| {cfgs} config frames, then {}", "gs register", hex(&resp));
    let gsid = resp[6];
    let mut socks = vec![s0];
    for _ in 0..4 {
        let mut s = Conn::open(addr, false);
        let mut b = GS_REGISTER_SOCKET.to_le_bytes().to_vec();
        b.push(gsid);
        b.resize(6, 0);
        s.send(&b);
        socks.push(s);
    }
    std::thread::sleep(Duration::from_millis(500));
    socks
}

fn gs_player_data(gs: &mut Conn, ch: &[u8], acc: &[u8], pwd: &[u8]) -> Vec<u8> {
    gs.send(&body(GS_PLAYERDATA, 0, &[(ch, 10), (acc, 10), (pwd, 10), (b"127.0.0.1", 15)], 51));
    gs.recv_id(GS_RESPONSE_PLAYERDATA, Duration::from_secs(10)).expect("player data")
}

/// `"key":"value"` or `"key":123` out of one JSON line of the bridge's --record output.
fn jfield<'a>(line: &'a str, key: &str) -> &'a str {
    let k = format!("\"{key}\":");
    let rest = &line[line.find(&k).map(|i| i + k.len()).unwrap_or(line.len())..];
    let rest = rest.trim_start_matches('"');
    &rest[..rest.find(['"', ',', '}']).unwrap_or(rest.len())]
}

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
}

/// Resend every client->login frame of a recorded trace (bodies re-enciphered with their key) and compare
/// the first reply with the recorded one.
fn replay(trace: &str, addr: &str) {
    let text = std::fs::read_to_string(trace).expect("trace");
    let lines: Vec<&str> = text.lines().filter(|l| jfield(l, "target").ends_with(":4000")).collect();
    for (i, l) in lines.iter().enumerate() {
        if jfield(l, "dir") != "c2s" {
            continue;
        }
        let conn = jfield(l, "conn");
        let want = lines[i + 1..].iter().find(|r| jfield(r, "conn") == conn && jfield(r, "dir") == "s2c").map(|r| jfield(r, "body"));
        let key: u8 = jfield(l, "key").parse().unwrap();
        let mut c = Conn::open(addr, false);
        c.write_raw(&proto::encode(key, &unhex(jfield(l, "body"))));
        let got = c.recv(Duration::from_secs(15)).map(|b| hex(&b)).unwrap_or_else(|e| e.into());
        let verdict = match want {
            Some(w) if w == got => "IDENTICAL".to_string(),
            Some(w) => {
                let (a, b) = (unhex(w), unhex(&got));
                let diff: Vec<usize> = (0..a.len().max(b.len())).filter(|&i| a.get(i) != b.get(i)).collect();
                format!("DIFF at {diff:?}
    cpp  {w}
    rust {got}")
            }
            None => format!("(no recorded reply) {got}"),
        };
        println!("conn {conn} {} {} bytes: {verdict}", jfield(l, "msgid"), got.len() / 2);
    }
}

/// A loaded player-data block re-packed like HG's `_iComposePlayerDataFileContents` + `bSendMsgToLS`:
/// no CharID (0..4), QuestID as a WORD, bank flag at 241, and the message ends at the bank count
/// (HG sends 37 + Index bytes, so neither the count nor the bank items arrive).
fn hg_save_block(data: &[u8], bank_modified: bool) -> Vec<u8> {
    let n = data[751] as usize;
    let mut out = data[..752 + n * 64].to_vec();
    out[..4].fill(0);
    out[153..155].fill(0);
    out[241] = bank_modified as u8;
    out
}

/// Game-server requests for a character that does not exist (the C++ Login crashes on the save).
fn ghost(login: &str, gs: &str) {
    let mut socks = fake_gs(gs, "127.0.0.1", 3007, &["default"]);
    let (ch, acc, pwd): (&[u8], &[u8], &[u8]) = (b"RtGhost", b"rtghost", b"rtpw1");
    let mut save = GS_SAVEPLAYERDATA.to_le_bytes().to_vec();
    save.extend_from_slice(&[0, 0]);
    for (v, n) in [(ch, 10), (acc, 10), (pwd, 10)] {
        let mut f = v.to_vec();
        f.resize(n, 0);
        save.extend_from_slice(&f);
    }
    save.push(0);
    let mut blk = vec![0u8; 752 + 64];
    blk[751] = 1;
    blk[752..758].copy_from_slice(b"Dagger");
    save.extend_from_slice(&blk);
    socks[0].send(&save);
    let p = gs_player_data(&mut socks[0], ch, acc, pwd);
    println!("{:<22}| type {:04x}, {} bytes", "ghost player data", u16::from_le_bytes([p[4], p[5]]), p.len());
    step(login, false, "login still served", login_req(b"rtghost", b"rtpw1", b"WS1"), false);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("ghost") {
        // probe ghost LOGIN_HOST:PORT GS_HOST:PORT
        ghost(&args[2], &args[3]);
        return;
    }
    if args.get(1).map(String::as_str) == Some("pdata") {
        // probe pdata LOGIN_HOST:PORT GS_HOST:PORT ACCOUNT PASSWORD CHAR OUT.hex
        // Player data the login hands a game server for an existing character; no save, so the DB is untouched.
        let (acc, pwd, ch) = (args[4].as_bytes(), args[5].as_bytes(), args[6].as_bytes());
        let mut socks = fake_gs(&args[3], "127.0.0.1", 3007, &["default", "elvfarm", "aresden", "elvine"]);
        step(&args[2], false, "enter game", enter_game(ch, acc, pwd), false);
        let p = gs_player_data(&mut socks[0], ch, acc, pwd);
        std::fs::write(&args[7], hex(&p)).expect("write output");
        println!("{:<22}| {} bytes -> {}", "gs player data", p.len(), args[7]);
        let mut logout = body(GS_NOSAVELOGOUT, 0, &[(ch, 10), (acc, 10), (pwd, 10)], 36);
        logout.push(0);
        socks[0].send(&logout);
        let _ = socks[0].recv_id(GS_RESPONSE_SAVEPLAYERDATA_REPLY, Duration::from_secs(5));
        return;
    }
    if args.get(1).map(String::as_str) == Some("psave") {
        // probe psave LOGIN_HOST:PORT GS_HOST:PORT ACCOUNT PASSWORD CHAR BLOCK.hex
        // Enters the game, then saves BLOCK (a player-data reply body as written by pdata; bytes 16.. are the data).
        let (acc, pwd, ch) = (args[4].as_bytes(), args[5].as_bytes(), args[6].as_bytes());
        let block = unhex(std::fs::read_to_string(&args[7]).expect("block").trim());
        // optional 8th arg: "hg" / "hgbank" re-packs the block the way HG's save does (see hg_save_block)
        let data = match args.get(8).map(String::as_str) {
            Some("hg") => hg_save_block(&block[16..], false),
            Some("hgbank") => hg_save_block(&block[16..], true),
            _ => block[16..].to_vec(),
        };
        let mut socks = fake_gs(&args[3], "127.0.0.1", 3007, &["default", "elvfarm", "aresden", "elvine"]);
        step(&args[2], false, "enter game", enter_game(ch, acc, pwd), false);
        let gs0 = &mut socks[0];
        gs_player_data(gs0, ch, acc, pwd);
        let mut confirm = body(GS_ENTERGAMECONFIRM, 0, &[(acc, 10), (pwd, 10), (b"rtgs", 10), (b"127.0.0.1", 16)], 52);
        confirm.resize(54, 0);
        gs0.send(&confirm);
        let mut save = GS_SAVEPLAYERDATA.to_le_bytes().to_vec();
        save.extend_from_slice(&[0, 0]);
        for (v, n) in [(ch, 10), (acc, 10), (pwd, 10)] {
            let mut f = v.to_vec();
            f.resize(n, 0);
            save.extend_from_slice(&f);
        }
        save.push(0);
        save.extend_from_slice(&data);
        gs0.send(&save);
        std::thread::sleep(Duration::from_secs(2)); // the save has no reply; give the DB writes time
        let mut logout = body(GS_NOSAVELOGOUT, 0, &[(ch, 10), (acc, 10), (pwd, 10)], 36);
        logout.push(0);
        gs0.send(&logout);
        let _ = gs0.recv_id(GS_RESPONSE_SAVEPLAYERDATA_REPLY, Duration::from_secs(5));
        println!("{:<22}| sent {} bytes", "gs save", save.len());
        return;
    }
    if args.get(1).map(String::as_str) == Some("replay") {
        // probe replay TRACE LOGIN_HOST:PORT GS_HOST:PORT
        let _gs = fake_gs(&args[4], "172.27.64.1", 3007, &["default", "elvfarm"]);
        replay(&args[2], &args[3]);
        return;
    }
    let addr = args.get(2).expect("usage: probe seq HOST:PORT [--ws] [--gs HOST:PORT]").clone();
    let ws = args.iter().any(|a| a == "--ws");
    let gs_addr = args.iter().position(|a| a == "--gs").and_then(|i| args.get(i + 1).cloned());
    let (acc, pwd, pwd2, ch): (&[u8], &[u8], &[u8], &[u8]) = (b"rtacc1", b"rtpw1", b"rtpw2", b"RtChar1");
    let a = addr.as_str();
    step(a, ws, "login unknown", login(acc, pwd, b"WS1"), false);
    step(a, ws, "create account", create_account(acc, pwd), false);
    step(a, ws, "create account again", create_account(acc, pwd), false);
    step(a, ws, "login wrong pwd", login(acc, b"nope", b"WS1"), false);
    step(a, ws, "login wrong world", login(acc, pwd, b"WS9"), false);
    step(a, ws, "login", login(acc, pwd, b"WS1"), false);
    step(a, ws, "create char bad stats", create_char(ch, acc, pwd, [10, 10, 10, 10, 10, 10]), false);
    step(a, ws, "create char", create_char(ch, acc, pwd, [10, 10, 10, 14, 14, 12]), false);
    step(a, ws, "create char again", create_char(ch, acc, pwd, [10, 10, 10, 14, 14, 12]), false);
    step(a, ws, "login with char", login(acc, pwd, b"WS1"), false);
    let chpw = |n1: &[u8], n2: &[u8]| body(CHANGEPASSWORD, 0, &[(acc, 10), (pwd, 10), (n1, 10), (n2, 10)], 46);
    step(a, ws, "change pwd mismatch", chpw(pwd2, b"other"), false);
    step(a, ws, "change pwd", chpw(pwd2, pwd2), false);
    step(a, ws, "login old pwd", login(acc, pwd, b"WS1"), false);
    step(a, ws, "login new pwd", login(acc, pwd2, b"WS1"), false);
    if let Some(g) = &gs_addr {
        step(a, ws, "enter game, no gs", enter_game(ch, acc, pwd2), false);
        let mut socks = fake_gs(g, "127.0.0.1", 3007, &["default"]);
        step(a, ws, "enter game", enter_game(ch, acc, pwd2), false);
        let ip = socks[0].recv_id(GS_CONFIRMEDIP, Duration::from_secs(5)).map(|b| hex(&b)).unwrap_or_else(|e| e.into());
        println!("{:<22}| {ip}", "gs got confirmed ip");
        step(a, ws, "enter game again", enter_game(ch, acc, pwd2), false);
        step(a, ws, "delete while playing", body(DELETECHARACTER, 0, &[(ch, 10), (acc, 10), (pwd2, 10), (b"WS1", 30)], 66), false);
        let gs0 = &mut socks[0];
        let p1 = gs_player_data(gs0, ch, acc, pwd2);
        println!("{:<22}| type {:04x}, {} bytes, head {}", "gs player data", u16::from_le_bytes([p1[4], p1[5]]), p1.len(), hex(&p1[..48.min(p1.len())]));
        let mut confirm = body(GS_ENTERGAMECONFIRM, 0, &[(acc, 10), (pwd2, 10), (b"rtgs", 10), (b"127.0.0.1", 16)], 52);
        confirm.resize(54, 0); // + level
        gs0.send(&confirm);
        // save the block we were given back, then load again: the bytes must survive the round trip
        let mut save = GS_SAVEPLAYERDATA.to_le_bytes().to_vec();
        save.extend_from_slice(&[0, 0]);
        for (v, n) in [(ch, 10), (acc, 10), (pwd2, 10)] {
            let mut f = v.to_vec();
            f.resize(n, 0);
            save.extend_from_slice(&f);
        }
        save.push(0);
        save.extend_from_slice(&p1[16..]);
        gs0.send(&save);
        let p2 = gs_player_data(gs0, ch, acc, pwd2);
        let diff: Vec<usize> = (0..p1.len().max(p2.len())).filter(|&i| p1.get(i) != p2.get(i)).collect();
        println!("{:<22}| {} bytes, {} differing offsets {:?}", "gs save+reload", p2.len(), diff.len(), &diff[..diff.len().min(16)]);
        let mut logout = body(GS_NOSAVELOGOUT, 0, &[(ch, 10), (acc, 10), (pwd2, 10)], 36);
        logout.push(0); // changing server -> reply + client removed
        gs0.send(&logout);
        let r = gs0.recv_id(GS_RESPONSE_SAVEPLAYERDATA_REPLY, Duration::from_secs(5)).map(|b| hex(&b)).unwrap_or_else(|e| e.into());
        println!("{:<22}| {r}", "gs logout reply");
        // a real logout (count flag 1) must free the entry too: entering again is confirmed, not PLAYING
        step(a, ws, "enter after change", enter_game(ch, acc, pwd2), false);
        let gs0 = &mut socks[0];
        let _ = gs0.recv_id(GS_CONFIRMEDIP, Duration::from_secs(5));
        let mut logout = body(GS_NOSAVELOGOUT, 0, &[(ch, 10), (acc, 10), (pwd2, 10)], 36);
        logout.push(1);
        gs0.send(&logout);
        std::thread::sleep(Duration::from_millis(500));
        step(a, ws, "enter after logout", enter_game(ch, acc, pwd2), false);
        let _ = socks[0].recv_id(GS_CONFIRMEDIP, Duration::from_secs(5));
        let mut logout = body(GS_NOSAVELOGOUT, 0, &[(ch, 10), (acc, 10), (pwd2, 10)], 36);
        logout.push(1);
        socks[0].send(&logout);
        std::thread::sleep(Duration::from_millis(500));
        drop(socks);
        std::thread::sleep(Duration::from_millis(300));
    }
    step(a, ws, "delete unknown char", body(DELETECHARACTER, 0, &[(b"RtNoSuch", 10), (acc, 10), (pwd2, 10), (b"WS1", 30)], 66), false);
    step(a, ws, "delete wrong pwd", body(DELETECHARACTER, 0, &[(ch, 10), (acc, 10), (pwd, 10), (b"WS1", 30)], 66), true);
    step(a, ws, "delete char", body(DELETECHARACTER, 0, &[(ch, 10), (acc, 10), (pwd2, 10), (b"WS1", 30)], 66), false);
    step(a, ws, "unknown msg", body(0x12345678, 0, &[], 20), true);
    step(a, ws, "short login", login(acc, pwd2, b"WS1")[..19].to_vec(), true);
    let _ = RESPONSE_LOG;
}
