//! Skills (HG.cpp, char/Client.cpp): CClient::CheckTotalSkillMasteryPoints (the 900-point cap),
//! SetDownSkillIndexHandler, TrainSkillResponse (skill manuals), UseSkillHandler (pretend corpse),
//! ClearSkillUsingStatus, the skill items (ITEMTYPE_USE_SKILL: DELAYEVENTTYPE_USEITEM_SKILL ->
//! iCalculateUseSkillItemEffect), mining (MineralGenerator, iCreateMineral, _CheckMiningAction, bDeleteMineral)
//! and fishing (FishGenerator, iCreateFish, bDeleteFish, iCheckFish, FishProcessor, ReqGetFishThisTimeHandler).
//! Taming (SKILLEFFECTTYPE_TAMING) is not ported: no skill in Skill.cfg has that type.
use crate::client::*;
use crate::combat::{SKILL_FISHING, SKILL_MINING, SKILL_PRETENDCORPSE};
use crate::consts::*;
use crate::delay::DelayEvent;
use crate::game::Game;
use crate::map::OWNERTYPE_PLAYER;
use crate::util::W;

const MAXENGAGINGFISH: i16 = 30;
const SKILLEFFECTTYPE_GET_FISH: i32 = 2;
const ITEM_FISH: i32 = 100;

pub struct Mineral {
    pub ty: i32,
    pub m: usize,
    pub difficulty: i32,
    pub remain: i32,
    pub dyn_obj: usize,
}

pub struct Fish {
    pub m: usize,
    pub item: Option<Item>,
    pub dyn_obj: usize,
    pub engaging: i16,
    pub difficulty: i32,
}

/// `CheckTotalSkillMasteryPoints` on (mastery, ssn): the (skill, new level) lowered to bring the total back to
/// 900, in order; None when nothing could be lowered (the total stays above).
pub fn skill_cap(mastery: &mut [u8], ssn: &mut [u32], down: i32, skill: usize) -> (Vec<(usize, i32)>, bool) {
    let mut out = Vec::new();
    let mut remain = mastery.iter().map(|&m| m as i32).sum::<i32>() - MAXSKILLPOINTS;
    while remain > 0 {
        let d = if down >= 0 && (down as usize) < mastery.len() && mastery[down as usize] > 0 {
            Some(down as usize)
        } else {
            // fix: the C++ searched only when a down skill was set (and at 0); with none set (-1, the default)
            // nothing was lowered and the total passed 900. Here the lowest-SSN skill of 21+ goes down then too.
            let mut best = None;
            let mut best_ssn = 99_999_999u32;
            for i in 0..mastery.len() {
                if mastery[i] >= 21 && i != skill && ssn[i] <= best_ssn {
                    best_ssn = ssn[i];
                    best = Some(i);
                }
            }
            best
        };
        let Some(d) = d else { return (out, false) };
        let pts = if mastery[d] <= 20 { mastery[d] as i32 } else { 1 };
        mastery[d] -= pts as u8;
        ssn[d] = crate::combat::skill_ssn_point(mastery[d] as i32 + 1) - 1;
        remain -= pts;
        out.push((d, pts));
    }
    (out, true)
}

/// The mineral a mining hit yields (`_CheckMiningAction`'s tables, same dice order) and its exp roll.
fn mined(r: &mut crate::rng::Rng, ty: i32) -> (i32, i32) {
    const COAL: i32 = 355;
    const IRON: i32 = 357;
    const BLONDE: i32 = 507;
    const SILVER: i32 = 356;
    const GOLD: i32 = 354;
    const MITHRAL: i32 = 508;
    const SAPPHIRE: i32 = 352;
    const CRYSTAL: i32 = 358;
    const EMERALD: i32 = 353;
    const RUBY: i32 = 351;
    const DIAMOND: i32 = 350;
    let item = match ty {
        1 => match r.dice(1, 6) {
            1..=4 => COAL,
            5 => IRON,
            _ => BLONDE,
        },
        2 => match r.dice(1, 7) {
            1..=3 => COAL,
            4..=6 => IRON,
            _ => {
                if r.dice(1, 3) == 2 { SILVER } else { BLONDE }
            }
        },
        3 => match r.dice(1, 6) {
            1 => COAL,
            2..=5 => IRON,
            _ => {
                if r.dice(1, 8) == 3 {
                    if r.dice(1, 2) == 1 { SILVER } else { BLONDE }
                } else {
                    IRON
                }
            }
        },
        4 => match r.dice(1, 6) {
            1 | 2 => COAL,
            3 => {
                if r.dice(1, 3) == 2 { SILVER } else { COAL }
            }
            4 | 5 => IRON,
            _ => {
                if r.dice(1, 8) == 3 {
                    if r.dice(1, 4) == 3 {
                        if r.dice(1, 4) < 3 { MITHRAL } else { GOLD }
                    } else {
                        SILVER
                    }
                } else if r.dice(1, 2) == 1 {
                    GOLD
                } else {
                    IRON
                }
            }
        },
        5 => {
            if r.dice(1, 19) == 3 { SAPPHIRE } else { CRYSTAL }
        }
        6 => match r.dice(1, 5) {
            1 => {
                if r.dice(1, 6) == 3 { EMERALD } else { CRYSTAL }
            }
            2 => {
                if r.dice(1, 6) == 3 { SAPPHIRE } else { CRYSTAL }
            }
            3 => {
                if r.dice(1, 6) == 3 { RUBY } else { CRYSTAL }
            }
            4 => CRYSTAL,
            _ => {
                if r.dice(1, 12) == 3 { DIAMOND } else { CRYSTAL }
            }
        },
        // the C++ keeps ITEM_COAL (no exp roll) for any other mineral type (none: types are 1..level <= 6)
        _ => return (COAL, 0),
    };
    let exp = match item {
        MITHRAL => r.dice(1, 15),
        GOLD => r.dice(1, 5),
        SILVER => r.dice(1, 4),
        CRYSTAL => r.dice(2, 3),
        SAPPHIRE | EMERALD | RUBY if ty == 6 => r.dice(2, 4),
        SAPPHIRE => r.dice(2, 3),
        DIAMOND => r.dice(2, 5),
        _ => r.dice(1, 3),
    };
    (item, exp)
}

/// `_bInitItemAttr`: a new item of count 1.
fn one(cfg: &crate::gamedata::ItemCfg) -> Item {
    let mut it = Item::new(cfg);
    it.count = 1;
    it
}

/// A mineral's (difficulty, remaining hits) by type (`iCreateMineral`).
fn mineral_stats(ty: i32) -> (i32, i32) {
    match ty {
        2 => (15, 15),
        3 => (20, 10),
        4 => (50, 8),
        5 => (70, 6),
        6 => (90, 4),
        _ => (10, 20),
    }
}

impl Game {
    /// `CClient::CheckTotalSkillMasteryPoints`
    pub fn check_total_skill_mastery(&mut self, h: usize, skill: usize) -> bool {
        let Some(c) = self.client_mut(h) else { return false };
        let (down, ok) = skill_cap(&mut c.skill_mastery, &mut c.skill_ssn, c.down_skill_index, skill);
        for (d, pts) in down {
            let c = self.client_mut(h).unwrap();
            for p in [EQUIPPOS_TWOHAND, EQUIPPOS_RHAND] {
                let w = c.equip_status[p as usize];
                if w != -1 && c.items[w as usize].as_ref().is_some_and(|i| i.cfg.related_skill == d as i32) {
                    c.hit_ratio = (c.hit_ratio - pts).max(0);
                }
            }
            let m = c.skill_mastery[d] as i32;
            self.notify(h, NOTIFY_SKILL, &[d as i32, m], None);
        }
        ok
    }

    /// `SetDownSkillIndexHandler`
    pub fn set_down_skill_index(&mut self, h: usize, s: i32) {
        let Some(c) = self.client_mut(h) else { return };
        if s < 0 || s as usize >= MAXSKILLTYPE {
            return;
        }
        if c.skill_mastery[s as usize] > 0 {
            c.down_skill_index = s;
        }
        let d = c.down_skill_index;
        self.notify(h, NOTIFY_DOWNSKILLINDEXSET, &[d], None);
    }

    /// `TrainSkillResponse(TRUE, ...)`: a skill manual raises the skill to `level` (never lowers it).
    pub fn train_skill(&mut self, h: usize, s: i32, level: i32) {
        let Some(c) = self.client_mut(h) else { return };
        if !c.is_init_complete || s < 0 || s as usize >= MAXSKILLTYPE || !(0..=100).contains(&level) {
            return;
        }
        if c.skill_mastery[s as usize] as i32 >= level {
            return;
        }
        c.skill_mastery[s as usize] = level as u8;
        self.check_total_skill_mastery(h, s as usize);
        let mut b = vec![0u8; 8];
        W(&mut b).u32(0, MSGID_NOTIFY);
        W(&mut b).u16(4, NOTIFY_SKILLTRAINSUCCESS as u16);
        b[6] = s as u8;
        b[7] = level as u8;
        // (ITEMLOG_SKILLLEARN is TAIWANLOG-only in the C++)
        self.send(h, &b);
    }

    /// `UseSkillHandler`: only pretend corpse acts here; the other skills just get marked in use.
    pub fn use_skill(&mut self, h: usize, s: i32) {
        let now = self.now();
        let Some(c) = self.client_mut(h) else { return };
        if !c.is_init_complete || s < 0 || s as usize >= MAXSKILLTYPE {
            return;
        }
        let Some(cfg) = self.data.skills[s as usize].clone() else { return };
        let c = self.client_mut(h).unwrap();
        if c.skill_using[s as usize] {
            return;
        }
        c.last_action_time = now;
        let level = c.skill_mastery[s as usize] as i32;
        if self.rng.dice(1, 100) > level {
            self.notify(h, NOTIFY_SKILLUSINGEND, &[0], None);
            return;
        }
        if cfg.type_ == SKILLEFFECTTYPE_PRETEND && cfg.value[0] == 1 {
            if self.is_relic_holder(h) {
                return;
            }
            let c = self.client(h).unwrap();
            let (m, x, y, admin) = (c.map_index, c.x, c.y, c.admin_user_level);
            if self.maps[m].is_fight_zone || self.maps[m].get_dead_owner(x, y).0 != 0 {
                self.notify(h, NOTIFY_SKILLUSINGEND, &[0], None);
                return;
            }
            if admin <= 0 {
                let near: i32 = [(0, -1), (0, 1), (-1, 0), (1, 0), (-1, -1), (1, -1), (-1, 1), (1, 1)]
                    .iter()
                    .map(|&(ox, oy)| self.maps[m].get_owner(x + ox, y + oy).0 as i32)
                    .sum();
                if near != 0 {
                    self.notify(h, NOTIFY_SKILLUSINGEND, &[0], None);
                    return;
                }
            }
            self.remove_magic_effect(h, OWNERTYPE_PLAYER, MAGICTYPE_INVISIBILITY);
            self.calc_ssn_skill(h, s, 1);
            self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTDYING as u16, 0, 1, 0);
            self.maps[m].clear_owner(h as i16, OWNERTYPE_PLAYER, x, y);
            self.maps[m].set_dead_owner(h as i16, OWNERTYPE_PLAYER, x, y);
        }
        if let Some(c) = self.client_mut(h) {
            c.skill_using[s as usize] = true;
        }
    }

    /// The pretend-corpse player stands up on (sx, sy) before an action (the C++ m_bSkillUsingStatus[19] blocks).
    /// `stop`: the stop handler's variant, which disconnects when someone else took the tile (false).
    pub fn pretend_corpse_reown(&mut self, h: usize, sx: i16, sy: i16, stop: bool) -> bool {
        let Some(c) = self.client(h) else { return false };
        // fix: the C++ tested m_bSkillUsingStatus[19], past the 11-skill array (pretend corpse is 10), so a
        // pretend corpse that stopped, attacked, picked up or cast stayed off its tile (unhittable) until it moved
        if !c.skill_using[SKILL_PRETENDCORPSE as usize] {
            return true;
        }
        let m = c.map_index;
        if stop && self.maps[m].get_owner(sx, sy).0 != 0 {
            self.delete_client(h, true, true, true, false);
            return false;
        }
        self.maps[m].clear_owner(h as i16, OWNERTYPE_PLAYER, sx, sy);
        self.maps[m].set_owner(h as i16, OWNERTYPE_PLAYER, sx, sy);
        true
    }

    /// `ClearSkillUsingStatus`
    pub fn clear_skill_using_status(&mut self, h: usize) {
        let Some(c) = self.client_mut(h) else { return };
        c.skill_using = [false; MAXSKILLTYPE];
        c.skill_using_time_id = [0; MAXSKILLTYPE];
        let f = std::mem::take(&mut c.allocated_fish);
        if f != 0 {
            if let Some(fish) = self.fish.get_mut(f).and_then(|x| x.as_mut()) {
                fish.engaging -= 1;
            }
            self.notify(h, NOTIFY_FISHCANCELED, &[0], None);
        }
    }

    /// The ITEMTYPE_USE_SKILL arm of `UseItemHandler` (fishing rods): one endurance point per cast, the result
    /// after the skill's value 2 seconds. An item without endurance does nothing, like the C++.
    pub fn use_skill_item(&mut self, h: usize, k: usize, dx: i16, dy: i16) {
        let now = self.now();
        let c = self.client_mut(h).unwrap();
        let Some(it) = c.items[k].as_mut() else { return };
        let s = it.cfg.related_skill;
        if it.cur_life_span == 0 || !(0..MAXSKILLTYPE as i32).contains(&s) || c.skill_using[s as usize] || it.max_life_span == 0 {
            return;
        }
        it.cur_life_span -= 1;
        if it.cur_life_span == 0 {
            self.notify(h, NOTIFY_ITEMLIFESPANEND, &[EQUIPPOS_NONE, k as i32], None);
            return;
        }
        let Some(delay) = self.data.skills[s as usize].as_ref().map(|c| c.value[1]) else { return };
        let c = self.client_mut(h).unwrap();
        let id = now;
        let e = DelayEvent {
            ty: DELAYEVENTTYPE_USEITEM_SKILL,
            effect: s,
            trigger: now.wrapping_add((delay * 1000) as u32),
            target: h,
            tt: OWNERTYPE_PLAYER,
            m: c.map_index,
            dx,
            dy,
            v1: c.skill_mastery[s as usize] as i32,
            v2: id,
        };
        self.register_delay_event_at(e);
        let c = self.client_mut(h).unwrap();
        c.skill_using[s as usize] = true;
        c.skill_using_time_id[s as usize] = id;
    }

    /// DELAYEVENTTYPE_USEITEM_SKILL: the cast lands unless the player did something else meanwhile.
    pub fn skill_item_due(&mut self, e: &DelayEvent) {
        let s = e.effect as usize;
        let Some(c) = self.client_mut(e.target) else { return };
        if e.tt != OWNERTYPE_PLAYER || !c.skill_using[s] || c.skill_using_time_id[s] != e.v2 {
            return;
        }
        c.skill_using[s] = false;
        c.skill_using_time_id[s] = 0;
        let r = self.use_skill_item_effect(e.target, e.v1, s, e.m, e.dx, e.dy);
        self.notify(e.target, NOTIFY_SKILLUSINGEND, &[r], None);
    }

    /// `iCalculateUseSkillItemEffect` for a player (fishing; taming is not ported).
    fn use_skill_item_effect(&mut self, h: usize, skill_level: i32, s: usize, m: usize, dx: i16, dy: i16) -> i32 {
        let Some(c) = self.client(h) else { return 0 };
        if c.map_index != m || skill_level == 0 {
            return 0;
        }
        let (lx, ly) = (c.x, c.y);
        if skill_level <= self.rng.dice(1, 105) || !self.maps[m].is_water(dx, dy) {
            return 0;
        }
        self.calc_ssn_skill(h, s as i32, 1);
        let Some(cfg) = self.data.skills[s].clone() else { return 1 };
        if cfg.type_ != SKILLEFFECTTYPE_GET {
            return 1;
        }
        let id = match cfg.value[0] {
            1 => 99,
            SKILLEFFECTTYPE_GET_FISH if self.check_fish(h, m, dx, dy) == 0 => ITEM_FISH,
            _ => 0,
        };
        if id == 0 {
            return 1;
        }
        if id == ITEM_FISH {
            self.notify(h, NOTIFY_FISHSUCCESS, &[0], None);
            let e = self.rng.dice(2, 5);
            self.get_exp(h, e, false);
        }
        if let Some(cfg) = self.data.item_by_id(id).cloned() {
            let mut it = one(&cfg);
            it.touch_effect_type = ITET_ID as i16;
            let (a, b) = (self.rng.dice(1, 100000) as i16, self.rng.dice(1, 100000) as i16);
            it.touch_effect_value = [a, b, self.now() as i16];
            self.drop_ground_item(m, lx, ly, it);
        }
        1
    }

    // ---- mining ----

    /// `MineralGenerator` (every 10 s): a new mineral on a quarter of the mining maps below their maximum.
    pub fn mineral_generator(&mut self) {
        for m in 0..self.maps.len() {
            if self.rng.dice(1, 4) != 1 {
                continue;
            }
            let mp = &self.maps[m];
            if !mp.mineral_generator || mp.cur_mineral >= mp.max_mineral {
                continue;
            }
            // fix: dice(1, 0) - 1 indexed point -1 on a map without mineral points
            if mp.total_mineral_points <= 0 {
                continue;
            }
            let p = self.rng.dice(1, mp.total_mineral_points) - 1;
            let (x, y) = self.maps[m].mineral_points[p as usize];
            // fix: the C++ `break`s out of the map loop on an unset point (later maps got no mineral that round)
            if x == -1 || y == -1 {
                continue;
            }
            let level = self.maps[m].mineral_level;
            self.create_mineral(m, x, y, level);
        }
    }

    /// `iCreateMineral`
    fn create_mineral(&mut self, m: usize, x: i16, y: i16, level: i32) -> usize {
        let Some(i) = (1..self.minerals.len()).find(|&i| self.minerals[i].is_none()) else { return 0 };
        let ty = self.rng.dice(1, level);
        let dt = if matches!(ty, 5 | 6) { DYNAMICOBJECT_MINERAL2 } else { DYNAMICOBJECT_MINERAL1 };
        let d = self.add_dynamic_object(i, 0, dt, m, x, y, 0, i as i32);
        if d == 0 {
            return 0;
        }
        let (difficulty, remain) = mineral_stats(ty);
        self.minerals[i] = Some(Mineral { ty, m, difficulty, remain, dyn_obj: d });
        self.maps[m].cur_mineral += 1;
        i
    }

    /// `_CheckMiningAction`: a pickaxe hit (weapon type 25, combat mode) on a mineral.
    pub fn check_mining_action(&mut self, h: usize, dx: i16, dy: i16) {
        self.remove_magic_effect(h, OWNERTYPE_PLAYER, MAGICTYPE_INVISIBILITY);
        let c = self.client(h).unwrap();
        let (m, x, y) = (c.map_index, c.x, c.y);
        let Some(t) = self.maps[m].tile(dx, dy) else { return };
        if !matches!(t.dynamic_object_type as i32, DYNAMICOBJECT_MINERAL1 | DYNAMICOBJECT_MINERAL2) {
            return;
        }
        let d = t.dynamic_object_id as usize;
        let a2 = c.appr[1] as u16;
        if (a2 & 0x0FF0) >> 4 != 25 || a2 & 0xF000 == 0 {
            return;
        }
        let skill = c.skill_mastery[SKILL_MINING as usize] as i32;
        let Some(mi) = self.dyn_objs.get(d).and_then(|o| o.as_ref()).map(|o| o.v1 as usize) else { return };
        let Some(mn) = self.minerals.get(mi).and_then(|x| x.as_ref()) else { return };
        if skill == 0 {
            return;
        }
        let (ty, chance) = (mn.ty, (skill - mn.difficulty).max(1));
        if self.rng.dice(1, 100) > chance {
            return;
        }
        self.calc_ssn_skill(h, SKILL_MINING, 1);
        let (id, exp) = mined(&mut self.rng, ty);
        if exp > 0 {
            self.get_exp(h, exp, false);
        }
        if let Some(cfg) = self.data.item_by_id(id).cloned() {
            self.drop_ground_item(m, x, y, one(&cfg));
        }
        let mn = self.minerals[mi].as_mut().unwrap();
        mn.remain -= 1;
        if mn.remain <= 0 {
            self.delete_mineral(mi);
        }
    }

    /// `bDeleteMineral` (+ the dynamic object it stood on)
    fn delete_mineral(&mut self, i: usize) {
        let Some(mn) = self.minerals[i].take() else { return };
        let Some(o) = self.dyn_objs.get_mut(mn.dyn_obj).and_then(|o| o.take()) else { return };
        self.send_event_type_b(MSGID_DYNAMICOBJECT, MSGTYPE_REJECT as u16, o.m, o.x, o.y, [o.ty as i16, mn.dyn_obj as i16, 0, 0]);
        if let Some(t) = self.maps[o.m].tile_mut(o.x, o.y) {
            (t.dynamic_object_id, t.dynamic_object_type) = (0, 0);
        }
        self.maps[mn.m].set_temp_move_allowed(o.x, o.y, true);
        self.maps[mn.m].cur_mineral -= 1;
    }

    // ---- fishing ----

    /// `FishGenerator` (every 4 s)
    pub fn fish_generator(&mut self) {
        for m in 0..self.maps.len() {
            if self.rng.dice(1, 4) != 1 || self.maps[m].cur_fish >= self.maps[m].max_fish {
                continue;
            }
            // fix: dice(1, 0) - 1 indexed point -1; an unset point `break`s the map loop in the C++
            let total = self.maps[m].total_fish_points;
            if total <= 0 {
                continue;
            }
            let p = self.rng.dice(1, total) - 1;
            let (px, py) = self.maps[m].fish_points[p as usize];
            if px == -1 || py == -1 {
                continue;
            }
            let x = px + (self.rng.dice(1, 3) - 2) as i16;
            let y = py + (self.rng.dice(1, 3) - 2) as i16;
            let r = &mut self.rng;
            let (id, difficulty) = match r.dice(1, 9) {
                1 => (570, r.dice(1, 10) + 5),
                2 => (571, r.dice(1, 5) + 15),
                3 => (572, r.dice(1, 10) + 20),
                4 => (573, 1),
                5 => (574, r.dice(1, 15) + 1),
                6 => (576, r.dice(1, 18) + 1),
                7 => (575, r.dice(1, 12) + 1),
                8 => (577, r.dice(1, 10) + 1),
                _ => match r.dice(1, 150) {
                    1..=3 => (390, r.dice(4, 4) + 20),
                    10 | 11 => (391, r.dice(4, 4) + 40),
                    20 => (5, r.dice(4, 4) + 5),
                    30 => (19, r.dice(4, 4) + 10),
                    40 => (27, r.dice(4, 4) + 15),
                    50 => (36, r.dice(4, 4) + 35),
                    60 => (56, r.dice(4, 4) + 40),
                    70 => (73, r.dice(4, 4) + 30),
                    90 => (351, r.dice(4, 4) + 30),
                    95 => (350, r.dice(4, 4) + 30),
                    // fix: the C++ went on with ITEM_NONE and an uninitialised difficulty (the init then failed)
                    _ => (0, 0),
                },
            };
            let last = 600_000 + 600_000 * (self.rng.dice(1, 3) - 1) as u32;
            if let Some(cfg) = self.data.item_by_id(id).filter(|_| id != 0).cloned() {
                self.create_fish(m, x, y, one(&cfg), difficulty, last);
            }
        }
    }

    /// `iCreateFish`
    pub fn create_fish(&mut self, m: usize, x: i16, y: i16, it: Item, difficulty: i32, last: u32) -> usize {
        if !self.maps[m].is_water(x, y) {
            return 0;
        }
        let Some(i) = (1..self.fish.len()).find(|&i| self.fish[i].is_none()) else { return 0 };
        let dt = if matches!(it.cfg.id_num, 101..=103 | 570..=577) { DYNAMICOBJECT_FISH } else { DYNAMICOBJECT_FISHOBJECT };
        let d = self.add_dynamic_object(i, 0, dt, m, x, y, last, 0);
        if d == 0 {
            return 0;
        }
        self.fish[i] = Some(Fish { m, item: Some(it), dyn_obj: d, engaging: 0, difficulty });
        self.maps[m].cur_fish += 1;
        i
    }

    /// `bDeleteFish`: `mode` 1 = caught, 2 = swam away (the dynamic object expired).
    pub fn delete_fish(&mut self, i: usize, mode: i32) {
        let Some(f) = self.fish.get_mut(i).and_then(|f| f.take()) else { return };
        if let Some(o) = self.dyn_objs.get_mut(f.dyn_obj).and_then(|o| o.take()) {
            // fix: on expiry the C++ sent this REJECT twice (CheckDynamicObjectList, then here)
            self.send_event_type_b(MSGID_DYNAMICOBJECT, MSGTYPE_REJECT as u16, o.m, o.x, o.y, [o.ty as i16, f.dyn_obj as i16, 0, 0]);
            if let Some(t) = self.maps[o.m].tile_mut(o.x, o.y).filter(|t| t.dynamic_object_id as usize == f.dyn_obj) {
                (t.dynamic_object_id, t.dynamic_object_type) = (0, 0);
            }
            self.maps[o.m].cur_fish -= 1;
        }
        for h in self.handles() {
            let c = self.client(h).unwrap();
            if c.is_init_complete && c.allocated_fish == i {
                self.notify(h, NOTIFY_FISHCANCELED, &[mode], None);
                // the fish is gone: ClearSkillUsingStatus finds no fish to release (and cancels again, as the C++)
                self.clear_skill_using_status(h);
            }
        }
    }

    /// `iCheckFish`: the first fish object within 2 tiles of the cast puts the player in fishing mode.
    fn check_fish(&mut self, h: usize, m: usize, dx: i16, dy: i16) -> usize {
        let Some(c) = self.client(h) else { return 0 };
        if !c.is_init_complete {
            return 0;
        }
        let Some(d) = (1..self.dyn_objs.len()).find(|&i| {
            self.dyn_objs[i].as_ref().is_some_and(|o| {
                o.m == m && matches!(o.ty, DYNAMICOBJECT_FISH | DYNAMICOBJECT_FISHOBJECT) && (o.x - dx).abs() <= 2 && (o.y - dy).abs() <= 2
            })
        }) else {
            return 0;
        };
        let fi = self.dyn_objs[d].as_ref().unwrap().owner;
        let Some(f) = self.fish.get(fi).and_then(|f| f.as_ref()) else { return 0 };
        let c = self.client(h).unwrap();
        if f.engaging >= MAXENGAGINGFISH || c.allocated_fish != 0 || c.map_index != m {
            return 0;
        }
        let Some(it) = f.item.as_ref() else { return 0 };
        let (price, spr, frame, name) = (it.price as i32 / 2, it.cfg.sprite as i32, it.cfg.sprite_frame as i32, it.cfg.name.clone());
        let c = self.client_mut(h).unwrap();
        c.allocated_fish = fi;
        c.fish_chance = 1;
        // (the C++ marks skill 1 in use here: the fishing skill's index before Skill.cfg was renumbered)
        c.skill_using[1] = true;
        self.notify(h, NOTIFY_EVENTFISHMODE, &[price, spr, frame], Some(&name));
        self.fish[fi].as_mut().unwrap().engaging += 1;
        d
    }

    /// `FishProcessor` (every 4 s): the catch chance of everyone in fishing mode drifts with their skill.
    pub fn fish_processor(&mut self) {
        for h in self.handles() {
            let c = self.client(h).unwrap();
            if !c.is_init_complete || c.allocated_fish == 0 {
                continue;
            }
            // fix: the C++ `break`s out of the loop for everyone on a vanished fish
            let Some(f) = self.fish.get(c.allocated_fish).and_then(|f| f.as_ref()) else { continue };
            // fix: the C++ read m_cSkillMastery[1] (agility: fishing's index before the renumbering)
            let skill = (c.skill_mastery[SKILL_FISHING as usize] as i32 - f.difficulty).max(1);
            let change = self.rng.dice(1, (skill / 10).max(1));
            let roll = self.rng.dice(1, 100);
            let c = self.client_mut(h).unwrap();
            if skill > roll {
                c.fish_chance = (c.fish_chance + change).min(99);
            } else if skill < roll {
                c.fish_chance = (c.fish_chance - change).max(1);
            } else {
                continue;
            }
            let ch = c.fish_chance;
            self.notify(h, NOTIFY_FISHCHANCE, &[ch], None);
        }
    }

    /// `ReqGetFishThisTimeHandler`: pull now, at the current chance.
    pub fn get_fish_this_time(&mut self, h: usize) {
        let Some(c) = self.client_mut(h) else { return };
        let fi = c.allocated_fish;
        if !c.is_init_complete || fi == 0 {
            return;
        }
        if self.fish.get(fi).is_none_or(|f| f.is_none()) {
            return;
        }
        self.client_mut(h).unwrap().skill_using[1] = false;
        let c = self.client(h).unwrap();
        let chance = c.fish_chance;
        if chance >= self.rng.dice(1, 100) {
            let difficulty = self.fish[fi].as_ref().unwrap().difficulty;
            let e = self.rng.dice(difficulty, 6);
            self.get_exp(h, e, false);
            self.calc_ssn_skill(h, SKILL_FISHING, difficulty);
            let c = self.client(h).unwrap();
            let (m, x, y) = (c.map_index, c.x, c.y);
            if let Some(it) = self.fish[fi].as_mut().unwrap().item.take() {
                self.drop_ground_item(m, x, y, it);
            }
            self.notify(h, NOTIFY_FISHSUCCESS, &[0], None);
            self.client_mut(h).unwrap().allocated_fish = 0;
            self.delete_fish(fi, 1);
            return;
        }
        self.fish[fi].as_mut().unwrap().engaging -= 1;
        self.notify(h, NOTIFY_FISHFAIL, &[0], None);
        self.client_mut(h).unwrap().allocated_fish = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_lowers_the_down_skill() {
        let mut m = [0u8; MAXSKILLTYPE];
        let mut s = [0u32; MAXSKILLTYPE];
        m[1..10].copy_from_slice(&[100; 9]); // 900
        m[10] = 1; // 901: pretend corpse just rose
        let (d, ok) = skill_cap(&mut m, &mut s, 3, 10);
        assert!(ok);
        assert_eq!(d, vec![(3, 1)]);
        assert_eq!((m[3], s[3]), (99, crate::combat::skill_ssn_point(100) - 1));
        assert_eq!(m.iter().map(|&x| x as i32).sum::<i32>(), 900);
    }

    #[test]
    fn cap_takes_all_of_a_small_skill_and_searches_without_down() {
        let mut m = [0u8; MAXSKILLTYPE];
        let mut s = [5u32; MAXSKILLTYPE];
        m[1..10].copy_from_slice(&[100; 9]);
        m[10] = 15; // 915
        m[2] = 100;
        // down skill 10 itself (15 <= 20): all 15 points go at once
        let (d, _) = skill_cap(&mut m.clone(), &mut s.clone(), 10, 10);
        assert_eq!(d, vec![(10, 15)]);
        // no down skill: the lowest SSN among 21+ (ties: the last one), never the rising skill
        s[4] = 1;
        let (d, ok) = skill_cap(&mut m, &mut s, -1, 10);
        assert!(ok);
        assert_eq!(d.len(), 15);
        // a lowered skill's SSN jumps to its next point - 1, so the next round picks another one
        assert_eq!(d[0], (4, 1));
        assert!(d.iter().all(|&(i, p)| i != 10 && p == 1), "{d:?}");
        assert_eq!(m.iter().map(|&x| x as i32).sum::<i32>(), 900);
        // under the cap: nothing
        let (d, ok) = skill_cap(&mut [10u8; MAXSKILLTYPE], &mut [0; MAXSKILLTYPE], -1, 0);
        assert!(d.is_empty() && ok);
    }

    #[test]
    fn mining_tables() {
        assert_eq!(mineral_stats(1), (10, 20));
        assert_eq!(mineral_stats(6), (90, 4));
        let mut r = crate::rng::Rng::new(3);
        for ty in 1..=6 {
            for _ in 0..200 {
                let (id, e) = mined(&mut r, ty);
                assert!(e >= 1, "type {ty} item {id}");
                let ok: &[i32] = match ty {
                    1 => &[355, 357, 507],
                    2 => &[355, 357, 356, 507],
                    3 => &[355, 357, 356, 507],
                    4 => &[355, 356, 357, 508, 354],
                    5 => &[352, 358],
                    _ => &[353, 358, 352, 351, 350],
                };
                assert!(ok.contains(&id), "type {ty} item {id}");
            }
        }
    }
}
