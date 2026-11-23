//! Server events with no C++ original (they come from the v8.00 client): Double EXP and Double EK (enemy kills).
//! A GM starts one for N minutes (`/doubleexp [min]`, `/doubleek [min]`, 0 stops it; gm.rs); while it runs `get_exp` doubles the
//! experience of monster kills and `enemy_kill_reward` doubles the EK it adds. The clients get a notice when it starts or ends and
//! NOTIFY_SERVEREVENTS (seconds left of each event, 0 = off) on entering and whenever the state changes: they draw an icon with it.
//! New characters also start with both events for `NEWBIE_MINUTES` of play of their own (`Client::newbie_min`).
use crate::consts::*;
use crate::game::Game;

pub const EVENT_DOUBLE_EXP: usize = 0;
pub const EVENT_DOUBLE_EK: usize = 1;
const NAMES: [&str; 2] = ["Double Experience Event", "Double Enemy Kill Event"];

/// Minutes of play with both events that a new character starts with (the Login's INSERT writes this into LeftShutupTime's top byte).
pub const NEWBIE_MINUTES: i32 = 120;

/// LeftShutupTime is saved as one u32: the shut-up time in its low 24 bits, the starter bonus minutes (0..127) above them. That keeps
/// the blob layout and the DB schema as they are; a C++ HG reading the column would see a huge shut-up time.
pub fn split_shutup(v: u32) -> (i32, i32) {
    ((v & 0x00FF_FFFF) as i32, ((v >> 24) & 0x7F) as i32)
}

/// v8.00 client toggles (`/typing`, `/lowhealth`, `/itemnames`) belong to the character: bits 24-26 of the `LeftPopTime` column
/// (the rating time is at most 20 min, so the top bits were free; the same trick as the starter bonus in `LeftShutupTime`, no schema
/// change). The column keeps the bits as "differs from the default" for the first two, so old rows (0) read typing ON, low-health ON.
pub const UI_TYPING: u8 = 1;
pub const UI_LOWHEALTH: u8 = 2;
pub const UI_ITEMNAMES: u8 = 4;
pub const UI_DEFAULT: u8 = UI_TYPING | UI_LOWHEALTH;

pub fn split_rating(v: u32) -> (i32, u8) {
    ((v & 0x00FF_FFFF) as i32, (((v >> 24) & 7) as u8) ^ UI_DEFAULT)
}

pub fn join_rating(rating: i32, ui: u8) -> u32 {
    (rating.max(0) as u32 & 0x00FF_FFFF) | (((ui & 7) ^ UI_DEFAULT) as u32) << 24
}

pub fn join_shutup(shutup: i32, newbie_min: i32) -> u32 {
    (shutup.max(0) as u32 & 0x00FF_FFFF) | ((newbie_min.clamp(0, 127) as u32) << 24)
}

#[derive(Default)]
pub struct Events {
    /// game time (`Game::now`) at which each event ends; 0 = not running
    pub until: [u32; 2],
}

/// Whole seconds left until `until` (rounded up, so a running event never shows 0); 0 when off or over.
pub fn secs_left(until: u32, now: u32) -> i32 {
    if until == 0 {
        return 0;
    }
    let d = until.wrapping_sub(now) as i32;
    if d > 0 {
        (d + 999) / 1000
    } else {
        0
    }
}

impl Game {
    pub fn event_active(&self, ev: usize) -> bool {
        secs_left(self.events.until[ev], self.now()) > 0
    }

    /// The event counts for this player: a running GM event, or the player's own starter bonus.
    pub fn event_on(&self, h: usize, ev: usize) -> bool {
        self.event_active(ev) || self.client(h).is_some_and(|c| c.newbie_min > 0)
    }

    /// NOTIFY_SERVEREVENTS to one player: per event, the longer of the GM event and the starter bonus.
    pub fn send_event_state(&mut self, h: usize) {
        let now = self.now();
        let newbie = self.client(h).map_or(0, |c| c.newbie_min * 60);
        let v = [
            secs_left(self.events.until[EVENT_DOUBLE_EXP], now).max(newbie),
            secs_left(self.events.until[EVENT_DOUBLE_EK], now).max(newbie),
        ];
        self.notify(h, NOTIFY_SERVEREVENTS, &v, None);
    }

    /// On entering the game: the icons, and a welcome for a character with the starter bonus.
    pub fn event_enter(&mut self, h: usize) {
        if let Some(c) = self.client_mut(h) {
            c.newbie_time = 0; // the first minute counts from the first tick
        }
        self.send_event_state(h);
        let ui = self.client(h).map_or(UI_DEFAULT, |c| c.ui_flags);
        self.notify(h, NOTIFY_CLIENTFLAGS, &[ui as i32], None);
        let mins = self.client(h).map_or(0, |c| c.newbie_min);
        if mins > 0 {
            let t = format!("Welcome! Double EXP and Double EK for your first {mins} minutes.");
            self.notify(h, NOTIFY_NOTICEMSG, &[], Some(t.as_bytes()));
        }
    }

    /// MSGID_REQUEST_CLIENTFLAGS `[id u32][type u16][flags u8]`: the player toggled one of /typing, /lowhealth, /itemnames; the new
    /// state is kept with the character (written with the next save).
    pub fn request_client_flags(&mut self, h: usize, d: &[u8]) {
        if d.len() < 7 {
            return;
        }
        if let Some(c) = self.client_mut(h) {
            c.ui_flags = d[6] & 7;
        }
    }

    /// Takes a minute off the starter bonus of a player in the game (called from the client timer).
    pub fn newbie_tick(&mut self, h: usize, now: u32) {
        let Some(c) = self.client_mut(h) else { return };
        if c.newbie_min <= 0 {
            return;
        }
        if c.newbie_time == 0 {
            c.newbie_time = now;
            return;
        }
        if now.wrapping_sub(c.newbie_time) < 60_000 {
            return;
        }
        c.newbie_time = now;
        c.newbie_min -= 1;
        let over = c.newbie_min == 0;
        // the client counts the seconds itself: tell it when the bonus is over, and every 10 minutes to keep the clocks close
        if over {
            self.notify(h, NOTIFY_NOTICEMSG, &[], Some(b"Your starter bonus (Double EXP and Double EK) has ended."));
        }
        if over || self.client(h).is_some_and(|c| c.newbie_min % 10 == 0) {
            self.send_event_state(h);
        }
    }

    fn broadcast_events(&mut self, notice: &str) {
        for h in self.handles() {
            if self.client(h).is_some_and(|c| c.is_init_complete) {
                self.notify(h, NOTIFY_NOTICEMSG, &[], Some(notice.as_bytes()));
                self.send_event_state(h);
            }
        }
    }

    /// Starts event `ev` for `minutes` (or stops it with 0 or less) and tells everybody.
    pub fn event_set(&mut self, ev: usize, minutes: i32) {
        if minutes <= 0 {
            self.events.until[ev] = 0;
            self.broadcast_events(&format!("{} has ended!", NAMES[ev]));
        } else {
            self.events.until[ev] = self.now().wrapping_add(minutes as u32 * 60_000).max(1);
            self.broadcast_events(&format!("{} has started!", NAMES[ev]));
        }
    }

    /// Ends the events whose time is up (4 s timer).
    pub fn events_tick(&mut self) {
        let now = self.now();
        for ev in 0..2 {
            if self.events.until[ev] != 0 && secs_left(self.events.until[ev], now) == 0 {
                self.event_set(ev, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rating_and_ui_flags_share_a_u32() {
        assert_eq!(split_rating(0), (0, UI_DEFAULT)); // an old row: typing and low-health warning on, item names off
        assert_eq!(split_rating(join_rating(1200, UI_ITEMNAMES)), (1200, UI_ITEMNAMES));
        assert_eq!(split_rating(join_rating(0, 0)), (0, 0));
        assert_eq!(join_rating(1200, UI_DEFAULT), 1200); // defaults leave the column as it was
        assert!(join_rating(0, UI_ITEMNAMES) <= i32::MAX as u32); // fits the signed DB column
    }

    #[test]
    fn shutup_and_starter_bonus_share_a_u32() {
        assert_eq!(split_shutup(join_shutup(60, NEWBIE_MINUTES)), (60, NEWBIE_MINUTES));
        assert_eq!(split_shutup(join_shutup(0, 0)), (0, 0));
        assert_eq!(split_shutup(0x00FF_FFFF), (0x00FF_FFFF, 0)); // a plain old shut-up time has no bonus
        assert_eq!(join_shutup(5, 200), 5 | (127 << 24)); // clamped to what fits the signed DB column
        assert!(join_shutup(0, NEWBIE_MINUTES) <= i32::MAX as u32);
        assert_eq!(join_shutup(0, NEWBIE_MINUTES), 2_013_265_920); // what the Login's INSERT writes
    }

    #[test]
    fn seconds_left() {
        assert_eq!(secs_left(0, 5_000), 0); // off
        assert_eq!(secs_left(65_000, 5_000), 60);
        assert_eq!(secs_left(5_001, 5_000), 1); // a running event never reads 0
        assert_eq!(secs_left(5_000, 5_000), 0); // over
        assert_eq!(secs_left(4_000, 5_000), 0);
        assert_eq!(secs_left(500, u32::MAX - 500), 2); // the game clock wraps like a DWORD
    }
}
