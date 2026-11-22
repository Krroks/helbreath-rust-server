//! `hgprobe skills`: RtHg1 (aresden 110,56) uses a FishingManual (teaches fishing, skill 5, to 20: Item.cfg was
//! renumbered on 2026-11-20), casts a FishingRod at the water of fish point 0 (59,99), uses an exp
//! slate, builds a slate from the four pieces, upgrades a Dagger with a Stone of Xelima and an AngelicPendant with
//! no Gizon points, sets its down skill and tries to play dead (skill 10: never loaded, see below), then stops.
//! `hgprobe mining`: RtHg1 (aresdend1 25,131) with a PickAxe and mining 100 waits for a mineral on (24,132) and
//! hits it until it is gone (run it with only that mineral point in the test copy's aresdend1.txt).
use super::fight::{keepalive, motion_body};
use super::items::{cmd, enter, item_list, place, slot};
use super::*;

/// Skilltypes.h: fishing is skill 5 since the renumbering
const SKILL_FISHING: i32 = 5;

fn ty(b: &[u8]) -> i32 {
    u16at(b, 4) as i32
}

fn sizes(got: &[Vec<u8>], t: i32) -> Vec<usize> {
    got.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == t).map(|b| b.len()).collect()
}

fn go(p: &mut Player, f: impl Fn(&Player) -> Vec<u8>, wait: u64, what: &str) -> Vec<Vec<u8>> {
    let b = f(p);
    p.c.send(&keepalive());
    p.c.send(&b);
    let got = p.c.drain(Duration::from_millis(wait));
    println!("     {what}: {:?}", got.iter().map(|b| format!("{:08x}/{:04x} {}", u32at(b, 0), u16at(b, 4), b.len())).collect::<Vec<_>>());
    got
}

fn item_row(id: &str, n: u32, name: &str, life: u32, effect2: i32, attr: u32) {
    sql(&format!("INSERT INTO item (CharID, ItemName, ItemID, Count, ItemType, ID1, ID2, ID3, Color, Effect1, Effect2, Effect3, LifeSpan, Attribute, ItemEquip, ItemPosX, ItemPosY) VALUES ({id}, '{name}', {}, 1, 0, 0, 0, 0, 0, 0, {effect2}, 0, {life}, {attr}, 0, 40, 40);", 9_800_000 + id.parse::<u32>().unwrap_or(0) * 20 + n));
}

fn set_skill(id: &str, s: u32, m: u32) {
    // in place: the Logins take the first 10 rows in insertion order, a re-inserted row 1 would fall behind row 10
    sql(&format!("UPDATE skill SET SkillMastery={m}, SkillSSN=0 WHERE CharID={id} AND SkillID={s}; INSERT INTO skill (CharID, SkillID, SkillMastery, SkillSSN) SELECT {id}, {s}, {m}, 0 FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM skill WHERE CharID={id} AND SkillID={s});"));
}

pub fn skills(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    let id1 = place("RtHg1", "aresden", 110, 56, "");
    sql(&format!("DELETE FROM item WHERE CharID={id1} AND CharID<>0; DELETE FROM bank_item WHERE CharID={id1} AND CharID<>0;"));
    for (n, (name, life, e2, attr)) in [
        ("FishingManual", 1, 0, 0u32),
        ("FishingRod", 200, 0, 0),
        ("AcientTablet", 1, 4, 0),
        ("AcientTablet(LU)", 1, 0, 0),
        ("AcientTablet(LD)", 1, 0, 0),
        ("AcientTablet(RU)", 1, 0, 0),
        ("AcientTablet(RD)", 1, 0, 0),
        ("Dagger", 300, 0, 0),
        ("StoneOfXelima", 1, 0, 0),
        ("AngelicPendant(STR)", 300, 0, 0x3000_0000),
    ]
    .iter()
    .enumerate()
    {
        item_row(&id1, n as u32, name, *life, *e2, *attr);
    }
    set_skill(&id1, SKILL_FISHING as u32, 10);
    // (pretend corpse is skill 10: a new character has it at 0, so UseSkillHandler answers SKILLUSINGEND; only the
    // Rust login + Rust HG carry it at all, see `corpse`)
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let (bag, _) = item_list(&p.items);
    let s = |n: &str| slot(&bag, n);
    k.check(s("FishingManual") >= 0 && s("AcientTablet(RD)") >= 0 && s("StoneOfXelima") >= 0, &format!("bag {bag:?}"));

    let got = go(&mut p, |x| cmd(x, COMMONTYPE_REQ_USEITEM, 0, [s("FishingManual"), 0, 0], b"", 0), 700, "FishingManual");
    let tr = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_SKILLTRAINSUCCESS).map(|b| (b.len(), b[6], b[7]));
    k.check(tr == Some((8, SKILL_FISHING as u8, 20)) && sizes(&got, NOTIFY_ITEMDEPLETED_ERASEITEM) == [10], &format!("SKILLTRAINSUCCESS 8 (5, 20) {tr:?} + ITEMDEPLETED 10"));

    // with fishing at 20 the cast usually misses (20 in 105) or finds no fish yet: it ends, or starts fishing mode
    let got = go(&mut p, |x| cmd(x, COMMONTYPE_REQ_USEITEM, 0, [s("FishingRod"), 59, 99], b"", 0), 7000, "FishingRod at (59, 99)"); // fishing ends after its 6 s delay
    let end = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_SKILLUSINGEND).map(|b| (b.len(), u16at(b, 6)));
    k.check(end.is_some_and(|e| e.0 == 8 && e.1 <= 1), &format!("rod: SKILLUSINGEND 8 (0/1) {end:?}"));

    let got = go(&mut p, |x| cmd(x, COMMONTYPE_REQ_USEITEM, 0, [s("AcientTablet"), 0, 0], b"", 0), 700, "exp slate");
    let st: Vec<u32> = got.iter().filter(|b| u32at(b, 0) == EVENT_MOTION && b.len() == 43 && u16at(b, 6) % 30000 == p.id).map(|b| u32at(b, 37)).collect();
    k.check(sizes(&got, NOTIFY_SLATE_EXP) == [6] && st.iter().any(|v| v & 0x1_0000 != 0), &format!("SLATE_EXP 6 {:?} + own NULLACTION with STATUS_GREENSLATE {st:x?}", sizes(&got, NOTIFY_SLATE_EXP)));

    let pieces = [s("AcientTablet(LU)"), s("AcientTablet(LD)"), s("AcientTablet(RU)"), s("AcientTablet(RD)")];
    let packed = i32::from_le_bytes(pieces.map(|x| x as u8));
    let got = go(&mut p, |x| cmd(x, COMMONTYPE_REQ_CREATESLATE, 0, [packed, 0, 0], b"", 0), 700, "create slate");
    let (ok, fail) = (sizes(&got, NOTIFY_SLATE_CREATESUCCESS), sizes(&got, NOTIFY_SLATE_CREATEFAIL));
    let dep = sizes(&got, NOTIFY_ITEMDEPLETED_ERASEITEM).len();
    k.check(dep == 4 && ((ok == [10] && sizes(&got, NOTIFY_ITEMOBTAINED) == [53]) || fail == [6]), &format!("4 pieces used {dep}, CREATESUCCESS 10 + ITEMOBTAINED 53 {ok:?} or CREATEFAIL 6 {fail:?}"));

    let got = go(&mut p, |x| cmd(x, COMMONTYPE_UPGRADEITEM, 0, [s("Dagger"), 0, 0], b"", 0), 700, "upgrade Dagger");
    let at: Vec<(usize, u32)> = got.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_ITEMATTRIBUTECHANGE).map(|b| (b.len(), u32at(b, 8))).collect();
    k.check(at.len() == 1 && at[0].0 == 20 && at[0].1 >> 28 <= 1 && sizes(&got, NOTIFY_ITEMDEPLETED_ERASEITEM).len() == 1, &format!("ITEMATTRIBUTECHANGE 20 (level 0/1) {at:x?} + the stone used"));

    let got = go(&mut p, |x| cmd(x, COMMONTYPE_UPGRADEITEM, 0, [s("AngelicPendant(STR)"), 0, 0], b"", 0), 700, "upgrade pendant (no Gizon)");
    let f = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_ITEMUPGRADEFAIL).map(|b| (b.len(), u16at(b, 6)));
    k.check(f == Some((8, 3)), &format!("ITEMUPGRADEFAIL 8 (3) {f:?}"));

    let got = go(&mut p, |x| cmd(x, COMMONTYPE_REQ_SETDOWNSKILLINDEX, 0, [1, 0, 0], b"", 0), 700, "down skill 1");
    let d = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_DOWNSKILLINDEXSET).map(|b| (b.len(), u16at(b, 6)));
    k.check(d == Some((8, 1)), &format!("DOWNSKILLINDEXSET 8 (1) {d:?}"));

    let got = go(&mut p, |x| cmd(x, COMMONTYPE_REQ_USESKILL, 0, [10, 0, 0], b"", 0), 700, "pretend corpse");
    let dying = got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTDYING as i32 && u16at(b, 6) % 30000 == p.id).map(|b| b.len());
    let end = sizes(&got, NOTIFY_SKILLUSINGEND);
    k.check(dying.is_none() && end == [8], &format!("skill 10 at 0: SKILLUSINGEND 8 {end:?} (no OBJECTDYING {dying:?})"));
    let got = go(&mut p, |x| motion_body(x, OBJECTSTOP as u16, 5, &[]), 700, "stand up (stop)");
    k.check(got.iter().any(|b| u32at(b, 0) == MSGID_RESPONSE_MOTION), "stop confirmed");
    k.0
}

/// `hgprobe corpse LOGIN OUT [lost]`: RtHg1 (aresden 110,56) with pretend corpse (skill 10) at 100 in the DB plays
/// dead (OBJECTDYING: mastery loaded; its SSN goes up by 8), logs out (the row must come back as 100/8 although the
/// probe set it to 7/0 meanwhile: the HG saved it), enters again and plays dead again (row 100/16 after).
/// With `lost` (a C++ Login or HG, which carry skills 0..9 only): SKILLUSINGEND both times, the row stays as set.
pub fn corpse(login: &str, out: &str, lost: bool) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    // level 50: ValidateSkills caps pretend corpse at level * 2
    let id1 = place("RtHg1", "aresden", 110, 56, ", Level=50");
    set_skill(&id1, 10, 100);
    let row = || sql(&format!("SELECT SkillMastery, SkillSSN FROM skill WHERE CharID={id1} AND SkillID=10;"));
    let skills0 = sql(&format!("SELECT GROUP_CONCAT(SkillMastery ORDER BY SkillID) FROM skill WHERE CharID={id1} AND SkillID<10;"));
    for round in 1..=2u32 {
        let Some(mut p) = (if round == 1 { enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) } else {
            // the C++ Login frees a logged-out account ~50-60 s after its enter
            let t = Instant::now();
            let mut gs = None;
            while gs.is_none() && t.elapsed() < Duration::from_secs(150) {
                std::thread::sleep(Duration::from_secs(2));
                gs = enter_game(login, b"rthg1", b"RtHg1", ENTER_NEW);
            }
            gs.and_then(|gs| init_player(&gs, b"rthg1", b"RtHg1", 1, &log, &mut k, None))
        }) else { return k.0 + 1 };
        // an NPC next to the player also refuses it (SKILLUSINGEND): a few tries
        let (mut dying, mut end) = (false, Vec::new());
        for _ in 0..(if lost { 1 } else { 6 }) {
            let got = go(&mut p, |x| cmd(x, COMMONTYPE_REQ_USESKILL, 0, [10, 0, 0], b"", 0), 2000, "pretend corpse");
            dying = got.iter().any(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTDYING as i32 && u16at(b, 6) % 30000 == p.id);
            end = sizes(&got, NOTIFY_SKILLUSINGEND);
            if dying {
                break;
            }
        }
        if lost {
            k.check(!dying && end == [8], &format!("round {round}: skill 10 not carried: SKILLUSINGEND 8 {end:?}"));
        } else {
            k.check(dying && end.is_empty(), &format!("round {round}: skill 10 at 100 loaded: own OBJECTDYING {dying} (last try: no SKILLUSINGEND {end:?})"));
        }
        go(&mut p, |x| motion_body(x, OBJECTSTOP as u16, 5, &[]), 700, "stand up (stop)");
        sql(&format!("UPDATE skill SET SkillMastery=7, SkillSSN=0 WHERE CharID={id1} AND SkillID=10;"));
        drop(p);
        let want = if lost { "7	0".to_string() } else { format!("100	{}", 8 * round) };
        let t = Instant::now();
        while row() != want && t.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(500));
        }
        std::thread::sleep(Duration::from_secs(2));
        k.check(row() == want, &format!("round {round}: skill 10 row after logout {:?}, expected {want:?}", row()));
        let skills = sql(&format!("SELECT GROUP_CONCAT(SkillMastery ORDER BY SkillID) FROM skill WHERE CharID={id1} AND SkillID<10;"));
        k.check(skills == skills0, &format!("round {round}: skills 0..9 unchanged {skills} (was {skills0})"));
    }
    k.0
}

/// `hgprobe fishing LOGIN OUT`: RtHg1 (aresden 110,56) with fishing (skill 5) at 100 and a FishingRod casts at the
/// water of fish point 0 (59,99) until a fish puts it in fishing mode, lets the chance climb, then pulls
/// (GETFISHTHISTIME) until a fish is caught. Only works since Item.cfg gives the rod skill 5 (before: skill 1).
pub fn fishing(login: &str, out: &str, at: Option<(&str, i32, i32, i32, i32)>) -> u32 {
    // MAP STAND_X STAND_Y CAST_X CAST_Y; default: aresden's fish point 0 (on the town lake tiles, which neither HG counts as water)
    let (map, sx, sy, cx, cy) = at.unwrap_or(("aresden", 110, 56, 59, 99));
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    let id1 = place("RtHg1", map, sx as i16, sy as i16, ", Level=50"); // ValidateSkills caps each skill at level x 2
    sql(&format!("DELETE FROM item WHERE CharID={id1} AND CharID<>0;"));
    item_row(&id1, 0, "FishingRod", 1000, 0, 0);
    set_skill(&id1, SKILL_FISHING as u32, 100);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let (bag, _) = item_list(&p.items);
    let rod = slot(&bag, "FishingRod");
    let (mut fish_mode, mut caught) = (false, false);
    let start = std::time::Instant::now();
    while !caught && start.elapsed() < Duration::from_secs(300) {
        if !fish_mode {
            let got = go(&mut p, |x| cmd(x, COMMONTYPE_REQ_USEITEM, 0, [rod, cx, cy], b"", 0), 7000, "cast"); // a recast within the 6 s delay cancels the previous one
            fish_mode = sizes(&got, NOTIFY_EVENTFISHMODE).len() == 1;
            continue;
        }
        // FishProcessor raises the chance every 4 s with fishing at 100
        let got = go(&mut p, |_| keepalive(), 12_000, "wait for the chance");
        let chances: Vec<u16> = got.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_FISHCHANCE).map(|b| u16at(b, 6)).collect();
        println!("     chances {chances:?}");
        let got = go(&mut p, |x| cmd(x, COMMONTYPE_REQ_GETFISHTHISTIME, 0, [0, 0, 0], b"", 0), 1500, "pull");
        caught = sizes(&got, NOTIFY_FISHSUCCESS) == [6] || !sizes(&got, NOTIFY_FISHSUCCESS).is_empty();
        if caught {
            k.check(got.iter().any(|b| u32at(b, 0) == MSGID_EVENT_COMMON && u16at(b, 4) as i32 == COMMONTYPE_ITEMDROP), "the fish lies on the fisher's tile (ITEMDROP)");
        }
        fish_mode = false; // a fail or a success ends fishing mode: cast again
    }
    k.check(caught, &format!("caught a fish within {:?}", start.elapsed()));
    k.0
}

pub fn mining(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    let id1 = place("RtHg1", "aresdend1", 25, 131, ", Strenght=100");
    sql(&format!("DELETE FROM item WHERE CharID={id1} AND CharID<>0;"));
    item_row(&id1, 0, "PickAxe", 1000, 0, 0);
    set_skill(&id1, 3, 100);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let (bag, _) = item_list(&p.items);
    go(&mut p, |x| cmd(x, COMMONTYPE_EQUIPITEM, 0, [slot(&bag, "PickAxe"), 0, 0], b"", 0), 700, "equip PickAxe");
    go(&mut p, |x| cmd(x, COMMONTYPE_TOGGLECOMBATMODE, 0, [0, 0, 0], b"", 0), 700, "combat mode");
    // wait for the mineral (MineralGenerator: every 10 s, a 1 in 4 roll)
    let mut seen = false;
    let start = std::time::Instant::now();
    while !seen && start.elapsed() < Duration::from_secs(240) {
        p.c.send(&keepalive());
        for b in p.c.drain(Duration::from_millis(1000)) {
            if u32at(&b, 0) == MSGID_DYNAMICOBJECT && u16at(&b, 4) == MSGTYPE_CONFIRM as u16 && (u16at(&b, 6), u16at(&b, 8)) == (24, 132) {
                println!("     mineral: {}", hex(&b));
                k.check(b.len() == 18 && matches!(u16at(&b, 10) as i32, DYNAMICOBJECT_MINERAL1 | DYNAMICOBJECT_MINERAL2), "DYNAMICOBJECT confirm 18 bytes, a mineral type");
                seen = true;
            }
        }
    }
    // a mineral already on the tile at the enter is in the map data, not in an event: try hitting anyway
    let (mut drops, mut gone, mut exp) = (0, false, 0);
    for _ in 0..40 {
        let mut extra = Vec::new();
        extra.extend_from_slice(&24i16.to_le_bytes());
        extra.extend_from_slice(&132i16.to_le_bytes());
        extra.extend_from_slice(&1u16.to_le_bytes());
        extra.extend_from_slice(&0u16.to_le_bytes());
        let got = go(&mut p, |x| motion_body(x, OBJECTATTACK as u16, 6, &extra), 1100, "hit the mineral");
        drops += got.iter().filter(|b| u32at(b, 0) == MSGID_EVENT_COMMON && u16at(b, 4) == COMMONTYPE_ITEMDROP as u16 && b.len() == 18 && (u16at(b, 6), u16at(b, 8)) == (25, 131)).count();
        exp += sizes(&got, NOTIFY_EXP).len();
        if got.iter().any(|b| u32at(b, 0) == MSGID_DYNAMICOBJECT && u16at(b, 4) == MSGTYPE_REJECT as u16 && (u16at(b, 6), u16at(b, 8)) == (24, 132)) {
            gone = true;
            break;
        }
    }
    // (GetExp divides by 100: a mining exp roll of 1..15 gives no exp at all)
    k.check(drops > 0, &format!("mined: {drops} ITEMDROP 18 on the miner's tile ({exp} NOTIFY_EXP)"));
    k.check(gone, "the mineral is used up (DYNAMICOBJECT reject)");
    k.0
}
