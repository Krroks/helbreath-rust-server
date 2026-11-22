//! `CGame` core: the single world loop (OnTimer every TICKDELAY ms + the message queue), client slots
//! (`bAccept` / `DeleteClient`) and the Login link (`bSendMsgToLS`, the 5 sub-sockets, registration, config frames).
use crate::client::*;
use crate::config::{ProgramCfg, Settings};
use crate::consts::*;
use crate::droplist::DropManager;
use crate::gamedata::GameData;
use crate::log::{log, log_file};
use crate::map::{Map, OWNERTYPE_PLAYER};
use crate::net::{Ev, Tx};
use crate::rng::Rng;
use crate::util::{cstr, l1, Rd, W};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

pub const MAXSUBLOGSOCK: usize = 5;
pub const MAXCLIENTS_: usize = MAXCLIENTS as usize;
pub const CLIENTTIMEOUT_MS: u32 = 18_000;

pub enum From {
    Client(usize),
    Login,
}

pub struct Msg {
    pub from: From,
    pub key: u8,
    pub body: Vec<u8>,
}

/// m_stOldClientList: who used a slot lately, to tell outdated messages from corrupt ones (log only).
struct ConfirmedIp {
    ip: Vec<u8>,
    time: u32,
}

pub struct Game {
    pub cfg: ProgramCfg,
    pub settings: Settings,
    pub data: GameData,
    pub maps: Vec<Map>,
    pub drops: DropManager,
    pub rng: Rng,
    pub clients: Vec<Option<Client>>,
    conn_slot: HashMap<u64, usize>,
    pub queue: VecDeque<Msg>,
    ev: UnboundedSender<Ev>,
    ls_addr: String,
    pub ls: [Option<Tx>; MAXSUBLOGSOCK],
    ls_connecting: [bool; MAXSUBLOGSOCK],
    ls_available: [bool; MAXSUBLOGSOCK],
    ls_connected: [bool; MAXSUBLOGSOCK],
    ls_active_count: usize,
    cur_ls: usize,
    pub gsid: u16,
    /// m_cGateServerStockMsg after its 6-byte header (bStockMsgToGateServer)
    pub stock: Vec<u8>,
    /// m_sForceRecallTime (minutes; 0 = the raid times of Settings.cfg)
    pub force_recall_time: i32,
    /// m_stCityStatus[side].iWins / iCrimes / iFunds (counted on enemy kills / PK penalties / the gold of shops, repairs
    /// and charged teleports; nothing reads them)
    pub city_funds: [u64; MAXSIDES as usize],
    pub city_wins: [u64; MAXSIDES as usize],
    pub city_crimes: [u64; MAXSIDES as usize],
    /// m_stPartyInfo: the local members of each party, in join order
    pub parties: HashMap<i32, Vec<usize>>,
    pub registered: bool,
    received_all_config: bool,
    pub game_started: bool,
    pub day_or_night: u8,
    /// m_bManualTime: /time 1|2 holds the day or the night against CheckDayOrNightMode
    pub manual_time: bool,
    pub total_clients: i32,
    pub max_clients: i32,
    pub level_exp_table: Vec<u32>,
    pub on_exit_process: bool,
    pub exit_process_time: u32,
    pub shutdown_code: u8,
    final_shutdown_count: i32,
    is_server_shutdowned: bool,
    pub quit: bool,
    start: Instant,
    t1: u32,
    t2: u32,
    t3: u32,
    t4: u32,
    t5: u32,
    t6: u32,
    fish_time: u32,
    whether_time: u32,
    sector_time: u32,
    /// m_dwSpecialEventTime / m_bIsSpecialEventTime / m_cSpecialEventType (SpecialEventHandler, read by MobGenerator)
    pub special_event_time: u32,
    pub special_event: bool,
    pub recent_dcs: Vec<crate::fightzone::RecentDc>,
    pub special_event_type: u8,
    pub total_game_server_clients: i32,
    /// m_onlineCntAdd: what the C++ adds to the online count it shows (/who, the website), drifting to 2d4 / 2d6 / 2d8
    pub online_cnt_add: i32,
    /// m_startTime
    pub start_time: u32,
    pub npcs: Vec<Option<crate::npc::Npc>>,
    pub map_npcs: Vec<crate::npc::MapNpcs>,
    /// ground items per (map, x, y), newest first (CTile::m_pItem)
    pub ground: std::collections::HashMap<(usize, i16, i16), Vec<crate::client::Item>>,
    /// m_stGroundNpcItem: (expiry time, map, x, y, tag), at most MAXGROUNDITEMS
    pub ground_timers: Vec<(u32, usize, i16, i16, u32)>,
    pub ground_tag: u32,
    /// m_dwCrusadeGUID from GameData/CrusadeGUID.txt
    pub crusade_guid: u32,
    /// the crusade state (crusade.rs)
    pub cr: crate::crusade::Crusade,
    /// the heldenian state (heldenian.rs)
    pub hd: crate::heldenian::Heldenian,
    /// the apocalypse state (apocalypse.rs)
    pub ap: crate::apocalypse::Apocalypse,
    /// the astoria events (astoria.rs)
    pub ast: crate::astoria::Astoria,
    /// Schedule.cfg's events (scheduler.rs)
    pub schedules: Vec<crate::scheduler::Schedule>,
    /// notice.txt (NoticeHandler): the messages, the last send time and the last index
    pub notices: Vec<Vec<u8>>,
    pub fz: crate::fightzone::FightZones,
    pub notice_time: u32,
    pub prev_notice: i32,
    confirmed_ips: Vec<ConfirmedIp>,
    pub delay_events: Vec<crate::delay::DelayEvent>,
    /// m_pDynamicObjectList (index 0 unused)
    pub dyn_objs: Vec<Option<crate::dynamic::DynObj>>,
    /// m_pMineral / m_pFish (index 0 unused)
    pub minerals: Vec<Option<crate::skills::Mineral>>,
    pub fish: Vec<Option<crate::skills::Fish>>,
    /// time-based reseeds of rand() (`srand(time(NULL))` every 3 minutes) unless a fixed seed was given
    reseed: bool,
}

/// `iGetLevelExp`, with the C++ int arithmetic (it wraps above level ~880).
pub fn level_exp(l: i32) -> u32 {
    if l == 0 {
        return 0;
    }
    let q = l / 17;
    (l.wrapping_mul(l)).wrapping_add(l.wrapping_mul(50i32.wrapping_add(l.wrapping_mul(q).wrapping_mul(q)))) as u32
}

/// `bReadCrusadeGUIDFile`: (CrusadeGUID, winner-side)
fn read_crusade_guid() -> (u32, i32) {
    let raw = std::fs::read("GameData/CrusadeGUID.txt").unwrap_or_default();
    let txt = crate::util::text_mode(&raw);
    let mut t = crate::util::StrTok::new(&txt, b"= \t\n");
    let (mut guid, mut winner) = (0, 0);
    while let Some(k) = t.get() {
        if k.starts_with(b"CrusadeGUID") {
            guid = crate::util::atoi(&t.get().unwrap_or_default()) as u32;
        } else if k.starts_with(b"winner-side") {
            winner = crate::util::atoi(&t.get().unwrap_or_default());
        }
    }
    (guid, winner)
}

fn unix_time() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32)
}

impl Game {
    pub fn new(cfg: ProgramCfg, settings: Settings, maps: Vec<Map>, ev: UnboundedSender<Ev>, ls_addr: String, seed: Option<u32>) -> Game {
        let start = Instant::now();
        let mut g = Game {
            cfg,
            settings,
            data: GameData::new(),
            maps,
            drops: DropManager::new(MAXNPCTYPES as usize),
            rng: Rng::new(seed.unwrap_or_else(unix_time)),
            clients: (0..MAXCLIENTS_).map(|_| None).collect(),
            conn_slot: HashMap::new(),
            queue: VecDeque::new(),
            ev,
            ls_addr,
            ls: Default::default(),
            ls_connecting: [false; MAXSUBLOGSOCK],
            ls_available: [false; MAXSUBLOGSOCK],
            ls_connected: [false; MAXSUBLOGSOCK],
            ls_active_count: 0,
            cur_ls: 0,
            stock: Vec::new(),
            force_recall_time: 0,
            city_funds: [0; MAXSIDES as usize],
            city_wins: [0; MAXSIDES as usize],
            city_crimes: [0; MAXSIDES as usize],
            parties: HashMap::new(),
            gsid: 0xFFFF, // GSID = -1 until the Login assigns one
            registered: false,
            received_all_config: false,
            game_started: false,
            day_or_night: 1,
            manual_time: false,
            total_clients: 0,
            max_clients: 0,
            level_exp_table: (0..900).map(level_exp).collect(),
            on_exit_process: false,
            exit_process_time: 0,
            shutdown_code: 0,
            final_shutdown_count: 0,
            is_server_shutdowned: false,
            quit: false,
            start,
            t1: 0,
            t2: 0,
            t3: 0,
            t4: 0,
            t5: 0,
            t6: 0,
            fish_time: 0,
            whether_time: 0,
            sector_time: 0,
            special_event_time: 0, // (set to the start time below)
            special_event: false,
            recent_dcs: Vec::new(),
            special_event_type: 0,
            total_game_server_clients: 0,
            online_cnt_add: 1,
            start_time: 0,
            npcs: (0..crate::npc::MAXNPCS).map(|_| None).collect(),
            map_npcs: Vec::new(),
            ground: Default::default(),
            ground_timers: Vec::new(),
            ground_tag: 0,
            crusade_guid: read_crusade_guid().0,
            cr: crate::crusade::Crusade { winner_side: read_crusade_guid().1, ..Default::default() },
            hd: Default::default(),
            ap: Default::default(),
            ast: Default::default(),
            schedules: Vec::new(),
            notices: Vec::new(),
            fz: Default::default(),
            notice_time: 0,
            prev_notice: -1,
            confirmed_ips: Vec::new(),
            delay_events: Vec::new(),
            dyn_objs: Vec::new(),
            minerals: (0..MAXMINERALS).map(|_| None).collect(),
            fish: (0..MAXFISHS).map(|_| None).collect(),
            reseed: seed.is_none(),
        };
        let now = g.now();
        (g.t1, g.t2, g.t3, g.t4, g.t5, g.t6, g.fish_time, g.whether_time, g.sector_time) = (now, now, now, now, now, now, now, now, now);
        g.special_event_time = now; // m_dwSpecialEventTime = dwTime: the first mob event is 5 minutes after the start
        g.notice_init();
        g.notice_time = now;
        g.fightzone_init();
        g.day_or_night = if crate::chrono_now().minute >= NIGHTTIME as u32 { 2 } else { 1 };
        g
    }

    /// timeGetTime(): milliseconds, wrapping like a DWORD. Starts well above 0 so "== NULL" tests stay meaningful.
    pub fn now(&self) -> u32 {
        (self.start.elapsed().as_millis() as u32).wrapping_add(1_000_000)
    }

    pub fn map_index(&self, name: &[u8]) -> Option<usize> {
        let n = cstr(name);
        self.maps.iter().position(|m| m.name[..m.name.len().min(10)] == n[..n.len().min(10)])
    }

    pub fn dice(&mut self, t: i32, r: i32) -> i32 {
        self.rng.dice(t, r)
    }

    // ---------------------------------------------------------------- client sockets

    /// Send a body to a client (`iSendMsg(cData, size, cKey)`); key 0 = plain.
    pub fn send_key(&self, h: usize, body: &[u8], key: u8) {
        if let Some(c) = self.clients.get(h).and_then(|c| c.as_ref()) {
            let _ = c.tx.send(proto::encode(key, body));
        }
    }
    pub fn send(&self, h: usize, body: &[u8]) {
        self.send_key(h, body, 0);
    }

    pub fn client(&self, h: usize) -> Option<&Client> {
        self.clients.get(h).and_then(|c| c.as_ref())
    }
    pub fn client_mut(&mut self, h: usize) -> Option<&mut Client> {
        self.clients.get_mut(h).and_then(|c| c.as_mut())
    }
    /// Players for the `m_iClientShortCut` loops (ascending handles; the C++ shortcut list is in accept order).
    pub fn handles(&self) -> Vec<usize> {
        (1..MAXCLIENTS_).filter(|&i| self.clients[i].is_some()).collect()
    }

    /// `bAccept`
    fn accept(&mut self, conn: u64, ip: Vec<u8>, tx: Tx) {
        if !self.data.all_ok() || self.on_exit_process || !self.game_started {
            return; // tx dropped: CLOSE_ANYWAY
        }
        let Some(h) = (1..MAXCLIENTS_).find(|&i| self.clients[i].is_none()) else { return };
        if let Some(p) = self.confirmed_ips.iter().position(|c| c.ip == ip) {
            self.confirmed_ips.remove(p);
        }
        // at most 2 connections per IP (the C++ counts the new one too: > 2 means a third)
        if self.clients.iter().flatten().filter(|c| c.ip_address == ip).count() + 1 > 2 {
            return;
        }
        let now = self.now();
        log(&format!("<{h}> New client: {}", l1(&ip)));
        self.clients[h] = Some(Client::new(h, conn, tx, ip, now));
        self.conn_slot.insert(conn, h);
        self.total_clients += 1;
        self.max_clients = self.max_clients.max(self.total_clients);
    }

    pub fn on_event(&mut self, ev: Ev) {
        if std::env::var_os("HG_TRACE").is_some() {
            let what = match &ev {
                Ev::Accept { conn, .. } => format!("accept {conn}"),
                Ev::Read { conn, body, .. } => format!("read {conn} {:08x} {}", Rd::new(body).u32(0), body.len()),
                Ev::Closed { conn } => format!("closed {conn}"),
                Ev::LsUp { i, .. } => format!("ls up {i}"),
                Ev::LsRead { i, body } => format!("ls read {i} {:08x}", Rd::new(body).u32(0)),
                Ev::LsDown { i } => format!("ls down {i}"),
                Ev::Stdin(l) => format!("stdin {l}"),
            };
            log(&format!("ev {what}"));
        }
        match ev {
            Ev::Accept { conn, ip, tx } => self.accept(conn, ip, tx),
            Ev::Read { conn, key, body } => {
                if let Some(&h) = self.conn_slot.get(&conn) {
                    let now = self.now();
                    if let Some(c) = self.client_mut(h) {
                        c.time = now;
                    }
                    self.queue.push_back(Msg { from: From::Client(h), key, body });
                }
            }
            Ev::Closed { conn } => {
                if let Some(&h) = self.conn_slot.get(&conn) {
                    let c = self.client(h).unwrap();
                    log(&format!("<{h}> Client Disconnected! ({})", l1(&c.ip_address)));
                    if self.now().wrapping_sub(c.logout_hack_check) < 1000 {
                        log_file(&format!("Logout Hack: ({}) Player: ({})", l1(&c.ip_address), l1(&c.char_name)));
                    }
                    self.delete_client(h, true, true, true, false);
                }
            }
            Ev::LsUp { i, tx } => self.ls_connected(i, tx),
            Ev::LsRead { body, .. } => self.queue.push_back(Msg { from: From::Login, key: 0, body }),
            Ev::LsDown { i } => self.ls_down(i),
            Ev::Stdin(line) => match line.trim() {
                // F1+F4 in the C++ window
                "shutdown" => self.local_shutdown(),
                "quit" => self.quit = true,
                _ => {}
            },
        }
    }

    // ---------------------------------------------------------------- timer

    /// `OnTimer`
    pub fn on_timer(&mut self) {
        let now = self.now();
        if now.wrapping_sub(self.t1) > 300 {
            self.npc_process();
            self.t1 = now;
        }
        self.msg_process();
        if now.wrapping_sub(self.t2) > 3000 {
            self.check_client_response_time();
            self.send_alive();
            self.check_day_or_night_mode();
            self.t2 = now;
            if !self.game_started && self.data.all_ok() && self.ls_active_count == MAXSUBLOGSOCK {
                self.on_start_game_signal();
                self.game_started = true;
            }
        }
        if now.wrapping_sub(self.t6) > 1000 {
            self.astoria_check_victory();
            self.delay_event_processor();
            self.send_stock_msg();
            if self.final_shutdown_count != 0 {
                self.final_shutdown_count -= 1;
                log(&format!("Final Shutdown...{}", self.final_shutdown_count));
                if self.final_shutdown_count <= 1 {
                    self.quit = true;
                    return;
                }
            }
            if !self.registered && self.ls[0].is_none() && !self.ls_connecting[0] {
                self.ls_connect(0);
            } else if self.registered {
                for b in 1..MAXSUBLOGSOCK {
                    if self.ls[b].is_none() && !self.ls_connecting[b] && !self.ls_connected[b] && self.ls_connected[b - 1] {
                        self.ls_connect(b);
                    }
                }
            }
            self.t6 = now;
        }
        if now.wrapping_sub(self.t3) > 3000 {
            self.sync_middleland();
            self.check_dynamic_objects();
            self.dynamic_object_effects();
            self.notice_handler();
            self.special_event_handler();
            self.fz_age_disconnects();
            self.t3 = now;
            self.confirmed_ips.retain(|c| now.wrapping_sub(c.time) <= 30_000);
        }
        if now.wrapping_sub(self.t4) > 600 {
            self.mob_generator();
            self.t4 = now;
        }
        if now.wrapping_sub(self.t5) > 180_000 {
            self.t5 = now;
            if self.reseed {
                self.rng.srand(unix_time());
            }
        }
        if now.wrapping_sub(self.fish_time) > 4000 {
            self.fish_processor();
            self.fish_generator();
            self.send_collected_mana();
            self.scheduler();
            self.fish_time = now;
        }
        if now.wrapping_sub(self.whether_time) > 60_000 {
            self.update_website_stats();
            self.whether_processor();
            self.abaddon_thunder();
            if self.hd.mode {
                self.update_heldenian_status();
            }
            self.whether_time = now;
            self.tile_cleaner();
            // m_onlineCntAdd moves one step towards 2d4 (first 5 min), 2d6 (first 30 min), then 2d8
            let up = now.wrapping_sub(self.start_time);
            let add = if up < 5 * 60_000 { self.dice(2, 4) } else if up < 30 * 60_000 { self.dice(2, 6) } else { self.dice(2, 8) };
            self.online_cnt_add += (add - self.online_cnt_add).signum();
        }
        self.fightzone_timer();
        if !self.is_server_shutdowned && self.on_exit_process && now.wrapping_sub(self.exit_process_time) > 2000 {
            if self.force_player_disconnect(15) == 0 {
                log("(!!!) GAME SERVER SHUTDOWN PROCESS COMPLETED! All players are disconnected.");
                self.is_server_shutdowned = true;
                if self.final_shutdown_count == 0 {
                    self.final_shutdown_count = 20;
                }
            }
            self.exit_process_time = now;
        }
        if now.wrapping_sub(self.sector_time) > 10_000 {
            for m in &mut self.maps {
                m.update_sector_info();
            }
            self.mineral_generator();
            self.sector_time = now;
        }
    }

    /// `OnStartGameSignal` (the map info files were read at boot; crusade/scheduler files come with later layers)
    fn on_start_game_signal(&mut self) {
        let t = std::time::Instant::now();
        self.crusade_init();
        self.heldenian_init();
        self.apocalypse_init();
        self.astoria_init();
        self.scheduler_init();
        self.init_map_npcs();
        let n = self.npcs.iter().flatten().count();
        log(&format!("(!) Game Server Activated. {n} NPCs placed in {} ms", t.elapsed().as_millis()));
    }

    fn local_shutdown(&mut self) {
        if self.on_exit_process {
            return;
        }
        self.shutdown_code = 2;
        self.on_exit_process = true;
        self.exit_process_time = self.now();
        log("(!) GAME SERVER SHUTDOWN PROCESS BEGIN(by Local command)!!!");
        self.send_ls_plain(MSGID_GAMESERVERSHUTDOWNED, &[]);
    }

    /// `_iForcePlayerDisconect`: logs out up to `n` players with a save; returns how many were found.
    pub fn force_player_disconnect(&mut self, n: usize) -> usize {
        let hs: Vec<usize> = self.handles().into_iter().take(n).collect();
        for &h in &hs {
            if self.client(h).is_some_and(|c| c.is_init_complete) {
                self.delete_client(h, true, true, true, false);
            } else {
                self.delete_client(h, false, false, true, false);
            }
        }
        hs.len()
    }

    /// `CheckClientResponseTime` (the per-player timers that belong to layer 2; regen, hunger, poison, exp and
    /// special-ability effects come with the combat/items layers).
    fn check_client_response_time(&mut self) {
        let now = self.now();
        for h in self.handles() {
            let Some(c) = self.clients[h].as_mut() else { continue };
            if now.wrapping_sub(c.time) > CLIENTTIMEOUT_MS {
                let init = c.is_init_complete;
                self.delete_client(h, init, init, true, false);
                continue;
            }
            if !c.is_init_complete {
                continue;
            }
            c.time_left_shutup = (c.time_left_shutup - 1).max(0);
            c.time_left_rating = (c.time_left_rating - 1).max(0);
            self.player_life_timers(h);
            let Some(c) = self.clients[h].as_mut() else { continue };
            let fight = self.maps[c.map_index].is_fight_zone;
            let c = self.client_mut(h).unwrap();
            if !fight && now.wrapping_sub(c.auto_save_time) > AUTOSAVETIME as u32 {
                c.auto_save_time = now;
                self.send_save(h, MSGID_REQUEST_SAVEPLAYERDATA, false);
            }
            let c = self.client_mut(h).unwrap();
            if c.special_ability_time == 3 {
                self.notify(h, NOTIFY_SPECIALABILITYENABLED, &[], None);
            }
            let c = self.client_mut(h).unwrap();
            c.special_ability_time = (c.special_ability_time - 3).max(0);
            self.special_ability_timer(h, now);
            let crusade = self.cr.mode;
            let c = self.client_mut(h).unwrap();
            c.locked_map_time -= 3;
            if c.locked_map_time < 0 {
                c.locked_map_time = 0;
                c.locked_map_name = b"NONE".to_vec();
            }
            c.dead_penalty_time = (c.dead_penalty_time - 3).max(0);
            if c.is_war_location {
                // in a crusade the recall time only runs inside the buildings
                if !crusade || c.is_in_building {
                    c.time_left_force_recall -= 1;
                }
                if c.time_left_force_recall <= 0 {
                    c.time_left_force_recall = 0;
                    c.war_begin_time = now;
                    c.is_war_location = false;
                    self.notify(h, NOTIFY_TOBERECALLED, &[], None);
                    self.request_teleport(h, 0, None, -1, -1);
                }
            }
            // fix: the C++ `break`s out of the whole loop here when this player was deleted, skipping everyone after
            let Some(c) = self.clients[h].as_mut() else { continue };
            if c.skill_msg_recv_count >= 2 {
                self.delete_client(h, true, true, true, false);
                continue;
            }
            c.skill_msg_recv_count = 0;
            self.apocalypse_player_timer(h);
            self.astoria_player_timer(h);
            let Some(c) = self.clients[h].as_mut() else { continue };
            let limit = self.maps[c.map_index].level_limit;
            if c.level < limit && c.admin_user_level < 1 {
                self.notify(h, NOTIFY_TOBERECALLED, &[], None);
                self.request_teleport(h, 0, None, -1, -1);
            }
            // the critical-attack recharge: one more (up to level / 10) every 13 checks
            let Some(c) = self.clients[h].as_mut() else { continue };
            c.cs.super_attack_count += 1;
            if c.cs.super_attack_count > 12 {
                c.cs.super_attack_count = 0;
                if c.super_attack_left < c.level / 10 {
                    c.super_attack_left += 1;
                }
                self.notify(h, NOTIFY_SUPERATTACKLEFT, &[], None);
            }
            if self.client(h).is_some_and(|c| c.cr.is_sending_map_status) {
                self.send_map_status(h);
            }
            if self.client(h).is_some_and(|c| c.construction_point > 0) {
                self.check_commander_cp(h);
            }
        }
    }

    /// `CheckDayOrNightMode`
    pub(crate) fn check_day_or_night_mode(&mut self) {
        if self.manual_time {
            return;
        }
        let prev = self.day_or_night;
        self.day_or_night = if crate::chrono_now().minute >= NIGHTTIME as u32 { 2 } else { 1 };
        if prev != self.day_or_night {
            for h in self.handles() {
                let c = self.client(h).unwrap();
                if c.is_init_complete && !self.maps[c.map_index].is_fixed_day_mode {
                    let d = self.day_or_night as i32;
                    self.notify(h, NOTIFY_TIMECHANGE, &[d], None);
                }
            }
        }
    }

    /// `WhetherProcessor`
    fn whether_processor(&mut self) {
        let now = self.now();
        for i in 0..self.maps.len() {
            if self.maps[i].is_fixed_day_mode {
                continue;
            }
            let prev = self.maps[i].weather;
            if self.maps[i].weather != 0 {
                if now.wrapping_sub(self.maps[i].weather_start) > self.maps[i].weather_duration {
                    self.maps[i].weather = 0;
                }
            } else if self.dice(1, 100) == 13 {
                self.maps[i].weather = self.dice(1, 3) as u8;
                self.maps[i].weather_start = now;
                self.maps[i].weather_duration = 60000 * 3 + 60000 * self.dice(1, 7) as u32;
            }
            if self.maps[i].is_fixed_snow_mode {
                self.maps[i].weather = self.dice(1, 3) as u8 + 3;
                self.maps[i].weather_start = now;
                self.maps[i].weather_duration = 60000 * 3 + 60000 * self.dice(1, 7) as u32;
            }
            if prev != self.maps[i].weather {
                let w = self.maps[i].weather as i32;
                for h in self.handles() {
                    let c = self.client(h).unwrap();
                    if c.is_init_complete && c.map_index == i {
                        self.notify(h, NOTIFY_WHETHERCHANGE, &[w], None);
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------- message queue

    /// `MsgProcess`
    fn msg_process(&mut self) {
        while let Some(m) = self.queue.pop_front() {
            match m.from {
                From::Client(h) => {
                    if self.client(h).is_none() {
                        log_file("Corrupt message received! (From unknown client):");
                        return;
                    }
                    self.client_mut(h).unwrap().connection_check += 1;
                    self.client_msg(h, m.key, &m.body);
                }
                From::Login => self.login_msg(&m.body),
            }
            if self.quit {
                return;
            }
        }
    }

    fn client_msg(&mut self, h: usize, key: u8, d: &[u8]) {
        let r = Rd::new(d);
        match r.u32(0) {
            MSGID_REQUEST_INITPLAYER => self.request_init_player(h, d, key),
            MSGID_COMMAND_MOTION => self.client_motion(h, d),
            MSGID_COMMAND_CHECKCONNECTION => self.check_connection(h, d),
            MSGID_COMMAND_CHATMSG => self.chat_msg(h, d),
            MSGID_REQUEST_TELEPORT => self.request_teleport(h, 4, None, -1, -1),
            MSGID_REQUEST_FULLOBJECTDATA => {
                self.client_mut(h).unwrap().connection_check -= 1;
                let id = r.u16(4) as usize;
                self.request_full_object_data(h, id);
            }
            MSGID_REQUEST_RESTART => self.request_restart(h),
            MSGID_REQUEST_RESURRECTPLAYER_YES => self.request_resurrect_player(h, true),
            MSGID_REQUEST_RESURRECTPLAYER_NO => self.request_resurrect_player(h, false),
            MSGID_COMMAND_COMMON => self.client_common(h, d),
            MSGID_LEVELUPSETTINGS => self.level_up_settings(h, d),
            MSGID_REQUEST_SETITEMPOS => {
                self.client_mut(h).unwrap().connection_check -= 1;
                self.set_item_pos(h, d);
            }
            MSGID_REQUEST_SELLITEMLIST => self.sell_item_list(h, d),
            MSGID_REQUEST_RETRIEVEITEM => self.retrieve_item(h, d),
            MSGID_REQUEST_CREATENEWGUILD => self.request_create_new_guild(h, d),
            MSGID_REQUEST_DISBANDGUILD => self.request_disband_guild(h, d),
            MSGID_REQUEST_TELEPORT_LIST => self.request_teleport_list(h, d),
            MSGID_REQUEST_CHARGED_TELEPORT => self.request_charged_teleport(h, d),
            MSGID_REQUEST_HELDENIAN_SCROLL => self.request_heldenian_scroll(h, d),
            MSGID_REQUEST_SETRECALLPNT => self.request_set_recall_point(h, d),
            MSGID_REQUEST_UPDATE_FRIENDS => self.request_update_friends(h, d),
            MSGID_REQUEST_CIVILRIGHT => self.request_civil_right(h),
            MSGID_REQUEST_NOTICEMENT => self.request_noticement(h, d),
            MSGID_REQUEST_FIGHTZONE_RESERVE => self.fightzone_reserve(h, d),
            MSGID_REQUEST_PANNING => self.request_panning(h, d),
            MSGID_REQUEST_DKITEM => self.get_dk_item(h, d),
            MSGID_STATECHANGEPOINT => self.state_change(h, d),
            MSGID_REQUEST_LGNPTS => self.request_legion_pts(h),
            MSGID_REQUEST_LGNSVC => self.request_legion_svc(h, d),
            // dead in the C++ (see PORTING.md): the angel shop is commented out, the manager messages do nothing
            MSGID_REQUEST_ANGEL
            | MSGID_MANAGER_INIT
            | MSGID_MANAGER_SHUTDOWN => log(&format!("<{h}> dead in the C++, ignored: msg {:08x} type {:04x}", r.u32(0), r.u16(4))),
            id => {
                log(&format!("Unknown message received! ({id:08x}) Delete Client"));
                self.delete_client(h, true, true, true, false);
            }
        }
    }

    /// `_SetItemPos`
    fn set_item_pos(&mut self, h: usize, d: &[u8]) {
        let r = Rd::new(d);
        let (i, x, y) = (r.u8(6) as usize, r.u16(7) as i16, r.u16(9) as i16);
        if let Some(c) = self.client_mut(h) {
            if i < MAXITEMS && c.items[i].is_some() {
                c.item_pos[i] = (x, y.max(-10));
            }
        }
    }

    /// `CheckConnectionHandler`
    fn check_connection(&mut self, h: usize, d: &[u8]) {
        let rcv = Rd::new(d).u32(6);
        let now = self.now();
        let c = self.client_mut(h).unwrap();
        if c.init_cc_time_rcv == 0 {
            c.init_cc_time_rcv = rcv;
            c.init_cc_time = now;
        } else {
            let gap_client = rcv.wrapping_sub(c.init_cc_time_rcv);
            let gap_server = now.wrapping_sub(c.init_cc_time);
            if gap_client < gap_server {
                return;
            }
            if (gap_client.wrapping_sub(gap_server) as i32).unsigned_abs() >= CLIENTTIMEOUT_MS {
                log_file(&format!("Client({}) speed hack? time gap {gap_client}/{gap_server}", l1(&c.char_name)));
                self.delete_client(h, true, true, true, true);
            }
        }
    }

    // ---------------------------------------------------------------- login link

    fn ls_connect(&mut self, i: usize) {
        self.ls_connecting[i] = true;
        tokio::spawn(crate::net::login_socket(self.ls_addr.clone(), i, self.ev.clone()));
    }

    /// XSOCKEVENT_CONNECTIONESTABLISH
    fn ls_connected(&mut self, i: usize, tx: Tx) {
        self.ls_connecting[i] = false;
        self.ls[i] = Some(tx);
        self.ls_available[i] = true;
        self.cur_ls = i;
        if !self.registered {
            self.send_register();
        } else {
            let mut b = MSGID_REQUEST_REGISTERGAMESERVERSOCKET.to_le_bytes().to_vec();
            b.extend_from_slice(&self.gsid.to_le_bytes());
            self.ls_raw(i, &b);
        }
        self.ls_connected[i] = true;
        self.ls_active_count += 1;
    }

    /// XSOCKEVENT_SOCKETCLOSED / SOCKETERROR on a sub-socket
    fn ls_down(&mut self, i: usize) {
        self.ls_connecting[i] = false;
        if self.ls[i].take().is_some() {
            self.ls_active_count -= 1;
            if self.ls_active_count == 0 {
                self.registered = false;
            }
        }
        self.ls_available[i] = false;
        self.ls_connected[i] = false;
    }

    fn ls_raw(&self, i: usize, body: &[u8]) {
        if let Some(tx) = &self.ls[i] {
            let _ = tx.send(proto::encode(0, body));
        }
    }

    /// `_bCheckSubLogSocketIndex`: the next usable sub-socket, round robin.
    fn next_ls(&mut self) -> Option<usize> {
        for _ in 0..MAXSUBLOGSOCK {
            // fix: the C++ advanced the index past MAXSUBLOGSOCK inside the loop (out-of-bounds read)
            self.cur_ls = (self.cur_ls + 1) % MAXSUBLOGSOCK;
            if self.ls[self.cur_ls].is_some() && self.ls_available[self.cur_ls] {
                return Some(self.cur_ls);
            }
        }
        if !self.on_exit_process {
            self.shutdown_code = 3;
            self.on_exit_process = true;
            self.exit_process_time = self.now();
            log("(!) GAME SERVER SHUTDOWN PROCESS STARTED(by Log-server connection Lost)!!!");
        }
        None
    }

    /// `bSendMsgToLS` for a message built whole (MSGID_PARTYOPERATION)
    pub fn send_ls_raw(&mut self, b: &[u8]) -> bool {
        let Some(i) = self.next_ls() else { return false };
        self.ls_raw(i, b);
        true
    }

    /// `bSendMsgToLS` for the messages that are msgid + MSGTYPE_CONFIRM + payload.
    pub fn send_ls_plain(&mut self, msgid: u32, payload: &[u8]) -> bool {
        let Some(i) = self.next_ls() else { return false };
        let mut b = msgid.to_le_bytes().to_vec();
        b.extend_from_slice(&(MSGTYPE_CONFIRM as u16).to_le_bytes());
        b.extend_from_slice(payload);
        self.ls_raw(i, &b);
        true
    }

    /// MSGID_REQUEST_REGISTERGAMESERVER (on the socket that just connected)
    fn send_register(&mut self) {
        let mut b = vec![0u8; 48 + self.maps.len() * 11];
        let mut w = W(&mut b);
        w.u32(0, MSGID_REQUEST_REGISTERGAMESERVER);
        w.u16(4, MSGTYPE_CONFIRM as u16);
        w.bytes(6, &self.cfg.server_name[..self.cfg.server_name.len().min(10)]);
        w.bytes(16, &self.cfg.world_name[..self.cfg.world_name.len().min(10)]);
        w.bytes(26, &self.cfg.game_server_addr[..self.cfg.game_server_addr.len().min(15)]);
        w.u16(42, self.cfg.game_server_port);
        w.u8(44, self.received_all_config as u8);
        w.u8(45, self.maps.len() as u8);
        w.u16(46, self.gsid);
        for (k, m) in self.maps.iter().enumerate() {
            w.bytes(48 + k * 11, &m.name[..m.name.len().min(10)]);
        }
        // the C++ sends 47 + 11*maps bytes, dropping the last map name's terminating zero
        b.truncate(47 + self.maps.len() * 11);
        self.ls_raw(self.cur_ls, &b);
    }

    fn send_alive(&mut self) {
        if !self.game_started {
            return;
        }
        let n = (self.total_clients as u16).to_le_bytes();
        self.send_ls_plain(MSGID_GAMESERVERALIVE, &n);
    }

    pub fn name10(s: &[u8]) -> [u8; 10] {
        let mut a = [0u8; 10];
        let s = cstr(s);
        let n = s.len().min(10);
        a[..n].copy_from_slice(&s[..n]);
        a
    }

    /// MSGID_REQUEST_PLAYERDATA
    pub fn send_request_player_data(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        let mut p = Vec::new();
        p.extend_from_slice(&Self::name10(&c.char_name));
        p.extend_from_slice(&Self::name10(&c.account_name));
        p.extend_from_slice(&Self::name10(&c.account_password));
        let mut ip = c.ip_address.clone();
        ip.resize(15, 0);
        p.extend_from_slice(&ip);
        p.push(0); // the C++ sends 52 bytes
        self.send_ls_plain(MSGID_REQUEST_PLAYERDATA, &p);
    }

    /// MSGID_ENTERGAMECONFIRM
    pub fn send_enter_game_confirm(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        let mut p = Vec::new();
        p.extend_from_slice(&Self::name10(&c.account_name));
        p.extend_from_slice(&Self::name10(&c.account_password));
        p.extend_from_slice(&Self::name10(&self.cfg.server_name));
        let mut ip = c.ip_address.clone();
        ip.resize(16, 0);
        p.extend_from_slice(&ip);
        p.extend_from_slice(&c.level.to_le_bytes());
        log(&format!("(Client Login) Enter Game: {}({})", l1(&c.char_name), c.level));
        self.send_ls_plain(MSGID_ENTERGAMECONFIRM, &p);
    }

    /// SAVEPLAYERDATA / SAVEPLAYERDATALOGOUT / SAVEPLAYERDATA_REPLY: names, flag, then the player data block.
    pub fn send_save(&mut self, h: usize, msgid: u32, flag: bool) -> bool {
        let Some(c) = self.client(h) else { return false };
        if cstr(&c.char_name).is_empty() {
            return false;
        }
        let mut p = Vec::new();
        p.extend_from_slice(&Self::name10(&c.char_name));
        p.extend_from_slice(&Self::name10(&c.account_name));
        p.extend_from_slice(&Self::name10(&c.account_password));
        p.push(flag as u8);
        p.extend_from_slice(&crate::player_data::compose(self, h));
        self.send_ls_plain(msgid, &p)
    }

    /// MSGID_REQUEST_NOSAVELOGOUT
    pub fn send_no_save_logout(&mut self, h: usize, count_logout: bool) {
        let c = self.client(h).unwrap();
        let mut p = Vec::new();
        p.extend_from_slice(&Self::name10(&c.char_name));
        p.extend_from_slice(&Self::name10(&c.account_name));
        p.extend_from_slice(&Self::name10(&c.account_password));
        p.push(count_logout as u8);
        self.send_ls_plain(MSGID_REQUEST_NOSAVELOGOUT, &p);
    }

    /// MSGID_REQUEST_SETACCOUNTWAITSTATUS: account name + level, 20 bytes.
    fn send_wait_status(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        let mut p = Self::name10(&c.account_name).to_vec();
        p.extend_from_slice(&c.level.to_le_bytes());
        self.send_ls_plain(MSGID_REQUEST_SETACCOUNTWAITSTATUS, &p);
    }

    /// MSGFROM_LOGSERVER cases of MsgProcess
    fn login_msg(&mut self, d: &[u8]) {
        let r = Rd::new(d);
        let (id, ty) = (r.u32(0), r.u16(4));
        match id {
            MSGID_RESPONSE_REGISTERGAMESERVER => {
                if ty == MSGTYPE_CONFIRM as u16 {
                    self.gsid = r.u8(6) as u16;
                    self.registered = true;
                    log(&format!("(*) Game Server registration to Log Server - Success! GSID[{}]", self.gsid));
                } else {
                    log("(ERROR) Game Server registration to Log Server - Fail!");
                }
                if !self.received_all_config {
                    for (ok, what) in [
                        (self.data.build_item_ok, "Build-Item"),
                        (self.data.item_ok, "Item"),
                        (self.data.npc_ok, "Npc"),
                        (self.data.magic_ok, "MAGIC"),
                        (self.data.skill_ok, "SKILL"),
                        (self.data.quest_ok, "QUEST"),
                        (self.data.potion_ok, "POTION"),
                    ] {
                        if !ok {
                            log(&format!("(ERROR) STOPPED! {what} configuration error."));
                        }
                    }
                    self.received_all_config = true;
                }
            }
            MSGID_ITEMCONFIGURATIONCONTENTS
            | MSGID_NPCCONFIGURATIONCONTENTS
            | MSGID_MAGICCONFIGURATIONCONTENTS
            | MSGID_SKILLCONFIGURATIONCONTENTS
            | MSGID_QUESTCONFIGURATIONCONTENTS
            | MSGID_POTIONCONFIGURATIONCONTENTS
            | MSGID_BUILDITEMCONFIGURATIONCONTENTS
            | MSGID_DUPITEMIDFILECONTENTS
            | MSGID_NOTICEMENTFILECONTENTS
            | MSGID_CRAFTINGCONFIGURATIONCONTENTS
            | MSGID_TELEPORTLISTCONTENTS => {
                self.data.on_config_frame(id, &d[6.min(d.len())..]);
                if id == MSGID_NPCCONFIGURATIONCONTENTS {
                    self.load_droplist();
                }
            }
            MSGID_UPDATECONFIGFILES => {
                self.force_player_disconnect(MAXCLIENTS_);
                let keep_teleports = std::mem::take(&mut self.data.teleports);
                self.data = GameData::new();
                self.data.teleports = keep_teleports;
            }
            MSGID_RESPONSE_PLAYERDATA => self.response_player_data(d),
            MSGID_RESPONSE_SAVEPLAYERDATA_REPLY => {
                let name = r.raw(6, 10);
                for h in self.handles() {
                    if Self::name10(&self.client(h).unwrap().char_name) == name[..] {
                        self.notify(h, NOTIFY_SERVERCHANGE, &[], None);
                    }
                }
            }
            MSGID_REQUEST_FORCEDISCONECTACCOUNT => self.force_disconnect_account(&r.s(6, 10), ty),
            MSGID_TOTALGAMESERVERCLIENTS => {
                self.total_game_server_clients = r.u16(6) as i32;
            }
            MSGID_GAMESERVERSHUTDOWNED => {
                self.shutdown_code = 1;
                self.on_exit_process = true;
                self.exit_process_time = self.now();
                log("(!) GAME SERVER SHUTDOWN PROCESS BEGIN(by Global command)!!!");
                self.send_ls_plain(MSGID_GAMESERVERSHUTDOWNED, &[]);
            }
            MSGID_SENDSERVERSHUTDOWNMSG => {
                let v = r.u16(6) as i32;
                if v == 1 || v == 2 {
                    for h in self.handles() {
                        if self.client(h).unwrap().is_init_complete {
                            self.notify(h, NOTIFY_SERVERSHUTDOWN, &[v], None);
                        }
                    }
                }
            }
            MSGID_CONFIRMEDIP => {
                let ip = cstr(&r.raw(4, 30)).to_vec();
                self.confirmed_ips.push(ConfirmedIp { ip, time: self.now() });
            }
            MSGID_SERVERSTOCKMSG => self.server_stock_msg(d),
            MSGID_RESPONSE_CREATENEWGUILD => self.response_create_new_guild(d),
            MSGID_PARTYOPERATION => self.party_operation_result(d),
            MSGID_RESPONSE_DISBANDGUILD => self.response_disband_guild(d),
            MSGID_RESPONSE_LGNPTS => self.response_legion_pts(d),
            MSGID_RESPONSE_LGNSVC => self.handle_legion_service(d),
            // password checks: later layers
            _ => {}
        }
    }

    /// droplist.xml once items and NPCs are known. The C++ never calls InitDrops, so this is loaded for the
    /// later layers but not used to drop anything yet.
    fn load_droplist(&mut self) {
        let Ok(xml) = std::fs::read_to_string("../configs/droplist.xml") else { return };
        let mut dm = DropManager::new(MAXNPCTYPES as usize);
        let data = &self.data;
        let errs = dm.parse(&xml, |n| data.find_npc(n), |n| data.find_item(n));
        for e in errs {
            log(&e);
        }
        self.drops = dm;
    }

    /// `ForceDisconnectAccount`
    fn force_disconnect_account(&mut self, account: &[u8], count: u16) {
        for h in self.handles() {
            let c = self.client(h).unwrap();
            if Self::name10(&c.account_name) == Self::name10(account) {
                log(&format!("<{h}> Force disconnect account: CharName({}) AccntName({}) Count({count})", l1(&c.char_name), l1(account)));
                self.notify(h, NOTIFY_FORCEDISCONN, &[count as i32], None);
                self.delete_client(h, true, true, true, false);
            }
        }
    }

    // ---------------------------------------------------------------- DeleteClient

    /// `DeleteClient(iClientH, bSave, bNotify, bCountLogout, bForceCloseConn)`
    pub fn delete_client(&mut self, h: usize, save: bool, notify: bool, count_logout: bool, force_close: bool) {
        let Some(c) = self.client(h) else { return };
        if c.is_init_complete {
            self.cancel_exchange_if_open(h);
            if let Some(k) = self.client(h).unwrap().has_item(crate::astoria::ITEM_RELIC) {
                let n = self.client(h).unwrap().items[k].as_ref().unwrap().cfg.name.clone();
                self.drop_item(h, k, 1, &n);
            }
            self.guild_logout(h);
            let sc = self.client(h).unwrap().is_on_server_change;
            self.party_logout(h, sc);
            if notify {
                self.send_event_near_type_a(h, MSGID_EVENT_LOG, MSGTYPE_REJECT as u16, 0, 0, 0);
            }
            for i in self.handles() {
                if i != h && self.client(i).unwrap().whisper_player_index == h as i32 {
                    self.client_mut(i).unwrap().whisper_player_index = -1;
                    let name = self.client(h).unwrap().char_name.clone();
                    self.notify(i, NOTIFY_WHISPERMODEOFF, &[], Some(&name));
                }
            }
            // bStockMsgToGateServer(GSM_DISCONNECT + name) + SendStockMsgToGateServer: the Login clears IsPlaying
            // (without it the account stays "playing"); C++ sends m_iIndexGSS + 1 bytes: a zero terminator.
            let mut p = vec![GSM_DISCONNECT as u8];
            p.extend_from_slice(&Self::name10(&self.client(h).unwrap().char_name));
            self.stock_msg(&p);
            self.send_stock_msg();
            let c = self.client(h).unwrap();
            let (m, x, y) = (c.map_index, c.x, c.y);
            self.maps[m].clear_owner(h as i16, OWNERTYPE_PLAYER, x, y);
            self.remove_delay_events(h, OWNERTYPE_PLAYER, 0);
            // fix: the C++ left the engaged fish counted for a player who logged out while fishing
            let f = self.client(h).unwrap().allocated_fish;
            if let Some(fish) = self.fish.get_mut(f).and_then(|x| x.as_mut()) {
                fish.engaging -= 1;
            }
        }
        let c = self.client(h).unwrap();
        if save && !c.is_on_server_change {
            let crusade = self.cr.mode;
            let c = self.client_mut(h).unwrap();
            if c.is_killed {
                Self::killed_logout_map(c, crusade);
            } else if force_close {
                c.map_name = b"bisle".to_vec();
                (c.x, c.y) = (-1, -1);
                c.locked_map_name = b"bisle".to_vec();
                c.locked_map_time = 10 * 60;
            }
            if c.is_observer_mode || c.map_name.starts_with(b"astoria") {
                c.map_name = SIDE_MAP[c.side.clamp(0, 3) as usize].to_vec();
                (c.x, c.y) = (-1, -1);
            }
            self.fz_record_disconnect(h);
            if self.client(h).unwrap().is_init_complete {
                self.send_save(h, MSGID_REQUEST_SAVEPLAYERDATALOGOUT, count_logout);
            } else {
                self.send_no_save_logout(h, count_logout);
            }
        } else if !c.is_on_waiting_process {
            self.send_no_save_logout(h, count_logout);
        } else {
            self.send_wait_status(h);
        }
        let c = self.clients[h].take().unwrap();
        self.conn_slot.remove(&c.conn);
        if !c.is_manager {
            self.total_clients -= 1;
        }
        // dropping `c.tx` closes the socket
    }

    /// The map a killed player is saved on (DeleteClient, bIsKilled branch): in a crusade a second death within
    /// 10 minutes locks the player in his town for 5.
    fn killed_logout_map(c: &mut Client, crusade: bool) {
        let tmp = std::mem::take(&mut c.map_name);
        (c.x, c.y) = (-1, -1);
        if c.is_neutral() {
            c.map_name = SIDE_MAP[0].to_vec();
            return;
        }
        if crusade {
            if c.dead_penalty_time > 0 {
                c.locked_map_name = SIDE_MAP[c.side.clamp(0, 3) as usize].to_vec();
                c.locked_map_time = 60 * 5;
            }
            c.dead_penalty_time = 60 * 10;
        }
        let s = c.side.clamp(1, 3) as usize;
        let foe = (1..=3).find(|&o| o != s && tmp == SIDE_MAP[o]);
        if let Some(o) = foe {
            c.locked_map_name = SIDE_MAP_JAIL[o].to_vec();
            c.locked_map_time = 60 * 3;
            c.map_name = SIDE_MAP_JAIL[o].to_vec();
        } else if c.level > 80 {
            c.map_name = SIDE_MAP_RES[s].to_vec();
        } else {
            c.map_name = SIDE_MAP_FARM[s].to_vec();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn exp_table() {
        assert_eq!(super::level_exp(1), 51);
        assert_eq!(super::level_exp(17), 17 * 17 + 17 * (50 + 17));
        assert_eq!(super::level_exp(100), 100 * 100 + 100 * (50 + 100 * 5 * 5));
    }
}
