//! Astoria (Astoria.cpp + HG.cpp ~40187-40380 and its branches): the CTR (capture the relic) and DS (destroy the
//! shield) events on the astoria map. CAstoria (start, relic pick-up / drop, victory, casualties), EventStart /
//! EventEnd, NotifyEventStats / NotifyRelicInAltar / NotifyRelicGrabbed, ShuffleAstoriaBasePos, CreateAstoriaFlags,
//! DeleteFlags, UpdateRelicPos, m_eventWinner (stats.ini), the /astoria and /endastoria GM commands, and the
//! astoria branches elsewhere (base teleports, the weekend recall, the relic holder's limits).
use crate::client::*;
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::util::cstr;

pub const ET_CAPTURE: i32 = 1;
pub const ET_DESTROY_SHIELD: i32 = 2;
const ET_MAX: usize = 4;
const MAXALTARS: usize = 2;
pub const ITEM_RELIC: i32 = 1005;
const RELICVICTORYTIME: u32 = 10 * 60 * 1000;
pub const STATUS_RELICHOLDER: i32 = 1 << 17;
/// the relic's unique id (CAstoria::_relic: the one genuine relic; copies made otherwise are fakes)
const RELIC_UID: u64 = 0x5245_4C49_4300_0001;

const RELIC_START: (i16, i16) = (168, 207);
const ALTAR_POS: [(i16, i16); MAXALTARS] = [(28, 105), (314, 321)];
pub const ASTORIA_START: [(i16, i16); MAXALTARS] = [(47, 116), (302, 302)];
const FLAGS_POS: [&[(i16, i16)]; MAXALTARS] = [
    &[(209, 179), (210, 179), (158, 202), (164, 198), (90, 104), (88, 111), (76, 97), (74, 110), (80, 151), (88, 144), (64, 143), (81, 135), (37, 177), (42, 144), (116, 179), (125, 172), (31, 105), (25, 105)],
    &[(211, 179), (211, 180), (272, 270), (264, 275), (270, 286), (279, 275), (245, 306), (253, 299), (264, 312), (268, 304), (293, 297), (298, 299), (175, 215), (179, 211), (232, 256), (241, 250), (311, 321), (317, 321)],
];

/// a running event (CAstoria)
#[derive(Clone, Debug, Default)]
pub struct Event {
    pub ty: i32,
    /// deaths, kills by side
    pub stats: [(i32, i32); 4],
    pub relic_pos: (i16, i16),
    pub holder: Option<usize>,
    pub owned_side: i32,
    pub owned_time: u32,
    pub holder_steps: u32,
    pub begin: u32,
}

#[derive(Default)]
pub struct Astoria {
    pub ev: Option<Event>,
    /// m_astoriaBasePos: the side of each altar / base
    pub base_pos: [i32; MAXALTARS],
    /// m_eventWinner by event type (stats.ini, [events])
    pub winners: [i32; ET_MAX],
}

/// the [events] section of stats.ini: key -> value
pub fn read_ini_section(txt: &str, section: &str) -> Vec<(String, i32)> {
    let mut on = false;
    let mut out = Vec::new();
    for l in txt.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            on = l.trim_matches(|c| c == '[' || c == ']').eq_ignore_ascii_case(section);
        } else if on {
            if let Some((k, v)) = l.split_once('=') {
                out.push((k.trim().to_string(), crate::util::atoi(v.trim().as_bytes())));
            }
        }
    }
    out
}

/// WritePrivateProfileString of one key in one section (the rest of the file is kept)
pub fn write_ini_key(txt: &str, section: &str, key: &str, val: i32) -> String {
    let mut out = Vec::new();
    let (mut on, mut found, mut seen) = (false, false, false);
    for l in txt.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            if on && !found {
                out.push(format!("{key}={val}"));
                found = true;
            }
            on = t.trim_matches(|c| c == '[' || c == ']').eq_ignore_ascii_case(section);
            seen |= on;
        } else if on && t.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key)) {
            out.push(format!("{key}={val}"));
            found = true;
            continue;
        }
        out.push(l.to_string());
    }
    if !found {
        if !seen {
            out.push(format!("[{section}]"));
        }
        out.push(format!("{key}={val}"));
    }
    out.join("\r\n") + "\r\n"
}

impl Game {
    pub fn astoria_map(&self) -> Option<usize> {
        self.map_index(b"astoria")
    }

    pub fn is_astoria_map(&self, m: usize) -> bool {
        self.astoria_map() == Some(m)
    }

    /// CGame::CGame: m_eventWinner from stats.ini
    pub fn astoria_init(&mut self) {
        // (GetPrivateProfileInt on "..\configs\stats.ini": relative to the server folder, as here)
        let txt = std::fs::read_to_string("../configs/stats.ini").unwrap_or_default();
        for (k, v) in read_ini_section(&txt, "events") {
            if let Ok(i) = k.parse::<usize>() {
                if i < ET_MAX {
                    self.ast.winners[i] = v;
                }
            }
        }
    }

    /// the running event's relic holder
    pub fn relic_holder(&self) -> Option<usize> {
        self.ast.ev.as_ref().and_then(|e| e.holder)
    }

    pub fn is_relic_holder(&self, h: usize) -> bool {
        self.relic_holder() == Some(h)
    }

    pub fn is_genuine_relic(&self, it: &Item) -> bool {
        self.ast.ev.is_some() && it.cfg.id_num == ITEM_RELIC && it.unique_id == RELIC_UID
    }

    fn astoria_players(&self) -> Vec<usize> {
        let m = self.astoria_map();
        self.handles().into_iter().filter(|&i| self.client(i).is_some_and(|c| c.is_init_complete && Some(c.map_index) == m)).collect()
    }

    /// `EventStart` (/astoria 1|2, the scheduler)
    pub fn astoria_event_start(&mut self, ty: i32) {
        if ty != ET_CAPTURE && ty != ET_DESTROY_SHIELD {
            return;
        }
        if self.ast.ev.is_some() {
            self.astoria_event_drop(); // m_astoria.reset(): the old one ends without a winner
        }
        let Some(m) = self.astoria_map() else {
            // shortcut: the C++ indexed the map list with -1 on a server without astoria; no event here
            log("(!) Astoria: no astoria map on this server");
            return;
        };
        // ShuffleAstoriaBasePos
        let mut sides = [ARESDEN, ELVINE];
        for i in 0..MAXALTARS {
            let pos = (self.rng.rand() as usize) % (MAXALTARS - i);
            self.ast.base_pos[i] = sides[pos];
            sides[pos] = sides[MAXALTARS - i - 1];
        }
        let mut ev = Event { ty, begin: self.now(), ..Default::default() };
        if ty == ET_CAPTURE {
            // _CaptureStart: the relic on its spot (bSetItem: it never expires)
            if let Some(cfg) = self.data.item_by_id(ITEM_RELIC).cloned() {
                let mut it = Item::new(&cfg);
                it.unique_id = RELIC_UID;
                self.drop_ground_item(m, RELIC_START.0, RELIC_START.1, it);
                // the C++ sends COMMONTYPE_SETITEM here (drop_ground_item's ITEMDROP draws the same)
            }
            ev.relic_pos = RELIC_START;
        }
        self.ast.ev = Some(ev);
        // CreateAstoriaFlags
        for i in 0..MAXALTARS {
            let d = if self.ast.base_pos[i] == ARESDEN { DYNAMICOBJECT_ARESDENFLAG } else { DYNAMICOBJECT_ELVINEFLAG };
            for &(x, y) in FLAGS_POS[i] {
                self.add_dynamic_object(0, 0, d, m, x, y, 0, 0);
            }
        }
        for i in self.handles() {
            let c = self.client(i).unwrap();
            if !c.is_init_complete {
                continue;
            }
            let (cm, gm) = (c.map_index, c.is_gm());
            self.notify(i, NOTIFY_EVENTSTART, &[ty], None);
            if cm == m && !gm {
                self.notify(i, NOTIFY_TOBERECALLED, &[], None);
                self.request_teleport(i, 0, None, -1, -1);
            }
        }
    }

    /// `EventEnd` (/endastoria, the relic's 10 minutes in an altar)
    pub fn astoria_event_end(&mut self) {
        let Some(ev) = self.ast.ev.as_ref() else {
            return; // fix: the C++ called m_astoria->GetVictoriousSide() with no event (/endastoria): a crash
        };
        let winner = if ev.ty == ET_CAPTURE { ev.owned_side } else { NEUTRAL };
        let ty = ev.ty as usize;
        self.ast.winners[ty] = winner;
        let txt = std::fs::read_to_string("../configs/stats.ini").unwrap_or_default();
        let _ = std::fs::write("../configs/stats.ini", write_ini_key(&txt, "events", &ty.to_string(), winner));
        for i in self.handles() {
            if self.client(i).unwrap().is_init_complete {
                self.notify(i, NOTIFY_CTRWINNER, &[winner], None);
            }
        }
        if let Some(m) = self.astoria_map() {
            for n in 1..self.npcs.len() {
                if self.npcs[n].as_ref().is_some_and(|x| x.map_index == m && matches!(x.type_, 36 | 37 | 39)) {
                    self.delete_npc(n);
                }
            }
        }
        self.astoria_event_drop();
    }

    /// CAstoria's destructor: the holder drops the relic, the relic leaves its tile, every flag goes
    fn astoria_event_drop(&mut self) {
        let Some(ev) = self.ast.ev.clone() else { return };
        if ev.ty == ET_CAPTURE {
            if let Some(h) = ev.holder {
                if let Some(k) = self.client(h).and_then(|c| c.has_item(ITEM_RELIC)) {
                    let n = self.client(h).unwrap().items[k].as_ref().unwrap().cfg.name.clone();
                    self.drop_item(h, k, 1, &n);
                }
            }
            if let Some(m) = self.astoria_map() {
                let (x, y) = self.ast.ev.as_ref().unwrap().relic_pos;
                // pGetItem: the top item of the relic's tile (the relic) goes
                if self.ground.get(&(m, x, y)).and_then(|v| v.first()).is_some_and(|i| self.is_genuine_relic(i)) {
                    self.take_ground_item(m, x, y);
                }
                let top = self.maps[m].tile(x, y).and_then(|t| t.top_item).unwrap_or((0, 0, 0));
                self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_SETITEM as u16, m, x, y, [top.0, top.1, top.2 as i16, 0]);
            }
        }
        self.ast.ev = None;
        self.delete_all_flags();
    }

    /// `DeleteFlags`: every aresden / elvine flag object of every map
    pub fn delete_all_flags(&mut self) {
        for i in 1..self.dyn_objs.len() {
            let Some(o) = self.dyn_objs[i].as_ref() else { continue };
            if o.ty != DYNAMICOBJECT_ARESDENFLAG && o.ty != DYNAMICOBJECT_ELVINEFLAG {
                continue;
            }
            let (m, x, y, ty) = (o.m, o.x, o.y, o.ty);
            self.send_event_type_b(MSGID_DYNAMICOBJECT, MSGTYPE_REJECT as u16, m, x, y, [ty as i16, i as i16, 0, 0]);
            if let Some(t) = self.maps[m].tile_mut(x, y) {
                (t.dynamic_object_id, t.dynamic_object_type) = (0, 0);
            }
            self.maps[m].set_temp_move_allowed(x, y, true);
            self.dyn_objs[i] = None;
        }
        for hm in self.hd.maps.iter_mut() {
            hm.flags.clear(); // the heldenian flags went with them
        }
    }

    /// `CAstoria::CheckVictory` + EventEnd (the 1 s timer)
    pub fn astoria_check_victory(&mut self) {
        let now = self.now();
        let won = self.ast.ev.as_ref().is_some_and(|e| e.ty == ET_CAPTURE && e.holder.is_none() && ALTAR_POS.contains(&e.relic_pos) && now.wrapping_sub(e.owned_time) > RELICVICTORYTIME);
        if won {
            self.astoria_event_end();
        }
    }

    /// `CAstoria::PlayerGetRelic` (the get-item motion and the possession spell, for the genuine relic)
    pub fn player_get_relic(&mut self, h: usize) {
        let Some(ev) = self.ast.ev.as_mut() else { return };
        ev.holder = Some(h);
        self.client_mut(h).unwrap().set_status_flag(STATUS_RELICHOLDER, true);
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        self.remove_magic_effect(h, crate::map::OWNERTYPE_PLAYER, MAGICTYPE_INVISIBILITY);
        let name = self.client(h).unwrap().char_name.clone();
        for i in self.astoria_players() {
            self.notify(i, NOTIFY_RELICGRABBED, &[], Some(&name));
        }
    }

    /// `CAstoria::PlayerDropRelic` + UpdateRelicPos (DropItemHandler of the genuine relic)
    fn player_drop_relic(&mut self, h: usize) {
        let c = self.client_mut(h).unwrap();
        c.set_status_flag(STATUS_RELICHOLDER, false);
        let pos = (c.x, c.y);
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        let now = self.now();
        let base = self.ast.base_pos;
        let Some(ev) = self.ast.ev.as_mut() else { return };
        (ev.holder, ev.relic_pos, ev.holder_steps) = (None, pos, 0);
        if let Some(a) = ALTAR_POS.iter().position(|&p| p == pos) {
            (ev.owned_side, ev.owned_time) = (base[a], now);
            let s = base[a];
            for i in self.astoria_players() {
                self.notify(i, NOTIFY_RELICINALTAR, &[s], None);
            }
        }
        self.update_relic_pos(None);
    }

    /// the relic part of DropItemHandler: Some(true) = the genuine relic, placed (no expiry); Some(false) = a fake
    /// one, deleted; None = not a relic
    pub fn drop_relic(&mut self, h: usize, it: &Item) -> Option<bool> {
        if it.cfg.id_num != ITEM_RELIC {
            return None;
        }
        if !self.is_genuine_relic(it) {
            return Some(false);
        }
        self.player_drop_relic(h);
        let c = self.client(h).unwrap();
        let (m, x, y) = (c.map_index, c.x, c.y);
        self.drop_ground_item(m, x, y, it.clone());
        Some(true)
    }

    /// `UpdateRelicPos` (None: everyone on astoria)
    pub fn update_relic_pos(&mut self, to: Option<usize>) {
        let Some(ev) = self.ast.ev.as_ref() else { return };
        let p = match ev.holder.and_then(|h| self.client(h)) {
            Some(c) => (c.x as i32, c.y as i32),
            None => (ev.relic_pos.0 as i32, ev.relic_pos.1 as i32),
        };
        let to = match to {
            Some(h) => vec![h],
            None => self.astoria_players(),
        };
        for i in to {
            self.notify(i, NOTIFY_RELICPOSITION, &[p.0, p.1], None);
        }
    }

    /// the holder's steps of the move handler: every 5th updates the relic's position
    pub fn relic_holder_moved(&mut self, h: usize) {
        let Some(ev) = self.ast.ev.as_mut().filter(|e| e.holder == Some(h)) else { return };
        ev.holder_steps += 1;
        if ev.holder_steps > 4 {
            ev.holder_steps = 0;
            self.update_relic_pos(None);
        }
    }

    /// `CAstoria::PlayerKill` + NotifyEventStats (KilledHandler, a player killed by a citizen of the other side)
    pub fn astoria_player_kill(&mut self, killer_side: i32, victim_side: i32) {
        let Some(ev) = self.ast.ev.as_mut() else { return };
        let k = |s: i32| s.clamp(0, 3) as usize;
        if killer_side != victim_side {
            ev.stats[k(killer_side)].1 += 1;
        }
        ev.stats[k(victim_side)].0 += 1;
        let s = ev.stats;
        for i in self.astoria_players() {
            self.notify(i, NOTIFY_CASUALTIES, &[s[1].0, s[2].0, s[3].0, s[1].1, s[2].1, s[3].1], None);
        }
    }

    /// the astoria part of the map entry: the casualties and the relic's position
    pub fn astoria_entry(&mut self, h: usize) {
        let Some(ev) = self.ast.ev.as_ref() else { return };
        let c = self.client(h).unwrap();
        if cstr(&c.map_name) != b"astoria" {
            return;
        }
        let s = ev.stats;
        self.notify(h, NOTIFY_CASUALTIES, &[s[1].0, s[2].0, s[3].0, s[1].1, s[2].1, s[3].1], None);
        self.update_relic_pos(Some(h));
    }

    /// the 3 s player timer: astoria is closed on friday-sunday when no event runs
    pub fn astoria_player_timer(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if self.ast.ev.is_none() && self.is_astoria_map(c.map_index) && !c.is_gm() {
            let d = crate::chrono_now().weekday; // 0 = sunday
            if matches!(d, 5 | 6 | 0) {
                self.notify(h, NOTIFY_TOBERECALLED, &[], None);
                self.request_teleport(h, 0, None, -1, -1);
            }
        }
    }

    /// the astoria part of RequestTeleportHandler: a base teleport ("astoria0", "astoria1") leads its side home;
    /// None when it does not apply, Some(None) = stay where the player is (no recall time)
    pub fn astoria_base_teleport(&self, h: usize, dest: &[u8]) -> Option<Option<Vec<u8>>> {
        let c = self.client(h)?;
        if !self.is_astoria_map(c.map_index) || !dest.starts_with(b"astoria") || dest.len() < 8 {
            return None;
        }
        let b = dest[7].wrapping_sub(b'0') as usize;
        // fix: the C++ read m_astoriaBasePos[digit] unchecked
        let side = *self.ast.base_pos.get(b)?;
        Some((c.side == side).then(|| SIDE_MAP[side as usize].to_vec()))
    }

    /// the relic holder can only walk, fight and carry it (no invisibility, pretend corpse, give or exchange)
    pub fn relic_blocks(&self, h: usize) -> bool {
        self.is_relic_holder(h)
    }

    /// the GM commands `/astoria N` and `/endastoria` (admin level 4)
    pub fn astoria_gm_command(&mut self, h: usize, cmd: &[u8]) -> bool {
        let Some(c) = self.client(h) else { return false };
        let (lvl, name) = (c.admin_user_level, String::from_utf8_lossy(cstr(&c.char_name)).into_owned());
        if cmd.starts_with(b"/astoria ") {
            if lvl >= 4 {
                let ty = crate::util::atoi(&cmd[9..]);
                self.astoria_event_start(ty);
                self.send_ls_plain(MSGID_GAMEMASTERLOG, format!("GM Order({name}): Start Astoria({})", ty as u32).as_bytes());
            }
        } else if cmd.starts_with(b"/endastoria") {
            if lvl >= 4 {
                self.send_ls_plain(MSGID_GAMEMASTERLOG, format!("GM Order({name}): End Astoria").as_bytes());
                self.astoria_event_end();
            }
        } else {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini() {
        assert_eq!(read_ini_section("", "events"), vec![]);
        let t = write_ini_key("", "events", "1", 2);
        assert_eq!(read_ini_section(&t, "events"), vec![("1".to_string(), 2)]);
        let t = write_ini_key("[other]\r\na=1\r\n[events]\r\n1=2\r\n", "events", "1", 1);
        assert_eq!(read_ini_section(&t, "events"), vec![("1".to_string(), 1)]);
        assert_eq!(read_ini_section(&t, "other"), vec![("a".to_string(), 1)]);
        let t = write_ini_key(&t, "events", "2", 0);
        assert_eq!(read_ini_section(&t, "events").len(), 2);
    }
}
