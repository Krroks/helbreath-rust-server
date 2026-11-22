//! `RequestTeleportHandler` (HG.cpp ~13817): teleport tiles (type 4, MSGID_REQUEST_TELEPORT), recalls (0 forced,
//! 1 normal) and scripted teleports (2 forced, 3 player). A destination on this server re-enters the player with
//! PlayerMapEntry; any other map saves through the Login (SAVEPLAYERDATA_REPLY) and the Login's reply makes us
//! send NOTIFY_SERVERCHANGE, after which the client reconnects through the Login to the right game server.
use crate::client::*;
use crate::consts::*;
use crate::entry::map_location_side;
use crate::game::Game;
use crate::map::OWNERTYPE_PLAYER;
use crate::util::cstr;

impl Game {
    pub fn request_teleport(&mut self, h: usize, ty: u8, map_name: Option<&[u8]>, dx: i16, dy: i16) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || c.is_killed || c.is_on_waiting_process {
            return;
        }
        if (ty == 1 || ty == 3) && c.admin_user_level == 0 && self.is_in_foe_map(h) {
            return;
        }
        let (sx, sy, m) = (c.x, c.y, c.map_index);
        let dest = self.maps[m].search_teleport_dest(sx, sy).cloned();
        let found = dest.is_some();
        // a type-4 request off a teleport tile falls through to a re-entry on the same spot (the C++ keeps the
        // reject commented out "till next client")
        let (mut dest_map, mut dest_x, mut dest_y, mut dir) = dest.map_or((Vec::new(), -1, -1, 0), |t| (t.dest_map, t.dest.0, t.dest.1, t.dir));
        // RemoveFromTarget: combat layer
        if self.ast.ev.is_some() && self.is_astoria_map(m) {
            if let Some(k) = self.client(h).unwrap().has_item(crate::astoria::ITEM_RELIC) {
                let n = self.client(h).unwrap().items[k].as_ref().unwrap().cfg.name.clone();
                self.drop_item(h, k, 1, &n);
            }
        }
        self.cancel_exchange_if_open(h);
        self.maps[m].clear_owner(h as i16, OWNERTYPE_PLAYER, sx, sy);
        self.send_event_near_type_a(h, MSGID_EVENT_LOG, MSGTYPE_REJECT as u16, 0, 0, 0);
        let map_side = map_location_side(&dest_map);
        let mut locked_notify = false;
        let c = self.client(h).unwrap();
        let locked = cstr(&c.locked_map_name) != b"NONE" && c.locked_map_time > 0;
        if locked {
            let s = if map_side >= 11 { map_side - 10 } else { map_side };
            if s == 0 || c.side != s {
                (dest_x, dest_y) = (-1, -1);
                locked_notify = true;
                dest_map = cstr(&c.locked_map_name).to_vec();
            }
        }
        let mut found = found;
        if c.is_neutral() && c.map_name.starts_with(b"default2") {
            let s = if map_side >= 11 { map_side - 10 } else { map_side };
            if s != NEUTRAL {
                // ChangeNation: the city-hall layer
            } else {
                found = true;
                dest_map = b"default2".to_vec();
                (dest_x, dest_y, dir) = (-1, -1, 5);
            }
        }
        let mut set_recall_astoria = true;
        match self.astoria_base_teleport(h, cstr(&dest_map)) {
            Some(Some(town)) => dest_map = town,
            Some(None) => {
                let c = self.client(h).unwrap();
                (dest_map, dest_x, dest_y, set_recall_astoria) = (cstr(&c.map_name).to_vec(), c.x, c.y, false);
            }
            None => {}
        }
        let c = self.client(h).unwrap();
        if self.cr.mode && c.side == crate::crusade::map_forbidden_side(cstr(&dest_map)) && c.admin_user_level == 0 {
            // an enemy building in a crusade: recalled (the time runs inside buildings)
            let c = self.client_mut(h).unwrap();
            (c.is_war_location, c.time_left_force_recall) = (true, 0);
        }
        let set_recall = set_recall_astoria;
        if locked {
            let c = self.client(h).unwrap();
            let forbidden = if dest_map == SIDE_MAP[ARESDEN as usize] { ARESDEN } else if dest_map == SIDE_MAP[ELVINE as usize] { ELVINE } else { 0 };
            if !(forbidden + c.side == 3 || cstr(&c.locked_map_name) == cstr(&dest_map)) {
                (dest_x, dest_y) = (-1, -1);
                locked_notify = true;
                dest_map = cstr(&c.locked_map_name).to_vec();
            }
        }
        if found && map_name.is_none() {
            if let Some(i) = self.map_index(&dest_map) {
                let c = self.client_mut(h).unwrap();
                (c.x, c.y, c.dir, c.map_index) = (dest_x, dest_y, dir, i);
                c.map_name = dest_map[..dest_map.len().min(10)].to_vec();
                // (-1,-1) (a locked map) ends on an initial point through bGetEmptyPosition, as in the C++
                return self.teleport_next_step(h, set_recall, locked_notify);
            }
            let c = self.client_mut(h).unwrap();
            (c.x, c.y, c.dir) = (dest_x, dest_y, dir);
            c.map_name = dest_map[..dest_map.len().min(10)].to_vec();
            // fix: the C++ did not set m_bIsOnServerChange on this path, so the client's disconnect saved again
            // and logged the account out of the Login in the middle of the server change
            return self.leave_server(h);
        }
        match ty {
            0 | 1 => {
                let c = self.client(h).unwrap();
                let s = c.side.clamp(0, 3) as usize;
                let mut tmp = if c.level > 80 { SIDE_MAP[s].to_vec() } else { SIDE_MAP_FARM[s].to_vec() };
                if locked {
                    locked_notify = true;
                    tmp = cstr(&c.locked_map_name).to_vec();
                }
                if let Some(i) = self.map_index(&tmp) {
                    let loc = self.client(h).unwrap().location.clone();
                    let (x, y) = self.get_map_initial_point(i, Some(&loc), (-1, -1));
                    let mn = self.maps[i].name.clone();
                    let c = self.client_mut(h).unwrap();
                    (c.x, c.y, c.map_index) = (x, y, i);
                    c.map_name = mn;
                    return self.teleport_next_step(h, set_recall, locked_notify);
                }
                let c = self.client_mut(h).unwrap();
                (c.x, c.y) = (-1, -1);
                c.map_name = tmp;
                self.leave_server(h);
            }
            2 | 3 => {
                let c = self.client(h).unwrap();
                let (mut dx, mut dy) = (dx, dy);
                let tmp = if locked && !map_name.is_some_and(|n| n.starts_with(b"resurr")) {
                    (dx, dy) = (-1, -1);
                    locked_notify = true;
                    cstr(&c.locked_map_name).to_vec()
                } else {
                    // fix: the C++ strcpy'd a NULL map name here when called without one
                    cstr(map_name.unwrap_or(&c.map_name)).to_vec()
                };
                let Some(i) = self.map_index(&tmp) else {
                    let c = self.client_mut(h).unwrap();
                    (c.x, c.y) = (dx, dy);
                    c.map_name = tmp;
                    return self.leave_server(h);
                };
                let (x, y) = if dx == -1 || dy == -1 {
                    let loc = self.client(h).unwrap().location.clone();
                    self.get_map_initial_point(i, Some(&loc), (dx, dy))
                } else {
                    (dx, dy)
                };
                let mn = self.maps[i].name.clone();
                let c = self.client_mut(h).unwrap();
                (c.x, c.y, c.map_index) = (x, y, i);
                c.map_name = mn;
                self.teleport_next_step(h, set_recall, locked_notify);
            }
            _ => self.teleport_next_step(h, set_recall, locked_notify),
        }
    }

    /// `RequestSetRecallPoint` (MSGID_REQUEST_SETRECALLPNT, 7 bytes: the point at offset 6): the next recall of a
    /// player in his home town goes to that initial point of the map he stands on, if the map has it.
    pub fn request_set_recall_point(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        // fix: the range test `< 1 && > 5` was never true, so the signed byte indexed m_pInitialPoint (20 entries)
        // unchecked; here a point past the array is ignored (the client sends 1..6)
        let p = crate::util::Rd::new(d).u8(6) as i8 as i32;
        if self.maps[c.map_index].initial_points.get(usize::try_from(p).unwrap_or(usize::MAX)).copied().flatten().is_none() {
            return;
        }
        self.client_mut(h).unwrap().next_recall_point = p;
    }

    /// The destination is on another server: save with reply, wait for the Login.
    fn leave_server(&mut self, h: usize) {
        let st = self.client(h).unwrap().magic_effect_status[MAGICTYPE_CONFUSE as usize];
        self.notify(h, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_CONFUSE, st], None);
        // SetSlateFlag(NOTIFY_SLATECLEAR): no broadcast
        self.client_mut(h).unwrap().status &= !(STATUS_REDSLATE | STATUS_BLUESLATE | STATUS_GREENSLATE);
        self.send_save(h, MSGID_REQUEST_SAVEPLAYERDATA_REPLY, false);
        let c = self.client_mut(h).unwrap();
        c.is_on_server_change = true;
        c.is_on_waiting_process = true;
    }

    /// RTH_NEXTSTEP
    fn teleport_next_step(&mut self, h: usize, set_recall: bool, locked_notify: bool) {
        self.player_map_entry(h, set_recall);
        self.apocalypse_gate_state(h);
        if self.hd.mode {
            self.update_heldenian_status();
        }
        if locked_notify {
            let c = self.client(h).unwrap();
            let (t, n) = (c.locked_map_time, c.locked_map_name.clone());
            self.notify(h, NOTIFY_LOCKEDMAP, &[t], Some(&n));
        }
    }
}

impl Game {
    /// the filters RequestTeleportListHandler applies to a Teleport.cfg entry for this player (iMapSide: the side
    /// of the town the player stands in)
    fn teleport_offered(&self, h: usize, t: &crate::gamedata::TeleportCfg) -> bool {
        let c = self.client(h).unwrap();
        let loc = cstr(&self.maps[c.map_index].location_name);
        let map_side = [ARESDEN, ELVINE, ISTRIA].into_iter().find(|&s| loc == SIDE_MAP[s as usize]).unwrap_or(-1);
        if t.min_lvl > c.level || t.max_lvl < c.level || (!t.neutral && c.is_neutral()) || (!t.criminal && c.pk_count > 0) {
            return false;
        }
        if self.ast.ev.is_none() && cstr(&t.target_map) == b"astoria" && matches!(crate::chrono_now().weekday, 5 | 6 | 0) {
            return false;
        }
        // middleland only for the last capture-the-relic winner (stats.ini; nobody before a first event)
        if cstr(&t.target_map) == b"middleland" && self.ast.winners[crate::astoria::ET_CAPTURE as usize] != c.side {
            return false;
        }
        if !self.hd.mode && cstr(&t.target_map) == b"BtField" && self.hd.w1 != c.side {
            return false;
        }
        (c.is_neutral() && (t.side == ALLSIDES || t.side == map_side)) || t.side == ALLSIDES || t.side == c.side
    }

    /// the cost shown and charged: free to BtField / HRampart during a heldenian
    fn teleport_cost(&self, t: &crate::gamedata::TeleportCfg) -> u32 {
        if (self.ast.ev.is_some() && cstr(&t.target_map) == b"astoria") || (self.hd.mode && matches!(cstr(&t.target_map), b"BtField" | b"HRampart")) {
            0
        } else {
            t.cost
        }
    }

    /// `RequestTeleportListHandler` (MSGID_REQUEST_TELEPORT_LIST): the destinations of an NPC (William, Gail)
    pub fn request_teleport_list(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || c.is_killed || c.is_on_waiting_process {
            return;
        }
        let npc = cstr(&crate::util::Rd::new(d).raw(6, 20)).to_vec();
        let here = cstr(&self.maps[c.map_index].name).to_vec();
        let mut b = vec![0u8; 10];
        b[..4].copy_from_slice(&MSGID_RESPONSE_TELEPORT_LIST.to_le_bytes());
        let mut n = 0i32;
        for (i, t) in self.data.teleports.iter().enumerate() {
            if cstr(&t.source_map) != here || cstr(&t.npc_name) != npc || !self.teleport_offered(h, t) {
                continue;
            }
            b.extend_from_slice(&(i as i32).to_le_bytes());
            b.extend_from_slice(&Game::pad10(&t.target_map));
            b.extend_from_slice(&t.x.to_le_bytes());
            b.extend_from_slice(&t.y.to_le_bytes());
            b.extend_from_slice(&self.teleport_cost(t).to_le_bytes());
            n += 1;
        }
        b[4..6].copy_from_slice(&(if n == 0 { MSGTYPE_REJECT } else { MSGTYPE_CONFIRM } as u16).to_le_bytes());
        b[6..10].copy_from_slice(&n.to_le_bytes());
        self.send(h, &b);
    }

    /// `RequestChargedTeleportHandler` (MSGID_REQUEST_CHARGED_TELEPORT): pay and go.
    pub fn request_charged_teleport(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || c.is_killed || c.is_on_waiting_process {
            return;
        }
        let i = crate::util::Rd::new(d).u8(6) as i8 as i32; // the C++ reads one char of the int
        let Some(t) = usize::try_from(i).ok().and_then(|i| self.data.teleports.get(i)).cloned() else { return };
        if cstr(&self.maps[c.map_index].name) != cstr(&t.source_map) {
            return;
        }
        let c = self.client(h).unwrap();
        let loc = cstr(&self.maps[c.map_index].location_name);
        let map_side = [ARESDEN, ELVINE, ISTRIA].into_iter().find(|&s| loc == SIDE_MAP[s as usize]).unwrap_or(-1);
        let mut err = 0i16;
        if t.min_lvl > c.level || t.max_lvl < c.level {
            err = 1;
        } else if !t.neutral && c.is_neutral() {
            err = 3;
        } else if !t.criminal && c.pk_count > 0 {
            err = 4;
        } else if (c.is_neutral() && t.side != ALLSIDES && t.side != map_side) || (!c.is_neutral() && t.side != ALLSIDES && t.side != c.side) {
            err = 5;
        } else if !self.teleport_offered(h, &t) {
            // fix: the C++ skipped the list's BtField (last winner only) rule here, so a forged index went anyway
            err = 5;
        }
        if err == 0 {
            let cost = self.teleport_cost(&t);
            let (k, gold) = self.item_count(h, b"Gold");
            if gold >= cost {
                if let Some(k) = k {
                    self.set_item_count(h, k, gold - cost);
                }
                self.calc_total_weight(h);
                self.add_city_funds(h, cost);
            } else {
                err = 6;
            }
        }
        if err != 0 {
            let mut b = vec![0u8; 8];
            b[..4].copy_from_slice(&MSGID_RESPONSE_CHARGED_TELEPORT.to_le_bytes());
            b[4..6].copy_from_slice(&(MSGTYPE_REJECT as u16).to_le_bytes());
            b[6..8].copy_from_slice(&err.to_le_bytes());
            self.send(h, &b);
            return;
        }
        let (mut x, mut y) = (t.x as i16, t.y as i16);
        if cstr(&t.target_map) == b"astoria" {
            // the start point of the player's base (the C++ read past the bases for a side that has none)
            let side = self.client(h).unwrap().side;
            if let Some(i) = self.ast.base_pos.iter().position(|&s| s == side) {
                (x, y) = crate::astoria::ASTORIA_START[i];
            }
            if self.ast.ev.is_some() {
                self.set_slate_flag(h, NOTIFY_SLATECLEAR, false);
            }
        }
        self.request_teleport(h, 2, Some(&t.target_map), x, y);
    }
}
