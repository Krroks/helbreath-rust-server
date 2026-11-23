//! `PutLogList` (the window's log panel -> stdout) and `PutLogFileList` (Events.log in the working directory).
//! Both only format the line and queue it: a writer thread owns stdout and the (once opened) Events.log, so the
//! world loop never waits on the console or the disk.
use chrono::{Datelike, Local, Timelike};
use std::io::Write;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;

enum Msg {
    Out(String),
    File(String),
    Flush(SyncSender<()>),
}

/// Lines queued before new ones are dropped (a stalled disk must not stall the game).
const QUEUE: usize = 16384;

fn writer(rx: Receiver<Msg>) {
    let mut file: Option<std::fs::File> = None;
    for m in rx {
        match m {
            Msg::Out(s) => println!("{s}"),
            Msg::File(s) => {
                if file.is_none() {
                    file = std::fs::OpenOptions::new().create(true).append(true).open("Events.log").ok();
                }
                if let Some(f) = file.as_mut() {
                    if f.write_all(s.as_bytes()).is_err() {
                        file = None; // reopen on the next line
                    }
                }
            }
            Msg::Flush(ack) => {
                let _ = std::io::stdout().flush();
                let _ = ack.send(());
            }
        }
    }
}

fn queue() -> &'static SyncSender<Msg> {
    static TX: OnceLock<SyncSender<Msg>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = sync_channel(QUEUE);
        std::thread::spawn(move || writer(rx));
        tx
    })
}

pub fn log(msg: &str) {
    let t = Local::now();
    let _ = queue().try_send(Msg::Out(format!("[{:02}:{:02}:{:02}] {msg}", t.hour(), t.minute(), t.second())));
}

pub fn log_file(msg: &str) {
    let t = Local::now();
    let _ = queue().try_send(Msg::File(format!("({:4}:{:02}:{:02}:{:02}:{:02}) - {msg}\r\n", t.year(), t.month(), t.day(), t.hour(), t.minute())));
}

/// Waits until everything queued so far is written (before the process ends).
pub fn flush() {
    let (ack, done) = sync_channel(1);
    if queue().send(Msg::Flush(ack)).is_ok() {
        let _ = done.recv();
    }
}

/// `std::process::exit` that does not lose the last lines.
pub fn exit(code: i32) -> ! {
    flush();
    std::process::exit(code)
}
