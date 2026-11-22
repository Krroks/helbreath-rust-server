//! Item upgrades (HG.cpp): RequestItemUpgradeHandler (COMMONTYPE_UPGRADEITEM) and bCheckIsItemUpgradeSuccess.
//! The upgrade level is the attribute's top nibble. Majestic items (the dark-knight / dark-mage weapons and the
//! angelic pendants) are paid with Gizon points (m_iGizonItemUpgradeLeft); the rest uses a Stone of Xelima
//! (weapons, wands) or of Merien (shields, capes, armour), which may fail and then break items of +3 and up.
use crate::client::*;
use crate::consts::*;
use crate::game::Game;
use crate::util::W;

const XELIMA: i32 = 656;
const MERIEN: i32 = 657;

/// bCheckIsItemUpgradeSuccess's chance in 1/10000 for an item at upgrade `v`; `custom` = attribute bit 1 (a
/// crafted item, whose purity `spec2` above 100 adds to it); `bonus` doubles it (Merien stone).
pub fn upgrade_chance(v: i32, custom: bool, spec2: i32, bonus: bool) -> i32 {
    let mut p = match v {
        0 => 30,
        1 => 25,
        2 => 20,
        3 => 15,
        4 | 5 => 10,
        6 | 7 => 8,
        8 => 5,
        9 => 3,
        _ => 1,
    };
    if custom && spec2 > 100 {
        p += if p > 20 { spec2 / 10 } else if p > 7 { spec2 / 20 } else { spec2 / 40 };
    }
    if bonus {
        p *= 2;
    }
    p * 100
}

/// The Gizon cost of a majestic weapon / wand upgrade from `v`.
pub fn gizon_cost(v: i32) -> i32 {
    v * (v + 6) / 8 + 2
}

/// The Gizon cost of an angelic pendant upgrade from `v` (None above +14).
pub fn pendant_cost(v: i32) -> Option<i32> {
    const T: [i32; 15] = [6, 8, 10, 13, 16, 20, 24, 29, 34, 39, 44, 49, 54, 59, 64];
    T.get(v as usize).copied()
}

/// A Merien upgrade's new maximum endurance: +20 % (crafted) or +15 %, the C++ float maths; a result past a
/// short keeps the old maximum.
pub fn merien_life(max: u16, custom: bool) -> u16 {
    let d1 = max as f64;
    let d3 = d1 + if custom { 0.2f32 as f64 } else { 0.15f32 as f64 } * d1;
    let s = d3 as i32 as i16;
    if s < 0 { max } else { s as u16 }
}

fn level(it: &Item) -> i32 {
    ((it.attribute & 0xF000_0000) >> 28) as i32
}

fn set_level(it: &mut Item, v: i32) {
    it.attribute = (it.attribute & 0x0FFF_FFFF) | ((v as u32) << 28);
}

impl Game {
    fn upgrade_fail(&mut self, h: usize, why: i32) {
        self.notify(h, NOTIFY_ITEMUPGRADEFAIL, &[why], None);
    }

    fn attr_change(&mut self, h: usize, k: usize, full: bool) {
        let Some(it) = self.client(h).unwrap().items[k].as_ref() else { return };
        let v = if full { [k as i32, it.attribute as i32, it.item_spec_effect_value[0] as i32, it.item_spec_effect_value[1] as i32] } else { [k as i32, it.attribute as i32, 0, 0] };
        self.notify(h, NOTIFY_ITEMATTRIBUTECHANGE, &v, None);
    }

    /// The Gizon payment: false (and the fail notify) when the points are short or the item is not the payer's
    /// own (touch values = the character ids). `owner_first`: the wands check the owner before the points.
    fn gizon_pay(&mut self, h: usize, k: usize, cost: Option<i32>, owner_first: bool) -> bool {
        let c = self.client(h).unwrap();
        let it = c.items[k].as_ref().unwrap();
        let owned = it.touch_effect_value == c.char_id_num;
        let left = c.gizon_item_upgrade_left;
        if owner_first && !owned {
            self.upgrade_fail(h, 2);
            return false;
        }
        let Some(cost) = cost.filter(|_| left > 0) else {
            self.upgrade_fail(h, 3);
            return false;
        };
        if !owned {
            self.upgrade_fail(h, 2);
            return false;
        }
        if left - cost < 0 {
            self.upgrade_fail(h, 3);
            return false;
        }
        let c = self.client_mut(h).unwrap();
        c.gizon_item_upgrade_left -= cost;
        let l = c.gizon_item_upgrade_left;
        self.notify(h, NOTIFY_GIZONITEMUPGRADELEFT, &[l], None);
        true
    }

    /// The majestic item becomes `id` (a new item in the same bag slot, owned by the player, upgrade `v`):
    /// NOTIFY_GIZONEITEMCHANGE. `color`: the final form's colour 9.
    fn majestic_change(&mut self, h: usize, k: usize, id: i32, v: i32, color: Option<u8>, keep_attr: bool) {
        let Some(cfg) = self.data.item_by_id(id).cloned() else {
            // the C++ left the uninitialised new item in the slot; here the old one stays
            self.attr_change(h, k, false);
            return;
        };
        let c = self.client_mut(h).unwrap();
        let mut it = Item::new(&cfg);
        it.count = 1;
        it.touch_effect_type = ITET_UNIQUE_OWNER as i16;
        it.touch_effect_value = c.char_id_num;
        if let Some(col) = color {
            it.item_color = col;
        }
        if keep_attr {
            it.attribute = c.items[k].as_ref().unwrap().attribute;
        }
        set_level(&mut it, v.min(15));
        let mut b = vec![0u8; 41];
        let mut w = W(&mut b);
        w.u32(0, MSGID_NOTIFY);
        w.u16(4, NOTIFY_GIZONEITEMCHANGE as u16);
        w.u16(6, k as u16);
        w.u8(8, it.cfg.item_type as u8);
        w.u16(9, it.cur_life_span);
        w.u16(11, it.cfg.sprite as u16);
        w.u16(13, it.cfg.sprite_frame as u16);
        w.u8(15, it.item_color);
        w.u8(16, it.item_spec_effect_value[1] as u8);
        w.u32(17, it.attribute);
        let mut name = crate::util::cstr(&it.cfg.name).to_vec();
        name.resize(20, 0);
        w.bytes(21, &name);
        c.items[k] = Some(it.clone());
        self.send(h, &b);
        self.item_log(ITEMLOG_UPGRADESUCCESS, h, -1, &it, false);
    }

    /// bCheckIsItemUpgradeSuccess (+ its item log)
    fn upgrade_roll(&mut self, h: usize, k: usize, bonus: bool) -> bool {
        let it = self.client(h).unwrap().items[k].clone().unwrap();
        let p = upgrade_chance(level(&it), it.attribute & 1 != 0, it.item_spec_effect_value[1] as i32, bonus);
        let ok = p >= self.rng.dice(1, 10000);
        self.item_log(if ok { ITEMLOG_UPGRADESUCCESS } else { ITEMLOG_UPGRADEFAIL }, h, -1, &it, false);
        ok
    }

    /// The stone upgrade: one roll; a failure breaks an item of +3 and up and uses the stone; a success raises
    /// the level by one up to `cap` (at the cap nothing changes and the stone stays). `merien`: also +15/20 %
    /// endurance. Returns false when the stone failed (the C++ returns without the final notify).
    fn stone_upgrade(&mut self, h: usize, k: usize, stone: i32, cap: i32, merien: bool) -> bool {
        let c = self.client(h).unwrap();
        // the last stone of the bag (the C++ loop keeps the last index)
        let Some(s) = (0..MAXITEMS).rev().find(|&i| c.items[i].as_ref().is_some_and(|i| i.cfg.id_num == stone)) else { return true };
        if !self.upgrade_roll(h, k, merien) {
            self.attr_change(h, k, false);
            if level(self.client(h).unwrap().items[k].as_ref().unwrap()) >= 3 {
                self.item_deplete(h, k, false);
            }
            self.item_deplete(h, s, false);
            return false;
        }
        let c = self.client_mut(h).unwrap();
        let it = c.items[k].as_mut().unwrap();
        let v = level(it) + 1;
        if v <= cap {
            set_level(it, v);
            if merien {
                let m = merien_life(it.max_life_span, it.attribute & 1 != 0);
                it.item_spec_effect_value[0] = m as i16;
                it.max_life_span = m;
            }
            self.item_deplete(h, s, false);
        }
        true
    }

    /// `RequestItemUpgradeHandler`
    pub fn request_item_upgrade(&mut self, h: usize, k: i32) {
        let Some(c) = self.client(h) else { return };
        if !(0..MAXITEMS as i32).contains(&k) {
            return;
        }
        let k = k as usize;
        let Some(it) = c.items[k].as_ref() else { return };
        let (cat, id, v, swe) = (it.cfg.category, it.cfg.id_num, level(it), (it.attribute & 0x00F0_0000) >> 20);
        // (the C++ refused levels >= 20 with fail 1: a nibble never is)
        match cat {
            46 => {
                if it.cfg.item_type != 1 || it.cfg.equip_pos < 11 || it.cfg.item_effect_type != 14 || !(16..=19).contains(&it.cfg.item_effect_value[0]) {
                    return self.upgrade_fail(h, 2);
                }
                if !self.gizon_pay(h, k, pendant_cost(v), false) {
                    return;
                }
                set_level(self.client_mut(h).unwrap().items[k].as_mut().unwrap(), v + 1);
                self.attr_change(h, k, false);
            }
            1 => match id {
                20 | 610..=613 => self.upgrade_fail(h, 2),
                719..=723 => {
                    if !self.gizon_pay(h, k, Some(gizon_cost(v)), false) {
                        return;
                    }
                    match (v, id) {
                        (2, 721) => self.majestic_change(h, k, 719, v + 2, None, false),
                        (6, 719) => self.majestic_change(h, k, 720, v + 2, None, false),
                        (14, 720) => self.majestic_change(h, k, 720, v + 2, Some(9), false),
                        _ => self.majestic_up(h, k, v + 2, 15),
                    }
                }
                717 => {
                    if self.gizon_pay(h, k, Some(gizon_cost(v)), false) {
                        self.majestic_up(h, k, v + 1, 7);
                    }
                }
                _ => {
                    if swe == ITEMSTAT_ANCIENT_U {
                        return self.upgrade_fail(h, 2);
                    }
                    let cap = if self.client(h).unwrap().items[k].as_ref().unwrap().attribute & 1 != 0 { 10 } else { 7 };
                    if self.stone_upgrade(h, k, XELIMA, cap, false) {
                        self.attr_change(h, k, false);
                    }
                }
            },
            5 | 6 | 11 | 13 | 15 => {
                // shields (5); armour, robes, capes+1, costumes; the merien / sang-ah / dark-knight / dark-mage sets
                let banned = if cat == 5 { matches!(id, 620 | 623) } else { matches!(id, 621 | 622 | 700..=702 | 704 | 706..=708 | 710..=713 | 715 | 716 | 719 | 724..=726 | 728..=731 | 733 | 775 | 776) };
                if banned || swe == 8 {
                    return self.upgrade_fail(h, 2);
                }
                if self.stone_upgrade(h, k, MERIEN, 10, true) {
                    self.attr_change(h, k, true);
                }
            }
            12 => self.upgrade_cape(h, k, id, v, swe),
            8 => match id {
                724..=726 => {
                    if !self.gizon_pay(h, k, Some(gizon_cost(v)), true) {
                        return;
                    }
                    if v == 0 {
                        let c = self.client_mut(h).unwrap();
                        let ids = c.char_id_num;
                        let it = c.items[k].as_mut().unwrap();
                        it.touch_effect_type = ITET_UNIQUE_OWNER as i16;
                        it.touch_effect_value = ids;
                    }
                    match (v, id) {
                        (2, 724) => self.majestic_change(h, k, 725, v + 2, None, false),
                        (6, 725) => self.majestic_change(h, k, 726, v + 2, None, false),
                        (14, 726) => self.majestic_change(h, k, 726, v + 1, Some(9), false),
                        _ => self.majestic_up(h, k, v + 2, 15),
                    }
                }
                _ => {
                    // fix: the C++ capped wands at +20, past the 4-bit level: +15 -> +16 wrapped to +0
                    if self.stone_upgrade(h, k, XELIMA, 15, false) {
                        self.attr_change(h, k, false);
                    }
                }
            },
            // bows (3) and the rest: nothing but the attribute echo
            _ => self.attr_change(h, k, false),
        }
    }

    /// A majestic item's plain level-up (no change of form).
    fn majestic_up(&mut self, h: usize, k: usize, v: i32, cap: i32) {
        let it = self.client_mut(h).unwrap().items[k].as_mut().unwrap();
        set_level(it, v.min(cap));
        let it = it.clone();
        self.attr_change(h, k, false);
        self.item_log(ITEMLOG_UPGRADESUCCESS, h, -1, &it, false);
    }

    /// The cape arm: a Merien stone turns a +0 cape into Cape+1 (50 contribution: the C++ takes them without
    /// checking for the plain cape) and a +0 hero cape into its +1 form (50 contribution needed).
    fn upgrade_cape(&mut self, h: usize, k: usize, id: i32, v: i32, swe: u32) {
        if swe == 8 {
            return self.upgrade_fail(h, 2);
        }
        let c = self.client(h).unwrap();
        let Some(s) = (0..MAXITEMS).rev().find(|&i| c.items[i].as_ref().is_some_and(|i| i.cfg.id_num == MERIEN)) else {
            return self.attr_change(h, k, true);
        };
        if !self.upgrade_roll(h, k, true) {
            self.attr_change(h, k, false);
            if v >= 3 {
                self.item_deplete(h, k, false);
            }
            // fix: the C++ kept the stone when a cape upgrade failed (free retries); every other failure uses it
            self.item_deplete(h, s, false);
            return;
        }
        let contribution = self.client(h).unwrap().contribution;
        let to = match id {
            402 if v == 0 => Some((429, v + 1)),
            400 if v == 0 && contribution >= 50 => Some((427, v)),
            401 if v == 0 && contribution >= 50 => Some((428, v)),
            _ => None,
        };
        let Some((to, nv)) = to else { return self.attr_change(h, k, true) };
        // the plain cape keeps its attribute (copied into the new item, which _bInitItemAttr leaves alone)
        self.majestic_change(h, k, to, nv, None, id == 402);
        self.client_mut(h).unwrap().contribution -= 50;
        self.item_deplete(h, s, false);
    }
}

const ITEMSTAT_ANCIENT_U: u32 = crate::combat::ITEMSTAT_ANCIENT as u32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas() {
        assert_eq!((upgrade_chance(0, false, 0, false), upgrade_chance(0, false, 0, true)), (3000, 6000));
        assert_eq!((upgrade_chance(9, false, 0, false), upgrade_chance(12, false, 0, false)), (300, 100));
        // crafted purity: +spec2/10 above 20 %, /20 above 7 %, /40 below
        assert_eq!(upgrade_chance(0, true, 200, false), (30 + 20) * 100);
        assert_eq!(upgrade_chance(4, true, 200, false), (10 + 10) * 100);
        assert_eq!(upgrade_chance(8, true, 200, true), (5 + 5) * 2 * 100);
        assert_eq!(upgrade_chance(0, true, 100, false), 3000);
        assert_eq!((gizon_cost(0), gizon_cost(2), gizon_cost(6), gizon_cost(14)), (2, 4, 11, 37));
        assert_eq!((pendant_cost(0), pendant_cost(14), pendant_cost(15)), (Some(6), Some(64), None));
        assert_eq!((merien_life(300, false), merien_life(300, true), merien_life(30000, true)), (345, 360, 30000));
        let mut it = Item { attribute: 0x1234_5678, ..Default::default() };
        set_level(&mut it, 15);
        assert_eq!((it.attribute, level(&it)), (0xF234_5678, 15));
    }
}
