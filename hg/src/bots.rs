//! Rust only (no C++ counterpart): in-process bots that hunt monsters. Configured by `Bots.cfg` (next to
//! `Settings.cfg`; `CFG_SAMPLE` below documents every key), off when `bot-count` is 0.
//! `hg --bots N --bots-login HOST:PORT [--bots-cfg FILE]` override the file.
//!
//! A bot is an ordinary `Client`: a task logs it in (the Login's ENTERGAME, so the Login knows the account) and
//! hands the game server a fake connection through the normal `Ev::Accept` / `Ev::Read` path. From then on its
//! brain (`Game::bots_process`, run after `msg_process` every tick) reads the world directly and acts by pushing the
//! same frames a client would send (`Game::queue`), so every rule and anti-hack check of the handlers applies to it.
//!
//! Modes: `Hunt` (walk with A* to the nearest hostile monster in view, combat mode on, hit it until it dies, drink a
//! red potion when hurt, read a recall scroll when nearly dead and out of potions, wander round its home tile),
//! `Rest` (away from its home map: wait until healed) and `Travel` (back to the home tile, through the teleport
//! tiles of the maps this server runs), `Shop` (a trip to a `gshop*` map to sell the loot and buy potions and scrolls).
//! Gear: it equips what it carries (the best weapon, empty armour slots). Between two fights it picks up what lies about.
use crate::client::{Client, Item};
use crate::consts::*;
use crate::game::{From as Src, Game, Msg, MAXCLIENTS_};
use crate::item_use::{ITEMTYPE_EAT, ITEMTYPE_USE_DEPLETE};
use crate::log::log;
use crate::map::OWNERTYPE_NPC;
use crate::shop::{repair_price, sell_price};
use crate::net::{self, Ev};
use crate::util::{Rd, W};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

/// Set by the game when it takes connections: before that `bAccept` drops them.
pub static READY: AtomicBool = AtomicBool::new(false);

const KEY: u8 = 47; // the key the Login's clients use
const CREATENEWACCOUNT: u32 = 0x0FC94209;
const CREATENEWCHARACTER: u32 = 0x0FC9420B;
/// the server drops a client that sends 7 moves in under 1032 ms / 7 attacks in under 1480 ms: never go below this
const MIN_STEP_MS: u32 = 150;
const MIN_ATTACK_MS: u32 = 220;
/// least A* radius when travelling to a teleport tile or home (hunting uses `path-limit`); the window grows to reach the goal
const TRAVEL_LIMIT: i16 = 90;

// ------------------------------------------------------------------ configuration

#[derive(Clone, Debug)]
pub struct BotCfg {
    pub count: usize,
    pub login: String,
    pub account_prefix: String,
    pub name_prefix: String,
    pub password: String,
    pub login_spread_ms: u64,
    pub relogin_ms: u64,
    pub think_ms: u32,
    pub step_ms: u32,
    pub attack_ms: u32,
    pub sight: i16,
    pub wander_radius: i16,
    pub path_limit: i16,
    pub tired_pct: i32,
    pub giveup_hits: u32,
    pub giveup_pct: i32,
    pub potion_pct: i32,
    pub recall_pct: i32,
    pub rest_pct: i32,
    pub auto_equip: bool,
    pub use_potions: bool,
    pub use_recall: bool,
    pub auto_return: bool,
    pub status_log_ms: u32,
    pub loot: bool,
    pub loot_radius: i16,
    pub shop: bool,
    pub sell_loot: bool,
    pub shop_potion_min: u32,
    pub shop_potion_target: u32,
    pub shop_scroll_min: u32,
    pub shop_scroll_target: u32,
    pub shop_bag_slots: usize,
    pub shop_cooldown_ms: u32,
    pub shop_potion_item: String,
    pub shop_scroll_item: String,
    pub repair: bool,
    pub repair_pct: i32,
    pub roam: bool,
    pub roam_idle_ms: u32,
    pub hunt_level_step: i32,
}

impl Default for BotCfg {
    fn default() -> BotCfg {
        BotCfg {
            count: 0,
            login: String::new(),
            account_prefix: "bot".into(),
            name_prefix: "Bot".into(),
            password: "botpw1".into(),
            login_spread_ms: 500,
            relogin_ms: 10_000,
            think_ms: 100,
            step_ms: 420,
            attack_ms: 650,
            sight: 12,
            wander_radius: 8,
            path_limit: 24,
            tired_pct: 35,
            giveup_hits: 12,
            giveup_pct: 70,
            potion_pct: 50,
            recall_pct: 25,
            rest_pct: 90,
            auto_equip: true,
            use_potions: true,
            use_recall: true,
            auto_return: true,
            status_log_ms: 10_000,
            loot: true,
            loot_radius: 6,
            shop: true,
            sell_loot: true,
            shop_potion_min: 2,
            shop_potion_target: 10,
            shop_scroll_min: 1,
            shop_scroll_target: 2,
            shop_bag_slots: 40,
            shop_cooldown_ms: 120_000,
            shop_potion_item: "RedPotion".into(),
            shop_scroll_item: "RecallScroll".into(),
            repair: true,
            repair_pct: 30,
            roam: true,
            roam_idle_ms: 8000,
            hunt_level_step: 6,
        }
    }
}

/// The sample `Bots.cfg`: every key with its default and what it does.
pub const CFG_SAMPLE: &str = "\
// Bots.cfg - hunting bots of the Rust game server (hg). Next to Settings.cfg; 'key = value', // or # start a comment.
// hg --bots N --bots-login HOST:PORT --bots-cfg FILE override bot-count, bot-login-address and the file name.
bot-count            = 0          // how many bots (0 = none): account <prefix>NN, character <name-prefix>NN
bot-login-address    = 127.0.0.1:4000   // the Login's client TCP port (login --tcp-port); needed when bot-count > 0
bot-account-prefix   = bot        // account name = prefix + 2 digits (8 characters of prefix at most)
bot-name-prefix      = Bot        // character name = prefix + 2 digits (8 characters of prefix at most)
bot-password         = botpw1     // password of the bot accounts (10 characters at most)
bot-login-spread-ms  = 500        // delay between the bots' first logins
bot-relogin-ms       = 10000      // wait before a bot logs in again after the game dropped it
think-ms             = 100        // how often a bot thinks
step-ms              = 420        // time between two steps (not below 150: the server's speed-hack check)
attack-ms            = 650        // time between two attacks (not below 220)
sight                = 12         // tiles around it in which it looks for monsters (at most 14)
wander-radius        = 8          // it wanders within this many tiles of its home tile when nothing is near
path-limit           = 24         // A* looks this far (tiles) from the bot
tired-percent        = 35         // below this HP % it only fights what is within 2 tiles
giveup-hits          = 12         // after this many hits ...
giveup-percent       = 70         // ... with the target still above this % of its HP, it leaves it alone for 2 minutes
potion-percent       = 50         // drinks a red potion below this HP %
recall-percent       = 25         // reads a recall scroll below this HP % when it has no potion left
rest-percent         = 90         // away from home it waits until its HP is back to this % before going home
auto-equip           = 1          // equips the best weapon and fills empty armour slots from the bag
use-potions          = 1
use-recall           = 1
auto-return          = 1          // walks home (through the teleport tiles) after a recall or a death
status-log-ms        = 10000      // one status line per bot in the log this often (0 = never)
loot                 = 1          // picks up items lying near it when no monster is (the monsters drop no gold: loot is sold)
loot-radius          = 6          // tiles around it in which it looks for items
shop                 = 1          // goes shopping: walks to a general shop (map gshop*), sells, buys, comes back
sell-loot            = 1          // sells the loot (shop category 11-50: jelly, hides ...) at the shop
shop-potion-min      = 2          // shopping trip when it carries fewer red potions than this ...
shop-potion-target   = 10         // ... and it buys up to this many
shop-scroll-min      = 1          // the same for recall scrolls
shop-scroll-target   = 2
shop-bag-slots       = 40         // a trip also when the bag holds this many items (it sells the loot)
shop-cooldown-ms     = 120000     // least time between two trips
shop-potion-item     = RedPotion  // what it buys as a potion
shop-scroll-item     = RecallScroll
repair               = 1          // repairs worn gear at the shop (only characters of a nation wear it out; neutral ones do not)
repair-percent       = 30         // a trip when a worn piece is below this % of its endurance (or broke) and it can pay
roam                 = 1          // with no monster in sight for roam-idle-ms it walks to a hunting ground (possibly through teleports)
roam-idle-ms         = 8000
hunt-level-step      = 6          // it fights monsters of HitDice up to 2 + level / this (0 = any): level 1 slimes, 12 orcs, 24 scorpions
";

impl BotCfg {
    /// Parses a `Bots.cfg`; unknown keys and bad values come back as warnings and leave the default.
    pub fn parse(text: &str) -> (BotCfg, Vec<String>) {
        let mut c = BotCfg::default();
        let mut warn = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split("//").next().unwrap().split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                warn.push(format!("line {}: no '='", n + 1));
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            let num = v.parse::<i64>().ok();
            let flag = match v.to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            };
            let mut ok = true;
            match (k, num) {
                ("bot-count", Some(x)) => c.count = x.clamp(0, 99) as usize,
                ("bot-login-spread-ms", Some(x)) => c.login_spread_ms = x.max(0) as u64,
                ("bot-relogin-ms", Some(x)) => c.relogin_ms = x.max(0) as u64,
                ("think-ms", Some(x)) => c.think_ms = x.clamp(20, 5000) as u32,
                ("step-ms", Some(x)) => c.step_ms = x.max(MIN_STEP_MS as i64) as u32,
                ("attack-ms", Some(x)) => c.attack_ms = x.max(MIN_ATTACK_MS as i64) as u32,
                ("sight", Some(x)) => c.sight = x.clamp(1, 14) as i16,
                ("wander-radius", Some(x)) => c.wander_radius = x.clamp(1, 20) as i16,
                ("path-limit", Some(x)) => c.path_limit = x.clamp(4, 60) as i16,
                ("tired-percent", Some(x)) => c.tired_pct = x.clamp(0, 100) as i32,
                ("giveup-hits", Some(x)) => c.giveup_hits = x.max(1) as u32,
                ("giveup-percent", Some(x)) => c.giveup_pct = x.clamp(1, 100) as i32,
                ("potion-percent", Some(x)) => c.potion_pct = x.clamp(0, 100) as i32,
                ("recall-percent", Some(x)) => c.recall_pct = x.clamp(0, 100) as i32,
                ("rest-percent", Some(x)) => c.rest_pct = x.clamp(1, 100) as i32,
                ("status-log-ms", Some(x)) => c.status_log_ms = x.max(0) as u32,
                ("loot-radius", Some(x)) => c.loot_radius = x.clamp(1, 14) as i16,
                ("shop-potion-min", Some(x)) => c.shop_potion_min = x.max(0) as u32,
                ("shop-potion-target", Some(x)) => c.shop_potion_target = x.clamp(0, 100) as u32,
                ("shop-scroll-min", Some(x)) => c.shop_scroll_min = x.max(0) as u32,
                ("shop-scroll-target", Some(x)) => c.shop_scroll_target = x.clamp(0, 100) as u32,
                ("shop-bag-slots", Some(x)) => c.shop_bag_slots = x.clamp(1, 50) as usize,
                ("shop-cooldown-ms", Some(x)) => c.shop_cooldown_ms = x.max(0) as u32,
                ("repair-percent", Some(x)) => c.repair_pct = x.clamp(1, 99) as i32,
                ("roam-idle-ms", Some(x)) => c.roam_idle_ms = x.max(0) as u32,
                ("hunt-level-step", Some(x)) => c.hunt_level_step = x.clamp(0, 100) as i32,
                ("roam", _) if flag.is_some() => c.roam = flag.unwrap(),
                ("repair", _) if flag.is_some() => c.repair = flag.unwrap(),
                ("loot", _) if flag.is_some() => c.loot = flag.unwrap(),
                ("shop", _) if flag.is_some() => c.shop = flag.unwrap(),
                ("sell-loot", _) if flag.is_some() => c.sell_loot = flag.unwrap(),
                ("shop-potion-item", _) => c.shop_potion_item = v.chars().take(20).collect(),
                ("shop-scroll-item", _) => c.shop_scroll_item = v.chars().take(20).collect(),
                ("auto-equip", _) if flag.is_some() => c.auto_equip = flag.unwrap(),
                ("use-potions", _) if flag.is_some() => c.use_potions = flag.unwrap(),
                ("use-recall", _) if flag.is_some() => c.use_recall = flag.unwrap(),
                ("auto-return", _) if flag.is_some() => c.auto_return = flag.unwrap(),
                ("bot-login-address", _) => c.login = v.to_string(),
                ("bot-account-prefix", _) => c.account_prefix = v.chars().take(8).collect(),
                ("bot-name-prefix", _) => c.name_prefix = v.chars().take(8).collect(),
                ("bot-password", _) => c.password = v.chars().take(10).collect(),
                _ => ok = false,
            }
            if !ok {
                warn.push(format!("line {}: ignored '{k} = {v}'", n + 1));
            }
        }
        (c, warn)
    }
}

// ------------------------------------------------------------------ state

#[derive(Default, Clone, Copy, PartialEq, Debug)]
enum Mode {
    #[default]
    Hunt,
    Rest,
    Travel,
    Shop,
}

#[derive(Default)]
pub struct Bot {
    pub acc: String,
    pub ch: String,
    mode: Mode,
    next_think: u32,
    path: Vec<(i16, i16)>,
    path_goal: (i16, i16),
    wander: Option<(i16, i16)>,
    /// npc index being hunted, 0 = none
    target: usize,
    /// the target's HP when the first hit was sent: a fight that does not go down is given up
    target_hp0: i32,
    /// where it started (map, tile): it hunts and wanders around there and comes back to it
    home: Option<(usize, (i16, i16))>,
    /// npc index -> until when it is left alone
    ignore: Vec<(usize, u32)>,
    /// bag slots the server refused to equip
    equip_tried: Vec<usize>,
    last_step: u32,
    last_attack: u32,
    last_toggle: u32,
    last_restart: u32,
    last_report: u32,
    last_potion: u32,
    last_recall: u32,
    last_equip: u32,
    last_teleport: u32,
    last_pos: (i16, i16),
    stuck: u32,
    no_route_logged: bool,
    /// shopping trip: the shop map, when it started, the last trade frame, (slot, count) pairs the shop did not take,
    /// and how many purchases in a row did not raise what it carries
    shop_map: usize,
    shop_start: u32,
    last_trade: u32,
    last_shop: u32,
    sell_tried: Vec<(usize, u32)>,
    repair_tried: Vec<usize>,
    idle_since: Option<u32>,
    last_roam: u32,
    buy_fails: u32,
    last_have: u32,
    /// items on the ground: the tile it is going to, tries there, tiles to leave alone until a time
    loot_goal: Option<(i16, i16)>,
    loot_tries: u32,
    loot_skip: Vec<((i16, i16), u32)>,
    loot_last_n: usize,
    pub attacks: u32,
    pub kills: u32,
}

#[derive(Default)]
pub struct Bots {
    pub list: Vec<Bot>,
    pub cfg: Arc<BotCfg>,
}

impl Bots {
    pub fn new(cfg: BotCfg) -> Bots {
        let list = (1..=cfg.count).map(|i| Bot { acc: format!("{}{i:02}", cfg.account_prefix), ch: format!("{}{i:02}", cfg.name_prefix), ..Default::default() }).collect();
        Bots { list, cfg: Arc::new(cfg) }
    }
}

// ------------------------------------------------------------------ login task

fn body(msgid: u32, msgtype: u16, fields: &[(&[u8], usize)], total: usize) -> Vec<u8> {
    let mut b = msgid.to_le_bytes().to_vec();
    b.extend_from_slice(&msgtype.to_le_bytes());
    for (v, n) in fields {
        let mut f = v.to_vec();
        f.resize(*n, 0);
        b.extend_from_slice(&f);
    }
    b.resize(total.max(b.len()), 0);
    b
}

/// One request to the Login's client port and its first reply.
async fn login_call(addr: &str, b: &[u8]) -> Option<Vec<u8>> {
    let mut s = tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect(addr)).await.ok()?.ok()?;
    s.write_all(&proto::encode(KEY, b)).await.ok()?;
    let mut sp = proto::Splitter::default();
    let mut buf = [0u8; 4096];
    let end = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(Some(f)) = sp.next_frame() {
            return Some(proto::decode(&f).1);
        }
        let n = tokio::time::timeout_at(end, s.read(&mut buf)).await.ok()?.ok()?;
        if n == 0 {
            return None;
        }
        sp.push(&buf[..n]);
    }
}

/// Creates the account and character when missing (the Login refuses duplicates), then asks to enter the game.
async fn login(addr: &str, acc: &[u8], ch: &[u8], pwd: &[u8]) -> bool {
    let f: &[(&[u8], usize)] = &[(acc, 10), (pwd, 10), (b"bot@example.invalid", 50), (b" ", 10), (b"30", 10), (b" ", 4), (b" ", 2), (b" ", 2), (b"Nowhere", 17), (b"", 28), (b"bot quiz?", 45), (b"bot answer", 20), (b"", 50)];
    let _ = login_call(addr, &body(CREATENEWACCOUNT, 0, f, 264)).await;
    let mut b = body(CREATENEWCHARACTER, 0, &[(ch, 10), (acc, 10), (pwd, 10), (b"WS1", 30)], 66);
    b.extend_from_slice(&[1, 1, 1, 1, 1, 10, 10, 10, 14, 14, 12]);
    let _ = login_call(addr, &b).await;
    let mut b = body(MSGID_REQUEST_ENTERGAME, ENTERGAMEMSGTYPE_NEW as u16, &[(ch, 10), (b"", 10), (acc, 10), (pwd, 10), (b"WS1", 10)], 56);
    b.extend_from_slice(&1i32.to_le_bytes());
    b.resize(200, 0);
    let Some(r) = login_call(addr, &b).await else { return false };
    let r = Rd::new(&r);
    r.u32(0) == MSGID_RESPONSE_ENTERGAME && r.u16(4) == ENTERGAMERESTYPE_CONFIRM as u16
}

/// Keeps one bot connected: log in, hand the game a fake connection, wait until the game drops it, try again.
pub async fn run_bot(n: usize, cfg: Arc<BotCfg>, acc: String, ch: String, ev: UnboundedSender<Ev>) {
    tokio::time::sleep(Duration::from_millis(cfg.login_spread_ms * n as u64)).await; // spread the logins
    let pwd = cfg.password.as_bytes();
    loop {
        while !READY.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        if !login(&cfg.login, acc.as_bytes(), ch.as_bytes(), pwd).await {
            log(&format!("(bot) {ch}: login refused, retrying"));
            tokio::time::sleep(Duration::from_millis(cfg.relogin_ms)).await;
            continue;
        }
        let conn = net::new_conn_id();
        let (tx, mut rx) = unbounded_channel();
        // a distinct "address" per bot: bAccept allows 2 connections per IP
        let _ = ev.send(Ev::Accept { conn, ip: format!("bot-{n}").into_bytes(), tx });
        let mut b = body(MSGID_REQUEST_INITPLAYER, 0, &[(ch.as_bytes(), 10), (acc.as_bytes(), 10), (pwd, 10), (b"", 1), (b"WS1", 20)], 58);
        b.truncate(58);
        let _ = ev.send(Ev::Read { conn, key: 0, body: b });
        while rx.recv().await.is_some() {} // the game's answers are not read: the brain looks at the world itself
        log(&format!("(bot) {ch}: connection closed by the game, back in {} ms", cfg.relogin_ms));
        tokio::time::sleep(Duration::from_millis(cfg.relogin_ms)).await;
    }
}

// ------------------------------------------------------------------ path finding

fn cheb(a: (i16, i16), b: (i16, i16)) -> i16 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// A* on the 8-connected grid inside `limit` tiles of `start`: the steps (start excluded) to the first tile within
/// `reach` of `goal` (Chebyshev), or None. `walk(x, y)` says if a tile can be entered.
pub fn find_path(start: (i16, i16), goal: (i16, i16), reach: i16, limit: i16, walk: impl Fn(i16, i16) -> bool) -> Option<Vec<(i16, i16)>> {
    if cheb(start, goal) <= reach {
        return Some(Vec::new());
    }
    let w = (2 * limit + 1) as i32;
    let idx = |p: (i16, i16)| -> Option<usize> {
        let (dx, dy) = (p.0 - start.0 + limit, p.1 - start.1 + limit);
        ((0..w as i16).contains(&dx) && (0..w as i16).contains(&dy)).then(|| (dy as i32 * w + dx as i32) as usize)
    };
    let n = (w * w) as usize;
    let (mut g, mut from, mut done) = (vec![i32::MAX; n], vec![usize::MAX; n], vec![false; n]);
    let at = |i: usize| (start.0 - limit + (i as i32 % w) as i16, start.1 - limit + (i as i32 / w) as i16);
    let h = |p: (i16, i16)| (cheb(p, goal) - reach).max(0) as i32;
    let s = idx(start)?;
    g[s] = 0;
    let mut open = BinaryHeap::new();
    open.push(Reverse((h(start), s)));
    while let Some(Reverse((_, i))) = open.pop() {
        if std::mem::replace(&mut done[i], true) {
            continue;
        }
        let p = at(i);
        if cheb(p, goal) <= reach {
            let mut path = vec![p];
            let mut k = i;
            while from[k] != s {
                k = from[k];
                path.push(at(k));
            }
            path.reverse();
            return Some(path);
        }
        for (dx, dy) in [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)] {
            let q = (p.0 + dx, p.1 + dy);
            let Some(j) = idx(q) else { continue };
            if done[j] || !walk(q.0, q.1) {
                continue;
            }
            // a diagonal step costs a little more, so straight lines win ties
            let c = g[i] + if dx != 0 && dy != 0 { 3 } else { 2 };
            if c < g[j] {
                (g[j], from[j]) = (c, i);
                open.push(Reverse((c + 2 * h(q), j)));
            }
        }
    }
    None
}

fn dir_to(from: (i16, i16), to: (i16, i16)) -> u8 {
    match ((to.0 - from.0).signum(), (to.1 - from.1).signum()) {
        (0, -1) => 1,
        (1, -1) => 2,
        (1, 0) => 3,
        (1, 1) => 4,
        (0, 1) => 5,
        (-1, 1) => 6,
        (-1, 0) => 7,
        (-1, -1) => 8,
        _ => 0,
    }
}

// ------------------------------------------------------------------ frames a client would send

/// COMMAND_MOTION: command, the position the client believes it has, direction, clock, then (attacks) the target
/// tile, attack type and target id.
fn motion(cmd: i32, pos: (i16, i16), dir: u8, now: u32, attack: Option<(i16, i16, i16, u16)>) -> Vec<u8> {
    let mut b = vec![0u8; 25];
    let mut w = W(&mut b);
    w.u32(0, MSGID_COMMAND_MOTION);
    w.u16(4, cmd as u16);
    w.u16(6, pos.0 as u16);
    w.u16(8, pos.1 as u16);
    w.u8(10, dir);
    w.u32(11, now);
    if let Some((dx, dy, ty, target)) = attack {
        w.u16(15, dx as u16);
        w.u16(17, dy as u16);
        w.u16(19, ty as u16);
        w.u16(21, target);
    }
    b
}

/// COMMAND_COMMON with one integer argument (equip / use item: the bag slot).
fn common(cmd: i32, pos: (i16, i16), v1: u32) -> Vec<u8> {
    let mut b = vec![0u8; 57];
    let mut w = W(&mut b);
    w.u32(0, MSGID_COMMAND_COMMON);
    w.u16(4, cmd as u16);
    w.u16(6, pos.0 as u16);
    w.u16(8, pos.1 as u16);
    w.u32(11, v1);
    b
}

fn is_potion(it: &Item) -> bool {
    it.count > 0 && (it.cfg.item_type == ITEMTYPE_USE_DEPLETE || it.cfg.item_type == ITEMTYPE_EAT) && it.cfg.item_effect_type == ITEMEFFECTTYPE_HP
}

fn is_recall_scroll(it: &Item) -> bool {
    it.count > 0 && it.cfg.item_type == ITEMTYPE_USE_DEPLETE && it.cfg.item_effect_type == ITEMEFFECTTYPE_MAGIC && it.cfg.item_effect_value[0] == 1
}

/// What the shops buy from a bot without it missing it: shop category 11-50 (jelly, hides ...), not the supplies
/// it uses, not gold.
fn is_loot(it: &Item) -> bool {
    it.count > 0 && it.cfg.item_type != 1 && (11..=50).contains(&it.cfg.category) && !is_potion(it) && !is_recall_scroll(it) && it.cfg.name.get(..4) != Some(b"Gold")
}

/// Gear (item type 1, shop category 1-50: weapons, armour, cloaks, necklaces) it will never wear: a weapon no better than
/// the one in hand, a piece for a slot already filled, one for a slot it never fills (it only wears head, body, arms,
/// legs and a shield) or one the server refused to equip. Gear for an empty slot stays: `bot_equip` will put it on.
/// A broken piece (0 endurance) is not surplus: the shop repairs it.
fn is_surplus(c: &Client, k: usize, it: &Item, tried: &[usize]) -> bool {
    let pos = it.cfg.equip_pos;
    let eq = |p: i32| c.equip_status.get(p as usize).copied().unwrap_or(-1);
    if it.count == 0 || it.cfg.item_type != 1 || !(1..=50).contains(&it.cfg.category) || it.cur_life_span == 0 || eq(pos) == k as i16 {
        return false;
    }
    if tried.contains(&k) {
        return true;
    }
    match pos {
        EQUIPPOS_RHAND | EQUIPPOS_TWOHAND => {
            let worn = [EQUIPPOS_RHAND, EQUIPPOS_TWOHAND].iter().filter_map(|&p| usize::try_from(eq(p)).ok().and_then(|j| c.items.get(j)?.as_ref()).map(weapon_score)).max();
            worn.is_some_and(|w| weapon_score(it) <= w)
        }
        EQUIPPOS_LHAND => eq(pos) >= 0 || eq(EQUIPPOS_TWOHAND) >= 0,
        EQUIPPOS_HEAD | EQUIPPOS_BODY | EQUIPPOS_ARMS | EQUIPPOS_LEGGINGS => eq(pos) >= 0,
        _ => true,
    }
}

/// A worn piece (or one that broke while worn) at less than `pct` % of its endurance that the shops repair (category
/// 1-10 at the blacksmith, 11-13 and 43-50 at the shop; the server only checks that the bot is on a shop map) and it can pay for.
fn repair_slot(c: &Client, equipped_once: &[usize], done: &[usize], pct: i32, gold: u32) -> Option<usize> {
    c.items.iter().enumerate().position(|(k, it)| {
        let Some(it) = it else { return false };
        let cat = it.cfg.category;
        let eq = c.equip_status.get(it.cfg.equip_pos as usize).copied().unwrap_or(-1);
        it.cfg.item_type == 1
            && ((1..=10).contains(&cat) || (11..=13).contains(&cat) || (43..=50).contains(&cat))
            && it.max_life_span > 0
            && (it.cur_life_span as i32) * 100 < pct * it.max_life_span as i32
            && (eq == k as i16 || (it.cur_life_span == 0 && equipped_once.contains(&k)))
            && !done.contains(&k)
            && repair_price(it) <= gold
    })
}

/// Twice the average damage of a weapon (dice thrown x (range + 1) + 2 x bonus, the small-target values): doubled so
/// that a 1d6 (3.5) beats a 1d5 (3) instead of tying with it after an integer division.
fn weapon_score(it: &Item) -> i32 {
    let v = it.cfg.item_effect_value;
    v[0] * (v[1] + 1) + 2 * v[2]
}

// ------------------------------------------------------------------ brain

impl Game {
    fn bot_send(&mut self, h: usize, body: Vec<u8>) {
        self.queue.push_back(Msg { from: Src::Client(h), key: 0, body });
    }

    /// The bots' turn: each one thinks every `think-ms`.
    pub fn bots_process(&mut self) {
        let now = self.now();
        let think = self.bots.cfg.think_ms;
        for i in 0..self.bots.list.len() {
            if now.wrapping_sub(self.bots.list[i].next_think) as i32 >= 0 {
                let mut b = std::mem::take(&mut self.bots.list[i]);
                b.next_think = now.wrapping_add(think);
                self.bot_think(&mut b, now);
                self.bots.list[i] = b;
            }
        }
    }

    fn bot_handle(&self, ch: &str) -> Option<usize> {
        let name = Game::pad10(ch.as_bytes());
        (1..MAXCLIENTS_).find(|&h| self.clients[h].as_ref().is_some_and(|c| c.is_init_complete && Game::pad10(&c.char_name) == name))
    }

    /// Hostile monster `i` of map `m` that is alive: (x, y, reach).
    fn bot_prey(&self, i: usize, m: usize) -> Option<(i16, i16, i16)> {
        let n = self.npcs.get(i)?.as_ref()?;
        (n.map_index == m && n.side >= SIDE_HOSTILE && !n.is_killed && n.hp > 0).then(|| (n.x, n.y, if n.size < 2 { 1 } else { n.size as i16 }))
    }

    /// First bag slot holding an item `f` likes.
    fn bot_slot(&self, h: usize, f: impl Fn(&Item) -> bool) -> Option<usize> {
        self.clients[h].as_ref()?.items.iter().position(|i| i.as_ref().is_some_and(&f))
    }

    fn bot_think(&mut self, b: &mut Bot, now: u32) {
        let cfg = Arc::clone(&self.bots.cfg);
        let Some(h) = self.bot_handle(&b.ch) else { return };
        let c = self.clients[h].as_mut().unwrap();
        c.time = now; // the keep-alive a connection would give (check_client_response_time kicks after 18 s)
        if c.is_killed {
            if now.wrapping_sub(b.last_restart) > 3000 {
                b.last_restart = now;
                let mut f = vec![0u8; 6];
                W(&mut f).u32(0, MSGID_REQUEST_RESTART);
                self.bot_send(h, f);
                log(&format!("(bot) {} died, restarting", b.ch));
            }
            (b.mode, b.target) = (Mode::Rest, 0);
            b.path.clear();
            return;
        }
        let (pos, m, hp, max_hp) = ((c.x, c.y), c.map_index, c.hp, c.get_max_hp());
        let (home_map, home) = *b.home.get_or_insert((m, pos));
        if pos != b.last_pos {
            (b.last_pos, b.stuck) = (pos, 0);
        }
        if cfg.auto_equip && now.wrapping_sub(b.last_equip) > 3000 {
            b.last_equip = now;
            self.bot_equip(h, b);
        }
        if cfg.status_log_ms > 0 && now.wrapping_sub(b.last_report) > cfg.status_log_ms {
            b.last_report = now;
            log(&format!("(bot) {}: {:?} {}({},{}) hp {hp}/{max_hp} target {} kills {}", b.ch, b.mode, String::from_utf8_lossy(&self.maps[m].name), pos.0, pos.1, b.target, b.kills));
        }
        if b.stuck > 25 {
            // 25 steps sent from the same tile: give up on this target and route
            if b.target != 0 {
                b.ignore.push((b.target, now.wrapping_add(20_000)));
            }
            if let Some(g) = b.loot_goal.take() {
                b.loot_skip.push((g, now.wrapping_add(120_000)));
            }
            (b.target, b.wander, b.stuck) = (0, None, 0);
            b.path.clear();
            return;
        }
        match b.mode {
            Mode::Hunt if m != home_map => {
                log(&format!("(bot) {}: away from home ({}), resting", b.ch, String::from_utf8_lossy(&self.maps[m].name)));
                (b.mode, b.target) = (Mode::Rest, 0);
                b.path.clear();
            }
            Mode::Hunt => self.bot_hunt(h, b, &cfg, (pos, m, hp, max_hp), home, now),
            Mode::Rest if m == home_map => b.mode = Mode::Hunt,
            Mode::Rest => {
                if cfg.auto_return && hp * 100 >= cfg.rest_pct * max_hp {
                    log(&format!("(bot) {}: healed, going home", b.ch));
                    (b.mode, b.no_route_logged) = (Mode::Travel, false);
                    b.path.clear();
                }
            }
            Mode::Travel => self.bot_travel(h, b, &cfg, (pos, m), (home_map, home), now),
            Mode::Shop => self.bot_shop(h, b, &cfg, (pos, m), now),
        }
    }

    /// Equips the best weapon and fills empty armour slots, one item per call; what the server refuses is not retried.
    fn bot_equip(&mut self, h: usize, b: &mut Bot) {
        let Some(c) = self.clients[h].as_ref() else { return };
        let gender = if (1..=3).contains(&c.type_) { 1 } else if (4..=6).contains(&c.type_) { 2 } else { 0 };
        let eq = |pos: i32| c.equip_status.get(pos as usize).copied().unwrap_or(-1);
        let worn = [EQUIPPOS_RHAND, EQUIPPOS_TWOHAND].iter().filter_map(|&p| usize::try_from(eq(p)).ok().and_then(|k| c.items.get(k)?.as_ref()).map(weapon_score)).max().unwrap_or(-1);
        let mut best: Option<(i32, usize)> = None;
        for (k, it) in c.items.iter().enumerate() {
            let Some(it) = it else { continue };
            let pos = it.cfg.equip_pos;
            if it.cfg.item_type != 1 || it.cur_life_span == 0 || b.equip_tried.contains(&k) || eq(pos) == k as i16 {
                continue;
            }
            if (it.attribute & 1 == 0 && it.cfg.level_limit > c.level) || (it.cfg.gender_limit != 0 && it.cfg.gender_limit != gender) || it.weight_of(1) > c.get_str() * 100 {
                continue;
            }
            let score = match pos {
                EQUIPPOS_RHAND | EQUIPPOS_TWOHAND if it.cfg.item_effect_type == ITEMEFFECTTYPE_ATTACK && weapon_score(it) > worn => 1000 + weapon_score(it),
                EQUIPPOS_HEAD | EQUIPPOS_BODY | EQUIPPOS_ARMS | EQUIPPOS_LEGGINGS if eq(pos) < 0 => {
                    let v = &it.cfg.item_effect_value;
                    let have = match v[3] {
                        10 => c.get_str(),
                        11 => c.get_dex(),
                        12 => c.vit,
                        13 => c.get_int(),
                        14 => c.get_mag(),
                        15 => c.range,
                        _ => i32::MAX,
                    };
                    if have < v[4] {
                        continue;
                    }
                    v[0]
                }
                EQUIPPOS_LHAND if eq(pos) < 0 && eq(EQUIPPOS_TWOHAND) < 0 => 0,
                _ => continue,
            };
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, k));
            }
        }
        if let Some((_, k)) = best {
            let name = String::from_utf8_lossy(&c.items[k].as_ref().unwrap().cfg.name).to_string();
            let pos = (c.x, c.y);
            b.equip_tried.push(k);
            log(&format!("(bot) {}: equips {name} (slot {k})", b.ch));
            self.bot_send(h, common(COMMONTYPE_EQUIPITEM, pos, k as u32));
        }
    }

    fn bot_hunt(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, st: ((i16, i16), usize, i32, i32), home: (i16, i16), now: u32) {
        let (pos, m, hp, max_hp) = st;
        // hurt: a red potion; nearly dead with none left: the recall scroll
        let potion = if cfg.use_potions { self.bot_slot(h, is_potion) } else { None };
        if let Some(k) = potion.filter(|_| hp * 100 < cfg.potion_pct * max_hp && now.wrapping_sub(b.last_potion) >= 1200) {
            b.last_potion = now;
            log(&format!("(bot) {}: hp {hp}/{max_hp}: drinks potion (slot {k})", b.ch));
            self.bot_send(h, common(COMMONTYPE_REQ_USEITEM, pos, k as u32));
            return;
        }
        if cfg.use_recall && potion.is_none() && hp * 100 < cfg.recall_pct * max_hp && now.wrapping_sub(b.last_recall) >= 5000 {
            if let Some(k) = self.bot_slot(h, is_recall_scroll) {
                b.last_recall = now;
                log(&format!("(bot) {}: hp {hp}/{max_hp} and no potion: reads a recall scroll", b.ch));
                self.bot_send(h, common(COMMONTYPE_REQ_USEITEM, pos, k as u32));
                return;
            }
        }
        let combat = self.clients[h].as_ref().is_some_and(|c| c.appr[1] as u16 & 0xF000 != 0);
        if !combat {
            if now.wrapping_sub(b.last_toggle) > 1500 {
                b.last_toggle = now;
                self.bot_send(h, common(COMMONTYPE_TOGGLECOMBATMODE, pos, 0));
            }
            return;
        }
        // the target that went down: count it
        if b.target != 0 && self.bot_prey(b.target, m).is_none() {
            if b.attacks > 0 {
                b.kills += 1;
                log(&format!("(bot) {}: target down ({} kills, {} hits sent)", b.ch, b.kills, b.attacks));
            }
            b.target = 0;
            b.path.clear();
        }
        b.ignore.retain(|&(_, t)| now.wrapping_sub(t) as i32 <= 0);
        let tired = hp * 100 < max_hp * cfg.tired_pct;
        let max_dice = self.bot_max_dice(h, cfg);
        // between two fights: supplies first, then what lies about, then the next monster
        if b.target == 0 && !tired && (self.bot_maybe_shop(h, b, cfg, (pos, m), now) || (cfg.loot && self.bot_loot(h, b, cfg, (pos, m), now))) {
            return;
        }
        if b.target == 0 {
            // hurt: only what is already on top of it
            let sight = if tired { 2 } else { cfg.sight };
            let mut best = (i16::MAX, 0);
            for dx in -sight..=sight {
                for dy in -sight..=sight {
                    let (o, ot) = self.maps[m].get_owner(pos.0 + dx, pos.1 + dy);
                    if ot == OWNERTYPE_NPC && o > 0 && !b.ignore.iter().any(|&(i, _)| i == o as usize) && self.bot_prey(o as usize, m).is_some() && self.npcs[o as usize].as_ref().is_some_and(|n| n.hit_dice <= max_dice) && dx.abs().max(dy.abs()) < best.0 {
                        best = (dx.abs().max(dy.abs()), o as usize);
                    }
                }
            }
            b.target = best.1;
            b.attacks = 0;
        }
        if let Some((tx, ty, reach)) = self.bot_prey(b.target, m) {
            b.idle_since = None;
            if cheb(pos, (tx, ty)) <= reach {
                let thp = self.npcs[b.target].as_ref().map_or(0, |n| n.hp);
                if b.attacks == 0 {
                    b.target_hp0 = thp;
                }
                if b.attacks >= cfg.giveup_hits && thp * 100 > b.target_hp0 * cfg.giveup_pct {
                    // still above the line after that many hits (unarmed against a scorpion...): leave it alone for 2 minutes
                    log(&format!("(bot) {}: giving up on npc {} ({thp}/{} HP after {} hits)", b.ch, b.target, b.target_hp0, b.attacks));
                    b.ignore.push((b.target, now.wrapping_add(120_000)));
                    b.target = 0;
                    return;
                }
                if now.wrapping_sub(b.last_attack) >= cfg.attack_ms {
                    b.last_attack = now;
                    b.attacks += 1;
                    let f = motion(OBJECTATTACK, pos, dir_to(pos, (tx, ty)).max(1), now, Some((tx, ty, 1, 10000 + b.target as u16)));
                    self.bot_send(h, f);
                }
            } else {
                self.bot_walk(h, b, cfg, (pos, m), (tx, ty), reach, None, cfg.path_limit, now);
            }
        } else if !tired {
            let idle = *b.idle_since.get_or_insert(now);
            if cfg.roam && now.wrapping_sub(idle) > cfg.roam_idle_ms && now.wrapping_sub(b.last_roam) > 20_000 {
                b.last_roam = now;
                if self.bot_roam(b, (pos, m), max_dice) {
                    return;
                }
            }
            if b.wander.is_none_or(|w| w == pos) {
                let r = cfg.wander_radius;
                let (dx, dy) = (self.dice(1, 2 * r as i32 + 1) as i16 - r - 1, self.dice(1, 2 * r as i32 + 1) as i16 - r - 1);
                b.wander = self.maps[m].get_moveable(home.0 + dx, home.1 + dy).0.then_some((home.0 + dx, home.1 + dy));
            }
            if let Some(w) = b.wander {
                self.bot_walk(h, b, cfg, (pos, m), w, 0, None, cfg.path_limit, now);
            }
        }
    }

    /// The highest HitDice of the monsters it fights (`hunt-level-step`): its level / step above 2, slimes at level 1.
    fn bot_max_dice(&self, h: usize, cfg: &BotCfg) -> i32 {
        let level = self.clients[h].as_ref().map_or(1, |c| c.level);
        if cfg.hunt_level_step > 0 { 2 + level / cfg.hunt_level_step } else { i32::MAX }
    }

    /// Nothing to fight in sight for a while: picks a hunting ground (one of the nearest live monsters it can beat, on
    /// this map or the nearest map of this server that has some), makes it the new home and goes there. False: none.
    fn bot_roam(&mut self, b: &mut Bot, at: ((i16, i16), usize), max_dice: i32) -> bool {
        let (pos, m) = at;
        let prey: Vec<(usize, (i16, i16))> = self.npcs.iter().flatten().filter(|n| n.side >= SIDE_HOSTILE && !n.is_killed && n.hp > 0 && n.hit_dice <= max_dice).map(|n| (n.map_index, (n.x, n.y))).collect();
        let mut here: Vec<(i16, i16)> = prey.iter().filter(|&&(pm, t)| pm == m && cheb(t, pos) > 2).map(|&(_, t)| t).collect();
        here.sort_by_key(|&t| cheb(t, pos));
        here.truncate(5); // several bots: not all of them to the same monster
        let ground = if !here.is_empty() {
            let k = self.dice(1, here.len() as i32) as usize - 1;
            Some((m, here[k]))
        } else {
            let mut has = vec![false; self.maps.len()];
            prey.iter().for_each(|&(pm, _)| has[pm] = true);
            let d = self.bot_route(m, &|d| has[d], pos).map(|(_, d)| d);
            let there: Vec<(i16, i16)> = prey.iter().filter(|&&(pm, _)| Some(pm) == d).map(|&(_, t)| t).collect();
            (!there.is_empty()).then(|| (d.unwrap(), there[self.dice(1, there.len() as i32) as usize - 1]))
        };
        let Some((gm, gt)) = ground else {
            log(&format!("(bot) {}: no monster it can beat on any map it can reach", b.ch));
            return false;
        };
        log(&format!("(bot) {}: no monster in sight, new hunting ground {}({},{})", b.ch, String::from_utf8_lossy(&self.maps[gm].name), gt.0, gt.1));
        (b.home, b.mode, b.no_route_logged, b.wander, b.idle_since) = (Some((gm, gt)), Mode::Travel, false, None, None);
        b.path.clear();
        true
    }

    /// Back to the home tile: through the teleport tiles of this server's maps, then walking.
    fn bot_travel(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), home: (usize, (i16, i16)), now: u32) {
        let (pos, m) = at;
        if m == home.0 {
            if cheb(pos, home.1) <= 2 {
                log(&format!("(bot) {}: home again", b.ch));
                (b.mode, b.target) = (Mode::Hunt, 0);
                b.path.clear();
            } else {
                self.bot_walk(h, b, cfg, at, home.1, 2, None, (cheb(pos, home.1) + 8).clamp(TRAVEL_LIMIT, 250), now);
            }
        } else if !self.bot_goto(h, b, cfg, at, &|d| d == home.0, now) && !b.no_route_logged {
            b.no_route_logged = true;
            log(&format!("(bot) {}: no teleport route from {} to {}, staying put", b.ch, String::from_utf8_lossy(&self.maps[m].name), String::from_utf8_lossy(&self.maps[home.0].name)));
        }
    }

    /// One step towards a map `to` likes: walk to the right teleport tile, then ask for the teleport. False: no route.
    fn bot_goto(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), to: &dyn Fn(usize) -> bool, now: u32) -> bool {
        let (pos, m) = at;
        let Some((src, _)) = self.bot_route(m, to, pos) else { return false };
        if pos == src {
            if now.wrapping_sub(b.last_teleport) >= 2000 {
                b.last_teleport = now;
                let mut f = vec![0u8; 6];
                W(&mut f).u32(0, MSGID_REQUEST_TELEPORT);
                W(&mut f).u16(4, MSGTYPE_CONFIRM as u16);
                self.bot_send(h, f);
            }
        } else {
            self.bot_walk(h, b, cfg, at, src, 0, Some(src), (cheb(pos, src) + 8).clamp(TRAVEL_LIMIT, 250), now);
        }
        true
    }

    /// The teleport tile of map `from` (nearest to `pos`) that starts the shortest chain of this server's maps to one
    /// `to` accepts, and that last map.
    fn bot_route(&self, from: usize, to: &dyn Fn(usize) -> bool, pos: (i16, i16)) -> Option<((i16, i16), usize)> {
        let mut first = vec![usize::MAX; self.maps.len()];
        let mut seen = vec![false; self.maps.len()];
        seen[from] = true;
        let mut q = VecDeque::from([from]);
        while let Some(a) = q.pop_front() {
            for t in &self.maps[a].teleport_locs {
                let Some(d) = self.map_index(&t.dest_map) else { continue };
                if seen[d] {
                    continue;
                }
                seen[d] = true;
                first[d] = if a == from { d } else { first[a] };
                if to(d) {
                    let hop = first[d];
                    let src = self.maps[from].teleport_locs.iter().filter(|t| self.map_index(&t.dest_map) == Some(hop)).map(|t| t.src).min_by_key(|&s| cheb(s, pos))?;
                    return Some((src, d));
                }
                q.push_back(d);
            }
        }
        None
    }

    fn bot_count(&self, h: usize, f: impl Fn(&Item) -> bool) -> u32 {
        self.clients[h].as_ref().map_or(0, |c| c.items.iter().flatten().filter(|i| f(i)).map(|i| i.count).sum())
    }

    /// Starts a shopping trip when supplies are low (or the bag is full of loot) and it could pay for something.
    fn bot_maybe_shop(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), now: u32) -> bool {
        if !cfg.shop || now.wrapping_sub(b.last_shop) < cfg.shop_cooldown_ms {
            return false;
        }
        let (pos, m) = at;
        let (potions, scrolls) = (self.bot_count(h, is_potion), self.bot_count(h, is_recall_scroll));
        let bag = self.clients[h].as_ref().map_or(0, |c| c.items.iter().flatten().count());
        let gold = self.item_count(h, b"Gold").1;
        let worn = cfg.repair && self.clients[h].as_ref().is_some_and(|c| repair_slot(c, &b.equip_tried, &[], cfg.repair_pct, gold).is_some());
        let low = (cfg.use_potions && potions < cfg.shop_potion_min) || (cfg.use_recall && scrolls < cfg.shop_scroll_min) || worn;
        let full = cfg.sell_loot && bag >= cfg.shop_bag_slots;
        if !low && !full {
            return false;
        }
        let neutral = self.clients[h].as_ref().is_some_and(|c| c.is_neutral());
        let mut wealth = self.item_count(h, b"Gold").1 as i64;
        if cfg.sell_loot {
            wealth += self.clients[h].as_ref().map_or(0, |c| c.items.iter().enumerate().filter_map(|(k, i)| i.as_ref().filter(|i| is_loot(i) || is_surplus(c, k, i, &b.equip_tried))).map(|i| sell_price(i, i.count as i32, neutral).unwrap_or(0) as i64).sum::<i64>());
        }
        let cheapest = self.data.item_by_name(cfg.shop_potion_item.as_bytes()).map_or(30, |i| i.price as i64);
        b.last_shop = now; // whatever happens below, the next look is after the cool-down
        if !full && wealth < cheapest {
            log(&format!("(bot) {}: low on supplies ({potions} potions, {scrolls} scrolls) but poor (~{wealth} gold)", b.ch));
            return false;
        }
        let Some((_, shop)) = self.bot_route(m, &|d| self.maps[d].name.starts_with(b"gshop"), pos) else {
            log(&format!("(bot) {}: no shop (gshop*) reachable from {}", b.ch, String::from_utf8_lossy(&self.maps[m].name)));
            return false;
        };
        log(&format!("(bot) {}: shopping trip to {} ({potions} potions, {scrolls} scrolls, {bag} items, ~{wealth} gold)", b.ch, String::from_utf8_lossy(&self.maps[shop].name)));
        (b.mode, b.shop_map, b.shop_start, b.buy_fails, b.last_have, b.target) = (Mode::Shop, shop, now, 0, 0, 0);
        (b.sell_tried, b.repair_tried) = (Vec::new(), Vec::new());
        b.path.clear();
        true
    }

    /// The shopping trip: go to the shop map, sell the loot one item at a time, buy up to the targets, then go home.
    fn bot_shop(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), now: u32) {
        let (pos, m) = at;
        if now.wrapping_sub(b.shop_start) > 180_000 {
            log(&format!("(bot) {}: the shopping trip takes too long, going home", b.ch));
            (b.mode, b.no_route_logged, b.last_shop) = (Mode::Travel, false, now);
            b.path.clear();
            return;
        }
        let shop = b.shop_map;
        if m != shop {
            if !self.bot_goto(h, b, cfg, at, &|d| d == shop, now) {
                log(&format!("(bot) {}: lost the way to the shop, going home", b.ch));
                (b.mode, b.no_route_logged) = (Mode::Travel, false);
            }
            return;
        }
        if now.wrapping_sub(b.last_trade) < 600 {
            return;
        }
        if cfg.sell_loot {
            let c = self.clients[h].as_ref().unwrap();
            let slot = c.items.iter().enumerate().find(|(k, i)| i.as_ref().is_some_and(|i| (is_loot(i) || is_surplus(c, *k, i, &b.equip_tried)) && !b.sell_tried.contains(&(*k, i.count))));
            if let Some((k, Some(it))) = slot {
                let n = it.count;
                b.sell_tried.push((k, n));
                b.last_trade = now;
                log(&format!("(bot) {}: sells {} x{n}", b.ch, String::from_utf8_lossy(&it.cfg.name)));
                let mut f = common(COMMONTYPE_REQ_SELLITEMCONFIRM, pos, k as u32);
                W(&mut f).u32(15, n);
                self.bot_send(h, f);
                return;
            }
        }
        let gold = self.item_count(h, b"Gold").1;
        let wants = [(cfg.use_potions, &cfg.shop_potion_item, cfg.shop_potion_target, self.bot_count(h, is_potion)), (cfg.use_recall, &cfg.shop_scroll_item, cfg.shop_scroll_target, self.bot_count(h, is_recall_scroll))];
        let have_all: u32 = wants.iter().map(|w| w.3).sum();
        if have_all > b.last_have {
            b.buy_fails = 0;
        }
        b.last_have = have_all;
        for (on, name, target, have) in wants {
            let Some(price) = self.data.item_by_name(name.as_bytes()).map(|i| i.price as u32).filter(|&p| p > 0) else { continue };
            let n = target.saturating_sub(have).min(gold / price);
            if on && n > 0 && b.buy_fails < 3 {
                b.buy_fails += 1;
                b.last_trade = now;
                log(&format!("(bot) {}: buys {n} {name} ({gold} gold)", b.ch));
                let mut f = common(COMMONTYPE_REQ_PURCHASEITEM, pos, n);
                W(&mut f).bytes(23, name.as_bytes());
                self.bot_send(h, f);
                return;
            }
        }
        if cfg.repair {
            let c = self.clients[h].as_ref().unwrap();
            if let Some(k) = repair_slot(c, &b.equip_tried, &b.repair_tried, cfg.repair_pct, gold) {
                log(&format!("(bot) {}: repairs {} (slot {k}, {} gold)", b.ch, String::from_utf8_lossy(&c.items[k].as_ref().unwrap().cfg.name), repair_price(c.items[k].as_ref().unwrap())));
                b.repair_tried.push(k);
                b.equip_tried.retain(|&t| t != k); // a repaired weapon may be worn again
                b.last_trade = now;
                self.bot_send(h, common(COMMONTYPE_REQ_REPAIRITEMCONFIRM, pos, k as u32));
                return;
            }
        }
        log(&format!("(bot) {}: shopping done ({} potions, {} scrolls, {gold} gold), going home", b.ch, self.bot_count(h, is_potion), self.bot_count(h, is_recall_scroll)));
        (b.mode, b.no_route_logged, b.last_shop) = (Mode::Travel, false, now);
        b.path.clear();
    }

    /// Picks up what lies near it: walks to the closest tile with items and sends the get-item command until it is empty
    /// (a tile it cannot clear in 4 tries, or reach, is left alone for 2 minutes). False: nothing to pick up.
    fn bot_loot(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), now: u32) -> bool {
        let (pos, m) = at;
        b.loot_skip.retain(|&(_, t)| now.wrapping_sub(t) as i32 <= 0);
        let skip = &b.loot_skip;
        let goal = self.ground.iter().filter(|(&(gm, x, y), v)| gm == m && !v.is_empty() && cheb((x, y), pos) <= cfg.loot_radius && !skip.iter().any(|&(t, _)| t == (x, y))).map(|(&(_, x, y), v)| ((x, y), v.len())).min_by_key(|&(t, _)| cheb(t, pos));
        let Some((g, n)) = goal else {
            b.loot_goal = None;
            return false;
        };
        if b.loot_goal != Some(g) {
            (b.loot_goal, b.loot_tries, b.loot_last_n) = (Some(g), 0, n);
            log(&format!("(bot) {}: loot at {g:?} ({n} items)", b.ch));
        }
        if n != b.loot_last_n {
            (b.loot_tries, b.loot_last_n) = (0, n); // something was picked up: the tries start again
        }
        if pos != g {
            if !self.bot_walk(h, b, cfg, at, g, 0, None, cfg.path_limit, now) {
                b.loot_skip.push((g, now.wrapping_add(120_000)));
            }
        } else if now.wrapping_sub(b.last_attack) >= cfg.attack_ms {
            b.last_attack = now;
            b.loot_tries += 1;
            if b.loot_tries > 4 {
                b.loot_skip.push((g, now.wrapping_add(120_000)));
                b.loot_goal = None;
            } else {
                self.bot_send(h, motion(OBJECTGETITEM, pos, 5, now, None));
            }
        }
        true
    }

    /// One step along the route to within `reach` of `goal`, planning it first when there is none. `allow` is a tile that
    /// may be entered although it is a teleport tile.
    fn bot_walk(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), goal: (i16, i16), reach: i16, allow: Option<(i16, i16)>, limit: i16, now: u32) -> bool {
        let (pos, m) = at;
        if let Some(k) = b.path.iter().position(|&p| p == pos) {
            b.path.drain(..=k);
        }
        let adjacent = |p: (i16, i16)| cheb(p, pos) == 1;
        let stale = b.path_goal != goal || b.path.first().is_none_or(|&p| !adjacent(p) || !self.maps[m].get_moveable(p.0, p.1).0);
        if stale {
            let map = &self.maps[m];
            let path = find_path(pos, goal, reach, limit, |x, y| map.get_moveable(x, y).0 && (!map.is_teleport(x, y) || allow == Some((x, y))));
            match path {
                Some(p) if !p.is_empty() => (b.path, b.path_goal) = (p, goal),
                _ => {
                    if b.mode == Mode::Travel && !b.no_route_logged {
                        b.no_route_logged = true;
                        log(&format!("(bot) {}: no walkable path from ({},{}) to {goal:?} (limit {limit})", b.ch, pos.0, pos.1));
                    }
                    (b.target, b.wander) = (0, None);
                    b.path.clear();
                    return false;
                }
            }
        }
        if now.wrapping_sub(b.last_step) >= cfg.step_ms {
            b.last_step = now;
            b.stuck += 1;
            let f = motion(OBJECTMOVE, pos, dir_to(pos, b.path[0]), now, None);
            self.bot_send(h, f);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_goes_around_a_wall() {
        // a wall at x = 5 from y = -3 to 3, open elsewhere: the route from (0, 0) to next to (10, 0) must bend
        let walk = |x: i16, y: i16| !(x == 5 && (-3..=3).contains(&y));
        let p = find_path((0, 0), (10, 0), 1, 20, walk).unwrap();
        let l = *p.last().unwrap();
        assert!((l.0 - 10).abs().max(l.1.abs()) <= 1);
        assert!(p.iter().all(|&(x, y)| walk(x, y)));
        assert!(p.iter().any(|&(_, y)| y.abs() >= 4));
        // every step is one tile
        assert!(p.windows(2).all(|w| (w[0].0 - w[1].0).abs().max((w[0].1 - w[1].1).abs()) == 1));
        // a closed box has no route
        assert!(find_path((0, 0), (10, 0), 0, 20, |x, y| x.abs() < 3 && y.abs() < 3).is_none());
        assert_eq!(find_path((0, 0), (1, 1), 1, 5, walk), Some(vec![]));
    }

    #[test]
    fn path_reaches_a_far_goal_with_a_grown_window() {
        // what bot_travel asks for: the window grows to cheb(start, goal) + 8 (capped at 250)
        let p = find_path((0, 0), (187, 130), 2, 195, |x, y| !(x == 90 && y > -150 && y < 100)).unwrap();
        assert!(p.len() >= 185 && p.len() < 200);
        assert!(cheb(*p.last().unwrap(), (187, 130)) <= 2);
        // outside a too small window it has no route
        assert!(find_path((0, 0), (187, 130), 2, 90, |_, _| true).is_none());
    }

    #[test]
    fn cfg_parses_clamps_and_warns() {
        let (c, w) = BotCfg::parse("bot-count = 3 // three\nstep-ms = 10\nattack-ms=1000\nbot-login-address = h:1\nauto-equip = off\nbogus = 1\nsight = x\n# note\nbot-name-prefix = LongLongName\n");
        assert_eq!((c.count, c.step_ms, c.attack_ms, c.login.as_str(), c.auto_equip), (3, MIN_STEP_MS, 1000, "h:1", false));
        assert_eq!(c.name_prefix, "LongLong");
        assert_eq!(w.len(), 2); // bogus, sight = x
        // the sample documents every key and parses without warnings into the defaults
        let (s, w) = BotCfg::parse(CFG_SAMPLE);
        assert!(w.is_empty(), "{w:?}");
        let d = BotCfg::default();
        assert_eq!((s.step_ms, s.potion_pct, s.login.as_str()), (d.step_ms, d.potion_pct, "127.0.0.1:4000"));
    }

    #[test]
    fn surplus_gear_is_what_it_will_never_wear() {
        let (tx, _rx) = unbounded_channel();
        let mut c = Client::new(1, 1, tx, b"1.2.3.4".to_vec(), 0);
        let gear = |pos: i32, dmg: i32| {
            let mut it = Item::new(&Default::default());
            (it.cfg.item_type, it.cfg.category, it.cfg.equip_pos, it.cfg.item_effect_value[0], it.cfg.item_effect_value[1]) = (1, 5, pos, dmg, 1);
            (it.max_life_span, it.cur_life_span) = (100, 100);
            it
        };
        c.items[0] = Some(gear(EQUIPPOS_RHAND, 3)); // wielded
        c.items[1] = Some(gear(EQUIPPOS_RHAND, 2)); // worse weapon
        c.items[2] = Some(gear(EQUIPPOS_RHAND, 9)); // better weapon
        c.items[3] = Some(gear(EQUIPPOS_HEAD, 1)); // empty slot
        c.items[4] = Some(gear(EQUIPPOS_BODY, 1)); // slot taken by 5
        c.items[5] = Some(gear(EQUIPPOS_BODY, 1));
        (c.equip_status[EQUIPPOS_RHAND as usize], c.equip_status[EQUIPPOS_BODY as usize]) = (0, 5);
        let sur = |k: usize, tried: &[usize]| is_surplus(&c, k, c.items[k].as_ref().unwrap(), tried);
        assert!(!sur(0, &[]) && sur(1, &[]) && !sur(2, &[]) && !sur(3, &[]) && sur(4, &[]) && !sur(5, &[]));
        assert!(sur(3, &[3]), "a piece the server refused is surplus");
        // a necklace is never worn by the bot; armour (category 11) is not "loot" and goes through the same rules
        let mut neck = gear(EQUIPPOS_NECK, 1);
        neck.cfg.category = 46;
        assert!(is_surplus(&c, 6, &neck, &[]) && !is_loot(&neck));
        // a broken piece is repaired, not sold
        let mut broken = gear(EQUIPPOS_HEAD, 1);
        (broken.max_life_span, broken.cur_life_span) = (100, 0);
        assert!(!is_surplus(&c, 6, &broken, &[]));
    }

    #[test]
    fn repair_picks_worn_and_broken_gear_it_can_pay() {
        let (tx, _rx) = unbounded_channel();
        let mut c = Client::new(1, 1, tx, b"1.2.3.4".to_vec(), 0);
        let mut it = Item::new(&Default::default());
        (it.cfg.item_type, it.cfg.category, it.cfg.equip_pos, it.cfg.price, it.price) = (1, 1, EQUIPPOS_RHAND, 1000, 1000);
        (it.max_life_span, it.cur_life_span) = (100, 20);
        c.items[0] = Some(it.clone());
        c.equip_status[EQUIPPOS_RHAND as usize] = 0;
        assert_eq!(repair_slot(&c, &[], &[], 30, 10_000), Some(0));
        assert_eq!(repair_slot(&c, &[], &[0], 30, 10_000), None, "once per trip");
        assert_eq!(repair_slot(&c, &[], &[], 30, 10), None, "too poor");
        assert_eq!(repair_slot(&c, &[], &[], 10, 10_000), None, "still above the threshold");
        // it broke while worn: the server unequipped it, but the bot remembers wearing it
        c.equip_status[EQUIPPOS_RHAND as usize] = -1;
        it.cur_life_span = 0;
        c.items[0] = Some(it);
        assert_eq!(repair_slot(&c, &[0], &[], 30, 10_000), Some(0));
        assert_eq!(repair_slot(&c, &[], &[], 30, 10_000), None, "a looted broken piece is left alone");
    }
}
