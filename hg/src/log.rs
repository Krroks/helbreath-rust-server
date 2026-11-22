//! `PutLogList` (the window's log panel -> stdout) and `PutLogFileList` (Events.log in the working directory).
use chrono::{Datelike, Local, Timelike};
use std::io::Write;

pub fn log(msg: &str) {
    let t = Local::now();
    println!("[{:02}:{:02}:{:02}] {msg}", t.hour(), t.minute(), t.second());
}

pub fn log_file(msg: &str) {
    let t = Local::now();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("Events.log") {
        let _ = write!(f, "({:4}:{:02}:{:02}:{:02}:{:02}) - {msg}\r\n", t.year(), t.month(), t.day(), t.hour(), t.minute());
    }
}
