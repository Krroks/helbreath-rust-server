//! Farming: a seed bag (ITEMEFFECTTYPE_ITEMFARM of `_bDepleteDestTypeItemUseEffect`) plants a "Crops" NPC
//! (`__bSetAgricultureItem`), a hoe (weapon type 27) grows and harvests it (the crops block of
//! `calculateAttackEffect`, `probabilityTable`, `bCropsItemDrop`), and `CMap::bAdd/bRemoveCropsTotalSum`
//! count the crops of a map.
use crate::client::*;
use crate::combat::SKILL_FARMING;
use crate::consts::*;
use crate::game::Game;
use crate::map::OWNERTYPE_NPC;
use crate::npc::MOVETYPE_RANDOM;

pub const NPC_CROPS: i16 = 64;
const ITEM_GINSENG: i32 = 731;

// Skill.cpp: the chance (percent, by the harvester's farming skill 20..100 down and the crop's skill 20..100 across)
// that a hoe hit counts, and that a stage yields a crop
const SKILLCROPSTABLE: [[i32; 9]; 9] = [
    [42, 34, 27, 21, 16, 12, 9, 7, 6],
    [43, 40, 33, 27, 22, 18, 15, 13, 10],
    [44, 41, 38, 32, 27, 23, 20, 18, 13],
    [45, 42, 39, 36, 31, 27, 24, 22, 15],
    [46, 43, 40, 37, 34, 30, 27, 25, 16],
    [47, 44, 41, 38, 35, 32, 29, 27, 20],
    [48, 45, 42, 39, 36, 33, 30, 28, 23],
    [49, 46, 43, 40, 37, 34, 31, 28, 25],
    [50, 47, 44, 41, 38, 35, 32, 29, 26],
];
const CROPSDROPTABLE: [[i32; 9]; 9] = [
    [40, 0, 0, 0, 0, 0, 0, 0, 0],
    [41, 38, 0, 0, 0, 0, 0, 0, 0],
    [43, 40, 36, 0, 0, 0, 0, 0, 0],
    [46, 42, 38, 35, 0, 0, 0, 0, 0],
    [50, 45, 41, 37, 33, 0, 0, 0, 0],
    [55, 49, 44, 40, 35, 31, 0, 0, 0],
    [61, 54, 48, 43, 38, 33, 30, 0, 0],
    [68, 60, 53, 47, 42, 37, 32, 28, 0],
    [76, 67, 59, 52, 46, 41, 35, 29, 24],
];

/// The table value for (skill, crop skill) when the roll `prob` (1..100) is within it, else 0; 0 outside the tables.
fn table_hit(x: i32, y: i32, table: i32, prob: impl FnOnce() -> i32) -> i32 {
    let (ix, iy) = ((x - 20) / 10, (y - 20) / 10);
    if !(0..=8).contains(&ix) || !(0..=8).contains(&iy) {
        return 0;
    }
    let v = match table {
        1 => SKILLCROPSTABLE[ix as usize][iy as usize],
        2 => CROPSDROPTABLE[ix as usize][iy as usize],
        _ => return 0,
    };
    // the C++ rolls dice(1, 100) before looking at the table number
    if v >= prob() { v } else { 0 }
}

impl Game {
    /// `probabilityTable`
    pub fn probability_table(&mut self, x: i32, y: i32, table: i32) -> i32 {
        table_hit(x, y, table, || self.rng.dice(1, 100))
    }

    /// `CMap::bAddCropsTotalSum`
    pub fn add_crops_total(&mut self, m: usize) {
        let t = &mut self.map_npcs[m].total_agriculture;
        if *t < MAXAGRICULTURE {
            *t += 1;
        }
    }

    /// `CMap::bRemoveCropsTotalSum`
    pub fn remove_crops_total(&mut self, m: usize) {
        // fix (gameplay-visible): the C++ only counted down below MAXAGRICULTURE, so a map that reached 200 crops
        // could never plant again until a restart; here it counts down from any value
        let t = &mut self.map_npcs[m].total_agriculture;
        *t = (*t - 1).max(0);
    }

    /// `__bSetAgricultureItem`: the player plants crop `ty` (1..14, needs farming skill `ssn`) at (x, y).
    /// true = the seed bag is used up.
    pub fn set_agriculture_item(&mut self, h: usize, x: i16, y: i16, ty: i32, ssn: i32) -> bool {
        let Some(c) = self.client(h) else { return false };
        let m = c.map_index;
        if self.map_npcs[m].total_agriculture >= MAXAGRICULTURE {
            self.notify(h, NOTIFY_NOMOREAGRICULTURE, &[], None);
            return false;
        }
        if ssn > c.skill_mastery[SKILL_FARMING as usize] as i32 {
            self.notify(h, NOTIFY_AGRICULTURESKILLLIMIT, &[], None);
            return false;
        }
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return false };
        // fix: the C++ kept the naming value on the two refusals below
        let (o, ot) = self.maps[m].get_owner(x, y);
        let on_structure = o != 0 && ot == OWNERTYPE_NPC && self.npcs.get(o as usize).and_then(|n| n.as_ref()).is_some_and(|n| n.action_limit == 5);
        if on_structure || !self.maps[m].is_farm(x, y) {
            self.maps[m].set_naming_value_empty(nv);
            self.notify(h, NOTIFY_AGRICULTURENOAREA, &[], None);
            return false;
        }
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        // (bCreateNewNpc: spot 0, side 0, summoned: the crop is gone after PLANTTIME)
        let Some(i) = self.create_npc(b"Crops", &name, m, 0, MOVETYPE_RANDOM, Some((x, y)), &[-1; 10], None, 0) else {
            self.maps[m].set_naming_value_empty(nv);
            return false;
        };
        let now = self.now();
        let n = self.npcs[i].as_mut().unwrap();
        (n.is_summoned, n.summoned_time) = (true, now);
        (n.side, n.status) = (0, n.status & 0x0FFF_FFFF);
        n.crops = ty;
        // the crops 1..14 need the planter's skill, anything else the full 100
        n.crops_skill_lv = if (1..=14).contains(&ty) { ssn } else { 100 };
        n.appr2 = (ty << 8 | 1) as i16;
        self.send_event_near_npc(i, MSGID_EVENT_LOG, MSGTYPE_CONFIRM as u16);
        true
    }

    /// The crops block of `calculateAttackEffect`: a hoe hit on a crop still growing (build count > 0). The hit does
    /// no damage (the caller returns 0).
    pub fn hoe_crop(&mut self, ah: usize, th: usize) {
        let c = self.client(ah).unwrap();
        let (skill, level) = (c.skill_mastery[SKILL_FARMING as usize] as i32, c.level);
        let n = self.npcs[th].as_ref().unwrap();
        let (crops, limit, stage) = (n.crops, n.crops_skill_lv, n.build_count);
        if skill < 20 || level < 20 {
            return;
        }
        if n.type_ == NPC_CROPS && matches!(stage, 1 | 8 | 18) {
            // 18: sprout, 8: grown, 1: ripe (taken away); each stage trains the skill and may yield a crop
            self.npcs[th].as_mut().unwrap().appr2 = (crops << 8 | if stage == 18 { 2 } else { 3 }) as i16;
            if skill <= limit + 10 {
                self.calc_ssn_skill(ah, SKILL_FARMING, 1);
            }
            self.send_event_near_npc(th, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16);
            if self.probability_table(skill, limit, 2) > 0 {
                if stage == 1 {
                    self.crops_item_drop(ah, th, true);
                } else {
                    if skill <= limit + 10 {
                        self.calc_ssn_skill(ah, SKILL_FARMING, 1);
                    }
                    self.crops_item_drop(ah, th, false);
                }
            }
            if stage == 1 {
                self.delete_npc(th);
                return;
            }
        }
        if self.probability_table(skill, limit, 1) > 0 || matches!(self.npcs[th].as_ref().unwrap().build_count, 1 | 8 | 18) {
            // the hoe wears out (the C++ tests `iTemp != -1 || item != NULL`: with no hoe it read the item list at -1)
            let c = self.client_mut(ah).unwrap();
            let k = c.equip_status[EQUIPPOS_RHAND as usize];
            if let Some(it) = usize::try_from(k).ok().and_then(|k| c.items[k].as_mut()) {
                it.cur_life_span = it.cur_life_span.saturating_sub(1);
                if it.cur_life_span == 0 {
                    let pos = it.cfg.equip_pos;
                    self.notify(ah, NOTIFY_ITEMLIFESPANEND, &[pos, k as i32], None);
                    self.release_item(ah, k as usize, true);
                }
            }
            self.npcs[th].as_mut().unwrap().build_count -= 1;
        }
        let n = self.npcs[th].as_mut().unwrap();
        n.build_count = n.build_count.max(0);
    }

    /// `bCropsItemDrop`: the crop's produce on the harvester's tile (or, when `at_crop`, on the crop's), and exp.
    pub fn crops_item_drop(&mut self, h: usize, th: usize, at_crop: bool) -> bool {
        let (id, throw) = match self.npcs[th].as_ref().unwrap().crops {
            c @ 1..=13 => (819 + c, if c <= 2 { 3 } else { 4 + (c - 3) / 2 }),
            14 => (ITEM_GINSENG, 10),
            _ => return false,
        };
        let e = self.rng.dice(throw, 10);
        self.get_exp(h, e, false);
        let Some(cfg) = self.data.item_by_id(id).cloned() else { return false };
        let mut it = Item::new(&cfg);
        it.count = 1;
        let (m, x, y) = if at_crop {
            let n = self.npcs[th].as_ref().unwrap();
            (n.map_index, n.x, n.y)
        } else {
            let c = self.client(h).unwrap();
            (c.map_index, c.x, c.y)
        };
        self.drop_ground_item(m, x, y, it);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables() {
        // roll 1..100 against the table value: a skill 20 harvester of a crop of 20 hits on 42 and fewer
        assert_eq!(table_hit(20, 20, 1, || 42), 42);
        assert_eq!(table_hit(20, 20, 1, || 43), 0);
        assert_eq!(table_hit(100, 100, 2, || 24), 24);
        assert_eq!(table_hit(100, 100, 2, || 25), 0);
        // the division truncates toward zero as in C: 11..29 is still row 0; below 10 and above 109 is outside; the
        // drop table has zero cells
        assert_eq!(table_hit(19, 20, 1, || 1), 42);
        assert_eq!(table_hit(9, 20, 1, || 1), 0);
        assert_eq!(table_hit(20, 110, 1, || 1), 0);
        assert_eq!(table_hit(20, 30, 2, || 1), 0);
        assert_eq!(table_hit(20, 20, 3, || 1), 0);
    }

    #[test]
    fn produce() {
        // bCropsItemDrop's table: crop -> item 819 + crop (820..832), exp dice 3 for the first two, then +1 per two crops
        let throws: Vec<i32> = (1..=14).map(|c| match c { 1 | 2 => 3, 3..=13 => 4 + (c - 3) / 2, _ => 10 }).collect();
        assert_eq!(throws, [3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 10]);
    }
}
