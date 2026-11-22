//! Crusade (HG.cpp ~33165-35950 and its scattered branches): bReadCrusadeStructureConfigFile, the strike points of
//! the map info files (+ _LinkStrikePointMapIndex, CMap::RestoreStrikePoints, bAdd/bRemoveCrusadeStructureInfo),
//! Global/Local/ManualStart/EndCrusadeMode, _CreateCrusadeGUID, CreateCrusadeStructures, KillCrusadeObjects,
//! CheckCrusadeResultCalculation, SelectCrusadeDutyHandler, RequestSetGuildTeleportLoc/ConstructLocHandler,
//! RequestGuildTeleportHandler, RequestSummonWarUnitHandler, RequestHelpHandler, MapStatusHandler + _SendMapStatus,
//! CheckCommanderConstructionPoint, SendCollectedMana, SyncMiddlelandMapInfo, the GSM_* crusade receivers,
//! MeteorStrikeMsgHandler / MeteorStrikeHandler / DoMeteorStrikeDamageHandler / CalcMeteorStrikeEffectHandler,
//! GrandMagicResultHandler, _GrandMagicLaunchMsgSend, __bSetConstructionKit, _bCrusadeLog, the structure behaviors
//! of CNpc (mana collector, detector, grand magic generator, mana stone) and the crusade-mode rewards of
//! NpcKilledHandler / EnemyKillRewardHandler / the hammer of calculateAttackEffect.
use crate::client::*;
use crate::consts::*;
use crate::delay::DelayEvent;
use crate::game::Game;
use crate::log::log;
use crate::map::{OWNERTYPE_NPC, OWNERTYPE_PLAYER};
use crate::npc::{MOVETYPE_FOLLOW, MOVETYPE_GUARD, MOVETYPE_RANDOM};
use crate::util::{atoi, cstr, StrTok};

pub const NPC_AGT: i16 = 36;
pub const NPC_CGT: i16 = 37;
pub const NPC_MS: i16 = 38;
pub const NPC_DT: i16 = 39;
pub const NPC_ESG: i16 = 40;
pub const NPC_GMG: i16 = 41;
pub const NPC_MANASTONE: i16 = 42;
pub const NPC_LWB: i16 = 43;
pub const NPC_GHK: i16 = 44;
pub const NPC_GHKABS: i16 = 45;
pub const NPC_TK: i16 = 46;
pub const NPC_BG: i16 = 47;
pub const NPC_CP: i16 = 51;
const MAXSTRUCT: usize = MAXCRUSADESTRUCTURES as usize;

/// the structures and war units KillCrusadeObjects kills at the end
pub fn is_crusade_object(t: i16) -> bool {
    matches!(t, NPC_LWB | NPC_GHK | NPC_GHKABS | NPC_TK | NPC_BG | NPC_CP | NPC_AGT | NPC_CGT | NPC_MS | NPC_DT | NPC_ESG | NPC_GMG | NPC_MANASTONE)
}

/// `iGetMapLocationForbiddenSide`: the side a town building belongs to the enemy of (-99: none)
pub fn map_forbidden_side(name: &[u8]) -> i32 {
    match name {
        b"wrhus_1" | b"bsmith_1" | b"gshop_1" | b"arefarm" | b"arewrhus" | b"cityhall_1" | b"cath_1" | b"gldhall_1" | b"cmdhall_1" | b"wzdtwr_1" => 2,
        // "Cmdhall_2" (capital C) as in the C++: the map is CmdHall_2, so neither commander hall ever matched
        b"wrhus_2" | b"bsmith_2" | b"gshop_2" | b"elvfarm" | b"elvwrhus" | b"cityhall_2" | b"cath_2" | b"gldhall_2" | b"Cmdhall_2" | b"wzdtwr_2" => 1,
        _ => -99,
    }
}

/// `m_iNpcConstructionPoint` (CGame::CGame): the construction points a war unit costs
pub fn npc_construction_point(t: i32) -> i32 {
    match t {
        1..=6 => 100,
        43 => 1000,
        44 => 2000,
        45 => 3000,
        46 => 2000,
        47 => 3000,
        51 => 1500,
        _ => 0,
    }
}

/// a map's `strike-point` (the meteor targets of a town): its HP and the building map it stands for
#[derive(Clone, Default, Debug)]
pub struct StrikePoint {
    pub x: i16,
    pub y: i16,
    pub hp: i32,
    pub init_hp: i32,
    pub effect: [(i16, i16); 5],
    pub related: Vec<u8>,
    pub map: Option<usize>,
}

/// m_stCrusadeStructureInfo (map, client and middleland copies): type 0 = empty
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct StructInfo {
    pub ty: i32,
    pub side: i32,
    pub x: i16,
    pub y: i16,
}

/// m_pGuildTeleportLoc: a guild's teleport (commander) and construction locations and its unit counts
#[derive(Clone, Default, Debug)]
pub struct GuildLoc {
    pub guid: i32,
    pub tp: (i32, i32),
    pub tp_map: Vec<u8>,
    pub tp_time: u32,
    pub cl: (i32, i32),
    pub cl_map: Vec<u8>,
    pub cl_time: u32,
    /// m_iV2: structures built
    pub structures: i32,
    pub summons: i32,
}

/// the per-player state of the crusade map (m_cConstructMapName ... m_bIsSendingMapStatus)
#[derive(Clone, Default, Debug)]
pub struct ClientCrusade {
    pub construct_map: Vec<u8>,
    pub construct_loc: (i32, i32),
    pub csi: Vec<StructInfo>,
    pub csi_send_point: usize,
    pub sending_map: Vec<u8>,
    pub is_sending_map_status: bool,
}

#[derive(Default)]
pub struct Crusade {
    pub mode: bool,
    pub winner_side: i32,
    pub collected_mana: [i32; 4],
    pub mana: [i32; 4],
    pub max_gmg_mana: i32,
    /// Crusade.cfg `crusade-structure` by number: (map, type, x, y)
    pub structures: Vec<Option<(Vec<u8>, i32, i16, i16)>>,
    pub guild_locs: Vec<GuildLoc>,
    pub middle: Vec<StructInfo>,
    /// per map: strike points by number (0..MAXSTRIKEPOINTS) and the count read
    pub strike: Vec<(Vec<Option<StrikePoint>>, usize)>,
    /// per map: the live structures (compacted, as bRemoveCrusadeStructureInfo does)
    pub map_structs: Vec<Vec<StructInfo>>,
    /// CMap::m_bIsDisabled (set by a crashed strike point; never read, as in the C++)
    pub disabled: Vec<bool>,
    /// m_stMeteorStrikeResult: casualties, crashed structures, structure damage
    pub meteor: (i32, i32, i32),
}

/// `bReadCrusadeStructureConfigFile`: (structures by number, Mana-Stone); None on a format error (what was read
/// before it is kept, as in the C++).
pub fn parse_crusade_cfg(txt: &[u8]) -> (Vec<Option<(Vec<u8>, i32, i16, i16)>>, i32, bool) {
    let mut st = vec![None; MAXSTRUCT];
    let mut mana = 0;
    let mut t = StrTok::new(txt, b"= \t\n");
    let num = |s: &[u8]| !s.is_empty() && s.iter().all(|c| c.is_ascii_digit());
    while let Some(k) = t.get() {
        if k.starts_with(b"crusade-structure") {
            let f: Vec<Vec<u8>> = (0..5).filter_map(|_| t.get()).collect();
            if f.len() < 5 || !num(&f[0]) || !num(&f[2]) || !num(&f[3]) || !num(&f[4]) {
                return (st, mana, false);
            }
            let i = atoi(&f[0]) as usize;
            // fix: the C++ indexed its 300-entry array with any number
            if i >= MAXSTRUCT || st[i].is_some() {
                return (st, mana, false);
            }
            st[i] = Some((f[1].clone(), atoi(&f[2]), atoi(&f[3]) as i16, atoi(&f[4]) as i16));
        } else if k.starts_with(b"Mana-Stone") {
            mana = atoi(&t.get().unwrap_or_default());
        }
    }
    (st, mana, true)
}

impl Game {
    pub fn aresden_map(&self) -> Option<usize> {
        self.map_index(SIDE_MAP[ARESDEN as usize])
    }
    pub fn elvine_map(&self) -> Option<usize> {
        self.map_index(SIDE_MAP[ELVINE as usize])
    }
    pub fn middleland_map(&self) -> Option<usize> {
        self.map_index(b"middleland")
    }

    /// OnStartGameSignal: Crusade.cfg, the strike points of the map info files and _LinkStrikePointMapIndex.
    pub fn crusade_init(&mut self) {
        let raw = std::fs::read("../configs/Crusade.cfg").unwrap_or_default();
        if raw.is_empty() {
            log("(ERROR) Cannot open Crusade Schedule file.");
        }
        let (st, mana, ok) = parse_crusade_cfg(&crate::util::text_mode(&raw));
        if !ok {
            log("(!!!) CRITICAL ERROR!  Crusade configuration file error - Wrong Data format.");
        }
        self.cr.structures = st;
        self.cr.max_gmg_mana = mana;
        self.cr.guild_locs = vec![GuildLoc::default(); MAXGUILDS as usize];
        let n = self.maps.len();
        self.cr.map_structs = vec![Vec::new(); n];
        self.cr.disabled = vec![false; n];
        self.cr.strike = vec![(vec![None; MAXSTRIKEPOINTS as usize], 0); n];
        for m in 0..n {
            for (k, f) in self.maps[m].info.clone() {
                if k != &b"strike-point"[..] || f.len() < 15 {
                    continue;
                }
                let i = atoi(&f[0]);
                // fix: the C++ wrote any strike point number into its 20-entry array
                if !(0..MAXSTRIKEPOINTS).contains(&i) || self.cr.strike[m].0[i as usize].is_some() {
                    log(&format!("(!!!) CRITICAL ERROR! Map Info file error - strike point {i}"));
                    continue;
                }
                let v = |j: usize| atoi(&f[j]);
                let mut p = StrikePoint { x: v(1) as i16, y: v(2) as i16, hp: v(3), init_hp: v(3), related: f[14].clone(), ..Default::default() };
                for e in 0..5 {
                    p.effect[e] = (v(4 + e * 2) as i16, v(5 + e * 2) as i16);
                }
                p.map = self.map_index(&p.related);
                if let Some(x) = p.map {
                    log(&format!("(!) Map({}) Strike Point({i}) Related Map({}) Index({x})", crate::util::l1(&self.maps[m].name), crate::util::l1(&p.related)));
                }
                self.cr.strike[m].0[i as usize] = Some(p);
                self.cr.strike[m].1 += 1;
            }
        }
    }

    /// `_bCrusadeLog`
    pub fn crusade_log(&mut self, action: i32, h: usize, data: i32, name: &str) {
        let pc = |g: &Game| g.client(h).map(|c| (String::from_utf8_lossy(cstr(&c.char_name)).into_owned(), String::from_utf8_lossy(cstr(&c.guild_name)).into_owned()));
        let txt = match action {
            CRUSADELOG_ENDCRUSADE => format!("\tEnd Crusade\t{name}"),
            CRUSADELOG_SELECTDUTY => {
                let Some((c, g)) = pc(self) else { return };
                format!("PC({c})\tSelect Duty\t({name})\t \t({g})")
            }
            CRUSADELOG_GETEXP => {
                let Some((c, g)) = pc(self) else { return };
                format!("PC({c})\tGet Exp\t({data})\t \t({g})")
            }
            CRUSADELOG_STARTCRUSADE => "\tStart Crusade".to_string(),
            // the C++ logs "Start Apocalypse" for every apocalypse line (its end and bosses too)
            CRUSADELOG_APOCALYPSE => "\tStart Apocalypse".to_string(),
            _ => return,
        };
        self.send_ls_plain(MSGID_GAMECRUSADELOG, txt.as_bytes());
    }

    /// `_CreateCrusadeGUID`
    fn write_crusade_guid(&self, guid: u32, winner: i32) {
        let _ = std::fs::create_dir_all("GameData");
        // fix: the C++ had the two lines commented out and wrote an empty file, so after a restart the GUID was 0:
        // no reward for the players of the last crusade and no winner-side shop prices / commander points
        let txt = format!("CrusadeGUID = {guid}\nwinner-side = {winner}\n");
        if std::fs::write("GameData/CrusadeGUID.txt", txt).is_err() {
            log(&format!("(ERROR) Cannot create CrusadeGUID({guid}) file"));
        }
    }

    /// `GlobalStartCrusadeMode`
    pub fn global_start_crusade(&mut self) {
        if self.cr.mode {
            return;
        }
        let guid = self.now();
        let mut d = vec![GSM_BEGINCRUSADE as u8];
        d.extend_from_slice(&guid.to_le_bytes());
        self.stock_msg(&d);
        self.local_start_crusade(guid);
    }

    /// `ManualEndCrusadeMode`
    pub fn manual_end_crusade(&mut self, winner: i32) {
        if !self.cr.mode {
            return;
        }
        self.local_end_crusade(winner);
        let mut d = vec![0u8; 18];
        (d[0], d[1]) = (GSM_ENDCRUSADE as u8, winner as u8);
        self.stock_msg(&d);
    }

    /// `LocalStartCrusadeMode` (guid 0: the F9 "resume" of the C++ window keeps the current GUID)
    pub fn local_start_crusade(&mut self, guid: u32) {
        if self.cr.mode {
            return;
        }
        self.cr.mode = true;
        self.cr.winner_side = 0;
        self.cr.collected_mana = [0; 4];
        self.cr.mana = [0; 4];
        if guid != 0 {
            self.write_crusade_guid(guid, 0);
            self.crusade_guid = guid;
        }
        for h in self.handles() {
            let g = self.crusade_guid;
            let c = self.client_mut(h).unwrap();
            if !c.is_init_complete {
                continue;
            }
            (c.crusade_duty, c.construction_point, c.crusade_guid) = (0, 0, g);
            self.notify(h, NOTIFY_CONSTRUCTIONPOINT, &[0, 0, 1], None); // 1: not shown
            self.notify(h, NOTIFY_CRUSADE, &[1, 0, 0, 0], None);
            // recall from an enemy town
            let c = self.client(h).unwrap();
            if c.admin_user_level == 0 {
                let loc = self.maps[c.map_index].location_name.clone();
                let foe = self.is_in_foe_map(h);
                let c = self.client_mut(h).unwrap();
                if foe || (c.is_neutral() && (loc.starts_with(b"aresden") || loc.starts_with(b"elvine") || loc.starts_with(b"middleland"))) {
                    (c.is_war_location, c.time_left_force_recall) = (true, 0);
                }
            }
        }
        for (pts, _) in self.cr.strike.iter_mut() {
            for p in pts.iter_mut().flatten() {
                p.hp = p.init_hp;
            }
        }
        self.create_crusade_structures();
        log("(!) Crusade Mode ON.");
        self.crusade_log(CRUSADELOG_STARTCRUSADE, 0, 0, "");
    }

    /// `LocalEndCrusadeMode`
    pub fn local_end_crusade(&mut self, winner: i32) {
        if !self.cr.mode {
            return;
        }
        self.cr.mode = false;
        self.cr.winner_side = winner;
        log("(!) Crusade Mode OFF.");
        self.kill_crusade_objects();
        self.write_crusade_guid(self.crusade_guid, winner);
        for h in self.handles() {
            let c = self.client_mut(h).unwrap();
            if !c.is_init_complete {
                continue;
            }
            (c.crusade_duty, c.construction_point) = (0, 0);
            self.notify(h, NOTIFY_CRUSADE, &[0, 0, 0, winner], None);
        }
        let s = match winner {
            2 => "Elvine Win!",
            1 => "Aresden Win!",
            _ => "Drawn!",
        };
        self.crusade_log(CRUSADELOG_ENDCRUSADE, 0, 0, s);
    }

    /// `CreateCrusadeStructures`: every Crusade.cfg structure of a map on this server, three random mana stones.
    fn create_crusade_structures(&mut self) {
        let ms: Vec<usize> = (0..MAXSTRUCT).filter(|&i| self.cr.structures[i].as_ref().is_some_and(|s| s.1 == NPC_MANASTONE as i32)).take(100).collect();
        let mut selected = vec![false; MAXSTRUCT];
        if ms.len() <= 3 {
            // fix: the C++ marked iMSIndex[0..3] even when there were fewer (an index of -1)
            for &i in &ms {
                selected[i] = true;
            }
        } else {
            let mut idx: Vec<Option<usize>> = ms.iter().map(|&i| Some(i)).collect();
            let mut left = 3;
            while left > 0 {
                let k = (self.dice(1, ms.len() as i32) - 1) as usize;
                if let Some(i) = idx[k].take() {
                    selected[i] = true;
                    left -= 1;
                }
            }
        }
        for i in 0..MAXSTRUCT {
            let Some((map, ty, x, y)) = self.cr.structures[i].clone() else { continue };
            if ty == 0 {
                continue;
            }
            let Some(z) = self.map_index(&map) else { continue };
            let Some(nv) = self.maps[z].get_empty_naming_value() else { continue };
            let name = format!("_{}{nv}", (z as u8 + 65) as char).into_bytes();
            let mname = self.maps[z].name.clone();
            let side = if cstr(&mname) == SIDE_MAP[ARESDEN as usize] { "Aresden" } else if cstr(&mname) == SIDE_MAP[ELVINE as usize] { "Elvine" } else { "" };
            let npc_name: Vec<u8> = match ty as i16 {
                NPC_AGT | NPC_CGT | NPC_ESG | NPC_GMG if !side.is_empty() => {
                    let p = match ty as i16 {
                        NPC_AGT => "AGT",
                        NPC_CGT => "CGT",
                        NPC_ESG => "ESG",
                        _ => "GMG",
                    };
                    format!("{p}-{side}").into_bytes()
                }
                NPC_AGT | NPC_CGT | NPC_ESG | NPC_GMG => Vec::new(),
                NPC_MANASTONE => {
                    if selected[i] {
                        b"ManaStone".to_vec()
                    } else {
                        Vec::new()
                    }
                }
                // the C++ took the npc config *entry* number `type`
                _ => self.data.npcs.get(ty as usize).and_then(|c| c.as_ref()).map(|c| c.name.clone()).unwrap_or_default(),
            };
            match self.create_npc(&npc_name, &name, z, 0, MOVETYPE_RANDOM, Some((x, y)), &[-1; 10], None, -1) {
                Some(h) => {
                    let n = self.npcs[h].as_ref().unwrap();
                    log(&format!("(!) Creating Crusade Structure({}) at {}({}, {})", crate::util::l1(&npc_name), crate::util::l1(&map), n.x, n.y));
                }
                None => self.maps[z].set_naming_value_empty(nv),
            }
        }
    }

    /// `KillCrusadeObjects`
    fn kill_crusade_objects(&mut self) {
        for i in 1..self.npcs.len() {
            if self.npcs[i].as_ref().is_some_and(|n| is_crusade_object(n.type_)) {
                self.npc_killed(0, 0, i, 0);
            }
        }
    }

    /// `CheckCrusadeResultCalculation` (the exp-stock timer)
    pub fn check_crusade_result(&mut self, h: usize) {
        let (mode, guid, winner) = (self.cr.mode, self.crusade_guid, self.cr.winner_side);
        let Some(c) = self.client_mut(h) else { return };
        if mode || c.crusade_guid == 0 {
            return;
        }
        if c.crusade_guid == guid {
            if winner == 0 {
                let e = c.war_contribution * 5;
                c.cs.exp_stock += e;
                let wc = c.war_contribution;
                self.notify(h, NOTIFY_CRUSADE, &[0, 0, wc, 0], None);
                self.crusade_log(CRUSADELOG_GETEXP, h, e, "");
            } else {
                c.war_contribution += c.level
                    * match c.level {
                        ..=80 => 200,
                        ..=160 => 125,
                        _ => 75,
                    };
                let won = winner == c.side;
                let e = c.war_contribution * if won { 35 } else { 5 };
                c.cs.exp_stock += e;
                let wc = if won { c.war_contribution } else { -c.war_contribution };
                self.notify(h, NOTIFY_CRUSADE, &[0, 0, wc, 0], None);
                self.crusade_log(CRUSADELOG_GETEXP, h, e, "");
            }
        } else {
            self.notify(h, NOTIFY_CRUSADE, &[0, 0, 0, -1], None);
        }
        let c = self.client_mut(h).unwrap();
        (c.crusade_duty, c.war_contribution, c.crusade_guid) = (0, 0, 0);
    }

    /// `SelectCrusadeDutyHandler` (COMMONTYPE_REQUEST_SELECTCRUSADEDUTY)
    pub fn select_crusade_duty(&mut self, h: usize, duty: i32) {
        let (mode, winner) = (self.cr.mode, self.cr.winner_side);
        let Some(c) = self.client_mut(h) else { return };
        if !mode || (c.guild_rank != 0 && duty == 3) {
            return;
        }
        if winner != c.side && c.guild_rank == 0 && duty == 3 {
            c.construction_point = 3000;
        }
        c.crusade_duty = duty;
        self.notify(h, NOTIFY_CRUSADE, &[1, duty, 0, 0], None);
        let s = match duty {
            1 => "Fighter",
            2 => "Constructor",
            _ => "Commander",
        };
        self.crusade_log(CRUSADELOG_SELECTDUTY, h, 0, s);
    }

    fn guild_slot(&self, guid: i32) -> Option<usize> {
        self.cr.guild_locs.iter().position(|g| g.guid == guid)
    }

    /// the slot of a guild for a new location: its own, a free one, else the one unused longest
    fn guild_slot_for(&mut self, guid: i32, construct: bool) -> Option<usize> {
        if let Some(i) = self.guild_slot(guid) {
            return Some(i);
        }
        if let Some(i) = self.cr.guild_locs.iter().position(|g| g.guid == 0) {
            self.cr.guild_locs[i].guid = guid;
            return Some(i);
        }
        let now = self.now();
        // fix: the C++ wrote the replacement into m_pGuildTeleportLoc[MAXGUILDS], past the array
        let i = (0..self.cr.guild_locs.len()).max_by_key(|&i| now.wrapping_sub(if construct { self.cr.guild_locs[i].cl_time } else { self.cr.guild_locs[i].tp_time }))?;
        log(if construct { "(X) No more GuildConstructLoc Space! Replaced." } else { "(X) No more GuildTeleportLoc Space! Replaced." });
        self.cr.guild_locs[i] = GuildLoc { guid, ..Default::default() };
        Some(i)
    }

    /// `RequestSetGuildTeleportLocHandler` / `RequestSetGuildConstructLocHandler` (COMMONTYPE_SETGUILDTELEPORTLOC /
    /// SETGUILDCONSTRUCTLOC) and their GSM_SETGUILD* receivers (`local` false: no relay).
    pub fn set_guild_loc(&mut self, h: usize, construct: bool, x: i32, mut y: i32, guid: i32, map: &[u8], local: bool) {
        if !self.cr.mode {
            return;
        }
        if local {
            let Some(c) = self.client(h) else { return };
            // fix: the C++ took the location from any player (the client offers it to commanders only), so a
            // forged message from any guild member, or from a guildless player, moved the guild's locations
            if c.guild_rank != 0 || c.crusade_duty != 3 || guid == 0 {
                return;
            }
            if !construct {
                y = y.clamp(100, 600);
            }
        }
        let map = cstr(map)[..cstr(map).len().min(10)].to_vec();
        let now = self.now();
        let Some(i) = self.guild_slot_for(guid, construct) else { return };
        let g = &mut self.cr.guild_locs[i];
        let (loc, m, t) = if construct { (&mut g.cl, &mut g.cl_map, &mut g.cl_time) } else { (&mut g.tp, &mut g.tp_map, &mut g.tp_time) };
        *t = now;
        if *loc == (x, y) && *m == map {
            return;
        }
        (*loc, *m) = ((x, y), map.clone());
        if local {
            log(&format!("SetGuild{}Loc: {guid} {} {x} {y}", if construct { "Construct" } else { "Teleport" }, crate::util::l1(&map)));
            let mut d = vec![0u8; 23];
            d[0] = if construct { GSM_SETGUILDCONSTRUCTLOC } else { GSM_SETGUILDTELEPORTLOC } as u8;
            d[1..5].copy_from_slice(&guid.to_le_bytes());
            d[5..9].copy_from_slice(&x.to_le_bytes());
            d[9..13].copy_from_slice(&y.to_le_bytes());
            d[13..13 + map.len()].copy_from_slice(&map);
            self.stock_msg(&d);
        }
    }

    /// `RequestGuildTeleportHandler` (COMMONTYPE_GUILDTELEPORT)
    pub fn guild_teleport(&mut self, h: usize) {
        if !self.cr.mode {
            return;
        }
        let Some(c) = self.client(h) else { return };
        if c.locked_map_time != 0 {
            let (t, n) = (c.locked_map_time, c.locked_map_name.clone());
            self.notify(h, NOTIFY_LOCKEDMAP, &[t], Some(&n));
            return;
        }
        let guid = c.guild_guid;
        if self.is_in_foe_map(h) {
            return;
        }
        // fix: a guildless player (GUID 0) matched a free slot and was sent to an empty map name
        let Some(i) = self.guild_slot(guid).filter(|&i| guid != 0 && !self.cr.guild_locs[i].tp_map.is_empty()) else { return };
        let g = self.cr.guild_locs[i].clone();
        self.request_teleport(h, 2, Some(&g.tp_map), g.tp.0 as i16, g.tp.1 as i16);
    }

    /// `RequestSummonWarUnitHandler` (COMMONTYPE_SUMMONWARUNIT): one unit or structure at (x, y) for its points.
    pub fn summon_war_unit(&mut self, h: usize, x: i16, y: i16, ty: i32, num: i32, mode: i32) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !self.cr.mode || !(0..MAXNPCTYPES).contains(&ty) || num > 10 {
            return;
        }
        let m = c.map_index;
        if c.construction_point < npc_construction_point(ty) || self.maps[m].is_fixed_day_mode {
            return;
        }
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return };
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        let c = self.client(h).unwrap();
        let (side, guid, cname) = (c.side, c.guild_guid, c.char_name.clone());
        let s = match side {
            1 => "Aresden",
            2 => "Elvine",
            _ => "",
        };
        let npc_name = match ty as i16 {
            NPC_LWB | NPC_AGT | NPC_CGT | NPC_MS | NPC_DT | NPC_CP if !s.is_empty() => {
                let p = match ty as i16 {
                    NPC_LWB => "LWB",
                    NPC_AGT => "AGT",
                    NPC_CGT => "CGT",
                    NPC_MS => "MS",
                    NPC_DT => "DT",
                    _ => "CP",
                };
                format!("{p}-{s}")
            }
            NPC_GHK => "GHK".into(),
            NPC_GHKABS => "GHKABS".into(),
            NPC_TK => "TK".into(),
            NPC_BG => "BG".into(),
            _ => String::new(),
        };
        log(&format!("(!) Request Summon War Unit ({ty}) ({npc_name})"));
        let fail = |g: &mut Game, code: i32| {
            g.maps[m].set_naming_value_empty(nv);
            g.notify(h, NOTIFY_CANNOTCONSTRUCT, &[code], None);
        };
        let now = self.now();
        // the guild counter raised (structures / summons), undone if the creation fails
        let mut counted: Option<(usize, bool)> = None;
        match ty as i16 {
            NPC_AGT | NPC_CGT | NPC_MS | NPC_DT => {
                let c = self.client(h).unwrap();
                let (cm, (cx, cy)) = (c.cr.construct_map.clone(), c.cr.construct_loc);
                if cstr(&cm) != cstr(&c.map_name) || (c.x as i32 - cx).abs() > 10 || (c.y as i32 - cy).abs() > 10 {
                    return fail(self, 2);
                }
                let Some(i) = self.guild_slot(guid) else { return fail(self, 3) };
                let g = &mut self.cr.guild_locs[i];
                g.tp_time = now;
                if g.structures >= MAXCONSTRUCTNUM {
                    return fail(self, 3);
                }
                g.structures += 1;
                counted = Some((i, true));
            }
            NPC_LWB | NPC_GHK | NPC_GHKABS | NPC_TK | NPC_BG | NPC_CP => {
                // fix: the C++ counted every guild's war units in slot 0 (its loop returned at i = 0), so after
                // 100 summons on the server nobody could summon, while the deaths were taken off each guild's slot
                let Some(i) = self.guild_slot(guid).filter(|_| guid != 0) else { return fail(self, 3) };
                if self.cr.guild_locs[i].summons >= MAX_CRUSADESUMMONMOB {
                    return fail(self, 4);
                }
                self.cr.guild_locs[i].summons += 1;
                counted = Some((i, false));
            }
            _ => {}
        }
        let mut bad = false;
        if matches!(ty as i16, NPC_AGT | NPC_CGT) {
            for ix in x - 2..=x + 2 {
                for iy in y - 2..=y + 2 {
                    let (o, ot) = self.maps[m].get_owner(ix, iy);
                    if o != 0 && ot == OWNERTYPE_NPC && self.npcs.get(o as usize).and_then(|n| n.as_ref()).is_some_and(|n| matches!(n.type_, NPC_AGT | NPC_CGT)) {
                        bad = true;
                    }
                }
            }
            if y <= 32 || y >= 783 {
                bad = true;
            }
        }
        let undo = |g: &mut Game| {
            if let Some((i, st)) = counted {
                let l = &mut g.cr.guild_locs[i];
                if st {
                    l.structures -= 1;
                } else {
                    l.summons -= 1;
                }
            }
        };
        if bad {
            undo(self);
            return fail(self, 1);
        }
        let mt = if mode == 0 { MOVETYPE_FOLLOW } else { MOVETYPE_GUARD };
        let Some(i) = self.create_npc(npc_name.as_bytes(), &name, m, 0, mt, Some((x, y)), &[-1; 10], None, -1) else {
            // fix: the C++ kept the guild count of a unit that was never created (MS-Elvine, TK, BG... are not
            // in Npc.cfg), so each try used up one of the guild's 40 structures / 100 units
            undo(self);
            self.maps[m].set_naming_value_empty(nv);
            return;
        };
        self.npcs[i].as_mut().unwrap().guild_guid = guid;
        if guid != 0 {
            log(&format!("Summon War Unit({ty}) GUID({guid})"));
        }
        if mode == 0 {
            // bSetNpcFollowMode(name, player name, OWNERTYPE_PLAYER): the player is on the same map
            if let Some(p) = self.handles().into_iter().find(|&p| Game::pad10(&self.client(p).unwrap().char_name) == Game::pad10(&cname)) {
                let ps = self.client(p).unwrap().side;
                if self.client(p).unwrap().map_index == m {
                    let n = self.npcs[i].as_mut().unwrap();
                    (n.move_type, n.follow_owner, n.follow_owner_type, n.side) = (MOVETYPE_FOLLOW, p, OWNERTYPE_PLAYER, ps);
                }
            }
        }
        let c = self.client_mut(h).unwrap();
        c.construction_point = (c.construction_point - npc_construction_point(ty)).max(0);
        let (cp, wc) = (c.construction_point, c.war_contribution);
        self.notify(h, NOTIFY_CONSTRUCTIONPOINT, &[cp, wc, 0], None);
    }

    /// `RequestHelpHandler` (COMMONTYPE_REQUEST_HELP): a fighter shows his position to his guild's commander.
    pub fn request_help(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if !self.cr.mode || c.guild_rank == -1 || c.crusade_duty != 1 {
            return;
        }
        let (x, y, hp, guid, name) = (c.x, c.y, c.hp, c.guild_guid, c.char_name.clone());
        for i in self.handles() {
            let o = self.client(i).unwrap();
            if o.guild_rank == 0 && o.crusade_duty == 3 && o.guild_guid == guid {
                self.notify(i, NOTIFY_HELP, &[x as i32, y as i32, hp], Some(&name));
                return;
            }
        }
        self.notify(h, NOTIFY_HELPFAILED, &[], None);
    }

    /// `MapStatusHandler` (COMMONTYPE_REQUEST_MAPSTATUS): 1 = the guild's teleport / construction locations,
    /// 3 = the structures of a map (all for a commander, the mana stones for the others).
    pub fn map_status(&mut self, h: usize, mode: i32, map: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if c.crusade_duty == 0 {
            return;
        }
        let (duty, guid, m) = (c.crusade_duty, c.guild_guid, c.map_index);
        match mode {
            1 => {
                let Some(i) = self.guild_slot(guid).filter(|_| guid != 0) else { return };
                let g = self.cr.guild_locs[i].clone();
                self.notify2(h, NOTIFY_TCLOC, &[g.tp.0, g.tp.1, 0, g.cl.0, g.cl.1], Some(&g.tp_map), Some(&g.cl_map));
                let c = self.client_mut(h).unwrap();
                (c.cr.construct_map, c.cr.construct_loc) = (g.cl_map.clone(), g.cl);
            }
            3 => {
                let src: Option<(Vec<StructInfo>, Vec<u8>)> = if cstr(map) == cstr(&self.maps[m].name) {
                    Some((self.cr.map_structs[m].clone(), cstr(&self.maps[m].name).to_vec()))
                } else if cstr(map) == b"middleland" {
                    Some((self.cr.middle.clone(), b"middleland".to_vec()))
                } else {
                    None
                };
                let mut csi = vec![StructInfo::default(); MAXSTRUCT];
                let mut sending = Vec::new();
                if let Some((list, name)) = src {
                    for (i, s) in list.iter().enumerate().take(MAXSTRUCT) {
                        // the others get the mana stones at their own index: a non-stone before them ends the list
                        if duty == 3 || s.ty == NPC_MANASTONE as i32 {
                            csi[i] = *s;
                        }
                    }
                    sending = name;
                }
                let c = self.client_mut(h).unwrap();
                (c.cr.csi, c.cr.csi_send_point, c.cr.sending_map) = (csi, 0, sending);
                self.send_map_status(h);
            }
            _ => {}
        }
    }

    /// `_SendMapStatus`: up to 100 structures per NOTIFY_MAPSTATUSNEXT, the rest on the next 3 s timers.
    pub fn send_map_status(&mut self, h: usize) {
        let Some(c) = self.client_mut(h) else { return };
        let mut d = vec![0u8; 13];
        let n = c.cr.sending_map.len().min(10);
        d[..n].copy_from_slice(&c.cr.sending_map[..n]);
        d[10..12].copy_from_slice(&(c.cr.csi_send_point as i16).to_le_bytes());
        if c.cr.csi_send_point == 0 {
            c.cr.is_sending_map_status = true;
        }
        let mut count = 0u8;
        let mut last = false;
        for _ in 0..100 {
            let p = c.cr.csi_send_point;
            if p >= MAXSTRUCT || c.cr.csi.get(p).map_or(true, |s| s.ty == 0) || (p >= 3 && c.crusade_duty != 3) {
                last = true;
                break;
            }
            let s = c.cr.csi[p];
            d.push(s.ty as u8);
            d.extend_from_slice(&s.x.to_le_bytes());
            d.extend_from_slice(&s.y.to_le_bytes());
            d.push(s.side as u8);
            count += 1;
            c.cr.csi_send_point += 1;
        }
        d[12] = count;
        if last {
            c.cr.is_sending_map_status = false;
        }
        let len = d.len() as i32;
        self.notify(h, if last { NOTIFY_MAPSTATUSLAST } else { NOTIFY_MAPSTATUSNEXT }, &[len], Some(&d));
    }

    /// `CheckCommanderConstructionPoint` (the 3 s timer, points > 0): a fighter's / constructor's points go to his
    /// guild's commander, here or on another server.
    pub fn check_commander_cp(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if !self.cr.mode || c.construction_point <= 0 || !matches!(c.crusade_duty, 1 | 2) {
            return;
        }
        let (cp, guid) = (c.construction_point, c.guild_guid);
        for i in self.handles() {
            let o = self.client_mut(i).unwrap();
            if o.crusade_duty == 3 && o.guild_guid == guid {
                o.construction_point = (o.construction_point + cp).min(MAXCONSTRUCTIONPOINT);
                o.war_contribution = (o.war_contribution + cp / 6).min(MAXWARCONTRIBUTION);
                let (ocp, owc) = (o.construction_point, o.war_contribution);
                self.notify(i, NOTIFY_CONSTRUCTIONPOINT, &[ocp, owc, 0], None);
                self.client_mut(h).unwrap().construction_point = 0;
                return;
            }
        }
        let mut d = vec![GSM_CONSTRUCTIONPOINT as u8];
        d.extend_from_slice(&guid.to_le_bytes());
        d.extend_from_slice(&cp.to_le_bytes());
        self.stock_msg(&d);
        self.client_mut(h).unwrap().construction_point = 0;
    }

    /// `GSM_ConstructionPoint`
    pub fn gsm_construction_point(&mut self, guid: i32, pts: i32) {
        if !self.cr.mode {
            return;
        }
        for i in self.handles() {
            let o = self.client_mut(i).unwrap();
            if o.crusade_duty == 3 && o.guild_guid == guid {
                o.construction_point = (o.construction_point + pts).min(MAXCONSTRUCTIONPOINT);
                o.war_contribution = (o.war_contribution + pts / 6).min(MAXWARCONTRIBUTION);
                let (ocp, owc) = (o.construction_point, o.war_contribution);
                self.notify(i, NOTIFY_CONSTRUCTIONPOINT, &[ocp, owc, 0], None);
                return;
            }
        }
    }

    /// `SendCollectedMana` (4 s timer)
    pub fn send_collected_mana(&mut self) {
        let cm = self.cr.collected_mana;
        if !self.cr.mode || (cm[1] == 0 && cm[2] == 0) {
            return;
        }
        let mut d = vec![GSM_COLLECTEDMANA as u8];
        d.extend_from_slice(&(cm[1] as u16).to_le_bytes());
        d.extend_from_slice(&(cm[2] as u16).to_le_bytes());
        self.cr.mana[ARESDEN as usize] += cm[1];
        self.cr.mana[ELVINE as usize] += cm[2];
        self.stock_msg(&d);
        self.cr.collected_mana = [0; 4];
    }

    /// GSM_COLLECTEDMANA from another server
    pub fn gsm_collected_mana(&mut self, a: i32, e: i32) {
        // fix: the C++ read the two counts and dropped them, so the mana collected in middleland never reached the
        // grand magic generators of a town server (the meteor strike only worked with every map on one server)
        if self.cr.mode {
            self.cr.mana[ARESDEN as usize] += a;
            self.cr.mana[ELVINE as usize] += e;
        }
    }

    /// `SyncMiddlelandMapInfo` (3 s timer): the middleland structures, here and for the other servers.
    pub fn sync_middleland(&mut self) {
        if !self.cr.mode {
            return;
        }
        let Some(mid) = self.middleland_map() else { return };
        self.cr.middle = self.cr.map_structs[mid].clone();
        let n = self.cr.middle.len();
        let mut d = vec![GSM_MIDDLEMAPSTATUS as u8];
        d.extend_from_slice(&(n as i16).to_le_bytes());
        for s in &self.cr.middle {
            d.push(s.ty as u8);
            d.push(s.side as u8);
            d.extend_from_slice(&s.x.to_le_bytes());
            d.extend_from_slice(&s.y.to_le_bytes());
        }
        if n != 0 {
            self.stock_msg(&d);
        }
    }

    /// GSM_MIDDLEMAPSTATUS (`d` from the count on); returns the bytes read
    pub fn gsm_middle_map_status(&mut self, d: &crate::util::Rd) -> usize {
        let n = d.u16(0) as i16 as i32;
        self.cr.middle = (0..n.max(0) as usize)
            .map(|i| {
                let o = 2 + i * 6;
                StructInfo { ty: d.u8(o) as i8 as i32, side: d.u8(o + 1) as i8 as i32, x: d.u16(o + 2) as i16, y: d.u16(o + 4) as i16 }
            })
            .collect();
        2 + n.max(0) as usize * 6
    }

    /// `CMap::bAddCrusadeStructureInfo` (bCreateNewNpc of types 36-39 and 42)
    pub fn add_structure_info(&mut self, m: usize, ty: i32, x: i16, y: i16, side: i32) {
        if let Some(l) = self.cr.map_structs.get_mut(m) {
            if l.len() < MAXSTRUCT {
                l.push(StructInfo { ty, side, x, y });
            }
        }
    }

    /// the crusade part of DeleteNpc: the structure info and the guild's unit counts
    pub fn crusade_npc_deleted(&mut self, i: usize) {
        let n = self.npcs[i].as_ref().unwrap();
        let (t, m, x, y, guid) = (n.type_, n.map_index, n.x, n.y, n.guild_guid);
        let now = self.now();
        match t {
            NPC_AGT | NPC_CGT | NPC_MS | NPC_DT | NPC_MANASTONE => {
                // bRemoveCrusadeStructureInfo: the first entry at (x, y), the rest moves up
                if let Some(l) = self.cr.map_structs.get_mut(m) {
                    if let Some(k) = l.iter().position(|s| s.x == x && s.y == y) {
                        l.remove(k);
                    }
                }
                if let Some(g) = self.cr.guild_locs.iter_mut().find(|g| g.guid == guid) {
                    g.tp_time = now;
                    g.structures = (g.structures - 1).max(0);
                }
            }
            NPC_LWB | NPC_GHK | NPC_GHKABS | NPC_TK | NPC_BG | NPC_CP => {
                if let Some(g) = self.cr.guild_locs.iter_mut().find(|g| g.guid == guid) {
                    g.summons = (g.summons - 1).max(0);
                }
            }
            t if t == crate::farming::NPC_CROPS => self.remove_crops_total(m),
            _ => {}
        }
    }

    /// `MeteorStrikeMsgHandler`: the grand magic of `side` falls on the enemy town in 5 s.
    pub fn meteor_strike_msg(&mut self, side: i32) {
        if !self.cr.mode {
            return;
        }
        let (target, loc, here, there) = match side {
            1 => (self.elvine_map(), b"elvine".as_slice(), 1, 2),
            2 => (self.aresden_map(), b"aresden".as_slice(), 3, 4),
            _ => return,
        };
        match target {
            Some(m) => {
                for i in self.handles() {
                    let c = self.client(i).unwrap();
                    if c.is_init_complete {
                        let v = if cstr(&self.maps[c.map_index].location_name) == loc { here } else { there };
                        self.notify(i, NOTIFY_METEORSTRIKECOMING, &[v], None);
                    }
                }
                let t = self.now().wrapping_add(5000);
                self.register_meteor_event(DELAYEVENTTYPE_METEORSTRIKE, t, m);
            }
            None => {
                for i in self.handles() {
                    self.notify(i, NOTIFY_METEORSTRIKECOMING, &[there], None);
                }
            }
        }
    }

    fn register_meteor_event(&mut self, ty: i32, trigger: u32, m: usize) {
        self.register_delay_event_at(DelayEvent { ty, effect: 0, trigger, target: 0, tt: 0, m, dx: 0, dy: 0, v1: 0, v2: 0 });
    }

    /// the three meteor delay events of DelayEventProcessor
    pub fn meteor_delay_event(&mut self, ty: i32, m: usize) {
        match ty {
            DELAYEVENTTYPE_METEORSTRIKE => self.meteor_strike(m),
            DELAYEVENTTYPE_DOMETEORSTRIKEDAMAGE => self.meteor_damage(m),
            DELAYEVENTTYPE_CALCMETEORSTRIKEEFFECT => self.calc_meteor_effect(m),
            _ => {}
        }
    }

    fn strike_total(&self, m: usize) -> usize {
        self.cr.strike.get(m).map_or(0, |s| s.1)
    }

    fn strike_hp(&self, m: usize, i: usize) -> i32 {
        self.cr.strike[m].0.get(i).and_then(|p| p.as_ref()).map_or(0, |p| p.hp)
    }

    /// `MeteorStrikeHandler`: each standing strike point loses 1d3 - (energy shield generators within 30) HP.
    fn meteor_strike(&mut self, m: usize) {
        if !self.cr.mode {
            return;
        }
        log("(!) Beginning Meteor Strike Procedure...");
        let total = self.strike_total(m);
        if total == 0 {
            log("(X) MeteorStrikeHandler Error! No Strike Points!");
            return;
        }
        // the C++ reads strike points 1..=count
        let targets: Vec<usize> = (1..=total).filter(|&i| self.strike_hp(m, i) > 0).collect();
        log(&format!("(!) Map({}) has {} available strike points", crate::util::l1(&self.maps[m].name), targets.len()));
        self.cr.meteor = (0, 0, 0);
        let now = self.now();
        if targets.is_empty() {
            log("(!) No strike points!");
            self.register_meteor_event(DELAYEVENTTYPE_CALCMETEORSTRIKEEFFECT, now.wrapping_add(6000), m);
            return;
        }
        for i in self.handles() {
            let c = self.client(i).unwrap();
            if c.is_init_complete && c.map_index == m {
                self.notify(i, NOTIFY_METEORSTRIKEHIT, &[], None);
            }
        }
        for t in targets {
            let p = self.cr.strike[m].0[t].clone().unwrap();
            let mut esg = 0;
            for ix in p.x - 30..=p.x + 30 {
                for iy in p.y - 30..=p.y + 30 {
                    let (o, ot) = self.maps[m].get_owner(ix, iy);
                    if ot == OWNERTYPE_NPC && self.npcs.get(o as usize).and_then(|n| n.as_ref()).is_some_and(|n| n.type_ == NPC_ESG) {
                        esg += 1;
                    }
                }
            }
            log(&format!("(!) Meteor Strike Target({}, {}) ESG({esg})", p.x, p.y));
            if esg >= 2 {
                continue;
            }
            let d = self.dice(1, 3) - esg;
            let sp = self.cr.strike[m].0[t].as_mut().unwrap();
            sp.hp -= d;
            if sp.hp <= 0 {
                sp.hp = 0;
                // fix: the C++ indexed the map list with -1 when the building map is on another server
                if let Some(r) = p.map {
                    self.cr.disabled[r] = true;
                }
                self.cr.meteor.1 += 1;
            } else {
                self.cr.meteor.2 += self.dice(1, 3) - esg;
                let e = (self.dice(1, 5) - 1) as usize;
                // MSVC evaluates the arguments right to left: the y roll comes first
                let ry = self.dice(1, 3) - 2;
                let rx = self.dice(1, 3) - 2;
                let (ex, ey) = p.effect[e];
                self.add_dynamic_object(0, 0, DYNAMICOBJECT_FIRE2, m, ex + rx as i16, ey + ry as i16, 60 * 1000 * 50, 0);
            }
        }
        for dt in [1000, 3000, 5000] {
            self.register_meteor_event(DELAYEVENTTYPE_DOMETEORSTRIKEDAMAGE, now.wrapping_add(dt), m);
        }
        self.register_meteor_event(DELAYEVENTTYPE_CALCMETEORSTRIKEEFFECT, now.wrapping_add(7000), m);
    }

    /// `DoMeteorStrikeDamageHandler`: 1d(level x 3) (at most 400) to every citizen on the map.
    fn meteor_damage(&mut self, m: usize) {
        if !self.cr.mode {
            return;
        }
        for i in self.handles() {
            let c = self.client(i).unwrap();
            if c.side == 0 || c.map_index != m {
                continue;
            }
            let mut dmg = self.dice(1, c.level * 3);
            let c = self.client(i).unwrap();
            match c.magic_effect_status[MAGICTYPE_PROTECT as usize] {
                2 => dmg = dmg / 2 - 2,
                5 => dmg = 0,
                _ => {}
            }
            if c.admin_user_level > 0 {
                dmg = 0;
            }
            // fix: a protected player hit for 1-4 got -2..0 "damage" and was healed
            let dmg = dmg.clamp(0, 400);
            let c = self.client_mut(i).unwrap();
            c.hp -= dmg;
            if c.hp <= 0 {
                self.killed_handler(i, 0, 0, dmg);
                self.cr.meteor.0 += 1;
            } else if dmg > 0 {
                self.notify(i, NOTIFY_HP, &[], None);
                self.send_event_near_type_a(i, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, dmg as i16, 0, 0);
                let c = self.client(i).unwrap();
                // fix: the C++ tested m_bSkillUsingStatus[19], past the skill array (pretend corpse is 10)
                if !c.skill_using[crate::combat::SKILL_PRETENDCORPSE as usize] {
                    let (cm, x, y) = (c.map_index, c.x, c.y);
                    self.maps[cm].clear_owner(i as i16, OWNERTYPE_PLAYER, x, y);
                    self.maps[cm].set_owner(i as i16, OWNERTYPE_PLAYER, x, y);
                }
                let c = self.client_mut(i).unwrap();
                let hold = c.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize];
                if hold != 0 {
                    self.notify(i, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_HOLDOBJECT, hold], None);
                    self.client_mut(i).unwrap().magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] = 0;
                    self.remove_delay_events(i, OWNERTYPE_PLAYER, MAGICTYPE_HOLDOBJECT);
                }
            }
        }
    }

    /// `CalcMeteorStrikeEffectHandler`: a town with no strike point left loses the crusade; otherwise the result
    /// goes to every server.
    fn calc_meteor_effect(&mut self, m: usize) {
        if !self.cr.mode {
            return;
        }
        let total = self.strike_total(m);
        let hps: Vec<i32> = (1..=total).map(|i| self.strike_hp(m, i).max(0)).collect();
        let active = hps.iter().filter(|&&h| h > 0).count() as i32;
        log(&format!("ActiveStructure:{active}  MapIndex:{m} Life: {hps:?}"));
        let (cas, crashed, dmg) = self.cr.meteor;
        let name = Game::pad10(&self.maps[m].name);
        if active == 0 {
            let w = if Some(m) == self.aresden_map() {
                2
            } else if Some(m) == self.elvine_map() {
                1
            } else {
                0
            };
            self.local_end_crusade(w);
            let mut d = vec![GSM_ENDCRUSADE as u8, w as u8];
            for v in [crashed, dmg, cas] {
                d.extend_from_slice(&(v as u16).to_le_bytes());
            }
            d.extend_from_slice(&name);
            self.stock_msg(&d);
        } else {
            let mut data = (total as u16).to_le_bytes().to_vec();
            for h in &hps {
                data.extend_from_slice(&(*h as u16).to_le_bytes());
            }
            let mut d = vec![GSM_GRANDMAGICRESULT as u8];
            d.extend_from_slice(&name);
            for v in [crashed, dmg, cas, active] {
                d.extend_from_slice(&(v as u16).to_le_bytes());
            }
            // fix: the C++ sent 18 + (count + 1) * 2 bytes of its 19 + (count + 1) * 2, losing the last byte
            d.extend_from_slice(&data);
            self.stock_msg(&d);
            self.grand_magic_result(&name, crashed, dmg, cas, active, total as i32, &data);
        }
        self.cr.meteor = (0, 0, 0);
    }

    /// `GrandMagicResultHandler` (`data`: the strike point count and their HP, u16 each)
    pub fn grand_magic_result(&mut self, map: &[u8], crashed: i32, dmg: i32, cas: i32, active: i32, count: i32, data: &[u8]) {
        if !self.cr.mode {
            return;
        }
        for i in self.handles() {
            self.notify2(i, NOTIFY_GRANDMAGICRESULT, &[crashed, dmg, cas, active, 0, 0, 0, 0, count], Some(map), Some(data));
        }
    }

    /// `_GrandMagicLaunchMsgSend`
    fn grand_magic_launch(&mut self, ty: i32, side: i32) {
        if !self.cr.mode {
            return;
        }
        let mut d = vec![GSM_GRANDMAGICLAUNCH as u8];
        d.extend_from_slice(&(ty as u16).to_le_bytes());
        d.extend_from_slice(&(side as u16).to_le_bytes());
        self.stock_msg(&d);
    }

    /// the action-limit-5 part of CNpc::behavior_stop: true when it searches a target (towers)
    pub fn structure_stop(&mut self, i: usize) -> bool {
        let n = self.npcs[i].as_mut().unwrap();
        match n.type_ {
            NPC_MS | NPC_DT | NPC_GMG => {
                if n.behavior_turn_count < 3 {
                    return false;
                }
                n.behavior_turn_count = 0;
                let ty = n.type_;
                let hit = match ty {
                    NPC_MS => self.mana_collector(i),
                    NPC_DT => self.detector(i),
                    _ => {
                        self.grand_magic_generator(i);
                        false
                    }
                };
                if hit {
                    let n = self.npcs[i].as_ref().unwrap();
                    let (x, y) = (n.x, n.y);
                    self.send_event_near_npc_v(i, MSGID_EVENT_MOTION, OBJECTATTACK as u16, x, y, 1);
                }
                false
            }
            NPC_ESG => false,
            NPC_MANASTONE => {
                n.behavior_turn_count = 0;
                n.v1 = (n.v1 + 5).min(5);
                false
            }
            _ => true,
        }
    }

    /// `CNpc::behavior_manaCollector`: same-side players around get 1d(Mag) MP, the mana stones give 3 mana.
    fn mana_collector(&mut self, i: usize) -> bool {
        let n = self.npcs[i].as_ref().unwrap();
        if n.appr2 != 0 {
            return false;
        }
        let (m, sx, sy, side) = (n.map_index, n.x, n.y, n.side);
        let mut hit = false;
        for x in sx - 5..=sx + 5 {
            for y in sy - 5..=sy + 5 {
                let (o, ot) = self.maps[m].get_owner(x, y);
                if o == 0 {
                    continue;
                }
                let o = o as usize;
                match ot {
                    OWNERTYPE_PLAYER => {
                        let Some(c) = self.client(o) else { continue };
                        let max = c.get_max_mp();
                        if side != c.side || c.mp >= max {
                            continue;
                        }
                        let mag = c.get_mag();
                        let mut t = self.dice(1, mag);
                        let c = self.client_mut(o).unwrap();
                        if c.fx.add_mp != 0 {
                            t += (c.fx.add_mp as f64 / 100.0 * t as f64) as i32;
                        }
                        c.mp = (c.mp + t).min(max);
                        self.notify(o, NOTIFY_MP, &[], None);
                    }
                    OWNERTYPE_NPC => {
                        let Some(s) = self.npcs.get_mut(o).and_then(|n| n.as_mut()) else { continue };
                        if s.type_ == NPC_MANASTONE && s.v1 > 0 {
                            let k = s.v1.min(3);
                            s.v1 -= k;
                            if let Some(cm) = self.cr.collected_mana.get_mut(side as usize) {
                                *cm += k;
                            }
                            hit = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        hit
    }

    /// `CNpc::behavior_detector`: enemies within 10 lose their invisibility.
    fn detector(&mut self, i: usize) -> bool {
        let n = self.npcs[i].as_ref().unwrap();
        if n.appr2 != 0 {
            return false;
        }
        let (m, sx, sy, side) = (n.map_index, n.x, n.y, n.side);
        let mut hit = false;
        for x in sx - 10..=sx + 10 {
            for y in sy - 10..=sy + 10 {
                let (o, ot) = self.maps[m].get_owner(x, y);
                let o = o as usize;
                let us = match ot {
                    OWNERTYPE_PLAYER if o != 0 => self.client(o).map(|c| c.side),
                    OWNERTYPE_NPC if o != 0 => self.npcs.get(o).and_then(|n| n.as_ref()).map(|n| n.side),
                    _ => None,
                };
                let Some(us) = us else { continue };
                if us == NEUTRAL || us == side {
                    continue;
                }
                if self.unit_effect(o, ot, MAGICTYPE_INVISIBILITY) != 0 {
                    // the effect and its status bit go, without MAGICEFFECTOFF (its release event finds 0 later)
                    let (e, st) = self.unit_mut(o, ot).unwrap();
                    e[MAGICTYPE_INVISIBILITY as usize] = 0;
                    *st &= !STATUS_INVISIBILITY;
                    if ot == OWNERTYPE_PLAYER {
                        self.send_event_near_type_a(o, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
                    } else {
                        self.send_event_near_npc(o, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16);
                    }
                }
                hit = true;
            }
        }
        hit
    }

    /// `CNpc::behavior_grandMagicGenerator`: past Mana-Stone mana, the meteor strike on the enemy town.
    fn grand_magic_generator(&mut self, i: usize) {
        let side = self.npcs[i].as_ref().unwrap().side;
        if side == NEUTRAL || !(0..4).contains(&side) {
            return;
        }
        if self.cr.mana[side as usize] > self.cr.max_gmg_mana {
            self.grand_magic_launch(1, side);
            self.meteor_strike_msg(side);
            self.cr.mana[side as usize] = 0;
        }
    }

    /// the GMG part of calculateAttackEffect: an enemy's hits drain the side's mana (1 per 500 damage points)
    pub fn gmg_hit(&mut self, i: usize, attacker_side: i32, ap_l: i32) {
        if attacker_side == 0 {
            return;
        }
        let n = self.npcs[i].as_mut().unwrap();
        n.v1 += ap_l;
        if n.v1 > 500 {
            n.v1 = 0;
            let s = n.side as usize;
            if let Some(m) = self.cr.mana.get_mut(s) {
                *m = (*m - 1).max(0);
                log(&format!("ManaStock down: {m}"));
            }
        }
    }

    /// the hammer (weapon 25) of a constructor on a structure being built: 0 = done (no damage), as in the C++
    pub fn hammer_structure(&mut self, ah: usize, i: usize) {
        let gm = self.client(ah).unwrap().is_gm();
        let n = self.npcs[i].as_mut().unwrap();
        if matches!(n.type_, NPC_AGT | NPC_CGT | NPC_MS | NPC_DT) {
            if gm {
                n.build_count = 1;
            }
            let ty = n.type_;
            match n.build_count {
                1 => {
                    n.appr2 = 0;
                    self.send_event_near_npc(i, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16);
                    if self.cr.mode {
                        let w = if matches!(ty, NPC_AGT | NPC_CGT) { 700 } else { 500 };
                        let c = self.client_mut(ah).unwrap();
                        c.war_contribution = (c.war_contribution + w).min(MAXWARCONTRIBUTION);
                        let (cp, wc) = (c.construction_point, c.war_contribution);
                        self.notify(ah, NOTIFY_CONSTRUCTIONPOINT, &[cp, wc, 0], None);
                    }
                }
                5 | 10 => {
                    n.appr2 = if n.build_count == 5 { 1 } else { 2 };
                    self.send_event_near_npc(i, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16);
                }
                _ => {}
            }
        }
        let n = self.npcs[i].as_mut().unwrap();
        n.build_count = (n.build_count - 1).max(0);
    }

    /// the crusade part of NpcKilledHandler: construction points and war contribution for an enemy's kill
    pub fn crusade_npc_killed(&mut self, attacker: usize, at: u8, i: usize) {
        if !self.cr.mode {
            return;
        }
        let n = self.npcs[i].as_ref().unwrap();
        let (cp, wc) = match n.type_ {
            1..=6 => (50, 100),
            36 | 37 => (700, 4000),
            38 => (500, 2000),
            39 => (500, 2000),
            40 => (1500, 5000),
            41 => (5000, 10000),
            43 => (500, 1000),
            44 => (1000, 2000),
            45 => (1500, 3000),
            46 => (1000, 2000),
            47 => (1500, 3000),
            51 => (800, 1500),
            64 => {
                // a crop killed in a crusade (case 64 of the points switch); DeleteNpc counts it out again
                let m = n.map_index;
                self.remove_crops_total(m);
                return;
            }
            _ => return,
        };
        let side = n.side;
        match at {
            OWNERTYPE_PLAYER => {
                let Some(c) = self.client_mut(attacker) else { return };
                if c.side != side {
                    c.construction_point = (c.construction_point + cp).min(MAXCONSTRUCTIONPOINT);
                    c.war_contribution = (c.war_contribution + wc).min(MAXWARCONTRIBUTION);
                } else {
                    c.war_contribution = 0;
                    let acc = String::from_utf8_lossy(cstr(&c.account_name)).into_owned();
                    log(&format!("WarContribution: Friendly Npc Killed by player({acc})! "));
                }
                let c = self.client(attacker).unwrap();
                let (ccp, cwc) = (c.construction_point, c.war_contribution);
                self.notify(attacker, NOTIFY_CONSTRUCTIONPOINT, &[ccp, cwc, 0], None);
            }
            OWNERTYPE_NPC => {
                let Some(a) = self.npcs.get(attacker).and_then(|n| n.as_ref()) else { return };
                let (guid, aside) = (a.guild_guid, a.side);
                if guid == 0 || aside == side {
                    return;
                }
                for h in self.handles() {
                    let c = self.client_mut(h).unwrap();
                    if c.guild_guid == guid && c.crusade_duty == 3 {
                        c.construction_point = (c.construction_point + cp).min(MAXCONSTRUCTIONPOINT);
                        let (ccp, cwc) = (c.construction_point, c.war_contribution);
                        self.notify(h, NOTIFY_CONSTRUCTIONPOINT, &[ccp, cwc, 0], None);
                        return;
                    }
                }
                let mut d = vec![GSM_CONSTRUCTIONPOINT as u8];
                d.extend_from_slice(&guid.to_le_bytes());
                d.extend_from_slice(&cp.to_le_bytes());
                self.stock_msg(&d);
            }
            _ => {}
        }
    }

    /// `__bSetConstructionKit` (ITEMEFFECTTYPE_CONSTRUCTIONKIT): a structure to build at (x, y).
    pub fn set_construction_kit(&mut self, h: usize, x: i16, y: i16, ty: i32) -> bool {
        let Some(c) = self.client(h) else { return false };
        let (m, side) = (c.map_index, c.side);
        if self.cr.map_structs.get(m).map_or(0, |l| l.len()) >= MAXSTRUCT {
            self.notify(h, NOTIFY_NOMORECRUSADESTRUCTURE, &[], None);
            return false;
        }
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return false };
        // CMap::IsBuild (the map's build areas, a heldenian/astoria feature): no map info line here sets one
        // shortcut: no build area is read, every tile counts as buildable; port it with the astoria layer
        if !self.cr.mode && self.ast.ev.is_none() {
            self.maps[m].set_naming_value_empty(nv);
            return false;
        }
        for ix in x - 1..=x + 1 {
            for iy in y - 1..=y + 1 {
                let (o, ot) = self.maps[m].get_owner(ix, iy);
                if o != 0 && ot == OWNERTYPE_NPC && self.npcs.get(o as usize).and_then(|n| n.as_ref()).is_some_and(|n| n.action_limit == 5) {
                    // fix: the C++ kept the naming value on these returns
                    self.maps[m].set_naming_value_empty(nv);
                    return false;
                }
            }
        }
        let s = match side {
            ARESDEN => "Aresden",
            ELVINE => "Elvine",
            ISTRIA => "Istria",
            _ => {
                self.maps[m].set_naming_value_empty(nv);
                return false;
            }
        };
        let p = match ty {
            1 => "AGT",
            2 => "CGT",
            3 => "MS",
            4 => "DT",
            _ => "",
        };
        let npc_name = if p.is_empty() { String::new() } else { format!("{p}-{s}") };
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        if self.create_npc(npc_name.as_bytes(), &name, m, 0, MOVETYPE_RANDOM, Some((x, y)), &[-1; 10], None, -1).is_none() {
            self.maps[m].set_naming_value_empty(nv);
            return false;
        }
        true
    }

    /// the GM commands of ChatMsgHandler: `/crusade` and `/endcrusade` (admin level 4)
    pub fn crusade_gm_command(&mut self, h: usize, cmd: &[u8]) -> bool {
        let Some(c) = self.client(h) else { return false };
        let (lvl, name) = (c.admin_user_level, String::from_utf8_lossy(cstr(&c.char_name)).into_owned());
        if cmd.starts_with(b"/crusade") {
            if lvl >= 4 {
                self.global_start_crusade();
                self.send_ls_plain(MSGID_GAMEMASTERLOG, format!("GM Order({name}): begincrusadetotalwar").as_bytes());
            }
        } else if cmd.starts_with(b"/endcrusade") {
            if lvl >= 4 {
                self.manual_end_crusade(0);
                self.send_ls_plain(MSGID_GAMEMASTERLOG, format!("GM Order({name}): endcrusade").as_bytes());
            }
        } else {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crusade_cfg() {
        let (st, mana, ok) = parse_crusade_cfg(b"// x\ncrusade-structure = 1  aresden 40 145 128 // cityhall left\ncrusade-structure = 17 middleland 42 418 147\nMana-Stone = 5000\n");
        assert!(ok);
        assert_eq!(mana, 5000);
        assert_eq!(st[1], Some((b"aresden".to_vec(), 40, 145, 128)));
        assert_eq!(st[17].as_ref().unwrap().1, 42);
        assert!(st[2].is_none());
        let (_, _, ok) = parse_crusade_cfg(b"crusade-structure = 1 aresden 40 1 1\ncrusade-structure = 1 aresden 40 2 2\n");
        assert!(!ok, "duplicate number");
        let (_, _, ok) = parse_crusade_cfg(b"crusade-structure = 400 aresden 40 1 1\n");
        assert!(!ok, "past the array");
    }

    #[test]
    fn costs() {
        assert_eq!(npc_construction_point(43), 1000);
        assert_eq!(npc_construction_point(36), 0);
        assert!(is_crusade_object(NPC_MANASTONE) && !is_crusade_object(21));
    }
}
