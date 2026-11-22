//! `hgprobe fight LOGIN_HOST:PORT MAPDATA_DIR OUT.jsonl`
//!
//! Fight scenario on aresden next to the slime spot (spot 1): session 1 puts RtHg1 there with the exp one short
//! of the next level, turns combat mode on, walks to slimes and hits them until one dies (exp, level up), spends
//! the 3 new stat points and logs out; session 2 (RtHg2) starts with 3 HP, stands next to slimes until one kills it,
//! asks to restart and must reappear (RESPONSE_INITDATA on `default`, travellers' restart map).
use super::*;

/// Walkability (attribute bits 0x80 = blocked, 0x40 = teleport) of a map's .amd, inside the 20-tile border.
pub fn amd(mapdir: &str, map: &str) -> impl Fn(i16, i16) -> bool {
    let raw = std::fs::read(format!("{mapdir}/{map}.amd")).expect("amd");
    let head = String::from_utf8_lossy(&raw[..256]).replace('\0', " ");
    let num = |k: &str| head.split(k).nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty())).and_then(|s| s.parse::<usize>().ok()).unwrap();
    let (sx, sy, ts) = (num("MAPSIZEX"), num("MAPSIZEY"), num("TILESIZE"));
    move |x: i16, y: i16| x >= 20 && y >= 20 && (x as usize) < sx - 20 && (y as usize) < sy - 20 && raw[256 + (y as usize * sx + x as usize) * ts + 8] & 0xC0 == 0
}

pub fn motion_body(p: &Player, ty: u16, dir: u8, extra: &[u8]) -> Vec<u8> {
    let mut b = body(COMMAND_MOTION, ty, &[], 6);
    b.extend_from_slice(&p.x.to_le_bytes());
    b.extend_from_slice(&p.y.to_le_bytes());
    b.push(dir);
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(extra);
    b.resize(b.len().max(17), 0);
    b
}

pub fn dir_to(x: i16, y: i16, tx: i16, ty: i16) -> u8 {
    match ((tx - x).signum(), (ty - y).signum()) {
        (0, -1) => 1,
        (1, -1) => 2,
        (1, 0) => 3,
        (1, 1) => 4,
        (0, 1) => 5,
        (-1, 1) => 6,
        (-1, 0) => 7,
        (-1, -1) => 8,
        _ => 0,
    }
}
const DX: [i16; 9] = [0, 0, 1, 1, 1, 0, -1, -1, -1];
const DY: [i16; 9] = [0, -1, -1, 0, 1, 1, 1, 0, -1];

#[derive(Default)]
pub struct Fight {
    npcs: std::collections::HashMap<u16, (i16, i16)>, // object id (10000+) -> position
    pub hp: i32,
    hp_seen: Vec<i32>,
    pub kills: u32,
    pub killed: bool,
    pub exp_notes: u32,
    level_up: Option<u32>,
    damage_taken: u32,
    hits_landed: u32,
    attack_confirms: u32,
    types: std::collections::BTreeMap<u16, u16>, // NPC object id -> type
    drops: Vec<(i16, i16, [u16; 3])>,            // COMMONTYPE_ITEMDROP: x, y, sprite/frame/color
    set_items: u32,
    obtained: Vec<Vec<u8>>,
    /// NOTIFY_QUESTCOUNTER / NOTIFY_QUESTCOMPLETED as they come
    pub quest: Vec<Vec<u8>>,
    /// the spell numbers (+ 100) of COMMONTYPE_MAGIC events
    pub magic: Vec<u16>,
}

impl Fight {
    pub fn full() -> Fight {
        Fight { hp: i32::MAX, ..Default::default() }
    }

    /// Tracks NPC positions and the notifications of interest from one server message.
    pub fn feed(&mut self, b: &[u8], me: u16) {
        let (id, ty) = (u32at(b, 0), u16at(b, 4) as i32);
        match id {
            EVENT_MOTION | EVENT_LOG if b.len() == 27 => {
                let o = u16at(b, 6);
                if id == EVENT_LOG && ty == MSGTYPE_REJECT {
                    self.npcs.remove(&o);
                } else if b[26] == 0 {
                    self.types.insert(o, u16at(b, 12));
                    if u16at(b, 12) == 10 {
                        // slimes only: rabbits and cats (side 0) never fight back
                        self.npcs.insert(o, (u16at(b, 8) as i16, u16at(b, 10) as i16));
                    }
                }
            }
            MSGID_EVENT_COMMON if b.len() == 18 && ty == COMMONTYPE_ITEMDROP => {
                self.drops.push((u16at(b, 6) as i16, u16at(b, 8) as i16, [u16at(b, 10), u16at(b, 12), u16at(b, 14)]));
            }
            MSGID_EVENT_COMMON if b.len() == 18 && ty == COMMONTYPE_SETITEM => self.set_items += 1,
            MSGID_EVENT_COMMON if b.len() == 18 && ty == COMMONTYPE_MAGIC => self.magic.push(u16at(b, 14)),
            EVENT_MOTION if ty == OBJECTDYING && b.len() == 15 && u16at(b, 6) >= 40000 => {
                self.kills += 1;
                self.npcs.remove(&(u16at(b, 6) - 30000));
            }
            EVENT_MOTION if ty == OBJECTDAMAGE && u16at(b, 6) >= 40000 => self.hits_landed += 1,
            EVENT_MOTION if ty == OBJECTDAMAGE && u16at(b, 6) == me + 30000 => self.damage_taken += 1,
            RESPONSE_MOTION if ty == OBJECTMOTION_ATTACK_CONFIRM => self.attack_confirms += 1,
            NOTIFY => match ty {
                NOTIFY_HP => {
                    let hp = u32at(b, 6) as i32;
                    if hp < self.hp {
                        self.damage_taken += 1;
                    }
                    self.hp = hp;
                    self.hp_seen.push(hp);
                }
                NOTIFY_DAMAGEMOVE => self.damage_taken += 1,
                NOTIFY_QUESTCOUNTER | NOTIFY_QUESTCOMPLETED => self.quest.push(b.to_vec()),
                NOTIFY_KILLED => self.killed = true,
                NOTIFY_ITEMOBTAINED => self.obtained.push(b.to_vec()),
                NOTIFY_EXP | NOTIFY_TRAVELERLIMITEDLEVEL => self.exp_notes += 1,
                NOTIFY_LEVELUP => self.level_up = Some(u32at(b, 6)),
                _ => {}
            },
            _ => {}
        }
    }
}

/// Walks to the nearest NPC and (with `attack`) hits it, until `done` holds or `secs` pass.
pub fn hunt(p: &mut Player, f: &mut Fight, walk: &dyn Fn(i16, i16) -> bool, attack: bool, secs: u64, done: &dyn Fn(&Fight) -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut last_req = Instant::now() - Duration::from_secs(10);
    while Instant::now() < end && !done(f) {
        if last_req.elapsed() > Duration::from_secs(5) {
            // refresh positions (REQUEST_FULLOBJECTDATA answers with the 27-byte stop form)
            let ids: Vec<u16> = f.npcs.keys().copied().collect();
            for o in ids {
                p.c.send(&body(MSGID_REQUEST_FULLOBJECTDATA, o, &[], 6));
            }
            last_req = Instant::now();
        }
        let me = p.id;
        for b in p.c.drain(Duration::from_millis(450)) {
            if u32at(&b, 0) == RESPONSE_MOTION && u16at(&b, 4) as i32 == OBJECTMOTION_REJECT {
                (p.x, p.y) = (u16at(&b, 6) as i16, u16at(&b, 8) as i16);
            }
            f.feed(&b, me);
        }
        if f.killed || done(f) {
            break;
        }
        let Some((&o, &(nx, ny))) = f.npcs.iter().min_by_key(|(_, &(x, y))| (x - p.x).abs().max((y - p.y).abs())) else { continue };
        if (nx - p.x).abs().max((ny - p.y).abs()) <= 1 {
            if attack {
                let mut extra = Vec::new();
                for v in [nx as u16, ny as u16, 1, o] {
                    extra.extend_from_slice(&v.to_le_bytes());
                }
                let b = motion_body(p, OBJECTATTACK as u16, dir_to(p.x, p.y, nx, ny), &extra);
                p.c.send(&b);
            }
            continue;
        }
        step(p, f, walk, nx, ny);
    }
}

/// One step towards (nx, ny), trying the neighbouring directions when blocked.
fn step(p: &mut Player, f: &mut Fight, walk: &dyn Fn(i16, i16) -> bool, nx: i16, ny: i16) {
    let me = p.id;
    let d0 = dir_to(p.x, p.y, nx, ny) as usize;
    for k in [0usize, 1, 7, 2, 6] {
        let dir = (d0 - 1 + k) % 8 + 1;
        let (tx, ty) = (p.x + DX[dir], p.y + DY[dir]);
        if walk(tx, ty) && !f.npcs.values().any(|&q| q == (tx, ty)) {
            let b = motion_body(p, OBJECTMOVE_, dir as u8, &[]);
            p.c.send(&b);
            let mut seen = Vec::new();
            let r = p.c.wait_for(Duration::from_secs(2), &mut seen, |b| u32at(b, 0) == RESPONSE_MOTION);
            for b in &seen {
                f.feed(b, me);
            }
            match r {
                Some(b) if u16at(&b, 4) == OBJECTMOVE_CONFIRM_ => (p.x, p.y) = (tx, ty),
                Some(b) if u16at(&b, 4) as i32 == OBJECTMOTION_REJECT => (p.x, p.y) = (u16at(&b, 6) as i16, u16at(&b, 8) as i16),
                _ => {}
            }
            break;
        }
    }
}

pub fn keepalive() -> Vec<u8> {
    let mut cc = body(COMMAND_CHECKCONNECTION, 0, &[], 6);
    cc.extend_from_slice(&12345u32.to_le_bytes());
    cc
}

pub fn fight(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    ensure_char(login, b"rthg2", b"RtHg2");
    let walk = amd(mapdir, "aresden");
    // a free tile just south of slime spot 1 (106,43)-(123,55)
    let (sx, sy) = (110..=120).flat_map(|x| (56..=60).map(move |y| (x, y))).find(|&(x, y)| walk(x, y)).expect("free tile near the spot");
    // level 30 with the 157 stat points of that level (dex 57: +7 hit ratio, about half the swings hit a slime)
    let level = 30;
    let l = level + 1;
    let q = l / 17;
    let next = l * l + l * (50 + l * q * q);
    sql(&format!("UPDATE char_database SET MapLoc='aresden', LocX={sx}, LocY={sy}, Level={level}, Exp={}, Strenght=40, Vitality=30, Dexterity=57, Intelligence=10, Magic=10, Agility=10, HP=170, Nation='Traveler' WHERE char_name='RtHg1' AND account_name LIKE 'rt%';", next - 1));
    println!("     RtHg1 level {level}, exp set to {} (next level at {next}), placed at aresden({sx},{sy})", next - 1);

    // session 1: kill slimes, gain exp, level up, spend the points
    let Some(gs) = enter_game(login, b"rthg1", b"RtHg1", ENTER_NEW) else {
        k.check(false, "login enter game");
        return k.0;
    };
    let Some(mut p) = init_player(&gs, b"rthg1", b"RtHg1", 1, &log, &mut k, None) else { return k.0 + 1 };
    k.check(p.map == b"aresden", &format!("entered aresden at ({},{})", p.x, p.y));
    p.c.send(&keepalive());
    let mut f = Fight { hp: i32::MAX, ..Default::default() };
    let mut tog = body(MSGID_COMMAND_COMMON, COMMONTYPE_TOGGLECOMBATMODE as u16, &[], 6);
    tog.extend_from_slice(&p.x.to_le_bytes());
    tog.extend_from_slice(&p.y.to_le_bytes());
    tog.resize(57, 0);
    p.c.send(&tog);
    // run with primary-drop-rate = 10000 in Settings.cfg to see a drop on most kills (~9 % per kill otherwise)
    hunt(&mut p, &mut f, &walk, true, 240, &|f| f.kills >= 3 && f.level_up.is_some() && !f.drops.is_empty());
    k.check(!f.killed, "survived session 1");
    println!("     drops seen: {:?}", f.drops);
    let mut kinds = std::collections::BTreeMap::new();
    for t in f.types.values() {
        *kinds.entry(*t).or_insert(0) += 1;
    }
    println!("     NPC types seen (type: objects): {kinds:?}");
    // rabbits/cats (55/56) come from the random generator anywhere outside the town avoid-rect, so they are only
    // sometimes in view here; informational
    println!("     rabbits/cats in view: {}", kinds.contains_key(&55) || kinds.contains_key(&56));
    k.check(!f.drops.is_empty(), &format!("COMMONTYPE_ITEMDROP after {} kills", f.kills));
    if let Some(&(dx, dy, spr)) = f.drops.first() {
        // walk onto the drop and pick it up (OBJECTGETITEM)
        let end = Instant::now() + Duration::from_secs(30);
        while (p.x, p.y) != (dx, dy) && Instant::now() < end {
            step(&mut p, &mut f, &walk, dx, dy);
            for b in p.c.drain(Duration::from_millis(100)) {
                f.feed(&b, p.id);
            }
        }
        let (n0, s0) = (f.obtained.len(), f.set_items);
        let b = motion_body(&p, OBJECTGETITEM as u16, 1, &[]);
        p.c.send(&b);
        let end = Instant::now() + Duration::from_secs(4);
        while f.obtained.len() == n0 && Instant::now() < end {
            for b in p.c.drain(Duration::from_millis(300)) {
                f.feed(&b, p.id);
            }
        }
        let got = f.obtained.get(n0).cloned();
        k.check(got.as_ref().is_some_and(|b| b.len() == 53), &format!("picked up the drop at ({dx},{dy}) {spr:?}: NOTIFY_ITEMOBTAINED {:?} bytes, SETITEM x{}", got.as_ref().map(|b| b.len()), f.set_items - s0));
        if let Some(b) = got {
            println!("     ITEMOBTAINED body {}", b.iter().map(|x| format!("{x:02x}")).collect::<String>());
        }
    }
    k.check(f.attack_confirms > 0, &format!("attacks confirmed (OBJECTMOTION_ATTACK_CONFIRM x{})", f.attack_confirms));
    k.check(f.hits_landed > 0, &format!("hits on NPCs seen (OBJECTDAMAGE x{})", f.hits_landed));
    k.check(f.kills >= 1, &format!("NPCs killed (OBJECTDYING x{})", f.kills));
    // the exp stock is credited every 5 s
    let end = Instant::now() + Duration::from_secs(8);
    while f.level_up.is_none() && Instant::now() < end {
        for b in p.c.drain(Duration::from_millis(500)) {
            f.feed(&b, p.id);
        }
    }
    k.check(f.exp_notes > 0, &format!("NOTIFY_EXP / TRAVELERLIMITEDLEVEL x{}", f.exp_notes));
    k.check(f.level_up.is_some_and(|l| l as i32 > level && l <= 50), &format!("NOTIFY_LEVELUP -> level {:?} (travellers stop at 50)", f.level_up));
    let mut lu = body(MSGID_LEVELUPSETTINGS, 0, &[], 6);
    lu.extend_from_slice(&[3, 0, 0, 0, 0, 0]);
    p.c.send(&lu);
    let mut seen = Vec::new();
    let r = p.c.wait_for(Duration::from_secs(3), &mut seen, |b| u32at(b, 0) == NOTIFY && matches!(u16at(b, 4) as i32, NOTIFY_SETTING_SUCCESS | NOTIFY_SETTING_FAILED));
    k.check(r.is_some_and(|b| u16at(&b, 4) as i32 == NOTIFY_SETTING_SUCCESS && b.len() == 34), "LEVELUPSETTINGS +3 str -> NOTIFY_SETTING_SUCCESS (34 bytes)");
    println!("     session 1: kills {}, hits {}, damage taken {}, hp seen {:?}", f.kills, f.hits_landed, f.damage_taken, f.hp_seen);
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let row = sql("SELECT Level, Strenght FROM char_database WHERE char_name='RtHg1';");
    k.check(row.split('\t').next().and_then(|v| v.parse::<u32>().ok()) == f.level_up, &format!("logout saved level/str {row:?}"));

    // session 2 (RtHg2: the Login keeps RtHg1's account busy for a while after a logout): 3 HP next to the
    // slimes until killed, then restart
    sql(&format!("UPDATE char_database SET MapLoc='aresden', LocX={sx}, LocY={sy}, HP=3, Nation='Traveler' WHERE char_name='RtHg2' AND account_name LIKE 'rt%';"));
    let Some(gs) = enter_game(login, b"rthg2", b"RtHg2", ENTER_NEW) else {
        k.check(false, "login enter game (session 2)");
        return k.0;
    };
    let Some(mut p) = init_player(&gs, b"rthg2", b"RtHg2", 2, &log, &mut k, None) else { return k.0 + 1 };
    p.c.send(&keepalive());
    let mut f = Fight { hp: 3, ..Default::default() };
    hunt(&mut p, &mut f, &walk, false, 240, &|f| f.killed);
    // a first hit can kill outright (3 HP): OBJECTDYING then, no OBJECTDAMAGE / NOTIFY_HP
    k.check(f.damage_taken > 0 || f.killed, &format!("took damage ({} hits, hp seen {:?})", f.damage_taken, f.hp_seen));
    k.check(f.killed, "NOTIFY_KILLED");
    if f.killed {
        p.c.drain(Duration::from_millis(800));
        p.c.send(&body(MSGID_REQUEST_RESTART, 0, &[], 6));
        let mut seen = Vec::new();
        let r = p.c.wait_for(Duration::from_secs(10), &mut seen, |b| u32at(b, 0) == RESPONSE_INITDATA);
        k.check(r.as_ref().is_some_and(|b| cstr(&b[30..40]) == b"default"), &format!("restart -> RESPONSE_INITDATA on {:?}", r.as_ref().map(|b| String::from_utf8_lossy(cstr(&b[30..40])).to_string())));
        p.c.drain(Duration::from_millis(600));
    }
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let row = sql("SELECT MapLoc, HP FROM char_database WHERE char_name='RtHg2';");
    k.check(row.starts_with("default\t") && !row.ends_with("\t0"), &format!("logout after restart saved {row:?}"));
    k.0
}
