//! Parties (HG.cpp ~36041..36700, JoinPartyHandler ~32668, the GetExp share): the Login's party manager owns the
//! parties (MSGID_PARTYOPERATION); each game server keeps its local members per party (m_stPartyInfo).
//! HG -> Login: [msgid][op][client][name 10][party id] in 50 bytes; op 1 create, 3 add member, 4 remove member,
//! 5 check member (at entry), 6 info, 7 changing server. Login -> HG: 1 create result, 2 party deleted, 3 not a member,
//! 4 join result, 5 info, 6 dismiss result (see PartyOperationResultHandler).
use crate::client::{Client, STATUS_GREENSLATE};
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::util::{cstr, Rd, W};

/// GetExp's party share: each living member's stock. `members` = living local members (including the killer).
/// Owner's decision (2026-11-20): the exp is split among the members; the C++ multiplied by the member count
/// (`dV1 * iPartyTotalMember`, HG.cpp ~36718), giving each of n members n times the kill's exp.
pub fn party_share(exp: i32, members: i32) -> i32 {
    let m = members.clamp(1, MAXPARTYMEMBERS);
    (exp as f64 / m as f64 + 0.75f32 as f64) as i32
}

/// What the party frame of our client shows about a member (Rust only; see `Game::party_hud_flush`).
#[derive(Clone, Default, PartialEq, Debug)]
pub struct HudState {
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub level: i32,
    pub dead: bool,
    pub map: Vec<u8>,
}

fn hud_state(c: &Client) -> HudState {
    HudState {
        hp: c.hp,
        max_hp: c.get_max_hp(),
        mp: c.mp,
        max_mp: c.get_max_mp(),
        level: c.level,
        dead: c.is_killed || c.hp <= 0,
        map: cstr(&c.map_name).to_vec(),
    }
}

impl Game {
    /// Rust only: keeps the party frame of the clients that announced CLIENTFEATURE_PARTY_HUD up to date, with NOTIFY_PARTY_HUD:
    /// a member's state when it changed, the whole roster when the party changed (and every 10 s, in case the client had not
    /// yet heard of a member). The original clients never get it. Called every ~300 ms.
    pub fn party_hud_flush(&mut self) {
        let slot = (self.now() / 10_000) as u64;
        for pid in self.parties.keys().copied().collect::<Vec<_>>() {
            let members: Vec<usize> = self.party_members(pid).into_iter().filter(|&h| self.client(h).is_some_and(|c| c.is_init_complete)).collect();
            let to: Vec<usize> = members.iter().copied().filter(|&h| self.client(h).unwrap().client_features & CLIENTFEATURE_PARTY_HUD != 0).collect();
            if to.is_empty() {
                continue;
            }
            let sig = members.iter().fold(pid as u64 ^ slot << 32, |a, &h| a.wrapping_mul(1_000_003).wrapping_add(h as u64 + 1));
            let states: Vec<(usize, Vec<u8>, HudState)> = members.iter().map(|&h| {
                let c = self.client(h).unwrap();
                (h, c.char_name.clone(), hud_state(c))
            }).collect();
            for &r in &to {
                let full = self.client(r).unwrap().hud_sig != sig;
                for (h, name, st) in &states {
                    if *h != r && (full || self.client(*h).unwrap().hud_sent != *st) {
                        let v = [st.hp.max(0), st.max_hp.max(0), st.mp.max(0), st.max_mp.max(0), st.level, st.dead as i32];
                        self.notify2(r, NOTIFY_PARTY_HUD, &v, Some(name), Some(&st.map));
                    }
                }
                self.client_mut(r).unwrap().hud_sig = sig;
            }
            for (h, _, st) in states {
                self.client_mut(h).unwrap().hud_sent = st;
            }
        }
    }

    /// a raw MSGID_PARTYOPERATION (the op is where MSGTYPE goes)
    fn party_op(&mut self, op: u16, gsch: u16, name: &[u8], pid: i32) {
        let mut b = vec![0u8; 50];
        let mut w = W(&mut b);
        w.u32(0, MSGID_PARTYOPERATION);
        w.u16(4, op);
        w.u16(6, gsch);
        w.bytes(8, &Game::pad10(name));
        w.u16(18, pid as u16);
        self.send_ls_raw(&b);
    }

    pub fn party_members(&self, pid: i32) -> Vec<usize> {
        self.parties.get(&pid).cloned().unwrap_or_default()
    }

    fn party_add_local(&mut self, pid: i32, h: usize) {
        let v = self.parties.entry(pid).or_default();
        if v.len() < MAXPARTYMEMBERS as usize && !v.contains(&h) {
            v.push(h);
        }
    }

    pub fn party_remove_local(&mut self, pid: i32, h: usize) {
        if let Some(v) = self.parties.get_mut(&pid) {
            v.retain(|&x| x != h);
            if v.is_empty() {
                self.parties.remove(&pid);
            }
        }
    }

    fn party_clear(&mut self, h: usize) {
        let c = self.client_mut(h).unwrap();
        c.party_id = 0;
        c.party_status = PARTYSTATUS_NULL;
        c.req_join_party_h = 0;
        c.hud_sig = 0;
    }

    /// NOTIFY_PARTY_COORDS about `from` to `to` (x, y given: 0, 0 when on another map)
    pub fn party_coords(&mut self, from: usize, to: usize, on_map: bool) {
        let Some(c) = self.client(from) else { return };
        let (x, y) = if on_map { (c.x as i32, c.y as i32) } else { (0, 0) };
        let v = [x, y, 0, c.hp, c.mp, c.get_max_hp(), c.get_max_mp()];
        let name = c.char_name.clone();
        self.notify(to, NOTIFY_PARTY_COORDS, &v, Some(&name));
    }

    /// to every local member on h's map (motion every 4 steps, HP every 4th / MP every 7th notify)
    pub fn party_coords_to_members(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        let (pid, m) = (c.party_id, c.map_index);
        for j in self.party_members(pid) {
            if j != h && self.client(j).is_some_and(|o| o.map_index == m) {
                self.party_coords(h, j, true);
            }
        }
    }

    /// `RequestCreatePartyHandler`
    fn request_create_party(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || c.party_status != PARTYSTATUS_NULL || (self.maps[c.map_index].is_party_disabled && !c.is_gm()) {
            return;
        }
        let name = c.char_name.clone();
        self.client_mut(h).unwrap().party_status = PARTYSTATUS_PROCESSING;
        self.party_op(1, h as u16, &name, 0);
    }

    /// `JoinPartyHandler` (COMMONTYPE_REQUEST_JOINPARTY): v1 0 = leave, 1 = ask `name` to party, 2 = party info
    pub fn join_party_cmd(&mut self, h: usize, v1: i32, name: &[u8]) {
        let Some(c) = self.client(h) else { return };
        match v1 {
            0 => self.request_delete_party(h),
            1 => {
                if c.party_id != 0 || c.party_status != PARTYSTATUS_NULL {
                    self.notify(h, NOTIFY_PARTY, &[7, 0], None);
                    return;
                }
                let name = cstr(name).to_vec();
                let Some(i) = self.handles().into_iter().find(|&i| cstr(&self.client(i).unwrap().char_name) == &name[..]) else { return };
                let (me, o) = (self.client(h).unwrap(), self.client(i).unwrap());
                // fix: the C++ let a player ask itself (both request fields then pointed at itself)
                if i == h || (o.appr[1] as u16 & 0xF000) >> 12 != 0 || o.side != me.side || (self.maps[o.map_index].is_party_disabled && !o.is_gm()) {
                    self.notify(h, NOTIFY_PARTY, &[7, 0], None);
                } else if o.party_status == PARTYSTATUS_PROCESSING {
                    self.notify(h, NOTIFY_PARTY, &[7, 0], None);
                    let c = self.client_mut(h).unwrap();
                    c.req_join_party_h = 0;
                    c.req_join_party_name.clear();
                    c.party_status = PARTYSTATUS_NULL;
                } else {
                    let (mine, theirs) = (me.char_name.clone(), o.char_name.clone());
                    let o = self.client_mut(i).unwrap();
                    o.req_join_party_h = h;
                    o.req_join_party_name = mine.clone();
                    self.notify(i, NOTIFY_QUERY_JOINPARTY, &[], Some(&mine));
                    let c = self.client_mut(h).unwrap();
                    c.req_join_party_h = i;
                    c.req_join_party_name = theirs;
                    c.party_status = PARTYSTATUS_PROCESSING;
                }
            }
            2 => self.get_party_info(h),
            _ => {}
        }
    }

    /// `RequestJoinPartyHandler` (/joinparty name): join the party of a local player.
    pub fn request_join_party(&mut self, h: usize, cmd: &[u8]) {
        let c = self.client(h).unwrap();
        if c.party_status != PARTYSTATUS_NULL {
            return;
        }
        let mut t = crate::util::StrTok::new(cmd, b"= \t\n");
        let _ = t.get();
        let Some(tok) = t.get() else { return };
        let me = c.char_name.clone();
        match self.handles().into_iter().find(|&i| cstr(&self.client(i).unwrap().char_name) == &tok[..]) {
            Some(i) => {
                let o = self.client(i).unwrap();
                if o.party_id == 0 || o.party_status != PARTYSTATUS_CONFIRM {
                    return;
                }
                let pid = o.party_id;
                self.party_op(3, h as u16, &me, pid);
            }
            None => self.notify(h, NOTIFY_PLAYERNOTONGAME, &[], Some(&Game::pad10(&tok))),
        }
    }

    /// `RequestDismissPartyHandler` (/dismissparty, accept-join v1 2, party-disabled maps)
    pub fn request_dismiss_party(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if c.party_status != PARTYSTATUS_CONFIRM {
            return;
        }
        let (name, pid) = (c.char_name.clone(), c.party_id);
        self.party_op(4, h as u16, &name, pid);
        self.client_mut(h).unwrap().party_status = PARTYSTATUS_PROCESSING;
    }

    /// `GetPartyInfoHandler` (/getpartyinfo)
    pub fn get_party_info(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if c.party_status != PARTYSTATUS_CONFIRM {
            return;
        }
        let (name, pid) = (c.char_name.clone(), c.party_id);
        self.party_op(6, h as u16, &name, pid);
    }

    /// `RequestDeletePartyHandler` (/deleteparty, JoinPartyHandler v1 0)
    pub fn request_delete_party(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if c.party_id == 0 {
            return;
        }
        let (name, pid) = (c.char_name.clone(), c.party_id);
        self.party_op(4, h as u16, &name, pid);
        self.client_mut(h).unwrap().party_status = PARTYSTATUS_PROCESSING;
    }

    /// the requester `iH` of h's pending query, if both sides still point at each other
    fn join_requester(&self, h: usize) -> Option<usize> {
        let c = self.client(h)?;
        let i = c.req_join_party_h;
        let o = self.client(i)?;
        (i != 0 && cstr(&o.char_name) == cstr(&c.req_join_party_name) && o.party_status == PARTYSTATUS_PROCESSING && o.req_join_party_h == h && cstr(&o.req_join_party_name) == cstr(&c.char_name)).then_some(i)
    }

    /// `RequestAcceptJoinPartyHandler` (COMMONTYPE_REQUEST_ACCEPTJOINPARTY): 0 refuse, 1 accept, 2 cancel / leave
    pub fn accept_join_party(&mut self, h: usize, result: i32) {
        let Some(c) = self.client(h) else { return };
        match result {
            0 => {
                let Some(i) = self.join_requester(h) else { return };
                self.notify(i, NOTIFY_PARTY, &[7, 0], None);
                self.party_clear(i);
                self.client_mut(i).unwrap().req_join_party_name.clear();
                let c = self.client_mut(h).unwrap();
                c.req_join_party_h = 0;
                c.req_join_party_name.clear();
            }
            1 => {
                let (status, pid) = (c.party_status, c.party_id);
                let Some(i) = self.join_requester(h) else { return };
                if status == PARTYSTATUS_CONFIRM && pid != 0 {
                    let name = self.client(h).unwrap().req_join_party_name.clone();
                    self.party_op(3, i as u16, &name, pid);
                } else if status == PARTYSTATUS_NULL {
                    // the party is made first; PartyOperationResult_Create then adds the requester
                    self.request_create_party(h);
                }
            }
            2 => {
                if c.party_id != 0 && c.party_status == PARTYSTATUS_CONFIRM {
                    self.request_dismiss_party(h);
                } else {
                    let i = c.req_join_party_h;
                    let me = c.char_name.clone();
                    if let Some(o) = self.client_mut(i) {
                        if o.req_join_party_h == h && cstr(&o.req_join_party_name) == cstr(&me) {
                            o.req_join_party_h = 0;
                            o.req_join_party_name.clear();
                        }
                    }
                    self.party_clear(h);
                    self.client_mut(h).unwrap().req_join_party_name.clear();
                }
            }
            _ => {}
        }
    }

    /// local client `h` when it is still the one named `name` (the Login echoes our handle)
    fn party_target(&self, h: usize, name: &[u8]) -> Option<usize> {
        // fix: the C++ bounds check was (h < 0) && (h > MAXCLIENTS), never true
        (h > 0 && h < MAXCLIENTS as usize && self.client(h).is_some_and(|c| cstr(&c.char_name) == cstr(name))).then_some(h)
    }

    /// `PartyOperationResultHandler`: `d` is the whole message.
    pub fn party_operation_result(&mut self, d: &[u8]) {
        let r = Rd::new(d);
        match r.u16(4) {
            1 => {
                let (ok, h, name, pid) = (r.u8(6), r.u16(7) as usize, r.raw(9, 10), r.u16(19) as i32);
                let Some(h) = self.party_target(h, &name) else { return };
                if self.client(h).unwrap().party_status != PARTYSTATUS_PROCESSING {
                    return;
                }
                if ok == 0 {
                    self.party_clear(h);
                    self.notify(h, NOTIFY_PARTY, &[1, 0], None);
                    return;
                }
                let c = self.client_mut(h).unwrap();
                c.party_id = pid;
                c.party_status = PARTYSTATUS_CONFIRM;
                self.notify(h, NOTIFY_PARTY, &[1, 1], None);
                self.party_add_local(pid, h);
                let c = self.client_mut(h).unwrap();
                if c.req_join_party_h != 0 && !c.req_join_party_name.is_empty() {
                    let (i, n) = (c.req_join_party_h, std::mem::take(&mut c.req_join_party_name));
                    c.req_join_party_h = 0;
                    self.party_op(3, i as u16, &n, pid);
                }
            }
            2 => {
                let pid = r.u16(6) as i32;
                self.parties.remove(&pid);
                for i in self.handles() {
                    if self.client(i).unwrap().party_id == pid {
                        self.notify(i, NOTIFY_PARTY, &[2, 0], None);
                        self.party_clear(i);
                    }
                }
            }
            3 => {
                let (h, name) = (r.u16(6) as usize, r.raw(8, 10));
                let Some(h) = self.party_target(h, &name) else { return };
                let pid = self.client(h).unwrap().party_id;
                self.party_remove_local(pid, h);
                let c = self.client_mut(h).unwrap();
                c.party_id = 0;
                c.party_status = PARTYSTATUS_NULL;
                self.notify(h, NOTIFY_PARTY, &[8, 0], None);
            }
            4 => {
                let (ok, h, name, pid) = (r.u8(6), r.u16(7) as usize, r.raw(9, 10), r.u16(19) as i32);
                let me = self.party_target(h, &name).filter(|&h| self.client(h).unwrap().party_status == PARTYSTATUS_PROCESSING);
                let Some(h) = me else {
                    // fix: a join answered on another server reached none of this server's members (the C++ only
                    // told the members on the joiner's server); they now get the same NOTIFY_PARTY 4
                    if ok == 1 && !self.handles().into_iter().any(|i| cstr(&self.client(i).unwrap().char_name) == cstr(&name)) {
                        for j in self.party_members(pid) {
                            self.notify(j, NOTIFY_PARTY, &[4, 1, 0], Some(&name));
                        }
                    }
                    return;
                };
                if ok == 0 {
                    self.party_clear(h);
                    self.notify(h, NOTIFY_PARTY, &[4, 0, 0], Some(&name));
                    self.client_mut(h).unwrap().req_join_party_name.clear();
                    return;
                }
                let c = self.client_mut(h).unwrap();
                c.party_id = pid;
                c.party_status = PARTYSTATUS_CONFIRM;
                c.req_join_party_h = 0;
                c.req_join_party_name.clear();
                self.notify(h, NOTIFY_PARTY, &[4, 1, 0], Some(&name));
                if self.party_members(pid).len() >= MAXPARTYMEMBERS as usize {
                    return;
                }
                self.party_add_local(pid, h);
                self.get_party_info(h);
                let m = self.client(h).unwrap().map_index;
                for j in self.party_members(pid) {
                    if j != h {
                        self.notify(j, NOTIFY_PARTY, &[4, 1, 0], Some(&name));
                        if self.client(j).unwrap().map_index == m {
                            self.party_coords(h, j, true);
                        }
                    }
                }
            }
            5 => {
                let (h, name, total) = (r.u16(6) as usize, r.raw(8, 10), r.u16(18) as i32);
                let Some(h) = self.party_target(h, &name) else { return };
                let c = self.client(h).unwrap();
                if c.party_status != PARTYSTATUS_CONFIRM {
                    return;
                }
                let (pid, m) = (c.party_id, c.map_index);
                let total = total.clamp(0, MAXPARTYMEMBERS);
                let list = r.raw(20, total as usize * 11);
                self.notify(h, NOTIFY_PARTY, &[5, 1, total], Some(&list));
                for j in self.party_members(pid) {
                    if j != h && self.client(j).unwrap().map_index == m {
                        self.party_coords(j, h, true);
                    }
                }
            }
            6 => {
                let (ok, h, name, pid) = (r.u8(6), r.u16(7) as usize, r.raw(9, 10), r.u16(19) as i32);
                if ok != 1 {
                    return;
                }
                // fix: the C++ looked the leaver up by our client handle on every server, so on the other servers (and
                // for a leaver who timed out changing servers) the members were usually never told; by name here
                let me = self.party_target(h, &name).or_else(|| self.handles().into_iter().find(|&i| cstr(&self.client(i).unwrap().char_name) == cstr(&name)));
                if let Some(h) = me {
                    if self.client(h).unwrap().party_id != pid {
                        return;
                    }
                }
                for i in self.handles() {
                    if self.client(i).unwrap().party_id == pid && pid != 0 {
                        self.notify(i, NOTIFY_PARTY, &[6, 1, 0], Some(&name));
                    }
                }
                if let Some(h) = me {
                    self.party_remove_local(pid, h);
                    self.party_clear(h);
                }
            }
            op => log(&format!("(!) MSGID_PARTYOPERATION {op} from the Login: unknown")),
        }
    }

    /// InitPlayerData: a member entering (the Login sends its PartyID while the party still has it).
    pub fn party_enter(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        if c.party_id == 0 {
            return;
        }
        if self.maps[c.map_index].is_party_disabled && !c.is_gm() {
            // the C++ asked RequestDismissPartyHandler here, a no-op for a status that is not CONFIRM yet
            self.client_mut(h).unwrap().party_status = PARTYSTATUS_CONFIRM;
            self.request_dismiss_party(h);
            return;
        }
        let (name, pid) = (c.char_name.clone(), c.party_id);
        // fix: the C++ never set the status here (uninitialised), so a member who changed servers shared no exp and
        // could not leave with /dismissparty; it is CONFIRM again (the Login's op 3 answer drops it if it is no member)
        self.client_mut(h).unwrap().party_status = PARTYSTATUS_CONFIRM;
        self.party_op(5, h as u16, &name, pid);
        self.party_add_local(pid, h);
    }

    /// PlayerMapEntry: coordinates between the new arrival and the local members.
    pub fn party_map_entry(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        let (pid, m) = (c.party_id, c.map_index);
        if pid == 0 {
            return;
        }
        if self.maps[m].is_fight_zone && self.maps[m].is_party_disabled && !c.is_gm() {
            self.request_dismiss_party(h);
        }
        for j in self.party_members(pid) {
            if j == h {
                continue;
            }
            if self.client(j).is_some_and(|o| o.map_index == m) {
                self.party_coords(j, h, true);
                self.party_coords(h, j, true);
            } else {
                self.party_coords(h, j, false);
            }
        }
    }

    /// DeleteClient: leaving the game leaves the party (op 4); a server change only marks it (op 7).
    pub fn party_logout(&mut self, h: usize, server_change: bool) {
        let c = self.client(h).unwrap();
        let (pid, name) = (c.party_id, c.char_name.clone());
        if pid == 0 {
            return;
        }
        // fix: the C++ sent op 7 only for a killed player (and op 4 on an unsaved logout only for a killed one), so a
        // member who never reached the other server, or left without a save, stayed listed in the Login's party
        if server_change {
            self.party_op(7, 0, &name, pid);
        } else {
            self.party_op(4, h as u16, &name, pid);
        }
        self.party_remove_local(pid, h);
    }

    /// GetExp's party branch: true when the exp was shared (else the caller gives it all to h).
    pub fn party_get_exp(&mut self, h: usize, e: i32, attacker_own: bool) -> bool {
        let now = self.now();
        let max_level = self.settings.max_player_level;
        let c = self.client(h).unwrap();
        if c.party_id == 0 || c.party_status != PARTYSTATUS_CONFIRM || now.wrapping_sub(c.last_action_time) >= 1000 * 60 * 5 {
            return false;
        }
        let members = self.party_members(c.party_id);
        if e < 10 || members.len() <= 1 {
            return false;
        }
        let alive: Vec<usize> = members.into_iter().filter(|&i| self.client(i).is_some_and(|o| o.hp > 0)).collect();
        let n = alive.len() as i32;
        let mut unit = party_share(e, n);
        for i in alive {
            let o = self.client_mut(i).unwrap();
            let slate = if o.status & STATUS_GREENSLATE != 0 { 3 } else { 1 };
            o.cs.exp_stock += if o.level == max_level { unit / 3 * slate } else { unit * slate };
        }
        let c = self.client_mut(h).unwrap();
        if c.status & STATUS_GREENSLATE != 0 {
            unit *= 3;
        }
        if c.level == max_level {
            unit /= 3;
        }
        if attacker_own && n > 1 {
            c.cs.exp_stock += unit / 10;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::client::Client;
    use crate::config::Settings;
    use crate::map::Map;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    fn member(g: &mut Game, h: usize, name: &str, hud: bool) -> UnboundedReceiver<Vec<u8>> {
        let (tx, rx) = unbounded_channel();
        let mut c = Client::new(h, h as u64, tx, b"1.2.3.4".to_vec(), 0);
        c.char_name = name.as_bytes().to_vec();
        c.map_name = b"default".to_vec();
        (c.is_init_complete, c.level, c.party_id, c.client_features) = (true, 20, 1, if hud { CLIENTFEATURE_PARTY_HUD } else { 0 });
        (c.str_, c.vit, c.dex, c.int_, c.mag) = (20, 20, 20, 10, 10);
        c.hp = c.get_max_hp();
        g.clients[h] = Some(c);
        rx
    }

    /// (name, hp, max hp, level, flags) of every NOTIFY_PARTY_HUD queued for a client
    fn huds(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, u32, u32, u16, u8)> {
        let mut v = Vec::new();
        while let Ok(f) = rx.try_recv() {
            let b = &f[3..];
            let r = Rd::new(b);
            if r.u32(0) == MSGID_NOTIFY && r.u16(4) as i32 == NOTIFY_PARTY_HUD {
                assert_eq!(b.len(), 45);
                assert_eq!(cstr(&b[16..26]), b"default");
                v.push((String::from_utf8_lossy(cstr(&b[6..16])).into_owned(), r.u32(26), r.u32(30), r.u16(42), b[44]));
            }
        }
        v
    }

    #[test]
    fn the_party_frame_only_goes_to_clients_that_announced_it_and_only_what_changed() {
        let (tx, _ev) = tokio::sync::mpsc::channel(8);
        let mut g = Game::new(Default::default(), Settings::default(), vec![Map::new(b"default", 100, 100)], tx, String::new(), Some(1));
        let mut a = member(&mut g, 1, "Alpha", true);
        let mut b = member(&mut g, 2, "Beta", true);
        let mut old = member(&mut g, 3, "Old", false);
        g.parties.insert(1, vec![1, 2, 3]);
        g.party_hud_flush();
        let hp = g.client(2).unwrap().hp as u32;
        assert_eq!(huds(&mut a), vec![("Beta".into(), hp, hp, 20, 0), ("Old".into(), hp, hp, 20, 0)], "the whole roster, not itself");
        assert_eq!(huds(&mut b).len(), 2);
        assert!(huds(&mut old).is_empty(), "an original client never gets it");
        g.party_hud_flush();
        assert!(huds(&mut a).is_empty() && huds(&mut b).is_empty(), "nothing changed");
        let c = g.client_mut(2).unwrap();
        (c.hp, c.is_killed) = (0, true);
        g.party_hud_flush();
        assert_eq!(huds(&mut a), vec![("Beta".into(), 0, hp, 20, 1)], "only Beta, dead");
        assert!(huds(&mut b).is_empty(), "not about itself");
    }

    #[test]
    fn share() {
        assert_eq!(party_share(100, 1), 100);
        assert_eq!(party_share(100, 2), 50);
        assert_eq!(party_share(33, 3), 11);
        assert_eq!(party_share(100, 40), 4); // capped at MAXPARTYMEMBERS (25): 100/25 + 0.75
        assert_eq!(party_share(100, 0), 100); // never divides by zero
    }
}
