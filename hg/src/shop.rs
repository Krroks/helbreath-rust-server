//! Shops and spell learning (HG.cpp): RequestPurchaseItemHandler, ReqSellItemHandler / ReqSellItemConfirmHandler,
//! RequestSellItemListHandler, ReqRepairItemHandler / ReqRepairItemCofirmHandler, RequestStudyMagicHandler.
//! The shop's item list is the client's own (its CONTENTS files): the server only checks m_bIsForSale.
use crate::client::{Item, MAXITEMS};
use crate::combat::ITEMTYPE_ARROW;
use crate::consts::*;
use crate::game::Game;
use crate::item_use::name20;
use crate::life::ITEMTYPE_CONSUME;
use crate::util::{cstr, W};

/// The gold a purchase costs (`RequestPurchaseItemHandler`): the winner side of the last crusade pays 90 %,
/// the charisma discount ((cha - 10) / 4 %, +10 for the other sides, 0..10 %) never goes under half price.
/// (A drawn crusade, winner 0, gives the neutral players the winner's price, as in the C++.)
/// The last heldenian's losers pay x1.5 before the discount.
pub fn purchase_cost(price: u16, count: u32, side: i32, cha: i32, winner: i32, held_loser: bool) -> i32 {
    let base = price as i32 * count as i32;
    let cost = if winner == side { ((price as f32) * 0.9f32 + 0.5f32) as i32 * count as i32 } else { base };
    let mut ratio = (cha - 10) / 4;
    if winner != side {
        ratio += 10;
    }
    let ratio = ratio.clamp(0, 10);
    let mut discount = (cost as f64 * (ratio as f64 / 100.0f32 as f64)) as i32;
    if cost - discount <= base / 2 {
        discount = cost - base / 2 + 1;
    }
    let cost = if held_loser { (cost as f64 * 1.5) as i32 } else { cost };
    cost - discount
}

/// The secondary-stat (nibble 5) and primary-stat (nibble 3) multipliers of the sale price.
fn stat_add(price: i32, mul: u32, value: u32) -> u32 {
    let pct = [0.0, 10.0, 20.0, 30.0, 35.0, 40.0, 50.0, 100.0, 200.0, 300.0, 400.0, 500.0, 700.0, 900.0f32];
    let d1 = price as f64 * mul as f64;
    let d2 = pct.get(value as usize).copied().unwrap_or(0.0) as f64;
    (d1 + d1 * (d2 / 100.0f32 as f64)) as i32 as u32
}

/// What a shop pays for `num` of an item (ReqSellItemHandler / ReqSellItemConfirmHandler share it), `None` =
/// not sellable here (1) or used up (2), as the CANNOTSELLITEM reason.
pub fn sell_price(it: &Item, num: i32, neutral: bool) -> Result<i32, i32> {
    let cat = it.cfg.category;
    let mut price = if (1..=10).contains(&cat) || it.cfg.item_effect_type == ITEMEFFECTTYPE_JEWELRY {
        if it.cur_life_span == 0 {
            return Err(2);
        }
        let max = if it.max_life_span != 0 { it.max_life_span as f64 } else { 1.0 };
        let mut p = ((it.cur_life_span as f64 / max) * 0.5f32 as f64 * it.price as f64) as i32 * num;
        let a = it.attribute;
        let mut add1 = 0u32;
        let mut add2 = 0u32;
        if a & 0x00F0_0000 != 0 {
            let mul = match (a >> 20) & 0xF {
                6 | 8 => 2,
                5 => 3,
                1 => 4,
                7 => 5,
                2 => 6,
                3 => 15,
                9 => 20,
                _ => 1,
            };
            add1 = stat_add(p, mul, (a >> 16) & 0xF);
        }
        if a & 0x0000_F000 != 0 {
            let mul = match (a >> 12) & 0xF {
                1 | 12 => 2,
                2..=7 => 4,
                8..=11 => 6,
                _ => 1, // fix: the C++ left dwMul2 uninitialised for primary stats 13-15
            };
            add2 = stat_add(p, mul, (a >> 8) & 0xF);
        }
        p = (p as u32).wrapping_add(add1 - add1 / 3).wrapping_add(add2 - add2 / 3) as i32;
        p
    } else if (11..=50).contains(&cat) {
        (it.price as i32 / 2) * num
    } else {
        return Err(1);
    };
    if neutral {
        price /= 2;
    }
    Ok(price.clamp(1, 1_000_000))
}

/// What a repair costs: half the price for the lost half of the endurance.
pub fn repair_price(it: &Item) -> u32 {
    if it.cur_life_span == 0 {
        return it.price as u32 / 2;
    }
    let max = if it.max_life_span != 0 { it.max_life_span as f64 } else { 1.0 };
    let d3 = (it.cur_life_span as f64 / max) * 0.5f32 as f64 * it.price as f64;
    // fix: the C++ stored a negative difference (endurance above the maximum) in a DWORD: a huge price
    ((it.price / 2) as f64 - d3).max(0.0) as u32
}

impl Game {
    /// `RequestPurchaseItemHandler` (COMMONTYPE_REQ_PURCHASEITEM: name, count)
    pub fn purchase_item(&mut self, h: usize, name: &[u8], num: i32) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !c.is_on_shop {
            return;
        }
        let n = name20(name);
        let (item_name, count): (Vec<u8>, u32) = if n.starts_with(b"10Arrows") {
            (b"Arrow".to_vec(), 10)
        } else if n.starts_with(b"100Arrows") {
            (b"Arrow".to_vec(), 100)
        } else {
            (cstr(&n).to_vec(), 1)
        };
        for _ in 0..num.max(0) {
            let Some(cfg) = self.data.item_by_name(&item_name).cloned() else { continue };
            if !cfg.is_for_sale {
                return;
            }
            let mut it = Item::new(&cfg);
            it.count = count;
            it.touch_effect_type = ITET_ID as i16;
            let (a, b) = (self.rng.dice(1, 100000), self.rng.dice(1, 100000));
            let t = crate::chrono_now();
            it.touch_effect_value = [a as i16, b as i16, (t.month * 100 + t.day) as i16];
            let c = self.client(h).unwrap();
            let cost = purchase_cost(it.price, count, c.side, c.range, self.cr.winner_side, self.held_winner(c.side) == Some(false));
            let (_, gold) = self.item_count(h, b"Gold");
            if gold < cost as u32 {
                let mut b = vec![0u8; 7];
                let mut w = W(&mut b);
                w.u32(0, MSGID_NOTIFY);
                w.u16(4, NOTIFY_NOTENOUGHGOLD as u16);
                w.u8(6, 0xFF);
                self.send(h, &b);
                return;
            }
            if !self.add_client_item_list(h, &it) {
                self.calc_total_weight(h);
                self.send_cannot_carry(h);
                // fix: the C++ went on looping (one CANNOTCARRYMOREITEM per remaining item, any count the client sent)
                return;
            }
            let mut b = vec![0u8; 48];
            let mut w = W(&mut b);
            w.u32(0, MSGID_NOTIFY);
            w.u16(4, NOTIFY_ITEMPURCHASED as u16);
            w.u8(6, 1);
            crate::entry::item_entry(&mut w, 7, &it, Some(false));
            w.u16(46, cost as u16);
            b.truncate(48);
            self.send(h, &b);
            // fix: the C++ took the price as a WORD, so items over 65535 gold cost price % 65536
            let (k, gold) = self.item_count(h, b"Gold");
            if let Some(k) = k {
                self.set_item_count(h, k, gold - cost as u32);
            }
            self.calc_total_weight(h);
            self.add_city_funds(h, cost as u32);
        }
    }

    /// `m_stCityStatus[side].iFunds += gold` (the income of shops, repairs and charged teleports; nothing reads it)
    pub fn add_city_funds(&mut self, h: usize, gold: u32) {
        let side = self.client(h).map_or(0, |c| c.side as usize);
        if let Some(f) = self.city_funds.get_mut(side) {
            *f += gold as u64;
        }
    }

    /// `ReqSellItemHandler` (COMMONTYPE_REQ_SELLITEM: slot, shop NPC type, count): the price offer.
    pub fn sell_item_offer(&mut self, h: usize, k: i32, whom: i32, num: i32) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !(0..MAXITEMS as i32).contains(&k) || num <= 0 {
            return;
        }
        let k = k as usize;
        let Some(it) = c.items[k].clone() else { return };
        if it.count < num as u32 || !matches!(whom, 15 | 24) {
            return;
        }
        self.calc_total_weight(h);
        let c = self.client(h).unwrap();
        let max_load = c.get_str() * 500 + c.level * 500;
        match sell_price(&it, num, c.is_neutral()) {
            Err(r) => self.notify(h, NOTIFY_CANNOTSELLITEM, &[k as i32, r], Some(&it.cfg.name)),
            Ok(p) => {
                let gold = self.data.item_by_name(b"Gold").map(|g| Item::new(g).weight_of(p as i64)).unwrap_or(0);
                if c.cur_weight_load + gold > max_load {
                    self.notify(h, NOTIFY_CANNOTSELLITEM, &[k as i32, 4], Some(&it.cfg.name));
                } else {
                    self.notify(h, NOTIFY_SELLITEMPRICE, &[k as i32, it.cur_life_span as i16 as i32, p, num], Some(&it.cfg.name));
                }
            }
        }
    }

    /// `ReqSellItemConfirmHandler`: the sale (the gold falls on the tile when it cannot be carried).
    pub fn sell_item_confirm(&mut self, h: usize, k: i32, num: i32) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !(0..MAXITEMS as i32).contains(&k) || num <= 0 || !c.is_on_shop {
            return;
        }
        let k = k as usize;
        let Some(it) = c.items[k].clone() else { return };
        if it.count < num as u32 {
            return;
        }
        self.calc_total_weight(h);
        let Ok(price) = sell_price(&it, num, self.client(h).unwrap().is_neutral()) else { return };
        self.notify(h, NOTIFY_ITEMSOLD, &[k as i32], None);
        self.item_log(ITEMLOG_SELL, h, -1, &it, false);
        if matches!(it.cfg.item_type, ITEMTYPE_CONSUME | ITEMTYPE_ARROW) {
            self.set_item_count(h, k, it.count - num as u32);
        } else {
            self.item_deplete(h, k, false);
        }
        let Some(cfg) = self.data.item_by_name(b"Gold").cloned() else { return };
        let mut gold = Item::new(&cfg);
        gold.count = price as u32;
        if self.add_client_item_list(h, &gold) {
            self.send_item_obtained(h, &gold);
        } else {
            let c = self.client(h).unwrap();
            let (m, x, y) = (c.map_index, c.x, c.y);
            self.drop_ground_item(m, x, y, gold);
            self.calc_total_weight(h);
            self.send_cannot_carry(h);
        }
    }

    /// `RequestSellItemListHandler` (MSGID_REQUEST_SELLITEMLIST): up to 12 (slot, count) sales; stops at the
    /// first empty entry.
    pub fn sell_item_list(&mut self, h: usize, d: &[u8]) {
        let r = crate::util::Rd::new(d);
        for i in 0..12 {
            let (k, n) = (r.u8(6 + i * 5) as i8 as i32, r.u32(7 + i * 5) as i32);
            if !(0..MAXITEMS as i32).contains(&k) || self.client(h).is_none_or(|c| c.items[k as usize].is_none()) {
                return;
            }
            self.sell_item_confirm(h, k, n);
        }
    }

    /// `ReqRepairItemHandler` (COMMONTYPE_REQ_REPAIRITEM: slot, NPC type): weapons at the blacksmith (24),
    /// armour and the rest at the shop (15).
    pub fn repair_item_offer(&mut self, h: usize, k: i32, whom: i32) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !(0..MAXITEMS as i32).contains(&k) {
            return;
        }
        let Some(it) = c.items[k as usize].clone() else { return };
        let cat = it.cfg.category;
        let need = if (1..=10).contains(&cat) {
            24
        } else if (43..=50).contains(&cat) || (11..=13).contains(&cat) {
            15
        } else {
            self.notify(h, NOTIFY_CANNOTREPAIRITEM, &[k, 1], Some(&it.cfg.name));
            return;
        };
        if whom != need {
            self.notify(h, NOTIFY_CANNOTREPAIRITEM, &[k, 2], Some(&it.cfg.name));
            return;
        }
        self.notify(h, NOTIFY_REPAIRITEMPRICE, &[k, it.cur_life_span as i32, repair_price(&it) as i32], Some(&it.cfg.name));
    }

    /// `ReqRepairItemCofirmHandler`
    pub fn repair_item_confirm(&mut self, h: usize, k: i32) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !(0..MAXITEMS as i32).contains(&k) || !c.is_on_shop {
            return;
        }
        let k = k as usize;
        let Some(it) = c.items[k].clone() else { return };
        let cat = it.cfg.category;
        if !((1..=10).contains(&cat) || (43..=50).contains(&cat) || (11..=13).contains(&cat)) {
            return;
        }
        let price = repair_price(&it);
        let (g, gold) = self.item_count(h, b"Gold");
        if gold < price {
            let mut b = vec![0u8; 7];
            let mut w = W(&mut b);
            w.u32(0, MSGID_NOTIFY);
            w.u16(4, NOTIFY_NOTENOUGHGOLD as u16);
            w.u8(6, k as u8);
            self.send(h, &b);
            return;
        }
        let it = self.client_mut(h).unwrap().items[k].as_mut().unwrap();
        it.cur_life_span = it.max_life_span;
        let life = it.cur_life_span as i32;
        self.notify(h, NOTIFY_ITEMREPAIRED, &[k as i32, life], None);
        if let Some(g) = g {
            self.set_item_count(h, g, gold - price);
        }
        self.calc_total_weight(h);
        self.add_city_funds(h, price);
    }

    /// `RequestStudyMagicHandler`: `purchase` = bought in the magic tower / shop with gold (COMMONTYPE_REQ_STUDYMAGIC),
    /// else a spell scroll. NOTIFY_MAGICSTUDYSUCCESS (index, name 30) or MAGICSTUDYFAIL (1, index, name, cost, int).
    pub fn study_magic(&mut self, h: usize, name: &[u8], purchase: bool) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        let mut n30 = cstr(name).to_vec();
        n30.resize(30, 0);
        let found = self.data.magic.iter().enumerate().find_map(|(i, f)| {
            let f = f.as_ref()?;
            let mut fname = cstr(f.get(1)?).to_vec();
            fname.resize(30, 0);
            let num = |j: usize| f.get(j).map(|t| crate::util::atoi(t)).unwrap_or(0);
            (fname == n30).then(|| (i, num(16), num(17)))
        });
        let Some((idx, req_int, cost)) = found else { return };
        let mut ok = true;
        let (g, gold) = self.item_count(h, b"Gold");
        let c = self.client(h).unwrap();
        // Rust only: GM spells (cost < 0): players can't learn them by any path, GMs learn them free in the shop
        let gm_spell = cost < 0;
        if gm_spell && !c.is_gm() {
            ok = false;
        }
        let cost = cost.max(0);
        if purchase && !gm_spell && (cost as u32 > gold || (!c.is_on_tower && !c.is_neutral() && !c.is_on_shop)) {
            ok = false;
        }
        if idx >= c.magic_mastery.len() || c.magic_mastery[idx] != 0 {
            return;
        }
        let mut b = vec![0u8; 46];
        let mut w = W(&mut b);
        w.u32(0, MSGID_NOTIFY);
        if req_int <= c.get_int() && ok {
            if purchase {
                if let Some(g) = g {
                    self.set_item_count(h, g, gold - cost as u32);
                }
            }
            self.calc_total_weight(h);
            self.client_mut(h).unwrap().magic_mastery[idx] = 1;
            w.u16(4, NOTIFY_MAGICSTUDYSUCCESS as u16);
            w.u8(6, idx as u8);
            w.bytes(7, &n30);
            b.truncate(37);
        } else {
            w.u16(4, NOTIFY_MAGICSTUDYFAIL as u16);
            w.u8(6, 1);
            w.u8(7, idx as u8);
            w.bytes(8, &n30);
            w.u32(38, cost as u32);
            w.u32(42, req_int as u32);
        }
        self.send(h, &b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(cat: i32, price: u16, life: (u16, u16), attr: u32) -> Item {
        let mut it = Item::default();
        it.cfg.category = cat;
        it.price = price;
        (it.cur_life_span, it.max_life_span) = life;
        it.attribute = attr;
        it
    }

    #[test]
    fn prices() {
        // citizens (side 1) get the full 10 % off, travellers 0.9 x and the charisma part
        assert_eq!(purchase_cost(1000, 1, 1, 10, 0, false), 900);
        assert_eq!(purchase_cost(1000, 1, 0, 10, 0, false), 900);
        assert_eq!(purchase_cost(1000, 1, 0, 50, 0, false), 810);
        assert_eq!(purchase_cost(10, 100, 1, 10, 0, false), 900);
        assert_eq!(purchase_cost(1, 1, 1, 10, 0, false), 1);
        // a worn weapon sells for half its price times its endurance left; travellers half of that
        assert_eq!(sell_price(&item(1, 1000, (50, 100), 0), 1, false), Ok(250));
        assert_eq!(sell_price(&item(1, 1000, (50, 100), 0), 1, true), Ok(125));
        assert_eq!(sell_price(&item(1, 1000, (0, 100), 0), 1, false), Err(2));
        assert_eq!(sell_price(&item(11, 100, (0, 0), 0), 3, false), Ok(150));
        assert_eq!(sell_price(&item(60, 100, (0, 0), 0), 1, false), Err(1));
        // a STRONG 2 sword (mul 2, +20 %): 250 + (600 - 200)
        assert_eq!(sell_price(&item(1, 1000, (50, 100), 0x0082_0000), 1, false), Ok(650));
        // primary stat 13 (uninitialised multiplier in the C++): x1 here
        assert_eq!(sell_price(&item(1, 1000, (100, 100), 0x0000_D100), 1, false), Ok(500 + 550 - 183));
        assert_eq!(repair_price(&item(1, 1000, (50, 100), 0)), 250);
        assert_eq!(repair_price(&item(1, 1000, (0, 100), 0)), 500);
        assert_eq!(repair_price(&item(1, 1000, (150, 100), 0)), 0);
    }
}
