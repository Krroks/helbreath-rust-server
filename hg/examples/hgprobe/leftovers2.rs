//! The batch D leftovers: `hgprobe leftovers2 LOGIN OUT.jsonl` (the All server; Rust login). Messages that were accepted and ignored:
//! the legion point services (MSGID_REQUEST_LGNPTS / LGNSVC through the Login), the respec with majestic points
//! (MSGID_STATECHANGEPOINT), Gail's Dark Knight / Dark Mage items (MSGID_REQUEST_DKITEM) and the observer's panning.
//!
//! S1 RtHg1 in aresden (account cash 200): the cash is asked, a service outside the city hall gives nothing.
//! S2 RtHg1 in cityhall_1: 10 000 gold, 2 majestic points (and a respec refused: its stats are not the cap's), 10 reputation,
//!    a hat (MP 7 attribute), a town change (the same town: nothing, not charged), no cash: nothing; the cash, the items, the
//!    nation, the reputation and the cash_transactions rows are checked after the logout.
//! S3 RtHg1 as a woman with 49 of 50 bag slots used: DKFullHelm(W) fits, then a hat and another DK item are refused (bag full), nothing charged.
//! S4 RtHg2 (level 180, stats 607, 2 majestic points, Fire-Wall known with 45 int needed): a respec of int / str / chr, a refused
//!    one (a byte that is no stat), a second, a third with no point left, a level-up setting; DK items 1 (helm), 13, 10.
//! S5 RtHg3 (GM) on aresden: panning before /obs gives nothing; as an observer a pan with no direction gives nothing and three pans answer RESPONSE_PANNING
use super::fight::keepalive;
use super::items::{enter, item_list, place, put_items, slot};
use super::social::{chat_body, notif};
use super::*;

/// keepalive + the message, then what comes within `ms`
fn ask(p: &mut Player, b: Vec<u8>, ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

fn lgnpts(p: &mut Player) -> Option<u32> {
    let got = ask(p, body(MSGID_REQUEST_LGNPTS, 0, &[], 6), 900);
    notif(&got, NOTIFY_LGNPTS).filter(|b| b.len() == 10).map(|b| u32at(b, 6))
}

fn lgnsvc(p: &mut Player, cmd: u16) -> Vec<Vec<u8>> {
    ask(p, body(MSGID_REQUEST_LGNSVC, cmd, &[], 6), 1200)
}

fn dkitem(p: &mut Player, n: i32) -> Vec<Vec<u8>> {
    ask(p, body(MSGID_REQUEST_DKITEM, 0, &[(b"Gail", 20), (&n.to_le_bytes(), 4)], 30), 800)
}

fn respec(p: &mut Player, stats: [i32; 3]) -> Vec<Vec<u8>> {
    let s: Vec<u8> = stats.iter().map(|&x| x as u8).collect();
    ask(p, body(MSGID_STATECHANGEPOINT, 0, &[(&s, 3)], 9), 900)
}

fn pan(p: &mut Player, dir: u8) -> Option<Vec<u8>> {
    let got = ask(p, body(MSGID_REQUEST_PANNING, 0, &[(&[dir], 1)], 7), 700);
    got.into_iter().find(|b| u32at(b, 0) == MSGID_RESPONSE_PANNING)
}

/// (name, count, attribute) of an ITEMOBTAINED
fn obtained(got: &[Vec<u8>]) -> Option<(String, u32, u32)> {
    notif(got, NOTIFY_ITEMOBTAINED).filter(|b| b.len() == 53).map(|b| (String::from_utf8_lossy(cstr(&b[7..27])).to_string(), u32at(b, 27), u32at(b, 47)))
}

fn side_of(got: &[Vec<u8>], id: u16) -> Option<u32> {
    got.iter().find(|b| u32at(b, 0) == EVENT_MOTION && u16at(b, 4) as i32 == OBJECTNULLACTION && u16at(b, 6) % 30000 == id).map(|b| u32at(b, 37) >> 28)
}

/// SETTING_SUCCESS: (level, str, vit, dex, int, mag, chr)
fn stats_of(got: &[Vec<u8>]) -> Option<[u32; 7]> {
    notif(got, NOTIFY_SETTING_SUCCESS).filter(|b| b.len() == 34).map(|b| std::array::from_fn(|i| u32at(b, 6 + i * 4)))
}

fn gizon_of(got: &[Vec<u8>]) -> Option<u16> {
    notif(got, NOTIFY_GIZONITEMUPGRADELEFT).filter(|b| b.len() == 20).map(|b| u16at(b, 6))
}

pub fn leftovers2(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2"), (b"rthg3", b"RtHg3")] {
        ensure_char(login, a, c);
    }
    sql("UPDATE account_database SET IsGMAccount=1 WHERE name='rthg3';");
    sql("UPDATE account_database SET cash=200 WHERE name='rthg1';");
    sql("DELETE FROM cash_transactions WHERE account_name LIKE 'rt%';");
    let strong = ", Strenght=300, Level=60, HP=300, Popularity=7, GizonItemUpgradeLeft=0, Gender=1";

    // ---- S1: outside the city hall
    let id1 = place("RtHg1", "aresden", 140, 132, strong);
    put_items(&id1, &[("Gold", 5, 1)]);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let cash = lgnpts(&mut p);
    k.check(cash == Some(200), &format!("S1 LGNPTS -> NOTIFY_LGNPTS 10 bytes, cash 200 {cash:?}"));
    let got = lgnsvc(&mut p, 8);
    k.check(obtained(&got).is_none() && gizon_of(&got).is_none(), "S1 service outside the city hall: nothing given");
    let cash = lgnpts(&mut p);
    k.check(cash == Some(200), &format!("S1 ... and nothing charged {cash:?}"));
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let c = sql("SELECT cash FROM account_database WHERE name='rthg1';");
    k.check(c == "200", &format!("S1 cash still 200 in the account {c:?}"));

    // ---- S2: the city hall
    let id1 = place("RtHg1", "cityhall_1", -1, -1, strong);
    put_items(&id1, &[("Gold", 5, 1)]);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"cityhall_1", &format!("S2 in {}", String::from_utf8_lossy(&p.map)));
    let cash = lgnpts(&mut p);
    k.check(cash == Some(200), &format!("S2 cash {cash:?}"));
    let got = lgnsvc(&mut p, 8);
    let ob = obtained(&got);
    k.check(ob.as_ref().is_some_and(|o| o.0 == "Gold" && o.1 == 10000), &format!("S2 10k Gold: ITEMOBTAINED Gold x10000 {ob:?}"));
    let cash = lgnpts(&mut p);
    k.check(cash == Some(199), &format!("S2 cash 199 {cash:?}"));
    let got = lgnsvc(&mut p, 6);
    let gz = gizon_of(&got);
    k.check(gz == Some(2), &format!("S2 2 Maj: GIZONITEMUPGRADELEFT 2 {gz:?}"));
    // a respec of a character whose stats are not the cap's 607: refused (the Rust HG says so with STATECHANGE_FAILED)
    let got = respec(&mut p, [MJSTAT_STR, MJSTAT_STR, MJSTAT_STR]);
    k.check(stats_of(&got).is_none() && gizon_of(&got).is_none(), "S2 respec with stats that do not add up to 607: refused");
    println!("     note: STATECHANGE_FAILED answered: {}", notif(&got, NOTIFY_STATECHANGE_FAILED).is_some());
    let got = lgnsvc(&mut p, 10);
    println!("     note: NOTIFY_EXP after the reputation: {}", notif(&got, NOTIFY_EXP).is_some());
    let got = lgnsvc(&mut p, 0);
    let ob = obtained(&got);
    k.check(ob.as_ref().is_some_and(|o| o.0 == "LegionHat(M)" && o.1 == 1 && o.2 == 0x6100), &format!("S2 LegionHat MP7: ITEMOBTAINED LegionHat(M) attribute 0x6100 {ob:?}"));
    let cash = lgnpts(&mut p);
    k.check(cash == Some(172), &format!("S2 cash 172 (1 + 1 + 1 + 25 charged) {cash:?}"));
    let got = lgnsvc(&mut p, 4);
    k.check(got.iter().all(|b| u16at(b, 4) as i32 != NOTIFY_RESPONSE_HUNTMODE) && side_of(&got, p.id).is_none(), "S2 town change to Aresden while in Aresden: nothing");
    let got = lgnsvc(&mut p, 5);
    let hm = notif(&got, NOTIFY_RESPONSE_HUNTMODE);
    k.check(hm.is_some_and(|b| b.len() == 16 && cstr(&b[6..16]) == b"elvine"), &format!("S2 town change to Elvine: HUNTMODE 16 elvine {:?}", hm.map(|b| hex(b))));
    let sd = side_of(&got, p.id);
    k.check(sd == Some(2), &format!("S2 ... and the NULLACTION shows side 2 {sd:?}"));
    let got = lgnsvc(&mut p, 5);
    k.check(notif(&got, NOTIFY_RESPONSE_HUNTMODE).is_none(), "S2 town change to Elvine again: nothing");
    let cash = lgnpts(&mut p);
    k.check(cash == Some(162), &format!("S2 cash 162 (only the first town change charged) {cash:?}"));
    sql("UPDATE account_database SET cash=0 WHERE name='rthg1';");
    let got = lgnsvc(&mut p, 8);
    k.check(obtained(&got).is_none(), "S2 no cash: no gold");
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let n = sql("SELECT COUNT(*) FROM cash_transactions WHERE account_name='rthg1';");
    k.check(n == "5", &format!("S2 5 cash_transactions rows (gold, 2 Maj, 10 Rep, hat, TC Elv) {n:?}"));
    let row = sql("SELECT Nation, Popularity, GizonItemUpgradeLeft FROM char_database WHERE char_name='RtHg1' AND account_name LIKE 'rt%';");
    k.check(row == "elvine\t17\t2", &format!("S2 saved nation elvine, reputation 7 + 10, 2 majestic points {row:?}"));
    let row = sql(&format!("SELECT ItemName, Count, Attribute FROM item WHERE CharID={id1} AND ItemName IN ('Gold','LegionHat(M)') ORDER BY ItemName;"));
    k.check(row == "Gold\t10005\t0\nLegionHat(M)\t1\t24832", &format!("S2 saved Gold 10005 and the hat with attribute 0x6100 {row:?}"));

    // ---- S3: a woman with a full bag
    sql("UPDATE account_database SET cash=100 WHERE name='rthg1';");
    let id1 = place("RtHg1", "cityhall_1", -1, -1, strong);
    sql("UPDATE char_database SET Gender=2 WHERE char_name='RtHg1' AND account_name LIKE 'rt%';");
    let rows: Vec<(&str, u32, u32)> = (0..49).map(|_| ("Dagger", 1, 300)).collect();
    put_items(&id1, &rows);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let (bag, _) = item_list(&p.items);
    k.check(bag.len() == 49, &format!("S3 49 bag items ({})", bag.len()));
    let got = dkitem(&mut p, 1);
    let ob = obtained(&got);
    k.check(ob.as_ref().is_some_and(|o| o.0 == "DKFullHelm(W)" && o.1 == 1), &format!("S3 DKITEM 1: DKFullHelm(W) {ob:?}"));
    let got = dkitem(&mut p, 2);
    k.check(obtained(&got).is_none() && notif(&got, NOTIFY_CANNOTCARRYMOREITEM).is_some_and(|b| b.len() == 6), "S3 DKITEM 2 with a full bag: CANNOTCARRYMOREITEM 6");
    let got = lgnsvc(&mut p, 0);
    k.check(obtained(&got).is_none() && notif(&got, NOTIFY_CANNOTCARRYMOREITEM).is_some_and(|b| b.len() == 6), "S3 hat with a full bag: CANNOTCARRYMOREITEM 6");
    let cash = lgnpts(&mut p);
    k.check(cash == Some(100), &format!("S3 nothing charged for the refused hat {cash:?}"));
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let n = sql(&format!("SELECT COUNT(*) FROM item WHERE CharID={id1} AND ItemName='DKFullHelm(W)';"));
    k.check(n == "1", &format!("S3 the helm is saved {n:?}"));

    // ---- S4: the respec and the Dark Knight items
    put_items(&id1, &[]); // (the ItemIDs of the 49 daggers would collide with the gold of RtHg2)
    let mut mm = "0".repeat(100);
    mm.replace_range(38..39, "1"); // Tremor, 33 int
    mm.replace_range(40..41, "1"); // Fire-Wall, 45 int
    let id2 = place("RtHg2", "aresden", 140, 132, &format!(", Level=180, Strenght=150, Vitality=150, Dexterity=100, Intelligence=45, Magic=100, Agility=62, HP=885, GizonItemUpgradeLeft=2, Gender=1, MagicMastery='{mm}'"));
    put_items(&id2, &[("Gold", 5, 1)]);
    let Some(mut p) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    let got = respec(&mut p, [MJSTAT_INT, MJSTAT_STR, MJSTAT_CHR]);
    let (gz, st) = (gizon_of(&got), stats_of(&got));
    k.check(gz == Some(1), &format!("S4 respec int / str / chr: GIZONITEMUPGRADELEFT 1 {gz:?}"));
    k.check(st == Some([180, 149, 150, 100, 44, 100, 61]), &format!("S4 ... SETTING_SUCCESS 180 / 149 150 100 44 100 61 {st:?}"));
    let order = got.iter().position(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_GIZONITEMUPGRADELEFT) < got.iter().position(|b| u32at(b, 0) == NOTIFY && u16at(b, 4) as i32 == NOTIFY_SETTING_SUCCESS);
    k.check(order, "S4 ... the majestic points come before the stats");
    let hp = notif(&got, NOTIFY_HP).map(|b| u32at(b, 6));
    k.check(hp == Some(884), &format!("S4 ... the lost strength clamps the HP (885 -> 884, NOTIFY_HP) {hp:?}"));
    // the three points are in the pool: a respec needs the stats back at 607 first
    let got = respec(&mut p, [MJSTAT_STR, MJSTAT_DEX, MJSTAT_VIT]);
    k.check(stats_of(&got).is_none() && gizon_of(&got).is_none(), "S4 stats at 604, not 607: refused");
    let lu = |p: &mut Player, v: [u8; 6]| ask(p, body(MSGID_LEVELUPSETTINGS, 0, &[(&v, 6)], 12), 900);
    let st = stats_of(&lu(&mut p, [0, 0, 0, 0, 0, 3]));
    k.check(st == Some([180, 149, 150, 100, 44, 100, 64]), &format!("S4 the 3 freed points go back (chr): SETTING_SUCCESS {st:?}"));
    let got = respec(&mut p, [MJSTAT_STR, 0, MJSTAT_DEX]);
    k.check(stats_of(&got).is_none() && gizon_of(&got).is_none(), "S4 a byte that is no stat: refused");
    let got = respec(&mut p, [MJSTAT_MAG, MJSTAT_MAG, MJSTAT_MAG]);
    let (gz, st) = (gizon_of(&got), stats_of(&got));
    k.check(gz == Some(0) && st == Some([180, 149, 150, 100, 44, 97, 64]), &format!("S4 respec 3 x mag: 0 points left, mag 97 {gz:?} {st:?}"));
    let st = stats_of(&lu(&mut p, [0, 0, 0, 0, 3, 0]));
    k.check(st == Some([180, 149, 150, 100, 44, 100, 64]), &format!("S4 back into mag {st:?}"));
    let got = respec(&mut p, [MJSTAT_VIT, MJSTAT_DEX, MJSTAT_STR]);
    k.check(stats_of(&got).is_none() && gizon_of(&got).is_none(), "S4 no majestic point left: refused");
    let got = dkitem(&mut p, 1);
    let ob = obtained(&got);
    k.check(ob.as_ref().is_some_and(|o| o.0 == "DKFullHelm(M)" && o.1 == 1), &format!("S4 DKITEM 1: DKFullHelm(M) {ob:?}"));
    let got = dkitem(&mut p, 13);
    let ob = obtained(&got);
    k.check(ob.as_ref().is_some_and(|o| o.0 == "DKFlameberg"), &format!("S4 DKITEM 13: DKFlameberg {ob:?}"));
    let got = dkitem(&mut p, 10);
    let ob = obtained(&got);
    k.check(ob.as_ref().is_some_and(|o| o.0 == "DMMagicStaff"), &format!("S4 DKITEM 10: DMMagicStaff {ob:?}"));
    let (bag, _) = item_list(&p.items);
    let _ = slot(&bag, "Gold");
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let row = sql("SELECT Strenght, Vitality, Dexterity, Intelligence, Magic, Agility, GizonItemUpgradeLeft, HP FROM char_database WHERE char_name='RtHg2' AND account_name LIKE 'rt%';");
    k.check(row == "149\t150\t100\t44\t100\t64\t0\t884", &format!("S4 saved stats, no points left, HP 884 {row:?}"));
    let row = sql("SELECT SUBSTRING(MagicMastery, 39, 1), SUBSTRING(MagicMastery, 41, 1) FROM char_database WHERE char_name='RtHg2' AND account_name LIKE 'rt%';");
    k.check(row == "1\t0", &format!("S4 Tremor (33 int) kept, Fire-Wall (45 int) forgotten {row:?}"));
    let rows = sql(&format!("SELECT ItemName FROM item WHERE CharID={id2} AND ItemName IN ('DKFullHelm(M)','DKFlameberg','DMMagicStaff') ORDER BY ItemName;"));
    k.check(rows == "DKFlameberg\nDKFullHelm(M)\nDMMagicStaff", &format!("S4 the three items are saved {rows:?}"));

    // ---- S5: the observer's panning (the edges are a unit test: the C++ reads outside the map there)
    place("RtHg3", "aresden", 140, 132, ", Level=100, HP=900, AdminLevel=4");
    let Some(mut p) = enter(login, b"rthg3", b"RtHg3", 3, &log, &mut k) else { return k.0 + 1 };
    k.check(pan(&mut p, 3).is_none(), "S5 panning before /obs: nothing");
    let b = chat_body(&p, b"RtHg3", b"/obs");
    let got = ask(&mut p, b, 800);
    k.check(notif(&got, NOTIFY_OBSERVERMODE).is_some(), "S5 /obs: OBSERVERMODE");
    for dir in [0u8, 9, 255] {
        k.check(pan(&mut p, dir).is_none(), &format!("S5 pan {dir}: no direction, nothing"));
    }
    let (mut x, mut y) = (140i32, 132i32);
    for dir in [3u8, 5, 6] {
        let r = pan(&mut p, dir);
        match dir {
            3 => x += 1,
            5 => y += 1,
            _ => (x, y) = (x - 1, y + 1),
        }
        println!("     pan {dir} -> {:?}", r.as_ref().map(|b| (b.len(), hex(&b[..b.len().min(14)]))));
        let ok = r.as_ref().is_some_and(|b| b.len() >= 13 && u16at(b, 4) as i32 == OBJECTMOVE_CONFIRM && u16at(b, 6) as i16 as i32 == x - 10 && u16at(b, 8) as i16 as i32 == y - 7 && b[10] == dir);
        k.check(ok, &format!("S5 pan {dir}: RESPONSE_PANNING (x - 10, y - 7) = ({}, {}), direction {dir}", x - 10, y - 7));
        // (the C++ sends 6 bytes more than the Rust HG: stack garbage after the tiles)
        println!("     {} bytes, {} tiles", r.as_ref().map_or(0, |b| b.len()), r.as_ref().map_or(0, |b| u16at(b, 11)));
    }
    k.0
}
