//! Slates (HG.cpp): SetSlateFlag, DELAYEVENTTYPE_ANCIENT_TABLET, ReqCreateSlateHandler. The flags act in
//! combat.rs / magic.rs (STATUS_REDSLATE: no damage, magic resisted; STATUS_BLUESLATE: free spells) and in
//! GetExp (STATUS_GREENSLATE: x3); the berserk slate is in item_use.rs.
use crate::client::*;
use crate::consts::*;
use crate::game::Game;

const ITEM_ANCIENTTABLET: i32 = 867;
const SLATE_PIECES: [i32; 4] = [868, 869, 870, 871]; // LU, LD, RU, RD

/// A slate type's status bit (1 invincible, 3 mana, 4 exp).
fn slate_bit(ty: i32) -> i32 {
    match ty {
        1 => STATUS_REDSLATE,
        3 => STATUS_BLUESLATE,
        4 => STATUS_GREENSLATE,
        _ => 0,
    }
}

/// The created slate's (type, colour) for ReqCreateSlateHandler's dice(1, 1000).
pub fn slate_kind(roll: i32) -> (i32, u8) {
    match roll {
        ..=49 => (1, 32),
        50..=249 => (2, 3),
        250..=749 => (4, 7),
        750..=949 => (3, 37),
        _ => (1, 32),
    }
}

impl Game {
    /// `SetSlateFlag` (on, or off for `ty`) + the OBJECTNULLACTION broadcast.
    pub fn set_slate_flag(&mut self, h: usize, ty: i32, on: bool) {
        let Some(c) = self.client_mut(h) else { return };
        if on {
            c.status |= slate_bit(ty);
        } else {
            // fix: the C++ cleared the first slate bit set (red, blue, green), whichever slate this event was for
            c.status &= !slate_bit(ty);
        }
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
    }

    /// DELAYEVENTTYPE_ANCIENT_TABLET: the slate's 10 minutes are over.
    pub fn slate_expired(&mut self, h: usize, ty: i32) {
        // fix: the C++ dereferenced a missing client and sent an uninitialised type when no slate bit was set
        if self.client(h).is_none() {
            return;
        }
        self.notify(h, NOTIFY_SLATE_STATUS, &[ty], None);
        self.set_slate_flag(h, ty, false);
    }

    /// `ReqCreateSlateHandler`: the four ancient tablet pieces (bag slots at byte 11..15) make a slate.
    pub fn create_slate(&mut self, h: usize, d: &[u8]) {
        if self.client(h).is_none_or(|c| c.is_on_server_change) {
            return;
        }
        let mut ok = self.rng.dice(1, 100) < self.settings.slate_success_rate;
        let mut slots = [0usize; 4];
        let mut have = [false; 4];
        for (i, slot) in slots.iter_mut().enumerate() {
            let k = d.get(11 + i).copied().unwrap_or(0xFF) as i8 as i32;
            let c = self.client(h).unwrap();
            // fix: the C++ read m_pItemList[k] before its (off-by-one) range check, with a signed byte index
            let Some(id) = (0..MAXITEMS as i32).contains(&k).then(|| c.items[k as usize].as_ref()).flatten().map(|it| it.cfg.id_num) else {
                self.notify(h, NOTIFY_SLATE_CREATEFAIL, &[], None);
                return;
            };
            *slot = k as usize;
            if let Some(p) = SLATE_PIECES.iter().position(|&x| x == id) {
                have[p] = true;
            }
        }
        let c = self.client(h).unwrap();
        if have.contains(&false) {
            return;
        }
        if c.admin_user_level > 3 {
            ok = true;
        }
        for k in slots {
            self.item_deplete(h, k, false);
        }
        if !ok {
            self.notify(h, NOTIFY_SLATE_CREATEFAIL, &[], None);
            return;
        }
        let (ty, color) = slate_kind(self.rng.dice(1, 1000));
        self.notify(h, NOTIFY_SLATE_CREATESUCCESS, &[ty], None);
        let Some(cfg) = self.data.item_by_id(ITEM_ANCIENTTABLET).cloned() else { return };
        let mut it = Item::new(&cfg);
        it.count = 1;
        it.touch_effect_type = ITET_ID as i16;
        let (a, b) = (self.rng.dice(1, 100000) as i16, self.rng.dice(1, 100000) as i16);
        it.touch_effect_value = [a, b, self.now() as i16];
        it.item_spec_effect_value[1] = ty as i16;
        it.item_color = color;
        self.item_log(ITEMLOG_GET, h, -1, &it, false);
        self.add_item(h, it);
    }
}

#[test]
fn slate_odds() {
    assert_eq!((slate_kind(1), slate_kind(49), slate_kind(50)), ((1, 32), (1, 32), (2, 3)));
    assert_eq!((slate_kind(250), slate_kind(749), slate_kind(750), slate_kind(950), slate_kind(1000)), ((4, 7), (4, 7), (3, 37), (1, 32), (1, 32)));
}
