//! `bReadProgramConfigFile` (GServer.cfg) and `bReadSettingsConfigFile` (Settings.cfg).
use crate::log::log;
use crate::util::{atoi, cstr, l1, text_mode, StrTok};

#[derive(Default, Clone)]
pub struct ProgramCfg {
    pub server_name: Vec<u8>,
    pub game_server_port: u16,
    pub log_server_addr: Vec<u8>,
    pub log_server_port: u16,
    pub website_addr: Vec<u8>,
    pub website_script_addr: Vec<u8>,
    pub website_port: u16,
    pub log_server_addr_external: Vec<u8>,
    pub game_server_addr: Vec<u8>,
    pub game_server_addr_external: Vec<u8>,
    /// m_iGateServerPort: the port of the Login's game-server listener. The C++ only sets it (to 5656) when
    /// `game-server-ext-address` is present, and then also replaces game-server-address with the first IP of the host.
    pub gate_server_port: u16,
    pub world_name: Vec<u8>,
    pub maps: Vec<Vec<u8>>,
}

#[derive(Clone)]
pub struct Settings {
    pub primary_drop_rate: i32,
    pub secondary_drop_rate: i32,
    pub enemy_kill_adjust: i32,
    pub raid_time: [i32; 7], // monday..sunday
    pub slate_success_rate: i32,
    pub char_stat_limit: i32,
    pub char_skill_limit: i32,
    pub rep_drop_modifier: i32,
    pub max_player_level: i32,
    pub exp_modifier: i32,
    /// level needed to create a guild (guild-min-level; the 3.82 server asks 20, the v8.00 client text says 80)
    pub guild_min_level: i32,
}

impl Default for Settings {
    fn default() -> Self {
        // fix: the C++ left every setting missing from the file uninitialised
        Settings {
            primary_drop_rate: 1,
            secondary_drop_rate: 1,
            enemy_kill_adjust: 1,
            raid_time: [3, 3, 3, 3, 10, 45, 60],
            slate_success_rate: 14,
            char_stat_limit: 200,
            char_skill_limit: 700,
            rep_drop_modifier: 0,
            max_player_level: 180,
            exp_modifier: 0,
            guild_min_level: 20,
        }
    }
}

fn read(path: &std::path::Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok().map(|r| text_mode(&r))
}

/// `local_ip` stands in for `gethostbyname(gethostname())` (the C++ uses it when game-server-ext-address is set).
pub fn read_program(path: &std::path::Path, local_ip: &[u8]) -> Result<ProgramCfg, String> {
    let data = read(path).ok_or("(ERROR) Cannot open configuration file.")?;
    let mut c = ProgramCfg::default();
    let mut t = StrTok::new(&data, b"= \t\n");
    let mut mode = 0;
    let too_long = |what: &str, tok: &[u8], n: usize| format!("(!!!) {what}({}) must be within {n} chars!", l1(tok));
    while let Some(tok) = t.get() {
        if mode != 0 {
            match mode {
                1 if tok.len() > 10 => return Err(too_long("Game server name", &tok, 10)),
                1 => {
                    c.server_name = tok.clone();
                    log(&format!("(*) Game server name : {}", l1(&tok)));
                }
                2 => c.game_server_port = atoi(&tok) as u16,
                3 | 7 | 8 | 9 if tok.len() > 15 => return Err(too_long("Address", &tok, 15)),
                3 => c.log_server_addr = tok.clone(),
                4 => c.log_server_port = atoi(&tok) as u16,
                5 => {
                    let w = tok.iter().position(|&b| b == b'/').unwrap_or(tok.len());
                    if tok.len() >= 120 || w == 0 || w > 60 || tok.len() - w > 60 {
                        return Err(format!("(!!!) Website script url({}) is incorrectly formed!", l1(&tok)));
                    }
                    c.website_addr = tok[..w].to_vec();
                    c.website_script_addr = tok[w..].to_vec();
                }
                6 => c.website_port = atoi(&tok) as u16,
                7 => c.log_server_addr_external = tok.clone(),
                8 => c.game_server_addr = tok.clone(),
                9 => {
                    c.game_server_addr_external = tok.clone();
                    c.game_server_addr = local_ip.to_vec();
                    c.gate_server_port = 5656;
                }
                10 if tok.len() > 10 => return Err(too_long("World server name", &tok, 10)),
                10 => c.world_name = tok.clone(),
                11 if tok.len() > 10 => return Err(too_long("CRITICAL ERROR! Map name", &tok, 10)),
                11 => {
                    // _bRegisterMap's duplicate check
                    if c.maps.iter().any(|m| m[..m.len().min(10)] == tok[..tok.len().min(10)]) {
                        return Err(format!("(ERROR) CRITICAL ERROR! Map ({}) is already installed! cannot add.", l1(&tok)));
                    }
                    c.maps.push(tok.clone());
                }
                _ => {}
            }
            mode = 0;
        } else {
            // memcmp prefixes, later ones win, as in the C++
            for (k, m) in [
                (&b"game-server-name"[..], 1),
                (b"game-server-port", 2),
                (b"log-server-address", 3),
                (b"log-server-port", 4),
                (b"website-script-address", 5),
                (b"website-port", 6),
                (b"log-server-ext-address", 7),
                (b"game-server-address", 8),
                (b"game-server-ext-address", 9),
                (b"world-server-name", 10),
                (b"game-server-map", 11),
            ] {
                if tok.starts_with(k) {
                    mode = m;
                }
            }
        }
    }
    Ok(c)
}

pub fn read_settings(path: &std::path::Path) -> Option<Settings> {
    let data = read(path)?;
    let mut s = Settings::default();
    let mut t = StrTok::new(&data, b"= \t\n");
    let mut mode = 0;
    let keys: [(&[u8], usize); 17] = [
        (b"primary-drop-rate", 1),
        (b"secondary-drop-rate", 2),
        (b"enemy-kill-adjust", 3),
        (b"monday-raid-time", 4),
        (b"tuesday-raid-time", 5),
        (b"wednesday-raid-time", 6),
        (b"thursday-raid-time", 7),
        (b"friday-raid-time", 8),
        (b"saturday-raid-time", 9),
        (b"sunday-raid-time", 10),
        (b"slate-success-rate", 11),
        (b"character-stat-limit", 12),
        (b"character-skill-limit", 13),
        (b"rep-drop-modifier", 14),
        (b"max-player-level", 15),
        (b"exp-modifier", 16),
        (b"guild-min-level", 17),
    ];
    while let Some(tok) = t.get() {
        let v = atoi(cstr(&tok));
        match mode {
            0 => {
                for (k, m) in keys {
                    if tok.starts_with(k) {
                        mode = m;
                    }
                }
                continue;
            }
            1 if (1..=10000).contains(&tok.len()) => s.primary_drop_rate = v,
            2 if (1..=10000).contains(&tok.len()) => s.secondary_drop_rate = v,
            3 if (1..=100).contains(&tok.len()) => s.enemy_kill_adjust = v,
            4..=10 => {
                let def = [3, 3, 3, 3, 10, 45, 60][mode - 4];
                s.raid_time[mode - 4] = if v == 0 { def } else { v };
            }
            11 => s.slate_success_rate = if v == 0 { 14 } else { v },
            12 => s.char_stat_limit = if v == 0 { 200 } else { v },
            13 => s.char_skill_limit = if v == 0 { 700 } else { v },
            14 => s.rep_drop_modifier = v.max(0),
            15 => s.max_player_level = if v >= 801 { 800 } else if v == 0 { 180 } else { v },
            16 => s.exp_modifier = v.max(0),
            17 => s.guild_min_level = if v <= 0 { 20 } else { v },
            _ => {}
        }
        mode = 0;
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, text: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("hg-cfg-{}-{name}", std::process::id()));
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn guild_min_level_setting() {
        let p = tmp("gl.cfg", "[SETTINGS]\nguild-min-level = 80\n");
        assert_eq!(read_settings(&p).unwrap().guild_min_level, 80);
        let p = tmp("gl0.cfg", "[SETTINGS]\nmax-player-level = 180\n");
        assert_eq!(read_settings(&p).unwrap().guild_min_level, 20); // absent: the 3.82 value
    }

    #[test]
    fn program_cfg() {
        let p = tmp("gs.cfg", "[CONFIG]\r\ngame-server-name = middle\r\ngame-server-port = 3007\r\nlog-server-address\t= 127.0.0.1\r\nlog-server-port = 4000\r\nworld-server-name = WS1\r\nwebsite-script-address = host.net/x.php\r\n[MAPS]\r\ngame-server-map = default\r\ngame-server-map = aresden\r\n");
        let c = read_program(&p, b"10.0.0.9").unwrap();
        assert_eq!((c.server_name.as_slice(), c.game_server_port, c.log_server_port, c.gate_server_port), (&b"middle"[..], 3007, 4000, 0));
        assert_eq!(c.maps, [b"default".to_vec(), b"aresden".to_vec()]);
        assert_eq!((c.website_addr.as_slice(), c.website_script_addr.as_slice()), (&b"host.net"[..], &b"/x.php"[..]));
        let p = tmp("gs2.cfg", "game-server-address = 1.2.3.4\ngame-server-ext-address = 5.6.7.8\n");
        let c = read_program(&p, b"10.0.0.9").unwrap();
        assert_eq!((c.game_server_addr.as_slice(), c.gate_server_port), (&b"10.0.0.9"[..], 5656));
        assert!(read_program(&tmp("gs3.cfg", "game-server-map = a\ngame-server-map = a\n"), b"").is_err());
        assert!(read_program(&tmp("gs4.cfg", "game-server-name = elevenchars\n"), b"").is_err());
    }

    #[test]
    fn settings_cfg() {
        let s = read_settings(&tmp("s.cfg", "[SETTINGS]\nmax-player-level = 900\nmonday-raid-time = 0\nexp-modifier = 1000\ncharacter-stat-limit = 1000")).unwrap();
        assert_eq!((s.max_player_level, s.raid_time[0], s.exp_modifier, s.char_stat_limit, s.char_skill_limit), (800, 3, 1000, 1000, 700));
    }
}
