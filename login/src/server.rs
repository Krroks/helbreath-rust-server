//! Port of Login/LoginServer.cpp, GameServer.cpp and char/Client.cpp.
//! One task owns this struct and handles one event at a time, like the C++ window-message loop.
//! Socket slots mirror the C++ arrays (`ClientSocket[500]`, `GameServerSocket[100]`, `GameServer[20]`,
//! `Client[500]`); emptying a slot drops its sender, which flushes and closes the connection.
use crate::db::{Db, DbCfg, QRes};
use crate::log::*;
use crate::msg::*;
use crate::party::{self, PartyManager};
use crate::util::*;
use chrono::{Datelike, Local, Timelike};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

pub type Tx = UnboundedSender<Vec<u8>>;

pub enum Ev {
    Open { gs: bool, conn: u64, peer: String, tx: Tx },
    Frame { gs: bool, conn: u64, frame: Vec<u8> },
    Closed { gs: bool, conn: u64 },
    /// stdin command standing in for the F1+F4 / F1+F5 hotkeys
    Cmd(String),
}

pub struct Sock {
    conn: u64,
    tx: Tx,
    peer: String,
}

struct GsSock {
    s: Sock,
    is_registered: bool,
    gsid: i8,
    gs_socket_id: i8,
}

struct GameServer {
    server_name: Vec<u8>,
    world_name: Vec<u8>,
    map_names: Vec<Vec<u8>>,
    server_ip: Vec<u8>,
    server_port: u16,
    internal_id: u16,
    connected_sockets: u8,
    socket_index: [i16; MAXSOCKETSPERSERVER],
    alive_response_time: u32,
    total_players_response: u32,
    is_initialized: bool,
    is_being_closed: bool,
    stock: Vec<u8>,
}

fn new_stock() -> Vec<u8> {
    let mut s = vec![0u8; 6];
    W(&mut s).u32(0, MSGID_SERVERSTOCKMSG);
    W(&mut s).u16(4, MSGTYPE_CONFIRM);
    s
}

struct Client {
    account_name: Vec<u8>,
    account_password: Vec<u8>,
    char_name: Vec<u8>,
    client_ip: Vec<u8>,
    connected_server_id: i8,
    time: u32,
    force_disconn_request_time: u32,
    is_playing: bool,
    is_on_server_change: bool,
}

pub struct Cfg {
    pub db: DbCfg,
    pub db_name: String,
    pub permitted: Vec<Vec<u8>>,
    pub listen_all: bool,
    pub config_dir: std::path::PathBuf,
    pub db_maintenance: bool,
}

/// Item.cfg .. Teleport.cfg in the order `bReadAllConfig` reads and `SendConfigToGS` sends them.
const CONFIGS: [(&str, u32); 14] = [
    ("Item.cfg", MSGID_ITEMCONFIGURATIONCONTENTS),
    ("Item2.cfg", MSGID_ITEMCONFIGURATIONCONTENTS),
    ("Item3.cfg", MSGID_ITEMCONFIGURATIONCONTENTS),
    ("Item4.cfg", MSGID_ITEMCONFIGURATIONCONTENTS),
    ("BuildItem.cfg", MSGID_BUILDITEMCONFIGURATIONCONTENTS),
    ("DupItemID.cfg", MSGID_DUPITEMIDFILECONTENTS),
    ("Magic.cfg", MSGID_MAGICCONFIGURATIONCONTENTS),
    ("noticement.txt", MSGID_NOTICEMENTFILECONTENTS),
    ("NPC.cfg", MSGID_NPCCONFIGURATIONCONTENTS),
    ("potion.cfg", MSGID_POTIONCONFIGURATIONCONTENTS),
    ("Quest.cfg", MSGID_QUESTCONFIGURATIONCONTENTS),
    ("Skill.cfg", MSGID_SKILLCONFIGURATIONCONTENTS),
    ("CraftItem.cfg", MSGID_CRAFTINGCONFIGURATIONCONTENTS),
    ("Teleport.cfg", MSGID_TELEPORTLISTCONTENTS),
];

/// `cItem`
#[derive(Default, Clone)]
struct Item {
    name: Vec<u8>,
    color: u8,
    dup_code: u8,
    touch_type: i16,
    tv1: i16,
    tv2: i16,
    tv3: i16,
    spec1: i16,
    spec2: i16,
    spec3: i16,
    pos_x: i16,
    pos_y: i16,
    life_span: u16,
    count: u32,
    attribute: u32,
    uid: u64,
    equipped: bool,
    in_bank: bool,
}

/// `cItem::MakeItemInfo` with the arguments CreateNewCharacter uses.
fn starter(name: &str, life_span: u16, touch_type: i16, x: i16, y: i16) -> Item {
    Item { name: name.into(), count: 1, touch_type, life_span, pos_x: x, pos_y: y, ..Default::default() }
}

fn local_now() -> [i32; 6] {
    let t = Local::now();
    [t.year(), t.month() as i32, t.day() as i32, t.hour() as i32, t.minute() as i32, t.second() as i32]
}

/// Account validity for a new account: one month from now ("Intial 30 Days"), as a DATETIME.
/// "until 2027-01-31: reason" (at most 100 bytes), the line the character screen shows for a banned account.
pub fn ban_text(block_date: &[u8], reason: &[u8]) -> Vec<u8> {
    let day = &block_date[..block_date.len().min(10)];
    let reason = cstr(reason);
    let mut t = b"until ".to_vec();
    t.extend_from_slice(day);
    t.extend_from_slice(b": ");
    t.extend_from_slice(if reason.is_empty() { b"no reason given" } else { reason });
    t.truncate(100);
    t
}

pub fn valid_date(now: chrono::NaiveDateTime) -> String {
    now.checked_add_months(chrono::Months::new(1)).unwrap_or(now).format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Guild size after a member leaves, never below 1.
// fixed: the C++ used WORD arithmetic, so 0 - 1 wrapped to 65535
pub fn members_after_remove(n: i32) -> u16 {
    (n - 1).clamp(1, u16::MAX as i32) as u16
}

fn sys_time_str() -> String {
    let [y, mo, d, h, mi, s] = local_now();
    format!("{y}-{mo}-{d} {h}:{mi}:{s}")
}

fn s(b: &[u8]) -> String {
    l1(cstr(b))
}

/// `char` / `sBYTE` printed with %d
fn i8d(v: i8) -> i32 {
    v as i32
}

/// Stat rule of CreateNewCharacter: sum 70, each 10..14, fewer than 4 characters on the account.
pub fn new_char_stats_ok(st: [u8; 6], chars_on_account: u8) -> bool {
    let sum: u32 = st.iter().map(|&v| v as u32).sum();
    sum == 70 && st.iter().all(|&v| (10..=14).contains(&v)) && chars_on_account < 4
}

/// GetCharList's packing of `SELECT * FROM char_database` rows (max 4) at `out[0..]`; returns *Size.
pub fn pack_char_list(r: &QRes, out: &mut [u8]) -> usize {
    let nrows = r.rows.len().min(4);
    // fixed: the C++ cleared only sizeof(char*) = 4 bytes; clear the whole list
    let len = (14 + nrows * 65).min(out.len());
    out[..len].fill(0);
    let mut w = W(out);
    w.u16(0, nrows as u16);
    for b in 0..nrows {
        let e = 1 + b * 65;
        for (f, name) in r.cols.iter().enumerate() {
            let v = r.get(b, f);
            let w16 = |w: &mut W, off: usize| w.u16(e + off, atoi(v) as u16);
            match name.as_str() {
                "char_name" => {
                    w.cstr(e, v);
                    w.u8(e + 10, 0x01);
                }
                "Appr1" => w16(&mut w, 11),
                "Appr2" => w16(&mut w, 13),
                "Appr3" => w16(&mut w, 15),
                "Appr4" => w16(&mut w, 17),
                "Gender" => w16(&mut w, 19),
                "Skin" => w16(&mut w, 21),
                "Level" => w16(&mut w, 23),
                "Exp" => w.u32(e + 25, atoul(v)),
                "Strenght" => w16(&mut w, 29),
                "Vitality" => w16(&mut w, 31),
                "Dexterity" => w16(&mut w, 33),
                "Intelligence" => w16(&mut w, 35),
                "Magic" => w16(&mut w, 37),
                "Agility" => w16(&mut w, 39),
                "ApprColor" => w.u32(e + 41, atoi(v) as u32),
                "MapLoc" => w.cstr(e + 55, v),
                _ => {}
            }
        }
    }
    14 + nrows * 65
}

/// Character block of the player data from a `SELECT * FROM char_database` row, in the layout HG's
/// `_bDecodePlayerDatafileContents` reads. Returns (CharID, AdminLevel, account name).
pub fn pack_char_row(r: &QRes, out: &mut [u8], in_party: impl Fn(u32) -> bool) -> (u32, u8, Vec<u8>) {
    let (mut char_id, mut admin_level, mut account) = (0u32, 0u8, Vec::new());
    // fields in result order: later writes overwrite the NULs of earlier strings, as in the C++
    for (f, name) in r.cols.iter().enumerate() {
        let v = r.get(0, f).to_vec();
        let mut w = W(out);
        let (i, ul) = (atoi(&v), atoul(&v));
        match name.as_str() {
                // fixed: the C++ then wrote a guild-exists flag over byte 0; HG reads a full DWORD CharID here
                "CharID" => {
                    char_id = i as u32;
                    w.u32(0, char_id);
                }
                "account_name" => account = v.clone(),
                "ID1" => w.u32(4, i as u32),
                "ID2" => w.u32(8, i as u32),
                "ID3" => w.u32(12, i as u32),
                "Level" => w.u16(16, i as u16),
                "Strenght" => w.u8(18, i as u8),
                "Vitality" => w.u8(19, i as u8),
                "Dexterity" => w.u8(20, i as u8),
                "Intelligence" => w.u8(21, i as u8),
                "Magic" => w.u8(22, i as u8),
                "Agility" => w.u8(23, i as u8),
                "Luck" => w.u8(24, i as u8),
                "Exp" => w.u32(25, ul),
                "Gender" => w.u8(29, i as u8),
                "Skin" => w.u8(30, i as u8),
                "HairStyle" => w.u8(31, i as u8),
                "HairColor" => w.u8(32, i as u8),
                "Underwear" => w.u8(33, i as u8),
                "Nation" => w.cstr(54, &v),
                "MapLoc" => w.cstr(64, &v),
                "LocX" => w.u16(74, i as u16),
                "LocY" => w.u16(76, i as u16),
                "Contribution" => w.u32(78, ul),
                "LeftSpecTime" => w.u32(82, ul),
                "LockMapName" => w.cstr(86, &v),
                "LockMapTime" => w.u32(96, ul),
                "BlockDate" => w.cstr(100, &v),
                "GuildName" => w.cstr(120, &v),
                "GuildID" => w.u16(140, i as u16),
                "GuildRank" => w.u8(142, i as u8),
                "FightNum" => w.u8(143, i as u8),
                "FightDate" => w.u32(144, ul),
                "FightTicket" => w.u8(148, i as u8),
                "QuestNum" => w.u16(149, ul as u16),
                "QuestID" => w.u32(151, ul),
                "QuestCount" => w.u16(155, ul as u16),
                "QuestRewType" => w.u16(157, i as u16),
                "QuestRewAmmount" => w.u32(159, ul),
                "QuestCompleted" => w.u8(163, i as u8),
                "EventID" => w.u32(164, ul),
                "WarCon" => w.u32(168, ul),
                "CruJob" => w.u8(172, i as u8),
                // fixed: the C++ swapped these; HG reads the crusade GUID at 173, construction points at 177
                "CruID" => w.u32(173, ul),
                "CruConstructPoint" => w.u32(177, ul),
                "Popularity" => w.u32(181, i as u32),
                "HP" => w.u32(185, ul),
                "MP" => w.u32(189, ul),
                "SP" => w.u32(193, ul),
                "EK" => w.u32(197, ul),
                "PK" => w.u32(201, ul),
                "RewardGold" => w.u32(205, ul),
                "DownSkillID" => w.u8(209, i as u8),
                "Hunger" => w.u8(210, i as u8),
                "LeftSAC" => w.u16(211, i as u16),
                "LeftShutupTime" => w.u32(213, ul),
                "LeftPopTime" => w.u32(217, ul),
                "LeftForceRecallTime" => w.u32(221, ul),
                "LeftFirmStaminarTime" => w.u32(225, ul),
                "LeftDeadPenaltyTime" => w.u32(229, ul),
                "PartyID" => w.u32(233, if in_party(ul) { ul } else { 0 }),
                "GizonItemUpgradeLeft" => w.u16(237, i as u16),
                "elo" => w.u16(239, i as u16),
                "AdminLevel" => {
                    admin_level = i as u8;
                    w.u8(242, admin_level);
                }
                // stored as '0'/'1' text, sent to the game server as 0/1 bytes
                "MagicMastery" => {
                    for (q, &ch) in cstr(&v).iter().take(100).enumerate() {
                        w.u8(243 + q, (ch == b'1') as u8);
                    }
                }
                "Profile" => w.cstr(343, &v),
            _ => {}
        }
    }
    (char_id, admin_level, account)
}

/// One bag (64-byte) or bank (59-byte) item at `ix`, as HG reads it; ItemID is 64-bit in both.
pub fn pack_item(out: &mut [u8], ix: usize, r: &QRes, row: usize, bank: bool) {
    let g = |n: &str| r.col(n).map_or(&b""[..], |c| r.get(row, c));
    let mut w = W(out);
    // ItemName may be NULL (the C++ would crash in strlen)
    w.cstr(ix, g("ItemName"));
    w.u32(ix + 20, atoul(g("Count")));
    w.u16(ix + 24, atoi(g("ItemType")) as u16);
    w.u32(ix + 26, atoi(g("ID1")) as u32);
    w.u32(ix + 30, atoi(g("ID2")) as u32);
    w.u32(ix + 34, atoi(g("ID3")) as u32);
    w.u8(ix + 38, atoi(g("Color")) as u8);
    w.u16(ix + 39, atoi(g("Effect1")) as u16);
    w.u16(ix + 41, atoi(g("Effect2")) as u16);
    w.u16(ix + 43, atoi(g("Effect3")) as u16);
    w.u16(ix + 45, atoi(g("LifeSpan")) as u16);
    w.u32(ix + 47, atoul(g("Attribute")));
    if bank {
        // fixed: the C++ wrote only the low 32 bits; HG reads a 64-bit ItemID (ullGetOffsetValue) at +51
        w.u64(ix + 51, atoull(g("ItemID")));
    } else {
        w.u8(ix + 51, atoi(g("ItemEquip")) as u8);
        w.u16(ix + 52, atoi(g("ItemPosX")) as u16);
        w.u16(ix + 54, atoi(g("ItemPosY")) as u16);
        w.u64(ix + 56, atoull(g("ItemID")));
    }
}

/// `UPDATE char_database` for a save block `c` (HG `_iComposePlayerDataFileContents` layout).
pub fn char_update_sql(c: Rd<'_>, char_id: u32) -> String {
    let u8_ = |o| c.u8(o);
    let u16_ = |o| c.u16(o);
    let i16_ = |o| c.u16(o) as i16;
    let u32_ = |o| c.u32(o);
    format!(
        "UPDATE `char_database` SET `ID1` = '{}',`ID2` = '{}',`ID3` = '{}',`Level` = '{}',`Strenght` = '{}',`Vitality` = '{}',`Dexterity` = '{}',`Intelligence` = '{}',`Magic` = '{}',`Agility` = '{}',`Luck` = '{}',`Exp` = '{}',`Gender` = '{}',`Skin` = '{}',`HairStyle` = '{}',`HairColor` = '{}',`Underwear` = '{}',`ApprColor` = '{}',`Appr1` = '{}',`Appr2` = '{}',`Appr3` = '{}',`Appr4` = '{}',`Nation` = '{}',`MapLoc` = '{}',`LocX` = '{}',`LocY` = '{}',`Profile` = '{}',`Contribution` = '{}',`LeftSpecTime` = '{}',`LockMapName` = '{}',`LockMapTime` = '{}',`BlockDate` = '{}',`GuildName` = '{}',`GuildID` = '{}',`GuildRank` = '{}',`FightNum` = '{}',`FightDate` = '{}',`FightTicket` = '{}',`QuestNum` = '{}',`QuestID` = '{}',`QuestCount` = '{}',`QuestRewType` = '{}',`QuestRewAmmount` = '{}',\t\t\t\t\t\t  `QuestCompleted` = '{}',`EventID` = '{}',`WarCon` = '{}',`CruJob` = '{}',`CruID` = '{}',`CruConstructPoint` = '{}', `Popularity` = '{}' ,`HP` = '{}',`MP` = '{}',`SP` = '{}',`EK` = '{}',`PK` = '{}',`RewardGold` = '{}',`DownSkillID` = '{}',`Hunger` = '{}',`LeftSAC` = '{}',`LeftShutupTime` = '{}',`LeftPopTime` = '{}',`LeftForceRecallTime` = '{}',`LeftFirmStaminarTime` = '{}',`LeftDeadPenaltyTime` = '{}',`MagicMastery` = '{}',`PartyID` = '{}',`GizonItemUpgradeLeft` = '{}',`elo` = '{}' WHERE `CharID` = '{char_id}' LIMIT 1;",
        c.u32(4) as i16,
        c.u32(8) as i16,
        c.u32(12) as i16,
        u16_(16),
        u8_(18),
        u8_(19),
        u8_(20),
        u8_(21),
        u8_(22),
        u8_(23),
        u8_(24),
        u32_(25),
        u8_(29),
        u8_(30),
        u8_(31),
        u8_(32),
        u8_(33),
        u32_(34),
        u32_(38),
        u32_(42),
        u32_(46),
        u32_(50),
        // fixed: the C++ put Nation, MapLoc, LockMapName and BlockDate in unescaped
        good(&c.s(54, 10)),
        good(&c.s(64, 10)),
        i16_(74),
        i16_(76),
        good(&c.s(343, 255)),
        u32_(78),
        u32_(82),
        good(&c.s(86, 10)),
        u32_(96),
        good(&c.s(100, 20)),
        good(&c.s(120, 20)),
        i16_(140),
        // fixed: the C++ read a WORD here, taking FightNum (143) as the high byte; HG keeps one signed byte
        u8_(142) as i8,
        u8_(143),
        u32_(144),
        u8_(148),
        u16_(149),
        u16_(151) as u32,
        u16_(155),
        i16_(157),
        u32_(159),
        u8_(163),
        u32_(164),
        u32_(168),
        u8_(172),
        // HG writes the crusade GUID at 173 and construction points at 177 (load now uses the same)
        u32_(173),
        u32_(177),
        u32_(181) as i32,
        u32_(185),
        u32_(189),
        u32_(193),
        u32_(197),
        u32_(201),
        u32_(205),
        u8_(209) as i8,
        u8_(210),
        u16_(211),
        u32_(213),
        u32_(217),
        u32_(221),
        u32_(225),
        u32_(229),
        (0..100).map(|q| if c.u8(243 + q) == 1 { '1' } else { '0' }).collect::<String>(),
        u32_(233),
        u16_(237),
        u16_(239),
    )
}

/// One `(...)` value tuple of the item / bank_item INSERT.
fn item_values(it: &Item, char_id: u32) -> String {
    // fixed: the C++ put item names in unescaped
    let n = good(&it.name);
    if it.in_bank {
        format!(
            "( '{char_id}', '{n}','{}','{}' ,'{}','{}','{}','{}','{}','{}','{}','{}' ,'{}' ,'{}')",
            it.count, it.touch_type, it.tv1, it.tv2, it.tv3, it.color, it.spec1, it.spec2, it.spec3, it.life_span, it.attribute, it.uid
        )
    } else {
        format!(
            "('{char_id}','{n}','{}','{}','{}','{}','{}','{}','{}','{}','{}','{}','{}','{}','{}','{}','{}')",
            it.count, it.touch_type, it.tv1, it.tv2, it.tv3, it.color, it.spec1, it.spec2, it.spec3, it.life_span, it.attribute, it.equipped as i32,
            it.pos_x, it.pos_y, it.uid
        )
    }
}

/// The multi-row INSERT; None when no item survived.
// fixed: the C++ put the comma by loop index, so a skipped first item gave `VALUES,(`
pub fn insert_sql(prefix: &str, values: &[String]) -> Option<String> {
    (!values.is_empty()).then(|| format!("{prefix}{};", values.join(",")))
}

/// Wrong passwords on one account: `ACCOUNT_MAX_FAILS` within `FAIL_WINDOW_MS` lock it for `ACCOUNT_LOCK_MS` (v8.00 "Account is
/// locked, check back in 30 minutes!"). shortcut: no per-IP limit (v8.00 also has "Too many login attempts, try again in 15 minutes"):
/// every browser reaches this Login through the bridge's address, so an IP limit would lock everybody out together.
pub const ACCOUNT_MAX_FAILS: u32 = 5;
pub const FAIL_WINDOW_MS: u32 = 10 * 60_000;
pub const ACCOUNT_LOCK_MS: u32 = 30 * 60_000;

/// `BanReason` of a lock saved in the database, so it survives a restart of the Login (a GM lifts it with `BlockDate='0000-00-00 00:00:00'`).
pub const LOCK_REASON: &[u8] = b"Too many wrong passwords";

/// Whole minutes (rounded up) until `block_date`, when the row is a saved lock and it has not ended; otherwise 0.
pub fn lock_minutes_in(block_date: &[u8], reason: &[u8], now: chrono::NaiveDateTime) -> u32 {
    if cstr(reason) != LOCK_REASON {
        return 0;
    }
    let Ok(t) = chrono::NaiveDateTime::parse_from_str(&String::from_utf8_lossy(cstr(block_date)), "%Y-%m-%d %H:%M:%S") else { return 0 };
    let secs = (t - now).num_seconds();
    if secs > 0 { (secs as u32).div_ceil(60) } else { 0 }
}

#[derive(Default, Clone, Debug)]
pub struct Fails {
    count: u32,
    since: u32,
    locked_until: u32, // `Server::now` time, 0 = never locked
}

impl Fails {
    /// Whole minutes (rounded up) left of an active lock, 0 when there is none.
    pub fn lock_minutes_left(&self, now: u32) -> u32 {
        let d = self.locked_until.wrapping_sub(now) as i32;
        if self.locked_until != 0 && d > 0 {
            (d as u32).div_ceil(60_000)
        } else {
            0
        }
    }

    /// Registers a wrong password; true when it is the one that locks the account.
    pub fn fail(&mut self, now: u32) -> bool {
        if self.count == 0 || now.wrapping_sub(self.since) > FAIL_WINDOW_MS {
            (self.count, self.since) = (0, now);
        }
        self.count += 1;
        if self.count >= ACCOUNT_MAX_FAILS {
            self.count = 0;
            self.locked_until = now.wrapping_add(ACCOUNT_LOCK_MS).max(1);
            return true;
        }
        false
    }
}

pub struct Server {
    cfg: Cfg,
    cfgs: Vec<Vec<u8>>,
    main_db: Option<Db>,
    csock: Vec<Option<Sock>>,
    gsock: Vec<Option<GsSock>>,
    gs: Vec<Option<GameServer>>,
    clients: Vec<Option<Client>>,
    queue: VecDeque<(Vec<u8>, usize)>,
    party: PartyManager,
    start: Instant,
    /// wrong-password counters and locks per account (lower case name)
    fails: HashMap<Vec<u8>, Fails>,
    autofix: bool,
    autofix_num: u8,
    timer_stopped: bool,
    mysql_timer: u32,
    check_accounts_timer: u32,
    servers_being_shutdown: bool,
    shutdown_interval: u32,
    shutdown_msg_index: u8,
    active_accounts: u16,
    peak_people_online: u16,
    total_accounts: u32,
    connected_hg_servers: u8,
}

// timeGetTime() counts from boot; start our clock well past the intervals compared against zero-initialised times
const CLOCK_BASE: u64 = 1 << 20;

impl Server {
    pub fn new(cfg: Cfg) -> Self {
        let mut srv = Server {
            cfg,
            cfgs: vec![Vec::new(); CONFIGS.len()],
            main_db: None,
            csock: (0..MAXCLIENTS).map(|_| None).collect(),
            gsock: (0..MAXGAMESERVERSOCKETS).map(|_| None).collect(),
            gs: (0..MAXGAMESERVERS).map(|_| None).collect(),
            clients: (0..MAXCLIENTS).map(|_| None).collect(),
            queue: VecDeque::new(),
            party: PartyManager::new(0),
            start: Instant::now(),
            fails: HashMap::new(),
            autofix: false,
            autofix_num: 1,
            timer_stopped: false,
            mysql_timer: 0,
            check_accounts_timer: 0,
            servers_being_shutdown: false,
            shutdown_interval: 0,
            shutdown_msg_index: 0,
            active_accounts: 0,
            peak_people_online: 0,
            total_accounts: 0,
            connected_hg_servers: 0,
        };
        srv.party = PartyManager::new(srv.now());
        srv
    }

    fn now(&self) -> u32 {
        (self.start.elapsed().as_millis() as u64 + CLOCK_BASE) as u32
    }

    fn opts_hb(&self) -> mysql_async::Opts {
        // fixed: the C++ hard-coded "helbreath" here instead of mysql-database
        self.cfg.db.opts(&self.cfg.db_name)
    }

    // ---------------------------------------------------------------- startup

    /// `DoInitialSetup` (after the config file) + `InitServer` minus the listeners, which main.rs owns.
    pub async fn init(&mut self) -> bool {
        match Db::connect(self.cfg.db.opts(&self.cfg.db_name)).await {
            Ok(d) => self.main_db = Some(d),
            Err(code) => {
                if code == 2003 {
                    log_lv("(ERROR) mySql server seems to be offline, please check the IP", WARN_MSG);
                }
                return false;
            }
        }
        log("(*) Connection to mySQL database was sucessfully established!");
        if !self.read_all_config() {
            return false;
        }
        if self.cfg.listen_all {
            log_lv("(ERROR) permitted-address line not found on config., server will be listening to all IPs!", WARN_MSG);
        }
        let t = self.now();
        if self.cfg.db_maintenance {
            self.optimize_database().await;
            self.repair_database().await;
        }
        self.check_active_accounts_number(t).await;
        true
    }

    /// `bReadAllConfig` / `ReadConfig`
    fn read_all_config(&mut self) -> bool {
        for (i, (name, _)) in CONFIGS.iter().enumerate() {
            let file = format!("Config/{name}");
            match std::fs::read(self.cfg.config_dir.join(name)) {
                Ok(raw) => self.cfgs[i] = text_mode(&raw),
                Err(_) => {
                    log_to(&format!("(ERROR) Cannot open configuration file [{file}]."), WARN_MSG, ERROR_LOGFILE);
                    log_lv("(ERROR) Stopped!", WARN_MSG);
                    return false;
                }
            }
        }
        true
    }

    async fn optimize_database(&mut self) {
        for t in ["account_database", "bank_item", "char_database", "guild", "guild_member", "item", "skill", "ipblocked"] {
            if self.autofix {
                return;
            }
            if let Some(db) = &mut self.main_db {
                db.qs(&format!("OPTIMIZE TABLE `{t}`;")).await;
            }
        }
    }

    async fn repair_database(&mut self) {
        for t in ["account_database", "bank_item", "char_database", "guild", "guild_member", "item", "skill", "ipblocked"] {
            if self.autofix {
                return;
            }
            if let Some(db) = &mut self.main_db {
                db.qs(&format!("REPAIR TABLE `{t}`;")).await;
            }
        }
    }

    async fn check_active_accounts_number(&mut self, time: u32) {
        if self.autofix {
            return;
        }
        let Some(db) = &mut self.main_db else { return };
        let Some(r) = db.qs(&format!("SHOW TABLE STATUS FROM `{}` LIKE 'account_database';", self.cfg.db_name.replace('`', ""))).await else { return };
        if let Some(c) = r.cols.iter().take(15).position(|c| c == "Rows") {
            self.total_accounts = atoi(r.get(0, c)) as u32;
        }
        self.active_accounts = self.clients.iter().filter(|c| c.is_some()).count() as u16;
        self.check_accounts_timer = time;
        self.peak_people_online = self.peak_people_online.max(self.active_accounts);
    }

    // ---------------------------------------------------------------- events

    pub async fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Open { gs: false, conn, peer, tx } => {
                // the C++ deletes its listeners while MySQL is being restarted
                if self.autofix {
                    return;
                }
                // OnUserAccept: no free slot -> accept and drop
                if let Some(i) = self.csock.iter().position(|s| s.is_none()) {
                    self.csock[i] = Some(Sock { conn, tx, peer });
                }
            }
            Ev::Open { gs: true, conn, peer, tx } => {
                if self.autofix {
                    return;
                }
                self.on_gate_server_accept(Sock { conn, tx, peer });
            }
            Ev::Frame { gs: false, conn, frame } => {
                let Some(id) = self.csock.iter().position(|s| s.as_ref().is_some_and(|s| s.conn == conn)) else { return };
                let (_, body) = proto::decode(&frame);
                if body.len() > MSGCLIENTBUFFERSIZE {
                    log_lv("(ERROR) An error occured in a client socket!", WARN_MSG);
                    self.csock[id] = None;
                    return;
                }
                self.on_client_read(id, body).await;
            }
            Ev::Frame { gs: true, conn, frame } => {
                let Some(id) = self.gsock.iter().position(|s| s.as_ref().is_some_and(|s| s.s.conn == conn)) else { return };
                let (_, body) = proto::decode(&frame);
                // fixed: the C++ ignores XSOCKEVENT_MSGSIZETOOLARGE for game servers and its stream desyncs;
                // we drop just that frame
                if body.len() > MSGBUFFERSIZE {
                    return;
                }
                self.on_game_server_read(id, body).await;
            }
            Ev::Closed { gs: false, conn } => {
                if let Some(id) = self.csock.iter().position(|s| s.as_ref().is_some_and(|s| s.conn == conn)) {
                    self.csock[id] = None;
                }
            }
            Ev::Closed { gs: true, conn } => {
                if let Some(id) = self.gsock.iter().position(|s| s.as_ref().is_some_and(|s| s.s.conn == conn)) {
                    self.close_game_server_socket(id, true);
                }
            }
            Ev::Cmd(c) => match c.trim() {
                // F1+F4
                "shutdown" => {
                    if !self.servers_being_shutdown {
                        self.servers_being_shutdown = true;
                        let t = self.now();
                        self.process_shutdown(t);
                    }
                }
                // F1+F5
                "reload" => {
                    log_lv("(*) Updating configuration files...", INFO_MSG);
                    if !self.read_all_config() {
                        log_lv("(ERROR) ERROR! Couldn't read configuration files!", WARN_MSG);
                    } else {
                        log_lv("(*) Done Updating Configs!", INFO_MSG);
                        self.send_updated_config_to_all_servers();
                    }
                }
                other => println!("commands: shutdown | reload (got {other:?})"),
            },
        }
    }

    fn on_gate_server_accept(&mut self, sock: Sock) {
        let Some(w) = self.gsock.iter().position(|s| s.is_none()) else { return };
        let peer = sock.peer.clone();
        self.gsock[w] = Some(GsSock { s: sock, is_registered: false, gsid: -1, gs_socket_id: -1 });
        log(&format!("(!) Game-server socket[{w}] IP({peer}) accepted on gate server socket."));
        if !self.cfg.listen_all && !self.cfg.permitted.iter().any(|p| is_same(p, peer.as_bytes())) {
            log_to(&format!("(ERROR) Game-server connection from non-authorized IP({peer}) refused."), WARN_MSG, HACK_LOGFILE);
            self.close_game_server_socket(w, true);
        }
    }

    // ---------------------------------------------------------------- sending

    fn send_client(&mut self, id: usize, body: &[u8]) {
        if body.len() > MSGCLIENTBUFFERSIZE {
            return;
        }
        if let Some(Some(s)) = self.csock.get(id) {
            let _ = s.tx.send(proto::encode(0, body));
        }
    }

    fn close_client(&mut self, id: usize) {
        if let Some(s) = self.csock.get_mut(id) {
            *s = None;
        }
    }

    /// `XSocket::iSendMsg` on a game-server socket, key 0.
    fn gs_sock_send(&mut self, sock: usize, body: &[u8], log: bool) {
        let Some(Some(gsock)) = self.gsock.get(sock) else { return };
        if body.len() > MSGBUFFERSIZE {
            return;
        }
        if log {
            let mut line = to_l1(&format!("Msg [{}] was sent = ", body.len()));
            line.extend(body.iter().map(|&c| if c == 0 { b' ' } else { c }));
            put_log_file_list(&line, Some(XSOCKET_LOGFILE));
        }
        let _ = gsock.s.tx.send(proto::encode(0, body));
    }

    /// `CGameServer::SendMsg`: always through the server's first socket.
    fn gs_send(&mut self, w: usize, body: &[u8], log: bool) {
        let Some(Some(g)) = self.gs.get(w) else { return };
        let idx = g.socket_index[0];
        if idx < 0 {
            return;
        }
        self.gs_sock_send(idx as usize, body, log);
    }

    /// `SendMsgToGS(GSID, ..)`: GSID is a *socket* index.
    fn send_msg_to_gs(&mut self, sock: usize, body: &[u8]) {
        let Some(Some(g)) = self.gsock.get(sock) else { return };
        // the C++ indexes GameServer[-1] for an unregistered socket; treat it as "no server"
        if g.gsid < 0 {
            return;
        }
        let w = g.gsid as usize;
        if self.gs.get(w).map_or(true, |g| g.is_none()) {
            return;
        }
        self.gs_send(w, body, false);
    }

    fn send_msg_to_all_game_servers(&mut self, except: Option<usize>, body: &[u8]) {
        for i in 0..MAXGAMESERVERS {
            if Some(i) != except && self.gs[i].is_some() {
                self.gs_send(i, body, false);
            }
        }
    }

    fn party_out(&mut self, out: Vec<party::Out>) {
        for o in out {
            match o {
                party::Out::All(d) => self.send_msg_to_all_game_servers(None, &d),
                party::Out::Gs(sock, d) => self.send_msg_to_gs(sock, &d),
            }
        }
    }

    // ---------------------------------------------------------------- client messages

    async fn on_client_read(&mut self, id: usize, body: Vec<u8>) {
        let size = body.len();
        let peer = self.csock[id].as_ref().map(|s| s.peer.clone()).unwrap_or_default();
        let Ok(mut db) = Db::connect(self.opts_hb()).await else {
            self.close_client(id);
            return;
        };
        let Some(r) = db.qs(&format!("SELECT * FROM `ipblocked` WHERE `ipaddress` = '{}';", good(peer.as_bytes()))).await else {
            self.close_client(id);
            return;
        };
        if !r.rows.is_empty() {
            log_to(&format!("(ERROR) IP address [{peer}] is on the block list and tried to login!"), WARN_MSG, HACK_LOGFILE);
            self.close_client(id);
            db.close().await;
            return;
        }
        let d = Rd::new(&body);
        let msgid = d.u32(0);
        let min = match msgid {
            MSGID_REQUEST_LOGIN | MSGID_REQUEST_CREATENEWCHARACTER | MSGID_REQUEST_CREATENEWACCOUNT | MSGID_REQUEST_CHANGEPASSWORD => 20,
            MSGID_REQUEST_DELETECHARACTER => 30,
            MSGID_REQUEST_ENTERGAME => 42,
            _ => 0,
        };
        if size < min {
            self.close_client(id);
            db.close().await;
            return;
        }
        match msgid {
            MSGID_REQUEST_LOGIN => {
                log("(ERROR) Processing client login...");
                if self.process_client_login(d.at(6), id, &mut db).await {
                    self.send_char_list(&d.s(6, 10), id, &mut db).await;
                }
            }
            MSGID_REQUEST_CREATENEWCHARACTER => {
                if self.process_client_login(d.at(16), id, &mut db).await {
                    self.create_new_character(d.at(6), id, &mut db).await;
                }
            }
            MSGID_REQUEST_CREATENEWACCOUNT => self.create_new_account(d.at(6), id, &mut db).await,
            MSGID_REQUEST_DELETECHARACTER => self.delete_character(d.at(6), id, &mut db).await,
            MSGID_REQUEST_CHANGEPASSWORD => {
                // fixed: the C++ logged in with the new password as world name (only "WS1" worked); the
                // request carries no world, so account + current password are checked alone
                if self.client_login(d.s(6, 10), d.s(16, 10), None, id, &mut db).await {
                    self.change_password(d.at(6), id, &mut db).await;
                }
            }
            MSGID_REQUEST_ENTERGAME => self.process_client_request_enter_game(d.at(4), id, &mut db).await,
            _ => {
                let dump = format!(" Unknown packet rcvd from client! message received! (0x{msgid:08X})");
                log(&dump);
                log_file(&dump, Some(CLIENTUNKNOWNMSG_LOGFILE2));
                let raw: Vec<u8> = body.iter().map(|&c| if c == 0 { b' ' } else { c }).collect();
                put_log_file_list(&raw, Some(CLIENTUNKNOWNMSG_LOGFILE));
                self.close_client(id);
            }
        }
        db.close().await;
    }

    /// `world` None: any world (change password carries none).
    async fn check_account_login(&mut self, acc: &[u8], pwd: &[u8], world: Option<&[u8]>, db: &mut Db) -> u8 {
        let mut sql = format!("SELECT `name`, `password` FROM `account_database` WHERE `name` = '{}' ", good(acc));
        if let Some(world) = world {
            sql = format!("{}AND `WorldName` = '{}' ", sql, good(world));
        }
        let Some(r) = db.qs(&sql).await else { return 0 };
        if r.rows.is_empty() {
            return ACCOUNTDONTEXISTS;
        }
        for (b, name) in r.cols.iter().enumerate().take(2) {
            let v = r.get(0, b);
            if name == "name" && !is_same(v, acc) {
                return ACCOUNTDONTEXISTS;
            } else if name == "password" && !is_same(v, pwd) {
                log_lv(
                    &format!("(ERROR)Wrong password: Account[ {} ] - Correct Password[ {} ] - Password received[ {} ]", s(acc), s(v), s(pwd)),
                    WARN_MSG,
                );
                return PASSWORDISWRONG;
            }
        }
        LOGINOK
    }

    async fn process_client_login(&mut self, d: Rd<'_>, id: usize, db: &mut Db) -> bool {
        self.client_login(d.s(0, 10), d.s(10, 10), Some(d.s(20, 10)), id, db).await
    }

    fn send_account_locked(&mut self, id: usize, minutes: u32) {
        let mut b = [0u8; 8];
        W(&mut b).u32(0, MSGID_RESPONSE_LOG);
        W(&mut b).u16(4, LOGRESMSGTYPE_ACCOUNTLOCKED);
        W(&mut b).u16(6, minutes.min(u16::MAX as u32) as u16);
        self.send_client(id, &b);
    }

    async fn client_login(&mut self, acc: Vec<u8>, pwd: Vec<u8>, world: Option<Vec<u8>>, id: usize, db: &mut Db) -> bool {
        // v8.00: a locked account is refused before its password is even looked at, so the lock cannot be tested with guesses
        let key = acc.to_ascii_lowercase();
        let now = self.now();
        let mut left = self.fails.get(&key).map_or(0, |f| f.lock_minutes_left(now));
        if left == 0 {
            left = self.account_lock_left(&acc, db).await; // a lock written before a restart of this Login
        }
        if left > 0 {
            self.send_account_locked(id, left);
            return false;
        }
        let code = match self.check_account_login(&acc, &pwd, world.as_deref(), db).await {
            ACCOUNTDONTEXISTS => LOGRESMSGTYPE_NOTEXISTINGACCOUNT,
            PASSWORDISWRONG => {
                if self.fails.entry(key).or_default().fail(now) {
                    log_lv(&format!("(ERROR) Account [{}] locked for 30 minutes: {ACCOUNT_MAX_FAILS} wrong passwords.", s(&acc)), WARN_MSG);
                    self.persist_account_lock(&acc, db).await;
                    self.send_account_locked(id, ACCOUNT_LOCK_MS / 60_000);
                    return false;
                }
                LOGRESMSGTYPE_PASSWORDMISMATCH
            }
            LOGINOK => {
                self.fails.remove(&key);
                return true;
            }
            _ => return false,
        };
        let mut b = [0u8; 6];
        W(&mut b).u32(0, MSGID_RESPONSE_LOG);
        W(&mut b).u16(4, code);
        self.send_client(id, &b);
        false
    }

    /// Minutes left of a lock saved in the database (`BlockDate` in the future with `BanReason` = `LOCK_REASON`), 0 if none. Needs the
    /// `BanReason` column like the bans do; without it the query fails and only the in-memory lock works.
    async fn account_lock_left(&mut self, acc: &[u8], db: &mut Db) -> u32 {
        let Some(r) = db.qs(&format!("SELECT `BlockDate`, `BanReason` FROM `account_database` WHERE `name` = '{}' ;", good(acc))).await else { return 0 };
        if r.rows.is_empty() {
            return 0;
        }
        lock_minutes_in(r.get(0, 0), r.get(0, 1), Local::now().naive_local())
    }

    /// Writes the lock as a ban of 30 minutes with `LOCK_REASON`, unless the account is banned already (its ban is kept).
    async fn persist_account_lock(&mut self, acc: &[u8], db: &mut Db) {
        if self.account_ban(acc, db).await.is_some() {
            return;
        }
        let until = Local::now().naive_local() + chrono::Duration::milliseconds(ACCOUNT_LOCK_MS as i64);
        let q = format!(
            "UPDATE `account_database` SET `BlockDate` = '{}', `BanReason` = '{}' WHERE `name` = '{}' ;",
            until.format("%Y-%m-%d %H:%M:%S"),
            s(LOCK_REASON),
            good(acc)
        );
        let _ = db.qs(&q).await;
    }

    async fn is_account_valid(&mut self, acc: &[u8], db: &mut Db) -> bool {
        let ga = good(acc);
        let Some(r) = db.qs(&format!("SELECT `ValidDate` FROM `account_database` WHERE `name` = '{ga}' ;")).await else { return false };
        // fixed (2026-11-23): the C++ answered true for an EXPIRED account and the flag sent to the client means "valid", so every
        // account inside its 30 days read "Your Account is not vaild. Please Vist Our Website!" on the character screen
        if is_date_past(r.get(0, 0), local_now()) {
            log_lv(&format!("(ERROR) Account [{ga}] Is Not Valid, Intial 30 Days For Account Vaild!"), WARN_MSG);
            return false;
        }
        true
    }

    /// v8.00 ban (no C++ original): `Some(text)` while the account's `BlockDate` lies in the future; the text is "until <date>: <reason>"
    /// from `BanReason`. A database without that column has no bans (the query fails and the account plays).
    async fn account_ban(&mut self, acc: &[u8], db: &mut Db) -> Option<Vec<u8>> {
        let r = db.qs(&format!("SELECT `BlockDate`, `BanReason` FROM `account_database` WHERE `name` = '{}' ;", good(acc))).await?;
        if r.rows.is_empty() || is_date_past(r.get(0, 0), local_now()) {
            return None;
        }
        Some(ban_text(r.get(0, 0), r.get(0, 1)))
    }

    async fn get_char_list(&mut self, acc: &[u8], out: &mut [u8], db: &mut Db) -> Option<usize> {
        out[..4].fill(0);
        let r = db.qs(&format!("SELECT * FROM `char_database` WHERE `account_name` = '{}' LIMIT 4;", good(acc))).await?;
        Some(pack_char_list(&r, out))
    }

    async fn send_char_list(&mut self, acc: &[u8], id: usize, db: &mut Db) {
        log(&format!("(ERROR) Getting character list for account [{}].", s(acc)));
        let mut b = vec![0u8; 500];
        let mut w = W(&mut b);
        w.u32(0, MSGID_RESPONSE_LOG);
        w.u16(4, MSGTYPE_CONFIRM);
        w.u16(6, UPPER_VERSION);
        w.u16(8, LOWER_VERSION);
        w.u8(10, 0x01);
        let valid = self.is_account_valid(acc, db).await;
        W(&mut b).u8(11, valid as u8);
        let size = self.get_char_list(acc, &mut b[23..], db).await.unwrap_or(0);
        let mut total = size + 23;
        if let Some(t) = self.account_ban(acc, db).await {
            // byte 12 (blank in the C++) says "banned"; the text follows the 8 bytes of time left that the client reads after the list
            let at = size + 18;
            if at + t.len() + 1 <= b.len() {
                b[12] = 1;
                b[at..at + t.len()].copy_from_slice(&t);
                total = at + t.len() + 1;
            }
        }
        self.send_client(id, &b[..total]);
    }

    async fn get_char_id(&mut self, ch: &[u8], acc: &[u8], db: &mut Db) -> u32 {
        let sql = format!("SELECT `CharID` FROM `char_database` WHERE `account_name` = '{}' AND `char_name` = '{}';", good(acc), good(ch));
        match db.qs(&sql).await {
            Some(r) => atoul(r.get(0, 0)),
            None => 0,
        }
    }

    async fn create_new_character(&mut self, d: Rd<'_>, id: usize, db: &mut Db) {
        let (ch, acc) = (d.s(0, 10), d.s(10, 10));
        let mut b = vec![0u8; 500];
        W(&mut b).u32(0, MSGID_RESPONSE_LOG);
        let gc = good(&ch);
        let Some(r) = db.qs(&format!("SELECT * FROM `char_database` WHERE `char_name` = '{gc}';")).await else { return };
        if !r.rows.is_empty() {
            W(&mut b).u16(4, LOGRESMSGTYPE_ALREADYEXISTINGCHARACTER);
            self.send_client(id, &b[..6]);
            return;
        }
        let ga = good(&acc);
        let Some(r) = db.qs(&format!("SELECT * FROM `char_database` WHERE `account_name` = '{ga}';")).await else { return };
        let nrows = r.rows.len() as u8;
        let st = [d.u8(65), d.u8(66), d.u8(67), d.u8(68), d.u8(69), d.u8(70)];
        if !new_char_stats_ok(st, nrows) {
            W(&mut b).u16(4, LOGRESMSGTYPE_NEWCHARACTERFAILED);
            self.send_client(id, &b[..6]);
            return;
        }
        let [str_, vit, dex, int, mag, agi] = st.map(|v| v as i32);
        // short promoted to int and printed with %u
        let appr1 = (((d.u8(62) as i32) << 8 | (d.u8(63) as i32) << 4 | d.u8(64) as i32) as i16) as i32 as u32;
        let sql = format!(
            "INSERT INTO `char_database` (`account_name`, `char_name`, `Strenght` , `Vitality` , `Dexterity` , `Intelligence` , `Magic` , `Agility` , `Appr1`, `Gender` , `Skin` , `HairStyle` , `HairColor` , `Underwear` , `HP` , `MP` , `SP`, `AdminLevel`, `LeftShutupTime`) VALUES ( '{ga}', '{gc}' , '{str_}' , '{vit}' , '{dex}' , '{int}' , '{mag}' , '{agi}' ,'{appr1}' , '{}', '{}', '{}' , '{}' , '{}' , '{}','{}','{}', '0', '2013265920');",
            d.u8(60),
            d.u8(61),
            d.u8(62),
            d.u8(63),
            d.u8(64),
            (vit * 3 + str_ / 2 + 2) as u32,
            (mag * 2 + int / 2 + 2) as u32,
            (str_ * 2 + 2) as u32
        );
        if db.qs(&sql).await.is_none() {
            return;
        }
        let char_id = self.get_char_id(&ch, &acc, db).await;
        // fixed: the C++ went on with CharID 0 (or crashed) when the new row could not be read back
        if char_id == 0 {
            return;
        }
        // faithful: three branches (skills 4,5,7 / 3 / others) that all insert mastery 10, SSN 0
        for sk in 0..MAXSKILLS {
            let sql = format!("INSERT INTO `skill` ( `CharID` , `SkillID`, `SkillMastery` , `SkillSSN`) VALUES ( '{char_id}' , '{sk}' , '10' , '0' );");
            if db.qs(&sql).await.is_none() {
                return;
            }
        }
        // Rust only: skill 10 (pretend corpse) starts at 0, as it always loaded before; see SKILL_PRETENDCORPSE
        let sql = format!("INSERT INTO `skill` ( `CharID` , `SkillID`, `SkillMastery` , `SkillSSN`) VALUES ( '{char_id}' , '{SKILL_PRETENDCORPSE}' , '0' , '0' );");
        if db.qs(&sql).await.is_none() {
            return;
        }
        let mut item = starter("Dagger", 300, 2, 30, 30);
        self.create_new_item(&mut item, char_id, db).await;
        let mut item = starter("RedPotion", 1, 0, 45, 30);
        for _ in 0..3 {
            item.uid = 0;
            self.create_new_item(&mut item, char_id, db).await;
        }
        let mut item = starter("RecallScroll", 1, 0, 60, 30);
        self.create_new_item(&mut item, char_id, db).await;
        W(&mut b).u16(4, LOGRESMSGTYPE_NEWCHARACTERCREATED);
        W(&mut b).cstr(6, &ch);
        // fixed: a failed list query sends an empty list (the C++ used an uninitialised size)
        let size = self.get_char_list(&acc, &mut b[16..], db).await.unwrap_or(0);
        self.send_client(id, &b[..size + 16]);
    }

    async fn delete_character(&mut self, d: Rd<'_>, id: usize, db: &mut Db) {
        if !self.process_client_login(d.at(10), id, db).await {
            self.close_client(id);
            return;
        }
        let (ch, acc) = (d.s(0, 10), d.s(10, 10));
        let mut b = vec![0u8; 500];
        let (ga, gc) = (good(&acc), good(&ch));
        // fixed: the C++ compared the escaped name with the registry's raw names
        if self.is_account_in_use(&acc).is_some() {
            W(&mut b).u32(0, MSGID_RESPONSE_LOG);
            W(&mut b).u16(4, ENTERGAMERESTYPE_PLAYING);
            self.send_client(id, &b[..6]);
            return;
        }
        let sql = format!("SELECT `account_name` ,`char_name` ,`Level` FROM `char_database` WHERE `account_name` = '{ga}' AND `char_name` = '{gc}';");
        let Some(r) = db.qs(&sql).await else { return };
        W(&mut b).u32(0, MSGID_RESPONSE_LOG);
        if r.rows.is_empty() {
            W(&mut b).u16(4, LOGRESMSGTYPE_NOTEXISTINGCHARACTER);
            self.send_client(id, &b[..6]);
            return;
        }
        for (f, name) in r.cols.iter().enumerate() {
            if name == "Level" && atoi(r.get(0, f)) > MAXDELETELEVEL {
                self.close_client(id);
                return;
            }
        }
        let char_id = self.get_char_id(&ch, &acc, db).await;
        let sql = format!("DELETE FROM `char_database` WHERE `account_name` = '{ga}' AND `char_name` = '{gc}' LIMIT 1;");
        if db.qs(&sql).await.is_none() {
            return;
        }
        if client_settings_table(db).await {
            db.qs(&format!("DELETE FROM `client_settings` WHERE `char_name` = '{gc}';")).await; // a new character of the same name starts clean
        }
        if db.qs(&format!("DELETE FROM `skill` WHERE `CharID` = '{char_id}';")).await.is_none() {
            return;
        }
        self.delete_all_items_from_char(char_id, db, true).await;
        W(&mut b).u16(4, LOGRESMSGTYPE_CHARACTERDELETED);
        let size = self.get_char_list(&acc, &mut b[7..], db).await.unwrap_or(0);
        self.send_client(id, &b[..size + 7]);
    }

    async fn change_password(&mut self, d: Rd<'_>, id: usize, db: &mut Db) {
        let mut b = [0u8; 50];
        W(&mut b).u32(0, MSGID_RESPONSE_CHANGEPASSWORD);
        let (acc, new1, new2) = (d.s(0, 10), d.s(20, 10), d.s(30, 10));
        // the client reads the WORD at +4; the new password follows at +6
        if !is_same(&new1, &new2) || new1.len() > 10 {
            W(&mut b).u32(4, LOGRESMSGTYPE_PASSWORDCHANGEFAIL);
        } else {
            W(&mut b).u32(4, LOGRESMSGTYPE_PASSWORDCHANGESUCCESS);
            let sql = format!("UPDATE `account_database` SET `password` = '{}' WHERE `name` = '{}' LIMIT 1;", good(&new1), good(&acc));
            if db.qs(&sql).await.is_none() {
                return;
            }
            W(&mut b).cstr(6, &new1);
        }
        self.send_client(id, &b[..16]);
    }

    async fn delete_all_items_from_char(&mut self, char_id: u32, db: &mut Db, bank: bool) {
        if db.qs(&format!("DELETE FROM `item` WHERE `CharID` = '{char_id}';")).await.is_none() {
            return;
        }
        if bank {
            db.qs(&format!("DELETE FROM `bank_item` WHERE `CharID` = '{char_id}';")).await;
        }
    }

    async fn account_exists(&mut self, acc: &[u8], db: &mut Db) -> bool {
        let Some(r) = db.qs(&format!("SELECT `name` FROM `account_database` WHERE `name` = '{}' LIMIT 1;", good(acc))).await else {
            return false;
        };
        // (the C++ also stores atoi(name) into the caller's first character; unused afterwards)
        r.rows.len() == 1
    }

    async fn create_new_account(&mut self, d: Rd<'_>, id: usize, db: &mut Db) {
        log("(ERROR) Create acc in progress.");
        let (acc, pwd) = (d.s(0, 10), d.s(10, 10));
        let email = d.s(20, 50);
        let quiz = d.s(143, 45);
        let answer = d.s(188, 20);
        let ip = self.csock[id].as_ref().map(|s| s.peer.clone()).unwrap_or_default();
        // fixed: the C++ wrote month + 1 as text (month 13 in December, stored as a zero date)
        let valid_date = valid_date(Local::now().naive_local());
        let mut b = [0u8; 6];
        W(&mut b).u32(0, MSGID_RESPONSE_LOG);
        if self.account_exists(&acc, db).await {
            W(&mut b).u16(4, LOGRESMSGTYPE_ALREADYEXISTINGACCOUNT);
            self.send_client(id, &b);
            return;
        }
        // fixed: the C++ stored the quiz in `Email`
        let sql = format!(
            "INSERT INTO `account_database` SET `name` = '{}' , `password` = '{}', `IsGMAccount` = '0', `LoginIpAddress` = '{}', `ValidDate` = '{valid_date}', `Email` = '{}', `Quiz` = '{}', `Answer` = '{}';",
            good(&acc),
            good(&pwd),
            good(ip.as_bytes()),
            good(&email),
            good(&quiz),
            good(&answer)
        );
        if db.qs(&sql).await.is_none() {
            return;
        }
        W(&mut b).u16(4, LOGRESMSGTYPE_NEWACCOUNTCREATED);
        self.send_client(id, &b);
    }

    fn is_account_in_use(&self, acc: &[u8]) -> Option<usize> {
        self.clients.iter().position(|c| c.as_ref().is_some_and(|c| is_same(&c.account_name, acc)))
    }

    /// Ok((ip, port, gs index)); Err(()) is FALSE, including the early FALSE for an uninitialised server.
    fn is_map_available(&self, map: &[u8], world: &[u8]) -> Option<(Vec<u8>, u16, u8)> {
        for (w, g) in self.gs.iter().enumerate() {
            let Some(g) = g else { continue };
            for m in &g.map_names {
                if is_same(m, map) && is_same(&g.world_name, world) {
                    if !g.is_initialized || g.is_being_closed {
                        return None;
                    }
                    return Some((g.server_ip.clone(), g.server_port, w as u8));
                }
            }
        }
        None
    }

    /// false when the registry is full.
    fn new_client(&mut self, acc: &[u8], pwd: &[u8], ch: &[u8], ip: &[u8], gsid: u8) -> bool {
        let t = self.now();
        let Some(w) = self.clients.iter().position(|c| c.is_none()) else { return false };
        {
            self.clients[w] = Some(Client {
                account_name: acc.to_vec(),
                account_password: pwd.to_vec(),
                char_name: ch.to_vec(),
                client_ip: ip.to_vec(),
                connected_server_id: gsid as i8,
                time: t,
                force_disconn_request_time: 0,
                is_playing: false,
                is_on_server_change: false,
            });
            self.active_accounts = self.active_accounts.wrapping_add(1);
            self.peak_people_online = self.peak_people_online.max(self.active_accounts);
        }
        true
    }

    /// `SAFEDELETE(Client[w])` (CClient destructor)
    fn delete_client(&mut self, w: usize) {
        if self.clients[w].take().is_some() && self.active_accounts > 0 {
            self.active_accounts -= 1;
        }
    }

    /// fixed (2026-11-23): the sessions bound to a game server that is gone, or was just replaced by a freshly registered one, are
    /// dead. The C++ kept them ("the delete is commented out"), so after an HG restart the account stayed "in use" for good: the
    /// client's "interrupt the connection?" button asked a server that no longer knew the player, and the timer below only removes
    /// clients that are not playing.
    fn drop_sessions_of_server(&mut self, gsid: usize) {
        for w in 0..MAXCLIENTS {
            if self.clients[w].as_ref().is_some_and(|c| c.connected_server_id == gsid as i8) {
                log(&format!("(!) Session of {} dropped: its game server [{gsid}] is gone.", s(&self.clients[w].as_ref().unwrap().account_name)));
                self.delete_client(w);
            }
        }
    }

    async fn process_client_request_enter_game(&mut self, d: Rd<'_>, id: usize, db: &mut Db) {
        if !self.process_client_login(d.at(22), id, db).await {
            return;
        }
        let mut b = [0u8; 100];
        W(&mut b).u32(0, MSGID_RESPONSE_ENTERGAME);
        let (ch, acc, pwd, world) = (d.s(2, 10), d.s(22, 10), d.s(32, 10), d.s(42, 10));
        let client_ip = self.csock[id].as_ref().map(|s| s.peer.clone()).unwrap_or_default().into_bytes();
        let sql = format!(
            "SELECT `MapLoc` FROM `char_database` WHERE `account_name` = '{}' AND `char_name` = '{}' LIMIT 1;",
            good(&acc),
            good(&ch)
        );
        let Some(r) = db.qs(&sql).await else { return };
        if r.rows.is_empty() {
            self.close_client(id);
            return;
        }
        let map = cstr(r.get(0, 0)).to_vec();
        if self.account_ban(&acc, db).await.is_some() {
            W(&mut b).u16(4, ENTERGAMERESTYPE_REJECT);
            W(&mut b).u8(6, REJECTTYPE_ACCOUNTBANNED);
            self.send_client(id, &b[..24]);
            return;
        }
        match d.u16(0) {
            t @ (ENTERGAMEMSGTYPE_NEW | ENTERGAMEMSGTYPE_CHANGINGSERVER) => {
                if let Some((gs_ip, gs_port, gsid)) = self.is_map_available(&map, &world) {
                    if t == ENTERGAMEMSGTYPE_NEW {
                        log(&format!("(*) Process (Enter Game) Client {} - {}, Enters Game Server At {} - {}", s(&ch), s(&client_ip), s(&map), s(&gs_ip)));
                    }
                    if self.is_account_in_use(&acc).is_some() {
                        W(&mut b).u16(4, ENTERGAMERESTYPE_PLAYING);
                    } else if !self.new_client(&acc, &pwd, &ch, &client_ip, gsid) {
                        // fixed: with a full registry the C++ still confirmed, and the game server was then refused
                        W(&mut b).u16(4, ENTERGAMERESTYPE_REJECT);
                        W(&mut b).u8(6, REJECTTYPE_MAXSERVERUSERLIMITREACHED);
                    } else {
                        self.send_game_server_ip(&client_ip, gsid);
                        W(&mut b).u16(4, ENTERGAMERESTYPE_CONFIRM);
                        W(&mut b).cstr(6, &gs_ip);
                        W(&mut b).u16(22, gs_port);
                    }
                } else {
                    W(&mut b).u16(4, ENTERGAMERESTYPE_REJECT);
                    W(&mut b).u8(6, REJECTTYPE_GAMESERVEROFFLINE);
                }
                self.send_client(id, &b[..24]);
            }
            ENTERGAMEMSGTYPE_NOENTER_FORCEDISCONN => {
                let in_use = self.is_account_in_use(&acc).filter(|&w| {
                    let c = self.clients[w].as_ref().unwrap();
                    is_same(&acc, &c.account_name) && is_same(&pwd, &c.account_password)
                });
                let Some(w) = in_use else {
                    self.close_client(id);
                    return;
                };
                let avail = self.is_map_available(&map, &world);
                if avail.is_some() {
                    self.request_force_disconnect(w, 10);
                } else {
                    self.delete_client(w);
                }
                let gs_ip = avail.map(|a| a.0).unwrap_or_default();
                log(&format!("(*) Process (Force Disconect) Client {} - {}, Enters Game Server At {} - {}", s(&ch), s(&client_ip), s(&map), s(&gs_ip)));
                let mut b = [0u8; 7];
                W(&mut b).u32(0, MSGID_RESPONSE_ENTERGAME);
                W(&mut b).u16(4, ENTERGAMERESTYPE_FORCEDISCONN);
                self.send_client(id, &b);
            }
            _ => self.close_client(id),
        }
    }

    fn send_game_server_ip(&mut self, ip: &[u8], gsid: u8) {
        let mut b = [0u8; 30];
        W(&mut b).u32(0, MSGID_CONFIRMEDIP);
        W(&mut b).bytes(4, cstr(ip));
        // fixed: the C++ passed this game-server index where SendMsgToGS wants a socket index
        self.gs_send(gsid as usize, &b, false);
    }

    /// `RequestForceDisconnect`
    fn request_force_disconnect(&mut self, w: usize, count: u16) {
        let t = self.now();
        let Some(gsid) = self.clients[w].as_ref().map(|c| c.connected_server_id) else { return };
        if gsid < 0 || self.gs.get(gsid as usize).map_or(true, |g| g.is_none()) {
            // fixed: nobody can answer a force-disconnect when the server is gone: the session simply ends
            self.delete_client(w);
            return;
        }
        let c = self.clients[w].as_mut().unwrap();
        let mut b = [0u8; 50];
        W(&mut b).u32(0, MSGID_REQUEST_FORCEDISCONECTACCOUNT);
        W(&mut b).u16(4, count);
        W(&mut b).cstr(6, &c.account_name);
        if c.force_disconn_request_time == 0 {
            c.force_disconn_request_time = t;
        }
        self.gs_send(gsid as usize, &b[..16], true);
        // the entry stays until the timer's force-disconnect timeout removes it (build/ fix of 2026-11-19)
    }

    // ---------------------------------------------------------------- game server messages

    fn registered(&self, sock: usize) -> bool {
        self.gsock[sock].as_ref().is_some_and(|g| g.is_registered)
    }

    async fn on_game_server_read(&mut self, sock: usize, body: Vec<u8>) {
        let d = Rd::new(&body);
        let msgid = d.u32(0);
        match msgid {
            MSGID_REQUEST_REGISTERGAMESERVER => self.register_game_server(d.at(6), sock),
            MSGID_REQUEST_REGISTERGAMESERVERSOCKET => self.register_game_server_socket(d.at(4), sock),
            _ if !self.registered(sock) => {
                // every other handled message requires a registered socket; unknown ones are logged below
                if !is_gs_msg(msgid) {
                    self.unknown_gs_msg(&body, msgid);
                }
            }
            MSGID_ENTERGAMECONFIRM => self.confirm_char_enter_game(d.at(6), sock),
            MSGID_REQUEST_SETACCOUNTWAITSTATUS => {
                let acc = d.s(6, 10);
                if let Some(w) = self.is_account_in_use(&acc) {
                    self.clients[w].as_mut().unwrap().is_on_server_change = true;
                }
            }
            MSGID_GAMEMASTERLOG => put_log_file_list(&d.s(6, body.len()), Some(GM_LOGFILE)),
            MSGID_PLAYERLOG => put_log_file_list(&d.s(6, body.len()), Some(PLAYER_LOGFILE)),
            MSGID_GAMEITEMLOG | MSGID_ITEMLOG => put_log_file_list(&d.s(6, body.len()), Some(ITEM_LOGFILE)),
            MSGID_GAMECRUSADELOG => put_log_file_list(&d.s(6, body.len()), Some(CRUSADE_LOGFILE)),
            MSGID_GAMESERVERALIVE => {
                let t = self.now();
                if let Some(g) = self.gs_of(sock) {
                    g.alive_response_time = t;
                }
            }
            MSGID_GAMESERVERSHUTDOWNED => {
                if let Some(g) = self.gs_of(sock) {
                    g.is_being_closed = true;
                    log_lv("(ERROR) A Game server is being closed!", WARN_MSG);
                }
            }
            MSGID_SERVERSTOCKMSG => self.server_stock_msg_handler(&body, sock),
            MSGID_PARTYOPERATION => self.party_operation_result_handler(&body, sock),
            MSGID_SENDSERVERSHUTDOWNMSG => {}
            m if is_db_msg(m) => match Db::connect(self.opts_hb()).await {
                Ok(mut db) => {
                    self.process_gs_msg_with_db_access(&body, sock, &mut db).await;
                    db.close().await;
                }
                Err(_) => {
                    if self.queue.len() >= MSGQUENESIZE {
                        log_file("@@@@@@ CRITICAL ERROR in MsgQuene!!! @@@@@@", None);
                    } else {
                        self.queue.push_back((body, sock));
                    }
                }
            },
            _ => self.unknown_gs_msg(&body, msgid),
        }
    }

    fn unknown_gs_msg(&mut self, body: &[u8], msgid: u32) {
        log_lv("(ERROR) Unknown packet rcvd from Game server socket!", WARN_MSG);
        let raw: Vec<u8> = body.iter().map(|&c| if c == 0 { b' ' } else { c }).collect();
        put_log_file_list(&raw, Some(GSUNKNOWNMSG_LOGFILE));
        let dump = format!(" Unknown packet rcvd from Game server! message received! (0x{msgid:08X})");
        log(&dump);
        log_file(&dump, Some(GSUNKNOWNMSG_LOGFILE2));
    }

    /// `GameServer[GameServerSocket[sock]->GSID]`, when both exist.
    fn gs_of(&mut self, sock: usize) -> Option<&mut GameServer> {
        let gsid = self.gsock[sock].as_ref()?.gsid;
        if gsid < 0 {
            return None;
        }
        self.gs.get_mut(gsid as usize)?.as_mut()
    }

    fn register_game_server(&mut self, d: Rd<'_>, sock: usize) {
        let server_name = d.s(0, 10);
        let world_name = d.s(10, 10);
        let server_ip = d.s(20, 16);
        let port = d.u16(36);
        let received_config = d.u8(38) != 0;
        let n_maps = d.u8(39) as usize;
        // (InternalID is read at +40 but overwritten with the slot index)
        let maps = d.at(42);
        let Some(w) = self.gs.iter().position(|g| g.is_none()) else { return };
        self.drop_sessions_of_server(w); // a fresh server has no players, whatever the slot's previous one left behind
        let t = self.now();
        let g = self.gsock[sock].as_mut().unwrap();
        g.gsid = w as i8;
        // CGameServer(ID)
        g.gs_socket_id = 0;
        g.is_registered = true;
        let mut socket_index = [-1i16; MAXSOCKETSPERSERVER];
        socket_index[0] = sock as i16;
        self.connected_hg_servers = self.connected_hg_servers.wrapping_add(1);
        // faithful: SafeCopy of an empty name writes nothing, so the constructor's "NONE" survives
        let world = if world_name.is_empty() { b"NONE".to_vec() } else { world_name };
        let map_names: Vec<Vec<u8>> = (0..n_maps).map(|b| maps.s(11 * b, 10)).collect();
        self.gs[w] = Some(GameServer {
            server_name,
            world_name: world.clone(),
            map_names: map_names.clone(),
            server_ip,
            server_port: port,
            internal_id: w as u16,
            connected_sockets: 1,
            socket_index,
            alive_response_time: t,
            // uninitialised in the C++ constructor
            total_players_response: 0,
            is_initialized: false,
            is_being_closed: false,
            stock: new_stock(),
        });
        log("(!) Maps registered:");
        for m in &map_names {
            log(&format!("{} - {}", s(&world), s(m)));
        }
        if !received_config {
            self.send_config_to_gs(w);
        }
        let mut b = [0u8; 8];
        W(&mut b).u32(0, MSGID_RESPONSE_REGISTERGAMESERVER);
        W(&mut b).u16(4, MSGTYPE_CONFIRM);
        W(&mut b).u16(6, w as u16);
        log(&format!("(!) Game Server registered at ID[{w}]-[{w}]."));
        self.send_msg_to_gs(sock, &b);
    }

    fn register_game_server_socket(&mut self, d: Rd<'_>, sock: usize) {
        let gsid = d.u8(0) as usize;
        log(&format!("(!) Trying to register socket on GS[{gsid}]."));
        let Some(Some(g)) = self.gs.get_mut(gsid) else {
            log_lv("(ERROR) GSID is not registered!", WARN_MSG);
            return;
        };
        let Some(b) = g.socket_index.iter().position(|&i| i < 0) else { return };
        g.socket_index[b] = sock as i16;
        g.connected_sockets += 1;
        let done = g.connected_sockets as usize == MAXSOCKETSPERSERVER;
        if done {
            g.is_initialized = true;
        }
        let name = s(&g.server_name);
        let gsock = self.gsock[sock].as_mut().unwrap();
        gsock.gsid = gsid as i8;
        gsock.gs_socket_id = b as i8;
        gsock.is_registered = true;
        log_lv(&format!("(!) Registered Socket({b}) Index({sock}) GSID({gsid}) SocketID({b})."), INFO_MSG);
        if done {
            log_lv(&format!("(!) Gameserver({name}) registered!"), INFO_MSG);
        }
    }

    /// Config files to game server `w` (through its first socket, like every login -> GS message).
    fn send_config_to_gs(&mut self, w: usize) {
        for (i, (_, msgid)) in CONFIGS.iter().enumerate() {
            let text = cstr(&self.cfgs[i]).to_vec();
            let mut b = vec![0u8; text.len() + 7];
            W(&mut b).u32(0, *msgid);
            W(&mut b).bytes(6, &text);
            self.gs_send(w, &b, false);
        }
    }

    fn send_updated_config_to_all_servers(&mut self) {
        log_lv("(*) Sending updated configuration to all servers...", INFO_MSG);
        let mut b = [0u8; 4];
        W(&mut b).u32(0, MSGID_UPDATECONFIGFILES);
        for w in 0..MAXGAMESERVERS {
            if self.gs[w].is_some() {
                self.gs_send(w, &b, false);
                // fixed: the C++ passed (BYTE)SocketIndex[0], so a lost first socket became socket 255
                self.send_config_to_gs(w);
            }
        }
        log_lv("(*) All servers are updated!", INFO_MSG);
    }

    fn close_game_server_socket(&mut self, id: usize, log_it: bool) {
        let Some(g) = &self.gsock[id] else { return };
        let (gsid, gssid) = (g.gsid, g.gs_socket_id);
        if gssid < 0 || gsid < 0 {
            self.gsock[id] = None;
            return;
        }
        let gsid = gsid as usize;
        if let Some(Some(g)) = self.gs.get_mut(gsid) {
            g.socket_index[gssid as usize] = -1;
        }
        self.gsock[id] = None;
        if log_it {
            log_lv(&format!("(ERROR) A GameServer socket was closed [{id}]."), WARN_MSG);
        }
        let Some(Some(g)) = self.gs.get_mut(gsid) else { return };
        g.connected_sockets = g.connected_sockets.wrapping_sub(1);
        if g.connected_sockets == 0 {
            self.delete_game_server(gsid);
        }
    }

    /// `~CGameServer`
    fn delete_game_server(&mut self, w: usize) {
        let Some(g) = self.gs[w].take() else { return };
        for &i in &g.socket_index {
            if i >= 0 {
                self.gsock[i as usize] = None;
            }
        }
        log_lv(&format!("(ERROR) GameServer [{}] connection was lost!", s(&g.server_name)), WARN_MSG);
        // fixed: the C++ told the party manager the last closed *socket* index; members record the server
        self.party.game_server_down(w as i32);
        self.connected_hg_servers = self.connected_hg_servers.wrapping_sub(1);
        self.drop_sessions_of_server(w);
    }

    fn confirm_char_enter_game(&mut self, d: Rd<'_>, sock: usize) {
        let (acc, pwd, ip) = (d.s(0, 10), d.s(10, 10), d.s(30, 16));
        let Some(w) = self.is_account_in_use(&acc) else { return };
        let gsid = self.gsock[sock].as_ref().map_or(-1, |g| g.gsid);
        let c = self.clients[w].as_ref().unwrap();
        if !is_same(&pwd, &c.account_password) || c.connected_server_id != gsid {
            log_to(
                &format!(
                    "(ERROR) Wrong data: Account({}) pwd[{}/{}] IP[{}/{}] GSID[{}/{}]",
                    s(&acc),
                    s(&pwd),
                    s(&c.account_password),
                    s(&ip),
                    s(&c.client_ip),
                    i8d(c.connected_server_id),
                    i8d(gsid)
                ),
                WARN_MSG,
                ERROR_LOGFILE,
            );
            self.request_force_disconnect(w, 10);
        } else {
            let t = self.now();
            let c = self.clients[w].as_mut().unwrap();
            c.is_playing = true;
            c.time = t;
            log(&format!("(ERROR) Set character({})[ID:{w}] status: playing.", s(&c.char_name)));
        }
    }

    async fn process_gs_msg_with_db_access(&mut self, body: &[u8], sock: usize, db: &mut Db) {
        let d = Rd::new(body);
        match d.u32(0) {
            MSGID_REQUEST_PLAYERDATA => self.process_request_player_data(d.at(6), sock, db).await,
            MSGID_REQUEST_NOSAVELOGOUT => self.process_client_logout(d.at(6), false, sock, db).await,
            MSGID_REQUEST_SAVEPLAYERDATALOGOUT | MSGID_REQUEST_SAVEPLAYERDATA_REPLY => {
                self.process_client_logout(d.at(6), true, sock, db).await
            }
            MSGID_REQUEST_SAVEPLAYERDATA => self.save_character(d.at(6), db).await,
            MSGID_REQUEST_CREATENEWGUILD => self.request_create_new_guild(d.at(6), sock, db).await,
            MSGID_REQUEST_DISBANDGUILD => self.request_disband_guild(d.at(6), sock, db).await,
            MSGID_REQUEST_UPDATEGUILDINFO_NEWGUILDSMAN => self.add_guild_member(d.at(6), db).await,
            MSGID_REQUEST_UPDATEGUILDINFO_DELGUILDSMAN => self.remove_guild_member(d.at(6), db).await,
            MSGID_REQUEST_LGNPTS => self.request_legion_pts(d.at(6), sock, db).await,
            MSGID_REQUEST_LGNSVC => self.request_legion_svc(d.at(4), sock, db).await,
            MSGID_SUBCASH => self.sub_account_cash(d.at(6), db).await,
            MSGID_REQUEST_BOTMEM => self.request_bot_memory(d.at(6), sock, db).await,
            MSGID_BOTMEM_SAVE => save_bot_memory(d.at(6), db).await,
            MSGID_REQUEST_BOTHIST => self.request_bot_history(d.at(6), sock, db).await,
            MSGID_BOTHIST_SAVE => save_bot_history(d.at(6), db).await,
            MSGID_CLIENTSETTINGS_SAVE => save_client_settings(d.at(6), db).await,
            _ => {}
        }
    }

    /// A game server asks for what bot `d[0..10]` remembers: the reply carries the stored text (hex), empty when there is none.
    async fn request_bot_memory(&mut self, d: Rd<'_>, sock: usize, db: &mut Db) {
        let bot = d.s(0, 10);
        if bot.is_empty() || !bot_memory_table(db).await {
            return;
        }
        let Some(r) = db.qs(&format!("SELECT `data` FROM `bot_memory` WHERE `bot` = '{}' LIMIT 1;", good(&bot))).await else { return };
        let data = if r.rows.len() == 1 { r.get(0, 0).to_vec() } else { Vec::new() };
        let mut b = vec![0u8; 16 + data.len() + 1];
        W(&mut b).u32(0, MSGID_RESPONSE_BOTMEM);
        W(&mut b).u16(4, LOGRESMSGTYPE_CONFIRM);
        W(&mut b).bytes(6, &bot);
        W(&mut b).bytes(16, &data);
        self.send_msg_to_gs(sock, &b);
    }

    /// A game server asks for the history of bot `d[0..10]`: its newest rows (what fits in a frame), oldest first, as hex of `t \t kind \t other \t text` lines.
    async fn request_bot_history(&mut self, d: Rd<'_>, sock: usize, db: &mut Db) {
        let bot = d.s(0, 10);
        if bot.is_empty() || !bot_history_table(db).await {
            return;
        }
        let Some(r) = db.qs(&format!("SELECT `t`, `kind`, `other`, `text` FROM `bot_history` WHERE `bot` = '{}' ORDER BY `id` DESC LIMIT 100;", good(&bot))).await else { return };
        let mut lines: Vec<String> = Vec::new();
        let mut size = 0;
        for i in 0..r.rows.len() {
            let l = format!("{}\t{}\t{}\t{}\n", s(r.get(i, 0)), s(r.get(i, 1)), s(r.get(i, 2)), s(r.get(i, 3)));
            if size + l.len() > 9000 {
                break;
            }
            size += l.len();
            lines.push(l);
        }
        lines.reverse();
        let data = to_hex(lines.concat().as_bytes());
        let mut b = vec![0u8; 16 + data.len() + 1];
        W(&mut b).u32(0, MSGID_RESPONSE_BOTHIST);
        W(&mut b).u16(4, LOGRESMSGTYPE_CONFIRM);
        W(&mut b).bytes(6, &bot);
        W(&mut b).bytes(16, data.as_bytes());
        self.send_msg_to_gs(sock, &b);
    }

    async fn process_request_player_data(&mut self, d: Rd<'_>, sock: usize, db: &mut Db) {
        let mut b = vec![0u8; 20000];
        W(&mut b).u32(0, MSGID_RESPONSE_PLAYERDATA);
        let (ch, acc, pwd, ip) = (d.s(0, 10), d.s(10, 10), d.s(20, 10), d.s(30, 15));
        W(&mut b).cstr(6, &ch);
        let gsid = self.gsock[sock].as_ref().map_or(-1, |g| g.gsid);
        let mut info_size = 0usize;
        let code = if let Some(w) = self.is_account_in_use(&acc) {
            let c = self.clients[w].as_ref().unwrap();
            if !is_same(&pwd, &c.account_password) || !is_same(&ch, &c.char_name) || c.connected_server_id != gsid {
                log_to(
                    &format!(
                        "(ERROR) Wrong data: Account({}) pwd[{}/{}] charname[{}/{}] IP[{}/{}] GSID[{}/{}]",
                        s(&acc),
                        s(&pwd),
                        s(&c.account_password),
                        s(&ch),
                        s(&c.char_name),
                        s(&ip),
                        s(&c.client_ip),
                        i8d(c.connected_server_id),
                        i8d(gsid)
                    ),
                    WARN_MSG,
                    ERROR_LOGFILE,
                );
                self.delete_client(w);
                LOGRESMSGTYPE_REJECT
            } else {
                info_size = self.get_character_info(&ch, &mut b[16..], db).await;
                if info_size == 0 {
                    self.delete_client(w);
                    LOGRESMSGTYPE_REJECT
                } else {
                    self.clients[w].as_mut().unwrap().is_on_server_change = false;
                    LOGRESMSGTYPE_CONFIRM
                }
            }
        } else {
            log_to(&format!("(ERROR) Character({}) data error: account not initialized!", s(&ch)), WARN_MSG, ERROR_LOGFILE);
            LOGRESMSGTYPE_REJECT
        };
        W(&mut b).u16(4, code);
        self.send_msg_to_gs(sock, &b[..16 + info_size]);
        if code == LOGRESMSGTYPE_CONFIRM {
            self.send_client_settings(&ch, sock, db).await; // after the player data: HG knows the character by then
        }
    }

    /// The saved client settings of `ch` to its game server, empty when there are none. Nothing at all when the query fails: the
    /// client then does not save, so it cannot overwrite what it never got.
    async fn send_client_settings(&mut self, ch: &[u8], sock: usize, db: &mut Db) {
        if !client_settings_table(db).await {
            return;
        }
        let Some(r) = db.qs(&format!("SELECT `data` FROM `client_settings` WHERE `char_name` = '{}' LIMIT 1;", good(ch))).await else { return };
        let data = if r.rows.len() == 1 { r.get(0, 0).to_vec() } else { Vec::new() };
        let mut b = vec![0u8; 16 + data.len() + 1];
        W(&mut b).u32(0, MSGID_RESPONSE_CLIENTSETTINGS);
        W(&mut b).u16(4, LOGRESMSGTYPE_CONFIRM);
        W(&mut b).bytes(6, &ch[..ch.len().min(10)]);
        W(&mut b).bytes(16, &data);
        self.send_msg_to_gs(sock, &b);
    }

    async fn guild_exists(&mut self, guild: &[u8], db: &mut Db) -> Option<u32> {
        let r = db.qs(&format!("SELECT `GuildID` FROM `guild` WHERE `GuildName` = '{}' LIMIT 1;", good(guild))).await?;
        (r.rows.len() == 1).then(|| atoi(r.get(0, 0)) as u32)
    }

    async fn is_gm_account(&mut self, acc: &[u8], db: &mut Db) -> bool {
        let r = db.qs(&format!("SELECT `IsGMAccount` FROM `account_database` WHERE `name` = '{}' LIMIT 1;", good(acc))).await;
        // the C++ dereferences a NULL row when the account is missing; we answer FALSE
        r.is_some_and(|r| !r.rows.is_empty() && atoi(r.get(0, 0)) > 0)
    }

    /// `GetCharacterInfo`: fills `out` (SendBuff+16) and returns its size, 0 on error.
    async fn get_character_info(&mut self, ch: &[u8], out: &mut [u8], db: &mut Db) -> usize {
        const CHAR_INDEX_END: usize = 751;
        let fail = |m: String, f: &str| {
            log_to(&m, WARN_MSG, f);
            0
        };
        let Some(r) = db.qs(&format!("SELECT * FROM `char_database` WHERE `char_name` = '{}' LIMIT 1;", good(ch))).await else { return 0 };
        if r.rows.is_empty() {
            return fail(format!("(ERROR) Character({}) data error: character not found in the database!", s(ch)), ERROR_LOGFILE);
        }
        let party = &self.party;
        let (char_id, admin_level, account) = pack_char_row(&r, out, |pid| party.is_member(pid as i32, ch));
        if char_id == 0 {
            return fail(format!("(ERROR) Character({}) data error: CharID is null!", s(ch)), ERROR_LOGFILE);
        }
        // fixed: the C++ checked an uninitialised account name, so every GM character was refused
        if admin_level > 0 && !self.is_gm_account(&account, db).await {
            return fail(format!("(ERROR) Character({}) tries to enter in game as GM in a non-GM account!!!", s(ch)), HACK_LOGFILE);
        }
        let sql = format!("SELECT `SkillID`, `SkillMastery`, `SkillSSN` FROM `skill` WHERE `CharID` = '{char_id}' AND `SkillID` <> '{SKILL_PRETENDCORPSE}' LIMIT {MAXSKILLS};");
        let Some(r) = db.qs(&sql).await else { return 0 };
        if r.rows.len() < MAXSKILLS as usize {
            return fail(format!("(ERROR) Character({}) data error: number of skills({}) don't match!", s(ch), r.rows.len()), ERROR_LOGFILE);
        }
        let (fid, fm, fs) = (r.col("SkillID").unwrap_or(0), r.col("SkillMastery").unwrap_or(0), r.col("SkillSSN").unwrap_or(0));
        let mut w = W(out);
        for b in 0..r.rows.len() {
            let sid = atoi(r.get(b, fid)) as u8 as usize;
            w.u8(598 + sid, atoi(r.get(b, fm)) as u8);
            w.u32(653 + sid * 4, atoi(r.get(b, fs)) as u32);
        }
        // Rust only: skill 10 from its own row, kept out of the query above (which picks the first 10 rows in
        // insertion order, as the C++); the servers insert it after rows 0..9, so the C++ Login's LIMIT 10 skips it too
        let sk = SKILL_PRETENDCORPSE;
        let Some(r) = db.qs(&format!("SELECT `SkillMastery`, `SkillSSN` FROM `skill` WHERE `CharID` = '{char_id}' AND `SkillID` = '{sk}' LIMIT 1;")).await else { return 0 };
        if r.rows.is_empty() {
            if db.qs(&format!("INSERT INTO `skill` ( `CharID` , `SkillID`, `SkillMastery` , `SkillSSN`) VALUES ( '{char_id}' , '{sk}' , '0' , '0' );")).await.is_none() {
                return 0;
            }
        } else {
            let mut w = W(out);
            w.u8(598 + sk as usize, atoi(r.get(0, 0)) as u8);
            w.u32(653 + sk as usize * 4, atoi(r.get(0, 1)) as u32);
        }
        let Some(r) = db.qs(&format!("SELECT * FROM `item` WHERE `CharID` = '{char_id}' LIMIT {MAXITEMS};")).await else { return 0 };
        let n_items = r.rows.len() as u8 as usize;
        W(out).u8(CHAR_INDEX_END, n_items as u8);
        for b in 0..n_items {
            pack_item(out, CHAR_INDEX_END + 1 + b * 64, &r, b, false);
        }
        let sql = format!("SELECT * FROM `bank_item` WHERE `CharID` = '{char_id}' ORDER BY `ItemName` ASC LIMIT {MAXBANKITEMS};");
        let Some(r) = db.qs(&sql).await else { return 0 };
        let n_bank = r.rows.len() as u8 as usize;
        // HG reads the bank count one byte after the bag items and the bank items after that
        // (its save writes them one byte earlier; save_character reads that layout)
        let bank_ix = CHAR_INDEX_END + 1 + n_items * 64 + 1;
        W(out).u8(bank_ix, n_bank as u8);
        for b in 0..n_bank {
            pack_item(out, bank_ix + 1 + b * 59, &r, b, true);
        }
        bank_ix + n_bank * 59 + 2
    }

    async fn process_client_logout(&mut self, d: Rd<'_>, save: bool, sock: usize, db: &mut Db) {
        let (ch, acc, pwd) = (d.s(0, 10), d.s(10, 10), d.s(20, 10));
        if ch.is_empty() || acc.is_empty() || pwd.is_empty() {
            return;
        }
        // true: logged out, false: changing servers
        let count_logout = d.u8(30) != 0;
        let Some(w) = self.is_account_in_use(&acc) else { return };
        let c = self.clients[w].as_ref().unwrap();
        if !(is_same(&acc, &c.account_name) && is_same(&pwd, &c.account_password) && is_same(&ch, &c.char_name)) {
            return;
        }
        if save {
            self.save_character(d, db).await;
            log(&format!("(ERROR) Player({})[ID:{w}] data saved Server Change.", s(&ch)));
        } else {
            log(&format!("(ERROR) Player({})[ID:{w}] logout with no save.", s(&ch)));
        }
        if !count_logout {
            let mut b = vec![0u8; 50];
            W(&mut b).u32(0, MSGID_RESPONSE_SAVEPLAYERDATA_REPLY);
            W(&mut b).cstr(6, &ch);
            self.send_msg_to_gs(sock, &b[..ch.len() + 6]);
        }
        // fixed: the C++ kept the entry on a real logout (its delete is commented out), so the account
        // stayed "playing" until the enter-game timeout
        self.delete_client(w);
    }

    async fn get_last_inserted_item_id(&mut self, db: &mut Db) -> u64 {
        let Some(r) = db.qs("SELECT `ItemID` FROM `bank_item` ORDER BY `ItemID` DESC  LIMIT 1;").await else { return 0 };
        let bank = if r.rows.is_empty() { 0 } else { atoull(r.get(0, 0)) };
        let Some(r) = db.qs("SELECT `ItemID` FROM `item` ORDER BY `ItemID` DESC  LIMIT 1;").await else { return 0 };
        let item = if r.rows.is_empty() { 0 } else { atoull(r.get(0, 0)) };
        item.max(bank)
    }

    async fn create_new_item(&mut self, it: &mut Item, char_id: u32, db: &mut Db) {
        if it.uid == 0 {
            it.uid = self.get_last_inserted_item_id(db).await + 1;
        }
        let n = good(&it.name);
        let sql = if it.in_bank {
            format!(
                "INSERT INTO `bank_item` ( `CharID` , `ItemName`, `Count` , `ItemType`, `ID1`, `ID2`, `ID3`, `Color`, `Effect1`, `Effect2`, `Effect3`, `LifeSpan`, `Attribute`, `ItemID`) VALUES ( '{char_id}' , '{n}' , '{}' , '{}' , '{}', '{}', '{}', '{}' , '{}' , '{}' , '{}' , '{}' , '{}' , '{}' );",
                it.count, it.touch_type, it.tv1, it.tv2, it.tv3, it.color, it.spec1, it.spec2, it.spec3, it.life_span, it.attribute, it.uid
            )
        } else {
            format!(
                "INSERT INTO `item` ( `CharID` , `ItemName`, `Count` , `ItemType`, `ID1`, `ID2`, `ID3`, `Color`, `Effect1`, `Effect2`, `Effect3`, `LifeSpan`, `Attribute`, `ItemEquip`, `ItemPosX`, `ItemPosY`, `ItemID`) VALUES ( '{char_id}' , '{n}' , '{}' , '{}' , '{}', '{}', '{}', '{}' , '{}' , '{}' , '{}' , '{}' , '{}' , '{}' , '{}' , '{}' , '{}' );",
                it.count, it.touch_type, it.tv1, it.tv2, it.tv3, it.color, it.spec1, it.spec2, it.spec3, it.life_span, it.attribute, it.equipped as i32, it.pos_x, it.pos_y, it.uid
            )
        };
        db.qs(&sql).await;
    }

    async fn check_dup_item(&mut self, it: &Item, db: &mut Db, bank: bool) -> u8 {
        // no dupe checking on bound items
        if it.touch_type == 1 || (it.tv1 == 0 && it.tv2 == 0 && it.tv3 == 0) {
            return NOITEM;
        }
        let table = if bank { "bank_item" } else { "item" };
        let sql = format!(
            "SELECT `ItemName` FROM `{table}` WHERE `ItemName` = '{}' AND `ID1` = '{}' AND `ID2` = '{}' AND `ID3` = '{}';",
            good(&it.name),
            it.tv1,
            it.tv2,
            it.tv3
        );
        let Some(r) = db.qs(&sql).await else { return 0 };
        let n = r.rows.len() as u8;
        let (bag, bnk) = if bank { (0, n) } else { (n, 0) };
        match (bag, bnk) {
            (1, 0) => ONEINBAG,
            (0, 1) => ONEINBANK,
            (1, 1) => ONEINBAG_ONEINBANK,
            (b, _) if b > 1 => DUPITEMINBAG,
            (_, k) if k > 1 => DUPITEMINBANK,
            _ => NOITEM,
        }
    }

    async fn process_item_save(&mut self, it: &mut Item, db: &mut Db) {
        if it.uid != 0 {
            if db.qs(&format!("DELETE FROM `item` WHERE `ItemID` = '{}';", it.uid)).await.is_none() {
                return;
            }
            if db.qs(&format!("DELETE FROM `bank_item` WHERE `ItemID` = '{}';", it.uid)).await.is_none() {
                return;
            }
        } else {
            it.uid = self.get_last_inserted_item_id(db).await + 1;
        }
        if it.dup_code != NOITEM {
            log_to(
                &format!("(ERROR) Item({}) with duplied ID on the database! ID1({}) ID2({}) ID3({})", s(&it.name), it.tv1, it.tv2, it.tv3),
                WARN_MSG,
                HACK_LOGFILE,
            );
        }
    }

    fn read_item(c: Rd<'_>, bank: bool) -> Item {
        Item {
            name: c.s(0, 20),
            count: c.u32(20),
            touch_type: c.u16(24) as i16,
            tv1: c.u16(26) as i16,
            tv2: c.u16(30) as i16,
            tv3: c.u32(34) as i16,
            color: c.u8(38),
            spec1: c.u16(39) as i16,
            spec2: c.u16(41) as i16,
            spec3: c.u16(43) as i16,
            life_span: c.u16(45),
            attribute: c.u32(47),
            equipped: !bank && c.u8(51) != 0,
            pos_x: if bank { 0 } else { c.u16(52) as i16 },
            pos_y: if bank { 0 } else { c.u16(54) as i16 },
            uid: c.u64(if bank { 51 } else { 56 }),
            in_bank: bank,
            dup_code: 0,
        }
    }

    /// `SaveCharacter(Data)`: Data = name/account/password/flag, then the character block at +31.
    async fn save_character(&mut self, d: Rd<'_>, db: &mut Db) {
        let (ch, acc) = (d.s(0, 10), d.s(10, 10));
        let char_id = self.get_char_id(&ch, &acc, db).await;
        // fixed: the C++ dereferenced a NULL row here for an unknown character; with CharID 0 it would also
        // delete and rewrite the items of CharID 0
        if char_id == 0 {
            log_to(&format!("(ERROR) Character({}) of account({}) not found, save ignored.", s(&ch), s(&acc)), WARN_MSG, ERROR_LOGFILE);
            return;
        }
        // fixed: the C++ deletes every item before re-inserting them, so a failed step lost them; one transaction
        if db.qs("START TRANSACTION;").await.is_none() {
            return;
        }
        let ok = self.save_character_rows(d.at(31), char_id, db).await;
        db.qs(if ok { "COMMIT;" } else { "ROLLBACK;" }).await;
    }

    async fn save_character_rows(&mut self, c: Rd<'_>, char_id: u32, db: &mut Db) -> bool {
        const CHAR_INDEX_END: usize = 751;
        // 0..9 as the C++, plus skill 10 (Rust only, see SKILL_PRETENDCORPSE; a C++ HG sends it as 0)
        for w in 0..=SKILL_PRETENDCORPSE as usize {
            let sql = format!(
                "UPDATE `skill` SET `SkillMastery` = '{}',`SkillSSN` = '{}' WHERE `CharID` = '{char_id}' AND `SkillID` = '{w}' LIMIT 1;",
                c.u8(598 + w),
                c.u32(653 + w * 4)
            );
            if db.qs(&sql).await.is_none() {
                return false;
            }
        }
        let n_items = c.u8(CHAR_INDEX_END) as usize;
        let mut cur_item_id = self.get_last_inserted_item_id(db).await + 1;
        // HG's save writes the bank count right after the bag items but sends only up to it (37 + Index
        // bytes), so the bank part normally never arrives
        let index = CHAR_INDEX_END + 1 + n_items * 64;
        let bank_sent = c.len() > index;
        let mut bank_modified = c.u8(241) != 0;
        if bank_modified && !bank_sent {
            // fixed: the C++ then deleted every bank item and saved none (count read past the message as 0)
            log_to(&format!("(ERROR) CharID({char_id}) save: bank marked modified but not sent, bank kept."), WARN_MSG, ERROR_LOGFILE);
            bank_modified = false;
        }
        self.delete_all_items_from_char(char_id, db, bank_modified).await;
        let mut vals = Vec::new();
        for w in 0..n_items {
            let mut it = Self::read_item(c.at(CHAR_INDEX_END + 1 + w * 64), false);
            if it.name.is_empty() {
                continue;
            }
            if it.uid == 0 {
                it.uid = cur_item_id;
                cur_item_id += 1;
            }
            it.dup_code = self.check_dup_item(&it, db, false).await;
            vals.push(item_values(&it, char_id));
            self.process_item_save(&mut it, db).await;
        }
        let bag = "INSERT INTO `item` ( `CharID` , `ItemName`, `Count` , `ItemType`, `ID1`, `ID2`, `ID3`, `Color`, `Effect1`, `Effect2`, `Effect3`, `LifeSpan`, `Attribute`, `ItemEquip`, `ItemPosX`, `ItemPosY`, `ItemID`) VALUES";
        if let Some(q) = insert_sql(bag, &vals) {
            if db.qs(&q).await.is_none() {
                return false;
            }
        }
        if bank_modified {
            let n_bank = c.u8(index) as usize;
            let mut vals = Vec::new();
            for w in 0..n_bank {
                let mut it = Self::read_item(c.at(index + 1 + w * 59), true);
                if it.name.is_empty() {
                    continue;
                }
                if it.uid == 0 {
                    it.uid = cur_item_id;
                    cur_item_id += 1;
                }
                it.dup_code = self.check_dup_item(&it, db, true).await;
                vals.push(item_values(&it, char_id));
                self.process_item_save(&mut it, db).await;
            }
            let bank = "INSERT INTO `bank_item` ( `CharID` , `ItemName`, `Count` , `ItemType`, `ID1`, `ID2`, `ID3`, `Color`, `Effect1`, `Effect2`, `Effect3`, `LifeSpan`, `Attribute`, `ItemID`) VALUES ";
            if let Some(q) = insert_sql(bank, &vals) {
                if db.qs(&q).await.is_none() {
                    return false;
                }
            }
        }
        db.qs(&char_update_sql(c, char_id)).await.is_some()
    }

    async fn request_create_new_guild(&mut self, d: Rd<'_>, sock: usize, db: &mut Db) {
        let (ch, acc, pwd, guild, loc) = (d.s(0, 10), d.s(10, 10), d.s(20, 10), d.s(30, 20), d.s(50, 10));
        let mut b = [0u8; 50];
        W(&mut b).u32(0, MSGID_RESPONSE_CREATENEWGUILD);
        W(&mut b).cstr(6, &ch);
        let reject = |b: &mut [u8; 50]| W(b).u16(4, LOGRESMSGTYPE_REJECT);
        // fix: the C++ checked the account in the "world" read at 60..90, where the HG sends the guild GUID (and nothing
        // after its 75 bytes): no account ever matched, so no guild could be created
        if self.check_account_login(&acc, &pwd, None, db).await != LOGINOK || self.guild_exists(&guild, db).await.is_some() {
            reject(&mut b);
            self.send_msg_to_gs(sock, &b[..16]);
            return;
        }
        let t = sys_time_str();
        let (gg, gc) = (good(&guild), good(&ch));
        let sql = format!(
            "INSERT INTO `guild` ( `GuildName` , `MasterName` , `Nation` , `NumberOfMembers` , `CreateDate` ) VALUES ('{gg}', '{gc}', '{}', '1', '{t}');",
            // fixed: the C++ put the guild location in unescaped
            good(&loc)
        );
        if db.qs(&sql).await.is_none() {
            return;
        }
        // (the C++ GuildID stays uninitialised if this lookup fails)
        let guild_id = self.guild_exists(&guild, db).await.unwrap_or(0);
        let sql = format!("INSERT INTO `guild_member` ( `GuildName` , `MemberName` , `JoinDate` ) VALUES ('{gg}', '{gc}', '{t}');");
        if db.qs(&sql).await.is_none() {
            return;
        }
        W(&mut b).u16(4, LOGRESMSGTYPE_CONFIRM);
        W(&mut b).u32(16, guild_id);
        self.send_msg_to_gs(sock, &b[..20]);
    }

    async fn is_guild_master(&mut self, ch: &[u8], guild: &[u8], db: &mut Db) -> bool {
        let Some(r) = db.qs(&format!("SELECT `MasterName` FROM `guild` WHERE `GuildName` = '{}' LIMIT 1;", good(guild))).await else {
            return false;
        };
        r.rows.len() == 1 && is_same(r.get(0, 0), ch)
    }

    async fn request_disband_guild(&mut self, d: Rd<'_>, sock: usize, db: &mut Db) {
        let (ch, acc, pwd, guild) = (d.s(0, 10), d.s(10, 10), d.s(20, 10), d.s(30, 20));
        let mut b = [0u8; 50];
        W(&mut b).u32(0, MSGID_RESPONSE_DISBANDGUILD);
        W(&mut b).cstr(6, &ch);
        // fix: as for the creation, the "world" at 60.. is past the 56 bytes the HG sends: no guild could be disbanded
        let ok = self.check_account_login(&acc, &pwd, None, db).await == LOGINOK
            && self.guild_exists(&guild, db).await.is_some()
            && self.is_guild_master(&ch, &guild, db).await;
        if ok {
            let gg = good(&guild);
            let sql = format!("DELETE FROM `guild` WHERE `GuildName` = '{gg}' AND `MasterName` = '{}' LIMIT 1;", good(&ch));
            if db.qs(&sql).await.is_none() {
                return;
            }
            if db.qs(&format!("DELETE FROM `guild_member` WHERE `GuildName` = '{gg}';")).await.is_none() {
                return;
            }
            W(&mut b).u16(4, LOGRESMSGTYPE_CONFIRM);
        } else {
            W(&mut b).u16(4, LOGRESMSGTYPE_REJECT);
        }
        self.send_msg_to_gs(sock, &b[..16]);
    }

    async fn add_guild_member(&mut self, d: Rd<'_>, db: &mut Db) {
        let (ch, guild) = (d.s(0, 10), d.s(10, 20));
        let Some(guild_id) = self.guild_exists(&guild, db).await else { return };
        let sql = format!(
            "INSERT INTO `guild_member` ( `GuildName` , `MemberName` , `JoinDate` ) VALUES ('{}', '{}', '{}');",
            good(&guild),
            good(&ch),
            sys_time_str()
        );
        if db.qs(&sql).await.is_none() {
            return;
        }
        let Some(r) = db.qs(&format!("SELECT `NumberOfMembers` FROM `guild` WHERE `GuildID` = '{guild_id}';")).await else { return };
        // (uninitialised in the C++ when the guild row is gone)
        let n: u16 = if r.rows.len() == 1 { (atoi(r.get(0, 0)) + 1) as u16 } else { 0 };
        db.qs(&format!("UPDATE `guild` SET `NumberOfMembers` = '{n}' WHERE `GuildID` = '{guild_id}' LIMIT 1;")).await;
    }

    async fn remove_guild_member(&mut self, d: Rd<'_>, db: &mut Db) {
        let (ch, guild) = (d.s(0, 10), d.s(10, 20));
        let Some(guild_id) = self.guild_exists(&guild, db).await else { return };
        if db.qs(&format!("DELETE FROM `guild_member` WHERE `MemberName` = '{}';", good(&ch))).await.is_none() {
            return;
        }
        let Some(r) = db.qs(&format!("SELECT `NumberOfMembers` FROM `guild` WHERE `GuildID` = '{guild_id}';")).await else { return };
        let mut n: u16 = 0;
        if r.rows.len() == 1 {
            n = members_after_remove(atoi(r.get(0, 0)));
        }
        db.qs(&format!("UPDATE `guild` SET `NumberOfMembers` = '{n}' WHERE `GuildID` = '{guild_id}' LIMIT 1;")).await;
    }

    async fn get_account_cash(&mut self, acc: &[u8], db: &mut Db) -> i32 {
        // fixed: the C++ put the account name in unescaped
        let Some(r) = db.qs(&format!("SELECT `cash` FROM `account_database` WHERE `name` = '{}' LIMIT 1;", good(acc))).await else { return 0 };
        if r.rows.len() != 1 {
            return 0;
        }
        atoi(r.get(0, 0))
    }

    async fn request_legion_pts(&mut self, d: Rd<'_>, sock: usize, db: &mut Db) {
        let client_h = d.u16(0);
        let cash = self.get_account_cash(&d.s(2, 10), db).await;
        let mut b = [0u8; 12];
        W(&mut b).u32(0, MSGID_RESPONSE_LGNPTS);
        W(&mut b).u16(4, LOGRESMSGTYPE_CONFIRM);
        W(&mut b).u16(6, client_h);
        W(&mut b).u32(8, cash as u32);
        self.send_msg_to_gs(sock, &b);
    }

    async fn request_legion_svc(&mut self, d: Rd<'_>, sock: usize, db: &mut Db) {
        let cmd = d.u16(0);
        // fixed: an unknown command matched the price-0 terminator in the C++ and was always granted
        let Some((_, price, _)) = lgn_svc(cmd) else { return };
        let client_h = d.u16(2);
        let cash = self.get_account_cash(&d.s(4, 10), db).await;
        // long < unsigned long compares as unsigned
        if (cash as u32) < price {
            return;
        }
        let mut b = [0u8; 8];
        W(&mut b).u32(0, MSGID_RESPONSE_LGNSVC);
        W(&mut b).u16(4, cmd);
        W(&mut b).u16(6, client_h);
        self.send_msg_to_gs(sock, &b);
    }

    async fn sub_account_cash(&mut self, d: Rd<'_>, db: &mut Db) {
        let acc = good(&d.s(0, 10));
        let Some((name, price, _)) = lgn_svc(d.u16(10)) else { return };
        // fixed: never take the unsigned cash below 0 (the C++ query then failed 10 times)
        let Some(r) = db.qs(&format!("UPDATE `account_database` SET `cash`=cash-'{price}' WHERE `name` = '{acc}' AND `cash` >= '{price}' LIMIT 1;")).await else {
            return;
        };
        let _ = r;
        if db.affected_rows() == 0 {
            return;
        }
        db.qs(&format!("INSERT INTO `cash_transactions` (account_name, service, date) VALUES ('{acc}', '{name}', NOW());")).await;
    }

    fn stock_to(&mut self, w: usize, data: &[u8]) {
        if let Some(Some(g)) = self.gs.get_mut(w) {
            // bStockMsgToGameServer
            if g.stock.len() + data.len() < MAXGAMESERVERSTOCKMSGSIZE - 10 {
                g.stock.extend_from_slice(data);
            }
        }
    }

    /// Stock to every other server (`GameServerSocket[ID]->GSID != w`).
    fn stock_others(&mut self, sock: usize, data: &[u8]) {
        let Some(g) = &self.gsock[sock] else { return };
        let own = g.gsid as i32;
        for w in 0..MAXGAMESERVERS {
            if own != w as i32 {
                self.stock_to(w, data);
            }
        }
    }

    fn stock_by_internal_id(&mut self, iid: u16, data: &[u8], first_only: bool, invert: bool) {
        for w in 0..MAXGAMESERVERS {
            let Some(g) = &self.gs[w] else { continue };
            if (g.internal_id == iid) != invert {
                self.stock_to(w, data);
                if first_only {
                    break;
                }
            }
        }
    }

    fn server_stock_msg_handler(&mut self, body: &[u8], sock: usize) {
        let d = Rd::new(body);
        let mut cp = 6usize;
        loop {
            let c = d.at(cp);
            match c.u8(0) {
                GSM_DISCONNECT => {
                    let name = c.s(1, 10);
                    cp += 11;
                    // fixed: the C++ returned from the whole handler here, dropping the stock messages after it,
                    // and on a miss deleted Client[MAXCLIENTS], one past the array
                    if let Some(w) = self.clients.iter().position(|cl| cl.as_ref().is_some_and(|cl| is_same(&cl.char_name, &name))) {
                        self.clients[w].as_mut().unwrap().is_playing = false;
                    }
                }
                GSM_REQUEST_FINDCHARACTER => {
                    let data = c.raw(0, 25);
                    self.stock_by_internal_id(c.u16(1), &data, false, true);
                    cp += 25;
                }
                GSM_RESPONSE_FINDCHARACTER => {
                    let data = c.raw(0, 39);
                    self.stock_by_internal_id(c.u16(1), &data, true, false);
                    cp += 39;
                }
                GSM_RESPONSE_FINDFRIEND => {
                    let data = c.raw(0, 25);
                    self.stock_by_internal_id(c.u16(1), &data, true, false);
                    cp += 25;
                }
                GSM_RESPONSE_SHUTUPPLAYER => {
                    let data = c.raw(0, 27);
                    self.stock_by_internal_id(c.u16(1), &data, true, false);
                    cp += 27;
                }
                op => {
                    // messages relayed to every other server: (bytes copied, bytes skipped)
                    let (n, skip) = match op {
                        GSM_REQUEST_FINDFRIEND => {
                            let n = c.u16(15) as usize + 17;
                            (n, n)
                        }
                        GSM_CHATMSG => {
                            let n = c.u16(16) as usize + 18;
                            (n, n)
                        }
                        GSM_WHISPERMSG => {
                            let n = c.u16(11) as usize + 13;
                            (n, n)
                        }
                        GSM_REQUEST_SHUTUPPLAYER => (27, 27),
                        // fixed: GSM_ENDAPOCALYPSE is 5 bytes (both HGs); the C++ relayed and skipped 18 like ENDCRUSADE
                        GSM_BEGINCRUSADE | GSM_BEGINAPOCALYPSE | GSM_ENDAPOCALYPSE | GSM_COLLECTEDMANA | GSM_GRANDMAGICLAUNCH => (5, 5),
                        GSM_ENDCRUSADE => (18, 18),
                        GSM_CONSTRUCTIONPOINT => (9, 9),
                        GSM_REQUEST_SUMMONPLAYER => (25, 25),
                        // fixed: the C++ relayed 25 bytes but skipped 35; HG never sends this one, keep both 25
                        GSM_REQUEST_SUMMONGUILD => (25, 25),
                        GSM_SETGUILDTELEPORTLOC | GSM_SETGUILDCONSTRUCTLOC => (23, 23),
                        GSM_MIDDLEMAPSTATUS => {
                            let n = 3 + c.u16(1) as usize * 6;
                            (n, n)
                        }
                        GSM_REQUEST_SETFORCERECALLTIME => (3, 3),
                        GSM_GRANDMAGICRESULT => {
                            // the Rust HG sends all 19 + (count + 1) * 2 bytes (the C++ HG cut the last one)
                            let n = 19 + (c.u16(19) as usize + 1) * 2;
                            (n, n)
                        }
                        // defined but never sent by the C++ HG; the Rust HG relays the heldenian start / end
                        GSM_STARTHELDENIAN => (7, 7),
                        GSM_ENDHELDENIAN => (9, 9),
                        _ => return,
                    };
                    let data = c.raw(0, n);
                    self.stock_others(sock, &data);
                    cp += skip;
                }
            }
        }
    }

    fn party_operation_result_handler(&mut self, body: &[u8], sock: usize) {
        let d = Rd::new(body);
        let req = d.u16(4);
        let gsch = d.u16(6) as i32;
        let name = d.s(8, 10);
        let mut party_id = d.u16(18) as i32;
        let client_h = sock as i32;
        // members record the game server they play on (see PartyManager::game_server_down)
        let server = self.gsock[sock].as_ref().map_or(-1, |g| g.gsid as i32);
        let mut out = Vec::new();
        // the reply is built over the received buffer (message + 2 zero bytes), so byte 21 keeps whatever was there
        let mut b = body.to_vec();
        b.resize(body.len().max(20) + 2, 0);
        let reply = |b: &mut Vec<u8>, kind: u16, ok: bool, pid: i32| {
            let mut w = W(b);
            w.u32(0, MSGID_PARTYOPERATION);
            w.u16(4, kind);
            w.u8(6, ok as u8);
            w.u16(7, gsch as u16);
            let mut n = name.clone();
            n.resize(10, 0);
            w.bytes(9, &n);
            w.u16(19, pid as u16);
            b[..22].to_vec()
        };
        match req {
            1 => {
                party_id = self.party.create_new_party(server, &name);
                let r = reply(&mut b, 1, party_id != 0, party_id);
                self.send_msg_to_gs(sock as u8 as usize, &r);
            }
            3 => {
                let ok = self.party.add_member(server, party_id, &name, &mut out);
                self.party_out(std::mem::take(&mut out));
                let r = reply(&mut b, 4, ok, party_id);
                if ok { self.send_msg_to_all_game_servers(None, &r) } else { self.send_msg_to_gs(sock, &r) }
            }
            4 => {
                let ok = self.party.remove_member(party_id, &name, &mut out);
                self.party_out(std::mem::take(&mut out));
                let r = reply(&mut b, 6, ok, party_id);
                if ok { self.send_msg_to_all_game_servers(None, &r) } else { self.send_msg_to_gs(sock, &r) }
            }
            5 => {
                self.party.check_party_member(client_h, gsch, party_id, &name, &mut out);
            }
            6 => {
                self.party.get_party_info(client_h, gsch, &name, party_id, &mut out);
            }
            7 => {
                let t = self.now();
                self.party.set_server_change_status(&name, party_id, t);
            }
            _ => {}
        }
        self.party_out(out);
    }

    fn process_shutdown(&mut self, time: u32) {
        if self.gs.iter().all(|g| g.is_none()) {
            self.servers_being_shutdown = false;
            self.shutdown_msg_index = 0;
            return;
        }
        if time.wrapping_sub(self.shutdown_interval) > SHUTDOWN_INTERVAL_MSG {
            self.shutdown_interval = time;
            let mut b = [0u8; 10];
            let len = if self.shutdown_msg_index <= 1 {
                log("(ERROR) Sending server shutdown announcement!");
                W(&mut b).u32(0, MSGID_SENDSERVERSHUTDOWNMSG);
                W(&mut b).u16(6, self.shutdown_msg_index as u16 + 1);
                8
            } else {
                log("(ERROR) Shutting down all the servers!");
                W(&mut b).u32(0, MSGID_GAMESERVERSHUTDOWNED);
                4
            };
            for w in 0..MAXGAMESERVERS {
                if self.gs[w].as_ref().is_some_and(|g| g.is_initialized) {
                    self.gs_send(w, &b[..len], false);
                }
            }
            self.shutdown_msg_index = self.shutdown_msg_index.wrapping_add(1);
        }
    }

    // ---------------------------------------------------------------- timer

    /// `MsgProcess`: queued game-server messages on the main connection, then the stock messages.
    async fn msg_process(&mut self) {
        while let Some((data, id)) = self.queue.pop_front() {
            let Some(mut db) = self.main_db.take() else { break };
            self.process_gs_msg_with_db_access(&data, id as u8 as usize, &mut db).await;
            self.main_db = Some(db);
        }
        for w in 0..MAXGAMESERVERS {
            let Some(g) = &mut self.gs[w] else { continue };
            if g.stock.len() > 6 {
                let data = std::mem::replace(&mut g.stock, new_stock());
                self.gs_send(w, &data, false);
            }
        }
    }

    /// `CheckServerStatus`
    async fn check_server_status(&mut self) -> bool {
        let ok = match &mut self.main_db {
            Some(db) => db.ping().await,
            None => false,
        };
        if ok {
            return true;
        }
        if let Some(mut db) = self.main_db.take() {
            db.close().await;
        }
        self.autofix = true;
        log_lv("(ERROR) MySql is inacessible, trying to restart the server...", WARN_MSG);
        self.csock.iter_mut().for_each(|s| *s = None);
        for w in 0..MAXGAMESERVERS {
            self.delete_game_server(w);
        }
        self.gsock.iter_mut().for_each(|s| *s = None);
        for w in 0..MAXCLIENTS {
            self.delete_client(w);
        }
        self.queue.clear();
        self.mysql_auto_fix().await;
        false
    }

    async fn mysql_auto_fix(&mut self) {
        self.mysql_timer = self.now();
        if self.autofix_num < MAX_MYSQL_RESTART_ATTEMPT {
            // RestartServer
            match Db::connect(self.cfg.db.opts(&self.cfg.db_name)).await {
                Ok(db) => {
                    log("(ERROR) Connection to mySQL database was sucessfully established!");
                    self.main_db = Some(db);
                    self.autofix_num = 1;
                    log("-Server was sucessfully restarted!");
                    self.autofix = false;
                }
                Err(_) => {
                    log_lv("(ERROR) Failed to connect to mySQL, please check if the mySQL server is online.", WARN_MSG);
                    log_lv(&format!("(ERROR) Server restart attempt {} of {} has failed...", self.autofix_num, MAX_MYSQL_RESTART_ATTEMPT), WARN_MSG);
                    self.autofix_num += 1;
                }
            }
        } else {
            self.timer_stopped = true;
            log_lv("(ERROR) CRITICAL ERROR! Impossible to restart MySql database.", WARN_MSG);
        }
    }

    pub async fn on_timer(&mut self) {
        if self.timer_stopped {
            return;
        }
        self.msg_process().await;
        let t = self.now();
        if self.servers_being_shutdown {
            self.process_shutdown(t);
        }
        if self.autofix {
            if t.wrapping_sub(self.mysql_timer) > MYSQL_AUTOFIX_TIMERSIZE {
                self.mysql_auto_fix().await;
            }
            return;
        }
        if !self.check_server_status().await {
            return;
        }
        if t.wrapping_sub(self.check_accounts_timer) > CHECKACCSTATUS_TIMERINTERVAL {
            self.check_active_accounts_number(t).await;
        }
        for w in 0..MAXCLIENTS {
            let Some(c) = &self.clients[w] else { continue };
            // (playing clients whose server is gone are kept: the delete is commented out in the C++)
            if !c.is_playing && t.wrapping_sub(c.time) > MAXWAITTIMEFORPLAYERENTERGAME {
                if c.force_disconn_request_time == 0 {
                    self.request_force_disconnect(w, 10);
                } else if t.wrapping_sub(c.force_disconn_request_time) > MAX_FORCEDISCONN_WAIT_TIME {
                    log_to(
                        &format!("(ERROR) Client({}) was deleted with no savedata due to no response from gameserver.", s(&c.account_name)),
                        WARN_MSG,
                        ERROR_LOGFILE,
                    );
                    self.delete_client(w);
                }
            }
        }
        for w in 0..MAXGAMESERVERS {
            let Some(g) = &self.gs[w] else { continue };
            if g.alive_response_time < t && t.wrapping_sub(g.alive_response_time) > MAX_GSALIVE_WAITINTERVAL {
                log_lv(&format!("(ERROR) There is no response from GameServer({})!", s(&g.server_name)), WARN_MSG);
            } else if t.wrapping_sub(g.total_players_response) > INTERVALTOSEND_TOTALPLAYERS {
                let mut b = [0u8; 8];
                W(&mut b).u32(0, MSGID_TOTALGAMESERVERCLIENTS);
                W(&mut b).u16(6, self.active_accounts);
                self.gs_send(w, &b, false);
                self.gs[w].as_mut().unwrap().total_players_response = t;
                let mut out = Vec::new();
                self.party.check_member_activity(t, &mut out);
                self.party_out(out);
            }
        }
    }
}

/// 0 not tried, 1 the `bot_memory` table is there, 2 it could not be made (a failed query makes `Db::q` retry for 10 s: never again).
static BOTMEM_TABLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Makes the table of the bots' memory the first time it is needed.
async fn bot_memory_table(db: &mut Db) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    if BOTMEM_TABLE.load(Relaxed) == 0 {
        let ok = db.qs("CREATE TABLE IF NOT EXISTS `bot_memory` (`bot` varchar(10) NOT NULL, `data` mediumtext NOT NULL, `updated` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP, PRIMARY KEY (`bot`)) ENGINE=InnoDB;").await.is_some();
        BOTMEM_TABLE.store(if ok { 1 } else { 2 }, Relaxed);
    }
    BOTMEM_TABLE.load(Relaxed) == 1
}

/// `d`: the bot's name (10 bytes) and its memory as hex digits (only those are accepted: they go into the query as they are).
async fn save_bot_memory(d: Rd<'_>, db: &mut Db) {
    let (bot, data) = (d.s(0, 10), d.s(10, 16000));
    if bot.is_empty() || !data.iter().all(|c| c.is_ascii_hexdigit()) || !bot_memory_table(db).await {
        return;
    }
    db.qs(&format!("REPLACE INTO `bot_memory` (`bot`, `data`) VALUES ('{}', '{}');", good(&bot), s(&data))).await;
}

/// 0 not tried, 1 the `bot_history` table is there, 2 it could not be made (as `BOTMEM_TABLE`).
static BOTHIST_TABLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The rows kept for each bot: the older ones go when it has more.
const BOTHIST_KEEP: usize = 1500;

async fn bot_history_table(db: &mut Db) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    if BOTHIST_TABLE.load(Relaxed) == 0 {
        let ok = db
            .qs("CREATE TABLE IF NOT EXISTS `bot_history` (`id` bigint NOT NULL AUTO_INCREMENT, `bot` varchar(10) NOT NULL, `t` bigint NOT NULL, `kind` tinyint NOT NULL, `other` varchar(10) NOT NULL, `text` varchar(240) NOT NULL, PRIMARY KEY (`id`), KEY `bot_id` (`bot`, `id`)) ENGINE=InnoDB;")
            .await
            .is_some();
        BOTHIST_TABLE.store(if ok { 1 } else { 2 }, Relaxed);
    }
    BOTHIST_TABLE.load(Relaxed) == 1
}

/// 0 not tried, 1 the `client_settings` table is there, 2 it could not be made (as `BOTMEM_TABLE`).
static SETTINGS_TABLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Makes the table of the clients' settings the first time it is needed.
async fn client_settings_table(db: &mut Db) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    if SETTINGS_TABLE.load(Relaxed) == 0 {
        let ok = db.qs("CREATE TABLE IF NOT EXISTS `client_settings` (`char_name` varchar(10) NOT NULL, `data` text NOT NULL, `updated` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP, PRIMARY KEY (`char_name`)) ENGINE=InnoDB;").await.is_some();
        SETTINGS_TABLE.store(if ok { 1 } else { 2 }, Relaxed);
    }
    SETTINGS_TABLE.load(Relaxed) == 1
}

/// The query that saves `hex` for `ch`; None unless `hex` is nothing but hex digits (it goes into the query as it is) and fits.
fn client_settings_sql(ch: &[u8], hex: &[u8]) -> Option<String> {
    if ch.is_empty() || hex.len() > 4000 || !hex.iter().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("REPLACE INTO `client_settings` (`char_name`, `data`) VALUES ('{}', '{}');", good(ch), s(hex)))
}

/// `d`: the character's name (10 bytes) and its settings as hex digits.
async fn save_client_settings(d: Rd<'_>, db: &mut Db) {
    let Some(sql) = client_settings_sql(&d.s(0, 10), &d.s(10, 4001)) else { return };
    if client_settings_table(db).await {
        db.qs(&sql).await;
    }
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `to_hex` read back (the digits were checked by the caller).
fn from_hex(h: &[u8]) -> Vec<u8> {
    h.chunks(2).filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok()).collect()
}

/// One line of a bot's history, `t \t kind \t other \t text` (t: unix ms, kind: 0 chat, 1 action, 2 deduction): `None` when it does not fit. What goes into the
/// query is only letters, digits, blanks and a few signs: no quote, backslash or control character gets through.
fn parse_bot_history(line: &str) -> Option<(u64, u8, String, String)> {
    let mut f = line.splitn(4, '\t');
    let t = f.next()?.parse::<u64>().ok()?;
    let kind = f.next()?.parse::<u8>().ok().filter(|k| *k <= 2)?;
    let clean = |s: &str, n: usize| -> String { s.chars().map(|c| if c.is_ascii_alphanumeric() || " .,:;!?()-_/+*=%#@&<>[]".contains(c) { c } else { ' ' }).take(n).collect() };
    let (other, text) = (clean(f.next()?, 10), clean(f.next()?, 240));
    (!text.trim().is_empty()).then_some((t, kind, other, text))
}

/// `d`: the bot's name (10 bytes) and new history lines as hex digits (only those are accepted). Each becomes a row; the oldest rows beyond `BOTHIST_KEEP` go.
async fn save_bot_history(d: Rd<'_>, db: &mut Db) {
    let (bot, data) = (d.s(0, 10), d.s(10, 16000));
    if bot.is_empty() || data.len() % 2 != 0 || !data.iter().all(|c| c.is_ascii_hexdigit()) || !bot_history_table(db).await {
        return;
    }
    let name = good(&bot);
    let text = String::from_utf8_lossy(&from_hex(&data)).to_string();
    let rows: Vec<String> = text.lines().filter_map(parse_bot_history).map(|(t, k, o, x)| format!("('{name}', {t}, {k}, '{o}', '{x}')")).collect();
    if rows.is_empty() {
        return;
    }
    db.qs(&format!("INSERT INTO `bot_history` (`bot`, `t`, `kind`, `other`, `text`) VALUES {};", rows.join(", "))).await;
    db.qs(&format!(
        "DELETE FROM `bot_history` WHERE `bot` = '{name}' AND `id` <= (SELECT `id` FROM (SELECT `id` FROM `bot_history` WHERE `bot` = '{name}' ORDER BY `id` DESC LIMIT 1 OFFSET {BOTHIST_KEEP}) `old`);"
    ))
    .await;
}

fn is_db_msg(m: u32) -> bool {
    matches!(
        m,
        MSGID_REQUEST_PLAYERDATA
            | MSGID_REQUEST_NOSAVELOGOUT
            | MSGID_REQUEST_SAVEPLAYERDATALOGOUT
            | MSGID_REQUEST_SAVEPLAYERDATA_REPLY
            | MSGID_REQUEST_SAVEPLAYERDATA
            | MSGID_REQUEST_CREATENEWGUILD
            | MSGID_REQUEST_DISBANDGUILD
            | MSGID_REQUEST_UPDATEGUILDINFO_NEWGUILDSMAN
            | MSGID_REQUEST_UPDATEGUILDINFO_DELGUILDSMAN
            | MSGID_REQUEST_LGNPTS
            | MSGID_REQUEST_LGNSVC
            | MSGID_SUBCASH
            | MSGID_REQUEST_BOTMEM
            | MSGID_BOTMEM_SAVE
            | MSGID_REQUEST_BOTHIST
            | MSGID_BOTHIST_SAVE
            | MSGID_CLIENTSETTINGS_SAVE
    )
}

/// Messages OnGameServerRead handles (and silently ignores from an unregistered socket).
fn is_gs_msg(m: u32) -> bool {
    is_db_msg(m)
        || matches!(
            m,
            MSGID_ENTERGAMECONFIRM
                | MSGID_REQUEST_SETACCOUNTWAITSTATUS
                | MSGID_GAMEMASTERLOG
                | MSGID_PLAYERLOG
                | MSGID_GAMEITEMLOG
                | MSGID_ITEMLOG
                | MSGID_GAMECRUSADELOG
                | MSGID_GAMESERVERALIVE
                | MSGID_GAMESERVERSHUTDOWNED
                | MSGID_SERVERSTOCKMSG
                | MSGID_PARTYOPERATION
                | MSGID_SENDSERVERSHUTDOWNMSG
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_history_lines_are_checked_before_they_reach_the_query() {
        assert_eq!(parse_bot_history("123\t0\tPulpo\tme: hola"), Some((123, 0, "Pulpo".into(), "me: hola".into())));
        assert_eq!(parse_bot_history("5\t2\tX\tit's a \"trap\"; DROP\\ ").map(|r| r.3), Some("it s a  trap ; DROP  ".to_string()), "quotes and backslashes become blanks");
        assert!(parse_bot_history("x\t0\ta\tb").is_none() && parse_bot_history("1\t3\ta\tb").is_none() && parse_bot_history("1\t0\ta").is_none() && parse_bot_history("1\t0\ta\t   ").is_none());
        assert_eq!(parse_bot_history(&format!("1\t1\t{}\t{}", "o".repeat(30), "t".repeat(500))).map(|r| (r.2.len(), r.3.len())), Some((10, 240)));
        assert_eq!(from_hex(to_hex(b"a\tb\n").as_bytes()), b"a\tb\n");
    }

    #[test]
    fn stats_rule() {
        assert!(new_char_stats_ok([10, 10, 10, 14, 14, 12], 0));
        assert!(!new_char_stats_ok([10, 10, 10, 14, 14, 11], 0)); // sum 69
        assert!(!new_char_stats_ok([9, 10, 13, 14, 14, 10], 0)); // below 10
        assert!(!new_char_stats_ok([15, 10, 10, 13, 12, 10], 0)); // above 14
        assert!(!new_char_stats_ok([10, 10, 10, 14, 14, 12], 4)); // account full
    }

    #[test]
    fn char_list_layout() {
        let r = QRes {
            cols: ["account_name", "char_name", "Level", "Exp", "Appr1", "MapLoc", "LocX"].map(String::from).to_vec(),
            rows: vec![["rtacc1", "RtChar1", "7", "4294967295", "-1", "default", "3"].map(|v| Some(v.as_bytes().to_vec())).to_vec()],
        };
        let mut out = [0xEEu8; 80];
        let n = pack_char_list(&r, &mut out);
        assert_eq!(n, 14 + 65);
        // the count WORD's high byte is overwritten by the first name
        assert_eq!(&out[..9], b"\x01RtChar1\0");
        assert_eq!(out[11], 1);
        assert_eq!(&out[12..14], &0xFFFFu16.to_le_bytes());
        assert_eq!(&out[24..26], &7u16.to_le_bytes());
        assert_eq!(&out[26..30], &u32::MAX.to_le_bytes());
        assert_eq!(&out[56..64], b"default\0");
    }

    fn q(cols: &[&str], vals: &[&str]) -> QRes {
        QRes { cols: cols.iter().map(|c| c.to_string()).collect(), rows: vec![vals.iter().map(|v| Some(v.as_bytes().to_vec())).collect()] }
    }

    #[test]
    fn char_list_cleared() {
        let r = q(&["char_name"], &["RtA"]);
        let mut out = [0xEEu8; 100];
        assert_eq!(pack_char_list(&r, &mut out), 79);
        assert!(out[5..79].iter().all(|&b| b == 0 || b == 1), "stale bytes left in the list");
        assert_eq!(out[79], 0xEE);
    }

    #[test]
    fn char_row_matches_hg_layout() {
        let r = q(
            &["account_name", "char_name", "CharID", "GuildName", "GuildRank", "CruJob", "CruID", "CruConstructPoint", "AdminLevel"],
            &["rtacc", "RtA", "300", "NONE", "-1", "2", "7", "9", "3"],
        );
        let mut out = vec![0u8; 800];
        let (id, admin, acc) = pack_char_row(&r, &mut out, |_| false);
        assert_eq!((id, admin, acc.as_slice()), (300, 3, &b"rtacc"[..]));
        // full DWORD CharID at 0 (no guild flag over its low byte)
        assert_eq!(&out[0..4], &300u32.to_le_bytes());
        assert_eq!(out[142], 0xFF);
        assert_eq!(&out[173..177], &7u32.to_le_bytes(), "crusade GUID at 173");
        assert_eq!(&out[177..181], &9u32.to_le_bytes(), "construction points at 177");
    }

    #[test]
    fn bank_item_id_is_64_bit() {
        let r = q(&["ItemName", "ItemID"], &["Rt", "4294967301"]);
        let mut out = vec![0u8; 100];
        pack_item(&mut out, 10, &r, 0, true);
        assert_eq!(&out[61..69], &4294967301u64.to_le_bytes());
    }

    #[test]
    fn save_sql_follows_hg_and_escapes() {
        let mut blk = vec![0u8; 760];
        let mut w = W(&mut blk);
        w.bytes(54, b"a'b");
        w.u16(142, (-1i16) as u16); // HG: WORD rank, then the fight zone byte over its high byte
        w.u8(143, 3);
        w.u32(173, 7);
        w.u32(177, 9);
        w.bytes(86, b"x\\");
        let sql = char_update_sql(Rd::new(&blk), 42);
        assert!(sql.contains("`GuildRank` = '-1',`FightNum` = '3'"), "{sql}");
        assert!(sql.contains("`CruID` = '7',`CruConstructPoint` = '9'"));
        assert!(sql.contains("`Nation` = 'a\\'b'") && sql.contains("`LockMapName` = 'x\\\\'"));
        assert!(sql.ends_with("WHERE `CharID` = '42' LIMIT 1;"));
    }

    #[test]
    fn item_insert_without_leading_comma() {
        assert_eq!(insert_sql("P ", &[]), None);
        assert_eq!(insert_sql("P ", &["(a)".into(), "(b)".into()]).as_deref(), Some("P (a),(b);"));
        let it = Item { name: b"Rt'x".to_vec(), uid: 5, ..Default::default() };
        assert!(item_values(&it, 1).starts_with("('1','Rt\\'x',"));
    }

    #[test]
    fn ban_text_has_date_and_reason() {
        assert_eq!(ban_text(b"2027-01-31 12:00:00", b"cheating\0\0"), b"until 2027-01-31: cheating".to_vec());
        assert_eq!(ban_text(b"2027-01-31 12:00:00", b""), b"until 2027-01-31: no reason given".to_vec());
        assert_eq!(ban_text(b"2027-01-31 12:00:00", &[b'x'; 300]).len(), 100);
    }

    #[test]
    fn wrong_passwords_lock_the_account() {
        let mut f = Fails::default();
        for k in 0..ACCOUNT_MAX_FAILS - 1 {
            assert!(!f.fail(1_000 + k * 1_000), "failure {k} must not lock");
        }
        assert_eq!(f.lock_minutes_left(10_000), 0);
        assert!(f.fail(10_000)); // the 5th
        assert_eq!(f.lock_minutes_left(10_000), 30);
        assert_eq!(f.lock_minutes_left(10_000 + 29 * 60_000 + 1), 1); // rounded up
        assert_eq!(f.lock_minutes_left(10_000 + 30 * 60_000), 0); // over
        // failures spread over more than the window never add up
        let mut g = Fails::default();
        for k in 0..10 {
            assert!(!g.fail(k * (FAIL_WINDOW_MS + 1)));
        }
    }

    #[test]
    fn client_settings_query_takes_hex_only() {
        let q = client_settings_sql(b"Neo", b"613d310a").unwrap();
        assert!(q.starts_with("REPLACE INTO `client_settings`") && q.contains("('Neo', '613d310a')"), "{q}");
        assert!(client_settings_sql(b"Neo", b"").is_some());
        assert!(client_settings_sql(b"Neo", b"'); DROP TABLE x; --").is_none());
        assert!(client_settings_sql(b"", b"00").is_none() && client_settings_sql(b"Neo", &[b'a'; 4001]).is_none());
    }

    #[test]
    fn saved_lock_minutes() {
        let n = chrono::NaiveDateTime::parse_from_str("2026-11-23 04:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(lock_minutes_in(b"2026-11-23 04:30:00", LOCK_REASON, n), 30);
        assert_eq!(lock_minutes_in(b"2026-11-23 04:00:01", LOCK_REASON, n), 1); // rounded up
        assert_eq!(lock_minutes_in(b"2026-11-23 04:00:00", LOCK_REASON, n), 0); // over
        assert_eq!(lock_minutes_in(b"2026-11-23 04:30:00", b"cheating", n), 0); // a real ban is not a lock
        assert_eq!(lock_minutes_in(b"0000-00-00 00:00:00", LOCK_REASON, n), 0);
    }

    #[test]
    fn valid_date_one_month_later() {
        let d = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(valid_date(d("2026-12-19 18:00:21")), "2027-01-19 18:00:21");
        assert_eq!(valid_date(d("2027-01-31 01:02:03")), "2027-02-28 01:02:03");
    }

    #[test]
    fn guild_count_and_services() {
        assert_eq!((members_after_remove(0), members_after_remove(1), members_after_remove(5)), (1, 1, 4));
        assert_eq!(lgn_svc(6), Some(("2 Maj", 1, 6)));
        assert_eq!(lgn_svc(99), None);
    }

    fn harness() -> Server {
        Server::new(Cfg {
            db: DbCfg { host: "none".into(), port: 0, user: String::new(), pwd: String::new() },
            db_name: "x".into(),
            permitted: vec![],
            listen_all: true,
            config_dir: ".".into(),
            db_maintenance: false,
        })
    }

    /// Game server `w` on the given socket slots; returns the receivers of those sockets.
    fn add_gs(srv: &mut Server, w: usize, socks: &[usize]) -> Vec<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>> {
        let mut socket_index = [-1i16; MAXSOCKETSPERSERVER];
        let mut rx = Vec::new();
        for (b, &i) in socks.iter().enumerate() {
            let (tx, r) = tokio::sync::mpsc::unbounded_channel();
            srv.gsock[i] = Some(GsSock { s: Sock { conn: i as u64, tx, peer: String::new() }, is_registered: true, gsid: w as i8, gs_socket_id: b as i8 });
            socket_index[b] = i as i16;
            rx.push(r);
        }
        srv.gs[w] = Some(GameServer {
            server_name: b"rt".to_vec(),
            world_name: b"WS1".to_vec(),
            map_names: vec![],
            server_ip: vec![],
            server_port: 0,
            internal_id: w as u16,
            connected_sockets: socks.len() as u8,
            socket_index,
            alive_response_time: 0,
            total_players_response: 0,
            is_initialized: true,
            is_being_closed: false,
            stock: new_stock(),
        });
        rx
    }

    #[test]
    fn confirmed_ip_goes_to_the_game_server() {
        let mut srv = harness();
        let mut g0 = add_gs(&mut srv, 0, &[3]);
        let mut g3 = add_gs(&mut srv, 3, &[0]);
        srv.send_game_server_ip(b"1.2.3.4", 0);
        let (_, body) = proto::decode(&g0[0].try_recv().expect("server 0 gets it"));
        assert_eq!(&body[..4], &MSGID_CONFIRMEDIP.to_le_bytes());
        assert!(g3[0].try_recv().is_err(), "socket 0 belongs to server 3");
    }

    #[test]
    fn stock_messages_after_a_disconnect_are_relayed() {
        let mut srv = harness();
        let _g0 = add_gs(&mut srv, 0, &[0]);
        let _g1 = add_gs(&mut srv, 1, &[1]);
        let mut body = new_stock();
        body.push(GSM_DISCONNECT);
        body.extend_from_slice(b"Nobody\0\0\0\0");
        let mut chat = vec![0u8; 18];
        chat[0] = GSM_CHATMSG;
        body.extend_from_slice(&chat);
        srv.server_stock_msg_handler(&body, 0);
        assert_eq!(srv.gs[1].as_ref().unwrap().stock.len(), 6 + 18);
        assert_eq!(srv.gs[0].as_ref().unwrap().stock.len(), 6);
    }

    #[test]
    fn game_server_loss_clears_its_party_members() {
        let mut srv = harness();
        let _g = add_gs(&mut srv, 2, &[7]);
        let mut req = vec![0u8; 50];
        W(&mut req).u32(0, MSGID_PARTYOPERATION);
        W(&mut req).u16(4, 1);
        W(&mut req).bytes(8, b"RtLead");
        srv.party_operation_result_handler(&req, 7);
        assert!(srv.party.is_member(1, b"RtLead"));
        srv.close_game_server_socket(7, false);
        assert!(!srv.party.is_member(1, b"RtLead"));
    }
}
