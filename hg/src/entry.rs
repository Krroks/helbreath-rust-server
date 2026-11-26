//! Entering the game: `RequestInitPlayerHandler`, `ResponsePlayerDataHandler`, `InitPlayerData`, `PlayerMapEntry`
//! (RESPONSE_INITDATA + the entry notifications), plus the bits of the item layer entry needs
//! (`bEquipItemHandler` appearance, `CalcTotalItemEffect` ratios, `iSetSide`, `GetMapInitialPoint`...).
use crate::client::*;
use crate::consts::*;
use crate::game::Game;
use crate::log::{log, log_file};
use crate::map::OWNERTYPE_PLAYER;
use crate::util::{cstr, l1, Rd, W};

const EMPTY_POS_X: [i16; 25] = [0, 1, 1, 0, -1, -1, -1, 0, 1, 2, 2, 2, 2, 1, 0, -1, -2, -2, -2, -2, -2, -1, 0, 1, 2];
const EMPTY_POS_Y: [i16; 25] = [0, 0, 1, 1, 1, 0, -1, -1, -1, -1, 0, 1, 2, 2, 2, 2, 2, 1, 0, -1, -2, -2, -2, -2, -2];

/// `bEquipItemHandler`. `live` = the COMMONTYPE_EQUIPITEM command: replaced items go through `ReleaseItemHandler`,
/// the change is shown around and the item effects are recomputed. While loading a character (`live` false) a
/// replaced item only loses its flag and nothing is sent (the not yet entered player is not on the map).
pub fn equip_item(g: &mut Game, h: usize, b: usize, live: bool, notify: bool) -> bool {
    // the GM event switches of the map (/eventshield, /eventarmor); GMs are not concerned
    let c = g.client(h).unwrap();
    let (mp, gm) = (&g.maps[c.map_index], c.is_gm());
    let (shield_off, armor_off) = (mp.is_shield_disabled && !gm, mp.is_armor_disabled && !gm);
    let c = g.client_mut(h).unwrap();
    let Some(it) = c.items.get(b).and_then(|i| i.clone()) else { return false };
    if it.cfg.item_type != 1 || it.cur_life_span == 0 {
        return false;
    }
    if it.attribute & 1 == 0 && it.cfg.level_limit > c.level {
        return false;
    }
    if it.cfg.gender_limit != 0 {
        let need = if (1..=3).contains(&c.type_) { 1 } else if (4..=6).contains(&c.type_) { 2 } else { it.cfg.gender_limit };
        if it.cfg.gender_limit != need {
            return false;
        }
    }
    if it.weight_of(1) > c.get_str() * 100 {
        return false;
    }
    let pos = it.cfg.equip_pos;
    let mut refused = (pos == EQUIPPOS_LHAND && shield_off) || (armor_off && matches!(pos, EQUIPPOS_BODY | EQUIPPOS_LEGGINGS | EQUIPPOS_ARMS | EQUIPPOS_HEAD));
    if matches!(pos, EQUIPPOS_BODY | EQUIPPOS_LEGGINGS | EQUIPPOS_ARMS | EQUIPPOS_HEAD) {
        let need = it.cfg.item_effect_value[4];
        let have = match it.cfg.item_effect_value[3] {
            10 => c.get_str(),
            11 => c.get_dex(),
            12 => c.vit,
            13 => c.get_int(),
            14 => c.get_mag(),
            15 => c.range,
            _ => i32::MAX,
        };
        refused |= have < need;
    }
    if refused {
        if live {
            // fix: the C++ also released the item already worn in that slot without telling the client (it is
            // told the refused item was released), so the client went on showing that one worn
            g.notify(h, NOTIFY_ITEMRELEASED, &[pos, b as i32], None);
        }
        return false;
    }
    let c = g.client_mut(h).unwrap();
    if live && matches!(it.cfg.id_num, 865 | 866) && c.get_int() >= 100 && c.get_mag() >= 100 && c.special_ability_time < 1 {
        // ITEM_RESURWAND_MS20 / MS10: the resurrection spell while held
        c.magic_mastery[94] = 1;
        g.notify(h, NOTIFY_SPELL_SKILL, &[], None);
    }
    let c = g.client(h).unwrap();
    if live && matches!(it.cfg.item_effect_type, 24 | 25) && c.fx.sa_type != 0 && pos != c.fx.sa_equip_pos {
        // another special-ability item is worn elsewhere: it comes off
        let sp = c.fx.sa_equip_pos;
        let k = if (0..MAXITEMEQUIPPOS as i32).contains(&sp) { c.equip_status[sp as usize] } else { -1 };
        g.notify(h, NOTIFY_ITEMRELEASED, &[sp, k as i32], None);
        if k >= 0 {
            g.release_item(h, k as usize, true);
        }
    }
    if pos == EQUIPPOS_NONE || pos < 0 || pos as usize >= MAXITEMEQUIPPOS {
        return false;
    }
    // the slots to free, in the C++ order, and whether that release is shown around
    let slots: &[(i32, bool)] = match pos {
        EQUIPPOS_TWOHAND => &[(EQUIPPOS_TWOHAND, false), (EQUIPPOS_RHAND, false), (EQUIPPOS_LHAND, false)],
        EQUIPPOS_LHAND => &[(EQUIPPOS_TWOHAND, false), (EQUIPPOS_LHAND, false)],
        EQUIPPOS_RHAND => &[(EQUIPPOS_TWOHAND, false), (EQUIPPOS_RHAND, false)],
        EQUIPPOS_FULLBODY => &[
            (EQUIPPOS_FULLBODY, false),
            (EQUIPPOS_HEAD, false),
            (EQUIPPOS_BODY, true),
            (EQUIPPOS_ARMS, false),
            (EQUIPPOS_LEGGINGS, false),
            (EQUIPPOS_PANTS, false),
            (EQUIPPOS_BACK, false),
        ],
        EQUIPPOS_HEAD => &[(EQUIPPOS_FULLBODY, false), (EQUIPPOS_HEAD, false)],
        EQUIPPOS_BODY => &[(EQUIPPOS_FULLBODY, false), (EQUIPPOS_BODY, false)],
        EQUIPPOS_ARMS => &[(EQUIPPOS_FULLBODY, false), (EQUIPPOS_ARMS, false)],
        EQUIPPOS_LEGGINGS => &[(EQUIPPOS_FULLBODY, false), (EQUIPPOS_LEGGINGS, false)],
        EQUIPPOS_PANTS => &[(EQUIPPOS_FULLBODY, false), (EQUIPPOS_PANTS, false)],
        EQUIPPOS_BACK => &[(EQUIPPOS_FULLBODY, false), (EQUIPPOS_BACK, false)],
        _ => &[(-1, false)],
    };
    for &(p, notice) in slots {
        let p = if p < 0 { pos } else { p };
        let i = g.client(h).unwrap().equip_status[p as usize];
        if i < 0 {
            continue;
        }
        if live {
            g.release_item(h, i as usize, notice);
        } else {
            let c = g.client_mut(h).unwrap();
            c.is_item_equipped[i as usize] = false;
            c.equip_status[p as usize] = -1;
        }
    }
    let c = g.client_mut(h).unwrap();
    c.equip_status[pos as usize] = b as i16;
    c.is_item_equipped[b] = true;
    let (av, col) = (it.cfg.appr_value as i16, it.item_color as i32);
    let speed = |c: &Client| (it.speed - c.get_str() / 13).max(0);
    match pos {
        EQUIPPOS_RHAND | EQUIPPOS_TWOHAND => {
            c.appr[1] = (c.appr[1] & 0xF00Fu16 as i16) | (av << 4);
            c.appr_color = (c.appr_color & 0x0FFF_FFFF) | (col << 28);
            // fix: for one-hand weapons the C++ or-ed the speed into the appearance temp, so the status lost it
            c.status = (c.status & !0xF) | speed(c);
            c.cs.combo = 0;
        }
        EQUIPPOS_LHAND => {
            c.appr[1] = (c.appr[1] & 0xFFF0u16 as i16) | av;
            c.appr_color = (c.appr_color & 0xF0FF_FFFFu32 as i32) | (col << 24);
        }
        EQUIPPOS_BODY => {
            if av < 100 {
                c.appr[2] = (c.appr[2] & 0x0FFF) | (av << 12);
            } else {
                c.appr[2] = (c.appr[2] & 0x0FFF) | ((av - 100) << 12);
                c.appr[3] |= 0x080;
            }
            c.appr_color = (c.appr_color & 0xFF0F_FFFFu32 as i32) | (col << 20);
        }
        EQUIPPOS_BACK => {
            c.appr[3] = (c.appr[3] & 0xF0FFu16 as i16) | (av << 8);
            c.appr_color = (c.appr_color & 0xFFF0_FFFFu32 as i32) | (col << 16);
        }
        EQUIPPOS_ARMS => {
            c.appr[2] = (c.appr[2] & 0xFFF0u16 as i16) | av;
            c.appr_color = (c.appr_color & 0xFFFF_0FFFu32 as i32) | (col << 12);
        }
        EQUIPPOS_PANTS => {
            c.appr[2] = (c.appr[2] & 0xF0FFu16 as i16) | (av << 8);
            c.appr_color = (c.appr_color & 0xFFFF_F0FFu32 as i32) | (col << 8);
        }
        EQUIPPOS_LEGGINGS => {
            c.appr[3] = (c.appr[3] & 0x0FFF) | (av << 12);
            c.appr_color = (c.appr_color & 0xFFFF_FF0Fu32 as i32) | (col << 4);
        }
        EQUIPPOS_HEAD => {
            c.appr[2] = (c.appr[2] & 0xFF0Fu16 as i16) | (av << 4);
            c.appr_color = (c.appr_color & 0xFFFF_FFF0u32 as i32) | col;
        }
        EQUIPPOS_FULLBODY => {
            c.appr[2] = (c.appr[2] & 0x0FFF) | (av << 12);
            c.appr_color &= 0xFFF0_FFFFu32 as i32;
        }
        _ => {}
    }
    let fx = it.cfg.special_effect;
    if it.cfg.item_effect_type == 24 {
        // ITEMEFFECTTYPE_ATTACK_SPECABLTY
        c.appr[3] = (c.appr[3] & 0xFFF3u16 as i16) | [0, 0x4, 0xC, 0x8].get(fx as usize).copied().unwrap_or(0);
    }
    if it.cfg.item_effect_type == 25 {
        // ITEMEFFECTTYPE_DEFENSE_SPECABLTY
        c.appr[3] &= 0xFFFCu16 as i16;
        match fx {
            0 => {}
            50..=52 => c.appr[3] |= 0x2,
            _ if c.admin_user_level >= 1 => c.appr[3] |= 0x1,
            _ => {}
        }
    }
    if !live {
        // fix: at load the C++ also broadcast an OBJECTNULLACTION of the not yet entered player near its saved spot
        return true;
    }
    g.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
    g.calc_total_item_effect(h, b as i32, notify);
    if (908..=911).contains(&it.cfg.id_num) {
        // the angelic pendants (their stat bonus comes with angels)
        g.notify(h, NOTIFY_SETTING_SUCCESS, &[], None);
    }
    true
}

impl Game {
    /// `RequestInitPlayerHandler`
    pub fn request_init_player(&mut self, h: usize, d: &[u8], _key: u8) {
        let Some(c) = self.client(h) else { return };
        if c.is_init_complete {
            return;
        }
        let r = Rd::new(d);
        // the C++ "decodes" the names with CMisc::bDecode, which returns at once
        let (ch, acc, pwd) = (r.raw(6, 10), r.raw(16, 10), r.raw(26, 10));
        let observer = r.u8(36) != 0;
        let same = |a: &[u8], b: &[u8]| Game::pad10(a) == Game::pad10(b);
        for i in self.handles() {
            if i == h {
                continue;
            }
            let o = self.client(i).unwrap();
            if same(&o.account_name, &acc) || same(&o.char_name, &ch) {
                if same(&o.account_password, &pwd) {
                    log(&format!("<{i}> Duplicate player! Deleted with data save : CharName({}) IP({})", l1(&o.char_name), l1(&o.ip_address)));
                    self.delete_client(i, true, true, false, false);
                } else {
                    let c = self.client_mut(h).unwrap();
                    (c.char_name, c.account_name, c.account_password) = (cstr(&ch).to_vec(), cstr(&acc).to_vec(), cstr(&pwd).to_vec());
                    self.delete_client(h, false, false, false, false);
                    return;
                }
            }
        }
        let c = self.client_mut(h).unwrap();
        (c.char_name, c.account_name, c.account_password) = (cstr(&ch).to_vec(), cstr(&acc).to_vec(), cstr(&pwd).to_vec());
        c.is_observer_mode = observer;
        self.send_request_player_data(h);
    }

    pub fn pad10(s: &[u8]) -> [u8; 10] {
        let mut a = [0u8; 10];
        let s = cstr(s);
        let n = s.len().min(10);
        a[..n].copy_from_slice(&s[..n]);
        a
    }

    /// `ResponsePlayerDataHandler`
    pub fn response_player_data(&mut self, d: &[u8]) {
        let r = Rd::new(d);
        let name = r.raw(6, 10);
        let Some(h) = self.handles().into_iter().find(|&i| Game::pad10(&self.client(i).unwrap().char_name)[..] == name[..]) else {
            log(&format!("(!)Non-existing player data received from Log server: CharName({})", l1(cstr(&name))));
            return;
        };
        match r.u16(4) as i32 {
            LOGRESMSGTYPE_CONFIRM => self.init_player_data(h, d),
            LOGRESMSGTYPE_REJECT => {
                if r.u8(16) == 1 {
                    let mut b = vec![0u8; 6];
                    W(&mut b).u32(0, MSGID_RESPONSE_INITPLAYER);
                    W(&mut b).u16(4, MSGTYPE_REJECT as u16);
                    self.send(h, &b);
                } else {
                    log(&format!("(HACK?) Not existing character({}) data request! Rejected!", l1(&self.client(h).unwrap().char_name)));
                    self.delete_client(h, false, false, true, false);
                }
            }
            _ => {}
        }
    }

    /// `InitPlayerData`
    fn init_player_data(&mut self, h: usize, d: &[u8]) {
        if self.client(h).is_none_or(|c| c.is_init_complete) {
            return;
        }
        {
            let c = self.client_mut(h).unwrap();
            (c.hit_ratio, c.defense_ratio, c.side) = (0, 0, NEUTRAL);
        }
        if !crate::player_data::decode(self, h, &d[16.min(d.len())..]) {
            log(&format!("(HACK?) Character({}) data error!", l1(&self.client(h).unwrap().char_name)));
            self.delete_client(h, false, true, true, false);
            return;
        }
        let (max_level, skill_limit) = (self.settings.max_player_level, self.settings.char_skill_limit);
        let c = self.client(h).unwrap();
        if !c.is_gm() && c.level > max_level {
            log_file(&format!("(HACK) ({}) Player: ({}) High level hack ({})", l1(&c.ip_address), l1(&c.char_name), c.level));
            self.delete_client(h, false, true, true, true);
            return;
        }
        let total: i32 = c.skill_mastery.iter().map(|&s| s as i32).sum();
        if total - 21 > skill_limit && c.admin_user_level == 0 {
            log_file(&format!("(HACK) Packet Editing: ({}) Player: ({}) - has more than allowed skill points ({total}).", l1(&c.ip_address), l1(&c.char_name)));
            self.delete_client(h, false, true, true, true);
            return;
        }
        if c.x == -1 && c.y == -1 {
            let (m, loc) = (c.map_index, c.location.clone());
            let (x, y) = self.get_map_initial_point(m, Some(&loc), (-1, -1));
            let c = self.client_mut(h).unwrap();
            (c.x, c.y) = (x, y);
        }
        let c = self.client_mut(h).unwrap();
        if c.pk_count > 0 {
            c.set_status_flag(STATUS_PK, true);
        }
        self.set_side(h);
        let next = self.level_exp_table[(self.client(h).unwrap().level as usize + 1).min(899)];
        self.client_mut(h).unwrap().next_level_exp = next;
        self.calc_total_item_effect(h, -1, false);
        self.calc_total_weight(h);
        self.send_quest_contents(h);
        self.validate_skills(h);
        let c = self.client_mut(h).unwrap();
        if (1..=4).contains(&c.admin_user_level) {
            c.set_status_flag(STATUS_INVISIBILITY, true);
            c.gm_flags |= GMFLAG_INVENCIBLE | GMFLAG_NOAGGRO;
        }
        // fix: the C++ tested an uninitialised cGuildStatus here (the guild check is the guild layer's job)
        // Rust only: byte 6 = is GM, so the client lists the GM spells only to GMs (older clients ignore it)
        let mut b = vec![0u8; 7];
        W(&mut b).u32(0, MSGID_RESPONSE_INITPLAYER);
        W(&mut b).u16(4, MSGTYPE_CONFIRM as u16);
        b[6] = c.is_gm() as u8;
        self.send(h, &b);
        self.client_mut(h).unwrap().is_init_complete = true;
        self.send_enter_game_confirm(h);
        self.event_enter(h);
        self.party_enter(h);
        self.send_client_settings(h);

        self.send_character_contents(h);
        let hs = self.client(h).unwrap().hunger_status;
        self.notify(h, NOTIFY_HUNGER, &[hs], None); // the HUD hunger tube needs the value from the start
        self.send_skill_contents(h);
        self.send_item_list(h);
        self.player_map_entry(h, true);
        self.apocalypse_gate_state(h);
        if let Some(ty) = self.ast.ev.as_ref().map(|e| e.ty) {
            self.notify(h, NOTIFY_EVENTSTART, &[ty], None);
        }
        self.fz_entry_recall(h);
    }

    /// MSGID_PLAYERCHARACTERCONTENTS (115 bytes)
    fn send_character_contents(&mut self, h: usize) {
        let c = self.client_mut(h).unwrap();
        let stats = c.str_ + c.dex + c.vit + c.int_ + c.mag + c.range;
        c.lu_pool = c.level * 3 - (stats - 70);
        let c = self.client(h).unwrap();
        let mut b = vec![0u8; 115];
        let mut w = W(&mut b);
        w.u32(0, MSGID_PLAYERCHARACTERCONTENTS);
        w.u16(4, MSGTYPE_CONFIRM as u16);
        let ints = [c.hp, c.mp, c.sp, c.defense_ratio, c.hit_ratio, c.level, c.get_str(), c.get_int(), c.vit, c.get_dex(), c.get_mag(), c.range];
        for (k, v) in ints.iter().enumerate() {
            w.u32(6 + k * 4, *v as u32);
        }
        w.u16(54, c.lu_pool as u16);
        w.u32(56, c.exp);
        w.u32(60, c.enemy_kill_count as u32);
        w.u32(64, c.pk_count as u32);
        w.u32(68, c.reward_gold as u32);
        w.bytes(72, &Game::pad10(&c.location));
        let mut g20 = cstr(&c.guild_name).to_vec();
        g20.resize(20, 0);
        w.bytes(82, &g20);
        w.u32(102, c.guild_rank as u32);
        w.u8(106, c.super_attack_left as u8);
        w.u32(107, c.fightzone_number as u32);
        self.send(h, &b);
    }

    /// MSGID_PLAYERSKILLCONTENTS: 10 skill masteries as WORDs (the C++ writes ints 2 bytes apart), 100 magic flags.
    fn send_skill_contents(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        let mut b = vec![0u8; 126];
        let mut w = W(&mut b);
        w.u32(0, MSGID_PLAYERSKILLCONTENTS);
        // the C++ leaves the message type as whatever the previous message left in the buffer: MSGTYPE_CONFIRM
        w.u16(4, MSGTYPE_CONFIRM as u16);
        for i in 0..10 {
            w.u16(6 + i * 2, c.skill_mastery[i] as u16);
        }
        w.bytes(26, &c.magic_mastery);
        self.send(h, &b);
    }

    /// MSGID_PLAYERITEMLISTCONTENTS
    fn send_item_list(&mut self, h: usize) {
        let c = self.client_mut(h).unwrap();
        // compact the bag (the C++ bubble pass moves items and their positions down over empty slots)
        let mut items: Vec<(Item, (i16, i16), bool)> = Vec::new();
        for i in 0..MAXITEMS {
            if let Some(it) = c.items[i].take() {
                items.push((it, c.item_pos[i], c.is_item_equipped[i]));
            }
        }
        let mut equip = [-1i16; MAXITEMEQUIPPOS];
        for (k, (it, p, e)) in items.into_iter().enumerate() {
            if e {
                equip[it.cfg.equip_pos.clamp(0, 14) as usize] = k as i16;
            }
            c.items[k] = Some(it);
            c.item_pos[k] = p;
            c.is_item_equipped[k] = e;
        }
        for k in c.items.iter().position(|i| i.is_none()).unwrap_or(MAXITEMS)..MAXITEMS {
            c.is_item_equipped[k] = false;
        }
        // fix: the C++ moved items but not m_sItemEquipmentStatus, which then pointed at the wrong slots
        c.equip_status = equip;
        self.calc_total_item_effect(h, -1, false);
        let c = self.client(h).unwrap();
        let bag: Vec<&Item> = c.items.iter().flatten().collect();
        let bank: Vec<&Item> = c.bank.iter().flatten().collect();
        let mut b = vec![0u8; 8 + bag.len() * 44 + bank.len() * 43];
        let mut w = W(&mut b);
        w.u32(0, MSGID_PLAYERITEMLISTCONTENTS);
        w.u16(4, MSGTYPE_CONFIRM as u16);
        w.u8(6, bag.len() as u8);
        let mut o = 7;
        for (k, it) in bag.iter().enumerate() {
            item_entry(&mut w, o, it, Some(c.is_item_equipped[k]));
            o += 44;
        }
        w.u8(o, bank.len() as u8);
        o += 1;
        for it in bank {
            item_entry(&mut w, o, it, None);
            o += 43;
        }
        // fix: the C++ sent 2 bytes less than it wrote (the two count bytes were not in the size)
        self.send(h, &b);
    }

    /// `PlayerMapEntry`: MSGID_RESPONSE_INITDATA and the notifications that follow.
    pub fn player_map_entry(&mut self, h: usize, set_recall_time: bool) {
        self.set_side(h);
        let c = self.client(h).unwrap();
        let (m, observer) = (c.map_index, c.is_observer_mode);
        let (x, y) = if !observer {
            self.get_empty_position(c.x, c.y, m)
        } else {
            self.get_map_initial_point(m, None, (c.x, c.y))
        };
        let c = self.client_mut(h).unwrap();
        (c.x, c.y) = (x, y);
        if c.pk_count > 0 {
            c.set_status_flag(STATUS_PK, true);
        }
        let c = self.client(h).unwrap();
        let map = &self.maps[m];
        let mut b = vec![0u8; 66];
        let mut w = W(&mut b);
        w.u32(0, MSGID_RESPONSE_INITDATA);
        w.u16(4, MSGTYPE_CONFIRM as u16);
        w.u16(6, h as u16);
        w.u16(8, (x - 17 - 5) as u16);
        w.u16(10, (y - 12 - 5) as u16);
        w.u16(12, c.type_ as u16);
        for k in 0..4 {
            w.u16(14 + k * 2, c.appr[k] as u16);
        }
        w.u32(22, c.appr_color as u32);
        w.u32(26, c.status as u32);
        w.bytes(30, &Game::pad10(&c.map_name));
        w.bytes(40, &Game::pad10(&map.location_name));
        w.u8(50, if map.is_fixed_day_mode { 1 } else { self.day_or_night });
        w.u8(51, if map.is_fixed_day_mode { 0 } else { map.weather });
        w.u32(52, c.contribution as u32);
        w.u8(56, observer as u8);
        w.u32(57, c.reputation as u32);
        w.u32(61, c.hp as u32);
        // shop price adjustment: -10 % for the last crusade's winner side in a shop
        w.u8(65, if c.is_on_shop && self.cr.winner_side == c.side { -10i8 as u8 } else { 0 });
        if !observer {
            self.maps[m].set_owner(h as i16, OWNERTYPE_PLAYER, x, y);
        }
        let md = self.compose_init_map_data(x - VIEW_LEFT, y - 7, h);
        b.extend_from_slice(&md);
        self.send(h, &b);
        self.astoria_entry(h);
        if self.hd.mode {
            self.update_heldenian_status();
        }

        self.send_event_near_type_a(h, MSGID_EVENT_LOG, MSGTYPE_CONFIRM as u16, 0, 0, 0);
        let now = self.now();
        if set_recall_time {
            self.set_side(h);
            let c = self.client_mut(h).unwrap();
            c.is_war_location = false;
            c.time_left_force_recall = 0;
            let (neutral, gm) = (c.is_neutral(), c.is_gm());
            let foe = !neutral && !gm && self.is_in_foe_map(h);
            let mn = cstr(&self.maps[self.client(h).unwrap().map_index].name).to_vec();
            if neutral && self.cr.mode && !gm && [SIDE_MAP[ARESDEN as usize], SIDE_MAP[ELVINE as usize], SIDE_MAP[ISTRIA as usize]].contains(&&mn[..]) {
                // a traveller in a town when a crusade is on is recalled at once
                let c = self.client_mut(h).unwrap();
                (c.war_begin_time, c.is_war_location, c.time_left_force_recall) = (now, true, 1);
            } else if foe {
                let c = self.client_mut(h).unwrap();
                c.war_begin_time = now;
                c.is_war_location = true;
                // SetForceRecallTime: m_sForceRecallTime (GM command, GSM_REQUEST_SETFORCERECALLTIME), else the raid time of the day
                let dow = crate::chrono_now().weekday as usize; // 0 = sunday
                let rt = if self.force_recall_time > 0 { self.force_recall_time } else { self.settings.raid_time[(dow + 6) % 7] };
                self.client_mut(h).unwrap().time_left_force_recall = 20 * rt;
            }
            let t = self.client(h).unwrap().time_left_force_recall;
            if t > 0 {
                self.notify(h, NOTIFY_FORCERECALLTIME, &[t], None);
            }
        }
        let c = self.client(h).unwrap();
        let map_side = map_location_side(&self.maps[c.map_index].name);
        let tmp_side = if map_side >= 11 { map_side - 10 } else { map_side };
        let fight = self.maps[c.map_index].is_fight_zone && self.fz.no_force_recall == 0;
        let (is_gm, neutral, side, jail) = (c.is_gm(), c.is_neutral(), c.side, self.is_in_jail(h));
        let c = self.client_mut(h).unwrap();
        c.is_in_building = false;
        if side != tmp_side && map_side != 0 {
            if map_side <= 10 && !is_gm && !neutral {
                c.war_begin_time = now;
                c.is_war_location = true;
                c.time_left_force_recall = 1;
                c.is_in_building = true;
            }
        } else if fight && !is_gm {
            c.war_begin_time = now;
            c.is_war_location = true;
            let t = crate::chrono_now();
            c.time_left_force_recall = 2 * 20 * 60 - ((t.hour as i32 % 2) * 20 * 60 + t.minute as i32 * 20) - 2 * 20;
        } else if jail && !is_gm {
            c.is_war_location = true;
            c.war_begin_time = now;
            if c.time_left_force_recall == 0 || c.time_left_force_recall > 20 * 5 {
                c.time_left_force_recall = 20 * 5;
            }
        }
        self.check_quest_environment(h);
        if self.ast.ev.is_some() && self.is_astoria_map(self.client(h).unwrap().map_index) {
            self.update_relic_pos(Some(h));
        }
        if self.client(h).unwrap().special_ability_time == 0 {
            self.notify(h, NOTIFY_SPECIALABILITYENABLED, &[], None);
        }
        if self.hd.mode {
            self.notify(h, NOTIFY_HELDENIANSTART, &[], None);
            self.notify(h, NOTIFY_HELDENIANTELEPORT, &[], None);
        }
        let (guid, crusade) = (self.crusade_guid, self.cr.mode);
        let c = self.client_mut(h).unwrap();
        if crusade {
            if c.crusade_guid == 0 {
                (c.crusade_duty, c.construction_point, c.crusade_guid) = (0, 0, guid);
            } else if c.crusade_guid != guid {
                (c.crusade_duty, c.construction_point, c.war_contribution, c.crusade_guid) = (0, 0, 0, guid);
                self.notify(h, NOTIFY_CRUSADE, &[1, 0, 0, -1], None);
            }
            let duty = self.client(h).unwrap().crusade_duty;
            self.notify(h, NOTIFY_CRUSADE, &[1, duty, 0, 0], None);
        } else if c.crusade_guid == guid {
            // the last crusade's result comes with the next exp-stock timer
            c.crusade_duty = 0;
            c.construction_point = 0;
        } else if c.crusade_guid != 0 {
            c.war_contribution = 0;
            c.crusade_guid = 0;
            self.notify(h, NOTIFY_CRUSADE, &[0, 0, 0, -1], None);
        }
        if fight {
            self.fightzone_entry(h);
            log_file(&format!("Char({})-Enter({}) Observer({})", l1(&self.client(h).unwrap().char_name), l1(&self.client(h).unwrap().map_name), observer as u8));
        }
        let c = self.client(h).unwrap();
        let (dsi, cp, wc, gz) = (c.down_skill_index, c.construction_point, c.war_contribution, c.gizon_item_upgrade_left);
        self.party_map_entry(h);
        self.notify(h, NOTIFY_SAFEATTACKMODE, &[], None);
        self.notify(h, NOTIFY_DOWNSKILLINDEXSET, &[dsi], None);
        self.notify(h, NOTIFY_ITEMPOSLIST, &[], None);
        self.notify(h, NOTIFY_CONSTRUCTIONPOINT, &[cp, wc, 1], None);
        self.notify(h, NOTIFY_GIZONITEMUPGRADELEFT, &[gz], None);
        self.client_mut(h).unwrap().next_recall_point = 0;
        self.quest_init(h);
    }

    /// `iComposeInitMapData`: the VIEW_W x 16 tiles from (sx, sy).
    pub fn compose_init_map_data(&mut self, sx: i16, sy: i16, h: usize) -> Vec<u8> {
        let mut out = vec![0u8, 0];
        let mut total = 0i16;
        for iy in 0..16 {
            for ix in 0..VIEW_W {
                if self.write_tile_data(&mut out, h, sx, sy, ix, iy) {
                    total += 1;
                }
            }
        }
        out[..2].copy_from_slice(&total.to_le_bytes());
        out
    }

    /// `iSetSide`
    pub fn set_side(&mut self, h: usize) {
        let m = self.client(h).unwrap().map_index;
        let map_loc = cstr(&self.maps[m].location_name).to_vec();
        let c = self.client_mut(h).unwrap();
        let loc = cstr(&c.location).to_vec();
        c.side = NEUTRAL;
        c.is_on_town_nonpk = loc.len() >= 3 && map_loc.len() >= 3 && loc[..3] == map_loc[..3];
        if loc.starts_with(b"are") {
            c.side = ARESDEN;
        } else if loc.starts_with(b"elv") {
            c.side = ELVINE;
        } else if loc.starts_with(b"ist") {
            c.side = ISTRIA;
        } else {
            if map_loc == SIDE_MAP[ELVINE as usize] || map_loc == SIDE_MAP[ARESDEN as usize] || map_loc == SIDE_MAP[ISTRIA as usize] {
                c.is_on_town_nonpk = true;
            }
            c.is_neutral = false;
        }
        let s = c.side;
        c.set_side_flag(s);
        let mn = cstr(&c.map_name).to_vec();
        if mn.starts_with(b"default") {
            c.is_neutral = true;
        }
        c.is_on_shop = mn.starts_with(b"bsmith") || mn.starts_with(b"gldhall") || mn.starts_with(b"gshop");
        c.is_on_warehouse = mn.starts_with(b"wrhus") || mn.get(3..).is_some_and(|r| r.starts_with(b"wrhus"));
        c.is_on_tower = mn.starts_with(b"wzdtwr");
    }

    /// `RequestCivilRightHandler` (MSGID_REQUEST_CIVILRIGHT): a neutral player of level 5 or more takes the nationality
    /// of the town he stands in (the answer holds 1 and the new location, else 0 and the old one); he leaves his party.
    pub fn request_civil_right(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        let result = (c.is_neutral() && c.level >= 5) as u16;
        if result == 1 {
            let loc = cstr(&self.maps[c.map_index].location_name).to_vec();
            self.client_mut(h).unwrap().location = loc;
        }
        self.set_side(h);
        let mut b = vec![0u8; 16];
        let mut w = W(&mut b);
        w.u32(0, MSGID_RESPONSE_CIVILRIGHT);
        w.u16(4, result);
        w.bytes(6, &Game::pad10(&self.client(h).unwrap().location));
        self.send(h, &b);
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        if self.client(h).is_some_and(|c| c.party_id != 0) {
            self.request_delete_party(h);
        }
    }

    /// `CClient::IsInFoeMap`
    pub fn is_in_foe_map(&self, h: usize) -> bool {
        let c = self.client(h).unwrap();
        let loc = cstr(&self.maps[c.map_index].location_name);
        match c.side {
            ARESDEN => loc == SIDE_MAP[ELVINE as usize],
            ELVINE => loc == SIDE_MAP[ARESDEN as usize],
            _ => false,
        }
    }

    pub fn is_in_jail(&self, h: usize) -> bool {
        let c = self.client(h).unwrap();
        self.maps[c.map_index].location_name.get(3..7) == Some(b"jail")
    }

    /// `GetMapInitialPoint`; `cur` is returned unchanged when the map has no initial point.
    pub fn get_map_initial_point(&mut self, m: usize, player_location: Option<&[u8]>, cur: (i16, i16)) -> (i16, i16) {
        let pts: Vec<(i16, i16)> = self.maps[m].initial_points.iter().flatten().copied().collect();
        if pts.is_empty() {
            return cur;
        }
        let i = if player_location.is_some_and(|l| l.starts_with(b"NONE")) { 0 } else { (self.dice(1, pts.len() as i32) - 1) as usize };
        pts[i]
    }

    /// `bGetEmptyPosition` when it succeeds.
    pub fn find_empty_position(&self, x: i16, y: i16, m: usize) -> Option<(i16, i16)> {
        (0..25).map(|i| (x + EMPTY_POS_X[i], y + EMPTY_POS_Y[i])).find(|&(nx, ny)| self.maps[m].get_moveable(nx, ny).0 && !self.maps[m].is_teleport(nx, ny))
    }

    /// `bGetEmptyPosition`
    pub fn get_empty_position(&mut self, x: i16, y: i16, m: usize) -> (i16, i16) {
        for i in 0..25 {
            let (nx, ny) = (x + EMPTY_POS_X[i], y + EMPTY_POS_Y[i]);
            if self.maps[m].get_moveable(nx, ny).0 && !self.maps[m].is_teleport(nx, ny) {
                return (nx, ny);
            }
        }
        // fix: the C++ passed uninitialised coordinates to GetMapInitialPoint when the map has no initial point
        self.get_map_initial_point(m, None, (x, y))
    }

    /// `iCalcTotalWeight`
    pub fn calc_total_weight(&mut self, h: usize) {
        let c = self.client_mut(h).unwrap();
        c.cur_weight_load = c.items.iter().flatten().map(|i| i.weight_of(i.count as i64)).sum();
    }

    /// `CClient::ValidateSkills`
    pub fn validate_skills(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        if c.is_gm() {
            return;
        }
        let mut sends = Vec::new();
        let c = self.client_mut(h).unwrap();
        for i in 0..MAXSKILLTYPE {
            let cap = match i {
                3 | 4 => c.get_str() * 2,         // mining, manufacturing
                1 | 2 | 5 | 8 | 10 => c.level * 2, // agility, prayer, fishing, crafting, pretend corpse
                6 | 7 => c.get_int() * 2,         // farming, alchemy
                // fix: the C++ zeroed the SSN (skill progress) of every other skill at each login
                _ => continue,
            };
            if c.skill_mastery[i] as i32 > cap {
                c.skill_mastery[i] = cap.clamp(0, 255) as u8;
                c.skill_ssn[i] = 0;
                sends.push((i as i32, c.skill_mastery[i] as i32));
            }
        }
        for (i, v) in sends {
            self.notify(h, NOTIFY_SKILL, &[i, v], None);
        }
    }
}

/// One item of MSGID_PLAYERITEMLISTCONTENTS: 44 bytes in the bag (with the equipped flag), 43 in the bank.
pub fn item_entry(w: &mut W<'_>, o: usize, it: &Item, equipped: Option<bool>) {
    let mut n = cstr(&it.cfg.name).to_vec();
    n.resize(20, 0);
    w.bytes(o, &n);
    w.u32(o + 20, it.count);
    w.u8(o + 24, it.cfg.item_type as u8);
    w.u8(o + 25, it.cfg.equip_pos as u8);
    let mut p = o + 26;
    if let Some(e) = equipped {
        w.u8(p, e as u8);
        p += 1;
    }
    w.u16(p, it.cfg.level_limit as u16);
    w.u8(p + 2, it.cfg.gender_limit as u8);
    w.u16(p + 3, it.cur_life_span);
    w.u16(p + 5, it.weight);
    w.u16(p + 7, it.cfg.sprite as u16);
    w.u16(p + 9, it.cfg.sprite_frame as u16);
    w.u8(p + 11, it.item_color);
    w.u8(p + 12, it.item_spec_effect_value[1] as u8);
    w.u32(p + 13, it.attribute);
}

/// `iGetMapLocationSide`
pub fn map_location_side(name: &[u8]) -> i32 {
    let n = cstr(name);
    // fix: the C++ read pMapName[length - 2] for a 1-character name
    if n.len() >= 2 && n[n.len() - 2] == b'_' {
        match n[n.len() - 1] {
            b'1' => return ARESDEN,
            b'2' => return ELVINE,
            b'3' => return ISTRIA,
            _ => {}
        }
    }
    for s in [ARESDEN, ELVINE, ISTRIA] {
        if n == SIDE_MAP[s as usize] {
            return s + 10;
        }
    }
    let pre: [(&[u8], i32); 9] = [
        (b"arebrk11", ARESDEN + 10),
        (b"elvbrk11", ELVINE + 10),
        (b"istbrk1", ISTRIA + 10),
        (b"arefarm", ARESDEN),
        (b"elvfarm", ELVINE),
        (b"istfarm", ISTRIA),
        (b"arewrhus", ARESDEN),
        (b"elvwrhus", ELVINE),
        (b"istwrhus", ISTRIA),
    ];
    pre.iter().find(|(p, _)| n.starts_with(p)).map_or(0, |&(_, s)| s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn map_sides() {
        use super::map_location_side as s;
        assert_eq!((s(b"aresden"), s(b"gshop_2"), s(b"elvfarm"), s(b"middleland"), s(b"x")), (11, 2, 2, 0, 0));
    }
}
