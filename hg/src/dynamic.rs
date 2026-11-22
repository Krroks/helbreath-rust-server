//! Dynamic objects (HG.cpp / map/DynamicObject.cpp): iAddDynamicObjectList, CheckDynamicObjectList,
//! DynamicObjectEffectProcessor (both on the 3-second timer) and CheckFireBluring. Fire walls / fields, poison
//! clouds, spike fields (no damage: drawn only, like the C++) and ice storms come from MAGICTYPE_CREATE_DYNAMIC.
//! Fish and minerals are created by skills.rs; the crusade flags come with their layer.
use crate::client::{MAGICTYPE_HOLDOBJECT, MAGICTYPE_ICE, MAGICTYPE_POISON, STATUS_POISON};
use crate::consts::*;
use crate::game::Game;
use crate::map::{OWNERTYPE_NPC, OWNERTYPE_PLAYER};

pub struct DynObj {
    pub owner: usize,
    pub owner_type: u8,
    pub ty: i32,
    pub m: usize,
    pub x: i16,
    pub y: i16,
    pub register_time: u32,
    /// lifetime in ms (0 = permanent)
    pub last_time: u32,
    pub v1: i32,
    pub count: i32,
}

/// A fire's lifetime under the weather (rain or snow 1..3).
pub fn fire_time(last: u32, weather: u8) -> u32 {
    if last == 0 {
        return 0;
    }
    let t = match weather {
        1 => last - last / 2,
        2 => last / 2 - last / 3,
        3 => last / 3 - last / 4,
        _ => last,
    };
    if t == 0 { 1000 } else { t }
}

impl Game {
    /// `iAddDynamicObjectList`: 0 when the tile already holds one (or does not suit the type).
    #[allow(clippy::too_many_arguments)]
    pub fn add_dynamic_object(&mut self, owner: usize, owner_type: u8, ty: i32, m: usize, x: i16, y: i16, last_time: u32, v1: i32) -> usize {
        let Some(t) = self.maps[m].tile(x, y) else { return 0 };
        if t.dynamic_object_type != 0 {
            return 0;
        }
        let mut last = last_time;
        match ty {
            DYNAMICOBJECT_FIRE => {
                if !self.maps[m].is_move_allowed(x, y) {
                    return 0;
                }
                last = fire_time(last, self.maps[m].weather);
            }
            DYNAMICOBJECT_FISH | DYNAMICOBJECT_FISHOBJECT if !self.maps[m].is_water(x, y) => return 0,
            DYNAMICOBJECT_MINERAL1 | DYNAMICOBJECT_MINERAL2 => {
                if !self.maps[m].get_moveable(x, y).0 {
                    return 0;
                }
                self.maps[m].set_temp_move_allowed(x, y, false);
            }
            DYNAMICOBJECT_ARESDENFLAG | DYNAMICOBJECT_ELVINEFLAG => self.maps[m].set_temp_move_allowed(x, y, false),
            _ => {}
        }
        let i = match self.dyn_objs.iter().skip(1).position(|o| o.is_none()) {
            Some(p) => p + 1,
            None if self.dyn_objs.len() < MAXDYNAMICOBJECTS as usize => {
                if self.dyn_objs.is_empty() {
                    self.dyn_objs.push(None); // index 0 is never used
                }
                self.dyn_objs.push(None);
                self.dyn_objs.len() - 1
            }
            None => return 0,
        };
        let now = self.now();
        if last != 0 {
            last += self.rng.dice(1, 4) as u32 * 1000;
        }
        self.dyn_objs[i] = Some(DynObj { owner, owner_type, ty, m, x, y, register_time: now, last_time: last, v1, count: 0 });
        let t = self.maps[m].tile_mut(x, y).unwrap();
        (t.dynamic_object_id, t.dynamic_object_type) = (i as u16, ty as i16);
        self.send_event_type_b(MSGID_DYNAMICOBJECT, MSGTYPE_CONFIRM as u16, m, x, y, [ty as i16, i as i16, 0, 0]);
        i
    }

    /// `CheckDynamicObjectList`: rain shortens fires, expired objects leave their tile (MSGTYPE_REJECT event).
    pub fn check_dynamic_objects(&mut self) {
        let now = self.now();
        for i in 1..self.dyn_objs.len() {
            let Some(o) = self.dyn_objs[i].as_mut() else { continue };
            if o.last_time == 0 {
                continue;
            }
            let w = self.maps[o.m].weather;
            if o.ty == DYNAMICOBJECT_FIRE && (1..=3).contains(&w) {
                o.last_time -= (o.last_time / 10) * w as u32;
            }
            if now.wrapping_sub(o.register_time) < o.last_time {
                continue;
            }
            let (m, x, y, ty) = (o.m, o.x, o.y, o.ty);
            if matches!(ty, DYNAMICOBJECT_FISH | DYNAMICOBJECT_FISHOBJECT) {
                // the fish swims away (bDeleteFish clears the tile and sends the REJECT)
                let f = o.owner;
                self.delete_fish(f, 2);
                self.dyn_objs[i] = None;
                continue;
            }
            if self.maps[m].tile(x, y).is_some_and(|t| t.dynamic_object_id as usize == i) {
                self.send_event_type_b(MSGID_DYNAMICOBJECT, MSGTYPE_REJECT as u16, m, x, y, [ty as i16, i as i16, 0, 0]);
                let t = self.maps[m].tile_mut(x, y).unwrap();
                (t.dynamic_object_id, t.dynamic_object_type) = (0, 0);
            }
            self.dyn_objs[i] = None;
        }
    }

    /// A tick of damage on a player standing in an object; true if it is still alive.
    fn dyn_hit_player(&mut self, h: usize, dmg: i32, now: u32) -> bool {
        let c = self.client_mut(h).unwrap();
        c.hp -= dmg;
        c.cs.last_damage_time = now;
        if c.hp <= 0 {
            // (the C++ names the victim as its own killer)
            self.killed_handler(h, h, OWNERTYPE_PLAYER, dmg);
            return false;
        }
        if dmg > 0 {
            self.notify(h, NOTIFY_HP, &[], None);
            let c = self.client_mut(h).unwrap();
            let hold = c.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize];
            if hold != 0 {
                c.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] = 0;
                self.notify(h, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_HOLDOBJECT, hold], None);
                self.remove_delay_events(h, OWNERTYPE_PLAYER, MAGICTYPE_HOLDOBJECT);
            }
        }
        true
    }

    /// A tick of damage on an NPC (crusade structures and the heldenian gate take none); true if still alive.
    fn dyn_hit_npc(&mut self, i: usize, mut dmg: i32, now: u32, gate: bool) -> bool {
        let n = self.npcs[i].as_mut().unwrap();
        if matches!(n.type_, 40 | 41 | 6) || (gate && matches!(n.type_, 5 | 8)) {
            dmg = 0;
        }
        if matches!(n.action_limit, 0 | 3 | 5) {
            n.hp -= dmg;
        }
        if n.hp <= 0 {
            self.npc_killed(i, OWNERTYPE_NPC, i, 0);
            return false;
        }
        if self.rng.dice(1, 3) == 2 {
            self.npcs[i].as_mut().unwrap().time = now;
        }
        self.npcs[i].as_mut().unwrap().magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] = 0;
        self.send_event_near_npc_v(i, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, dmg as i16, 0, 0);
        true
    }

    /// The pretend-corpse player lying on (x, y) (a dead owner with HP left) takes `dmg`.
    fn dyn_hit_dead_owner(&mut self, m: usize, x: i16, y: i16, dmg_dice: (i32, i32), now: u32, set_time: bool) {
        let (o, t) = self.maps[m].get_dead_owner(x, y);
        if t != OWNERTYPE_PLAYER || self.client(o as usize).is_none_or(|c| c.hp <= 0) {
            return;
        }
        let h = o as usize;
        let d = self.rng.dice(dmg_dice.0, dmg_dice.1);
        let c = self.client_mut(h).unwrap();
        c.hp -= d;
        if set_time {
            c.cs.last_damage_time = now;
        }
        if c.hp <= 0 {
            self.killed_handler(h, h, OWNERTYPE_PLAYER, d);
        } else if d > 0 {
            self.notify(h, NOTIFY_HP, &[], None);
        }
    }

    /// `DynamicObjectEffectProcessor`: poison clouds (3x3, 1d6 / 1d8 + poison), ice storms (5x5, 3d3+5 + ice),
    /// fires (3x3, 1d6); an ice storm shortens the fires it covers and a fire the ice storms.
    pub fn dynamic_object_effects(&mut self) {
        let now = self.now();
        for i in 0..self.dyn_objs.len() {
            let Some(o) = self.dyn_objs[i].as_mut() else { continue };
            let (ty, m, x, y, v1) = (o.ty, o.m, o.x, o.y, o.v1);
            let r = match ty {
                DYNAMICOBJECT_PCLOUD_BEGIN => 1,
                DYNAMICOBJECT_ICESTORM => 2,
                DYNAMICOBJECT_FIRE => {
                    if o.count == 1 {
                        self.fire_bluring(m, x, y);
                    }
                    let o = self.dyn_objs[i].as_mut().unwrap();
                    o.count = (o.count + 1).min(10);
                    1
                }
                _ => continue,
            };
            for ix in x - r..=x + r {
                for iy in y - r..=y + r {
                    let (oh, ot) = self.maps[m].get_owner(ix, iy);
                    let oh = oh as usize;
                    if oh != 0 && ot == OWNERTYPE_PLAYER && self.client(oh).is_some_and(|c| !c.is_killed && !c.is_invincible()) {
                        let d = match ty {
                            DYNAMICOBJECT_PCLOUD_BEGIN => self.rng.dice(1, if v1 < 20 { 6 } else { 8 }),
                            DYNAMICOBJECT_ICESTORM => self.rng.dice(3, 3) + 5,
                            _ => self.rng.dice(1, 6),
                        };
                        if self.dyn_hit_player(oh, d, now) {
                            if ty == DYNAMICOBJECT_PCLOUD_BEGIN && !self.resist_magic(oh, OWNERTYPE_PLAYER, 100) && !self.client(oh).unwrap().cs.is_poisoned {
                                let c = self.client_mut(oh).unwrap();
                                (c.cs.is_poisoned, c.cs.poison_level, c.cs.poison_time) = (true, v1, now);
                                c.set_status_flag(STATUS_POISON, true);
                                self.notify(oh, NOTIFY_MAGICEFFECTON, &[MAGICTYPE_POISON, v1], None);
                            }
                            if ty == DYNAMICOBJECT_ICESTORM && !self.resist_ice(oh, OWNERTYPE_PLAYER) {
                                self.add_magic_effect(oh, OWNERTYPE_PLAYER, MAGICTYPE_ICE, self.ice_storm_secs(), 1);
                            }
                        }
                    } else if oh != 0 && ot == OWNERTYPE_NPC && self.npcs.get(oh).is_some_and(|n| n.is_some()) {
                        let d = match ty {
                            DYNAMICOBJECT_PCLOUD_BEGIN => self.rng.dice(1, if v1 < 20 { 6 } else { 8 }),
                            DYNAMICOBJECT_ICESTORM => self.rng.dice(3, 3) + 5,
                            _ => self.rng.dice(1, 6),
                        };
                        if self.dyn_hit_npc(oh, d, now, ty == DYNAMICOBJECT_PCLOUD_BEGIN) && ty == DYNAMICOBJECT_ICESTORM && !self.resist_ice(oh, OWNERTYPE_NPC) {
                            self.add_magic_effect(oh, OWNERTYPE_NPC, MAGICTYPE_ICE, self.ice_storm_secs(), 1);
                        }
                    }
                    match ty {
                        DYNAMICOBJECT_ICESTORM => {
                            self.dyn_hit_dead_owner(m, ix, iy, (3, 2), now, false);
                            self.shorten_dynamic(m, ix, iy, DYNAMICOBJECT_FIRE);
                        }
                        DYNAMICOBJECT_FIRE => {
                            self.dyn_hit_dead_owner(m, ix, iy, (1, 6), now, true);
                            self.shorten_dynamic(m, ix, iy, DYNAMICOBJECT_ICESTORM);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn ice_storm_secs(&self) -> u32 {
        // g_magicConfigList[MAGIC_ICESTORM (55)]->m_sValue[MAGICV_ICEDURATION (6)]
        self.data.magic.get(55).and_then(|r| r.as_deref()).and_then(crate::magic::Spell::parse).map_or(0, |s| s.val[6].max(0) as u32)
    }

    /// A tenth off the lifetime of the object of type `ty` on (x, y).
    fn shorten_dynamic(&mut self, m: usize, x: i16, y: i16, ty: i32) {
        let Some(t) = self.maps[m].tile(x, y) else { return };
        if t.dynamic_object_type as i32 != ty {
            return;
        }
        if let Some(o) = self.dyn_objs.get_mut(t.dynamic_object_id as usize).and_then(|o| o.as_mut()) {
            o.last_time -= o.last_time / 10;
        }
    }

    /// `CheckFireBluring`: coal lying next to a new fire catches fire (6 s).
    fn fire_bluring(&mut self, m: usize, x: i16, y: i16) {
        for ix in x - 1..=x + 1 {
            for iy in y - 1..=y + 1 {
                if self.ground.get(&(m, ix, iy)).and_then(|v| v.first()).is_some_and(|i| i.cfg.id_num == 355) {
                    // ITEM_COAL
                    self.take_ground_item(m, ix, iy);
                    self.add_dynamic_object(0, 0, DYNAMICOBJECT_FIRE, m, ix, iy, 6000, 0);
                    let top = self.maps[m].tile(ix, iy).and_then(|t| t.top_item).unwrap_or((0, 0, 0));
                    self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_SETITEM as u16, m, ix, iy, [top.0, top.1, top.2 as i16, 0]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fire_under_weather() {
        assert_eq!(fire_time(30_000, 0), 30_000);
        assert_eq!(fire_time(30_000, 1), 15_000);
        assert_eq!(fire_time(30_000, 2), 5_000);
        assert_eq!(fire_time(30_000, 3), 2_500);
        assert_eq!(fire_time(2, 3), 1000);
        assert_eq!(fire_time(0, 1), 0);
    }
}
