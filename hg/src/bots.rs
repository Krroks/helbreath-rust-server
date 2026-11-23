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
//! tiles of the maps this server runs), `Shop` (a trip to a `gshop*` map: sells, buys supplies, gear and spells, repairs), `Enlist`
//! (takes a nationality in a city) and `Raid` (fights players of the rival nation).
//! Gear: it equips what it carries (the best weapon, empty armour slots). Between two fights it picks up what lies about.
use crate::client::{Client, Item, ARESDEN, ELVINE, SIDE_MAP};
use crate::consts::*;
use crate::game::{From as Src, Game, Msg, MAXCLIENTS_};
use crate::item_use::{ITEMTYPE_EAT, ITEMTYPE_USE_DEPLETE};
use crate::log::log;
use crate::map::OWNERTYPE_NPC;
use crate::magic::Spell;
use crate::shop::{repair_price, sell_price};
use crate::net::{self, Ev};
use crate::util::{Rd, W};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::unbounded_channel;

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
    pub archetypes: Vec<String>,
    pub enlist: bool,
    pub enlist_level: i32,
    pub nation: String,
    pub buy_gear: bool,
    pub gear_reserve: u32,
    pub roam_idle_ms: u32,
    pub hunt_level_step: i32,
    pub cast: bool,
    pub cast_range: i16,
    pub cast_ms: u32,
    pub learn_spells: bool,
    pub party: bool,
    pub pace_mix: [u32; 3],
    pub paused: bool,
    pub chat: bool,
    pub flee: bool,
    pub heal_pct: i32,
    pub arrow_min: u32,
    pub bow_range: i16,
    pub flee_pct: i32,
    pub recover: bool,
    pub recover_high: i32,
    pub chat_greet_pct: i32,
    pub summary_ms: u32,
    pub schedule: bool,
    pub session_min: (u32, u32),
    pub gap_min: (u32, u32),
    pub drop_pct: i32,
    pub drop_check_ms: u32,
    pub party_range: i16,
    pub party_follow: bool,
    pub quests: bool,
    pub guilds: bool,
    pub web_port: u16,
    pub crusade: bool,
    pub quest_minutes: u32,
    pub move_minutes: (u32, u32),
    pub raid: bool,
    pub raid_level: i32,
    pub raid_cooldown_ms: u32,
    pub raid_ms: u32,
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
            archetypes: ["warrior", "knight", "duelist", "mage", "archer", "warrior"].map(String::from).to_vec(),
            enlist: true,
            enlist_level: 5,
            nation: "random".into(),
            buy_gear: true,
            gear_reserve: 60,
            cast: true,
            cast_range: 6,
            cast_ms: 1500,
            learn_spells: true,
            party: true,
            pace_mix: [25, 35, 40],
            paused: false,
            chat: true,
            flee: true,
            heal_pct: 55,
            arrow_min: 40,
            bow_range: 7,
            flee_pct: 25,
            recover: true,
            recover_high: 70,
            chat_greet_pct: 35,
            summary_ms: 300_000,
            schedule: true,
            session_min: (60, 180),
            gap_min: (10, 90),
            drop_pct: 20,
            drop_check_ms: 90_000,
            party_range: 10,
            party_follow: true,
            quests: true,
            guilds: true,
            web_port: 8097,
            crusade: true,
            quest_minutes: 8,
            move_minutes: (20, 60),
            raid: true,
            raid_level: 10,
            raid_cooldown_ms: 600_000,
            raid_ms: 180_000,
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
bot-archetypes       = warrior,knight,duelist,mage,archer,warrior  // comma list, bot N takes entry N (cycling). warrior: str, vit, dex, any weapon;
                                  // knight: vit, str, dex, one-handed weapon and shield; duelist: dex, str, vit, one-handed weapon, no shield;
                                  // mage: int, mag, vit, empty hands once it knows an attack spell. Also the stats and looks a new character is made with
enlist               = 1          // a neutral bot of enlist-level or more walks to its nation's city and takes the nationality
enlist-level         = 5          // (the server asks for 5)
bot-nation           = random     // aresden, elvine or random
buy-gear             = 1          // spends the gold on a better weapon and on armour for empty slots at the shop
gear-reserve         = 60         // gold it keeps for potions when buying gear
cast                 = 1          // a character with an attack spell casts it at monsters within cast-range (mages: archetype mage)
cast-range           = 6
cast-ms              = 1500       // time between two casts
learn-spells         = 1          // buys an attack spell it can learn (Int) at the shop
bot-pace-mix         = 25,35,40   // weights of the paces relaxed, steady and driven: how many bots are slow and easy-going (slower steps and
                                  // blows, a pause after each kill, now and then a few minutes of standing about), middling or as fast as the other keys say
drop-percent         = 20         // every drop-check-ms, between two fights, this % that it drops on the ground one thing it does not care about (loot
drop-check-ms        = 90000      // it would sell, gear it will never wear); 0 = never. Other bots and players may pick it up
bot-pause            = 0          // 1: the bots stand still (and stay connected) until it is 0 again
summary-ms           = 300000     // one line in the log with how many bots do what, their levels and deaths, this often (0 = never)
schedule             = 1          // a bot plays sessions: it logs off after session-minutes and comes back after gap-minutes (more of them at night)
session-minutes      = 60,180     // least and most
gap-minutes          = 10,90
                                  // Bots.cfg is read again when it changes (every 5 s): bot-count, the paces, classes, timings and the rest
                                  // take effect at once (not the Login address, account and name prefix or password)
heal-percent         = 55         // a bot that knows a healing spell casts it on itself or a party mate below this % HP
arrow-min            = 40         // an archer goes shopping for 100 arrows when it has fewer
bow-range            = 7          // an archer shoots monsters up to this far
flee                 = 1          // below flee-percent HP, with no potion or scroll left, it walks away from the monster it fights (12 steps at most)
flee-percent         = 25
recover              = 1          // below tired-percent HP (a mage: below 30 % mana) it stands still, away from monsters, until recover-percent
recover-percent      = 70
chat                 = 1          // answers a greeting, its name or a whisper from a player (not from other bots), and now and then greets one that comes close
chat-greet-percent   = 35         // chance that it says hello to a player that comes within 6 tiles (once in 10 minutes for each)
party                = 1          // bots of the same map and side near each other form parties (they share the exp)
party-range          = 10
party-follow         = 1          // the bots of a party hunt together: they follow the one with the lowest handle and fight what a mate fights
raid                 = 1          // a citizen of raid-level or more now and then spends raid-ms in a map of the rival nation, fighting its players
raid-level           = 10
raid-cooldown-ms     = 600000     // least time between two raids (each check: 1 in 3)
raid-ms              = 180000
move-minutes         = 20,60      // now and then (about this many minutes, least and most) a bot between two fights goes to another hunting map; 0,0 = never
quests               = 1          // a citizen of level 11 or more takes a monster-hunt quest from the city hall (any town tile), hunts it down and goes back for the reward
quest-minutes        = 8          // least time between two quests (a quest that takes 25 minutes is cancelled)
guilds               = 1          // citizens put points in charisma (the Login asks 20 and level 20 for a guild); the first one ready founds a guild in a town, the
                                  // others of its nation join it (the master approves). They skip the ticket item: the server routine that giving the ticket runs is called
crusade              = 1          // during a crusade a citizen of level 10 or more takes the fighter duty and raids the enemy more often
web-port             = 8097       // 127.0.0.1:port serves the bots as JSON (/api/bots, /api/map/<name>) for the page http://127.0.0.1:8080/bots (serve.mjs proxies it); 0 = off; read at start only
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
                ("bot-pace-mix", _) => {
                    let w: Vec<u32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
                    if w.len() == 3 && w.iter().sum::<u32>() > 0 {
                        c.pace_mix = [w[0], w[1], w[2]];
                    } else {
                        warn.push(format!("bot-pace-mix wants three weights like 25,35,40, got '{v}'"));
                    }
                }
                ("drop-percent", Some(x)) => c.drop_pct = x.clamp(0, 100) as i32,
                ("drop-check-ms", Some(x)) => c.drop_check_ms = x.max(1000) as u32,
                ("heal-percent", Some(x)) => c.heal_pct = x.clamp(10, 95) as i32,
                ("arrow-min", Some(x)) => c.arrow_min = x.max(0) as u32,
                ("bow-range", Some(x)) => c.bow_range = x.clamp(2, 10) as i16,
                ("flee-percent", Some(x)) => c.flee_pct = x.clamp(1, 90) as i32,
                ("recover-percent", Some(x)) => c.recover_high = x.clamp(40, 100) as i32,
                ("flee", _) if flag.is_some() => c.flee = flag.unwrap(),
                ("recover", _) if flag.is_some() => c.recover = flag.unwrap(),
                ("chat-greet-percent", Some(x)) => c.chat_greet_pct = x.clamp(0, 100) as i32,
                ("chat", _) if flag.is_some() => c.chat = flag.unwrap(),
                ("summary-ms", Some(x)) => c.summary_ms = x.max(0) as u32,
                ("bot-pause", _) if flag.is_some() => c.paused = flag.unwrap(),
                ("schedule", _) if flag.is_some() => c.schedule = flag.unwrap(),
                ("session-minutes", _) | ("gap-minutes", _) => {
                    let w: Vec<u32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
                    if w.len() == 2 && w[0] >= 1 && w[0] <= w[1] {
                        if k == "session-minutes" { c.session_min = (w[0], w[1]) } else { c.gap_min = (w[0], w[1]) }
                    } else {
                        warn.push(format!("{k} wants two numbers like 60,180 (least, most), got '{v}'"));
                    }
                }
                ("move-minutes", _) => {
                    let w: Vec<u32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
                    if w.len() == 2 && w[0] <= w[1] {
                        c.move_minutes = (w[0], w[1]);
                    } else {
                        warn.push(format!("move-minutes wants two numbers like 20,60, got '{v}'"));
                    }
                }
                ("web-port", Some(x)) => c.web_port = x.clamp(0, 65535) as u16,
                ("guilds", _) if flag.is_some() => c.guilds = flag.unwrap(),
                ("crusade", _) if flag.is_some() => c.crusade = flag.unwrap(),
                ("quest-minutes", Some(x)) => c.quest_minutes = x.max(1) as u32,
                ("quests", _) if flag.is_some() => c.quests = flag.unwrap(),
                ("party-follow", _) if flag.is_some() => c.party_follow = flag.unwrap(),
                ("party-range", Some(x)) => c.party_range = x.clamp(2, 20) as i16,
                ("raid-level", Some(x)) => c.raid_level = x.clamp(1, 180) as i32,
                ("raid-cooldown-ms", Some(x)) => c.raid_cooldown_ms = x.max(0) as u32,
                ("raid-ms", Some(x)) => c.raid_ms = x.max(10_000) as u32,
                ("party", _) if flag.is_some() => c.party = flag.unwrap(),
                ("raid", _) if flag.is_some() => c.raid = flag.unwrap(),
                ("cast-range", Some(x)) => c.cast_range = x.clamp(1, 10) as i16,
                ("cast-ms", Some(x)) => c.cast_ms = x.max(300) as u32,
                ("cast", _) if flag.is_some() => c.cast = flag.unwrap(),
                ("learn-spells", _) if flag.is_some() => c.learn_spells = flag.unwrap(),
                ("enlist-level", Some(x)) => c.enlist_level = x.clamp(1, 180) as i32,
                ("gear-reserve", Some(x)) => c.gear_reserve = x.max(0) as u32,
                ("bot-archetypes", _) => c.archetypes = v.split(',').map(|a| a.trim().to_ascii_lowercase()).filter(|a| !a.is_empty()).collect(),
                ("bot-nation", _) => c.nation = v.to_ascii_lowercase(),
                ("enlist", _) if flag.is_some() => c.enlist = flag.unwrap(),
                ("buy-gear", _) if flag.is_some() => c.buy_gear = flag.unwrap(),
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

/// The archetypes: warrior 0, mage 1, knight 2, duelist 3, archer 4.
fn arch_id(name: &str) -> Option<u8> {
    ["warrior", "mage", "knight", "duelist", "archer"].iter().position(|a| *a == name).map(|i| i as u8)
}

/// Archetype of bot `i` (1-based): entry `i` of `bot-archetypes`, cycling.
fn arch_of(cfg: &BotCfg, i: usize) -> u8 {
    cfg.archetypes.get((i - 1) % cfg.archetypes.len().max(1)).and_then(|a| arch_id(a)).unwrap_or(0)
}

/// Where the three level-up points go (stat index: str 0, vit 1, dex 2, int 3, mag 4), cycling.
const POINTS: [[usize; 4]; 5] = [[0, 1, 0, 2], [3, 4, 3, 1], [1, 0, 1, 2], [2, 0, 2, 1], [2, 0, 2, 1]];

/// The stats a new character starts with (str, vit, dex, int, mag, agi: 10-14 each, 70 in all).
const START_STATS: [[u8; 6]; 5] = [[14, 12, 12, 10, 10, 12], [10, 10, 10, 14, 14, 12], [13, 14, 11, 10, 10, 12], [12, 10, 14, 10, 10, 14], [11, 10, 14, 10, 11, 14]];

/// The looks a new character is made with, spread by a hash of the bot's number: gender 1-2, skin 1-3, hair style 0-7,
/// hair colour 0-15, underwear colour 0-7.
fn look_of(i: usize) -> [u8; 5] {
    let h = (i as u32).wrapping_mul(2_654_435_761) ^ 0x9E37_79B9;
    let h = h ^ (h >> 15);
    let h = h.wrapping_mul(2_246_822_519);
    [1 + (h >> 28) as u8 % 2, 1 + (h >> 8) as u8 % 3, (h >> 12) as u8 % 8, (h >> 16) as u8 % 16, (h >> 20) as u8 % 8]
}

/// What the bot's class lets it wear: a mage that can cast has empty hands, a knight or a duelist a one-handed weapon,
/// a duelist no shield.
#[derive(Clone, Copy, Default)]
struct Role {
    mage: bool,
    one_hand: bool,
    no_shield: bool,
    /// an archer wears a bow and nothing else in the hands; `shoot`: it has the bow on and arrows to shoot (so it keeps no melee weapon)
    archer: bool,
    shoot: bool,
}

/// `ready`: a mage that knows an attack spell, an archer that wears a bow and has arrows.
fn role_of(arch: u8, ready: bool) -> Role {
    Role { mage: arch == 1 && ready, one_hand: matches!(arch, 2 | 3), no_shield: arch == 3, archer: arch == 4, shoot: arch == 4 && ready }
}

impl Role {
    /// Positions it will not fill.
    fn refuses(self, pos: i32) -> bool {
        (self.mage && matches!(pos, EQUIPPOS_RHAND | EQUIPPOS_TWOHAND | EQUIPPOS_LHAND)) || (self.one_hand && pos == EQUIPPOS_TWOHAND) || (self.no_shield && pos == EQUIPPOS_LHAND) || (self.archer && pos == EQUIPPOS_LHAND) || (self.shoot && pos == EQUIPPOS_RHAND)
    }
}

/// How a bot plays: multipliers of `step-ms`, `attack-ms` and `sight`, the pause after a kill (min, max ms) and the
/// breaks (about every `break_every` ms it stands about for about `break_len` ms; 0 = never). Each bot then gets +-15 %.
struct Pace {
    name: &'static str,
    step: f32,
    attack: f32,
    sight: f32,
    react: (u32, u32),
    break_every: u32,
    break_len: u32,
}

const PACES: [Pace; 3] = [
    Pace { name: "relaxed", step: 1.7, attack: 1.5, sight: 0.7, react: (2000, 7000), break_every: 240_000, break_len: 45_000 },
    Pace { name: "steady", step: 1.3, attack: 1.2, sight: 0.85, react: (600, 3000), break_every: 600_000, break_len: 25_000 },
    Pace { name: "driven", step: 1.0, attack: 1.0, sight: 1.0, react: (0, 0), break_every: 0, break_len: 0 },
];

#[derive(Default, Clone, Copy, PartialEq, Debug)]
enum Mode {
    #[default]
    Hunt,
    Rest,
    Travel,
    Shop,
    Enlist,
    Raid,
    Quest,
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
    /// the archetype (`arch_id`): where the level-up points go (and `pt_pos`: the next point of the cycle) and what it wears
    arch: u8,
    pt_pos: usize,
    last_points: u32,
    /// enlisting: the city map, tries, the last attempt
    city: Option<usize>,
    enlist_tries: u32,
    last_enlist: u32,
    /// the HitDice it fights now, and the highest it was allowed to fight when it last looked for a ground
    ground_dice: i32,
    max_dice_seen: i32,
    wish_tried: Vec<Vec<u8>>,
    wish_buys: u32,
    last_cast: u32,
    /// party: who it invited, since when and whether the invitation went out; the last invitation, the last accept
    invite: Option<(String, u32, bool)>,
    last_invite: u32,
    last_accept: u32,
    /// raid: the last one, when this one started, its map, the player in sight and the kills
    last_raid: u32,
    raid_start: u32,
    raid_map: usize,
    raid_target: usize,
    raid_kills: u32,
    /// player handles it gave up on during this raid, until a time
    raid_ignore: Vec<(usize, u32)>,
    last_level: i32,
    /// its number (0-based), whether it was connected at the last look, when its session ends and what it is called
    n: usize,
    online: bool,
    session_until: u32,
    deaths: u32,
    last_chat: u32,
    /// the last time it showed the typing bubble before a chat line
    last_typing: u32,
    last_guild: u32,
    /// quest: 0 going to a town, 1 asking for it, 2 hunting, 3 back to hand it in; when it started, the last ask, the monster type it must kill (0 = any)
    qphase: u8,
    quest_start: u32,
    last_quest: u32,
    last_ask: u32,
    quest_type: i32,
    quest_asks: u32,
    shop_cd: u32,
    next_move: u32,
    last_follow: u32,
    accepts: (u32, String),
    recovering: bool,
    flee_steps: u32,
    last_greet_look: u32,
    greeted: Vec<(String, u32)>,
    last_drop_check: u32,
    /// its pace (index in PACES) and what that makes of the timings
    pace: usize,
    step_ms: u32,
    attack_ms: u32,
    sight: i16,
    react: (u32, u32),
    break_every: u32,
    break_len: u32,
    react_until: u32,
    next_break: u32,
    pause_until: u32,
    pause_hp: i32,
    spell_tried: Vec<Vec<u8>>,
    /// the bag slot of the last equip command and when: it is not sold before the server has worn it
    last_equip_slot: (usize, u32),
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

/// What the web page shows of a bot that is offline: how it was when it was last seen.
#[derive(Clone, Default)]
struct BotView {
    level: i32,
    exp: u32,
    side: i32,
    guild: String,
    rank: i32,
    map: String,
    x: i16,
    y: i16,
    seen: u64,
}

struct ChatLine {
    t: u64,
    map: String,
    from: String,
    to: String,
    text: String,
    bot: bool,
}

#[derive(Default)]
pub struct Bots {
    pub list: Vec<Bot>,
    pub cfg: Arc<BotCfg>,
    /// bots that were asked to put the weapons away (combat mode off) until a time: a party invitation needs it
    pub peace: Vec<(String, u32)>,
    /// for the web page: the last known look of each bot, the chat of the last hour (bots and the players that talk to them) and the published JSON
    views: Vec<BotView>,
    chat_log: std::collections::VecDeque<ChatLine>,
    pub web: Option<crate::bots_web::Web>,
    next_publish: u32,
    /// what it is going to say: bot number, whisper to (the player's name), the text, when
    chat_q: Vec<(usize, Option<String>, String, u32)>,
    /// to start the tasks of bots added while it runs; where the config file is, when it was last read, the next look
    pub ev: Option<net::EvTx>,
    pub cfg_path: String,
    pub cfg_mtime: Option<std::time::SystemTime>,
    next_reload: u32,
    next_summary: u32,
    /// `bots pause` from the console (the config key `bot-pause` does the same)
    paused: bool,
}

impl Bot {
    /// Bot number `i` (1-based) of `cfg`: its names, class and pace (the pace is a slot of 0..100 spread by a
    /// multiplicative hash that the mix cuts), then +-15 % on each timing.
    fn made(cfg: &BotCfg, i: usize) -> Bot {
        let mut b = Bot { acc: format!("{}{i:02}", cfg.account_prefix), ch: format!("{}{i:02}", cfg.name_prefix), n: i - 1, ..Default::default() };
        b.tune(cfg);
        b
    }

    /// (Re)sets what comes from the config: class, pace and timings.
    fn tune(&mut self, cfg: &BotCfg) {
        let i = self.n + 1;
        let total: u32 = cfg.pace_mix.iter().sum();
        let slot = (i as u32).wrapping_mul(2_654_435_761) % 100 * total / 100;
        let pace = if slot < cfg.pace_mix[0] { 0 } else if slot < cfg.pace_mix[0] + cfg.pace_mix[1] { 1 } else { 2 };
        let p = &PACES[pace];
        let j = |k: u32| 0.85 + (((i as u32).wrapping_mul(k).wrapping_mul(2_246_822_519) >> 8) % 31) as f32 / 100.0;
        self.arch = arch_of(cfg, i);
        self.pace = pace;
        self.step_ms = ((cfg.step_ms as f32 * p.step * j(3)) as u32).max(MIN_STEP_MS);
        self.attack_ms = ((cfg.attack_ms as f32 * p.attack * j(5)) as u32).max(MIN_ATTACK_MS);
        self.sight = ((cfg.sight as f32 * p.sight * j(7)) as i16).clamp(4, 14);
        self.react = p.react;
        self.break_every = (p.break_every as f32 * j(11)) as u32;
        self.break_len = (p.break_len as f32 * j(13)) as u32;
    }
}

impl Bots {
    pub fn new(cfg: BotCfg) -> Bots {
        let list = (1..=cfg.count).map(|i| Bot::made(&cfg, i)).collect();
        Bots { list, cfg: Arc::new(cfg), ..Default::default() }
    }
}

// ------------------------------------------------------------------ chat

const HI_ES: [&str; 6] = ["hola", "buenas", "hey, que tal", "ola", "hola :)", "ey"];
const HI_EN: [&str; 5] = ["hi", "hey", "hello", "yo", "hi there"];
const HOW_ES: [&str; 5] = ["bien, subiendo nivel", "aqui farmeando", "tranqui, y tu?", "cazando un rato", "pues aqui, a lo mio"];
const HOW_EN: [&str; 4] = ["good, grinding", "just hunting", "chill, you?", "levelling up"];
const HELP_ES: [&str; 3] = ["ahora no puedo, sorry", "estoy liado", "suerte!"];
const HELP_EN: [&str; 3] = ["cant right now, sorry", "busy atm", "good luck!"];
const PARTY_ES: [&str; 3] = ["ahora estoy con mi grupo", "mejor otro dia", "vale, luego"];
const PARTY_EN: [&str; 2] = ["in a party already", "maybe later"];
const ANY_ES: [&str; 6] = ["jaja", "vale", "mm", "ni idea", "si?", "ok"];
const ANY_EN: [&str; 5] = ["lol", "ok", "hm", "no idea", "yeah?"];

/// What a player said, as the words of the text.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// Whether the text is English (a few common words), and which bank answers it: a greeting, a question, a call for help,
/// an invitation to a party, or anything else; `None` when it is not for a bot that was not named or whispered to.
fn chat_kind(text: &str, named: bool, whispered: bool) -> Option<(bool, u8)> {
    let w = words(text);
    let has = |l: &[&str]| w.iter().any(|t| l.contains(&t.as_str()));
    let english = has(&["hi", "hello", "hey", "how", "what", "you", "help", "the", "are", "thanks", "party", "group", "yo"]) && !has(&["hola", "buenas", "que", "tal", "ayuda", "grupo"]);
    let hello = has(&["hola", "buenas", "ola", "saludos", "ey", "hi", "hello", "hey", "yo", "buen"]);
    let kind = if has(&["ayuda", "ayudame", "help", "socorro"]) {
        2
    } else if has(&["party", "grupo", "group", "unete", "invite"]) {
        3
    } else if text.contains('?') || has(&["tal", "haces", "how", "what"]) {
        1
    } else if hello {
        0
    } else {
        4
    };
    (hello || named || whispered).then_some((english, kind))
}

fn li_map(g: &Game, h: usize) -> usize {
    g.clients[h].as_ref().map_or(0, |c| c.map_index)
}

fn pick(bank: &[&'static str], r: i32) -> &'static str {
    bank[(r.max(1) as usize - 1) % bank.len()]
}

// ------------------------------------------------------------------ login task

/// Until when (unix ms) bot `n` must not log in: its time off, or `u64::MAX` when it was taken out by `bot-count`.
static OFFLINE: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

fn set_offline(n: usize, until: u64) {
    let mut v = OFFLINE.lock().unwrap();
    if v.len() <= n {
        v.resize(n + 1, 0);
    }
    v[n] = until;
}

fn offline_until(n: usize) -> u64 {
    OFFLINE.lock().unwrap().get(n).copied().unwrap_or(0)
}

pub fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

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
async fn login(addr: &str, acc: &[u8], ch: &[u8], pwd: &[u8], look: [u8; 5], stats: [u8; 6]) -> bool {
    let f: &[(&[u8], usize)] = &[(acc, 10), (pwd, 10), (b"bot@example.invalid", 50), (b" ", 10), (b"30", 10), (b" ", 4), (b" ", 2), (b" ", 2), (b"Nowhere", 17), (b"", 28), (b"bot quiz?", 45), (b"bot answer", 20), (b"", 50)];
    let _ = login_call(addr, &body(CREATENEWACCOUNT, 0, f, 264)).await;
    let mut b = body(CREATENEWCHARACTER, 0, &[(ch, 10), (acc, 10), (pwd, 10), (b"WS1", 30)], 66);
    b.extend_from_slice(&look);
    b.extend_from_slice(&stats);
    let _ = login_call(addr, &b).await;
    let mut b = body(MSGID_REQUEST_ENTERGAME, ENTERGAMEMSGTYPE_NEW as u16, &[(ch, 10), (b"", 10), (acc, 10), (pwd, 10), (b"WS1", 10)], 56);
    b.extend_from_slice(&1i32.to_le_bytes());
    b.resize(200, 0);
    let Some(r) = login_call(addr, &b).await else { return false };
    let r = Rd::new(&r);
    r.u32(0) == MSGID_RESPONSE_ENTERGAME && r.u16(4) == ENTERGAMERESTYPE_CONFIRM as u16
}

/// Keeps one bot connected: log in, hand the game a fake connection, wait until the game drops it, try again.
pub async fn run_bot(n: usize, cfg: Arc<BotCfg>, acc: String, ch: String, ev: net::EvTx) {
    tokio::time::sleep(Duration::from_millis(cfg.login_spread_ms * n as u64)).await; // spread the logins
    let pwd = cfg.password.as_bytes();
    loop {
        while unix_ms() < offline_until(n) {
            tokio::time::sleep(Duration::from_secs(2)).await; // its time off, or not part of the bots any more
        }
        while !READY.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        if !login(&cfg.login, acc.as_bytes(), ch.as_bytes(), pwd, look_of(n + 1), START_STATS[arch_of(&cfg, n + 1) as usize]).await {
            log(&format!("(bot) {ch}: login refused, retrying"));
            tokio::time::sleep(Duration::from_millis(cfg.relogin_ms)).await;
            continue;
        }
        let conn = net::new_conn_id();
        let (tx, mut rx) = unbounded_channel();
        // a distinct "address" per bot: bAccept allows 2 connections per IP
        let _ = ev.send(Ev::Accept { conn, ip: format!("bot-{n}").into_bytes(), tx }).await;
        let mut b = body(MSGID_REQUEST_INITPLAYER, 0, &[(ch.as_bytes(), 10), (acc.as_bytes(), 10), (pwd, 10), (b"", 1), (b"WS1", 20)], 58);
        b.truncate(58);
        let _ = ev.send(Ev::Read { conn, key: 0, body: b }).await;
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

/// Level-up points the server will take: the pool, within `(level - 1) * 3 + 70` stat points in all (a new character's
/// pool of 3 cannot be spent: its stats already add up to the level 1 limit).
fn c_pool(g: &Game, h: usize) -> i32 {
    g.clients[h].as_ref().map_or(0, |c| c.lu_pool.min((c.level - 1) * 3 + 70 - (c.str_ + c.vit + c.dex + c.int_ + c.mag + c.range)))
}

/// A bow in the bag, worn or not.
fn has_bow(c: &Client) -> bool {
    c.items.iter().flatten().any(|i| i.cfg.item_effect_type == ITEMEFFECTTYPE_ATTACK_ARROW)
}

fn arrows_of(c: &Client) -> u32 {
    c.items.iter().flatten().filter(|i| i.cfg.item_type == crate::combat::ITEMTYPE_ARROW).map(|i| i.count).sum()
}

/// How many red potions it wants to carry at `level`: a trip below the first, a purchase up to the second.
fn potion_wants(cfg: &BotCfg, level: i32) -> (u32, u32) {
    (cfg.shop_potion_min + level.max(0) as u32 / 10, (cfg.shop_potion_target + level.max(0) as u32 / 2).min(60))
}

fn is_recall_scroll(it: &Item) -> bool {
    it.count > 0 && it.cfg.item_type == ITEMTYPE_USE_DEPLETE && it.cfg.item_effect_type == ITEMEFFECTTYPE_MAGIC && it.cfg.item_effect_value[0] == 1
}

/// What the shops buy from a bot without it missing it: shop category 11-50 (jelly, hides ...), not the supplies
/// it uses, not gold.
fn is_loot(it: &Item) -> bool {
    it.count > 0 && it.cfg.item_type != 1 && (11..=50).contains(&it.cfg.category) && !is_potion(it) && !is_recall_scroll(it) && it.cfg.name.get(..4) != Some(b"Gold")
}

/// The bag slots it does not care about: loot a shop would buy and gear it will never wear (the item it has just sent
/// the equip command for is not one yet).
fn junk_slots(c: &Client, b: &Bot, role: Role, now: u32) -> Vec<usize> {
    c.items
        .iter()
        .enumerate()
        .filter(|(k, i)| i.as_ref().is_some_and(|i| (is_loot(i) || is_surplus(c, *k, i, &b.equip_tried, role)) && !(b.last_equip_slot.0 == *k && now.wrapping_sub(b.last_equip_slot.1) < 10_000)))
        .map(|(k, _)| k)
        .collect()
}

/// Gear (item type 1, shop category 1-50: weapons, armour, cloaks, necklaces) it will never wear: a weapon no better than
/// the one in hand, a piece for a slot already filled, one for a slot it never fills (it only wears head, body, arms,
/// legs and a shield) or one the server refused to equip. Gear for an empty slot stays: `bot_equip` will put it on.
/// A broken piece (0 endurance) is not surplus: the shop repairs it.
fn is_surplus(c: &Client, k: usize, it: &Item, tried: &[usize], role: Role) -> bool {
    let pos = it.cfg.equip_pos;
    let eq = |p: i32| c.equip_status.get(p as usize).copied().unwrap_or(-1);
    if it.count == 0 || it.cfg.item_type != 1 || !(1..=50).contains(&it.cfg.category) || it.cur_life_span == 0 || eq(pos) == k as i16 {
        return false;
    }
    let gender = if (1..=3).contains(&c.type_) { 1 } else if (4..=6).contains(&c.type_) { 2 } else { 0 };
    if tried.contains(&k) || role.refuses(pos) || (role.archer && pos == EQUIPPOS_TWOHAND && it.cfg.item_effect_type != ITEMEFFECTTYPE_ATTACK_ARROW) || (it.cfg.gender_limit != 0 && gender != 0 && it.cfg.gender_limit != gender) {
        return true; // e.g. a mage casts with empty hands (the server refuses spells with a weapon or a shield), or it is made for the other gender
    }
    match pos {
        EQUIPPOS_RHAND | EQUIPPOS_TWOHAND => {
            let worn = [EQUIPPOS_RHAND, EQUIPPOS_TWOHAND].iter().filter_map(|&p| usize::try_from(eq(p)).ok().and_then(|j| c.items.get(j)?.as_ref()).map(weapon_score)).max();
            worn.is_some_and(|w| weapon_score(it) <= w)
        }
        EQUIPPOS_LHAND => eq(pos) >= 0 || eq(EQUIPPOS_TWOHAND) >= 0,
        EQUIPPOS_HEAD | EQUIPPOS_BODY | EQUIPPOS_ARMS | EQUIPPOS_PANTS | EQUIPPOS_LEGGINGS => eq(pos) >= 0 && wear_score(c, it, role).is_none(),
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

/// What wearing `it` would bring, or None when the bot cannot or will not: level, gender, weight and stat limits, and
/// only a better weapon, a piece for an empty armour slot or a shield without a two-handed weapon.
fn wear_score(c: &Client, it: &Item, role: Role) -> Option<i32> {
    let gender = if (1..=3).contains(&c.type_) { 1 } else if (4..=6).contains(&c.type_) { 2 } else { 0 };
    if (it.attribute & 1 == 0 && it.cfg.level_limit > c.level) || (it.cfg.gender_limit != 0 && it.cfg.gender_limit != gender) || it.weight_of(1) > c.get_str() * 100 {
        return None;
    }
    let eq = |pos: i32| c.equip_status.get(pos as usize).copied().unwrap_or(-1);
    let pos = it.cfg.equip_pos;
    if role.refuses(pos) {
        return None;
    }
    match pos {
        EQUIPPOS_RHAND | EQUIPPOS_TWOHAND => {
            let held: Vec<&Item> = [EQUIPPOS_RHAND, EQUIPPOS_TWOHAND].iter().filter_map(|&p| usize::try_from(eq(p)).ok().and_then(|k| c.items.get(k)?.as_ref())).collect();
            let worn = held.iter().map(|i| weapon_score(i)).max().unwrap_or(-1);
            let worn_bow = held.iter().any(|i| i.cfg.item_effect_type == ITEMEFFECTTYPE_ATTACK_ARROW);
            match it.cfg.item_effect_type {
                // only an archer wears a bow, and it prefers one to any melee weapon: it only changes it for a better bow
                ITEMEFFECTTYPE_ATTACK_ARROW if role.archer => (!worn_bow || weapon_score(it) > worn).then(|| 2000 + weapon_score(it)),
                // an archer fights with a melee weapon until it has a bow, then keeps the bow
                ITEMEFFECTTYPE_ATTACK if !worn_bow => (weapon_score(it) > worn).then(|| 1000 + weapon_score(it)),
                _ => None,
            }
        }
        EQUIPPOS_HEAD | EQUIPPOS_BODY | EQUIPPOS_ARMS | EQUIPPOS_PANTS | EQUIPPOS_LEGGINGS => {
            let v = &it.cfg.item_effect_value;
            let worn = usize::try_from(eq(pos)).ok().and_then(|k| c.items.get(k)?.as_ref()).map(|i| i.cfg.item_effect_value[0]);
            let have = match v[3] {
                10 => c.get_str(),
                11 => c.get_dex(),
                12 => c.vit,
                13 => c.get_int(),
                14 => c.get_mag(),
                15 => c.range,
                _ => i32::MAX,
            };
            // an empty slot: any piece it can wear; a worn one: a piece at least a fifth and two points better
            (have >= v[4]).then_some(()).and_then(|_| match worn {
                None => Some(v[0]),
                Some(cur) => (v[0] >= cur + 2 && v[0] * 10 >= cur * 12).then_some(v[0] - cur),
            })
        }
        EQUIPPOS_LHAND if eq(pos) < 0 && eq(EQUIPPOS_TWOHAND) < 0 => Some(0),
        _ => None,
    }
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
        self.bots.peace.retain(|(_, t)| now.wrapping_sub(*t) as i32 <= 0);
        if now.wrapping_sub(self.bots.next_reload) as i32 >= 0 {
            self.bots.next_reload = now.wrapping_add(5000);
            self.bots_reload();
        }
        let summary = self.bots.cfg.summary_ms;
        if summary > 0 && now.wrapping_sub(self.bots.next_summary) as i32 >= 0 {
            self.bots.next_summary = now.wrapping_add(summary);
            log(&self.bots_summary());
        }
        self.bots_chat_process(now);
        self.bots_publish(now);
        let paused = self.bots.paused || self.bots.cfg.paused;
        for i in 0..self.bots.list.len() {
            if now.wrapping_sub(self.bots.list[i].next_think) as i32 >= 0 {
                let mut b = std::mem::take(&mut self.bots.list[i]);
                b.next_think = now.wrapping_add(think);
                if paused {
                    if let Some(h) = self.bot_handle(&b.ch) {
                        self.clients[h].as_mut().unwrap().time = now; // still connected, standing there
                    }
                } else {
                    self.bot_think(&mut b, now);
                }
                self.bots.list[i] = b;
            }
        }
    }

    /// Called by the chat handler for each client that receives a line (`mode` 0 normal, 20 whisper): a bot that was greeted,
    /// named or whispered to by a player queues an answer, a few seconds later as a person types it.
    pub fn bots_hear(&mut self, speaker: usize, listener: usize, mode: u8, d: &[u8]) {
        if !self.bots.cfg.chat || self.bots.list.is_empty() || d.len() < 23 {
            return;
        }
        let (Some(sp), Some(li)) = (self.client(speaker), self.client(listener)) else { return };
        let prefix = self.bots.cfg.name_prefix.as_bytes();
        let (sname, lname) = (crate::util::cstr(&sp.char_name), crate::util::cstr(&li.char_name));
        if sname.starts_with(prefix) || !lname.starts_with(prefix) {
            return; // bots do not talk to bots
        }
        let (sname, lname) = (String::from_utf8_lossy(sname).to_string(), String::from_utf8_lossy(lname).to_string());
        let Some(n) = self.bots.list.iter().position(|b| b.ch == lname) else { return };
        let text = String::from_utf8_lossy(crate::util::cstr(&d[21..])).to_string();
        let whisper = mode == 20;
        let map = String::from_utf8_lossy(&self.maps[li_map(self, listener)].name).to_string();
        self.bots_note_chat(&sname, if whisper { &lname } else { "" }, &text, false, &map);
        let Some((english, kind)) = chat_kind(&text, text.to_lowercase().contains(&lname.to_lowercase()), whisper) else { return };
        if self.bots.chat_q.iter().any(|q| q.0 == n) {
            return;
        }
        let r = self.dice(1, 1000);
        let bank: &[&str] = match (kind, english) {
            (0, false) => &HI_ES,
            (0, true) => &HI_EN,
            (1, false) => &HOW_ES,
            (1, true) => &HOW_EN,
            (2, false) => &HELP_ES,
            (2, true) => &HELP_EN,
            (3, false) => &PARTY_ES,
            (3, true) => &PARTY_EN,
            (_, false) => &ANY_ES,
            (_, true) => &ANY_EN,
        };
        let now = self.now();
        let delay = [4000, 2000, 1000][self.bots.list[n].pace] + self.dice(1, 3500) as u32; // the slow ones take longer to type
        self.bots.chat_q.push((n, whisper.then_some(sname), pick(bank, r).to_string(), now.wrapping_add(delay)));
    }

    /// One chat line for the web page (the last 300): `bot` = a bot said it. The same line heard by several bots is kept once.
    fn bots_note_chat(&mut self, from: &str, to: &str, text: &str, bot: bool, map: &str) {
        let t = crate::bots::unix_ms();
        if self.bots.chat_log.iter().rev().take(20).any(|l| l.from == from && l.text == text && t.saturating_sub(l.t) < 1500) {
            return;
        }
        self.bots.chat_log.push_back(ChatLine { t, map: map.to_string(), from: from.to_string(), to: to.to_string(), text: text.to_string(), bot });
        while self.bots.chat_log.len() > 300 {
            self.bots.chat_log.pop_front();
        }
    }

    /// The bot says `text` (nearby, or as a whisper to `to`): the same frame a client sends, through the chat handler.
    fn bot_say(&mut self, h: usize, to: Option<&str>, text: &str) {
        let Some(c) = self.clients[h].as_ref() else { return };
        let mut f = vec![0u8; 22 + text.len().min(80)];
        let mut w = W(&mut f);
        w.u32(0, MSGID_COMMAND_CHATMSG);
        w.u16(6, c.x as u16);
        w.u16(8, c.y as u16);
        w.bytes(10, &Game::pad10(&c.char_name));
        w.bytes(21, &text.as_bytes()[..text.len().min(80)]);
        let to_h = to.and_then(|n| self.handles().into_iter().find(|&i| self.clients[i].as_ref().is_some_and(|o| crate::util::cstr(&o.char_name) == n.as_bytes())));
        if to.is_some() && to_h.is_none() {
            return; // it logged off meanwhile
        }
        if let Some(j) = to_h {
            let name = self.clients[j].as_ref().unwrap().char_name.clone();
            let c = self.clients[h].as_mut().unwrap();
            (c.whisper_player_index, c.whisper_player_name) = (j as i32, crate::util::cstr(&name).to_vec());
        }
        let (from, map) = self.clients[h].as_ref().map_or((String::new(), String::new()), |c| (String::from_utf8_lossy(crate::util::cstr(&c.char_name)).to_string(), String::from_utf8_lossy(&self.maps[c.map_index].name).to_string()));
        self.bots_note_chat(&from, to.unwrap_or(""), text, true, &map);
        self.chat_msg(h, &f);
        self.broadcast_typing(h, false);
        if let Some(c) = self.clients[h].as_mut() {
            (c.whisper_player_index, c.whisper_player_name) = (-1, Vec::new());
        }
    }

    /// The answers that are due, and the greeting of a player that comes close.
    fn bots_chat_process(&mut self, now: u32) {
        let mut i = 0;
        while i < self.bots.chat_q.len() {
            if now.wrapping_sub(self.bots.chat_q[i].3) as i32 >= 0 {
                let (n, to, text, _) = self.bots.chat_q.remove(i);
                let ch = self.bots.list[n].ch.clone();
                if let Some(h) = self.bot_handle(&ch) {
                    let last = self.bots.list[n].last_chat;
                    if now.wrapping_sub(last) > 6000 {
                        self.bots.list[n].last_chat = now;
                        log(&format!("(bot) {ch}: says{} '{text}'", to.as_ref().map_or(String::new(), |t| format!(" to {t}"))));
                        self.bot_say(h, to.as_deref(), &text);
                    } else {
                        // it spoke a moment ago: it waits (nobody types two lines in a second)
                        self.bots.chat_q.insert(i, (n, to, text, last.wrapping_add(6500)));
                        i += 1;
                    }
                }
            } else {
                // v8.00 typing bubble: the line is "typed" for the last moments before it is said (a long line takes longer)
                let (n, due) = (self.bots.chat_q[i].0, self.bots.chat_q[i].3);
                let type_ms = (800 + 60 * self.bots.chat_q[i].2.len() as i32).min(4000);
                if due.wrapping_sub(now) as i32 <= type_ms && now.wrapping_sub(self.bots.list[n].last_typing) > 3500 {
                    self.bots.list[n].last_typing = now;
                    let ch = self.bots.list[n].ch.clone();
                    if let Some(h) = self.bot_handle(&ch) {
                        self.broadcast_typing(h, true);
                    }
                }
                i += 1;
            }
        }
    }

    /// A player (not a bot) within 6 tiles that it has not greeted in 10 minutes: maybe a hello.
    fn bot_greet(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, now: u32) {
        if !cfg.chat || now.wrapping_sub(b.last_greet_look) < 2500 || now.wrapping_sub(b.last_chat) < 20_000 {
            return;
        }
        b.last_greet_look = now;
        b.greeted.retain(|(_, t)| now.wrapping_sub(*t) < 600_000);
        let Some(c) = self.clients[h].as_ref() else { return };
        let (pos, m) = ((c.x, c.y), c.map_index);
        let near = self.handles().into_iter().filter(|&j| j != h).find_map(|j| {
            let o = self.clients[j].as_ref()?;
            let name = String::from_utf8_lossy(crate::util::cstr(&o.char_name)).to_string();
            (o.map_index == m && !o.is_killed && cheb(pos, (o.x, o.y)) <= 6 && !name.starts_with(&cfg.name_prefix) && !b.greeted.iter().any(|(g, _)| *g == name)).then_some(name)
        });
        if let Some(name) = near {
            b.greeted.push((name, now));
            if self.dice(1, 100) <= cfg.chat_greet_pct && !self.bots.chat_q.iter().any(|q| q.0 == b.n) {
                let bank: &[&str] = if self.dice(1, 2) == 1 { &HI_ES } else { &HI_EN };
                let r = self.dice(1, 1000);
                let delay = [3000, 1500, 800][b.pace] + self.dice(1, 2500) as u32;
                self.bots.chat_q.push((b.n, None, pick(bank, r).to_string(), now.wrapping_add(delay)));
            }
        }
    }

    /// Every 2 s: the JSON the web side serves (`bots_web.rs`): every bot with what it was when last seen, the maps that have bots
    /// (with their walkability bitmaps, made once) and the last chat lines.
    fn bots_publish(&mut self, now: u32) {
        use crate::bots_web::{base64, json_str};
        let Some(web) = self.bots.web.clone() else { return };
        if now.wrapping_sub(self.bots.next_publish) as i32 <= 0 {
            return;
        }
        self.bots.next_publish = now.wrapping_add(2000);
        let t = unix_ms();
        let mut views = std::mem::take(&mut self.bots.views);
        views.resize(self.bots.list.len(), BotView::default());
        let mut per_map = vec![0u32; self.maps.len()];
        let side_name = |s: i32| match s {
            ARESDEN => "aresden",
            ELVINE => "elvine",
            _ => "neutral",
        };
        let mut rows: Vec<String> = Vec::new();
        for (i, b) in self.bots.list.iter().enumerate() {
            let mut live = String::new();
            if let Some(h) = self.bot_handle(&b.ch) {
                let c = self.clients[h].as_ref().unwrap();
                per_map[c.map_index] += 1;
                views[i] = BotView {
                    level: c.level,
                    exp: c.exp,
                    side: c.side,
                    guild: String::from_utf8_lossy(crate::util::cstr(&c.guild_name)).to_string(),
                    rank: c.guild_rank,
                    map: String::from_utf8_lossy(&self.maps[c.map_index].name).to_string(),
                    x: c.x,
                    y: c.y,
                    seen: t,
                };
                live = format!(",\"hp\":{},\"maxhp\":{},\"mp\":{},\"maxmp\":{},\"gold\":{},\"party\":{},\"quest\":{}", c.hp, c.get_max_hp(), c.mp, c.get_max_mp(), self.item_count(h, b"Gold").1, self.party_members(c.party_id).len(), c.quest);
            }
            let v = &views[i];
            let guild = if v.guild.is_empty() || v.guild.starts_with("NONE") { "" } else { v.guild.as_str() };
            rows.push(format!(
                "{{\"n\":{},\"online\":{},\"level\":{},\"exp\":{},\"nation\":\"{}\",\"guild\":{},\"rank\":{},\"class\":\"{}\",\"pace\":\"{}\",\"mode\":\"{:?}\",\"map\":{},\"x\":{},\"y\":{},\"kills\":{},\"deaths\":{},\"seen\":{}{live}}}",
                json_str(&b.ch),
                !live.is_empty(),
                v.level,
                v.exp,
                side_name(v.side),
                json_str(guild),
                v.rank,
                ["warrior", "mage", "knight", "duelist", "archer"][(b.arch as usize).min(4)],
                PACES[b.pace.min(2)].name,
                b.mode,
                json_str(&v.map),
                v.x,
                v.y,
                b.kills,
                b.deaths,
                v.seen
            ));
        }
        self.bots.views = views;
        // the maps that have bots: size, count, and the walkability bitmap once
        let mut maps: Vec<String> = Vec::new();
        let mut made: Vec<(String, String)> = Vec::new();
        {
            let w = web.lock().unwrap();
            for (m, &n) in per_map.iter().enumerate().filter(|(_, n)| **n > 0) {
                let mp = &self.maps[m];
                let name = String::from_utf8_lossy(&mp.name).to_string();
                maps.push(format!("{{\"name\":{},\"w\":{},\"h\":{},\"count\":{n}}}", json_str(&name), mp.size_x, mp.size_y));
                if !w.maps.contains_key(&name) {
                    let (sx, sy) = (mp.size_x.max(0) as usize, mp.size_y.max(0) as usize);
                    let mut bits = vec![0u8; (sx * sy).div_ceil(8)];
                    for y in 0..sy {
                        for x in 0..sx {
                            if mp.get_moveable(x as i16, y as i16).0 {
                                bits[(y * sx + x) / 8] |= 1 << ((y * sx + x) % 8);
                            }
                        }
                    }
                    let tps: Vec<String> = mp.teleport_locs.iter().map(|t| format!("[{},{},{}]", t.src.0, t.src.1, json_str(&String::from_utf8_lossy(&t.dest_map)))).collect();
                    made.push((name.clone(), format!("{{\"name\":{},\"w\":{sx},\"h\":{sy},\"bits\":\"{}\",\"teleports\":[{}]}}", json_str(&name), base64(&bits), tps.join(","))));
                }
            }
        }
        let chat: Vec<String> = self.bots.chat_log.iter().rev().take(150).rev().map(|l| format!("{{\"t\":{},\"map\":{},\"from\":{},\"to\":{},\"text\":{},\"bot\":{}}}", l.t, json_str(&l.map), json_str(&l.from), json_str(&l.to), json_str(&l.text), l.bot)).collect();
        let body = format!("{{\"t\":{t},\"paused\":{},\"bots\":[{}],\"maps\":[{}],\"chat\":[{}]}}", self.bots.paused || self.bots.cfg.paused, rows.join(","), maps.join(","), chat.join(","));
        let mut w = web.lock().unwrap();
        w.bots = body;
        w.maps.extend(made);
    }

    /// Reads Bots.cfg again when its time changed: the new values replace the old ones at once, bots are added (their tasks
    /// started) or taken out (they log off) to match `bot-count`.
    fn bots_reload(&mut self) {
        let path = self.bots.cfg_path.clone();
        if path.is_empty() {
            return;
        }
        let Ok(mtime) = std::fs::metadata(&path).and_then(|m| m.modified()) else { return };
        if self.bots.cfg_mtime == Some(mtime) {
            return;
        }
        let first = self.bots.cfg_mtime.is_none();
        self.bots.cfg_mtime = Some(mtime);
        if first {
            return; // what main.rs read at the start
        }
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let (mut cfg, warnings) = BotCfg::parse(&text);
        for w in warnings {
            log(&format!("(!) {path}: {w}"));
        }
        let old = Arc::clone(&self.bots.cfg);
        // what the running tasks were started with does not change
        (cfg.login, cfg.account_prefix, cfg.name_prefix, cfg.password, cfg.login_spread_ms, cfg.relogin_ms) = (old.login.clone(), old.account_prefix.clone(), old.name_prefix.clone(), old.password.clone(), old.login_spread_ms, old.relogin_ms);
        let cfg = Arc::new(cfg);
        for b in self.bots.list.iter_mut() {
            b.tune(&cfg);
        }
        if let Some(ev) = self.bots.ev.clone() {
            while self.bots.list.len() < cfg.count {
                let b = Bot::made(&cfg, self.bots.list.len() + 1);
                tokio::spawn(run_bot(b.n, Arc::clone(&cfg), b.acc.clone(), b.ch.clone(), ev.clone()));
                self.bots.list.push(b);
            }
        }
        for n in 0..cfg.count.min(self.bots.list.len()) {
            if offline_until(n) == u64::MAX {
                set_offline(n, 0); // taken out before, back in now
            }
        }
        log(&format!("(*) {path} read again: {} bots wanted, {} known{}", cfg.count, self.bots.list.len(), if cfg.paused { ", paused" } else { "" }));
        self.bots.cfg = cfg;
    }

    /// One line: who is connected and what they do.
    pub fn bots_summary(&self) -> String {
        let mut modes = [0u32; 8];
        let (mut online, mut lv_sum, mut lv_min, mut lv_max, mut kills, mut deaths, mut gold) = (0u32, 0i64, i32::MAX, 0, 0u32, 0u32, 0i64);
        for b in &self.bots.list {
            kills += b.kills;
            deaths += b.deaths;
            let Some(h) = self.bot_handle(&b.ch) else { continue };
            let c = self.clients[h].as_ref().unwrap();
            online += 1;
            lv_sum += c.level as i64;
            (lv_min, lv_max) = (lv_min.min(c.level), lv_max.max(c.level));
            gold += self.item_count(h, b"Gold").1 as i64;
            modes[b.mode as usize] += 1;
        }
        let n = online.max(1) as i64;
        format!(
            "(bots) {online}/{} connected{}: hunting {}, resting {}, travelling {}, shopping {}, enlisting {}, raiding {}, on quests {} | level {}-{} (avg {}) | {kills} kills, {deaths} deaths | {} gold each on average",
            self.bots.list.len(),
            if self.bots.paused || self.bots.cfg.paused { " (paused)" } else { "" },
            modes[0],
            modes[1],
            modes[2],
            modes[3],
            modes[4],
            modes[5],
            modes[6],
            if online > 0 { lv_min } else { 0 },
            lv_max,
            lv_sum / n,
            gold / n
        )
    }

    /// The console: `bots status | pause | resume | count N | reload`.
    pub fn bots_command(&mut self, args: &str) {
        let mut t = args.split_whitespace();
        match (t.next(), t.next().and_then(|n| n.parse::<usize>().ok())) {
            (Some("status") | None, _) => log(&self.bots_summary()),
            (Some("pause"), _) => (self.bots.paused, ) = (true, ),
            (Some("resume"), _) => (self.bots.paused, ) = (false, ),
            (Some("reload"), _) => self.bots.cfg_mtime = Some(std::time::UNIX_EPOCH),
            (Some("count"), Some(n)) => {
                let mut cfg = (*self.bots.cfg).clone();
                cfg.count = n;
                self.bots.cfg = Arc::new(cfg);
                self.bots.cfg_mtime = Some(std::time::UNIX_EPOCH); // the reload adds the bots
                log(&format!("(*) bots: count {n} (Bots.cfg is read again; edit it too or the file wins)"));
            }
            _ => log("(!) bots: status | pause | resume | count N | reload"),
        }
        if matches!(args.split_whitespace().next(), Some("pause" | "resume")) {
            log(&format!("(*) bots {}", if self.bots.paused { "paused" } else { "resumed" }));
        }
    }

    fn bot_handle(&self, ch: &str) -> Option<usize> {
        let name = Game::pad10(ch.as_bytes());
        (1..MAXCLIENTS_).find(|&h| self.clients[h].as_ref().is_some_and(|c| c.is_init_complete && Game::pad10(&c.char_name) == name))
    }

    /// Hostile monster `i` of map `m` that is alive: (x, y, reach).
    fn bot_prey(&self, i: usize, m: usize) -> Option<(i16, i16, i16)> {
        let n = self.npcs.get(i)?.as_ref()?;
        (n.map_index == m && n.side >= SIDE_HOSTILE && !n.is_killed && n.hp > 0 && self.maps[m].is_attack_enabled).then(|| (n.x, n.y, if n.size < 2 { 1 } else { n.size as i16 }))
    }

    /// First bag slot holding an item `f` likes.
    fn bot_slot(&self, h: usize, f: impl Fn(&Item) -> bool) -> Option<usize> {
        self.clients[h].as_ref()?.items.iter().position(|i| i.as_ref().is_some_and(&f))
    }

    fn bot_think(&mut self, b: &mut Bot, now: u32) {
        let cfg = Arc::clone(&self.bots.cfg);
        let Some(h) = self.bot_handle(&b.ch) else {
            b.online = false;
            return;
        };
        if b.n >= cfg.count {
            log(&format!("(bot) {}: taken out (bot-count {}), logging off", b.ch, cfg.count));
            set_offline(b.n, u64::MAX);
            self.delete_client(h, true, true, true, false);
            b.online = false;
            return;
        }
        if !b.online {
            // just connected: it starts afresh where the game put it (a first session is cut short at random so that they do not all leave at once)
            b.online = true;
            (b.mode, b.target, b.home, b.wander, b.idle_since, b.invite) = (Mode::Hunt, 0, None, None, None, None);
            b.path.clear();
            let span = self.dice(cfg.session_min.0 as i32, cfg.session_min.1 as i32).max(1) as u32 * 60_000;
            let cut = if b.last_level == 0 { 20 + self.dice(1, 80) as u32 } else { 100 };
            b.session_until = now.wrapping_add(span / 100 * cut);
        }
        if cfg.schedule && now.wrapping_sub(b.session_until) as i32 >= 0 && matches!(b.mode, Mode::Hunt | Mode::Rest | Mode::Travel) && b.target == 0 {
            let hour = crate::chrono_now().hour;
            // more of them at night (20-1 h), fewer in the morning
            let f = match hour { 20..=23 | 0..=1 => 0.5, 2..=8 => 1.6, _ => 1.0 };
            let gap = ((self.dice(cfg.gap_min.0 as i32, cfg.gap_min.1 as i32).max(1) as f32 * f) as u64).max(1) * 60_000;
            log(&format!("(bot) {}: logs off for {} min", b.ch, gap / 60_000));
            set_offline(b.n, unix_ms() + gap);
            self.delete_client(h, true, true, true, false);
            b.online = false;
            return;
        }
        let c = self.clients[h].as_mut().unwrap();
        c.time = now; // the keep-alive a connection would give (check_client_response_time kicks after 18 s)
        if c.is_killed {
            if now.wrapping_sub(b.last_restart) > 3000 {
                b.last_restart = now;
                let mut f = vec![0u8; 6];
                W(&mut f).u32(0, MSGID_REQUEST_RESTART);
                self.bot_send(h, f);
                b.deaths += 1;
                log(&format!("(bot) {} died, restarting", b.ch));
            }
            (b.mode, b.target) = (Mode::Rest, 0);
            b.path.clear();
            return;
        }
        let (pos, m, hp, max_hp) = ((c.x, c.y), c.map_index, c.hp, c.get_max_hp());
        if b.last_level == 0 {
            let p = &PACES[b.pace];
            log(&format!("(bot) {}: {} pace (step {} ms, blow {} ms, sight {}, pause after a kill {}-{} ms, break every ~{} s for ~{} s)", b.ch, p.name, b.step_ms, b.attack_ms, b.sight, b.react.0, b.react.1, b.break_every / 1000, b.break_len / 1000));
        }
        if c.level != b.last_level {
            log(&format!("(bot) {}: level {} ({} exp, {})", b.ch, c.level, c.exp, if c.is_neutral() { "neutral" } else { "citizen" }));
            b.last_level = c.level;
        }
        self.bot_greet(h, b, &cfg, now);
        let (home_map, home) = *b.home.get_or_insert((m, pos));
        if pos != b.last_pos {
            (b.last_pos, b.stuck) = (pos, 0);
        }
        if cfg.auto_equip && now.wrapping_sub(b.last_equip) > 3000 {
            b.last_equip = now;
            self.bot_equip(h, b);
        }
        if c_pool(self, h) > 0 && now.wrapping_sub(b.last_points) > 3000 {
            self.bot_points(h, b);
            b.last_points = now;
        }
        if cfg.status_log_ms > 0 && now.wrapping_sub(b.last_report) > cfg.status_log_ms {
            b.last_report = now;
            log(&format!("(bot) {}: {:?} {}({},{}) hp {hp}/{max_hp} target {} kills {}", b.ch, b.mode, String::from_utf8_lossy(&self.maps[m].name), pos.0, pos.1, b.target, b.kills));
        }
        self.bot_party(h, b, &cfg, now);
        self.bot_guild(h, b, &cfg, now);
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
            Mode::Enlist => self.bot_enlist(h, b, &cfg, (pos, m), now),
            Mode::Raid => self.bot_raid(h, b, &cfg, (pos, m), (hp, max_hp), now),
            Mode::Quest => self.bot_quest(h, b, &cfg, (pos, m), (hp, max_hp), home, now),
        }
    }

    /// Equips the best weapon and fills empty armour slots, one item per call; what the server refuses is not retried.
    fn bot_equip(&mut self, h: usize, b: &mut Bot) {
        let Some(c) = self.clients[h].as_ref() else { return };
        b.equip_tried.retain(|&k| c.items.get(k).is_some_and(|i| i.is_some())); // a freed slot may hold something new
        let role = self.bot_role(h, b);
        let eq = |pos: i32| c.equip_status.get(pos as usize).copied().unwrap_or(-1);
        if role.mage {
            if let Some(k) = [EQUIPPOS_RHAND, EQUIPPOS_TWOHAND, EQUIPPOS_LHAND].into_iter().map(eq).find(|&k| k >= 0) {
                let pos = (c.x, c.y);
                log(&format!("(bot) {}: puts away what it holds in its hands (slot {k}): spells need empty hands", b.ch));
                self.bot_send(h, common(COMMONTYPE_RELEASEITEM, pos, k as u32));
                return;
            }
        }
        let mut best: Option<(i32, usize)> = None;
        for (k, it) in c.items.iter().enumerate() {
            let Some(it) = it else { continue };
            if it.cfg.item_type != 1 || it.cur_life_span == 0 || b.equip_tried.contains(&k) || eq(it.cfg.equip_pos) == k as i16 {
                continue;
            }
            if let Some(score) = wear_score(c, it, role) {
                if best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, k));
                }
            }
        }
        if let Some((_, k)) = best {
            let name = String::from_utf8_lossy(&c.items[k].as_ref().unwrap().cfg.name).to_string();
            let pos = (c.x, c.y);
            b.equip_tried.push(k);
            b.last_equip_slot = (k, self.now());
            log(&format!("(bot) {}: equips {name} (slot {k})", b.ch));
            self.bot_send(h, common(COMMONTYPE_EQUIPITEM, pos, k as u32));
        }
    }

    /// Hurt: a red potion; nearly dead with none left: the recall scroll. True: it did one (the turn is spent).
    fn bot_survive(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, pos: (i16, i16), hp: (i32, i32), now: u32) -> bool {
        let (hp, max_hp) = hp;
        if self.bot_heal(h, b, cfg, pos, now) {
            return true;
        }
        let potion = if cfg.use_potions { self.bot_slot(h, is_potion) } else { None };
        if let Some(k) = potion.filter(|_| hp * 100 < cfg.potion_pct * max_hp && now.wrapping_sub(b.last_potion) >= 1200) {
            b.last_potion = now;
            log(&format!("(bot) {}: hp {hp}/{max_hp}: drinks potion (slot {k})", b.ch));
            self.bot_send(h, common(COMMONTYPE_REQ_USEITEM, pos, k as u32));
            return true;
        }
        if cfg.use_recall && potion.is_none() && hp * 100 < cfg.recall_pct * max_hp && now.wrapping_sub(b.last_recall) >= 5000 {
            if let Some(k) = self.bot_slot(h, is_recall_scroll) {
                b.last_recall = now;
                log(&format!("(bot) {}: hp {hp}/{max_hp} and no potion: reads a recall scroll", b.ch));
                self.bot_send(h, common(COMMONTYPE_REQ_USEITEM, pos, k as u32));
                return true;
            }
        }
        false
    }

    /// Parties among the bots: it accepts an invitation from a bot, and (idle, no party) invites the nearest bot of its
    /// map and side: one that is in a party with room, or, if none is, a free one with a higher handle. The invited bot
    /// is asked to put its weapons away first (the server refuses an invitation to a player in combat mode).
    fn bot_party(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, now: u32) {
        let Some(c) = self.clients[h].as_ref() else { return };
        let pos = (c.x, c.y);
        if c.req_join_party_h != 0 {
            // a request nobody is waiting for any more (the asker is in the party, or gone): forget it, the server never clears it
            let asker = self.clients[c.req_join_party_h].as_ref();
            if asker.is_none_or(|o| o.party_status != PARTYSTATUS_PROCESSING || o.req_join_party_h != h) {
                let c = self.clients[h].as_mut().unwrap();
                (c.req_join_party_h, c.req_join_party_name) = (0, Vec::new());
                return;
            }
        }
        let Some(c) = self.clients[h].as_ref() else { return };
        if c.req_join_party_h != 0 && c.party_status != PARTYSTATUS_PROCESSING {
            if now.wrapping_sub(b.last_accept) > 1500 && crate::util::cstr(&c.req_join_party_name).starts_with(cfg.name_prefix.as_bytes()) {
                let from = String::from_utf8_lossy(crate::util::cstr(&c.req_join_party_name)).to_string();
                b.accepts = if now.wrapping_sub(b.last_accept) < 6000 && b.accepts.1 == from { (b.accepts.0 + 1, from.clone()) } else { (1, from.clone()) };
                b.last_accept = now;
                // the same request again and again: the server did not take the first answers (a full party): it answers no
                let yes = b.accepts.0 <= 3;
                log(&format!("(bot) {}: {} the party of {from}", b.ch, if yes { "accepts" } else { "turns down" }));
                self.bot_send(h, common(COMMONTYPE_REQUEST_ACCEPTJOINPARTY, pos, yes as u32));
            }
            return;
        }
        if !cfg.party || b.mode != Mode::Hunt || c.party_id != 0 || c.party_status != PARTYSTATUS_NULL || c.is_killed {
            b.invite = None;
            return;
        }
        if let Some((name, start, sent)) = b.invite.clone() {
            let peaceful = self.bot_handle(&name).and_then(|j| self.clients[j].as_ref()).map(|o| o.appr[1] as u16 & 0xF000 == 0);
            if peaceful.is_none() || now.wrapping_sub(start) > 12_000 {
                b.invite = None;
            } else if !sent && peaceful == Some(true) {
                log(&format!("(bot) {}: invites {name} to a party", b.ch));
                b.invite = Some((name.clone(), start, true));
                let mut f = common(COMMONTYPE_REQUEST_JOINPARTY, pos, 1);
                W(&mut f).bytes(23, name.as_bytes());
                self.bot_send(h, f);
            }
            return;
        }
        if now.wrapping_sub(b.last_invite) < 45_000 {
            return;
        }
        let (m, side) = (c.map_index, c.side);
        let mut best: Option<(i16, String)> = None;
        for j in self.handles() {
            let Some(o) = self.clients[j].as_ref() else { continue };
            let d = cheb(pos, (o.x, o.y));
            let free = o.party_status == PARTYSTATUS_NULL && o.party_id == 0;
            let room = o.party_status == PARTYSTATUS_CONFIRM && self.party_members(o.party_id).len() < MAXPARTYMEMBERS as usize;
            let bot = j != h && crate::util::cstr(&o.char_name).starts_with(cfg.name_prefix.as_bytes());
            if bot && o.map_index == m && o.side == side && !o.is_killed && d <= cfg.party_range && (room || (free && h < j)) && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, String::from_utf8_lossy(crate::util::cstr(&o.char_name)).to_string()));
            }
        }
        if let Some((_, name)) = best {
            b.last_invite = now;
            b.invite = Some((name.clone(), now, false));
            self.bots.peace.push((name, now.wrapping_add(10_000)));
        }
    }

    /// Guilds among the bots (`guilds`): a master approves the join requests of bots of its side; a guildless citizen asks to
    /// join the master of its side, or, when there is none and it can (level 20, charisma 20, standing in a town), founds
    /// one. Crusade: during one it takes the fighter duty.
    fn bot_guild(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, now: u32) {
        let Some(c) = self.clients[h].as_ref() else { return };
        if c.is_neutral() || c.is_killed || now.wrapping_sub(b.last_guild) < 4000 {
            return;
        }
        let (pos, side, rank, level, chr, in_town, me) = ((c.x, c.y), c.side, c.guild_rank, c.level, c.range, c.is_on_town_nonpk, Game::pad10(&c.char_name));
        b.last_guild = now;
        let prefix = cfg.name_prefix.as_bytes();
        let is_bot = |g: &Game, j: usize| g.clients[j].as_ref().is_some_and(|o| crate::util::cstr(&o.char_name).starts_with(prefix));
        if cfg.crusade && self.cr.mode && level >= 10 && c.crusade_duty == 0 {
            log(&format!("(bot) {}: a crusade is on: takes the fighter duty", b.ch));
            self.bot_send(h, common(COMMONTYPE_REQUEST_SELECTCRUSADEDUTY, pos, 1));
            return;
        }
        if !cfg.guilds {
            return;
        }
        if rank == 0 {
            // the master: whoever asked it and still qualifies
            let asker = self.handles().into_iter().filter(|&j| is_bot(self, j)).find(|&j| self.clients[j].as_ref().is_some_and(|o| o.guild_request == Some((crate::guild::GuildReq::Join, me))));
            if let Some(j) = asker {
                let name = self.clients[j].as_ref().unwrap().char_name.clone();
                log(&format!("(bot) {}: approves {} into its guild", b.ch, String::from_utf8_lossy(crate::util::cstr(&name))));
                let mut f = common(COMMONTYPE_JOINGUILDAPPROVE, pos, 0);
                W(&mut f).bytes(23, &name);
                self.bot_send(h, f);
            }
            return;
        }
        if rank != -1 || level < 10 || now.wrapping_sub(b.last_ask) < 60_000 && b.last_ask != 0 {
            return;
        }
        let master = self.handles().into_iter().find(|&j| j != h && is_bot(self, j) && self.clients[j].as_ref().is_some_and(|o| o.guild_rank == 0 && o.side == side));
        if let Some(j) = master {
            b.last_ask = now.max(1);
            let mname = String::from_utf8_lossy(crate::util::cstr(&self.clients[j].as_ref().unwrap().char_name)).to_string();
            if self.guild_ticket(h, j, crate::guild::ITEM_GUILDADMISSIONTICKET) {
                log(&format!("(bot) {}: asks to join the guild of {mname}", b.ch));
            }
            return;
        }
        // one founder at a time: another bot of its side that already has a name in the making (the creation is not answered yet) holds it back
        let founding = self.handles().into_iter().any(|j| j != h && is_bot(self, j) && self.clients[j].as_ref().is_some_and(|o| o.side == side && o.guild_rank == -1 && !o.guild_name.is_empty() && !o.guild_name.starts_with(b"NONE")));
        if level >= 20 && chr >= 20 && in_town && !founding {
            let names: [&str; 3] = if side == ARESDEN { ["Vanguard", "Ironwall", "Dawnblades"] } else { ["Silverleaf", "Moonguard", "Wolfpack"] };
            let name = names[(b.n) % 3];
            log(&format!("(bot) {}: founds the guild {name}", b.ch));
            let mut f = vec![0u8; 57];
            W(&mut f).u32(0, MSGID_REQUEST_CREATENEWGUILD);
            W(&mut f).bytes(36, name.as_bytes());
            b.last_ask = now.max(1);
            self.bot_send(h, f);
        }
    }

    /// The bot of its party with the lowest handle (the one the others follow), if that is not itself.
    fn bot_party_leader(&self, h: usize) -> Option<usize> {
        let pid = self.clients[h].as_ref()?.party_id;
        if pid == 0 {
            return None;
        }
        let prefix = self.bots.cfg.name_prefix.as_bytes();
        let leader = self.party_members(pid).into_iter().filter(|&j| self.clients[j].as_ref().is_some_and(|o| crate::util::cstr(&o.char_name).starts_with(prefix))).min()?;
        (leader != h).then_some(leader)
    }

    /// A party mate (not the leader) too far from the leader, or on another map, goes to the leader's tile (when the leader is
    /// hunting): the group stays together. True: it set off.
    fn bot_follow(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), now: u32) -> bool {
        if !cfg.party_follow || now.wrapping_sub(b.last_follow) < 5000 {
            return false;
        }
        let Some(l) = self.bot_party_leader(h) else { return false };
        let Some(lc) = self.clients[l].as_ref() else { return false };
        let (lname, lm, lpos) = (String::from_utf8_lossy(crate::util::cstr(&lc.char_name)).to_string(), lc.map_index, (lc.x, lc.y));
        let leader_hunts = !lc.is_killed && self.bots.list.iter().any(|x| x.ch == lname && x.mode == Mode::Hunt);
        let (pos, m) = at;
        if !leader_hunts || (lm == m && cheb(pos, lpos) <= 12) {
            return false;
        }
        b.last_follow = now;
        log(&format!("(bot) {}: goes to its party leader {lname} ({}({},{}))", b.ch, String::from_utf8_lossy(&self.maps[lm].name), lpos.0, lpos.1));
        (b.home, b.mode, b.no_route_logged, b.wander, b.idle_since, b.target) = (Some((lm, lpos)), Mode::Travel, false, None, None, 0);
        b.path.clear();
        true
    }

    /// The monster a party mate is fighting (a bot's target on this map within `r` tiles), if there is one: the group fights together.
    fn bot_assist(&self, h: usize, m: usize, pos: (i16, i16), r: i16, ignore: &[(usize, u32)]) -> Option<usize> {
        let pid = self.clients[h].as_ref()?.party_id;
        if pid == 0 {
            return None;
        }
        self.party_members(pid).into_iter().filter(|&j| j != h).find_map(|j| {
            let name = String::from_utf8_lossy(crate::util::cstr(&self.clients[j].as_ref()?.char_name)).to_string();
            let t = self.bots.list.iter().find(|x| x.ch == name)?.target;
            let (tx, ty, _) = self.bot_prey(t, m)?;
            (t != 0 && cheb(pos, (tx, ty)) <= r && !ignore.iter().any(|&(i, _)| i == t)).then_some(t)
        })
    }

    /// A citizen of level 11 or more with no quest, after the cool-down and once in two looks, goes to a town for one (or goes
    /// on with the one it has).
    fn bot_maybe_quest(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, now: u32) -> bool {
        let Some(c) = self.clients[h].as_ref() else { return false };
        if !cfg.quests || c.is_neutral() || c.level < 11 || (b.last_quest != 0 && now.wrapping_sub(b.last_quest) < cfg.quest_minutes * 60_000) {
            return false;
        }
        let has = c.quest != 0;
        if b.last_quest == 0 {
            b.last_quest = now.max(1); // the first look comes one cool-down after it logs in
            if !has {
                return false;
            }
        } else {
            b.last_quest = now.max(1);
            if !has && self.dice(1, 2) != 1 {
                return false;
            }
        }
        log(&format!("(bot) {}: {}", b.ch, if has { "goes on with its quest" } else { "goes to the city hall for a quest" }));
        (b.mode, b.qphase, b.quest_start, b.quest_type, b.quest_asks, b.target, b.no_route_logged) = (Mode::Quest, if has { 2 } else { 0 }, now, 0, 0, 0, false);
        b.path.clear();
        true
    }

    /// The quest: a town tile (the city hall answers anywhere in town), ask, accept, kill what it asks on the map it names,
    /// back to town to hand it in. A quest that is not a monster hunt, one it cannot win, or one that takes too long is cancelled.
    fn bot_quest(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), hp: (i32, i32), home: (i16, i16), now: u32) {
        let (pos, m) = at;
        let Some(c) = self.clients[h].as_ref() else { return };
        let (quest, asked, done, in_town, side) = (c.quest, c.asked_quest, c.is_quest_completed, c.is_on_town_nonpk, c.side);
        let end = |g: &mut Game, b: &mut Bot, why: &str, cancel: bool| {
            log(&format!("(bot) {}: quest over: {why}", b.ch));
            if cancel {
                g.bot_send(h, common(COMMONTYPE_REQUEST_CANCELQUEST, pos, 0));
            }
            (b.mode, b.qphase, b.quest_type, b.no_route_logged, b.last_quest) = (Mode::Travel, 0, 0, false, now.max(1));
            b.path.clear();
        };
        if now.wrapping_sub(b.quest_start) > 25 * 60_000 {
            end(self, b, "it takes too long", true);
            return;
        }
        let towns: Vec<bool> = self.maps.iter().map(|mp| mp.name == SIDE_MAP[side.clamp(0, 3) as usize]).collect();
        match b.qphase {
            0 | 3 => {
                // to a town of its nation
                if !in_town {
                    if !self.bot_goto(h, b, cfg, at, &|d| towns[d], now) && !b.no_route_logged {
                        b.no_route_logged = true;
                        end(self, b, "no way to a town", quest != 0);
                    }
                    return;
                }
                if now.wrapping_sub(b.last_ask) < 2500 {
                    return;
                }
                b.last_ask = now;
                if b.qphase == 3 {
                    if quest == 0 {
                        end(self, b, "reward collected", false);
                        return;
                    }
                    // the reward goes into the bag: with no free slot it is not paid (and could be asked again), so it makes room first
                    let role = self.bot_role(h, b);
                    let full = self.clients[h].as_ref().is_some_and(|c| c.items.iter().all(|i| i.is_some()));
                    if full {
                        let c = self.clients[h].as_ref().unwrap();
                        let k = junk_slots(c, b, role, now).first().copied().or_else(|| c.items.iter().enumerate().rposition(|(k, i)| i.as_ref().is_some_and(|i| !is_potion(i) && !is_recall_scroll(i) && i.cfg.name.get(..4) != Some(b"Gold") && c.equip_status.get(i.cfg.equip_pos as usize) != Some(&(k as i16)))));
                        if let Some(k) = k {
                            self.bot_drop_slot(h, b, k, pos);
                            b.loot_skip.push((pos, now.wrapping_add(600_000)));
                            return;
                        }
                    }
                    b.quest_asks += 1;
                    if b.quest_asks > 4 {
                        end(self, b, "the city hall would not pay", true);
                        return;
                    }
                    self.bot_send(h, common(COMMONTYPE_TALKTONPC, pos, 4)); // the reward
                    return;
                }
                b.qphase = 1;
                b.quest_asks = 0;
            }
            1 => {
                if quest != 0 {
                    b.qphase = 2;
                    return;
                }
                if now.wrapping_sub(b.last_ask) < 2500 {
                    return;
                }
                b.last_ask = now;
                b.quest_asks += 1;
                if b.quest_asks > 4 {
                    end(self, b, "the city hall has no quest for it", false);
                    return;
                }
                if asked != 0 {
                    self.bot_send(h, common(COMMONTYPE_QUESTACCEPTED, pos, 0));
                } else {
                    self.bot_send(h, common(COMMONTYPE_TALKTONPC, pos, 4));
                }
            }
            _ => {
                if quest == 0 {
                    end(self, b, "the quest is gone", false);
                    return;
                }
                if done {
                    log(&format!("(bot) {}: quest done, back to town for the reward", b.ch));
                    (b.qphase, b.quest_asks, b.quest_type) = (3, 0, 0);
                    return;
                }
                let Some(q) = self.quest_cfg(quest) else { return end(self, b, "unknown quest", true) };
                if q.ty != crate::quest::QUESTTYPE_MONSTERHUNT {
                    end(self, b, "not a monster hunt", true);
                    return;
                }
                let target_map = self.map_index(crate::util::cstr(&q.target_name));
                let strongest = self.npcs.iter().flatten().filter(|n| n.type_ as i32 == q.target_type).map(|n| n.hit_dice).max();
                let max_dice = self.bot_max_dice(h, cfg);
                if target_map.is_none() || strongest.is_none_or(|d| d > max_dice + 2) {
                    end(self, b, "the monsters it names are too strong or not on this server", true);
                    return;
                }
                if b.quest_type != q.target_type {
                    log(&format!("(bot) {}: quest {quest}: kill {} of type {} on {} ({} of them alive there)", b.ch, q.max_count, q.target_type, String::from_utf8_lossy(crate::util::cstr(&q.target_name)), self.npcs.iter().flatten().filter(|n| Some(n.map_index) == target_map && n.type_ as i32 == q.target_type && !n.is_killed).count()));
                }
                b.quest_type = q.target_type;
                let alive = self.npcs.iter().flatten().any(|n| Some(n.map_index) == target_map && n.type_ as i32 == q.target_type && !n.is_killed && n.hp > 0);
                if !alive {
                    end(self, b, "none of that kind alive on the quest map", true);
                    return;
                }
                if Some(m) != target_map {
                    let tm = target_map.unwrap();
                    if !self.bot_goto(h, b, cfg, at, &|d| d == tm, now) {
                        end(self, b, "no way to the quest map", true);
                    }
                    return;
                }
                // on the map: fight what is in sight, else walk to the nearest of the kind
                let seen = self.bot_prey_of_type(m, pos, b.sight, q.target_type);
                if seen.is_some() || b.target != 0 {
                    self.bot_hunt(h, b, cfg, (pos, m, hp.0, hp.1), home, now);
                } else if let Some((tx, ty)) = self.npcs.iter().flatten().filter(|n| n.map_index == m && n.type_ as i32 == q.target_type && !n.is_killed && n.hp > 0).map(|n| (n.x, n.y)).min_by_key(|&t| cheb(t, pos)) {
                    self.bot_walk(h, b, cfg, at, (tx, ty), 1, None, (cheb(pos, (tx, ty)) + 8).clamp(30, 250), now);
                } else {
                    end(self, b, "none of them left on the map", true);
                }
            }
        }
    }

    /// A live monster of type `ty` within `r` tiles.
    fn bot_prey_of_type(&self, m: usize, pos: (i16, i16), r: i16, ty: i32) -> Option<usize> {
        (-r..=r).find_map(|dx| {
            (-r..=r).find_map(|dy| {
                let (o, ot) = self.maps[m].get_owner(pos.0 + dx, pos.1 + dy);
                (ot == OWNERTYPE_NPC && o > 0 && self.bot_prey(o as usize, m).is_some() && self.npcs[o as usize].as_ref().is_some_and(|n| n.type_ as i32 == ty)).then_some(o as usize)
            })
        })
    }

    /// Now and then (cool-down, then 1 in 3 per check) a citizen of `raid-level` goes to a map where players of the rival
    /// nation are (also one of its own maps: defending it), or else to one of the rival's maps where attacking is allowed.
    fn bot_maybe_raid(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), hp: (i32, i32), now: u32) -> bool {
        let Some(c) = self.clients[h].as_ref() else { return false };
        let war = cfg.crusade && self.cr.mode;
        let cooldown = if war { cfg.raid_cooldown_ms.min(60_000) } else { cfg.raid_cooldown_ms };
        if !cfg.raid || c.is_neutral() || c.level < (if war { 10 } else { cfg.raid_level }) || hp.0 * 10 < hp.1 * 8 || (b.last_raid != 0 && now.wrapping_sub(b.last_raid) < cooldown) {
            return false;
        }
        if b.last_raid == 0 {
            b.last_raid = now.max(1); // the first look comes one cool-down after it logs in (its side is known by then)
            return false;
        }
        b.last_raid = now.wrapping_sub(cooldown).wrapping_add(60_000); // not this time: look again in a minute
        let enemy = if c.side == ARESDEN { ELVINE } else { ARESDEN };
        if self.dice(1, if war { 1 } else { 3 }) != 1 {
            return false;
        }
        let (pos, m) = at;
        let foe: Vec<bool> = self.maps.iter().map(|mp| crate::util::cstr(&mp.location_name) == SIDE_MAP[enemy as usize] && mp.is_attack_enabled).collect();
        let mut with_players = vec![false; self.maps.len()];
        for j in self.handles() {
            if let Some(o) = self.clients[j].as_ref().filter(|o| o.side == enemy && !o.is_killed) {
                with_players[o.map_index] = self.maps[o.map_index].is_attack_enabled; // also its own maps: defending against invaders
            }
        }
        let d = if with_players[m] { Some(m) } else { self.bot_route(m, &|d| with_players[d], pos).or_else(|| self.bot_route(m, &|d| foe[d], pos)).map(|r| r.1) };
        let Some(d) = d else { return false };
        log(&format!("(bot) {}: raid on {} ({} players of the rival nation there)", b.ch, String::from_utf8_lossy(&self.maps[d].name), self.handles().iter().filter(|&&j| self.clients[j].as_ref().is_some_and(|o| o.side == enemy && o.map_index == d)).count()));
        (b.mode, b.last_raid, b.raid_start, b.raid_map, b.raid_target, b.raid_kills, b.target, b.no_route_logged) = (Mode::Raid, now, now, d, 0, 0, 0, false);
        (b.attacks, b.raid_ignore) = (0, Vec::new());
        b.path.clear();
        true
    }

    /// The raid: to the map, then the nearest player of the rival nation in sight, until the time is up or it is hurt.
    fn bot_raid(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), hp: (i32, i32), now: u32) {
        let (pos, m) = at;
        let Some(c) = self.clients[h].as_ref() else { return };
        let enemy = if c.side == ARESDEN { ELVINE } else { ARESDEN };
        if now.wrapping_sub(b.raid_start) > cfg.raid_ms || hp.0 * 100 < hp.1 * cfg.tired_pct || c.is_neutral() {
            log(&format!("(bot) {}: raid over ({} kills), going home", b.ch, b.raid_kills));
            (b.mode, b.no_route_logged, b.last_raid, b.target) = (Mode::Travel, false, now, 0);
            b.path.clear();
            return;
        }
        if m != b.raid_map {
            let rm = b.raid_map;
            if !self.bot_goto(h, b, cfg, at, &|d| d == rm, now) {
                log(&format!("(bot) {}: no way to the raid map, going home", b.ch));
                (b.mode, b.no_route_logged, b.last_raid) = (Mode::Travel, false, now);
            }
            return;
        }
        if self.bot_survive(h, b, cfg, pos, hp, now) {
            return;
        }
        let combat = self.clients[h].as_ref().is_some_and(|c| c.appr[1] as u16 & 0xF000 != 0);
        if !combat {
            if now.wrapping_sub(b.last_toggle) > 1500 {
                b.last_toggle = now;
                self.bot_send(h, common(COMMONTYPE_TOGGLECOMBATMODE, pos, 0));
            }
            return;
        }
        let alive = |g: &Game, j: usize| g.clients[j].as_ref().filter(|o| o.side == enemy && o.map_index == m && !o.is_killed && o.hp > 0).map(|o| (o.x, o.y));
        if b.raid_target != 0 && alive(self, b.raid_target).is_none() {
            b.raid_kills += 1;
            log(&format!("(bot) {}: rival player down ({} kills in this raid)", b.ch, b.raid_kills));
            b.raid_target = 0;
        }
        b.raid_ignore.retain(|&(_, t)| now.wrapping_sub(t) as i32 <= 0);
        if b.raid_target == 0 {
            let ignored = |j: usize| b.raid_ignore.iter().any(|&(i, _)| i == j);
            let best = self.handles().into_iter().filter(|&j| !ignored(j)).filter_map(|j| alive(self, j).map(|t| (cheb(pos, t), j))).filter(|&(d, _)| d <= b.sight * 2).min();
            (b.raid_target, b.attacks) = (best.map_or(0, |(_, j)| j), 0);
        }
        if let Some((tx, ty)) = alive(self, b.raid_target) {
            if self.bot_role(h, b).shoot && cheb(pos, (tx, ty)) <= cfg.bow_range {
                // the target is a player (a client handle), not an npc index: its HP is read from the client
                let thp = self.clients[b.raid_target].as_ref().map_or(0, |o| o.hp);
                if b.attacks == 0 {
                    b.target_hp0 = thp;
                }
                if b.attacks >= cfg.giveup_hits * 2 && thp * 100 > b.target_hp0 * cfg.giveup_pct {
                    log(&format!("(bot) {}: giving up on player {} ({thp}/{} HP after {} arrows)", b.ch, b.raid_target, b.target_hp0, b.attacks));
                    b.raid_ignore.push((b.raid_target, now.wrapping_add(120_000)));
                    b.raid_target = 0;
                    return;
                }
                if now.wrapping_sub(b.last_attack) >= b.attack_ms {
                    b.last_attack = now;
                    b.attacks += 1;
                    let f = motion(OBJECTATTACK, pos, dir_to(pos, (tx, ty)).max(1), now, Some((tx, ty, 2, b.raid_target as u16)));
                    self.bot_send(h, f);
                }
                return; // standing off: the arrow reaches it
            }
            if cfg.cast && cheb(pos, (tx, ty)) <= cfg.cast_range && self.bot_attack_spell(h).is_some() {
                if now.wrapping_sub(b.last_cast) >= cfg.cast_ms * b.attack_ms / cfg.attack_ms.max(1) && self.bot_cast(h, pos, (tx, ty), now) {
                    b.last_cast = now;
                }
            } else if cheb(pos, (tx, ty)) <= 1 {
                if now.wrapping_sub(b.last_attack) >= b.attack_ms {
                    b.last_attack = now;
                    let f = motion(OBJECTATTACK, pos, dir_to(pos, (tx, ty)).max(1), now, Some((tx, ty, 1, b.raid_target as u16)));
                    self.bot_send(h, f);
                }
            } else {
                self.bot_walk(h, b, cfg, at, (tx, ty), 1, None, cfg.path_limit, now);
            }
        } else {
            // nobody in sight: look around
            if b.wander.is_none_or(|w| w == pos) {
                let (dx, dy) = (self.dice(1, 25) as i16 - 13, self.dice(1, 25) as i16 - 13);
                b.wander = self.maps[m].get_moveable(pos.0 + dx, pos.1 + dy).0.then_some((pos.0 + dx, pos.1 + dy));
            }
            if let Some(w) = b.wander {
                self.bot_walk(h, b, cfg, at, w, 0, None, cfg.path_limit, now);
            }
        }
    }

    fn bot_hunt(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, st: ((i16, i16), usize, i32, i32), home: (i16, i16), now: u32) {
        let (pos, m, hp, max_hp) = st;
        if self.bot_survive(h, b, cfg, pos, (hp, max_hp), now) {
            return;
        }
        // a break: it stands about until the time is up, or until something hurts it
        if b.pause_until != 0 && now.wrapping_sub(b.pause_until) as i32 <= 0 {
            if hp >= b.pause_hp {
                return;
            }
            b.pause_until = now;
            log(&format!("(bot) {}: hit during its break, back to it", b.ch));
        }
        // asked to put the weapons away for a party invitation
        if self.bots.peace.iter().any(|(n, t)| *n == b.ch && now.wrapping_sub(*t) as i32 <= 0) {
            let combat = self.clients[h].as_ref().is_some_and(|c| c.appr[1] as u16 & 0xF000 != 0);
            if combat && now.wrapping_sub(b.last_toggle) > 1500 {
                b.last_toggle = now;
                self.bot_send(h, common(COMMONTYPE_TOGGLECOMBATMODE, pos, 0));
            }
            return;
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
            if b.react.1 > 0 {
                b.react_until = now.wrapping_add(b.react.0 + self.dice(1, (b.react.1 - b.react.0) as i32) as u32); // a breather after a kill
            }
        }
        b.ignore.retain(|&(_, t)| now.wrapping_sub(t) as i32 <= 0);
        let tired = hp * 100 < max_hp * cfg.tired_pct;
        let max_dice = self.bot_max_dice(h, cfg);
        // between two fights: supplies first, then what lies about, then the next monster
        if b.target == 0 {
            b.flee_steps = 0;
        }
        // hurt, or a mage out of mana: it stands still, away from monsters, until it has recovered (hysteresis: down at `tired`, up at `recover-percent`)
        if cfg.recover {
            let casts = self.bot_role(h, b).mage;
            let (mp, max_mp) = self.clients[h].as_ref().map_or((0, 1), |c| (c.mp, c.get_max_mp().max(1)));
            let low = hp * 100 < max_hp * cfg.tired_pct || (casts && mp * 100 < max_mp * 30);
            let high = hp * 100 >= max_hp * cfg.recover_high && (!casts || mp * 100 >= max_mp * cfg.recover_high);
            if !b.recovering && low {
                b.recovering = true;
                log(&format!("(bot) {}: hp {hp}/{max_hp}, mp {mp}/{max_mp}: stands still to recover", b.ch));
            } else if b.recovering && high {
                b.recovering = false;
                log(&format!("(bot) {}: recovered (hp {hp}/{max_hp}, mp {mp}/{max_mp})", b.ch));
            }
            if b.recovering && b.target == 0 && !self.bot_hostile_near(m, pos, 5) {
                return;
            }
        }
        if b.target == 0 && !tired && now.wrapping_sub(b.react_until) as i32 <= 0 && b.react_until != 0 {
            return; // still getting its breath after the last kill
        }
        if b.target == 0 && !tired && b.break_every > 0 {
            if b.next_break == 0 {
                b.next_break = now.wrapping_add(b.break_every / 2 + self.dice(1, b.break_every as i32) as u32);
            } else if now.wrapping_sub(b.next_break) as i32 >= 0 {
                let len = b.break_len / 2 + self.dice(1, b.break_len as i32) as u32;
                b.next_break = now.wrapping_add(len).wrapping_add(b.break_every / 2 + self.dice(1, b.break_every as i32) as u32);
                (b.pause_until, b.pause_hp) = (now.wrapping_add(len), hp);
                log(&format!("(bot) {}: takes a break of {} s", b.ch, len / 1000));
                return;
            }
        }
        if b.mode == Mode::Hunt && b.target == 0 && !tired && self.bot_maybe_enlist(h, b, cfg, now) {
            return;
        }
        if b.mode == Mode::Hunt && b.target == 0 && !tired && self.bot_maybe_raid(h, b, cfg, (pos, m), (hp, max_hp), now) {
            return;
        }
        if b.mode == Mode::Hunt && b.target == 0 && !tired && self.bot_maybe_quest(h, b, cfg, now) {
            return;
        }
        if b.mode == Mode::Hunt && cfg.roam && cfg.move_minutes.1 > 0 && b.target == 0 && !tired && self.bot_party_leader(h).is_none() {
            if b.next_move == 0 {
                b.next_move = now.wrapping_add(self.dice(cfg.move_minutes.0 as i32, cfg.move_minutes.1 as i32).max(1) as u32 * 60_000);
            } else if now.wrapping_sub(b.next_move) as i32 >= 0 {
                b.next_move = now.wrapping_add(self.dice(cfg.move_minutes.0 as i32, cfg.move_minutes.1 as i32).max(1) as u32 * 60_000);
                if self.bot_roam_to(b, (pos, m), max_dice, false, true) {
                    return;
                }
            }
        }
        if b.mode == Mode::Hunt && b.target == 0 && !tired && self.bot_follow(h, b, cfg, (pos, m), now) {
            return;
        }
        if b.mode == Mode::Hunt && cfg.roam && b.target == 0 && !tired && self.bot_party_leader(h).is_none() {
            if b.max_dice_seen == 0 {
                b.max_dice_seen = max_dice;
            } else if max_dice > b.max_dice_seen {
                b.max_dice_seen = max_dice;
                if self.bot_roam(b, (pos, m), max_dice, true) {
                    return;
                }
            }
        }
        if b.target == 0 && !tired && self.bot_maybe_drop(h, b, cfg, pos, now) {
            return;
        }
        if b.target == 0 && !tired && ((b.mode == Mode::Hunt && self.bot_maybe_shop(h, b, cfg, (pos, m), now)) || (cfg.loot && self.bot_loot(h, b, cfg, (pos, m), now))) {
            return;
        }
        if b.target == 0 {
            // hurt: only what is already on top of it
            let sight = if tired { 2 } else { b.sight };
            let mut best = (i16::MAX, 0);
            for dx in -sight..=sight {
                for dy in -sight..=sight {
                    let (o, ot) = self.maps[m].get_owner(pos.0 + dx, pos.1 + dy);
                    if ot == OWNERTYPE_NPC && o > 0 && !b.ignore.iter().any(|&(i, _)| i == o as usize) && self.bot_prey(o as usize, m).is_some() && self.npcs[o as usize].as_ref().is_some_and(|n| (n.hit_dice <= max_dice || b.quest_type != 0) && (b.quest_type == 0 || n.type_ as i32 == b.quest_type)) && dx.abs().max(dy.abs()) < best.0 {
                        best = (dx.abs().max(dy.abs()), o as usize);
                    }
                }
            }
            if let Some(t) = self.bot_assist(h, m, pos, sight + 4, &b.ignore).filter(|_| cfg.party_follow && !tired) {
                best = (0, t);
            }
            b.target = best.1;
            b.attacks = 0;
            if let Some(n) = self.npcs.get(b.target).and_then(|n| n.as_ref()) {
                b.ground_dice = b.ground_dice.max(n.hit_dice);
            }
        }
        if let Some((tx, ty, reach)) = self.bot_prey(b.target, m) {
            b.idle_since = None;
            // nearly dead with nothing to drink or read: away from it, a few steps (it keeps up more often than not, but a bot
            // that gets out of its reach can recover; after 12 steps it turns and fights)
            if cfg.flee && hp * 100 < max_hp * cfg.flee_pct && b.flee_steps < 12 && self.bot_count(h, is_potion) == 0 && self.bot_count(h, is_recall_scroll) == 0 && cheb(pos, (tx, ty)) <= 4 {
                if b.flee_steps == 0 {
                    log(&format!("(bot) {}: hp {hp}/{max_hp} and nothing to drink: walks away from the monster", b.ch));
                }
                b.flee_steps += 1;
                if self.bot_flee(h, b, cfg, (pos, m), (tx, ty), now) {
                    return;
                }
            }
            if cfg.cast && cheb(pos, (tx, ty)) <= cfg.cast_range && self.bot_attack_spell(h).is_some() {
                let thp = self.npcs[b.target].as_ref().map_or(0, |n| n.hp);
                if b.attacks == 0 {
                    b.target_hp0 = thp;
                }
                if b.attacks >= cfg.giveup_hits * 2 && thp * 100 > b.target_hp0 * cfg.giveup_pct {
                    log(&format!("(bot) {}: giving up on npc {} ({thp}/{} HP after {} casts)", b.ch, b.target, b.target_hp0, b.attacks));
                    b.ignore.push((b.target, now.wrapping_add(120_000)));
                    b.target = 0;
                    return;
                }
                if now.wrapping_sub(b.last_cast) >= cfg.cast_ms * b.attack_ms / cfg.attack_ms.max(1) && self.bot_cast(h, pos, (tx, ty), now) {
                    b.last_cast = now;
                    b.attacks += 1;
                }
                return; // standing off: the spell reaches it
            }
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
                if now.wrapping_sub(b.last_attack) >= b.attack_ms {
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
            if b.mode == Mode::Hunt && cfg.roam && now.wrapping_sub(idle) > cfg.roam_idle_ms && now.wrapping_sub(b.last_roam) > 20_000 && self.bot_party_leader(h).is_none() {
                b.last_roam = now;
                if self.bot_roam(b, (pos, m), max_dice, false) {
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

    /// What its class lets it wear now: a mage only gives up its weapons once it has an attack spell to cast with.
    fn bot_role(&self, h: usize, b: &Bot) -> Role {
        let casts = self.clients[h].as_ref().is_some_and(|c| {
            self.data.magic.iter().enumerate().any(|(i, f)| c.magic_mastery.get(i) == Some(&1) && f.as_deref().and_then(Spell::parse).is_some_and(|sp| sp.category == 1 && matches!(sp.ty, 1 | 3) && sp.cost >= 0))
        });
        let shoots = self.clients[h].as_ref().is_some_and(|c| {
            let bow = usize::try_from(c.equip_status[EQUIPPOS_TWOHAND as usize]).ok().and_then(|k| c.items.get(k)?.as_ref()).is_some_and(|i| i.cfg.item_effect_type == ITEMEFFECTTYPE_ATTACK_ARROW);
            bow && arrows_of(c) > 0
        });
        role_of(b.arch, if b.arch == 4 { shoots } else { casts })
    }

    /// Whether a monster that attacks (hostile, alive, attack allowed) is within `r` tiles.
    fn bot_hostile_near(&self, m: usize, pos: (i16, i16), r: i16) -> bool {
        (-r..=r).any(|dx| {
            (-r..=r).any(|dy| {
                let (o, ot) = self.maps[m].get_owner(pos.0 + dx, pos.1 + dy);
                ot == OWNERTYPE_NPC && o > 0 && self.bot_prey(o as usize, m).is_some()
            })
        })
    }

    /// One step away from the monster at `from` (a tile 8 to 3 tiles off in the opposite direction that it can reach).
    /// False: nowhere to go.
    fn bot_flee(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), from: (i16, i16), now: u32) -> bool {
        let (pos, m) = at;
        let (dx, dy) = ((pos.0 - from.0).signum(), (pos.1 - from.1).signum());
        let (dx, dy) = if (dx, dy) == (0, 0) { (1, 0) } else { (dx, dy) };
        for r in (3..=8).rev() {
            let goal = (pos.0 + dx * r, pos.1 + dy * r);
            if self.maps[m].get_moveable(goal.0, goal.1).0 && self.bot_walk(h, b, cfg, at, goal, 0, None, 14, now) {
                return true;
            }
        }
        false
    }

    /// The healing spell it could cast now (Magic.cfg type 2, not for sale to nobody): the dearest it has the mana for.
    fn bot_heal_spell(&self, h: usize) -> Option<(usize, i32)> {
        let c = self.clients[h].as_ref()?;
        self.data
            .magic
            .iter()
            .enumerate()
            .filter(|&(i, _)| c.magic_mastery.get(i) == Some(&1))
            .filter_map(|(i, f)| Some((i, Spell::parse(f.as_deref()?)?)))
            .filter(|(_, sp)| sp.category == 0 && sp.ty == 2 && sp.mana <= c.mp && sp.cost >= 0)
            .max_by_key(|(_, sp)| sp.cost)
            .map(|(i, sp)| (i, sp.mana))
    }

    /// Heals itself or the weakest party mate (same map, within 8 tiles) below `heal-percent`. True: it cast.
    fn bot_heal(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, pos: (i16, i16), now: u32) -> bool {
        if !cfg.cast || now.wrapping_sub(b.last_cast) < 2500 {
            return false;
        }
        let Some((idx, _)) = self.bot_heal_spell(h) else { return false };
        let Some(c) = self.clients[h].as_ref() else { return false };
        let (m, pid) = (c.map_index, c.party_id);
        let mut best: Option<(i32, (i16, i16), String)> = None;
        let mates = if pid != 0 { self.party_members(pid) } else { Vec::new() };
        for j in std::iter::once(h).chain(mates) {
            let Some(o) = self.clients[j].as_ref() else { continue };
            if o.map_index != m || o.is_killed || cheb(pos, (o.x, o.y)) > 8 {
                continue;
            }
            let pct = o.hp * 100 / o.get_max_hp().max(1);
            if pct < cfg.heal_pct && best.as_ref().is_none_or(|(p, _, _)| pct < *p) {
                best = Some((pct, (o.x, o.y), String::from_utf8_lossy(crate::util::cstr(&o.char_name)).to_string()));
            }
        }
        let Some((pct, at, name)) = best else { return false };
        b.last_cast = now;
        log(&format!("(bot) {}: heals {name} ({pct} % HP)", b.ch));
        self.bot_cast_idx(h, pos, at, idx, now);
        true
    }

    /// The attack spell it could cast now: learned (mastery 1), a damage type, the strongest (the dearest) it has the mana for.
    /// (index, mana).
    fn bot_attack_spell(&self, h: usize) -> Option<(usize, i32)> {
        let c = self.clients[h].as_ref()?;
        self.data
            .magic
            .iter()
            .enumerate()
            .filter(|&(i, _)| c.magic_mastery.get(i) == Some(&1))
            .filter_map(|(i, f)| Some((i, Spell::parse(f.as_deref()?)?)))
            .filter(|(_, sp)| sp.category == 1 && matches!(sp.ty, 1 | 3) && sp.mana <= c.mp && sp.cost >= 0)
            .max_by_key(|(_, sp)| sp.cost)
            .map(|(i, sp)| (i, sp.mana))
    }

    /// The spell it could learn now: an attack spell first, then a healing one; not learned, within its Int and its gold less the reserve; the dearest.
    fn bot_spell_wish(&self, h: usize, gold: u32, reserve: u32, tried: &[Vec<u8>]) -> Option<(Vec<u8>, u32)> {
        let c = self.clients[h].as_ref()?;
        let budget = gold.checked_sub(reserve)? as i32;
        self.data
            .magic
            .iter()
            .enumerate()
            .filter(|&(i, _)| c.magic_mastery.get(i) == Some(&0))
            .filter_map(|(_, f)| {
                let f = f.as_deref()?;
                let sp = Spell::parse(f)?;
                let name = crate::util::cstr(f.get(1)?).to_vec();
                Some((sp, name, crate::util::atoi(f.get(16)?)))
            })
            .filter(|(sp, name, req_int)| ((sp.category == 1 && matches!(sp.ty, 1 | 3)) || (sp.category == 0 && sp.ty == 2)) && sp.cost >= 0 && sp.cost <= budget && *req_int <= c.get_int() && !tried.contains(name))
            .max_by_key(|(sp, _, _)| (sp.category == 1) as i32 * 100_000 + sp.cost)
            .map(|(sp, name, _)| (name, sp.cost as u32))
    }

    /// The precast motion and the cast at a tile: the two frames a client sends. False: no spell it could cast.
    fn bot_cast(&mut self, h: usize, pos: (i16, i16), at: (i16, i16), now: u32) -> bool {
        let Some((idx, _)) = self.bot_attack_spell(h) else { return false };
        self.bot_cast_idx(h, pos, at, idx, now);
        true
    }

    /// Spell `idx` at a tile: the precast motion and the cast, the two frames a client sends.
    fn bot_cast_idx(&mut self, h: usize, pos: (i16, i16), at: (i16, i16), idx: usize, now: u32) {
        let f = motion(OBJECTMAGIC, pos, dir_to(pos, at).max(1), now, Some((0, 0, 0, 0)));
        self.bot_send(h, f);
        let mut f = common(COMMONTYPE_MAGIC, pos, at.0 as u32);
        let mut w = W(&mut f);
        w.u32(15, at.1 as u32);
        w.u32(19, idx as u32 + 100);
        self.bot_send(h, f);
    }

    /// Spends the level-up points (3 a level) as the archetype wants (`POINTS`).
    fn bot_points(&mut self, h: usize, b: &mut Bot) {
        let pool = c_pool(self, h).min(30);
        let cycle = POINTS[b.arch as usize];
        let mut v = [0u8; 6];
        let (chr, citizen, level) = self.clients[h].as_ref().map_or((20, false, 1), |c| (c.range, !c.is_neutral(), c.level));
        let want_chr = if self.bots.cfg.guilds && citizen && level >= 8 { (20 - chr).clamp(0, pool) } else { 0 };
        v[5] = want_chr as u8;
        for _ in 0..pool - want_chr {
            v[cycle[b.pt_pos % 4]] += 1;
            b.pt_pos += 1;
        }
        let mut f = vec![0u8; 12];
        W(&mut f).u32(0, MSGID_LEVELUPSETTINGS);
        f[6..12].copy_from_slice(&v);
        log(&format!("(bot) {}: spends {pool} level-up points {v:?} (str vit dex int mag chr)", b.ch));
        self.bot_send(h, f);
    }

    /// A neutral bot at `enlist-level` goes for its nation's city.
    fn bot_maybe_enlist(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, now: u32) -> bool {
        let Some(c) = self.clients[h].as_ref() else { return false };
        if !cfg.enlist || !c.is_neutral() || c.level < cfg.enlist_level || now.wrapping_sub(b.last_enlist) < 60_000 {
            return false;
        }
        let level = c.level;
        b.last_enlist = now;
        let pick = match cfg.nation.as_str() {
            "aresden" => 0,
            "elvine" => 1,
            _ => self.dice(1, 2) - 1,
        };
        let name: &[u8] = if pick == 0 { b"aresden" } else { b"elvine" };
        let Some(city) = self.map_index(name) else {
            log(&format!("(bot) {}: no {} city on this server, not enlisting", b.ch, String::from_utf8_lossy(name)));
            return false;
        };
        log(&format!("(bot) {}: level {level}, going to {} to take its nationality", b.ch, String::from_utf8_lossy(name)));
        (b.mode, b.city, b.enlist_tries, b.target, b.no_route_logged) = (Mode::Enlist, Some(city), 0, 0, false);
        b.path.clear();
        true
    }

    /// Walks to the city, asks for the nationality (up to 3 times), then settles there as its new home.
    fn bot_enlist(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), now: u32) {
        let (pos, m) = at;
        let Some(city) = b.city else { return };
        let neutral = self.clients[h].as_ref().is_none_or(|c| c.is_neutral());
        if !neutral {
            log(&format!("(bot) {}: enlisted ({})", b.ch, self.clients[h].as_ref().map_or(String::new(), |c| String::from_utf8_lossy(crate::util::cstr(&c.location)).to_string())));
            (b.mode, b.home, b.wander, b.idle_since, b.ground_dice, b.target) = (Mode::Hunt, Some((m, pos)), None, None, 0, 0);
            b.path.clear();
            return;
        }
        if now.wrapping_sub(b.last_enlist) > 300_000 || b.enlist_tries >= 3 {
            log(&format!("(bot) {}: could not enlist, going back to hunting", b.ch));
            (b.mode, b.no_route_logged, b.last_enlist) = (Mode::Travel, false, now);
            b.path.clear();
            return;
        }
        if m != city {
            if !self.bot_goto(h, b, cfg, at, &|d| d == city, now) {
                log(&format!("(bot) {}: no teleport route to the city", b.ch));
                (b.mode, b.no_route_logged, b.last_enlist) = (Mode::Travel, false, now);
            }
            return;
        }
        if now.wrapping_sub(b.last_trade) >= 2500 {
            b.last_trade = now;
            b.enlist_tries += 1;
            let mut f = vec![0u8; 6];
            W(&mut f).u32(0, MSGID_REQUEST_CIVILRIGHT);
            self.bot_send(h, f);
        }
    }

    /// The best thing it could buy now with `gold - reserve`: armour for an empty slot first (the best defence), else a
    /// better weapon. (name, price). Not what it already carries to wear, nor what it tried.
    fn bot_wish(&self, h: usize, gold: u32, reserve: u32, tried: &[Vec<u8>], role: Role) -> Option<(Vec<u8>, u32)> {
        let c = self.clients[h].as_ref()?;
        let budget = gold.checked_sub(reserve)?;
        let eq = |pos: i32| c.equip_status.get(pos as usize).copied().unwrap_or(-1);
        let carried: Vec<(i32, i32)> = c.items.iter().enumerate().filter_map(|(k, i)| i.as_ref().filter(|i| i.cfg.item_type == 1 && eq(i.cfg.equip_pos) != k as i16).map(|i| (i.cfg.equip_pos, wear_score(c, i, role).unwrap_or(-1)))).collect();
        let (mut armour, mut weapon): (Option<(i32, &crate::gamedata::ItemCfg)>, Option<(i32, &crate::gamedata::ItemCfg)>) = (None, None);
        for cfg in self.data.items.iter().flatten() {
            if !cfg.is_for_sale || cfg.price == 0 || cfg.price as u32 > budget || cfg.item_type != 1 || tried.iter().any(|t| *t == cfg.name) {
                continue;
            }
            let it = Item::new(cfg);
            let Some(score) = wear_score(c, &it, role) else { continue };
            let pos = cfg.equip_pos;
            let is_weapon = pos == EQUIPPOS_RHAND || pos == EQUIPPOS_TWOHAND;
            if carried.iter().any(|&(p, s)| s >= 0 && ((p == pos) || (is_weapon && (p == EQUIPPOS_RHAND || p == EQUIPPOS_TWOHAND)) && s >= score)) {
                continue; // something as good is already in the bag, waiting to be worn
            }
            let slot = if is_weapon { &mut weapon } else { &mut armour };
            if slot.is_none_or(|(s, _)| score > s) {
                *slot = Some((score, cfg));
            }
        }
        armour.or(weapon).map(|(_, cfg)| (cfg.name.clone(), cfg.price as u32))
    }

    /// The highest HitDice of the monsters it fights (`hunt-level-step`): its level / step above 2, slimes at level 1.
    fn bot_max_dice(&self, h: usize, cfg: &BotCfg) -> i32 {
        let level = self.clients[h].as_ref().map_or(1, |c| c.level);
        if cfg.hunt_level_step > 0 { 2 + level / cfg.hunt_level_step } else { i32::MAX }
    }

    /// Nothing to fight in sight for a while: picks a hunting ground (one of the nearest live monsters it can beat, on
    /// this map or the nearest map of this server that has some), makes it the new home and goes there. False: none.
    fn bot_roam(&mut self, b: &mut Bot, at: ((i16, i16), usize), max_dice: i32, forced: bool) -> bool {
        self.bot_roam_to(b, at, max_dice, forced, false)
    }

    /// The hunting map: every reachable map (at most 4 teleports away) with monsters it can beat is a candidate, weighted by how
    /// many there are and how strong, less for each teleport, so that bots spread over the world instead of all taking the
    /// nearest map; `forced` (it levelled up) wants one with stronger monsters than it fights now, `leave` another map than this.
    /// Then the tile: one of the 5 nearest of the strongest it can beat there. True: it set off.
    fn bot_roam_to(&mut self, b: &mut Bot, at: ((i16, i16), usize), max_dice: i32, forced: bool, leave: bool) -> bool {
        let (pos, m) = at;
        let prey: Vec<(usize, (i16, i16), i32)> = self.npcs.iter().flatten().filter(|n| n.side >= SIDE_HOSTILE && !n.is_killed && n.hp > 0 && n.hit_dice <= max_dice && self.maps[n.map_index].is_attack_enabled).map(|n| (n.map_index, (n.x, n.y), n.hit_dice)).collect();
        let hops = self.bot_hops(m, 4);
        let mut per_map: Vec<(u32, i32)> = vec![(0, 0); self.maps.len()]; // (monsters, strongest HitDice)
        for &(pm, t, d) in &prey {
            if pm != m || cheb(t, pos) > 2 {
                per_map[pm] = (per_map[pm].0 + 1, per_map[pm].1.max(d));
            }
        }
        let mut cands: Vec<(usize, u32)> = Vec::new();
        for (d, &(n, best)) in per_map.iter().enumerate() {
            let Some(h) = hops[d] else { continue };
            if n == 0 || (forced && best <= b.ground_dice) || (leave && d == m) {
                continue;
            }
            // what it is worth: monsters (up to 30) times strength, nearer maps first
            cands.push((d, (n.min(30) * (1 + best as u32) * 10 / (1 + 3 * h as u32)).max(1)));
        }
        if cands.is_empty() {
            if !forced && !leave {
                log(&format!("(bot) {}: no monster it can beat on any map it can reach", b.ch));
            }
            return false;
        }
        let total: u32 = cands.iter().map(|c| c.1).sum();
        let mut roll = self.dice(1, total as i32) as u32;
        let gm = cands.iter().find(|c| {
            if roll <= c.1 {
                true
            } else {
                roll -= c.1;
                false
            }
        }).map_or(cands[0].0, |c| c.0);
        let mut here: Vec<(i16, i16, i32)> = prey.iter().filter(|&&(pm, t, _)| pm == gm && (gm != m || cheb(t, pos) > 2)).map(|&(_, t, d)| (t.0, t.1, d)).collect();
        let best = here.iter().map(|t| t.2).max();
        here.retain(|t| best.is_some_and(|d| t.2 >= d - 1));
        if gm == m {
            here.sort_by_key(|t| cheb((t.0, t.1), pos)); // on this map: the nearest, not all the same one
            here.truncate(5);
        }
        let Some(&(gx, gy, gd)) = (!here.is_empty()).then(|| &here[self.dice(1, here.len() as i32) as usize - 1]) else { return false };
        log(&format!(
            "(bot) {}: {}, new hunting ground {}({gx},{gy}) (HitDice {gd}, up to {max_dice})",
            b.ch,
            if forced { "stronger now" } else if leave { "time for another map" } else { "no monster in sight" },
            String::from_utf8_lossy(&self.maps[gm].name)
        ));
        (b.home, b.mode, b.no_route_logged, b.wander, b.idle_since, b.ground_dice) = (Some((gm, (gx, gy))), Mode::Travel, false, None, None, gd);
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
        } else if !self.bot_goto(h, b, cfg, at, &|d| d == home.0, now) {
            // no way back (a one-way teleport): this place becomes home, and the hunting ground search takes it from here
            log(&format!("(bot) {}: no teleport route from {} to {}, home is here now", b.ch, String::from_utf8_lossy(&self.maps[m].name), String::from_utf8_lossy(&self.maps[home.0].name)));
            (b.home, b.mode, b.idle_since, b.wander) = (Some((m, pos)), Mode::Hunt, None, None);
            b.path.clear();
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

    /// The maps of this server reachable from `from` through teleport tiles, with the number of teleports (at most `max`).
    fn bot_hops(&self, from: usize, max: u8) -> Vec<Option<u8>> {
        let mut hops = vec![None; self.maps.len()];
        hops[from] = Some(0);
        let mut q = VecDeque::from([from]);
        while let Some(a) = q.pop_front() {
            let d = hops[a].unwrap();
            if d >= max {
                continue;
            }
            for t in &self.maps[a].teleport_locs {
                if let Some(n) = self.map_index(&t.dest_map) {
                    if hops[n].is_none() {
                        hops[n] = Some(d + 1);
                        q.push_back(n);
                    }
                }
            }
        }
        hops
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
        if !cfg.shop {
            return false;
        }
        if b.shop_cd == 0 {
            // the first look comes after a random part of the cool-down: fifty bots do not walk into the shop together
            b.shop_cd = cfg.shop_cooldown_ms / 2 + self.dice(1, cfg.shop_cooldown_ms.max(1) as i32) as u32;
            b.last_shop = now.wrapping_sub(cfg.shop_cooldown_ms / 2);
        }
        if now.wrapping_sub(b.last_shop) < b.shop_cd {
            return false;
        }
        let (pos, m) = at;
        let role = self.bot_role(h, b);
        let (potions, scrolls) = (self.bot_count(h, is_potion), self.bot_count(h, is_recall_scroll));
        let bag = self.clients[h].as_ref().map_or(0, |c| c.items.iter().flatten().count());
        let gold = self.item_count(h, b"Gold").1;
        let worn = cfg.repair && self.clients[h].as_ref().is_some_and(|c| repair_slot(c, &b.equip_tried, &[], cfg.repair_pct, gold).is_some());
        let wish = cfg.buy_gear && self.bot_wish(h, gold, cfg.gear_reserve, &b.wish_tried, role).is_some();
        let spell = cfg.learn_spells && self.bot_spell_wish(h, gold, cfg.gear_reserve, &b.spell_tried).is_some();
        let arrows_low = role.archer && self.clients[h].as_ref().is_some_and(|c| arrows_of(c) < cfg.arrow_min && has_bow(c)) && gold >= 200;
        let level = self.clients[h].as_ref().map_or(1, |c| c.level);
        let low = arrows_low || (cfg.use_potions && potions < potion_wants(cfg, level).0) || (cfg.use_recall && scrolls < cfg.shop_scroll_min) || worn || wish || spell;
        let full = cfg.sell_loot && bag >= cfg.shop_bag_slots;
        if !low && !full {
            return false;
        }
        let neutral = self.clients[h].as_ref().is_some_and(|c| c.is_neutral());
        let mut wealth = self.item_count(h, b"Gold").1 as i64;
        if cfg.sell_loot {
            wealth += self.clients[h].as_ref().map_or(0, |c| c.items.iter().enumerate().filter_map(|(k, i)| i.as_ref().filter(|i| is_loot(i) || is_surplus(c, k, i, &b.equip_tried, role))).map(|i| sell_price(i, i.count as i32, neutral).unwrap_or(0) as i64).sum::<i64>());
        }
        let cheapest = self.data.item_by_name(cfg.shop_potion_item.as_bytes()).map_or(30, |i| i.price as i64);
        b.last_shop = now; // whatever happens below, the next look is after the cool-down
        b.shop_cd = cfg.shop_cooldown_ms / 2 + self.dice(1, cfg.shop_cooldown_ms.max(1) as i32) as u32;
        if !full && wealth < cheapest {
            log(&format!("(bot) {}: low on supplies ({potions} potions, {scrolls} scrolls) but poor (~{wealth} gold)", b.ch));
            return false;
        }
        // a shop it can leave again: back home, or at least to a map with monsters (gshop_2f has no way out)
        let back = b.home.map_or(m, |h| h.0);
        let mut has = vec![false; self.maps.len()];
        self.npcs.iter().flatten().filter(|n| n.side >= SIDE_HOSTILE && !n.is_killed && self.maps[n.map_index].is_attack_enabled).for_each(|n| has[n.map_index] = true);
        let Some((_, shop)) = self.bot_route(m, &|d| self.maps[d].name.starts_with(b"gshop") && (d == back || self.bot_route(d, &|x| x == back || has[x], pos).is_some()), pos) else {
            log(&format!("(bot) {}: no shop (gshop*) reachable from {}", b.ch, String::from_utf8_lossy(&self.maps[m].name)));
            return false;
        };
        log(&format!("(bot) {}: shopping trip to {} ({potions} potions, {scrolls} scrolls, {bag} items, ~{wealth} gold)", b.ch, String::from_utf8_lossy(&self.maps[shop].name)));
        (b.mode, b.shop_map, b.shop_start, b.buy_fails, b.last_have, b.target) = (Mode::Shop, shop, now, 0, 0, 0);
        (b.sell_tried, b.repair_tried, b.wish_buys) = (Vec::new(), Vec::new(), 0);
        b.path.clear();
        true
    }

    /// The shopping trip: go to the shop map, sell the loot one item at a time, buy up to the targets, then go home.
    fn bot_shop(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, at: ((i16, i16), usize), now: u32) {
        let (pos, m) = at;
        let role = self.bot_role(h, b);
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
            let slot = junk_slots(c, b, role, now).into_iter().find(|k| c.items[*k].as_ref().is_some_and(|i| !b.sell_tried.contains(&(*k, i.count)))).map(|k| (k, &c.items[k]));
            if let Some((k, Some(it))) = slot {
                let n = it.count;
                b.sell_tried.push((k, n));
                b.equip_tried.retain(|&t| t != k);
                b.last_trade = now;
                log(&format!("(bot) {}: sells {} x{n}", b.ch, String::from_utf8_lossy(&it.cfg.name)));
                let mut f = common(COMMONTYPE_REQ_SELLITEMCONFIRM, pos, k as u32);
                W(&mut f).u32(15, n);
                self.bot_send(h, f);
                return;
            }
        }
        let gold = self.item_count(h, b"Gold").1;
        let wants = [(cfg.use_potions, &cfg.shop_potion_item, potion_wants(cfg, self.clients[h].as_ref().map_or(1, |c| c.level)).1, self.bot_count(h, is_potion)), (cfg.use_recall, &cfg.shop_scroll_item, cfg.shop_scroll_target, self.bot_count(h, is_recall_scroll))];
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
        if role.archer && b.buy_fails < 3 && self.clients[h].as_ref().is_some_and(|c| arrows_of(c) < cfg.arrow_min && has_bow(c)) && gold >= 150 {
            b.buy_fails += 1;
            b.last_trade = now;
            log(&format!("(bot) {}: buys 100 arrows ({gold} gold)", b.ch));
            let mut f = common(COMMONTYPE_REQ_PURCHASEITEM, pos, 1);
            W(&mut f).bytes(23, b"100Arrows");
            self.bot_send(h, f);
            return;
        }
        if cfg.buy_gear && b.wish_buys < 4 {
            if let Some((name, price)) = self.bot_wish(h, gold, cfg.gear_reserve, &b.wish_tried, role) {
                log(&format!("(bot) {}: buys {} ({price} of {gold} gold)", b.ch, String::from_utf8_lossy(&name)));
                (b.wish_buys, b.last_trade) = (b.wish_buys + 1, now);
                b.wish_tried.push(name.clone());
                let mut f = common(COMMONTYPE_REQ_PURCHASEITEM, pos, 1);
                W(&mut f).bytes(23, &name);
                self.bot_send(h, f);
                return;
            }
        }
        if cfg.learn_spells {
            if let Some((name, cost)) = self.bot_spell_wish(h, gold, cfg.gear_reserve, &b.spell_tried) {
                log(&format!("(bot) {}: learns {} ({cost} of {gold} gold)", b.ch, String::from_utf8_lossy(&name)));
                b.last_trade = now;
                b.spell_tried.push(name.clone());
                let mut f = common(COMMONTYPE_REQ_STUDYMAGIC, pos, 0);
                W(&mut f).bytes(23, &name);
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

    /// Puts bag slot `k` (the whole stack) on the ground where it stands.
    fn bot_drop_slot(&mut self, h: usize, b: &Bot, k: usize, pos: (i16, i16)) {
        let Some(name) = self.clients[h].as_ref().and_then(|c| c.items.get(k)?.as_ref()).map(|i| i.cfg.name.clone()) else { return };
        log(&format!("(bot) {}: drops {} (slot {k}) on the ground", b.ch, String::from_utf8_lossy(crate::util::cstr(&name))));
        let mut f = common(COMMONTYPE_ITEMDROP, pos, k as u32);
        let mut w = W(&mut f);
        w.u32(15, u32::MAX); // the whole stack
        w.bytes(23, &name);
        self.bot_send(h, f);
    }

    /// Every `drop-check-ms` there is a `drop-percent` chance that it leaves one thing it does not care about on the ground
    /// (a player empties his bag like that now and then). The tile is left alone by its own looting for 10 minutes.
    fn bot_maybe_drop(&mut self, h: usize, b: &mut Bot, cfg: &BotCfg, pos: (i16, i16), now: u32) -> bool {
        if cfg.drop_pct == 0 || now.wrapping_sub(b.last_drop_check) < cfg.drop_check_ms {
            return false;
        }
        b.last_drop_check = now;
        if self.dice(1, 100) > cfg.drop_pct {
            return false;
        }
        let role = self.bot_role(h, b);
        let Some(c) = self.clients[h].as_ref() else { return false };
        let junk = junk_slots(c, b, role, now);
        if junk.is_empty() {
            return false;
        }
        let k = junk[self.dice(1, junk.len() as i32) as usize - 1];
        self.bot_drop_slot(h, b, k, pos);
        b.loot_skip.push((pos, now.wrapping_add(600_000)));
        true
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
        } else if now.wrapping_sub(b.last_attack) >= b.attack_ms {
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
    fn bot_walk(&mut self, h: usize, b: &mut Bot, _cfg: &BotCfg, at: ((i16, i16), usize), goal: (i16, i16), reach: i16, allow: Option<(i16, i16)>, limit: i16, now: u32) -> bool {
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
        if now.wrapping_sub(b.last_step) >= b.step_ms {
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
        let sur = |k: usize, tried: &[usize]| is_surplus(&c, k, c.items[k].as_ref().unwrap(), tried, Role::default());
        assert!(!sur(0, &[]) && sur(1, &[]) && !sur(2, &[]) && !sur(3, &[]) && sur(4, &[]) && !sur(5, &[]));
        assert!(sur(3, &[3]), "a piece the server refused is surplus");
        // a necklace is never worn by the bot; armour (category 11) is not "loot" and goes through the same rules
        let mut neck = gear(EQUIPPOS_NECK, 1);
        neck.cfg.category = 46;
        assert!(is_surplus(&c, 6, &neck, &[], Role::default()) && !is_loot(&neck));
        // a broken piece is repaired, not sold
        let mut broken = gear(EQUIPPOS_HEAD, 1);
        (broken.max_life_span, broken.cur_life_span) = (100, 0);
        assert!(!is_surplus(&c, 6, &broken, &[], Role::default()));
    }

    #[test]
    fn paces_follow_the_mix_and_stay_legal() {
        let mut cfg = BotCfg { count: 50, ..Default::default() };
        let bots = Bots::new(cfg.clone());
        let n = |p: usize| bots.list.iter().filter(|b| b.pace == p).count();
        // 25 / 35 / 40 %: the hash spreads the slots, so within a few bots of the ideal
        assert!((n(0) as i32 - 12).abs() <= 5 && (n(1) as i32 - 17).abs() <= 6 && (n(2) as i32 - 20).abs() <= 6, "{} {} {}", n(0), n(1), n(2));
        assert!(bots.list.iter().all(|b| b.step_ms >= MIN_STEP_MS && b.attack_ms >= MIN_ATTACK_MS && (4..=14).contains(&b.sight)));
        // a slow bot is slower than a driven one, and no two are alike
        let avg = |p: usize| bots.list.iter().filter(|b| b.pace == p).map(|b| b.step_ms).sum::<u32>() / n(p).max(1) as u32;
        assert!(avg(0) > avg(1) && avg(1) > avg(2));
        assert!(bots.list.iter().map(|b| b.step_ms).collect::<std::collections::HashSet<_>>().len() > 10);
        // one weight only: every bot has that pace
        cfg.pace_mix = [0, 0, 1];
        assert!(Bots::new(cfg).list.iter().all(|b| b.pace == 2));
    }

    #[test]
    fn junk_is_loot_and_spare_gear_but_not_what_is_being_worn() {
        let (tx, _rx) = unbounded_channel();
        let mut c = Client::new(1, 1, tx, b"1.2.3.4".to_vec(), 0);
        let b = Bot::default();
        let mut jelly = Item::new(&Default::default());
        (jelly.cfg.item_type, jelly.cfg.category, jelly.cfg.name) = (crate::life::ITEMTYPE_CONSUME, 20, b"Jelly".to_vec());
        let mut potion = jelly.clone();
        (potion.cfg.item_type, potion.cfg.item_effect_type, potion.cfg.name) = (ITEMTYPE_USE_DEPLETE, ITEMEFFECTTYPE_HP, b"RedPotion".to_vec());
        let mut spare = Item::new(&Default::default());
        (spare.cfg.item_type, spare.cfg.category, spare.cfg.equip_pos, spare.cfg.item_effect_value[0], spare.cfg.item_effect_value[1]) = (1, 1, EQUIPPOS_RHAND, 1, 1);
        (spare.max_life_span, spare.cur_life_span) = (100, 100);
        let mut worn = spare.clone();
        worn.cfg.item_effect_value[1] = 9;
        (c.items[0], c.items[1], c.items[2], c.items[3]) = (Some(jelly), Some(potion), Some(worn), Some(spare));
        c.equip_status[EQUIPPOS_RHAND as usize] = 2;
        assert_eq!(junk_slots(&c, &b, Role::default(), 100_000), vec![0, 3], "the jelly and the weaker spare weapon: not the potion, not the wielded one");
        let b = Bot { last_equip_slot: (3, 99_000), ..Default::default() };
        assert_eq!(junk_slots(&c, &b, Role::default(), 100_000), vec![0], "a piece it has just sent the equip command for is not junk yet");
    }

    #[test]
    fn classes_differ_in_points_stats_looks_and_gear() {
        let cfg = BotCfg { count: 10, ..Default::default() };
        let arch: Vec<u8> = (1..=10).map(|i| arch_of(&cfg, i)).collect();
        assert_eq!(arch, vec![0, 2, 3, 1, 4, 0, 0, 2, 3, 1], "warrior, knight, duelist, mage, archer, warrior, cycling");
        for st in START_STATS {
            assert!(st.iter().sum::<u8>() == 70 && st.iter().all(|v| (10..=14).contains(v)), "{st:?}: the Login only takes 10-14 each and 70 in all");
        }
        let looks: std::collections::HashSet<[u8; 5]> = (1..=50).map(look_of).collect();
        assert!(looks.len() > 30, "{} different looks in 50", looks.len());
        assert!((1..=50).map(look_of).all(|l| (1..=2).contains(&l[0]) && (1..=3).contains(&l[1]) && l[2] < 8 && l[3] < 16 && l[4] < 8));
        // a knight takes no two-handed weapon, a duelist neither shield nor two-handed
        assert!(role_of(2, false).refuses(EQUIPPOS_TWOHAND) && !role_of(2, false).refuses(EQUIPPOS_LHAND));
        assert!(role_of(3, false).refuses(EQUIPPOS_LHAND) && role_of(3, false).refuses(EQUIPPOS_TWOHAND) && !role_of(3, false).refuses(EQUIPPOS_RHAND));
        assert!(!role_of(0, false).refuses(EQUIPPOS_TWOHAND) && !role_of(1, false).refuses(EQUIPPOS_RHAND));
    }

    #[test]
    fn chat_answers_greetings_in_the_language_it_hears() {
        assert_eq!(chat_kind("hola", false, false), Some((false, 0)));
        assert_eq!(chat_kind("Hello there", false, false), Some((true, 0)));
        assert_eq!(chat_kind("que tal?", false, true), Some((false, 1)), "a whisper is answered whatever it says");
        assert_eq!(chat_kind("help me please", true, false), Some((true, 2)));
        assert_eq!(chat_kind("quieres un grupo", true, false), Some((false, 3)));
        assert_eq!(chat_kind("vendo espada", false, false), None, "a line nobody is talking to a bot with is not answered");
        assert_eq!(chat_kind("vendo espada", false, true), Some((false, 4)));
        assert_eq!(pick(&HI_ES, 1), HI_ES[0]);
    }

    #[test]
    fn an_archer_wears_bows_and_shoots_with_empty_off_hand() {
        let (tx, _rx) = unbounded_channel();
        let mut c = Client::new(1, 1, tx, b"1.2.3.4".to_vec(), 0);
        c.str_ = 10;
        let mut bow = Item::new(&Default::default());
        (bow.cfg.item_type, bow.cfg.category, bow.cfg.equip_pos, bow.cfg.item_effect_type, bow.cfg.item_effect_value[0], bow.cfg.item_effect_value[1]) = (1, 1, EQUIPPOS_TWOHAND, ITEMEFFECTTYPE_ATTACK_ARROW, 1, 6);
        (bow.max_life_span, bow.cur_life_span) = (100, 100);
        let mut sword = bow.clone();
        (sword.cfg.equip_pos, sword.cfg.item_effect_type) = (EQUIPPOS_RHAND, ITEMEFFECTTYPE_ATTACK);
        let archer = role_of(4, false);
        assert!(wear_score(&c, &bow, archer).is_some(), "an archer wears a bow");
        assert!(wear_score(&c, &sword, archer).is_some(), "with no bow yet it fights with a sword");
        assert!(wear_score(&c, &sword, role_of(4, true)).is_none(), "and with a bow ready, not with a sword");
        assert!(wear_score(&c, &bow, role_of(0, false)).is_none(), "a warrior has no use for it");
        assert!(is_surplus(&c, 0, &sword, &[], role_of(4, true)), "with a bow ready the spare sword is surplus");
        assert!(role_of(4, true).refuses(EQUIPPOS_RHAND) && !role_of(4, false).refuses(EQUIPPOS_RHAND), "the dagger stays until the bow and arrows are there");
        assert!(role_of(4, false).refuses(EQUIPPOS_LHAND), "no shield with a two-handed bow");
    }

    #[test]
    fn a_clearly_better_armour_piece_replaces_the_worn_one_and_gold_buys_more_potions_later() {
        let (tx, _rx) = unbounded_channel();
        let mut c = Client::new(1, 1, tx, b"1.2.3.4".to_vec(), 0);
        c.str_ = 10;
        let piece = |def: i32| {
            let mut it = Item::new(&Default::default());
            (it.cfg.item_type, it.cfg.category, it.cfg.equip_pos, it.cfg.item_effect_value[0], it.cfg.item_effect_value[3]) = (1, 6, EQUIPPOS_BODY, def, 0);
            (it.max_life_span, it.cur_life_span) = (100, 100);
            it
        };
        (c.items[0], c.items[1], c.items[2], c.items[3]) = (Some(piece(10)), Some(piece(11)), Some(piece(14)), Some(piece(30)));
        c.equip_status[EQUIPPOS_BODY as usize] = 0;
        let role = Role::default();
        let ws = |k: usize| wear_score(&c, c.items[k].as_ref().unwrap(), role);
        assert_eq!((ws(1), ws(2), ws(3)), (None, Some(4), Some(20)), "11 over 10 is not worth the change, 14 is (a fifth and two points), 30 more");
        assert!(is_surplus(&c, 1, c.items[1].as_ref().unwrap(), &[], role), "the marginal piece is surplus");
        assert!(!is_surplus(&c, 3, c.items[3].as_ref().unwrap(), &[], role), "the better one is kept to be worn");
        let cfg = BotCfg::default();
        assert_eq!(potion_wants(&cfg, 1), (cfg.shop_potion_min, cfg.shop_potion_target));
        assert!(potion_wants(&cfg, 40).0 > cfg.shop_potion_min && potion_wants(&cfg, 40).1 > potion_wants(&cfg, 10).1 && potion_wants(&cfg, 180).1 <= 60);
    }

    #[test]
    fn a_mage_keeps_its_hands_empty() {
        let (tx, _rx) = unbounded_channel();
        let mut c = Client::new(1, 1, tx, b"1.2.3.4".to_vec(), 0);
        c.str_ = 10;
        let mut w = Item::new(&Default::default());
        (w.cfg.item_type, w.cfg.category, w.cfg.equip_pos, w.cfg.item_effect_type, w.cfg.item_effect_value[0], w.cfg.item_effect_value[1]) = (1, 1, EQUIPPOS_RHAND, ITEMEFFECTTYPE_ATTACK, 1, 6);
        (w.max_life_span, w.cur_life_span) = (100, 100);
        assert!(wear_score(&c, &w, Role::default()).is_some(), "a warrior wields it");
        let mage = role_of(1, true);
        assert!(wear_score(&c, &w, mage).is_none() && is_surplus(&c, 0, &w, &[], mage), "a mage that can cast neither wears nor keeps it");
        assert!(wear_score(&c, &w, role_of(1, false)).is_some(), "a mage that cannot cast yet fights with it");
        let mut shield = w.clone();
        shield.cfg.equip_pos = EQUIPPOS_LHAND;
        assert!(wear_score(&c, &shield, mage).is_none());
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

    // ---------------------------------------------------------------- tests on a real Game (a small world)

    use crate::config::Settings;
    use crate::map::{Map, TeleportLoc};
    use crate::npc::Npc;

    fn world(maps: Vec<Map>) -> (Game, tokio::sync::mpsc::Receiver<Ev>) {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        (Game::new(Default::default(), Settings::default(), maps, tx, String::new(), Some(1)), rx)
    }

    /// A level 20 player with full HP on map `map`.
    fn add(g: &mut Game, h: usize, name: &str, side: i32, map: usize, pos: (i16, i16)) {
        let (tx, _rx) = unbounded_channel();
        let mut c = Client::new(h, h as u64, tx, b"1.2.3.4".to_vec(), 0);
        c.char_name = name.as_bytes().to_vec();
        (c.is_init_complete, c.side, c.map_index, c.x, c.y, c.level) = (true, side, map, pos.0, pos.1, 20);
        (c.str_, c.vit, c.dex, c.int_, c.mag) = (20, 20, 20, 10, 10);
        c.hp = c.get_max_hp();
        g.clients[h] = Some(c);
    }

    fn open_map(name: &[u8]) -> Map {
        Map::new(name, 100, 100)
    }

    /// The frames the bots queued, as (msgid, command, the target field of a motion).
    fn queued(g: &Game) -> Vec<(u32, u16, u16)> {
        g.queue.iter().map(|m| (Rd::new(&m.body).u32(0), Rd::new(&m.body).u16(4), if m.body.len() >= 23 { Rd::new(&m.body).u16(21) } else { 0 })).collect()
    }

    fn item(name: &[u8], count: u32) -> Item {
        let mut it = Item::new(&Default::default());
        (it.cfg.name, it.count) = (name.to_vec(), count);
        it
    }

    fn raider(g: &mut Game, cfg: &BotCfg, n: usize) -> Bot {
        let mut b = Bot::made(cfg, n);
        let now = 100_000;
        (b.mode, b.raid_map, b.raid_start, b.last_attack, b.last_step) = (Mode::Raid, 0, now, 0, 0);
        let _ = g;
        b
    }

    #[test]
    fn an_archer_on_a_raid_shoots_at_the_rival_player_not_at_npc_zero() {
        let (mut g, _rx) = world(vec![open_map(b"default")]);
        add(&mut g, 1, "Bot05", ARESDEN, 0, (40, 40));
        add(&mut g, 2, "Rival", ELVINE, 0, (43, 40));
        add(&mut g, 3, "Friend", ARESDEN, 0, (41, 40)); // nearer, but of its own side
        {
            let c = g.clients[1].as_mut().unwrap();
            let mut bow = item(b"LongBow", 1);
            (bow.cfg.item_type, bow.cfg.equip_pos, bow.cfg.item_effect_type, bow.max_life_span, bow.cur_life_span) = (1, EQUIPPOS_TWOHAND, ITEMEFFECTTYPE_ATTACK_ARROW, 100, 100);
            let mut arrows = item(b"Arrow", 20);
            arrows.cfg.item_type = crate::combat::ITEMTYPE_ARROW;
            (c.items[0], c.items[1], c.equip_status[EQUIPPOS_TWOHAND as usize], c.appr[1]) = (Some(bow), Some(arrows), 0, 0xF000u16 as i16);
        }
        let cfg = BotCfg { count: 6, ..Default::default() };
        let mut b = raider(&mut g, &cfg, 5);
        assert_eq!(b.arch, 4, "bot 5 is the archer");
        assert!(g.bot_role(1, &b).shoot, "a bow on and arrows in the bag");
        let (hp, max) = (g.clients[1].as_ref().unwrap().hp, g.clients[1].as_ref().unwrap().get_max_hp());
        g.bot_raid(1, &mut b, &cfg, ((40, 40), 0), (hp, max), 100_000);
        assert_eq!(b.raid_target, 2, "the rival, not the player of its own side");
        let q = queued(&g);
        assert_eq!(q.len(), 1);
        assert_eq!((q[0].0, q[0].1, q[0].2), (MSGID_COMMAND_MOTION, OBJECTATTACK as u16, 2), "an arrow at client 2 (before the fix: 10000 + npc 0)");
        // it gives up on a target that does not lose HP (the check reads the client's HP, not an npc's)
        g.queue.clear();
        (b.attacks, b.target_hp0) = (cfg.giveup_hits * 2, g.clients[2].as_ref().unwrap().hp);
        g.bot_raid(1, &mut b, &cfg, ((40, 40), 0), (hp, max), 101_000);
        assert_eq!((b.raid_target, b.raid_ignore.len()), (0, 1), "gave up and left that player alone");
        assert!(queued(&g).iter().all(|q| q.1 != OBJECTATTACK as u16), "no more arrows at it");
        // and it does not take the same player again while the time is not up
        g.bot_raid(1, &mut b, &cfg, ((40, 40), 0), (hp, max), 102_000);
        assert_eq!(b.raid_target, 0);
    }

    #[test]
    fn a_raid_fights_in_melee_walks_to_the_rival_and_ends_when_the_time_is_up() {
        let (mut g, _rx) = world(vec![open_map(b"default")]);
        add(&mut g, 1, "Bot01", ARESDEN, 0, (40, 40));
        add(&mut g, 2, "Rival", ELVINE, 0, (41, 40));
        g.clients[1].as_mut().unwrap().appr[1] = 0xF000u16 as i16;
        let cfg = BotCfg { count: 6, ..Default::default() };
        let mut b = raider(&mut g, &cfg, 1);
        let (hp, max) = (g.clients[1].as_ref().unwrap().hp, g.clients[1].as_ref().unwrap().get_max_hp());
        g.bot_raid(1, &mut b, &cfg, ((40, 40), 0), (hp, max), 100_000);
        assert_eq!(queued(&g), vec![(MSGID_COMMAND_MOTION, OBJECTATTACK as u16, 2)], "next to the rival: a blow at client 2");
        // far: it walks
        g.queue.clear();
        g.clients[2].as_mut().unwrap().x = 46;
        b.raid_target = 0;
        g.bot_raid(1, &mut b, &cfg, ((40, 40), 0), (hp, max), 105_000);
        assert_eq!(queued(&g).iter().map(|q| q.1).collect::<Vec<_>>(), vec![OBJECTMOVE as u16]);
        // the time is up: it goes home
        g.bot_raid(1, &mut b, &cfg, ((40, 40), 0), (hp, max), 100_000 + cfg.raid_ms + 1);
        assert_eq!(b.mode, Mode::Travel);
        // hurt: it ends the raid too
        b.mode = Mode::Raid;
        g.bot_raid(1, &mut b, &cfg, ((40, 40), 0), (1, max), 100_500);
        assert_eq!(b.mode, Mode::Travel, "below tired-percent");
    }

    #[test]
    fn a_citizen_of_level_11_takes_a_quest_after_the_cool_down_and_goes_on_with_the_one_it_has() {
        let (mut g, _rx) = world(vec![open_map(b"default")]);
        add(&mut g, 1, "Bot01", ARESDEN, 0, (40, 40));
        let cfg = BotCfg { count: 2, ..Default::default() };
        let mut b = Bot::made(&cfg, 1);
        g.clients[1].as_mut().unwrap().side = crate::client::NEUTRAL;
        assert!(!g.bot_maybe_quest(1, &mut b, &cfg, 100_000), "a neutral character is not asked");
        (g.clients[1].as_mut().unwrap().side, g.clients[1].as_mut().unwrap().level) = (ARESDEN, 5);
        assert!(!g.bot_maybe_quest(1, &mut b, &cfg, 100_000), "level 5 is too low");
        g.clients[1].as_mut().unwrap().level = 12;
        assert!(!g.bot_maybe_quest(1, &mut b, &cfg, 100_000), "the first look only starts the cool-down");
        assert!(!g.bot_maybe_quest(1, &mut b, &cfg, 100_001), "inside the cool-down");
        // after it, one look in two: within a few looks it goes
        let mut t = 100_000;
        let went = (0..30).any(|_| {
            t += cfg.quest_minutes * 60_000 + 1;
            g.bot_maybe_quest(1, &mut b, &cfg, t)
        });
        assert!(went && b.mode == Mode::Quest && b.qphase == 0, "goes to the city hall");
        // with a quest already in the log it goes on at once, hunting
        let mut b2 = Bot::made(&cfg, 2);
        g.clients[1].as_mut().unwrap().quest = 3;
        assert!(g.bot_maybe_quest(1, &mut b2, &cfg, 100_000) || g.bot_maybe_quest(1, &mut b2, &cfg, 100_000 + 1));
        assert_eq!((b2.mode, b2.qphase), (Mode::Quest, 2), "back to the hunt phase");
        // quests off: never
        let off = BotCfg { quests: false, ..cfg.clone() };
        let mut b3 = Bot::made(&off, 1);
        assert!(!g.bot_maybe_quest(1, &mut b3, &off, 9_999_999));
    }

    #[test]
    fn a_poor_bot_does_not_go_shopping_and_a_rich_one_with_no_potions_does() {
        let mut default = open_map(b"default");
        default.teleport_locs.push(TeleportLoc { src: (20, 20), dest_map: b"gshop_1".to_vec(), dest: (5, 5), dir: 1 });
        let mut shop = open_map(b"gshop_1");
        shop.teleport_locs.push(TeleportLoc { src: (5, 6), dest_map: b"default".to_vec(), dest: (20, 21), dir: 1 });
        let (mut g, _rx) = world(vec![default, shop]);
        add(&mut g, 1, "Bot01", ARESDEN, 0, (40, 40));
        let cfg = BotCfg { count: 2, ..Default::default() };
        let mut b = Bot::made(&cfg, 1);
        (b.shop_cd, b.last_shop, b.home) = (1, 0, Some((0, (40, 40))));
        assert!(!g.bot_maybe_shop(1, &mut b, &cfg, ((40, 40), 0), 100_000), "no gold, nothing to sell");
        assert_eq!(b.mode, Mode::Hunt);
        g.clients[1].as_mut().unwrap().items[0] = Some(item(b"Gold", 500));
        (b.shop_cd, b.last_shop) = (1, 0);
        assert!(g.bot_maybe_shop(1, &mut b, &cfg, ((40, 40), 0), 100_000), "gold and no red potion");
        assert_eq!((b.mode, b.shop_map), (Mode::Shop, 1));
        // the cool-down holds the next trip back
        b.mode = Mode::Hunt;
        assert!(!g.bot_maybe_shop(1, &mut b, &cfg, ((40, 40), 0), 100_001));
        // shops off: never
        let off = BotCfg { shop: false, ..cfg.clone() };
        (b.shop_cd, b.last_shop) = (1, 0);
        assert!(!g.bot_maybe_shop(1, &mut b, &off, ((40, 40), 0), 9_999_999));
    }

    #[test]
    fn the_maps_within_reach_are_counted_in_teleports() {
        let mut maps: Vec<Map> = ["a", "b", "c", "d", "island"].iter().map(|n| open_map(n.as_bytes())).collect();
        for (from, to) in [(0usize, "b"), (1, "c"), (2, "d")] {
            maps[from].teleport_locs.push(TeleportLoc { src: (1, 1), dest_map: to.as_bytes().to_vec(), dest: (2, 2), dir: 1 });
        }
        let (g, _rx) = world(maps);
        assert_eq!(g.bot_hops(0, 2), vec![Some(0), Some(1), Some(2), None, None], "d is 3 teleports away, the island none");
        assert_eq!(g.bot_hops(0, 4)[3], Some(3));
        assert_eq!(g.bot_hops(3, 4), vec![None, None, None, Some(0), None], "one-way: nothing from d");
    }

    #[test]
    fn a_party_follows_its_lowest_bot_and_fights_what_a_mate_fights() {
        let (mut g, _rx) = world(vec![open_map(b"default")]);
        let cfg = BotCfg { count: 10, ..Default::default() };
        g.bots = Bots::new(cfg.clone());
        add(&mut g, 2, "Human", ARESDEN, 0, (5, 5));
        add(&mut g, 3, "Bot03", ARESDEN, 0, (75, 75));
        add(&mut g, 5, "Bot05", ARESDEN, 0, (40, 40));
        add(&mut g, 9, "Bot09", ARESDEN, 0, (42, 40));
        for h in [2, 3, 5, 9] {
            g.clients[h].as_mut().unwrap().party_id = 7;
        }
        g.parties.insert(7, vec![2, 3, 5, 9]);
        assert_eq!(g.bot_party_leader(5), Some(3), "a human with a lower handle is not the leader");
        assert_eq!(g.bot_party_leader(9), Some(3));
        assert_eq!(g.bot_party_leader(3), None, "the leader follows nobody");
        // the mate far from the leader goes to it, while the leader hunts
        let mut b = Bot::made(&cfg, 5);
        g.bots.list[2].mode = Mode::Rest;
        assert!(!g.bot_follow(5, &mut b, &cfg, ((40, 40), 0), 100_000), "the leader is not hunting");
        g.bots.list[2].mode = Mode::Hunt;
        assert!(g.bot_follow(5, &mut b, &cfg, ((40, 40), 0), 100_000));
        assert_eq!((b.mode, b.home), (Mode::Travel, Some((0, (75, 75)))));
        // close to it: it stays
        let mut near = Bot::made(&cfg, 9);
        g.clients[9].as_mut().unwrap().x = 70;
        g.clients[9].as_mut().unwrap().y = 75;
        assert!(!g.bot_follow(9, &mut near, &cfg, ((70, 75), 0), 100_000));
        // the leader fights npc 7: the others take it, unless they gave up on it
        g.npcs[7] = Some(Npc { map_index: 0, x: 72, y: 75, side: SIDE_HOSTILE, hp: 10, ..Default::default() });
        g.bots.list[2].target = 7;
        assert_eq!(g.bot_assist(9, 0, (70, 75), 8, &[]), Some(7));
        assert_eq!(g.bot_assist(9, 0, (70, 75), 8, &[(7, 200_000)]), None);
        assert_eq!(g.bot_assist(9, 0, (40, 40), 8, &[]), None, "too far from the monster");
        g.clients[9].as_mut().unwrap().party_id = 0;
        assert_eq!(g.bot_assist(9, 0, (70, 75), 8, &[]), None, "not in a party");
    }
}
