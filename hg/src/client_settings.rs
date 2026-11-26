//! Rust only: the client's own settings (volume, F-key shortcuts, display toggles, mute list) kept in the database, because the
//! browser build forgets everything on reload. The client sends them as text lines (`key=number`, `M=muted name`) when they change
//! (MSGID_REQUEST_SAVECLIENTSETTINGS); this server relays them to the Login (table `client_settings`, one row per character), which
//! sends them back after the player data; the client gets them on entering (MSGID_NOTIFY_CLIENTSETTINGS). The server does not read them.
use crate::consts::*;
use crate::game::Game;
use crate::util::{cstr, Rd};

pub const MAX_SETTINGS: usize = 2000;

/// Text lines only: no NUL or other control byte but the line feed, and not longer than the client may send.
pub fn valid(t: &[u8]) -> bool {
    t.len() <= MAX_SETTINGS && t.iter().all(|&c| c == b'\n' || c >= 0x20)
}

/// Hex on the Login link and in the database, so nothing of the text can reach a query.
pub fn hex(t: &[u8]) -> String {
    t.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(h: &[u8]) -> Option<Vec<u8>> {
    if h.len() % 2 != 0 || !h.iter().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    h.chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok()).collect()
}

/// MSGID_NOTIFY_CLIENTSETTINGS: `[id u32][type u16][length u16][text]`.
pub fn encode(text: &[u8]) -> Vec<u8> {
    let mut b = MSGID_NOTIFY_CLIENTSETTINGS.to_le_bytes().to_vec();
    b.extend_from_slice(&(MSGTYPE_CONFIRM as u16).to_le_bytes());
    b.extend_from_slice(&(text.len() as u16).to_le_bytes());
    b.extend_from_slice(text);
    b
}

impl Game {
    /// `[id u32][type u16][text]` from a player in the game, at most one a second (the client waits two between two sends).
    pub fn request_save_client_settings(&mut self, h: usize, d: &[u8]) {
        let now = self.now();
        let Some(text) = d.get(6..) else { return };
        let Some(c) = self.client_mut(h) else { return };
        if !c.is_init_complete || !valid(text) || now.wrapping_sub(c.settings_time) < 1000 {
            return;
        }
        c.settings_time = now;
        c.client_settings = Some(text.to_vec());
        let mut p = Game::pad10(&c.char_name).to_vec();
        p.extend_from_slice(hex(text).as_bytes());
        p.push(0);
        self.send_ls_plain(MSGID_CLIENTSETTINGS_SAVE, &p);
    }

    /// MSGID_RESPONSE_CLIENTSETTINGS from the Login: `[id u32][type u16][character 10][hex, NUL]`, empty text = nothing saved yet.
    pub fn login_client_settings(&mut self, d: &[u8]) {
        if d.len() < 16 {
            return;
        }
        let name = cstr(&Rd::new(d).s(6, 10)).to_vec();
        let Some(text) = unhex(cstr(&d[16..])).filter(|t| valid(t)) else { return };
        let Some(h) = self.handles().into_iter().find(|&h| self.client(h).is_some_and(|c| cstr(&c.char_name) == name.as_slice())) else { return };
        let c = self.client_mut(h).unwrap();
        c.client_settings = Some(text);
        if c.is_init_complete {
            self.send_client_settings(h);
        }
    }

    /// On entering the game. Nothing is sent while the Login has not answered, so the client never overwrites what it has not seen.
    pub fn send_client_settings(&mut self, h: usize) {
        if let Some(t) = self.client(h).and_then(|c| c.client_settings.as_deref()) {
            let b = encode(t);
            self.send(h, &b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_checks_and_hex() {
        let t = b"SoundVolume=40\nShortCut1=103\nM=Bad Guy\n";
        assert!(valid(t) && valid(b""));
        assert!(!valid(b"a=1\0") && !valid(b"a=\x07") && !valid(&[b'a'; MAX_SETTINGS + 1]));
        assert_eq!(unhex(hex(t).as_bytes()).unwrap(), t);
        assert_eq!(unhex(b""), Some(Vec::new()));
        assert!(unhex(b"abc").is_none() && unhex(b"zz").is_none() && unhex(b"';").is_none());
    }

    #[test]
    fn notify_layout() {
        let b = encode(b"a=1\n");
        assert_eq!(b.len(), 12);
        assert_eq!(u32::from_le_bytes(b[..4].try_into().unwrap()), MSGID_NOTIFY_CLIENTSETTINGS);
        assert_eq!((u16::from_le_bytes([b[6], b[7]]), &b[8..]), (4, &b"a=1\n"[..]));
    }
}
