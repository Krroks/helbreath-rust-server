//! Apocalypse (HG.cpp ~40682-41085 and its branches): the ApocalypseMap / ApocalypseMobGenType / ApocalypseBossMob /
//! DynamicGateType / DynamicGateCoord map info lines, Global/LocalStart/EndApocalypse(Mode), Notify_ApocalypseGateState,
//! Use_ApocalypseGate, GenerateApocalypseBoss, GenerateSlime, SendThunder, DoAbaddonThunderDamageHandler, the
//! DeleteNpc part (an emptied map opens its gate or calls its boss), DELAYEVENTTYPE_KILL_ABADDON / END_APOCALYPSE,
//! the /apocalypse and /endapocalypse GM commands.
use crate::client::{MAGICTYPE_HOLDOBJECT, MAGICTYPE_PROTECT};
use crate::consts::*;
use crate::delay::DelayEvent;
use crate::game::Game;
use crate::log::log;
use crate::map::{OWNERTYPE_NPC, OWNERTYPE_PLAYER};
use crate::npc::MOVETYPE_RANDOMAREA;
use crate::util::{atoi, cstr};

/// the apocalypse part of a map's info
#[derive(Clone, Default, Debug)]
pub struct AMap {
    pub is_apocalypse: bool,
    /// 1: no respawn during the apocalypse, 2: the boss when the map is empty (then 1)
    pub gen_type: i32,
    /// the gen type of the map info (restored at the end)
    pub gen_type_cfg: i32,
    pub boss: i32,
    /// left, top, right, bottom
    pub boss_rect: (i16, i16, i16, i16),
    /// 0 none, 1 open during the apocalypse, 2 open when the map is empty, 3 closed, 4 open (boss), 5 GM
    pub gate_type: i32,
    pub gate_type_cfg: i32,
    pub gate_rect: (i16, i16, i16, i16),
    pub gate_dest: Vec<u8>,
    pub gate_target: (i16, i16),
    /// m_iMaximumObject of the map info (the apocalypse sets it to 0 on the gen-type maps)
    pub max_object: i32,
}

#[derive(Default)]
pub struct Apocalypse {
    pub mode: bool,
    pub gate_open: bool,
    pub maps: Vec<AMap>,
}

impl Game {
    /// OnStartGameSignal: the apocalypse map info lines.
    pub fn apocalypse_init(&mut self) {
        self.ap.maps = (0..self.maps.len())
            .map(|m| {
                let mut a = AMap { max_object: self.maps[m].maximum_object, ..Default::default() };
                for (k, f) in &self.maps[m].info {
                    let v = |i: usize| f.get(i).map_or(0, |t| atoi(t));
                    match *k {
                        b"ApocalypseMap" => a.is_apocalypse = v(0) != 0,
                        b"ApocalypseMobGenType" => a.gen_type = v(0),
                        b"ApocalypseBossMob" => (a.boss, a.boss_rect) = (v(0), (v(1) as i16, v(2) as i16, v(3) as i16, v(4) as i16)),
                        b"DynamicGateType" => a.gate_type = v(0),
                        b"DynamicGateCoord" => {
                            a.gate_rect = (v(0) as i16, v(1) as i16, v(2) as i16, v(3) as i16);
                            a.gate_dest = f.get(4).cloned().unwrap_or_default();
                            a.gate_target = (v(5) as i16, v(6) as i16);
                        }
                        _ => {}
                    }
                }
                (a.gen_type_cfg, a.gate_type_cfg) = (a.gen_type, a.gate_type);
                a
            })
            .collect();
    }

    pub fn is_apocalypse_map(&self, m: usize) -> bool {
        self.ap.maps.get(m).is_some_and(|a| a.is_apocalypse)
    }

    /// `GlobalStartApocalypseMode` (`h`: the GM, 0 = local)
    pub fn global_start_apocalypse(&mut self, h: usize) {
        if h != 0 && self.client(h).is_some_and(|c| c.admin_user_level < 3) {
            self.notify(h, NOTIFY_ADMINUSERLEVELLOW, &[], None);
            return;
        }
        if self.ap.mode || self.cr.mode {
            return;
        }
        let c = self.client(h).filter(|_| h != 0).map(|c| (crate::util::l1(&c.char_name), crate::util::l1(&c.ip_address)));
        self.crusade_log(CRUSADELOG_APOCALYPSE, 0, 0, "Begin Apocalypse.");
        match c {
            Some((n, ip)) => {
                log(&format!("Admin Order({n}): /beginapocalypse \tAdminIP({ip})"));
                self.notify(h, NOTIFY_IPACCOUNTINFO, &[], Some(b"Apocalypse starting in progress."));
            }
            None => log("Local command: beginapocalypse"),
        }
        let mut d = vec![0u8; 5];
        d[0] = GSM_BEGINAPOCALYPSE as u8;
        self.stock_msg(&d);
        self.local_start_apocalypse();
    }

    /// `GlobalEndApocalypseMode` (`h` 0: the 15-minute event)
    pub fn global_end_apocalypse(&mut self, h: usize) {
        if !self.ap.mode {
            return;
        }
        match self.client(h).filter(|_| h > 0).map(|c| (crate::util::l1(&c.char_name), crate::util::l1(&c.ip_address))) {
            Some((n, ip)) => {
                log(&format!("Admin Order({n}): /endapocalypse \tAdminIP({ip})"));
                self.notify(h, NOTIFY_IPACCOUNTINFO, &[], Some(b"Apocalypse ending in progress."));
            }
            None => log("Automated: endapocalypse"),
        }
        self.crusade_log(CRUSADELOG_APOCALYPSE, 0, 0, "End Apocalypse.");
        let mut d = vec![0u8; 5];
        d[0] = GSM_ENDAPOCALYPSE as u8;
        self.stock_msg(&d);
        self.local_end_apocalypse();
    }

    /// `LocalStartApocalypse`
    pub fn local_start_apocalypse(&mut self) {
        if self.ap.mode {
            return;
        }
        (self.ap.mode, self.ap.gate_open) = (true, true);
        for i in self.handles() {
            self.notify(i, NOTIFY_APOCGATESTARTMSG, &[], None);
        }
        for m in 0..self.maps.len() {
            if self.ap.maps[m].gen_type != 0 {
                self.maps[m].maximum_object = 0;
            }
        }
        for i in self.handles() {
            self.apocalypse_gate_state(i);
        }
        log("(_)Apocalypse Mode ON.");
    }

    /// `LocalEndApocalypse`
    pub fn local_end_apocalypse(&mut self) {
        if !self.ap.mode {
            return;
        }
        (self.ap.mode, self.ap.gate_open) = (false, false);
        for i in self.handles() {
            self.notify(i, NOTIFY_APOCGATEENDMSG, &[], None);
            let c = self.client(i).unwrap();
            if c.admin_user_level == 0 && self.is_apocalypse_map(c.map_index) {
                self.notify(i, NOTIFY_TOBERECALLED, &[], None);
                self.request_teleport(i, 0, None, -1, -1);
            }
        }
        for m in 0..self.maps.len() {
            let a = &mut self.ap.maps[m];
            if a.gen_type != 0 || a.gen_type_cfg != 0 {
                // fix (gameplay-visible): the C++ "restored" m_iMaximumObject with itself (it stayed 0, the maps
                // never filled up again until a restart) and left the boss maps at gen type 1 (no boss in the next
                // apocalypse); both go back to the map info here
                (self.maps[m].maximum_object, a.gen_type) = (a.max_object, a.gen_type_cfg);
            }
            if a.gate_type == 4 {
                a.gate_type = 3;
            }
            if a.gate_type == 2 {
                self.generate_slime(m); // "to force the gate to close" (the mode is already off: nothing, as in the C++)
            }
        }
        // fix: the C++ notified inside the map loop and returned from the whole function at the first empty client
        // slot (the rest of the maps kept their gates, the log line never came)
        for i in self.handles() {
            self.apocalypse_gate_state(i);
        }
        log("(_) Apocalypse Mode OFF.");
    }

    fn gate_center(&self, m: usize) -> (i32, i32) {
        let r = self.ap.maps[m].gate_rect;
        ((r.0 as i32 + r.2 as i32) / 2, (r.1 as i32 + r.3 as i32) / 2)
    }

    /// `Notify_ApocalypseGateState`
    pub fn apocalypse_gate_state(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !self.ap.mode {
            return;
        }
        let (m, mn) = (c.map_index, c.map_name.clone());
        let (gx, gy) = self.gate_center(m);
        let a = self.ap.maps[m].clone();
        let open = |g: &mut Game, x, y| {
            g.notify(h, NOTIFY_APOCGATEOPEN, &[x, y], Some(&mn));
            g.notify(h, NOTIFY_APOCGATESTARTMSG, &[], None);
        };
        match a.gate_type {
            0 | 1 => {
                if self.ap.gate_open {
                    open(self, gx, gy);
                } else {
                    let (x, y) = if a.gate_type == 0 { (0, 0) } else { (gx, gy) };
                    self.notify(h, NOTIFY_APOCGATECLOSE, &[x, y], Some(&mn));
                    self.notify(h, NOTIFY_APOCGATEENDMSG, &[], None);
                }
            }
            2 => {
                if self.map_npcs[m].total_active == 0 {
                    open(self, gx, gy);
                } else {
                    self.notify(h, NOTIFY_APOCGATECLOSE, &[gx, gy], Some(&mn));
                }
            }
            3 => self.notify(h, NOTIFY_APOCGATECLOSE, &[gx, gy], Some(&mn)),
            4 => open(self, gx, gy),
            5 => self.notify(h, NOTIFY_APOCGATEOPEN, &[a.gate_rect.0 as i32 + 1, a.gate_rect.1 as i32 + 1], Some(&mn)),
            _ => {}
        }
    }

    /// `Use_ApocalypseGate` (3 s timer): standing in an open gate takes the player through.
    pub fn use_apocalypse_gate(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !self.ap.mode {
            return;
        }
        let (m, x, y) = (c.map_index, c.x, c.y);
        let a = &self.ap.maps[m];
        let ok = match a.gate_type {
            0 => false,
            1 => self.ap.gate_open,
            2 => self.map_npcs[m].total_active == 0,
            _ => true,
        };
        let r = a.gate_rect;
        if ok && x >= r.0 && x <= r.2 && y >= r.1 && y <= r.3 {
            let (d, t) = (a.gate_dest.clone(), a.gate_target);
            self.request_teleport(h, 2, Some(&d), t.0, t.1);
        }
    }

    /// the apocalypse part of DeleteNpc, after the map's active count went down
    pub fn apocalypse_npc_deleted(&mut self, m: usize) {
        if self.map_npcs[m].total_active != 0 {
            return;
        }
        match self.ap.maps[m].gen_type {
            1 => {
                // fix: the C++ stopped at the first empty client slot (return)
                for i in self.handles() {
                    self.apocalypse_gate_state(i);
                }
            }
            2 => {
                self.generate_apocalypse_boss(m);
                self.ap.maps[m].gen_type = 1;
            }
            _ => {}
        }
    }

    /// `GenerateApocalypseBoss`: the map's boss in its rect; Abaddon (81) dies by itself in 5 minutes and ends the
    /// apocalypse 15 minutes later.
    fn generate_apocalypse_boss(&mut self, m: usize) {
        if self.map_npcs[m].total_active != 0 || !self.ap.mode {
            return;
        }
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return };
        let a = self.ap.maps[m].clone();
        let npc_name: &[u8] = match a.boss {
            0 => {
                self.maps[m].set_naming_value_empty(nv);
                return;
            }
            49 => b"Hellclaw",
            50 => b"Tigerworm",
            66 => b"Wyvern",
            73 => b"Fire-Wyvern",
            81 => b"Abaddon",
            _ => b"Demon",
        };
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        let Some(b) = self.create_npc(npc_name, &name, m, 0, MOVETYPE_RANDOMAREA, None, &[-1; 10], Some(a.boss_rect), -1) else {
            // fix: the C++ went on with m_pNpcList[MAXNPCS] (no boss found) and crashed
            self.maps[m].set_naming_value_empty(nv);
            return;
        };
        self.npcs[b].as_mut().unwrap().is_master = true;
        let txt = format!("{} has spawned in {}", crate::util::l1(npc_name), crate::util::l1(&self.maps[m].name));
        log(&txt);
        self.crusade_log(CRUSADELOG_APOCALYPSE, 0, 0, &txt);
        let n = self.npcs[b].as_ref().unwrap();
        // fix: the C++ sent the boss's x as its y on the minimap
        let (bx, by) = (n.x as i32, n.y as i32);
        let mname = cstr(&self.maps[m].name).to_vec();
        for i in self.handles() {
            let c = self.client(i).unwrap();
            if !c.is_init_complete {
                continue;
            }
            if cstr(&self.maps[c.map_index].name).starts_with(&mname) {
                self.notify(i, NOTIFY_MONSTEREVENT_POSITION, &[bx, by, a.boss], None);
            }
            if a.boss == 81 {
                self.notify(i, NOTIFY_IPACCOUNTINFO, &[], Some(b"Abbadon has appeared ..."));
            }
        }
        if a.boss == 81 {
            let now = self.now();
            self.register_delay_event_at(DelayEvent { ty: DELAYEVENTTYPE_KILL_ABADDON, effect: 0, trigger: now.wrapping_add(5 * 60 * 1000), target: b, tt: OWNERTYPE_NPC, m, dx: 0, dy: 0, v1: 0, v2: 0 });
            self.register_delay_event_at(DelayEvent { ty: DELAYEVENTTYPE_END_APOCALYPSE, effect: 0, trigger: now.wrapping_add(15 * 60 * 1000), target: 0, tt: 0, m, dx: 0, dy: 0, v1: 0, v2: 0 });
        }
        if self.ap.maps[m].gate_type == 3 {
            self.ap.maps[m].gate_type = 4;
            for i in self.handles() {
                self.apocalypse_gate_state(i);
            }
        }
    }

    /// `GenerateSlime`
    fn generate_slime(&mut self, m: usize) {
        if self.map_npcs[m].total_active != 0 || !self.ap.mode {
            return;
        }
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return };
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        let rect = self.ap.maps[m].boss_rect;
        match self.create_npc(b"Slime", &name, m, 0, MOVETYPE_RANDOMAREA, None, &[-1; 10], Some(rect), 0) {
            Some(i) => self.npcs[i].as_mut().unwrap().is_master = true,
            None => self.maps[m].set_naming_value_empty(nv),
        }
    }

    /// DELAYEVENTTYPE_KILL_ABADDON / END_APOCALYPSE
    pub fn apocalypse_delay_event(&mut self, ty: i32, target: usize) {
        match ty {
            DELAYEVENTTYPE_KILL_ABADDON => {
                // fix: the C++ killed whatever NPC held the slot 5 minutes later
                if self.npcs.get(target).and_then(|n| n.as_ref()).is_some_and(|n| n.type_ == 81) {
                    self.npc_killed(target, OWNERTYPE_NPC, target, 0);
                }
            }
            DELAYEVENTTYPE_END_APOCALYPSE => self.global_end_apocalypse(0),
            _ => {}
        }
    }

    /// `SendThunder`
    fn send_thunder(&mut self, h: usize, x: i16, y: i16, v3: i16, v4: i16) {
        let mut b = vec![0u8; 18];
        b[..4].copy_from_slice(&MSGID_EVENT_COMMON.to_le_bytes());
        b[4..6].copy_from_slice(&(COMMONTYPE_MAGIC as u16).to_le_bytes());
        for (k, v) in [x, y, x, y, v3, v4].into_iter().enumerate() {
            b[6 + k * 2..8 + k * 2].copy_from_slice(&v.to_le_bytes());
        }
        self.send(h, &b);
    }

    /// `DoAbaddonThunderDamageHandler(-1)` (the 1-minute timer): 1 in 15, thunder on abaddon for 101-120 damage.
    pub fn abaddon_thunder(&mut self) {
        if !self.ap.mode || self.dice(1, 15) != 13 {
            return;
        }
        for i in self.handles() {
            let c = self.client(i).unwrap();
            if !c.map_name.starts_with(b"abaddon") {
                continue;
            }
            let (x, y, t) = (c.x, c.y, c.type_);
            // MSVC evaluates SendThunder's arguments right to left: the y roll comes first
            let (e, ry, rx) = match self.dice(1, 4) {
                1 => (203, self.dice(1, 5), -self.dice(1, 7)),
                2 => (202, -self.dice(1, 5), self.dice(1, 7)),
                3 => (201, 5 - self.dice(1, 9), self.dice(1, 7)),
                _ => (200, 7 - self.dice(1, 4), 5 - self.dice(1, 9)),
            };
            let (dx, dy) = (rx, ry);
            self.send_thunder(i, x, y, e, t);
            self.send_thunder(i, x + dx as i16, y + dy as i16, 161, t);
            if self.client(i).unwrap().admin_user_level > 0 {
                continue; // fix: the C++ returned, sparing everyone after a GM
            }
            let mut dmg = self.dice(1, 20) + 100;
            let c = self.client_mut(i).unwrap();
            if matches!(c.magic_effect_status[MAGICTYPE_PROTECT as usize], 2 | 5) {
                dmg /= 2;
            }
            c.hp -= dmg;
            if c.hp <= 0 {
                self.killed_handler(i, i, 0, dmg);
                continue;
            }
            self.notify(i, NOTIFY_HP, &[], None);
            self.send_event_near_type_a(i, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, dmg as i16, 0, 0);
            let c = self.client(i).unwrap();
            // fix: the C++ tested m_bSkillUsingStatus[19] (pretend corpse is 10)
            if !c.skill_using[crate::combat::SKILL_PRETENDCORPSE as usize] {
                let (m, x, y) = (c.map_index, c.x, c.y);
                self.maps[m].clear_owner(i as i16, OWNERTYPE_PLAYER, x, y);
                self.maps[m].set_owner(i as i16, OWNERTYPE_PLAYER, x, y);
            }
            let hold = self.client(i).unwrap().magic_effect_status[MAGICTYPE_HOLDOBJECT as usize];
            if hold != 0 {
                let v = match hold {
                    20 => 20,
                    1 => 1,
                    _ => 2,
                };
                self.notify(i, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_HOLDOBJECT, v], None);
                self.remove_delay_events(i, OWNERTYPE_PLAYER, MAGICTYPE_HOLDOBJECT);
                self.client_mut(i).unwrap().magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] = 0;
            }
        }
    }

    /// the 3 s player timer: outside an apocalypse its maps recall their players; the gates take them through
    pub fn apocalypse_player_timer(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if self.is_apocalypse_map(c.map_index) && !self.ap.mode && c.admin_user_level == 0 {
            let c = self.client_mut(h).unwrap();
            (c.is_war_location, c.time_left_force_recall) = (true, 0);
        }
        self.use_apocalypse_gate(h);
    }

    /// the GM commands `/apocalypse` and `/endapocalypse` (admin level 4)
    pub fn apocalypse_gm_command(&mut self, h: usize, cmd: &[u8]) -> bool {
        let Some(c) = self.client(h) else { return false };
        let (lvl, name) = (c.admin_user_level, String::from_utf8_lossy(cstr(&c.char_name)).into_owned());
        let start = cmd.starts_with(b"/apocalypse");
        if !start && !cmd.starts_with(b"/endapocalypse") {
            return false;
        }
        if lvl >= 4 {
            let what = if start { "beginapocalypse" } else { "endapocalypse" };
            self.send_ls_plain(MSGID_GAMEMASTERLOG, format!("GM Order({name:<10}): /{what}").as_bytes());
            if start {
                self.global_start_apocalypse(h);
            } else {
                self.global_end_apocalypse(h);
            }
        }
        true
    }
}
