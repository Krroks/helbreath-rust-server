//! `hgprobe quest LOGIN MAPDATA OUT.jsonl`: quests. Session 1 (RtHg1, level 15, aresden town): an npc with nothing
//! to say (NPCTALK 130 + type), the guard (200), the city hall offers a level 11-20 monster hunt in arefarm, accept
//! (QUESTCONTENTS + QUESTCOUNTER), ask again (101: not done), cancel (QUESTABORTED), a second accept without a talk
//! does nothing (Rust fix). Session 2: quest 3 (30 slimes in arefarm) at 29, placed next to the arefarm slimes:
//! entry counter 1, one slime kill -> QUESTCOUNTER 0 + QUESTCOMPLETED, the city hall pays the exp reward (QUESTREWARD
//! 4,1,450, contribution +2).
use super::fight::{amd, hunt, keepalive, Fight};
use super::items::{cmd, enter, place};
use super::social::{notif, ty};
use super::*;

fn talk(p: &mut Player, who: i32, ms: u64) -> Vec<Vec<u8>> {
    p.c.send(&keepalive());
    let b = cmd(p, COMMONTYPE_TALKTONPC, 0, [who, 0, 0], b"", 0);
    p.c.send(&b);
    p.c.drain(Duration::from_millis(ms))
}

fn quest_notes(got: &[Vec<u8>]) -> Vec<String> {
    got.iter().filter(|b| u32at(b, 0) == NOTIFY && (2900..=2925).contains(&ty(b)) || u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_QUESTCOUNTER).map(|b| hex(b)).collect()
}

pub fn quest(login: &str, mapdir: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    ensure_char(login, b"rthg1", b"RtHg1");
    let reset = ", Contribution=0, QuestNum=0, QuestID=0, QuestCount=0, QuestRewType=0, QuestRewAmmount=0, QuestCompleted=0";
    place("RtHg1", "aresden", 100, 100, &format!(", Level=15, HP=300{reset}"));
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"aresden", "entered aresden");
    let n0 = |got: &[Vec<u8>]| notif(got, NOTIFY_NPCTALK).map(|b| (b.len(), u16at(b, 6)));
    let got = talk(&mut p, 15, 600);
    k.check(n0(&got) == Some((64, 145)), &format!("npc 15: NPCTALK 64, 145 {:?}", n0(&got)));
    let got = talk(&mut p, 21, 600);
    k.check(n0(&got) == Some((64, 200)), &format!("guard, aresdener in aresden: NPCTALK 200 {:?}", n0(&got)));
    let got = talk(&mut p, 4, 600);
    let offer = notif(&got, NOTIFY_NPCTALK).cloned().unwrap_or_default();
    println!("     city hall offer {}", hex(&offer));
    let target_type = u16at(&offer, 14);
    k.check(
        offer.len() == 64 && u16at(&offer, 6) == 1 && [56, 10, 16].contains(&target_type) && cstr(&offer[44..64]) == b"arefarm",
        "city hall offers a monster hunt in arefarm (type 1, target 56/10/16)",
    );
    p.c.send(&cmd(&p, COMMONTYPE_QUESTACCEPTED, 0, [0, 0, 0], b"", 0));
    let got = p.c.drain(Duration::from_millis(600));
    let qc = notif(&got, NOTIFY_QUESTCONTENTS);
    let cnt = notif(&got, NOTIFY_QUESTCOUNTER);
    k.check(qc.is_some_and(|b| b.len() == 44 && u16at(b, 6) == 4 && u16at(b, 8) == 1 && u16at(b, 12) == target_type && cstr(&b[24..44]) == b"arefarm"), &format!("accept: QUESTCONTENTS 44 {:?}", qc.map(|b| hex(b))));
    k.check(cnt.is_some_and(|b| b.len() == 24 && u32at(b, 6) == u16at(&offer, 16) as u32), &format!("accept: QUESTCOUNTER 24 = the count {:?}", cnt.map(|b| hex(b))));
    let got = talk(&mut p, 4, 600);
    k.check(n0(&got) == Some((64, 101)), &format!("city hall while on the quest: 101 {:?}", n0(&got)));
    p.c.send(&cmd(&p, COMMONTYPE_REQUEST_CANCELQUEST, 0, [0, 0, 0], b"", 0));
    let got = p.c.drain(Duration::from_millis(600));
    k.check(notif(&got, NOTIFY_QUESTABORTED).is_some_and(|b| b.len() == 8), "cancel: QUESTABORTED 8");
    p.c.send(&cmd(&p, COMMONTYPE_QUESTACCEPTED, 0, [0, 0, 0], b"", 0));
    let got = p.c.drain(Duration::from_millis(800));
    k.check(notif(&got, NOTIFY_QUESTCONTENTS).is_none(), &format!("second accept without a talk: nothing {:?}", quest_notes(&got)));
    drop(p);

    // session 2: quest 3 (slimes, 30, arefarm) at 29 kills, the exp reward (-1, 450) chosen at the talk
    let walk = amd(mapdir, "arefarm");
    let (sx, sy) = (118..=136).flat_map(|x| (103..=108).map(move |y| (x, y))).find(|&(x, y)| walk(x, y)).expect("free tile south of the slimes");
    std::thread::sleep(Duration::from_secs(2));
    place(
        "RtHg1",
        "arefarm",
        sx,
        sy,
        ", Level=30, Strenght=40, Vitality=30, Dexterity=57, Intelligence=10, Magic=10, Agility=10, HP=400, Contribution=0, QuestNum=3, QuestID=354, QuestCount=29, QuestRewType=-1, QuestRewAmmount=450, QuestCompleted=0",
    );
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"arefarm", &format!("entered arefarm at ({},{})", p.x, p.y));
    let entry: Vec<Vec<u8>> = p.c.drain(Duration::from_millis(600));
    let _ = entry; // the entry counter came with init_player's messages; checked through the kill below
    let mut tog = body(MSGID_COMMAND_COMMON, COMMONTYPE_TOGGLECOMBATMODE as u16, &[], 6);
    tog.extend_from_slice(&p.x.to_le_bytes());
    tog.extend_from_slice(&p.y.to_le_bytes());
    tog.resize(57, 0);
    p.c.send(&tog);
    let mut f = Fight::default();
    f.hp = i32::MAX;
    hunt(&mut p, &mut f, &walk, true, 180, &|f| f.quest.iter().any(|b| ty(b) == NOTIFY_QUESTCOMPLETED));
    let notes: Vec<String> = f.quest.iter().map(|b| hex(b)).collect();
    println!("     kills {} quest notes {notes:?}", f.kills);
    k.check(f.quest.iter().any(|b| ty(b) == NOTIFY_QUESTCOUNTER && b.len() == 24 && u32at(b, 6) == 0), "a slime kill: QUESTCOUNTER 0");
    k.check(f.quest.iter().any(|b| ty(b) == NOTIFY_QUESTCOMPLETED && b.len() == 8), "QUESTCOMPLETED 8");
    let got = talk(&mut p, 4, 800);
    let r = notif(&got, NOTIFY_QUESTREWARD);
    println!("     reward {:?}", r.map(|b| hex(b)));
    k.check(
        r.is_some_and(|b| b.len() == 38 && u16at(b, 6) == 4 && u16at(b, 8) == 1 && u32at(b, 10) == 450 && b[14..20] == *b"\xb0\xe6\xc7\xe8\xc4\xa1" && u32at(b, 34) == 2),
        "city hall pays: QUESTREWARD 38 (4, 1, 450, exp name, contribution 2)",
    );
    let got = talk(&mut p, 4, 600);
    k.check(n0(&got).is_some_and(|x| x.0 == 64), &format!("city hall after the reward talks again {:?}", n0(&got)));
    k.0
}
