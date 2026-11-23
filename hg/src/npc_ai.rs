//! NPC combat behaviour: `CNpc::targetSearch`, `getDangerValue`, `behavior_stop`, `behavior_attack`,
//! `NpcBehavior_Flee`, `NpcBehavior_Dead`, `NpcKilledHandler`, `DeleteNpc`, `RemoveFromTarget`,
//! `NpcRequestAssistance`, the crusade towers' attacks (their other behaviors are in crusade.rs). Heldenian
//! bookkeeping is not ported.
use crate::client::*;
use crate::consts::*;
use crate::game::Game;
use crate::map::{OWNERTYPE_NPC, OWNERTYPE_PLAYER};
use crate::util::cstr;
use crate::npc::{misc_dir, MOVETYPE_FOLLOW, MOVETYPE_RANDOM, BEHAVIOR_ATTACK, BEHAVIOR_DEAD, BEHAVIOR_FLEE, BEHAVIOR_MOVE, NPC_AGC, NPC_CAT, NPC_CP, NPC_CT, NPC_DARK_ELF, NPC_RABBIT};

const DIR_X: [i16; 9] = [0, 0, 1, 1, 1, 0, -1, -1, -1];
const DIR_Y: [i16; 9] = [0, -1, -1, 0, 1, 1, 1, 0, -1];

impl Game {
    /// Position of a unit (player or NPC handle), if it exists.
    pub fn unit_pos(&self, h: usize, t: u8) -> Option<(i16, i16)> {
        match t {
            OWNERTYPE_PLAYER => self.client(h).map(|c| (c.x, c.y)),
            OWNERTYPE_NPC => self.npcs.get(h).and_then(|n| n.as_ref()).map(|n| (n.x, n.y)),
            _ => None,
        }
    }

    /// `Unit::IsDead` (hp <= 0); a vanished unit counts as dead.
    pub fn target_dead(&self, h: usize, t: u8) -> bool {
        match t {
            OWNERTYPE_PLAYER => self.client(h).is_none_or(|c| c.hp <= 0),
            OWNERTYPE_NPC => self.npcs.get(h).and_then(|n| n.as_ref()).is_none_or(|n| n.hp <= 0),
            _ => true,
        }
    }

    /// `CNpc::targetSearch`: the nearest hostile unit within the search range (Chebyshev distance).
    pub fn npc_target_search(&mut self, i: usize) -> Option<(usize, u8)> {
        let n = self.npcs[i].as_ref()?;
        let (m, sx, sy, r, side, ty, sa) = (n.map_index, n.x, n.y, n.target_search_range as i16, n.side, n.type_, n.special_ability);
        let search_type = (ty == NPC_CP as i16) as i32;
        let (mut best, mut dist) = (None, 100);
        for ix in sx - r..=sx + r {
            let mut y = sy - r;
            while y <= sy + r {
                let iy = y;
                if self.maps[m].block_empty(ix, iy) {
                    y = crate::map::next_block(iy); // nothing alive in this 8x8 block
                    continue;
                }
                y += 1;
                let (o, ot) = self.maps[m].get_owner(ix, iy);
                if o <= 0 || (ot == OWNERTYPE_NPC && o as usize == i) {
                    continue;
                }
                let o = o as usize;
                let (dx, dy, tside, pk, invisible) = match ot {
                    OWNERTYPE_PLAYER => {
                        let Some(c) = self.client(o) else {
                            self.maps[m].clear_owner(o as i16, OWNERTYPE_PLAYER, ix, iy);
                            continue;
                        };
                        if c.gm_flags & GMFLAG_NOAGGRO != 0 || search_type == 1 {
                            continue;
                        }
                        (c.x, c.y, c.side, c.pk_count, c.magic_effect_status[MAGICTYPE_INVISIBILITY as usize] != 0)
                    }
                    OWNERTYPE_NPC => {
                        let Some(t) = self.npcs.get(o).and_then(|n| n.as_ref()) else {
                            self.maps[m].clear_owner(o as i16, OWNERTYPE_NPC, ix, iy);
                            continue;
                        };
                        let tt = t.type_ as i32;
                        if search_type == 1 && !(36..=41).contains(&tt) || search_type == 0 && (tt == NPC_CT || tt == NPC_AGC) {
                            continue;
                        }
                        // type 21 (guards) ignore NPCs with players next to them: the C++ zeroed the owner
                        // but still compared it; nothing in the stock configs is type 21 and hostile to NPCs
                        (t.x, t.y, t.side, 0, t.magic_effect_status[MAGICTYPE_INVISIBILITY as usize] != 0)
                    }
                    _ => continue,
                };
                // fix: the C++ turns side 10 into HOSTILE (4) at init but still tests `< 10`, so monsters fell into the
                // town-NPC branch and never aggroed travellers (side 0, no PK); HOSTILE takes the monster branch here
                if side < SIDE_HOSTILE {
                    if tside == 0 {
                        if pk == 0 {
                            continue;
                        }
                    } else if (pk == 0 && tside == side) || side == 0 {
                        continue;
                    }
                } else if (ot == OWNERTYPE_NPC && tside == 0) || tside == side {
                    continue;
                }
                if invisible && sa != 1 {
                    continue;
                }
                let d = (sx - dx).abs().max((sy - dy).abs());
                if d < dist {
                    dist = d;
                    best = Some((o, ot));
                }
            }
        }
        best
    }

    /// `CNpc::getDangerValue` (dynamic objects are not ported, so fire fields do not count).
    fn npc_danger_value(&self, i: usize, dx: i16, dy: i16) -> i32 {
        let n = self.npcs[i].as_ref().unwrap();
        let mut v = 0;
        for ix in dx - 2..=dx + 2 {
            let mut y = dy - 2;
            while y <= dy + 2 {
                let iy = y;
                if self.maps[n.map_index].block_empty(ix, iy) {
                    y = crate::map::next_block(iy);
                    continue;
                }
                y += 1;
                let (o, ot) = self.maps[n.map_index].get_owner(ix, iy);
                if o <= 0 {
                    continue;
                }
                let side = match ot {
                    OWNERTYPE_PLAYER => self.client(o as usize).map(|c| c.side),
                    OWNERTYPE_NPC => self.npcs.get(o as usize).and_then(|n| n.as_ref()).map(|n| n.side),
                    _ => None,
                };
                // the C++ GetOwner returns the unit pointer, so a stale tile owner counts as nothing
                let Some(side) = side else { continue };
                v += if side != n.side { 1 } else { -1 };
            }
        }
        v
    }

    /// One step of an NPC towards (dx, dy) with cGetNextMoveDir; false when blocked.
    pub fn npc_step_to(&mut self, i: usize, dx: i16, dy: i16) -> bool {
        let n = self.npcs[i].as_ref().unwrap();
        let (m, x, y, turn, mut err) = (n.map_index, n.x, n.y, n.turn, n.tmp_error);
        let dir = self.next_move_dir(x, y, dx, dy, m, turn, &mut err);
        self.npcs[i].as_mut().unwrap().tmp_error = err;
        if dir == 0 {
            return false;
        }
        let (nx, ny) = (x + DIR_X[dir as usize], y + DIR_Y[dir as usize]);
        self.maps[m].clear_owner(i as i16, OWNERTYPE_NPC, x, y);
        self.maps[m].set_owner(i as i16, OWNERTYPE_NPC, nx, ny);
        let n = self.npcs[i].as_mut().unwrap();
        (n.x, n.y, n.dir) = (nx, ny, dir);
        self.send_event_near_npc(i, MSGID_EVENT_MOTION, OBJECTMOVE as u16);
        true
    }

    /// `CNpc::behavior_stop` (mana collectors, detectors and grand magic generators are crusade structures).
    pub fn npc_behavior_stop(&mut self, i: usize) {
        let n = self.npcs[i].as_mut().unwrap();
        n.behavior_turn_count += 1;
        // only the crusade structures act here: collectors, detectors, generators, mana stones; towers look around
        if n.action_limit != 5 || !self.structure_stop(i) {
            return;
        }
        if let Some((t, tt)) = self.npc_target_search(i) {
            let n = self.npcs[i].as_mut().unwrap();
            (n.behavior, n.behavior_turn_count, n.target, n.target_type) = (BEHAVIOR_ATTACK, 0, t, tt);
        }
    }

    fn npc_attack_done(&mut self, i: usize) {
        let n = self.npcs[i].as_mut().unwrap();
        n.attack_count += 1;
        if !n.is_perm_attack_mode && n.action_limit == 0 {
            match n.attack_strategy {
                ATTACKAI_EXCHANGEATTACK => (n.behavior_turn_count, n.behavior) = (0, BEHAVIOR_FLEE),
                ATTACKAI_TWOBYONEATTACK if n.attack_count >= 2 => (n.behavior_turn_count, n.behavior) = (0, BEHAVIOR_FLEE),
                _ => {}
            }
        }
    }

    /// the action-limit-5 attacks of CNpc::behavior_attack: the crossbow tower (36) shoots, the cannon tower (37)
    /// casts 61; the heldenian towers (87, 89) the same on players only. Other structures do nothing.
    fn tower_attack(&mut self, i: usize, t: usize, tt: u8, (dx, dy): (i16, i16), dir: u8, ranged: bool) {
        let n = self.npcs[i].as_ref().unwrap();
        let (ty, sx, sy) = (n.type_, n.x, n.y);
        let (ax, ay) = if ranged { (dx, dy) } else { (sx + DIR_X[dir as usize], sy + DIR_Y[dir as usize]) };
        let player = tt == OWNERTYPE_PLAYER && self.client(t).is_some();
        match ty {
            36 | 87 if ty == 36 || player => {
                self.send_event_near_npc_v(i, MSGID_EVENT_MOTION, OBJECTATTACK as u16, ax, ay, 2);
                self.attack_effect(t, tt, i, OWNERTYPE_NPC, dx, dy, 2, false, false);
            }
            37 | 89 if ty == 37 || player => {
                self.send_event_near_npc_v(i, MSGID_EVENT_MOTION, OBJECTATTACK as u16, dx, dy, 1);
                self.npcs[i].as_mut().unwrap().magic_hit_ratio = 1000;
                self.npc_magic(i, dx, dy, 61);
            }
            _ => {}
        }
    }

    /// `CNpc::behavior_attack`: melee when adjacent, ranged (mode 20) within the attack range, else chase.
    pub fn npc_behavior_attack(&mut self, i: usize) {
        let n = self.npcs[i].as_mut().unwrap();
        if n.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] != 0 || n.is_killed {
            return;
        }
        match n.action_limit {
            1 | 2 | 3 | 4 | 6 | 8 => return,
            5 if n.build_count > 0 => return, // a structure still being built
            _ => {}
        }
        if n.behavior_turn_count == 0 {
            n.attack_count = 0;
        }
        n.behavior_turn_count += 1;
        if n.behavior_turn_count > 20 {
            n.behavior_turn_count = 0;
            if !n.is_perm_attack_mode {
                n.behavior = BEHAVIOR_MOVE;
            }
            return;
        }
        let (sx, sy, t, tt) = (n.x, n.y, n.target, n.target_type);
        let Some((dx, dy)) = self.unit_pos(t, tt) else {
            let n = self.npcs[i].as_mut().unwrap();
            (n.behavior_turn_count, n.behavior) = (0, BEHAVIOR_MOVE);
            return;
        };
        let danger = self.npc_danger_value(i, dx, dy);
        let n = self.npcs[i].as_ref().unwrap();
        let (perm, bravery, structure) = (n.is_perm_attack_mode, n.bravery, n.action_limit == 5);
        if danger > bravery && !perm && !structure {
            let n = self.npcs[i].as_mut().unwrap();
            (n.behavior_turn_count, n.behavior) = (0, BEHAVIOR_FLEE);
            return;
        }
        if n.hp <= 2 && self.dice(1, bravery) <= 3 && !perm && !structure {
            let n = self.npcs[i].as_mut().unwrap();
            (n.behavior_turn_count, n.behavior) = (0, BEHAVIOR_FLEE);
            return;
        }
        let dir = misc_dir(sx, sy, dx, dy);
        if dir == 0 {
            return;
        }
        self.npcs[i].as_mut().unwrap().dir = dir;
        let n = self.npcs[i].as_ref().unwrap();
        let (range, ty, ml) = (n.attack_range as i16, n.type_, n.magic_level);
        if (sx - dx).abs() <= 1 && (sy - dy).abs() <= 1 {
            if structure {
                self.tower_attack(i, t, tt, (dx, dy), dir, false);
            } else if ml == 11 {
                // magic level 11 casts 75 on its own tile (no spell 75 in Magic.cfg: only the motion is seen)
                self.send_event_near_npc_v(i, MSGID_EVENT_MOTION, OBJECTATTACK as u16, sx, sy, 1);
                self.npcs[i].as_mut().unwrap().magic_hit_ratio = 1000;
                self.npc_magic(i, sx, sy, 75);
            } else {
                let (ax, ay) = (sx + DIR_X[dir as usize], sy + DIR_Y[dir as usize]);
                self.send_event_near_npc_v(i, MSGID_EVENT_MOTION, OBJECTATTACK as u16, ax, ay, 1);
                self.attack_effect(t, tt, i, OWNERTYPE_NPC, dx, dy, 1, false, false);
            }
            if self.npcs[i].is_some() {
                self.npc_attack_done(i);
            }
            return;
        }
        // casters: magic level > 0 (its table) or < 0 (43/37/0), half the time within 9x7
        let mut chase = false;
        if ml != 0 && self.dice(1, 2) == 1 && (sx - dx).abs() <= 9 && (sy - dy).abs() <= 7 {
            let mp = self.npcs[i].as_ref().unwrap().mp;
            if let Some(mt) = self.npc_pick_spell(mp, if ml == 11 { 0 } else { ml }) {
                let n = self.npcs[i].as_ref().unwrap();
                if ml > 0 && n.ai_level >= 2 {
                    let pfm = self.unit_effect(t, tt, MAGICTYPE_PROTECT) == crate::combat::MAGICPROTECT_PFM;
                    if pfm && ((sx - dx).abs() > range || (sy - dy).abs() > range) {
                        let n = self.npcs[i].as_mut().unwrap();
                        (n.behavior_turn_count, n.behavior) = (0, BEHAVIOR_MOVE);
                        return;
                    }
                    chase = pfm || (mt == 35 && self.unit_effect(t, tt, MAGICTYPE_HOLDOBJECT) != 0);
                }
                if !chase {
                    let (ax, ay) = (sx + DIR_X[dir as usize], sy + DIR_Y[dir as usize]);
                    self.send_event_near_npc_v(i, MSGID_EVENT_MOTION, OBJECTATTACK as u16, ax, ay, 1);
                    self.npc_magic(i, dx, dy, mt);
                    let now = self.now();
                    if let Some(n) = self.npcs[i].as_mut() {
                        n.time = now;
                    }
                    return;
                }
            } else if ml == 11 {
                chase = true; // the C++ `goto NBA_CHASE` of magic level 11
            }
        }
        if !chase && range > 1 && (sx - dx).abs() <= range && (sy - dy).abs() <= range && structure {
            self.tower_attack(i, t, tt, (dx, dy), dir, true);
            if self.npcs[i].is_some() {
                self.npc_attack_done(i);
            }
            return;
        }
        if !chase && range > 1 && (sx - dx).abs() <= range && (sy - dy).abs() <= range {
            // type 51 (magic 61) is NPC magic: not ported, it shoots like the rest
            let mode = if ty == NPC_DARK_ELF as i16 { 2 } else { 20 };
            if matches!(ty, 63 | 53 | 79) && !self.target_dead(t, tt) && !self.resist_ice(t, tt) && self.unit_effect(t, tt, MAGICTYPE_ICE) == 0 {
                // frost, beholder, type 79: 5 s of ice (no MAGICEFFECTON notify in the C++ either)
                let now = self.now();
                self.unit_mut(t, tt).unwrap().0[MAGICTYPE_ICE as usize] = 1;
                self.set_unit_status_flag(t, tt, STATUS_FROZEN, true);
                self.register_delay_event(DELAYEVENTTYPE_MAGICRELEASE, MAGICTYPE_ICE, now.wrapping_add(5 * 1000), t, tt);
            }
            self.send_event_near_npc_v(i, MSGID_EVENT_MOTION, OBJECTATTACK as u16, dx, dy, mode as i16);
            self.attack_effect(t, tt, i, OWNERTYPE_NPC, dx, dy, mode, false, false);
            if self.npcs[i].is_some() {
                self.npc_attack_done(i);
            }
            return;
        }
        // NBA_CHASE
        let n = self.npcs[i].as_mut().unwrap();
        if n.action_limit != 0 {
            return;
        }
        n.attack_count = 0;
        self.npc_step_to(i, dx, dy);
    }

    /// `NpcBehavior_Flee`
    pub fn npc_behavior_flee(&mut self, i: usize) {
        let n = self.npcs[i].as_mut().unwrap();
        if n.is_killed {
            return;
        }
        n.behavior_turn_count += 1;
        match n.attack_strategy {
            ATTACKAI_EXCHANGEATTACK | ATTACKAI_TWOBYONEATTACK => {
                if n.behavior_turn_count >= 2 {
                    (n.behavior, n.behavior_turn_count) = (BEHAVIOR_ATTACK, 0);
                    return;
                }
            }
            _ => {
                if self.dice(1, 2) == 1 {
                    self.npc_request_assistance(i);
                }
            }
        }
        let n = self.npcs[i].as_mut().unwrap();
        if n.behavior_turn_count > 10 {
            (n.behavior_turn_count, n.behavior, n.tmp_error) = (0, BEHAVIOR_MOVE, 0);
            if n.hp <= 3 {
                let hd = n.hit_dice;
                let add = self.dice(1, hd);
                let n = self.npcs[i].as_mut().unwrap();
                n.hp = (n.hp + add).max(1);
            }
            return;
        }
        if let Some((t, tt)) = self.npc_target_search(i) {
            let n = self.npcs[i].as_mut().unwrap();
            (n.target, n.target_type) = (t, tt);
        }
        let n = self.npcs[i].as_ref().unwrap();
        let (sx, sy) = (n.x, n.y);
        // fix: the C++ dereferenced a vanished target here
        let Some((dx, dy)) = self.unit_pos(n.target, n.target_type) else { return };
        self.npc_step_to(i, sx - (dx - sx), sy - (dy - sy));
    }

    /// `NpcBehavior_Dead`
    pub fn npc_behavior_dead(&mut self, i: usize) {
        let now = self.now();
        let n = self.npcs[i].as_mut().unwrap();
        n.behavior_turn_count += 1;
        if n.behavior_turn_count > 5 {
            n.behavior_turn_count = 0;
        }
        if now.wrapping_sub(n.dead_time) > n.regen_time {
            self.delete_npc(i);
        }
    }

    /// `NpcRequestAssistance`
    fn npc_request_assistance(&mut self, i: usize) {
        let n = self.npcs[i].as_ref().unwrap();
        let (m, sx, sy, side, t, tt) = (n.map_index, n.x, n.y, n.side, n.target, n.target_type);
        for ix in sx - 8..=sx + 8 {
            let mut y = sy - 8;
            while y <= sy + 8 {
                let iy = y;
                if self.maps[m].block_empty(ix, iy) {
                    y = crate::map::next_block(iy);
                    continue;
                }
                y += 1;
                let (o, ot) = self.maps[m].get_owner(ix, iy);
                if o <= 0 || ot != OWNERTYPE_NPC || o as usize == i {
                    continue;
                }
                let Some(h) = self.npcs.get_mut(o as usize).and_then(|n| n.as_mut()) else { continue };
                if h.side == side && !h.is_perm_attack_mode && h.behavior == BEHAVIOR_MOVE {
                    (h.behavior, h.behavior_turn_count, h.target, h.target_type) = (BEHAVIOR_ATTACK, 0, t, tt);
                    return;
                }
            }
        }
    }

    /// `RemoveFromTarget`: NPCs chasing this unit go back to moving (guild summons are not ported).
    pub fn remove_from_target(&mut self, h: usize, t: u8, code: i32) {
        for n in self.npcs.iter_mut().flatten() {
            if n.target == h && n.target_type == t {
                if code == MAGICTYPE_INVISIBILITY && n.special_ability == 1 {
                    continue;
                }
                (n.behavior, n.target, n.target_type) = (BEHAVIOR_MOVE, 0, 0);
            }
        }
    }

    /// `NpcKilledHandler` (the heldenian points and the SA 7/8 death spells are not ported).
    /// `bAnalyzeCriminalAction` (the melee kill; the magic callers come with magic): killing a player who is not
    /// hostile to the attacker (same side, no PK) calls a guard of the attacker's town map next to him, set to
    /// attack him for good. The guard is a summon (no exp, no drops, gone after SUMMONTIME).
    /// `check`: only tell whether a guard would be called (bIsCheck).
    pub fn analyze_criminal_action(&mut self, h: usize, dx: i16, dy: i16, check: bool) -> bool {
        let Some(c) = self.client(h) else { return false };
        if !c.is_init_complete || self.cr.mode || self.maps[c.map_index].is_fight_zone {
            return false;
        }
        let (m, x, y) = (c.map_index, c.x, c.y);
        let (o, ot) = self.maps[m].get_owner(dx, dy);
        if ot != OWNERTYPE_PLAYER || self.client(o as usize).is_none() || self.is_player_hostile(h, o as usize) {
            return false;
        }
        if check {
            return true;
        }
        let mn = crate::util::cstr(&c.map_name);
        let guard: &[u8] = [(ARESDEN, &b"Guard-Aresden"[..]), (ELVINE, b"Guard-Elvine"), (NEUTRAL, b"Guard-Neutral"), (ISTRIA, b"Guard-Istria")]
            .iter()
            .find(|(s, _)| SIDE_MAP[*s as usize] == mn)
            .map_or(b"XXX", |(_, g)| g);
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return false };
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        let Some(i) = self.create_npc(guard, &name, m, 0, MOVETYPE_RANDOM, Some((x, y)), &[-1; 10], None, -1) else {
            self.maps[m].set_naming_value_empty(nv);
            return false;
        };
        let now = self.now();
        // bSetNpcAttackMode; fix: the C++ looked the new NPC up by the first 5 bytes of its name ("_q184" also
        // matches "_q1845"), here by handle
        let n = self.npcs[i].as_mut().unwrap();
        (n.is_summoned, n.summoned_time) = (true, now);
        (n.behavior, n.behavior_turn_count, n.target, n.target_type, n.is_perm_attack_mode) = (BEHAVIOR_ATTACK, 0, h, OWNERTYPE_PLAYER, true);
        false
    }

    pub fn npc_killed(&mut self, attacker: usize, at: u8, i: usize, damage: i32) {
        let now = self.now();
        let Some(n) = self.npcs[i].as_mut() else { return };
        if n.is_killed {
            return;
        }
        (n.is_killed, n.hp, n.last_damage) = (true, 0, damage);
        let m = n.map_index;
        self.map_npcs[m].total_alive -= 1;
        self.remove_from_target(i, OWNERTYPE_NPC, 0);
        let n = self.npcs[i].as_mut().unwrap();
        (n.target, n.target_type) = (0, 0);
        let weapon = if at == OWNERTYPE_PLAYER { self.client(attacker).map_or(1, |c| (c.appr[1] as i32 & 0x0FF0) >> 4) } else { 1 };
        self.send_event_near_npc_v(i, MSGID_EVENT_MOTION, OBJECTDYING as u16, damage as i16, weapon as i16, 0);
        let n = self.npcs[i].as_mut().unwrap();
        let (x, y) = (n.x, n.y);
        (n.behavior, n.behavior_turn_count, n.dead_time) = (BEHAVIOR_DEAD, 0, now);
        self.maps[m].clear_owner(i as i16, OWNERTYPE_NPC, x, y);
        self.maps[m].set_dead_owner(i as i16, OWNERTYPE_NPC, x, y);
        if self.maps[m].map_type == MAPTYPE_NOPENALTY_NOREWARD {
            return;
        }
        self.npc_dead_item_generator(i, attacker, at);
        if at == OWNERTYPE_PLAYER && self.client(attacker).is_some() && !self.npcs[i].as_ref().unwrap().is_summoned {
            let n = self.npcs[i].as_ref().unwrap();
            let mut exp = n.exp / 3 + n.no_die_remain_exp.max(0);
            let c = self.client(attacker).unwrap();
            if c.fx.add_exp != 0 {
                exp += (c.fx.add_exp as f64 / 100.0 * exp as f64) as i32;
            }
            if self.cr.mode && exp > 10 {
                // fix (gameplay-visible): the C++ test `!= default || != arefarm || != elvfarm` is always true, so
                // the crusade cut exp to 1/4 everywhere; here it spares the neutral map and the farms, as meant
                let mn = cstr(&c.map_name);
                if mn != SIDE_MAP[NEUTRAL as usize] && mn != b"arefarm" && mn != b"elvfarm" {
                    exp /= 4;
                }
            }
            if c.level > 100 && (n.type_ as i32 == NPC_RABBIT || n.type_ as i32 == NPC_CAT) {
                exp = 0;
            }
            self.get_exp(attacker, exp, false);
            let t = self.npcs[i].as_ref().unwrap().type_ as i32;
            self.quest_kill(attacker, t);
        }
        self.crusade_npc_killed(attacker, at, i);
        // special abilities 7 / 8: a last Fire-Strike / Mass-Fire-Strike where it fell
        let n = self.npcs[i].as_mut().unwrap();
        if matches!(n.special_ability, 7 | 8) {
            (n.mp, n.magic_hit_ratio) = (100, 100);
            let (x, y, spell) = (n.x, n.y, if n.special_ability == 7 { 30 } else { 61 });
            self.npc_magic(i, x, y, spell);
        }
        self.heldenian_npc_killed(attacker, at, i);
    }

    /// `DeleteNpc`: the corpse goes away and the drops made on deletion fall (`m_drops` is empty in the C++, so
    /// only the 1/50 gold and the slate roll remain).
    pub fn delete_npc(&mut self, i: usize) {
        if self.npcs[i].is_none() {
            return;
        }
        self.remove_delay_events(i, OWNERTYPE_NPC, 0);
        self.send_event_near_npc(i, MSGID_EVENT_LOG, MSGTYPE_REJECT as u16);
        let n = self.npcs[i].as_ref().unwrap();
        let (m, x, y, spot) = (n.map_index, n.x, n.y, n.spot_mob_index);
        self.maps[m].clear_owner(i as i16, OWNERTYPE_NPC, x, y);
        let nv = crate::util::atoi(&n.name[2.min(n.name.len())..]);
        if nv >= 0 {
            self.maps[m].set_naming_value_empty(nv as usize);
        }
        self.map_npcs[m].total_active -= 1;
        self.apocalypse_npc_deleted(m);
        if spot != 0 {
            if let Some(s) = self.map_npcs[m].spots.iter_mut().find(|s| s.index as i32 == spot) {
                s.cur_mobs -= 1;
            }
        }
        self.remove_from_target(i, OWNERTYPE_NPC, 0);
        // fix: the C++ followers only noticed a dead master when its slot was NULL, and followed whatever NPC
        // reused the slot; they go back to random movement as soon as the master is deleted
        for f in self.npcs.iter_mut().flatten() {
            if f.move_type == MOVETYPE_FOLLOW && f.follow_owner_type == OWNERTYPE_NPC && f.follow_owner == i {
                (f.move_type, f.follow_owner) = (MOVETYPE_RANDOM, 0);
            }
        }
        self.crusade_npc_deleted(i);
        self.npc_delete_drops(i);
        self.npcs[i] = None;
    }
}
