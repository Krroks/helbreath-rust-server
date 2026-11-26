//! Constants from Shared/NetMessages.h, version.h, common.h, lgnSvcs.h and Login/defs.hpp.
//! `VERSIONPLUSMODIFIER` = LOWER_VERSION % 5 = 0, so the modifiers are plain numbers.
#![allow(dead_code)]

pub const UPPER_VERSION: u16 = 1;
pub const LOWER_VERSION: u16 = 0;
const M2: u32 = 0x03;
const M3: u32 = 0x09;
const M4: u32 = 0x07;
const M5: u32 = 0x05;

pub const MSGTYPE_CONFIRM: u16 = 0x0F14;
pub const MSGTYPE_REJECT: u16 = 0x0F15;

// client <-> login
pub const MSGID_REQUEST_LOGIN: u32 = 0x0AC94201;
pub const MSGID_RESPONSE_LOG: u32 = 0x0AC94203;
pub const MSGID_REQUEST_CREATENEWACCOUNT: u32 = 0x0FC94202 + M4;
pub const MSGID_REQUEST_CREATENEWCHARACTER: u32 = 0x0FC94204 + M4;
pub const MSGID_REQUEST_ENTERGAME: u32 = 0x0FC94205 + M4;
pub const MSGID_RESPONSE_ENTERGAME: u32 = 0x0FC94206 + M4;
pub const MSGID_REQUEST_DELETECHARACTER: u32 = 0x0FC94207 + M4;
pub const MSGID_REQUEST_CHANGEPASSWORD: u32 = 0x0FC94210 + M4;
pub const MSGID_RESPONSE_CHANGEPASSWORD: u32 = 0x0FC94211 + M4;

// game server <-> login
pub const MSGID_REQUEST_CREATENEWGUILD: u32 = 0x0FC94208 + M4;
pub const MSGID_RESPONSE_CREATENEWGUILD: u32 = 0x0FC94209 + M4;
pub const MSGID_REQUEST_DISBANDGUILD: u32 = 0x0FC9420A + M4;
pub const MSGID_RESPONSE_DISBANDGUILD: u32 = 0x0FC9420B + M4;
pub const MSGID_REQUEST_UPDATEGUILDINFO_NEWGUILDSMAN: u32 = 0x0FC9420C + M4;
pub const MSGID_REQUEST_UPDATEGUILDINFO_DELGUILDSMAN: u32 = 0x0FC9420D + M4;
pub const MSGID_REQUEST_LGNPTS: u32 = 0x0FC94221 + M4;
pub const MSGID_RESPONSE_LGNPTS: u32 = 0x0FC94222 + M4;
pub const MSGID_REQUEST_LGNSVC: u32 = 0x0FC94223 + M4;
pub const MSGID_RESPONSE_LGNSVC: u32 = 0x0FC94224 + M4;
pub const MSGID_SUBCASH: u32 = 0x0FC94225 + M4;
// Rust only (no C++ counterpart): what the in-process bots remember of the players (hg `bots.rs`), kept in the `bot_memory` table
pub const MSGID_REQUEST_BOTMEM: u32 = 0x0FC94230 + M4;
pub const MSGID_BOTMEM_SAVE: u32 = 0x0FC94231 + M4;
pub const MSGID_RESPONSE_BOTMEM: u32 = 0x0FC94232 + M4;
// and what they said, did and concluded (hg `bots.rs`), kept in the `bot_history` table, one row each
pub const MSGID_REQUEST_BOTHIST: u32 = 0x0FC94233 + M4;
pub const MSGID_BOTHIST_SAVE: u32 = 0x0FC94234 + M4;
pub const MSGID_RESPONSE_BOTHIST: u32 = 0x0FC94235 + M4;
/// Rust only: the client settings of a character (table client_settings), HG -> Login and back
pub const MSGID_CLIENTSETTINGS_SAVE: u32 = 0x0FC94236 + M4;
pub const MSGID_RESPONSE_CLIENTSETTINGS: u32 = 0x0FC94237 + M4;

pub const MSGID_ITEMCONFIGURATIONCONTENTS: u32 = 0x0FA314D9 + M4;
pub const MSGID_NPCCONFIGURATIONCONTENTS: u32 = 0x0FA314DA + M4;
pub const MSGID_MAGICCONFIGURATIONCONTENTS: u32 = 0x0FA314DB + M4;
pub const MSGID_SKILLCONFIGURATIONCONTENTS: u32 = 0x0FA314DC + M4;
pub const MSGID_POTIONCONFIGURATIONCONTENTS: u32 = 0x0FA314DE + M4;
pub const MSGID_QUESTCONFIGURATIONCONTENTS: u32 = 0x0FA40001 + M4;
pub const MSGID_BUILDITEMCONFIGURATIONCONTENTS: u32 = 0x0FA40002 + M4;
pub const MSGID_DUPITEMIDFILECONTENTS: u32 = 0x0FA40003 + M4;
pub const MSGID_NOTICEMENTFILECONTENTS: u32 = 0x0FA40004 + M4;
pub const MSGID_CRAFTINGCONFIGURATIONCONTENTS: u32 = 0x0FA40005 + M3;
pub const MSGID_TELEPORTLISTCONTENTS: u32 = 0x0FA40008 + M4;
pub const MSGID_UPDATECONFIGFILES: u32 = 0x0FA40009 + M4;

pub const MSGID_REQUEST_PLAYERDATA: u32 = 0x0C152210 + M3;
pub const MSGID_RESPONSE_PLAYERDATA: u32 = 0x0C152211 + M3;
pub const MSGID_RESPONSE_SAVEPLAYERDATA_REPLY: u32 = 0x0C152212 + M3;
pub const MSGID_REQUEST_SAVEPLAYERDATALOGOUT: u32 = 0x0DF3074F + M3;
pub const MSGID_REQUEST_NOSAVELOGOUT: u32 = 0x0DF30750 + M3;
pub const MSGID_REQUEST_SAVEPLAYERDATA: u32 = 0x0DF3076F + M3;
pub const MSGID_REQUEST_SAVEPLAYERDATA_REPLY: u32 = 0x0DF30770 + M3;

pub const MSGID_REQUEST_REGISTERGAMESERVER: u32 = 0x0512A3F4 + M5;
pub const MSGID_RESPONSE_REGISTERGAMESERVER: u32 = 0x0512A3F5 + M5;
pub const MSGID_REQUEST_REGISTERGAMESERVERSOCKET: u32 = 0x0512A3F8 + M2;
pub const MSGID_GAMESERVERALIVE: u32 = 0x12A01002 + M2;
pub const MSGID_TOTALGAMESERVERCLIENTS: u32 = 0x12A01005 + M2;
pub const MSGID_ENTERGAMECONFIRM: u32 = 0x12A01006 + M2;
pub const MSGID_CONFIRMEDIP: u32 = 0x12A01008 + M2;
pub const MSGID_GAMESERVERSHUTDOWNED: u32 = 0x14000000 + M2;
pub const MSGID_REQUEST_FORCEDISCONECTACCOUNT: u32 = 0x15000000 + M2;
pub const MSGID_SENDSERVERSHUTDOWNMSG: u32 = 0x20000000 + M2;
pub const MSGID_ITEMLOG: u32 = 0x210A914D + M2;
pub const MSGID_GAMEMASTERLOG: u32 = 0x210A914E + M2;
pub const MSGID_GAMEITEMLOG: u32 = 0x210A914F + M2;
pub const MSGID_GAMECRUSADELOG: u32 = 0x210A9150 + M2;
pub const MSGID_PLAYERLOG: u32 = 0x210A9151 + M2;
pub const MSGID_REQUEST_SETACCOUNTWAITSTATUS: u32 = 0x25000199 + M2;
pub const MSGID_SERVERSTOCKMSG: u32 = 0x3AE90013 + M2;
pub const MSGID_PARTYOPERATION: u32 = 0x3C00123A + M2;

pub const LOGRESMSGTYPE_CONFIRM: u16 = 0x0F14;
pub const LOGRESMSGTYPE_REJECT: u16 = 0x0F15;
pub const LOGRESMSGTYPE_PASSWORDMISMATCH: u16 = 0x0F16;
pub const LOGRESMSGTYPE_NOTEXISTINGACCOUNT: u16 = 0x0F17;
/// v8.00 "Account is locked, check back in N minutes!": too many wrong passwords (the original header already names it; payload: u16 minutes)
pub const LOGRESMSGTYPE_ACCOUNTLOCKED: u16 = 0x0F31;
pub const LOGRESMSGTYPE_NEWACCOUNTCREATED: u16 = 0x0F18;
pub const LOGRESMSGTYPE_ALREADYEXISTINGACCOUNT: u16 = 0x0F1A;
pub const LOGRESMSGTYPE_NOTEXISTINGCHARACTER: u16 = 0x0F1B;
pub const LOGRESMSGTYPE_NEWCHARACTERCREATED: u16 = 0x0F1C;
pub const LOGRESMSGTYPE_NEWCHARACTERFAILED: u16 = 0x0F1D;
pub const LOGRESMSGTYPE_ALREADYEXISTINGCHARACTER: u16 = 0x0F1E;
pub const LOGRESMSGTYPE_CHARACTERDELETED: u16 = 0x0F1F;
pub const LOGRESMSGTYPE_PASSWORDCHANGESUCCESS: u32 = 0x0A00;
pub const LOGRESMSGTYPE_PASSWORDCHANGEFAIL: u32 = 0x0A01;

pub const ENTERGAMEMSGTYPE_NEW: u16 = 0x0F1C + M2 as u16;
pub const ENTERGAMEMSGTYPE_NOENTER_FORCEDISCONN: u16 = 0x0F1D + M2 as u16;
pub const ENTERGAMEMSGTYPE_CHANGINGSERVER: u16 = 0x0F1E + M2 as u16;
pub const ENTERGAMERESTYPE_PLAYING: u16 = 0x0F20 + M2 as u16;
pub const ENTERGAMERESTYPE_REJECT: u16 = 0x0F21 + M2 as u16;
pub const ENTERGAMERESTYPE_CONFIRM: u16 = 0x0F22 + M2 as u16;
pub const ENTERGAMERESTYPE_FORCEDISCONN: u16 = 0x0F23 + M2 as u16;
pub const REJECTTYPE_GAMESERVEROFFLINE: u8 = 0x05;
pub const REJECTTYPE_MAXSERVERUSERLIMITREACHED: u8 = 0x06;
/// no original: the account is banned (BlockDate in the future); the client shows the reason it got with the character list
pub const REJECTTYPE_ACCOUNTBANNED: u8 = 0x08;

pub const GSM_REQUEST_FINDCHARACTER: u8 = 0x01 + M2 as u8;
pub const GSM_RESPONSE_FINDCHARACTER: u8 = 0x02 + M2 as u8;
pub const GSM_GRANDMAGICRESULT: u8 = 0x03 + M2 as u8;
pub const GSM_GRANDMAGICLAUNCH: u8 = 0x04 + M2 as u8;
pub const GSM_COLLECTEDMANA: u8 = 0x05 + M2 as u8;
pub const GSM_BEGINCRUSADE: u8 = 0x06 + M2 as u8;
pub const GSM_ENDCRUSADE: u8 = 0x07 + M2 as u8;
pub const GSM_MIDDLEMAPSTATUS: u8 = 0x08 + M2 as u8;
pub const GSM_SETGUILDTELEPORTLOC: u8 = 0x09 + M2 as u8;
pub const GSM_CONSTRUCTIONPOINT: u8 = 0x0A + M2 as u8;
pub const GSM_SETGUILDCONSTRUCTLOC: u8 = 0x0B + M2 as u8;
pub const GSM_CHATMSG: u8 = 0x0C + M2 as u8;
pub const GSM_WHISPERMSG: u8 = 0x0D + M2 as u8;
pub const GSM_DISCONNECT: u8 = 0x0E + M2 as u8;
pub const GSM_REQUEST_SUMMONPLAYER: u8 = 0x0F + M2 as u8;
pub const GSM_REQUEST_SHUTUPPLAYER: u8 = 0x10 + M2 as u8;
pub const GSM_RESPONSE_SHUTUPPLAYER: u8 = 0x11 + M2 as u8;
pub const GSM_REQUEST_SETFORCERECALLTIME: u8 = 0x12 + M2 as u8;
pub const GSM_REQUEST_SUMMONGUILD: u8 = 0x13 + M2 as u8;
pub const GSM_BEGINAPOCALYPSE: u8 = 0x14 + M2 as u8;
pub const GSM_ENDAPOCALYPSE: u8 = 0x15 + M2 as u8;
pub const GSM_STARTHELDENIAN: u8 = 0x16 + M2 as u8;
pub const GSM_ENDHELDENIAN: u8 = 0x17 + M2 as u8;
pub const GSM_REQUEST_FINDFRIEND: u8 = 0x18 + M2 as u8;
pub const GSM_RESPONSE_FINDFRIEND: u8 = 0x19 + M2 as u8;

pub const MAXDELETELEVEL: i32 = 100;
pub const MAXPARTYMEMBERS: i32 = 25;

// defs.hpp
pub const MAXGAMESERVERS: usize = 20;
pub const MAXSOCKETSPERSERVER: usize = 5;
pub const MAXGAMESERVERSOCKETS: usize = MAXGAMESERVERS * MAXSOCKETSPERSERVER;
pub const MAXCLIENTS: usize = 500;
pub const MAXSKILLS: u32 = 10;
/// Rust only: skill 10 (pretend corpse), past the C++ Login's MAXSKILLS rows; carried in the player data at
/// 598+10 / 653+40, bytes the C++ HG and Login never read or write.
pub const SKILL_PRETENDCORPSE: u32 = MAXSKILLS;
pub const MAXITEMS: u32 = 50;
pub const MAXBANKITEMS: u32 = 120;
pub const MSGQUENESIZE: usize = 5000;
pub const MAXALLOWEDQUERYERROR: u32 = 10;
pub const MAINTIMERSIZE: u64 = 300;
pub const MAX_MYSQL_RESTART_ATTEMPT: u8 = 5;
pub const MYSQL_AUTOFIX_TIMERSIZE: u32 = 30000;
pub const CHECKACCSTATUS_TIMERINTERVAL: u32 = 3 * 60000;
pub const MAXWAITTIMEFORPLAYERENTERGAME: u32 = 50000;
pub const MAX_GSALIVE_WAITINTERVAL: u32 = 40000;
pub const INTERVALTOSEND_TOTALPLAYERS: u32 = 5000;
pub const MAX_FORCEDISCONN_WAIT_TIME: u32 = 12000;
pub const SHUTDOWN_INTERVAL_MSG: u32 = 30000;
pub const MAXGAMESERVERSTOCKMSGSIZE: usize = 10000;
// net/XSocket.h: receive/send buffer sizes of client and game server sockets
pub const MSGBUFFERSIZE: usize = 50000;
pub const MSGCLIENTBUFFERSIZE: usize = 30000;

// dup item codes
pub const NOITEM: u8 = 0x00;
pub const ONEINBAG: u8 = 0x01;
pub const ONEINBANK: u8 = 0x02;
pub const ONEINBAG_ONEINBANK: u8 = 0x03;
pub const DUPITEMINBAG: u8 = 0x04;
pub const DUPITEMINBANK: u8 = 0x05;

// CheckAccountLogin results
pub const ACCOUNTDONTEXISTS: u8 = 0x01;
pub const PASSWORDISWRONG: u8 = 0x02;
pub const LOGINOK: u8 = 0x03;

// log files
pub const XSOCKET_LOGFILE: &str = "Logs/XSocket.log";
pub const GM_LOGFILE: &str = "Logs/GM/";
pub const ITEM_LOGFILE: &str = "Logs/Item/";
pub const CRUSADE_LOGFILE: &str = "Logs/Crusade/";
pub const PLAYER_LOGFILE: &str = "Logs/Player/";
pub const HACK_LOGFILE: &str = "Logs/Hack.log";
pub const ERROR_LOGFILE: &str = "Logs/Error.log";
pub const MYSQL_ERROR_LOGFILE: &str = "Logs/MysqlError.log";
pub const GSUNKNOWNMSG_LOGFILE: &str = "Logs/Unknown socket msgs from gameserver.log";
pub const CLIENTUNKNOWNMSG_LOGFILE: &str = "Logs/Unknown socket msgs from client.log";
pub const QUERYERROR_LOGFILE: &str = "Logs/Query errors.log";
pub const CLIENTUNKNOWNMSG_LOGFILE2: &str = "Logs/Unknown_MsgIds_From_client.log";
pub const GSUNKNOWNMSG_LOGFILE2: &str = "Logs/Unknown_MsgIds_From_Gameserver.log";

/// lgnSvcs.h `lgnPtsSvcs`: (name, price, cmd); the last entry is the terminator the lookups stop on.
pub const LGN_PTS_SVCS: &[(&str, u32, u16)] = &[
    ("LegionHat MP7", 25, 0),
    ("LegionHat HP7", 25, 1),
    ("LegionHelm HP7", 25, 2),
    ("LegionHelm MP7", 25, 3),
    ("TC(Are)", 10, 4),
    ("TC(Elv)", 10, 5),
    ("2 Maj", 1, 6),
    ("20 Maj", 10, 7),
    ("10k Gold", 1, 8),
    ("100k Gold", 10, 9),
    ("10 Rep", 1, 10),
    ("100 Rep", 10, 11),
    ("", 0, 0),
];

/// Service for `cmd`; None for an unknown one (the C++ lookup stopped on the price-0 terminator).
pub fn lgn_svc(cmd: u16) -> Option<(&'static str, u32, u16)> {
    LGN_PTS_SVCS.iter().find(|s| s.1 != 0 && s.2 == cmd).copied()
}
