//! `hgprobe pvp LOGIN_HOST:PORT MAPDATA_DIR OUT.jsonl`
//!
//! PvP scenario on aresden (characters set up by SQL): RtHg1 (aresden citizen, level 60)
//! kills, in this order, RtHg2 (elvine citizen, level 40: enemy kill -> NOTIFY_ENEMYKILLREWARD + NOTIFY_EXP),
//! RtHg3 (aresden, PK 2: NOTIFY_PKCAPTURED) and RtHg4 (aresden, PK 0: same-town kill -> NOTIFY_PKPENALTY and the
//! teleport to arejail, since aresden is the killer's home town). Each victim logs in after the previous one
//! left (2 clients per IP), has 5 HP and sends a keepalive every 2 s (a player silent for RAGPROTECTIONTIME cannot
//! be hit).
use super::fight::{amd, keepalive, motion_body};
use super::*;

/// Exp at which level `l` starts (iGetLevelExp).
pub fn level_exp(l: i32) -> i32 {
    let q = l / 17;
    l * l + l * (50 + l * q * q)
}

fn ty(b: &[u8]) -> i32 {
    u16at(b, 4) as i32
}

/// Hits victim `v` until it gets NOTIFY_KILLED; returns every body the killer received meanwhile, and the
/// victim's NOTIFY types.
pub fn kill(k: &mut Player, v: &mut Player, secs: u64) -> (bool, Vec<Vec<u8>>, Vec<i32>) {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut got = Vec::new();
    let mut ka = Instant::now() - Duration::from_secs(5);
    let mut killed = false;
    let mut vn = Vec::new();
    while Instant::now() < end && !killed {
        if ka.elapsed() > Duration::from_secs(2) {
            v.c.send(&keepalive());
            k.c.send(&keepalive());
            ka = Instant::now();
        }
        let mut extra = Vec::new();
        for x in [v.x as u16, v.y as u16, 1, v.id] {
            extra.extend_from_slice(&x.to_le_bytes());
        }
        let b = motion_body(k, OBJECTATTACK as u16, fight_dir(k, v), &extra);
        k.c.send(&b);
        got.extend(k.c.drain(Duration::from_millis(500)));
        let vb = v.c.drain(Duration::from_millis(100));
        killed = vb.iter().any(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_KILLED);
        vn.extend(vb.iter().filter(|b| u32at(b, 0) == NOTIFY).map(|b| ty(b)));
    }
    // the rewards follow the kill in the same tick; collect what is still on the way
    got.extend(k.c.drain(Duration::from_millis(800)));
    vn.extend(v.c.drain(Duration::from_millis(100)).iter().filter(|b| u32at(b, 0) == NOTIFY).map(|b| ty(b)));
    (killed, got, vn)
}

pub fn fight_dir(k: &Player, v: &Player) -> u8 {
    match ((v.x - k.x).signum(), (v.y - k.y).signum()) {
        (0, -1) => 1,
        (1, -1) => 2,
        (1, 0) => 3,
        (1, 1) => 4,
        (0, 1) => 5,
        (-1, 1) => 6,
        (-1, 0) => 7,
        _ => 8,
    }
}

/// Items falling on death: DROPITEMFIN_ERASEITEM to the victim, COMMONTYPE_ITEMDROP (18 bytes) to the killer.
fn drops(k: &mut Checks, who: &str, got: &[Vec<u8>], vn: &[i32], n: usize) {
    let erased = vn.iter().filter(|&&t| t == NOTIFY_DROPITEMFIN_ERASEITEM || t == NOTIFY_DROPITEMFIN_COUNTCHANGED).count();
    let seen = got.iter().filter(|b| u32at(b, 0) == MSGID_EVENT_COMMON && ty(b) == COMMONTYPE_ITEMDROP && b.len() == 18).count();
    // a touched item or a lucky roll keeps an item, so up to n
    k.check(erased <= n && seen == erased && (n == 0 || erased > 0), &format!("{who}: {erased} items dropped (victim notifies), {seen} ITEMDROP events seen by the killer, expected up to {n}"));
}

/// NPC types appearing (EVENT_LOG confirm, 27-byte NPC form: id 6, x 8, y 10, type 12).
fn npc_appears(got: &[Vec<u8>]) -> Vec<u16> {
    got.iter().filter(|b| u32at(b, 0) == EVENT_LOG && u16at(b, 4) == MSGTYPE_CONFIRM_ && b.len() == 27 && u16at(b, 6) > 10000).map(|b| u16at(b, 12)).collect()
}

fn notifies(got: &[Vec<u8>]) -> Vec<(i32, usize)> {
    got.iter().filter(|b| u32at(b, 0) == NOTIFY).map(|b| (ty(b), b.len())).collect()
}

pub fn pvp(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    let names: [(&[u8], &[u8]); 4] = [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2"), (b"rthg3", b"RtHg3"), (b"rthg4", b"RtHg4")];
    for (a, c) in names {
        ensure_char(login, a, c);
    }
    let walk = amd(mapdir, "aresden");
    // a free 3x3 block in town, away from the no-attack areas: killer in the middle, victims east, south, west
    let (cx, cy) = (150..=180).flat_map(|x| (90..=120).map(move |y| (x, y))).find(|&(x, y)| (-1..=1).all(|dx| (-1..=1).all(|dy| walk(x + dx, y + dy)))).expect("free block");
    let at = [(cx, cy), (cx + 1, cy), (cx, cy + 1), (cx - 1, cy)];
    // stats add up to 70 + 3 * (level - 1)
    let row = |n: &str, nation: &str, l: i32, (x, y): (i16, i16), hp: i32, pk: i32, st: &str| {
        sql(&format!("UPDATE char_database SET MapLoc='aresden', LocX={x}, LocY={y}, Level={l}, Exp={}, {st}, HP={hp}, PK={pk}, EK=0, RewardGold=0, Nation='{nation}', LockMapName='NONE', LockMapTime=0 WHERE char_name='{n}' AND account_name LIKE 'rt%';", level_exp(l)));
    };
    row("RtHg1", "aresden", 60, at[0], 400, 0, "Strenght=157, Vitality=10, Dexterity=50, Intelligence=10, Magic=10, Agility=10");
    // low vitality and strength: a small max HP and slow regeneration
    let vst = "Strenght=10, Vitality=10, Dexterity=10, Intelligence=10, Magic=10, Agility=137";
    row("RtHg2", "elvine", 40, at[1], 5, 0, vst);
    row("RtHg3", "aresden", 40, at[2], 5, 2, vst);
    row("RtHg4", "aresden", 40, at[3], 5, 0, vst);
    println!("     killer at aresden({cx},{cy}), victims at {:?}", &at[1..]);

    // the game server takes at most 2 clients per IP: the killer stays, each victim logs in after the previous one left
    let log2 = &log;
    let enter = |i: usize, k: &mut Checks| -> Option<Player> {
        let (a, c) = names[i];
        let Some(gs) = enter_game(login, a, c, ENTER_NEW) else {
            k.check(false, &format!("login enter game {}", String::from_utf8_lossy(c)));
            return None;
        };
        let p = init_player(&gs, a, c, i as u8 + 1, log2, k, None)?;
        k.check(p.map == b"aresden", &format!("P{} entered aresden at ({},{})", i + 1, p.x, p.y));
        Some(p)
    };
    let leave = |v: Player, k1: &mut Player| {
        drop(v);
        k1.c.send(&keepalive());
        k1.c.drain(Duration::from_secs(2));
    };
    let Some(mut p1) = enter(0, &mut k) else { return k.0 + 1 };
    let mut tog = body(MSGID_COMMAND_COMMON, COMMONTYPE_TOGGLECOMBATMODE as u16, &[], 6);
    tog.extend_from_slice(&p1.x.to_le_bytes());
    tog.extend_from_slice(&p1.y.to_le_bytes());
    tog.resize(57, 0);
    p1.c.send(&tog);
    p1.c.drain(Duration::from_millis(300));
    let p1 = &mut p1;

    // 1: enemy kill
    let Some(mut p2) = enter(1, &mut k) else { return k.0 + 1 };
    let (dead, got, vn) = kill(p1, &mut p2, 60);
    drops(&mut k, "RtHg2 (penalty 2 - 1 below level 80)", &got, &vn, 1);
    leave(p2, p1);
    println!("     kill 1 notifies (type, size): {:?}", notifies(&got));
    k.check(dead, "RtHg2 (elvine) killed");
    k.check(!npc_appears(&got).contains(&21), "no guard called for an enemy kill");
    let ekr = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_ENEMYKILLREWARD);
    k.check(ekr.is_some_and(|b| b.len() == 48 && cstr(&b[14..24]) == b"RtHg2"), &format!("NOTIFY_ENEMYKILLREWARD 48 bytes naming RtHg2 ({:?} bytes)", ekr.map(|b| b.len())));
    if let Some(b) = ekr {
        println!("     ENEMYKILLREWARD exp {} EK {} guild rank {} war contribution {}", u32at(b, 6), u32at(b, 10), u16at(b, 44), u16at(b, 46));
    }
    k.check(got.iter().any(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_EXP), "NOTIFY_EXP after the enemy kill");

    // 2: criminal captured
    let Some(mut p3) = enter(2, &mut k) else { return k.0 + 1 };
    let (dead, got, vn) = kill(p1, &mut p3, 60);
    drops(&mut k, "RtHg3 (PK 2: penalty 3 - 1)", &got, &vn, 2);
    leave(p3, p1);
    println!("     kill 2 notifies (type, size): {:?}", notifies(&got));
    k.check(dead, "RtHg3 (aresden, PK 2) killed");
    k.check(!npc_appears(&got).contains(&21), "no guard called for killing a criminal");
    let pkc = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_PKCAPTURED);
    k.check(pkc.is_some_and(|b| b.len() == 28 && u16at(b, 6) == 2 && u16at(b, 8) == 40 && cstr(&b[10..20]) == b"RtHg3"), &format!("NOTIFY_PKCAPTURED 28 bytes (PK 2, level 40, RtHg3) {:?}", pkc.map(|b| hex(b))));
    if let Some(b) = pkc {
        println!("     PKCAPTURED reward gold {} exp {}", u32at(b, 20), u32at(b, 24));
    }

    // 3: same-town kill of an innocent -> PK and jail
    let Some(mut p4) = enter(3, &mut k) else { return k.0 + 1 };
    let (dead, mut got, vn) = kill(p1, &mut p4, 60);
    drops(&mut k, "RtHg4 (no penalty for an innocent killed by his own side)", &got, &vn, 0);
    let mut seen = Vec::new();
    p1.c.wait_for(Duration::from_secs(5), &mut seen, |b| u32at(b, 0) == RESPONSE_INITDATA);
    got.extend(seen);
    println!("     kill 3 notifies (type, size): {:?}", notifies(&got));
    k.check(dead, "RtHg4 (aresden, PK 0) killed");
    k.check(npc_appears(&got).contains(&21), &format!("a guard (type 21) appears next to the killer (bAnalyzeCriminalAction): {:?}", npc_appears(&got)));
    let pen = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_PKPENALTY);
    k.check(pen.is_some_and(|b| b.len() == 38 && u32at(b, 34) == 1), &format!("NOTIFY_PKPENALTY 38 bytes, PK count 1 ({:?})", pen.map(|b| (b.len(), u32at(b, 34)))));
    let init = got.iter().find(|b| u32at(b, 0) == RESPONSE_INITDATA);
    let sc = got.iter().any(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_SERVERCHANGE as i32);
    k.check(init.is_some_and(|b| cstr(&b[30..40]) == b"arejail") || sc, &format!("teleported to arejail ({:?}, server change {sc})", init.map(|b| String::from_utf8_lossy(cstr(&b[30..40])).to_string())));
    let order: Vec<String> = got.iter().map(|b| if u32at(b, 0) == NOTIFY { format!("N{}", ty(b)) } else { format!("{:08x}/{:x}", u32at(b, 0), ty(b)) }).collect();
    println!("     kill 3 message order: {order:?}");
    // the client's drop command (COMMONTYPE_ITEMDROP): the first bag item, whole
    p1.c.send(&keepalive());
    let items = p1.items.clone();
    let mut dropc = body(MSGID_COMMAND_COMMON, COMMONTYPE_ITEMDROP as u16, &[], 6);
    for v in [p1.x, p1.y] {
        dropc.extend_from_slice(&v.to_le_bytes());
    }
    dropc.push(0);
    for v in [0i32, 1, 0] {
        dropc.extend_from_slice(&v.to_le_bytes());
    }
    let name = items.get(7..27).map(cstr).unwrap_or(b"").to_vec();
    let mut n30 = name.clone();
    n30.resize(30, 0);
    dropc.extend_from_slice(&n30);
    dropc.resize(57, 0);
    p1.c.send(&dropc);
    let got = p1.c.drain(Duration::from_millis(800));
    let erase = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_DROPITEMFIN_ERASEITEM);
    k.check(erase.is_some_and(|b| b.len() == 12 && u16at(b, 6) == 0), &format!("drop command ({}) -> NOTIFY_DROPITEMFIN_ERASEITEM {:?}", String::from_utf8_lossy(&name), erase.map(|b| hex(b))));
    k.check(got.iter().any(|b| u32at(b, 0) == MSGID_EVENT_COMMON && ty(b) == COMMONTYPE_ITEMDROP && b.len() == 18), "COMMONTYPE_ITEMDROP (18 bytes) at the dropper's tile");
    drop(p4);
    // the C++ saves a few seconds after the logout
    let q = "SELECT char_name, MapLoc, EK, PK, LockMapName, LockMapTime, RewardGold, Exp FROM char_database WHERE char_name LIKE 'RtHg%' ORDER BY char_name;";
    let end = Instant::now() + Duration::from_secs(20);
    let mut r = sql(q);
    while !r.starts_with("RtHg1	arejail") && Instant::now() < end {
        std::thread::sleep(Duration::from_secs(1));
        r = sql(q);
    }
    println!("     saved: {}", r.replace('\n', " | "));
    let r1 = r.lines().next().unwrap_or("");
    k.check(r1.contains("\tarejail\t") && r1.contains("\t1\tarejail\t"), &format!("RtHg1 saved in arejail with PK 1 and the jail lock ({r1:?})"));
    k.0
}
