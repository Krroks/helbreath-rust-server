//! Rust only: the health snapshot behind `/stats` (page `Client/Web/tools/stats.html`, served by serve.mjs). Every 2 s the game loop
//! builds the JSON (`Game::stats_publish`) into `bots_web::Shared::stats`; the HTTP task only serves it at `GET /api/stats`, so it never
//! touches the game state. If the game loop stalls the snapshot stops being renewed and its `t` goes stale: the page shows that age.
use crate::bots_web::json_str;
use crate::consts::*;
use crate::events::{secs_left, EVENT_DOUBLE_EK, EVENT_DOUBLE_EXP};
use crate::game::Game;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

/// Frames and bytes sent to the clients, and log lines that were dropped because the writer thread fell behind.
pub static FRAMES_OUT: AtomicU64 = AtomicU64::new(0);
pub static BYTES_OUT: AtomicU64 = AtomicU64::new(0);
pub static LOG_DROPPED: AtomicU64 = AtomicU64::new(0);

pub fn count_out(bytes: usize) {
    FRAMES_OUT.fetch_add(1, Relaxed);
    BYTES_OUT.fetch_add(bytes as u64, Relaxed);
}

/// A game tick that takes longer than this is logged and listed under the problems.
const SLOW_MS: u128 = 200;
/// Windows of one publish (2 s) kept: one minute.
const KEEP: usize = 30;
const PUBLISH_MS: u32 = 2000;

#[derive(Clone, Copy, Default)]
struct Window {
    ticks: u32,
    sum_us: u64,
    max_us: u64,
    gap_max_ms: u32,
}

pub struct Stats {
    started: Instant,
    last_tick: Option<Instant>,
    win: Window,
    hist: VecDeque<Window>,
    pub ticks: u64,
    pub slow: u64,
    pub events: u64,
    pub msgs: u64,
    next_publish: u32,
    prev: (Instant, u64, u64, u64, u64),
    problems: VecDeque<String>,
}

impl Default for Stats {
    fn default() -> Stats {
        let now = Instant::now();
        Stats {
            started: now,
            last_tick: None,
            win: Window::default(),
            hist: VecDeque::new(),
            ticks: 0,
            slow: 0,
            events: 0,
            msgs: 0,
            next_publish: 0,
            prev: (now, 0, 0, 0, 0),
            problems: VecDeque::new(),
        }
    }
}

impl Stats {
    /// One `on_timer` call that took `took`.
    pub fn tick(&mut self, took: Duration) {
        let now = Instant::now();
        if let Some(l) = self.last_tick {
            self.win.gap_max_ms = self.win.gap_max_ms.max((now - l).as_millis() as u32);
        }
        self.last_tick = Some(now);
        let us = took.as_micros() as u64;
        self.win.ticks += 1;
        self.win.sum_us += us;
        self.win.max_us = self.win.max_us.max(us);
        self.ticks += 1;
        if took.as_millis() > SLOW_MS {
            self.slow += 1;
            self.problem(format!("slow tick: {} ms", took.as_millis()));
        }
    }

    pub fn problem(&mut self, what: String) {
        let t = chrono::Local::now().format("%H:%M:%S");
        self.problems.push_back(format!("{t} {what}"));
        while self.problems.len() > 20 {
            self.problems.pop_front();
        }
    }
}

/// Everything the page shows, as plain numbers (so the JSON and the verdict are testable without a running game).
#[derive(Default)]
pub struct Snap {
    pub t_unix_ms: u64,
    pub uptime_s: u64,
    pub version: String,
    pub tick_ms: i32,
    pub ticks_per_s: f64,
    pub avg_ms: f64,
    pub max_ms: f64,
    pub max_ms_1m: f64,
    pub gap_ms: u32,
    pub gap_ms_1m: u32,
    pub slow_total: u64,
    pub msgs_pending: usize,
    pub ev_pending: usize,
    pub ev_cap: usize,
    pub msgs_in_s: f64,
    pub events_s: f64,
    pub frames_out_s: f64,
    pub bytes_out_s: f64,
    pub msgs_total: u64,
    pub frames_total: u64,
    pub bytes_total: u64,
    pub log_dropped: u64,
    pub sockets: usize,
    pub players: usize,
    pub bots: usize,
    pub registered: bool,
    pub links: usize,
    pub links_max: usize,
    pub game_started: bool,
    pub data_ok: bool,
    pub night: bool,
    pub npcs: usize,
    pub ground: usize,
    pub double_exp_s: i32,
    pub double_ek_s: i32,
    pub crusade: bool,
    pub heldenian: bool,
    pub apocalypse: bool,
    /// name, players, bots, npcs, ground items
    pub maps: Vec<(String, usize, usize, usize, usize)>,
    /// name, level, map, guild
    pub people: Vec<(String, i32, String, String)>,
    pub problems: Vec<String>,
}

/// ("ok" | "warn" | "bad", the reasons)
pub fn health(s: &Snap) -> (&'static str, Vec<String>) {
    let mut bad = Vec::new();
    let mut warn = Vec::new();
    if !s.registered {
        bad.push("not registered with the Login".to_string());
    } else if s.links < s.links_max {
        warn.push(format!("Login sockets {}/{}", s.links, s.links_max));
    }
    if !s.data_ok {
        bad.push("configuration from the Login incomplete".to_string());
    }
    if s.max_ms_1m > 200.0 {
        bad.push(format!("a tick took {:.0} ms in the last minute", s.max_ms_1m));
    } else if s.max_ms_1m > 100.0 {
        warn.push(format!("a tick took {:.0} ms in the last minute", s.max_ms_1m));
    }
    if s.gap_ms_1m > 1000 {
        bad.push(format!("the loop paused {} ms in the last minute", s.gap_ms_1m));
    } else if s.gap_ms_1m > 300 {
        warn.push(format!("the loop paused {} ms in the last minute", s.gap_ms_1m));
    }
    if s.ev_cap > 0 && s.ev_pending * 2 > s.ev_cap {
        bad.push(format!("event queue {}/{}", s.ev_pending, s.ev_cap));
    } else if s.ev_cap > 0 && s.ev_pending * 10 > s.ev_cap {
        warn.push(format!("event queue {}/{}", s.ev_pending, s.ev_cap));
    }
    if s.msgs_pending > 2000 {
        warn.push(format!("{} client messages waiting", s.msgs_pending));
    }
    if s.log_dropped > 0 {
        warn.push(format!("{} log lines dropped", s.log_dropped));
    }
    if !bad.is_empty() {
        bad.extend(warn);
        ("bad", bad)
    } else if !warn.is_empty() {
        ("warn", warn)
    } else {
        ("ok", Vec::new())
    }
}

fn arr<T>(v: &[T], f: impl Fn(&T) -> String) -> String {
    format!("[{}]", v.iter().map(f).collect::<Vec<_>>().join(","))
}

pub fn to_json(s: &Snap) -> String {
    let (level, reasons) = health(s);
    format!(
        concat!(
            "{{\"t\":{},\"uptime_s\":{},\"version\":{},",
            "\"health\":{{\"level\":\"{}\",\"reasons\":{}}},",
            "\"loop\":{{\"tick_ms\":{},\"ticks_per_s\":{:.1},\"avg_ms\":{:.3},\"max_ms\":{:.2},\"max_ms_1m\":{:.2},\"gap_ms\":{},\"gap_ms_1m\":{},",
            "\"slow_total\":{},\"msgs_pending\":{},\"ev_pending\":{},\"ev_cap\":{}}},",
            "\"net\":{{\"sockets\":{},\"players\":{},\"bots\":{},\"msgs_in_s\":{:.1},\"events_s\":{:.1},\"frames_out_s\":{:.1},\"bytes_out_s\":{:.0},",
            "\"msgs_total\":{},\"frames_total\":{},\"bytes_total\":{},\"log_dropped\":{}}},",
            "\"login\":{{\"registered\":{},\"links\":{},\"links_max\":{},\"game_started\":{},\"data_ok\":{}}},",
            "\"world\":{{\"night\":{},\"npcs\":{},\"ground\":{},\"double_exp_s\":{},\"double_ek_s\":{},\"crusade\":{},\"heldenian\":{},\"apocalypse\":{},",
            "\"maps\":{}}},",
            "\"people\":{},\"problems\":{}}}"
        ),
        s.t_unix_ms,
        s.uptime_s,
        json_str(&s.version),
        level,
        arr(&reasons, |r| json_str(r)),
        s.tick_ms,
        s.ticks_per_s,
        s.avg_ms,
        s.max_ms,
        s.max_ms_1m,
        s.gap_ms,
        s.gap_ms_1m,
        s.slow_total,
        s.msgs_pending,
        s.ev_pending,
        s.ev_cap,
        s.sockets,
        s.players,
        s.bots,
        s.msgs_in_s,
        s.events_s,
        s.frames_out_s,
        s.bytes_out_s,
        s.msgs_total,
        s.frames_total,
        s.bytes_total,
        s.log_dropped,
        s.registered,
        s.links,
        s.links_max,
        s.game_started,
        s.data_ok,
        s.night,
        s.npcs,
        s.ground,
        s.double_exp_s,
        s.double_ek_s,
        s.crusade,
        s.heldenian,
        s.apocalypse,
        arr(&s.maps, |m| format!("{{\"name\":{},\"players\":{},\"bots\":{},\"npcs\":{},\"ground\":{}}}", json_str(&m.0), m.1, m.2, m.3, m.4)),
        arr(&s.people, |p| format!("{{\"name\":{},\"level\":{},\"map\":{},\"guild\":{}}}", json_str(&p.0), p.1, json_str(&p.2), json_str(&p.3))),
        arr(&s.problems, |p| json_str(p)),
    )
}

impl Game {
    /// Every 2 s (called at the top of `on_timer`): the snapshot for the web side. It costs one pass over the clients, the NPCs and the
    /// ground items, so it is not done at all when nothing serves it.
    pub fn stats_publish(&mut self) {
        let now = self.now();
        let Some(web) = self.bots.web.clone() else { return };
        if now.wrapping_sub(self.stats.next_publish) as i32 <= 0 {
            return;
        }
        self.stats.next_publish = now.wrapping_add(PUBLISH_MS);
        let snap = self.stats_snapshot(now);
        let json = to_json(&snap);
        web.lock().unwrap().stats = json;
    }

    fn stats_snapshot(&mut self, now: u32) -> Snap {
        let t = Instant::now();
        let (frames, bytes) = (FRAMES_OUT.load(Relaxed), BYTES_OUT.load(Relaxed));
        let (p_t, p_ev, p_msg, p_fr, p_by) = self.stats.prev;
        let dt = (t - p_t).as_secs_f64().max(0.001);
        self.stats.prev = (t, self.stats.events, self.stats.msgs, frames, bytes);
        let w = std::mem::take(&mut self.stats.win);
        self.stats.hist.push_back(w);
        while self.stats.hist.len() > KEEP {
            self.stats.hist.pop_front();
        }
        let max_us_1m = self.stats.hist.iter().map(|w| w.max_us).max().unwrap_or(0);
        let gap_1m = self.stats.hist.iter().map(|w| w.gap_max_ms).max().unwrap_or(0);
        let (ev_pending, ev_cap) = self.ev_load();
        let bot_names: std::collections::HashSet<&str> = self.bots.list.iter().map(|b| b.ch.as_str()).collect();
        let nmaps = self.maps.len();
        let mut per_map = vec![(0usize, 0usize, 0usize, 0usize); nmaps];
        let (mut sockets, mut players, mut bots) = (0, 0, 0);
        let mut people = Vec::new();
        for c in self.clients.iter().flatten() {
            sockets += 1;
            if !c.is_init_complete {
                continue;
            }
            let name = String::from_utf8_lossy(crate::util::cstr(&c.char_name)).to_string();
            let bot = bot_names.contains(name.as_str());
            if c.map_index < nmaps {
                if bot {
                    per_map[c.map_index].1 += 1;
                } else {
                    per_map[c.map_index].0 += 1;
                }
            }
            if bot {
                bots += 1;
            } else {
                players += 1;
                if people.len() < 100 {
                    let map = self.maps.get(c.map_index).map_or(String::new(), |m| String::from_utf8_lossy(crate::util::cstr(&m.name)).to_string());
                    people.push((name, c.level, map, String::from_utf8_lossy(crate::util::cstr(&c.guild_name)).to_string()));
                }
            }
        }
        let mut npcs = 0;
        for n in self.npcs.iter().flatten() {
            npcs += 1;
            if n.map_index < nmaps {
                per_map[n.map_index].2 += 1;
            }
        }
        let mut by_map: HashMap<usize, usize> = HashMap::new();
        let mut ground = 0;
        for ((m, _, _), v) in &self.ground {
            ground += v.len();
            *by_map.entry(*m).or_default() += v.len();
        }
        for (m, n) in by_map {
            if m < nmaps {
                per_map[m].3 += n;
            }
        }
        let maps = (0..nmaps)
            .filter(|&i| per_map[i] != (0, 0, 0, 0))
            .map(|i| (String::from_utf8_lossy(crate::util::cstr(&self.maps[i].name)).to_string(), per_map[i].0, per_map[i].1, per_map[i].2, per_map[i].3))
            .collect();
        Snap {
            t_unix_ms: crate::bots::unix_ms(),
            uptime_s: self.stats.started.elapsed().as_secs(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            tick_ms: TICKDELAY,
            ticks_per_s: w.ticks as f64 / dt,
            avg_ms: if w.ticks > 0 { w.sum_us as f64 / w.ticks as f64 / 1000.0 } else { 0.0 },
            max_ms: w.max_us as f64 / 1000.0,
            max_ms_1m: max_us_1m as f64 / 1000.0,
            gap_ms: w.gap_max_ms,
            gap_ms_1m: gap_1m,
            slow_total: self.stats.slow,
            msgs_pending: self.queue.len(),
            ev_pending,
            ev_cap,
            msgs_in_s: (self.stats.msgs - p_msg) as f64 / dt,
            events_s: (self.stats.events - p_ev) as f64 / dt,
            frames_out_s: (frames - p_fr) as f64 / dt,
            bytes_out_s: (bytes - p_by) as f64 / dt,
            msgs_total: self.stats.msgs,
            frames_total: frames,
            bytes_total: bytes,
            log_dropped: LOG_DROPPED.load(Relaxed),
            sockets,
            players,
            bots,
            registered: self.registered,
            links: self.ls_links(),
            links_max: MAXSUBLOGSOCK as usize,
            game_started: self.game_started,
            data_ok: self.data.all_ok(),
            night: self.day_or_night == 2,
            npcs,
            ground,
            double_exp_s: secs_left(self.events.until[EVENT_DOUBLE_EXP], now),
            double_ek_s: secs_left(self.events.until[EVENT_DOUBLE_EK], now),
            crusade: self.cr.mode,
            heldenian: self.hd.mode,
            apocalypse: self.ap.mode,
            maps,
            people,
            problems: self.stats.problems.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok() -> Snap {
        Snap { registered: true, links: 5, links_max: 5, data_ok: true, ev_cap: 8192, ..Default::default() }
    }

    #[test]
    fn verdicts() {
        assert_eq!(health(&ok()).0, "ok");
        let mut s = ok();
        s.max_ms_1m = 150.0;
        assert_eq!(health(&s).0, "warn");
        s.registered = false;
        let (l, r) = health(&s);
        assert_eq!(l, "bad");
        assert!(r[0].contains("Login") && r.len() == 2, "{r:?}"); // the bad reasons first, then the warnings
        let mut q = ok();
        q.ev_pending = 5000;
        assert_eq!(health(&q).0, "bad");
        q.ev_pending = 900;
        assert_eq!(health(&q).0, "warn");
    }

    #[test]
    fn json_is_well_formed() {
        let mut s = ok();
        s.version = "0.1".into();
        s.maps.push(("a\"b".into(), 1, 2, 3, 4));
        s.people.push(("Neo".into(), 80, "default".into(), "".into()));
        s.problems.push("slow tick: 250 ms".into());
        let j = to_json(&s);
        assert!(j.starts_with("{\"t\":0,") && j.ends_with("\"problems\":[\"slow tick: 250 ms\"]}"), "{j}");
        assert!(j.contains("\"maps\":[{\"name\":\"a\\\"b\",\"players\":1,\"bots\":2,\"npcs\":3,\"ground\":4}]"));
        // every brace and bracket outside the strings closes
        let (mut depth, mut in_str, mut esc) = (0i32, false, false);
        for c in j.chars() {
            match (in_str, esc, c) {
                (true, true, _) => esc = false,
                (true, false, '\\') => esc = true,
                (true, false, '"') => in_str = false,
                (false, _, '"') => in_str = true,
                (false, _, '{' | '[') => depth += 1,
                (false, _, '}' | ']') => depth -= 1,
                _ => {}
            }
            assert!(depth >= 0);
        }
        assert_eq!(depth, 0);
    }

    #[test]
    fn slow_ticks_are_listed() {
        let mut s = Stats::default();
        s.tick(Duration::from_millis(5));
        s.tick(Duration::from_millis(250));
        assert_eq!((s.ticks, s.slow, s.problems.len()), (2, 1, 1));
        for _ in 0..40 {
            s.problem("x".into());
        }
        assert_eq!(s.problems.len(), 20);
    }
}
