//! Movement and what others see: `ClientMotionHandler`, `iClientMotion_Move_Handler`, `iClientMotion_Stop_Handler`,
//! `SendEventToNearClient_TypeA` (players), `iComposeMoveMapData`, `WriteTileData`, `RequestFullObjectData`,
//! `SendObjectMotionRejectMsg`, `_bGetIsPlayerHostile`.
use crate::client::*;
use crate::consts::*;
use crate::game::Game;
use crate::map::{move_strip, OWNERTYPE_NPC, OWNERTYPE_PLAYER};
use crate::util::{Rd, W};

impl Game {
    /// `ClientMotionHandler`. Attack, magic and get-item come with the combat/magic/item layers.
    pub fn client_motion(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || c.is_killed || c.is_observer_mode {
            return;
        }
        let r = Rd::new(d);
        let (cmd, sx, sy, dir, ctime) = (r.u16(4) as i32, r.u16(6) as i16, r.u16(8) as i16, r.u8(10), r.u32(11));
        match cmd {
            OBJECTSTOP => match self.motion_stop(h, sx, sy, dir) {
                1 => self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTSTOP as u16, 0, 0, 0),
                2 => self.send_motion_reject(h),
                _ => {}
            },
            OBJECTRUN | OBJECTMOVE => {
                match self.motion_move(h, sx, sy, dir, cmd == OBJECTRUN) {
                    1 => {
                        self.client_mut(h).unwrap().rejected_moves = 0;
                        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, cmd as u16, 0, 0, 0);
                    }
                    2 => {
                        let c = self.client_mut(h).unwrap();
                        c.rejected_moves += 1;
                        if c.rejected_moves >= 5 {
                            c.rejected_moves = 0;
                            self.send_motion_reject(h);
                        }
                    }
                    _ => {}
                }
                if self.client(h).is_some_and(|c| c.hp <= 0) {
                    self.killed_handler(h, 0, 0, 1);
                }
                self.check_move_frequency(h, ctime);
            }
            OBJECTDAMAGEMOVE => {
                if self.motion_move(h, sx, sy, dir, false) == 1 {
                    let d = self.client(h).map_or(0, |c| c.cs.last_damage);
                    self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTDAMAGEMOVE as u16, d as i16, 0, 0);
                }
                if self.client(h).is_some_and(|c| c.hp <= 0) {
                    self.killed_handler(h, 0, 0, 1);
                }
            }
            OBJECTATTACK | OBJECTATTACKMOVE => self.client_motion_attack(h, d, cmd),
            OBJECTMAGIC => match self.motion_magic(h, sx, sy, dir) {
                1 => {
                    self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTMAGIC as u16, r.u16(15) as i16, 10, 0);
                    self.client_mut(h).unwrap().cs.has_precasted = true;
                }
                2 => self.send_motion_reject(h),
                _ => {}
            },
            OBJECTGETITEM => match self.motion_get_item(h, sx, sy, dir) {
                1 => self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTGETITEM as u16, 0, 0, 0),
                2 => self.send_motion_reject(h),
                _ => {}
            },
            _ => {}
        }
    }

    /// `bCheckClientMoveFrequency` (it only tracks the time; the C++ never acts on the result)
    fn check_move_frequency(&mut self, h: usize, t: u32) {
        let Some(c) = self.client_mut(h) else { return };
        if c.move_freq_time == 0 {
            c.move_freq_time = t;
        } else if c.is_move_blocked {
            c.move_freq_time = 0;
            c.is_move_blocked = false;
        } else if c.is_attack_mode_change {
            c.move_freq_time = 0;
            c.is_attack_mode_change = false;
        } else {
            c.move_freq_time = t;
        }
    }

    /// `CMap::IncPlayerActivity` of the player's map (moves, attacks, item pick-ups, spells, chat: the busiest sector
    /// of a map is where the special mob event goes)
    pub fn inc_player_activity(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        let (m, x, y) = (c.map_index, c.x, c.y);
        self.maps[m].inc_player_activity(x, y);
    }

    /// `iClientMotion_Stop_Handler`: 1 = done, 2 = position mismatch.
    fn motion_stop(&mut self, h: usize, sx: i16, sy: i16, dir: u8) -> i32 {
        let c = self.client_mut(h).unwrap();
        if dir == 0 || dir > 8 || c.is_killed || !c.is_init_complete {
            return 0;
        }
        if sx != c.x || sy != c.y {
            return 2;
        }
        self.inc_player_activity(h);
        if !self.pretend_corpse_reown(h, sx, sy, true) {
            return 0;
        }
        self.clear_skill_using_status(h);
        let c = self.client_mut(h).unwrap();
        c.dir = dir;
        let mut b = vec![0u8; 6];
        W(&mut b).u32(0, MSGID_RESPONSE_MOTION);
        W(&mut b).u16(4, OBJECTMOTION_CONFIRM as u16);
        self.send(h, &b);
        1
    }

    /// `iClientMotion_Move_Handler`: 1 = moved, 2 = position mismatch, 0 = rejected or deleted.
    pub fn motion_move(&mut self, h: usize, sx: i16, sy: i16, dir: u8, run: bool) -> i32 {
        let now = self.now();
        let c = self.client_mut(h).unwrap();
        if dir == 0 || dir > 8 || c.is_killed || !c.is_init_complete {
            return 0;
        }
        if sx != c.x || sy != c.y {
            return 2;
        }
        c.last_action_time = now;
        // message speed check: 7 steps faster than the client can animate them is a speed hack
        let (cnt, lat, limit) = if run { (&mut c.run_msg_recv_count, &mut c.run_lat, 43 * 8 * 7 - 1500) } else { (&mut c.move_msg_recv_count, &mut c.move_lat, 72 * 8 * 7 - 3000) };
        *cnt += 1;
        if *cnt >= 7 {
            if *lat != 0 && now.wrapping_sub(*lat) < limit {
                self.delete_client(h, true, true, true, false);
                return 0;
            }
            *lat = now;
            *cnt = 0;
        }
        self.inc_player_activity(h);
        self.clear_skill_using_status(h);
        let c = self.client_mut(h).unwrap();
        let (mut dx, mut dy) = (c.x, c.y);
        match dir {
            1 => dy -= 1,
            2 => (dx, dy) = (dx + 1, dy - 1),
            3 => dx += 1,
            4 => (dx, dy) = (dx + 1, dy + 1),
            5 => dy += 1,
            6 => (dx, dy) = (dx - 1, dy + 1),
            7 => dx -= 1,
            _ => (dx, dy) = (dx - 1, dy - 1),
        }
        let m = c.map_index;
        let held = c.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] != 0;
        let (ok, _dyn_type) = self.maps[m].get_moveable(dx, dy);
        if ok && !held {
            // the quest check sees the position before the step (as the C++); relic, spike damage: their layers
            self.check_quest_completed(h);
            let c = self.client_mut(h).unwrap();
            let (ox, oy) = (c.x, c.y);
            (c.x, c.y, c.dir) = (dx, dy, dir);
            self.maps[m].clear_owner(h as i16, OWNERTYPE_PLAYER, ox, oy);
            self.maps[m].set_owner(h as i16, OWNERTYPE_PLAYER, dx, dy);
            let c = self.client_mut(h).unwrap();
            if c.party_id != 0 {
                c.party_coord_steps += 1;
                if c.party_coord_steps >= 4 {
                    c.party_coord_steps = 0;
                    self.party_coords_to_members(h);
                }
            }
            let c = self.client_mut(h).unwrap();
            c.hp = c.hp.max(0);
            let mut b = vec![0u8; 16];
            let mut w = W(&mut b);
            w.u32(0, MSGID_RESPONSE_MOTION);
            w.u16(4, OBJECTMOVE_CONFIRM as u16);
            w.u16(6, (dx - VIEW_LEFT) as u16);
            w.u16(8, (dy - 7) as u16);
            w.u8(10, dir);
            let mut sp_used = 0u8;
            if run {
                if c.time_left_firm_stamina == 0 {
                    c.sp -= 1;
                    sp_used = 1;
                }
                if c.sp < -10 {
                    c.sp = 0;
                    self.delete_client(h, true, true, true, false);
                    return 0;
                }
            }
            let c = self.client(h).unwrap();
            W(&mut b).u8(11, sp_used);
            W(&mut b).u32(12, c.hp as u32);
            let md = self.compose_move_map_data(dx - VIEW_LEFT, dy - 7, h, dir);
            b.extend_from_slice(&md);
            self.send(h, &b);
            1
        } else {
            let c = self.client_mut(h).unwrap();
            c.is_move_blocked = true;
            let c = self.client(h).unwrap();
            let mut b = vec![0u8; 42];
            let mut w = W(&mut b);
            w.u32(0, MSGID_RESPONSE_MOTION);
            w.u16(4, OBJECTMOVE_REJECT as u16);
            w.u16(6, h as u16);
            w.u16(8, c.x as u16);
            w.u16(10, c.y as u16);
            w.u16(12, c.type_ as u16);
            w.u8(14, c.dir);
            w.bytes(15, &Game::pad10(&c.char_name));
            for k in 0..4 {
                w.u16(25 + k * 2, c.appr[k] as u16);
            }
            w.u32(33, c.appr_color as u32);
            w.u32(37, c.status as u32);
            // fix: the C++ sends 42 bytes but writes 41; the last one was uninitialised stack
            self.send(h, &b);
            let (owner, class) = self.maps[m].get_owner(dx, dy);
            if owner != 0 {
                let id = if class == OWNERTYPE_PLAYER { owner as usize } else { owner as usize + 10000 };
                self.request_full_object_data(h, id);
            }
            0
        }
    }

    /// `iRequestPanningMapDataRequest` (MSGID_REQUEST_PANNING, the direction at 6): an observer's view (a GM in
    /// /setobservermode) moves one tile and the tiles that come into view are sent (MSGID_RESPONSE_PANNING: the
    /// move-confirm layout: x - 10, y - 7, direction, then iComposeMoveMapData).
    pub fn request_panning(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_observer_mode || c.is_killed || !c.is_init_complete {
            return;
        }
        let dir = Rd::new(d).u8(6);
        let m = c.map_index;
        let Some((dx, dy)) = pan_target(c.x, c.y, dir, self.maps[m].size_x, self.maps[m].size_y) else { return };
        let c = self.client_mut(h).unwrap();
        (c.x, c.y, c.dir) = (dx, dy, dir);
        let mut b = vec![0u8; 11];
        let mut w = W(&mut b);
        w.u32(0, MSGID_RESPONSE_PANNING);
        w.u16(4, OBJECTMOVE_CONFIRM as u16);
        w.u16(6, (dx - VIEW_LEFT) as u16);
        w.u16(8, (dy - 7) as u16);
        w.u8(10, dir);
        let md = self.compose_move_map_data(dx - VIEW_LEFT, dy - 7, h, dir);
        b.extend_from_slice(&md);
        // fix: the C++ sent 6 bytes more than it had written (iSize + 12 + 1 + 4 for an 11-byte header: stack garbage)
        self.send(h, &b);
    }

    /// `iComposeMoveMapData`: the tiles that come into view after a step.
    fn compose_move_map_data(&mut self, sx: i16, sy: i16, h: usize, dir: u8) -> Vec<u8> {
        let mut out = vec![0u8, 0];
        let mut total = 0i16;
        for (ix, iy) in move_strip(dir) {
            if self.write_tile_data(&mut out, h, sx, sy, ix, iy) {
                total += 1;
            }
        }
        out[..2].copy_from_slice(&total.to_le_bytes());
        out
    }

    /// `WriteTileData`: one tile of a map view (owner, dead owner, top item, dynamic object).
    pub fn write_tile_data(&mut self, out: &mut Vec<u8>, h: usize, sx: i16, sy: i16, ix: i16, iy: i16) -> bool {
        let m = self.client(h).unwrap().map_index;
        // fix: the C++ read tiles outside the map (and outside the tile array) near the borders
        let Some(t) = self.maps[m].tile(sx + ix, sy + iy).cloned() else { return false };
        if t.owner == 0 && t.dead_owner == 0 && t.top_item.is_none() && t.dynamic_object_type == 0 {
            return false;
        }
        let live = |g: &Game, o: i16, class: u8| match class {
            OWNERTYPE_PLAYER => g.client(o as usize).is_some(),
            OWNERTYPE_NPC => g.npcs.get(o as usize).is_some_and(|n| n.is_some()),
            _ => false,
        };
        let mut header = 0u8;
        if t.owner != 0 {
            if live(self, t.owner, t.owner_class) {
                header |= 1;
            } else if let Some(tm) = self.maps[m].tile_mut(sx + ix, sy + iy) {
                tm.owner = 0;
            }
        }
        if t.dead_owner != 0 {
            if live(self, t.dead_owner, t.dead_owner_class) {
                header |= 2;
            } else if let Some(tm) = self.maps[m].tile_mut(sx + ix, sy + iy) {
                tm.dead_owner = 0;
            }
        }
        if t.top_item.is_some() {
            header |= 4;
        }
        if t.dynamic_object_type != 0 {
            header |= 8;
        }
        out.extend_from_slice(&ix.to_le_bytes());
        out.extend_from_slice(&iy.to_le_bytes());
        out.push(header);
        let viewer_admin = self.client(h).unwrap().admin_user_level;
        let player = |out: &mut Vec<u8>, o: usize, hostile_mask: bool| {
            let c = self.client(o).unwrap();
            out.extend_from_slice(&(o as u16).to_le_bytes());
            out.extend_from_slice(&c.type_.to_le_bytes());
            out.push(c.dir);
            for a in c.appr {
                out.extend_from_slice(&a.to_le_bytes());
            }
            out.extend_from_slice(&c.appr_color.to_le_bytes());
            let st = if hostile_mask { c.status & STATUS_ENEMYFLAGS } else { c.status };
            out.extend_from_slice(&st.to_le_bytes());
            out.extend_from_slice(&Game::pad10(&c.char_name));
        };
        let npc = |out: &mut Vec<u8>, o: usize| {
            let n = self.npcs[o].as_ref().unwrap();
            out.extend_from_slice(&((o + 10000) as u16).to_le_bytes());
            out.extend_from_slice(&n.type_.to_le_bytes());
            out.push(n.dir);
            out.extend_from_slice(&n.appr2.to_le_bytes());
            out.extend_from_slice(&n.status.to_le_bytes());
            out.extend_from_slice(&crate::npc::npc_name5(&n.name));
        };
        if header & 1 != 0 {
            let o = t.owner as usize;
            if t.owner_class == OWNERTYPE_NPC {
                npc(out, o);
            } else {
                let mask = self.is_player_hostile(h, o) && o != h && viewer_admin == 0;
                player(out, o, mask);
            }
        }
        if header & 2 != 0 {
            if t.dead_owner_class == OWNERTYPE_NPC {
                npc(out, t.dead_owner as usize);
            } else {
                player(out, t.dead_owner as usize, false);
            }
        }
        if let Some((spr, frame, col)) = t.top_item {
            out.extend_from_slice(&spr.to_le_bytes());
            out.extend_from_slice(&frame.to_le_bytes());
            out.push(col);
        }
        if t.dynamic_object_type != 0 {
            out.extend_from_slice(&t.dynamic_object_id.to_le_bytes());
            out.extend_from_slice(&t.dynamic_object_type.to_le_bytes());
        }
        true
    }

    /// `_bGetIsPlayerHostile`
    pub fn is_player_hostile(&self, h: usize, o: usize) -> bool {
        let (Some(c), Some(t)) = (self.client(h), self.client(o)) else { return false };
        if h == o {
            return true;
        }
        if c.is_neutral() || c.side == t.side {
            t.pk_count != 0
        } else {
            true
        }
    }

    /// `SendEventToNearClient_TypeA` for a player owner (NPC owners come with layer 3).
    pub fn send_event_near_type_a(&mut self, h: usize, msgid: u32, ty: u16, v1: i16, v2: i16, v3: i16) {
        let key = (self.rng.rand() % 255) as u8 + 1;
        let Some(c) = self.client(h) else { return };
        let ty_i = ty as i32;
        let owner_send = matches!(ty_i, OBJECTNULLACTION | OBJECTDAMAGE | OBJECTDYING);
        let mut b = vec![0u8; 43];
        let mut w = W(&mut b);
        w.u32(0, msgid);
        w.u16(4, ty);
        let (len, status_at) = match ty_i {
            OBJECTMAGIC | OBJECTDAMAGE | OBJECTDAMAGEMOVE | OBJECTDYING => {
                w.u16(6, (h + 30000) as u16);
                w.u8(8, c.dir);
                w.u8(9, v1 as u8);
                w.u8(10, v2 as u8);
                if ty_i == OBJECTDYING {
                    w.u16(11, c.x as u16);
                    w.u16(13, c.y as u16);
                    (15, None)
                } else {
                    (11, None)
                }
            }
            OBJECTATTACK | OBJECTATTACKMOVE => {
                w.u16(6, (h + 30000) as u16);
                w.u8(8, c.dir);
                w.u8(9, (v1 - c.x) as u8);
                w.u8(10, (v2 - c.y) as u8);
                w.u16(11, v3 as u16);
                (13, None)
            }
            _ => {
                w.u16(6, h as u16);
                w.u16(8, c.x as u16);
                w.u16(10, c.y as u16);
                w.u16(12, c.type_ as u16);
                w.u8(14, c.dir);
                w.bytes(15, &Game::pad10(&c.char_name));
                for k in 0..4 {
                    w.u16(25 + k * 2, c.appr[k] as u16);
                }
                w.u32(33, c.appr_color as u32);
                w.u32(37, c.status as u32);
                w.u8(41, (ty_i == OBJECTNULLACTION && c.is_killed) as u8);
                (43, Some(37))
            }
        };
        let (m, x, y, status) = (c.map_index, c.x, c.y, c.status);
        for i in self.handles() {
            let o = self.client(i).unwrap();
            if !o.is_init_complete || o.map_index != m || o.x < x - VIEW_NEAR_X || o.x > x + VIEW_NEAR_X || o.y < y - VIEW_NEAR_Y || o.y > y + VIEW_NEAR_Y {
                continue;
            }
            if !owner_send && i == h {
                continue;
            }
            if let Some(at) = status_at {
                let st = if self.is_player_hostile(i, h) && h != i && o.admin_user_level == 0 { status & STATUS_ENEMYFLAGS } else { status };
                b[at..at + 4].copy_from_slice(&st.to_le_bytes());
            }
            self.send_key(i, &b[..len], key);
        }
    }

    /// `RequestFullObjectData` for players (NPC ids >= 10000 come with layer 3).
    pub fn request_full_object_data(&mut self, h: usize, id: usize) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || id == 0 {
            return;
        }
        if id >= 10000 {
            let Some(n) = self.npcs.get(id - 10000).and_then(|n| n.as_ref()) else { return };
            let mut b = vec![0u8; 27];
            let mut w = W(&mut b);
            w.u32(0, MSGID_EVENT_MOTION);
            w.u16(4, OBJECTSTOP as u16);
            w.u16(6, id as u16);
            w.u16(8, n.x as u16);
            w.u16(10, n.y as u16);
            w.u16(12, n.type_ as u16);
            w.u8(14, n.dir);
            w.bytes(15, &crate::npc::npc_name5(&n.name));
            w.u16(20, n.appr2 as u16);
            w.u32(22, n.status as u32);
            w.u8(26, n.is_killed as u8);
            self.send(h, &b);
            return;
        }
        if id >= MAXCLIENTS as usize {
            return;
        }
        let Some(o) = self.client(id) else { return };
        let mut b = vec![0u8; 43];
        let mut w = W(&mut b);
        w.u32(0, MSGID_EVENT_MOTION);
        w.u16(4, OBJECTSTOP as u16);
        w.u16(6, id as u16);
        w.u16(8, o.x as u16);
        w.u16(10, o.y as u16);
        w.u16(12, o.type_ as u16);
        w.u8(14, o.dir);
        w.bytes(15, &Game::pad10(&o.char_name));
        for k in 0..4 {
            w.u16(25 + k * 2, o.appr[k] as u16);
        }
        w.u32(33, o.appr_color as u32);
        let st = if self.is_player_hostile(h, id) && id != h && c.admin_user_level == 0 { o.status & STATUS_ENEMYFLAGS } else { o.status };
        w.u32(37, st as u32);
        w.u8(41, o.is_killed as u8);
        self.send(h, &b);
    }

    /// `SendObjectMotionRejectMsg`
    pub fn send_motion_reject(&mut self, h: usize) {
        let c = self.client_mut(h).unwrap();
        c.is_move_blocked = true;
        let mut b = vec![0u8; 10];
        let mut w = W(&mut b);
        w.u32(0, MSGID_RESPONSE_MOTION);
        w.u16(4, OBJECTMOTION_REJECT as u16);
        w.u16(6, c.x as u16);
        w.u16(8, c.y as u16);
        self.send(h, &b);
    }
}

/// The tile an observer's pan of direction `dir` (1..8, clockwise from north) reaches from (x, y), None when the direction is
/// none (cDir is a char: 0 and the bytes above 127 are refused) or the tile is outside 1..=size.
pub fn pan_target(x: i16, y: i16, dir: u8, size_x: i16, size_y: i16) -> Option<(i16, i16)> {
    let (dx, dy) = match dir {
        1 => (0, -1),
        2 => (1, -1),
        3 => (1, 0),
        4 => (1, 1),
        5 => (0, 1),
        6 => (-1, 1),
        7 => (-1, 0),
        8 => (-1, -1),
        _ => return None,
    };
    let (nx, ny) = (x + dx, y + dy);
    (nx >= 1 && nx <= size_x && ny >= 1 && ny <= size_y).then_some((nx, ny))
}

#[cfg(test)]
mod pan_tests {
    use super::pan_target;

    #[test]
    fn pans() {
        assert_eq!(pan_target(50, 40, 1, 100, 100), Some((50, 39)));
        assert_eq!(pan_target(50, 40, 4, 100, 100), Some((51, 41)));
        assert_eq!(pan_target(50, 40, 8, 100, 100), Some((49, 39)));
        // the map's own tiles are 1..=size: x or y 0 and size + 1 are refused, so are no direction and the signed-negative bytes
        assert_eq!(pan_target(1, 5, 7, 100, 100), None);
        assert_eq!(pan_target(1, 1, 8, 100, 100), None);
        assert_eq!(pan_target(5, 1, 2, 100, 100), None);
        assert_eq!(pan_target(100, 5, 3, 100, 100), None);
        assert_eq!(pan_target(5, 100, 5, 100, 100), None);
        assert_eq!(pan_target(1, 1, 4, 100, 100), Some((2, 2)));
        assert!([0u8, 9, 127, 128, 255].iter().all(|&d| pan_target(50, 50, d, 100, 100).is_none()));
    }
}
