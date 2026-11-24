//! Helbreath game server ("HG"), ported from the C++ `HG` project. See PORTING.md.
//!
//! usage: hg [--dir SERVERDIR] [--bind ADDR] [--ws-port N] [--no-tcp] [--login HOST:PORT] [--local-ip IP] [--seed N]
//!           [--bots N --bots-login HOST:PORT] [--bots-cfg FILE] [--bots-sample]   (Rust only bots: see bots.rs, Bots.cfg)
//!
//! Runs from SERVERDIR (a folder like run/server/Maps/All: GServer.cfg, Settings.cfg, MAPDATA/, GameData/, and
//! ../configs/droplist.xml). Clients connect with plain TCP on game-server-port and, with --ws-port, WebSocket
//! (one binary message = one frame). The Login is reached at log-server-address and the C++ gate port 5656
//! (--login overrides both). --local-ip stands in for the host address the C++ registers when
//! game-server-ext-address is set. --seed fixes rand() (no time reseeds), to compare runs.
//! stdin: `shutdown` (F1+F4 in the C++ window), `quit`.
#![allow(dead_code)]
#[path = "../../login/src/util.rs"]
mod util;
mod bots;
mod bots_web;
mod chat;
mod combat;
mod delay;
mod drops;
mod farming;
mod dynamic;
mod item_use;
mod items;
mod exchange;
mod crafting;
mod itemlog;
mod shop;
mod slates;
mod upgrade;
mod skills;
mod life;
mod magic;
mod npc_ai;
mod client;
mod config;
mod consts;
mod droplist;
mod entry;
mod game;
mod gsm;
mod guild;
mod party;
mod quest;
mod crusade;
mod heldenian;
mod apocalypse;
mod astoria;
mod scheduler;
mod events;
mod stats;
mod gm;
mod notice;
mod energy;
mod fightzone;
mod gizon;
mod reward;
mod website;
mod legion;
mod gamedata;
mod log;
mod map;
mod motion;
mod net;
mod notify;
mod npc;
mod player_data;
mod rng;
mod teleport;

use chrono::{Datelike, Local, Timelike};
use log::log;
use std::path::PathBuf;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc::channel;

#[derive(Debug, Default, Clone, Copy)]
pub struct Now {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = sunday, as SYSTEMTIME.wDayOfWeek
    pub weekday: u32,
}

/// GetLocalTime
pub fn chrono_now() -> Now {
    let t = Local::now();
    Now { year: t.year() as u32, month: t.month(), day: t.day(), hour: t.hour(), minute: t.minute(), second: t.second(), weekday: t.weekday().num_days_from_sunday() }
}

pub fn now_fields() -> [i32; 6] {
    let t = chrono_now();
    [t.year as i32, t.month as i32, t.day as i32, t.hour as i32, t.minute as i32, t.second as i32]
}

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--bots-sample") {
        print!("{}", bots::CFG_SAMPLE);
        return;
    }
    if let Some(d) = arg(&args, "--dir") {
        if let Err(e) = std::env::set_current_dir(&d) {
            eprintln!("cannot enter {d}: {e}");
            log::exit(1);
        }
    }
    log("(*) INITIALIZING GAME SERVER...");
    let local_ip = arg(&args, "--local-ip").unwrap_or_else(|| "127.0.0.1".into());
    let cfg = match config::read_program(&PathBuf::from("GServer.cfg"), local_ip.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            log(&e);
            log("(!!!) CRITICAL ERROR! Cannot execute server! config file contents error!");
            log::exit(1);
        }
    };
    let Some(settings) = config::read_settings(&PathBuf::from("Settings.cfg")) else {
        log("(!!!) CRITICAL ERROR! Cannot execute server! Settings.cfg file contents error!");
        log::exit(1);
    };
    let mut maps = Vec::new();
    let dir = PathBuf::from("MAPDATA");
    for name in &cfg.maps {
        let n = util::l1(name);
        let mut m = match map::Map::load(&dir, &n) {
            Ok(m) => m,
            Err(e) => {
                log(&format!("(ERROR) Data file {n} loading fail! {e}"));
                log::exit(1);
            }
        };
        // the C++ reads the .txt files in OnStartGameSignal; nothing uses them before that
        match map::find_file(&dir, &format!("{n}.txt")).map(|p| std::fs::read(p).map(|r| util::text_mode(&r))) {
            Some(Ok(t)) => {
                if let Err(e) = m.read_info(&t) {
                    log(&format!("(!!!) Map info file {n}.txt: {e}"));
                }
            }
            _ => log(&format!("(!!!) Map info file {n}.txt not found")),
        }
        maps.push(m);
    }
    log(&format!("(*) {} maps loaded.", maps.len()));
    let login = arg(&args, "--login").unwrap_or_else(|| {
        let port = if cfg.gate_server_port != 0 { cfg.gate_server_port } else { 5656 };
        format!("{}:{port}", util::l1(&cfg.log_server_addr))
    });
    let bind = arg(&args, "--bind").unwrap_or_else(|| util::l1(&cfg.game_server_addr));
    let seed = arg(&args, "--seed").and_then(|s| s.parse().ok());
    let tcp_port = cfg.game_server_port;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async move {
        let (tx, mut rx) = channel(net::EV_CAP);
        if !args.iter().any(|a| a == "--no-tcp") {
            tokio::spawn(net::listen(format!("{bind}:{tcp_port}"), false, tx.clone()));
        }
        if let Some(p) = arg(&args, "--ws-port") {
            tokio::spawn(net::listen(format!("{bind}:{p}"), true, tx.clone()));
        }
        let stdin_tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let _ = stdin_tx.send(net::Ev::Stdin(l)).await;
            }
        });
        log(&format!("(*) Login (gate) server at {login}"));
        // Rust only (bots.rs): Bots.cfg, overridden by --bots N --bots-login HOST:PORT --bots-cfg FILE
        let cfg_path = arg(&args, "--bots-cfg").unwrap_or_else(|| "Bots.cfg".into());
        let mut bcfg = match std::fs::read_to_string(&cfg_path) {
            Ok(t) => {
                let (mut c, warnings) = bots::BotCfg::parse(&t);
                for w in warnings {
                    log(&format!("(!) {cfg_path}: {w}"));
                }
                c.load_names(&cfg_path);
                c
            }
            Err(_) => bots::BotCfg::default(),
        };
        if let Some(n) = arg(&args, "--bots").and_then(|s| s.parse().ok()) {
            bcfg.count = n;
        }
        if let Some(l) = arg(&args, "--bots-login") {
            bcfg.login = l;
        }
        if bcfg.count > 0 && bcfg.login.is_empty() {
            log("(!!!) bots need bot-login-address in Bots.cfg or --bots-login HOST:PORT (the Login's client port)");
            log::exit(1);
        }
        let bots = bots::Bots::new(bcfg);
        for (i, b) in bots.list.iter().enumerate() {
            tokio::spawn(bots::run_bot(i, bots.cfg.clone(), b.acc.clone(), b.ch.clone(), tx.clone()));
        }
        if !bots.list.is_empty() {
            log(&format!("(*) {} bots from {cfg_path}", bots.list.len()));
        }
        let ev_tx = tx.clone();
        let mut g = game::Game::new(cfg, settings, maps, tx, login, seed);
        g.bots = bots;
        if g.bots.cfg.web_port > 0 {
            let web: bots_web::Web = Default::default();
            tokio::spawn(bots_web::serve(g.bots.cfg.web_port, web.clone()));
            g.bots.web = Some(web);
        }
        (g.bots.ev, g.bots.cfg_mtime, g.bots.cfg_path) = (Some(ev_tx), std::fs::metadata(&cfg_path).and_then(|m| m.modified()).ok(), cfg_path.clone());
        // timeSetEvent(TICKDELAY, TIME_PERIODIC) -> WM_USER_TIMERSIGNAL -> OnTimer
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(consts::TICKDELAY as u64));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while !g.quit {
            tokio::select! {
                _ = tick.tick() => {
                    let t = std::time::Instant::now();
                    g.on_timer();
                    g.stats.tick(t.elapsed());
                    if t.elapsed().as_millis() > 200 {
                        log(&format!("(!) slow tick: {} ms", t.elapsed().as_millis()));
                    }
                }
                ev = rx.recv() => match ev {
                    Some(ev) => {
                        g.stats.events += 1;
                        g.on_event(ev)
                    }
                    None => break,
                },
            }
        }
        log("(!) Game server stopped.");
        log::flush();
    });
}
