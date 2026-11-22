//! `hgprobe heldenian LOGIN OUT.jsonl`: a heldenian with two game servers, All (CmdHall_1, Gail) and Events
//! (BtField, GodH, HRampart). Session 1, RtHg1 (GM level 4, level 100) in CmdHall_1: Gail's teleport list before
//! the heldenian (nothing for a level 100 aresdener: BtField only for the last winner) -> REJECT; `/heldenian` ->
//! IPACCOUNTINFO + HELDENIANSTART + HELDENIANTELEPORT + CONSTRUCTIONPOINT (10000 + 100 x charisma, at most 12000) + HELDENIANCOUNT;
//! Gail's list now has BtField for free. Session 2, on BtField (the Events server): the towers of the map info stand
//! there and HELDENIANCOUNT counts them; `/endheldenian 1` -> HELDENIANVICTORY (the type 1 winner by towers) +
//! HELDENIANEND. Session 3, CmdHall_1 again: BtField is offered (cost 1000) only if aresden won. Session 4, on
//! BtField: `/heldenian` there puts up the towers (HELDENIANCOUNT), `/endheldenian 1` ends it.
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

/// MSGID_RESPONSE_TELEPORT_LIST: (confirm, [(map, cost)])
fn teleport_list(p: &mut Player, npc: &[u8]) -> Option<(bool, Vec<(Vec<u8>, i32)>)> {
    p.c.send(&keepalive());
    let mut n = npc.to_vec();
    n.resize(20, 0);
    let b = body(MSGID_REQUEST_TELEPORT_LIST, 0, &[(&n[..], 20)], 26);
    p.c.send(&b);
    let got = p.c.drain(Duration::from_millis(700));
    let r = got.iter().find(|b| u32at(b, 0) == MSGID_RESPONSE_TELEPORT_LIST)?;
    let k = u32at(r, 6) as usize;
    let e = (0..k).filter(|i| 10 + i * 26 + 26 <= r.len()).map(|i| {
        let o = 10 + i * 26;
        (cstr(&r[o + 4..o + 14]).to_vec(), u32at(r, o + 22) as i32)
    });
    Some((u16at(r, 4) == MSGTYPE_CONFIRM as u16, e.collect()))
}

fn count(b: Option<&Vec<u8>>) -> Option<Vec<u16>> {
    b.filter(|b| b.len() == 22).map(|b| (0..8).map(|k| u16at(b, 6 + k * 2)).collect())
}

pub fn heldenian(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    let gm = ", Level=100, HP=900, AdminLevel=4, WarCon=0, CruJob=0, CruID=0, GuildName='NONE', GuildID=-1, GuildRank=-1";
    place("RtHg1", "CmdHall_1", -1, -1, gm);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"CmdHall_1", "entered CmdHall_1");
    let l = teleport_list(&mut p, b"Gail");
    println!("     Gail before {l:?}");
    k.check(l.as_ref().is_some_and(|(ok, e)| !ok && e.is_empty()), "Gail before the heldenian: REJECT, nothing");
    let got = say(&mut p, b"/heldenian", 2000);
    let ip = notif(&got, NOTIFY_IPACCOUNTINFO);
    k.check(ip.is_some_and(|b| cstr(&b[6..]) == b"Heldenian starting in progress."), "/heldenian: IPACCOUNTINFO");
    k.check(notif(&got, NOTIFY_HELDENIANSTART).is_some_and(|b| b.len() == 6) && notif(&got, NOTIFY_HELDENIANTELEPORT).is_some_and(|b| b.len() == 6), "/heldenian: HELDENIANSTART + HELDENIANTELEPORT");
    let cp = notif(&got, NOTIFY_CONSTRUCTIONPOINT);
    k.check(cp.is_some_and(|b| b.len() == 12 && (10000..=12000).contains(&u16at(b, 6))), &format!("/heldenian: CONSTRUCTIONPOINT 10000 + 100 x charisma {:?}", cp.map(|b| hex(b))));
    let c0 = count(notif(&got, NOTIFY_HELDENIANCOUNT));
    k.check(c0.is_some(), &format!("/heldenian: HELDENIANCOUNT 22 {c0:?}"));
    let l = teleport_list(&mut p, b"Gail");
    println!("     Gail during {l:?}");
    k.check(l.as_ref().is_some_and(|(ok, e)| *ok && e.iter().any(|(m, c)| m == b"BtField" && *c == 0)), "Gail during the heldenian: BtField for free");
    drop(p);

    std::thread::sleep(Duration::from_secs(2));
    place("RtHg1", "BtField", 60, 230, gm);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"BtField", &format!("entered BtField (the Events server) at ({},{})", p.x, p.y));
    let towers = p.npcs.iter().filter(|n| n.1 == 87 || n.1 == 89).count();
    k.check(towers > 0, &format!("the heldenian towers stand on BtField: {towers} in view"));
    let c1 = count(notif(&p.entry, NOTIFY_HELDENIANCOUNT));
    println!("     count at the entry {c1:?}");
    k.check(c1.as_ref().is_some_and(|v| v[0] > 0 && v[4] > 0), "HELDENIANCOUNT: towers on both sides");
    let got = say(&mut p, b"/endheldenian 1", 2000);
    let v = notif(&got, NOTIFY_HELDENIANVICTORY).filter(|b| b.len() == 8).map(|b| u16at(b, 6) as i16);
    let c1 = c1.unwrap_or_default();
    let expect = if c1.first() > c1.get(4) { 1 } else if c1.first() < c1.get(4) { 2 } else { -1 };
    k.check(v == Some(expect), &format!("/endheldenian: HELDENIANVICTORY {v:?} (towers say {expect})"));
    k.check(notif(&got, NOTIFY_HELDENIANEND).is_some_and(|b| b.len() == 6), "/endheldenian: HELDENIANEND");
    drop(p);

    std::thread::sleep(Duration::from_secs(2));
    place("RtHg1", "CmdHall_1", -1, -1, gm);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(notif(&p.entry, NOTIFY_HELDENIANSTART).is_none(), "after the end: no HELDENIANSTART at the entry");
    let l = teleport_list(&mut p, b"Gail");
    println!("     Gail after {l:?}");
    let bt = l.as_ref().and_then(|(_, e)| e.iter().find(|(m, _)| m == b"BtField").map(|e| e.1));
    k.check(if expect == 1 { bt == Some(1000) } else { bt.is_none() }, &format!("Gail after: BtField at 1000 only for the winner {bt:?}"));
    drop(p);

    // session 4: the heldenian started on the Events server itself (the C++ starts only the server of the GM)
    std::thread::sleep(Duration::from_secs(2));
    place("RtHg1", "BtField", 60, 230, gm);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let got = say(&mut p, b"/heldenian", 2500);
    let c4 = count(got.iter().filter(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_HELDENIANCOUNT).last());
    println!("     count after a local start {c4:?}");
    // the type alternates when the GUID file keeps the last one (the Rust HG): this second one is type 2, whose
    // towers stand on HRampart; the C++ (empty file) always runs type 1, with BtField's towers
    let towers = c4.as_ref().map(|v| (v[0], v[4]));
    k.check(towers.is_some_and(|(a, e)| (a > 0 && e > 0) || (a, e) == (0, 0)), &format!("local /heldenian on BtField: HELDENIANCOUNT (towers {towers:?}: type 1 > 0, type 2 none here)"));
    let got = say(&mut p, b"/endheldenian 1", 2000);
    k.check(notif(&got, NOTIFY_HELDENIANVICTORY).is_some_and(|b| b.len() == 8) && notif(&got, NOTIFY_HELDENIANEND).is_some(), "local /endheldenian: HELDENIANVICTORY + HELDENIANEND");
    k.0
}
