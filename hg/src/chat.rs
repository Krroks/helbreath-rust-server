//! `ChatMsgHandler` (HG.cpp ~6111) and `ToggleWhisperPlayer` (/to). Message: [id][type][x][y][name 10][mode][text].
//! The HG rewrites type = sender handle and byte 20 = send mode, then forwards the same bytes.
//! '^' guild and '%' GM lines and whispers to other servers go through the gate-server stock (gsm.rs). GM commands: later.
use crate::consts::*;
use crate::game::Game;

use crate::util::{cstr, StrTok, W};

const CHAT_NORMAL: u8 = 0;
pub const CHAT_GUILD: u8 = 1;
pub const CHAT_SHOUT: u8 = 2;
pub const CHAT_GM: u8 = 10;
const CHAT_NATIONSHOUT: u8 = 3;
const CHAT_PARTY: u8 = 4;
const CHAT_WHISPER: u8 = 20;

/// MSGID_EVENT_TYPING: `[id u32][type u16][name 10][state u8]` (state 1 = typing, 0 = stopped).
pub fn encode_typing(name: &[u8], typing: bool) -> Vec<u8> {
    let mut b = vec![0u8; 17];
    let mut w = W(&mut b);
    w.u32(0, MSGID_EVENT_TYPING);
    w.bytes(6, &name[..name.len().min(10)]);
    w.u8(16, typing as u8);
    b
}

impl Game {
    /// v8.00 (no C++ original): a player starts or stops typing a chat line; the players within sight get a bubble over its head.
    /// A client repeats state 1 every few seconds while it types, so one that vanishes is forgotten by the others on their own.
    pub fn request_typing(&mut self, h: usize, d: &[u8]) {
        let now = self.now();
        let Some(c) = self.client_mut(h) else { return };
        if !c.is_init_complete || now.wrapping_sub(c.typing_time) < 1000 {
            return;
        }
        c.typing_time = now;
        let (m, x, y, name) = (c.map_index, c.x, c.y, crate::util::cstr(&c.char_name).to_vec());
        let b = encode_typing(&name, crate::util::Rd::new(d).u8(6) != 0);
        for o in self.handles() {
            if o != h && self.client(o).is_some_and(|c| c.is_init_complete && c.map_index == m && (c.x - x).abs() <= VIEW_NEAR_X && (c.y - y).abs() <= VIEW_NEAR_Y) {
                self.send(o, &b);
            }
        }
    }
    pub fn chat_msg(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || d.len() > 83 + 30 || c.time_left_shutup > 0 {
            return;
        }
        // fix: the C++ wrote pData[dwMsgSize-1] and read pData+21 without checking the message is that long
        if d.len() < 22 {
            return;
        }
        let name = cstr(&c.char_name);
        if d.get(10..10 + name.len()) != Some(name) {
            return;
        }
        if c.is_observer_mode && c.admin_user_level == 0 {
            return;
        }
        self.inc_player_activity(h);
        let c = self.client(h).unwrap();
        let mut d = d.to_vec();
        if c.admin_user_level > 0 {
            let txt = format!("GM Chat({}):\"{}\"", crate::util::l1(&c.char_name), crate::util::l1(cstr(&d[21..d.len() - 1])));
            self.send_ls_plain(MSGID_GAMEMASTERLOG, txt.as_bytes());
        }
        let c = self.client(h).unwrap();
        let (level, dead) = (c.level, c.is_dead());
        let mut mode = 0u8;
        match d[21] {
            b'$' => {
                d[21] = b' ';
                mode = CHAT_PARTY;
            }
            b'@' => {
                d[21] = b' ';
                if level > 1 {
                    mode = CHAT_GUILD;
                }
            }
            b'!' => {
                d[21] = b' ';
                if level > 10 && !dead {
                    mode = CHAT_SHOUT;
                }
            }
            b'~' => {
                d[21] = b' ';
                if level > 1 && !dead {
                    mode = CHAT_NATIONSHOUT;
                }
            }
            b'%' => {
                d[21] = b' ';
                let text = cstr(&d[21..d.len() - 1]).to_vec();
                let c = self.client(h).unwrap();
                if text.len() < 90 && c.admin_user_level > 0 {
                    let name = c.char_name.clone();
                    self.stock_chat(CHAT_GM, 0, &name, &text);
                }
                if self.client(h).unwrap().admin_user_level > 0 {
                    mode = CHAT_GM;
                }
            }
            b'^' => {
                d[21] = b' ';
                let text = cstr(&d[21..d.len() - 1]).to_vec();
                let c = self.client(h).unwrap();
                let (rank, guid, name) = (c.guild_rank, c.guild_guid, c.char_name.clone());
                if rank != -1 && level > 10 && !dead {
                    mode = CHAT_GUILD;
                    // fix: the C++ relayed the line to the other servers even when it did not send it here
                    // (level 10 or less, dead)
                    if text.len() < 90 {
                        self.stock_chat(CHAT_GUILD, guid, &name, &text);
                    }
                }
            }
            b'/' => {
                let cmd = cstr(&d[21..]).to_vec();
                self.chat_command(h, &cmd);
                return;
            }
            _ => {}
        }
        let n = d.len();
        d[n - 1] = 0;
        // confusion magic garbles the text: magic layer
        let c = self.client(h).unwrap();
        if mode == 0 && c.whisper_player_index != -1 {
            mode = CHAT_WHISPER;
            if d[21] == b'#' || c.time_left_shutup > 0 {
                mode = CHAT_NORMAL;
            }
        }
        d[4..6].copy_from_slice(&(h as u16).to_le_bytes());
        d[20] = mode;
        let c = self.client(h).unwrap();
        let (m, x, y, side, party, is_gm) = (c.map_index, c.x, c.y, c.side, c.party_id, c.is_gm());
        let guild = Game::pad20(&c.guild_name);
        let chat_off = self.maps[m].is_chat_disabled;
        let (crusade, neutral) = (self.cr.mode, self.client(h).unwrap().is_neutral());
        if mode != CHAT_WHISPER {
            for i in self.handles() {
                let o = self.client(i).unwrap();
                if !o.is_init_complete {
                    continue;
                }
                if chat_off && i != h && !is_gm && !o.is_gm() {
                    continue;
                }
                // in a crusade the normal and shout lines do not reach the enemy side
                let enemy = crusade && !neutral && !o.is_neutral() && o.side != side;
                let to = match mode {
                    CHAT_NORMAL => !enemy && o.map_index == m && (o.x - x).abs() <= VIEW_NEAR_X && (o.y - y).abs() <= VIEW_NEAR_Y,
                    CHAT_GUILD => Game::pad20(&o.guild_name) == guild && !o.guild_name.starts_with(b"NONE"),
                    CHAT_SHOUT => !enemy,
                    CHAT_GM => true,
                    CHAT_NATIONSHOUT => o.side == side,
                    CHAT_PARTY => o.party_id != 0 && o.party_id == party,
                    _ => false,
                };
                if to {
                    self.send(i, &d);
                    self.bots_hear(h, i, mode, &d);
                }
            }
        } else {
            let wi = c.whisper_player_index;
            let target_gm = wi > 0 && self.client(wi as usize).is_some_and(|t| t.is_gm());
            if !chat_off || is_gm || target_gm {
                self.send(h, &d);
                let wname = c.whisper_player_name.clone();
                if wi == crate::gsm::WHISPER_OTHER_SERVER {
                    let mut b = vec![GSM_WHISPERMSG as u8];
                    b.extend_from_slice(&Game::pad10(&wname));
                    b.extend_from_slice(&(d.len() as u16).to_le_bytes());
                    b.extend_from_slice(&d);
                    self.stock_msg(&b);
                } else if let Some(t) = (wi > 0).then(|| self.client(wi as usize)).flatten() {
                    if cstr(&wname) == cstr(&t.char_name) {
                        self.send(wi as usize, &d);
                        self.bots_hear(h, wi as usize, mode, &d);
                    }
                }
            }
        }
    }

    pub fn pad20(s: &[u8]) -> [u8; 20] {
        let mut a = [0u8; 20];
        let s = cstr(s);
        let n = s.len().min(20);
        a[..n].copy_from_slice(&s[..n]);
        a
    }

    /// `ToggleWhisperPlayer`
    pub fn toggle_whisper(&mut self, h: usize, cmd: &[u8]) {
        let mut t = StrTok::new(cmd, b"= \t\n");
        let _ = t.get();
        let Some(tok) = t.get() else {
            let c = self.client_mut(h).unwrap();
            c.whisper_player_index = -1;
            c.whisper_player_name.clear();
            c.is_checking_whisper_player = false;
            self.notify(h, NOTIFY_WHISPERMODEOFF, &[], Some(b""));
            return;
        };
        let name = Game::pad10(&tok);
        self.client_mut(h).unwrap().whisper_player_index = -1;
        let found = self.handles().into_iter().find(|&i| Game::pad10(&self.client(i).unwrap().char_name) == name);
        match found {
            Some(i) if i == h => {}
            Some(i) => {
                let c = self.client_mut(h).unwrap();
                c.whisper_player_index = i as i32;
                c.whisper_player_name = cstr(&name).to_vec();
                let n = c.whisper_player_name.clone();
                self.notify(h, NOTIFY_WHISPERMODEON, &[], Some(&n));
            }
            None => {
                self.stock_find_character(h, &name);
                let c = self.client_mut(h).unwrap();
                c.whisper_player_name = cstr(&name).to_vec();
                c.is_checking_whisper_player = true;
            }
        }
    }

    /// `CheckAndNotifyPlayerConnection` (/fi name)
    pub fn check_and_notify_player_connection(&mut self, h: usize, cmd: &[u8]) {
        let mut t = StrTok::new(cmd, b"= 	
");
        let _ = t.get();
        let Some(tok) = t.get() else { return };
        let name = Game::pad10(&tok);
        let gm = self.client(h).unwrap().admin_user_level > 0;
        if let Some(i) = self.handles().into_iter().find(|&i| Game::pad10(&self.client(i).unwrap().char_name) == name) {
            let o = self.client(i).unwrap();
            let mut loc = vec![0u8; 14];
            if gm {
                loc[..10].copy_from_slice(&Game::pad10(&o.map_name));
                loc[10..12].copy_from_slice(&(o.x as u16).to_le_bytes());
                loc[12..14].copy_from_slice(&(o.y as u16).to_le_bytes());
            }
            let n = o.char_name.clone();
            self.notify2(h, NOTIFY_PLAYERONGAME, &[], Some(&n), Some(&loc));
            return;
        }
        self.stock_find_character(h, &name);
    }
}

#[cfg(test)]
mod typing_tests {
    use super::*;

    #[test]
    fn typing_event_layout() {
        let b = encode_typing(b"Leslietat", true);
        assert_eq!(b.len(), 17);
        assert_eq!(u32::from_le_bytes(b[0..4].try_into().unwrap()), MSGID_EVENT_TYPING);
        assert_eq!(&b[6..15], b"Leslietat");
        assert_eq!((b[15], b[16]), (0, 1));
        assert_eq!(encode_typing(b"X", false)[16], 0);
    }
}
