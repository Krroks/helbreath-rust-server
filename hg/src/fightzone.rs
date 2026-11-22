//! Fight zone reservations and tickets (HG.cpp): FightzoneReserveHandler (MSGID_REQUEST_FIGHTZONE_RESERVE: 1500
//! gold for fight zone N, on alternating days per side, until 5 minutes before the end of the 2-hour slot, one per
//! player), FightzoneReserveProcessor (every 2 hours on the clock: the player reservations are cleared),
//! GetFightzoneTicketHandler (COMMONTYPE_REQ_GETOCCUPYFIGHTZONETICKET: one ArenaTicket of the reserved zone, dated
//! the end of the slot, out of 50), AdminOrder_ReserveFightzone (/reservefightzone [n]: toggles the fight zone
//! forced recall and reserves / frees zone n for GMs) and AdminOrder_GetFightzoneTicket (/getticket: three tickets).
use crate::client::Item;
use crate::consts::*;
use crate::game::Game;
use crate::log::{log, log_file};
use crate::util::{atoi, cstr, l1, StrTok};

/// m_iFightZoneReserve: 0 free, -1 reserved by a GM, else the reserving client's handle
#[derive(Debug, Default)]
pub struct FightZones {
    pub reserve: [i32; MAXFIGHTZONE as usize],
    /// m_iFightzoneNoForceRecall: 1 lets players stay in fight zones
    pub no_force_recall: i32,
    /// m_dwCanFightzoneReserveTime
    pub reserve_time: u32,
}

/// `RecentDisconnect`: a non-GM who logged out inside a fight zone, with the times (up to 3) of those logouts
#[derive(Debug, Default, Clone)]
pub struct RecentDc {
    pub name: Vec<u8>,
    pub times: [u32; 3],
    pub count: i32,
}

/// The reservation checks of FightzoneReserveHandler: the result sent (1 = reserved).
pub fn reserve_result(t: &crate::Now, zone_taken: bool, gold: u32, side: i32, zone: i32, has_zone: bool) -> i32 {
    // in 1/20 s: the slot ends on an even hour
    let left = 2 * 20 * 60 - ((t.hour as i32 % 2) * 20 * 60 + t.minute as i32 * 20) - 5 * 20;
    if left <= 0 {
        0
    } else if zone_taken {
        -1
    } else if gold < 1500 {
        -2
    } else if (t.day as i32 + side + zone) % 2 != 0 {
        -3
    } else if has_zone {
        -4
    } else {
        1
    }
}

/// m_iReserveTime: MMDDHH of the slot's end (the next even hour)
pub fn reserve_stamp(t: &crate::Now) -> i32 {
    (t.month * 10000 + t.day * 100 + t.hour) as i32 + if t.hour % 2 != 0 { 1 } else { 2 }
}

impl Game {
    /// The m_recentDCs part of DeleteClient (a saved logout): a non-GM leaving from a fight zone is remembered (3 logouts
    /// within 10 minutes: the 4th puts him in town); the next entry within 18 s of one of them is not recalled.
    pub fn fz_record_disconnect(&mut self, h: usize) {
        let now = self.now();
        let Some(c) = self.client(h) else { return };
        if !self.maps.get(c.map_index).is_some_and(|m| m.is_fight_zone) || c.is_gm() {
            return;
        }
        let name = cstr(&c.char_name).to_vec();
        if let Some(i) = self.recent_dcs.iter().position(|r| r.name == name) {
            if self.recent_dcs[i].count >= 3 {
                self.recent_dcs.remove(i);
                let c = self.client_mut(h).unwrap();
                c.map_name = crate::client::SIDE_MAP[c.side.clamp(0, 3) as usize].to_vec();
                (c.x, c.y) = (-1, -1);
            } else {
                let r = &mut self.recent_dcs[i];
                r.count += 1;
                if let Some(t) = r.times.iter_mut().find(|t| **t == 0) {
                    *t = now;
                }
            }
        } else {
            self.recent_dcs.push(RecentDc { name, times: [now, 0, 0], count: 1 });
        }
    }

    /// The m_recentDCs part of the 3 s timer: a logout is forgotten after 10 minutes.
    pub fn fz_age_disconnects(&mut self) {
        let now = self.now();
        for r in &mut self.recent_dcs {
            for t in r.times.iter_mut() {
                if *t != 0 && now.wrapping_sub(*t) > 10 * 60 * 1000 {
                    r.count -= 1;
                    *t = 0;
                }
            }
        }
        self.recent_dcs.retain(|r| r.count != 0);
    }

    /// The end of ResponsePlayerDataHandler: a non-GM entering a fight zone from the Login is recalled to his town,
    /// unless he logged out of a fight zone less than 18 s ago (a dropped connection).
    pub fn fz_entry_recall(&mut self, h: usize) {
        let now = self.now();
        let Some(c) = self.client(h) else { return };
        if !self.maps[c.map_index].is_fight_zone || c.is_gm() {
            return;
        }
        let mut recall = true;
        if let Some(i) = self.recent_dcs.iter().position(|r| r.name == cstr(&c.char_name)) {
            if self.recent_dcs[i].times.iter().any(|&t| now.wrapping_sub(t) < 18_000) {
                recall = false;
            } else {
                self.recent_dcs.remove(i);
            }
        }
        if recall {
            self.request_teleport(h, 0, None, -1, -1);
        }
    }

    pub fn fightzone_init(&mut self) {
        let t = crate::chrono_now();
        self.fz.reserve_time = self.now().wrapping_sub(((t.hour % 2) * 3600 + t.minute * 60) * 1000);
    }

    /// the 2-hour timer of OnTimer
    pub fn fightzone_timer(&mut self) {
        let now = self.now();
        if now.wrapping_sub(self.fz.reserve_time) > 2 * 3600 * 1000 {
            self.fightzone_reserve_processor();
            self.fz.reserve_time = now;
        }
    }

    /// `FightzoneReserveProcessor`: the GM reservations stay
    fn fightzone_reserve_processor(&mut self) {
        for r in self.fz.reserve.iter_mut().filter(|r| **r != -1) {
            *r = 0;
        }
        for h in self.handles() {
            if self.client(h).unwrap().fightzone_number != 0 {
                self.notify(h, NOTIFY_FIGHTZONERESERVE, &[-2], None);
            }
        }
    }

    /// `FightzoneReserveHandler`: the zone number is an int at 6
    pub fn fightzone_reserve(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        let zone = d.get(6..10).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()));
        if !(1..=MAXFIGHTZONE).contains(&zone) {
            return;
        }
        let t = crate::chrono_now();
        let (gk, gold) = self.item_count(h, b"Gold");
        let c = self.client(h).unwrap();
        let result = reserve_result(&t, self.fz.reserve[zone as usize - 1] != 0, gold, c.side, zone, c.fightzone_number != 0);
        if result == 1 {
            if let Some(k) = gk {
                self.set_item_count(h, k, gold - 1500);
            }
            self.calc_total_weight(h);
            self.fz.reserve[zone as usize - 1] = h as i32;
            let c = self.client_mut(h).unwrap();
            c.fightzone_number = zone;
            c.reserve_time = reserve_stamp(&t);
            c.fightzone_ticket_number = 50;
            let txt = format!("(*) Reserve FIGHTZONETICKET : Char({}) TICKENUMBER ({})", l1(&c.char_name), c.reserve_time);
            log_file(&txt);
            log(&txt);
        }
        let mut b = MSGID_RESPONSE_FIGHTZONE_RESERVE.to_le_bytes().to_vec();
        b.extend_from_slice(&(if result == 1 { MSGTYPE_CONFIRM } else { MSGTYPE_REJECT } as u16).to_le_bytes());
        b.extend_from_slice(&result.to_le_bytes());
        self.send(h, &b);
    }

    /// `GetFightzoneTicketHandler`
    pub fn get_fightzone_ticket(&mut self, h: usize) {
        let Some(c) = self.client_mut(h) else { return };
        if c.fightzone_ticket_number <= 0 {
            c.fightzone_number *= -1;
            self.notify(h, NOTIFY_FIGHTZONERESERVE, &[-1], None);
            return;
        }
        let name = if c.fightzone_number == 1 { "ArenaTicket".to_string() } else { format!("ArenaTicket({})", c.fightzone_number) };
        let rt = c.reserve_time;
        let Some(cfg) = self.data.item_by_name(name.as_bytes()).cloned() else { return };
        let mut it = Item::new(&cfg);
        it.count = 1;
        let (month, day) = (rt / 10000, rt % 10000 / 100);
        (it.touch_effect_type, it.touch_effect_value) = (ITET_DATE as i16, [month as i16, day as i16, (rt % 100) as i16]);
        if self.add_client_item_list(h, &it) {
            let c = self.client_mut(h).unwrap();
            c.cur_weight_load = c.cur_weight_load.max(0);
            c.fightzone_ticket_number -= 1;
            self.item_log(ITEMLOG_GET, h, -1, &it, false);
            self.send_item_obtained(h, &it);
        } else {
            self.calc_total_weight(h);
            self.send_cannot_carry(h);
        }
    }

    /// `AdminOrder_ReserveFightzone` (/reservefightzone [n], level 2)
    pub fn gm_reserve_fightzone(&mut self, h: usize, cmd: &[u8]) {
        if self.client(h).unwrap().admin_user_level < 2 {
            self.notify(h, NOTIFY_ADMINUSERLEVELLOW, &[], None);
            return;
        }
        let mut t = StrTok::new(cmd, b"= \t\n");
        t.get();
        let n = t.get();
        // every call toggles the forced recall, with or without a zone
        self.fz.no_force_recall ^= 1;
        self.notify(h, NOTIFY_FIGHTZONERESERVE, &[if self.fz.no_force_recall == 1 { -5 } else { -4 }], None);
        let Some(n) = n else { return };
        let i = atoi(&n) - 1;
        // fix: the C++ indexed m_iFightZoneReserve[atoi - 1] unchecked
        let name = l1(&self.client(h).unwrap().char_name);
        let Some(r) = self.fz.reserve.get_mut(i as usize) else { return };
        if *r != -1 {
            *r = -1;
            self.send_ls_plain(MSGID_GAMEMASTERLOG, format!("GM Order({name}): FightzoneNumber({i}) FightzoneReserved").as_bytes());
            self.notify(h, NOTIFY_FIGHTZONERESERVE, &[i + 1], None);
        } else {
            *r = 0;
            self.send_ls_plain(MSGID_GAMEMASTERLOG, format!("GM Order({name}): FightzoneNumber({i}) Cancel FightzoneReserved").as_bytes());
            self.notify(h, NOTIFY_FIGHTZONERESERVE, &[-3], None);
        }
    }

    /// `AdminOrder_GetFightzoneTicket` (/getticket, level 2): three tickets of the fight zone the GM is on (valid
    /// 3 hours), or elsewhere of fight zone 1 (2 hours); the GM's own reservation is left as it was
    pub fn gm_get_ticket(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        let mname = self.maps[c.map_index].name.clone();
        let in_fz = mname.starts_with(b"fightzone");
        let saved = (c.reserve_time, c.fightzone_number, c.fightzone_ticket_number);
        let t = crate::chrono_now();
        let c = self.client_mut(h).unwrap();
        c.reserve_time = (t.month * 10000 + t.day * 100 + t.hour + if in_fz { 3 } else { 2 }) as i32;
        c.fightzone_ticket_number = 10;
        c.fightzone_number = if in_fz { atoi(cstr(&mname[9..])) } else { 1 };
        for _ in 0..3 {
            self.get_fightzone_ticket(h);
        }
        let c = self.client_mut(h).unwrap();
        (c.reserve_time, c.fightzone_number, c.fightzone_ticket_number) = saved;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(month: u32, day: u32, hour: u32, minute: u32) -> crate::Now {
        crate::Now { month, day, hour, minute, ..Default::default() }
    }

    #[test]
    fn reserve() {
        // day 4, aresden (1), zone 1: (4 + 1 + 1) % 2 == 0 -> allowed
        assert_eq!(reserve_result(&at(11, 4, 10, 0), false, 2000, 1, 1, false), 1);
        assert_eq!(reserve_result(&at(11, 4, 11, 55), false, 2000, 1, 1, false), 0, "the last 5 minutes of the slot");
        assert_eq!(reserve_result(&at(11, 4, 10, 0), true, 2000, 1, 1, false), -1);
        assert_eq!(reserve_result(&at(11, 4, 10, 0), false, 1499, 1, 1, false), -2);
        assert_eq!(reserve_result(&at(11, 4, 10, 0), false, 2000, 1, 2, false), -3, "the other day");
        assert_eq!(reserve_result(&at(11, 4, 10, 0), false, 2000, 1, 1, true), -4);
        assert_eq!(reserve_stamp(&at(11, 4, 10, 0)), 110412);
        assert_eq!(reserve_stamp(&at(11, 4, 11, 30)), 110412);
    }
}
