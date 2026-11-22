//! HG/map: `CMap` + `CTile` + `CTeleportLoc`, the `.amd` tile file (`_bDecodeMapDataFileContents`) and the per-map
//! info file (`CGame::_bReadMapInfoFiles`). Info records that later layers act on (NPC spawns, mob generators,
//! fish/mineral points, crusade/heldenian data...) are kept as raw field lists in `Map::info` for now.
use crate::consts::MAXSECTORS;
use crate::util::{atoi, cstr, StrTok};
use std::path::{Path, PathBuf};

pub const OWNERTYPE_PLAYER: u8 = 1;
pub const OWNERTYPE_NPC: u8 = 2;
pub const MAXTELEPORTLOC: usize = 200;
pub const MAXINITIALPOINT: usize = 20;
pub const MAXNMR: usize = 50;
pub const MAXWAYPOINTCFG: usize = 200;

#[derive(Clone, Default)]
pub struct Tile {
    pub owner: i16,
    pub owner_class: u8,
    pub dead_owner: i16,
    pub dead_owner_class: u8,
    pub attribute: u8,
    pub move_allowed: bool,
    pub temp_move_allowed: bool,
    pub teleport: bool,
    pub farm: bool,
    pub water: bool,
    /// Top ground item drawn by the client: (sprite, sprite frame, color). Items themselves come with layer 3.
    pub top_item: Option<(i16, i16, u8)>,
    pub dynamic_object_id: u16,
    pub dynamic_object_type: i16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TeleportLoc {
    pub src: (i16, i16),
    pub dest_map: Vec<u8>,
    pub dest: (i16, i16),
    pub dir: u8,
}

/// Per-map info keywords in the order `_bReadMapInfoFiles` tests them (later matches win, as there) and the
/// number of field tokens each record takes. `item-event` is variable (ends at "EOL" or after 5 mobs).
const KEYS: &[(&[u8], usize)] = &[
    (b"teleport-loc", 6),
    (b"waypoint", 3),
    (b"npc", 13),
    (b"random-mob-generator", 2),
    (b"maximum-object", 1),
    (b"npc-avoidrect", 5),
    (b"spot-mob-generator", 14),
    (b"map-location", 1),
    (b"initial-point", 3),
    (b"no-attack-area", 5),
    (b"fixed-dayornight-mode", 1),
    (b"fish-point", 3),
    (b"max-fish", 1),
    (b"type", 1),
    (b"level-limit", 1),
    (b"mineral-generator", 2),
    (b"mineral-point", 3),
    (b"max-mineral", 1),
    (b"upper-level-limit", 1),
    (b"strategic-point", 5),
    (b"energy-sphere-creation-point", 4),
    (b"energy-sphere-goal-point", 6),
    (b"strike-point", 15),
    (b"item-event", 0),
    (b"energy-sphere-auto-creation", 0),
    // fix: the C++ never left this mode (cReadModeB = 0 only), so every later token was eaten and the read failed
    (b"mobevent-amount", 1),
    (b"ApocalypseMobGenType", 1),
    (b"ApocalypseBossMob", 5),
    (b"DynamicGateType", 1),
    (b"DynamicGateCoord", 7),
    (b"ApocalypseMap", 1),
    (b"HeldenianMap", 1),
    (b"HeldenianTower", 4),
    (b"HeldenianModeMap", 1),
    (b"HeldenianWinningZone", 2),
    (b"HeldenianGateDoor", 3),
];

pub struct Map {
    pub name: Vec<u8>,
    pub location_name: Vec<u8>,
    pub size_x: i16,
    pub size_y: i16,
    pub tile_data_size: i16,
    pub tiles: Vec<Tile>,
    pub teleport_locs: Vec<TeleportLoc>,
    pub initial_points: Vec<Option<(i16, i16)>>,
    pub no_attack_rects: Vec<(i32, i32, i32, i32)>,
    pub waypoints: Vec<Option<(i32, i32)>>,
    pub map_type: i32,
    pub level_limit: i32,
    pub upper_level_limit: i32,
    pub maximum_object: i32,
    pub random_mob_generator: bool,
    pub random_mob_level: i32,
    /// sMobEventAmount (mobevent-amount, 15 by default): the monsters of a mob event
    pub mob_event_amount: i32,
    /// m_stTempSectorInfo[x][y].iPlayerActivity (a 20 x 20 tile sector per entry) and m_iMaxPx / m_iMaxPy, the busiest
    /// sector of the last 10 s (the other four activity kinds and the aged m_stSectorInfo are only written, never read)
    pub player_activity: Vec<u16>,
    pub max_px: i32,
    pub max_py: i32,
    /// fish-point / mineral-point lists by index ((-1, -1) = unused), their counts, max-fish / max-mineral and
    /// the live counts (iCreateFish / iCreateMineral); mineral-generator (on, level)
    pub fish_points: Vec<(i16, i16)>,
    pub total_fish_points: i32,
    pub max_fish: i32,
    pub cur_fish: i32,
    pub mineral_points: Vec<(i16, i16)>,
    pub total_mineral_points: i32,
    pub max_mineral: i32,
    pub cur_mineral: i32,
    pub mineral_generator: bool,
    pub mineral_level: i32,
    pub is_fixed_day_mode: bool,
    pub is_fixed_snow_mode: bool,
    pub is_fight_zone: bool,
    pub is_energy_sphere_auto_creation: bool,
    /// energy-sphere-creation-point (type, x, y) and -goal-point (result, aresden x y, elvine x y) by index, their
    /// counts, and m_iCurEnergySphereGoalPointIndex (-1: no sphere)
    pub es_create: [(i32, i32, i32); crate::consts::MAXENERGYSPHERES as usize],
    pub es_goal: [(i32, i32, i32, i32, i32); crate::consts::MAXENERGYSPHERES as usize],
    pub es_total_create: i32,
    pub es_total_goal: i32,
    pub es_cur_goal: i32,
    /// m_cWhetherStatus (weather), driven by WhetherProcessor.
    pub weather: u8,
    pub weather_start: u32,
    pub weather_duration: u32,
    pub is_party_disabled: bool,
    pub is_chat_disabled: bool,
    /// the GM event switches of a fight zone (/setattackmode, /eventspell, /eventarmor, /eventshield, /eventillusion)
    pub is_attack_enabled: bool,
    pub magic_limited: [bool; 100],
    pub is_armor_disabled: bool,
    pub is_shield_disabled: bool,
    pub is_perm_illusion_on: bool,
    /// Every info record by keyword (fields after the keyword), for the layers that use them.
    pub info: Vec<(&'static [u8], Vec<Vec<u8>>)>,
    naming: [bool; 1000],
}

/// `fopen("mapdata\\name.ext")` on Windows: exact name first, then a case-insensitive match.
pub fn find_file(dir: &Path, file: &str) -> Option<PathBuf> {
    let p = dir.join(file);
    if p.exists() {
        return Some(p);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|e| e.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(file)))
}

fn num(t: &[u8]) -> Result<i32, String> {
    if crate::gamedata::is_number(t) {
        Ok(atoi(t))
    } else {
        Err(format!("not a number: {}", crate::util::l1(t)))
    }
}

impl Map {
    pub fn new(name: &[u8], size_x: i16, size_y: i16) -> Map {
        let n = (size_x.max(0) as usize) * (size_y.max(0) as usize);
        let t = Tile { move_allowed: true, temp_move_allowed: true, ..Default::default() };
        let mut m = Map {
            name: name.to_vec(),
            location_name: Vec::new(),
            size_x,
            size_y,
            tile_data_size: 0,
            tiles: vec![t; n],
            teleport_locs: Vec::new(),
            initial_points: vec![None; MAXINITIALPOINT],
            no_attack_rects: Vec::new(),
            waypoints: vec![None; MAXWAYPOINTCFG],
            map_type: 0,
            level_limit: 0,
            upper_level_limit: 0,
            maximum_object: 0,
            random_mob_generator: false,
            random_mob_level: 0,
            mob_event_amount: 15,
            player_activity: vec![0; (MAXSECTORS * MAXSECTORS) as usize],
            max_px: 0,
            max_py: 0,
            fish_points: vec![(-1, -1); 200],
            total_fish_points: 0,
            max_fish: 0,
            cur_fish: 0,
            mineral_points: vec![(-1, -1); 200],
            total_mineral_points: 0,
            max_mineral: 0,
            cur_mineral: 0,
            mineral_generator: false,
            mineral_level: 0,
            is_fixed_day_mode: false,
            is_fixed_snow_mode: false,
            is_fight_zone: false,
            is_energy_sphere_auto_creation: false,
            es_create: Default::default(),
            es_goal: Default::default(),
            es_total_create: 0,
            es_total_goal: 0,
            es_cur_goal: -1,
            weather: 0,
            weather_start: 0,
            weather_duration: 0,
            is_party_disabled: false,
            is_chat_disabled: false,
            is_attack_enabled: true,
            magic_limited: [false; 100],
            is_armor_disabled: false,
            is_shield_disabled: false,
            is_perm_illusion_on: false,
            info: Vec::new(),
            naming: [false; 1000],
        };
        // the name-based flags at the top of _bReadMapInfoFiles
        m.is_fight_zone = name.starts_with(b"fight");
        m.is_fixed_snow_mode = name.starts_with(b"icebound");
        m
    }

    /// `bInit` / `_bDecodeMapDataFileContents`: `dir` is the server's MAPDATA folder.
    pub fn load(dir: &Path, name: &str) -> Result<Map, String> {
        let path = find_file(dir, &format!("{name}.amd")).ok_or(format!("{name}.amd not found"))?;
        let raw = std::fs::read(&path).map_err(|e| e.to_string())?;
        Self::from_amd(name.as_bytes(), &raw)
    }

    pub fn from_amd(name: &[u8], raw: &[u8]) -> Result<Map, String> {
        let mut header: Vec<u8> = raw.iter().take(256).map(|&c| if c == 0 { b' ' } else { c }).collect();
        header.resize(256, b' ');
        let (mut sx, mut sy, mut ts) = (0, 0, 0);
        let mut t = StrTok::new(&header, b"= \t\n");
        let mut mode = 0;
        while let Some(tok) = t.get() {
            match mode {
                1 => sx = atoi(&tok),
                2 => sy = atoi(&tok),
                3 => ts = atoi(&tok),
                _ => {}
            }
            mode = if tok.starts_with(b"MAPSIZEX") {
                1
            } else if tok.starts_with(b"MAPSIZEY") {
                2
            } else if tok.starts_with(b"TILESIZE") {
                3
            } else {
                0
            };
        }
        // fix: the C++ trusted the header and read tile records past the end of the file
        if sx <= 0 || sy <= 0 || ts < 10 || raw.len() < 256 + (sx * sy * ts) as usize {
            return Err(format!("bad map header {sx}x{sy} tile {ts}, {} bytes", raw.len()));
        }
        let mut m = Map::new(name, sx as i16, sy as i16);
        m.tile_data_size = ts as i16;
        for iy in 0..sy {
            for ix in 0..sx {
                let o = 256 + ((iy * sx + ix) * ts) as usize;
                let a = raw[o + 8];
                let i = m.idx(ix as i16, iy as i16).unwrap();
                let tile = &mut m.tiles[i];
                tile.attribute = a;
                tile.move_allowed = a & 0x80 == 0;
                tile.teleport = a & 0x40 != 0;
                tile.farm = a & 0x20 != 0;
                // owner's decision (2026-11-20): the town lake tile (367), where Aresden's and Elvine's fish points
                // are, is water too; the C++ only counted the sea (19), so nobody could fish in the towns
                tile.water = matches!(i16::from_le_bytes([raw[o], raw[o + 1]]), 19 | 367);
            }
        }
        Ok(m)
    }

    // fix: the C++ indexed tiles with x + y*m_sSizeY (wrong for non-square maps)
    pub fn idx(&self, x: i16, y: i16) -> Option<usize> {
        self.valid(x, y).then(|| x as usize + y as usize * self.size_x as usize)
    }

    /// `bIsValidLoc`
    pub fn valid(&self, x: i16, y: i16) -> bool {
        x >= 0 && x < self.size_x && y >= 0 && y < self.size_y
    }

    pub fn tile(&self, x: i16, y: i16) -> Option<&Tile> {
        self.idx(x, y).map(|i| &self.tiles[i])
    }
    pub fn tile_mut(&mut self, x: i16, y: i16) -> Option<&mut Tile> {
        self.idx(x, y).map(|i| &mut self.tiles[i])
    }

    pub fn set_owner(&mut self, owner: i16, class: u8, x: i16, y: i16) {
        if let Some(t) = self.tile_mut(x, y) {
            t.owner = owner;
            t.owner_class = class;
        }
    }
    pub fn set_dead_owner(&mut self, owner: i16, class: u8, x: i16, y: i16) {
        if let Some(t) = self.tile_mut(x, y) {
            t.dead_owner = owner;
            t.dead_owner_class = class;
        }
    }
    /// (owner, class); (0, 0) outside the map.
    pub fn get_owner(&self, x: i16, y: i16) -> (i16, u8) {
        self.tile(x, y).map_or((0, 0), |t| (t.owner, t.owner_class))
    }
    pub fn get_dead_owner(&self, x: i16, y: i16) -> (i16, u8) {
        self.tile(x, y).map_or((0, 0), |t| (t.dead_owner, t.dead_owner_class))
    }
    /// `ClearOwner`: clears the live and the dead slot when they hold this owner.
    pub fn clear_owner(&mut self, owner: i16, class: u8, x: i16, y: i16) {
        if let Some(t) = self.tile_mut(x, y) {
            if t.owner == owner && t.owner_class == class {
                t.owner = 0;
                t.owner_class = 0;
            }
            if t.dead_owner == owner && t.dead_owner_class == class {
                t.dead_owner = 0;
                t.dead_owner_class = 0;
            }
        }
    }
    pub fn clear_dead_owner(&mut self, x: i16, y: i16) {
        if let Some(t) = self.tile_mut(x, y) {
            t.dead_owner = 0;
            t.dead_owner_class = 0;
        }
    }

    fn inside_border(&self, x: i16, y: i16) -> bool {
        x >= 20 && x < self.size_x - 20 && y >= 20 && y < self.size_y - 20
    }

    /// `bGetMoveable`: inside the 20-tile border, no owner, walkable. Also returns the dynamic object type.
    pub fn get_moveable(&self, x: i16, y: i16) -> (bool, i16) {
        if !self.inside_border(x, y) {
            return (false, 0);
        }
        let t = self.tile(x, y).unwrap();
        (t.owner == 0 && t.move_allowed && t.temp_move_allowed, t.dynamic_object_type)
    }

    /// `bGetIsMoveAllowedTile`
    pub fn is_move_allowed(&self, x: i16, y: i16) -> bool {
        self.inside_border(x, y) && self.tile(x, y).is_some_and(|t| t.move_allowed && t.temp_move_allowed)
    }

    /// `bGetIsTeleport`
    pub fn is_teleport(&self, x: i16, y: i16) -> bool {
        if x < 14 || x >= self.size_x - 16 || y < 12 || y >= self.size_y - 14 {
            return false;
        }
        self.tile(x, y).is_some_and(|t| t.teleport)
    }
    pub fn is_water(&self, x: i16, y: i16) -> bool {
        self.tile(x, y).is_some_and(|t| t.water)
    }
    /// `bGetIsFarm`
    pub fn is_farm(&self, x: i16, y: i16) -> bool {
        if x < 14 || x >= self.size_x - 16 || y < 12 || y >= self.size_y - 14 {
            return false;
        }
        self.tile(x, y).is_some_and(|t| t.farm)
    }
    /// `iGetAttribute`
    pub fn attribute(&self, x: i16, y: i16, mask: u8) -> u8 {
        self.tile(x, y).map_or(0, |t| t.attribute & mask)
    }
    pub fn set_temp_move_allowed(&mut self, x: i16, y: i16, f: bool) {
        if let Some(t) = self.tile_mut(x, y) {
            t.temp_move_allowed = f;
        }
    }

    /// `bSearchTeleportDest`
    pub fn search_teleport_dest(&self, x: i16, y: i16) -> Option<&TeleportLoc> {
        self.teleport_locs.iter().find(|t| t.src == (x, y))
    }

    /// `iGetEmptyNamingValue` (0..999)
    pub fn get_empty_naming_value(&mut self) -> Option<usize> {
        let i = (0..1000).find(|&i| !self.naming[i])?;
        self.naming[i] = true;
        Some(i)
    }
    pub fn set_naming_value_empty(&mut self, v: usize) {
        if let Some(n) = self.naming.get_mut(v) {
            *n = false;
        }
    }

    /// `_bReadMapInfoFiles` minus the game calls; `text` is the file through `util::text_mode`.
    pub fn read_info(&mut self, text: &[u8]) -> Result<(), String> {
        let mut t = StrTok::new(cstr(text), b"= \t\n");
        let mut cur: Option<(&'static [u8], usize, Vec<Vec<u8>>)> = None;
        while let Some(tok) = t.get() {
            if let Some((k, n, f)) = cur.as_mut() {
                if *k == b"item-event" {
                    // index name amount total month day type, then mobs up to "EOL"; a 6th mob token ends it too
                    if f.len() >= 7 && (tok == b"EOL" || f.len() >= 12) {
                        let r = cur.take().unwrap();
                        self.info.push((r.0, r.2));
                    } else {
                        f.push(tok);
                    }
                    continue;
                } else {
                    f.push(tok);
                    if *k == b"spot-mob-generator" && f.len() == 2 {
                        // type 1: rect + mob type + max mobs; type 2: 10 waypoints + mob type + max mobs
                        *n = match atoi(&f[1]) {
                            1 => 8,
                            2 => 14,
                            // fix: the C++ stayed on the type field and read the next tokens as types
                            _ => return Err("spot-mob-generator type".into()),
                        };
                    }
                    if f.len() == *n {
                        let (k, _, f) = cur.take().unwrap();
                        self.apply(k, &f)?;
                        self.info.push((k, f));
                    }
                    continue;
                }
            }
            if tok.starts_with(b"[END-MAP-INFO]") {
                break;
            }
            if let Some(&(k, n)) = KEYS.iter().rev().find(|(k, _)| tok.starts_with(k)) {
                if k == b"energy-sphere-auto-creation" {
                    self.is_energy_sphere_auto_creation = true;
                } else {
                    cur = Some((k, n, Vec::new()));
                }
            }
        }
        if let Some((k, _, f)) = cur {
            if k == b"item-event" && f.len() >= 7 {
                self.info.push((k, f));
            } else {
                return Err(format!("map info ends inside '{}'", crate::util::l1(k)));
            }
        }
        self.setup_no_attack_area();
        Ok(())
    }

    /// `CMap::_SetupNoAttackArea`: tiles of the no-attack rects (the whole map for top -10) get attribute 0x04.
    fn setup_no_attack_area(&mut self) {
        for (l, t, r, b) in self.no_attack_rects.clone() {
            let (l, t, r, b) = if t == -10 { (0, 0, self.size_x as i32 - 1, self.size_y as i32 - 1) } else if t > 0 { (l, t, r, b) } else { continue };
            for x in l..=r {
                for y in t..=b {
                    if let Some(tile) = self.tile_mut(x as i16, y as i16) {
                        tile.attribute |= 0x04;
                    }
                }
            }
        }
    }

    /// The records layer 1/2 use, typed and checked as in the C++.
    fn apply(&mut self, k: &[u8], f: &[Vec<u8>]) -> Result<(), String> {
        match k {
            b"teleport-loc" => {
                // fix: the C++ allocated m_pTeleportLoc[index] past MAXTELEPORTLOC
                if self.teleport_locs.len() < MAXTELEPORTLOC {
                    let mut dest_map = f[2].clone();
                    dest_map.truncate(10);
                    self.teleport_locs.push(TeleportLoc {
                        src: (num(&f[0])? as i16, num(&f[1])? as i16),
                        dest_map,
                        dest: (num(&f[3])? as i16, num(&f[4])? as i16),
                        dir: num(&f[5])? as u8,
                    });
                }
            }
            b"waypoint" => {
                let i = num(&f[0])?;
                // fix: unchecked m_WaypointList[atoi(token)] in the C++
                let w = self.waypoints.get_mut(i as usize).ok_or("waypoint index")?;
                if w.is_some() {
                    return Err("duplicated waypoint".into());
                }
                *w = Some((num(&f[1])?, num(&f[2])?));
            }
            b"initial-point" => {
                let i = num(&f[0])?;
                // fix: unchecked m_pInitialPoint[atoi(token)] in the C++
                if let Some(p) = self.initial_points.get_mut(i as usize) {
                    *p = Some((num(&f[1])? as i16, num(&f[2])? as i16));
                }
            }
            b"no-attack-area" => {
                if self.no_attack_rects.len() < MAXNMR {
                    self.no_attack_rects.push((num(&f[1])?, num(&f[2])?, num(&f[3])?, num(&f[4])?));
                }
            }
            b"map-location" => self.location_name = f[0][..f[0].len().min(10)].to_vec(),
            b"fixed-dayornight-mode" => {
                self.is_fixed_day_mode = atoi(&f[0]) != 0;
                if self.is_fixed_day_mode {
                    self.is_fixed_snow_mode = false;
                }
            }
            b"type" => self.map_type = atoi(&f[0]),
            b"level-limit" => self.level_limit = atoi(&f[0]),
            b"upper-level-limit" => self.upper_level_limit = atoi(&f[0]),
            b"maximum-object" => self.maximum_object = num(&f[0])?,
            k @ (b"fish-point" | b"mineral-point") => {
                let fish = k == b"fish-point";
                let list = if fish { &mut self.fish_points } else { &mut self.mineral_points };
                // fix: unchecked m_FishPointList / m_MineralPointList[atoi(token)] in the C++
                let p = list.get_mut(num(&f[0])? as usize).ok_or("fish/mineral point index")?;
                if p.0 != -1 {
                    return Err("duplicate fish/mineral point".into());
                }
                *p = (num(&f[1])? as i16, num(&f[2])? as i16);
                *(if fish { &mut self.total_fish_points } else { &mut self.total_mineral_points }) += 1;
            }
            b"mobevent-amount" => self.mob_event_amount = num(&f[0])? as i32,
            b"max-fish" => self.max_fish = atoi(&f[0]),
            b"max-mineral" => self.max_mineral = atoi(&f[0]),
            b"mineral-generator" => {
                self.mineral_generator = atoi(&f[0]) != 0;
                self.mineral_level = atoi(&f[1]);
            }
            b"random-mob-generator" => {
                self.random_mob_generator = atoi(&f[0]) != 0;
                self.random_mob_level = atoi(&f[1]);
            }
            b"energy-sphere-creation-point" => {
                // fix: unchecked m_stEnergySphereCreationList[atoi(token)] in the C++
                let p = self.es_create.get_mut(num(&f[0])? as usize).ok_or("energy-sphere-creation-point index")?;
                if p.0 != 0 {
                    return Err("Duplicate EnergySphereCreation number".into());
                }
                *p = (num(&f[1])?, num(&f[2])?, num(&f[3])?);
                self.es_total_create += 1;
            }
            b"energy-sphere-goal-point" => {
                let p = self.es_goal.get_mut(num(&f[0])? as usize).ok_or("energy-sphere-goal-point index")?;
                if p.0 != 0 {
                    return Err("Duplicate EnergySphereGoal number".into());
                }
                *p = (num(&f[1])?, num(&f[2])?, num(&f[3])?, num(&f[4])?, num(&f[5])?);
                self.es_total_goal += 1;
            }
            _ => {}
        }
        Ok(())
    }

    /// Raw records for one keyword.
    /// `CMap::IncPlayerActivity`
    pub fn inc_player_activity(&mut self, x: i16, y: i16) {
        // fix: the C++ indexed the 60 x 60 sectors unchecked
        let (sx, sy) = (x as i32 / 20, y as i32 / 20);
        if (0..MAXSECTORS).contains(&sx) && (0..MAXSECTORS).contains(&sy) {
            self.player_activity[(sx * MAXSECTORS + sy) as usize] += 1;
        }
    }

    /// The player part of `UpdateMapSectorInfo`: the busiest sector (the first one in x, y order on a tie, none = 0, 0)
    /// of the last period, then the counts start again.
    pub fn update_sector_info(&mut self) {
        let (mut best, mut at) = (0, 0);
        for (i, &a) in self.player_activity.iter().enumerate() {
            if a > best {
                (best, at) = (a, i as i32);
            }
        }
        (self.max_px, self.max_py) = (at / MAXSECTORS, at % MAXSECTORS);
        self.player_activity.fill(0);
    }

    pub fn records<'a>(&'a self, key: &'a [u8]) -> impl Iterator<Item = &'a Vec<Vec<u8>>> + 'a {
        self.info.iter().filter(move |(k, _)| *k == key).map(|(_, f)| f)
    }
}

/// `_tmp_iMoveLocX/Y` (char/movement.h): the strip of tiles (relative to the 21x16 view origin) that comes into
/// view after a step in direction `dir` (1..8), in the table order.
pub fn move_strip(dir: u8) -> Vec<(i16, i16)> {
    let r = crate::consts::VIEW_W - 1; // right-most column of the window
    let row = |y: i16| (0..=r).map(move |x| (x, y));
    let col = |x: i16| (0..=15).map(move |y| (x, y));
    match dir {
        1 => row(0).collect(),
        2 => row(0).chain((1..=15).map(|y| (r, y))).collect(),
        3 => col(r).collect(),
        4 => col(r).chain((0..r).rev().map(|x| (x, 15))).collect(),
        5 => row(15).collect(),
        6 => col(0).chain((1..=r).map(|x| (x, 15))).collect(),
        7 => col(0).collect(),
        8 => (0..=15).rev().map(|y| (0, y)).chain((1..=r).map(|x| (x, 0))).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busiest_sector() {
        let mut m = Map::new(b"t", 200, 200);
        m.update_sector_info();
        assert_eq!((m.max_px, m.max_py), (0, 0));
        for (x, y, n) in [(25, 45, 3), (65, 5, 3), (130, 130, 2)] {
            for _ in 0..n {
                m.inc_player_activity(x, y);
            }
        }
        m.inc_player_activity(-1, 5); // outside: ignored, not a panic
        m.inc_player_activity(2000, 5);
        m.update_sector_info();
        // a tie: the first sector in x, y order
        assert_eq!((m.max_px, m.max_py), (1, 2));
        // the counts start again
        m.update_sector_info();
        assert_eq!((m.max_px, m.max_py), (0, 0));
    }

    fn amd(sx: i32, sy: i32, f: impl Fn(i32, i32) -> (i16, u8)) -> Vec<u8> {
        let mut h = format!("MAPSIZEX = {sx} MAPSIZEY = {sy} TILESIZE = 10").into_bytes();
        h.resize(256, 0);
        for y in 0..sy {
            for x in 0..sx {
                let (spr, a) = f(x, y);
                let mut t = [0u8; 10];
                t[..2].copy_from_slice(&spr.to_le_bytes());
                t[8] = a;
                h.extend_from_slice(&t);
            }
        }
        h
    }

    #[test]
    fn amd_decode_and_bounds() {
        let m = Map::from_amd(b"test", &amd(60, 60, |x, y| if (x, y) == (30, 31) { (0, 0x80) } else if (x, y) == (14, 12) { (19, 0x40) } else { (0, 0) })).unwrap();
        assert_eq!((m.size_x, m.size_y, m.tile_data_size), (60, 60, 10));
        assert!(!m.get_moveable(30, 31).0 && m.get_moveable(30, 30).0);
        assert!(!m.get_moveable(19, 30).0 && m.get_moveable(20, 30).0 && !m.get_moveable(40, 30).0 && m.get_moveable(39, 30).0);
        assert!(m.is_teleport(14, 12) && m.is_water(14, 12) && !m.is_teleport(13, 12));
        assert!(Map::from_amd(b"short", &amd(60, 60, |_, _| (0, 0))[..1000]).is_err());
    }

    #[test]
    fn non_square_index() {
        let m = Map::from_amd(b"wide", &amd(80, 50, |x, y| (0, if (x, y) == (70, 40) { 0x80 } else { 0 }))).unwrap();
        assert!(!m.tile(70, 40).unwrap().move_allowed);
        assert_eq!(m.tiles.iter().filter(|t| !t.move_allowed).count(), 1);
        assert!(m.tile(80, 0).is_none() && m.tile(0, 50).is_none());
    }

    #[test]
    fn owners() {
        let mut m = Map::new(b"x", 50, 50);
        m.set_owner(5, OWNERTYPE_PLAYER, 25, 25);
        assert_eq!(m.get_owner(25, 25), (5, OWNERTYPE_PLAYER));
        assert!(!m.get_moveable(25, 25).0);
        m.clear_owner(6, OWNERTYPE_PLAYER, 25, 25);
        assert_eq!(m.get_owner(25, 25).0, 5);
        m.clear_owner(5, OWNERTYPE_PLAYER, 25, 25);
        assert_eq!(m.get_owner(25, 25), (0, 0));
        m.set_owner(1, 1, -1, 3); // ignored
    }

    #[test]
    fn info_file() {
        let mut m = Map::new(b"fightzone1", 100, 100);
        let txt = b"map-location = aresden\ntype = 1\nteleport-loc = 30 40 elvine 50 60 5\ninitial-point = 0 70 80\nnpc = Guard-Aresden 1 0 0 0 0 0 0 0 0 0 0 a\nnpc-avoidrect = 1 10 10 20 20\nfixed-dayornight-mode = 1\nitem-event = 0 Gold 10 5 12 25 1 Slime Orc EOL\nmobevent-amount = 7\nwaypoint = 3 1 2\nno-attack-area = 1 5 6 7 8\n[END-MAP-INFO]\ntype = 9";
        m.read_info(txt).unwrap();
        assert_eq!(m.location_name, b"aresden");
        assert_eq!(m.map_type, 1);
        assert!(m.is_fight_zone && m.is_fixed_day_mode);
        assert_eq!(m.search_teleport_dest(30, 40).unwrap().dest_map, b"elvine");
        assert_eq!(m.initial_points[0], Some((70, 80)));
        assert_eq!(m.records(b"npc").count(), 1);
        assert_eq!(m.records(b"npc-avoidrect").count(), 1, "npc-avoidrect is not an npc line");
        assert_eq!(m.records(b"item-event").next().unwrap().len(), 9);
        assert_eq!(m.records(b"mobevent-amount").count(), 1);
        assert_eq!(m.waypoints[3], Some((1, 2)));
        assert_eq!((m.no_attack_rects.clone(), m.attribute(7, 8, 0x06), m.attribute(5, 6, 0x04), m.attribute(8, 8, 0x04)), (vec![(5, 6, 7, 8)], 4, 4, 0));
        assert!(Map::new(b"x", 10, 10).read_info(b"teleport-loc = 1 2 elvine").is_err());
        assert!(Map::new(b"x", 10, 10).read_info(b"teleport-loc = 1 x elvine 1 1 1").is_err());
    }

    #[test]
    fn strips() {
        assert_eq!(move_strip(1).len(), 28);
        assert_eq!(move_strip(2).len(), 43);
        assert_eq!(move_strip(4)[16], (26, 15));
        assert_eq!(move_strip(8)[0], (0, 15));
        assert_eq!(move_strip(8)[16], (1, 0));
    }

    #[test]
    fn real_maps() {
        let base = Path::new(r"G:\DEV\hb\run\server\Maps\All");
        let Ok(cfg) = std::fs::read(base.join("GServer.cfg")) else { return };
        let cfg = crate::util::text_mode(&cfg);
        let mut t = StrTok::new(&cfg, b"= \t\n");
        let (mut maps, mut tps, mut npcs, mut bad) = (0, 0, 0, Vec::new());
        while let Some(k) = t.get() {
            if k.starts_with(b"game-server-map") {
                let name = crate::util::l1(&t.get().unwrap());
                let dir = base.join("MAPDATA");
                let mut m = Map::load(&dir, &name).unwrap();
                let txt = find_file(&dir, &format!("{name}.txt")).map(|p| crate::util::text_mode(&std::fs::read(p).unwrap()));
                if let Err(e) = txt.map_or(Err("no .txt".to_string()), |t| m.read_info(&t)) {
                    bad.push(format!("{name}: {e}"));
                }
                maps += 1;
                tps += m.teleport_locs.len();
                npcs += m.records(b"npc").count();
            }
        }
        println!("maps {maps} teleports {tps} npc spawns {npcs} info errors {bad:?}");
        assert!(maps > 0);
    }
}
