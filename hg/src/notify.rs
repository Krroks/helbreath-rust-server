//! `SendNotifyMsg` (HG.cpp ~9768): MSGID_NOTIFY + type + a per-type payload. Only the types layers 1-2 send
//! are here; the others are added with the layers that send them.
use crate::client::{MAXITEMS, MAXMAGICTYPE, MAXSKILLTYPE};
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::util::{cstr, W};

impl Game {
    /// `v` = sV1, sV2, ... (missing ones are 0), `s` = pString.
    pub fn notify(&mut self, to: usize, ty: i32, v: &[i32], s: Option<&[u8]>) {
        self.notify2(to, ty, v, s, None)
    }

    /// with pString2
    pub fn notify2(&mut self, to: usize, ty: i32, v: &[i32], s: Option<&[u8]>, s2: Option<&[u8]>) {
        if (ty == NOTIFY_HP || ty == NOTIFY_MP) && self.client(to).is_some_and(|c| c.party_id != 0) {
            // every 4th HP / 7th MP notify: NOTIFY_PARTY_COORDS to the local members on the map (itself included)
            let c = self.client_mut(to).unwrap();
            let (n, every) = if ty == NOTIFY_HP { (&mut c.party_hp_count, 4) } else { (&mut c.party_mp_count, 7) };
            *n += 1;
            if *n >= every {
                *n = 0;
                let (pid, m) = (c.party_id, c.map_index);
                for j in self.party_members(pid) {
                    if self.client(j).is_some_and(|o| o.map_index == m) {
                        // fix: the C++ sent the receiving member's max HP as this player's
                        self.party_coords(to, j, true);
                    }
                }
            }
        }
        let Some(c) = self.client(to) else { return };
        let a = |i: usize| v.get(i).copied().unwrap_or(0);
        let mut b = vec![0u8; 1000];
        let mut w = W(&mut b);
        w.u32(0, MSGID_NOTIFY);
        w.u16(4, ty as u16);
        let pad = |s: Option<&[u8]>, n: usize| {
            let mut x = cstr(s.unwrap_or(b"")).to_vec();
            x.resize(n, 0);
            x
        };
        let len = match ty {
            NOTIFY_QUESTCOUNTER => {
                w.u32(6, a(0) as u32);
                24
            }
            NOTIFY_LOCKEDMAP => {
                w.u16(6, a(0) as u16);
                w.bytes(8, &pad(s, 10));
                18
            }
            NOTIFY_SPECIALABILITYENABLED | NOTIFY_FORCEDISCONN | NOTIFY_OBSERVERMODE | NOTIFY_FORCERECALLTIME | NOTIFY_DOWNSKILLINDEXSET
            | NOTIFY_SKILLUSINGEND | NOTIFY_FISHCANCELED | NOTIFY_FISHSUCCESS | NOTIFY_FISHFAIL | NOTIFY_FISHCHANCE
            | NOTIFY_SLATE_STATUS | NOTIFY_ITEMUPGRADEFAIL | NOTIFY_QUESTCOMPLETED | NOTIFY_QUESTABORTED | NOTIFY_CANNOTCONSTRUCT
            | NOTIFY_METEORSTRIKECOMING | NOTIFY_METEORSTRIKEHIT | NOTIFY_HELPFAILED => {
                w.u16(6, a(0) as u16);
                8
            }
            NOTIFY_QUESTCONTENTS => {
                // sV1..sV3, sV4 (unused, 0), sV5..sV9 as shorts, then a 20-byte target name
                for k in 0..9 {
                    w.u16(6 + k * 2, a(k) as u16);
                }
                if s.is_some() {
                    w.bytes(24, &pad(s, 20));
                }
                44
            }
            NOTIFY_NPCTALK => {
                // sV1..sV9 as shorts, pString (reward name) and pString2 (target name), 20 bytes each
                for k in 0..9 {
                    w.u16(6 + k * 2, a(k) as u16);
                }
                if s.is_some() {
                    w.bytes(24, &pad(s, 20));
                }
                if s2.is_some() {
                    w.bytes(44, &pad(s2, 20));
                }
                64
            }
            NOTIFY_QUESTREWARD | NOTIFY_HELP => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                w.u32(10, a(2) as u32);
                w.bytes(14, &pad(s, 20));
                w.u32(34, a(3) as u32);
                38
            }
            NOTIFY_GIZONITEMUPGRADELEFT | NOTIFY_ITEMATTRIBUTECHANGE => {
                w.u16(6, a(0) as u16);
                w.u32(8, a(1) as u32);
                w.u32(12, a(2) as u32);
                w.u32(16, a(3) as u32);
                20
            }
            NOTIFY_ITEMPOSLIST => {
                for i in 0..MAXITEMS {
                    w.u16(6 + i * 4, c.item_pos[i].0 as u16);
                    w.u16(8 + i * 4, c.item_pos[i].1 as u16);
                }
                6 + MAXITEMS * 4
            }
            NOTIFY_APOCGATESTARTMSG | NOTIFY_APOCGATEENDMSG | NOTIFY_EVENTRESET => 6,
            NOTIFY_TOTALUSERS | NOTIFY_CANNOTRATING => {
                w.u16(6, a(0) as u16);
                8
            }
            NOTIFY_RATINGPLAYER => {
                w.u8(6, a(0) as u8);
                w.bytes(7, &pad(s, 10));
                w.u32(17, c.reputation as u32);
                22
            }
            NOTIFY_NOTICEMSG => {
                let t = cstr(s.unwrap_or(b"")).to_vec();
                w.bytes(6, &t);
                7 + t.len()
            }
            NOTIFY_PLAYERPROFILE => {
                // at most 100 bytes copied, the C++ length counts the whole string (zeros after the 100)
                let t = cstr(s.unwrap_or(b"")).to_vec();
                w.bytes(6, &t[..t.len().min(100)]);
                (7 + t.len()).min(1000)
            }
            NOTIFY_ADMINIFO => {
                for k in 0..8 {
                    w.u32(6 + k * 4, a(k + 1) as u32);
                }
                38
            }
            NOTIFY_GLOBALATTACKMODE | NOTIFY_EVENTTP | NOTIFY_EVENTILLUSION => {
                w.u8(6, a(0) as u8);
                7
            }
            NOTIFY_EVENTSPELL => {
                w.u8(6, a(0) as u8);
                w.u8(7, a(1) as u8);
                w.u8(8, c.is_gm() as u8);
                9
            }
            NOTIFY_EVENTARMOR | NOTIFY_EVENTSHIELD | NOTIFY_EVENTCHAT | NOTIFY_EVENTPARTY => {
                w.u8(6, a(0) as u8);
                w.u8(7, c.is_gm() as u8);
                8
            }
            NOTIFY_CTRWINNER | NOTIFY_EVENTSTART | NOTIFY_EVENTSTARTING | NOTIFY_EVENTSTARTING2 | NOTIFY_EVENTSTARTING3 | NOTIFY_RELICINALTAR => {
                w.u8(6, a(0) as u8);
                7
            }
            NOTIFY_CASUALTIES => {
                for k in 0..6 {
                    w.u16(6 + k * 2, a(k) as u16);
                }
                18
            }
            NOTIFY_RELICPOSITION => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                10
            }
            NOTIFY_RELICGRABBED => {
                w.bytes(6, &pad(s, 10));
                17
            }
            NOTIFY_APOCGATEOPEN | NOTIFY_APOCGATECLOSE => {
                w.u32(6, a(0) as u32);
                w.u32(10, a(1) as u32);
                w.bytes(14, &pad(s, 10));
                24
            }
            NOTIFY_MONSTEREVENT_POSITION => {
                w.u8(6, a(2) as u8);
                w.u16(7, a(0) as u16);
                w.u16(9, a(1) as u16);
                11
            }
            NOTIFY_HELDENIANVICTORY => {
                w.u16(6, a(0) as u16);
                8
            }
            NOTIFY_HELDENIANTELEPORT | NOTIFY_HELDENIANSTART | NOTIFY_HELDENIANEND | NOTIFY_NOTFLAGSPOT | NOTIFY_ADMINUSERLEVELLOW => 6,
            NOTIFY_HELDENIANCOUNT => {
                for k in 0..8 {
                    w.u16(6 + k * 2, a(k) as u16);
                }
                22
            }
            NOTIFY_ENEMYKILLS => {
                w.u32(6, a(0) as u32);
                10
            }
            NOTIFY_REWARDGOLD => {
                w.u32(6, c.reward_gold as u32);
                10
            }
            NOTIFY_IPACCOUNTINFO => {
                let t = cstr(s.unwrap_or(b"")).to_vec();
                w.bytes(6, &t);
                7 + t.len()
            }
            NOTIFY_TCLOC => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                w.bytes(10, &pad(s, 10));
                w.u16(20, a(3) as u16);
                w.u16(22, a(4) as u16);
                w.bytes(24, &pad(s2, 10));
                34
            }
            NOTIFY_MAPSTATUSNEXT | NOTIFY_MAPSTATUSLAST => {
                // sV1 raw bytes of pString (map name, send point, count, 6-byte entries)
                let d = s.unwrap_or(b"");
                let n = (a(0).max(0) as usize).min(d.len()).min(990);
                w.bytes(6, &d[..n]);
                6 + n
            }
            NOTIFY_GRANDMAGICRESULT => {
                for k in 0..3 {
                    w.u16(6 + k * 2, a(k) as u16);
                }
                w.bytes(12, &pad(s, 10));
                w.u16(22, a(3) as u16);
                // sV9 = the strike point count: pString2 holds the count and the HPs ((count + 1) shorts, raw)
                let n = (a(8).max(0) as usize + 1) * 2;
                if a(8) > 0 {
                    let d = s2.unwrap_or(b"");
                    w.bytes(24, &d[..n.min(d.len())]);
                }
                24 + n
            }
            NOTIFY_CRUSADE => {
                for k in 0..4 {
                    w.u32(6 + k * 4, a(k) as u32);
                }
                22
            }
            NOTIFY_CONSTRUCTIONPOINT | NOTIFY_SPECIALABILITYSTATUS | NOTIFY_DAMAGEMOVE => {
                for k in 0..3 {
                    w.u16(6 + k * 2, a(k) as u16);
                }
                12
            }
            NOTIFY_SAFEATTACKMODE => {
                w.u8(6, c.is_safe_attack_mode as u8);
                7
            }
            NOTIFY_SERVERSHUTDOWN | NOTIFY_WHETHERCHANGE | NOTIFY_TIMECHANGE => {
                w.u8(6, a(0) as u8);
                7
            }
            NOTIFY_QUERY_JOINGUILDREQPERMISSION | NOTIFY_QUERY_DISMISSGUILDREQPERMISSION | NOTIFY_CANNOTJOINMOREGUILDSMAN => {
                w.bytes(6, &pad(s, 10)); // the asking player's name (iFromH)
                16
            }
            COMMONTYPE_JOINGUILDAPPROVE => {
                w.bytes(6, &pad(s, 20)); // the master's guild (iFromH)
                w.u16(26, GUILDSTARTRANK as u16);
                28
            }
            COMMONTYPE_JOINGUILDREJECT | COMMONTYPE_DISMISSGUILDAPPROVE | COMMONTYPE_DISMISSGUILDREJECT => {
                w.bytes(6, &pad(s, 20));
                w.u16(26, GUILDSTARTRANK as u16);
                w.bytes(28, &pad(Some(&c.location), 10));
                38
            }
            NOTIFY_NOGUILDMASTERLEVEL | NOTIFY_CANNOTBANGUILDMAN => 6,
            NOTIFY_REQGUILDNAMEANSWER => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                w.bytes(10, &pad(s, 20));
                32
            }
            NOTIFY_PARTY => match a(0) {
                4 | 6 => {
                    for k in 0..3 {
                        w.u16(6 + k * 2, a(k) as u16);
                    }
                    w.bytes(12, &pad(s, 10));
                    22
                }
                5 => {
                    for k in 0..3 {
                        w.u16(6 + k * 2, a(k) as u16);
                    }
                    let n = a(2).max(0) as usize * 11;
                    let mut l = s.unwrap_or(b"").to_vec();
                    l.resize(n, 0);
                    w.bytes(12, &l);
                    12 + n
                }
                _ => {
                    for k in 0..4 {
                        w.u16(6 + k * 2, a(k) as u16);
                    }
                    14
                }
            },
            NOTIFY_QUERY_JOINPARTY => {
                let t = cstr(s.unwrap_or(b""));
                w.bytes(6, t);
                7 + t.len()
            }
            NOTIFY_PARTY_COORDS => {
                // sV1 x, sV2 y, sV4 hp, sV5 mp, sV6 max hp, sV7 max mp after the name
                w.bytes(6, &pad(s, 10));
                for (k, i) in [0, 1, 3, 4, 5, 6].into_iter().enumerate() {
                    w.u32(16 + k * 4, a(i) as u32);
                }
                40
            }
            NOTIFY_PLAYERSHUTUP => {
                w.u16(6, a(0) as u16);
                w.bytes(8, &pad(s, 10));
                19
            }
            NOTIFY_PLAYERONGAME | NOTIFY_FRIENDONGAME => {
                w.bytes(6, &pad(s, 10));
                if let Some(s2) = s2 {
                    let mut x = s2.to_vec();
                    x.resize(14, 0);
                    w.bytes(16, &x);
                }
                31
            }
            NOTIFY_WHISPERMODEON | NOTIFY_WHISPERMODEOFF | NOTIFY_PLAYERNOTONGAME => {
                w.bytes(6, &pad(s, 10));
                w.bytes(16, b"          "); // 10 spaces, as the C++ (seen on the wire)
                27
            }
            NOTIFY_MAGICEFFECTOFF | NOTIFY_MAGICEFFECTON => {
                w.u16(6, a(0) as u16);
                w.u32(8, a(1) as u32);
                w.u32(12, a(2) as u32);
                16
            }
            NOTIFY_SERVERCHANGE => {
                w.bytes(6, &pad(Some(&c.map_name), 10));
                w.bytes(16, &pad(Some(&self.cfg.log_server_addr_external), 15));
                w.u32(31, self.cfg.log_server_port as u32);
                35
            }
            NOTIFY_SLATE_EXP | NOTIFY_SLATE_MANA | NOTIFY_SLATE_INVINCIBLE | NOTIFY_SLATE_CREATEFAIL => 6,
            NOTIFY_SLATE_CREATESUCCESS => {
                w.u32(6, a(0) as u32);
                10
            }
            NOTIFY_EVENTFISHMODE | NOTIFY_ENERGYSPHEREGOALIN => {
                for k in 0..3 {
                    w.u16(6 + k * 2, a(k) as u16);
                }
                w.bytes(12, &pad(s, 20));
                32
            }
            NOTIFY_SKILL => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                10
            }
            NOTIFY_FIGHTZONERESERVE => {
                w.u32(6, a(0) as u32);
                10
            }
            NOTIFY_GUILDDISBANDED => {
                w.bytes(6, &pad(s, 20));
                w.bytes(26, &pad(Some(&c.location), 10));
                36
            }
            NOTIFY_CLIENTFLAGS => {
                w.u32(6, a(0) as u32);
                10
            }
            NOTIFY_PARTY_HUD => {
                // sV1 hp, sV2 max hp, sV3 mp, sV4 max mp, sV5 level, sV6 flags; pString the name, pString2 the map
                w.bytes(6, &pad(s, 10));
                w.bytes(16, &pad(s2, 10));
                for k in 0..4 {
                    w.u32(26 + k * 4, a(k) as u32);
                }
                w.u16(42, a(4) as u16);
                w.u8(44, a(5) as u8);
                45
            }
            NOTIFY_SERVEREVENTS => {
                w.u32(6, a(0) as u32);
                w.u32(10, a(1) as u32);
                14
            }
            NOTIFY_TELEPORT_REJECTED => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                10
            }
            NOTIFY_HP => {
                w.u32(6, c.hp as u32);
                w.u32(10, c.mp as u32);
                14
            }
            NOTIFY_MP => {
                w.u32(6, c.mp as u32);
                10
            }
            NOTIFY_SP => {
                w.u32(6, c.sp as u32);
                10
            }
            NOTIFY_TRAVELERLIMITEDLEVEL | NOTIFY_LIMITEDLEVEL => {
                w.u32(6, c.exp);
                10
            }
            NOTIFY_EXP => {
                w.u32(6, c.exp);
                w.u32(10, c.reputation as u32);
                14
            }
            NOTIFY_LEVELUP | NOTIFY_SETTING_SUCCESS => {
                for (k, v) in [c.level, c.get_str(), c.vit, c.get_dex(), c.get_int(), c.get_mag(), c.range].iter().enumerate() {
                    w.u32(6 + k * 4, *v as u32);
                }
                34
            }
            NOTIFY_PKPENALTY => {
                for (k, v) in [c.exp as i32, c.get_str(), c.vit, c.get_dex(), c.get_int(), c.get_mag(), c.range, c.pk_count].iter().enumerate() {
                    w.u32(6 + k * 4, *v as u32);
                }
                38
            }
            NOTIFY_PKCAPTURED => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                w.bytes(10, &pad(s, 10));
                w.u32(20, c.reward_gold as u32);
                w.u32(24, c.exp);
                28
            }
            NOTIFY_ENEMYKILLREWARD => {
                // a(0) = the victim
                let v = self.client(a(0) as usize);
                w.u32(6, c.exp);
                w.u32(10, c.enemy_kill_count as u32);
                w.bytes(14, &pad(v.map(|v| &v.char_name[..]), 10));
                w.bytes(24, &pad(v.map(|v| &v.guild_name[..]), 20));
                w.u16(44, v.map_or(0, |v| v.guild_rank) as u16);
                w.u16(46, c.war_contribution as u16);
                48
            }
            NOTIFY_KILLED => {
                w.bytes(6, &pad(s, 20));
                26
            }
            NOTIFY_HUNGER => {
                w.u8(6, a(0) as u8);
                7
            }
            NOTIFY_SUPERATTACKLEFT => {
                w.u16(6, c.super_attack_left as u16);
                8
            }
            NOTIFY_ITEMRELEASED | NOTIFY_ITEMLIFESPANEND | NOTIFY_ITEMDEPLETED_ERASEITEM => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                10
            }
            NOTIFY_DROPITEMFIN_ERASEITEM | NOTIFY_DROPITEMFIN_COUNTCHANGED => {
                w.u16(6, a(0) as u16);
                w.u32(8, a(1) as u32);
                12
            }
            NOTIFY_SETITEMCOUNT => {
                w.u16(6, a(0) as u16);
                w.u32(8, a(1) as u32);
                w.u8(12, a(2) as u8);
                13
            }
            NOTIFY_ITEMCOLORCHANGE | NOTIFY_SHOWMAP | NOTIFY_ENERGYSPHERECREATED => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                10
            }
            NOTIFY_ITEMSOLD | NOTIFY_ITEMREPAIRED => {
                w.u32(6, a(0) as u32);
                w.u32(10, a(1) as u32);
                14
            }
            NOTIFY_REPAIRITEMPRICE | NOTIFY_SELLITEMPRICE => {
                for i in 0..4 {
                    w.u32(6 + i * 4, a(i) as u32);
                }
                w.bytes(22, &pad(s, 20));
                42
            }
            NOTIFY_CANNOTREPAIRITEM | NOTIFY_CANNOTSELLITEM => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                w.bytes(10, &pad(s, 20));
                30
            }
            NOTIFY_CANNOTGIVEITEM | NOTIFY_GIVEITEMFIN_COUNTCHANGED | NOTIFY_GIVEITEMFIN_ERASEITEM => {
                w.u16(6, a(0) as u16);
                w.u32(8, a(1) as u32);
                w.bytes(12, &pad(s, 20));
                32
            }
            // NOTIFY_LGNPTS: the cash of the account (a DWORD)
            NOTIFY_LGNPTS => {
                w.u32(6, a(0) as u32);
                10
            }
            // fix: the C++ SendNotifyMsg has no case for NOTIFY_STATECHANGE_FAILED, so a refused respec was never answered
            // and the client kept its half-made stat changes; here the 6-byte message the client handles is sent
            NOTIFY_STATECHANGE_FAILED | NOTIFY_NOMOREAGRICULTURE | NOTIFY_AGRICULTURESKILLLIMIT | NOTIFY_AGRICULTURENOAREA | NOTIFY_NOMORECRUSADESTRUCTURE | NOTIFY_TOBERECALLED | NOTIFY_SETTING_FAILED | NOTIFY_CANNOTRECALL | NOTIFY_RESURRECTPLAYER | NOTIFY_CANNOTITEMTOBANK
            | NOTIFY_EXCHANGEITEMCOMPLETE | NOTIFY_CANCELEXCHANGEITEM | NOTIFY_CRAFTING_SUCCESS | NOTIFY_POTIONSUCCESS
            | NOTIFY_LOWPOTIONSKILL | NOTIFY_POTIONFAIL | NOTIFY_NOMATCHINGPOTION | NOTIFY_CRAFTING_FAIL
            | NOTIFY_NOMATCHINGCRAFTING | NOTIFY_NO_CRAFT_CONTRIB => 6,
            NOTIFY_BUILDITEMSUCCESS | NOTIFY_BUILDITEMFAIL => {
                w.u16(6, a(0) as u16);
                w.u16(8, a(1) as u16);
                10
            }
            NOTIFY_SPELL_SKILL => {
                w.bytes(6, &c.magic_mastery);
                w.bytes(6 + MAXMAGICTYPE, &c.skill_mastery);
                6 + MAXMAGICTYPE + MAXSKILLTYPE
            }
            _ => {
                log(&format!("(!) notify type {ty:04x} not ported yet"));
                return;
            }
        };
        b.truncate(len);
        self.send(to, &b);
    }
}
