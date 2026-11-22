//! `hgprobe deaths LOGIN MAPDATA OUT.jsonl`: deaths and combat leftovers on the All server, in aresden.
//! 1. Elo: RtHg1 (aresden, level 60) kills RtHg2 (elvine, level 40, an enemy kill from exp level 30): both log out
//!    and the `elo` column holds 3 (killer) and 65533 = -3 (victim).
//! 2. RtHg3 (GM level 4, level 100): /summon Slime 1 0 7 and 8, and a plain one, are killed by hand; the spells of
//!    the special abilities 7 (Fire-Strike, 130) and 8 (Mass-Fire-Strike, 161) appear as COMMONTYPE_MAGIC events
//!    where the slime fell, the plain slime casts nothing.
//! 3. The Direction-Bow against a ring of rabbits (/summon Rabbit 24): some shot hurts two rabbits (the one in
//!    between too); the Fire-Bow: a fire (MSGID_DYNAMICOBJECT type 1) at a target two or more tiles away, none at
//!    an adjacent one.
use super::fight::{amd, dir_to, keepalive, motion_body, hunt, Fight};
use super::items::{cmd, enter, item_list, place, put_items, slot};
use super::pvp::{fight_dir, kill, level_exp};
use super::social::chat_body;
use super::*;

fn say(p: &mut Player, f: &mut Fight, text: &[u8], ms: u64) {
    p.c.send(&keepalive());
    let b = chat_body(p, b"RtHg3", text);
    p.c.send(&b);
    let me = p.id;
    for b in p.c.drain(Duration::from_millis(ms)) {
        f.feed(&b, me);
    }
}

/// Rabbits (type 55) in view: object id -> position, from the 27-byte appear / stop forms.
fn rabbits(got: &[Vec<u8>], r: &mut std::collections::HashMap<u16, (i16, i16)>) {
    for b in got {
        let (id, ty) = (u32at(b, 0), u16at(b, 4));
        if (id == EVENT_LOG || id == EVENT_MOTION) && b.len() == 27 {
            let o = u16at(b, 6);
            if id == EVENT_LOG && ty == MSGTYPE_REJECT_ {
                r.remove(&o);
            } else if b[26] == 0 && u16at(b, 12) == 55 {
                r.insert(o, (u16at(b, 8) as i16, u16at(b, 10) as i16));
            }
        } else if id == EVENT_MOTION && ty as i32 == OBJECTDYING && u16at(b, 6) >= 40000 {
            r.remove(&(u16at(b, 6) - 30000));
        }
    }
}

/// Refreshes every known rabbit's position (REQUEST_FULLOBJECTDATA) and returns the bodies seen meanwhile.
fn refresh(p: &mut Player, r: &mut std::collections::HashMap<u16, (i16, i16)>) -> Vec<Vec<u8>> {
    let ids: Vec<u16> = r.keys().copied().collect();
    for o in ids {
        p.c.send(&body(MSGID_REQUEST_FULLOBJECTDATA, o, &[], 6));
    }
    let got = p.c.drain(Duration::from_millis(500));
    rabbits(&got, r);
    got
}

/// One arrow at rabbit `o` at (tx, ty); returns the rabbits hurt or killed and the bodies seen.
fn shoot(p: &mut Player, o: u16, tx: i16, ty: i16) -> (std::collections::BTreeSet<u16>, Vec<Vec<u8>>) {
    let mut extra = Vec::new();
    for v in [tx as u16, ty as u16, 2, o] {
        extra.extend_from_slice(&v.to_le_bytes());
    }
    p.c.send(&keepalive());
    let b = motion_body(p, OBJECTATTACK as u16, dir_to(p.x, p.y, tx, ty), &extra);
    p.c.send(&b);
    let got = p.c.drain(Duration::from_millis(900));
    let hurt = got
        .iter()
        .filter(|b| u32at(b, 0) == EVENT_MOTION && matches!(u16at(b, 4) as i32, OBJECTDAMAGE | OBJECTDYING) && u16at(b, 6) >= 40000)
        .map(|b| u16at(b, 6) - 30000)
        .collect();
    (hurt, got)
}

fn fires(got: &[Vec<u8>], tx: i16, ty: i16) -> bool {
    got.iter().any(|b| u32at(b, 0) == MSGID_DYNAMICOBJECT && u16at(b, 4) == MSGTYPE_CONFIRM_ && (u16at(b, 6), u16at(b, 8)) == (tx as u16, ty as u16) && u16at(b, 10) == 1)
}

pub fn deaths(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    let names: [(&[u8], &[u8]); 3] = [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2"), (b"rthg3", b"RtHg3")];
    for (a, c) in names {
        ensure_char(login, a, c);
    }
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg3';");
    let walk = amd(mapdir, "aresden");
    // a free 7x7 block in town (away from the no-attack areas): the killer in the middle
    let (cx, cy) = (140..=190).flat_map(|x| (85..=125).map(move |y| (x, y))).find(|&(x, y)| (-3..=3).all(|dx| (-3..=3).all(|dy| walk(x + dx, y + dy)))).expect("free block");
    println!("     block at aresden({cx},{cy})");

    // --- 1: Elo
    let row = |n: &str, nation: &str, l: i32, (x, y): (i16, i16), hp: i32, st: &str| {
        sql(&format!("UPDATE char_database SET MapLoc='aresden', LocX={x}, LocY={y}, Level={l}, Exp={}, {st}, HP={hp}, PK=0, EK=0, RewardGold=0, elo=0, Nation='{nation}', LockMapName='NONE', LockMapTime=0 WHERE char_name='{n}' AND account_name LIKE 'rt%';", level_exp(l)));
    };
    row("RtHg1", "aresden", 60, (cx, cy), 400, "Strenght=157, Vitality=10, Dexterity=50, Intelligence=10, Magic=10, Agility=10");
    row("RtHg2", "elvine", 40, (cx + 1, cy), 5, "Strenght=10, Vitality=10, Dexterity=10, Intelligence=10, Magic=10, Agility=137");
    {
        let Some(mut p1) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
        let mut tog = body(MSGID_COMMAND_COMMON, COMMONTYPE_TOGGLECOMBATMODE as u16, &[], 6);
        tog.extend_from_slice(&p1.x.to_le_bytes());
        tog.extend_from_slice(&p1.y.to_le_bytes());
        tog.resize(57, 0);
        p1.c.send(&tog);
        p1.c.drain(Duration::from_millis(300));
        let Some(mut p2) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
        let (dead, _, _) = kill(&mut p1, &mut p2, 90);
        k.check(dead, "RtHg2 (elvine, level 40) killed by RtHg1 (level 60)");
        let _ = fight_dir(&p1, &p2);
        drop(p2);
        drop(p1);
    }
    let q = "SELECT elo FROM char_database WHERE char_name IN ('RtHg1','RtHg2') AND account_name LIKE 'rt%' ORDER BY char_name;";
    let end = Instant::now() + Duration::from_secs(25);
    let mut r = sql(q);
    while r.lines().next() == Some("0") && Instant::now() < end {
        std::thread::sleep(Duration::from_secs(1));
        r = sql(q);
    }
    let v: Vec<&str> = r.lines().collect();
    println!("     elo saved (RtHg1, RtHg2): {v:?}");
    k.check(v == ["3", "65533"], "Elo: killer 0 -> 3, victim 0 -> -3 (saved as the WORD 65533)");

    // --- 2: the death spells of the special abilities 7 and 8
    let id = place("RtHg3", "aresden", cx, cy, ", Level=100, HP=900, AdminLevel=4, Strenght=200, Vitality=50, Dexterity=70, Intelligence=10, Magic=10, Agility=27");
    put_items(&id, &[("Direction-Bow", 1, 10000), ("Fire-Bow", 1, 10000), ("Arrow", 300, 1)]);
    let Some(mut p) = enter(login, b"rthg3", b"RtHg3", 3, &log, &mut k) else { return k.0 + 1 };
    let mut tog = body(MSGID_COMMAND_COMMON, COMMONTYPE_TOGGLECOMBATMODE as u16, &[], 6);
    tog.extend_from_slice(&p.x.to_le_bytes());
    tog.extend_from_slice(&p.y.to_le_bytes());
    tog.resize(57, 0);
    p.c.send(&tog);
    p.c.drain(Duration::from_millis(300));
    for (sa, want) in [(7, vec![130u16]), (8, vec![161]), (0, vec![])] {
        let mut f = Fight::full();
        say(&mut p, &mut f, format!("/summon Slime 1 0 {sa}").as_bytes(), 800);
        hunt(&mut p, &mut f, &walk, true, 60, &|f| f.kills >= 1);
        let me = p.id;
        for b in p.c.drain(Duration::from_millis(1000)) {
            f.feed(&b, me);
        }
        println!("     SA {sa}: kills {}, magic events {:?}", f.kills, f.magic);
        k.check(f.kills >= 1 && f.magic == want, &format!("SA {sa} slime killed: death spell events {want:?} (got {:?}, kills {})", f.magic, f.kills));
    }

    // --- 3: the bows
    let (bag, _) = item_list(&p.items);
    let (dbow, fbow) = (slot(&bag, "Direction-Bow"), slot(&bag, "Fire-Bow"));
    k.check(dbow >= 0 && fbow >= 0 && slot(&bag, "Arrow") >= 0, &format!("RtHg3 bag has both bows and the arrows ({bag:?})"));
    let mut rab = std::collections::HashMap::new();
    for round in 0..2 {
        // the ring: every tile within two of the shooter
        p.c.send(&keepalive());
        let b = chat_body(&p, b"RtHg3", b"/summon Rabbit 24 0 0");
        p.c.send(&b);
        let got = p.c.drain(Duration::from_millis(900));
        rabbits(&got, &mut rab);
        let got = p.c.drain(Duration::from_millis(100));
        rabbits(&got, &mut rab);
        if round == 0 {
            p.c.send(&keepalive());
            p.c.send(&cmd(&p, COMMONTYPE_EQUIPITEM, 0, [dbow, 0, 0], b"", 0));
            let got = p.c.drain(Duration::from_millis(800));
            let ev = got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && u16at(b, 4) as i32 == OBJECTNULLACTION && u16at(b, 6) % 30000 == p.id);
            let w = ev.map(|b| (u16at(b, 27) & 0x0FF0) >> 4);
            k.check(w == Some(43), &format!("equip Direction-Bow: weapon appearance 43 ({w:?})"));
            refresh(&mut p, &mut rab);
            println!("     shooter ({},{}), rabbits {:?}", p.x, p.y, rab.values().collect::<Vec<_>>());
            let mut best = 0;
            for shot in 0..30 {
                refresh(&mut p, &mut rab);
                let (gx, gy) = (p.x, p.y);
                let cand = rab.iter().find(|(_, &(x, y))| {
                    let (dx, dy) = (x - gx, y - gy);
                    dx % 2 == 0 && dy % 2 == 0 && dx.abs().max(dy.abs()) == 2 && rab.values().any(|&q| q == (gx + dx / 2, gy + dy / 2))
                });
                let Some((&o, &(tx, ty))) = cand else {
                    // the rabbits wander and die: a new ring
                    let b = chat_body(&p, b"RtHg3", b"/summon Rabbit 24 0 0");
                    p.c.send(&b);
                    let got = p.c.drain(Duration::from_millis(900));
                    rabbits(&got, &mut rab);
                    continue;
                };
                let (hurt, _) = shoot(&mut p, o, tx, ty);
                println!("     direction bow shot {shot} at {o} ({tx},{ty}): hurt {hurt:?}");
                best = best.max(hurt.len());
                if best >= 2 {
                    break;
                }
            }
            k.check(best >= 2, &format!("Direction-Bow: one arrow hurt {best} rabbits (the one in between too)"));
        } else {
            p.c.send(&keepalive());
            p.c.send(&cmd(&p, COMMONTYPE_EQUIPITEM, 0, [fbow, 0, 0], b"", 0));
            let got = p.c.drain(Duration::from_millis(800));
            let ev = got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && u16at(b, 4) as i32 == OBJECTNULLACTION && u16at(b, 6) % 30000 == p.id);
            let w = ev.map(|b| (u16at(b, 27) & 0x0FF0) >> 4);
            k.check(w == Some(44), &format!("equip Fire-Bow: weapon appearance 44 ({w:?})"));
            refresh(&mut p, &mut rab);
            let (gx, gy) = (p.x, p.y);
            // the rabbits wander: the nearest ones, positions just refreshed
            let far = rab.iter().filter(|(_, &(x, y))| (x - gx).abs().max((y - gy).abs()) >= 2).min_by_key(|(_, &(x, y))| (x - gx).abs().max((y - gy).abs())).map(|(&o, &q)| (o, q));
            let near = rab.iter().find(|(_, &(x, y))| (x - gx).abs().max((y - gy).abs()) == 1).map(|(&o, &q)| (o, q));
            if let Some((o, (tx, ty))) = near {
                let (hurt, got) = shoot(&mut p, o, tx, ty);
                k.check(!fires(&got, tx, ty), &format!("Fire-Bow at an adjacent rabbit ({tx},{ty}): no fire (hurt {hurt:?})"));
            } else {
                k.check(false, "an adjacent rabbit for the Fire-Bow");
            }
            let far = if near.is_some() {
                refresh(&mut p, &mut rab);
                rab.iter().filter(|(_, &(x, y))| (x - gx).abs().max((y - gy).abs()) >= 2).min_by_key(|(_, &(x, y))| (x - gx).abs().max((y - gy).abs())).map(|(&o, &q)| (o, q))
            } else {
                far
            };
            if let Some((o, (tx, ty))) = far {
                let (hurt, got) = shoot(&mut p, o, tx, ty);
                println!("     fire bow at {o} ({tx},{ty}): hurt {hurt:?}, dynamic events {:?}", got.iter().filter(|b| u32at(b, 0) == MSGID_DYNAMICOBJECT).map(|b| hex(b)).collect::<Vec<_>>());
                k.check(fires(&got, tx, ty), &format!("Fire-Bow at a rabbit two or more tiles away ({tx},{ty}): fire (MSGID_DYNAMICOBJECT type 1)"));
                // (the fire itself burns the rabbits around it on the 3 s timer: not checked)
                let _ = hurt;
            } else {
                k.check(false, "a distant rabbit for the Fire-Bow");
            }
        }
    }
    k.0
}
