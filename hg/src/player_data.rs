//! The player data block the Login sends (MSGID_RESPONSE_PLAYERDATA) and the HG sends back on save:
//! `_bDecodePlayerDatafileContents` (HG.cpp ~4570) and `_iComposePlayerDataFileContents` (~4951).
//! Layout: fixed character fields up to 751, item count at 751, bag items of 64 bytes from 752, one spare byte,
//! the bank count, then bank items of 59 bytes.
use crate::client::*;
use crate::game::Game;
use crate::log::{log, log_file};
use crate::util::{atoi, cstr, l1, Rd, StrTok, W};

const CHAR_INDEX_END: usize = 751;

/// (mastery byte, SSN dword) offsets of skill `b`: the C++ arrays at 598 (10 of the 55 bytes before 653) and 653
/// (10 of the dwords before 751). Skill 10 lands at 608 / 693, unused by the C++ HG and Login (Rust only).
fn skill_offsets(b: usize) -> (usize, usize) {
    (598 + b, 653 + b * 4)
}
const ITEMTYPE_NOTUSED: i32 = -1;
pub const ITEMTYPE_EQUIP: i32 = 1;
const ITEMEFFECTTYPE_ALTERITEMDROP: i32 = 26;
const ITET_UNIQUE_OWNER: i16 = 1;

fn item_fields(it: &mut Item, r: Rd<'_>) {
    it.touch_effect_type = r.u16(24) as i16;
    it.touch_effect_value = [r.u32(26) as i16, r.u32(30) as i16, r.u32(34) as i16];
    it.item_color = r.u8(38);
    it.item_spec_effect_value = [r.u16(39) as i16, r.u16(41) as i16, r.u16(43) as i16];
    it.cur_life_span = r.u16(45);
    it.attribute = r.u32(47);
}

/// The C++ post-processing of a loaded item (durability from upgrades, stat nibble, zemstone fix).
fn fix_item(it: &mut Item, name: &[u8]) {
    // the C++ "magic jewel" hack tests id > 884 && id < 880, which is never true; nothing to port
    if (it.attribute >> 28) > 0 && matches!(it.cfg.category, 5 | 6) {
        it.max_life_span = it.item_spec_effect_value[0] as u16;
    }
    it.adjust_by_stat();
    if it.cur_life_span > it.max_life_span {
        it.cur_life_span = it.max_life_span;
    }
    if it.cur_life_span == 0 && it.cfg.item_effect_type == ITEMEFFECTTYPE_ALTERITEMDROP {
        log_file(&format!("(!) Player({}) has a Zemstone with lifespan = 0!", l1(name)));
        it.cur_life_span = 1;
    }
}

/// `_bDecodePlayerDatafileContents`; `d` starts at the char ID (body offset 16 of RESPONSE_PLAYERDATA).
pub fn decode(g: &mut Game, h: usize, d: &[u8]) -> bool {
    let r = Rd::new(d);
    let max_load_base;
    let mut not_used_price = 0i32;
    {
        let c = g.client_mut(h).unwrap();
        c.char_id = r.u32(0);
        c.char_id_num = [r.u32(4) as i16, r.u32(8) as i16, r.u32(12) as i16];
        c.level = r.u16(16) as i32;
        c.str_ = r.u8(18) as i32;
        c.vit = r.u8(19) as i32;
        c.dex = r.u8(20) as i32;
        c.int_ = r.u8(21) as i32;
        c.mag = r.u8(22) as i32;
        c.range = r.u8(23) as i32;
        c.luck = r.u8(24) as i32;
        c.exp = r.u32(25);
        c.sex = r.u8(29);
        c.skin = r.u8(30);
        c.hair_style = r.u8(31);
        c.hair_color = r.u8(32);
        c.underwear = r.u8(33);
        c.location = r.s(54, 10);
        c.map_name = r.s(64, 10);
    }
    let map_name = g.client(h).unwrap().map_name.clone();
    let Some(mi) = g.map_index(&map_name) else {
        log(&format!("(!) Player({}) tries to enter unknown map : {}", l1(&g.client(h).unwrap().char_name), l1(&map_name)));
        return false;
    };
    let c = g.client_mut(h).unwrap();
    c.map_index = mi;
    c.x = r.u16(74) as i16;
    c.y = r.u16(76) as i16;
    c.contribution = r.u32(78) as i32;
    c.special_ability_time = r.u32(82) as i32;
    c.locked_map_name = r.s(86, 10);
    c.locked_map_time = r.u32(96) as i32;
    let block_date = r.s(100, 20);
    let mut t = StrTok::new(&block_date, b" :-");
    for k in 0..3 {
        c.penalty_block[k] = atoi(&t.get().unwrap_or_default());
    }
    c.guild_name = r.s(120, 20);
    c.guild_guid = r.u16(140) as i16 as i32;
    c.guild_rank = r.u8(142) as i8 as i32;
    c.fightzone_number = r.u8(143) as i32;
    c.reserve_time = r.u32(144) as i32;
    c.fightzone_ticket_number = r.u8(148) as i32;
    c.quest = r.u16(149) as i32;
    c.quest_id = r.u32(151);
    c.cur_quest_count = r.u16(155) as i32;
    c.quest_reward_type = r.u16(157) as i16 as i32;
    c.quest_reward_amount = r.u32(159) as i32;
    c.is_quest_completed = r.u8(163) != 0;
    c.special_event_id = r.u32(164) as i32;
    c.war_contribution = r.u32(168) as i32;
    c.crusade_duty = r.u8(172) as i32;
    c.crusade_guid = r.u32(173);
    c.construction_point = r.u32(177) as i32;
    c.reputation = r.u32(181) as i32;
    c.hp = r.u32(185) as i32;
    c.mp = r.u32(189) as i32;
    c.sp = r.u32(193) as i32;
    c.enemy_kill_count = r.u32(197) as i32;
    c.pk_count = r.u32(201) as i32;
    c.reward_gold = r.u32(205) as i32;
    c.down_skill_index = r.u8(209) as i8 as i32;
    c.hunger_status = r.u8(210) as i32;
    c.super_attack_left = r.u16(211) as i32;
    c.time_left_shutup = r.u32(213) as i32;
    c.time_left_rating = r.u32(217) as i32;
    c.time_left_force_recall = r.u32(221) as i32;
    c.time_left_firm_stamina = r.u32(225) as i32;
    c.dead_penalty_time = r.u32(229) as i32;
    c.party_id = r.u32(233) as i32;
    c.gizon_item_upgrade_left = r.u16(237) as i32;
    c.elo = r.u16(239) as i32;
    c.admin_user_level = r.u8(242) as i32;
    for i in 0..MAXMAGICTYPE {
        c.magic_mastery[i] = r.u8(243 + i);
    }
    c.profile = r.s(343, 255);
    // the C++ reads skills 0..9; skill 10 (pretend corpse) is Rust only, in bytes the C++ HG and Login leave 0
    for b in 0..MAXSKILLTYPE {
        let (m, ssn) = skill_offsets(b);
        c.skill_mastery[b] = r.u8(m);
        c.skill_ssn[b] = r.u32(ssn);
    }
    c.equip_status = [-1; MAXITEMEQUIPPOS];
    c.is_item_equipped = [false; MAXITEMS];
    max_load_base = c.get_str() * 500 + c.level * 500; // _iCalcMaxLoad
    let char_name = c.char_name.clone();
    let id_num = c.char_id_num;

    let n_items = r.u8(CHAR_INDEX_END) as usize;
    let n_bank = r.u8(CHAR_INDEX_END + 1 + n_items * 64 + 1) as usize;
    if n_items > MAXITEMS || n_bank > MAXBANKITEMS {
        return false;
    }
    let mut equip = Vec::new();
    for b in 0..n_items {
        let ir = r.at(CHAR_INDEX_END + 1 + b * 64);
        let name = ir.s(0, 20);
        let Some(cfg) = g.data.item_by_name(&name) else {
            log_file(&format!("(!!!) Client({})-Item({}) is not existing Item! Conection closed.", l1(&char_name), l1(&name)));
            return false;
        };
        let mut it = Item::new(cfg);
        let count = ir.u32(20);
        if it.weight_of(count as i64) > max_load_base {
            log_file(&format!("(!) Client({})-Item({}) is too heavy.", l1(&char_name), l1(&it.cfg.name)));
            return false;
        }
        it.count = count;
        item_fields(&mut it, ir);
        if it.touch_effect_type == ITET_UNIQUE_OWNER && it.touch_effect_value != id_num {
            log(&format!("(!) Player({}) have Item({}) with another owner ID", l1(&char_name), l1(&it.cfg.name)));
        }
        fix_item(&mut it, &char_name);
        let c = g.client_mut(h).unwrap();
        c.item_pos[b] = (ir.u16(52) as i16, (ir.u16(54) as i16).max(-10));
        it.unique_id = ir.u64(56);
        if it.cfg.item_type == ITEMTYPE_NOTUSED {
            not_used_price += it.price as i32;
            continue;
        }
        if let Some(p) = g.data.dup_item_price(it.touch_effect_type as i32, it.touch_effect_value.map(|v| v as i32)) {
            not_used_price += p as i32; // _bCheckDupItemID replaces the price, then the item goes away
            continue;
        }
        if ir.u8(51) != 0 && it.cfg.item_type == ITEMTYPE_EQUIP {
            equip.push(b);
        }
        g.client_mut(h).unwrap().items[b] = Some(it);
    }
    let bank_ix = CHAR_INDEX_END + 1 + n_items * 64 + 2;
    for b in 0..n_bank {
        let ir = r.at(bank_ix + b * 59);
        let name = ir.s(0, 20);
        let Some(cfg) = g.data.item_by_name(&name) else {
            log_file(&format!("(!!!) Client({})-Bank Item({}) is a non-existing item! Connection closed.", l1(&char_name), l1(&name)));
            return false;
        };
        let mut it = Item::new(cfg);
        let count = ir.u32(20);
        if it.weight_of(count as i64) > max_load_base {
            return false;
        }
        it.count = count;
        item_fields(&mut it, ir);
        it.unique_id = ir.u64(51);
        fix_item(&mut it, &char_name);
        if it.cfg.item_type == ITEMTYPE_NOTUSED {
            not_used_price += it.price as i32;
            continue;
        }
        if let Some(p) = g.data.dup_item_price(it.touch_effect_type as i32, it.touch_effect_value.map(|v| v as i32)) {
            not_used_price += p as i32;
            continue;
        }
        g.client_mut(h).unwrap().bank[b] = Some(it);
    }
    // equipping needs the sprite type, so it runs after the appearance fields below (the C++ did it inside the
    // item loop with m_sType still 0, so gender-limited items were never checked at load)
    let stat_limit = g.settings.char_stat_limit;
    let (mi, cx, cy) = { let c = g.client(h).unwrap(); (c.map_index, c.x, c.y) };
    let valid = g.maps[mi].valid(cx, cy);
    let c = g.client_mut(h).unwrap();
    if !valid && (c.x != -1 || c.y != -1) {
        log_file(&format!("Invalid location error! {} ({}, {})", l1(&c.char_name), c.x, c.y));
        return false;
    }
    if !c.is_gm() {
        for v in [c.str_, c.dex, c.vit, c.int_, c.mag, c.range] {
            if v < 10 || v > stat_limit {
                return false;
            }
        }
    }
    if !valid_name(&c.char_name) || !valid_name(&c.account_name) {
        return false;
    }
    if c.penalty_block[0] != 0 && !crate::util::is_date_past(&block_date, crate::now_fields()) {
        return false;
    }
    if c.reserve_time != 0 {
        let t = crate::chrono_now();
        let now_sum = (t.month * 10000 + t.day * 100 + t.hour) as i64;
        if now_sum >= c.reserve_time as i64 {
            c.fightzone_number = 0;
            c.reserve_time = 0;
            c.fightzone_ticket_number = 0;
            // fix: the C++ sent this notify to client index `i` (the magic-mastery loop counter), not to the player
            g.notify(h, crate::consts::NOTIFY_FIGHTZONERESERVE, &[-2], None);
        }
    }
    let c = g.client_mut(h).unwrap();
    c.admin_user_level = c.admin_user_level.max(0);
    // fix: the C++ left the sprite type uninitialised for a sex other than 1/2
    let mut t: i16 = if c.sex == 2 { 4 } else { 1 };
    t += match c.skin {
        2 => 1,
        3 => 2,
        _ => 0,
    };
    if c.admin_user_level >= 10 {
        t = c.admin_user_level as i16;
    }
    c.type_ = t;
    c.appr[0] = ((c.hair_style as i16) << 8) | ((c.hair_color as i16) << 4) | c.underwear as i16;
    for b in equip {
        if !crate::entry::equip_item(g, h, b, false, false) {
            g.client_mut(h).unwrap().is_item_equipped[b] = false;
        }
    }
    let now16 = g.now() as i16;
    let c = g.client_mut(h).unwrap();
    if c.char_id_num[0] == 0 {
        let mut t1: i32 = 1;
        let name = cstr(&c.char_name).to_vec();
        for k in 0..10 {
            t1 += *name.get(k).unwrap_or(&0) as i8 as i32;
        }
        // the C++ adds abs(c ^ c) = 0 for the third id, leaving it 1
        c.char_id_num = [now16, t1 as i16, 1];
    }
    let c = g.client_mut(h).unwrap();
    c.reward_gold += not_used_price;
    if c.location == b"NONE" {
        c.is_neutral = true;
    }
    true
}

/// `_iComposePlayerDataFileContents`: the save block (no CharID; it starts at offset 0 = CharID slot).
pub fn compose(g: &mut Game, h: usize) -> Vec<u8> {
    let c = g.client_mut(h).unwrap();
    if c.hp <= 0 {
        c.hp = c.get_max_hp();
    }
    if c.mp <= 0 {
        c.mp = 30;
    }
    if c.sp < 0 {
        c.sp = 20;
    }
    let c = g.client(h).unwrap();
    let mut b = vec![0u8; CHAR_INDEX_END + 2 + MAXITEMS * 64 + 1 + MAXBANKITEMS * 59];
    let mut w = W(&mut b);
    for k in 0..3 {
        w.u32(4 + k * 4, c.char_id_num[k] as i32 as u32);
    }
    w.u16(16, c.level as u16);
    for (o, v) in [(18, c.str_), (19, c.vit), (20, c.dex), (21, c.int_), (22, c.mag), (23, c.range), (24, c.luck)] {
        w.u8(o, v as u8);
    }
    w.u32(25, c.exp);
    for (o, v) in [(29, c.sex), (30, c.skin), (31, c.hair_style), (32, c.hair_color), (33, c.underwear)] {
        w.u8(o, v);
    }
    w.u32(34, c.appr_color as u32);
    for k in 0..4 {
        w.u32(38 + k * 4, c.appr[k] as i32 as u32);
    }
    w.cstr(54, &c.location);
    w.cstr(64, &c.map_name);
    w.u16(74, c.x as u16);
    w.u16(76, c.y as u16);
    w.u32(78, c.contribution as u32);
    w.u32(82, c.special_ability_time as u32);
    w.cstr(86, &c.locked_map_name);
    w.u32(96, c.locked_map_time as u32);
    let pb = format!("{}-{}-{} 00:00:00", c.penalty_block[0], c.penalty_block[1], c.penalty_block[2]);
    w.cstr(100, pb.as_bytes());
    if c.guild_rank != -1 {
        w.cstr(120, &c.guild_name);
        w.u16(140, c.guild_guid as u16);
    } else {
        w.cstr(120, b"NONE");
        w.u16(140, 0xFFFF);
    }
    // the C++ writes the rank as a WORD whose high byte the fight zone number overwrites
    w.u8(142, c.guild_rank as u8);
    w.u8(143, c.fightzone_number as u8);
    w.u32(144, c.reserve_time as u32);
    w.u8(148, c.fightzone_ticket_number as u8);
    w.u16(149, c.quest as u16);
    // fix: the C++ saved the quest ID as a WORD (the Login reads a DWORD here)
    w.u32(151, c.quest_id);
    w.u16(155, c.cur_quest_count as u16);
    w.u16(157, c.quest_reward_type as u16);
    w.u32(159, c.quest_reward_amount as u32);
    w.u8(163, c.is_quest_completed as u8);
    for (o, v) in [(164, c.special_event_id), (168, c.war_contribution)] {
        w.u32(o, v as u32);
    }
    w.u8(172, c.crusade_duty as u8);
    w.u32(173, c.crusade_guid);
    for (o, v) in [
        (177, c.construction_point),
        (181, c.reputation),
        (185, c.hp),
        (189, c.mp),
        (193, c.sp),
        (197, c.enemy_kill_count),
        (201, c.pk_count),
        (205, c.reward_gold),
    ] {
        w.u32(o, v as u32);
    }
    w.u8(209, c.down_skill_index as u8);
    w.u8(210, c.hunger_status as u8);
    w.u16(211, c.super_attack_left as u16);
    for (o, v) in [
        (213, c.time_left_shutup),
        (217, c.time_left_rating),
        (221, c.time_left_force_recall),
        (225, c.time_left_firm_stamina),
        (229, c.dead_penalty_time),
        (233, c.party_id),
    ] {
        w.u32(o, v as u32);
    }
    w.u16(237, c.gizon_item_upgrade_left as u16);
    w.u16(239, c.elo as u16);
    w.u8(241, c.is_bank_modified as u8);
    for i in 0..MAXMAGICTYPE {
        w.u8(243 + i, c.magic_mastery[i]);
    }
    w.cstr(343, if cstr(&c.profile).is_empty() { b"__________" } else { &c.profile });
    for i in 0..MAXSKILLTYPE {
        let (m, ssn) = skill_offsets(i);
        w.u8(m, c.skill_mastery[i]);
        w.u32(ssn, c.skill_ssn[i]);
    }
    let mut n = 0;
    for i in 0..MAXITEMS {
        let Some(it) = &c.items[i] else { continue };
        let ix = CHAR_INDEX_END + 1 + n * 64;
        item_out(&mut w, ix, it);
        w.u8(ix + 51, c.is_item_equipped[i] as u8);
        w.u16(ix + 52, c.item_pos[i].0 as u16);
        w.u16(ix + 54, c.item_pos[i].1 as u16);
        w.u64(ix + 56, it.unique_id);
        n += 1;
    }
    w.u8(CHAR_INDEX_END, n as u8);
    let index = CHAR_INDEX_END + 1 + n * 64;
    let mut nb = 0;
    if c.is_bank_modified {
        for it in c.bank.iter().flatten() {
            let ix = index + 1 + nb * 59;
            item_out(&mut w, ix, it);
            w.u64(ix + 51, it.unique_id);
            nb += 1;
        }
    }
    w.u8(index, nb as u8);
    // fix: the C++ returned `Index`, so the bank count and the bank items were never sent (a modified bank
    // reached the Login empty); send them when the bank was modified
    b.truncate(if c.is_bank_modified { index + 1 + nb * 59 } else { index });
    b
}

fn item_out(w: &mut W<'_>, ix: usize, it: &Item) {
    w.cstr(ix, &it.cfg.name);
    w.u32(ix + 20, it.count);
    w.u16(ix + 24, it.touch_effect_type as u16);
    for k in 0..3 {
        w.u32(ix + 26 + k * 4, it.touch_effect_value[k] as i32 as u32);
    }
    w.u8(ix + 38, it.item_color);
    for k in 0..3 {
        w.u16(ix + 39 + k * 2, it.item_spec_effect_value[k] as u16);
    }
    w.u16(ix + 45, it.cur_life_span);
    w.u32(ix + 47, it.attribute);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_layout() {
        assert_eq!(skill_offsets(0), (598, 653));
        assert_eq!(skill_offsets(9), (607, 689));
        // skill 10: right after the C++ fields (mastery 598..607, SSN 653..692), before the item count at 751
        assert_eq!(skill_offsets(MAXSKILLTYPE - 1), (608, 693));
        let (m, ssn) = skill_offsets(MAXSKILLTYPE - 1);
        assert!(m < 653 && ssn + 4 <= CHAR_INDEX_END);
    }
}
