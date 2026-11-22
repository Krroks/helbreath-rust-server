//! `hgprobe crusade LOGIN OUT.jsonl`: a crusade on one server hosting aresden and middleland. Session 1 (RtHg1, GM
//! level 4, war contribution 1000, next to the aresden city hall): `/crusade` -> NOTIFY_CONSTRUCTIONPOINT (0, 0, 1)
//! + NOTIFY_CRUSADE (1, 0); fighter duty -> NOTIFY_CRUSADE (1, 1); commander duty without a guild: nothing; the
//! middleland map status (3 s later, after SyncMiddlelandMapInfo) lists its 3 mana stones; aresden's has none for a
//! fighter. Session 2 (the crusade still on): the entry repeats NOTIFY_CRUSADE (1, 1) and the energy shield
//! generators of Crusade.cfg stand by the city hall; `/endcrusade` -> NOTIFY_CRUSADE (0, 0, 0, winner 0), then the
//! exp-stock timer pays the war contribution: NOTIFY_CRUSADE (0, 0, 1000, 0).
use super::fight::keepalive;
use super::items::{cmd, enter, place};
use super::social::{chat_body, notif};
use super::*;

fn i32at(b: &[u8], o: usize) -> i32 {
    u32at(b, o) as i32
}

fn crusade_v(b: Option<&Vec<u8>>) -> Option<[i32; 4]> {
    b.filter(|b| b.len() == 22).map(|b| [i32at(b, 6), i32at(b, 10), i32at(b, 14), i32at(b, 18)])
}

fn say(p: &mut Player, text: &[u8], ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let b = chat_body(p, b"RtHg1", text);
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

fn send_cmd(p: &mut Player, t: i32, v: [i32; 3], s: &[u8], ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let b = cmd(p, t, 0, v, s, 0);
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

/// NOTIFY_MAPSTATUSLAST: (map name, entries (type, x, y, side))
fn map_status(b: &[u8]) -> (Vec<u8>, Vec<(u8, i16, i16, u8)>) {
    let n = b.get(18).copied().unwrap_or(0) as usize;
    let e = (0..n).filter(|i| 19 + i * 6 + 6 <= b.len()).map(|i| {
        let o = 19 + i * 6;
        (b[o], u16at(b, o + 1) as i16, u16at(b, o + 3) as i16, b[o + 5])
    });
    (cstr(&b[6..16]).to_vec(), e.collect())
}

pub fn crusade(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    // a GM character needs a GM account (the Rust login checks it; the C++ one refuses every GM)
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    place("RtHg1", "aresden", 140, 132, ", Level=100, HP=900, AdminLevel=4, WarCon=1000, CruJob=0, CruID=0, CruConstructPoint=0, GuildName='NONE', GuildID=-1, GuildRank=-1");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"aresden", "entered aresden");
    let got = say(&mut p, b"/crusade", 1500);
    let cp = notif(&got, NOTIFY_CONSTRUCTIONPOINT);
    k.check(cp.is_some_and(|b| b.len() == 12 && u16at(b, 10) == 1), &format!("/crusade: CONSTRUCTIONPOINT (0, 0, 1) {:?}", cp.map(|b| hex(b))));
    let cr = crusade_v(notif(&got, NOTIFY_CRUSADE));
    k.check(cr == Some([1, 0, 0, 0]), &format!("/crusade: CRUSADE (1, 0, 0, 0) {cr:?}"));
    let got = send_cmd(&mut p, COMMONTYPE_REQUEST_SELECTCRUSADEDUTY, [1, 0, 0], b"", 600);
    let cr = crusade_v(notif(&got, NOTIFY_CRUSADE));
    k.check(cr == Some([1, 1, 0, 0]), &format!("fighter duty: CRUSADE (1, 1) {cr:?}"));
    let got = send_cmd(&mut p, COMMONTYPE_REQUEST_SELECTCRUSADEDUTY, [3, 0, 0], b"", 600);
    k.check(notif(&got, NOTIFY_CRUSADE).is_none(), "commander duty without a guild: nothing");
    std::thread::sleep(Duration::from_millis(3500));
    let got = send_cmd(&mut p, COMMONTYPE_REQUEST_MAPSTATUS, [3, 0, 0], b"middleland", 800);
    let ms = notif(&got, NOTIFY_MAPSTATUSLAST);
    println!("     middleland status {:?}", ms.map(|b| hex(b)));
    let (name, e) = ms.map(|b| map_status(b)).unwrap_or_default();
    k.check(name == b"middleland" && e.len() == 3 && e.iter().all(|s| s.0 == 42 && s.3 == 0), &format!("middleland map status: 3 mana stones {e:?}"));
    let got = send_cmd(&mut p, COMMONTYPE_REQUEST_MAPSTATUS, [3, 0, 0], b"aresden", 800);
    let ms = notif(&got, NOTIFY_MAPSTATUSLAST);
    let (name, e) = ms.map(|b| map_status(b)).unwrap_or_default();
    k.check(ms.is_some_and(|b| b.len() == 19) && name == b"aresden" && e.is_empty(), &format!("aresden map status for a fighter: no mana stone {:?}", ms.map(|b| hex(b))));
    drop(p);

    std::thread::sleep(Duration::from_secs(2));
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let cr = crusade_v(notif(&p.entry, NOTIFY_CRUSADE));
    k.check(cr == Some([1, 1, 0, 0]), &format!("entry during the crusade: CRUSADE (1, 1) {cr:?}"));
    let esg = p.npcs.iter().filter(|n| n.1 == 40).count();
    k.check(esg >= 1, &format!("energy shield generators by the city hall: {esg}"));
    let got = say(&mut p, b"/endcrusade", 1500);
    let cr = crusade_v(notif(&got, NOTIFY_CRUSADE));
    k.check(cr == Some([0, 0, 0, 0]), &format!("/endcrusade: CRUSADE (0, 0, 0, winner 0) {cr:?}"));
    let mut paid = None;
    for _ in 0..8 {
        p.c.send(&keepalive());
        let got = p.c.drain(Duration::from_millis(1000));
        if let Some(v) = crusade_v(notif(&got, NOTIFY_CRUSADE)) {
            paid = Some(v);
            break;
        }
    }
    k.check(paid == Some([0, 0, 1000, 0]), &format!("exp-stock timer: CRUSADE (0, 0, war contribution 1000) {paid:?}"));
    k.0
}
