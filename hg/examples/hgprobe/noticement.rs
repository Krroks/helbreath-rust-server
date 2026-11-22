//! `hgprobe noticement LOGIN OUT.jsonl`: noticement, notice.txt and the energy sphere on the All server. The run
//! needs a test notice.txt with two `notify_msg` lines in the server folder (the probe does not write it).
//! RtHg1 (GM level 4) on fightzone5 at (65,36): MSGID_REQUEST_NOTICEMENT with size 0 (REJECT + the Login's
//! noticement.txt) and with that size (CONFIRM 6); /energysphere (ENERGYSPHERECREATED at the creation point
//! 66,37, an Energy-Sphere NPC, type 35, in view), a second /energysphere (nothing: a goal is pending), a walk up
//! to the roaming sphere and a punch (knocked back two tiles); then waits for a notice (NOTICETIME is 80 s).
use super::fight::{keepalive, motion_body};
use super::items::{enter, place};
use super::social::{chat_body, notif};
use super::*;

fn say(p: &mut Player, text: &[u8], ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let b = chat_body(p, b"RtHg1", text);
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

fn noticement(p: &mut Player, size: u32) -> Option<Vec<u8>> {
    let mut b = body(MSGID_REQUEST_NOTICEMENT, 0, &[], 6);
    b.extend_from_slice(&size.to_le_bytes());
    p.c.send(&b);
    p.c.drain(Duration::from_millis(600)).into_iter().find(|b| u32at(b, 0) == MSGID_RESPONSE_NOTICEMENT)
}

pub fn noticement_probe(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    place("RtHg1", "fightzone5", 65, 36, ", Level=100, HP=900, Dexterity=200, Strenght=200, AdminLevel=4");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"fightzone5", &format!("entered fightzone5 at ({},{})", p.x, p.y));

    // the Login sends noticement.txt in text mode (CRLF -> LF): the size comes from the answer
    let r = noticement(&mut p, 0);
    let n = r.as_ref().map_or(0, |b| b.len().saturating_sub(8));
    k.check(r.as_ref().is_some_and(|b| u16at(b, 4) == MSGTYPE_REJECT_ && b[6..].starts_with(b"Welcome to Helbreath") && b[b.len() - 2..] == [0, 0]), &format!("NOTICEMENT size 0: REJECT + noticement.txt ({n} bytes) + 2 zeros"));
    let r = noticement(&mut p, n as u32);
    k.check(r.as_ref().is_some_and(|b| u16at(b, 4) == MSGTYPE_CONFIRM_ && b.len() == 6), "NOTICEMENT same size: CONFIRM 6");

    let got = say(&mut p, b"/energysphere ", 1000);
    let c = notif(&got, NOTIFY_ENERGYSPHERECREATED);
    println!("     ENERGYSPHERECREATED {:?}", c.map(|b| hex(b)));
    k.check(c.is_some_and(|b| b.len() == 10 && u16at(b, 6) == 66 && u16at(b, 8) == 37), "/energysphere: ENERGYSPHERECREATED 10 (66,37)");
    let sphere = got.iter().find(|b| (u32at(b, 0) == EVENT_LOG || u32at(b, 0) == EVENT_MOTION) && u16at(b, 6) > 10000 && u16at(b, 12) == 35).map(|b| u16at(b, 6));
    k.check(sphere.is_some(), &format!("an Energy-Sphere (type 35) in view {sphere:?}"));
    let got2 = say(&mut p, b"/energysphere ", 600);
    k.check(notif(&got2, NOTIFY_ENERGYSPHERECREATED).is_none(), "second /energysphere: nothing (a goal is pending)");

    if let Some(o) = sphere {
        let mut tog = body(MSGID_COMMAND_COMMON, COMMONTYPE_TOGGLECOMBATMODE as u16, &[], 6);
        tog.extend_from_slice(&p.x.to_le_bytes());
        tog.extend_from_slice(&p.y.to_le_bytes());
        tog.resize(57, 0);
        p.c.send(&tog);
        let early: Vec<Vec<u8>> = got.into_iter().chain(got2).chain(p.c.drain(Duration::from_millis(400))).collect();
        // the sphere roams (MOVETYPE_RANDOM): follow its EVENT_MOTION position, punch only when it is next to us
        let (mut moved, mut pos, mut punches) = (0, (66i16, 37i16), 0);
        for b in early.iter().filter(|b| (u32at(b, 0) == EVENT_MOTION || u32at(b, 0) == EVENT_LOG) && u16at(b, 6) == o) {
            pos = (u16at(b, 8) as i16, u16at(b, 10) as i16);
        }
        let t = Instant::now();
        while moved == 0 && t.elapsed() < Duration::from_secs(25) {
            let (dx, dy) = (pos.0 - p.x, pos.1 - p.y);
            let punched = dx.abs().max(dy.abs()) == 1;
            let dir = [[8u8, 1, 2], [7, 0, 3], [6, 5, 4]][(dy.signum() + 1) as usize][(dx.signum() + 1) as usize];
            if !punched && dir != 0 {
                // walk next to it; the answer gives our tile
                p.c.send(&motion_body(&p, OBJECTMOVE_, dir, &[]));
                let (sx, sy) = (p.x + dx.signum(), p.y + dy.signum());
                for b in p.c.drain(Duration::from_millis(500)) {
                    if u32at(&b, 0) == RESPONSE_MOTION {
                        if u16at(&b, 4) == OBJECTMOVE_CONFIRM_ {
                            (p.x, p.y) = (sx, sy);
                        } else if u16at(&b, 4) as i32 == OBJECTMOTION_REJECT {
                            (p.x, p.y) = (u16at(&b, 6) as i16, u16at(&b, 8) as i16);
                        }
                    } else if u32at(&b, 0) == EVENT_MOTION && u16at(&b, 6) == o {
                        pos = (u16at(&b, 8) as i16, u16at(&b, 10) as i16);
                    }
                }
                continue;
            }
            if punched {
                let mut extra = Vec::new();
                for v in [pos.0 as u16, pos.1 as u16, 1, o] {
                    extra.extend_from_slice(&v.to_le_bytes());
                }
                p.c.send(&motion_body(&p, OBJECTATTACK as u16, dir, &extra));
                punches += 1;
            }
            for b in p.c.drain(Duration::from_millis(800)) {
                if u32at(&b, 0) == EVENT_MOTION && u16at(&b, 6) == o {
                    pos = (u16at(&b, 8) as i16, u16at(&b, 10) as i16);
                    // the knock-back: moves right after a punch (its own roaming is one step at a time)
                    if punched && u16at(&b, 4) == OBJECTMOVE_ {
                        moved += 1;
                    }
                }
            }
        }
        println!("     {punches} punches, sphere at {pos:?}");
        k.check(moved == 2, &format!("a punch knocks the sphere back two tiles ({moved} OBJECTMOVE)"));
    }

    let t = Instant::now();
    let mut notice = None;
    while t.elapsed() < Duration::from_secs(95) && notice.is_none() {
        p.c.send(&keepalive());
        notice = p.c.drain(Duration::from_secs(5)).into_iter().find(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_NOTICEMSG);
    }
    println!("     notice {:?}", notice.as_ref().map(|b| String::from_utf8_lossy(cstr(&b[6..])).to_string()));
    k.check(notice.is_some(), "a notice.txt message within 95 s");
    k.0
}
