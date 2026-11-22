//! Potion making, crafting and manufacturing: ReqCreatePotionHandler (HG.cpp ~25722, Potion.cfg),
//! ReqCreateCraftingHandler (~39518, CraftItem.cfg), BuildItemHandler (~30031, BuildItem.cfg).
//! Item logs (bNeedLog, ITEMLOG_MAKE) are not ported.
use crate::client::{Client, Item, MAXITEMS};
use crate::combat::{SKILL_ALCHEMY, SKILL_MANUFACTURING};
use crate::consts::*;
use crate::game::Game;
use crate::life::ITEMTYPE_CONSUME;
use crate::player_data::ITEMTYPE_EQUIP;
use crate::util::{atoi, cstr};

const ITEMTYPE_NONE: i32 = 0;
const ITEMTYPE_MATERIAL: i32 = 12;
const ITEM_STONEOFMERIEN: i32 = 657;

/// A potion / crafting record: name, the 12-value ingredient array (id, count pairs, by id descending),
/// skill limit (crafting: the risk of losing a material), difficulty.
pub struct Recipe {
    pub name: Vec<u8>,
    pub array: [i16; 12],
    pub limit: i32,
    pub difficulty: i32,
}

pub fn recipe(f: &[Vec<u8>]) -> Recipe {
    let mut array = [0i16; 12];
    for (k, a) in array.iter_mut().enumerate() {
        *a = atoi(&f[2 + k]) as i16;
    }
    Recipe { name: cstr(&f[1]).to_vec(), array, limit: atoi(&f[14]), difficulty: atoi(&f[15]) }
}

/// The ingredient slots of a potion / crafting request: the six slot bytes merged (a slot named twice counts twice)
/// and sorted by item id, descending. None when a slot is out of range, empty or holds fewer than asked.
/// Returns (slot, count) pairs and the 12-value array the recipes are matched against.
pub fn ingredients(c: &Client, raw: &[u8]) -> Option<(Vec<(usize, u32)>, [i16; 12])> {
    let mut v: Vec<(usize, u32)> = Vec::new();
    for &b in &raw[..6] {
        let k = b as i8;
        if k as i32 >= MAXITEMS as i32 {
            return None;
        }
        if k < 0 {
            continue;
        }
        c.items[k as usize].as_ref()?;
        match v.iter_mut().find(|e| e.0 == k as usize) {
            Some(e) => e.1 += 1,
            None => v.push((k as usize, 1)),
        }
    }
    if v.iter().any(|&(k, n)| c.items[k].as_ref().unwrap().count < n) {
        return None;
    }
    let id = |k: usize| c.items[k].as_ref().unwrap().cfg.id_num;
    // the C++ bubble sort (stable: equal ids keep their order)
    v.sort_by(|a, b| id(b.0).cmp(&id(a.0)));
    let mut a = [-1i16; 12];
    for i in 0..6 {
        a[i * 2 + 1] = 0;
    }
    for (i, &(k, n)) in v.iter().enumerate() {
        a[i * 2] = id(k) as i16;
        a[i * 2 + 1] = n as i16;
    }
    Some((v, a))
}

/// The last recipe whose array matches (the C++ loops over all of them and keeps the last match).
pub fn find_recipe<'a>(list: impl Iterator<Item = &'a Option<Vec<Vec<u8>>>>, a: &[i16; 12]) -> Option<Recipe> {
    list.flatten().map(|f| recipe(f)).filter(|r| r.array == *a).last()
}

/// The crafted jewel's purity: the average of the wares' purities, 80 % of it plus 1d(the rest);
/// no ware with a purity: 20 + 1d80; neck crafting (needs contribution): 0. Clamped to 200 (below 0: 20).
pub fn craft_purity(purities: &[i32], neck: bool, dice: &mut impl FnMut(i32, i32) -> i32) -> i32 {
    let mut p = if purities.is_empty() {
        20 + dice(1, 80)
    } else {
        let avg = purities.iter().sum::<i32>() / purities.len() as i32;
        let base = avg * 4 / 5;
        base + dice(1, avg - base)
    };
    if neck {
        p = 0;
    }
    if p > 200 {
        200
    } else if p < 0 {
        20
    } else {
        p
    }
}

/// BuildItemHandler's quality of a manufactured weapon/armour: (spec value 2, max life span, spec value 1).
pub fn build_quality(total_value: i32, max_value: i32, average: i32, max_life: u16) -> (i16, u16, i16) {
    let v3 = if total_value <= 0 { 1.0 } else { total_value as f64 };
    let pct = (v3 / max_value as f64 * 100.0) as i32;
    let r = pct - average;
    let spec2 = if r > 0 {
        (r as f64 / (100 - average) as f64 * 100.0) as i32
    } else if r < 0 {
        (r as f64 / average as f64 * 100.0) as i32
    } else {
        0
    } as i16;
    let life = max_life as i32 + (spec2 as f64 / 100.0 * max_life as f64) as i32;
    let w = if life <= 0 { 1 } else { life as u16 };
    if w as i32 <= max_life as i32 * 2 {
        (spec2, w, w as i16)
    } else {
        (spec2, max_life, max_life as i16)
    }
}

impl Game {
    /// Uses up the ingredients: stacks lose the count, other items are depleted.
    fn use_up(&mut self, h: usize, v: &[(usize, u32)]) {
        for &(k, n) in v {
            let Some(it) = self.client(h).and_then(|c| c.items[k].as_ref()) else { continue };
            if it.cfg.item_type == ITEMTYPE_CONSUME {
                let left = it.count - n;
                self.set_item_count(h, k, left);
            } else {
                self.item_deplete(h, k, false);
            }
        }
    }

    /// `ReqCreatePotionHandler` (COMMONTYPE_REQ_CREATEPOTION, slots at 11..17)
    pub fn create_potion(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client_mut(h) else { return };
        c.skill_msg_recv_count += 1;
        let Some((v, a)) = ingredients(c, &d[11..17]) else { return };
        let Some(r) = find_recipe(self.data.potions.iter(), &a) else {
            self.notify(h, NOTIFY_NOMATCHINGPOTION, &[], None);
            return;
        };
        let mastery = self.client(h).unwrap().skill_mastery[SKILL_ALCHEMY as usize] as i32;
        if r.limit > mastery {
            self.notify(h, NOTIFY_LOWPOTIONSKILL, &[], Some(&r.name));
            return;
        }
        let mut level = (mastery - r.difficulty).max(1);
        if mastery == 100 {
            level += 5;
        }
        if self.dice(1, 100) > level {
            self.notify(h, NOTIFY_POTIONFAIL, &[], Some(&r.name));
            return;
        }
        self.calc_ssn_skill(h, SKILL_ALCHEMY, 1);
        self.use_up(h, &v);
        self.notify(h, NOTIFY_POTIONSUCCESS, &[], Some(&r.name));
        let exp = self.dice(2, r.difficulty / 3);
        self.get_exp(h, exp, false);
        if let Some(cfg) = self.data.item_by_name(&r.name).cloned() {
            let mut it = Item::new(&cfg);
            it.count = 1;
            self.add_item(h, it);
        }
    }

    /// `ReqCreateCraftingHandler` (COMMONTYPE_CRAFTITEM: a 20-byte name at 11, slots at 31..37)
    pub fn create_crafting(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client_mut(h) else { return };
        c.skill_msg_recv_count += 1;
        let Some((v, a)) = ingredients(c, &d[31..37]) else { return };
        let mut purities = Vec::new();
        let mut needed_contrib = 0;
        for &(k, n) in &v {
            let it = c.items[k].as_ref().unwrap();
            let mut p = it.item_spec_effect_value[1] as i32;
            if it.cfg.item_type == ITEMTYPE_NONE && it.cfg.sprite == 6 && it.cfg.sprite_frame == 129 {
                p = 100; // Merien stones count as 100 % purity
            }
            if it.cfg.item_type == ITEMTYPE_CONSUME || n > 1 {
                p = -1;
            }
            if it.attribute & 0xF000_0000 != 0 {
                self.notify(h, NOTIFY_CRAFTING_FAIL, &[], None);
                return;
            }
            if it.cfg.item_type == ITEMTYPE_EQUIP && it.cfg.equip_pos == EQUIPPOS_NECK {
                needed_contrib = 10;
            }
            if p != -1 {
                purities.push(p);
            }
        }
        let Some(r) = find_recipe(self.data.crafting.iter(), &a) else {
            self.notify(h, NOTIFY_NOMATCHINGCRAFTING, &[1], None);
            return;
        };
        if self.client(h).unwrap().contribution < needed_contrib {
            self.notify(h, NOTIFY_NO_CRAFT_CONTRIB, &[2], None);
            return;
        }
        if self.dice(1, 100) > r.difficulty {
            self.notify(h, NOTIFY_CRAFTING_FAIL, &[], None);
            // a failure uses up the Merien stones and risks each equipment / material ware
            for &(k, _) in &v {
                let Some(it) = self.client(h).and_then(|c| c.items[k].as_ref()) else { continue };
                if it.cfg.id_num == ITEM_STONEOFMERIEN {
                    self.item_deplete(h, k, false);
                } else if matches!(it.cfg.item_type, ITEMTYPE_EQUIP | ITEMTYPE_MATERIAL) && self.dice(1, 100) < r.limit {
                    self.item_deplete(h, k, false);
                }
            }
            return;
        }
        let rng = &mut self.rng;
        let purity = craft_purity(&purities, needed_contrib != 0, &mut |t, n| rng.dice(t, n));
        self.use_up(h, &v);
        self.client_mut(h).unwrap().contribution -= needed_contrib;
        self.notify(h, NOTIFY_CRAFTING_SUCCESS, &[], None);
        let exp = self.dice(2, 100);
        self.get_exp(h, exp, false);
        let Some(cfg) = self.data.item_by_name(&r.name).cloned() else { return };
        let mut it = Item::new(&cfg);
        it.count = 1;
        if purity != 0 {
            it.item_spec_effect_value[1] = purity as i16;
            it.attribute = 1;
        }
        it.touch_effect_type = ITET_ID as i16;
        let (x, y) = (self.dice(1, 100000), self.dice(1, 100000));
        let t = crate::chrono_now();
        // fix: the C++ formatted the date with "%d%2d" (day 5 -> "11 5", read back as 11); month*100+day here
        it.touch_effect_value = [x as i16, y as i16, (t.month * 100 + t.day) as i16];
        self.add_item(h, it);
    }

    /// `BuildItemHandler` (COMMONTYPE_BUILDITEM: the item name at 11, the six ware slots at 31..37)
    pub fn build_item(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client_mut(h) else { return };
        c.skill_msg_recv_count += 1;
        let name = cstr(&d[11..31]).to_vec();
        // the slots, -1 entries moved to the end
        let mut el: Vec<i8> = d[31..37].iter().map(|&b| b as i8).filter(|&k| k != -1).collect();
        el.resize(6, -1);
        let skill = c.skill_mastery[SKILL_MANUFACTURING as usize] as i32;
        if self.dice(1, 100) > skill {
            self.notify(h, NOTIFY_BUILDITEMFAIL, &[], None);
            return;
        }
        let c = self.client(h).unwrap();
        // fix: the C++ accepted slot MAXITEMS (`> MAXITEMS`), one past the bag
        if el.iter().any(|&k| k != -1 && (k < 0 || k as usize >= MAXITEMS || c.items[k as usize].is_none())) {
            return;
        }
        let Some(b) = self.data.build_items.iter().find(|f| cstr(&f[0]) == name.as_slice()).cloned() else { return };
        let n = |i: usize| atoi(&b[i]);
        if n(1) > skill {
            return;
        }
        let mut left: Vec<i64> = c.items.iter().map(|i| i.as_ref().map_or(0, |i| i.count as i64)).collect();
        let mut used = [false; 6];
        // which material each ware slot matched (for the count it gives up)
        let mut taken: Vec<(usize, i64)> = Vec::new();
        let (mut matched, mut total) = (0, 0);
        for x in 0..6 {
            let (id, count, value) = (n(2 + x * 3), n(3 + x * 3) as i64, n(4 + x * 3));
            if count == 0 {
                matched += 1;
                continue;
            }
            for z in 0..6 {
                if el[z] == -1 || used[z] {
                    continue;
                }
                let k = el[z] as usize;
                let it = c.items[k].as_ref().unwrap();
                // fix: the C++ only required some of the slot's count left (`> 0`), so one stack could pay for two
                // materials with less than their sum; here the remaining count must cover this material
                if it.cfg.id_num == id && it.count as i64 >= count && left[k] >= count {
                    let mut t = it.item_spec_effect_value[1] as i32;
                    if t > skill {
                        t -= (t - skill) / 2;
                    }
                    total += t * value;
                    left[k] -= count;
                    matched += 1;
                    used[z] = true;
                    taken.push((k, count));
                    break;
                }
            }
        }
        if matched != 6 {
            self.notify(h, NOTIFY_BUILDITEMFAIL, &[], None);
            return;
        }
        let max_value: i32 = (0..6).map(|x| n(4 + x * 3) * 100).sum();
        let Some(cfg) = self.data.item_by_name(&name).cloned() else { return };
        let mut it = Item::new(&cfg);
        it.count = 1;
        it.attribute = (it.attribute & 0xFFFF_FFFE) | 1;
        it.touch_effect_type = ITET_ID as i16;
        if cfg.item_type == ITEMTYPE_MATERIAL {
            let t = self.dice(1, skill / 2 + 1) - 1;
            it.item_spec_effect_value[1] = (skill / 2 + t) as i16;
            let (x, y) = (self.dice(1, 100000), self.dice(1, 100000));
            it.touch_effect_value = [x as i16, y as i16, self.now() as i16];
        } else {
            it.attribute = (it.attribute & 0x0000_FFFF) | ((n(22) as u16 as u32) << 16);
            let (spec2, max_life, spec1) = build_quality(total, max_value, n(20), it.max_life_span);
            it.item_spec_effect_value[1] = spec2;
            let (x, y) = (self.dice(1, 100000), self.dice(1, 100000));
            it.touch_effect_value = [x as i16, y as i16, self.now() as i16];
            (it.max_life_span, it.cur_life_span, it.item_spec_effect_value[0]) = (max_life, max_life, spec1);
            it.item_color = 2;
        }
        let (spec2, ty) = (it.item_spec_effect_value[1] as i32, it.cfg.item_type);
        self.add_item(h, it);
        // the C++ tests `sV1 >= 0` on a DWORD (always true): a negative quality goes out as a plain short, which
        // the client shows as is
        self.notify(h, NOTIFY_BUILDITEMSUCCESS, &[spec2, ty], None);
        // fix: the C++ took material x's count from ware slot x, whichever material that slot had matched
        for (k, count) in taken {
            let Some(it) = self.client(h).and_then(|c| c.items[k].as_ref()) else { continue };
            let left = (it.count as i64 - count).max(0) as u32;
            self.set_item_count(h, k, left);
        }
        if n(21) > skill {
            self.calc_ssn_skill(h, SKILL_MANUFACTURING, 1);
        }
        let exp = self.dice(2, n(1) / 4);
        self.get_exp(h, exp, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(line: &str) -> Vec<Vec<u8>> {
        line.split_whitespace().map(|t| t.as_bytes().to_vec()).collect()
    }

    #[test]
    fn recipes_and_formulas() {
        let r = recipe(&rec("4 DilutionPotion 220 1 217 1 208 1 190 2 -1 0 -1 0 30 20"));
        assert_eq!((r.name.as_slice(), r.array, r.limit, r.difficulty), (&b"DilutionPotion"[..], [220, 1, 217, 1, 208, 1, 190, 2, -1, 0, -1, 0], 30, 20));
        let list = vec![Some(rec("1 RedPotion 220 1 192 1 191 1 -1 0 -1 0 -1 0 20 10")), None, Some(rec("4 DilutionPotion 220 1 217 1 208 1 190 2 -1 0 -1 0 30 20"))];
        assert_eq!(find_recipe(list.iter(), &[220, 1, 192, 1, 191, 1, -1, 0, -1, 0, -1, 0]).map(|r| r.name), Some(b"RedPotion".to_vec()));
        assert!(find_recipe(list.iter(), &[220, 1, 192, 2, -1, 0, -1, 0, -1, 0, -1, 0]).is_none());

        // ingredients: slots 3, 1, 3 (two of the stack in 3), -1...; sorted by id descending
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut c = Client::new(1, 1, tx, b"1.2.3.4".to_vec(), 0);
        let it = |id: i32, count: u32| {
            let mut i = Item::default();
            i.cfg.id_num = id;
            i.count = count;
            Some(i)
        };
        c.items[1] = it(192, 1);
        c.items[3] = it(220, 2);
        let raw = [3u8, 1, 3, 0xFF, 0xFF, 0xFF];
        let (v, a) = ingredients(&c, &raw).unwrap();
        assert_eq!(v, vec![(3, 2), (1, 1)]);
        assert_eq!(a, [220, 2, 192, 1, -1, 0, -1, 0, -1, 0, -1, 0]);
        assert!(ingredients(&c, &[3, 3, 3, 0xFF, 0xFF, 0xFF]).is_none()); // 3 asked, 2 held
        assert!(ingredients(&c, &[2, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]).is_none()); // empty slot
        assert!(ingredients(&c, &[50, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]).is_none()); // past the bag

        // purity: no ware -> 20 + 1d80; wares 90 and 70 -> avg 80, 64 + 1d16; neck -> 0
        let mut fixed = |_t: i32, n: i32| n; // max roll
        assert_eq!(craft_purity(&[], false, &mut fixed), 100);
        assert_eq!(craft_purity(&[90, 70], false, &mut fixed), 80);
        assert_eq!(craft_purity(&[90, 70], true, &mut fixed), 0);
        assert_eq!(craft_purity(&[250, 250], false, &mut fixed), 200);

        // build quality: ShortSword (values 2 2 1 0 -> max 500, average 33), max life 1000
        assert_eq!(build_quality(500, 500, 33, 1000), (100, 2000, 2000));
        assert_eq!(build_quality(165, 500, 33, 1000), (0, 1000, 1000));
        assert_eq!(build_quality(0, 500, 33, 1000), (-100, 1, 1)); // 0 % -> -100, life 0 -> 1
        assert_eq!(build_quality(400, 500, 33, 1000), (70, 1700, 1700));
    }
}
