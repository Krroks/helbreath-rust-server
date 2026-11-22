//! Port of Login/char/PartyManager.cpp. Messages go out through the returned `Out` list instead of
//! calling back into the server. Replies go to the requesting game-server *socket*; members record
//! the game-server *index* they play on, so `game_server_down` can find them.
use crate::msg::{MAXPARTYMEMBERS, MSGID_PARTYOPERATION};
use crate::util::{cstr, W};

pub const MAXPARTY: usize = 200;

pub enum Out {
    /// `SendMsgToAllGameServers(NULL, .., TRUE)`
    All(Vec<u8>),
    /// `SendMsgToGS(iClientH, ..)`
    Gs(usize, Vec<u8>),
}

#[derive(Default, Clone)]
struct Member {
    party_id: i32,
    index: i32,
    name: Vec<u8>,
    server_change_time: u32,
}

pub struct PartyManager {
    member_num: [i32; MAXPARTY],
    members: Vec<Member>,
    check_act_time: u32,
}

fn msg(len: usize) -> Vec<u8> {
    let mut d = vec![0u8; len];
    W(&mut d).u32(0, MSGID_PARTYOPERATION);
    d
}

/// `memcpy(cp, name, 10)` from a 12-byte zeroed name buffer.
fn name10(name: &[u8]) -> Vec<u8> {
    let mut n = cstr(name).to_vec();
    n.resize(10, 0);
    n
}

impl PartyManager {
    pub fn new(now: u32) -> Self {
        PartyManager { member_num: [0; MAXPARTY], members: vec![Member::default(); MAXPARTY], check_act_time: now }
    }

    fn clear(&mut self, i: usize) {
        self.members[i] = Member::default();
    }

    /// Party id of `name` if it is listed in `party_id` (used by GetCharacterInfo).
    pub fn is_member(&self, party_id: i32, name: &[u8]) -> bool {
        (1..MAXPARTY).any(|i| self.members[i].party_id == party_id && self.members[i].name == cstr(name))
    }

    pub fn create_new_party(&mut self, client_h: i32, master: &[u8]) -> i32 {
        let master = cstr(master);
        if (1..MAXPARTY).any(|i| self.members[i].party_id != 0 && self.members[i].name == master) {
            return 0;
        }
        let Some(party_id) = (1..MAXPARTY).find(|&i| self.member_num[i] == 0) else { return 0 };
        self.member_num[party_id] += 1;
        for i in 1..MAXPARTY {
            if self.members[i].party_id == 0 {
                self.members[i] = Member { party_id: party_id as i32, index: client_h, name: master.to_vec(), server_change_time: 0 };
                return party_id as i32;
            }
        }
        0
    }

    pub fn delete_party(&mut self, party_id: i32, out: &mut Vec<Out>) -> bool {
        let mut flag = false;
        if let Some(n) = self.member_num.get_mut(party_id as usize) {
            *n = 0;
        }
        for i in 1..MAXPARTY {
            if self.members[i].party_id == party_id {
                self.clear(i);
                flag = true;
            }
        }
        let mut d = msg(120);
        let mut w = W(&mut d);
        w.u16(4, 2);
        w.u16(6, party_id as u16);
        d.truncate(10);
        out.push(Out::All(d));
        flag
    }

    fn dec(&mut self, party_id: i32, out: &mut Vec<Out>) {
        let Some(n) = self.member_num.get_mut(party_id as usize) else { return };
        *n -= 1;
        if *n <= 1 {
            self.delete_party(party_id, out);
        }
    }

    pub fn add_member(&mut self, client_h: i32, party_id: i32, name: &[u8], out: &mut Vec<Out>) -> bool {
        let name = cstr(name);
        // party 0 is the "free slot" marker, ids past the table cannot be counted
        if party_id <= 0 || party_id as usize >= MAXPARTY || self.member_num[party_id as usize] >= MAXPARTYMEMBERS {
            return false;
        }
        for i in 1..MAXPARTY {
            if self.members[i].party_id != 0 && self.members[i].name == name {
                let old = self.members[i].party_id;
                self.clear(i);
                // fixed: the C++ decremented the *target* party instead of the one the member left
                self.dec(old, out);
            }
        }
        for i in 1..MAXPARTY {
            if self.members[i].party_id == 0 {
                self.members[i] = Member { party_id, index: client_h, name: name.to_vec(), server_change_time: 0 };
                if let Some(n) = self.member_num.get_mut(party_id as usize) {
                    *n += 1;
                }
                return true;
            }
        }
        false
    }

    pub fn remove_member(&mut self, party_id: i32, name: &[u8], out: &mut Vec<Out>) -> bool {
        let name = cstr(name).to_vec();
        for i in 1..MAXPARTY {
            if self.members[i].party_id == party_id && self.members[i].name == name {
                self.clear(i);
                self.dec(party_id, out);
                return true;
            }
        }
        false
    }

    pub fn check_party_member(&mut self, client_h: i32, gsch: i32, party_id: i32, name: &[u8], out: &mut Vec<Out>) -> bool {
        for i in 1..MAXPARTY {
            if self.members[i].party_id == party_id && self.members[i].name == cstr(name) {
                self.members[i].server_change_time = 0;
                return true;
            }
        }
        let mut d = msg(120);
        let mut w = W(&mut d);
        w.u16(4, 3);
        w.u16(6, gsch as u16);
        w.bytes(8, &name10(name));
        d.truncate(20);
        out.push(Out::Gs(client_h as usize, d));
        false
    }

    pub fn get_party_info(&mut self, client_h: i32, gsch: i32, name: &[u8], party_id: i32, out: &mut Vec<Out>) -> bool {
        let mut d = msg(1024);
        let mut w = W(&mut d);
        w.u16(4, 5);
        w.u16(6, gsch as u16);
        w.bytes(8, &name10(name));
        let mut total = 0usize;
        for i in 1..MAXPARTY {
            if self.members[i].party_id == party_id && self.members[i].party_id != 0 {
                w.bytes(20 + total * 11, &name10(&self.members[i].name));
                total += 1;
            }
        }
        w.u16(18, total as u16);
        d.truncate(20 + total * 11 + 1);
        out.push(Out::Gs(client_h as usize, d));
        true
    }

    /// Drops the members playing on game server `server`.
    pub fn game_server_down(&mut self, server: i32) {
        // fixed: the C++ started at slot 0 and also matched empty slots (index 0), corrupting the counts
        for i in 1..MAXPARTY {
            if self.members[i].party_id != 0 && self.members[i].index == server {
                let p = self.members[i].party_id as usize;
                if let Some(n) = self.member_num.get_mut(p) {
                    *n -= 1;
                }
                self.clear(i);
            }
        }
    }

    pub fn set_server_change_status(&mut self, name: &[u8], party_id: i32, now: u32) {
        for i in 1..MAXPARTY {
            if self.members[i].party_id == party_id && self.members[i].name == cstr(name) {
                self.members[i].server_change_time = now;
                return;
            }
        }
    }

    pub fn check_member_activity(&mut self, now: u32, out: &mut Vec<Out>) {
        if now.wrapping_sub(self.check_act_time) > 1000 * 2 {
            self.check_act_time = now;
        } else {
            return;
        }
        for i in 1..MAXPARTY {
            let m = self.members[i].clone();
            if m.server_change_time != 0 && now.wrapping_sub(m.server_change_time) > 1000 * 20 {
                let mut d = msg(120);
                let mut w = W(&mut d);
                w.u16(4, 6);
                w.u8(6, 1);
                w.u16(7, 0);
                w.bytes(9, &name10(&m.name));
                w.u16(19, m.party_id as u16);
                d.truncate(22);
                out.push(Out::All(d));
                self.remove_member(m.party_id, &m.name, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_member_leaves_old_party() {
        let mut p = PartyManager::new(0);
        let mut out = Vec::new();
        let a = p.create_new_party(0, b"Alice");
        assert!(p.add_member(0, a, b"Bob", &mut out));
        assert!(p.add_member(0, a, b"Carl", &mut out));
        let b = p.create_new_party(1, b"Dora");
        assert!(p.add_member(1, b, b"Eve", &mut out));
        // Carl moves from party a (3 members) to party b: a keeps 2, b grows to 3
        assert!(p.add_member(1, b, b"Carl", &mut out));
        assert_eq!((p.member_num[a as usize], p.member_num[b as usize]), (2, 3));
        assert!(out.is_empty(), "no party should have been disbanded");
        assert!(!p.add_member(0, 0, b"Zed", &mut out));
    }

    #[test]
    fn game_server_down_skips_empty_slots() {
        let mut p = PartyManager::new(0);
        let mut out = Vec::new();
        let a = p.create_new_party(1, b"Alice");
        p.add_member(1, a, b"Bob", &mut out);
        let b = p.create_new_party(0, b"Dora");
        p.add_member(0, b, b"Eve", &mut out);
        p.game_server_down(0);
        assert_eq!(p.member_num[0], 0, "empty slots must not be counted");
        assert_eq!(p.member_num[a as usize], 2);
        assert!(!p.is_member(b, b"Eve"));
        assert!(p.is_member(a, b"Bob"));
    }
}
