//! `hgprobe gm382 LOGIN OUT.jsonl`: the vanilla 3.82 GM commands of gm.rs (Rust only: the C++ HG does not have them).
//! RtHg1 (GM level 4) with RtHg2 (a player), then RtHg3 (GM level 2) in aresden (the HG takes 2 clients per IP): the level gates (ADMINUSERLEVELLOW),
//! /kill + /revive (KILLED, the dying and damage motions, the HP), /summondemon + /unsummondemon, /summondeath +
//! /clearnpc, /safe /peace /unsafe /war (GLOBALATTACKMODE), /setzerk + /setfreeze (the status bits seen by RtHg2),
//! /time and /weather (TIMECHANGE, WHETHERCHANGE).
use super::fight::keepalive;
use super::items::{enter, place};
use super::social::{chat_body, notif, ty};
use super::*;

/// `who` says `text`; whatever `who` gets within `ms` (the others are drained by the caller)
fn say(p: &mut Player, name: &[u8], text: &[u8], ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let b = chat_body(p, name, text);
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

fn low(got: &[Vec<u8>]) -> bool {
    notif(got, NOTIFY_ADMINUSERLEVELLOW).is_some_and(|b| b.len() == 6)
}

/// an NPC of `npc_type` appears (the 27-byte forms): its object id
fn appeared(got: &[Vec<u8>], npc_type: u16) -> Option<u16> {
    got.iter().find(|b| (u32at(b, 0) == EVENT_LOG || u32at(b, 0) == EVENT_MOTION) && b.len() == 27 && u16at(b, 12) == npc_type && b[26] == 0).map(|b| u16at(b, 6))
}

fn dying(got: &[Vec<u8>], id: u16) -> bool {
    got.iter().any(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTDYING && b.len() == 15 && u16at(b, 6) == id)
}

/// the status of the last OBJECTNULLACTION 43 of `id`
fn status_of(got: &[Vec<u8>], id: u16) -> Option<u32> {
    got.iter().rev().find(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTNULLACTION && b.len() == 43 && u16at(b, 6) == id).map(|b| u32at(b, 37))
}

pub fn gm382(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2"), (b"rthg3", b"RtHg3")] {
        ensure_char(login, a, c);
    }
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name IN ('rthg1', 'rthg3');");
    place("RtHg1", "aresden", 140, 132, ", Level=100, HP=900, AdminLevel=4");
    place("RtHg2", "aresden", 143, 132, ", Level=30, HP=300, AdminLevel=0");
    place("RtHg3", "aresden", 137, 132, ", Level=100, HP=900, AdminLevel=2");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    let (qid, pid) = (q.id, p.id);
    let both = |p: &mut Player, q: &mut Player, ms: u64| (p.c.drain(Duration::from_millis(ms)), q.c.drain(Duration::from_millis(ms)));
    let am = |g: &[Vec<u8>], v: u8| notif(g, NOTIFY_GLOBALATTACKMODE).is_some_and(|b| b.len() == 7 && b[6] == v);

    // --- the level gates of a player (0)
    let got = say(&mut q, b"RtHg2", b"/kill RtHg1", 500);
    k.check(low(&got), "player /kill: ADMINUSERLEVELLOW 6");
    for c in [&b"/time 1"[..], b"/summondemon", b"/safe", b"/setzerk 1", b"/clearnpc", b"/weather 4 30"] {
        let got = say(&mut q, b"RtHg2", c, 300);
        k.check(low(&got), &format!("player {}: ADMINUSERLEVELLOW", String::from_utf8_lossy(c)));
    }
    let (gp, _) = both(&mut p, &mut q, 200);
    k.check(notif(&gp, NOTIFY_KILLED).is_none(), "RtHg1 was not killed by the player");
    // --- /weather first (the 1-minute weather processor of the HG may start one in the map later)
    both(&mut p, &mut q, 100);
    let wc = |g: &[Vec<u8>], v: u8| notif(g, NOTIFY_WHETHERCHANGE).is_some_and(|b| b.len() == 7 && b[6] == v);
    let _ = say(&mut p, b"RtHg1", b"/weather 4 30", 0);
    let (gp, gq) = both(&mut p, &mut q, 600);
    k.check(wc(&gp, 4) && wc(&gq, 4), "/weather 4 30: WHETHERCHANGE 7 (4, snow) to both players of the map");
    let got = say(&mut p, b"RtHg1", b"/weather 5 30", 500);
    k.check(notif(&got, NOTIFY_WHETHERCHANGE).is_none(), "/weather 5 30 while the snow lasts: nothing changes");
    let got = say(&mut p, b"RtHg1", b"/weather 3", 500);
    k.check(notif(&got, NOTIFY_WHETHERCHANGE).is_none(), "/weather with no time: ignored");

    // --- /kill and /revive
    let got = say(&mut p, b"RtHg1", b"/kill NoSuchGuy", 500);
    k.check(notif(&got, NOTIFY_PLAYERNOTONGAME).is_some(), "/kill NoSuchGuy: PLAYERNOTONGAME");
    both(&mut p, &mut q, 100);
    let _ = say(&mut p, b"RtHg1", b"/kill RtHg2 5", 0);
    let (gp, gq) = both(&mut p, &mut q, 700);
    k.check(notif(&gq, NOTIFY_KILLED).is_some_and(|b| b.len() == 26 && cstr(&b[6..26]).is_empty()), "/kill RtHg2 5: KILLED 26 (empty killer) to RtHg2");
    let ev = |g: &[Vec<u8>]| g.iter().find(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTDYING && u16at(b, 6) == qid + 30000).map(|b| b[9]);
    k.check(ev(&gp) == Some(5) && ev(&gq) == Some(5), &format!("/kill RtHg2 5: OBJECTDYING (damage 5) seen by both {:?} {:?}", ev(&gp), ev(&gq)));
    let _ = say(&mut p, b"RtHg1", b"/revive RtHg2 3 50", 0);
    let (gp, gq) = both(&mut p, &mut q, 700);
    k.check(notif(&gq, NOTIFY_HP).is_some_and(|b| u32at(b, 6) == 50), &format!("/revive RtHg2 3 50: HP 50 to RtHg2 {:?}", notif(&gq, NOTIFY_HP).map(|b| hex(b))));
    let dm = gp.iter().find(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTDAMAGE && u16at(b, 6) == qid + 30000);
    k.check(dm.is_some_and(|b| b[9] == 3), &format!("/revive RtHg2 3 50: OBJECTDAMAGE (damage 3) seen by RtHg1 {:?}", dm.map(|b| hex(b))));
    let _ = say(&mut p, b"RtHg1", b"/revive RtHg2 1 99999", 0);
    let (_, gq) = both(&mut p, &mut q, 500);
    let hp = notif(&gq, NOTIFY_HP).map(|b| u32at(b, 6) as i32);
    k.check(hp.is_some_and(|h| (1..99999).contains(&h)), &format!("/revive hp above the maximum is cut to it {hp:?}"));

    // --- /summondeath + /clearnpc by the level 4 GM
    say(&mut p, b"RtHg1", b"/noaggro", 200);
    say(&mut p, b"RtHg1", b"/invincible", 200);
    both(&mut p, &mut q, 100);
    let got = say(&mut p, b"RtHg1", b"/summondeath", 800);
    let wy = appeared(&got, 66);
    k.check(wy.is_some(), &format!("/summondeath: a Wyvern (66) appears {wy:?}"));
    let got = say(&mut p, b"RtHg1", b"/clearnpc", 900);
    k.check(wy.is_some_and(|w| dying(&got, w + 30000)), "/clearnpc: the Wyvern dies (OBJECTDYING)");
    k.check(wy.is_some_and(|w| got.iter().any(|b| u32at(b, 0) == EVENT_LOG && u16at(b, 4) == MSGTYPE_REJECT as u16 && u16at(b, 6) == w)), "/clearnpc: the Wyvern is deleted at once (EVENT_LOG reject)");
    k.check(!dying(&got, 10246 + 30000), "/clearnpc spares the merchant (McGaffin, type 67)");
    say(&mut p, b"RtHg1", b"/noaggro", 200);
    say(&mut p, b"RtHg1", b"/invincible", 200);

    // --- /safe, /war
    both(&mut p, &mut q, 100);
    let _ = say(&mut p, b"RtHg1", b"/safe", 0);
    let (gp, gq) = both(&mut p, &mut q, 600);
    k.check(am(&gp, 0) && am(&gq, 0), "/safe: GLOBALATTACKMODE 7 (0) to both");
    let got = say(&mut p, b"RtHg1", b"/war", 600);
    k.check(am(&got, 1), "/war: GLOBALATTACKMODE (1)");
    both(&mut p, &mut q, 100);

    // --- /setzerk and /setfreeze: the aura bits of RtHg1's status, seen by RtHg2
    let status = |q: &mut Player, p: &mut Player, cmd: &[u8]| -> Option<u32> {
        let _ = say(p, b"RtHg1", cmd, 300);
        status_of(&q.c.drain(Duration::from_millis(500)), pid)
    };
    let s = status(&mut q, &mut p, b"/setzerk 1");
    k.check(s.is_some_and(|s| s & 0x20 != 0), &format!("/setzerk 1: berserk bit (0x20) in the status RtHg2 sees {s:?}"));
    let s = status(&mut q, &mut p, b"/setfreeze 1");
    k.check(s.is_some_and(|s| s & 0x60 == 0x60), &format!("/setfreeze 1: frozen bit (0x40) on top {s:?}"));
    let s = status(&mut q, &mut p, b"/setzerk 0");
    k.check(s.is_some_and(|s| s & 0x20 == 0 && s & 0x40 != 0), &format!("/setzerk 0: berserk off, frozen stays {s:?}"));
    let s = status(&mut q, &mut p, b"/setfreeze 0");
    k.check(s.is_some_and(|s| s & 0x60 == 0), &format!("/setfreeze 0: both off {s:?}"));

    // --- /time (every player)
    both(&mut p, &mut q, 100);
    let tc = |g: &[Vec<u8>], v: u8| notif(g, NOTIFY_TIMECHANGE).is_some_and(|b| b.len() == 7 && b[6] == v);
    let night = || if (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() / 60) % 60 >= NIGHTTIME as u64 { 2u8 } else { 1 };
    let opposite = 3 - night();
    let _ = say(&mut p, b"RtHg1", format!("/time {opposite}").as_bytes(), 0);
    let (gp, gq) = both(&mut p, &mut q, 600);
    k.check(tc(&gp, opposite) && tc(&gq, opposite), &format!("/time {opposite}: TIMECHANGE 7 ({opposite}, against the clock) to both"));
    let (gp, gq) = both(&mut p, &mut q, 7000);
    k.check(notif(&gp, NOTIFY_TIMECHANGE).is_none() && notif(&gq, NOTIFY_TIMECHANGE).is_none(), "the forced time holds: no TIMECHANGE from the 3 s day/night check in 7 s");
    let got = say(&mut p, b"RtHg1", format!("/time {}", 3 - opposite).as_bytes(), 500);
    k.check(tc(&got, 3 - opposite), &format!("/time {}: TIMECHANGE ({})", 3 - opposite, 3 - opposite));
    let got = say(&mut p, b"RtHg1", format!("/time {opposite}").as_bytes(), 500);
    k.check(tc(&got, opposite), &format!("/time {opposite} again: TIMECHANGE ({opposite})"));
    let got = say(&mut p, b"RtHg1", b"/time 0", 500);
    k.check(tc(&got, night()), &format!("/time 0: back to the clock, TIMECHANGE ({})", night()));
    let (gp, gq) = both(&mut p, &mut q, 4000);
    k.check(notif(&gp, NOTIFY_TIMECHANGE).is_none() && notif(&gq, NOTIFY_TIMECHANGE).is_none(), "after /time 0 the clock decides again: no further TIMECHANGE");
    both(&mut p, &mut q, 100);

    // --- the level 2 GM (RtHg2 leaves: the HG takes two clients per IP)
    drop(q);
    let Some(mut r) = enter(login, b"rthg3", b"RtHg3", 3, &log, &mut k) else { return k.0 + 1 };
    p.c.drain(Duration::from_millis(300));
    for c in [&b"/summondeath"[..], b"/clearnpc", b"/weather 4 30", b"/time 1"] {
        let got = say(&mut r, b"RtHg3", c, 300);
        k.check(low(&got), &format!("level 2 {} (needs 3): ADMINUSERLEVELLOW", String::from_utf8_lossy(c)));
    }
    let got = say(&mut r, b"RtHg3", b"/summondemon", 700);
    let demon = appeared(&got, 31);
    k.check(demon.is_some(), &format!("level 2 /summondemon: a Demon (31) appears {demon:?}"));
    let got = say(&mut r, b"RtHg3", b"/unsummondemon", 700);
    k.check(demon.is_some_and(|d| dying(&got, d + 30000)), "level 2 /unsummondemon: the Demon dies (OBJECTDYING)");
    p.c.drain(Duration::from_millis(100));
    let got = say(&mut r, b"RtHg3", b"/peace", 600);
    k.check(am(&got, 0), "level 2 /peace: GLOBALATTACKMODE (0)");
    let got = say(&mut r, b"RtHg3", b"/unsafe", 600);
    k.check(am(&got, 1), "level 2 /unsafe: GLOBALATTACKMODE (1)");
    let got = say(&mut p, b"RtHg1", b"/cleanmap aresden", 1500);
    k.check(got.iter().any(|b| u32at(b, 0) == RESPONSE_INITDATA), "/cleanmap (the other name of /clearmap): the players of the map are re-entered (INITDATA)");
    k.0
}
