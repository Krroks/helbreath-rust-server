//! `hgprobe gsm LOGIN OUT.jsonl`: gate-server stock messages between two game servers (A hosts aresden, B elvfarm).
//! RtHg1 (aresden, server A) whispers to RtHg2 (elvfarm, server B): /to -> NOTIFY_WHISPERMODEON after the
//! GSM_REQUEST/RESPONSE_FINDCHARACTER round trip, then a line arrives at RtHg2 through GSM_WHISPERMSG; RtHg2 asks
//! /fi RtHg1 -> NOTIFY_PLAYERONGAME (31 bytes, no location for a non-GM); a '^' guild line (both in guild 777 by SQL)
//! reaches RtHg2 through GSM_CHATMSG.
use super::fight::keepalive;
use super::items::{enter, place};
use super::*;

pub fn ty(b: &[u8]) -> i32 {
    u16at(b, 4) as i32
}

pub fn notif<'a>(got: &'a [Vec<u8>], t: i32) -> Option<&'a Vec<u8>> {
    got.iter().find(|b| u32at(b, 0) == NOTIFY && ty(b) == t)
}

/// keepalive + `b`, then whatever both players get within `ms`
pub fn both(p: &mut Player, q: &mut Player, f: impl Fn(&Player) -> Vec<u8>, ms: u64) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    p.c.send(&keepalive());
    q.c.send(&keepalive());
    let b = f(p);
    p.c.send(&b);
    let a = p.c.drain(Duration::from_millis(ms));
    let c = q.c.drain(Duration::from_millis(300));
    (a, c)
}

pub fn chat_body(p: &Player, name: &[u8], text: &[u8]) -> Vec<u8> {
    let mut b = body(COMMAND_CHATMSG, 0, &[], 6);
    b.extend_from_slice(&p.x.to_le_bytes());
    b.extend_from_slice(&p.y.to_le_bytes());
    let mut n = name.to_vec();
    n.resize(10, 0);
    b.extend_from_slice(&n);
    b.push(0);
    b.extend_from_slice(text);
    b.push(0);
    b
}

pub fn gsm(login: &str, out: &str) -> u32 {
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2")] {
        ensure_char(login, a, c);
    }
    place("RtHg1", "aresden", 100, 100, ", HP=300, GuildName='rtgsm', GuildID=777, GuildRank=0");
    place("RtHg2", "elvfarm", 100, 100, ", HP=300, Nation='elvine', GuildName='rtgsm', GuildID=777, GuildRank=12");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    k.check(p.map == b"aresden" && q.map == b"elvfarm", &format!("maps {} / {}", String::from_utf8_lossy(&p.map), String::from_utf8_lossy(&q.map)));
    let (a, _) = both(&mut p, &mut q, |x| chat_body(x, b"RtHg1", b"/to RtHg2"), 4000);
    let on = notif(&a, NOTIFY_WHISPERMODEON);
    k.check(on.is_some_and(|b| b.len() == 27 && cstr(&b[6..16]) == b"RtHg2"), &format!("RtHg1 /to RtHg2 on the other server: WHISPERMODEON 27 {:?}", on.map(|b| hex(b))));
    let (_, c) = both(&mut p, &mut q, |x| chat_body(x, b"RtHg1", b"hello over there"), 300);
    let mut got = c;
    got.extend(q.c.drain(Duration::from_millis(3000)));
    let w = got.iter().find(|b| is_chat_with(b, b"hello over there"));
    k.check(w.is_some_and(|b| b[20] == 20 && cstr(&b[10..20]) == b"RtHg1"), &format!("RtHg2 gets the whisper (mode 20) {:?}", w.map(|b| hex(b))));
    let (_, c) = both(&mut p, &mut q, |x| chat_body(x, b"RtHg1", b"^guild over there"), 300);
    let mut got = c;
    got.extend(q.c.drain(Duration::from_millis(3000)));
    let w = got.iter().find(|b| is_chat_with(b, b" guild over there"));
    k.check(w.is_some_and(|b| b.len() == 22 + 17 && b[20] == 1 && cstr(&b[10..20]) == b"RtHg1"), &format!("'^' guild chat reaches RtHg2 on the other server (GSM_CHATMSG, mode 1) {:?}", w.map(|b| hex(b))));
    let (a, _) = both(&mut q, &mut p, |x| chat_body(x, b"RtHg2", b"/fi RtHg1"), 4000);
    let f = notif(&a, NOTIFY_PLAYERONGAME);
    k.check(f.is_some_and(|b| b.len() == 31 && cstr(&b[6..16]) == b"RtHg1" && b[16..30].iter().all(|&x| x == 0)), &format!("RtHg2 /fi RtHg1: PLAYERONGAME 31 {:?}", f.map(|b| hex(b))));
    k.0
}

/// MSGID_REQUEST_CREATENEWGUILD / DISBANDGUILD: [char 10][account 10][password 10][guild 20]
fn guild_req(msgid: u32, ch: &[u8], acc: &[u8], guild: &[u8]) -> Vec<u8> {
    body(msgid, 0, &[(ch, 10), (acc, 10), (b"rtpw1", 10), (guild, 20)], 56)
}

fn row(q: &str) -> String {
    sql(q)
}

/// `hgprobe guild LOGIN OUT.jsonl`: RtHg1 (aresden, level 60, charisma 20) creates rtguild in Aresden, RtHg2 gives it
/// a GuildAdmissionTicket (NOTIFY_QUERY_JOINGUILDREQPERMISSION), RtHg1 approves (COMMONTYPE_JOINGUILDAPPROVE 28,
/// NEWGUILDSMAN 17), guild chat '@' and '^', REQGUILDNAME, RtHg2 gives the GuildSecessionTicket and RtHg1 approves
/// (DISMISSGUILDAPPROVE 38), /ban of a non-member (CANNOTBANGUILDMAN), then RtHg1 disbands. The guild rows are checked.
pub fn guild(login: &str, out: &str) -> u32 {
    use super::items::{cmd, item_list, slot};
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2")] {
        ensure_char(login, a, c);
    }
    let stats = ", HP=300, Intelligence=67, Agility=20, GuildName='NONE', GuildID=0, GuildRank=-1";
    place("RtHg1", "aresden", 100, 100, stats);
    let id2 = place("RtHg2", "aresden", 101, 100, stats);
    sql(&format!("DELETE FROM item WHERE CharID={id2} AND CharID<>0;"));
    for (i, n) in ["GuildAdmissionTicket", "GuildSecessionTicket"].iter().enumerate() {
        sql(&format!("INSERT INTO item (CharID, ItemName, ItemID, Count, ItemType, ID1, ID2, ID3, Color, Effect1, Effect2, Effect3, LifeSpan, Attribute, ItemEquip, ItemPosX, ItemPosY) VALUES ({id2}, '{n}', {}, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 40, 40);", 9_950_000 + i));
    }
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    let (bag, _) = item_list(&q.items);
    let (adm, sec) = (slot(&bag, "GuildAdmissionTicket"), slot(&bag, "GuildSecessionTicket"));
    k.check(adm >= 0 && sec >= 0, &format!("RtHg2 has both tickets {bag:?}"));
    let (a, _) = both(&mut p, &mut q, |_| guild_req(MSGID_REQUEST_CREATENEWGUILD, b"RtHg1", b"rthg1", b"rtguild"), 2500);
    let r = a.iter().find(|b| u32at(b, 0) == MSGID_RESPONSE_CREATENEWGUILD);
    k.check(r.is_some_and(|b| b.len() == 6 && u16at(b, 4) == MSGTYPE_CONFIRM_), &format!("RESPONSE_CREATENEWGUILD confirm (6) {:?}", r.map(|b| hex(b))));
    let g = row("SELECT CONCAT(GuildName, ':', MasterName, ':', Nation, ':', NumberOfMembers) FROM guild WHERE GuildName='rtguild';");
    k.check(g == "rtguild:RtHg1:aresden:1", &format!("guild row {g:?}"));
    // the ticket
    let (pid, px, py) = (p.id, p.x, p.y);
    let (b, a) = both(&mut q, &mut p, |x| cmd(x, COMMONTYPE_GIVEITEMTOCHAR, adm as u8, [1, px as i32, py as i32], b"GuildAdmissionTicket", pid as i32), 800);
    let qn = notif(&a, NOTIFY_QUERY_JOINGUILDREQPERMISSION);
    k.check(qn.is_some_and(|b| b.len() == 16 && cstr(&b[6..16]) == b"RtHg2"), &format!("RtHg1 gets QUERY_JOINGUILDREQPERMISSION 16 {:?}", qn.map(|b| hex(b))));
    k.check(notif(&b, NOTIFY_GIVEITEMFIN_ERASEITEM).is_some(), "RtHg2's ticket is used up (GIVEITEMFIN_ERASEITEM)");
    let (a, b) = both(&mut p, &mut q, |x| cmd(x, COMMONTYPE_JOINGUILDAPPROVE, 0, [0, 0, 0], b"RtHg2", 0), 800);
    let ap = notif(&b, COMMONTYPE_JOINGUILDAPPROVE);
    k.check(ap.is_some_and(|b| b.len() == 28 && cstr(&b[6..26]) == b"rtguild" && u16at(b, 26) == 12), &format!("RtHg2 gets JOINGUILDAPPROVE 28 {:?}", ap.map(|b| hex(b))));
    let nm = notif(&a, NOTIFY_NEWGUILDSMAN);
    k.check(nm.is_some_and(|b| b.len() == 17 && cstr(&b[6..16]) == b"RtHg2"), &format!("RtHg1 gets NEWGUILDSMAN 17 {:?}", nm.map(|b| hex(b))));
    std::thread::sleep(Duration::from_millis(800));
    let m = row("SELECT CONCAT(COUNT(*), ':', (SELECT NumberOfMembers FROM guild WHERE GuildName='rtguild')) FROM guild_member WHERE GuildName='rtguild';");
    k.check(m == "2:2", &format!("guild_member rows : NumberOfMembers {m:?}"));
    // chat
    let (_, a) = both(&mut q, &mut p, |x| chat_body(x, b"RtHg2", b"@hi guild"), 300);
    let c = a.iter().find(|b| is_chat_with(b, b" hi guild"));
    k.check(c.is_some_and(|b| b[20] == 1), &format!("'@' guild chat reaches RtHg1 (mode 1) {:?}", c.map(|b| hex(b))));
    let (_, a) = both(&mut q, &mut p, |x| chat_body(x, b"RtHg2", b"^hello all"), 300);
    let c = a.iter().find(|b| is_chat_with(b, b" hello all"));
    k.check(c.is_some_and(|b| b[20] == 1), &format!("'^' guild chat reaches RtHg1 (mode 1) {:?}", c.map(|b| hex(b))));
    let (b, _) = both(&mut q, &mut p, |x| cmd(x, COMMONTYPE_REQGUILDNAME, 0, [pid as i32, 7, 0], b"", 0), 600);
    let gn = notif(&b, NOTIFY_REQGUILDNAMEANSWER);
    k.check(gn.is_some_and(|b| b.len() == 32 && u16at(b, 6) == 0 && u16at(b, 8) == 7 && cstr(&b[10..30]) == b"rtguild"), &format!("REQGUILDNAMEANSWER 32 (rank 0, index 7, rtguild) {:?}", gn.map(|b| hex(b))));
    // leaving
    let (b, a) = both(&mut q, &mut p, |x| cmd(x, COMMONTYPE_GIVEITEMTOCHAR, sec as u8, [1, px as i32, py as i32], b"GuildSecessionTicket", pid as i32), 800);
    let qn = notif(&a, NOTIFY_QUERY_DISMISSGUILDREQPERMISSION);
    k.check(qn.is_some_and(|b| b.len() == 16 && cstr(&b[6..16]) == b"RtHg2"), &format!("RtHg1 gets QUERY_DISMISSGUILDREQPERMISSION 16 {:?}", qn.map(|b| hex(b))));
    k.check(notif(&b, NOTIFY_GIVEITEMFIN_ERASEITEM).is_some(), "the secession ticket is used up");
    let (a, b) = both(&mut p, &mut q, |x| cmd(x, COMMONTYPE_DISMISSGUILDAPPROVE, 0, [0, 0, 0], b"RtHg2", 0), 800);
    let ap = notif(&b, COMMONTYPE_DISMISSGUILDAPPROVE);
    k.check(ap.is_some_and(|b| b.len() == 38 && cstr(&b[6..26]) == b"rtguild" && cstr(&b[28..38]) == b"aresden"), &format!("RtHg2 gets DISMISSGUILDAPPROVE 38 {:?}", ap.map(|b| hex(b))));
    let dm = notif(&a, NOTIFY_DISMISSGUILDSMAN);
    k.check(dm.is_some_and(|b| b.len() == 17 && cstr(&b[6..16]) == b"RtHg2"), &format!("RtHg1 gets DISMISSGUILDSMAN 17 {:?}", dm.map(|b| hex(b))));
    let (a, _) = both(&mut p, &mut q, |x| chat_body(x, b"RtHg1", b"/ban RtHg2"), 600);
    k.check(notif(&a, NOTIFY_CANNOTBANGUILDMAN).is_some_and(|b| b.len() == 6), "/ban of a non-member: CANNOTBANGUILDMAN 6");
    std::thread::sleep(Duration::from_millis(800));
    let m = row("SELECT CONCAT(COUNT(*), ':', (SELECT NumberOfMembers FROM guild WHERE GuildName='rtguild')) FROM guild_member WHERE GuildName='rtguild';");
    k.check(m == "1:1", &format!("after leaving: guild_member rows : NumberOfMembers {m:?}"));
    let (a, _) = both(&mut p, &mut q, |_| guild_req(MSGID_REQUEST_DISBANDGUILD, b"RtHg1", b"rthg1", b"rtguild"), 2500);
    let r = a.iter().find(|b| u32at(b, 0) == MSGID_RESPONSE_DISBANDGUILD);
    k.check(r.is_some_and(|b| b.len() == 6 && u16at(b, 4) == MSGTYPE_CONFIRM_), &format!("RESPONSE_DISBANDGUILD confirm (6) {:?}", r.map(|b| hex(b))));
    let g = row("SELECT COUNT(*) FROM guild WHERE GuildName='rtguild';");
    k.check(g == "0", &format!("guild row gone ({g})"));
    k.0
}

/// `hgprobe party LOGIN MAPDATA OUT.jsonl`: RtHg1 asks RtHg2 (both aresden, next to the slimes) to party
/// (NOTIFY_QUERY_JOINPARTY), RtHg2 accepts: the Login makes the party (NOTIFY_PARTY 1) and adds RtHg1 (NOTIFY_PARTY 4 to
/// both, the member list 5, coordinates); party chat '$'; RtHg1 kills slimes and RtHg2 (idle) gets exp from the share;
/// RtHg2 leaves (NOTIFY_PARTY 2 to both: a party of one is deleted).
pub fn party(login: &str, mapdir: &str, out: &str) -> u32 {
    use super::fight::{amd, hunt, Fight};
    use super::items::cmd;
    let mut k = Checks(0);
    let log = std::fs::File::create(out).expect("out");
    for (a, c) in [(b"rthg1", b"RtHg1"), (b"rthg2", b"RtHg2")] {
        ensure_char(login, a, c);
    }
    let walk = amd(mapdir, "aresden");
    let (sx, sy) = (110..=120).flat_map(|x| (56..=60).map(move |y| (x, y))).find(|&(x, y)| walk(x, y) && walk(x + 1, y)).expect("free tiles near the slimes");
    place("RtHg1", "aresden", sx, sy, ", HP=600, PartyID=0");
    place("RtHg2", "aresden", sx + 1, sy, ", HP=600, PartyID=0");
    let Some(mut p) = enter(login, b"rthg1", b"RtHg1", 1, &log, &mut k) else { return k.0 + 1 };
    let Some(mut q) = enter(login, b"rthg2", b"RtHg2", 2, &log, &mut k) else { return k.0 + 1 };
    let party_n = |got: &[Vec<u8>]| -> Vec<(usize, u16, u16, String)> {
        got.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_PARTY).map(|b| (b.len(), u16at(b, 6), u16at(b, 8), String::from_utf8_lossy(cstr(b.get(12..22).unwrap_or(&[]))).to_string())).collect()
    };
    let (_, b) = both(&mut p, &mut q, |x| cmd(x, COMMONTYPE_REQUEST_JOINPARTY, 0, [1, 0, 0], b"RtHg2", 0), 600);
    let qj = notif(&b, NOTIFY_QUERY_JOINPARTY);
    k.check(qj.is_some_and(|b| b.len() == 12 && cstr(&b[6..]) == b"RtHg1"), &format!("RtHg2 gets QUERY_JOINPARTY 12 {:?}", qj.map(|b| hex(b))));
    let (mut b, mut a) = both(&mut q, &mut p, |x| cmd(x, COMMONTYPE_REQUEST_ACCEPTJOINPARTY, 0, [1, 0, 0], b"", 0), 2500);
    a.extend(p.c.drain(Duration::from_millis(1500)));
    b.extend(q.c.drain(Duration::from_millis(300)));
    let (pa, pb) = (party_n(&a), party_n(&b));
    println!("     NOTIFY_PARTY to RtHg1 {pa:?}, to RtHg2 {pb:?}");
    k.check(pb.iter().any(|x| x.0 == 14 && x.1 == 1 && x.2 == 1), "RtHg2: NOTIFY_PARTY 1,1 (14, party made)");
    k.check(pa.iter().any(|x| x.0 == 22 && x.1 == 4 && x.2 == 1 && x.3 == "RtHg1") && pb.iter().any(|x| x.0 == 22 && x.1 == 4 && x.2 == 1 && x.3 == "RtHg1"), "both: NOTIFY_PARTY 4,1 RtHg1 (22, joined)");
    k.check(pa.iter().any(|x| x.0 == 34 && x.1 == 5 && x.2 == 1), "RtHg1: NOTIFY_PARTY 5 member list (12 + 2 x 11)");
    let co = notif(&b, NOTIFY_PARTY_COORDS);
    k.check(co.is_some_and(|b| b.len() == 40 && cstr(&b[6..16]) == b"RtHg1"), &format!("RtHg2 gets RtHg1's PARTY_COORDS 40 {:?}", co.map(|b| hex(b))));
    let (_, b) = both(&mut p, &mut q, |x| chat_body(x, b"RtHg1", b"$party hi"), 300);
    let c = b.iter().find(|b| is_chat_with(b, b" party hi"));
    k.check(c.is_some_and(|b| b[20] == 4), &format!("'$' party chat reaches RtHg2 (mode 4) {:?}", c.map(|b| hex(b))));
    // exp share
    let mut tog = body(MSGID_COMMAND_COMMON, COMMONTYPE_TOGGLECOMBATMODE as u16, &[], 6);
    tog.extend_from_slice(&p.x.to_le_bytes());
    tog.extend_from_slice(&p.y.to_le_bytes());
    tog.resize(57, 0);
    p.c.send(&tog);
    let mut f = Fight::default();
    f.hp = i32::MAX;
    let (mut q_exp, mut q_got) = (0, Vec::new());
    let end = Instant::now() + Duration::from_secs(150);
    while Instant::now() < end && (f.kills < 1 || q_exp == 0) && !f.killed {
        hunt(&mut p, &mut f, &walk, true, 5, &|f| f.kills >= 1);
        q.c.send(&keepalive());
        for b in q.c.drain(Duration::from_millis(200)) {
            if u32at(&b, 0) == NOTIFY && ty(&b) == NOTIFY_EXP {
                q_exp += 1;
            }
            q_got.push(b);
        }
    }
    k.check(f.kills >= 1, &format!("RtHg1 killed {} slime(s), {} exp notifies", f.kills, f.exp_notes));
    k.check(q_exp > 0, &format!("RtHg2 (idle) gets exp from the party share: {q_exp} NOTIFY_EXP"));
    let co: Vec<usize> = q_got.iter().filter(|b| u32at(b, 0) == NOTIFY && ty(b) == NOTIFY_PARTY_COORDS).map(|b| b.len()).collect();
    println!("     RtHg2 got {} PARTY_COORDS while RtHg1 hunted", co.len());
    // leaving
    let (mut b, mut a) = both(&mut q, &mut p, |x| cmd(x, COMMONTYPE_REQUEST_ACCEPTJOINPARTY, 0, [2, 0, 0], b"", 0), 2500);
    a.extend(p.c.drain(Duration::from_millis(1000)));
    b.extend(q.c.drain(Duration::from_millis(300)));
    let (pa, pb) = (party_n(&a), party_n(&b));
    println!("     leaving: NOTIFY_PARTY to RtHg1 {pa:?}, to RtHg2 {pb:?}");
    // a party of two: the Login deletes it (op 2 to all) before its op-6 answer, which then finds nobody in it
    k.check(pa.iter().any(|x| x.0 == 14 && x.1 == 2) && pb.iter().any(|x| x.0 == 14 && x.1 == 2), "both: NOTIFY_PARTY 2 (party deleted)");
    k.0
}
