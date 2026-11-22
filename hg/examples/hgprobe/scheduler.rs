//! `hgprobe notices LOGIN OUT.jsonl SECS`: RtHg1 waits in aresden and counts the scheduler's notices
//! (NOTIFY_EVENTSTARTING / 2 / 3) for SECS seconds. With an event 11 minutes ahead in Schedule.cfg the
//! 10-minute notice comes once in its minute (the C++ sent it every other 4 s tick).
use super::fight::keepalive;
use super::items::{enter, place};
use super::*;

pub fn notices(login: &str, out: &str, secs: u64) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    place("RtHg1", "aresden", 140, 132, ", Level=100, HP=900");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let mut seen = Vec::new();
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        p.c.send(&keepalive());
        for b in p.c.drain(Duration::from_millis(1000)) {
            let t = u16at(&b, 4) as i32;
            if u32at(&b, 0) == NOTIFY && (NOTIFY_EVENTSTARTING..=NOTIFY_EVENTSTARTING3).contains(&t) {
                println!("     notice {t} {}", hex(&b));
                seen.push(t);
            }
        }
    }
    let n3 = seen.iter().filter(|&&t| t == NOTIFY_EVENTSTARTING3).count();
    k.check(n3 == 1, &format!("EVENTSTARTING3 once in its minute: {n3} ({seen:?})"));
    k.0
}
