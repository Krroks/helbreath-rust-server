//! `NpcDeadItemGenerator` (HG.cpp ~31351), `CNpc::GetGenLevel` (Npc.cpp 1531) and `CItem::InitStats`
//! (char/item/Item.cpp 70): what an NPC killed by a player drops.
//! Not ported: `NpcDeadItemGeneratorWithItemEvent` / `bCheckInItemEventList` (only middleland has item-event lines,
//! for 8-15 September, with item names that match no config: dead data, see PORTING.md), item logs.
use crate::client::Item;
use crate::consts::*;
use crate::game::Game;
use crate::map::OWNERTYPE_PLAYER;
use crate::rng::Rng;

const NPC_GUARD: i16 = 21;
const NPC_DUMMY: i16 = 34;
const NPC_AGT: i16 = 36;
const NPC_CGT: i16 = 37;
const NPC_DT: i16 = 39;
const ITEM_ZEM: i32 = 650;
const ITEM_STONEOFXELIMA: i32 = 656;
const ITEM_STONEOFMERIEN: i32 = 657;

/// `CNpc::GetGenLevel`
pub fn gen_level(ty: i16) -> i32 {
    match ty {
        10 | 16 | 22 | 55 | 56 => 1,
        14 | 18 | 17 | 11 => 2,
        23 | 12 => 3,
        61 | 27 => 4,
        72 | 76 | 74 | 13 | 28 | 53 | 60 | 62 => 5,
        29 | 33 | 48 | 54 | 65 | 78 => 6,
        70 | 71 | 30 | 63 | 79 => 7,
        31 | 32 | 49 | 50 | 52 => 8,
        58 => 9,
        77 | 59 | 75 => 10,
        _ => 0,
    }
}

/// The potion / zem / stone table of the secondary drop (0 = nothing).
fn secondary_item(r: &mut Rng) -> i32 {
    let v = match r.dice(1, 10000) {
        1..=3000 => 1,
        3001..=4000 => 2,
        4001..=5500 => 3,
        5501..=7000 => 4,
        7001..=8500 => 5,
        8501..=9100 => 6,
        9101..=9900 => 7,
        _ => 8,
    };
    match v {
        1 => 95,
        2 => 91,
        3 => 93,
        4 => 96,
        5 => 92,
        6 => 94,
        7 => if r.dice(1, 3) == 1 { 390 } else { 95 },
        _ => match r.dice(1, 8) {
            1 | 2 => if r.dice(1, 3) == 1 { ITEM_ZEM } else { 0 },
            3..=6 => if r.dice(1, 3) == 1 { 391 } else { 95 },
            _ => match r.dice(1, 8) {
                1..=5 => 95,
                6 => if r.dice(1, 2) == 1 { ITEM_STONEOFXELIMA } else { 0 },
                7 => if r.dice(1, 2) == 1 { ITEM_STONEOFMERIEN } else { 0 },
                _ => 0,
            },
        },
    }
}

/// The gen-level weapon / wand / armour tables (0 = nothing).
fn gen_level_item(r: &mut Rng, g: i32) -> i32 {
    let pick = |r: &mut Rng, t: &[i32]| t[r.dice(1, t.len() as i32) as usize - 1];
    if r.dice(1, 10000) <= 8000 {
        if r.dice(1, 10000) <= 8000 {
            match g {
                1 => pick(r, &[1, 8, 59]),
                2 => pick(r, &[12, 15, 65, 62, 23, 31]),
                3 => pick(r, &[17, 68, 23, 31]),
                4 => pick(r, &[23, 25, 28, 31, 34]),
                5 => pick(r, &[31, 34, 71]),
                6 => pick(r, &[50, 54, 46, 31, 34]),
                7 => pick(r, &[50, 54, 31, 34]),
                8 => pick(r, &[50, 54, 560, 31, 34, 55, 615]),
                9 => pick(r, &[23, 25, 28, 31, 34, 760]),
                10 => pick(r, &[46, 31, 34, 760, 761]),
                _ => 0,
            }
        } else {
            match g {
                2 | 3 => 258,
                4..=6 => 257,
                7 | 8 => 256,
                _ => 0,
            }
        }
    } else {
        // gen levels 9 and 10 roll dice(1, 9/10): above 8 nothing, like the C++
        match r.dice(1, g) {
            1 | 2 => pick(r, &[79, 81]),
            3 => 81,
            4 => pick(r, &[454, 472, 461, 482, 83]),
            5 => pick(r, &[455, 475, 84]),
            6 => match r.dice(1, 3) {
                1 => pick(r, &[456, 476]),
                2 => pick(r, &[458, 478]),
                _ => 85,
            },
            7 => match r.dice(1, 6) {
                1 => pick(r, &[457, 477]),
                2 => pick(r, &[458, 478]),
                3 => 86,
                4 => 87,
                5 => pick(r, &[600, 602]),
                _ => pick(r, &[601, 603]),
            },
            8 => 402,
            _ => 0,
        }
    }
}

/// The 1..13 stat value curve of InitStats (dice(1, 30000)).
fn stat_value(x: i32) -> i32 {
    const T: [i32; 12] = [10000, 17400, 22400, 25400, 27400, 28400, 28900, 29300, 29600, 29800, 29900, 29970];
    T.iter().position(|&t| x < t).map_or(13, |p| p as i32 + 1)
}

fn set_nibble(a: &mut u32, n: u32, v: i32) {
    *a = (*a & !(0xF << (n * 4))) | ((v as u32 & 0xF) << (n * 4));
}

/// `CItem::InitStats`: rolls the primary (nibbles 5/4) and secondary (3/2) stats of a weapon, armour or jewel.
pub fn init_stats(it: &mut Item, r: &mut Rng, g: i32) {
    let et = it.cfg.item_effect_type;
    let (st, mut sv);
    match et {
        ITEMEFFECTTYPE_ATTACK_MANASAVE | ITEMEFFECTTYPE_ATTACK => {
            let (t, color) = if et == ITEMEFFECTTYPE_ATTACK_MANASAVE {
                (10, 5) // ITEMSTAT_CASTPROB
            } else {
                match r.dice(1, 10000) {
                    1..=299 => (6, 2),     // LIGHT
                    300..=999 => (8, 3),   // STRONG
                    1000..=2499 => (1, 5), // CRITICAL
                    2500..=4499 => (5, 1), // AGILE
                    4500..=6499 => (3, 7), // RIGHTEOUS
                    6500..=8099 => (2, 4), // POISONING
                    8100..=9699 => (7, 6), // SHARP
                    _ => (9, 8),           // ANCIENT
                }
            };
            st = t;
            it.item_color = color;
            sv = stat_value(r.dice(1, 30000));
            sv = match st {
                1 => sv.max(5),
                2 | 6 => sv.max(4),
                8 => sv.max(2),
                _ => sv,
            };
            if g <= 2 {
                sv = sv.min(7);
            }
            set_nibble(&mut it.attribute, 5, st);
            set_nibble(&mut it.attribute, 4, sv);
            if r.dice(1, 10000) >= 6000 {
                let t2 = match r.dice(1, 10000) {
                    1..=4999 => 2,    // HITPROB
                    5000..=8499 => 10, // CAD
                    8500..=9499 => 12, // GOLD
                    _ => 11,          // EXP
                };
                let mut v2 = stat_value(r.dice(1, 30000));
                v2 = match t2 {
                    2 => v2.max(3),
                    10 => v2.min(7),
                    11 => 2,
                    _ => 5,
                };
                if g <= 2 {
                    v2 = v2.min(7);
                }
                set_nibble(&mut it.attribute, 3, t2);
                set_nibble(&mut it.attribute, 2, v2);
            }
        }
        ITEMEFFECTTYPE_DEFENSE | ITEMEFFECTTYPE_JEWELRY => {
            let jewel = et == ITEMEFFECTTYPE_JEWELRY;
            // fix: the C++ took m_sItemEffectValue1 as uint32, a negative or > 30000 value wrapped dice()'s range
            let bonus = if jewel { it.cfg.item_effect_value[0].clamp(0, 29000) } else { 0 };
            st = match (jewel, r.dice(1, 10000)) {
                (false, 1..=5999) => 8,    // STRONG
                (false, 6000..=8999) => 6, // LIGHT
                (false, 9000..=9554) => 11, // MANACONV
                (false, _) => 12,          // CRITICAL2
                (true, 1..=4999) => 6,
                (true, 5000..=7999) => 11,
                (true, _) => 12,
            };
            sv = stat_value(r.dice(1, 30000 - bonus) + bonus);
            sv = match st {
                6 => sv.max(4),
                8 => sv.max(2),
                _ => {
                    let v = ((sv + 1) / 2).max(1);
                    if g <= 3 { v.min(2) } else { v }
                }
            };
            if g <= 2 {
                sv = sv.min(7);
            }
            set_nibble(&mut it.attribute, 5, st);
            set_nibble(&mut it.attribute, 4, sv);
            if r.dice(1, 10000) <= 8000 {
                let (t2, mut v2) = if jewel {
                    let t2 = match r.dice(1, 13000) {
                        0..=1000 => 1,  // PSNRES
                        1001..=2000 => 3, // DEF
                        2001..=3000 => 5, // SPREC
                        3001..=5400 => 4, // HPREC
                        5401..=7800 => 6, // MPREC
                        7801..=9000 => 7, // MR
                        9001..=11000 => 11, // EXP
                        _ => 12,         // GOLD
                    };
                    // the C++ table skips 22400..22999 (value 1 there); kept, it only shifts a rare roll
                    const T: [i32; 12] = [15000, 22400, 26100, 27700, 28700, 29200, 29450, 29649, 29793, 29888, 29935, 29967];
                    let x = r.dice(1, 29980 - bonus) + bonus;
                    let v = if (22400..23000).contains(&x) { 1 } else { T.iter().position(|&t| x < t).map_or(13, |p| p as i32 + 1) };
                    (t2, v)
                } else {
                    let t2 = match r.dice(1, 10000) {
                        0..=1000 => 1,
                        1001..=2000 => 3,
                        2001..=3600 => 5,
                        3601..=6000 => 4,
                        6001..=8400 => 6,
                        8401..=9600 => 7,
                        9601..=9900 => 8, // PA
                        _ => 9,           // MA
                    };
                    const T: [i32; 12] = [13400, 22400, 26100, 27700, 28700, 29260, 29520, 29705, 29840, 29930, 29975, 30005];
                    let x = r.dice(1, 30017);
                    (t2, T.iter().position(|&t| x < t).map_or(13, |p| p as i32 + 1))
                };
                let floor3 = if jewel { matches!(t2, 1 | 3 | 7) } else { matches!(t2, 1 | 3 | 7 | 8 | 9) };
                if floor3 {
                    v2 = v2.max(3);
                }
                if g <= 2 {
                    v2 = v2.min(7);
                }
                set_nibble(&mut it.attribute, 3, t2);
                set_nibble(&mut it.attribute, 2, v2);
            }
        }
        _ => return,
    }
    it.adjust_by_stat();
}

impl Game {
    /// `NpcDeadItemGenerator`. `m_drops` is empty in the C++ (InitDrops is never called), so the droplist branch is
    /// never taken. The C++ never loads dwGoldDropValue, so its gold branch (30 %) dropped nothing; here it uses the
    /// NPC.cfg Gold column (owner, 2026-11-22).
    pub fn npc_dead_item_generator(&mut self, i: usize, ah: usize, at: u8) {
        let Some(n) = self.npcs.get(i).and_then(|n| n.as_ref()) else { return };
        if at != OWNERTYPE_PLAYER || n.is_summoned || matches!(n.type_, NPC_GUARD | NPC_DUMMY | NPC_AGT | NPC_CGT | NPC_DT) {
            return;
        }
        let (m, x, y, g) = (n.map_index, n.x, n.y, gen_level(n.type_));
        // the +1500 is for anyone not in a party being formed (PARTYSTATUS_PROCESSING); parties and heldenian
        // (the last heldenian's winners get +1500 more)
        let mut p = self.settings.primary_drop_rate + self.settings.rep_drop_modifier;
        if let Some(c) = self.client(ah) {
            p += 1500;
            if self.held_winner(c.side) == Some(true) {
                p += 1500;
            }
        }
        if p <= self.rng.dice(1, 10000) {
            return;
        }
        let it = if self.rng.dice(1, 10000) <= 3000 {
            // gold: value <= 4 -> 1d(value), else value +/- 1d(value/5); doubled with a special ability; +AddGold %
            let n = self.npcs[i].as_ref().unwrap();
            let (npc_name, sa) = (n.npc_name.clone(), n.special_ability);
            let v = self.data.npcs.iter().flatten().find(|c| Game::pad20(&c.name) == Game::pad20(&npc_name)).map_or(0, |c| c.gold_drop);
            if v <= 0 {
                return;
            }
            let mut count = if v <= 4 {
                self.rng.dice(1, v)
            } else if self.rng.dice(1, 2) == 1 {
                v + self.rng.dice(1, v / 5)
            } else {
                v - self.rng.dice(1, v / 5)
            };
            if sa != 0 {
                count *= 2;
            }
            if let Some(c) = self.client(ah) {
                count += (c.fx.add_gold as f64 / 100.0 * count as f64) as i32;
            }
            let Some(cfg) = self.data.item_by_name(b"Gold").cloned() else { return };
            let mut it = Item::new(&cfg);
            it.count = count.max(1) as u32;
            it
        } else {
            let id = if self.rng.dice(1, 10000) <= self.settings.secondary_drop_rate + self.settings.rep_drop_modifier {
                secondary_item(&mut self.rng)
            } else {
                if g == 0 {
                    return;
                }
                gen_level_item(&mut self.rng, g)
            };
            let Some(cfg) = self.data.item_by_id(id).cloned() else { return };
            let mut it = Item::new(&cfg);
            it.count = 1;
            init_stats(&mut it, &mut self.rng, g);
            it
        };
        let mut it = it;
        it.touch_effect_type = ITET_ID as i16;
        let t = crate::chrono_now();
        it.touch_effect_value = [self.rng.dice(1, 100000) as i16, self.rng.dice(1, 100000) as i16, (t.month * 100 + t.day) as i16];
        // fix (gameplay-visible): the C++ compared the tile's top item *id* with ITEMTYPE_RELIC (13, a MainGauche+1: such a
        // tile swallowed the drop), while bSetItem refused to cover the real relic (1005) yet the drop event, the log and
        // the expiry still went out for the item that was never put down; the drop vanishes when the relic is on top
        if self.ground.get(&(m, x, y)).and_then(|v| v.first()).is_some_and(|t| t.cfg.id_num == crate::astoria::ITEM_RELIC) {
            return;
        }
        let name = self.npcs[i].as_ref().map(|n| n.npc_name.clone()).unwrap_or_default();
        self.item_log_npc_drop(&name, &it);
        self.add_ground_item(m, x, y, it, TILECLEANTIME as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamedata::ItemCfg;

    fn item(et: i32) -> Item {
        Item::new(&ItemCfg { item_effect_type: et, weight: 100, max_life_span: 1000, ..Default::default() })
    }

    #[test]
    fn stat_curve() {
        assert_eq!(stat_value(1), 1);
        assert_eq!(stat_value(9999), 1);
        assert_eq!(stat_value(10000), 2);
        assert_eq!(stat_value(29969), 12);
        assert_eq!(stat_value(29970), 13);
        assert_eq!(stat_value(30000), 13);
    }

    #[test]
    fn gen_levels() {
        assert_eq!(gen_level(10), 1); // slime
        assert_eq!(gen_level(55), 1); // rabbit
        assert_eq!(gen_level(59), 10); // ettin
        assert_eq!(gen_level(21), 0);
    }

    #[test]
    fn stat_rolls_stay_in_range() {
        let mut r = Rng::new(7);
        for _ in 0..2000 {
            for et in [ITEMEFFECTTYPE_ATTACK, ITEMEFFECTTYPE_ATTACK_MANASAVE, ITEMEFFECTTYPE_DEFENSE, ITEMEFFECTTYPE_JEWELRY] {
                for g in [1, 5] {
                    let mut it = item(et);
                    init_stats(&mut it, &mut r, g);
                    let (st, sv, t2, v2) = ((it.attribute >> 20) & 15, (it.attribute >> 16) & 15, (it.attribute >> 12) & 15, (it.attribute >> 8) & 15);
                    assert!((1..=12).contains(&st) && (1..=13).contains(&sv), "{et} {st} {sv}");
                    assert!(t2 == 0 && v2 == 0 || (1..=12).contains(&t2) && (1..=13).contains(&v2));
                    if g <= 2 {
                        assert!(sv <= 7 && v2 <= 7);
                    }
                    match st {
                        1 => assert!(sv >= 5),
                        6 => assert!(sv >= 4),
                        8 => assert!(sv >= 2),
                        11 | 12 if g <= 3 => assert!(sv <= 2),
                        _ => {}
                    }
                    if et == ITEMEFFECTTYPE_ATTACK_MANASAVE {
                        assert_eq!((st, it.item_color), (10, 5));
                    }
                    if et == ITEMEFFECTTYPE_ATTACK && t2 == 11 {
                        assert_eq!(v2, 2);
                    }
                }
            }
        }
        let mut it = item(0);
        init_stats(&mut it, &mut r, 1);
        assert_eq!(it.attribute, 0); // other items get no stats
    }

    #[test]
    fn tables_give_known_ids() {
        let mut r = Rng::new(3);
        for _ in 0..5000 {
            let id = secondary_item(&mut r);
            assert!([0, 91, 92, 93, 94, 95, 96, 390, 391, ITEM_ZEM, ITEM_STONEOFXELIMA, ITEM_STONEOFMERIEN].contains(&id));
            assert!([0, 1, 8, 59, 79, 81].contains(&gen_level_item(&mut r, 1)));
        }
    }
}
