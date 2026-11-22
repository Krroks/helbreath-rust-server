//! The batch E leftovers: `hgprobe leftovers3 LOGIN OUT.jsonl` (the All server; Rust login). COMMONTYPE_REQ_GETREWARDMONEY
//! (the reward gold of kills as a Gold item + NOTIFY_REWARDGOLD) and COMMONTYPE_REQ_GETHEROMANTLE (the hero items for enemy
//! kills and contribution). The website POST is a unit test (hg/src/website.rs); the test server must have no website.
//!
//! S1 RtHg1 (level 1, str 10: load 5500, Gold x5 in the bag, reward gold 3000): the first request carries half the free
//!    load (2749 Gold), the second the rest (251), the stack is 3005 Gold in the end, reward gold 0 saved.
//! S2 RtHg2 (Gold x109999 weighs 5499 of the 5500 load, reward 1000): nothing given, nothing sent, the reward gold is kept.
//! S3 RtHg3 (aresden man, 350 EK, 25 contribution): the helm (150 EK / 20) is paid, bound to him and NOTIFY_ENEMYKILLS says 200;
//!    the cap (100 / 20), hauberk (100 / 10), cape (300), leggings, plate, an index out of the table: refused (5 contribution, 200 EK left).
//! S4 RtHg1 (elvine woman, 500 EK, 100 contribution): the plate mail for a woman of Elvine; then with 49 other items the cape
//!    takes the last slot and the helm answers CANNOTCARRYMOREITEM and charges nothing.
//! S5 RtHg2 (a traveller with 500 EK) and RtHg3 (99 EK) are given nothing.
use super::fight::keepalive;
use super::items::{enter, place, put_items};
use super::social::notif;
use super::*;

fn common(p: &mut Player, t: i32, v: i32) -> Vec<Vec<u8>> {
    let mut b = body(MSGID_COMMAND_COMMON, t as u16, &[], 6);
    for x in [p.x, p.y] {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b.push(1);
    b.extend_from_slice(&v.to_le_bytes());
    b.resize(57, 0);
    p.c.send(&keepalive());
    p.c.send(&b);
    p.c.drain(Duration::from_millis(800))
}

/// (name, count) of an ITEMOBTAINED
fn obtained(got: &[Vec<u8>]) -> Option<(String, u32)> {
    notif(got, NOTIFY_ITEMOBTAINED).filter(|b| b.len() == 53).map(|b| (String::from_utf8_lossy(cstr(&b[7..27])).to_string(), u32at(b, 27)))
}

fn rewardgold(got: &[Vec<u8>]) -> Option<u32> {
    notif(got, NOTIFY_REWARDGOLD).filter(|b| b.len() == 10).map(|b| u32at(b, 6))
}

fn enemykills(got: &[Vec<u8>]) -> Option<u32> {
    notif(got, NOTIFY_ENEMYKILLS).filter(|b| b.len() == 10).map(|b| u32at(b, 6))
}

/// iGetItemWeight of Gold (weight 1, a twentieth of it, at least 1)
fn gold_weight(count: u32) -> i32 {
    (count as i32 / 20).max(1)
}

pub fn leftovers3(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2"), (b"rthg3", b"RtHg3")] {
        ensure_char(login, a, c);
    }
    let weak = ", Level=1, Exp=0, Strenght=10";
    let max_load = 10 * 500 + 500;

    // ---- S1: the reward gold, in two parts
    let id1 = place("RtHg1", "aresden", 140, 132, &format!("{weak}, RewardGold=3000"));
    put_items(&id1, &[("Gold", 5, 1)]);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let got = common(&mut p, COMMONTYPE_REQ_GETREWARDMONEY, 0);
    let left = (max_load - gold_weight(5)) / 2;
    let (ob, rg) = (obtained(&got), rewardgold(&got));
    k.check(ob == Some(("Gold".into(), left as u32)), &format!("S1 first request: ITEMOBTAINED Gold x{left} (half the free load) {ob:?}"));
    k.check(rg == Some(3000 - left as u32), &format!("S1 ... and NOTIFY_REWARDGOLD 10 bytes with what is left {rg:?}"));
    let got = common(&mut p, COMMONTYPE_REQ_GETREWARDMONEY, 0);
    let (ob, rg) = (obtained(&got), rewardgold(&got));
    k.check(ob == Some(("Gold".into(), 3000 - left as u32)), &format!("S1 second request: the rest of it {ob:?}"));
    k.check(rg == Some(0), &format!("S1 ... NOTIFY_REWARDGOLD 0 {rg:?}"));
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let row = sql("SELECT RewardGold FROM char_database WHERE char_name='RtHg1' AND account_name LIKE 'rt%';");
    k.check(row == "0", &format!("S1 saved reward gold 0 {row:?}"));
    let n = sql(&format!("SELECT SUM(Count) FROM item WHERE CharID={id1} AND ItemName='Gold';"));
    k.check(n == "3005", &format!("S1 saved Gold 3005 in all {n:?}"));

    // ---- S2: a bag heavier than the load
    let id2 = place("RtHg2", "aresden", 140, 132, &format!("{weak}, RewardGold=1000"));
    put_items(&id2, &[("Gold", 109999, 1)]);
    let Some(mut p) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    println!("     S2 bag {:?}", super::items::item_list(&p.items).0);
    let got = common(&mut p, COMMONTYPE_REQ_GETREWARDMONEY, 0);
    k.check(obtained(&got).is_none() && rewardgold(&got).is_none(), "S2 a free load of 1 (half of it is 0): nothing given, nothing sent");
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let row = sql("SELECT RewardGold FROM char_database WHERE char_name='RtHg2' AND account_name LIKE 'rt%';");
    k.check(row == "1000", &format!("S2 the reward gold is kept {row:?}"));

    // ---- S3: the hero helm of an Aresden man
    let id3 = place("RtHg3", "aresden", 140, 132, ", EK=350, Contribution=25, Gender=1");
    put_items(&id3, &[("Dagger", 1, 300)]);
    let Some(mut p) = enter(login, b"rthg3", b"RtHg3", 3, &log, &mut k) else { return k.0 + 1 };
    let got = common(&mut p, COMMONTYPE_REQ_GETHEROMANTLE, 1);
    let (ob, ek) = (obtained(&got), enemykills(&got));
    k.check(ob == Some(("aHeroHelm(M)".into(), 1)), &format!("S3 helm: ITEMOBTAINED aHeroHelm(M) {ob:?}"));
    k.check(ek == Some(200), &format!("S3 ... NOTIFY_ENEMYKILLS 200 {ek:?}"));
    for (req, what) in [(2, "cap (contribution 5 < 20)"), (5, "hauberk (contribution 5 < 10)"), (0, "cape (200 EK < 300)"), (6, "leggings"), (3, "plate"), (7, "an index past the table"), (-1, "a negative index")] {
        let got = common(&mut p, COMMONTYPE_REQ_GETHEROMANTLE, req);
        k.check(obtained(&got).is_none() && enemykills(&got).is_none() && notif(&got, NOTIFY_CANNOTCARRYMOREITEM).is_none(), &format!("S3 {what}: refused, silent"));
    }
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let row = sql("SELECT EK, Contribution FROM char_database WHERE char_name='RtHg3' AND account_name LIKE 'rt%';");
    k.check(row == "200\t5", &format!("S3 saved 200 EK, 5 contribution {row:?}"));
    let row = sql(&format!("SELECT i.ItemName, i.ItemType, i.ID1 = c.ID1 AND i.ID2 = c.ID2 AND i.ID3 = c.ID3 FROM item i, char_database c WHERE i.CharID={id3} AND c.CharID={id3} AND i.ItemName LIKE 'aHero%';"));
    k.check(row == "aHeroHelm(M)\t1\t1", &format!("S3 the helm is saved, bound to its owner (ITET_UNIQUE_OWNER, the character's id) {row:?}"));

    // ---- S4: an Elvine woman, then a full bag
    let id1 = place("RtHg1", "elvine", 120, 120, ", Nation='elvine', EK=500, Contribution=100, Gender=2");
    put_items(&id1, &[("Dagger", 1, 300)]);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let got = common(&mut p, COMMONTYPE_REQ_GETHEROMANTLE, 3);
    let (ob, ek) = (obtained(&got), enemykills(&got));
    k.check(ob == Some(("eHeroArmor(W)".into(), 1)), &format!("S4 plate: ITEMOBTAINED eHeroArmor(W) {ob:?}"));
    k.check(ek == Some(200), &format!("S4 ... NOTIFY_ENEMYKILLS 200 {ek:?}"));
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let rows: Vec<(&str, u32, u32)> = (0..49).map(|_| ("Dagger", 1, 300)).collect();
    sql(&format!("UPDATE char_database SET EK=500, Contribution=100 WHERE CharID={id1};"));
    // (the ItemIDs of 49 rows reach those of the other characters)
    for other in ["RtHg2", "RtHg3"] {
        put_items(&sql(&format!("SELECT CharID FROM char_database WHERE char_name='{other}' AND account_name LIKE 'rt%';")), &[]);
    }
    put_items(&id1, &rows);
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(super::items::item_list(&p.items).0.len() == 49, "S4 49 bag items");
    let got = common(&mut p, COMMONTYPE_REQ_GETHEROMANTLE, 0);
    let (ob, ek) = (obtained(&got), enemykills(&got));
    k.check(ob.as_ref().is_some_and(|o| o.0 == "ElvineHeroCape") && ek == Some(200), &format!("S4 the cape takes the 50th slot: ElvineHeroCape, 200 EK {ob:?} {ek:?}"));
    let got = common(&mut p, COMMONTYPE_REQ_GETHEROMANTLE, 1);
    k.check(obtained(&got).is_none() && enemykills(&got).is_none() && notif(&got, NOTIFY_CANNOTCARRYMOREITEM).is_some_and(|b| b.len() == 6), "S4 bag full: CANNOTCARRYMOREITEM 6, nothing charged");
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let row = sql(&format!("SELECT EK, Contribution FROM char_database WHERE CharID={id1};"));
    k.check(row == "200\t100", &format!("S4 saved 200 EK, 100 contribution (the cape costs no contribution) {row:?}"));

    // ---- S5: a traveller, and too few kills
    put_items(&id1, &[]);
    let id2 = place("RtHg2", "default", 100, 100, ", Nation='NONE', EK=500, Contribution=100, Gender=1");
    put_items(&id2, &[("Dagger", 1, 300)]);
    let Some(mut p) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    let got = common(&mut p, COMMONTYPE_REQ_GETHEROMANTLE, 0);
    k.check(obtained(&got).is_none() && enemykills(&got).is_none(), "S5 a traveller (no side): nothing");
    drop(p);
    std::thread::sleep(Duration::from_secs(3));
    let id3 = place("RtHg3", "aresden", 140, 132, ", EK=99, Contribution=100, Gender=1");
    put_items(&id3, &[("Dagger", 1, 300)]);
    let Some(mut p) = enter(login, b"rthg3", b"RtHg3", 3, &log, &mut k) else { return k.0 + 1 };
    let got = common(&mut p, COMMONTYPE_REQ_GETHEROMANTLE, 2);
    k.check(obtained(&got).is_none() && enemykills(&got).is_none(), "S5 99 EK: nothing");
    k.0
}
