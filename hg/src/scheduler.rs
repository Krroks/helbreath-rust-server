//! The event scheduler (HG.cpp ~35768-35930): bReadSchedulerConfigFile (../configs/Schedule.cfg, lines
//! `event = CTR|DS|Crusade day hour minute`, day 0 = sunday) and Scheduler (the 4 s timer): an event starts at its
//! minute, with NOTIFY_EVENTSTARTING 2 hours, EVENTSTARTING2 30 minutes and EVENTSTARTING3 10 minutes before.
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::util::{atoi, StrTok};

pub const ET_CRUSADE: i32 = 3;
const MAXSCHEDULE: usize = 10;
const WEEK: i32 = 7 * 24 * 60;

/// what a schedule entry last did (the C++ EventStatus)
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Status {
    /// the C++ zero-initialised its entries to ES_ANNOUNCED_ONCE
    #[default]
    Once,
    Twice,
    Thrice,
    Started,
    Ended,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Schedule {
    pub ty: i32,
    /// minute of the week (day 0 = sunday)
    pub at: i32,
    pub status: Status,
}

/// `bReadSchedulerConfigFile`: at most MAXSCHEDULE events; an unknown type skips its line.
pub fn parse_schedule(txt: &[u8]) -> Vec<Schedule> {
    let mut out = Vec::new();
    let mut t = StrTok::new(txt, b"= \t\n");
    while let Some(k) = t.get() {
        if !k.starts_with(b"event") {
            continue;
        }
        let Some(ty) = t.get() else { break };
        let ty = if ty.starts_with(b"CTR") {
            crate::astoria::ET_CAPTURE
        } else if ty.starts_with(b"DS") {
            crate::astoria::ET_DESTROY_SHIELD
        } else if ty.starts_with(b"Crusade") {
            ET_CRUSADE
        } else {
            continue;
        };
        if out.len() >= MAXSCHEDULE {
            log("(!) WARNING! Too many events in scheduler!");
            break;
        }
        let v: Vec<i32> = (0..3).map(|_| atoi(&t.get().unwrap_or_default())).collect();
        out.push(Schedule { ty, at: (v[0] * 24 + v[1]) * 60 + v[2], status: Status::Once });
    }
    out
}

/// What the scheduler does with an entry at `now` (minute of the week): the notify to send or the start.
pub fn schedule_step(s: &mut Schedule, now: i32) -> Option<i32> {
    // fix: the C++ added 2 hours / 30 / 10 minutes to the clock without carrying past midnight (hour 24, 25), so an
    // event at 0:00-1:59 got no 2-hour notice and the 30 / 10 minute ones failed across midnight
    let left = (s.at - now).rem_euclid(WEEK);
    if left == 0 {
        s.status = Status::Started;
        return Some(0);
    }
    let (want, st, n) = match left {
        120 => (true, Status::Once, NOTIFY_EVENTSTARTING),
        30 => (true, Status::Twice, NOTIFY_EVENTSTARTING2),
        10 => (true, Status::Thrice, NOTIFY_EVENTSTARTING3),
        _ => (false, Status::Ended, 0),
    };
    if !want {
        s.status = Status::Ended;
        return None;
    }
    // fix: the C++ set ES_ENDED on any tick that announced nothing, including the next tick of the same minute,
    // so every notice went out again every other tick (about 7 times in its minute)
    if s.status == st {
        return None;
    }
    s.status = st;
    Some(n)
}

impl Game {
    /// OnStartGameSignal: ../configs/Schedule.cfg
    pub fn scheduler_init(&mut self) {
        match std::fs::read("../configs/Schedule.cfg") {
            Ok(raw) => self.schedules = parse_schedule(&crate::util::text_mode(&raw)),
            Err(_) => log("(ERROR) Cannot open Scheduler file."),
        }
    }

    /// `Scheduler` (4 s timer)
    pub fn scheduler(&mut self) {
        if self.schedules.is_empty() || self.cr.mode || self.ast.ev.is_some() {
            return;
        }
        let t = crate::chrono_now();
        let now = ((t.weekday * 24 + t.hour) * 60 + t.minute) as i32;
        for i in 0..self.schedules.len() {
            let ty = self.schedules[i].ty;
            match schedule_step(&mut self.schedules[i], now) {
                Some(0) => {
                    if ty == ET_CRUSADE {
                        self.global_start_crusade();
                    } else {
                        self.astoria_event_start(ty);
                    }
                }
                Some(n) => {
                    for h in self.handles() {
                        if self.client(h).unwrap().is_init_complete {
                            self.notify(h, n, &[ty], None);
                        }
                    }
                }
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        let s = parse_schedule(b"game-server-name = X\nschedule = 3 10 55\nevent = CTR 3 10 55\nevent = Foo 1 1 1\nevent = Crusade 0 0 5\n");
        assert_eq!(s.len(), 2, "the user's `schedule =` lines are not events; unknown types are skipped");
        assert_eq!((s[0].ty, s[0].at), (1, (3 * 24 + 10) * 60 + 55));
        assert_eq!((s[1].ty, s[1].at), (ET_CRUSADE, 5));
    }

    #[test]
    fn announcements() {
        // crusade on sunday 00:05: 2 hours before is saturday 22:05, across the week's end
        let mut s = Schedule { ty: ET_CRUSADE, at: 5, status: Status::Ended };
        let sat = |h: i32, m: i32| (6 * 24 + h) * 60 + m;
        assert_eq!(schedule_step(&mut s, sat(22, 5)), Some(NOTIFY_EVENTSTARTING));
        assert_eq!(schedule_step(&mut s, sat(22, 5)), None, "once per minute");
        assert_eq!(schedule_step(&mut s, sat(22, 5)), None);
        assert_eq!(schedule_step(&mut s, sat(23, 35)), Some(NOTIFY_EVENTSTARTING2));
        assert_eq!(schedule_step(&mut s, sat(23, 50)), None);
        assert_eq!(schedule_step(&mut s, sat(23, 55)), Some(NOTIFY_EVENTSTARTING3));
        assert_eq!(schedule_step(&mut s, 5), Some(0));
    }
}
