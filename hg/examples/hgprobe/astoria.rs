//! `hgprobe astoria LOGIN OUT.jsonl`: servers All and Events (astoria). Session 1, RtHg1 (GM level 4) in
//! cityhall_1 (All): William's list has no middleland (only the last capture-the-relic winner gets it; stats.ini has
//! none). Session 2 on astoria (Events) at the relic's spot (168,207): `/astoria 1` -> EVENTSTART 7 (1); picking up
//! the relic -> ITEMOBTAINED + RELICGRABBED 17 (RtHg1); dropping it -> RELICPOSITION 10 at the player's tile;
//! `/endastoria` -> CTRWINNER 7 (0: the relic was in no altar).
use super::fight::{keepalive, motion_body};
use super::items::{cmd, enter, item_list, place};
use super::social::{chat_body, notif};
use super::*;

fn say(p: &mut Player, text: &[u8], ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let b = chat_body(p, b"RtHg1", text);
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

fn teleport_list(p: &mut Player, npc: &[u8]) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let mut n = npc.to_vec();
    n.resize(20, 0);
    p.c.send(&body(MSGID_REQUEST_TELEPORT_LIST, 0, &[(&n[..], 20)], 26));
    let got = p.c.drain(Duration::from_millis(700));
    got.iter()
        .find(|b| u32at(b, 0) == MSGID_RESPONSE_TELEPORT_LIST)
        .map(|r| (0..u32at(r, 6) as usize).filter(|i| 10 + i * 26 + 26 <= r.len()).map(|i| cstr(&r[14 + i * 26..24 + i * 26]).to_vec()).collect())
        .unwrap_or_default()
}

pub fn astoria(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    let gm = ", Level=100, HP=900, AdminLevel=4, GuildName='NONE', GuildID=-1, GuildRank=-1";
    place("RtHg1", "cityhall_1", -1, -1, gm);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let l = teleport_list(&mut p, b"William");
    println!("     William {:?}", l.iter().map(|m| String::from_utf8_lossy(m).to_string()).collect::<Vec<_>>());
    k.check(!l.is_empty() && !l.iter().any(|m| m == b"middleland"), "William: no middleland without a capture-the-relic win");
    drop(p);

    std::thread::sleep(Duration::from_secs(2));
    place("RtHg1", "astoria", 168, 207, gm);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"astoria", &format!("entered astoria (Events) at ({},{})", p.x, p.y));
    let bag = item_list(&p.items).0.len();
    let got = say(&mut p, b"/astoria 1", 1500);
    let ev = notif(&got, NOTIFY_EVENTSTART);
    k.check(ev.is_some_and(|b| b.len() == 7 && b[6] == 1), &format!("/astoria 1: EVENTSTART 7 (capture) {:?}", ev.map(|b| hex(b))));
    k.check((p.x, p.y) == (168, 207), "standing on the relic's spot");
    p.c.send(&keepalive());
    p.c.send(&motion_body(&p, OBJECTGETITEM as u16, 1, &[]));
    let got = p.c.drain(Duration::from_millis(1200));
    let ob = notif(&got, NOTIFY_ITEMOBTAINED);
    k.check(ob.is_some_and(|b| cstr(&b[7..27]) == b"Relic"), "pick-up: ITEMOBTAINED Relic");
    let gr = notif(&got, NOTIFY_RELICGRABBED);
    k.check(gr.is_some_and(|b| b.len() == 17 && cstr(&b[6..16]) == b"RtHg1"), &format!("pick-up: RELICGRABBED 17 (RtHg1) {:?}", gr.map(|b| hex(b))));
    p.c.send(&keepalive());
    p.c.send(&cmd(&p, COMMONTYPE_ITEMDROP, 0, [bag as i32, 1, 0], b"Relic", 0));
    let got = p.c.drain(Duration::from_millis(1200));
    let rp = notif(&got, NOTIFY_RELICPOSITION);
    k.check(rp.is_some_and(|b| b.len() == 10 && (u16at(b, 6), u16at(b, 8)) == (168, 207)), &format!("drop: RELICPOSITION 10 at (168,207) {:?}", rp.map(|b| hex(b))));
    let got = say(&mut p, b"/endastoria", 1500);
    let w = notif(&got, NOTIFY_CTRWINNER);
    k.check(w.is_some_and(|b| b.len() == 7 && b[6] == 0), &format!("/endastoria: CTRWINNER 7 (0) {:?}", w.map(|b| hex(b))));
    k.0
}
