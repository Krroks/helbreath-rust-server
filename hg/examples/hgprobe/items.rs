//! `hgprobe items LOGIN_HOST:PORT MAPDATA_DIR OUT.jsonl`
//!
//! Item layer. RtHg1 (aresden, level 60, 10 HP, Gold 5000, Dagger, WoodShield, 3 RedPotion, Meat by SQL) in the
//! shop gshop_1: equips the dagger and the shield, releases the dagger, drinks a potion, eats the meat, buys 2 red
//! potions, sells the dagger (offer + confirm), asks a repair price; then walks into nothing more and logs out.
//! RtHg2 (aresden) in the warehouse wrhus_1 gives its Dagger to Howard (bank), logs out (bank row saved), logs in
//! again (bank list) and takes it back (RESPONSE_RETRIEVEITEM), logs out (bag row saved, bank empty).
//! RtHg3 (aresden, int 77) in the magic tower wzdtwr_1 buys Fire-Wall with gold.
use super::fight::keepalive;
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
        MSGID_RESPONSE_RETRIEVEITEM => "RR",
        _ => "",
    };
    if name.is_empty() { format!("{id:08x}/{:x}:{}", ty(b), b.len()) } else { format!("{name}{}:{}", ty(b), b.len()) }
}

/// MSGID_COMMAND_COMMON: x, y, dir, v1, v2, v3, string (30), v4.
pub fn cmd(p: &Player, t: i32, dir: u8, v: [i32; 3], s: &[u8], v4: i32) -> Vec<u8> {
    let mut b = body(MSGID_COMMAND_COMMON, t as u16, &[], 6);
    for x in [p.x, p.y] {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b.push(dir);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    let mut n = s.to_vec();
    n.resize(30, 0);
    b.extend_from_slice(&n);
    b.extend_from_slice(&v4.to_le_bytes());
    b
}

fn n_of(got: &[Vec<u8>], t: i32) -> Option<&Vec<u8>> {
    got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == t)
}

/// (bag: (slot, name, count)), (bank: names) of a PLAYERITEMLISTCONTENTS body.
pub fn item_list(b: &[u8]) -> (Vec<(usize, String, u32)>, Vec<String>) {
    if b.len() < 8 {
        return (Vec::new(), Vec::new());
    }
    let n = b[6] as usize;
    let bag = (0..n).map(|i| (i, String::from_utf8_lossy(cstr(&b[7 + i * 44..27 + i * 44])).to_string(), u32at(b, 27 + i * 44))).collect();
    let o = 7 + n * 44;
    let nb = b.get(o).copied().unwrap_or(0) as usize;
    let bank = (0..nb).map(|i| String::from_utf8_lossy(cstr(&b[o + 1 + i * 43..o + 21 + i * 43])).to_string()).collect();
    (bag, bank)
}

pub fn slot(bag: &[(usize, String, u32)], name: &str) -> i32 {
    bag.iter().find(|(_, n, _)| n == name).map_or(-1, |(i, _, _)| *i as i32)
}

fn step(p: &mut Player, f: impl Fn(&Player) -> Vec<u8>, what: &str) -> Vec<Vec<u8>> {
    let b = f(p);
    p.c.send(&keepalive());
    p.c.send(&b);
    let got = p.c.drain(Duration::from_millis(700));
    println!("     {what}: {:?}", got.iter().map(|b| short(b)).collect::<Vec<_>>());
    got
}

pub fn put_items(id: &str, rows: &[(&str, u32, u32)]) {
    sql(&format!("DELETE FROM item WHERE CharID={id} AND CharID<>0;"));
    sql(&format!("DELETE FROM bank_item WHERE CharID={id} AND CharID<>0;"));
    for (i, (name, count, life)) in rows.iter().enumerate() {
        sql(&format!("INSERT INTO item (CharID, ItemName, ItemID, Count, ItemType, ID1, ID2, ID3, Color, Effect1, Effect2, Effect3, LifeSpan, Attribute, ItemEquip, ItemPosX, ItemPosY) VALUES ({id}, '{name}', {}, {count}, 0, 0, 0, 0, 0, 0, 0, 0, {life}, 0, 0, 40, 40);", 9_900_000 + id.parse::<u32>().unwrap_or(0) * 20 + i as u32));
    }
}

pub fn place(ch: &str, map: &str, x: i16, y: i16, extra: &str) -> String {
    sql(&format!("UPDATE char_database SET MapLoc='{map}', LocX={x}, LocY={y}, Level=60, Exp=39000, Strenght=40, Vitality=40, Dexterity=40, Intelligence=77, Magic=40, Agility=10, HP=10, MP=100, SP=50, Hunger=50, PK=0, Nation='aresden', LockMapName='NONE', LockMapTime=0, LeftSpecTime=0, MagicMastery='{}'{extra} WHERE char_name='{ch}' AND account_name LIKE 'rt%';", "0".repeat(100)));
    sql(&format!("SELECT CharID FROM char_database WHERE char_name='{ch}' AND account_name LIKE 'rt%';"))
}

pub fn enter(login: &str, acc: &[u8], ch: &[u8], who: u8, log: &std::fs::File, k: &mut Checks) -> Option<Player> {
    for _ in 0..18 {
        if let Some(gs) = enter_game(login, acc, ch, ENTER_NEW) {
            return init_player(&gs, acc, ch, who, log, k, None);
        }
        std::thread::sleep(Duration::from_secs(5)); // the C++ Login frees a logged-out account ~50-60 s after its enter
    }
    k.check(false, &format!("login enter game {}", String::from_utf8_lossy(ch)));
    None
}

/// The first NPC of that type: (object id, x, y) from its full object data.
pub fn npc_at(p: &mut Player, npc_type: u16) -> Option<(u16, i16, i16)> {
    let id = p.npcs.iter().find(|n| n.1 == npc_type).map(|n| n.0)?;
    p.c.send(&body(MSGID_REQUEST_FULLOBJECTDATA, id, &[], 6));
    let got = p.c.drain(Duration::from_millis(500));
    got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && u16at(b, 6) % 30000 == id % 30000).map(|b| (id, u16at(b, 8) as i16, u16at(b, 10) as i16))
}

pub fn items(login: &str, mapdir: &str, out: &str) -> u32 {

    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2"), (b"rthg3", b"RtHg3")] {
        ensure_char(login, a, c);
    }
    let id1 = place("RtHg1", "gshop_1", 51, 41, "");
    put_items(&id1, &[("Gold", 5000, 1), ("Dagger", 1, 300), ("WoodShield", 1, 100), ("RedPotion", 3, 1), ("Meat", 1, 1)]);

    // --- shop: equip / release / use / buy / sell / repair
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let (bag, _) = item_list(&p.items);
    println!("     RtHg1 bag {bag:?}");
    let (dagger, shield, potion, meat, gold) = (slot(&bag, "Dagger"), slot(&bag, "WoodShield"), slot(&bag, "RedPotion"), slot(&bag, "Meat"), slot(&bag, "Gold"));
    k.check(dagger >= 0 && shield >= 0 && potion >= 0 && meat >= 0 && gold >= 0, "RtHg1 bag has the SQL items");
    let own_null = |got: &[Vec<u8>], id: u16| got.iter().any(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTNULLACTION && u16at(b, 6) % 30000 == id);
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_EQUIPITEM, 0, [dagger, 0, 0], b"", 0), "equip dagger");
    let ev = got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTNULLACTION && u16at(b, 6) % 30000 == p.id);
    // appearance 2 (weapon in bits 4-11) at 27 in the 43-byte player NULLACTION
    k.check(ev.is_some_and(|b| b.len() == 43 && (u16at(b, 27) & 0x0FF0) != 0), &format!("equip dagger: own OBJECTNULLACTION 43 with a weapon appearance {:?}", ev.map(|b| hex(b))));
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_EQUIPITEM, 0, [shield, 0, 0], b"", 0), "equip shield");
    k.check(own_null(&got, p.id), "equip shield: own OBJECTNULLACTION");
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_RELEASEITEM, 0, [dagger, 0, 0], b"", 0), "release dagger");
    let ev = got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTNULLACTION && u16at(b, 6) % 30000 == p.id);
    k.check(ev.is_some_and(|b| (u16at(b, 27) & 0x0FF0) == 0), &format!("release dagger: weapon appearance cleared {:?}", ev.map(|b| hex(b))));
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_REQ_USEITEM, 0, [potion, 0, 0], b"", 0), "drink red potion");
    let hp = n_of(&got, NOTIFY_HP).map(|b| (u32at(b, 6), b.len()));
    let sc = n_of(&got, NOTIFY_ITEMDEPLETED_ERASEITEM).map(|b| b.len());
    k.check(hp.is_some_and(|(h, l)| h > 10 && l == 14) && sc == Some(10), &format!("red potion: ITEMDEPLETED_ERASEITEM 10 then NOTIFY_HP 14 > 10 {hp:?}"));
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_REQ_USEITEM, 0, [meat, 0, 0], b"", 0), "eat meat");
    k.check(n_of(&got, NOTIFY_ITEMDEPLETED_ERASEITEM).is_some_and(|b| u16at(b, 6) as i32 == meat), "meat eaten (ITEMDEPLETED_ERASEITEM)");
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_REQ_PURCHASEITEM, 0, [2, 0, 0], b"RedPotion", 0), "buy 2 red potions");
    let bought: Vec<&Vec<u8>> = got.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_ITEMPURCHASED).collect();
    let price = bought.first().map(|b| u16at(b, 46));
    let gold_left = got.iter().rev().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_SETITEMCOUNT && u16at(b, 6) as i32 == gold).map(|b| u32at(b, 8));
    k.check(bought.len() == 2 && bought.iter().all(|b| b.len() == 48 && cstr(&b[7..27]) == b"RedPotion"), &format!("ITEMPURCHASED x2 (48 bytes), price {price:?}, gold left {gold_left:?}"));
    k.check(price.is_some_and(|pr| gold_left == Some(5000 - 2 * pr as u32)), "gold count = 5000 - 2 x price");
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_REQ_SELLITEM, 0, [dagger, 15, 1], b"Dagger", 0), "sell dagger offer");
    let offer = n_of(&got, NOTIFY_SELLITEMPRICE).map(|b| (b.len(), u32at(b, 6), u32at(b, 10), u32at(b, 14), u32at(b, 18)));
    k.check(offer.is_some_and(|o| o.0 == 42 && o.1 as i32 == dagger && o.3 >= 1), &format!("SELLITEMPRICE 42 (slot, life, price, count) {offer:?}"));
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_REQ_SELLITEMCONFIRM, 0, [dagger, 1, 0], b"Dagger", 0), "sell dagger confirm");
    let sold = n_of(&got, NOTIFY_ITEMSOLD).map(|b| b.len());
    let obt = n_of(&got, NOTIFY_ITEMOBTAINED).map(|b| (b.len(), u32at(b, 27)));
    k.check(sold == Some(14) && obt.is_some_and(|o| o.0 == 53), &format!("ITEMSOLD 14 + ITEMOBTAINED 53 (gold count {obt:?})"));
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_REQ_REPAIRITEM, 0, [shield, 15, 0], b"WoodShield", 0), "repair shield offer (shop)");
    let rep = got.iter().find(|b| u32at(b, 0) == NOTIFY && matches!(ty(b), NOTIFY_REPAIRITEMPRICE | NOTIFY_CANNOTREPAIRITEM)).map(|b| (ty(b), b.len(), u32at(b, 6)));
    k.check(rep.is_some_and(|r| (r.0 == NOTIFY_REPAIRITEMPRICE && r.1 == 42) || (r.0 == NOTIFY_CANNOTREPAIRITEM && r.1 == 30)), &format!("repair answer (type, size, slot) {rep:?}"));
    drop(p);

    // --- warehouse: deposit, logout, login (bank list), withdraw, logout
    let id2 = place("RtHg2", "wrhus_1", 69, 43, "");
    put_items(&id2, &[("Gold", 100, 1), ("Dagger", 1, 300)]);
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    let (bag, _) = item_list(&q.items);
    let dg = slot(&bag, "Dagger");
    let howard = npc_at(&mut q, 20); // Howard, the warehouse keeper
    println!("     RtHg2 bag {bag:?}, Howard {howard:?}, npcs {:?}", q.npcs);
    k.check(howard.is_some() && dg >= 0, "Howard in view and a dagger to bank");
    if let Some((hid, hx, hy)) = howard {
        let got = step(&mut q, |x| cmd(x, COMMONTYPE_GIVEITEMTOCHAR, dg as u8, [1, hx as i32, hy as i32], b"Dagger", hid as i32), "give dagger to Howard");
        let tb = n_of(&got, NOTIFY_ITEMTOBANK).map(|b| (b.len(), b[6], String::from_utf8_lossy(cstr(&b[8..28])).to_string()));
        let fin = n_of(&got, NOTIFY_GIVEITEMFIN_ERASEITEM).map(|b| b.len());
        k.check(tb.as_ref().is_some_and(|t| t.0 == 56 && t.2 == "Dagger") && fin == Some(32), &format!("ITEMTOBANK 56 (slot, name) {tb:?} + GIVEITEMFIN_ERASEITEM 32 {fin:?}"));
    }
    drop(q);
    let mut row = String::new();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_secs(1));
        row = sql(&format!("SELECT ItemName FROM bank_item WHERE CharID={id2};"));
        if !row.is_empty() {
            break;
        }
    }
    k.check(row == "Dagger", &format!("logout saved the bank row {row:?}"));
    if let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) {
    let (bag, bank) = item_list(&q.items);
    k.check(bank == ["Dagger"] && slot(&bag, "Dagger") < 0, &format!("login again: bank list {bank:?}, bag {bag:?}"));
    let mut rb = body(MSGID_REQUEST_RETRIEVEITEM, 0, &[], 6);
    rb.push(0);
    rb.resize(8, 0);
    let got = step(&mut q, |_| rb.clone(), "retrieve bank item 0");
    let rr = got.iter().find(|b| u32at(b, 0) == MSGID_RESPONSE_RETRIEVEITEM).map(|b| (b.len(), ty(b), b[6], b[7]));
    k.check(rr.is_some_and(|r| r.0 == 8 && r.1 == MSGTYPE_CONFIRM && r.2 == 0), &format!("RESPONSE_RETRIEVEITEM 8 confirm (bank 0, bag slot) {rr:?}"));
    drop(q);
    let mut rows = (String::new(), String::new());
    for _ in 0..20 {
        std::thread::sleep(Duration::from_secs(1));
        rows = (sql(&format!("SELECT ItemName FROM bank_item WHERE CharID={id2};")), sql(&format!("SELECT COUNT(*) FROM item WHERE CharID={id2} AND ItemName='Dagger';")));
        if rows.0.is_empty() && rows.1 == "1" {
            break;
        }
    }
    k.check(rows.0.is_empty() && rows.1 == "1", &format!("logout after the withdrawal: bank {:?}, bag daggers {}", rows.0, rows.1));

    }
    // --- magic tower: learn Fire-Wall (int 45, 240 gold)
    let id3 = place("RtHg3", "wzdtwr_1", 0, 0, ", HP=300, MP=200");
    put_items(&id3, &[("Gold", 1000, 1)]);
    let Some(mut r) = enter(login, b"rthg3", b"RtHg3", 3, &log, &mut k) else { return k.0 + 1 };
    let got = step(&mut r, |x| cmd(x, COMMONTYPE_REQ_STUDYMAGIC, 0, [0, 0, 0], b"Fire-Wall", 0), "study Fire-Wall");
    let ok = n_of(&got, NOTIFY_MAGICSTUDYSUCCESS).map(|b| (b.len(), b[6], String::from_utf8_lossy(cstr(&b[7..37])).to_string()));
    let g = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_SETITEMCOUNT).map(|b| u32at(b, 8));
    k.check(ok.as_ref().is_some_and(|o| o.0 == 37 && o.1 == 40) && g == Some(760), &format!("MAGICSTUDYSUCCESS 37 (40, Fire-Wall) {ok:?}, gold {g:?}"));
    let got = step(&mut r, |x| cmd(x, COMMONTYPE_REQ_STUDYMAGIC, 0, [0, 0, 0], b"Energy-Strike", 0), "study a spell above int 77 or too dear");
    println!("     second study: {:?}", got.iter().map(|b| hex(b)).collect::<Vec<_>>());
    drop(r);
    // (the tower is a no-attack area, where attack spells fizzle: the wall is cast by RtHg4 on huntzone1)
    let walk = super::fight::amd(mapdir, "huntzone1");
    let (fx, fy) = (40..=70).flat_map(|x| (110..=130).map(move |y| (x, y))).find(|&(x, y)| walk(x, y) && walk(x + 1, y) && walk(x + 1, y + 1) && walk(x + 1, y - 1)).expect("free tiles on huntzone1");
    let mut mm = "0".repeat(100);
    mm.replace_range(40..41, "1");
    ensure_char(login, b"rthg4", b"RtHg4");
    place("RtHg4", "huntzone1", fx, fy, &format!(", HP=300, MP=200, MagicMastery='{mm}'"));
    let Some(mut r) = enter(login, b"rthg4", b"RtHg4", 4, &log, &mut k) else { return k.0 + 1 };
    // fire wall next to RtHg4: MSGID_DYNAMICOBJECT confirm, fire ticks (NOTIFY_HP), expiry
    r.c.send(&keepalive());
    r.c.send(&super::fight::motion_body(&r, OBJECTMAGIC as u16, 5, &40u16.to_le_bytes()));
    let pre = r.c.drain(Duration::from_millis(400));
    let (rx, ry) = (r.x as i32 + 1, r.y as i32); // next to it: the 3x3 fire damage reaches it (a cast on its own tile has no direction: uninitialised offsets in the C++)
    let got = step(&mut r, |x| cmd(x, COMMONTYPE_MAGIC, 1, [rx, ry, 140], b"", 0), "cast Fire-Wall next to itself");
    let dy: Vec<&Vec<u8>> = got.iter().filter(|b| u32at(b, 0) == MSGID_DYNAMICOBJECT && ty(b) == MSGTYPE_CONFIRM).collect();
    k.check(pre.iter().any(|b| u32at(b, 0) == RESPONSE_MOTION) && !dy.is_empty() && dy.iter().all(|b| b.len() == 18 && u16at(b, 10) == 1), &format!("Fire-Wall: DYNAMICOBJECT confirm 18 bytes (x, y, type 1, id) x{} {:?}", dy.len(), dy.first().map(|b| hex(b))));
    let start = Instant::now();
    let (mut hps, mut gone) = (Vec::new(), None);
    while start.elapsed() < Duration::from_secs(45) && gone.is_none() {
        r.c.send(&keepalive());
        for b in r.c.drain(Duration::from_secs(1)) {
            if u32at(&b, 0) == NOTIFY && ty(&b) == NOTIFY_HP {
                hps.push(u32at(&b, 6));
            }
            if u32at(&b, 0) == MSGID_DYNAMICOBJECT && ty(&b) == MSGTYPE_REJECT && u16at(&b, 6) as i32 == rx && u16at(&b, 8) as i32 == ry {
                gone = Some(start.elapsed().as_secs());
            }
        }
    }
    k.check(hps.windows(2).filter(|w| w[1] < w[0]).count() >= 3, &format!("next to the fire: NOTIFY_HP falling at each 3 s tick (regeneration in between) {hps:?}"));
    k.check(gone.is_some_and(|s| (25..=40).contains(&s)), &format!("the fire on its tile expired (MSGTYPE_REJECT) after {gone:?} s (30 s + 1d4)"));
    drop(r);
    k.0
}

/// `hgprobe relogin`: RtHg1 logs in on gshop_1, logs out after 3 s, and logs in again: how many seconds the Login
/// keeps the account busy (the C++ Login frees it ~50 s after the enter once the HG sent GSM_DISCONNECT).
pub fn relogin(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    let id1 = place("RtHg1", "gshop_1", 51, 41, "");
    put_items(&id1, &[("Gold", 100, 1)]);
    let Some(p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    std::thread::sleep(Duration::from_secs(3));
    drop(p);
    let t = Instant::now();
    let mut gs = None;
    while gs.is_none() && t.elapsed() < Duration::from_secs(150) {
        std::thread::sleep(Duration::from_secs(2));
        gs = enter_game(login, b"rthg1", b"RtHg1", ENTER_NEW);
    }
    let secs = t.elapsed().as_secs();
    k.check(gs.is_some() && secs < 90, &format!("re-login accepted {secs} s after the logout"));
    if let Some(gs) = gs {
        let p = init_player(&gs, b"rthg1", b"RtHg1", 1, &log, &mut k, None);
        k.check(p.is_some(), "second session enters the game");
    }
    k.0
}

/// `hgprobe trade`: RtHg1 (Dagger, 1000 gold) and RtHg2 (500 gold, WoodShield, RedPotion) side by side on gshop_1.
/// RtHg1 opens the window on RtHg2 with the dagger, adds 300 gold; RtHg2 adds 200 gold and the shield; both
/// confirm -> EXCHANGEITEMCOMPLETE, the items changed hands (bags, then the saved rows). Then RtHg2 opens a
/// second window and RtHg1 cancels it (CANCELEXCHANGEITEM to both).
pub fn trade(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2")] {
        ensure_char(login, a, c);
    }
    let id1 = place("RtHg1", "gshop_1", 51, 41, "");
    put_items(&id1, &[("Gold", 1000, 1), ("Dagger", 1, 300)]);
    let id2 = place("RtHg2", "gshop_1", 52, 41, "");
    put_items(&id2, &[("Gold", 500, 1), ("WoodShield", 1, 100), ("RedPotion", 1, 1)]);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    let _ = p.c.drain(Duration::from_millis(500));
    let (bag1, _) = item_list(&p.items);
    let (bag2, _) = item_list(&q.items);
    let (dagger, gold, gold2, shield) = (slot(&bag1, "Dagger"), slot(&bag1, "Gold"), slot(&bag2, "Gold"), slot(&bag2, "WoodShield"));
    k.check(dagger >= 0 && gold >= 0 && gold2 >= 0 && shield >= 0, &format!("bags {bag1:?} {bag2:?}"));
    // one window message: (size, v1 slot, amount, name, owner name)
    let win = |got: &[Vec<u8>], t: i32| -> Vec<(usize, u16, u32, String, String)> {
        got.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == t).map(|b| (b.len(), u16at(b, 6), u32at(b, 12), String::from_utf8_lossy(cstr(&b[23..43])).to_string(), String::from_utf8_lossy(cstr(&b[43..53])).to_string())).collect()
    };
    let both = |p: &mut Player, q: &mut Player, b: Vec<u8>, what: &str| -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
        p.c.send(&keepalive());
        p.c.send(&b);
        let a = p.c.drain(Duration::from_millis(700));
        let c = q.c.drain(Duration::from_millis(300));
        println!("     {what}: {:?} / {:?}", a.iter().map(|b| short(b)).collect::<Vec<_>>(), c.iter().map(|b| short(b)).collect::<Vec<_>>());
        (a, c)
    };
    let (qx, qy, qid) = (q.x, q.y, q.id);
    let (a, b) = { let m = cmd(&p, COMMONTYPE_EXCHANGEITEMTOCHAR, dagger as u8, [1, qx as i32, qy as i32], b"Dagger", qid as i32); both(&mut p, &mut q, m, "RtHg1 opens with the dagger") };
    let (wa, wb) = (win(&a, NOTIFY_OPENEXCHANGEWINDOW), win(&b, NOTIFY_OPENEXCHANGEWINDOW));
    k.check(wa.len() == 1 && wa[0].0 == 57 && wa[0].1 == dagger as u16 + 1000 && wa[0].3 == "Dagger" && wa[0].4 == "RtHg1", &format!("OPENEXCHANGEWINDOW 57 to RtHg1 (slot + 1000) {wa:?}"));
    k.check(wb.len() == 1 && wb[0].0 == 57 && wb[0].1 == dagger as u16 && wb[0].2 == 1 && wb[0].4 == "RtHg1", &format!("OPENEXCHANGEWINDOW 57 to RtHg2 {wb:?}"));
    let (a, b) = { let m = cmd(&p, COMMONTYPE_SETEXCHANGEITEM, 0, [gold, 300, 0], b"", 0); both(&mut p, &mut q, m, "RtHg1 adds 300 gold") };
    let (wa, wb) = (win(&a, NOTIFY_SETEXCHANGEITEM), win(&b, NOTIFY_SETEXCHANGEITEM));
    k.check(wa.len() == 1 && wb.len() == 1 && wb[0].0 == 57 && wb[0].2 == 300 && wb[0].3 == "Gold", &format!("SETEXCHANGEITEM 57 gold {wa:?} {wb:?}"));
    let (b, _) = { let m = cmd(&q, COMMONTYPE_SETEXCHANGEITEM, 0, [gold2, 200, 0], b"", 0); both(&mut q, &mut p, m, "RtHg2 adds 200 gold") };
    k.check(win(&b, NOTIFY_SETEXCHANGEITEM).len() == 1, "SETEXCHANGEITEM gold of RtHg2");
    let _ = { let m = cmd(&q, COMMONTYPE_SETEXCHANGEITEM, 0, [shield, 1, 0], b"", 0); both(&mut q, &mut p, m, "RtHg2 adds the shield") };
    let (a, b) = { let m = cmd(&p, COMMONTYPE_CONFIRMEXCHANGEITEM, 0, [0, 0, 0], b"", 0); both(&mut p, &mut q, m, "RtHg1 confirms") };
    k.check(n_of(&a, NOTIFY_EXCHANGEITEMCOMPLETE).is_none() && n_of(&b, NOTIFY_EXCHANGEITEMCOMPLETE).is_none(), "one confirm: no trade yet");
    let (b, a) = { let m = cmd(&q, COMMONTYPE_CONFIRMEXCHANGEITEM, 0, [0, 0, 0], b"", 0); both(&mut q, &mut p, m, "RtHg2 confirms") };
    let ca = n_of(&a, NOTIFY_EXCHANGEITEMCOMPLETE).map(|b| b.len());
    let cb = n_of(&b, NOTIFY_EXCHANGEITEMCOMPLETE).map(|b| b.len());
    k.check(ca == Some(6) && cb == Some(6), &format!("EXCHANGEITEMCOMPLETE 6 to both {ca:?} {cb:?}"));
    let got_a: Vec<String> = a.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_ITEMOBTAINED).map(|b| format!("{}:{}", b.len(), String::from_utf8_lossy(cstr(&b[7..27])))).collect();
    let got_b: Vec<String> = b.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_ITEMOBTAINED).map(|b| format!("{}:{}", b.len(), String::from_utf8_lossy(cstr(&b[7..27])))).collect();
    k.check(got_a.len() == 2 && got_b.len() == 2, &format!("ITEMOBTAINED RtHg1 {got_a:?}, RtHg2 {got_b:?}"));
    let erase_a = n_of(&a, NOTIFY_GIVEITEMFIN_ERASEITEM).map(|b| (b.len(), u16at(b, 6)));
    let cnt_a = a.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_SETITEMCOUNT).map(|b| (b.len(), u16at(b, 6), u32at(b, 8))).collect::<Vec<_>>();
    k.check(erase_a.is_some_and(|e| e.0 == 32 && e.1 == dagger as u16) && cnt_a.iter().any(|c| c.1 == gold as u16 && c.2 == 700), &format!("RtHg1 loses the dagger (GIVEITEMFIN_ERASEITEM {erase_a:?}) and 300 gold (SETITEMCOUNT {cnt_a:?})"));
    println!("     RtHg1 got {:?}", a.iter().map(|b| short(b)).collect::<Vec<_>>());

    // second window, cancelled by the other side
    let (bag2b, _) = (slot(&bag2, "RedPotion"), 0);
    let (px, py, pid) = (p.x, p.y, p.id);
    let _ = { let m = cmd(&q, COMMONTYPE_EXCHANGEITEMTOCHAR, bag2b as u8, [1, px as i32, py as i32], b"RedPotion", pid as i32); both(&mut q, &mut p, m, "RtHg2 opens with a potion") };
    let (a, b) = { let m = cmd(&p, COMMONTYPE_CANCELEXCHANGEITEM, 0, [0, 0, 0], b"", 0); both(&mut p, &mut q, m, "RtHg1 cancels") };
    let (xa, xb) = (n_of(&a, NOTIFY_CANCELEXCHANGEITEM).map(|b| b.len()), n_of(&b, NOTIFY_CANCELEXCHANGEITEM).map(|b| b.len()));
    k.check(xa == Some(6) && xb == Some(6), &format!("CANCELEXCHANGEITEM 6 to both {xa:?} {xb:?}"));
    drop(p);
    drop(q);
    let row = |id: &str| sql(&format!("SELECT GROUP_CONCAT(CONCAT(ItemName, ':', Count) ORDER BY ItemName) FROM item WHERE CharID={id};"));
    let mut rows = (String::new(), String::new());
    for _ in 0..20 {
        std::thread::sleep(Duration::from_secs(1));
        rows = (row(&id1), row(&id2));
        if rows.0.contains("WoodShield") && rows.1.contains("Dagger") {
            break;
        }
    }
    k.check(rows.0 == "Gold:900,WoodShield:1" && rows.1 == "Dagger:1,Gold:600,RedPotion:1", &format!("saved bags RtHg1 {:?}, RtHg2 {:?}", rows.0, rows.1));
    k.0
}

/// MSGID_COMMAND_COMMON for the crafting commands: x, y, dir, then (crafting / build) a 20-byte name, then 6 slots.
fn craft_msg(p: &Player, t: i32, name: Option<&[u8]>, slots: [i8; 6]) -> Vec<u8> {
    let mut b = body(MSGID_COMMAND_COMMON, t as u16, &[], 6);
    for x in [p.x, p.y] {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b.push(0);
    if let Some(n) = name {
        let mut n = n.to_vec();
        n.resize(20, 0);
        b.extend_from_slice(&n);
    }
    b.extend(slots.iter().map(|&s| s as u8));
    b.resize(60, 0);
    b
}

/// `hgprobe craft`: RtHg1 (gshop_1, alchemy and manufacturing 100) makes a RedPotion (SlimeJelly + AntLeg +
/// SnakeTongue), fails to match a potion with one jelly, crafts RubyWare (Crystal + 2 Ruby, 70 %) and builds a
/// SuperCoal (3 Coal); the saved bag is checked.
pub fn craft(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    let id1 = place("RtHg1", "gshop_1", 51, 41, "");
    put_items(&id1, &[("SlimeJelly", 2, 1), ("AntLeg", 1, 1), ("SnakeTongue", 1, 1), ("Coal", 3, 1), ("Crystal", 1, 1), ("Ruby", 2, 1)]);
    for s in [7, 4] {
        sql(&format!("DELETE FROM skill WHERE CharID={id1} AND SkillID={s}; INSERT INTO skill (CharID, SkillID, SkillMastery, SkillSSN) VALUES ({id1}, {s}, 100, 0);"));
    }
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let (bag, _) = item_list(&p.items);
    let s = |n: &str| slot(&bag, n) as i8;
    let (jelly, leg, tongue, coal, crystal, ruby) = (s("SlimeJelly"), s("AntLeg"), s("SnakeTongue"), s("Coal"), s("Crystal"), s("Ruby"));
    k.check([jelly, leg, tongue, coal, crystal, ruby].iter().all(|&x| x >= 0), &format!("bag {bag:?}"));
    let names = |got: &[Vec<u8>]| -> Vec<String> { got.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_ITEMOBTAINED).map(|b| String::from_utf8_lossy(cstr(&b[7..27])).to_string()).collect() };
    let n6 = |got: &[Vec<u8>], t: i32| n_of(got, t).map(|b| b.len());
    // CheckClientResponseTime (every 3 s) disconnects a player that sent 2 or more skill messages since the last check
    let pause = || std::thread::sleep(Duration::from_millis(3200));

    let got = step(&mut p, |x| craft_msg(x, COMMONTYPE_REQ_CREATEPOTION, None, [jelly, leg, tongue, -1, -1, -1]), "potion: jelly + leg + tongue");
    let ok = n6(&got, NOTIFY_POTIONSUCCESS);
    let fail = n6(&got, NOTIFY_POTIONFAIL);
    k.check((ok == Some(6) && names(&got) == ["RedPotion"]) || fail == Some(6), &format!("POTIONSUCCESS 6 + ITEMOBTAINED RedPotion {ok:?} {:?}, or POTIONFAIL {fail:?}", names(&got)));
    pause();
    let got = step(&mut p, |x| craft_msg(x, COMMONTYPE_REQ_CREATEPOTION, None, [jelly, -1, -1, -1, -1, -1]), "potion: one jelly");
    let nm = n6(&got, NOTIFY_NOMATCHINGPOTION);
    // after a success the jelly slot holds 1 jelly; after a failure 2: one jelly matches no potion either way
    k.check(nm == Some(6), &format!("NOMATCHINGPOTION 6 {nm:?}"));
    pause();
    let got = step(&mut p, |x| craft_msg(x, COMMONTYPE_CRAFTITEM, Some(b"RubyWare"), [crystal, ruby, ruby, -1, -1, -1]), "craft RubyWare");
    let (ok, fail) = (n6(&got, NOTIFY_CRAFTING_SUCCESS), n6(&got, NOTIFY_CRAFTING_FAIL));
    let crafted = names(&got);
    let ware = got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_ITEMOBTAINED).map(|b| (b.len(), hex(&b[44..53])));
    k.check((ok == Some(6) && crafted == ["RubyWare"]) || fail == Some(6), &format!("CRAFTING_SUCCESS 6 + RubyWare {ok:?} {crafted:?} (tail {ware:?}), or CRAFTING_FAIL {fail:?}"));
    pause();
    let got = step(&mut p, |x| craft_msg(x, COMMONTYPE_BUILDITEM, Some(b"SuperCoal"), [coal, coal, coal, -1, -1, -1]), "build SuperCoal");
    let bs = n_of(&got, NOTIFY_BUILDITEMSUCCESS).map(|b| (b.len(), u16at(b, 6) as i16, u16at(b, 8)));
    k.check(bs.is_some_and(|b| b.0 == 10 && b.2 == 12) && names(&got) == ["SuperCoal"], &format!("BUILDITEMSUCCESS 10 (purity, type 12) {bs:?} + ITEMOBTAINED {:?}", names(&got)));
    let coal_gone = got.iter().any(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_ITEMDEPLETED_ERASEITEM && u16at(b, 6) == coal as u16);
    k.check(coal_gone, "the 3 coal are used up (ITEMDEPLETED_ERASEITEM on the coal slot)");
    drop(p);
    let mut row = String::new();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_secs(1));
        row = sql(&format!("SELECT GROUP_CONCAT(CONCAT(ItemName, ':', Count) ORDER BY ItemName) FROM item WHERE CharID={id1};"));
        if row.contains("SuperCoal") {
            break;
        }
    }
    k.check(row.contains("SuperCoal:1") && !row.contains(",Coal:") && !row.starts_with("Coal:"), &format!("saved bag {row:?}"));
    k.0
}

/// `hgprobe angel`: RtHg1 (gshop_1) wears an AngelicPendant(STR) (attribute 0x30000000: +4 str): equip -> own
/// OBJECTNULLACTION with STATUS_ANGELSTR (0x1000) at 37; release -> SETTING_SUCCESS 34 and the bit cleared.
pub fn angel(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    let id1 = place("RtHg1", "gshop_1", 51, 41, "");
    put_items(&id1, &[("Gold", 10, 1)]);
    sql(&format!("INSERT INTO item (CharID, ItemName, ItemID, Count, ItemType, ID1, ID2, ID3, Color, Effect1, Effect2, Effect3, LifeSpan, Attribute, ItemEquip, ItemPosX, ItemPosY) VALUES ({id1}, 'AngelicPendant(STR)', 9900990, 1, 0, 0, 0, 0, 0, 0, 0, 0, 300, {}, 0, 40, 40);", 0x3000_0000u32));
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let (bag, _) = item_list(&p.items);
    let pend = slot(&bag, "AngelicPendant(STR)");
    k.check(pend >= 0, &format!("bag {bag:?}"));
    let st = |got: &[Vec<u8>], id: u16| -> Vec<u32> { got.iter().filter(|b| u32at(b, 0) == EVENT_MOTION && ty(b) == OBJECTNULLACTION && b.len() == 43 && u16at(b, 6) % 30000 == id).map(|b| u32at(b, 37)).collect() };
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_EQUIPITEM, 0, [pend, 0, 0], b"", 0), "equip the pendant");
    let s = st(&got, p.id);
    k.check(s.iter().any(|v| v & 0x1000 != 0), &format!("equip: own NULLACTION with STATUS_ANGELSTR {:x?}", s));
    let got = step(&mut p, |x| cmd(x, COMMONTYPE_RELEASEITEM, 0, [pend, 0, 0], b"", 0), "release the pendant");
    let s = st(&got, p.id);
    let ok = n_of(&got, NOTIFY_SETTING_SUCCESS).map(|b| b.len());
    k.check(ok == Some(34) && s.last().is_some_and(|v| v & 0xF000 == 0), &format!("release: SETTING_SUCCESS 34 {ok:?}, angel bits cleared {:x?}", s));
    k.0
}
