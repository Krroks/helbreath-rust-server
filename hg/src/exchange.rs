//! The trade window between two players (HG.cpp ~27773-28207): ExchangeItemHandler (COMMONTYPE_EXCHANGEITEMTOCHAR),
//! SetExchangeItem, ConfirmExchangeItem, CancelExchangeItem, _ClearExchangeStatus, _iGetItemSpaceLeft. Item logs
//! (_bItemLog ITEMLOG_EXCHANGE) and the astoria relic checks are not ported (no item logs, no astoria).
use crate::client::{Item, MAXITEMS};
use crate::consts::*;
use crate::game::Game;
use crate::combat::ITEMTYPE_ARROW;
use crate::item_use::name20;
use crate::life::ITEMTYPE_CONSUME;
use crate::map::OWNERTYPE_PLAYER;
use crate::util::{cstr, W};

/// MAXCLIENTS of the trade checks (the client handle bound of ExchangeItemHandler).
const MAX_HANDLE: u16 = MAXCLIENTS as u16;

/// The weight check of ConfirmExchangeItem: each side must carry what the other gives.
// fix: the C++ assigned each item's weight (`iItemWeightA = ...`) instead of adding it, so only the last item counted
pub fn weights_fit(left_a: i32, left_b: i32, give_a: &[i32], give_b: &[i32]) -> bool {
    let (wa, wb): (i32, i32) = (give_a.iter().sum(), give_b.iter().sum());
    left_a >= wb && left_b >= wa
}

fn stackable(it: &Item) -> bool {
    matches!(it.cfg.item_type, ITEMTYPE_CONSUME | ITEMTYPE_ARROW)
}

impl Game {
    /// NOTIFY_OPENEXCHANGEWINDOW / NOTIFY_SETEXCHANGEITEM (57 bytes): slot (+1000 for the owner's own window),
    /// sprite, frame, amount (int), colour, life, max life, spec value 2 + 100, name 20, owner name 10, attribute.
    fn notify_exchange_item(&mut self, to: usize, ty: i32, from: usize, k: usize, slot_value: i32, amount: i32) {
        let Some(it) = self.client(from).and_then(|c| c.items[k].clone()) else { return };
        let from_name = self.client(from).unwrap().char_name.clone();
        let mut b = vec![0u8; 57];
        let mut w = W(&mut b);
        w.u32(0, MSGID_NOTIFY);
        w.u16(4, ty as u16);
        w.u16(6, slot_value as u16);
        w.u16(8, it.cfg.sprite as u16);
        w.u16(10, it.cfg.sprite_frame as u16);
        w.u32(12, amount as u32);
        w.u8(16, it.item_color);
        w.u16(17, it.cur_life_span);
        w.u16(19, it.max_life_span);
        w.u16(21, (it.item_spec_effect_value[1] as i32 + 100) as u16);
        w.bytes(23, &name20(&it.cfg.name));
        let mut n = cstr(&from_name).to_vec();
        n.resize(10, 0);
        w.bytes(43, &n);
        w.u32(53, it.attribute);
        self.send(to, &b);
    }

    /// `ExchangeItemHandler`: opens the window with the player on (dx, dy) and puts the first item in.
    #[allow(clippy::too_many_arguments)]
    pub fn exchange_item_to_char(&mut self, h: usize, k: i32, amount: i32, dx: i16, dy: i16, obj: u16) {
        let Some(c) = self.client(h) else { return };
        // fix: an amount of 0 or less was accepted (the DWORD compare only stopped negative ones)
        if k < 0 || k as usize >= MAXITEMS || amount <= 0 || c.exchange_mode || obj >= MAX_HANDLE || c.is_on_waiting_process || self.is_relic_holder(h) {
            return;
        }
        let k = k as usize;
        let Some(it) = c.items[k].as_ref() else { return };
        if it.count < amount as u32 {
            return;
        }
        let name = it.cfg.name.clone();
        let m = c.map_index;
        let mut owner = self.maps[m].tile(dx, dy).and_then(|t| (t.owner != 0 && t.owner_class == OWNERTYPE_PLAYER).then_some(t.owner as usize));
        if owner.is_none() {
            self.clear_exchange_status(h);
            return;
        }
        if obj != 0 && self.client(obj as usize).is_some() && owner != Some(obj as usize) {
            owner = None;
        }
        // fix: the tile owner could be the trader himself (a window with oneself that confirm never completes)
        let Some(o) = owner.filter(|&o| o != h && self.client(o).is_some()) else {
            self.clear_exchange_status(h);
            return;
        };
        let oc = self.client(o).unwrap();
        if oc.exchange_mode || (oc.appr[2] as u16 & 0xF000) != 0 || self.maps[oc.map_index].is_fight_zone {
            self.clear_exchange_status(h);
            return;
        }
        let (hname, oname) = (self.client(h).unwrap().char_name.clone(), oc.char_name.clone());
        let c = self.client_mut(h).unwrap();
        (c.exchange_mode, c.exchange_h, c.exchange_name, c.exchange_count) = (true, o, oname, 1);
        c.exchange_items = Default::default();
        c.exchange_items.iter_mut().for_each(|e| e.0 = -1);
        c.exchange_items[0] = (k as i32, amount, name);
        let oc = self.client_mut(o).unwrap();
        (oc.exchange_mode, oc.exchange_h, oc.exchange_name, oc.exchange_count) = (true, h, hname, 0);
        oc.exchange_items = Default::default();
        oc.exchange_items.iter_mut().for_each(|e| e.0 = -1);
        self.notify_exchange_item(h, NOTIFY_OPENEXCHANGEWINDOW, h, k, k as i32 + 1000, amount);
        self.notify_exchange_item(o, NOTIFY_OPENEXCHANGEWINDOW, h, k, k as i32, amount);
    }

    /// `SetExchangeItem`: one more item (4 at most) in the window.
    pub fn set_exchange_item(&mut self, h: usize, k: i32, amount: i32) {
        let Some(c) = self.client(h) else { return };
        // fix: `m_exchangeCount > 4` let a fifth item be written past m_exchangeItems[4]
        if c.is_on_waiting_process || c.exchange_count >= 4 || !c.exchange_mode || c.exchange_h == 0 {
            return;
        }
        let x = c.exchange_h;
        if self.client(x).is_none_or(|o| name10(&o.char_name) != name10(&c.exchange_name)) {
            return;
        }
        if k < 0 || k as usize >= MAXITEMS || amount <= 0 {
            return;
        }
        let k = k as usize;
        let Some(it) = c.items[k].as_ref() else { return };
        if it.count < amount as u32 {
            return;
        }
        if c.exchange_items[..c.exchange_count].iter().any(|e| e.0 == k as i32) {
            self.clear_exchange_status(x);
            self.clear_exchange_status(h);
            return;
        }
        let name = it.cfg.name.clone();
        let c = self.client_mut(h).unwrap();
        c.exchange_items[c.exchange_count] = (k as i32, amount, name);
        c.exchange_count += 1;
        self.notify_exchange_item(h, NOTIFY_SETEXCHANGEITEM, h, k, k as i32 + 1000, amount);
        self.notify_exchange_item(x, NOTIFY_SETEXCHANGEITEM, h, k, k as i32, amount);
    }

    /// The items a side gives, checked against its bag (None: an item is gone, renamed or short).
    fn exchange_goods(&self, h: usize) -> Option<Vec<(usize, i32, Item)>> {
        let c = self.client(h)?;
        let mut v = Vec::new();
        for (k, amount, name) in &c.exchange_items[..c.exchange_count] {
            let it = c.items.get(*k as usize)?.as_ref()?;
            if name20(&it.cfg.name) != name20(name) || (stackable(it) && *amount as u32 > it.count) {
                return None;
            }
            let given = if stackable(it) {
                let mut p = Item::new(self.data.item_by_name(&it.cfg.name)?);
                p.count = *amount as u32;
                p
            } else {
                it.clone()
            };
            v.push((*k as usize, *amount, given));
        }
        Some(v)
    }

    /// `ConfirmExchangeItem`: when both sides confirmed, the items change hands.
    pub fn confirm_exchange_item(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if c.is_on_waiting_process || !c.exchange_mode || c.exchange_h == 0 || self.is_relic_holder(h) || self.is_relic_holder(c.exchange_h) {
            return;
        }
        let x = c.exchange_h;
        if x == h {
            return;
        }
        let Some(o) = self.client(x) else {
            self.clear_exchange_status(h);
            return;
        };
        if name10(&c.exchange_name) != name10(&o.char_name) || !o.exchange_mode || name10(&o.exchange_name) != name10(&c.char_name) {
            self.clear_exchange_status(h);
            self.clear_exchange_status(x);
            return;
        }
        self.client_mut(h).unwrap().exchange_confirm = true;
        if !self.client(x).unwrap().exchange_confirm {
            return;
        }
        let (Some(ga), Some(gb)) = (self.exchange_goods(h), self.exchange_goods(x)) else {
            self.clear_exchange_status(h);
            self.clear_exchange_status(x);
            return;
        };
        let left = |g: &Game, i: usize| {
            let c = g.client(i).unwrap();
            c.get_str() * 500 + c.level * 500 - c.cur_weight_load
        };
        let wa: Vec<i32> = ga.iter().map(|(_, n, it)| it.weight_of(*n as i64)).collect();
        let wb: Vec<i32> = gb.iter().map(|(_, n, it)| it.weight_of(*n as i64)).collect();
        if !weights_fit(left(self, h), left(self, x), &wa, &wb) || self.item_space_left(h) == 0 || self.item_space_left(x) == 0 {
            self.clear_exchange_status(h);
            self.clear_exchange_status(x);
            return;
        }
        let (hname, xname) = (self.client(h).unwrap().char_name.clone(), self.client(x).unwrap().char_name.clone());
        self.hand_over(x, h, gb, &hname);
        self.hand_over(h, x, ga, &xname);
        for i in [h, x] {
            let c = self.client_mut(i).unwrap();
            (c.exchange_mode, c.exchange_confirm, c.exchange_h, c.exchange_count) = (false, false, 0, 0);
            c.exchange_name.clear();
            // fix: the C++ reset the confirmer's m_exchangeCount twice and never the partner's
            c.exchange_items.iter_mut().for_each(|e| e.0 = -1);
        }
        self.notify(h, NOTIFY_EXCHANGEITEMCOMPLETE, &[], None);
        self.notify(x, NOTIFY_EXCHANGEITEMCOMPLETE, &[], None);
        self.calc_total_weight(h);
        self.calc_total_weight(x);
    }

    /// One side of the confirmed trade: `to` gets each item (bAddItem), `from` loses it (SetItemCount for a
    /// stack, else ReleaseItemHandler + NOTIFY_GIVEITEMFIN_ERASEITEM with the receiver's name).
    fn hand_over(&mut self, from: usize, to: usize, goods: Vec<(usize, i32, Item)>, to_name: &[u8]) {
        for (k, amount, item) in goods {
            let stack = stackable(&item);
            self.item_log(ITEMLOG_EXCHANGE, from, to as i32, &item, false);
            self.add_item(to, item);
            if stack {
                let n = self.client(from).unwrap().items[k].as_ref().map_or(0, |i| i.count as i64);
                self.set_item_count(from, k, (n - amount as i64).max(0) as u32);
            } else {
                self.release_item(from, k, true);
                self.notify(from, NOTIFY_GIVEITEMFIN_ERASEITEM, &[k as i32, amount], Some(to_name));
                let c = self.client_mut(from).unwrap();
                c.is_item_equipped[k] = false;
                c.items[k] = None;
                c.cs.arrow_index = crate::combat::arrow_index(c);
            }
        }
    }

    /// `_iGetItemSpaceLeft`
    pub fn item_space_left(&self, h: usize) -> usize {
        self.client(h).map_or(0, |c| c.items.iter().filter(|i| i.is_none()).count())
    }

    /// `CancelExchangeItem`; also the exchange part of DeleteClient, KilledHandler and RequestTeleportHandler.
    pub fn cancel_exchange_item(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        let x = c.exchange_h;
        self.clear_exchange_status(x);
        self.clear_exchange_status(h);
    }

    /// The exchange cancel of DeleteClient / KilledHandler / RequestTeleportHandler (only in exchange mode).
    pub fn cancel_exchange_if_open(&mut self, h: usize) {
        if self.client(h).is_some_and(|c| c.exchange_mode) {
            self.cancel_exchange_item(h);
        }
    }

    /// `_ClearExchangeStatus`
    pub fn clear_exchange_status(&mut self, h: usize) {
        if h == 0 {
            return;
        }
        let Some(c) = self.client(h) else { return };
        if c.exchange_mode {
            self.notify(h, NOTIFY_CANCELEXCHANGEITEM, &[], None);
        }
        let c = self.client_mut(h).unwrap();
        (c.exchange_mode, c.exchange_confirm, c.exchange_h) = (false, false, 0);
        c.exchange_name.clear();
    }
}

fn name10(n: &[u8]) -> [u8; 10] {
    let mut a = [0u8; 10];
    let s = cstr(n);
    a[..s.len().min(10)].copy_from_slice(&s[..s.len().min(10)]);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_weights() {
        // A can carry 100, B 50; A gives 30 + 30, B gives 60 + 50
        assert!(!weights_fit(100, 50, &[30, 30], &[60, 50])); // B cannot carry 60, A cannot carry 110
        // the C++ counted only the last item: 30 <= 50 and 50 <= 100 passed although B gets 60
        assert!(!weights_fit(100, 50, &[30, 30], &[50]));
        assert!(weights_fit(100, 60, &[30, 30], &[50, 50]));
        assert!(weights_fit(0, 0, &[], &[]));
        assert_eq!(name10(b"RtHg1\0xx"), *b"RtHg1\0\0\0\0\0");
    }
}
