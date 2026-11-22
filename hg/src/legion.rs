//! The legion point services (HG.cpp + Shared/lgnSvcs.h): bSendMsgToLS(MSGID_REQUEST_LGNPTS / MSGID_REQUEST_LGNSVC) for the
//! client's requests, the Login's MSGID_RESPONSE_LGNPTS (NOTIFY_LGNPTS: the account's cash) and MSGID_RESPONSE_LGNSVC
//! (HandleLegionService: a town change, 10 000 / 100 000 gold, the legion hats and helms, majestic and reputation
//! points, only in the city hall), ChangeNation, GetLegionGold, GetLegionItem and SubstractCash (MSGID_SUBCASH).
use crate::client::{Item, ARESDEN, ELVINE, SIDE_MAP};
use crate::consts::*;
use crate::game::Game;
use crate::util::{Rd, W};

// lgnSvcs.h: the commands (the enum order is the order of the table, which HandleLegionService indexes by command)
pub const CMD_LGNSVC_MAGEHAT: u16 = 0;
pub const CMD_LGNSVC_BMAGEHAT: u16 = 1;
pub const CMD_LGNSVC_WARRIORHELM: u16 = 2;
pub const CMD_LGNSVC_BMAGEHELM: u16 = 3;
pub const CMD_LGNSVC_TOARE: u16 = 4;
pub const CMD_LGNSVC_TOELV: u16 = 5;
pub const CMD_LGNSVC_MAJ2: u16 = 6;
pub const CMD_LGNSVC_MAJ20: u16 = 7;
pub const CMD_LGNSVC_GOLD10: u16 = 8;
pub const CMD_LGNSVC_GOLD100: u16 = 9;
pub const CMD_LGNSVC_REP10: u16 = 10;
pub const CMD_LGNSVC_REP100: u16 = 11;
/// lgnPtsSvcs[].price by command
pub const LGN_PRICES: [i32; 12] = [25, 25, 25, 25, 10, 10, 1, 10, 1, 10, 1, 10];

impl Game {
    /// MSGID_REQUEST_LGNPTS to the Login: msgtype 0, the client's handle and its account (18 bytes).
    pub fn request_legion_pts(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        let mut b = vec![0u8; 18];
        let mut w = W(&mut b);
        w.u32(0, MSGID_REQUEST_LGNPTS);
        w.u16(6, h as u16);
        w.bytes(8, &Self::name10(&c.account_name));
        self.send_ls_raw(&b);
    }

    /// MSGID_REQUEST_LGNSVC to the Login: the client's first 6 bytes (its message id and the command), then the same.
    pub fn request_legion_svc(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        let r = Rd::new(d);
        let mut b = vec![0u8; 18];
        let mut w = W(&mut b);
        w.u32(0, r.u32(0));
        w.u16(4, r.u16(4));
        w.u16(6, h as u16);
        w.bytes(8, &Self::name10(&c.account_name));
        self.send_ls_raw(&b);
    }

    /// MSGID_RESPONSE_LGNPTS (the client's handle at 6, the cash as a DWORD at 8): NOTIFY_LGNPTS.
    pub fn response_legion_pts(&mut self, d: &[u8]) {
        let r = Rd::new(d);
        // (the handle comes from the Login: a handle past the client list is ignored, the C++ indexed it unchecked)
        self.notify(r.u16(6) as usize, NOTIFY_LGNPTS, &[r.u32(8) as i32], None);
    }

    /// `HandleLegionService` (MSGID_RESPONSE_LGNSVC: the command at 4, the client's handle at 6): the Login found the
    /// account's cash enough; the service is given only in the city hall, saved, and then the cash is taken
    /// (SubstractCash) - when the player could not receive it (a full bag, the same town) nothing is taken.
    pub fn handle_legion_service(&mut self, d: &[u8]) {
        let r = Rd::new(d);
        let (h, cmd) = (r.u16(6) as usize, r.u16(4));
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete || !c.map_name.starts_with(b"cityhall") {
            return;
        }
        let account = Self::name10(&c.account_name);
        match cmd {
            CMD_LGNSVC_TOARE => {
                if !self.change_nation(h, ARESDEN) {
                    return;
                }
            }
            CMD_LGNSVC_TOELV => {
                if !self.change_nation(h, ELVINE) {
                    return;
                }
            }
            CMD_LGNSVC_GOLD10 => {
                if !self.get_legion_gold(h, 10000) {
                    return;
                }
            }
            CMD_LGNSVC_GOLD100 => {
                if !self.get_legion_gold(h, 100000) {
                    return;
                }
            }
            CMD_LGNSVC_MAGEHAT | CMD_LGNSVC_BMAGEHAT | CMD_LGNSVC_WARRIORHELM | CMD_LGNSVC_BMAGEHELM => {
                if !self.get_legion_item(h, cmd) {
                    return;
                }
            }
            CMD_LGNSVC_MAJ2 | CMD_LGNSVC_MAJ20 => {
                let c = self.client_mut(h).unwrap();
                c.gizon_item_upgrade_left += 2 * LGN_PRICES[cmd as usize];
                let left = c.gizon_item_upgrade_left;
                self.notify(h, NOTIFY_GIZONITEMUPGRADELEFT, &[left, 1], None);
            }
            CMD_LGNSVC_REP10 | CMD_LGNSVC_REP100 => {
                self.client_mut(h).unwrap().reputation += 10 * LGN_PRICES[cmd as usize];
                // fix: the C++ did not tell the client, whose reputation stayed as it was until the next exp message
                self.notify(h, NOTIFY_EXP, &[], None);
            }
            _ => return,
        }
        self.send_save(h, MSGID_REQUEST_SAVEPLAYERDATA, true);
        // SubstractCash: MSGID_SUBCASH (the account and the command: the Login takes the price and logs the service)
        let mut b = vec![0u8; 18];
        let mut w = W(&mut b);
        w.u32(0, MSGID_SUBCASH);
        w.bytes(6, &account);
        w.u16(16, cmd);
        self.send_ls_raw(&b);
    }

    /// `ChangeNation`: the player becomes a citizen of the other town (location, side flag; no guild or map change).
    fn change_nation(&mut self, h: usize, side: i32) -> bool {
        let c = self.client_mut(h).unwrap();
        if c.side == side {
            return false;
        }
        c.side = side;
        c.location = SIDE_MAP[side as usize].to_vec();
        c.set_side_flag(side);
        let mut b = vec![0u8; 16];
        let mut w = W(&mut b);
        w.u32(0, MSGID_NOTIFY);
        w.u16(4, NOTIFY_RESPONSE_HUNTMODE as u16);
        w.bytes(6, &Game::pad10(&self.client(h).unwrap().location));
        self.send(h, &b);
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        if self.client(h).is_some_and(|c| c.party_id != 0) {
            self.request_delete_party(h);
        }
        true
    }

    /// `GetLegionGold`: gold into the bag (or NOTIFY_CANNOTCARRYMOREITEM and no service).
    fn get_legion_gold(&mut self, h: usize, count: u32) -> bool {
        let Some(cfg) = self.data.item_by_name(b"Gold").cloned() else { return false };
        let mut it = Item::new(&cfg);
        it.count = count;
        self.give_legion_item(h, it)
    }

    /// `GetLegionItem`: the golden hat / helm of the player's sex, bound to him, with the HP 7 / MP 7 attribute.
    fn get_legion_item(&mut self, h: usize, cmd: u16) -> bool {
        if self.item_space_left(h) == 0 {
            self.send_cannot_carry(h);
            return false;
        }
        let c = self.client(h).unwrap();
        let sex = match c.sex {
            1 => "M",
            2 => "W",
            // fix: the C++ built the item name from an uninitialised buffer for any other sex
            _ => return false,
        };
        let (name, attr) = match cmd {
            CMD_LGNSVC_MAGEHAT => ("LegionHat", 0x6100),
            CMD_LGNSVC_BMAGEHAT => ("LegionHat", 0x4100),
            CMD_LGNSVC_WARRIORHELM => ("LegionHelm", 0x4100),
            _ => ("LegionHelm", 0x6100),
        };
        let Some(cfg) = self.data.item_by_name(format!("{name}({sex})").as_bytes()).cloned() else { return false };
        let mut it = Item::new(&cfg);
        it.attribute = attr;
        (it.touch_effect_type, it.touch_effect_value) = (ITET_UNIQUE_OWNER as i16, c.char_id_num);
        self.give_legion_item(h, it)
    }

    /// The tail GetLegionGold and GetLegionItem share: _bAddClientItemList + NOTIFY_ITEMOBTAINED, or no room.
    fn give_legion_item(&mut self, h: usize, it: Item) -> bool {
        if self.add_client_item_list(h, &it) {
            self.send_item_obtained(h, &it);
            true
        } else {
            self.send_cannot_carry(h);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_follow_the_table() {
        // lgnSvcs.h: 4 items at 25, the two town changes at 10, 2 / 20 majestic points at 1 / 10, gold, reputation
        assert_eq!(LGN_PRICES[CMD_LGNSVC_BMAGEHELM as usize], 25);
        assert_eq!((LGN_PRICES[CMD_LGNSVC_TOARE as usize], LGN_PRICES[CMD_LGNSVC_TOELV as usize]), (10, 10));
        assert_eq!(2 * LGN_PRICES[CMD_LGNSVC_MAJ20 as usize], 20);
        assert_eq!(10 * LGN_PRICES[CMD_LGNSVC_REP100 as usize], 100);
    }
}
