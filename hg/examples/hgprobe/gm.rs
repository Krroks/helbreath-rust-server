//! `hgprobe gm LOGIN OUT.jsonl`: GM chat commands on the All server. RtHg1 (GM level 4) and RtHg2 (a player) in
//! aresden: /who, /afk twice, /checkrep, /setpf + /pf, /shutup RtHg2 1, /checkip, /gns on an NPC in view,
//! /createitem Arrow 0 0 5, /polymorph Slime (RtHg2 sees RtHg1 change), /summonplayer RtHg2, /teleport fightzone1,
//! the fight zone switches /eventchat, /setattackmode 0, /eventtp, /eventreset, then /closeconn RtHg2.
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

fn text_of(b: &[u8]) -> Vec<u8> {
    cstr(&b[6..]).to_vec()
}

pub fn gm(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2")] {
        ensure_char(login, a, c);
    }
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    place("RtHg1", "aresden", 140, 132, ", Level=100, HP=900, AdminLevel=4, Popularity=7");
    place("RtHg2", "aresden", 143, 132, ", Level=30, HP=300, AdminLevel=0");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };

    let got = say(&mut p, b"/who", 600);
    let w = notif(&got, NOTIFY_TOTALUSERS);
    k.check(w.is_some_and(|b| b.len() == 8 && u16at(b, 6) >= 1), &format!("/who: TOTALUSERS 8 (online + the C++ padding) {:?}", w.map(|b| hex(b))));
    let got = say(&mut p, b"/afk", 600);
    k.check(notif(&got, NOTIFY_NOTICEMSG).is_some_and(|b| text_of(b) == b"You are now AFK."), "/afk: NOTICEMSG You are now AFK.");
    let got = say(&mut p, b"/afk", 600);
    k.check(notif(&got, NOTIFY_NOTICEMSG).is_some_and(|b| text_of(b) == b"You are no longer AFK."), "/afk again: no longer AFK");
    let got = say(&mut p, b"/checkrep RtHg2", 600);
    let lines: Vec<Vec<u8>> = got.iter().filter(|b| u32at(b, 0) == COMMAND_CHATMSG).map(|b| cstr(&b[21..]).to_vec()).collect();
    println!("     /checkrep {:?}", lines.iter().map(|l| String::from_utf8_lossy(l).to_string()).collect::<Vec<_>>());
    k.check(lines.first().is_some_and(|l| l == b" You have 7 reputation points.") && lines.get(1).is_some_and(|l| l.starts_with(b" RtHg2 has ")), "/checkrep RtHg2: own and RtHg2's reputation (server chat lines)");
    say(&mut p, b"/setpf hello world", 400);
    let got = say(&mut p, b"/pf RtHg1", 600);
    let pf = notif(&got, NOTIFY_PLAYERPROFILE);
    k.check(pf.is_some_and(|b| text_of(b) == b"RtHg1 Profile:hello_world"), &format!("/setpf + /pf: PLAYERPROFILE {:?}", pf.map(|b| String::from_utf8_lossy(&text_of(b)).to_string())));
    let got = say(&mut p, b"/shutup RtHg2 1", 600);
    let gq = q.c.drain(Duration::from_millis(300));
    let su = |g: &[Vec<u8>]| notif(g, NOTIFY_PLAYERSHUTUP).is_some_and(|b| b.len() == 19 && u16at(b, 6) == 1 && cstr(&b[8..18]) == b"RtHg2");
    k.check(su(&got) && su(&gq), "/shutup RtHg2 1: PLAYERSHUTUP 19 to both");
    let got = say(&mut p, b"/checkip 1", 600);
    let n = got.iter().filter(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_IPACCOUNTINFO).count();
    k.check(n == 2, &format!("/checkip 1: 2 IPACCOUNTINFO lines ({n})"));
    if let Some((_, _, name)) = p.npcs.first().cloned() {
        let got = say(&mut p, format!("/gns {name}").as_bytes(), 600);
        let a = notif(&got, NOTIFY_ADMINIFO);
        k.check(a.is_some_and(|b| b.len() == 38), &format!("/gns {name}: ADMINIFO 38 {:?}", a.map(|b| hex(b))));
    }
    let got = say(&mut p, b"/createitem Arrow 0 0 5", 800);
    let ob = notif(&got, NOTIFY_ITEMOBTAINED);
    k.check(ob.is_some_and(|b| cstr(&b[7..27]) == b"Arrow" && u32at(b, 27) == 5), &format!("/createitem Arrow 0 0 5: ITEMOBTAINED Arrow x5 {:?}", ob.map(|b| hex(b))));
    q.c.drain(Duration::from_millis(200));
    say(&mut p, b"/polymorph Slime", 600);
    let gq = q.c.drain(Duration::from_millis(400));
    let seen = gq.iter().find(|b| u32at(b, 0) == EVENT_MOTION && u16at(b, 6) == p.id);
    println!("     RtHg2 sees {:?}", seen.map(|b| hex(b)));
    k.check(seen.is_some_and(|b| u16at(b, 12) == 10), "/polymorph Slime: RtHg2 sees RtHg1 as type 10");
    say(&mut p, b"/polymorph off", 400);
    let got = say(&mut p, b"/summonplayer RtHg2", 600);
    let _ = got;
    let gq = q.c.drain(Duration::from_millis(1200));
    k.check(gq.iter().any(|b| u32at(b, 0) == RESPONSE_INITDATA || (u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_SERVERCHANGE)), "/summonplayer RtHg2: RtHg2 moves (INITDATA)");
    let got = say(&mut p, b"/teleport fightzone1", 1500);
    let id = got.iter().find(|b| u32at(b, 0) == RESPONSE_INITDATA);
    k.check(id.is_some_and(|b| cstr(&b[30..40]) == b"fightzone1"), "/teleport fightzone1: INITDATA fightzone1");
    let got = say(&mut p, b"/eventchat", 600);
    let ec = notif(&got, NOTIFY_EVENTCHAT);
    k.check(ec.is_some_and(|b| b.len() == 8 && b[6] == 1 && b[7] == 1), &format!("/eventchat: EVENTCHAT 8 (1, GM) {:?}", ec.map(|b| hex(b))));
    let got = say(&mut p, b"/setattackmode 0", 600);
    let am = notif(&got, NOTIFY_GLOBALATTACKMODE);
    k.check(am.is_some_and(|b| b.len() == 7 && b[6] == 0), "/setattackmode 0: GLOBALATTACKMODE 7 (0)");
    let got = say(&mut p, b"/eventtp", 600);
    let tp = notif(&got, NOTIFY_EVENTTP);
    k.check(tp.is_some_and(|b| b.len() == 7 && b[6] == 1), "/eventtp: EVENTTP 7 (1)");
    let got = say(&mut p, b"/eventreset", 600);
    k.check(notif(&got, NOTIFY_EVENTRESET).is_some_and(|b| b.len() == 6), "/eventreset: EVENTRESET 6");
    say(&mut p, b"/setattackmode 1", 400);
    say(&mut p, b"/eventtp 0", 400);
    say(&mut p, b"/closeconn RtHg2", 800);
    q.c.drain(Duration::from_millis(1500));
    let after = say(&mut p, b"/checkip 1", 600);
    let n = after.iter().filter(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_IPACCOUNTINFO).count();
    k.check(n == 1, &format!("/closeconn RtHg2: one player left on the IP ({n})"));
    k.0
}
