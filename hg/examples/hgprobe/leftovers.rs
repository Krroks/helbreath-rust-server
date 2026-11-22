//! The social / events leftovers (batch C): `hgprobe leftovers LOGIN OUT.jsonl` (civil right + Kennedy),
//! `hgprobe eventparty LOGIN MAPDATA OUT.jsonl`, `hgprobe friends LOGIN OUT.jsonl` (two servers: restart_ae.sh) and
//! `hgprobe mobevent LOGIN MAPDATA OUT.jsonl` (waits for the 5-minute special event: see the function).
use super::fight::{amd, keepalive};
use super::items::{cmd, enter, item_list, npc_at, place, put_items, slot};
use super::social::{both, chat_body, notif, ty};
use super::*;

fn say(p: &mut Player, name: &[u8], text: &[u8], ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let b = chat_body(p, name, text);
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

/// (length, result, location) of a RESPONSE_CIVILRIGHT
fn civil(got: &[Vec<u8>]) -> Option<(usize, u16, String)> {
    got.iter().find(|b| u32at(b, 0) == MSGID_RESPONSE_CIVILRIGHT).map(|b| (b.len(), u16at(b, 4), String::from_utf8_lossy(cstr(&b[6..16])).to_string()))
}

/// The side nibble of a NULLACTION event of `id`.
fn side_of(got: &[Vec<u8>], id: u16) -> Option<u32> {
    got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && u16at(b, 4) as i32 == OBJECTNULLACTION && u16at(b, 6) % 30000 == id).map(|b| u32at(b, 37) >> 28)
}

/// `hgprobe leftovers LOGIN OUT.jsonl` (the All server):
/// civil right: RtHg1 (neutral, level 10, in Aresden) asks MSGID_REQUEST_CIVILRIGHT -> RESPONSE_CIVILRIGHT 16 bytes
/// result 1 + "aresden" and a NULLACTION with side 1; asks again (no longer neutral) -> result 0, "aresden"; RtHg2
/// (neutral, level 4) -> result 0, "NONE".
/// Kennedy: RtHg3 (rank 12 of the guild rtkenn, in gldhall_1) gives its GuildSecessionTicket to Kennedy: the ticket is
/// used up (GIVEITEMFIN_ERASEITEM naming Kennedy), RtHg3 gets DISMISSGUILDAPPROVE (38 bytes, rtkenn) and 300 less exp,
/// the master RtHg4 gets DISMISSGUILDSMAN (17 bytes, RtHg3); RtHg4 gives his own ticket: it is used up and nothing else.
pub fn leftovers(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2"), (b"rthg3", b"RtHg3"), (b"rthg4", b"RtHg4")] {
        ensure_char(login, a, c);
    }
    place("RtHg1", "aresden", 140, 132, ", Level=10, HP=300, Nation='NONE', PartyID=0");
    place("RtHg2", "aresden", 143, 132, ", Level=4, HP=300, Nation='NONE', PartyID=0");
    let id3 = place("RtHg3", "gldhall_1", 37, 48, ", HP=300, GuildName='rtkenn', GuildID=779, GuildRank=12");
    let id4 = place("RtHg4", "gldhall_1", 38, 48, ", HP=300, GuildName='rtkenn', GuildID=779, GuildRank=0");
    put_items(&id3, &[("GuildSecessionTicket", 1, 1)]);
    put_items(&id4, &[("GuildSecessionTicket", 1, 1)]);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    // civil right
    let ask = |p: &mut Player| {
        p.c.send(&keepalive());
        p.c.send(&body(MSGID_REQUEST_CIVILRIGHT, 0, &[], 6));
        p.c.drain(Duration::from_millis(800))
    };
    let got = ask(&mut p);
    let c1 = civil(&got);
    k.check(c1 == Some((16, 1, "aresden".into())), &format!("neutral level 10: RESPONSE_CIVILRIGHT 16, result 1, aresden {c1:?}"));
    let sd = side_of(&got, p.id);
    k.check(sd == Some(1), &format!("the NULLACTION of RtHg1 now has side 1 (aresden) {sd:?}"));
    let got = ask(&mut p);
    let c2 = civil(&got);
    k.check(c2 == Some((16, 0, "aresden".into())), &format!("asked again: result 0 (not neutral any more), aresden {c2:?}"));
    let got = ask(&mut q);
    let c3 = civil(&got);
    k.check(c3 == Some((16, 0, "NONE".into())), &format!("neutral level 4: result 0, location kept {c3:?}"));

    // Kennedy (the server takes two connections per IP: the first two leave)
    drop((p, q));
    std::thread::sleep(Duration::from_secs(3));
    let l = sql("SELECT CONCAT(c1.Nation, ':', c2.Nation) FROM char_database c1, char_database c2 WHERE c1.char_name='RtHg1' AND c2.char_name='RtHg2' AND c1.account_name LIKE 'rt%' AND c2.account_name LIKE 'rt%';");
    k.check(l == "aresden:NONE", &format!("saved nations after the logouts (RtHg1 now aresden, RtHg2 still NONE) {l:?}"));
    let Some(mut r) = enter(login, b"rthg3", b"RtHg3", 3, &log, &mut k) else { return k.0 + 1 };
    let Some(mut s) = enter(login, b"rthg4", b"RtHg4", 4, &log, &mut k) else { return k.0 + 1 };
    let kennedy = npc_at(&mut r, 26);
    println!("     Kennedy {kennedy:?}, npcs {:?}", r.npcs);
    k.check(kennedy.is_some(), "Kennedy in view in gldhall_1");
    let Some((kid, kx, ky)) = kennedy else { return k.0 };
    let (bag, _) = item_list(&r.items);
    let t3 = slot(&bag, "GuildSecessionTicket");
    let (bag, _) = item_list(&s.items);
    let t4 = slot(&bag, "GuildSecessionTicket");
    k.check(t3 >= 0 && t4 >= 0, &format!("both have the ticket ({t3}, {t4})"));
    s.c.drain(Duration::from_millis(100));
    let (a, b) = both(&mut r, &mut s, |x| cmd(x, COMMONTYPE_GIVEITEMTOCHAR, t3 as u8, [1, kx as i32, ky as i32], b"GuildSecessionTicket", kid as i32), 1200);
    let er = notif(&a, NOTIFY_GIVEITEMFIN_ERASEITEM);
    k.check(er.is_some_and(|b| cstr(&b[12..32]) == b"Kennedy"), &format!("RtHg3: GIVEITEMFIN_ERASEITEM naming Kennedy {:?}", er.map(|b| hex(b))));
    let ap = notif(&a, COMMONTYPE_DISMISSGUILDAPPROVE);
    k.check(ap.is_some_and(|b| b.len() == 38 && cstr(&b[6..26]) == b"rtkenn"), &format!("RtHg3: DISMISSGUILDAPPROVE 38 (rtkenn) {:?}", ap.map(|b| hex(b))));
    let ex = notif(&a, NOTIFY_EXP);
    k.check(ex.is_some_and(|b| b.len() == 14 && u32at(b, 6) == 38700), &format!("RtHg3: NOTIFY_EXP 39000 - 300 = 38700 {:?}", ex.map(|b| u32at(b, 6))));
    let dm = notif(&b, NOTIFY_DISMISSGUILDSMAN);
    k.check(dm.is_some_and(|b| b.len() == 17 && cstr(&b[6..16]) == b"RtHg3"), &format!("RtHg4: DISMISSGUILDSMAN 17 (RtHg3) {:?}", dm.map(|b| hex(b))));
    let sd = side_of(&b, r.id);
    k.check(sd.is_some(), &format!("RtHg4 sees RtHg3's NULLACTION {sd:?}"));
    let (a, b) = both(&mut s, &mut r, |x| cmd(x, COMMONTYPE_GIVEITEMTOCHAR, t4 as u8, [1, kx as i32, ky as i32], b"GuildSecessionTicket", kid as i32), 1200);
    let er = notif(&a, NOTIFY_GIVEITEMFIN_ERASEITEM);
    k.check(er.is_some_and(|b| cstr(&b[12..32]) == b"Kennedy"), &format!("the master's ticket is used up too (naming Kennedy) {:?}", er.map(|b| hex(b))));
    k.check(notif(&a, COMMONTYPE_DISMISSGUILDAPPROVE).is_none() && notif(&a, NOTIFY_EXP).is_none() && notif(&b, NOTIFY_DISMISSGUILDSMAN).is_none(), "the master leaves nothing: no DISMISSGUILDAPPROVE / EXP / DISMISSGUILDSMAN");
    k.0
}

/// `hgprobe eventparty LOGIN MAPDATA OUT.jsonl` (the All server): RtHg1 (GM level 4) and RtHg2 on fightzone1, in a party.
/// /eventparty 0: both get NOTIFY_EVENTPARTY 8 bytes (1, then the GM byte), RtHg2 is dismissed from the party (the party of
/// two is deleted: NOTIFY_PARTY 2 to both); RtHg2's own /eventparty -> NOTIFY_ADMINUSERLEVELLOW; /eventparty 1 -> 0; a bare
/// /eventparty toggles (1) and again (0).
pub fn eventparty(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2")] {
        ensure_char(login, a, c);
    }
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    let walk = amd(mapdir, "fightzone1");
    let (sx, sy) = (30..=60).flat_map(|x| (30..=60).map(move |y| (x, y))).find(|&(x, y)| walk(x, y) && walk(x + 1, y)).expect("free tiles");
    place("RtHg1", "fightzone1", sx, sy, ", Level=100, HP=900, AdminLevel=4, PartyID=0");
    // (a non-GM entering a fight zone from the Login is recalled to his town: RtHg2 is summoned in)
    place("RtHg2", "aresden", 143, 132, ", Level=60, HP=600, AdminLevel=0, PartyID=0");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    say(&mut p, b"RtHg1", b"/summonplayer RtHg2", 600);
    let got = q.c.drain(Duration::from_millis(1500));
    let id = got.iter().find(|b| u32at(b, 0) == RESPONSE_INITDATA);
    k.check(id.is_some_and(|b| cstr(&b[30..40]) == b"fightzone1"), "RtHg1 summons RtHg2 into fightzone1");
    let (_, _) = both(&mut p, &mut q, |x| cmd(x, COMMONTYPE_REQUEST_JOINPARTY, 0, [1, 0, 0], b"RtHg2", 0), 600);
    let (mut b, mut a) = both(&mut q, &mut p, |x| cmd(x, COMMONTYPE_REQUEST_ACCEPTJOINPARTY, 0, [1, 0, 0], b"", 0), 2500);
    a.extend(p.c.drain(Duration::from_millis(1000)));
    b.extend(q.c.drain(Duration::from_millis(300)));
    let joined = |g: &[Vec<u8>]| g.iter().any(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_PARTY && u16at(b, 6) == 4);
    k.check(joined(&a) && joined(&b), "RtHg1 and RtHg2 are in a party (NOTIFY_PARTY 4)");
    let party2 = |g: &[Vec<u8>]| g.iter().any(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_PARTY && b.len() == 14 && u16at(b, 6) == 2);

    let (mut a, mut b) = both(&mut p, &mut q, |x| chat_body(x, b"RtHg1", b"/eventparty 0"), 1500);
    a.extend(p.c.drain(Duration::from_millis(1500)));
    b.extend(q.c.drain(Duration::from_millis(500)));
    let (ea, eb) = (notif(&a, NOTIFY_EVENTPARTY), notif(&b, NOTIFY_EVENTPARTY));
    k.check(ea.is_some_and(|b| b.len() == 8 && b[6] == 1 && b[7] == 1) && eb.is_some_and(|b| b.len() == 8 && b[6] == 1 && b[7] == 0), &format!("/eventparty 0: EVENTPARTY 8 (1, GM 1) to RtHg1 and (1, GM 0) to RtHg2 {:?} {:?}", ea.map(|b| hex(b)), eb.map(|b| hex(b))));
    k.check(party2(&a) && party2(&b), "RtHg2 is dismissed: the party of two is deleted (NOTIFY_PARTY 2 to both)");
    let (a, b) = both(&mut q, &mut p, |x| chat_body(x, b"RtHg2", b"/eventparty 1"), 800);
    k.check(notif(&a, NOTIFY_ADMINUSERLEVELLOW).is_some_and(|b| b.len() == 6) && notif(&a, NOTIFY_EVENTPARTY).is_none() && notif(&b, NOTIFY_EVENTPARTY).is_none(), "RtHg2's /eventparty: ADMINUSERLEVELLOW 6, nothing changes");
    let (a, b) = both(&mut p, &mut q, |x| chat_body(x, b"RtHg1", b"/eventparty 1"), 800);
    let (ea, eb) = (notif(&a, NOTIFY_EVENTPARTY), notif(&b, NOTIFY_EVENTPARTY));
    k.check(ea.is_some_and(|b| b[6] == 0) && eb.is_some_and(|b| b[6] == 0), "/eventparty 1: EVENTPARTY 0 to both");
    let (a, _) = both(&mut p, &mut q, |x| chat_body(x, b"RtHg1", b"/eventparty"), 800);
    k.check(notif(&a, NOTIFY_EVENTPARTY).is_some_and(|b| b[6] == 1), "bare /eventparty toggles: EVENTPARTY 1");
    let (a, _) = both(&mut p, &mut q, |x| chat_body(x, b"RtHg1", b"/eventparty"), 800);
    k.check(notif(&a, NOTIFY_EVENTPARTY).is_some_and(|b| b[6] == 0), "bare /eventparty again: EVENTPARTY 0");

    // a non-GM who enters a fight zone from the Login (no logout of a fight zone in the last 18 s) is recalled
    drop((p, q));
    std::thread::sleep(Duration::from_secs(3));
    ensure_char(login, b"rthg3", b"RtHg3");
    let id3 = place("RtHg3", "fightzone1", sx, sy, ", Level=60, HP=600, AdminLevel=0, PartyID=0");
    put_items(&id3, &[("Gold", 10, 1)]);
    let Some(mut r) = enter(login, b"rthg3", b"RtHg3", 3, &log, &mut k) else { return k.0 + 1 };
    let mut got = r.entry.clone();
    got.extend(r.c.drain(Duration::from_millis(2500)));
    let maps: Vec<String> = got.iter().filter(|b| u32at(b, 0) == RESPONSE_INITDATA).map(|b| String::from_utf8_lossy(cstr(&b[30..40])).to_string()).collect();
    println!("     RtHg3 entered fightzone1: then {maps:?}");
    k.check(r.map == b"fightzone1" && maps.len() == 1 && maps[0] != "fightzone1", &format!("RtHg3 (non-GM) is recalled out of the fight zone {maps:?}"));
    k.0
}

/// `hgprobe friends LOGIN OUT.jsonl` (two servers: restart_ae.sh): RtHg1 and RtHg3 on aresden (All), RtHg2 on maze
/// (Events). RtHg1 sends its friend list "RtHg3, RtHg2, Nobody, RtHg1": NOTIFY_FRIENDONGAME (31 bytes, the name at 6)
/// at once for RtHg3 and RtHg1 (the C++ finds the sender too), later for RtHg2 (GSM_REQUEST_FINDFRIEND to the other
/// server and its GSM_RESPONSE_FINDFRIEND back); none for Nobody.
pub fn friends(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2"), (b"rthg3", b"RtHg3")] {
        ensure_char(login, a, c);
    }
    place("RtHg1", "aresden", 100, 100, ", HP=300");
    place("RtHg2", "maze", 115, 105, ", HP=300");
    place("RtHg3", "aresden", 102, 100, ", HP=300");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    let Some(mut r) = enter(login, b"rthg3", b"RtHg3", 3, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"aresden" && q.map == b"maze" && r.map == b"aresden", "maps aresden / maze / aresden");
    let names = |got: &[Vec<u8>]| -> Vec<String> { got.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_FRIENDONGAME).map(|b| format!("{}:{}", b.len(), String::from_utf8_lossy(cstr(&b[6..16])))).collect() };
    let list = b"RtHg3, RtHg2, Nobody, RtHg1";
    p.c.send(&keepalive());
    p.c.send(&body(MSGID_REQUEST_UPDATE_FRIENDS, 0, &[(list, list.len() + 1)], 6));
    let first = p.c.drain(Duration::from_millis(300));
    k.check(names(&first) == vec!["31:RtHg3", "31:RtHg1"], &format!("at once: FRIENDONGAME for RtHg3 and RtHg1 {:?}", names(&first)));
    let later = p.c.drain(Duration::from_millis(4000));
    k.check(names(&later) == vec!["31:RtHg2"], &format!("then, from the other server: RtHg2 only {:?}", names(&later)));
    // the same list from the other side: RtHg1 and RtHg3 are on the other server
    q.c.send(&keepalive());
    let l2 = b"RtHg1,RtHg3,RtHg2";
    q.c.send(&body(MSGID_REQUEST_UPDATE_FRIENDS, 0, &[(l2, l2.len() + 1)], 6));
    let got = q.c.drain(Duration::from_millis(4500));
    let mut n = names(&got);
    n.sort();
    k.check(n == vec!["31:RtHg1", "31:RtHg2", "31:RtHg3"], &format!("RtHg2 asks for RtHg1, RtHg3 (other server) and itself: all three {n:?}"));
    r.c.drain(Duration::from_millis(100));
    k.0
}

/// `hgprobe mobevent LOGIN MAPDATA OUT.jsonl` (the All server with random-mob-generator kept on aresdend1 only: the test
/// scripts comment it out of the other maps): RtHg1 (level 140) stands on aresdend1 and chats every 2.5 s (a player
/// action that counts for the sector activity). The special event is on 5 minutes after the server's start (and once at
/// the start): the next generator pass with a 3 in 1d10 puts the monster and its followers at the busiest sector and tells
/// the map NOTIFY_MONSTEREVENT_POSITION (11 bytes: the NPC type, x, y). Waits up to 6 minutes (MOB_WAIT seconds) for it.
pub fn mobevent(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    let walk = amd(mapdir, "aresdend1");
    // a sector centre (20 k + 10) with open ground around it, so that the event's monsters can stand there
    let (cx, cy) = (2..9).flat_map(|a| (2..9).map(move |b| (a * 20 + 10, b * 20 + 10))).find(|&(x, y)| (-3..=3).all(|dx| (-3..=3).all(|dy| walk(x + dx, y + dy)))).expect("an open sector centre");
    let (sx, sy) = (cx + 1, cy);
    place("RtHg1", "aresdend1", sx, sy, ", Level=140, HP=3000, Vitality=300, AdminLevel=4");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    // GM invincible so that the 15 monsters do not end the run
    say(&mut p, b"RtHg1", b"/invincible", 300);
    let t0 = Instant::now();
    let mut ev = None;
    let mut seen: std::collections::BTreeSet<u16> = Default::default();
    let max_wait: u64 = std::env::var("MOB_WAIT").ok().and_then(|v| v.parse().ok()).unwrap_or(360);
    while t0.elapsed() < Duration::from_secs(max_wait) && ev.is_none() {
        for b in say(&mut p, b"RtHg1", b"hi", 2500) {
            if u32at(&b, 0) == NOTIFY && ty(&b) == NOTIFY_MONSTEREVENT_POSITION {
                ev = Some(b);
            } else if u32at(&b, 0) == EVENT_LOG && b.len() == 27 && (10000..30000).contains(&u16at(&b, 6)) {
                seen.insert(u16at(&b, 6));
            }
        }
    }
    println!("     waited {} s", t0.elapsed().as_secs());
    k.check(ev.is_some(), "NOTIFY_MONSTEREVENT_POSITION arrived");
    let Some(ev) = ev else { return k.0 };
    for b in p.c.drain(Duration::from_millis(3000)) {
        if u32at(&b, 0) == EVENT_LOG && b.len() == 27 && (10000..30000).contains(&u16at(&b, 6)) {
            seen.insert(u16at(&b, 6));
        }
    }
    let (t, x, y) = (ev[6], u16at(&ev, 7) as i32, u16at(&ev, 9) as i32);
    let (cx, cy) = (cx as i32, cy as i32);
    println!("     event: npc type {t} at ({x},{y}); busiest sector centre ({cx},{cy}); {} NPCs seen", seen.len());
    k.check(ev.len() == 11 && t > 0, &format!("11 bytes, an NPC type ({t})"));
    k.check((x - cx).abs() <= 12 && (y - cy).abs() <= 12, &format!("the monster is at the busiest sector ({x},{y}) vs ({cx},{cy})"));
    k.check(seen.len() >= 8, &format!("the master and its followers show up around ({} NPCs seen)", seen.len()));
    k.0
}
