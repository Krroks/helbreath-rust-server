//! `hgprobe magic LOGIN_HOST:PORT MAPDATA_DIR OUT.jsonl`
//!
//! Magic scenario on aresden next to slime spot 1: RtHg1 (aresden citizen, level 60, int 77 mag 90, spells set
//! in MagicMastery by SQL) heals itself, puts a defense shield on itself (60 s: its real MAGICRELEASE expiry is
//! waited for), casts magic missile, energy bolt (area), hold person and chill wind (ice area) on slimes; then
//! RtHg2 (elvine, SwordofIceElemental: SA ice) activates its special ability and hits RtHg1 until it freezes
//! (30 s, expiry waited for too); RtHg1's recall is refused within 10 s of a hit, accepted later.
use super::fight::{amd, keepalive, motion_body};
use super::*;

fn ty(b: &[u8]) -> i32 {
    u16at(b, 4) as i32
}

fn short(b: &[u8]) -> String {
    let id = u32at(b, 0);
    let name = match id {
        NOTIFY => "N",
        EVENT_MOTION => "EM",
        EVENT_LOG => "EL",
        MSGID_EVENT_COMMON => "EC",
        RESPONSE_MOTION => "RM",
        _ => "",
    };
    if name.is_empty() { format!("{id:08x}/{:x}:{}", ty(b), b.len()) } else { format!("{name}{}:{}", ty(b), b.len()) }
}

fn common(p: &Player, t: i32, v: [i32; 3]) -> Vec<u8> {
    let mut b = body(MSGID_COMMAND_COMMON, t as u16, &[], 6);
    for x in [p.x, p.y] {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b.push(1);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b.resize(57, 0);
    b
}

struct Mage {
    slimes: std::collections::BTreeMap<u16, (i16, i16)>,
    frozen: Vec<u16>,
}

impl Mage {
    fn feed(&mut self, got: &[Vec<u8>]) {
        for b in got {
            let (id, t) = (u32at(b, 0), ty(b));
            if (id == EVENT_MOTION || id == EVENT_LOG) && b.len() == 27 && u16at(b, 6) > 10000 {
                let o = u16at(b, 6);
                if id == EVENT_LOG && t == MSGTYPE_REJECT {
                    self.slimes.remove(&o);
                } else if u16at(b, 12) == 10 && b[26] == 0 {
                    self.slimes.insert(o, (u16at(b, 8) as i16, u16at(b, 10) as i16));
                    // status at 22: bit 6 = frozen
                    if u32at(b, 22) & (1 << 6) != 0 && !self.frozen.contains(&o) {
                        self.frozen.push(o);
                    }
                }
            }
            if id == EVENT_MOTION && t == OBJECTDYING && u16at(b, 6) >= 40000 {
                self.slimes.remove(&(u16at(b, 6) - 30000));
            }
        }
    }
}

/// Precast (OBJECTMAGIC motion) then COMMONTYPE_MAGIC on (x, y); returns the precast answers and the cast answers.
fn cast(p: &mut Player, spell: i32, x: i16, y: i16, m: &mut Mage) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    p.c.send(&keepalive());
    p.c.send(&motion_body(p, OBJECTMAGIC as u16, 5, &(spell as u16).to_le_bytes()));
    let pre = p.c.drain(Duration::from_millis(400));
    p.c.send(&common(p, COMMONTYPE_MAGIC, [x as i32, y as i32, spell + 100]));
    let got = p.c.drain(Duration::from_millis(900));
    m.feed(&pre);
    m.feed(&got);
    println!("     cast {spell} at ({x},{y}): precast {:?} -> {:?}", pre.iter().map(|b| short(b)).collect::<Vec<_>>(), got.iter().map(|b| short(b)).collect::<Vec<_>>());
    (pre, got)
}

fn has_n(got: &[Vec<u8>], t: i32) -> Option<&Vec<u8>> {
    got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == t)
}

fn magic_event(got: &[Vec<u8>], spell: i32) -> bool {
    got.iter().any(|b| u32at(b, 0) == MSGID_EVENT_COMMON && ty(b) == COMMONTYPE_MAGIC && u16at(b, 14) as i32 == spell + 100)
}

/// Casts an attack spell on the nearest slime (refreshing its position first); true if it was hit or killed.
fn hit_slime(p: &mut Player, spell: i32, m: &mut Mage, k: &mut Checks, what: &str) -> bool {
    for _ in 0..30 {
        let near = m.slimes.iter().map(|(&o, &(x, y))| (o, x, y)).filter(|&(_, x, y)| (x - p.x).abs() <= 9 && (y - p.y).abs() <= 7).min_by_key(|&(_, x, y)| (x - p.x).abs().max((y - p.y).abs()));
        let Some((o, _, _)) = near else {
            p.c.send(&keepalive());
            let g = p.c.drain(Duration::from_secs(1));
            m.feed(&g);
            continue;
        };
        p.c.send(&body(MSGID_REQUEST_FULLOBJECTDATA, o, &[], 6));
        let g = p.c.drain(Duration::from_millis(300));
        m.feed(&g);
        let Some(&(x, y)) = m.slimes.get(&o) else { continue };
        let (pre, got) = cast(p, spell, x, y, m);
        let ok = pre.iter().any(|b| u32at(b, 0) == RESPONSE_MOTION && ty(b) == OBJECTMOTION_CONFIRM && b.len() == 6);
        let hit = got.iter().any(|b| u32at(b, 0) == EVENT_MOTION && matches!(ty(b), OBJECTDAMAGE | OBJECTDYING) && u16at(b, 6) as i32 == o as i32 + 30000);
        let mp = has_n(&got, NOTIFY_MP).map(|b| u32at(b, 6));
        k.check(ok && magic_event(&got, spell) && mp.is_some(), &format!("{what}: precast confirmed, COMMONTYPE_MAGIC event + NOTIFY_MP {mp:?}"));
        return hit;
    }
    k.check(false, &format!("{what}: no slime in range"));
    false
}

pub fn magic(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    ensure_char(login, b"rthg2", b"RtHg2");
    let walk = amd(mapdir, "aresden");
    let (sx, sy) = (110..=120).flat_map(|x| (56..=60).map(move |y| (x, y))).find(|&(x, y)| walk(x, y) && walk(x + 1, y)).expect("free tiles near the spot");
    let mut mastery = vec![b'0'; 100];
    for s in [0, 1, 10, 12, 13, 25, 45] {
        mastery[s] = b'1';
    }
    let mastery = String::from_utf8(mastery).unwrap();
    // level 60: 247 stat points
    sql(&format!("UPDATE char_database SET MapLoc='aresden', LocX={sx}, LocY={sy}, Level=60, Exp=39000, Strenght=20, Vitality=40, Dexterity=10, Intelligence=77, Magic=90, Agility=10, HP=100, MP=338, SP=150, Hunger=100, PK=0, Nation='aresden', LockMapName='NONE', LockMapTime=0, LeftSpecTime=0, MagicMastery='{mastery}' WHERE char_name='RtHg1' AND account_name LIKE 'rt%';"));
    sql(&format!("UPDATE char_database SET MapLoc='aresden', LocX={}, LocY={sy}, Level=60, Exp=39000, Strenght=150, Vitality=30, Dexterity=37, Intelligence=10, Magic=10, Agility=10, HP=200, SP=150, Hunger=100, PK=0, Nation='elvine', LockMapName='NONE', LockMapTime=0, LeftSpecTime=0 WHERE char_name='RtHg2' AND account_name LIKE 'rt%';", sx + 1));
    let id2 = sql("SELECT CharID FROM char_database WHERE char_name='RtHg2' AND account_name LIKE 'rt%';");
    sql(&format!("DELETE FROM item WHERE CharID={id2} AND CharID<>0;"));
    sql(&format!("INSERT INTO item (CharID, ItemName, ItemID, Count, ItemType, ID1, ID2, ID3, Color, Effect1, Effect2, Effect3, LifeSpan, Attribute, ItemEquip, ItemPosX, ItemPosY) VALUES ({id2}, 'SwordofIceElemental', 614, 1, 0, 0, 0, 0, 0, 0, 0, 0, 7000, 0, 1, 40, 40);"));
    println!("     RtHg1 at aresden({sx},{sy}), RtHg2 (CharID {id2}) east of it");

    let Some(gs) = enter_game(login, b"rthg1", b"RtHg1", ENTER_NEW) else {
        k.check(false, "login enter game");
        return k.0 + 1;
    };
    let Some(mut p) = init_player(&gs, b"rthg1", b"RtHg1", 1, &log, &mut k, None) else { return k.0 + 1 };
    k.check(p.map == b"aresden", &format!("RtHg1 entered aresden at ({},{})", p.x, p.y));
    let mut m = Mage { slimes: Default::default(), frozen: Vec::new() };
    let g = p.c.drain(Duration::from_secs(1));
    m.feed(&g);
    let (px, py) = (p.x, p.y);

    // heal (1) on itself: HP 100 -> more
    let (pre, got) = cast(&mut p, 1, px, py, &mut m);
    let hp = has_n(&got, NOTIFY_HP).map(|b| u32at(b, 6));
    k.check(pre.iter().any(|b| u32at(b, 0) == RESPONSE_MOTION && b.len() == 6), "heal precast confirmed (6 bytes)");
    k.check(hp.is_some_and(|h| h > 100) && magic_event(&got, 1), &format!("heal: NOTIFY_HP {hp:?} > 100 and the COMMONTYPE_MAGIC event"));
    let mp1 = has_n(&got, NOTIFY_MP).map(|b| (u32at(b, 6), b.len()));
    println!("     NOTIFY_MP after heal (value, size) {mp1:?}");

    // a cast without the precast is ignored (no event, no mana)
    p.c.send(&common(&p, COMMONTYPE_MAGIC, [px as i32, py as i32, 101]));
    let got = p.c.drain(Duration::from_millis(600));
    k.check(!magic_event(&got, 1) && has_n(&got, NOTIFY_MP).is_none(), "COMMONTYPE_MAGIC without the precast is ignored");

    // defense shield (13) on itself: MAGICEFFECTON(PROTECT 11, DS 3), released after 60 s
    let (_, got) = cast(&mut p, 13, px, py, &mut m);
    let ds_at = Instant::now();
    let on = has_n(&got, NOTIFY_MAGICEFFECTON);
    k.check(on.is_some_and(|b| b.len() == 16 && u16at(b, 6) == 11 && u32at(b, 8) == 3), &format!("defense shield: NOTIFY_MAGICEFFECTON 16 bytes (11, 3) {:?}", on.map(|b| hex(b))));
    let st = got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTNULLACTION && u16at(b, 6) == p.id + 30000);
    println!("     own NULLACTION after the shield: {:?}", st.map(|b| hex(b)));

    // attack spells on slimes
    let mut hits = 0;
    for (s, what) in [(0, "magic missile"), (0, "magic missile 2"), (10, "energy bolt (area)"), (25, "hold person"), (45, "chill wind (ice area)"), (10, "energy bolt 2")] {
        if hit_slime(&mut p, s, &mut m, &mut k, what) {
            hits += 1;
        }
    }
    k.check(hits >= 2, &format!("slimes damaged by the attack spells ({hits} of 6 casts)"));
    println!("     slimes seen frozen: {:?}", m.frozen);

    // RtHg2 (elvine) with the SA ice sword
    let Some(gs2) = enter_game(login, b"rthg2", b"RtHg2", ENTER_NEW) else {
        k.check(false, "login enter game RtHg2");
        return k.0 + 1;
    };
    let Some(mut q) = init_player(&gs2, b"rthg2", b"RtHg2", 2, &log, &mut k, None) else { return k.0 + 1 };
    let g = q.c.drain(Duration::from_millis(800));
    println!("     RtHg2 at ({},{}), SA notifies at entry: {:?}", q.x, q.y, g.iter().filter(|b| u32at(b, 0) == NOTIFY && matches!(ty(b), NOTIFY_SPECIALABILITYENABLED | NOTIFY_SPECIALABILITYSTATUS)).map(|b| hex(b)).collect::<Vec<_>>());
    q.c.send(&common(&q, COMMONTYPE_TOGGLECOMBATMODE, [0, 0, 0]));
    q.c.send(&common(&q, COMMONTYPE_REQUEST_ACTIVATESPECABLTY, [0, 0, 0]));
    let g = q.c.drain(Duration::from_millis(600));
    let sa = has_n(&g, NOTIFY_SPECIALABILITYSTATUS);
    k.check(sa.is_some_and(|b| b.len() == 12 && u16at(b, 6) == 1 && u16at(b, 8) == 2), &format!("SA activated: NOTIFY_SPECIALABILITYSTATUS 12 bytes (1, ice 2, last secs) {:?}", sa.map(|b| hex(b))));
    let mut frozen_at = None;
    let mut p_seen = Vec::new();
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end && frozen_at.is_none() {
        p.c.send(&keepalive());
        q.c.send(&keepalive());
        let mut extra = Vec::new();
        for v in [p.x as u16, p.y as u16, 1, p.id] {
            extra.extend_from_slice(&v.to_le_bytes());
        }
        q.c.send(&motion_body(&q, OBJECTATTACK as u16, 7, &extra));
        q.c.drain(Duration::from_millis(500));
        let g = p.c.drain(Duration::from_millis(100));
        if g.iter().any(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_MAGICEFFECTON && u16at(b, 6) == 23) {
            frozen_at = Some(Instant::now());
        }
        p_seen.extend(g);
    }
    let on = has_n(&p_seen, NOTIFY_MAGICEFFECTON);
    k.check(frozen_at.is_some(), &format!("SA ice hit froze RtHg1: NOTIFY_MAGICEFFECTON {:?}", on.map(|b| hex(b))));
    // recall within 10 s of a hit is refused
    let (px, py) = (p.x, p.y);
    let (_, got) = cast(&mut p, 12, px, py, &mut m);
    k.check(has_n(&got, NOTIFY_CANNOTRECALL).is_some_and(|b| b.len() == 6), "recall right after a hit: NOTIFY_CANNOTRECALL (6 bytes)");
    // the SA runs out (last secs + the 3 s timer), the ice (30 s) and the shield (60 s) are released
    let (mut sa_off, mut ice_off, mut ds_off) = (None, None, None);
    let end = Instant::now() + Duration::from_secs(65);
    while Instant::now() < end && (ice_off.is_none() || ds_off.is_none() || sa_off.is_none()) {
        p.c.send(&keepalive());
        q.c.send(&keepalive());
        for b in q.c.drain(Duration::from_millis(500)) {
            if u32at(&b, 0) == NOTIFY && ty(&b) == NOTIFY_SPECIALABILITYSTATUS && u16at(&b, 6) == 3 && sa_off.is_none() {
                sa_off = Some(b.len());
            }
        }
        for b in p.c.drain(Duration::from_millis(500)) {
            if u32at(&b, 0) == NOTIFY && ty(&b) == NOTIFY_MAGICEFFECTOFF {
                match u16at(&b, 6) {
                    23 => ice_off = Some((frozen_at.map(|t| t.elapsed().as_secs()), hex(&b))),
                    11 => ds_off = Some((ds_at.elapsed().as_secs(), hex(&b))),
                    _ => {}
                }
            }
        }
    }
    k.check(sa_off == Some(12), &format!("SA ran out: NOTIFY_SPECIALABILITYSTATUS (3) {sa_off:?}"));
    k.check(ice_off.as_ref().is_some_and(|(s, _)| s.is_some_and(|s| (29..=32).contains(&s))), &format!("ice released after 30 s (MAGICRELEASE): {ice_off:?}"));
    k.check(ds_off.as_ref().is_some_and(|(s, _)| (59..=63).contains(s)), &format!("defense shield released after 60 s (MAGICRELEASE): {ds_off:?}"));
    drop(q);
    // recall now works: the player is moved (RESPONSE_INITDATA)
    p.c.send(&keepalive());
    let (px, py) = (p.x, p.y);
    let (_, mut got) = cast(&mut p, 12, px, py, &mut m);
    // a slime may have hit RtHg1 meanwhile (no recall for 10 s after a hit)
    for _ in 0..3 {
        if has_n(&got, NOTIFY_CANNOTRECALL).is_none() {
            break;
        }
        // hold the slimes next to RtHg1 (a held NPC does not attack), then wait out the 10 s
        let ids: Vec<u16> = m.slimes.keys().copied().collect();
        for o in ids {
            p.c.send(&body(MSGID_REQUEST_FULLOBJECTDATA, o, &[], 6));
        }
        let g = p.c.drain(Duration::from_millis(400));
        m.feed(&g);
        let near: Vec<(i16, i16)> = m.slimes.values().copied().filter(|&(x, y)| (x - px).abs() <= 2 && (y - py).abs() <= 2).collect();
        for (x, y) in near {
            cast(&mut p, 25, x, y, &mut m);
        }
        for _ in 0..11 {
            p.c.send(&keepalive());
            let g = p.c.drain(Duration::from_secs(1));
            m.feed(&g);
        }
        got = cast(&mut p, 12, px, py, &mut m).1;
    }
    let mut seen = Vec::new();
    p.c.wait_for(Duration::from_secs(5), &mut seen, |b| u32at(b, 0) == RESPONSE_INITDATA);
    got.extend(seen);
    let init = got.iter().find(|b| u32at(b, 0) == RESPONSE_INITDATA);
    k.check(init.is_some(), &format!("recall: RESPONSE_INITDATA ({:?})", init.map(|b| String::from_utf8_lossy(cstr(&b[30..40])).to_string())));
    k.0
}

/// `hgprobe resurrect`: RtHg1 (aresden, Resurrection spell 94 set by SQL, special ability ready) and RtHg2 (aresden
/// citizen, 3 HP, Fire-Wall) 4 tiles apart on huntzone1. RtHg2 casts a fire wall next to itself and dies in it; RtHg1 casts 94 on its tile: RtHg1 gets
/// SPELL_SKILL + SPECIALABILITYSTATUS (1, 55, 1200), RtHg2 NOTIFY_RESURRECTPLAYER; RtHg2 answers yes and re-enters
/// the same spot alive (RESPONSE_INITDATA); the saved HP is half its maximum or more.
pub fn resurrect(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    ensure_char(login, b"rthg2", b"RtHg2");
    let walk = amd(mapdir, "huntzone1");
    let (sx, sy) = (40..=70).flat_map(|x| (110..=130).map(move |y| (x, y))).find(|&(x, y)| (0..=5).all(|d| walk(x + d, y)) && walk(x + 5, y + 1) && walk(x + 5, y - 1)).expect("free tiles on huntzone1");
    let mut mastery = vec![b'0'; 100];
    mastery[94] = b'1';
    let mastery = String::from_utf8(mastery).unwrap();
    let mut fw = "0".repeat(100);
    fw.replace_range(40..41, "1");
    sql(&format!("UPDATE char_database SET MapLoc='huntzone1', LocX={sx}, LocY={sy}, Level=140, Exp=1281000, Strenght=60, Vitality=10, Dexterity=10, Intelligence=150, Magic=150, Agility=10, HP=60, MP=338, SP=50, Hunger=100, PK=0, Nation='aresden', LockMapName='NONE', LockMapTime=0, LeftSpecTime=0, LeftSAC=0, MagicMastery='{mastery}' WHERE char_name='RtHg1' AND account_name LIKE 'rt%';"));
    sql(&format!("UPDATE char_database SET MapLoc='huntzone1', LocX={}, LocY={sy}, Level=30, Exp=9000, Strenght=20, Vitality=20, Dexterity=20, Intelligence=10, Magic=10, Agility=10, HP=3, MP=100, SP=50, Hunger=100, PK=0, Nation='aresden', LockMapName='NONE', LockMapTime=0, LeftSpecTime=0, MagicMastery='{fw}' WHERE char_name='RtHg2' AND account_name LIKE 'rt%';", sx + 4));
    let id1 = sql("SELECT CharID FROM char_database WHERE char_name='RtHg1' AND account_name LIKE 'rt%';");
    sql(&format!("DELETE FROM item WHERE CharID={id1} AND CharID<>0; INSERT INTO item (CharID, ItemName, ItemID, Count, ItemType, ID1, ID2, ID3, Color, Effect1, Effect2, Effect3, LifeSpan, Attribute, ItemEquip, ItemPosX, ItemPosY) VALUES ({id1}, 'ResurWand(MS.20)', 9900991, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1000, 0, 0, 40, 40);"));
    let Some(gs) = enter_game(login, b"rthg1", b"RtHg1", ENTER_NEW) else { return k.0 + 1 };
    let Some(mut p) = init_player(&gs, b"rthg1", b"RtHg1", 1, &log, &mut k, None) else { return k.0 + 1 };
    // equipping the wand grants the spell (int and mag 100, special ability ready): NOTIFY_SPELL_SKILL
    p.c.send(&common(&p, COMMONTYPE_EQUIPITEM, [0, 0, 0]));
    let g = p.c.drain(Duration::from_millis(600));
    k.check(has_n(&g, NOTIFY_SPELL_SKILL).is_some(), "equip the ResurWand: NOTIFY_SPELL_SKILL");
    let Some(gs2) = enter_game(login, b"rthg2", b"RtHg2", ENTER_NEW) else { return k.0 + 1 };
    let Some(mut q) = init_player(&gs2, b"rthg2", b"RtHg2", 2, &log, &mut k, None) else { return k.0 + 1 };
    let (qx, qy) = (q.x, q.y);
    let mut m = Mage { slimes: Default::default(), frozen: Vec::new() };
    let mut killed = false;
    let end = Instant::now() + Duration::from_secs(90);
    let mut next_cast = Instant::now();
    while Instant::now() < end && !killed {
        if Instant::now() >= next_cast {
            // the cast can fail its probability roll (80 %): cast again every 10 s until dead
            q.c.send(&keepalive());
            q.c.send(&motion_body(&q, OBJECTMAGIC as u16, 5, &40u16.to_le_bytes()));
            let _ = q.c.drain(Duration::from_millis(400));
            q.c.send(&common(&q, COMMONTYPE_MAGIC, [qx as i32 + 1, qy as i32, 140]));
            next_cast = Instant::now() + Duration::from_secs(10);
        }
        p.c.send(&keepalive());
        q.c.send(&keepalive());
        let _ = p.c.drain(Duration::from_millis(300));
        killed = q.c.drain(Duration::from_millis(700)).iter().any(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_KILLED);
    }
    k.check(killed, &format!("RtHg2 killed by its fire wall at ({qx},{qy})"));
    if !killed {
        return k.0;
    }
    let (_, got) = cast(&mut p, 94, qx, qy, &mut m);
    let skill = has_n(&got, NOTIFY_SPELL_SKILL).map(|b| b.len());
    let sa = has_n(&got, NOTIFY_SPECIALABILITYSTATUS).map(|b| (b.len(), u16at(b, 6), u16at(b, 8), u16at(b, 10)));
    k.check(skill.is_some() && sa == Some((12, 1, 55, 1200)) && magic_event(&got, 94), &format!("caster: SPELL_SKILL {skill:?}, SPECIALABILITYSTATUS (size, 1, 55, 1200) {sa:?}, magic event"));
    let g = q.c.drain(Duration::from_millis(600));
    let ask = has_n(&g, NOTIFY_RESURRECTPLAYER).map(|b| b.len());
    k.check(ask == Some(6), &format!("dead player: NOTIFY_RESURRECTPLAYER 6 {ask:?}"));
    q.c.send(&body(MSGID_REQUEST_RESURRECTPLAYER_YES, 0, &[], 6));
    let mut seen = Vec::new();
    let r = q.c.wait_for(Duration::from_secs(10), &mut seen, |b| u32at(b, 0) == RESPONSE_INITDATA);
    let at = r.as_ref().map(|b| (String::from_utf8_lossy(cstr(&b[30..40])).to_string(), u16at(b, 8) as i16, u16at(b, 10) as i16));
    println!("     after yes: {:?} -> {at:?}", seen.iter().map(|b| short(b)).collect::<Vec<_>>());
    k.check(at.as_ref().is_some_and(|a| a.0 == "huntzone1"), &format!("yes -> RESPONSE_INITDATA on huntzone1 {at:?}"));
    let _ = q.c.drain(Duration::from_millis(600));
    // a second yes nobody asked for: the resurrection hack disconnects
    q.c.send(&body(MSGID_REQUEST_RESURRECTPLAYER_YES, 0, &[], 6));
    std::thread::sleep(Duration::from_secs(1));
    drop(q);
    drop(p);
    let mut row = String::new();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_secs(1));
        row = sql("SELECT HP FROM char_database WHERE char_name='RtHg2' AND account_name LIKE 'rt%';");
        if row.parse::<i32>().is_ok_and(|h| h > 1) {
            break;
        }
    }
    k.check(row.parse::<i32>().is_ok_and(|h| h > 1), &format!("RtHg2 saved alive, HP {row}"));
    k.0
}

/// The player casts Recall on its own tile and waits for the RESPONSE_INITDATA of the recall:
/// (map, x, y) of the new spot, or None.
fn recall(p: &mut Player, m: &mut Mage) -> Option<(String, i16, i16)> {
    let (px, py) = (p.x, p.y);
    let (_, mut got) = cast(p, 12, px, py, m);
    let mut seen = Vec::new();
    p.c.wait_for(Duration::from_secs(5), &mut seen, |b| u32at(b, 0) == RESPONSE_INITDATA);
    got.extend(seen);
    let d = got.iter().find(|b| u32at(b, 0) == RESPONSE_INITDATA)?;
    let at = (String::from_utf8_lossy(cstr(&d[30..40])).to_string(), u16at(d, 8) as i16 + 19, u16at(d, 10) as i16 + 17);
    (p.x, p.y) = (at.1, at.2);
    p.c.drain(Duration::from_millis(600));
    Some(at)
}

/// `hgprobe recallpoint LOGIN MAPDATA OUT.jsonl`: MSGID_REQUEST_SETRECALLPNT. RtHg1 (aresden, level 60, Recall) in the
/// middle of the town asks for recall point 4 (aresden's initial point 4 is (140,205)) and recalls: it lands on
/// aresden near that point (the level 60 plain recall goes to arefarm); at the map entry the choice is forgotten, so
/// the next recall is the plain one.
pub fn recallpoint(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    let walk = amd(mapdir, "aresden");
    let (sx, sy) = (140..=190).flat_map(|x| (85..=125).map(move |y| (x, y))).find(|&(x, y)| walk(x, y)).expect("a free tile in town");
    let mut mastery = vec![b'0'; 100];
    mastery[12] = b'1';
    let mastery = String::from_utf8(mastery).unwrap();
    sql(&format!("UPDATE char_database SET MapLoc='aresden', LocX={sx}, LocY={sy}, Level=60, Exp=39000, Strenght=20, Vitality=40, Dexterity=10, Intelligence=77, Magic=90, Agility=10, HP=400, MP=338, SP=150, Hunger=100, PK=0, Nation='aresden', LockMapName='NONE', LockMapTime=0, LeftSpecTime=0, MagicMastery='{mastery}' WHERE char_name='RtHg1' AND account_name LIKE 'rt%';"));
    let Some(gs) = enter_game(login, b"rthg1", b"RtHg1", ENTER_NEW) else { return k.0 + 1 };
    let Some(mut p) = init_player(&gs, b"rthg1", b"RtHg1", 1, &log, &mut k, None) else { return k.0 + 1 };
    let mut m = Mage { slimes: Default::default(), frozen: Vec::new() };
    k.check(p.map == b"aresden", &format!("RtHg1 entered aresden at ({},{})", p.x, p.y));
    p.c.send(&keepalive());
    let mut b = body(MSGID_REQUEST_SETRECALLPNT, 0, &[], 7);
    b[6] = 4;
    p.c.send(&b);
    let got = p.c.drain(Duration::from_millis(400));
    k.check(got.iter().all(|b| u32at(b, 0) != RESPONSE_INITDATA), "the recall point request is not answered");
    let at = recall(&mut p, &mut m);
    println!("     recall with point 4: {at:?}");
    k.check(at.as_ref().is_some_and(|a| a.0 == "aresden" && (a.1 - 140).abs() <= 3 && (a.2 - 205).abs() <= 3), &format!("recall point 4 -> aresden near (140,205) {at:?}"));
    let at = recall(&mut p, &mut m);
    println!("     second recall: {at:?}");
    k.check(at.as_ref().is_some_and(|a| a.0 == "arefarm"), &format!("the choice is forgotten at the map entry: a plain recall goes to arefarm {at:?}"));
    k.0
}

/// `hgprobe polymorph LOGIN MAPDATA OUT.jsonl`: a polymorph spell. Needs a Magic.cfg line `magic = 5 Polymorph 20 0 6
/// 10 1 1 1 0 0 0 0 0 0 0 0 -1 0 0` (type 20, 6 s, value 1: none of the shipped spells has the type). RtHg1 (GM
/// level 4, spell 5 learned) casts it on RtHg2 and then on a summoned slime: the target is shown as type 18
/// (OBJECTNULLACTION), RtHg2 is told NOTIFY_MAGICEFFECTON (20, 1); a second cast on the same target does nothing; after
/// the 6 s NOTIFY_MAGICEFFECTOFF (20, 1) and, in the Rust HG, the original type is shown again (the C++ never gave it back).
pub fn polymorph(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    ensure_char(login, b"rthg2", b"RtHg2");
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg1';");
    let walk = amd(mapdir, "aresden");
    let (sx, sy) = (140..=190).flat_map(|x| (85..=125).map(move |y| (x, y))).find(|&(x, y)| (-2..=3).all(|d| walk(x + d, y))).expect("free tiles in town");
    let mut mastery = vec![b'0'; 100];
    mastery[5] = b'1';
    let mastery = String::from_utf8(mastery).unwrap();
    sql(&format!("UPDATE char_database SET MapLoc='aresden', LocX={sx}, LocY={sy}, Level=100, Exp=39000, Strenght=20, Vitality=40, Dexterity=10, Intelligence=77, Magic=90, Agility=10, HP=900, MP=338, SP=150, Hunger=100, PK=0, Nation='aresden', LockMapName='NONE', LockMapTime=0, LeftSpecTime=0, AdminLevel=4, MagicMastery='{mastery}' WHERE char_name='RtHg1' AND account_name LIKE 'rt%';"));
    sql(&format!("UPDATE char_database SET MapLoc='aresden', LocX={}, LocY={sy}, Level=30, Exp=9000, Strenght=20, Vitality=20, Dexterity=20, Intelligence=10, Magic=10, Agility=10, HP=300, MP=100, SP=50, Hunger=100, PK=0, Nation='aresden', LockMapName='NONE', LockMapTime=0, LeftSpecTime=0, AdminLevel=0 WHERE char_name='RtHg2' AND account_name LIKE 'rt%';", sx + 2));
    let Some(gs) = enter_game(login, b"rthg1", b"RtHg1", ENTER_NEW) else { return k.0 + 1 };
    let Some(mut p) = init_player(&gs, b"rthg1", b"RtHg1", 1, &log, &mut k, None) else { return k.0 + 1 };
    let Some(gs2) = enter_game(login, b"rthg2", b"RtHg2", ENTER_NEW) else { return k.0 + 1 };
    let Some(mut q) = init_player(&gs2, b"rthg2", b"RtHg2", 2, &log, &mut k, None) else { return k.0 + 1 };
    let mut m = Mage { slimes: Default::default(), frozen: Vec::new() };
    p.c.drain(Duration::from_millis(500));
    q.c.drain(Duration::from_millis(300));
    // the types shown by the OBJECTNULLACTIONs of object `id` in `got`
    let types = |got: &[Vec<u8>], id: u16| -> Vec<u16> { got.iter().filter(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTNULLACTION && u16at(b, 6) % 30000 == id % 30000 && b.len() >= 27).map(|b| u16at(b, 12)).collect() };

    // --- on a player
    let (qx, qy) = (q.x, q.y);
    let (_, got) = cast(&mut p, 5, qx, qy, &mut m);
    let seen_by_q = q.c.drain(Duration::from_millis(500));
    let t0 = Instant::now();
    let on = has_n(&seen_by_q, NOTIFY_MAGICEFFECTON);
    k.check(on.is_some_and(|b| b.len() == 16 && u16at(b, 6) == 20 && u32at(b, 8) == 1), &format!("RtHg2: NOTIFY_MAGICEFFECTON 16 bytes (20, 1) {:?}", on.map(|b| hex(b))));
    let shown = types(&got, q.id);
    k.check(shown == [18], &format!("RtHg1 sees RtHg2 as type 18 (OBJECTNULLACTION): {shown:?}"));
    k.check(has_n(&got, NOTIFY_MP).is_some() && magic_event(&got, 5), "the cast spends mana: NOTIFY_MP and the COMMONTYPE_MAGIC event");
    let (_, got) = cast(&mut p, 5, qx, qy, &mut m);
    let again = q.c.drain(Duration::from_millis(500));
    k.check(has_n(&again, NOTIFY_MAGICEFFECTON).is_none() && types(&got, q.id).is_empty() && has_n(&got, NOTIFY_MP).is_some(), "a second cast on the same target: no new effect, the mana is spent");
    let mut off = None;
    let mut restored = Vec::new();
    while t0.elapsed() < Duration::from_secs(14) && off.is_none() {
        p.c.send(&keepalive());
        q.c.send(&keepalive());
        let g = p.c.drain(Duration::from_millis(400));
        restored.extend(types(&g, q.id));
        for b in q.c.drain(Duration::from_millis(400)) {
            if u32at(&b, 0) == NOTIFY && ty(&b) == NOTIFY_MAGICEFFECTOFF && off.is_none() {
                off = Some((t0.elapsed().as_secs(), hex(&b)));
            }
        }
    }
    let g = p.c.drain(Duration::from_millis(500));
    restored.extend(types(&g, q.id));
    println!("     player: effect off {off:?}, types shown after it {restored:?}");
    k.check(off.as_ref().is_some_and(|(s, _)| (5..=9).contains(s)), &format!("RtHg2: NOTIFY_MAGICEFFECTOFF after the 6 s {off:?}"));
    println!("     (the Rust HG shows the original type again after the effect, the C++ never does: {restored:?})");

    // --- on an NPC
    let mut tries = 0;
    let mut slime = None;
    while tries < 6 && slime.is_none() {
        tries += 1;
        p.c.send(&keepalive());
        p.c.send(&super::social::chat_body(&p, b"RtHg1", b"/summon Slime 1 0 0"));
        let g = p.c.drain(Duration::from_millis(900));
        m.feed(&g);
        let Some((&o, _)) = m.slimes.iter().next() else { continue };
        p.c.send(&body(MSGID_REQUEST_FULLOBJECTDATA, o, &[], 6));
        let g = p.c.drain(Duration::from_millis(300));
        m.feed(&g);
        let Some(&(x, y)) = m.slimes.get(&o) else { continue };
        let (_, got) = cast(&mut p, 5, x, y, &mut m);
        if types(&got, o) == [18] {
            slime = Some(o);
        }
    }
    k.check(slime.is_some(), "the cast slime is shown as type 18 (OBJECTNULLACTION of the NPC)");
    let t1 = Instant::now();
    let mut back = Vec::new();
    while t1.elapsed() < Duration::from_secs(12) {
        p.c.send(&keepalive());
        let g = p.c.drain(Duration::from_millis(500));
        if let Some(o) = slime {
            back.extend(types(&g, o));
        }
    }
    println!("     NPC: types shown after the effect {back:?} (Rust: back to 10; C++: nothing)");
    drop(q);
    drop(p);
    k.0
}
