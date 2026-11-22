//! Headless test client for a game server (the Rust HG or the C++ HGserver), through a Login.
//!
//!   hgprobe play LOGIN_HOST:PORT MAPDATA_DIR OUT.jsonl
//!
//! Creates rthg1/RtHg1 and rthg2/RtHg2 when missing (through the Login), puts both on `default` next to the
//! elvfarm teleport (SQL on the hb-mysql container, rt* rows only), then: both enter the game, RtHg1 chats,
//! RtHg2 whispers to RtHg1 (/to), RtHg1 walks south onto the teleport tile (RtHg2 must see the steps), asks for the
//! teleport (same server -> new RESPONSE_INITDATA; other server -> NOTIFY_SERVERCHANGE, reconnect through the
//! Login, enter again), walks two tiles on elvfarm and both log out. Every check prints `ok`/`FAIL`; the exit
//! code is the number of failures. Every server->client body is written to OUT.jsonl for diffing runs.
//!
//! With `--ws2 HOST:PORT` RtHg2 talks to the game server over WebSocket at that address.
//!
//!   hgprobe fight LOGIN_HOST:PORT MAPDATA_DIR OUT.jsonl   (see hgprobe/fight.rs)
//!
//!   hgprobe pvp LOGIN_HOST:PORT MAPDATA_DIR OUT.jsonl     (see hgprobe/pvp.rs)
//!
//!   hgprobe deaths LOGIN_HOST:PORT MAPDATA_DIR OUT.jsonl  (see hgprobe/deaths.rs)
//!
//!   hgprobe farming|recallpoint|polymorph LOGIN_HOST:PORT MAPDATA_DIR OUT.jsonl   (see hgprobe/farming.rs, magic.rs)
//!
//!   hgprobe leftovers|friends LOGIN OUT.jsonl, hgprobe eventparty|mobevent LOGIN MAPDATA OUT.jsonl   (see hgprobe/leftovers.rs)
//!
//!   hgprobe leftovers3 LOGIN OUT.jsonl   (reward gold, hero items: see hgprobe/leftovers3.rs)
//!   hgprobe leftovers2 LOGIN OUT.jsonl   (legion point services, respec, Gail's DK items, observer panning: see hgprobe/leftovers2.rs)
//!
//!   hgprobe corpse LOGIN_HOST:PORT OUT.jsonl [lost]   (skill 10 persistence, see hgprobe/skills.rs)
//!
//!   hgprobe clean      delete the rt* characters (items, bank, skills) and accounts again (SQL)
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

#[allow(dead_code)]
#[path = "../src/consts.rs"]
mod consts;
use consts::*;

#[path = "hgprobe/fight.rs"]
mod fight;
#[path = "hgprobe/pvp.rs"]
mod pvp;
#[path = "hgprobe/items.rs"]
mod items;
#[path = "hgprobe/magic.rs"]
mod magic;
#[path = "hgprobe/skills.rs"]
mod skills;
#[path = "hgprobe/quest.rs"]
mod quest;
#[path = "hgprobe/crusade.rs"]
mod crusade;
#[path = "hgprobe/heldenian.rs"]
mod heldenian;
#[path = "hgprobe/apocalypse.rs"]
mod apocalypse;
#[path = "hgprobe/astoria.rs"]
mod astoria;
#[path = "hgprobe/scheduler.rs"]
mod scheduler;
#[path = "hgprobe/gm382.rs"]
mod gm382;
#[path = "hgprobe/gm.rs"]
mod gm;
#[path = "hgprobe/noticement.rs"]
mod noticement;
#[path = "hgprobe/deaths.rs"]
mod deaths;
#[path = "hgprobe/fightzone.rs"]
mod fightzone;
#[path = "hgprobe/social.rs"]
mod social;
#[path = "hgprobe/farming.rs"]
mod farming;
#[path = "hgprobe/leftovers.rs"]
mod leftovers;
#[path = "hgprobe/leftovers2.rs"]
mod leftovers2;
#[path = "hgprobe/leftovers3.rs"]
mod leftovers3;

const KEY: u8 = 47;
const MSGTYPE_CONFIRM_: u16 = MSGTYPE_CONFIRM as u16;
const MSGTYPE_REJECT_: u16 = MSGTYPE_REJECT as u16;
const CREATENEWACCOUNT: u32 = 0x0FC94209;
const CREATENEWCHARACTER: u32 = 0x0FC9420B;
const ENTER_NEW: u16 = ENTERGAMEMSGTYPE_NEW as u16;
const ENTER_CHANGING: u16 = ENTERGAMEMSGTYPE_CHANGINGSERVER as u16;
const ENTERRES_CONFIRM: u16 = ENTERGAMERESTYPE_CONFIRM as u16;
const REQUEST_INITPLAYER: u32 = MSGID_REQUEST_INITPLAYER;
const RESPONSE_INITPLAYER: u32 = MSGID_RESPONSE_INITPLAYER;
const RESPONSE_INITDATA: u32 = MSGID_RESPONSE_INITDATA;
const PLAYERCHARACTERCONTENTS: u32 = MSGID_PLAYERCHARACTERCONTENTS;
const PLAYERITEMLISTCONTENTS: u32 = MSGID_PLAYERITEMLISTCONTENTS;
const COMMAND_MOTION: u32 = MSGID_COMMAND_MOTION;
const RESPONSE_MOTION: u32 = MSGID_RESPONSE_MOTION;
const EVENT_MOTION: u32 = MSGID_EVENT_MOTION;
const EVENT_LOG: u32 = MSGID_EVENT_LOG;
const COMMAND_CHATMSG: u32 = MSGID_COMMAND_CHATMSG;
const COMMAND_CHECKCONNECTION: u32 = MSGID_COMMAND_CHECKCONNECTION;
const REQUEST_TELEPORT: u32 = MSGID_REQUEST_TELEPORT;
const NOTIFY: u32 = MSGID_NOTIFY;
const NOTIFY_SERVERCHANGE_: u16 = NOTIFY_SERVERCHANGE as u16;
const NOTIFY_WHISPERMODEON_: u16 = NOTIFY_WHISPERMODEON as u16;
const OBJECTMOVE_: u16 = OBJECTMOVE as u16;
const OBJECTMOVE_CONFIRM_: u16 = OBJECTMOVE_CONFIRM as u16;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn u16at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b.get(o).copied().unwrap_or(0), b.get(o + 1).copied().unwrap_or(0)])
}
fn u32at(b: &[u8], o: usize) -> u32 {
    u16at(b, o) as u32 | (u16at(b, o + 2) as u32) << 16
}
fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// msgid + msgtype + fields, each zero padded to its width.
fn body(msgid: u32, msgtype: u16, fields: &[(&[u8], usize)], total: usize) -> Vec<u8> {
    let mut b = msgid.to_le_bytes().to_vec();
    b.extend_from_slice(&msgtype.to_le_bytes());
    for (v, n) in fields {
        let mut f = v.to_vec();
        f.resize(*n, 0);
        b.extend_from_slice(&f);
    }
    b.resize(total.max(b.len()), 0);
    b
}

struct Conn {
    s: TcpStream,
    ws: Option<tungstenite::WebSocket<TcpStream>>,
    sp: proto::Splitter,
    who: u8,
    log: Option<std::fs::File>,
    t0: Instant,
}

impl Conn {
    fn open(addr: &str, who: u8, log: Option<std::fs::File>) -> Conn {
        let s = TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect {addr}: {e}"));
        s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        Conn { s, ws: None, sp: proto::Splitter::default(), who, log, t0: Instant::now() }
    }
    /// The same over WebSocket (one binary message = one frame).
    fn open_ws(addr: &str, who: u8, log: Option<std::fs::File>) -> Conn {
        let mut c = Conn::open(addr, who, log);
        c.s.set_read_timeout(None).unwrap();
        let (w, _) = tungstenite::client::client(format!("ws://{addr}/"), c.s.try_clone().unwrap()).expect("ws handshake");
        // the clone is a separate handle on Windows: its read timeout must be set on it
        w.get_ref().set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        c.ws = Some(w);
        c
    }
    fn send(&mut self, body: &[u8]) {
        let f = proto::encode(KEY, body);
        match &mut self.ws {
            Some(w) => drop(w.send(tungstenite::Message::Binary(f.into()))),
            None => drop(self.s.write_all(&f)),
        }
    }
    fn recv(&mut self, wait: Duration) -> Option<Vec<u8>> {
        let end = Instant::now() + wait;
        loop {
            if let Ok(Some(f)) = self.sp.next_frame() {
                let (key, b) = proto::decode(&f);
                if let Some(l) = &mut self.log {
                    let _ = writeln!(l, "{{\"p\":{},\"t\":{},\"key\":{key},\"msgid\":\"0x{:08x}\",\"type\":\"0x{:04x}\",\"body\":\"{}\"}}", self.who, self.t0.elapsed().as_millis(), u32at(&b, 0), u16at(&b, 4), hex(&b));
                }
                return Some(b);
            }
            if let Some(w) = &mut self.ws {
                match w.read() {
                    Ok(tungstenite::Message::Binary(b)) => self.sp.push(&b),
                    Ok(tungstenite::Message::Close(_)) => return None,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                    Err(_) => return None,
                }
                if Instant::now() > end {
                    return None;
                }
                continue;
            }
            let mut buf = [0u8; 65536];
            match self.s.read(&mut buf) {
                Ok(0) => return None,
                Ok(n) => self.sp.push(&buf[..n]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => return None,
            }
            if Instant::now() > end {
                return None;
            }
        }
    }
    /// Everything that arrives within `wait`.
    fn drain(&mut self, wait: Duration) -> Vec<Vec<u8>> {
        let end = Instant::now() + wait;
        let mut v = Vec::new();
        while let Some(b) = self.recv(end.saturating_duration_since(Instant::now())) {
            v.push(b);
        }
        v
    }
    /// First body matching `f`, keeping the others in `seen`.
    fn wait_for(&mut self, wait: Duration, seen: &mut Vec<Vec<u8>>, f: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
        let end = Instant::now() + wait;
        while let Some(b) = self.recv(end.saturating_duration_since(Instant::now())) {
            let hit = f(&b);
            seen.push(b.clone());
            if hit {
                return Some(b);
            }
        }
        None
    }
}

struct Checks(u32);
impl Checks {
    fn check(&mut self, ok: bool, what: &str) {
        println!("{} {what}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            self.0 += 1;
        }
    }
}

fn sql(q: &str) -> String {
    let out = std::process::Command::new("docker").args(["exec", "hb-mysql", "mysql", "-N", "-uhelbreath", "-phelbreath", "helbreath", "-e", q]).output().expect("docker");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn login_once(login: &str, b: Vec<u8>) -> Option<Vec<u8>> {
    let mut c = Conn::open(login, 0, None);
    c.send(&b);
    c.recv(Duration::from_secs(10))
}

fn ensure_char(login: &str, acc: &[u8], ch: &[u8]) {
    let pwd: &[u8] = b"rtpw1";
    let f: &[(&[u8], usize)] = &[(acc, 10), (pwd, 10), (b"rt@example.invalid", 50), (b" ", 10), (b"30", 10), (b" ", 4), (b" ", 2), (b" ", 2), (b"Nowhere", 17), (b"", 28), (b"rt quiz?", 45), (b"rt answer", 20), (b"", 50)];
    let _ = login_once(login, body(CREATENEWACCOUNT, 0, f, 264));
    let mut b = body(CREATENEWCHARACTER, 0, &[(ch, 10), (acc, 10), (pwd, 10), (b"WS1", 30)], 66);
    b.extend_from_slice(&[1, 1, 1, 1, 1, 10, 10, 10, 14, 14, 12]); // male, skin 1, hair, colour, underwear, stats
    let _ = login_once(login, b);
}

/// Login ENTERGAME -> (gs address, port).
fn enter_game(login: &str, acc: &[u8], ch: &[u8], ty: u16) -> Option<(String, u16)> {
    let mut b = body(MSGID_REQUEST_ENTERGAME, ty, &[(ch, 10), (b"", 10), (acc, 10), (b"rtpw1", 10), (b"WS1", 10)], 56);
    b.extend_from_slice(&1i32.to_le_bytes());
    b.resize(200, 0);
    let r = login_once(login, b)?;
    if u32at(&r, 0) != MSGID_RESPONSE_ENTERGAME || u16at(&r, 4) != ENTERRES_CONFIRM {
        println!("     enter game reply {}", hex(&r));
        return None;
    }
    Some((String::from_utf8_lossy(cstr(&r[6..22])).to_string(), u16at(&r, 22)))
}

struct Player {
    c: Conn,
    id: u16,
    x: i16,
    y: i16,
    map: Vec<u8>,
    items: Vec<u8>, // the PLAYERITEMLISTCONTENTS body
    npcs: Vec<(u16, u16, String)>,
    /// the notifications that came right after RESPONSE_INITDATA
    entry: Vec<Vec<u8>>,
}

/// INITPLAYER on the game server; returns the player once RESPONSE_INITDATA arrived.
fn init_player(gs: &(String, u16), acc: &[u8], ch: &[u8], who: u8, log: &std::fs::File, k: &mut Checks, ws: Option<&str>) -> Option<Player> {
    let l = Some(log.try_clone().unwrap());
    let mut c = match ws {
        Some(a) => Conn::open_ws(a, who, l),
        None => Conn::open(&format!("{}:{}", gs.0, gs.1), who, l),
    };
    let mut b = body(REQUEST_INITPLAYER, 0, &[(ch, 10), (acc, 10), (b"rtpw1", 10), (b"", 1), (b"WS1", 20)], 58);
    b.truncate(58);
    c.send(&b);
    let mut seen = Vec::new();
    let d = c.wait_for(Duration::from_secs(10), &mut seen, |b| u32at(b, 0) == RESPONSE_INITDATA);
    let ids: Vec<u32> = seen.iter().map(|b| u32at(b, 0)).collect();
    k.check(ids.contains(&RESPONSE_INITPLAYER) && seen.iter().any(|b| u32at(b, 0) == RESPONSE_INITPLAYER && u16at(b, 4) == MSGTYPE_CONFIRM_), &format!("P{who} RESPONSE_INITPLAYER confirm"));
    k.check(seen.iter().any(|b| u32at(b, 0) == PLAYERCHARACTERCONTENTS && b.len() == 115), &format!("P{who} PLAYERCHARACTERCONTENTS (115 bytes)"));
    let items = seen.iter().find(|b| u32at(b, 0) == PLAYERITEMLISTCONTENTS).cloned();
    k.check(items.as_ref().is_some_and(|b| b[6] >= 1), &format!("P{who} item list ({} bag items)", items.as_ref().map_or(0, |b| b[6])));
    let d = d?;
    let (id, x, y) = (u16at(&d, 6), u16at(&d, 8) as i16 + 19, u16at(&d, 10) as i16 + 17);
    let map = cstr(&d[30..40]).to_vec();
    k.check(id != 0 && d.len() >= 68, &format!("P{who} RESPONSE_INITDATA id {id} at {}({x},{y}), {} bytes", String::from_utf8_lossy(&map), d.len()));
    let npcs = map_npcs(&d[66..]);
    println!("     P{who} NPCs in view: {npcs:?}");
    if map == b"default" {
        k.check(!npcs.is_empty(), &format!("P{who} sees NPCs in the first map data ({})", npcs.len()));
    }
    let entry = c.drain(Duration::from_millis(600)); // the entry notifications
    Some(Player { c, id, x, y, map, items: items.unwrap_or_default(), npcs, entry })
}

/// (object id, type, name) of every NPC in a map-data block (iComposeInitMapData layout).
fn map_npcs(md: &[u8]) -> Vec<(u16, u16, String)> {
    let total = u16at(md, 0) as usize;
    let (mut o, mut out) = (2, Vec::new());
    for _ in 0..total {
        let hdr = md[o + 4];
        o += 5;
        for bit in [1u8, 2] {
            if hdr & bit != 0 {
                let id = u16at(md, o);
                if id >= 10000 {
                    out.push((id, u16at(md, o + 2), String::from_utf8_lossy(cstr(&md[o + 11..o + 16])).to_string()));
                    o += 16;
                } else {
                    o += 31;
                }
            }
        }
        if hdr & 4 != 0 {
            o += 5;
        }
        if hdr & 8 != 0 {
            o += 4;
        }
    }
    out
}

fn motion(p: &mut Player, dir: u8) {
    let mut b = body(COMMAND_MOTION, OBJECTMOVE_, &[], 6);
    b.extend_from_slice(&p.x.to_le_bytes());
    b.extend_from_slice(&p.y.to_le_bytes());
    b.push(dir);
    b.extend_from_slice(&0u32.to_le_bytes());
    b.resize(17, 0);
    p.c.send(&b);
}

fn chat(p: &mut Player, name: &[u8], text: &[u8]) {
    let mut b = body(COMMAND_CHATMSG, 0, &[], 6);
    b.extend_from_slice(&p.x.to_le_bytes());
    b.extend_from_slice(&p.y.to_le_bytes());
    let mut n = name.to_vec();
    n.resize(10, 0);
    b.extend_from_slice(&n);
    b.push(0);
    b.extend_from_slice(text);
    b.push(0);
    p.c.send(&b);
}

fn is_chat_with(b: &[u8], text: &[u8]) -> bool {
    u32at(b, 0) == COMMAND_CHATMSG && b.get(21..21 + text.len()) == Some(text)
}

/// A column of `n` walkable, non-teleport tiles ending right above teleport tile (tx, ty) on the .amd map.
fn plan(mapdir: &str) -> (i16, i16) {
    let raw = std::fs::read(format!("{mapdir}/default.amd")).expect("default.amd");
    let head = String::from_utf8_lossy(&raw[..256]).replace('\0', " ");
    let num = |k: &str| head.split(k).nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty())).and_then(|s| s.parse::<usize>().ok()).unwrap();
    let (sx, ts) = (num("MAPSIZEX"), num("TILESIZE"));
    let attr = |x: usize, y: usize| raw[256 + (y * sx + x) * ts + 8];
    for tx in 127..=129 {
        let ok = (72..78).all(|y| attr(tx, y) & 0xC0 == 0) && attr(tx, 78) & 0x40 != 0 && (72..76).all(|y| attr(tx + 2, y) & 0xC0 == 0);
        if ok {
            return (tx as i16, 72);
        }
    }
    panic!("no straight path to the elvfarm teleport on default");
}

fn play(login: &str, mapdir: &str, out: &str, ws2: Option<&str>) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    ensure_char(login, b"rthg2", b"RtHg2");
    let (sx, sy) = plan(mapdir);
    for (ch, x) in [("RtHg1", sx), ("RtHg2", sx + 2)] {
        sql(&format!("UPDATE char_database SET MapLoc='default', LocX={x}, LocY={sy}, Nation='Traveler' WHERE char_name='{ch}' AND account_name LIKE 'rt%';"));
    }
    k.check(sql("SELECT COUNT(*) FROM char_database WHERE char_name IN ('RtHg1','RtHg2');") == "2", "characters RtHg1/RtHg2 exist");

    let Some(gs1) = enter_game(login, b"rthg1", b"RtHg1", ENTER_NEW) else {
        k.check(false, "login enter game P1");
        return k.0;
    };
    println!("     P1 game server {}:{}", gs1.0, gs1.1);
    let Some(mut p1) = init_player(&gs1, b"rthg1", b"RtHg1", 1, &log, &mut k, None) else { return k.0 + 1 };
    let Some(gs2) = enter_game(login, b"rthg2", b"RtHg2", ENTER_NEW) else {
        k.check(false, "login enter game P2");
        return k.0;
    };
    let Some(mut p2) = init_player(&gs2, b"rthg2", b"RtHg2", 2, &log, &mut k, ws2) else { return k.0 + 1 };
    let mut seen = Vec::new();
    let sees = p1.c.wait_for(Duration::from_secs(2), &mut seen, |b| u32at(b, 0) == EVENT_LOG && u16at(b, 4) == MSGTYPE_CONFIRM_ && u16at(b, 6) == p2.id);
    k.check(sees.is_some(), "P1 sees P2 enter (EVENT_LOG confirm)");

    // keep-alive
    let mut cc = body(COMMAND_CHECKCONNECTION, 0, &[], 6);
    cc.extend_from_slice(&12345u32.to_le_bytes());
    p1.c.send(&cc);
    p2.c.send(&cc);

    chat(&mut p1, b"RtHg1", b"hello rt");
    let a = p1.c.wait_for(Duration::from_secs(2), &mut seen, |b| is_chat_with(b, b"hello rt"));
    let b2 = p2.c.wait_for(Duration::from_secs(2), &mut seen, |b| is_chat_with(b, b"hello rt"));
    k.check(a.as_ref().is_some_and(|b| u16at(b, 4) == p1.id && b[20] == 0), "P1 gets its own normal chat (sender id, mode 0)");
    k.check(b2.is_some(), "P2 hears P1");

    chat(&mut p2, b"RtHg2", b"/to RtHg1");
    let w = p2.c.wait_for(Duration::from_secs(2), &mut seen, |b| u32at(b, 0) == NOTIFY && u16at(b, 4) == NOTIFY_WHISPERMODEON_);
    k.check(w.is_some_and(|b| cstr(&b[6..16]) == b"RtHg1"), "P2 /to RtHg1 -> WHISPERMODEON");
    chat(&mut p2, b"RtHg2", b"psst");
    let a = p1.c.wait_for(Duration::from_secs(2), &mut seen, |b| is_chat_with(b, b"psst"));
    k.check(a.is_some_and(|b| b[20] == 20), "P1 gets the whisper (mode 20)");

    // walk south onto the teleport
    let mut moved = 0;
    for _ in 0..6 {
        motion(&mut p1, 5);
        let r = p1.c.wait_for(Duration::from_secs(2), &mut seen, |b| u32at(b, 0) == RESPONSE_MOTION);
        if r.as_ref().is_some_and(|b| u16at(b, 4) == OBJECTMOVE_CONFIRM_) {
            p1.y += 1;
            moved += 1;
        } else {
            println!("     move reply {:?}", r.map(|b| hex(&b[..b.len().min(16)])));
            break;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    k.check(moved == 6, &format!("P1 walked {moved}/6 tiles south to ({},{})", p1.x, p1.y));
    let steps: Vec<Vec<u8>> = p2.c.drain(Duration::from_millis(500)).into_iter().filter(|b| u32at(b, 0) == EVENT_MOTION && u16at(b, 4) == OBJECTMOVE_ && u16at(b, 6) == p1.id).collect();
    k.check(steps.len() >= 5, &format!("P2 saw {} of P1's steps (EVENT_MOTION)", steps.len()));

    p1.c.send(&body(REQUEST_TELEPORT, MSGTYPE_CONFIRM_, &[], 6));
    let r = p1.c.wait_for(Duration::from_secs(10), &mut seen, |b| u32at(b, 0) == RESPONSE_INITDATA || (u32at(b, 0) == NOTIFY && u16at(b, 4) == NOTIFY_SERVERCHANGE_));
    let gone = p2.c.wait_for(Duration::from_secs(2), &mut seen, |b| u32at(b, 0) == EVENT_LOG && u16at(b, 4) == MSGTYPE_REJECT_ && u16at(b, 6) == p1.id);
    k.check(gone.is_some(), "P2 sees P1 leave (EVENT_LOG reject)");
    match r {
        Some(b) if u32at(&b, 0) == RESPONSE_INITDATA => {
            p1.map = cstr(&b[30..40]).to_vec();
            (p1.x, p1.y) = (u16at(&b, 8) as i16 + 19, u16at(&b, 10) as i16 + 17);
            k.check(p1.map == b"elvfarm", &format!("teleport on the same server -> {}({},{})", String::from_utf8_lossy(&p1.map), p1.x, p1.y));
            p1.c.drain(Duration::from_millis(600));
        }
        Some(b) => {
            let (addr, port) = (String::from_utf8_lossy(cstr(&b[16..31])).to_string(), u32at(&b, 31));
            k.check(cstr(&b[6..16]) == b"elvfarm", &format!("NOTIFY_SERVERCHANGE to {} via login {addr}:{port}", String::from_utf8_lossy(cstr(&b[6..16]))));
            drop(p1);
            std::thread::sleep(Duration::from_millis(500));
            let Some(gs) = enter_game(&format!("{addr}:{port}"), b"rthg1", b"RtHg1", ENTER_CHANGING) else {
                k.check(false, "login enter game (changing server)");
                return k.0;
            };
            k.check(gs != gs1, &format!("changing server: new game server {}:{}", gs.0, gs.1));
            let Some(np) = init_player(&gs, b"rthg1", b"RtHg1", 1, &log, &mut k, None) else { return k.0 + 1 };
            p1 = np;
            k.check(p1.map == b"elvfarm", "entered elvfarm on the other server");
        }
        None => k.check(false, "teleport reply"),
    }
    let mut moved = 0;
    for _ in 0..2 {
        motion(&mut p1, 5);
        let r = p1.c.wait_for(Duration::from_secs(2), &mut seen, |b| u32at(b, 0) == RESPONSE_MOTION);
        if r.is_some_and(|b| u16at(&b, 4) == OBJECTMOVE_CONFIRM_) {
            p1.y += 1;
            moved += 1;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    k.check(moved == 2, &format!("P1 walked {moved}/2 on elvfarm"));
    let (fx, fy) = (p1.x, p1.y);
    drop(p1);
    drop(p2);
    std::thread::sleep(Duration::from_secs(3));
    let row = sql("SELECT MapLoc, LocX, LocY FROM char_database WHERE char_name='RtHg1';");
    k.check(row == format!("elvfarm\t{fx}\t{fy}"), &format!("logout saved RtHg1 at {row:?}, expected elvfarm {fx} {fy}"));
    k.0
}

/// Removes everything the probe created: rt* characters (with their items, bank items and skills) and accounts.
fn clean() {
    let ids = "SELECT CharID FROM char_database WHERE char_name LIKE 'rt%' OR account_name LIKE 'rt%'";
    for t in ["item", "bank_item", "skill"] {
        sql(&format!("DELETE FROM {t} WHERE CharID IN ({ids}) AND CharID <> 0;"));
    }
    sql("DELETE FROM char_database WHERE char_name LIKE 'rt%' OR account_name LIKE 'rt%';");
    sql("DELETE FROM guild_member WHERE GuildName LIKE 'rt%' OR MemberName LIKE 'rt%';");
    sql("DELETE FROM guild WHERE GuildName LIKE 'rt%';");
    sql("DELETE FROM cash_transactions WHERE account_name LIKE 'rt%';");
    sql("DELETE FROM account_database WHERE name LIKE 'rt%';");
    println!(
        "rt chars left: {}, rt accounts left: {}, rt guilds left: {}, CharID=0 items: {}",
        sql("SELECT COUNT(*) FROM char_database WHERE char_name LIKE 'rt%' OR account_name LIKE 'rt%';"),
        sql("SELECT COUNT(*) FROM account_database WHERE name LIKE 'rt%';"),
        sql("SELECT COUNT(*) FROM guild WHERE GuildName LIKE 'rt%';"),
        sql("SELECT COUNT(*) FROM item WHERE CharID = 0;")
    );
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("play") => std::process::exit(play(&a[2], &a[3], &a[4], a.iter().position(|x| x == "--ws2").and_then(|i| a.get(i + 1)).map(String::as_str)) as i32),
        Some("fight") => std::process::exit(fight::fight(&a[2], &a[3], &a[4]) as i32),
        Some("pvp") => std::process::exit(pvp::pvp(&a[2], &a[3], &a[4]) as i32),
        Some("magic") => std::process::exit(magic::magic(&a[2], &a[3], &a[4]) as i32),
        Some("items") => std::process::exit(items::items(&a[2], &a[3], &a[4]) as i32),
        Some("trade") => std::process::exit(items::trade(&a[2], &a[3]) as i32),
        Some("skills") => std::process::exit(skills::skills(&a[2], &a[3]) as i32),
        Some("corpse") => std::process::exit(skills::corpse(&a[2], &a[3], a.get(4).is_some_and(|x| x == "lost")) as i32),
        Some("mining") => std::process::exit(skills::mining(&a[2], &a[3]) as i32),
        Some("fishing") => {
            let n = |i: usize| a[i].parse::<i32>().expect("number");
            let at = (a.len() >= 9).then(|| (a[4].as_str(), n(5), n(6), n(7), n(8)));
            std::process::exit(skills::fishing(&a[2], &a[3], at) as i32)
        }
        Some("craft") => std::process::exit(items::craft(&a[2], &a[3]) as i32),
        Some("resurrect") => std::process::exit(magic::resurrect(&a[2], &a[3], &a[4]) as i32),
        Some("angel") => std::process::exit(items::angel(&a[2], &a[3]) as i32),
        Some("relogin") => std::process::exit(items::relogin(&a[2], &a[3]) as i32),
        Some("guild") => std::process::exit(social::guild(&a[2], &a[3]) as i32),
        Some("party") => std::process::exit(social::party(&a[2], &a[3], &a[4]) as i32),
        Some("quest") => std::process::exit(quest::quest(&a[2], &a[3], &a[4]) as i32),
        Some("crusade") => std::process::exit(crusade::crusade(&a[2], &a[3]) as i32),
        Some("heldenian") => std::process::exit(heldenian::heldenian(&a[2], &a[3]) as i32),
        Some("apocalypse") => std::process::exit(apocalypse::apocalypse(&a[2], &a[3]) as i32),
        Some("astoria") => std::process::exit(astoria::astoria(&a[2], &a[3]) as i32),
        Some("gm") => std::process::exit(gm::gm(&a[2], &a[3]) as i32),
        Some("gm382") => std::process::exit(gm382::gm382(&a[2], &a[3]) as i32),
        Some("fightzone") => std::process::exit(fightzone::fightzone(&a[2], &a[3]) as i32),
        Some("deaths") => std::process::exit(deaths::deaths(&a[2], &a[3], &a[4]) as i32),
        Some("noticement") => std::process::exit(noticement::noticement_probe(&a[2], &a[3]) as i32),
        Some("notices") => std::process::exit(scheduler::notices(&a[2], &a[3], a[4].parse().unwrap_or(120)) as i32),
        Some("gsm") => std::process::exit(social::gsm(&a[2], &a[3]) as i32),
        Some("farming") => std::process::exit(farming::farming(&a[2], &a[3], &a[4]) as i32),
        Some("recallpoint") => std::process::exit(magic::recallpoint(&a[2], &a[3], &a[4]) as i32),
        Some("polymorph") => std::process::exit(magic::polymorph(&a[2], &a[3], &a[4]) as i32),
        Some("leftovers") => std::process::exit(leftovers::leftovers(&a[2], &a[3]) as i32),
        Some("eventparty") => std::process::exit(leftovers::eventparty(&a[2], &a[3], &a[4]) as i32),
        Some("friends") => std::process::exit(leftovers::friends(&a[2], &a[3]) as i32),
        Some("mobevent") => std::process::exit(leftovers::mobevent(&a[2], &a[3], &a[4]) as i32),
        Some("leftovers2") => std::process::exit(leftovers2::leftovers2(&a[2], &a[3]) as i32),
        Some("leftovers3") => std::process::exit(leftovers3::leftovers3(&a[2], &a[3]) as i32),
        Some("clean") => clean(),
        // a test account + character to play by hand (password rtpw1)
        Some("mkacct") => ensure_char(&a[2], a[3].as_bytes(), a[4].as_bytes()),
        _ => eprintln!("usage: hgprobe play LOGIN MAPDATA OUT.jsonl [--ws2 HOST:PORT] | hgprobe clean"),
    }
}
