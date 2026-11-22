//! Guilds (HG.cpp): RequestCreateNewGuildHandler / ResponseCreateNewGuildHandler (~8970), RequestDisbandGuildHandler /
//! ResponseDisbandGuildHandler, JoinGuildApprove/Reject, DismissGuildApprove/Reject (~11375), SendGuildMsg (~12249),
//! UserCommand_BanGuildsman (/ban), RequestGuildNameHandler, the guild tickets of GiveItemHandler. The Login keeps the
//! guild rows (create / disband / add / remove member). Guild teleport and construction locations are crusade-only
//! in the C++ (not ported). MSGID_GUILDNOTIFY is never sent by either Login and GuildNotifyHandler does nothing (dead code, see PORTING.md).
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::util::{cstr, Rd, W};

pub const ITEM_GUILDADMISSIONTICKET: i32 = 88;
pub const ITEM_GUILDSECESSIONTICKET: i32 = 89;

/// What a player asked a guild master with a ticket (kept until the master answers or either logs out).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuildReq {
    Join,
    Leave,
}

/// RequestCreateNewGuildHandler's requirements (level, charisma, side, town). `name` is the guild name asked for.
pub fn can_create_guild(level: i32, chr: i32, neutral: bool, in_town: bool, name: &[u8]) -> bool {
    let name = cstr(name);
    // fix: the C++ accepted an empty name and "NONE" (the guild name of every guildless player)
    level >= 20 && chr >= 20 && !neutral && in_town && !name.is_empty() && !name.starts_with(b"NONE")
}

/// The ticket checks of GiveItemHandler: `giver` = (guild rank, guild name, side, neutral), `master` the same.
pub fn ticket_request(id: i32, giver: (i32, &[u8], i32, bool), master: (i32, &[u8], i32)) -> Option<GuildReq> {
    if master.0 != 0 {
        return None;
    }
    match id {
        ITEM_GUILDADMISSIONTICKET if giver.0 == -1 && !giver.3 && giver.2 == master.2 => Some(GuildReq::Join),
        ITEM_GUILDSECESSIONTICKET if giver.0 != -1 && Game::pad20(giver.1) == Game::pad20(master.1) => Some(GuildReq::Leave),
        _ => None,
    }
}

impl Game {
    fn by_name10(&self, name: &[u8]) -> Option<usize> {
        let n = Game::pad10(name);
        self.handles().into_iter().find(|&i| Game::pad10(&self.client(i).unwrap().char_name) == n)
    }

    fn guild_reply(&mut self, h: usize, msgid: u32, ok: bool) {
        let mut b = vec![0u8; 6];
        W(&mut b).u32(0, msgid);
        W(&mut b).u16(4, if ok { MSGTYPE_CONFIRM } else { MSGTYPE_REJECT } as u16);
        self.send(h, &b);
    }

    fn reset_guild(&mut self, h: usize) {
        let c = self.client_mut(h).unwrap();
        c.guild_name = b"NONE".to_vec();
        c.guild_rank = -1;
        c.guild_guid = -1;
    }

    /// MSGID_REQUEST_CREATENEWGUILD / DISBANDGUILD to the Login: char, account, password, guild (20)
    /// [+ location 10 + GUID 4 for a creation]: 75 / 56 bytes as the C++.
    fn send_guild_to_ls(&mut self, h: usize, msgid: u32) {
        let c = self.client(h).unwrap();
        let create = msgid == MSGID_REQUEST_CREATENEWGUILD;
        let mut b = vec![0u8; if create { 75 } else { 56 }];
        let mut w = W(&mut b);
        w.u32(0, msgid);
        w.u16(4, MSGTYPE_CONFIRM as u16);
        w.bytes(6, &Game::pad10(&c.char_name));
        w.bytes(16, &Game::pad10(&c.account_name));
        w.bytes(26, &Game::pad10(&c.account_password));
        w.bytes(36, &Game::pad20(&c.guild_name));
        if create {
            w.bytes(56, &Game::pad10(&c.location));
            w.u32(66, c.guild_guid as u32);
        }
        self.send_ls_plain(msgid, &b[6..]);
    }

    /// MSGID_REQUEST_UPDATEGUILDINFO_NEWGUILDSMAN / DELGUILDSMAN: char + guild name (36 bytes)
    fn send_guild_member_to_ls(&mut self, h: usize, msgid: u32) {
        let c = self.client(h).unwrap();
        let mut p = Game::pad10(&c.char_name).to_vec();
        p.extend_from_slice(&Game::pad20(&c.guild_name));
        self.send_ls_plain(msgid, &p);
    }

    /// `RequestCreateNewGuildHandler`: [id][type][char 10][account 10][password 10][guild 20]
    pub fn request_create_new_guild(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || self.cr.mode {
            return;
        }
        let name = Rd::new(d).s(36, 20);
        if c.guild_rank != -1 {
            log(&format!("(!)Cannot create guild! Already guild member.: CharName({})", crate::util::l1(&c.char_name)));
            return;
        }
        if !can_create_guild(c.level, c.range, c.is_neutral(), c.is_on_town_nonpk, &name) {
            self.guild_reply(h, MSGID_RESPONSE_CREATENEWGUILD, false);
            return;
        }
        let loc = self.maps[c.map_index].location_name.clone();
        let t = crate::chrono_now();
        let guid = (t.year + t.month + t.day + t.hour + t.minute).wrapping_add(self.now()) as i32;
        let c = self.client_mut(h).unwrap();
        c.guild_name = name;
        c.location = loc;
        c.guild_guid = guid;
        self.send_guild_to_ls(h, MSGID_REQUEST_CREATENEWGUILD);
    }

    /// `ResponseCreateNewGuildHandler`: [id][result][char 10][GuildID 4 on a confirm]
    pub fn response_create_new_guild(&mut self, d: &[u8]) {
        let r = Rd::new(d);
        let name = r.raw(6, 10);
        // fix: the C++ also wanted level >= 20 and charisma >= 20 again here, so a player who lost them meanwhile kept
        // the guild name without any answer
        let Some(h) = self.by_name10(&name) else {
            log(&format!("(!)Non-existing player data received from Log server(2): CharName({})", crate::util::l1(cstr(&name))));
            return;
        };
        let ok = r.u16(4) as i32 == LOGRESMSGTYPE_CONFIRM;
        let c = self.client_mut(h).unwrap();
        if ok {
            c.guild_rank = 0;
            // fix: the C++ kept its time-based GUID until the next login, where the Login's GuildID replaces it, so
            // guild chat between the master and members who joined meanwhile broke at each relog
            if d.len() >= 20 {
                c.guild_guid = r.u32(16) as u16 as i16 as i32;
            }
            log(&format!("(!) New guild({}) creation success! : character({})", crate::util::l1(cstr(&c.guild_name)), crate::util::l1(cstr(&c.char_name))));
        } else {
            self.reset_guild(h);
        }
        self.guild_reply(h, MSGID_RESPONSE_CREATENEWGUILD, ok);
    }

    /// `RequestDisbandGuildHandler`
    pub fn request_disband_guild(&mut self, h: usize, d: &[u8]) {
        // fix: the C++ did not check the client exists / is initialised
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || self.cr.mode {
            return;
        }
        let name = Rd::new(d).raw(36, 20);
        if c.guild_rank != 0 || Game::pad20(&c.guild_name) != Game::pad20(&name) {
            log(&format!("(!)Cannot Disband guild! Not guildmaster.: CharName({})", crate::util::l1(cstr(&c.char_name))));
            return;
        }
        self.send_guild_to_ls(h, MSGID_REQUEST_DISBANDGUILD);
    }

    /// `ResponseDisbandGuildHandler`
    pub fn response_disband_guild(&mut self, d: &[u8]) {
        let r = Rd::new(d);
        let Some(h) = self.by_name10(&r.raw(6, 10)) else { return };
        let ok = r.u16(4) as i32 == LOGRESMSGTYPE_CONFIRM;
        if ok {
            self.send_guild_msg(h, NOTIFY_GUILDDISBANDED);
            let c = self.client(h).unwrap();
            let (m, x, y) = (c.map_index, c.x, c.y);
            self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_CLEARGUILDNAME as u16, m, x, y, [0; 4]);
            self.reset_guild(h);
        }
        self.guild_reply(h, MSGID_RESPONSE_DISBANDGUILD, ok);
    }

    /// `SendGuildMsg`: to every player of h's guild on this server.
    fn send_guild_msg(&mut self, h: usize, ty: i32) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        let (guild, name) = (Game::pad20(&c.guild_name), Game::pad10(&c.char_name));
        for i in self.handles() {
            if Game::pad20(&self.client(i).unwrap().guild_name) != guild {
                continue;
            }
            let mut b = vec![0u8; 26];
            W(&mut b).u32(0, MSGID_NOTIFY);
            W(&mut b).u16(4, ty as u16);
            match ty {
                NOTIFY_GUILDDISBANDED => {
                    if i == h {
                        continue;
                    }
                    b[6..26].copy_from_slice(&guild);
                    self.send(i, &b);
                    self.reset_guild(i);
                }
                _ => {
                    // NEWGUILDSMAN / DISMISSGUILDSMAN: the name, 17 bytes
                    b[6..16].copy_from_slice(&name);
                    self.send(i, &b[..17]);
                }
            }
        }
    }

    /// The ticket part of GiveItemHandler: true when the item was a ticket used up by a request to the master.
    pub fn guild_ticket(&mut self, h: usize, master: usize, id: i32) -> bool {
        let (Some(g), Some(m)) = (self.client(h), self.client(master)) else { return false };
        let req = ticket_request(id, (g.guild_rank, &g.guild_name, g.side, g.is_neutral()), (m.guild_rank, &m.guild_name, m.side));
        // the secession ticket is given like any item during a crusade
        let Some(req) = req.filter(|r| !(self.cr.mode && *r == GuildReq::Leave)) else { return false };
        let name = g.char_name.clone();
        self.client_mut(h).unwrap().guild_request = Some((req, Game::pad10(&self.client(master).unwrap().char_name)));
        let ty = if req == GuildReq::Join { NOTIFY_QUERY_JOINGUILDREQPERMISSION } else { NOTIFY_QUERY_DISMISSGUILDREQPERMISSION };
        self.notify(master, ty, &[], Some(&name));
        true
    }

    /// The Kennedy branch of GiveItemHandler (a guild secession ticket given to the NPC outside a crusade): a member
    /// of rank other than master leaves his guild on the spot (the Login and the guild told, 300 exp lost); a master or
    /// a guildless player loses the ticket for nothing.
    pub fn kennedy_leave_guild(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        if c.guild_rank == 0 || c.guild_rank == -1 {
            return;
        }
        let old = c.guild_name.clone();
        self.send_guild_member_to_ls(h, MSGID_REQUEST_UPDATEGUILDINFO_DELGUILDSMAN);
        self.send_guild_msg(h, NOTIFY_DISMISSGUILDSMAN);
        self.notify(h, COMMONTYPE_DISMISSGUILDAPPROVE, &[], Some(&old));
        self.reset_guild(h);
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        let c = self.client_mut(h).unwrap();
        c.exp = (c.exp - 300).max(0);
        self.notify(h, NOTIFY_EXP, &[], None);
    }

    /// The master's answer target: the named player, only if it asked this master (and still qualifies).
    // fix: the C++ took any JOIN/DISMISS approve from anyone: a forged message put any player of the same side into
    // the sender's guild (even "NONE") or threw anyone out of theirs
    fn asked(&self, master: usize, name: &[u8], req: GuildReq) -> Option<usize> {
        let i = self.by_name10(name)?;
        let m = self.client(master)?;
        let c = self.client(i)?;
        if m.guild_rank != 0 || c.guild_request != Some((req, Game::pad10(&m.char_name))) {
            return None;
        }
        let ok = match req {
            GuildReq::Join => c.guild_rank == -1,
            GuildReq::Leave => c.guild_rank > 0 && Game::pad20(&c.guild_name) == Game::pad20(&m.guild_name),
        };
        ok.then_some(i)
    }

    /// `JoinGuildApproveHandler` / `JoinGuildRejectHandler` / `DismissGuildApproveHandler` / `DismissGuildRejectHandler`
    pub fn guild_answer(&mut self, h: usize, ty: i32, name: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        let req = if matches!(ty, COMMONTYPE_JOINGUILDAPPROVE | COMMONTYPE_JOINGUILDREJECT) { GuildReq::Join } else { GuildReq::Leave };
        let Some(i) = self.asked(h, name, req) else { return };
        self.client_mut(i).unwrap().guild_request = None;
        let m = self.client(h).unwrap();
        let (mguild, mguid, mloc, mside) = (m.guild_name.clone(), m.guild_guid, m.location.clone(), m.side);
        match ty {
            COMMONTYPE_JOINGUILDAPPROVE => {
                if self.client(i).unwrap().side != mside {
                    return;
                }
                let c = self.client_mut(i).unwrap();
                c.guild_name = mguild.clone();
                c.guild_guid = mguid;
                c.location = mloc;
                c.guild_rank = GUILDSTARTRANK;
                self.notify(i, COMMONTYPE_JOINGUILDAPPROVE, &[], Some(&mguild));
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
                let c = self.client(i).unwrap();
                let (m, x, y) = (c.map_index, c.x, c.y);
                self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_CLEARGUILDNAME as u16, m, x, y, [0; 4]);
                self.send_guild_msg(i, NOTIFY_NEWGUILDSMAN);
                self.send_guild_member_to_ls(i, MSGID_REQUEST_UPDATEGUILDINFO_NEWGUILDSMAN);
            }
            COMMONTYPE_DISMISSGUILDAPPROVE => {
                self.send_guild_member_to_ls(i, MSGID_REQUEST_UPDATEGUILDINFO_DELGUILDSMAN);
                self.send_guild_msg(i, NOTIFY_DISMISSGUILDSMAN);
                self.reset_guild(i);
                self.notify(i, COMMONTYPE_DISMISSGUILDAPPROVE, &[], Some(&mguild));
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
                let c = self.client(i).unwrap();
                let (m, x, y) = (c.map_index, c.x, c.y);
                self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_CLEARGUILDNAME as u16, m, x, y, [0; 4]);
            }
            _ => self.notify(i, ty, &[], Some(&mguild)),
        }
    }

    /// `UserCommand_BanGuildsman` (/ban name): the master throws a member out.
    pub fn ban_guildsman(&mut self, h: usize, cmd: &[u8]) {
        if self.client(h).unwrap().guild_rank != 0 {
            self.notify(h, NOTIFY_NOGUILDMASTERLEVEL, &[], None);
            return;
        }
        let mut t = crate::util::StrTok::new(cmd, b"= \t\n");
        let _ = t.get();
        let Some(tok) = t.get() else { return };
        let target = Game::pad10(&tok);
        let Some(i) = self.by_name10(&target) else {
            self.notify(h, NOTIFY_PLAYERNOTONGAME, &[], Some(&target));
            return;
        };
        // fix: the C++ let the master ban himself (the guild row kept a master who was no longer in it)
        if i == h || Game::pad20(&self.client(h).unwrap().guild_name) != Game::pad20(&self.client(i).unwrap().guild_name) {
            self.notify(h, NOTIFY_CANNOTBANGUILDMAN, &[], None);
            return;
        }
        self.send_guild_member_to_ls(i, MSGID_REQUEST_UPDATEGUILDINFO_DELGUILDSMAN);
        self.send_guild_msg(i, NOTIFY_DISMISSGUILDSMAN);
        self.reset_guild(i);
        // NOTIFY_SUCCESSBANGUILDMAN and COMMONTYPE_BANGUILD have no case in the C++ SendNotifyMsg: nothing is sent
        self.send_event_near_type_a(i, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        let c = self.client(i).unwrap();
        let (m, x, y) = (c.map_index, c.x, c.y);
        self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_CLEARGUILDNAME as u16, m, x, y, [0; 4]);
    }

    /// `RequestGuildNameHandler` (COMMONTYPE_REQGUILDNAME v1 = object id, v2 = index)
    pub fn request_guild_name(&mut self, h: usize, obj: i32, index: i32) {
        if obj <= 0 || obj as usize >= MAXCLIENTS as usize {
            return;
        }
        let Some(o) = self.client(obj as usize) else { return };
        let (rank, name) = (o.guild_rank, o.guild_name.clone());
        self.notify(h, NOTIFY_REQGUILDNAMEANSWER, &[rank, index], Some(&name));
    }

    /// A player leaving: requests it made or that wait for it lapse.
    pub fn guild_logout(&mut self, h: usize) {
        let n = Game::pad10(&self.client(h).unwrap().char_name);
        for i in self.handles() {
            let c = self.client_mut(i).unwrap();
            if i == h || c.guild_request.is_some_and(|r| r.1 == n) {
                c.guild_request = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_requirements() {
        assert!(can_create_guild(20, 20, false, true, b"rtguild"));
        assert!(!can_create_guild(19, 20, false, true, b"rtguild"));
        assert!(!can_create_guild(20, 19, false, true, b"rtguild"));
        assert!(!can_create_guild(20, 20, true, true, b"rtguild"));
        assert!(!can_create_guild(20, 20, false, false, b"rtguild"));
        assert!(!can_create_guild(20, 20, false, true, b"NONE"));
        assert!(!can_create_guild(20, 20, false, true, b""));
    }

    #[test]
    fn tickets() {
        let master = (0, &b"rtguild"[..], 1);
        assert_eq!(ticket_request(ITEM_GUILDADMISSIONTICKET, (-1, b"NONE", 1, false), master), Some(GuildReq::Join));
        assert_eq!(ticket_request(ITEM_GUILDADMISSIONTICKET, (-1, b"NONE", 2, false), master), None); // other side
        assert_eq!(ticket_request(ITEM_GUILDADMISSIONTICKET, (-1, b"NONE", 1, true), master), None); // neutral
        assert_eq!(ticket_request(ITEM_GUILDADMISSIONTICKET, (12, b"other", 1, false), master), None); // in a guild
        assert_eq!(ticket_request(ITEM_GUILDADMISSIONTICKET, (-1, b"NONE", 1, false), (12, b"rtguild", 1)), None); // not master
        assert_eq!(ticket_request(ITEM_GUILDSECESSIONTICKET, (12, b"rtguild", 1, false), master), Some(GuildReq::Leave));
        assert_eq!(ticket_request(ITEM_GUILDSECESSIONTICKET, (12, b"other", 1, false), master), None);
        assert_eq!(ticket_request(ITEM_GUILDSECESSIONTICKET, (-1, b"NONE", 1, false), master), None);
        assert_eq!(ticket_request(1, (-1, b"NONE", 1, false), master), None);
    }
}
