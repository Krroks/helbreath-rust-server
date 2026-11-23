//! The chat commands of ChatMsgHandler ('/'), in the C++ order (prefix tests as its memcmp lengths), and the
//! AdminOrder_* / PlayerOrder_* handlers: /who, /send, /goto, /clearmap, /afk, /checkrep, /setpf, /pf, /shutup,
//! /rep+ /rep-, /hold /tgt /free, /closeconn, the /event* switches of fight zones, /attack, /createfish, /teleport,
//! /summonplayer, /unsummonall, /unsummonboss, /checkip, /polymorph, /noaggro, /invincible, /setinvi, /sethp /setmp
//! /setmag, /gns, /setattackmode, /setforcerecalltime, /summon, /disconnectall, /createitem,
//! /shutdownthisserverrightnow, /setobservermode, and (last impl block) the vanilla 3.82 ones: /kill, /revive,
//! /summondemon, /summondeath, /unsummondemon, /clearnpc, /setzerk, /setfreeze, /safe, /peace, /unsafe, /war, /weather, /time.
//! (/fi, /to, /ban, the party ones and the event starts live with
//! their layers.)
use crate::client::*;
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::magic::Spell;
use crate::map::{OWNERTYPE_NPC, OWNERTYPE_PLAYER};
use crate::npc::{BEHAVIOR_ATTACK, BEHAVIOR_MOVE, MOVETYPE_FOLLOW, MOVETYPE_RANDOM};
use crate::util::{atoi, cstr, l1, StrTok};

pub const STATUS_AFK: i32 = 1 << 18;

/// the maps /teleport accepts
const TELEPORT_MAPS: &[&[u8]] = &[
    b"arefarm", b"aresden", b"aresdend1", b"cath_1", b"wrhus_1", b"bsmith_1", b"gshop_1", b"wrhus_1f", b"bsmith_1f", b"gshop_1f", b"gldhall_1", b"cityhall_1", b"CmdHall_1", b"wzdtwr_1", b"huntzone2", b"huntzone4",
    b"areuni", b"resurr1", b"arejail", b"arewrhus", b"arebrk11", b"arebrk12", b"arebrk21", b"arebrk22", b"elvfarm", b"elvine", b"elvined1", b"cath_2", b"wrhus_2", b"gshop_2", b"bsmith_2", b"wrhus_2f",
    b"gshop_2f", b"bsmith_2f", b"gldhall_2", b"cityhall_2", b"CmdHall_2", b"wzdtwr_2", b"huntzone1", b"huntzone3", b"elvuni", b"resurr2", b"elvjail", b"elvwrhus", b"elvbrk11", b"elvbrk12", b"elvbrk21",
    b"elvbrk22", b"istfarm", b"default2", b"cath_3", b"wrhus_3", b"gshop_3", b"bsmith_3", b"wrhus_3f", b"gshop_3f", b"bsmith_3f", b"gldhall_3", b"cityhall_3", b"CmdHall_3", b"wzdtwr_3", b"huntzone5",
    b"huntzone6", b"garden", b"resurr3", b"istjail", b"istwrhus", b"istbrk1", b"istbrk2", b"dglv2", b"dglv3", b"dglv4", b"bisle", b"default", b"fightzone1", b"fightzone2", b"fightzone3", b"fightzone4",
    b"fightzone5", b"fightzone6", b"fightzone7", b"fightzone8", b"fightzone9", b"middleland", b"middled1n", b"middled1x", b"toh1", b"toh2", b"toh3", b"2ndmiddle", b"icebound", b"barracks1", b"barracks2",
    b"bsmith", b"astoria", b"test", b"inferniaA", b"inferniaB", b"maze", b"procella", b"abaddon", b"druncncity", b"GodH", b"BtField", b"HRampart",
];

/// the maps /send accepts
const SEND_MAPS: &[&[u8]] = &[
    b"2ndmiddle", b"abaddon", b"arebrk11", b"arebrk12", b"arebrk21", b"arebrk22", b"arefarm", b"arejail", b"aremidl", b"aremidr", b"aresden", b"aresdend1", b"areuni", b"arewrhus", b"bisle", b"bsmith_1",
    b"bsmith_1f", b"bsmith_2", b"bsmith_2f", b"BtField", b"cath_1", b"cath_2", b"cityhall_1", b"cityhall_2", b"CmdHall_1", b"CmdHall_2", b"default", b"dglv2", b"dglv3", b"dglv4", b"druncncity", b"elvbrk11",
    b"elvbrk12", b"elvbrk21", b"elvbrk22", b"elvfarm", b"elvine", b"elvined1", b"elvjail", b"elvmidl", b"elvmidr", b"elvuni", b"elvwrhus", b"fightzone1", b"fightzone2", b"fightzone3", b"fightzone4",
    b"fightzone5", b"fightzone6", b"fightzone7", b"fightzone8", b"fightzone9", b"fightzone10", b"gldhall_1", b"gldhall_2", b"GodH", b"gshop_1", b"gshop_1f", b"gshop_2", b"gshop_2f", b"HRampart", b"huntzone1",
    b"huntzone2", b"huntzone3", b"huntzone4", b"icebound", b"inferniaA", b"inferniaB", b"maze", b"middled1n", b"middled1x", b"middleland", b"penalty", b"procella", b"resurr1", b"resurr2", b"toh1", b"toh2",
    b"toh3", b"wrhus_1", b"wrhus_1f", b"wrhus_2", b"wrhus_2f", b"wzdtwr_1", b"wzdtwr_2", b"barracks1", b"barracks2", b"bsmith",
];

/// AdminOrder_Polymorph: (name, compared length, type); the C++ compared 6 bytes of "GHC" (exact here)
const POLYMORPH: &[(&[u8], usize, i16)] = &[
    (b"Slime", 5, 10), (b"Skeleton", 8, 11), (b"Stone-Golem", 11, 12), (b"Cyclops", 7, 13), (b"Orc", 3, 14), (b"ShopKeeper", 10, 15), (b"Giant-Ant", 9, 16), (b"Scorpion", 8, 17), (b"Zombie", 6, 18), (b"Gandlf", 6, 19),
    (b"Howard", 6, 20), (b"Gaurd", 5, 21), (b"Amphis", 6, 22), (b"Clay-Golem", 10, 23), (b"Tom", 3, 24), (b"William", 7, 25), (b"Kennedy", 7, 26), (b"Hellbound", 9, 27), (b"Troll", 5, 28), (b"Orge", 4, 29),
    (b"Liche", 5, 30), (b"Demon", 5, 31), (b"Unicorn", 7, 32), (b"WereWolf", 8, 33), (b"Dummy", 5, 34), (b"Energy-Sphere", 13, 35), (b"AGT", 3, 36), (b"CGT", 3, 37), (b"MS", 2, 38), (b"DT", 2, 39),
    (b"ESG", 3, 40), (b"GMG", 3, 41), (b"ManaStone", 9, 42), (b"LWB", 3, 43), (b"GHK", 3, 44), (b"GHC", 4, 45), (b"TK", 2, 46), (b"BG", 2, 47), (b"Stalker", 7, 48), (b"Hellclaw", 8, 49),
    (b"Tigerworm", 8, 50), (b"CP", 2, 51), (b"Gagoyle", 7, 52), (b"Beholder", 8, 53), (b"Dark-Elf", 8, 54), (b"Rabbit", 6, 55), (b"Cat", 3, 56), (b"Giant-Frog", 10, 57), (b"Mountain-Giant", 14, 58), (b"Ettin", 5, 59),
    (b"Cannibal-Plant", 13, 60), (b"Rudolph", 7, 61), (b"DireBoar", 8, 62), (b"Frost", 5, 63), (b"Crops", 5, 64), (b"Ice-Golem", 9, 65), (b"Wyvern", 6, 66), (b"McGaffin", 8, 67), (b"Perry", 5, 68), (b"Devlin", 6, 69),
    (b"Barlog", 6, 70), (b"Centaurus", 9, 71), (b"Claw-Turtle", 11, 72), (b"Fire-Wyvern", 11, 73), (b"Giant-Crayfish", 14, 74), (b"Giant-Lizard", 12, 75), (b"Giant-Plant", 11, 76), (b"MasterMage-Orc", 14, 77), (b"Minotaurs", 9, 78), (b"Nizie", 5, 79),
    (b"Tentocle", 8, 80), (b"Abaddon", 7, 81), (b"Sor", 3, 82), (b"ATK", 3, 83), (b"Elf", 3, 84), (b"DSK", 3, 85), (b"HBT", 3, 86), (b"CT", 2, 87), (b"Bar", 3, 88), (b"AGC", 3, 89),
    (b"Gail", 4, 90), (b"gate", 4, 91),
];

/// the 1st..nth tokens after the command (seps "= \t\n")
fn args(cmd: &[u8]) -> Vec<Vec<u8>> {
    let mut t = StrTok::new(cmd, b"= \t\n");
    let _ = t.get();
    std::iter::from_fn(|| t.get()).collect()
}

/// a character name token: its first 10 bytes, zero padded (memcmp(..., 10))
fn name10(tok: &[u8]) -> [u8; 10] {
    Game::pad10(&tok[..tok.len().min(10)])
}

impl Game {
    /// `_iGetMagicNumber`: the Magic.cfg entry of a spell name (30 bytes)
    fn magic_number(&self, name: &[u8]) -> Option<i32> {
        let mut n30 = cstr(name).to_vec();
        n30.resize(30, 0);
        self.data.magic.iter().position(|f| {
            f.as_ref().and_then(|f| f.get(1)).is_some_and(|t| {
                let mut t = cstr(t).to_vec();
                t.resize(30, 0);
                t == n30
            })
        }).map(|i| i as i32)
    }

    fn player_named(&self, name: &[u8; 10]) -> Option<usize> {
        self.handles().into_iter().find(|&i| Game::pad10(&self.client(i).unwrap().char_name) == *name)
    }

    fn gm_log(&mut self, txt: &str) {
        self.send_ls_plain(MSGID_GAMEMASTERLOG, txt.as_bytes());
    }

    fn admin_low(&mut self, h: usize) {
        self.notify(h, NOTIFY_ADMINUSERLEVELLOW, &[], None);
    }

    fn level(&self, h: usize) -> i32 {
        self.client(h).map_or(0, |c| c.admin_user_level)
    }

    fn cname(&self, h: usize) -> String {
        self.client(h).map_or(String::new(), |c| l1(&c.char_name))
    }

    /// ChatMsgHandler's '/' branch: true when a command took the line
    pub fn chat_command(&mut self, h: usize, cmd: &[u8]) {
        let p = |s: &[u8]| cmd.starts_with(s);
        let lvl = self.level(h);
        if p(b"/who") {
            let n = self.total_game_server_clients + self.online_cnt_add;
            self.notify(h, NOTIFY_TOTALUSERS, &[n], None);
        } else if cmd == b"/mcount" {
            // Apocalypse_MonsterCount: NOTIFY_MONSTERCOUNT has no case in SendNotifyMsg, nothing is sent
        } else if p(b"/send") || p(b"/push ") {
            self.gm_push_player(h, cmd);
        } else if p(b"/goto ") {
            self.gm_goto(h, cmd);
        } else if p(b"/clearmap") || p(b"/cleanmap") {
            self.gm_clean_map(h, cmd);
        } else if cmd == b"/afk" {
            let c = self.client_mut(h).unwrap();
            let on = c.status & STATUS_AFK == 0;
            c.set_status_flag(STATUS_AFK, on);
            self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
            let t: &[u8] = if on { b"You are now AFK." } else { b"You are no longer AFK." };
            self.notify(h, NOTIFY_NOTICEMSG, &[], Some(t));
        } else if p(b"/checkrep") {
            self.gm_check_rep(h, cmd);
        } else if p(b"/fi ") {
            self.check_and_notify_player_connection(h, cmd);
        } else if p(b"/to") {
            self.toggle_whisper(h, cmd);
        } else if p(b"/setpf ") {
            // SetPlayerProfile: spaces become '_', at most 255 bytes
            let t: Vec<u8> = cmd[7..].iter().take(255).map(|&b| if b == b' ' { b'_' } else { b }).collect();
            self.client_mut(h).unwrap().profile = t;
        } else if p(b"/pf ") {
            self.gm_get_profile(h, cmd);
        } else if p(b"/shutup ") {
            self.gm_shutup(h, cmd);
        } else if p(b"/rep+ ") {
            self.set_player_reputation(h, cmd, 1);
        } else if p(b"/rep- ") {
            self.set_player_reputation(h, cmd, 0);
        } else if p(b"/hold") {
            self.summon_mob_action(h, 1, cmd);
        } else if p(b"/tgt ") || p(b"/target ") {
            self.summon_mob_action(h, 2, cmd);
        } else if p(b"/free") {
            self.summon_mob_action(h, 0, cmd);
        } else if p(b"/closeconn ") {
            self.gm_close_conn(h, cmd);
        } else if p(b"/eventspell ") {
            self.gm_event_spell(h, cmd);
        } else if p(b"/eventarmor") {
            self.gm_event_switch(h, cmd, 0);
        } else if p(b"/eventshield") {
            self.gm_event_switch(h, cmd, 1);
        } else if p(b"/eventchat") {
            self.gm_event_switch(h, cmd, 2);
        } else if p(b"/eventparty") {
            self.gm_event_switch(h, cmd, 3);
        } else if p(b"/doubleexp") || p(b"/doubleek") {
            self.gm_server_event(h, cmd);
        } else if p(b"/eventreset") {
            self.gm_event_reset(h);
        } else if p(b"/eventtp") {
            self.gm_event_tp(h, cmd);
        } else if p(b"/eventillusion") {
            self.gm_event_switch(h, cmd, 4);
        } else if p(b"/ban") {
            self.ban_guildsman(h, cmd);
        } else if p(b"/reservefightzone") {
            self.gm_reserve_fightzone(h, cmd);
        } else if p(b"/dissmiss") {
            // UserCommand_DissmissGuild: empty in the C++
        } else if p(b"/attack ") || p(b"/callguard ") {
            self.gm_call_guard(h, cmd);
        } else if p(b"/createfish ") {
            self.gm_create_fish(h, cmd);
        } else if p(b"/teleport ") || p(b"/tp ") {
            self.gm_teleport(h, cmd);
        } else if p(b"/summonplayer ") {
            self.gm_summon_player(h, cmd);
        } else if p(b"/unsummonall") {
            self.gm_unsummon(h, false);
        } else if p(b"/unsummonboss") || p(b"/clearboss") {
            self.gm_unsummon(h, true);
        } else if p(b"/kill ") {
            self.gm_kill(h, cmd);
        } else if p(b"/revive ") {
            self.gm_revive(h, cmd);
        } else if p(b"/summondemon") {
            self.gm_summon_one(h, 2, b"Demon", "/summondemon");
        } else if p(b"/summondeath") {
            self.gm_summon_one(h, 3, b"Wyvern", "/summondeath");
        } else if p(b"/unsummondemon") {
            self.gm_unsummon_demon(h);
        } else if p(b"/unsummonnpc") || p(b"/clearnpc") {
            self.gm_clear_npc(h);
        } else if p(b"/setzerk ") {
            self.gm_set_aura(h, cmd, true);
        } else if p(b"/setfreeze ") {
            self.gm_set_aura(h, cmd, false);
        } else if p(b"/safe") || p(b"/peace") {
            self.gm_safe(h, true, "/safe or /peace");
        } else if p(b"/unsafe") || p(b"/war") {
            self.gm_safe(h, false, "/unsafe or /war");
        } else if p(b"/weather") {
            self.gm_weather(h, cmd);
        } else if p(b"/time ") {
            self.gm_time(h, cmd);
        } else if p(b"/checkip ") {
            self.gm_check_ip(h, cmd);
        } else if p(b"/polymorph ") {
            self.gm_polymorph(h, cmd);
        } else if p(b"/noaggro") || p(b"/invincible") {
            if lvl >= 4 {
                self.client_mut(h).unwrap().gm_flags ^= if p(b"/noaggro") { GMFLAG_NOAGGRO } else { GMFLAG_INVENCIBLE };
            }
        } else if p(b"/setinvi") || p(b"/invi") {
            if self.client(h).unwrap().is_gm() {
                self.client_mut(h).unwrap().status ^= STATUS_INVISIBILITY;
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
            }
        } else if p(b"/sethp") || p(b"/setmp") || p(b"/setmag") {
            let c = self.client_mut(h).unwrap();
            if c.is_gm() {
                // (uint32 in the C++, no notify: the client sees it with the next HP / MP message)
                let v = atoi(&cmd[if p(b"/setmag") { 7 } else { 6 }..]);
                match cmd[4] {
                    b'h' => c.hp = v,
                    b'm' if p(b"/setmp") => c.mp = v,
                    _ => c.mag = v,
                }
            }
        } else if p(b"/gns ") {
            self.gm_npc_status(h, cmd);
        } else if p(b"/setattackmode ") {
            self.gm_set_attack_mode(h, cmd);
        } else if p(b"/setforcerecalltime ") {
            self.gm_force_recall_time(h, cmd);
        } else if p(b"/summon ") {
            self.gm_summon(h, cmd);
        } else if p(b"/disconnectall") {
            if lvl < 4 {
                return self.admin_low(h);
            }
            self.force_player_disconnect(MAXCLIENTS as usize);
        } else if p(b"/createitem ") || p(b"/ci ") {
            self.gm_create_item(h, cmd);
        } else if p(b"/energysphere ") {
            if self.level(h) >= 2 {
                self.energy_sphere_create(h as i32);
                let t = format!("GM Order({}): energysphere ", crate::util::l1(&self.client(h).unwrap().char_name));
                self.send_ls_plain(MSGID_GAMEMASTERLOG, t.as_bytes());
            }
        } else if self.apocalypse_gm_command(h, cmd) || self.heldenian_gm_command(h, cmd) || self.crusade_gm_command(h, cmd) || self.astoria_gm_command(h, cmd) {
        } else if p(b"/joinparty ") {
            self.request_join_party(h, cmd);
        } else if p(b"/dismissparty") {
            self.request_dismiss_party(h);
        } else if p(b"/getpartyinfo") {
            self.get_party_info(h);
        } else if p(b"/deleteparty") {
            self.request_delete_party(h);
        } else if p(b"/shutdownthisserverrightnow ") && lvl >= 4 {
            self.shutdown_by_admin();
        } else if (p(b"/setobservermode") || p(b"/obs")) && lvl >= 3 {
            self.gm_observer_mode(h);
        } else if p(b"/getticket") && lvl >= 2 {
            self.gm_get_ticket(h);
        }
    }

    /// AdminOrder_Pushplayer (/send name map x y): one of the listed maps
    fn gm_push_player(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 1 {
            return self.admin_low(h);
        }
        let a = args(cmd);
        let Some(name) = a.first() else { return };
        // fix: the C++ teleported to an uninitialised map name when none was given
        let Some(map) = a.get(1).cloned() else { return };
        let (x, y) = (a.get(2).map_or(-1, |t| atoi(t)), a.get(3).map_or(-1, |t| atoi(t)));
        let Some(i) = self.player_named(&name10(name)) else { return };
        if SEND_MAPS.contains(&&map[..]) {
            self.request_teleport(i, 2, Some(&map), x as i16, y as i16);
        }
        let (gc, tc) = (self.client(h).unwrap(), self.client(i).unwrap());
        let txt = format!("({}) GM({}) sends ({}) Player({}) to [{}]({x},{y})", l1(&gc.ip_address), l1(&gc.char_name), l1(&tc.ip_address), l1(&tc.char_name), l1(&map));
        self.gm_log(&txt);
    }

    /// AdminOrder_GoTo (/goto name): to the player, or a find-character request to the other servers
    fn gm_goto(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 1 {
            return self.admin_low(h);
        }
        let a = args(cmd);
        let Some(name) = a.first() else { return };
        let n = name10(name);
        if let Some(i) = self.player_named(&n) {
            let t = self.client(i).unwrap();
            let (map, x, y) = (cstr(&t.map_name).to_vec(), t.x, t.y);
            let txt = if x == -1 { format!("GM Order({}): GoTo MapName({})", self.cname(h), l1(&map)) } else { format!("GM Order({}): GoTo MapName({})({x} {y})", self.cname(h), l1(&map)) };
            self.gm_log(&txt);
            self.request_teleport(h, 2, Some(&map), x, y);
            return;
        }
        let mut d = vec![GSM_REQUEST_FINDCHARACTER as u8];
        d.extend_from_slice(&self.gsid.to_le_bytes());
        d.extend_from_slice(&(h as u16).to_le_bytes());
        d.extend_from_slice(&n);
        d.extend_from_slice(&Game::pad10(&self.client(h).unwrap().char_name));
        self.stock_msg(&d);
    }

    /// AdminOrder_CleanMap (/clearmap map): every ground item of the map goes; its players are re-entered to see it
    fn gm_clean_map(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 4 {
            return self.admin_low(h);
        }
        let Some(map) = args(cmd).into_iter().next() else { return };
        let Some(m) = self.maps.iter().position(|x| Game::pad10(&x.name) == Game::pad10(&map)) else { return };
        // the C++ skipped row and column 0
        let keys: Vec<_> = self.ground.keys().filter(|k| k.0 == m && k.1 >= 1 && k.2 >= 1).cloned().collect();
        for k in keys {
            self.ground.remove(&k);
            if let Some(t) = self.maps[m].tile_mut(k.1, k.2) {
                t.top_item = None;
            }
        }
        self.ground_timers.retain(|t| !(t.1 == m && t.2 >= 1 && t.3 >= 1));
        // fix (gameplay-visible): the C++ re-entered every player of the server, and none at all when the GM was not
        // on the cleaned map; here the players of that map
        for i in self.handles() {
            let c = self.client(i).unwrap();
            if c.map_index == m {
                let (map, x, y) = (cstr(&c.map_name).to_vec(), c.x, c.y);
                self.request_teleport(i, 2, Some(&map), x, y);
            }
        }
    }

    /// AdminOrder_CheckRep (/checkrep [name])
    fn gm_check_rep(&mut self, h: usize, cmd: &[u8]) {
        let rep = self.client(h).unwrap().reputation;
        self.show_client_msg(h, format!(" You have {rep} reputation points.").as_bytes());
        if !self.client(h).unwrap().is_gm() {
            return;
        }
        let name = args(cmd).into_iter().next().unwrap_or_else(|| b"null".to_vec());
        for i in self.handles() {
            let c = self.client(i).unwrap();
            if Game::pad10(&c.char_name) == name10(&name) {
                let txt = format!(" {} has {} reputation points.", l1(&c.char_name), c.reputation);
                self.show_client_msg(h, txt.as_bytes());
            }
        }
    }

    /// GetPlayerProfile (/pf name)
    fn gm_get_profile(&mut self, h: usize, cmd: &[u8]) {
        let Some(name) = args(cmd).into_iter().next() else { return };
        let n = name10(&name);
        match self.player_named(&n) {
            Some(i) => {
                let txt = [cstr(&n), b" Profile:", &self.client(i).unwrap().profile].concat();
                self.notify(h, NOTIFY_PLAYERPROFILE, &[], Some(&txt));
            }
            None => self.notify(h, NOTIFY_PLAYERNOTONGAME, &[], Some(&n)),
        }
    }

    /// ShutUpPlayer (/shutup name minutes): here or on the other servers (GSM_REQUEST_SHUTUPPLAYER)
    fn gm_shutup(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let a = args(cmd);
        let Some(name) = a.first() else { return };
        let n = name10(name);
        let t = a.get(1).map_or(0, |t| atoi(t)).max(0);
        if let Some(i) = self.player_named(&n) {
            self.client_mut(i).unwrap().time_left_shutup = t * 20;
            self.notify(h, NOTIFY_PLAYERSHUTUP, &[t], Some(&n));
            self.notify(i, NOTIFY_PLAYERSHUTUP, &[t], Some(&n));
            return;
        }
        let mut d = vec![GSM_REQUEST_SHUTUPPLAYER as u8];
        d.extend_from_slice(&self.gsid.to_le_bytes());
        d.extend_from_slice(&(h as u16).to_le_bytes());
        d.extend_from_slice(&n);
        d.extend_from_slice(&(t as u16).to_le_bytes());
        d.extend_from_slice(&Game::pad10(&self.client(h).unwrap().char_name));
        self.stock_msg(&d);
    }

    /// SetPlayerReputation (/rep+ /rep- name): once in 20 minutes, level 15+, no criminal, not from the same IP
    fn set_player_reputation(&mut self, h: usize, cmd: &[u8], v: i32) {
        let c = self.client(h).unwrap();
        if c.level < 15 {
            return;
        }
        if c.time_left_rating != 0 || c.pk_count != 0 {
            let t = c.time_left_rating;
            return self.notify(h, NOTIFY_CANNOTRATING, &[t], None);
        }
        if c.is_neutral() {
            return self.notify(h, NOTIFY_CANNOTRATING, &[0], None);
        }
        let Some(name) = args(cmd).into_iter().next() else { return };
        let n = name10(&name);
        let Some(i) = self.player_named(&n) else { return self.notify(h, NOTIFY_PLAYERNOTONGAME, &[], Some(&n)) };
        if i == h || cstr(&self.client(i).unwrap().ip_address) == cstr(&self.client(h).unwrap().ip_address) {
            return;
        }
        self.client_mut(i).unwrap().reputation += if v == 0 { -1 } else { 1 };
        self.client_mut(h).unwrap().time_left_rating = 20 * 60;
        self.notify(i, NOTIFY_RATINGPLAYER, &[v], Some(&n));
        self.notify(h, NOTIFY_RATINGPLAYER, &[v], Some(&n));
    }

    /// SetSummonMobAction (/free 0, /hold 1, /tgt name 2): the player's summons
    fn summon_mob_action(&mut self, h: usize, mode: i32, cmd: &[u8]) {
        let c = self.client(h).unwrap();
        if c.is_neutral() {
            return;
        }
        let (map, side) = (c.map_name.clone(), c.side);
        let mut target = None;
        if mode == 2 {
            let Some(name) = args(cmd).into_iter().next() else { return };
            target = self.handles().into_iter().find(|&i| {
                let o = self.client(i).unwrap();
                Game::pad10(&o.char_name) == name10(&name) && cstr(&o.map_name) == cstr(&map)
            });
            if !target.is_some_and(|t| self.client(t).is_some_and(|o| o.side != 0 && o.side != side)) {
                return;
            }
        }
        for n in self.npcs.iter_mut().flatten() {
            if !(n.is_summoned && n.follow_owner == h && n.follow_owner_type == OWNERTYPE_PLAYER && !n.is_killed) {
                continue;
            }
            n.summon_control_mode = mode;
            n.behavior_turn_count = 0;
            match target {
                Some(t) => (n.behavior, n.target, n.target_type, n.is_perm_attack_mode) = (BEHAVIOR_ATTACK, t, OWNERTYPE_PLAYER, true),
                None => (n.behavior, n.target, n.is_perm_attack_mode) = (BEHAVIOR_MOVE, 0, false),
            }
        }
    }

    /// AdminOrder_CloseConn (/closeconn name)
    fn gm_close_conn(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let Some(name) = args(cmd).into_iter().next() else { return };
        let n = name10(&name);
        let Some(i) = self.player_named(&n) else { return self.notify(h, NOTIFY_PLAYERNOTONGAME, &[], Some(&n)) };
        if !self.client(i).unwrap().is_init_complete {
            return;
        }
        if self.level(h) < self.level(i) {
            return self.admin_low(h);
        }
        let txt = format!("GM Order({}): Close Connection PC({})", self.cname(h), self.cname(i));
        self.gm_log(&txt);
        self.delete_client(i, true, true, true, true);
    }

    fn fightzone_gm(&mut self, h: usize) -> Option<usize> {
        let m = self.client(h)?.map_index;
        if !self.maps[m].is_fight_zone {
            return None;
        }
        if self.level(h) < 1 {
            self.admin_low(h);
            return None;
        }
        Some(m)
    }

    pub fn map_players(&self, m: usize) -> Vec<usize> {
        self.handles().into_iter().filter(|&i| self.client(i).unwrap().map_index == m).collect()
    }

    /// AdminOrder_EventSpell (/eventspell name [1|0]): a spell allowed or banned in the fight zone
    fn gm_event_spell(&mut self, h: usize, cmd: &[u8]) {
        if cmd.len() <= 12 {
            return;
        }
        let Some(m) = self.fightzone_gm(h) else { return };
        let a = args(cmd);
        let Some(name) = a.first() else { return };
        let Some(id) = self.magic_number(&name[..name.len().min(30)]) else { return };
        let lim = &mut self.maps[m].magic_limited[id as usize];
        *lim = match a.get(1).map(|t| t[0]) {
            Some(b'1') => false,
            Some(b'0') => true,
            Some(_) => *lim,
            None => !*lim,
        };
        let on = *lim;
        let sp = self.data.magic.get(id as usize).and_then(|r| r.as_deref()).and_then(crate::magic::Spell::parse);
        for i in self.map_players(m) {
            if let Some(sp) = &sp {
                let c = self.client(i).unwrap();
                if on && !c.is_gm() && (0..c.magic_effect_status.len() as i32).contains(&sp.ty) && c.magic_effect_status[sp.ty as usize] == sp.val[0] {
                    self.remove_magic_effect(i, OWNERTYPE_PLAYER, sp.ty);
                }
            }
            self.notify(i, NOTIFY_EVENTSPELL, &[on as i32, id], None);
        }
    }

    /// AdminOrder_EventArmor (0) / Sheild (1) / Chat (2) / Party (3) / Illusion (4): a fight zone switch
    fn gm_event_switch(&mut self, h: usize, cmd: &[u8], which: u8) {
        let Some(m) = self.fightzone_gm(h) else { return };
        let mp = &mut self.maps[m];
        // armor, shield, chat and party are "disabled" flags (1 = allowed); the illusion is "on" (1 = on)
        let flag = match which {
            0 => &mut mp.is_armor_disabled,
            1 => &mut mp.is_shield_disabled,
            2 => &mut mp.is_chat_disabled,
            3 => &mut mp.is_party_disabled,
            _ => &mut mp.is_perm_illusion_on,
        };
        *flag = match args(cmd).first().map(|t| t[0]) {
            Some(b'1') => which == 4,
            Some(b'0') => which != 4,
            Some(_) => *flag,
            None => !*flag,
        };
        let on = *flag;
        for i in self.map_players(m) {
            let c = self.client(i).unwrap();
            let (gm, dead) = (c.is_gm(), c.is_killed);
            match which {
                0 => {
                    if on && !gm && !dead {
                        for pos in [EQUIPPOS_HEAD, EQUIPPOS_BODY, EQUIPPOS_ARMS, EQUIPPOS_LEGGINGS, EQUIPPOS_PANTS] {
                            self.release_equipped(i, pos, false);
                        }
                        self.send_event_near_type_a(i, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
                    }
                    self.notify(i, NOTIFY_EVENTARMOR, &[on as i32], None);
                }
                1 => {
                    if on && !gm && !dead {
                        self.release_equipped(i, EQUIPPOS_LHAND, true);
                    }
                    self.notify(i, NOTIFY_EVENTSHIELD, &[on as i32], None);
                }
                2 => self.notify(i, NOTIFY_EVENTCHAT, &[on as i32], None),
                3 => {
                    if on && !gm && self.client(i).unwrap().party_id != 0 {
                        self.request_dismiss_party(i);
                    }
                    self.notify(i, NOTIFY_EVENTPARTY, &[on as i32], None);
                }
                _ => {
                    if !gm {
                        if on {
                            // (no STATUS_ILLUSION here, unlike the map entry: as in the C++)
                            self.client_mut(i).unwrap().magic_effect_status[MAGICTYPE_CONFUSE as usize] = 3;
                            self.notify(i, NOTIFY_MAGICEFFECTON, &[MAGICTYPE_CONFUSE, 3, h as i32], None);
                        } else {
                            self.remove_magic_effect(i, OWNERTYPE_PLAYER, MAGICTYPE_CONFUSE);
                        }
                    }
                    self.notify(i, NOTIFY_EVENTILLUSION, &[on as i32], None);
                }
            }
        }
    }

    /// NOTIFY_ITEMRELEASED + ReleaseItemHandler of what the player wears at `pos`
    fn release_equipped(&mut self, i: usize, pos: i32, notice: bool) {
        let k = self.client(i).unwrap().equip_status[pos as usize];
        if k != -1 {
            self.notify(i, NOTIFY_ITEMRELEASED, &[pos, k as i32], None);
            self.release_item(i, k as usize, notice);
        }
    }

    /// the fight zone part of the map entry: the GM event switches in force there
    pub fn fightzone_entry(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        let (m, gm, dead) = (c.map_index, c.is_gm(), c.is_killed);
        let mp = &self.maps[m];
        let (shield, armor, illusion, chat, limited) = (mp.is_shield_disabled, mp.is_armor_disabled, mp.is_perm_illusion_on, mp.is_chat_disabled, mp.magic_limited);
        if shield {
            self.notify(h, NOTIFY_EVENTSHIELD, &[1], None);
        }
        if armor {
            if !gm && !dead {
                for pos in [EQUIPPOS_HEAD, EQUIPPOS_BODY, EQUIPPOS_ARMS, EQUIPPOS_LEGGINGS, EQUIPPOS_PANTS] {
                    self.release_equipped(h, pos, false);
                }
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
            }
            self.notify(h, NOTIFY_EVENTARMOR, &[1], None);
        }
        if illusion {
            // the illusion comes from a GM on the map (none: nothing)
            if let Some(g) = (!gm).then(|| self.map_players(m).into_iter().find(|&i| self.client(i).unwrap().is_gm())).flatten() {
                self.notify(h, NOTIFY_MAGICEFFECTON, &[MAGICTYPE_CONFUSE, 3, g as i32], None);
                let c = self.client_mut(h).unwrap();
                c.magic_effect_status[MAGICTYPE_CONFUSE as usize] = 3;
                c.set_status_flag(STATUS_ILLUSION, true);
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
            }
            self.notify(h, NOTIFY_EVENTILLUSION, &[1], None);
        }
        if chat {
            self.notify(h, NOTIFY_EVENTCHAT, &[1], None);
        }
        for (i, &l) in limited.iter().enumerate() {
            if l {
                self.notify(h, NOTIFY_EVENTSPELL, &[1, i as i32], None);
            }
        }
    }

    /// AdminOrder_EventReset: every switch of the fight zone back to normal
    /// v8.00 events (no C++ original), GM level 4: `/doubleexp [minutes]` / `/doubleek [minutes]`, 60 minutes by default, 0 stops it
    fn gm_server_event(&mut self, h: usize, cmd: &[u8]) {
        log(&format!("(gm) {}: {} (admin level {})", self.cname(h), l1(cmd), self.level(h)));
        if self.level(h) < 4 {
            return self.admin_low(h);
        }
        let ev = if cmd.starts_with(b"/doubleexp") { crate::events::EVENT_DOUBLE_EXP } else { crate::events::EVENT_DOUBLE_EK };
        let arg = cmd.iter().position(|&b| b == b' ').map(|i| &cmd[i + 1..]).unwrap_or(b"");
        let minutes = if arg.iter().all(|b| b.is_ascii_whitespace()) { 60 } else { crate::util::atoi(arg).clamp(0, 24 * 60) };
        self.event_set(ev, minutes);
        log(&format!("(gm) event {ev} set to {minutes} min, active: {}", self.event_active(ev)));
        let who = self.cname(h);
        self.gm_log(&format!("{who} {} {minutes} min", String::from_utf8_lossy(&cmd[..cmd.iter().position(|&b| b == b' ').unwrap_or(cmd.len())])));
    }

    fn gm_event_reset(&mut self, h: usize) {
        let Some(m) = self.fightzone_gm(h) else { return };
        let illusion = self.maps[m].is_perm_illusion_on;
        let mp = &mut self.maps[m];
        mp.magic_limited = [false; 100];
        (mp.is_party_disabled, mp.is_shield_disabled, mp.is_armor_disabled, mp.is_chat_disabled) = (false, false, false, false);
        for i in self.map_players(m) {
            if illusion {
                self.remove_magic_effect(i, OWNERTYPE_PLAYER, MAGICTYPE_CONFUSE);
            }
            self.notify(i, NOTIFY_EVENTRESET, &[], None);
        }
        self.maps[m].is_perm_illusion_on = false;
    }

    /// AdminOrder_EventTP (/eventtp [1|0]): William's teleports from each city hall to this fight zone
    fn gm_event_tp(&mut self, h: usize, cmd: &[u8]) {
        let Some(_m) = self.fightzone_gm(h) else { return };
        let here = cstr(&self.client(h).unwrap().map_name).to_vec();
        let exists = self.data.teleports.iter().any(|t| t.admin_created && cstr(&t.target_map) == &here[..]);
        let enabling = match args(cmd).first().map(|t| t[0]) {
            Some(b'1') => true,
            Some(b'0') => false,
            Some(_) => true,
            None => !exists,
        };
        if enabling {
            for side in 1..MAXSIDES {
                if self.data.teleports.len() >= MAXTELEPORTLIST as usize {
                    break;
                }
                self.data.teleports.push(crate::gamedata::TeleportCfg {
                    admin_created: true,
                    npc_name: b"William".to_vec(),
                    source_map: format!("cityhall_{side}").into_bytes(),
                    target_map: here.clone(),
                    x: -1,
                    y: -1,
                    min_lvl: 0,
                    max_lvl: MAXLEVEL,
                    side,
                    ..Default::default()
                });
            }
        } else {
            for _ in 1..MAXSIDES {
                if let Some(k) = self.data.teleports.iter().position(|t| t.admin_created && cstr(&t.target_map) == &here[..]) {
                    self.data.teleports.remove(k);
                }
            }
        }
        for i in self.handles() {
            self.notify(i, NOTIFY_EVENTTP, &[enabling as i32], None);
        }
    }

    /// AdminOrder_CallGuard (/attack name): a guard of the player's town attacks him
    fn gm_call_guard(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let Some(name) = args(cmd).into_iter().next() else { return };
        let n = name10(&name);
        let Some(i) = self.player_named(&n) else { return self.notify(h, NOTIFY_PLAYERNOTONGAME, &[], Some(&n)) };
        let c = self.client(i).unwrap();
        let (m, x, y) = (c.map_index, c.x, c.y);
        let guard: &[u8] = if c.map_name.starts_with(b"aresden") {
            b"Guard-Aresden"
        } else if c.map_name.starts_with(b"elvine") {
            b"Guard-Elvine"
        } else if c.map_name.starts_with(b"defaul") {
            b"Guard-Istria" // sideMap[ISTRIA] is "default2", compared on 6 bytes
        } else {
            b"Guard-Neutral"
        };
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return };
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        let Some(g) = self.create_npc(guard, &name, m, 0, MOVETYPE_RANDOM, Some((x, y)), &[-1; 10], None, -1) else {
            self.maps[m].set_naming_value_empty(nv);
            return;
        };
        let now = self.now();
        let txt = format!("GM Order({}): Guard Attack PC({})", self.cname(h), self.cname(i));
        self.gm_log(&txt);
        let n = self.npcs[g].as_mut().unwrap();
        (n.is_summoned, n.summoned_time) = (true, now);
        // bSetNpcAttackMode(name, player, TRUE)
        (n.behavior, n.behavior_turn_count, n.target, n.target_type, n.is_perm_attack_mode) = (BEHAVIOR_ATTACK, 0, i, OWNERTYPE_PLAYER, true);
    }

    /// AdminOrder_CreateFish (/createfish x y difficulty)
    fn gm_create_fish(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let a = args(cmd);
        let v = |i: usize| a.get(i).map_or(0, |t| atoi(t));
        let (x, y, t) = (v(0), v(1), v(2));
        if x == 0 || y == 0 || t == 0 {
            return;
        }
        let Some(cfg) = self.data.item_by_id(100).cloned() else { return };
        let m = self.client(h).unwrap().map_index;
        self.create_fish(m, x as i16, y as i16, Item::new(&cfg), t, 60000 * 20);
    }

    /// AdminOrder_Teleport (/teleport map [x y], /tp): to one of the listed maps
    fn gm_teleport(&mut self, h: usize, cmd: &[u8]) {
        let c = self.client(h).unwrap();
        if c.is_observer_mode || c.admin_user_level < 1 {
            return;
        }
        let a = args(cmd);
        let Some(map) = a.first().cloned() else { return };
        let mut x = a.get(1).map_or(-1, |t| atoi(t));
        let y = a.get(2).map_or(-1, |t| atoi(t));
        if y == -1 {
            x = -1;
        }
        if !TELEPORT_MAPS.contains(&&map[..]) {
            return;
        }
        let txt = if x == -1 && y == -1 { format!("GM Order({}): teleport MapName({})", self.cname(h), l1(&map)) } else { format!("GM Order({}): teleport MapName({})({x} {y})", self.cname(h), l1(&map)) };
        self.gm_log(&txt);
        self.request_teleport(h, 2, Some(&map), x as i16, y as i16);
    }

    /// AdminOrder_SummonPlayer (/summonplayer name): here or through GSM_REQUEST_SUMMONPLAYER
    fn gm_summon_player(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let Some(name) = args(cmd).into_iter().next() else { return };
        let n = name10(&name);
        let c = self.client(h).unwrap();
        let (map, x, y) = (c.map_name.clone(), c.x, c.y);
        if let Some(i) = self.player_named(&n) {
            if i != h {
                self.request_teleport(i, 2, Some(&map), x, y);
            }
            return;
        }
        let txt = format!("GM Order({}): PC({}) Summoned to ({})", self.cname(h), l1(&n), l1(&map));
        self.gm_log(&txt);
        let mut d = vec![GSM_REQUEST_SUMMONPLAYER as u8];
        d.extend_from_slice(&n);
        d.extend_from_slice(&Game::pad10(&map));
        d.extend_from_slice(&(x as u16).to_le_bytes());
        d.extend_from_slice(&(y as u16).to_le_bytes());
        self.stock_msg(&d);
    }

    /// AdminOrder_UnsummonAll / UnsummonBoss (level 3)
    fn gm_unsummon(&mut self, h: usize, boss: bool) {
        if self.level(h) < 3 {
            return self.admin_low(h);
        }
        for i in 1..self.npcs.len() {
            let Some(n) = self.npcs[i].as_mut() else { continue };
            if n.is_killed {
                continue;
            }
            if boss {
                if !matches!(n.type_, 31 | 49 | 45 | 47 | 50 | 52) {
                    continue;
                }
                n.is_summoned = true; // no exp, no drop
            } else if !n.is_summoned {
                continue;
            }
            self.npc_killed(h, OWNERTYPE_PLAYER, i, 0);
        }
    }

    /// AdminOrder_CheckIP (/checkip ip-prefix)
    fn gm_check_ip(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let Some(ip) = args(cmd).into_iter().next() else { return };
        for i in self.handles() {
            let c = self.client(i).unwrap();
            if !c.ip_address.starts_with(&ip) {
                continue;
            }
            let txt = format!("Name({}/{}) Loc({}: {} {}) Level({}:0) Init({}) IP({})", l1(&c.account_name), l1(&c.char_name), l1(&c.map_name), c.x, c.y, c.level, c.is_init_complete as i32, l1(&c.ip_address));
            self.notify(h, NOTIFY_IPACCOUNTINFO, &[], Some(txt.as_bytes()));
        }
    }

    /// AdminOrder_Polymorph (/polymorph name|off): the GM looks like an NPC type
    fn gm_polymorph(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 4 {
            return self.admin_low(h);
        }
        let a = args(cmd);
        let c = self.client_mut(h).unwrap();
        if c.original_type == 0 {
            c.original_type = c.type_;
        } else {
            c.type_ = c.original_type;
        }
        if let Some(t) = a.first() {
            if t.starts_with(b"off") {
                c.type_ = c.original_type;
            } else if let Some(&(_, _, ty)) = POLYMORPH.iter().find(|(n, len, _)| {
                let mut z = n.to_vec();
                z.push(0);
                let mut tk = t.clone();
                tk.push(0);
                tk.len() >= *len && z.len() >= *len && tk[..*len] == z[..*len]
            }) {
                c.type_ = ty;
            }
        }
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
    }

    /// AdminOrder_GetNpcStatus (/gns name): NOTIFY_ADMINIFO for every NPC of that object name
    fn gm_npc_status(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        // fix: the C++ compared an uninitialised name when none was given
        let Some(name) = args(cmd).into_iter().next() else { return };
        let mut key = name.clone();
        key.resize(5, 0);
        let found: Vec<usize> = (0..self.npcs.len()).filter(|&i| self.npcs[i].as_ref().is_some_and(|n| { let mut k = n.name.clone(); k.resize(5, 0); k[..5] == key[..5] })).collect();
        for i in found {
            let n = self.npcs[i].as_ref().unwrap();
            let v = [1, n.hp, n.defense_ratio, n.is_summoned as i32, n.action_limit, n.hit_dice, n.dead_time as i32, n.regen_time as i32, n.is_killed as i32];
            self.notify(h, NOTIFY_ADMINIFO, &v, None);
        }
    }

    /// AdminOrder_SetAttackMode (/setattackmode 1|0, on a fight map)
    fn gm_set_attack_mode(&mut self, h: usize, cmd: &[u8]) {
        let c = self.client(h).unwrap();
        if !c.map_name.starts_with(b"figh") {
            return;
        }
        if c.admin_user_level < 3 {
            return self.admin_low(h);
        }
        let m = c.map_index;
        let on = match args(cmd).first().map(|t| t[0]) {
            Some(b'1') => true,
            Some(b'0') => false,
            _ => return,
        };
        self.maps[m].is_attack_enabled = on;
        for i in self.map_players(m) {
            if self.client(i).unwrap().is_init_complete {
                self.notify(i, NOTIFY_GLOBALATTACKMODE, &[on as i32], None);
            }
        }
    }

    /// AdminOrder_SetForceRecallTime (/setforcerecalltime minutes): here and on the other servers
    fn gm_force_recall_time(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 3 {
            return self.admin_low(h);
        }
        let Some(t) = args(cmd).into_iter().next() else { return };
        let t = atoi(&t).max(0);
        self.force_recall_time = t;
        let mut d = vec![GSM_REQUEST_SETFORCERECALLTIME as u8];
        d.extend_from_slice(&(t as u16).to_le_bytes());
        self.stock_msg(&d);
        log(&format!("(!) Admin Set Force Recall Time ({t})min"));
    }

    /// AdminOrder_Summon (/summon npc [count [summoned [sa]]]): a group by the GM, the first one leads
    fn gm_summon(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 3 {
            return self.admin_low(h);
        }
        let a = args(cmd);
        let Some(npc) = a.first().cloned() else { return };
        let num = a.get(1).map_or(1, |t| atoi(t)).clamp(1, 50);
        let summoned = a.get(2).is_some_and(|t| t[0] == b'1');
        let sa = a.get(3).map_or(0, |t| t[0].wrapping_sub(b'0') as i8);
        let sa = if (0..=8).contains(&sa) { sa as u8 } else { 0 };
        let c = self.client(h).unwrap();
        let (m, x, y) = (c.map_index, c.x, c.y);
        let txt = format!("GM Order({}): Summon NPC({})-Count({num})", self.cname(h), l1(&npc));
        self.gm_log(&txt);
        let now = self.now();
        let make = |g: &mut Game| -> Option<usize> {
            let nv = g.maps[m].get_empty_naming_value()?;
            let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
            match g.create_npc(&npc, &name, m, sa, MOVETYPE_RANDOM, Some((x, y)), &[-1; 10], None, -1) {
                Some(i) => {
                    if summoned {
                        let n = g.npcs[i].as_mut().unwrap();
                        (n.is_summoned, n.summoned_time) = (true, now);
                    }
                    Some(i)
                }
                None => {
                    g.maps[m].set_naming_value_empty(nv);
                    None
                }
            }
        };
        let master = make(self);
        if let Some(i) = master {
            self.npcs[i].as_mut().unwrap().is_master = true;
        }
        for _ in 0..num - 1 {
            let Some(i) = make(self) else { break };
            // bSetNpcFollowMode(slave, master, OWNERTYPE_NPC)
            if let Some(mh) = master {
                let side = self.npcs[mh].as_ref().unwrap().side;
                let n = self.npcs[i].as_mut().unwrap();
                (n.move_type, n.follow_owner, n.follow_owner_type, n.side) = (MOVETYPE_FOLLOW, mh, OWNERTYPE_NPC, side);
            }
        }
    }

    /// AdminOrder_CreateItem (/createitem name [attribute [endurance% [count]]], /ci): level 4
    fn gm_create_item(&mut self, h: usize, cmd: &[u8]) {
        let lvl = self.level(h);
        if lvl < 4 {
            if lvl != 0 {
                self.admin_low(h);
            }
            return;
        }
        let a = args(cmd);
        let Some(name) = a.first().cloned() else { return };
        let attribute = a.get(1).map_or(0, |t| atoi(t)) as u32;
        let endu = a.get(2).map_or(0, |t| atoi(t));
        let amount = a.get(3).map_or(1, |t| atoi(t));
        let mut i = amount;
        while i > 0 {
            let Some(cfg) = self.data.item_by_name(&name).cloned() else { return };
            let mut it = Item::new(&cfg);
            if matches!(cfg.item_type, crate::life::ITEMTYPE_CONSUME | crate::combat::ITEMTYPE_ARROW) {
                it.count = amount as u32;
                i = 0;
            } else {
                it.count = 1;
            }
            if attribute != 0 {
                it.attribute = attribute;
                if attribute == 1 {
                    if (1..=200).contains(&endu) {
                        it.item_color = 2;
                        it.item_spec_effect_value[1] = (endu - 100) as i16;
                        let d = (it.item_spec_effect_value[1] as f64 / 100.0f32 as f64) * it.max_life_span as f64;
                        let t = it.max_life_span as i32 + d as i32;
                        let w = if t <= 0 { 1 } else { t as u16 };
                        if w as u32 <= it.max_life_span as u32 * 2 {
                            it.max_life_span = w;
                            it.item_spec_effect_value[0] = w as i16;
                            it.cur_life_span = w;
                        } else {
                            it.item_spec_effect_value[0] = it.max_life_span as i16;
                        }
                    } else {
                        it.attribute = 0;
                    }
                } else if cfg.item_effect_type == ITEMEFFECTTYPE_ATTACK {
                    it.item_color = match (attribute & 0xF00000) >> 20 {
                        6 => 2,
                        8 => 3,
                        1 => 5,
                        5 => 1,
                        3 => 7,
                        2 => 4,
                        7 => 6,
                        9 => 8,
                        _ => it.item_color,
                    };
                } else if cfg.item_effect_type == ITEMEFFECTTYPE_ATTACK_MANASAVE && (attribute & 0xF00000) >> 20 == 10 {
                    it.item_color = 5;
                }
            }
            let t = crate::chrono_now();
            if matches!(cfg.id_num, 511 | 513 | 515 | 517 | 530 | 531 | 532 | 533 | 534) {
                (it.touch_effect_type, it.touch_effect_value) = (ITET_DATE as i16, [t.month as i16, t.day as i16, 24]);
            } else {
                let (a1, a2) = (self.dice(1, 100000), self.dice(1, 100000));
                // wsprintf("%d%2d", month, day)
                let v3 = atoi(format!("{}{:2}", t.month, t.day).as_bytes());
                (it.touch_effect_type, it.touch_effect_value) = (ITET_GM_MADE as i16, [a1 as i16, a2 as i16, v3 as i16]);
            }
            if !self.add_client_item_list(h, &it) {
                return;
            }
            self.send_item_obtained(h, &it);
            let txt = format!("GM Order({}): Create ItemName({})", self.cname(h), l1(&name));
            self.gm_log(&txt);
            i -= 1;
        }
    }

    /// /shutdownthisserverrightnow (level 4)
    fn shutdown_by_admin(&mut self) {
        self.shutdown_code = 2;
        self.on_exit_process = true;
        self.exit_process_time = self.now();
        log("(!) GAME SERVER SHUTDOWN PROCESS BEGIN(by Admin-Command)!!!");
        self.send_ls_plain(MSGID_GAMESERVERSHUTDOWNED, &[]);
        // SaveOccupyFlagData returns at once in the C++
    }

    /// AdminOrder_SetObserverMode (/setobservermode, /obs: level 3)
    fn gm_observer_mode(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        let (m, x, y, obs) = (c.map_index, c.x, c.y, c.is_observer_mode);
        if obs {
            self.maps[m].set_owner(h as i16, OWNERTYPE_PLAYER, x, y);
            self.send_event_near_type_a(h, MSGID_EVENT_LOG, MSGTYPE_CONFIRM as u16, 0, 0, 0);
            self.notify(h, NOTIFY_OBSERVERMODE, &[0], None);
        } else {
            self.maps[m].clear_owner(h as i16, OWNERTYPE_PLAYER, x, y);
            self.send_event_near_type_a(h, MSGID_EVENT_LOG, MSGTYPE_REJECT as u16, 0, 0, 0);
            self.notify(h, NOTIFY_OBSERVERMODE, &[1], None);
        }
        self.client_mut(h).unwrap().is_observer_mode = !obs;
        let _ = OWNERTYPE_NPC;
    }
}

/// The vanilla 3.82 GM commands (levels of the v3.82 AdminSettings.cfg; the `/enableadmincommand` gate of the
/// Equilibrium server is not ported, as for /summon and /createitem).
impl Game {
    /// AdminOrder_Kill (/kill name [damage]): level 2. HP 0, no penalty, no reward, no killer.
    fn gm_kill(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let a = args(cmd);
        let n = name10(a.first().map_or(&b"null"[..], |t| &t[..]));
        let damage = a.get(1).map_or(1, |t| atoi(t));
        let Some(i) = self.player_named(&n) else { return self.notify(h, NOTIFY_PLAYERNOTONGAME, &[], Some(&n)) };
        // fix: a client still entering the world has no map position (the C++ killed it all the same)
        if !self.client(i).unwrap().is_init_complete {
            return;
        }
        let c = self.client_mut(i).unwrap();
        (c.hp, c.is_killed) = (0, true);
        let (m, x, y) = (c.map_index, c.x, c.y);
        self.cancel_exchange_if_open(i);
        let txt = format!("GM Order({}): /kill ({})", self.cname(h), self.cname(i));
        self.gm_log(&txt);
        self.remove_from_target(i, OWNERTYPE_PLAYER, 0);
        self.notify(i, NOTIFY_KILLED, &[], Some(b""));
        self.send_event_near_type_a(i, MSGID_EVENT_MOTION, OBJECTDYING as u16, damage as i16, 1, 0);
        self.maps[m].clear_owner(i as i16, OWNERTYPE_PLAYER, x, y);
        self.maps[m].set_dead_owner(i as i16, OWNERTYPE_PLAYER, x, y);
    }

    /// AdminOrder_Revive (/revive name [damage [hp]]): level 2. HP back to `hp` (1 by default, at most the maximum);
    /// no re-entry, the client sees the HP and a damage motion.
    fn gm_revive(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let a = args(cmd);
        let n = name10(a.first().map_or(&b"null"[..], |t| &t[..]));
        let damage = a.get(1).map_or(1, |t| atoi(t));
        let hp = a.get(2).map_or(1, |t| atoi(t));
        let Some(i) = self.player_named(&n) else { return self.notify(h, NOTIFY_PLAYERNOTONGAME, &[], Some(&n)) };
        // fix: a client still entering the world has no map position
        if !self.client(i).unwrap().is_init_complete {
            return;
        }
        let txt = format!("GM Order({}): /revive ({})", self.cname(h), self.cname(i));
        self.gm_log(&txt);
        let c = self.client_mut(i).unwrap();
        c.hp = hp.min(c.get_max_hp());
        c.is_killed = false;
        let (m, x, y) = (c.map_index, c.x, c.y);
        self.notify(i, NOTIFY_HP, &[], None);
        self.send_event_near_type_a(i, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, damage as i16, 1, 0);
        self.maps[m].clear_owner(i as i16, OWNERTYPE_PLAYER, x, y);
        // fix: the C++ then called SetDeadOwner, leaving the revived player a corpse for the map (nobody could
        // target him); here he is the live owner of his tile again (when nobody else stands there now)
        if self.maps[m].get_owner(x, y).0 == 0 {
            self.maps[m].set_owner(i as i16, OWNERTYPE_PLAYER, x, y);
        }
    }

    /// AdminOrder_SummonDemon (level 2) / AdminOrder_SummonDeath (level 3): one `npc` with a random special ability
    /// where the GM stands
    fn gm_summon_one(&mut self, h: usize, level: i32, npc: &[u8], cmd_name: &str) {
        if self.level(h) < level {
            return self.admin_low(h);
        }
        let c = self.client(h).unwrap();
        let (m, x, y) = (c.map_index, c.x, c.y);
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return };
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        let sa = (self.rng.rand() % 9) as u8;
        if self.create_npc(npc, &name, m, sa, MOVETYPE_RANDOM, Some((x, y)), &[-1; 10], None, -1).is_none() {
            self.maps[m].set_naming_value_empty(nv);
            return;
        }
        let txt = format!("GM Order({}): {cmd_name} (done)", self.cname(h));
        self.gm_log(&txt);
    }

    /// AdminOrder_UnsummonDemon (/unsummondemon): every Demon (type 31) of the server falls, level 2
    fn gm_unsummon_demon(&mut self, h: usize) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        for i in 1..self.npcs.len() {
            let Some(n) = self.npcs[i].as_mut() else { continue };
            if n.type_ != 31 || n.is_killed {
                continue;
            }
            // fix: as /unsummonboss, the GM gets no exp and no drop (the C++ gave him the kill)
            n.is_summoned = true;
            self.npc_killed(h, OWNERTYPE_PLAYER, i, 0);
        }
        let txt = format!("GM Order({}): /unsummondemon (done)", self.cname(h));
        self.gm_log(&txt);
    }

    /// AdminOrder_ClearNpc (/clearnpc, /unsummonnpc): every NPC of the GM's map goes, except the summoned ones, the
    /// merchants and the gates; level 3
    fn gm_clear_npc(&mut self, h: usize) {
        if self.level(h) < 3 {
            return self.admin_low(h);
        }
        let m = self.client(h).unwrap().map_index;
        for i in 1..self.npcs.len() {
            let Some(n) = self.npcs[i].as_mut() else { continue };
            if n.is_summoned || n.map_index != m || matches!(n.type_, 15 | 19 | 20 | 24 | 25 | 26 | 67 | 68 | 69 | 90 | 91) {
                continue;
            }
            // m_bIsUnsummoned in the C++ (no exp, no drop): the same marking as /unsummonboss
            n.is_summoned = true;
            self.npc_killed(h, OWNERTYPE_PLAYER, i, 0);
            self.delete_npc(i);
        }
        let txt = format!("GM Order({}): /clearnpc (done)", self.cname(h));
        self.gm_log(&txt);
    }

    /// AdminOrder_SetZerk (/setzerk 1|0, `zerk`) and AdminOrder_SetFreeze (/setfreeze 1|0): the aura on the GM, level 2
    fn gm_set_aura(&mut self, h: usize, cmd: &[u8], zerk: bool) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let Some(t) = args(cmd).into_iter().next() else { return };
        let on = t[0] == b'1';
        if on || t[0] == b'0' {
            if zerk && on {
                // the C++ stores the Value4 of Magic.cfg entry 32 (Invisibility, 0), not Berserk's: aura only
                let v = self.data.magic.get(32).and_then(|r| r.as_deref()).and_then(Spell::parse).map_or(0, |s| s.val[3]);
                self.client_mut(h).unwrap().magic_effect_status[MAGICTYPE_BERSERK as usize] = v;
            }
            self.set_unit_status_flag(h, OWNERTYPE_PLAYER, if zerk { STATUS_BERSERK } else { STATUS_FROZEN }, on);
        }
        let txt = format!("GM Order({}): {} (done)", self.cname(h), if zerk { "/setzerk" } else { "/setfreeze" });
        self.gm_log(&txt);
    }

    /// AdminOrder_Safe (/safe, /peace, /unsafe, /war): attacks off / on in the GM's map, level 2
    fn gm_safe(&mut self, h: usize, safe: bool, cmd_name: &str) {
        if self.level(h) < 2 {
            return self.admin_low(h);
        }
        let m = self.client(h).unwrap().map_index;
        self.maps[m].is_attack_enabled = !safe;
        for i in self.map_players(m) {
            if self.client(i).unwrap().is_init_complete {
                self.notify(i, NOTIFY_GLOBALATTACKMODE, &[!safe as i32], None);
            }
        }
        let txt = format!("GM Order({}): {cmd_name}", self.cname(h));
        self.gm_log(&txt);
    }

    /// AdminOrder_Weather (/weather type seconds): rain (1..3) or snow (4..6) on every map that has none, level 3
    fn gm_weather(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 3 {
            return self.admin_low(h);
        }
        let a = args(cmd);
        let (Some(t), Some(s)) = (a.first(), a.get(1)) else { return };
        // fix: the C++ multiplied the time by 1000 inside the map loop (each next map got it 1000 times longer);
        // and the type is kept to the client's 0..6
        let (ty, ms) = (atoi(t).clamp(0, 6) as u8, (atoi(s) as u32).wrapping_mul(1000));
        let txt = format!("GM Order({}): /weather (done)", self.cname(h));
        self.gm_log(&txt);
        let now = self.now();
        for m in 0..self.maps.len() {
            if self.maps[m].is_fixed_day_mode {
                continue;
            }
            let prev = self.maps[m].weather;
            if prev != 0 {
                if now.wrapping_sub(self.maps[m].weather_start) > self.maps[m].weather_duration {
                    self.maps[m].weather = 0;
                }
            } else {
                (self.maps[m].weather, self.maps[m].weather_start, self.maps[m].weather_duration) = (ty, now, ms);
            }
            let w = self.maps[m].weather;
            if prev != w {
                for i in self.map_players(m) {
                    if self.client(i).unwrap().is_init_complete {
                        self.notify(i, NOTIFY_WHETHERCHANGE, &[w as i32], None);
                    }
                }
            }
        }
    }

    /// AdminOrder_Time (/time 0|1|2): automatic, day, night for the whole server, level 3
    fn gm_time(&mut self, h: usize, cmd: &[u8]) {
        if self.level(h) < 3 {
            return self.admin_low(h);
        }
        let Some(t) = args(cmd).into_iter().next() else { return };
        // fix: the C++ sent the previous log line for "/time 0"
        let txt = match t[0] {
            b'2' => "Forces night mode",
            b'1' => "Forces day mode",
            b'0' => "Disable force time mode",
            _ => "",
        };
        match t[0] {
            b'2' | b'1' => (self.day_or_night, self.manual_time) = (t[0] - b'0', true),
            b'0' => {
                self.manual_time = false;
                self.check_day_or_night_mode();
            }
            _ => {}
        }
        if !txt.is_empty() {
            let txt = format!("GM Order({}): {txt}", self.cname(h));
            self.gm_log(&txt);
        }
        let d = self.day_or_night as i32;
        for i in self.handles() {
            let c = self.client(i).unwrap();
            if c.is_init_complete && !self.maps[c.map_index].is_fixed_day_mode {
                self.notify(i, NOTIFY_TIMECHANGE, &[d], None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert_eq!(args(b"/send RtHg1 aresden 100 120"), vec![b"RtHg1".to_vec(), b"aresden".to_vec(), b"100".to_vec(), b"120".to_vec()]);
        assert_eq!(name10(b"AVeryLongCharName"), *b"AVeryLongC");
        assert!(TELEPORT_MAPS.contains(&&b"BtField"[..]) && !TELEPORT_MAPS.contains(&&b"penalty"[..]));
        assert_eq!(POLYMORPH.len(), 82);
    }
}
