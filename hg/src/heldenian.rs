//! Heldenian (HG.cpp ~41086-41950 and its scattered branches): the heldenian map info lines (HeldenianMap,
//! HeldenianTower, HeldenianModeMap, HeldenianWinningZone, HeldenianGateDoor), _CreateHeldenianGUID /
//! bReadHeldenianGUIDFile, StartHeldenianMode, UpdateHeldenianStatus, SetHeldenianFlag, HeldenianEndWarNow,
//! HeldenianPlayerKill, CheckHeldenianResultCalculation, RequestHeldenianScroll, GetOccupyFlagHandler,
//! DeleteOccupyFlags + CMap::iRegisterOccupyFlag, CClient::CheckNearbyFlags / IsHeldWinner / IsHeldLoser, the
//! /heldenian and /endheldenian GM commands, and the GSM_STARTHELDENIAN / GSM_ENDHELDENIAN relay (defined but never
//! sent by the C++).
use crate::client::*;
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::map::OWNERTYPE_PLAYER;
use crate::npc::MOVETYPE_RANDOM;
use crate::util::{atoi, cstr, StrTok};

const MAX_HELDENIANTOWER: usize = 200;
const MAXHELDENIANDOOR: usize = 10;
const MAXHELDENIANSUMMONPOINT: i32 = 12000;
const FLAGRANGE_X: i16 = 20;
const FLAGRANGE_Y: i16 = 16;
pub const NPC_CT: i16 = 87;
pub const NPC_AGC: i16 = 89;

/// a placed occupy flag (COccupyFlag)
#[derive(Clone, Debug)]
pub struct Flag {
    pub side: i32,
    pub x: i16,
    pub y: i16,
    pub ek: i32,
    pub dyn_obj: usize,
}

/// the heldenian part of a map's info
#[derive(Clone, Default, Debug)]
pub struct HMap {
    pub is_heldenian: bool,
    /// (type, side, x, y)
    pub towers: Vec<(i32, i32, i16, i16)>,
    pub mode_map: i32,
    pub winning_zone: (i16, i16),
    /// (dir, x, y)
    pub doors: Vec<(u8, i16, i16)>,
    pub flags: Vec<Flag>,
}

#[derive(Default)]
pub struct Heldenian {
    pub mode: bool,
    pub ty: i32,
    pub last_ty: i32,
    pub guid: u32,
    pub w1: i32,
    pub w2: i32,
    /// aresden / elvine: towers left, flags, kills, deaths
    pub towers: [i32; 2],
    pub flags: [i32; 2],
    pub kills: [i32; 2],
    pub dead: [i32; 2],
    pub maps: Vec<HMap>,
}

/// `bReadHeldenianGUIDFile`: only the keys present change.
pub fn parse_guid_file(txt: &[u8], h: &mut Heldenian) {
    let mut t = StrTok::new(txt, b"= \t\n");
    while let Some(k) = t.get() {
        if k.starts_with(b"Heldenian-GUID") {
            h.guid = atoi(&t.get().unwrap_or_default()) as u32;
        } else if k.starts_with(b"Heldenian-Type1-Winner") {
            h.w1 = atoi(&t.get().unwrap_or_default());
        } else if k.starts_with(b"Heldenian-Type2-Winner") {
            h.w2 = atoi(&t.get().unwrap_or_default());
        } else if k.starts_with(b"Heldenian-LastType") {
            h.last_ty = atoi(&t.get().unwrap_or_default());
        }
    }
}

/// the tower NPC of a (type, side), as StartHeldenianMode names them
fn tower_name(ty: i32, side: i32) -> Option<&'static str> {
    Some(match (ty, side) {
        (87, 1) => "CT-Aresden",
        (87, 2) => "CT-Elvine",
        (89, 1) => "AGC-Aresden",
        (89, 2) => "AGC-Elvine",
        (36, 1) => "AGT-Aresden",
        (36, 2) => "AGT-Elvine",
        (37, 1) => "CGT-Aresden",
        (37, 2) => "CGT-Elvine",
        (39, 1) => "DT-Aresden",
        (39, 2) => "DT-Elvine",
        _ => return None,
    })
}

/// the summon scroll Gail sells and its points (RequestHeldenianScroll)
pub fn scroll_points(id: i32) -> Option<i32> {
    Some(match id {
        875 => 2000, // Sorceress
        876 => 3000, // Ancient Temple Knight
        877 => 1500, // Elf Master
        878 => 3000, // DarkShadow Knight
        879 => 4000, // Heavy Battle Tank
        880 => 3000, // Barbarian
        _ => return None,
    })
}

impl Game {
    /// OnStartGameSignal: the heldenian map info lines and the GUID file.
    pub fn heldenian_init(&mut self) {
        self.hd.maps = vec![HMap::default(); self.maps.len()];
        for m in 0..self.maps.len() {
            let mut h = HMap::default();
            for (k, f) in self.maps[m].info.clone() {
                let v = |i: usize| f.get(i).map_or(0, |t| atoi(t));
                match k {
                    b"HeldenianMap" => h.is_heldenian = v(0) != 0,
                    b"HeldenianTower" if h.towers.len() < MAX_HELDENIANTOWER => h.towers.push((v(0), v(1), v(2) as i16, v(3) as i16)),
                    b"HeldenianModeMap" => h.mode_map = v(0),
                    b"HeldenianWinningZone" => h.winning_zone = (v(0) as i16, v(1) as i16),
                    b"HeldenianGateDoor" if h.doors.len() < MAXHELDENIANDOOR => h.doors.push((v(0) as u8, v(1) as i16, v(2) as i16)),
                    _ => {}
                }
            }
            self.hd.maps[m] = h;
        }
        (self.hd.w1, self.hd.w2) = (-1, -1);
        self.read_heldenian_guid();
    }

    fn read_heldenian_guid(&mut self) {
        match std::fs::read("GameData/HeldenianGUID.txt") {
            Ok(raw) => parse_guid_file(&crate::util::text_mode(&raw), &mut self.hd),
            Err(_) => log("(ERROR) Cannot open Heldenian-GUID file."),
        }
    }

    /// `_CreateHeldenianGUID`
    fn write_heldenian_guid(&self) {
        let _ = std::fs::create_dir_all("GameData");
        let h = &self.hd;
        // fix: the C++ had its four lines commented out and wrote an empty file: the type never alternated (always 1),
        // the winners were lost at each restart and IsHeldWinner / IsHeldLoser (drops, shop prices) never applied
        let txt = format!("Heldenian-GUID = {}\nHeldenian-Type1-Winner = {}\nHeldenian-Type2-Winner = {}\nHeldenian-LastType = {}\n", h.guid, h.w1, h.w2, h.ty);
        if std::fs::write("GameData/HeldenianGUID.txt", txt).is_err() {
            log(&format!("(ERROR) Cannot create Heldenian-GUID({}) file", h.guid));
        }
    }

    pub fn is_heldenian_map(&self, m: usize) -> bool {
        self.hd.maps.get(m).is_some_and(|h| h.is_heldenian)
    }

    /// `CClient::IsHeldWinner` / `IsHeldLoser` (by the last heldenian's type)
    pub fn held_winner(&self, side: i32) -> Option<bool> {
        match self.hd.last_ty {
            1 => Some(side == self.hd.w1),
            2 => Some(side == self.hd.w2),
            _ => None,
        }
    }

    /// `StartHeldenianMode` (`h`: the GM, None for a GSM relay with its type and GUID)
    pub fn start_heldenian(&mut self, h: Option<usize>, relayed: Option<(i32, u32)>) {
        if self.hd.mode || self.ap.mode || self.cr.mode {
            return;
        }
        self.read_heldenian_guid();
        match relayed {
            Some((ty, guid)) => (self.hd.ty, self.hd.guid) = (ty, guid),
            None => {
                self.hd.ty = self.hd.last_ty % 2 + 1;
                // fix: the C++ never set m_dwHeldenianGUID (only the empty file could), so every summon scroll
                // ever bought stayed valid; each heldenian gets its own here
                self.hd.guid = self.now();
            }
        }
        match h.and_then(|h| self.client(h)) {
            Some(c) => {
                log(&format!("Admin Order({}): /beginheld \tAdminIP({})", crate::util::l1(&c.char_name), crate::util::l1(&c.ip_address)));
                self.notify(h.unwrap(), NOTIFY_IPACCOUNTINFO, &[], Some(b"Heldenian starting in progress."));
            }
            None => log("Local command: beginHeldenian"),
        }
        for i in self.handles() {
            let c = self.client_mut(i).unwrap();
            if !c.is_init_complete {
                continue;
            }
            c.special_event_id = 1;
            c.war_contribution = 0;
            c.construction_point = (10000 + c.range * 100).min(MAXHELDENIANSUMMONPOINT);
            let cp = c.construction_point;
            self.notify(i, NOTIFY_HELDENIANSTART, &[], None);
            self.notify(i, NOTIFY_HELDENIANTELEPORT, &[], None);
            self.notify(i, NOTIFY_CONSTRUCTIONPOINT, &[cp, 0, 0], None);
        }
        let ty = self.hd.ty;
        for m in 0..self.maps.len() {
            if !self.is_heldenian_map(m) {
                continue;
            }
            self.delete_occupy_flags(m);
            for i in self.handles() {
                let c = self.client(i).unwrap();
                if c.is_init_complete && c.map_index == m && c.admin_user_level == 0 {
                    self.request_teleport(i, 1, None, -1, -1);
                }
            }
            self.kill_map_npcs(m);
            let name = cstr(&self.maps[m].name).to_vec();
            let hm = self.hd.maps[m].clone();
            if ty == 1 && name == b"BtField" {
                for (i, &(tt, side, x, y)) in hm.towers.iter().enumerate() {
                    if tt < 1 || tt > MAXNPCTYPES {
                        break;
                    }
                    if self.heldenian_npc(m, i, tower_name(tt, side).unwrap_or(""), (x, y), side, None).is_some() {
                        match side {
                            1 => self.hd.towers[0] += 1,
                            2 => self.hd.towers[1] += 1,
                            _ => {}
                        }
                    }
                }
                log(&format!("Towers Created on ({}) For Event Type {ty} HeldenianAresdenLeftTower : {} , HeldenianElvineLeftTower : {}", crate::util::l1(&name), self.hd.towers[0], self.hd.towers[1]));
            } else if ty == 2 {
                if name == b"GodH" {
                    let (zx, zy) = hm.winning_zone;
                    let w1 = self.hd.w1;
                    self.set_heldenian_flag(m, zx, zy, w1, 616, None, 2);
                }
                if name == b"HRampart" {
                    let w2 = self.hd.w2;
                    for (i, &(tt, _, x, y)) in hm.towers.iter().enumerate() {
                        if tt < 1 || tt > MAXNPCTYPES {
                            break;
                        }
                        self.heldenian_npc(m, i, tower_name(tt, w2).unwrap_or(""), (x, y), w2, None);
                    }
                    for (i, &(dir, x, y)) in hm.doors.iter().enumerate() {
                        if x < 1 {
                            break;
                        }
                        let gate = if w2 == 1 { "gate-a" } else { "gate-e" };
                        if self.heldenian_npc(m, i, gate, (x, y), w2, Some(dir)).is_some() {
                            log(&format!("(_) Gate installed at {x},{y}"));
                        }
                    }
                }
            }
        }
        self.hd.mode = true;
        self.update_heldenian_status();
        let w = if ty == 1 { self.hd.w1 } else { self.hd.w2 };
        log(&format!("(_) Heldenian mode ON. (type {ty}, last winner:{w})"));
        if relayed.is_none() {
            let mut d = vec![GSM_STARTHELDENIAN as u8];
            d.extend_from_slice(&(ty as u16).to_le_bytes());
            d.extend_from_slice(&self.hd.guid.to_le_bytes());
            self.stock_msg(&d);
        }
    }

    /// a tower or gate of StartHeldenianMode: built (build count 0, appearance 0), on `side`, a group master
    fn heldenian_npc(&mut self, m: usize, i: usize, npc_name: &str, (x, y): (i16, i16), side: i32, dir: Option<u8>) -> Option<usize> {
        let nv = self.maps[m].get_empty_naming_value()?;
        // the C++ names them by the tower index, not the map: '_', 'A' + i, the naming value
        let name = [&[b'_', (i as u8).wrapping_add(65)][..], nv.to_string().as_bytes()].concat();
        let Some(n) = self.create_npc(npc_name.as_bytes(), &name, m, 0, MOVETYPE_RANDOM, Some((x, y)), &[-1; 10], None, -1) else {
            self.maps[m].set_naming_value_empty(nv);
            return None;
        };
        let npc = self.npcs[n].as_mut().unwrap();
        (npc.build_count, npc.side, npc.is_master) = (0, side, true);
        npc.status = (npc.status & 0x0FFF_FFFF) | (side << 28);
        match dir {
            Some(d) => npc.dir = d,
            None => npc.appr2 = 0,
        }
        Some(n)
    }

    fn kill_map_npcs(&mut self, m: usize) {
        for n in 1..self.npcs.len() {
            if self.npcs[n].as_ref().is_some_and(|x| !x.is_killed && x.map_index == m) {
                self.npc_killed(0, 0, n, 0);
            }
        }
    }

    /// `UpdateHeldenianStatus`
    pub fn update_heldenian_status(&mut self) {
        let h = &self.hd;
        let v = [h.towers[0], h.flags[0], h.kills[0], h.dead[0], h.towers[1], h.flags[1], h.kills[1], h.dead[1]];
        for i in self.handles() {
            if self.client(i).unwrap().is_init_complete {
                self.notify(i, NOTIFY_HELDENIANCOUNT, &v, None);
            }
        }
    }

    /// `SetHeldenianFlag`: code 1 = a guild master's flag item, code 2 = the defenders' flag of GodH at the start.
    pub fn set_heldenian_flag(&mut self, m: usize, x: i16, y: i16, side: i32, ek: i32, h: Option<usize>, code: i32) -> bool {
        if !self.maps[m].is_move_allowed(x, y) || self.hd.maps[m].flags.len() >= MAXOCCUPYFLAG as usize - 1 {
            return false;
        }
        // fix: CMap::iRegisterOccupyFlag never marked the tile, so the "flag already here" test never held and
        // flags piled up on one tile; a tile takes one flag here
        if self.hd.maps[m].flags.iter().any(|f| (f.x, f.y) == (x, y)) {
            return false;
        }
        let (mode, ty, godh) = (self.hd.mode, self.hd.ty, cstr(&self.maps[m].name) == b"GodH");
        let mut placed = false;
        if code == 1 {
            let Some(c) = h.and_then(|h| self.client(h)) else { return false };
            let (pside, rank, cname) = (c.side, c.guild_rank, c.char_name.clone());
            let dyn_ty = match pside {
                ARESDEN => DYNAMICOBJECT_ARESDENFLAG,
                ELVINE => DYNAMICOBJECT_ELVINEFLAG,
                _ => 0,
            };
            if mode && ty == 1 && self.is_heldenian_map(m) && rank == 0 {
                if dyn_ty == 0 {
                    return false;
                }
                if !self.register_flag(m, x, y, side, dyn_ty) {
                    return false;
                }
                self.hd.flags[(pside - 1) as usize] += 1;
                log(&format!("Player({}) Placed Flag On ({}) Cords({x}/{y}) Type ({ty}) (Ares Flag ++ {} Elvine Flag ++ {}", crate::util::l1(&cname), crate::util::l1(&self.maps[m].name), self.hd.flags[0], self.hd.flags[1]));
                placed = true;
            } else if mode && ty == 2 && godh && rank == 0 && pside != self.hd.w1 {
                let z = self.hd.maps[m].winning_zone;
                if (z.0 - x).abs() > 1 || (z.1 - y).abs() > 1 || dyn_ty == 0 {
                    return false;
                }
                if !self.register_flag(m, x, y, side, dyn_ty) {
                    return false;
                }
                log(&format!("Player Placed Flag {} Cords({x}/{y})", crate::util::l1(&self.maps[m].name)));
                self.end_heldenian(ty, pside, true);
                let c = self.client_mut(h.unwrap()).unwrap();
                c.construction_point = (c.construction_point + 1000 * ek).min(MAXHELDENIANSUMMONPOINT);
                c.war_contribution += 100 * ek;
                placed = true;
            }
        } else if code == 2 && ty == 2 && godh {
            // fix (gameplay-visible): StartHeldenianMode sets this flag before m_bHeldenianMode, so the C++ test
            // `m_bHeldenianMode && ...` failed and GodH never had its defenders' flag
            let dyn_ty = match self.hd.w1 {
                ARESDEN => DYNAMICOBJECT_ARESDENFLAG,
                ELVINE => DYNAMICOBJECT_ELVINEFLAG,
                _ => return false,
            };
            if !self.register_flag(m, x, y, side, dyn_ty) {
                return false;
            }
            // fix: the C++ logged through m_pClientList[-1] (no player for the server's flag)
            log(&format!("Server Set Defending Flag On({}) Cords({x}/{y}) Type({ty})", crate::util::l1(&self.maps[m].name)));
            placed = true;
        }
        self.update_heldenian_status();
        // fix: the C++ returned TRUE when no flag was placed (outside a heldenian map, not a guild master...), so
        // the flag item was used up for nothing
        placed
    }

    /// the dynamic object + CMap::iRegisterOccupyFlag (the C++ passes 615 as the flag's EK count)
    fn register_flag(&mut self, m: usize, x: i16, y: i16, side: i32, dyn_ty: i32) -> bool {
        let s = self.maps[m].size_x.min(self.maps[m].size_y) as i16;
        let d = self.add_dynamic_object(0, 0, dyn_ty, m, x, y, 0, 0);
        if d == 0 {
            return false;
        }
        if x < 20 || x >= s - 20 || y < 20 || y >= s - 20 {
            log("Error registering MasterOccupyFlag.");
            return true;
        }
        self.hd.maps[m].flags.push(Flag { side, x, y, ek: 615, dyn_obj: d });
        true
    }

    /// `DeleteOccupyFlags`
    pub fn delete_occupy_flags(&mut self, m: usize) {
        for f in std::mem::take(&mut self.hd.maps[m].flags) {
            log(&format!("(*)Delete OccupyFlag: Side({}) XY({}, {})", f.side, f.x, f.y));
            let Some(o) = self.dyn_objs.get(f.dyn_obj).and_then(|o| o.as_ref()).map(|o| o.ty) else { continue };
            self.send_event_type_b(MSGID_DYNAMICOBJECT, MSGTYPE_REJECT as u16, m, f.x, f.y, [o as i16, f.dyn_obj as i16, 0, 0]);
            if let Some(t) = self.maps[m].tile_mut(f.x, f.y) {
                (t.dynamic_object_id, t.dynamic_object_type) = (0, 0);
            }
            self.dyn_objs[f.dyn_obj] = None;
        }
    }

    /// `HeldenianEndWarNow` (`local`: relay the result to the other servers)
    pub fn end_heldenian(&mut self, ty: i32, side: i32, local: bool) {
        if !self.hd.mode {
            return;
        }
        let msg;
        if ty == 1 {
            // fix: the C++ zeroed the tower counts before comparing them (the type 1 winner never changed), and
            // returned when this server has no BtField, leaving the heldenian on for good (on a server with no
            // heldenian map the random generators stayed stopped until a restart)
            let t = self.hd.towers;
            if t[0] > t[1] {
                self.hd.w1 = 1;
            } else if t[0] < t[1] {
                self.hd.w1 = 2;
            }
            msg = match self.hd.w1 {
                1 => "Heldenian Type 1 Finished. Aresden won.",
                2 => "Heldenian Type 1 Finished. Elvine won.",
                _ => "Heldenian Type 1 Finished. Draw.",
            };
        } else {
            msg = match side {
                ARESDEN => "Heldenian Type 2 Finished. Aresden won.",
                ELVINE => "Heldenian Type 2 Finished. Elvine won.",
                _ => "Heldenian Type 2 Finished. Draw.",
            };
            if side == ARESDEN || side == ELVINE {
                self.hd.w2 = side;
            }
        }
        (self.hd.towers, self.hd.flags, self.hd.kills, self.hd.dead) = ([0; 2], [0; 2], [0; 2], [0; 2]);
        let winner = if ty == 1 { self.hd.w1 } else { self.hd.w2 };
        for m in 0..self.maps.len() {
            if !self.is_heldenian_map(m) {
                continue;
            }
            self.delete_occupy_flags(m);
            self.kill_map_npcs(m);
            for j in self.handles() {
                let c = self.client(j).unwrap();
                if !c.is_init_complete || c.map_index != m {
                    continue;
                }
                self.notify(j, NOTIFY_HELDENIANVICTORY, &[winner], None);
                self.notify(j, NOTIFY_HELDENIANEND, &[], None);
                let c = self.client_mut(j).unwrap();
                c.special_event_id = 2;
                if c.side != side && c.admin_user_level == 0 {
                    (c.is_war_location, c.time_left_force_recall) = (true, 300);
                    self.notify(j, NOTIFY_TOBERECALLED, &[], None);
                    self.request_teleport(j, 0, None, -1, -1);
                }
            }
        }
        // fix: the towers killed above still counted down (the mode is on until the end), leaving -N towers for the
        // next heldenian (a second type 1 started with 0 towers left); the counts start from 0 again
        (self.hd.towers, self.hd.flags, self.hd.kills, self.hd.dead) = ([0; 2], [0; 2], [0; 2], [0; 2]);
        log(msg);
        self.write_heldenian_guid();
        self.hd.last_ty = ty;
        self.hd.mode = false;
        self.update_heldenian_status();
        log("(!!) Heldenian Mode OFF.");
        if local {
            let mut d = vec![GSM_ENDHELDENIAN as u8];
            for v in [ty, side, self.hd.w1, self.hd.w2] {
                d.extend_from_slice(&(v as i16).to_le_bytes());
            }
            self.stock_msg(&d);
        }
    }

    /// GSM_ENDHELDENIAN: the deciding server's winners, then the local end
    pub fn gsm_end_heldenian(&mut self, ty: i32, side: i32, w1: i32, w2: i32) {
        if !self.hd.mode {
            return;
        }
        (self.hd.w1, self.hd.w2) = (w1, w2);
        self.hd.towers = [0; 2];
        self.end_heldenian(ty, side, false);
    }

    /// `HeldenianPlayerKill`
    pub fn heldenian_player_kill(&mut self, victim_side: i32) {
        if victim_side == 1 {
            self.hd.dead[0] += 1;
            self.hd.kills[1] += 1;
        } else {
            self.hd.dead[1] += 1;
            self.hd.kills[0] += 1;
        }
        self.update_heldenian_status();
    }

    /// the heldenian part of NpcKilledHandler: a summoner's war contribution, the type 1 tower count
    pub fn heldenian_npc_killed(&mut self, attacker: usize, at: u8, i: usize) {
        if !self.hd.mode {
            return;
        }
        let n = self.npcs[i].as_ref().unwrap();
        let (m, t, side, hit_dice) = (n.map_index, n.type_, n.side, n.hit_dice);
        if !self.is_heldenian_map(m) {
            return;
        }
        if at == crate::map::OWNERTYPE_NPC {
            if let Some(a) = self.npcs.get(attacker).and_then(|a| a.as_ref()) {
                let o = a.follow_owner;
                if a.is_summoned && a.follow_owner_type == OWNERTYPE_PLAYER && !a.is_killed && self.client(o).is_some_and(|c| !c.is_killed) {
                    self.client_mut(o).unwrap().war_contribution += hit_dice;
                }
            }
        }
        if self.hd.ty == 1 && (t == NPC_CT || t == NPC_AGC) {
            let k = match side {
                1 => 0,
                2 => 1,
                _ => return,
            };
            self.hd.towers[k] -= 1;
            log(&format!("{} Tower Broken, Left TOWER {}", if k == 0 { "Aresden" } else { "Elvine" }, self.hd.towers[k]));
            self.update_heldenian_status();
            if self.hd.towers[k] == 0 {
                let ty = self.hd.ty;
                self.end_heldenian(ty, if k == 0 { ELVINE } else { ARESDEN }, true);
            }
        }
    }

    /// `CheckHeldenianResultCalculation` (the exp-stock timer)
    pub fn check_heldenian_result(&mut self, h: usize) {
        let (mode, ty, w1, w2) = (self.hd.mode, self.hd.ty, self.hd.w1, self.hd.w2);
        let Some(c) = self.client_mut(h) else { return };
        if mode || c.special_event_id <= 0 || c.war_contribution <= 0 {
            return;
        }
        c.war_contribution = c.war_contribution.min(100 * 1000);
        self.notify(h, NOTIFY_CONSTRUCTIONPOINT, &[0, 0, 1], None);
        let w = if ty == 1 { w1 } else { w2 };
        self.notify(h, NOTIFY_HELDENIANVICTORY, &[w], None);
        let c = self.client_mut(h).unwrap();
        c.war_contribution += if c.level < 180 { (200 - c.level) * 100 } else { 2000 };
        let e = c.war_contribution * if w == c.side { 50 } else { 25 };
        c.cs.exp_stock += e;
        log(&format!("Heldenian Exp:     PC({}) Got {e} xp.", crate::util::l1(&c.char_name)));
        (c.special_event_id, c.war_contribution, c.construction_point) = (0, 0, 0);
    }

    /// `RequestHeldenianScroll` (MSGID_REQUEST_HELDENIAN_SCROLL, from Gail): a guild master buys a summon scroll.
    pub fn request_heldenian_scroll(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !self.hd.mode || c.guild_rank != 0 {
            return;
        }
        if c.items.iter().all(|i| i.is_some()) {
            self.send_cannot_carry(h);
            return;
        }
        let id = crate::util::Rd::new(d).u16(26) as i32;
        log(&format!("PC({}) obtained a summon scroll ({id}).   {}({} {})", crate::util::l1(&c.char_name), crate::util::l1(&c.map_name), c.x, c.y));
        let need = scroll_points(id).unwrap_or_else(|| {
            log("Gail asked to create a wrong item!");
            999999
        });
        let c = self.client_mut(h).unwrap();
        // fix: the C++ took the points when there were enough and gave the scroll in any case
        if c.construction_point < need {
            return;
        }
        c.construction_point -= need;
        let (cp, wc, idn) = (c.construction_point, c.war_contribution, c.char_id_num);
        self.notify(h, NOTIFY_CONSTRUCTIONPOINT, &[cp, wc, 0], None);
        let Some(cfg) = self.data.item_by_id(id).cloned() else { return };
        let mut it = Item::new(&cfg);
        (it.touch_effect_type, it.touch_effect_value) = (ITET_UNIQUE_OWNER as i16, idn);
        let g = self.hd.guid;
        it.item_spec_effect_value[0] = g as u16 as i16;
        it.item_spec_effect_value[1] = (g >> 16) as u16 as i16;
        self.add_item(h, it);
    }

    /// `GetOccupyFlagHandler` (COMMONTYPE_REQ_GETOCCUPYFLAG): a guild master's flag for 2 EKs.
    pub fn get_occupy_flag(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if c.enemy_kill_count < 2 || !self.hd.mode || c.guild_rank != 0 {
            return;
        }
        let name: &[u8] = match c.side {
            1 => b"AresdenFlag",
            2 => b"ElvineFlag",
            _ => return,
        };
        let Some(cfg) = self.data.item_by_name(name).cloned() else { return };
        let mut it = Item::new(&cfg);
        let c = self.client_mut(h).unwrap();
        // fix: with exactly 2 EKs the C++ gave a flag with an unset EK number and kept the EKs (endless flags)
        c.enemy_kill_count -= 2;
        (it.item_spec_effect_value[1], it.touch_effect_type, it.touch_effect_value) = (1, ITET_UNIQUE_OWNER as i16, c.char_id_num);
        let (ek, name) = (c.enemy_kill_count, c.char_name.clone());
        if self.add_client_item_list(h, &it) {
            log(&format!("(*) Get Flag : Char({}) Flag-EK(1) Player-EK({ek})", crate::util::l1(&name)));
            self.send_item_obtained(h, &it);
            self.notify(h, NOTIFY_ENEMYKILLS, &[ek], None);
        } else {
            self.client_mut(h).unwrap().enemy_kill_count += 2;
            self.calc_total_weight(h);
            self.send_cannot_carry(h);
        }
    }

    /// ITEMEFFECTTYPE_OCCUPYFLAG of _bDepleteDestTypeItemUseEffect: true = the flag item is used up
    pub fn use_occupy_flag(&mut self, h: usize, k: usize, x: i16, y: i16) -> bool {
        let c = self.client_mut(h).unwrap();
        let it = c.items[k].as_mut().unwrap();
        if it.cfg.item_effect_value[1] != 0 {
            it.item_spec_effect_value[0] = 100;
        }
        let (side, ek, m) = (it.cfg.item_effect_value[0], it.item_spec_effect_value[1] as i32, c.map_index);
        let ok = self.set_heldenian_flag(m, x, y, side, ek, Some(h), 1);
        if !ok {
            self.notify(h, NOTIFY_NOTFLAGSPOT, &[], None);
        }
        ok
    }

    /// the summon scrolls (ITEMTYPE_USE_PERM, effect 5) of UseItemHandler
    pub fn use_heldenian_scroll(&mut self, h: usize, k: usize) {
        let c = self.client(h).unwrap();
        let (m, x, y) = (c.map_index, c.x as i32, c.y as i32);
        if !(self.hd.mode && self.is_heldenian_map(m) && cstr(&c.map_name) != b"GodH") {
            if !self.hd.mode {
                self.item_deplete(h, k, true);
            }
            return;
        }
        let it = c.items[k].as_ref().unwrap();
        let v = it.cfg.item_effect_value;
        let n = if v[5] == -1 { v[1] } else { -v[1] };
        let s = it.item_spec_effect_value;
        let guid = (s[0] as u16 as u32) | ((s[1] as u16 as u32) << 16);
        if it.touch_effect_type as i32 == ITET_UNIQUE_OWNER {
            if guid == self.hd.guid {
                self.player_magic(h, x, y, 31, true, n);
                let c = self.client_mut(h).unwrap();
                c.war_contribution = (c.war_contribution + 300).min(MAXWARCONTRIBUTION);
                if c.war_contribution >= 256 * 128 {
                    let (cp, wc) = (c.construction_point, c.war_contribution % (256 * 128));
                    self.notify(h, NOTIFY_CONSTRUCTIONPOINT, &[cp, wc, 1], None);
                }
            }
            // older scrolls go without effect
            self.item_deplete(h, k, true);
        } else {
            self.player_magic(h, x, y, 31, true, n);
            self.item_deplete(h, k, true);
        }
    }

    /// `CClient::CheckNearbyFlags`: a type 1 attacker near a flag of his side hits for x1.3
    pub fn check_nearby_flags(&self, h: usize) -> bool {
        let Some(c) = self.client(h) else { return false };
        if !self.hd.mode || self.hd.ty != 1 || cstr(&self.maps[c.map_index].name) != b"BtField" {
            return false;
        }
        self.hd.maps[c.map_index].flags.iter().any(|f| f.side == c.side && (f.x - c.x).abs() < FLAGRANGE_X && (f.y - c.y).abs() < FLAGRANGE_Y)
    }

    /// the GM commands `/heldenian` and `/endheldenian N` (admin level 4)
    pub fn heldenian_gm_command(&mut self, h: usize, cmd: &[u8]) -> bool {
        let Some(c) = self.client(h) else { return false };
        let (lvl, name) = (c.admin_user_level, String::from_utf8_lossy(cstr(&c.char_name)).into_owned());
        let end = cmd.starts_with(b"/endheldenian ");
        if !end && !cmd.starts_with(b"/heldenian") {
            return false;
        }
        if lvl < 4 {
            self.notify(h, NOTIFY_ADMINUSERLEVELLOW, &[], None);
            return true;
        }
        if end {
            self.send_ls_plain(MSGID_GAMEMASTERLOG, format!("GM Order({name:<10}): /endheldenian").as_bytes());
            let side = atoi(&cmd[14..]);
            let ty = self.hd.ty;
            self.end_heldenian(ty, side, true);
        } else {
            self.send_ls_plain(MSGID_GAMEMASTERLOG, format!("GM Order({name:<10}): /beginheldenian").as_bytes());
            self.start_heldenian(Some(h), None);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_file() {
        let mut h = Heldenian { w1: -1, w2: -1, ..Default::default() };
        parse_guid_file(b"", &mut h);
        assert_eq!((h.guid, h.w1, h.last_ty), (0, -1, 0), "an empty file changes nothing");
        parse_guid_file(b"Heldenian-GUID = 77\nHeldenian-Type1-Winner = 2\nHeldenian-Type2-Winner = 1\nHeldenian-LastType = 1\n", &mut h);
        assert_eq!((h.guid, h.w1, h.w2, h.last_ty), (77, 2, 1, 1));
    }

    #[test]
    fn names() {
        assert_eq!(tower_name(87, 2), Some("CT-Elvine"));
        assert_eq!(tower_name(39, 0), None);
        assert_eq!(scroll_points(879), Some(4000));
        assert_eq!(scroll_points(1), None);
    }
}
