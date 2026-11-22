//! `hgprobe fightzone LOGIN OUT.jsonl`: fight zone reservations and tickets on the All server. RtHg1 (GM level 4,
//! aresden, Gold 2000): MSGID_REQUEST_FIGHTZONE_RESERVE for zone 1 or 2 (the one allowed today: CONFIRM 1, gold to
//! 500), the same zone again (REJECT -1), COMMONTYPE_REQ_GETOCCUPYFIGHTZONETICKET (ArenaTicket of that zone),
//! /reservefightzone (NOTIFY_FIGHTZONERESERVE -5: no forced recall), /reservefightzone 3 (-4 and 3),
//! /reservefightzone 3 again (-5, -3), /reservefightzone (-4), /getticket (three ArenaTickets: fight zone 1 outside
//! fight zones).
use super::fight::keepalive;
use super::items::{enter, place, put_items};
use super::social::chat_body;
use super::*;

const RESERVE: u32 = MSGID_REQUEST_FIGHTZONE_RESERVE;

fn say(p: &mut Player, text: &[u8], ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let b = chat_body(p, b"RtHg1", text);
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

fn reserves(got: &[Vec<u8>]) -> Vec<i32> {
    got.iter().filter(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_FIGHTZONERESERVE && b.len() == 10).map(|b| u32at(b, 6) as i32).collect()
}

fn tickets(got: &[Vec<u8>]) -> Vec<String> {
    // name, and count 1 (u32 at 27)
    got.iter().filter(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_ITEMOBTAINED && u32at(b, 27) == 1).map(|b| String::from_utf8_lossy(cstr(&b[7..27])).to_string()).collect()
}

fn reserve(p: &mut Player, zone: i32) -> (Option<(u16, i32)>, Vec<Vec<u8>>) {
    let mut b = body(RESERVE, 0, &[], 6);
    b.extend_from_slice(&zone.to_le_bytes());
    p.c.send(&b);
    let got = p.c.drain(Duration::from_millis(700));
    let r = got.iter().find(|b| u32at(b, 0) == MSGID_RESPONSE_FIGHTZONE_RESERVE).map(|b| (u16at(b, 4), u32at(b, 6) as i32));
    (r, got)
}

pub fn fightzone(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    let id = place("RtHg1", "aresden", 140, 132, ", Level=100, HP=900, AdminLevel=4, FightNum=0, FightDate=0, FightTicket=0");
    put_items(&id, &[("Gold", 2000, 1)]);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };

    // the allowed zone alternates with the day and the side
    let (mut r, mut got) = reserve(&mut p, 1);
    let mut zone = 1;
    if r == Some((MSGTYPE_REJECT_, -3)) {
        zone = 2;
        (r, got) = reserve(&mut p, 2);
    }
    println!("     zone {zone}: {r:?}");
    let gold = got.iter().find(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_SETITEMCOUNT).map(|b| u32at(b, 8));
    k.check(r == Some((MSGTYPE_CONFIRM_, 1)) && gold == Some(500), &format!("RESERVE zone {zone}: CONFIRM 1, gold 2000 -> 500 ({gold:?}) (fails in the last 5 minutes of an odd hour: result 0)"));
    let (r, _) = reserve(&mut p, zone);
    k.check(r == Some((MSGTYPE_REJECT_, -1)), &format!("RESERVE zone {zone} again: REJECT -1 (taken) {r:?}"));

    let mut b = body(MSGID_COMMAND_COMMON, COMMONTYPE_REQ_GETOCCUPYFIGHTZONETICKET as u16, &[], 6);
    b.extend_from_slice(&p.x.to_le_bytes());
    b.extend_from_slice(&p.y.to_le_bytes());
    b.resize(57, 0);
    p.c.send(&b);
    let t = tickets(&p.c.drain(Duration::from_millis(700)));
    let want = if zone == 1 { "ArenaTicket".to_string() } else { format!("ArenaTicket({zone})") };
    k.check(t == vec![want.clone()], &format!("GETOCCUPYFIGHTZONETICKET: {want} {t:?}"));

    let r = reserves(&say(&mut p, b"/reservefightzone", 600));
    k.check(r == vec![-5], &format!("/reservefightzone: FIGHTZONERESERVE -5 (no forced recall) {r:?}"));
    let r = reserves(&say(&mut p, b"/reservefightzone 3", 600));
    k.check(r == vec![-4, 3], &format!("/reservefightzone 3: -4, 3 {r:?}"));
    let r = reserves(&say(&mut p, b"/reservefightzone 3", 600));
    k.check(r == vec![-5, -3], &format!("/reservefightzone 3 again: -5, -3 (freed) {r:?}"));
    let r = reserves(&say(&mut p, b"/reservefightzone", 600));
    k.check(r == vec![-4], &format!("/reservefightzone: -4 (forced recall back) {r:?}"));
    let t = tickets(&say(&mut p, b"/getticket", 800));
    k.check(t.len() == 3 && t.iter().all(|n| n == "ArenaTicket"), &format!("/getticket in aresden: 3 ArenaTicket {t:?}"));
    k.0
}
