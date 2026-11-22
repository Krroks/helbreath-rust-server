//! `hgprobe apocalypse LOGIN OUT.jsonl`: servers All and Events. Session 1, RtHg1 (GM level 4) in aresden (All):
//! `/apocalypse` -> IPACCOUNTINFO + APOCGATESTARTMSG + the gate state of a map without a gate (APOCGATEOPEN 24 at
//! 0,0); the start goes to the Events server (GSM_BEGINAPOCALYPSE). Session 2 on inferniaA (Events, gate type 2 to
//! maze at 115,105): the entry sends its gate state (closed while monsters are left, open when the map is empty);
//! `/endapocalypse` -> IPACCOUNTINFO + APOCGATEENDMSG. Session 3 on inferniaA: no gate message any more.
use super::fight::keepalive;
use super::items::{enter, place};
use super::social::{chat_body, notif};
use super::*;

fn say(p: &mut Player, text: &[u8], ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let b = chat_body(p, b"RtHg1", text);
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

/// APOCGATEOPEN / APOCGATECLOSE: (open, x, y, map)
fn gate(got: &[Vec<u8>]) -> Option<(bool, i32, i32, Vec<u8>)> {
    got.iter()
        .find(|b| u32at(b, 0) == NOTIFY && matches!(u16at(b, 4) as i32, NOTIFY_APOCGATEOPEN | NOTIFY_APOCGATECLOSE) && b.len() == 24)
        .map(|b| (u16at(b, 4) as i32 == NOTIFY_APOCGATEOPEN, u32at(b, 6) as i32, u32at(b, 10) as i32, cstr(&b[14..24]).to_vec()))
}

pub fn apocalypse(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    let gm = ", Level=100, HP=900, AdminLevel=4, GuildName='NONE', GuildID=-1, GuildRank=-1";
    place("RtHg1", "aresden", 140, 132, gm);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let got = say(&mut p, b"/apocalypse", 1500);
    k.check(notif(&got, NOTIFY_IPACCOUNTINFO).is_some_and(|b| cstr(&b[6..]) == b"Apocalypse starting in progress."), "/apocalypse: IPACCOUNTINFO");
    k.check(notif(&got, NOTIFY_APOCGATESTARTMSG).is_some_and(|b| b.len() == 6), "/apocalypse: APOCGATESTARTMSG");
    let g = gate(&got);
    k.check(g == Some((true, 0, 0, b"aresden".to_vec())), &format!("/apocalypse: gate state of aresden (no gate): APOCGATEOPEN 0,0 {g:?}"));
    drop(p);

    std::thread::sleep(Duration::from_secs(2));
    place("RtHg1", "inferniaA", 115, 110, gm);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"inferniaA", &format!("entered inferniaA (Events) at ({},{})", p.x, p.y));
    let g = gate(&p.entry);
    println!("     inferniaA gate at the entry {g:?}, NPCs in view {}", p.npcs.len());
    k.check(g.as_ref().is_some_and(|g| (g.1, g.2) == (115, 105) && g.3 == b"inferniaA"), "the entry sends inferniaA's gate (115,105)");
    let got = say(&mut p, b"/endapocalypse", 1500);
    k.check(notif(&got, NOTIFY_IPACCOUNTINFO).is_some_and(|b| cstr(&b[6..]) == b"Apocalypse ending in progress."), "/endapocalypse: IPACCOUNTINFO");
    k.check(notif(&got, NOTIFY_APOCGATEENDMSG).is_some_and(|b| b.len() == 6), "/endapocalypse: APOCGATEENDMSG");
    drop(p);

    std::thread::sleep(Duration::from_secs(2));
    place("RtHg1", "inferniaA", 115, 110, gm);
    let Some(p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(gate(&p.entry).is_none(), "after the end: no gate state at the entry");
    k.0
}
