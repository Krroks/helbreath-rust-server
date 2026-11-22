//! `PutLogList` / `PutLogFileList` from Login/main.cpp. The window's log panel becomes stdout;
//! files are written relative to the working directory, as the C++ does.
use crate::msg::*;
use crate::util::{cstr, l1, to_l1};
use chrono::{Datelike, Local, Timelike};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;

pub const DEFAULT_MSG: u8 = 0;
pub const WARN_MSG: u8 = 1;
pub const INFO_MSG: u8 = 2;
const MAXLOGMSGSIZE: usize = 400;
const MAXLOGLINESIZE: usize = 5000;

pub fn put_log_file_list(msg: &[u8], file: Option<&str>) {
    let msg = cstr(msg);
    if msg.len() > MAXLOGLINESIZE {
        return;
    }
    let t = Local::now();
    let _ = create_dir_all("Logs");
    let dated = |dir: &str, what: &str| {
        let _ = create_dir_all(dir.trim_end_matches('/'));
        format!("{dir}{what} Events {:4}-{:02}-{:02}.log", t.year(), t.month(), t.day())
    };
    // "ab" for the packet dumps, "at" (LF -> CRLF on write) for everything else
    let (path, binary) = match file {
        None => ("Events.log".to_string(), false),
        Some(GM_LOGFILE) => (dated(GM_LOGFILE, "GM"), false),
        Some(PLAYER_LOGFILE) => (dated(PLAYER_LOGFILE, "Player"), false),
        Some(ITEM_LOGFILE) => (dated(ITEM_LOGFILE, "Item"), false),
        Some(CRUSADE_LOGFILE) => (dated(CRUSADE_LOGFILE, "Crusade"), false),
        Some(f @ (XSOCKET_LOGFILE | GSUNKNOWNMSG_LOGFILE | CLIENTUNKNOWNMSG_LOGFILE)) => (f.to_string(), true),
        Some(f) => (f.to_string(), false),
    };
    let mut line = to_l1(&format!("({:4}:{:02}:{:02}:{:02}:{:02}) - ", t.year(), t.month(), t.day(), t.hour(), t.minute()));
    line.extend_from_slice(msg);
    line.push(b'\n');
    if !binary {
        line = line.iter().flat_map(|&c| if c == b'\n' { vec![b'\r', b'\n'] } else { vec![c] }).collect();
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(&line);
    }
}

pub fn put_log_list(msg: &[u8], level: u8, file: Option<&str>) {
    let m = cstr(msg);
    if m.len() <= MAXLOGMSGSIZE {
        let tag = match level {
            WARN_MSG => "WARN ",
            INFO_MSG => "INFO ",
            _ => "",
        };
        println!("{tag}{}", l1(m));
    } else if m.len() > MAXLOGLINESIZE {
        return;
    }
    // faithful: the `if(PutOnFile == TRUE)` guard is commented out in the C++, every line also goes to a file
    put_log_file_list(m, file);
}

pub fn log(msg: &str) {
    put_log_list(&to_l1(msg), DEFAULT_MSG, None);
}

pub fn log_lv(msg: &str, level: u8) {
    put_log_list(&to_l1(msg), level, None);
}

pub fn log_to(msg: &str, level: u8, file: &str) {
    put_log_list(&to_l1(msg), level, Some(file));
}

pub fn log_file(msg: &str, file: Option<&str>) {
    put_log_file_list(&to_l1(msg), file);
}
