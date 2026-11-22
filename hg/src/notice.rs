//! notice.txt and noticement.txt (HG.cpp): bReadNotifyMsgListFile + NoticeHandler (a random `notify_msg = text`
//! line to everyone every NOTICETIME, 3 s timer), RequestNoticementHandler (the client's noticement download: the
//! Login's noticement.txt when the client's size differs, else a CONFIRM).
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::util::StrTok;

/// `bReadNotifyMsgListFile`: the token after each `notify_msg` (seps "=\t\n;"), at most MAXNOTIFYMSGS.
pub fn parse_notices(txt: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut t = StrTok::new(txt, b"=\t\n;");
    let mut want = false;
    while let Some(tok) = t.get() {
        if want {
            if out.len() < MAXNOTIFYMSGS as usize {
                out.push(tok);
            }
            want = false;
        } else if tok.starts_with(b"notify_msg") {
            want = true;
        }
    }
    out
}

impl Game {
    pub fn notice_init(&mut self) {
        match std::fs::read("notice.txt") {
            Ok(raw) => self.notices = parse_notices(&crate::util::text_mode(&raw)),
            Err(_) => log("(!) Notify Message list file not found!..."),
        }
    }

    /// `NoticeHandler` (3 s timer): needs two notices at least, never the same one twice in a row
    pub fn notice_handler(&mut self) {
        let n = self.notices.len() as i32;
        let now = self.now();
        if n <= 1 || now.wrapping_sub(self.notice_time) <= NOTICETIME as u32 {
            return;
        }
        self.notice_time = now;
        let i = loop {
            let i = self.rng.dice(1, n) - 1;
            if i != self.prev_notice {
                break i;
            }
        };
        self.prev_notice = i;
        let t = self.notices[i as usize].clone();
        for h in self.handles() {
            self.notify(h, NOTIFY_NOTICEMSG, &[], Some(&t));
        }
    }

    /// `RequestNoticementHandler`: the client sends the size of its copy (int at 6)
    pub fn request_noticement(&mut self, h: usize, d: &[u8]) {
        let data = &self.data.noticement;
        if data.len() < 10 {
            return;
        }
        let size = d.get(6..10).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()));
        let mut b = MSGID_RESPONSE_NOTICEMENT.to_le_bytes().to_vec();
        if size as usize != data.len() {
            b.extend_from_slice(&(MSGTYPE_REJECT as u16).to_le_bytes());
            b.extend_from_slice(data);
            b.extend_from_slice(&[0, 0]);
        } else {
            b.extend_from_slice(&(MSGTYPE_CONFIRM as u16).to_le_bytes());
        }
        self.send(h, &b);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse() {
        let n = super::parse_notices(b"notify_msg = Hello_all\nnotify_msg\tSecond;junk\nwelcome_line\n");
        // the space is not a separator: the C++ kept it too
        assert_eq!(n, vec![b" Hello_all".to_vec(), b"Second".to_vec()], "lines without notify_msg are not notices");
        assert!(super::parse_notices(b"welcome_To_Helbreath\nEnjoy\n").is_empty());
    }
}
