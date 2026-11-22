//! Gate-server stock messages (HG.cpp ~34424): bStockMsgToGateServer, SendStockMsgToGateServer,
//! ServerStockMsgHandler, ChatMsgHandlerGSM, GSM_RequestFindCharacter / FindFriend / ShutupPlayer. The Login relays
//! them between the game servers (Login server.rs `server_stock_msg_handler`).
use crate::consts::*;
use crate::game::Game;
use crate::util::{cstr, Rd};

/// index of a whisper target on another game server (m_iWhisperPlayerIndex)
pub const WHISPER_OTHER_SERVER: i32 = 10000;

impl Game {
    /// `bStockMsgToGateServer`
    pub fn stock_msg(&mut self, d: &[u8]) -> bool {
        if 6 + self.stock.len() + d.len() >= MAXGATESERVERSTOCKMSGSIZE as usize - 10 {
            return false;
        }
        self.stock.extend_from_slice(d);
        true
    }

    /// `SendStockMsgToGateServer` (every second and at DeleteClient)
    pub fn send_stock_msg(&mut self) {
        if self.stock.is_empty() {
            return;
        }
        let mut p = std::mem::take(&mut self.stock);
        p.push(0); // the C++ sends m_iIndexGSS + 1 bytes: a zero terminator
        self.send_ls_plain(MSGID_SERVERSTOCKMSG, &p);
    }

    fn find_by_name(&self, name: &[u8]) -> Option<usize> {
        let name = cstr(name);
        self.handles().into_iter().find(|&i| cstr(&self.client(i).unwrap().char_name) == name)
    }

    /// `ServerStockMsgHandler`: `d` is the whole MSGID_SERVERSTOCKMSG message.
    pub fn server_stock_msg(&mut self, d: &[u8]) {
        let r = Rd::new(d);
        let mut cp = 6usize;
        while cp < d.len() {
            let c = r.at(cp);
            let n = match c.u8(0) as i32 {
                GSM_DISCONNECT => {
                    let name = c.s(1, 10);
                    // fix: the C++ left the loop after the first whisperer (goto SSMH_LOOPBREAK), so other whisperers
                    // stayed in whisper mode and every stock message after this one was dropped
                    for i in self.handles() {
                        let o = self.client(i).unwrap();
                        if o.whisper_player_index != -1 && cstr(&o.whisper_player_name) == &name[..] {
                            self.client_mut(i).unwrap().whisper_player_index = -1;
                            self.notify(i, NOTIFY_WHISPERMODEOFF, &[], Some(&name));
                        }
                    }
                    11
                }
                GSM_WHISPERMSG => {
                    let name = c.s(1, 10);
                    let len = c.u16(11) as usize;
                    // fix: the C++ copied len bytes into a 256-byte buffer
                    let msg = c.raw(13, len.min(256));
                    if let Some(i) = self.find_by_name(&name) {
                        self.send(i, &msg);
                    }
                    13 + len
                }
                GSM_CHATMSG => {
                    let len = c.u16(16) as usize;
                    self.chat_msg_gsm(c.u8(1), c.u32(2) as i32, &c.raw(6, 10), &c.raw(18, len));
                    18 + len
                }
                GSM_REQUEST_SUMMONPLAYER => {
                    let (name, map) = (c.s(1, 10), c.s(11, 10));
                    if let Some(i) = self.find_by_name(&name) {
                        self.request_teleport(i, 2, Some(&map), c.u16(21) as i16, c.u16(23) as i16);
                    }
                    25
                }
                GSM_REQUEST_SHUTUPPLAYER => {
                    self.gsm_request_shutup(&c.s(5, 10), c.u16(1), c.u16(3), c.u16(15), &c.raw(17, 10));
                    27
                }
                GSM_RESPONSE_SHUTUPPLAYER => {
                    let (sid, ch) = (c.u16(1), c.u16(3) as usize);
                    if sid == self.gsid {
                        let gm = c.s(5, 10);
                        if self.client(ch).is_some_and(|o| cstr(&o.char_name) == &gm[..] && o.admin_user_level > 0) {
                            self.notify(ch, NOTIFY_PLAYERSHUTUP, &[c.u16(15) as i32], Some(&c.s(17, 10)));
                        }
                    }
                    27
                }
                GSM_REQUEST_FINDCHARACTER => {
                    self.gsm_request_find_character(c.u16(1), c.u16(3), &c.s(5, 10), &c.raw(15, 10));
                    25
                }
                GSM_RESPONSE_FINDCHARACTER => {
                    let (sid, ch) = (c.u16(1), c.u16(3) as usize);
                    let (name, req) = (c.s(5, 10), c.s(15, 10));
                    let mut loc = c.raw(25, 14);
                    if sid == self.gsid {
                        if let Some(o) = self.client(ch) {
                            if o.is_checking_whisper_player && cstr(&o.whisper_player_name) == &name[..] {
                                let o = self.client_mut(ch).unwrap();
                                o.whisper_player_index = WHISPER_OTHER_SERVER;
                                o.is_checking_whisper_player = false;
                                let n = o.whisper_player_name.clone();
                                self.notify(ch, NOTIFY_WHISPERMODEON, &[], Some(&n));
                            } else {
                                if o.admin_user_level == 0 {
                                    loc.fill(0);
                                }
                                if cstr(&o.char_name) == &req[..] {
                                    self.notify2(ch, NOTIFY_PLAYERONGAME, &[], Some(&name), Some(&loc));
                                }
                            }
                        }
                    }
                    39
                }
                GSM_REQUEST_FINDFRIEND => {
                    let len = c.u16(15) as usize;
                    self.gsm_request_find_friend(c.u16(1), c.u16(3), &c.raw(5, 10), &c.raw(17, len));
                    17 + len
                }
                GSM_RESPONSE_FINDFRIEND => {
                    let (sid, ch) = (c.u16(1), c.u16(3) as usize);
                    let (req, friend) = (c.s(5, 10), c.s(15, 10));
                    if sid == self.gsid && self.client(ch).is_some_and(|o| cstr(&o.char_name) == &req[..]) {
                        self.notify2(ch, NOTIFY_FRIENDONGAME, &[], Some(&friend), Some(b""));
                    }
                    25
                }
                GSM_REQUEST_SETFORCERECALLTIME => {
                    self.force_recall_time = c.u16(1) as i16 as i32;
                    crate::log::log(&format!("(!) GSM: Force Recall Time ({})min", self.force_recall_time));
                    // fix: the C++ fell through into `default` here and dropped the stock messages after this one
                    3
                }
                GSM_SETGUILDTELEPORTLOC | GSM_SETGUILDCONSTRUCTLOC => {
                    let construct = c.u8(0) as i32 == GSM_SETGUILDCONSTRUCTLOC;
                    self.set_guild_loc(0, construct, c.u32(5) as i32, c.u32(9) as i32, c.u32(1) as i32, &c.raw(13, 10), false);
                    23
                }
                GSM_CONSTRUCTIONPOINT => {
                    self.gsm_construction_point(c.u32(1) as i32, c.u32(5) as i32);
                    9
                }
                GSM_BEGINCRUSADE => {
                    self.local_start_crusade(c.u32(1));
                    5
                }
                GSM_ENDCRUSADE => {
                    // fix: the C++ read 17 of the 18 bytes, so the stock messages after it were misread
                    self.local_end_crusade(c.u8(1) as i8 as i32);
                    18
                }
                GSM_COLLECTEDMANA => {
                    self.gsm_collected_mana(c.u16(1) as i32, c.u16(3) as i32);
                    5
                }
                GSM_GRANDMAGICLAUNCH => {
                    if c.u16(1) == 1 {
                        self.meteor_strike_msg(c.u16(3) as i8 as i32);
                    }
                    5
                }
                GSM_MIDDLEMAPSTATUS => 1 + self.gsm_middle_map_status(&r.at(cp + 1)),
                GSM_GRANDMAGICRESULT => {
                    // fix: the C++ read 19 bytes and left the strike point list (count + HPs) to be misread as
                    // more stock messages
                    let n = c.u16(19) as usize;
                    let data = c.raw(19, (n + 1) * 2);
                    let map = c.raw(1, 10);
                    let w = |o: usize| c.u16(o) as i32;
                    self.grand_magic_result(&map, w(11), w(13), w(15), w(17), n as i32, &data);
                    19 + (n + 1) * 2
                }
                // the C++ defines these two but never sends them (an unknown type ended its loop)
                GSM_STARTHELDENIAN => {
                    self.start_heldenian(None, Some((c.u16(1) as i32, c.u32(3))));
                    7
                }
                GSM_ENDHELDENIAN => {
                    let v = |o: usize| c.u16(o) as i16 as i32;
                    self.gsm_end_heldenian(v(1), v(3), v(5), v(7));
                    9
                }
                GSM_BEGINAPOCALYPSE => {
                    self.local_start_apocalypse();
                    5
                }
                // fix: both HGs send 5 bytes; the Logins relayed and skipped 18 (13 bytes of what followed)
                GSM_ENDAPOCALYPSE => {
                    self.local_end_apocalypse();
                    5
                }
                _ => break,
            };
            cp += n;
        }
    }

    /// `ChatMsgHandlerGSM`: a guild / shout / GM chat line from another server.
    fn chat_msg_gsm(&mut self, ty: u8, v1: i32, name: &[u8], text: &[u8]) {
        let mut b = vec![0u8; 22 + text.len()];
        b[..4].copy_from_slice(&MSGID_COMMAND_CHATMSG.to_le_bytes());
        b[10..20].copy_from_slice(&name[..10]);
        b[20] = ty;
        b[21..21 + text.len()].copy_from_slice(text);
        for i in self.handles() {
            let o = self.client(i).unwrap();
            // fix: the C++ stopped at the first client still logging in (break), skipping everyone after it
            if !o.is_init_complete {
                continue;
            }
            let to = match ty {
                crate::chat::CHAT_GUILD => o.guild_guid == v1 && o.guild_guid != 0,
                crate::chat::CHAT_SHOUT | crate::chat::CHAT_GM => true,
                _ => false,
            };
            if to {
                self.send(i, &b);
            }
        }
    }

    /// `GSM_RequestFindCharacter`
    fn gsm_request_find_character(&mut self, sid: u16, ch: u16, name: &[u8], req: &[u8]) {
        let Some(i) = self.find_by_name(name) else { return };
        let o = self.client(i).unwrap();
        let mut b = vec![0u8; 39];
        b[0] = GSM_RESPONSE_FINDCHARACTER as u8;
        b[1..3].copy_from_slice(&sid.to_le_bytes());
        b[3..5].copy_from_slice(&ch.to_le_bytes());
        b[5..15].copy_from_slice(&Game::pad10(name));
        b[15..25].copy_from_slice(&req[..10]);
        b[25..35].copy_from_slice(&Game::pad10(&o.map_name));
        b[35..37].copy_from_slice(&(o.x as u16).to_le_bytes());
        b[37..39].copy_from_slice(&(o.y as u16).to_le_bytes());
        self.stock_msg(&b);
    }

    /// `RequestUpdateFriendsHandler` (MSGID_REQUEST_UPDATE_FRIENDS): the friend list as up to 13 names separated by ", ".
    /// The friends on this server get a NOTIFY_FRIENDONGAME each; the others are asked on the other servers
    /// (GSM_REQUEST_FINDFRIEND), whose answers come back as GSM_RESPONSE_FINDFRIEND.
    pub fn request_update_friends(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        let me = Game::pad10(&c.char_name);
        let mut t = crate::util::StrTok::new(d.get(6..).unwrap_or(&[]), b", ");
        let mut rest = Vec::new();
        for _ in 0..13 {
            let Some(tok) = t.get() else { break };
            // the C++ `continue`d on an empty token without taking the next one: it burnt the rest of the 13 turns
            if tok.is_empty() {
                break;
            }
            match self.find_by_name(&Game::pad10(&tok)) {
                Some(i) => {
                    let name = Game::pad10(&self.client(i).unwrap().char_name);
                    self.notify2(h, NOTIFY_FRIENDONGAME, &[], Some(&name), Some(b""));
                }
                None => {
                    // fix: the C++ strcat'ed the names into a 256-byte buffer (and sent them in another one) unchecked;
                    // the message (17 bytes + names) stays within 256
                    if rest.len() + tok.len() + 1 > 256 - 17 {
                        break;
                    }
                    rest.extend_from_slice(&tok);
                    rest.push(b' ');
                }
            }
        }
        if !rest.is_empty() {
            let mut b = vec![GSM_REQUEST_FINDFRIEND as u8];
            b.extend_from_slice(&self.gsid.to_le_bytes());
            b.extend_from_slice(&(h as u16).to_le_bytes());
            b.extend_from_slice(&me);
            b.extend_from_slice(&(rest.len() as u16).to_le_bytes());
            b.extend_from_slice(&rest);
            self.stock_msg(&b);
        }
    }

    /// `GSM_RequestFindFriend`: up to 13 names separated by ", ".
    fn gsm_request_find_friend(&mut self, sid: u16, ch: u16, req: &[u8], names: &[u8]) {
        let mut t = crate::util::StrTok::new(names, b", ");
        for _ in 0..13 {
            let Some(tok) = t.get() else { break };
            if tok.is_empty() {
                break;
            }
            let f = Game::pad10(&tok);
            if self.find_by_name(&f).is_none() {
                continue;
            }
            let mut b = vec![0u8; 25];
            b[0] = GSM_RESPONSE_FINDFRIEND as u8;
            b[1..3].copy_from_slice(&sid.to_le_bytes());
            b[3..5].copy_from_slice(&ch.to_le_bytes());
            b[5..15].copy_from_slice(&req[..10]);
            b[15..25].copy_from_slice(&f);
            self.stock_msg(&b);
        }
    }

    /// `GSM_RequestShutupPlayer`
    fn gsm_request_shutup(&mut self, player: &[u8], sid: u16, ch: u16, minutes: u16, gm: &[u8]) {
        let Some(i) = self.find_by_name(player) else { return };
        let mut b = vec![0u8; 27];
        b[0] = GSM_RESPONSE_SHUTUPPLAYER as u8;
        b[1..3].copy_from_slice(&sid.to_le_bytes());
        b[3..5].copy_from_slice(&ch.to_le_bytes());
        b[5..15].copy_from_slice(&Game::pad10(gm));
        b[15..17].copy_from_slice(&minutes.to_le_bytes());
        b[17..27].copy_from_slice(&Game::pad10(player));
        self.client_mut(i).unwrap().time_left_shutup = minutes as i32 * 20;
        self.notify(i, NOTIFY_PLAYERSHUTUP, &[minutes as i32], Some(player));
        self.stock_msg(&b);
    }

    /// `GSM_REQUEST_FINDCHARACTER` for /to and /fi: [type][GSID][client][name 10][requester 10]
    pub fn stock_find_character(&mut self, h: usize, name: &[u8]) {
        let mut b = vec![0u8; 25];
        b[0] = GSM_REQUEST_FINDCHARACTER as u8;
        b[1..3].copy_from_slice(&self.gsid.to_le_bytes());
        b[3..5].copy_from_slice(&(h as u16).to_le_bytes());
        b[5..15].copy_from_slice(&Game::pad10(name));
        b[15..25].copy_from_slice(&Game::pad10(&self.client(h).unwrap().char_name));
        self.stock_msg(&b);
    }

    /// `GSM_CHATMSG` [type][chat type][int][name 10][len][text]
    pub fn stock_chat(&mut self, ty: u8, v: i32, name: &[u8], text: &[u8]) {
        let mut b = vec![GSM_CHATMSG as u8, ty];
        b.extend_from_slice(&v.to_le_bytes());
        b.extend_from_slice(&Game::pad10(name));
        b.extend_from_slice(&(text.len() as u16).to_le_bytes());
        b.extend_from_slice(text);
        self.stock_msg(&b);
    }
}
