//! NPCs: `CNpc` (char/Npc.h/.cpp), `bCreateNewNpc`, `initNpcAttr`, the spot part of `MobGenerator`, the map-info
//! `npc` lines (`_bReadMapInfoFiles` case 3), `NpcProcess` with `behavior_move`/`behavior_stop`, `cGetNextMoveDir`
//! (+ CMisc::GetPoint / cGetNextMoveDir), and the NPC branch of `SendEventToNearClient_TypeA`.
//! Not yet: target search / attack / flee / death (combat layer), the random mob generator, summons, special
//! abilities (`_cGetSpecialAbility`), crusade structures.
use crate::client::{ARESDEN, ELVINE, MAGICTYPE_BERSERK, MAGICTYPE_HOLDOBJECT, SIDE_MAP, STATUS_BERSERK};
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::map::OWNERTYPE_NPC;
use crate::util::{atoi, cstr, W};

pub const MAXNPCS: usize = 15000;
pub const MOVETYPE_STOP: u8 = 0;
pub const MOVETYPE_SEQWAYPOINT: u8 = 1;
pub const MOVETYPE_RANDOMWAYPOINT: u8 = 2;
pub const MOVETYPE_FOLLOW: u8 = 3;
pub const MOVETYPE_RANDOMAREA: u8 = 4;
pub const MOVETYPE_RANDOM: u8 = 5;
pub const MOVETYPE_GUARD: u8 = 6;
pub const BEHAVIOR_STOP: u8 = 0;
pub const BEHAVIOR_MOVE: u8 = 1;
pub const BEHAVIOR_ATTACK: u8 = 2;
pub const BEHAVIOR_FLEE: u8 = 3;
pub const BEHAVIOR_DEAD: u8 = 4;
// Shared/npcType.h
pub const NPC_CP: i32 = 51;
pub const NPC_DARK_ELF: i32 = 54;
pub const NPC_RABBIT: i32 = 55;
pub const NPC_CAT: i32 = 56;
pub const NPC_CT: i32 = 87;
pub const NPC_AGC: i32 = 89;
const DIR_X: [i16; 9] = [0, 0, 1, 1, 1, 0, -1, -1, -1];
const DIR_Y: [i16; 9] = [0, -1, -1, 0, 1, 1, 1, 0, -1];

#[derive(Clone, Default)]
pub struct Npc {
    pub handle: usize,
    pub name: Vec<u8>, // the 5-char object name (`_L12` ...)
    pub npc_name: Vec<u8>,
    pub type_: i16,
    pub map_index: usize,
    pub x: i16,
    pub y: i16,
    pub v: (i16, i16),
    pub d: (i16, i16),
    pub dir: u8,
    pub appr2: i16,
    pub status: i32,
    pub side: i32,
    pub hp: i32,
    pub mp: i32,
    pub exp: i32,
    pub hit_dice: i32,
    pub max_mana: i32,
    pub magic_level: i32,
    pub action_limit: i32,
    pub action_time: u32,
    pub size: i32,
    pub move_type: u8,
    /// a random-generator group leader (stops 1 turn in 3 so its followers keep up)
    pub is_master: bool,
    /// MOVETYPE_FOLLOW: who is followed (m_iFollowOwnerIndex / m_cFollowOwnerType)
    pub follow_owner: usize,
    pub follow_owner_type: u8,
    pub behavior: u8,
    pub behavior_turn_count: i32,
    pub turn: u8,
    pub tmp_error: i32,
    pub waypoints: [i32; 10],
    pub total_waypoint: usize,
    pub cur_waypoint: usize,
    pub random_area: (i16, i16, i16, i16), // left, top, right, bottom
    pub spot_mob_index: i32,
    pub bravery: i32,
    pub attack_strategy: i32,
    pub ai_level: i32,
    pub is_killed: bool,
    pub time: u32,
    pub hp_up_time: u32,
    pub mp_up_time: u32,
    pub magic_effect_status: Vec<i32>,
    // combat (initNpcAttr / CNpc)
    pub defense_ratio: i32,
    pub hit_ratio: i32,
    pub attack_dice_throw: i32,
    pub attack_dice_range: i32,
    pub target_search_range: i32,
    pub attack_range: i32,
    pub abs_damage: i32,
    pub resist_magic: i32,
    pub magic_hit_ratio: i32,
    pub attribute: i32,
    pub regen_time: u32,
    pub special_ability: u8,
    pub no_die_remain_exp: i32,
    pub target: usize,
    pub target_type: u8,
    pub attack_count: i32,
    pub is_perm_attack_mode: bool,
    /// summoned (guards called on criminals): no exp, no drops, gone after SUMMONTIME
    pub is_summoned: bool,
    /// m_iSummonControlMode: 1 = /hold (the summon stays and does not strike back), 2 = /tgt
    pub summon_control_mode: i32,
    pub summoned_time: u32,
    pub dead_time: u32,
    pub last_damage: i32,
    /// m_iBuildCount (Npc.cfg min bravery): the hammer hits a structure still needs
    pub build_count: i32,
    /// m_iV1: a mana stone's mana, the damage a grand magic generator took
    pub v1: i32,
    /// m_iGuildGUID: the guild of a summoned war unit / structure
    pub guild_guid: i32,
    /// m_sOriginalType: the type before a polymorph spell (0 = never changed)
    pub original_type: i16,
    /// m_iNpcCrops: the crop of a planted seed (1..14; 0 = not a crop) and the farming skill it asks of its harvester
    pub crops: i32,
    pub crops_skill_lv: i32,
}

/// A map's `spot-mob-generator` (index, type, rect or waypoints, mob type, max mobs) with its live count.
#[derive(Clone, Default)]
pub struct Spot {
    pub index: usize,
    pub kind: i32,
    pub rect: (i16, i16, i16, i16),
    pub waypoints: [i32; 10],
    pub mob_type: i32,
    pub max_mobs: i32,
    pub cur_mobs: i32,
}

/// MobGenerator's spot table: mob type -> (NPC config name, special-ability probability).
fn spot_mob(t: i32) -> (&'static str, i32) {
    match t {
        10 => ("Slime", 5),
        16 => ("Giant-Ant", 10),
        14 => ("Orc", 15),
        18 => ("Zombie", 15),
        11 => ("Skeleton", 35),
        6 => ("Orc-Mage", 30),
        17 => ("Scorpion", 15),
        12 => ("Stone-Golem", 25),
        13 => ("Cyclops", 35),
        22 => ("Amphis", 20),
        23 => ("Clay-Golem", 20),
        24 => ("Guard-Aresden", 20),
        25 => ("Guard-Elvine", 20),
        26 => ("Guard-Neutral", 20),
        27 => ("Hellbound", 20),
        28 => ("Troll", 20),
        29 => ("Orge", 20),
        30 => ("Liche", 30),
        31 => ("Demon", 20),
        32 => ("Unicorn", 35),
        33 => ("WereWolf", 25),
        34 => ("Dummy", 5),
        35 => ("Attack-Dummy", 5),
        48 => ("Stalker", 20),
        49 => ("Hellclaw", 20),
        50 => ("Tigerworm", 20),
        52 => ("Gagoyle", 20),
        53 => ("Beholder", 20),
        54 => ("Dark-Elf", 20),
        57 => ("Giant-Frog", 10),
        58 => ("Mountain-Giant", 25),
        59 => ("Ettin", 20),
        60 => ("Cannibal-Plant", 20),
        61 => ("Rudolph", 20),
        62 => ("DireBoar", 20),
        63 => ("Frost", 20),
        65 => ("Ice-Golem", 20),
        66 => ("Wyvern", 20),
        55 => ("Rabbit", 20),
        67 => ("McGaffin", 20),
        68 => ("Perry", 20),
        69 => ("Devlin", 20),
        73 => ("Fire-Wyvern", 20),
        70 => ("Barlog", 20),
        80 => ("Tentocle", 20),
        71 => ("Centaurus", 20),
        75 => ("Giant-Lizard", 20),
        78 => ("Minotaurs", 20),
        81 => ("Abaddon", 20),
        72 => ("Claw-Turtle", 20),
        74 => ("Giant-Crayfish", 20),
        76 => ("Giant-Plant", 20),
        77 => ("MasterMage-Orc", 20),
        79 => ("Nizie", 20),
        82 => ("Guard-Istria", 20),
        _ => ("Orc", 15),
    }
}

/// MobGenerator's spot table, third column: the special-ability kind passed to `_cGetSpecialAbility`.
fn spot_kind_sa(t: i32) -> i32 {
    match t {
        18 | 17 | 22 | 48 => 3,
        11 | 13 | 30 | 31 | 49 | 50 | 52 | 53 | 54 | 59 | 63 | 65 => 8,
        6 | 32 => 7,
        12 | 23 | 60 => 5,
        16 | 57 => 2,
        66 | 73 | 81 => 10,
        _ => 1,
    }
}

impl Game {
    /// `_cGetSpecialAbility`
    pub fn special_ability(&mut self, kind: i32) -> u8 {
        let pick = |g: &mut Game, t: &[u8]| t[g.dice(1, t.len() as i32) as usize - 1];
        match kind {
            1 => pick(self, &[3, 4]),
            2 | 6 => pick(self, &[3, 4, 5]),
            3 => pick(self, &[3, 4, 5, 6]),
            4 => pick(self, &[3, 4, 7]),
            5 => pick(self, &[3, 4, 7, 8]),
            7 => pick(self, &[1, 2, 4]),
            8 => pick(self, &[1, 2, 4, 3, 8]),
            9 => self.dice(1, 8) as u8,
            10 => 1,
            _ => 0, // fix: the C++ returned an uninitialised char for other kinds
        }
    }
}

/// Per-map NPC bookkeeping the C++ keeps on CMap.
#[derive(Clone, Default)]
pub struct MapNpcs {
    pub spots: Vec<Spot>,
    pub avoid_rects: Vec<(i16, i16, i16, i16)>,
    pub total_active: i32,
    pub total_alive: i32,
    /// CMap::m_iTotalAgriculture: the crops planted on the map
    pub total_agriculture: i32,
}

fn rect(f: &[Vec<u8>], i: usize) -> (i16, i16, i16, i16) {
    (atoi(&f[i]) as i16, atoi(&f[i + 1]) as i16, atoi(&f[i + 2]) as i16, atoi(&f[i + 3]) as i16)
}

impl Game {
    /// The NPC part of OnStartGameSignal/_bReadMapInfoFiles: spot generators, avoid rects and the fixed `npc` lines.
    pub fn init_map_npcs(&mut self) {
        for m in 0..self.maps.len() {
            let mut mn = MapNpcs::default();
            for f in self.maps[m].records(b"spot-mob-generator") {
                let kind = atoi(&f[1]);
                let mut s = Spot { index: atoi(&f[0]).max(0) as usize, kind, ..Default::default() };
                if kind == 1 {
                    s.rect = rect(f, 2);
                    (s.mob_type, s.max_mobs) = (atoi(&f[6]), atoi(&f[7]));
                } else {
                    for k in 0..10 {
                        s.waypoints[k] = atoi(&f[2 + k]);
                    }
                    (s.mob_type, s.max_mobs) = (atoi(&f[12]), atoi(&f[13]));
                }
                mn.spots.push(s);
            }
            for f in self.maps[m].records(b"npc-avoidrect") {
                mn.avoid_rects.push(rect(f, 1));
            }
            self.map_npcs.push(mn);
            let lines: Vec<Vec<Vec<u8>>> = self.maps[m].records(b"npc").cloned().collect();
            for f in lines {
                // name, move type, 10 waypoint indexes, name prefix
                let mut wp = [-1i32; 10];
                for k in 0..10 {
                    wp[k] = atoi(&f[2 + k]);
                }
                let Some(nv) = self.maps[m].get_empty_naming_value() else { continue };
                let name = vec![*f[12].first().unwrap_or(&b'_'), m as u8 + 65];
                if self.create_npc(&f[0], &name, m, 0, atoi(&f[1]) as u8, None, &wp, None, -1).is_none() {
                    self.maps[m].set_naming_value_empty(nv);
                }
            }
        }
    }

    /// `bCreateNewNpc` (no summons / berserk / side change yet). Returns the NPC handle.
    #[allow(clippy::too_many_arguments)]
    pub fn create_npc(&mut self, npc_name: &[u8], name: &[u8], m: usize, sa: u8, move_type: u8, pos: Option<(i16, i16)>, wp: &[i32; 10], area: Option<(i16, i16, i16, i16)>, spot: i32) -> Option<usize> {
        if name.is_empty() || npc_name.is_empty() {
            return None;
        }
        let h = (1..MAXNPCS).find(|&i| self.npcs[i].is_none())?;
        let cfg = self.data.npcs.iter().flatten().find(|c| Game::pad20(&c.name) == Game::pad20(npc_name))?.clone();
        // initNpcAttr
        let hp = if cfg.hit_dice <= 5 { (self.dice(cfg.hit_dice, 4) + cfg.hit_dice).abs() } else { (cfg.hit_dice * 5 + self.dice(1, cfg.hit_dice)).abs() };
        // Rust only (owner, 2026-11-22): the C++ GetExp divided every exp by 100 (for its x100000 NPC exp), which left
        // quests, skills, crafting and farming at ~0; the /100 now only applies to the NPC exp, here
        let exp = (self.dice(cfg.exp_dice, 4) + cfg.exp_dice) / 100;
        let attack_strategy = self.dice(1, 10);
        let ai_level = self.dice(1, 3);
        // initNpcAttr: the special ability raises the exp (3/4 also change the damage absorption)
        let (mut sa, mut exp, mut abs_damage) = (sa, exp, cfg.abs_damage);
        let pct = |e: i32, p: f64| e + (e as f64 * p) as i32;
        match sa {
            1 | 8 => exp = pct(exp, 0.25),
            2 => exp = pct(exp, 0.30),
            3 | 4 => {
                let k = sa;
                if (k == 3 && abs_damage > 0) || (k == 4 && abs_damage < 0) {
                    sa = 0;
                } else {
                    let t = 20 + self.dice(1, 60);
                    abs_damage = if k == 3 { (abs_damage - t).max(-90) } else { (abs_damage + t).min(90) };
                }
                exp = pct(exp, if k == 3 { abs_damage.abs() } else { abs_damage } as f64 / 100.0);
            }
            5 => exp = pct(exp, 0.15),
            6 | 7 => exp = pct(exp, 0.20),
            _ => {}
        }
        let no_die_remain_exp = exp - exp / 3;
        if cfg.day_of_week_limit < 10 && cfg.day_of_week_limit as u32 != crate::chrono_now().weekday {
            return None;
        }
        let (sx, sy) = (self.maps[m].size_x, self.maps[m].size_y);
        let waypoint = |g: &Game, i: i32| g.maps[m].waypoints.get(i.max(0) as usize).copied().flatten().map_or((0, 0), |(x, y)| (x as i16, y as i16));
        let (mut x, mut y) = match move_type {
            MOVETYPE_GUARD | MOVETYPE_RANDOM => match pos {
                Some(p) if p.0 != 0 && p.1 != 0 => p,
                _ => {
                    let mut found = None;
                    for _ in 0..=30 {
                        let x = (self.rng.rand() % (sx as i32 - 50)) as i16 + 15;
                        let y = (self.rng.rand() % (sy as i32 - 50)) as i16 + 15;
                        if !self.map_npcs[m].avoid_rects.iter().any(|r| x >= r.0 && x <= r.2 && y >= r.1 && y <= r.3) {
                            found = Some((x, y));
                            break;
                        }
                    }
                    found?
                }
            },
            MOVETYPE_RANDOMAREA => {
                let a = area?;
                // fix: the C++ divided by a zero-width area (rand() % 0)
                let x = (self.rng.rand() % ((a.2 - a.0) as i32).max(1)) as i16 + a.0;
                let y = (self.rng.rand() % ((a.3 - a.1) as i32).max(1)) as i16 + a.1;
                (x, y)
            }
            MOVETYPE_RANDOMWAYPOINT => {
                let a = self.dice(1, 10) - 1;
                let b = self.dice(1, 10) - 1;
                (waypoint(self, wp[a as usize]).0, waypoint(self, wp[b as usize]).1)
            }
            _ => match pos {
                Some(p) if p.0 != 0 && p.1 != 0 => p,
                _ => waypoint(self, wp[0]),
            },
        };
        // bGetEmptyPosition must find a free tile or the NPC is not created
        (x, y) = self.find_empty_position(x, y, m)?;
        let total_waypoint = wp.iter().filter(|&&w| w != -1).count();
        let mut n = Npc {
            handle: h,
            name: name[..name.len().min(5)].to_vec(),
            npc_name: cfg.name.clone(),
            type_: cfg.type_ as i16,
            map_index: m,
            x,
            y,
            v: (x, y),
            side: if cfg.side == 10 { SIDE_HOSTILE } else { cfg.side }, // initNpcAttr: "temporary code" 10 -> HOSTILE
            hp: hp.max(1),
            mp: cfg.max_mana,
            exp,
            hit_dice: cfg.hit_dice,
            max_mana: cfg.max_mana,
            magic_level: cfg.magic_level,
            action_limit: cfg.action_limit,
            action_time: cfg.action_time,
            size: cfg.size,
            move_type,
            waypoints: *wp,
            total_waypoint,
            random_area: area.unwrap_or_default(),
            spot_mob_index: spot,
            attack_strategy,
            ai_level,
            magic_effect_status: vec![0; 100],
            defense_ratio: cfg.defense_ratio,
            hit_ratio: cfg.hit_ratio,
            attack_dice_throw: cfg.attack_dice_throw,
            attack_dice_range: cfg.attack_dice_range,
            target_search_range: cfg.target_search_range,
            attack_range: cfg.attack_range,
            abs_damage,
            resist_magic: cfg.resist_magic,
            magic_hit_ratio: cfg.magic_hit_ratio,
            attribute: cfg.attribute,
            regen_time: cfg.regen_time,
            special_ability: sa,
            no_die_remain_exp,
            ..Default::default()
        };
        n.status = (n.status & !(0xF << 8)) | ((sa as i32) << 8);
        n.status = (n.status & 0x0FFF_FFFF) | (n.side << 28);
        n.d = match move_type {
            MOVETYPE_GUARD => (x, y),
            MOVETYPE_SEQWAYPOINT => {
                n.cur_waypoint = 1;
                waypoint(self, wp[1])
            }
            MOVETYPE_RANDOMWAYPOINT => {
                // fix: the C++ took rand() % (total - 1), a division by zero with one waypoint
                n.cur_waypoint = (self.rng.rand() % (total_waypoint as i32 - 1).max(1)) as usize + 1;
                waypoint(self, wp[n.cur_waypoint.min(9)])
            }
            MOVETYPE_RANDOMAREA => {
                let a = n.random_area;
                ((self.rng.rand() % ((a.2 - a.0) as i32).max(1)) as i16 + a.0, (self.rng.rand() % ((a.3 - a.1) as i32).max(1)) as i16 + a.1)
            }
            MOVETYPE_RANDOM => ((self.rng.rand() % (sx as i32 - 50)) as i16 + 15, (self.rng.rand() % (sy as i32 - 50)) as i16 + 15),
            _ => (0, 0),
        };
        match n.action_limit {
            2 | 3 | 5 => {
                n.behavior = BEHAVIOR_STOP;
                n.dir = match n.type_ {
                    15 | 19 | 20 | 24 | 25 | 26 => (4 + self.dice(1, 3) - 1) as u8,
                    _ => self.dice(1, 8) as u8,
                };
            }
            8 => {
                n.dir = 3;
                n.behavior = BEHAVIOR_STOP;
                if n.size > 0 {
                    for ax in x - 1..=x + 1 {
                        for ay in y - 1..=y + 1 {
                            self.maps[m].set_temp_move_allowed(ax, ay, false);
                        }
                    }
                }
            }
            _ => {
                n.behavior = BEHAVIOR_MOVE;
                n.dir = 5;
            }
        }
        n.turn = (self.rng.rand() % 2) as u8;
        n.appr2 = match n.type_ {
            1..=6 => {
                let a = (self.rng.rand() % 13) << 4;
                let b = self.rng.rand() % 9;
                (0xF000u16 as i16) | a as i16 | b as i16
            }
            36..=39 => 3,
            64 => 1,
            _ => 0,
        };
        let now = self.now();
        n.time = now.wrapping_add((self.rng.rand() % 10000) as u32);
        (n.mp_up_time, n.hp_up_time) = (now, now);
        n.bravery = self.rng.rand() % 3 + cfg.min_bravery;
        n.build_count = cfg.min_bravery;
        self.maps[m].set_owner(h as i16, OWNERTYPE_NPC, x, y);
        self.map_npcs[m].total_active += 1;
        self.map_npcs[m].total_alive += 1;
        if matches!(n.type_, 36..=39 | 42) {
            self.add_structure_info(m, n.type_ as i32, x, y, n.side);
        }
        let crop = n.type_ == crate::farming::NPC_CROPS;
        self.npcs[h] = Some(n);
        if crop {
            // a crop is counted and announced by __bSetAgricultureItem once its crop type is set
            self.add_crops_total(m);
            return Some(h);
        }
        self.send_event_near_npc(h, MSGID_EVENT_LOG, MSGTYPE_CONFIRM as u16);
        Some(h)
    }

    /// `MobGenerator` (every 600 ms): per map, the random mob generator then the spot generators.
    pub fn mob_generator(&mut self) {
        // the spot tables come from the map info, read in OnStartGameSignal
        if self.on_exit_process || self.map_npcs.len() != self.maps.len() {
            return;
        }
        for m in 0..self.maps.len() {
            // fix (gameplay-visible): in a crusade the C++ `break`s out of the map loop at middleland, stopping the random
            // and spot generators of middleland and of every map after it; only middleland's random generator stops
            let mid_war = self.cr.mode && cstr(&self.maps[m].name) == b"middleland";
            // fix (gameplay-visible): in a type 1 heldenian the C++ returned from the whole generator, so no monster
            // spawned on any map of the server; only the heldenian maps stop here
            if self.hd.mode && self.hd.ty == 1 && self.is_heldenian_map(m) {
                continue;
            }
            if !mid_war && self.maps[m].random_mob_generator && self.maps[m].maximum_object - 30 > self.map_npcs[m].total_active {
                self.random_mob_generator(m);
            }
            if self.maps[m].maximum_object <= self.map_npcs[m].total_active {
                continue;
            }
            for j in 0..self.map_npcs[m].spots.len() {
                let s = self.map_npcs[m].spots[j].clone();
                // the C++ loop starts at spot 1
                if s.index < 1 || self.dice(1, 3) != 2 || s.max_mobs <= s.cur_mobs {
                    continue;
                }
                let Some(nv) = self.maps[m].get_empty_naming_value() else { continue };
                let (npc_name, prob_sa) = spot_mob(s.mob_type);
                let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
                let mut sa = 0;
                if s.mob_type != 34 && self.dice(1, 100) <= prob_sa {
                    sa = self.special_ability(spot_kind_sa(s.mob_type));
                }
                let made = match s.kind {
                    1 => self.create_npc(npc_name.as_bytes(), &name, m, sa, MOVETYPE_RANDOMAREA, None, &[-1; 10], Some(s.rect), s.index as i32),
                    2 => self.create_npc(npc_name.as_bytes(), &name, m, sa, MOVETYPE_RANDOMWAYPOINT, None, &s.waypoints, None, s.index as i32),
                    _ => None,
                };
                if made.is_some() {
                    self.map_npcs[m].spots[j].cur_mobs += 1;
                } else {
                    self.maps[m].set_naming_value_empty(nv);
                }
            }
        }
    }

    /// `SpecialEventHandler` (3 s timer): every MOBEVENTTIME (5 minutes) the next MobGenerator pass may start a mob
    /// event (type 1) or, 1 time in 350, a demon / unicorn visit (type 2).
    pub fn special_event_handler(&mut self) {
        let now = self.now();
        if now.wrapping_sub(self.special_event_time) < MOBEVENTTIME as u32 {
            return;
        }
        self.special_event_time = now;
        self.special_event = true;
        self.special_event_type = if self.dice(1, 350) == 98 { 2 } else { 1 };
    }

    /// The random part of `MobGenerator`: a master from the map's level table anywhere outside the avoid rects and,
    /// for the small kinds, a few followers next to it (bSetNpcFollowMode); and the special events: on a map that
    /// rolls 3 in 1d10 while m_bIsSpecialEventTime is on, type 1 puts the monster at the map's busiest sector with
    /// mobevent-amount followers and tells the players of the map where, type 2 sends a demon or a unicorn.
    fn random_mob_generator(&mut self, m: usize) {
        // fix: the C++ ran the follower count on an uninitialised iResult/bMaster when no naming value was free
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return };
        let master_name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        let (mut res, berserk, firm_sa) = random_mob_pick(&mut self.rng, self.maps[m].random_mob_level);
        // pX / pY: the start the master is created at (none = anywhere), then where it (or the last follower) ended up
        let mut pos: Option<(i16, i16)> = None;
        let mut event = false;
        let map_name = cstr(&self.maps[m].name).to_vec();
        if self.special_event && self.dice(1, 10) == 3 {
            event = true;
            if self.special_event_type == 1 {
                if self.maps[m].max_px != 0 {
                    let p = (self.maps[m].max_px as i16 * 20 + 10, self.maps[m].max_py as i16 * 20 + 10);
                    pos = Some(p);
                    if self.cr.mode {
                        if map_name == SIDE_MAP[ARESDEN as usize] {
                            res = 20;
                        } else if map_name == SIDE_MAP[ELVINE as usize] {
                            res = 19;
                        }
                    }
                    log(&format!("(!) Mob-Event Map({}) Loc({},{})", crate::util::l1(&map_name), p.0, p.1));
                }
            } else {
                if self.dice(1, 3) == 2 {
                    res = if is_town_map(&map_name, true) && self.dice(1, 30) != 5 { 5 } else { 16 };
                } else {
                    res = 17;
                }
                self.special_event = false;
            }
        }
        let (npc_name, prob_sa, kind_sa) = random_mob(res);
        let sa = if firm_sa != 0 { firm_sa } else if self.dice(1, 100) <= prob_sa { self.special_ability(kind_sa) } else { 0 };
        let master = self.create_npc(npc_name.as_bytes(), &master_name, m, sa, MOVETYPE_RANDOM, pos, &[-1; 10], None, 0);
        match master {
            Some(h) => {
                self.npc_set_firm(h, berserk, true);
                let n = self.npcs[h].as_ref().unwrap();
                pos = Some((n.x, n.y));
            }
            None => self.maps[m].set_naming_value_empty(nv),
        }
        let mut total = match res {
            1..=3 | 10 | 12 | 18 | 23 | 31 => self.dice(1, 5) - 1,
            4..=7 | 11 | 13..=15 | 28..=30 | 32 | 33 | 42 | 44 => self.dice(1, 3) - 1,
            8 | 9 | 16 | 17 | 21 | 22 | 24 | 25 | 27 => self.dice(1, 2) - 1,
            26 => self.dice(1, 4) - 1,
            34..=41 | 43 | 45 => 1,
            _ => 0,
        };
        if master.is_none() {
            total = 0;
        }
        if total >= 2 {
            match res {
                1..=5 | 7 | 8 | 10..=13 | 24..=30 | 32 | 33 => {
                    if self.dice(1, 5) != 1 {
                        total = 0;
                    }
                }
                6 | 9 | 14..=18 | 31 | 34..=42 | 44..=49 => {
                    if self.dice(1, 5) == 1 {
                        total = 0;
                    }
                }
                _ => {}
            }
        }
        if event {
            if self.special_event_type == 1 {
                if !matches!(res, 15 | 16 | 21) {
                    total = self.maps[m].mob_event_amount;
                }
                let (px, py) = pos.unwrap_or((0, 0));
                for j in self.map_players(m) {
                    if self.client(j).unwrap().is_init_complete {
                        self.notify(j, NOTIFY_MONSTEREVENT_POSITION, &[px as i32, py as i32, random_mob_type(res)], None);
                    }
                }
            } else if is_town_map(&map_name, false) {
                total = 0;
            }
            self.special_event = false;
        }
        for _ in 0..total {
            let Some(nv) = self.maps[m].get_empty_naming_value() else { continue };
            let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
            let sa = if firm_sa != 0 { firm_sa } else if self.dice(1, 100) <= prob_sa { self.special_ability(kind_sa) } else { 0 };
            // followers start next to the master (bCreateNewNpc hands the position back in pX/pY: each one starts
            // where the last one ended up)
            match self.create_npc(npc_name.as_bytes(), &name, m, sa, MOVETYPE_RANDOM, pos, &[-1; 10], None, 0) {
                Some(h) => {
                    self.npc_set_firm(h, berserk, false);
                    let n = self.npcs[h].as_ref().unwrap();
                    pos = Some((n.x, n.y));
                    // bSetNpcFollowMode by handle (the C++ looked both names up by their first 5 bytes); a failed master has none
                    if let Some(mh) = master {
                        let side = self.npcs[mh].as_ref().unwrap().side;
                        let n = self.npcs[h].as_mut().unwrap();
                        (n.move_type, n.follow_owner, n.follow_owner_type, n.side) = (MOVETYPE_FOLLOW, mh, OWNERTYPE_NPC, side);
                    }
                }
                None => self.maps[m].set_naming_value_empty(nv),
            }
        }
    }

    /// bCreateNewNpc's bFirmBerserk (the berserk flag; the doubled m_iExpDice is never read after initNpcAttr) and bIsMaster.
    fn npc_set_firm(&mut self, h: usize, berserk: bool, master: bool) {
        let n = self.npcs[h].as_mut().unwrap();
        n.is_master = master;
        // fix: the C++ only called SetMagicFlag (the status bit), so m_cMagicEffectStatus stayed 0 and the berserk
        // damage bonus never applied to these NPCs
        if berserk {
            n.magic_effect_status[MAGICTYPE_BERSERK as usize] = 1;
            n.status |= STATUS_BERSERK;
        }
    }

    /// `NpcProcess` (movement only until combat is ported).
    pub fn npc_process(&mut self) {
        let now = self.now();
        for i in 1..MAXNPCS {
            let Some(n) = self.npcs[i].as_ref() else { continue };
            let mut at = match n.behavior {
                BEHAVIOR_ATTACK => n.action_time + 50 * self.rng.dice(1, 7) as u32,
                BEHAVIOR_MOVE | BEHAVIOR_FLEE => n.action_time + 400,
                _ => n.action_time,
            };
            let n = self.npcs[i].as_mut().unwrap();
            if n.magic_effect_status[crate::client::MAGICTYPE_ICE as usize] != 0 {
                at += at / 2;
            }
            if now.wrapping_sub(n.time) as i32 <= at as i32 {
                continue;
            }
            n.time = now;
            if n.magic_level != 0 && now.wrapping_sub(n.mp_up_time) > MPUPTIME as u32 {
                n.mp_up_time = now;
                let add = self.rng.dice(1, n.max_mana / 5);
                let n = self.npcs[i].as_mut().unwrap();
                n.mp = (n.mp + add).min(n.max_mana);
            }
            let n = self.npcs[i].as_mut().unwrap();
            if now.wrapping_sub(n.hp_up_time) > HPUPTIME as u32 && !n.is_killed {
                n.hp_up_time = now;
                let hd = n.hit_dice;
                let max = self.rng.dice(hd, 8) + hd;
                let n = self.npcs[i].as_mut().unwrap();
                if n.hp < max {
                    let add = self.rng.dice(1, hd);
                    let n = self.npcs[i].as_mut().unwrap();
                    n.hp = (n.hp + add).min(max).max(1);
                }
            }
            match self.npcs[i].as_ref().unwrap().behavior {
                BEHAVIOR_DEAD => self.npc_behavior_dead(i),
                BEHAVIOR_STOP => self.npc_behavior_stop(i),
                BEHAVIOR_MOVE => self.npc_behavior_move(i),
                BEHAVIOR_ATTACK => self.npc_behavior_attack(i),
                BEHAVIOR_FLEE => self.npc_behavior_flee(i),
                _ => {}
            }
            if let Some(n) = self.npcs[i].as_ref().filter(|n| n.is_summoned && n.hp != 0) {
                let age = now.wrapping_sub(n.summoned_time);
                match n.type_ {
                    29 if age > 1000 * 90 => self.npc_killed(0, 0, i, 0),
                    64 if age > PLANTTIME as u32 => self.delete_npc(i),
                    29 | 64 => {}
                    _ if age > SUMMONTIME as u32 => self.npc_killed(0, 0, i, 0),
                    _ => {}
                }
            }
        }
    }

    /// `CNpc::behavior_move` without target search (combat layer) and follow mode (summons).
    fn npc_behavior_move(&mut self, i: usize) {
        let n = self.npcs[i].as_mut().unwrap();
        if n.is_killed || (n.is_summoned && n.summon_control_mode == 1) || n.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] != 0 {
            return;
        }
        if matches!(n.action_limit, 2 | 3 | 5 | 8) {
            n.behavior = BEHAVIOR_STOP;
            n.behavior_turn_count = 0;
            return;
        }
        n.behavior_turn_count += 1;
        if n.behavior_turn_count > 5 {
            n.behavior_turn_count = 0;
            if (n.v.0 - n.x).abs() <= 2 && (n.v.1 - n.y).abs() <= 2 {
                self.next_waypoint_dest(i);
            }
            let n = self.npcs[i].as_mut().unwrap();
            n.v = (n.x, n.y);
        }
        if let Some((t, tt)) = self.npc_target_search(i) {
            if self.npcs[i].as_ref().unwrap().action_time >= 1000 || self.dice(1, 3) == 3 {
                let n = self.npcs[i].as_mut().unwrap();
                (n.behavior, n.behavior_turn_count, n.target, n.target_type) = (BEHAVIOR_ATTACK, 0, t, tt);
                return;
            }
        }
        if self.npcs[i].as_ref().unwrap().is_master && self.dice(1, 3) == 2 {
            return;
        }
        let n = self.npcs[i].as_ref().unwrap();
        let (m, x, y, mut d, turn, mut err) = (n.map_index, n.x, n.y, n.d, n.turn, n.tmp_error);
        let follow = n.move_type == MOVETYPE_FOLLOW;
        if follow {
            let (fo, ft) = (n.follow_owner, n.follow_owner_type);
            let to = if ft == OWNERTYPE_NPC { self.npcs.get(fo).and_then(|f| f.as_ref()).map(|f| (f.x, f.y)) } else { self.client(fo).map(|c| (c.x, c.y)) };
            let Some(to) = to else {
                let n = self.npcs[i].as_mut().unwrap();
                (n.move_type, n.follow_owner) = (MOVETYPE_RANDOM, 0);
                return;
            };
            if (x - to.0).abs().max((y - to.1).abs()) < 3 {
                return;
            }
            d = to;
        }
        let dir = self.next_move_dir(x, y, d.0, d.1, m, turn, &mut err);
        self.npcs[i].as_mut().unwrap().tmp_error = err;
        if dir == 0 {
            if !follow && self.dice(1, 10) == 3 {
                self.next_waypoint_dest(i);
            }
            return;
        }
        let (nx, ny) = (x + DIR_X[dir as usize], y + DIR_Y[dir as usize]);
        self.maps[m].clear_owner(i as i16, OWNERTYPE_NPC, x, y);
        self.maps[m].set_owner(i as i16, OWNERTYPE_NPC, nx, ny);
        let n = self.npcs[i].as_mut().unwrap();
        (n.x, n.y, n.dir) = (nx, ny, dir);
        self.send_event_near_npc(i, MSGID_EVENT_MOTION, OBJECTMOVE as u16);
    }

    /// `CNpc::nextWaypointDest`
    fn next_waypoint_dest(&mut self, i: usize) {
        let n = self.npcs[i].as_ref().unwrap();
        let (m, mt, a) = (n.map_index, n.move_type, n.random_area);
        let (sx, sy) = (self.maps[m].size_x as i32, self.maps[m].size_y as i32);
        let d = match mt {
            MOVETYPE_RANDOMAREA => ((self.rng.rand() % ((a.2 - a.0) as i32).max(1)) as i16 + a.0, (self.rng.rand() % ((a.3 - a.1) as i32).max(1)) as i16 + a.1),
            MOVETYPE_RANDOM => {
                let mut d = None;
                for _ in 0..=30 {
                    let x = (self.rng.rand() % (sx - 50)) as i16 + 15;
                    let y = (self.rng.rand() % (sy - 50)) as i16 + 15;
                    if !self.map_npcs[m].avoid_rects.iter().any(|r| x >= r.0 && x <= r.2 && y >= r.1 && y <= r.3) {
                        d = Some((x, y));
                        break;
                    }
                }
                match d {
                    Some(d) => d,
                    None => return,
                }
            }
            MOVETYPE_SEQWAYPOINT | MOVETYPE_RANDOMWAYPOINT => {
                // waypoint walking: next waypoint in order / at random
                let n = self.npcs[i].as_ref().unwrap();
                let tw = n.total_waypoint.max(1);
                let cw = if mt == MOVETYPE_SEQWAYPOINT { (n.cur_waypoint + 1) % tw } else { self.rng.rand() as usize % tw };
                let w = self.npcs[i].as_ref().unwrap().waypoints[cw.min(9)];
                self.npcs[i].as_mut().unwrap().cur_waypoint = cw;
                self.maps[m].waypoints.get(w.max(0) as usize).copied().flatten().map_or((0, 0), |(x, y)| (x as i16, y as i16))
            }
            _ => return,
        };
        self.npcs[i].as_mut().unwrap().d = d;
    }

    /// `CGame::cGetNextMoveDir`
    pub fn next_move_dir(&self, sx: i16, sy: i16, dx: i16, dy: i16, m: usize, turn: u8, err: &mut i32) -> u8 {
        if (sx, sy) == (dx, dy) {
            return 0;
        }
        let (rx, ry) = if (sx - dx).abs() <= 1 && (sy - dy).abs() <= 1 { (dx, dy) } else { get_point(sx, sy, dx, dy, err) };
        let dir = misc_dir(sx, sy, rx, ry) as i32;
        for k in 0..8 {
            let mut t = if turn == 0 { dir + k } else { dir - k };
            if t > 8 {
                t -= 8;
            }
            if t < 1 {
                t += 8;
            }
            if self.maps[m].get_moveable(sx + DIR_X[t as usize], sy + DIR_Y[t as usize]).0 {
                return t as u8;
            }
        }
        0
    }

    /// SendEventToNearClient_TypeA for an NPC owner (the 27-byte object form; damage/attack forms come with combat).
    pub fn send_event_near_npc(&mut self, i: usize, msgid: u32, ty: u16) {
        self.send_event_near_npc_v(i, msgid, ty, 0, 0, 0)
    }

    /// The NPC branch of SendEventToNearClient_TypeA with the damage/dying (11/15 bytes, id + 40000) and
    /// attack (13 bytes) forms.
    pub fn send_event_near_npc_v(&mut self, i: usize, msgid: u32, ty: u16, v1: i16, v2: i16, v3: i16) {
        let key = (self.rng.rand() % 255) as u8 + 1;
        let Some(n) = self.npcs[i].as_ref() else { return };
        let mut b = vec![0u8; 27];
        let mut w = W(&mut b);
        w.u32(0, msgid);
        w.u16(4, ty);
        let len = match ty as i32 {
            OBJECTDAMAGE | OBJECTDAMAGEMOVE | OBJECTDYING => {
                w.u16(6, (i + 40000) as u16);
                w.u8(8, n.dir);
                w.u8(9, v1 as u8);
                w.u8(10, v2 as u8);
                if ty as i32 == OBJECTDYING {
                    w.u16(11, n.x as u16);
                    w.u16(13, n.y as u16);
                    15
                } else {
                    11
                }
            }
            OBJECTATTACK | OBJECTATTACKMOVE => {
                w.u16(6, (i + 40000) as u16);
                w.u8(8, n.dir);
                w.u8(9, (v1 - n.x) as u8);
                w.u8(10, (v2 - n.y) as u8);
                w.u16(11, v3 as u16);
                13
            }
            _ => {
                w.u16(6, (i + 10000) as u16);
                w.u16(8, n.x as u16);
                w.u16(10, n.y as u16);
                w.u16(12, n.type_ as u16);
                w.u8(14, n.dir);
                w.bytes(15, &npc_name5(&n.name));
                w.u16(20, n.appr2 as u16);
                w.u32(22, n.status as u32);
                w.u8(26, (ty as i32 == OBJECTNULLACTION && n.is_killed) as u8);
                27
            }
        };
        let (m, x, y) = (n.map_index, n.x, n.y);
        for h in self.handles() {
            let c = self.client(h).unwrap();
            if c.is_init_complete && c.map_index == m && c.x >= x - VIEW_NEAR_X && c.x <= x + VIEW_NEAR_X && c.y >= y - VIEW_NEAR_Y && c.y <= y + VIEW_NEAR_Y {
                self.send_key(h, &b[..len], key);
            }
        }
    }
}

pub fn npc_name5(n: &[u8]) -> [u8; 5] {
    let mut a = [0u8; 5];
    let s = cstr(n);
    let k = s.len().min(5);
    a[..k].copy_from_slice(&s[..k]);
    a
}

/// `CMisc::GetPoint`: one Bresenham step from (x0,y0) towards (x1,y1), carrying the error term.
pub fn get_point(x0: i16, y0: i16, x1: i16, y1: i16, err: &mut i32) -> (i16, i16) {
    if (x0, y0) == (x1, y1) {
        return (x0, y0);
    }
    let (mut dx, mut dy) = ((x1 - x0) as i32, (y1 - y0) as i32);
    let xi = if dx >= 0 { 1 } else { dx = -dx; -1 };
    let yi = if dy >= 0 { 1 } else { dy = -dy; -1 };
    let (mut rx, mut ry) = (x0 as i32, y0 as i32);
    let mut e = *err;
    if dx > dy {
        e += dy;
        if e > dx {
            e -= dx;
            ry += yi;
        }
        rx += xi;
    } else {
        e += dx;
        if e > 0 {
            e -= dy;
            rx += xi;
        }
        ry += yi;
    }
    *err = e;
    (rx as i16, ry as i16)
}

/// `CMisc::cGetNextMoveDir`
pub fn misc_dir(sx: i16, sy: i16, dx: i16, dy: i16) -> u8 {
    let (ax, ay) = (sx - dx, sy - dy);
    match (ax.signum(), ay.signum()) {
        (0, 1) => 1,
        (0, -1) => 5,
        (1, 0) => 7,
        (-1, 0) => 3,
        (1, 1) => 8,
        (-1, 1) => 2,
        (1, -1) => 6,
        (-1, -1) => 4,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_event_tables() {
        // sNpcType of the table kinds the events can pick: Demon, Unicorn, Skeleton, YB-Aresden / Elvine
        assert_eq!([16, 17, 5, 19, 20].map(random_mob_type), [31, 32, 11, 1, 1]);
        assert_eq!(random_mob_type(0), 14);
        assert!(is_town_map(b"aresden", false) && is_town_map(b"elvfarm", false) && is_town_map(b"elvined1", false));
        assert!(!is_town_map(b"middled1n", false) && is_town_map(b"middled1n", true) && !is_town_map(b"huntzone1", true));
    }

    #[test]
    fn directions_and_line() {
        assert_eq!((misc_dir(5, 5, 5, 1), misc_dir(5, 5, 9, 5), misc_dir(5, 5, 1, 9), misc_dir(5, 5, 5, 5)), (1, 3, 6, 0));
        let mut e = 0;
        let mut p = (0, 0);
        let mut path = Vec::new();
        for _ in 0..4 {
            p = get_point(p.0, p.1, 4, 2, &mut e);
            path.push(p);
        }
        assert_eq!(path.last(), Some(&(4, 2)));
        assert!(path.windows(2).all(|w| (w[1].0 - w[0].0).abs() <= 1 && (w[1].1 - w[0].1).abs() <= 1));
    }
}

/// The level tables of the random mob generator: (mob kind, bFirmBerserk, iFirmSAType). The dice order is the C++ one.
pub fn random_mob_pick(rng: &mut crate::rng::Rng, level: i32) -> (i32, bool, u8) {
    let mut d = |n: i32| rng.dice(1, n);
    let r = d(100);
    fn pick(d: &mut dyn FnMut(i32) -> i32, t: &[i32]) -> i32 {
        t[d(t.len() as i32) as usize - 1]
    }
    // fix: many tables end with `iResult < 100`, so a roll of 100 kept the value 100 and spawned the default Orc;
    // the last range takes it here
    let (mut fb, mut sa) = (false, 0u8);
    let res = match level {
        1 => match r { ..=19 => 1, ..=39 => 2, ..=84 => 24, ..=94 => 25, _ => 3 },
        2 => match r { ..=39 => 1, ..=79 => 2, _ => 10 },
        3 => match r {
            ..=19 => pick(&mut d, &[3, 4]),
            ..=24 => 30,
            ..=49 => pick(&mut d, &[5, 6, 7]),
            ..=74 => pick(&mut d, &[8, 8, 11, 12, 18, 26, 28]),
            _ => pick(&mut d, &[9, 9, 13, 14, 27]),
        },
        4 => match r { ..=49 => pick(&mut d, &[2, 10]), ..=79 => pick(&mut d, &[8, 11]), _ => pick(&mut d, &[14, 9]) },
        5 | 11 => {
            let t: [&[i32]; 5] = if level == 5 { [&[2; 5], &[3, 4], &[5, 7], &[8, 8, 11], &[11, 14, 9]] } else { [&[2; 5], &[3, 4], &[5, 7], &[10, 10, 11], &[11, 7, 8]] };
            match r { ..=29 => pick(&mut d, t[0]), ..=59 => pick(&mut d, t[1]), ..=79 => pick(&mut d, t[2]), ..=94 => pick(&mut d, t[3]), _ => pick(&mut d, t[4]) }
        }
        6 => match r { ..=59 => pick(&mut d, &[5, 6, 7, 4]), ..=89 => pick(&mut d, &[8, 8, 11, 12]), _ => pick(&mut d, &[26, 9, 13, 14, 18, 28, 27]) },
        7 => match r {
            ..=49 => pick(&mut d, &[3, 6, 10, 3, 50]),
            ..=59 => 30,
            ..=84 => pick(&mut d, &[50, 6, 6, 12]),
            _ => {
                if d(3) == 1 { 12 } else if d(100) < 3 { 17 } else { 12 }
            }
        },
        8 => match r { ..=69 => pick(&mut d, &[4, 5]), ..=89 => pick(&mut d, &[8, 11, 14]), _ => pick(&mut d, &[9, 14]) },
        9 | 10 => {
            let res = match (level, r) {
                (_, ..=69) => pick(&mut d, &[4, 5]),
                (9, ..=94) => pick(&mut d, &[8, 9, 13]),
                (_, ..=94) => pick(&mut d, &[13, 13, 14]),
                (9, _) => pick(&mut d, &[9, 9, 9, 14, 14, 15]),
                _ => pick(&mut d, &[14, 14, 15]),
            };
            if d(3) == 1 && res != 16 {
                fb = true;
            }
            res
        }
        12 | 16 => match r {
            ..=39 if level == 16 => pick(&mut d, &[1, 2, 10]),
            ..=49 if level == 16 => 30,
            ..=49 => pick(&mut d, &[1, 2, 10]),
            ..=84 => pick(&mut d, &[5, 4]),
            _ => pick(&mut d, &[8, 11, 26]),
        },
        13 => match r {
            ..=74 => {
                fb = true;
                sa = 4 - (d(2) - 1) as u8;
                match r { ..=14 => 4, ..=39 => 14, ..=59 => 9, _ => 13 }
            }
            ..=94 => 23,
            _ => 22,
        },
        14 => match r {
            ..=29 => 23,
            ..=49 => 22,
            ..=69 => {
                fb = true;
                sa = 4 - (d(2) - 1) as u8;
                15
            }
            ..=89 => 16,
            _ => 21,
        },
        15 => match r {
            ..=79 => {
                fb = true;
                match r { ..=34 => 23, ..=49 => 22, _ => 16 }
            }
            _ => 21,
        },
        17 => match r {
            ..=29 => pick(&mut d, &[22, 8, 24, 5]),
            ..=39 => 30,
            ..=69 => 32,
            ..=89 => {
                if d(5) == 1 {
                    fb = true;
                }
                31
            }
            _ => 33,
        },
        18 => match r { ..=2 => 35, ..=12 => 48, ..=50 => 44, ..=85 => 45, ..=95 => 34, _ => 26 },
        19 => match r { ..=15 => 40, ..=25 => 42, ..=35 => 21, ..=60 => 43, ..=89 => 23, _ => 22 },
        20 => match r { 1 => 37, ..=3 => 36, ..=8 => 49, 9 => 38, ..=20 => 21, ..=35 => 16, ..=45 => 40, ..=55 => 41, ..=75 => 28, ..=95 => 43, _ => 22 },
        21 => match r {
            ..=93 => {
                fb = true;
                17
            }
            94 => 36,
            95 => 37,
            96 => 47,
            97 => 35,
            98 => 49,
            _ => 51,
        },
        _ => r,
    };
    (res, fb, sa)
}

/// MobGenerator's random table: kind -> (NPC config name, special-ability probability, ability kind).
/// The maps the type 2 event names: aresden, elvine, arefarm, elvfarm (prefixes) and, for the demon's roll, middled1n
fn is_town_map(name: &[u8], with_middle: bool) -> bool {
    [&b"aresden"[..], b"arefarm", b"elvfarm", b"elvine"].iter().any(|p| name.starts_with(p)) || (with_middle && name.starts_with(b"middled1n"))
}

/// sNpcType of the random generator's kinds (the number the special event tells the players)
fn random_mob_type(r: i32) -> i32 {
    match r {
        1 => 10, 2 => 16, 3 => 14, 4 => 18, 5 => 11, 6 => 14, 7 => 17, 8 => 12, 9 => 13, 10 => 22, 11 => 23, 12 => 28,
        13 => 29, 14 => 27, 15 => 30, 16 => 31, 17 => 32, 18 => 33, 19 | 20 => 1, 21 => 52, 22 => 53, 23 => 54,
        24 => 55, 25 => 56, 26 => 57, 27 => 58, 28 => 59, 29 => 60, 30 => 61, 31 => 65, 32 => 62, 33 => 63, 34 => 48,
        35 => 49, 36 => 66, 37 => 73, 38 => 70, 39 | 43 => 80, 40 => 71, 41 => 75, 42 => 78, 44 => 72, 45 => 74,
        46 | 50 => 76, 47 => 77, 48 => 79, 49 => 50, 51 => 81,
        _ => 14,
    }
}

fn random_mob(r: i32) -> (&'static str, i32, i32) {
    match r {
        1 => ("Slime", 5, 1),
        2 => ("Giant-Ant", 10, 2),
        3 => ("Orc", 15, 1),
        4 => ("Zombie", 15, 3),
        5 => ("Skeleton", 35, 8),
        6 => ("Orc-Mage", 30, 7),
        7 => ("Scorpion", 15, 3),
        8 => ("Stone-Golem", 25, 5),
        9 => ("Cyclops", 35, 8),
        10 => ("Amphis", 20, 3),
        11 => ("Clay-Golem", 20, 5),
        12 => ("Troll", 25, 3),
        13 => ("Orge", 25, 1),
        14 => ("Hellbound", 25, 8),
        15 => ("Liche", 30, 8),
        16 => ("Demon", 20, 8),
        17 => ("Unicorn", 35, 7),
        18 => ("WereWolf", 25, 1),
        19 => ("YB-Aresden", 15, 1),
        20 => ("YB-Elvine", 15, 1),
        21 => ("Gagoyle", 20, 8),
        22 => ("Beholder", 20, 5),
        23 => ("Dark-Elf", 20, 3),
        24 => ("Rabbit", 5, 1),
        25 => ("Cat", 10, 2),
        26 => ("Giant-Frog", 10, 2),
        27 => ("Mountain-Giant", 25, 1),
        28 => ("Ettin", 20, 8),
        29 => ("Cannibal-Plant", 20, 5),
        30 => ("Rudolph", 20, 5),
        31 => ("Ice-Golem", 35, 8),
        32 => ("DireBoar", 20, 5),
        33 => ("Frost", 30, 8),
        34 => ("Stalker", 20, 1),
        35 => ("Hellclaw", 20, 1),
        36 => ("Wyvern", 20, 10),
        37 => ("Fire-Wyvern", 20, 10),
        38 => ("Barlog", 20, 1),
        39 | 43 => ("Tentocle", 20, 1),
        40 => ("Centaurus", 20, 1),
        41 => ("Giant-Lizard", 20, 1),
        42 => ("Minotaurs", 20, 1),
        44 => ("Claw-Turtle", 20, 1),
        45 => ("Giant-Crayfish", 20, 1),
        46 | 50 => ("Giant-Plant", 20, 1),
        47 => ("MasterMage-Orc", 20, 1),
        48 => ("Nizie", 20, 1),
        49 => ("Tigerworm", 20, 1),
        51 => ("Abaddon", 20, 10),
        _ => ("Orc", 15, 1),
    }
}

#[cfg(test)]
mod random_mob_tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn aresden_level_1_gives_rabbits_and_cats() {
        let mut rng = Rng::new(1);
        let mut seen = std::collections::BTreeMap::new();
        for _ in 0..10000 {
            let (r, fb, sa) = random_mob_pick(&mut rng, 1);
            assert!(!fb && sa == 0);
            *seen.entry(r).or_insert(0) += 1;
        }
        assert_eq!(seen.keys().copied().collect::<Vec<_>>(), vec![1, 2, 3, 24, 25]);
        // 45 % rabbits, 10 % cats, 19 % slimes
        assert!((4200..4800).contains(&seen[&24]) && (800..1200).contains(&seen[&25]) && (1600..2200).contains(&seen[&1]));
        assert_eq!(random_mob(24).0, "Rabbit");
    }

    #[test]
    fn every_level_maps_to_a_named_kind() {
        let mut rng = Rng::new(2);
        for level in 1..=21 {
            for _ in 0..3000 {
                let (r, fb, sa) = random_mob_pick(&mut rng, level);
                assert!(r != 100 && (1..=51).contains(&r), "level {level} gave {r}");
                assert!(sa == 0 || (fb && (sa == 3 || sa == 4)));
            }
        }
        // the C++ default for other levels: the raw roll
        let (r, _, _) = random_mob_pick(&mut rng, 0);
        assert!((1..=100).contains(&r));
    }
}
