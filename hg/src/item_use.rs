//! The item layer's player commands (HG.cpp): COMMONTYPE_EQUIPITEM / RELEASEITEM (bEquipItemHandler is in
//! entry.rs, ReleaseItemHandler in combat.rs), UseItemHandler + _bDepleteDestTypeItemUseEffect, SetItemCount,
//! GiveItemHandler (to players, NPCs and the warehouse keeper: bSetItemToBankItem) and RequestRetrieveItemHandler.
use crate::client::{Item, MAGICTYPE_BERSERK, MAGICTYPE_ICE, MAGICTYPE_INVISIBILITY, MAGICTYPE_POISON, MAXBANKITEMS, MAXITEMS, STATUS_BERSERK, STATUS_POISON};
use crate::combat::{arrow_index, ITEMTYPE_ARROW};
use crate::consts::*;
use crate::game::Game;
use crate::life::ITEMTYPE_CONSUME;
use crate::map::OWNERTYPE_PLAYER;
use crate::util::{cstr, Rd, W};

pub const ITEMTYPE_USE_DEPLETE: i32 = 3;
pub const ITEMTYPE_EAT: i32 = 7;
pub const ITEMTYPE_USE_SKILL: i32 = 8;
pub const ITEMTYPE_USE_PERM: i32 = 9;
pub const ITEMTYPE_USE_DEPLETE_DEST: i32 = 11;
pub const ITEMTYPE_RELIC: i32 = 13;

/// The dice of an HP/MP/SP potion: the item's own spec values when set (a crafted potion), else the config's.
pub fn potion_dice(it: &Item) -> (i32, i32, i32) {
    let s = it.item_spec_effect_value;
    if s[0] == 0 {
        let v = it.cfg.item_effect_value;
        (v[0], v[1], v[2])
    } else {
        (s[0] as i32, s[1] as i32, s[2] as i32)
    }
}

/// A point refill: `cur` + the roll, capped at `max`; HP never drops below 1.
pub fn refill(cur: i32, roll: i32, max: i32, hp: bool) -> i32 {
    let v = (cur + roll).min(max);
    if hp { v.max(1) } else { v }
}

/// The 20-byte, NUL-padded item name the C++ compares with memcmp(.., 20).
pub fn name20(n: &[u8]) -> [u8; 20] {
    let mut b = [0u8; 20];
    let n = cstr(n);
    let l = n.len().min(20);
    b[..l].copy_from_slice(&n[..l]);
    b
}

/// The map number NOTIFY_SHOWMAP sends for a map name (0 = no map of it).
pub fn show_map_index(map: &[u8]) -> i32 {
    let names: [&[u8]; 10] = [b"aresden", b"elvine", b"middleland", b"default", b"huntzone2", b"huntzone1", b"huntzone4", b"huntzone3", b"arefarm", b"elvfarm"];
    names.iter().position(|n| cstr(map) == *n).map_or(0, |i| i as i32 + 1)
}

impl Game {
    /// `SetItemCount` (by slot): 0 removes the item (ItemDepleteHandler), otherwise NOTIFY_SETITEMCOUNT.
    pub fn set_item_count(&mut self, h: usize, k: usize, count: u32) {
        let Some(c) = self.client_mut(h) else { return };
        let Some(it) = c.items.get_mut(k).and_then(|i| i.as_mut()) else { return };
        if count == 0 {
            self.item_deplete(h, k, false);
        } else {
            it.count = count;
            self.notify(h, NOTIFY_SETITEMCOUNT, &[k as i32, count as i32, 1], None);
        }
    }

    /// `dwGetItemCount(name)`: the count of the first stack of that name.
    pub fn item_count(&self, h: usize, name: &[u8]) -> (Option<usize>, u32) {
        let Some(c) = self.client(h) else { return (None, 0) };
        let n = name20(name);
        match c.items.iter().position(|i| i.as_ref().is_some_and(|i| name20(&i.cfg.name) == n)) {
            Some(k) => (Some(k), c.items[k].as_ref().unwrap().count),
            None => (None, 0),
        }
    }

    /// `UseItemHandler`
    pub fn use_item(&mut self, h: usize, k: i32, dx: i16, dy: i16, dest: i32) {
        let now = self.now();
        let Some(c) = self.client(h) else { return };
        if c.is_killed || !c.is_init_complete || k < 0 || k as usize >= MAXITEMS {
            return;
        }
        let k = k as usize;
        let Some(it) = c.items[k].clone() else { return };
        let t = it.cfg.item_type;
        let v = it.cfg.item_effect_value;
        match t {
            ITEMTYPE_USE_DEPLETE | ITEMTYPE_EAT => {}
            ITEMTYPE_USE_DEPLETE_DEST => {
                if self.deplete_dest_effect(h, dx, dy, k, dest) {
                    self.item_deplete(h, k, true);
                }
                return;
            }
            ITEMTYPE_ARROW => {
                let c = self.client_mut(h).unwrap();
                c.cs.arrow_index = arrow_index(c);
                return;
            }
            ITEMTYPE_USE_PERM => {
                if it.cfg.item_effect_type == ITEMEFFECTTYPE_SHOWLOCATION && v[0] == 1 {
                    let m = show_map_index(&c.map_name);
                    self.notify(h, NOTIFY_SHOWMAP, &[1, m], None);
                }
                return;
            }
            ITEMTYPE_USE_SKILL => {
                self.use_skill_item(h, k, dx, dy);
                return;
            }
            _ => return,
        }
        let mut result = 0;
        match it.cfg.item_effect_type {
            ITEMEFFECTTYPE_WARM => {
                if c.magic_effect_status[MAGICTYPE_ICE as usize] == 1 {
                    self.remove_delay_events(h, OWNERTYPE_PLAYER, MAGICTYPE_ICE);
                    self.register_delay_event(DELAYEVENTTYPE_MAGICRELEASE, MAGICTYPE_ICE, now.wrapping_add(1000), h, OWNERTYPE_PLAYER);
                }
                self.client_mut(h).unwrap().cs.warm_time = now.max(1);
            }
            ITEMEFFECTTYPE_LOTTERY => {
                // the C++ rolls twice and does nothing with the result
                self.rng.dice(1, it.cfg.special_effect_value1);
                self.rng.dice(1, it.cfg.special_effect_value1);
            }
            ITEMEFFECTTYPE_SLATES if it.cfg.id_num == 867 && self.ast.ev.is_some() && self.is_astoria_map(c.map_index) => return,
            ITEMEFFECTTYPE_SLATES if it.cfg.id_num == 867 && matches!(it.item_spec_effect_value[1], 1 | 3 | 4) => {
                // invincible (1), mana (3), exp (4): 10 minutes
                let ty = it.item_spec_effect_value[1] as i32;
                self.set_slate_flag(h, ty, true);
                self.register_delay_event(DELAYEVENTTYPE_ANCIENT_TABLET, ty, now.wrapping_add(600_000), h, OWNERTYPE_PLAYER);
                result = match ty { 1 => 4, 3 => 5, _ => 6 };
            }
            ITEMEFFECTTYPE_SLATES => {
                // ITEM_ANCIENTTABLET: the berserk slate (2)
                if it.cfg.id_num == 867 && it.item_spec_effect_value[1] == 2 {
                    if c.magic_effect_status[MAGICTYPE_BERSERK as usize] != 0 {
                        self.remove_delay_events(h, OWNERTYPE_PLAYER, MAGICTYPE_BERSERK);
                    }
                    let c = self.client_mut(h).unwrap();
                    c.magic_effect_status[MAGICTYPE_BERSERK as usize] = 1;
                    c.set_status_flag(STATUS_BERSERK, true);
                    self.register_delay_event(DELAYEVENTTYPE_MAGICRELEASE, MAGICTYPE_BERSERK, now.wrapping_add(600_000), h, OWNERTYPE_PLAYER);
                    self.notify(h, NOTIFY_MAGICEFFECTON, &[MAGICTYPE_BERSERK, 1], None);
                }
            }
            e @ (ITEMEFFECTTYPE_HP | ITEMEFFECTTYPE_MP | ITEMEFFECTTYPE_SP) => {
                let (cur, max) = match e {
                    ITEMEFFECTTYPE_HP => (c.hp, c.get_max_hp()),
                    ITEMEFFECTTYPE_MP => (c.mp, c.get_max_mp()),
                    _ => (c.sp, c.get_max_sp()),
                };
                let poisoned = c.cs.is_poisoned;
                if cur < max {
                    let (a, b, d) = potion_dice(&it);
                    let roll = self.rng.dice(a, b) + d;
                    let c = self.client_mut(h).unwrap();
                    match e {
                        ITEMEFFECTTYPE_HP => (c.hp, result) = (refill(cur, roll, max, true), 1),
                        ITEMEFFECTTYPE_MP => (c.mp, result) = (refill(cur, roll, max, false), 2),
                        _ => (c.sp, result) = (refill(cur, roll, max, false), 3),
                    }
                }
                if e == ITEMEFFECTTYPE_SP && poisoned {
                    let c = self.client_mut(h).unwrap();
                    c.cs.is_poisoned = false;
                    c.set_status_flag(STATUS_POISON, false);
                    self.notify(h, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_POISON], None);
                }
            }
            ITEMEFFECTTYPE_HPSTOCK => {
                // food: two separate rolls, one for the HP stock and one for the hunger
                let s = self.rng.dice(v[0], v[1]) + v[2];
                let f = self.rng.dice(v[0], v[1]) + v[2];
                let c = self.client_mut(h).unwrap();
                c.cs.hp_stock = (c.cs.hp_stock + s).clamp(0, 500);
                c.hunger_status = (c.hunger_status + f).clamp(0, 100);
            }
            ITEMEFFECTTYPE_STUDYSKILL => {
                let sev1 = it.item_spec_effect_value[0] as i32;
                self.train_skill(h, v[0], if sev1 == 0 { v[1] } else { sev1 });
            }
            ITEMEFFECTTYPE_STUDYMAGIC => {
                if let Some(name) = self.data.magic.get(v[0].max(0) as usize).and_then(|r| r.as_ref()).and_then(|f| f.get(1)).cloned() {
                    self.study_magic(h, &name, false);
                }
            }
            ITEMEFFECTTYPE_MAGIC => {
                let (x, y, mi, map_name) = (c.x as i32, c.y as i32, c.map_index, c.map_name.clone());
                self.remove_magic_effect(h, OWNERTYPE_PLAYER, MAGICTYPE_INVISIBILITY);
                let fight_zone = self.maps[mi].is_fight_zone;
                match v[0] {
                    1 => self.request_teleport(h, 1, None, -1, -1), // recall scroll
                    2 => self.player_magic(h, x, y, 32, true, 0),   // invisibility scroll
                    3 if !fight_zone => self.player_magic(h, x, y, 34, true, 0),
                    4 => {
                        let dest: Option<Vec<u8>> = match v[1] {
                            1 => Some(b"bisle".to_vec()),
                            11..=19 => {
                                // a fight-zone ticket: valid on its day, before its hour
                                let t = chrono_now();
                                let tv = it.touch_effect_value;
                                (tv[0] as u32 == t.0 && tv[1] as u32 == t.1 && (tv[2] as u32) > t.2).then(|| format!("fightzone{}", v[1] - 10).into_bytes())
                            }
                            _ => None,
                        };
                        if let Some(d) = dest.filter(|d| cstr(&map_name) != &d[..]) {
                            self.item_deplete(h, k, true);
                            self.request_teleport(h, 3, Some(&d), -1, -1);
                        }
                    }
                    5 => {
                        // the heldenian summon scrolls (ITEMTYPE_USE_PERM in the C++ config)
                        self.use_heldenian_scroll(h, k);
                        return;
                    }
                    _ => {}
                }
            }
            ITEMEFFECTTYPE_FIRMSTAMINAR => {
                let c = self.client_mut(h).unwrap();
                c.time_left_firm_stamina = (c.time_left_firm_stamina + v[0]).min(20 * 30);
            }
            ITEMEFFECTTYPE_CHANGEATTR => {
                let c = self.client_mut(h).unwrap();
                let type_of = |sex: u8, skin: u8| if sex == 1 { 1 } else { 4 } + skin.saturating_sub(1).min(2) as i16;
                match v[0] {
                    1 => c.hair_color = (c.hair_color + 1) % 16,
                    2 => c.hair_style = (c.hair_style + 1) % 8,
                    3 => {
                        c.skin = if c.skin >= 3 { 1 } else { c.skin + 1 };
                        c.type_ = type_of(c.sex, c.skin);
                    }
                    4 if c.appr[2] as u16 & 0xFF0F == 0 => {
                        // sex change, only with nothing on the body
                        c.sex = if c.sex == 1 { 2 } else { 1 };
                        c.type_ = type_of(c.sex, c.skin);
                    }
                    5 => c.underwear = (c.underwear + 1) % 8,
                    _ => {}
                }
                if matches!(v[0], 1 | 2 | 4 | 5) {
                    c.appr[0] = ((c.hair_style as i16) << 8) | ((c.hair_color as i16) << 4) | c.underwear as i16;
                }
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
            }
            _ => {}
        }
        if self.client(h).is_none() {
            return;
        }
        // (the C++ depletes again after a teleport scroll already used itself up: a no-op on an empty slot)
        self.item_deplete(h, k, true);
        match result {
            1 => self.notify(h, NOTIFY_HP, &[], None),
            2 => self.notify(h, NOTIFY_MP, &[], None),
            3 => self.notify(h, NOTIFY_SP, &[], None),
            4 => self.notify(h, NOTIFY_SLATE_INVINCIBLE, &[], None),
            5 => self.notify(h, NOTIFY_SLATE_MANA, &[], None),
            6 => self.notify(h, NOTIFY_SLATE_EXP, &[], None),
            _ => {}
        }
    }

    /// `_bDepleteDestTypeItemUseEffect`: dyes, construction kits, occupy flags and farming seed bags (any other
    /// effect is used up for nothing, like the C++ default).
    fn deplete_dest_effect(&mut self, h: usize, dx: i16, dy: i16, k: usize, dest: i32) -> bool {
        let c = self.client_mut(h).unwrap();
        let Some(it) = c.items[k].as_ref() else { return false };
        if it.cfg.item_effect_type == ITEMEFFECTTYPE_OCCUPYFLAG {
            return self.use_occupy_flag(h, k, dx, dy);
        }
        if it.cfg.item_effect_type == ITEMEFFECTTYPE_ITEMFARM {
            let (ty, ssn) = (it.cfg.item_effect_value[0], it.cfg.item_effect_value[1]);
            return self.set_agriculture_item(h, dx, dy, ty, ssn);
        }
        if it.cfg.item_effect_type == ITEMEFFECTTYPE_CONSTRUCTIONKIT {
            // a kit is used up when its last use (life span) goes
            let ty = it.cfg.item_effect_value[0];
            if !self.set_construction_kit(h, dx, dy, ty) {
                return false;
            }
            let it = self.client_mut(h).unwrap().items[k].as_mut().unwrap();
            it.cur_life_span = it.cur_life_span.wrapping_sub(1);
            return it.cur_life_span == 0;
        }
        if it.cfg.item_effect_type != ITEMEFFECTTYPE_DYE {
            return true;
        }
        let color = it.cfg.item_effect_value[0];
        // fix: the C++ used the dye up when the target slot was out of range or empty
        let Some(d) = (0..MAXITEMS as i32).contains(&dest).then(|| c.items[dest as usize].as_mut()).flatten() else { return false };
        if d.cfg.category == 11 || d.cfg.category == 12 {
            d.item_color = color as u8;
            self.notify(h, NOTIFY_ITEMCOLORCHANGE, &[dest, color & 0xFF], None);
            true
        } else {
            self.notify(h, NOTIFY_ITEMCOLORCHANGE, &[dest, -1], None);
            false
        }
    }

    /// `GiveItemHandler`: an item (or part of a stack) handed to the player / NPC on (dx, dy); the warehouse
    /// keeper ("Howard") banks it; anything else lets it fall. A guild admission / secession ticket given to a guild
    /// master asks to join / leave (guild.rs); a secession ticket given to Kennedy leaves the guild at once.
    #[allow(clippy::too_many_arguments)]
    pub fn give_item(&mut self, h: usize, k: i32, amount: i32, dx: i16, dy: i16, obj: u16, name: &[u8]) {
        let Some(c) = self.client(h) else { return };
        // fix: the C++ read m_pItemList[sItemIndex] before checking the index
        if c.is_on_waiting_process || !c.is_init_complete || k < 0 || k as usize >= MAXITEMS || amount <= 0 {
            return;
        }
        let k = k as usize;
        let Some(it) = c.items[k].as_ref() else { return };
        let m = c.map_index;
        let same_map = if (obj as i32) < MAXCLIENTS {
            self.client(obj as usize).is_none_or(|o| o.map_index == m)
        } else if obj > 10000 && ((obj - 10000) as usize) < self.npcs.len() {
            self.npcs[(obj - 10000) as usize].as_ref().is_none_or(|n| n.map_index == m)
        } else {
            true
        };
        if !same_map || name20(&it.cfg.name) != name20(name) || it.cfg.item_type == ITEMTYPE_RELIC {
            return;
        }
        let (x, y) = (c.x, c.y);
        let stack = matches!(it.cfg.item_type, ITEMTYPE_CONSUME | ITEMTYPE_ARROW) && it.count > amount as u32;
        let (count, cfg_name) = (it.count, it.cfg.name.clone());
        // GetOwner on the target tile, kept only when it is the object the client named
        let mut owner = self.maps[m].tile(dx, dy).and_then(|t| (t.owner != 0).then_some((t.owner as usize, t.owner_class)));
        if obj != 0 {
            let named = if obj < 10000 { obj as usize } else { (obj - 10000) as usize };
            let exists = if obj < 10000 { self.client(named).is_some() } else { self.npcs.get(named).is_some_and(|n| n.is_some()) };
            if exists && owner.is_some_and(|(o, _)| o != named) {
                owner = None;
            }
        }
        let item = if stack {
            let Some(cfg) = self.data.item_by_name(&cfg_name).cloned() else { return };
            let mut part = Item::new(&cfg);
            part.count = amount as u32;
            self.set_item_count(h, k, count - amount as u32);
            part
        } else {
            self.release_item(h, k, true);
            let c = self.client_mut(h).unwrap();
            c.is_item_equipped[k] = false;
            c.items[k].take().unwrap()
        };
        let mut to_name = Vec::new();
        let mut ticket = false;
        match owner {
            None => {
                self.item_log(ITEMLOG_DROP, h, -1, &item, false);
                self.drop_ground_item(m, x, y, item);
                if !stack {
                    self.notify(h, NOTIFY_DROPITEMFIN_ERASEITEM, &[k as i32, amount], None);
                }
            }
            Some((o, OWNERTYPE_PLAYER)) => {
                to_name = self.client(o).map(|c| cstr(&c.char_name).to_vec()).unwrap_or_default();
                if o == h {
                    // fix: the C++ deleted the split-off part of a stack given to oneself (the amount was lost) and
                    // moved a whole item to another slot; here nothing changes
                    if stack {
                        self.set_item_count(h, k, count);
                    } else {
                        self.client_mut(h).unwrap().items[k] = Some(item);
                        self.calc_total_item_effect(h, k as i32, true);
                    }
                    return;
                }
                if !stack && self.guild_ticket(h, o, item.cfg.id_num) {
                    self.item_log(ITEMLOG_DEPLETE, h, -1, &item, false);
                    ticket = true;
                } else if self.add_client_item_list(o, &item) {
                    self.item_log(ITEMLOG_GIVE, h, o as i32, &item, false);
                    self.send_item_obtained(o, &item);
                } else {
                    self.item_log(ITEMLOG_DROP, h, -1, &item, false);
                    self.drop_ground_item(m, x, y, item);
                    self.send_cannot_carry(o);
                    if stack {
                        self.notify(h, NOTIFY_CANNOTGIVEITEM, &[k as i32, amount], Some(&to_name));
                        self.calc_total_weight(h);
                        return;
                    }
                    to_name.clear();
                }
            }
            Some((o, _)) => {
                let npc_name = self.npcs.get(o).and_then(|n| n.as_ref()).map(|n| cstr(&n.npc_name).to_vec()).unwrap_or_default();
                to_name = npc_name.clone();
                if npc_name.starts_with(b"Howard") {
                    if let Err(item) = self.item_to_bank(h, item) {
                        self.notify(h, NOTIFY_CANNOTITEMTOBANK, &[], None);
                        self.item_log(ITEMLOG_DROP, h, -1, &item, false);
                        self.drop_ground_item(m, x, y, item);
                    }
                } else if !stack && npc_name.starts_with(b"Kennedy") && !self.cr.mode && item.cfg.id_num == crate::guild::ITEM_GUILDSECESSIONTICKET {
                    // the ticket is used up (a member, not the master, leaves his guild)
                    self.kennedy_leave_guild(h);
                } else {
                    self.item_log(ITEMLOG_DROP, h, -1, &item, false);
                    self.drop_ground_item(m, x, y, item);
                    if !stack {
                        to_name.clear();
                    }
                }
            }
        }
        if stack {
            if owner.is_some() {
                self.notify(h, NOTIFY_GIVEITEMFIN_COUNTCHANGED, &[k as i32, amount], Some(&to_name));
            }
        } else {
            if owner.is_some() {
                // (a guild ticket's erase notice says 1 whatever the client asked)
                self.notify(h, NOTIFY_GIVEITEMFIN_ERASEITEM, &[k as i32, if ticket { 1 } else { amount }], Some(&to_name));
            }
            let c = self.client_mut(h).unwrap();
            c.cs.arrow_index = arrow_index(c);
        }
        self.calc_total_weight(h);
    }

    /// `bSetItemToBankItem`: the first free bank slot, then NOTIFY_ITEMTOBANK (slot, WriteItemData, effect value
    /// 2, attribute, spec value 2). Gives the item back when the player is not in a warehouse or the bank is full.
    pub fn item_to_bank(&mut self, h: usize, it: Item) -> Result<(), Item> {
        let Some(c) = self.client_mut(h) else { return Err(it) };
        if !c.is_on_warehouse {
            return Err(it);
        }
        let Some(i) = c.bank.iter().position(|b| b.is_none()) else { return Err(it) };
        let mut b = vec![0u8; 56];
        let mut w = W(&mut b);
        w.u32(0, MSGID_NOTIFY);
        w.u16(4, NOTIFY_ITEMTOBANK as u16);
        w.u8(6, i as u8);
        w.u8(7, 1);
        crate::entry::item_entry(&mut w, 8, &it, Some(false));
        w.u16(47, it.cfg.item_effect_value[1] as u16);
        w.u32(49, it.attribute);
        w.u8(53, it.item_spec_effect_value[1] as u8);
        // fix: the C++ sent 56 bytes of a 54-byte message (2 uninitialised stack bytes; zeros here)
        w.u16(54, 0);
        // (ITEMLOG_DEPOSIT is TAIWANLOG-only in the C++)
        c.bank[i] = Some(it);
        c.is_bank_modified = true;
        self.calc_total_weight(h);
        self.send(h, &b);
        Ok(())
    }

    /// `RequestRetrieveItemHandler` (MSGID_REQUEST_RETRIEVEITEM): a bank item back into the bag (stacking
    /// consumables), the bank closes its gap, RESPONSE_RETRIEVEITEM confirm (bank slot, bag slot) or reject.
    pub fn retrieve_item(&mut self, h: usize, d: &[u8]) {
        let bi = Rd::new(d).u8(6) as i8;
        let Some(c) = self.client(h) else { return };
        if !c.is_on_warehouse {
            return;
        }
        let reply = |g: &mut Game, ok: Option<(usize, usize)>| {
            let mut b = vec![0u8; 8];
            let mut w = W(&mut b);
            w.u32(0, MSGID_RESPONSE_RETRIEVEITEM);
            if let Some((bi, k)) = ok {
                w.u16(4, MSGTYPE_CONFIRM as u16);
                w.u8(6, bi as u8);
                w.u8(7, k as u8);
            } else {
                w.u16(4, MSGTYPE_REJECT as u16);
            }
            g.send(h, &b);
        };
        // fix: the C++ indexed the bank with an unchecked (signed) client byte
        let Some(it) = (bi >= 0 && (bi as usize) < MAXBANKITEMS).then(|| c.bank[bi as usize].clone()).flatten() else {
            reply(self, None);
            return;
        };
        let bi = bi as usize;
        let max_load = c.get_str() * 500 + c.level * 500;
        if it.weight_of(it.count as i64) + c.cur_weight_load > max_load {
            self.send_cannot_carry(h);
            // fix: the C++ said so and handed the item over anyway (no return), past the weight limit
            return;
        }
        // only Zem is logged (forced): ITEMLOG_RETRIEVE is otherwise refused by _bItemLog
        // (the C++ logs the bag item after the move: the whole stack when it stacked)
        let zem = it.cfg.id_num == 650;
        let stack = matches!(it.cfg.item_type, ITEMTYPE_CONSUME | ITEMTYPE_ARROW);
        let same = stack.then(|| c.items.iter().position(|i| i.as_ref().is_some_and(|i| i.cfg.item_type == it.cfg.item_type && name20(&i.cfg.name) == name20(&it.cfg.name)))).flatten();
        let k = if let Some(k) = same {
            let n = c.items[k].as_ref().unwrap().count + it.count;
            self.set_item_count(h, k, n);
            k
        } else if let Some(k) = c.items.iter().position(|i| i.is_none()) {
            let c = self.client_mut(h).unwrap();
            c.items[k] = Some(it);
            c.item_pos[k] = (40, 30);
            c.is_item_equipped[k] = false;
            k
        } else {
            reply(self, None);
            return;
        };
        if zem {
            let it = self.client(h).unwrap().items[k].clone().unwrap();
            self.item_log(ITEMLOG_RETRIEVE, h, -1, &it, true);
        }
        let c = self.client_mut(h).unwrap();
        c.bank.remove(bi);
        c.bank.push(None);
        c.is_bank_modified = true;
        c.cs.arrow_index = arrow_index(c);
        self.calc_total_weight(h);
        reply(self, Some((bi, k)));
    }
}

/// Local (month, day, hour), as GetLocalTime gives the C++.
pub fn chrono_now() -> (u32, u32, u32) {
    let t = crate::chrono_now();
    (t.month, t.day, t.hour)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn potions_and_names() {
        assert_eq!(refill(10, 25, 30, true), 30);
        assert_eq!(refill(10, -20, 30, true), 1);
        assert_eq!(refill(10, -20, 30, false), -10);
        let mut it = Item::default();
        it.cfg.item_effect_value = [3, 8, 10, 0, 0, 0];
        assert_eq!(potion_dice(&it), (3, 8, 10));
        it.item_spec_effect_value = [5, 6, 7];
        assert_eq!(potion_dice(&it), (5, 6, 7));
        assert_eq!(name20(b"Gold\0junk"), name20(b"Gold"));
        assert_eq!((show_map_index(b"aresden"), show_map_index(b"elvfarm"), show_map_index(b"bisle")), (1, 10, 0));
    }
}
