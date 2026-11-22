//! `_bItemLog` (the non-TAIWANLOG build): one tab-separated line per logged item action, sent to the Login as
//! MSGID_GAMEITEMLOG (the Login writes it to its item log file). CItem::m_isLogged is never set from the config
//! in the C++ (the read is commented out, so it holds whatever the allocation left): here every item counts as
//! logged, except gold stacks of 10000 or less (CItem::IsLogged). The (name, item) variant only ever logs NPC
//! drops: every other caller passes no item, which returns early.
use crate::client::Item;
use crate::consts::*;
use crate::game::Game;
use crate::util::cstr;

fn is_logged(it: &Item) -> bool {
    it.cfg.id_num != 90 || it.count > 10000 // ITEM_GOLD
}

impl Game {
    /// `_bItemLog(iAction, iGiveH, iRecvH, pItem, bForceItemLog)`: `recv` is the receiving player (exchange,
    /// give) or the bought count (buy).
    pub fn item_log(&mut self, action: i32, h: usize, recv: i32, it: &Item, force: bool) {
        if !force && (!is_logged(it) || action == ITEMLOG_RETRIEVE) {
            return;
        }
        let Some(c) = self.client(h) else { return };
        let s = |b: &[u8]| String::from_utf8_lossy(cstr(b)).into_owned();
        let other = || self.client(recv.max(0) as usize).map(|o| (s(&o.char_name), s(&o.ip_address)));
        let what = match action {
            ITEMLOG_EXCHANGE => "Exchange",
            ITEMLOG_GIVE => "Give",
            ITEMLOG_DROP => "Drop",
            ITEMLOG_GET => "Get",
            ITEMLOG_MAKE => "Make",
            ITEMLOG_DEPLETE => "Deplete",
            ITEMLOG_BUY => "Buy",
            ITEMLOG_SELL => "Sell",
            ITEMLOG_RETRIEVE => "Retrieve",
            ITEMLOG_DEPOSIT => "Deposit",
            ITEMLOG_DUPITEMID => "HaveDupItem",
            ITEMLOG_UPGRADEFAIL => "Upgrade Fail",
            ITEMLOG_UPGRADESUCCESS => "Upgrade Success",
            _ => return,
        };
        let count = if action == ITEMLOG_BUY { recv as u32 } else { it.count };
        let t = it.touch_effect_value;
        let head = format!(
            "PC({})\t{what}\t{}({count} {} {} {} {:x})\t{}({} {})",
            s(&c.char_name),
            s(&it.cfg.name),
            t[0],
            t[1],
            t[2],
            it.attribute,
            s(&c.map_name),
            c.x,
            c.y
        );
        let ip = s(&c.ip_address);
        let txt = if matches!(action, ITEMLOG_EXCHANGE | ITEMLOG_GIVE) {
            let Some((name, oip)) = other() else { return };
            format!("{head}\tPC({name})\tIP({ip}->{oip})")
        } else {
            format!("{head}\t \tIP({ip})")
        };
        self.send_ls_plain(MSGID_GAMEITEMLOG, txt.as_bytes());
    }

    /// `_bItemLog(ITEMLOG_NEWGENDROP, ..., npc name, item)`
    pub fn item_log_npc_drop(&mut self, npc: &[u8], it: &Item) {
        if !is_logged(it) {
            return;
        }
        let t = it.touch_effect_value;
        let txt = format!("NPC({})\tDrop\t{}({} {} {} {})", String::from_utf8_lossy(cstr(npc)), String::from_utf8_lossy(cstr(&it.cfg.name)), it.count, t[0], t[1], t[2]);
        // the C++ sends it on behalf of client 0 (no client): bSendMsgToLS ignores the handle for this message
        self.send_ls_plain(MSGID_GAMEITEMLOG, txt.as_bytes());
    }
}
