//! Items on the ground: `CMap::bSetItem` / `pGetItem` (12 per tile, newest on top), `SendEventToNearClient_TypeB`,
//! `iClientMotion_GetItem_Handler`, and the NPC drops (`NpcDeadItemGenerator`, the `DeleteNpc` rolls).
//! `AddGroundItem` / `TileCleaner` (NPC drops vanish after 15 min, slates after 30), `DropItemHandler` and
//! `PenaltyItemDrop`. Not ported: item logs, the astoria relic.
//! `NpcDeadItemGenerator` lives in drops.rs.
use crate::client::{Item, MAXITEMS};
use crate::combat::{arrow_index, ITEMTYPE_ARROW};
use crate::life::ITEMTYPE_CONSUME;

const ITEM_RELIC: i32 = 1005;
use crate::consts::*;
use crate::game::Game;
use crate::util::{Rd, W};

/// Most tiles one MSGID_REQUEST_GROUNDITEMNAMES may ask for.
pub const GROUNDNAMES_MAX: usize = 24;

/// MSGID_RESPONSE_GROUNDITEMNAMES: `[id u32][type u16][count u8]` then per tile `[x i16][y i16][name 20][attribute u32][count u32]`
/// (a zeroed name = nothing lies there any more). The client turns name + attribute into the display name.
pub fn encode_ground_names(rows: &[(i16, i16, Option<&Item>)]) -> Vec<u8> {
    let mut b = vec![0u8; 7 + rows.len() * 32];
    let mut w = W(&mut b);
    w.u32(0, MSGID_RESPONSE_GROUNDITEMNAMES);
    w.u8(6, rows.len() as u8);
    for (k, (x, y, it)) in rows.iter().enumerate() {
        let o = 7 + k * 32;
        w.u16(o, *x as u16);
        w.u16(o + 2, *y as u16);
        if let Some(it) = it {
            w.bytes(o + 4, &it.cfg.name[..it.cfg.name.len().min(19)]);
            w.u32(o + 24, it.attribute);
            w.u32(o + 28, it.count);
        }
    }
    b
}

impl Game {
    /// v8.00 "ground item names" (no C++ original): the client asks for the top item of the tiles it draws one on;
    /// only tiles within the view are answered, at most `GROUNDNAMES_MAX` per request.
    pub fn request_ground_item_names(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        let (m, cx, cy) = (c.map_index, c.x, c.y);
        let r = Rd::new(d);
        let n = (r.u8(6) as usize).min(GROUNDNAMES_MAX);
        let tiles: Vec<(i16, i16)> = (0..n)
            .map(|k| (r.u16(7 + k * 4) as i16, r.u16(9 + k * 4) as i16))
            .filter(|&(x, y)| (x - cx).abs() <= VIEW_NEAR_X && (y - cy).abs() <= VIEW_NEAR_Y)
            .collect();
        if tiles.is_empty() {
            return;
        }
        let rows: Vec<(i16, i16, Option<&Item>)> = tiles.iter().map(|&(x, y)| (x, y, self.ground.get(&(m, x, y)).and_then(|v| v.first()))).collect();
        let b = encode_ground_names(&rows);
        self.send(h, &b);
    }

    /// `bSetItem` + the COMMONTYPE_ITEMDROP event. The tile's top item is what the client draws.
    pub fn drop_ground_item(&mut self, m: usize, x: i16, y: i16, it: Item) -> bool {
        if !self.maps[m].valid(x, y) {
            return false;
        }
        let v = self.ground.entry((m, x, y)).or_default();
        if v.first().is_some_and(|i| i.cfg.id_num == ITEM_RELIC) {
            return false; // a relic stays alone
        }
        if v.len() >= ITEMS_PER_TILE as usize {
            v.pop();
        }
        let top = (it.cfg.sprite as i16, it.cfg.sprite_frame as i16, it.item_color);
        v.insert(0, it);
        self.maps[m].tile_mut(x, y).unwrap().top_item = Some(top);
        self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_ITEMDROP as u16, m, x, y, [top.0, top.1, top.2 as i16, 0]);
        true
    }

    /// `pGetItem`: takes the top item; the tile then shows the next one.
    pub fn take_ground_item(&mut self, m: usize, x: i16, y: i16) -> Option<Item> {
        let v = self.ground.get_mut(&(m, x, y))?;
        if v.is_empty() {
            return None;
        }
        let it = v.remove(0);
        let top = v.first().map(|i| (i.cfg.sprite as i16, i.cfg.sprite_frame as i16, i.item_color));
        if v.is_empty() {
            self.ground.remove(&(m, x, y));
        }
        self.maps[m].tile_mut(x, y).unwrap().top_item = top;
        Some(it)
    }

    /// `SendEventToNearClient_TypeB` (18 bytes, to players within 10x8 of the spot).
    pub fn send_event_type_b(&mut self, msgid: u32, ty: u16, m: usize, x: i16, y: i16, v: [i16; 4]) {
        let key = (self.rng.rand() % 255) as u8 + 1;
        let mut b = vec![0u8; 18];
        let mut w = W(&mut b);
        w.u32(0, msgid);
        w.u16(4, ty);
        w.u16(6, x as u16);
        w.u16(8, y as u16);
        for (k, v) in v.iter().enumerate() {
            w.u16(10 + k * 2, *v as u16);
        }
        for h in self.handles() {
            let c = self.client(h).unwrap();
            if c.map_index == m && c.x >= x - VIEW_NEAR_X && c.x <= x + VIEW_NEAR_X && c.y >= y - 8 && c.y <= y + 8 {
                self.send_key(h, &b, key);
            }
        }
    }

    /// `iClientMotion_GetItem_Handler`: 1 = done, 2 = position mismatch.
    pub fn motion_get_item(&mut self, h: usize, sx: i16, sy: i16, dir: u8) -> i32 {
        let Some(c) = self.client(h) else { return 0 };
        if dir == 0 || dir > 8 || c.is_killed || !c.is_init_complete {
            return 0;
        }
        if (sx, sy) != (c.x, c.y) {
            return 2;
        }
        let m = c.map_index;
        self.inc_player_activity(h);
        self.pretend_corpse_reown(h, sx, sy, false);
        self.clear_skill_using_status(h);
        if let Some(it) = self.take_ground_item(m, sx, sy) {
            if self.add_client_item_list(h, &it) {
                if self.is_genuine_relic(&it) {
                    self.player_get_relic(h);
                }
                let top = self.maps[m].tile(sx, sy).and_then(|t| t.top_item).unwrap_or((0, 0, 0));
                self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_SETITEM as u16, m, sx, sy, [top.0, top.1, top.2 as i16, 0]);
                self.item_log(ITEMLOG_GET, h, -1, &it, false);
                self.send_item_obtained(h, &it);
            } else {
                // back on top of the pile, without an event (like the C++)
                self.ground.entry((m, sx, sy)).or_default().insert(0, it);
                let t = &self.ground[&(m, sx, sy)][0];
                let top = (t.cfg.sprite as i16, t.cfg.sprite_frame as i16, t.item_color);
                self.maps[m].tile_mut(sx, sy).unwrap().top_item = Some(top);
                self.send_cannot_carry(h);
            }
        }
        let mut b = vec![0u8; 6];
        W(&mut b).u32(0, MSGID_RESPONSE_MOTION);
        W(&mut b).u16(4, OBJECTMOTION_CONFIRM as u16);
        self.send(h, &b);
        1
    }

    /// The drops of `DeleteNpc` (`m_drops` is empty in the C++, so no secondary drop table): 1 in 50 gold
    /// (never, since the NPC gold value is never loaded) and a 60 in 100000 slate.
    pub fn npc_delete_drops(&mut self, i: usize) {
        // the 1/50 gold needs dwGoldDropValue > 0, which the C++ never loads (no dice drawn either)
        if self.rng.dice(1, 100000) < 60 {
            let id = [868, 869, 870, 871][self.rng.dice(1, 4) as usize - 1];
            if let Some(cfg) = self.data.item_by_id(id).cloned() {
                let mut it = Item::new(&cfg);
                it.count = 1;
                it.touch_effect_type = ITET_ID as i16;
                it.touch_effect_value = [self.rng.dice(1, 100000) as i16, self.rng.dice(1, 100000) as i16, self.now() as i16];
                let n = self.npcs[i].as_ref().unwrap();
                let (m, x, y, name) = (n.map_index, n.x, n.y, n.npc_name.clone());
                self.item_log_npc_drop(&name, &it);
                // fix: the C++ registered pItem (the other drop, usually NULL) with AddGroundItem, so slates never expired
                self.add_ground_item(m, x, y, it, TILECLEANTIMEPLAYER as u32);
            }
        }
    }

    /// `bSetItem` + ITEMDROP event + `AddGroundItem(dwTime)`: the item vanishes after `ms` (TileCleaner).
    pub fn add_ground_item(&mut self, m: usize, x: i16, y: i16, mut it: Item, ms: u32) {
        self.ground_tag = self.ground_tag.wrapping_add(1).max(1);
        it.ground_tag = self.ground_tag;
        if !self.drop_ground_item(m, x, y, it) {
            return;
        }
        if self.ground_timers.len() >= MAXGROUNDITEMS as usize {
            crate::log::log("WARNING: Ground NPC item list is full, item will not be cleared.");
            return;
        }
        let t = self.now().wrapping_add(ms);
        self.ground_timers.push((t, m, x, y, self.ground_tag));
    }

    /// `DropItemHandler`: puts bag item `k` on the player's tile. `amount` -1 = the whole stack; a part of a
    /// consumable/arrow stack splits it. `name` (20 bytes) must be the item's.
    pub fn drop_item(&mut self, h: usize, k: usize, amount: i32, name: &[u8]) {
        // shortcut: the death drops (PenaltyItemDrop, bByPlayer = FALSE: forced log) are the -1 amounts; a
        // client sending -1 only changes whether a small gold stack is logged
        let forced = amount == -1;
        let Some(c) = self.client(h) else { return };
        if c.is_on_waiting_process || !c.is_init_complete || k >= MAXITEMS {
            return;
        }
        let Some(it) = c.items[k].as_ref() else { return };
        if amount < -1 {
            return;
        }
        let stack = matches!(it.cfg.item_type, ITEMTYPE_CONSUME | ITEMTYPE_ARROW);
        let amount = if stack && amount == -1 { it.count as i32 } else { amount };
        let pad = |n: &[u8]| {
            let mut b = [0u8; 20];
            let n = crate::util::cstr(n);
            b[..n.len().min(20)].copy_from_slice(&n[..n.len().min(20)]);
            b
        };
        if pad(&it.cfg.name) != pad(name) {
            return;
        }
        let (m, x, y) = (c.map_index, c.x, c.y);
        if self.ground.get(&(m, x, y)).and_then(|v| v.first()).is_some_and(|i| i.cfg.id_num == ITEM_RELIC) {
            return;
        }
        if stack && it.count as i64 - amount as i64 > 0 {
            let Some(cfg) = self.data.item_by_name(&it.cfg.name).cloned() else { return };
            if amount <= 0 {
                return;
            }
            let mut part = Item::new(&cfg);
            part.count = amount as u32;
            let left = it.count - amount as u32;
            // SetItemCount (left > 0 here)
            self.client_mut(h).unwrap().items[k].as_mut().unwrap().count = left;
            self.notify(h, NOTIFY_SETITEMCOUNT, &[k as i32, left as i32, 1], None);
            self.item_log(ITEMLOG_DROP, h, -1, &part, forced);
            self.add_ground_item(m, x, y, part, TILECLEANTIMEPLAYER as u32);
            self.notify(h, NOTIFY_DROPITEMFIN_COUNTCHANGED, &[k as i32, amount], None);
        } else {
            // the C++ then sent NOTIFY_ITEMRELEASED if the item was still equipped, which it never is here
            self.release_item(h, k, true);
            let c = self.client_mut(h).unwrap();
            let it = c.items[k].take().unwrap();
            c.is_item_equipped[k] = false;
            // a used-up alter-drop item vanishes; the event's relic stays where it falls (a fake one vanishes)
            let relic = if it.cfg.item_effect_type == ITEMEFFECTTYPE_ALTERITEMDROP && it.cur_life_span == 0 { Some(false) } else { self.drop_relic(h, &it) };
            if relic == Some(true) {
                self.item_log(ITEMLOG_DROP, h, -1, &it, forced);
            }
            if relic.is_none() {
                self.item_log(ITEMLOG_DROP, h, -1, &it, forced);
                self.add_ground_item(m, x, y, it, TILECLEANTIMEPLAYER as u32);
            }
            self.notify(h, NOTIFY_DROPITEMFIN_ERASEITEM, &[k as i32, amount], None);
            let c = self.client_mut(h).unwrap();
            c.cs.arrow_index = arrow_index(c);
        }
        self.calc_total_weight(h);
    }

    /// `CClient::PenaltyItemDrop` (the two-argument form, ITEMDROP undefined): an alter-drop item (if worn) falls
    /// instead; otherwise `total` random bag items fall, except the player's own touched items, special-ability
    /// items unless the killer had an SA (1..5), and 1 in 10 for a lucky player.
    pub fn penalty_item_drop(&mut self, h: usize, total: i32, sa_attacked: bool) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        let alter = c.fx.alter_item_drop_index;
        if alter != -1 && c.items.get(alter as usize).is_some_and(|i| i.is_some()) {
            let found = if c.items[alter as usize].as_ref().unwrap().cfg.item_effect_type == ITEMEFFECTTYPE_ALTERITEMDROP {
                Some(alter as usize)
            } else {
                crate::log::log("Alter Drop Item Index Error1");
                c.items.iter().position(|i| i.as_ref().is_some_and(|i| i.cfg.item_effect_type == ITEMEFFECTTYPE_ALTERITEMDROP))
            };
            if let Some(k) = found {
                let c = self.client_mut(h).unwrap();
                let it = c.items[k].as_mut().unwrap();
                it.cur_life_span = it.cur_life_span.saturating_sub(1);
                let name = it.cfg.name.clone();
                self.drop_item(h, k, -1, &name);
                self.client_mut(h).unwrap().fx.alter_item_drop_index = -1;
                return;
            }
        }
        for _ in 0..total {
            let c = self.client(h).unwrap();
            let list: Vec<usize> = (0..MAXITEMS).filter(|&j| c.items[j].is_some()).collect();
            if list.is_empty() {
                return;
            }
            let k = list[self.rng.dice(1, list.len() as i32) as usize - 1];
            let c = self.client(h).unwrap();
            let it = c.items[k].as_ref().unwrap();
            let own = it.touch_effect_type != 0 && it.touch_effect_value == c.char_id_num;
            let sa_item = matches!(it.cfg.item_effect_type, ITEMEFFECTTYPE_ATTACK_SPECABLTY | ITEMEFFECTTYPE_DEFENSE_SPECABLTY) && !sa_attacked;
            let (lucky, name) = (c.fx.is_lucky_effect, it.cfg.name.clone());
            if own || sa_item || (lucky && self.rng.dice(1, 10) == 5) {
                continue;
            }
            self.drop_item(h, k, -1, &name);
        }
    }

    /// `TileCleaner` (every minute): removes the expired ground items still lying where they fell.
    pub fn tile_cleaner(&mut self) {
        let now = self.now();
        let (due, keep): (Vec<_>, Vec<_>) = self.ground_timers.drain(..).partition(|e| now.wrapping_sub(e.0) as i32 >= 0);
        self.ground_timers = keep;
        for (_, m, x, y, tag) in due {
            let Some(v) = self.ground.get_mut(&(m, x, y)) else { continue };
            let Some(k) = v.iter().position(|i| i.ground_tag == tag) else { continue };
            v.remove(k);
            let top = v.first().map(|i| (i.cfg.sprite as i16, i.cfg.sprite_frame as i16, i.item_color));
            if v.is_empty() {
                self.ground.remove(&(m, x, y));
            }
            self.maps[m].tile_mut(x, y).unwrap().top_item = top;
            // fix: the C++ sent SETITEM only when the tile became empty (a cleaned top item stayed drawn under
            // the next one) and also for items already picked up; here whenever the top item changed
            if k == 0 {
                let t = top.unwrap_or((0, 0, 0));
                self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_SETITEM as u16, m, x, y, [t.0, t.1, t.2 as i16, 0]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamedata::ItemCfg;

    #[test]
    fn ground_names_layout() {
        let cfg = ItemCfg { name: b"Dagger+1".to_vec(), ..Default::default() };
        let mut it = Item::new(&cfg);
        it.attribute = 0x1234_5678;
        it.count = 3;
        let b = encode_ground_names(&[(10, 20, Some(&it)), (11, 21, None)]);
        assert_eq!(b.len(), 7 + 64);
        assert_eq!(u32::from_le_bytes(b[0..4].try_into().unwrap()), MSGID_RESPONSE_GROUNDITEMNAMES);
        assert_eq!(b[6], 2);
        assert_eq!((u16::from_le_bytes([b[7], b[8]]), u16::from_le_bytes([b[9], b[10]])), (10, 20));
        assert_eq!(&b[11..19], b"Dagger+1");
        assert_eq!(b[19], 0);
        assert_eq!(u32::from_le_bytes(b[31..35].try_into().unwrap()), 0x1234_5678);
        assert_eq!(u32::from_le_bytes(b[35..39].try_into().unwrap()), 3);
        assert_eq!(&b[39 + 4..39 + 24], &[0u8; 20]); // the empty tile has no name
    }
}
