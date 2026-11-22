//! `hgprobe farming LOGIN MAPDATA OUT.jsonl`: seed bags and the hoe on arefarm.
//! RtHg1 (aresden, level 60, farming skill 60, a Hoe and four seed bags) stands next to a farm tile: Ginseng (needs
//! skill 90) -> NOTIFY_AGRICULTURESKILLLIMIT, a watermelon on a tile that is not farmland -> NOTIFY_AGRICULTURENOAREA,
//! a watermelon on the farm tile -> a Crops NPC (type 64, appearance 1 << 8 | 1) in view and the bag used up, a
//! second one on the same tile -> NOAREA (the bag stays). Then the Hoe (weapon 27) hits the crop until it is gone:
//! its appearance goes to stage 2 and 3 (OBJECTNULLACTION), some stage drops a watermelon (COMMONTYPE_ITEMDROP), the
//! last one removes the crop (EVENT_LOG reject).
use super::fight::{dir_to, keepalive, motion_body};
use super::items::{cmd, enter, item_list, place, put_items, slot};
use super::*;

fn n_of(got: &[Vec<u8>], t: i32) -> Option<&Vec<u8>> {
    got.iter().find(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == t)
}

/// (attribute of every tile, size) of a map's .amd
fn tiles(mapdir: &str, map: &str) -> (Vec<u8>, usize, usize) {
    let raw = std::fs::read(format!("{mapdir}/{map}.amd")).expect("amd");
    let head = String::from_utf8_lossy(&raw[..256]).replace('\0', " ");
    let num = |k: &str| head.split(k).nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty())).and_then(|s| s.parse::<usize>().ok()).unwrap();
    let (sx, sy, ts) = (num("MAPSIZEX"), num("MAPSIZEY"), num("TILESIZE"));
    let attr = (0..sx * sy).map(|i| raw[256 + i * ts + 8]).collect();
    (attr, sx, sy)
}

pub fn farming(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    let (attr, sx, sy) = tiles(mapdir, "arefarm");
    let a = |x: i16, y: i16| attr[y as usize * sx + x as usize];
    let free = |x: i16, y: i16| a(x, y) & 0xC0 == 0;
    let farm = |x: i16, y: i16| free(x, y) && a(x, y) & 0x20 != 0 && x >= 14 && (x as usize) < sx - 16 && y >= 12 && (y as usize) < sy - 14;
    // a walkable player tile P with the farm tile T east of it and a free non-farm tile N two west of it
    let (px, py) = (30..sx as i16 - 30)
        .flat_map(|x| (30..sy as i16 - 30).map(move |y| (x, y)))
        .find(|&(x, y)| free(x, y) && farm(x + 1, y) && free(x - 1, y) && !farm(x - 2, y) && free(x - 2, y) && !farm(x - 1, y))
        .expect("a farm tile");
    println!("     P arefarm({px},{py}), farm tile ({},{}), plain tile ({},{})", px + 1, py, px - 2, py);

    let id = place("RtHg1", "arefarm", px, py, ", Level=60, HP=900, Strenght=60, Vitality=40, Dexterity=40, Intelligence=77, Magic=10, Agility=10");
    put_items(&id, &[("Hoe", 1, 10000), ("SeedBag(Ginseng)", 1, 1), ("SeedBag(WaterMelon)", 1, 1), ("SeedBag(WaterMelon)", 1, 1)]);
    sql(&format!("DELETE FROM skill WHERE CharID={id} AND SkillID=6; INSERT INTO skill (CharID, SkillID, SkillMastery, SkillSSN) VALUES ({id}, 6, 60, 0);"));
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"arefarm", &format!("RtHg1 entered arefarm at ({},{})", p.x, p.y));
    let (bag, _) = item_list(&p.items);
    let (hoe, gin, melon) = (slot(&bag, "Hoe"), slot(&bag, "SeedBag(Ginseng)"), slot(&bag, "SeedBag(WaterMelon)"));
    k.check(hoe >= 0 && gin >= 0 && melon >= 0, &format!("the bag holds the hoe and the seeds ({bag:?})"));
    let (tx, ty) = (px + 1, py);
    let step = |p: &mut Player, slot: i32, x: i16, y: i16, what: &str| {
        p.c.send(&keepalive());
        p.c.send(&cmd(p, COMMONTYPE_REQ_USEITEM, 0, [slot, x as i32, y as i32], b"", 0));
        let got = p.c.drain(Duration::from_millis(900));
        println!("     {what}: {:?}", got.iter().map(|b| format!("{:08x}/{:x}:{}", u32at(b, 0), u16at(b, 4), b.len())).collect::<Vec<_>>());
        got
    };

    let got = step(&mut p, gin, tx, ty, "Ginseng seed (skill 90 of 60)");
    k.check(n_of(&got, NOTIFY_AGRICULTURESKILLLIMIT).is_some_and(|b| b.len() == 6) && n_of(&got, NOTIFY_ITEMDEPLETED_ERASEITEM).is_none(), "Ginseng: NOTIFY_AGRICULTURESKILLLIMIT (6 bytes), the bag is kept");
    let got = step(&mut p, melon, px - 2, py, "watermelon on a plain tile");
    k.check(n_of(&got, NOTIFY_AGRICULTURENOAREA).is_some_and(|b| b.len() == 6) && n_of(&got, NOTIFY_ITEMDEPLETED_ERASEITEM).is_none(), "plain tile: NOTIFY_AGRICULTURENOAREA (6 bytes), the bag is kept");
    let got = step(&mut p, melon, tx, ty, "watermelon on the farm tile");
    let crop = got.iter().find(|b| u32at(b, 0) == EVENT_LOG && u16at(b, 4) == MSGTYPE_CONFIRM_ && b.len() == 27 && u16at(b, 12) == 64);
    k.check(crop.is_some_and(|b| (u16at(b, 8), u16at(b, 10)) == (tx as u16, ty as u16) && u16at(b, 20) == 0x0101 && u32at(b, 22) >> 28 == 0), &format!("a Crops NPC (type 64) appears on the farm tile, appearance 0x0101, side 0: {:?}", crop.map(|b| hex(b))));
    k.check(n_of(&got, NOTIFY_ITEMDEPLETED_ERASEITEM).is_some(), "the seed bag is used up (ITEMDEPLETED_ERASEITEM)");
    let Some(crop) = crop.map(|b| u16at(b, 6)) else { return k.0 + 1 };
    let melon2 = melon + 1; // (the bag does not close its gap when an item is used up)
    let got = step(&mut p, melon2, tx, ty, "second watermelon on the same tile");
    k.check(n_of(&got, NOTIFY_AGRICULTURENOAREA).is_some() && n_of(&got, NOTIFY_ITEMDEPLETED_ERASEITEM).is_none(), "a crop on the tile: NOTIFY_AGRICULTURENOAREA, the bag is kept");

    // the hoe: combat mode, equip, hit
    p.c.send(&keepalive());
    p.c.send(&cmd(&p, COMMONTYPE_TOGGLECOMBATMODE, 0, [0, 0, 0], b"", 0));
    p.c.drain(Duration::from_millis(300));
    p.c.send(&cmd(&p, COMMONTYPE_EQUIPITEM, 0, [hoe, 0, 0], b"", 0));
    let got = p.c.drain(Duration::from_millis(800));
    let w = got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && u16at(b, 4) as i32 == OBJECTNULLACTION && u16at(b, 6) % 30000 == p.id).map(|b| (u16at(b, 27) & 0x0FF0) >> 4);
    k.check(w == Some(27), &format!("equip the Hoe: weapon appearance 27 ({w:?})"));
    let (mut stages, mut drops, mut gone, mut hits) = (Vec::new(), 0, false, 0);
    let end = Instant::now() + Duration::from_secs(200);
    while Instant::now() < end && !gone {
        p.c.send(&keepalive());
        let mut extra = Vec::new();
        for v in [tx as u16, ty as u16, 1, crop] {
            extra.extend_from_slice(&v.to_le_bytes());
        }
        let b = motion_body(&p, OBJECTATTACK as u16, dir_to(p.x, p.y, tx, ty), &extra);
        p.c.send(&b);
        hits += 1;
        for b in p.c.drain(Duration::from_millis(650)) {
            let (id, t) = (u32at(&b, 0), u16at(&b, 4));
            if id == EVENT_MOTION && t as i32 == OBJECTNULLACTION && b.len() == 27 && u16at(&b, 6) == crop {
                stages.push(u16at(&b, 20));
            } else if id == MSGID_EVENT_COMMON && t as i32 == COMMONTYPE_ITEMDROP && b.len() == 18 {
                println!("     drop at ({},{}) sprite {} frame {}", u16at(&b, 6), u16at(&b, 8), u16at(&b, 10), u16at(&b, 12));
                drops += 1;
            } else if id == EVENT_LOG && t == MSGTYPE_REJECT_ && u16at(&b, 6) == crop {
                gone = true;
            }
        }
    }
    println!("     {hits} hits, crop appearances {stages:04x?}, drops {drops}");
    k.check(gone, "the harvested crop is removed (EVENT_LOG reject)");
    k.check(stages == [0x0102, 0x0103, 0x0103], &format!("the crop went through the stages 2, 3, 3 ({stages:04x?})"));
    k.check(drops >= 1, &format!("a stage dropped a watermelon ({drops} drops)"));
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    k.0
}
