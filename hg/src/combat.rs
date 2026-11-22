//! Physical combat: `CalcTotalItemEffect` (HG.cpp ~23030), `calculateAttackEffect` (char/combat.cpp),
//! `iClientMotion_Attack_Handler` + `_CheckAttackType`, `EnduStrippingDamage`, `CalculateSSN_*`,
//! `iGetComboAttackBonus`, `_iGetWeaponSkillType`, `_iGetArrowItemIndex`, `ItemDepleteHandler`.
//! Not ported: angels (SetAngel),
//! apocalypse/flag bonuses, CheckTotalSkillMasteryPoints (the 900-point cap).
use crate::client::*;
use crate::consts::*;
use crate::game::Game;
use crate::map::{OWNERTYPE_NPC, OWNERTYPE_PLAYER};
use crate::npc::{BEHAVIOR_ATTACK, BEHAVIOR_DEAD, MAXNPCS, NPC_CP, NPC_DARK_ELF};
use crate::util::{Rd, W};

pub const HITRATIOFACTOR: f64 = 80.0;
const DIR_X: [i16; 9] = [0, 0, 1, 1, 1, 0, -1, -1, -1];
const DIR_Y: [i16; 9] = [0, -1, -1, 0, 1, 1, 1, 0, -1];
pub const ITEMTYPE_ARROW: i32 = 6;
const ITEM_STORMBRINGER: i32 = 845;
const ITEM_FIRE_BOW: i32 = 873;
const ITEM_DIRECTION_BOW: i32 = 874;
const ITEM_PICKAXE: i32 = 231;
// HG/char/item/Item.h enums
pub const ITEMSTAT_CRITICAL: i32 = 1;
pub const ITEMSTAT_POISONING: i32 = 2;
pub const ITEMSTAT_RIGHTEOUS: i32 = 3;
pub const ITEMSTAT_SHARP: i32 = 7;
pub const ITEMSTAT_ANCIENT: i32 = 9;
pub const ITEMSTAT_MANACONV: i32 = 11;
pub const ITEMSTAT_CRITICAL2: i32 = 12;
const WET_ADDMAGICDMGPCT: i32 = 1;
// HG/char/Magic.h
pub const MAGICPROTECT_PFA: i32 = 1;
pub const MAGICPROTECT_PFM: i32 = 2;
pub const MAGICPROTECT_DS: i32 = 3;
pub const MAGICPROTECT_GDS: i32 = 4;
pub const MAGICPROTECT_AMP: i32 = 5;
// Shared/Skilltypes.h
pub const SKILL_AGILITY: i32 = 1;
pub const SKILL_PRAYER: i32 = 2;
pub const SKILL_MINING: i32 = 3;
pub const SKILL_MANUFACTURING: i32 = 4;
pub const SKILL_FISHING: i32 = 5;
pub const SKILL_FARMING: i32 = 6;
pub const SKILL_ALCHEMY: i32 = 7;
pub const SKILL_CRAFTING: i32 = 8;
pub const SKILL_PRETENDCORPSE: i32 = 10;

/// What `CalcTotalItemEffect` derives from the equipped items (CClient m_cAttackDice*, m_iAdd*, absorptions...).
#[derive(Clone, Debug, Default)]
pub struct ItemFx {
    pub dt_sm: i32,
    pub dr_sm: i32,
    pub ab_sm: i32,
    pub dt_l: i32,
    pub dr_l: i32,
    pub ab_l: i32,
    pub abs_shield: i32,
    pub abs_armor: [i32; MAXITEMEQUIPPOS],
    pub mp_save_ratio: i32,
    pub add_resist_magic: i32,
    pub add_physical_damage: i32,
    pub add_magical_damage: i32,
    pub add_magic_dmg_pct: i32,
    pub is_lucky_effect: bool,
    pub magic_damage_save_item_index: i32,
    pub side_effect_max_hp_down: i32,
    pub add_abs: [i32; 4], // air, earth, fire, water
    pub custom_attack: i32,
    pub custom_defense: i32,
    pub min_ap_sm: i32,
    pub min_ap_l: i32,
    pub max_ap_sm: i32,
    pub max_ap_l: i32,
    pub swe_type: i32,
    pub swe_value: i32,
    pub add_hp: i32,
    pub add_sp: i32,
    pub add_mp: i32,
    pub add_ar: i32,
    pub add_pr: i32,
    pub add_dr: i32,
    pub add_mr: i32,
    pub add_abs_pd: i32,
    pub add_abs_md: i32,
    pub add_cd: i32,
    pub add_exp: i32,
    pub add_gold: i32,
    pub hp_static_stock: i32,
    pub sa_type: i32,
    pub sa_last_sec: i32,
    pub sa_equip_pos: i32,
    pub add_trans_mana: i32,
    pub add_charge_critical: i32,
    pub alter_item_drop_index: i32,
    pub is_right: bool,
}

/// Per-player combat state of CClient that is not saved.
#[derive(Clone, Debug)]
pub struct Cs {
    pub exp_stock: i32,
    pub auto_exp_amount: i32,
    pub hp_stock: i32,
    pub combo: i32,
    pub super_attack_count: i32,
    pub last_damage: i32,
    pub last_damage_time: u32,
    pub is_poisoned: bool,
    pub poison_level: i32,
    pub poison_time: u32,
    pub sa_enabled: bool,
    pub sa_start_time: u32,
    /// m_hasPrecasted: the OBJECTMAGIC motion came before COMMONTYPE_MAGIC
    pub has_precasted: bool,
    pub attack_msg_count: i32,
    pub attack_lat: u32,
    pub arrow_index: i32,
    /// m_dwWarmEffectTime: when a warm potion was drunk (0 = never): 30 s of ice immunity
    pub warm_time: u32,
}

impl Default for Cs {
    fn default() -> Cs {
        Cs { exp_stock: 0, auto_exp_amount: 0, hp_stock: 0, combo: 0, super_attack_count: 0, last_damage: 0, last_damage_time: 0, is_poisoned: false, poison_level: 0, poison_time: 0, sa_enabled: false, sa_start_time: 0, has_precasted: false, attack_msg_count: 0, attack_lat: 0, arrow_index: -1, warm_time: 0 }
    }
}

/// The hit chance in percent: (attacker hit ratio / target defense ratio) * 80, clamped to 15..99.
/// A target facing away (same direction as the attacker) defends with half its ratio.
pub fn hit_chance(hit_ratio: i32, defense_ratio: i32, same_dir: bool) -> i32 {
    let mut dr = defense_ratio;
    if same_dir {
        dr /= 2;
    }
    let dr = dr.max(1);
    (((hit_ratio.max(0) as f64) / dr as f64 * HITRATIOFACTOR) as i32).clamp(MINIMUMHITRATIO, MAXIMUMHITRATIO)
}

/// Weapon damage scaled by strength: ap + ap * (str / 5) / 100, rounded.
pub fn str_scaled(ap: i32, str_: i32) -> i32 {
    let s = if str_ <= 0 { 1.0 } else { str_ as f64 } / 5.0;
    let a = ap as f64;
    (a + a * (s / 100.0) + 0.5) as i32
}

/// The part of `ap` an armour piece absorbs (`absorb` percent, at most 80).
pub fn armor_absorb(absorb: i32, ap: i32) -> i32 {
    if absorb <= 0 {
        return 0;
    }
    (absorb.min(80) as f64 / 100.0 * ap as f64) as i32
}

/// `iGetComboAttackBonus`
pub fn combo_bonus(skill: i32, combo: i32) -> i32 {
    const T5: [i32; 5] = [0, 0, 0, 1, 2];
    const T6: [i32; 5] = [0; 5];
    const T7: [i32; 5] = [0, 0, 1, 2, 3];
    const T8: [i32; 5] = [0, 0, 1, 3, 5];
    const T9: [i32; 5] = [0, 0, 2, 4, 8];
    if combo <= 1 || combo > 4 {
        return 0; // the C++ allows up to 6 but the tables have 5 entries and the count wraps after 4
    }
    let t = match skill {
        5 => &T5,
        6 | 14 => &T6,
        7 | 10 | 21 => &T7,
        8 => &T8,
        9 => &T9,
        _ => return 0,
    };
    t[combo as usize]
}

/// `_iGetWeaponSkillType` from the appearance weapon number.
pub fn weapon_skill(w: i32) -> i32 {
    match w {
        0 => 5,
        1..=2 => 7,
        7 => 9,
        3..=19 => 8,
        20..=29 => 10,
        30..=34 => 14,
        35..=39 => 21,
        _ if w >= 40 => 6,
        _ => 1,
    }
}

/// `_iCalcSkillSSNpoint`
pub fn skill_ssn_point(level: i32) -> u32 {
    if level < 1 {
        1
    } else if level <= 50 {
        level as u32
    } else {
        (level * level / 7) as u32
    }
}

fn nib(a: u32, n: u32) -> i32 {
    ((a >> (n * 4)) & 0xF) as i32
}

impl Game {
    /// `CalcTotalItemEffect` (angels are not ported).
    pub fn calc_total_item_effect(&mut self, h: usize, equip_item: i32, notify: bool) {
        let Some(c) = self.client_mut(h) else { return };
        c.status &= !STATUS_ANGELS; // SetAngel(0, no notify); the worn pendants set theirs again below
        let mut angels: Vec<(usize, i32)> = Vec::new();
        let (r, t) = (c.equip_status[EQUIPPOS_RHAND as usize], c.equip_status[EQUIPPOS_TWOHAND as usize]);
        if r != -1 && t != -1 && c.items[r as usize].is_some() {
            c.is_item_equipped[r as usize] = false;
            c.equip_status[EQUIPPOS_RHAND as usize] = -1;
        }
        let prev_sa = c.fx.sa_type;
        let mut f = ItemFx { magic_damage_save_item_index: -1, alter_item_drop_index: -1, ..Default::default() };
        c.hit_ratio = 0;
        c.defense_ratio = c.get_dex() * 10;
        for (k, it) in c.items.iter().enumerate() {
            if let Some(it) = it {
                if it.cfg.item_effect_type == ITEMEFFECTTYPE_ALTERITEMDROP && it.cur_life_span > 0 {
                    f.alter_item_drop_index = k as i32;
                }
            }
        }
        let mut sa_notify = None;
        let add_stat = |f: &mut ItemFx, t: i32, v: i32, pos: usize| match t {
            1 => f.add_pr += v * 7,
            2 => f.add_ar += v * 7,
            3 => f.add_dr += v * 7,
            4 => f.add_hp += v * 7,
            5 => f.add_sp += v * 7,
            6 => f.add_mp += v * 7,
            7 => f.add_mr += v * 7,
            8 => f.abs_armor[pos.min(MAXITEMEQUIPPOS - 1)] += v * 3,
            9 => f.add_abs_md = (f.add_abs_md + v * 3).min(80),
            10 => f.add_cd += v,
            11 => f.add_exp += v * 10,
            12 => f.add_gold += v * 10,
            _ => {}
        };
        for k in 0..MAXITEMS {
            let Some(it) = c.items[k].as_ref().filter(|_| c.is_item_equipped[k]) else { continue };
            let (ev, a, pos) = (it.cfg.item_effect_value, it.attribute, it.cfg.equip_pos as usize);
            let et = it.cfg.item_effect_type;
            match et {
                ITEMEFFECTTYPE_MAGICDAMAGESAVE => f.magic_damage_save_item_index = k as i32,
                ITEMEFFECTTYPE_HP => f.hp_static_stock += ev[3],
                ITEMEFFECTTYPE_JEWELRY => {
                    match nib(a, 5) {
                        ITEMSTAT_SHARP => (f.dr_sm, f.dr_l) = (f.dr_sm + 1, f.dr_l + 1),
                        ITEMSTAT_ANCIENT => (f.dr_sm, f.dr_l) = (f.dr_sm + 2, f.dr_l + 2),
                        ITEMSTAT_MANACONV => f.add_trans_mana = (f.add_trans_mana + nib(a, 4)).min(13),
                        ITEMSTAT_CRITICAL2 => f.add_charge_critical = (f.add_charge_critical + nib(a, 4)).min(20),
                        _ => {}
                    }
                    add_stat(&mut f, nib(a, 3), nib(a, 2), pos);
                }
                ITEMEFFECTTYPE_ATTACK_SPECABLTY | ITEMEFFECTTYPE_ATTACK_DEFENSE | ITEMEFFECTTYPE_ATTACK_MANASAVE | ITEMEFFECTTYPE_ATTACK_MAXHPDOWN | ITEMEFFECTTYPE_ATTACK => {
                    // fix: the C++ multiplied the small/medium dice and bonus by 100 into a char (a test leftover)
                    (f.dt_sm, f.dr_sm, f.ab_sm) = (ev[0], ev[1], ev[2]);
                    (f.dt_l, f.dr_l, f.ab_l) = if et == ITEMEFFECTTYPE_ATTACK_MANASAVE { (ev[0], ev[1], ev[2]) } else { (ev[3], ev[4], ev[5]) };
                    let bonus = (a >> 28) as i32;
                    f.add_physical_damage += bonus;
                    // DM magic staff/wand and black mage temple staff get half: ceil(bonus / 2)
                    f.add_magical_damage += if matches!(it.cfg.id_num, 1081 | 1082 | 1083) { (bonus + 1) / 2 } else { bonus };
                    c.hit_ratio += c.skill_mastery.get(it.cfg.related_skill as usize).copied().unwrap_or(0) as i32;
                    if a & 1 != 0 {
                        f.custom_attack = (f.custom_attack + it.item_spec_effect_value[1] as i32).clamp(-100, 100);
                        let d = (f.custom_attack as f64 / 100.0 * 5.0) as i32;
                        let (max_sm, max_l) = (f.dt_sm * f.dr_sm + f.ab_sm, f.dt_l * f.dr_l + f.ab_l);
                        if f.custom_attack > 0 {
                            f.min_ap_sm = (f.dt_sm + f.ab_sm + d).max(1).min(max_sm);
                            f.min_ap_l = (f.dt_l + f.ab_l + d).max(1).min(max_l);
                        } else if f.custom_attack < 0 {
                            f.max_ap_sm = (max_sm + d).max(1).max(max_sm);
                            f.max_ap_l = (max_l + d).max(1).max(max_l);
                        }
                    }
                    let t5 = nib(a, 5);
                    if t5 != 0 {
                        (f.swe_type, f.swe_value) = (t5, nib(a, 4));
                        match t5 {
                            ITEMSTAT_RIGHTEOUS => f.is_right = true,
                            ITEMSTAT_SHARP => (f.dr_sm, f.dr_l) = (f.dr_sm + 1, f.dr_l + 1),
                            ITEMSTAT_ANCIENT => (f.dr_sm, f.dr_l) = (f.dr_sm + 2, f.dr_l + 2),
                            _ => {}
                        }
                    }
                    add_stat(&mut f, nib(a, 3), nib(a, 2), pos);
                    match et {
                        ITEMEFFECTTYPE_ATTACK_MAXHPDOWN => {
                            f.side_effect_max_hp_down = it.cfg.special_effect;
                            f.add_ar += 70;
                        }
                        ITEMEFFECTTYPE_ATTACK_MANASAVE => {
                            f.mp_save_ratio = (f.mp_save_ratio + ev[3]).min(80);
                            if ev[4] == WET_ADDMAGICDMGPCT {
                                f.add_magic_dmg_pct += ev[5];
                            }
                        }
                        ITEMEFFECTTYPE_ATTACK_DEFENSE => f.abs_armor[EQUIPPOS_BODY as usize] += it.cfg.special_effect,
                        ITEMEFFECTTYPE_ATTACK_SPECABLTY => {
                            (f.sa_type, f.sa_last_sec, f.sa_equip_pos) = (it.cfg.special_effect, it.cfg.special_effect_value1, pos as i32);
                            if notify && equip_item == k as i32 {
                                sa_notify = Some(f.sa_type);
                            }
                        }
                        _ => {}
                    }
                }
                ITEMEFFECTTYPE_ADDEFFECT => {
                    let v = ev[1];
                    let sv2 = it.item_spec_effect_value[1] as i32;
                    match ev[0] {
                        1 => f.add_resist_magic += v,
                        2 => f.mp_save_ratio = (f.mp_save_ratio + v).min(80),
                        3 => f.add_physical_damage += v,
                        4 => c.defense_ratio += v,
                        5 => f.is_lucky_effect = v != 0,
                        6 => f.add_magical_damage += v,
                        7..=10 => f.add_abs[(ev[0] - 7) as usize] += v,
                        11 => f.add_pr += v,
                        12 => c.hit_ratio += v,
                        13 => f.add_hp += sv2 / 5,
                        14 => f.add_ar += sv2 / 5,
                        15 => f.add_abs_md = (f.add_abs_md + sv2 / 10).min(80),
                        20 => {
                            for p in [EQUIPPOS_HEAD, EQUIPPOS_BODY, EQUIPPOS_ARMS, EQUIPPOS_PANTS] {
                                f.abs_armor[p as usize] += sv2 / 10;
                            }
                        }
                        16..=19 => angels.push(((ev[0] - 16) as usize, ((a & 0xF000_0000) >> 28) as i32 + 1)),
                        _ => {}
                    }
                }
                ITEMEFFECTTYPE_ATTACK_ARROW => {
                    if c.cs.arrow_index == -1 || c.items.get(c.cs.arrow_index as usize).is_none_or(|i| i.is_none()) {
                        c.cs.arrow_index = arrow_index(c);
                    }
                    // the C++ takes the dice from the bow, not from the arrow
                    (f.dt_sm, f.dr_sm, f.ab_sm, f.dt_l, f.dr_l, f.ab_l) = if c.cs.arrow_index == -1 { (0, 0, 0, 0, 0, 0) } else { (ev[0], ev[1], ev[2], ev[3], ev[4], ev[5]) };
                    c.hit_ratio += c.skill_mastery.get(it.cfg.related_skill as usize).copied().unwrap_or(0) as i32;
                }
                ITEMEFFECTTYPE_DEFENSE_SPECABLTY | ITEMEFFECTTYPE_DEFENSE => {
                    c.defense_ratio += ev[0];
                    if a & 1 != 0 {
                        let sv2 = it.item_spec_effect_value[1] as i32;
                        f.custom_defense += sv2;
                        c.defense_ratio = (c.defense_ratio + (sv2 as f64 / 100.0 * ev[0] as f64 / 2.0) as i32).max(1);
                    }
                    match nib(a, 5) {
                        ITEMSTAT_SHARP => (f.dr_sm, f.dr_l) = (f.dr_sm + 1, f.dr_l + 1),
                        ITEMSTAT_ANCIENT => (f.dr_sm, f.dr_l) = (f.dr_sm + 2, f.dr_l + 2),
                        ITEMSTAT_MANACONV => f.add_trans_mana = (f.add_trans_mana + nib(a, 4)).min(13),
                        ITEMSTAT_CRITICAL2 => f.add_charge_critical = (f.add_charge_critical + nib(a, 4)).min(20),
                        _ => {}
                    }
                    add_stat(&mut f, nib(a, 3), nib(a, 2), pos);
                    if pos == EQUIPPOS_LHAND as usize {
                        f.abs_shield = ev[1];
                    } else {
                        f.abs_armor[pos.min(MAXITEMEQUIPPOS - 1)] += ev[1];
                    }
                    if et == ITEMEFFECTTYPE_DEFENSE_SPECABLTY {
                        (f.sa_type, f.sa_last_sec, f.sa_equip_pos) = (it.cfg.special_effect, it.cfg.special_effect_value1, pos as i32);
                        if notify && equip_item == k as i32 {
                            sa_notify = Some(f.sa_type);
                        }
                    }
                }
                _ => {}
            }
        }
        let (sa, sa_time, enabled) = (f.sa_type, c.special_ability_time, c.cs.sa_enabled);
        c.fx = f;
        for (i, v) in angels {
            self.set_angel_stat(h, i, v);
            self.set_angel(h, STATUS_ANGELSTR << i);
        }
        if let Some(t) = sa_notify {
            self.notify(h, NOTIFY_SPECIALABILITYSTATUS, &[2, t, sa_time], None);
        }
        let lost = prev_sa != 0 && sa == 0;
        let changed = prev_sa != 0 && sa != 0 && prev_sa != sa;
        if notify && lost {
            self.notify(h, NOTIFY_SPECIALABILITYSTATUS, &[4], None);
        }
        if notify && (lost || changed) && enabled {
            if changed {
                // fix: the C++ sent this one to client `i`, an uninitialised loop variable
                self.notify(h, NOTIFY_SPECIALABILITYSTATUS, &[3], None);
            }
            let c = self.client_mut(h).unwrap();
            c.cs.sa_enabled = false;
            c.special_ability_time = SPECABLTYTIMESEC;
            c.appr[3] &= 0xFF0Fu16 as i16;
            self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        }
    }

    /// `ItemDepleteHandler` (no item log)
    pub fn item_deplete(&mut self, h: usize, k: usize, use_result: bool) {
        let Some(c) = self.client_mut(h) else { return };
        if k >= MAXITEMS || c.items[k].is_none() {
            return;
        }
        let it = c.items[k].clone().unwrap();
        // (the C++ callers that pass bIsLog = FALSE, the slate pieces, deplete materials: not logged either way)
        if !matches!(it.cfg.item_type, 5 | 7 | 3 | 11 | 12) && !matches!(it.cfg.id_num, 380..=382) {
            self.item_log(ITEMLOG_DEPLETE, h, -1, &it, false);
        }
        self.release_item(h, k, true);
        self.notify(h, NOTIFY_ITEMDEPLETED_ERASEITEM, &[k as i32, use_result as i32], None);
        let c = self.client_mut(h).unwrap();
        c.items[k] = None;
        c.is_item_equipped[k] = false;
        c.cs.arrow_index = arrow_index(c);
        self.calc_total_weight(h);
    }

    /// `ReleaseItemHandler`: unequip, clear the item's appearance bits, show the change to the players around
    /// (OBJECTNULLACTION, when `notice`) and recompute the item effects. The angelic pendants' stat bonus comes
    /// with angels (only their SETTING_SUCCESS notify is sent).
    pub fn release_item(&mut self, h: usize, k: usize, notice: bool) {
        let Some(c) = self.client_mut(h) else { return };
        if k >= c.items.len() {
            return;
        }
        let Some((pos, et, id)) = c.items[k].as_ref().filter(|i| i.cfg.item_type == crate::player_data::ITEMTYPE_EQUIP).map(|i| (i.cfg.equip_pos as i32, i.cfg.item_effect_type, i.cfg.id_num)) else { return };
        if !c.is_item_equipped[k] {
            return;
        }
        let (a2, a3, a4) = (c.appr[1] as u16, c.appr[2] as u16, c.appr[3] as u16);
        let (a2, a3, a4, col) = match pos {
            EQUIPPOS_RHAND => {
                c.status &= !0x0F; // the weapon speed nibble
                (a2 & 0xF00F, a3, a4, 0x0FFF_FFFF)
            }
            EQUIPPOS_LHAND => (a2 & 0xFFF0, a3, a4, 0xF0FF_FFFF),
            EQUIPPOS_TWOHAND => (a2 & 0xF00F, a3, a4, 0x0FFF_FFFF),
            EQUIPPOS_BODY => (a2, a3 & 0x0FFF, a4 & 0xFF7F, 0xFF0F_FFFF),
            EQUIPPOS_BACK => (a2, a3, a4 & 0xF0FF, 0xFFF0_FFFF),
            EQUIPPOS_ARMS => (a2, a3 & 0xFFF0, a4, 0xFFFF_0FFF),
            EQUIPPOS_PANTS => (a2, a3 & 0xF0FF, a4, 0xFFFF_F0FF),
            EQUIPPOS_LEGGINGS => (a2, a3, a4 & 0x0FFF, 0xFFFF_FF0F),
            EQUIPPOS_HEAD => (a2, a3 & 0xFF0F, a4, 0xFFFF_FFF0),
            EQUIPPOS_FULLBODY => (a2, a3 & 0x0FFF, a4, 0xFFF0_FFFF),
            _ => (a2, a3, a4, 0xFFFF_FFFFu32),
        };
        let a4 = match et {
            ITEMEFFECTTYPE_ATTACK_SPECABLTY => a4 & 0xFFF3,
            ITEMEFFECTTYPE_DEFENSE_SPECABLTY => a4 & 0xFFFC,
            _ => a4,
        };
        (c.appr[1], c.appr[2], c.appr[3]) = (a2 as i16, a3 as i16, a4 as i16);
        c.appr_color = (c.appr_color as u32 & col) as i32;
        c.is_item_equipped[k] = false;
        if (0..MAXITEMEQUIPPOS as i32).contains(&pos) {
            c.equip_status[pos as usize] = -1;
        }
        if pos == EQUIPPOS_LFINGER && (908..=911).contains(&id) {
            // ITEM_ANGELICPENDANT_STR / DEX / INT / MAG
            self.set_angel_stat(h, (id - 908) as usize, 0);
            self.set_angel(h, 0);
            self.notify(h, NOTIFY_SETTING_SUCCESS, &[], None);
        }
        if matches!(id, 865 | 866) {
            // ITEM_RESURWAND_MS20 / MS10: the resurrection spell goes with the wand
            self.client_mut(h).unwrap().magic_mastery[94] = 0;
            self.notify(h, NOTIFY_SPELL_SKILL, &[], None);
        }
        if notice {
            self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        }
        self.calc_total_item_effect(h, k as i32, true);
    }

    /// `CClient::SetAngelStr/Dex/Int/Mag`: the bonus, then HP/SP (str) or MP (int, mag) capped to the new maximum.
    pub fn set_angel_stat(&mut self, h: usize, i: usize, v: i32) {
        let Some(c) = self.client_mut(h) else { return };
        c.angel[i] = v;
        let mut n = Vec::new();
        if i == 0 && c.hp > c.get_max_hp() {
            c.hp = c.get_max_hp();
            n.push(NOTIFY_HP);
        }
        if i == 0 && c.sp > c.get_max_sp() {
            c.sp = c.get_max_sp();
            n.push(NOTIFY_SP);
        }
        if i >= 2 && c.mp > c.get_max_mp() {
            c.mp = c.get_max_mp();
            n.push(NOTIFY_MP);
        }
        for t in n {
            self.notify(h, t, &[], None);
        }
    }

    /// `SetAngel(status)`: 0 clears the four angel bits, else sets `bit`; the players around see the change.
    pub fn set_angel(&mut self, h: usize, bit: i32) {
        let Some(c) = self.client_mut(h) else { return };
        if bit == 0 {
            c.status &= !STATUS_ANGELS;
        } else {
            c.status |= bit;
        }
        // fix: at load (CalcTotalItemEffect from the player data) the C++ broadcast a player not yet on the map
        if c.is_init_complete {
            self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        }
    }

    /// `CalculateSSN_SkillIndex`
    pub fn calc_ssn_skill(&mut self, h: usize, s: i32, mut v: i32) {
        let Some(c) = self.client_mut(h) else { return };
        if !c.is_init_complete || c.is_killed || s < 0 || s as usize >= MAXSKILLTYPE || c.skill_mastery[s as usize] == 0 {
            return;
        }
        let s = s as usize;
        match s as i32 {
            SKILL_AGILITY | SKILL_PRAYER | SKILL_PRETENDCORPSE => v *= v * 8,
            SKILL_CRAFTING | SKILL_FARMING | SKILL_MANUFACTURING | SKILL_ALCHEMY | SKILL_MINING | SKILL_FISHING => v *= v * 2,
            _ => {}
        }
        let old = c.skill_ssn[s];
        c.skill_ssn[s] = c.skill_ssn[s].wrapping_add(v as u32);
        if c.skill_mastery[s] >= 100 || c.skill_ssn[s] <= skill_ssn_point(c.skill_mastery[s] as i32 + 1) {
            return;
        }
        c.skill_mastery[s] += 1;
        let cap = match s as i32 {
            SKILL_MINING | SKILL_MANUFACTURING => Some(c.get_str() * 2),
            SKILL_PRETENDCORPSE | SKILL_FISHING => Some(c.level * 2),
            SKILL_FARMING | SKILL_ALCHEMY | SKILL_CRAFTING => Some(c.get_int() * 2),
            _ => None,
        };
        if cap.is_some_and(|cap| c.skill_mastery[s] as i32 > cap) {
            c.skill_mastery[s] -= 1;
            c.skill_ssn[s] = old;
            return;
        }
        c.skill_ssn[s] = 0;
        for p in [EQUIPPOS_TWOHAND, EQUIPPOS_RHAND] {
            let w = c.equip_status[p as usize];
            if w != -1 && c.items[w as usize].as_ref().is_some_and(|i| i.cfg.related_skill == s as i32) {
                c.hit_ratio += 1;
            }
        }
        self.check_total_skill_mastery(h, s);
        let m = self.client(h).unwrap().skill_mastery[s] as i32;
        self.notify(h, NOTIFY_SKILL, &[s as i32, m], None);
    }

    fn weapon_index(c: &Client) -> i16 {
        let t = c.equip_status[EQUIPPOS_TWOHAND as usize];
        if t != -1 { t } else { c.equip_status[EQUIPPOS_RHAND as usize] }
    }

    /// `calculateAttackEffect`: one physical hit; returns the exp the hit itself gives (NPCs' no-die exp).
    #[allow(clippy::too_many_arguments)]
    pub fn attack_effect(&mut self, th: usize, tt: u8, ah: usize, at: u8, tdx: i16, tdy: i16, mode: i32, near: bool, dash: bool) -> i32 {
        let now = self.now();
        let mut exp = 0;
        let mut weapon_type = 0;
        let (mut ap_sm, mut ap_l, mut ahr, adir, aside, berserk, ahp, apos, party);
        let mut asa_value = 0;
        let mut asa = 0u8;
        match at {
            OWNERTYPE_PLAYER => {
                let Some(c) = self.client(ah) else { return 0 };
                if !self.maps[c.map_index].is_attack_enabled {
                    return 0;
                }
                let c = self.client_mut(ah).unwrap();
                if c.status & STATUS_INVISIBILITY != 0 {
                    c.set_status_flag(STATUS_INVISIBILITY, false);
                    c.magic_effect_status[MAGICTYPE_INVISIBILITY as usize] = 0;
                    self.remove_delay_events(ah, OWNERTYPE_PLAYER, MAGICTYPE_INVISIBILITY);
                }
                let c = self.client(ah).unwrap();
                if c.appr[1] as u16 & 0xF000 == 0 {
                    return 0;
                }
                weapon_type = ((c.appr[1] as u16 & 0x0FF0) >> 4) as i32;
                let (f, s) = (c.fx.clone(), c.get_str());
                ahr = c.hit_ratio;
                if weapon_type == 0 {
                    let d = self.rng.dice(1, s / 12).max(1);
                    (ap_sm, ap_l) = (d, d);
                } else {
                    let a = self.rng.dice(f.dt_sm, f.dr_sm) + f.ab_sm;
                    let b = self.rng.dice(f.dt_l, f.dr_l) + f.ab_l;
                    if weapon_type < 40 {
                        (ap_sm, ap_l) = (str_scaled(a, s), str_scaled(b, s));
                    } else {
                        // bNormalMissileAttack: set but never read in the C++
                        let x = self.rng.dice(1, s / 20);
                        let y = self.rng.dice(1, s / 20);
                        (ap_sm, ap_l) = (a + x, b + y);
                    }
                }
                if f.custom_attack != 0 {
                    if f.min_ap_sm != 0 && ap_sm < f.min_ap_sm {
                        ap_sm = f.min_ap_sm;
                    }
                    if f.min_ap_l != 0 && ap_l < f.min_ap_l {
                        ap_l = f.min_ap_l;
                    }
                    if f.max_ap_sm != 0 && ap_sm > f.max_ap_sm {
                        ap_sm = f.max_ap_sm;
                    }
                    if f.max_ap_l != 0 && ap_l > f.max_ap_l {
                        ap_l = f.max_ap_l;
                    }
                }
                let c = self.client(ah).unwrap();
                (adir, aside, berserk) = (c.dir, c.side, c.magic_effect_status[MAGICTYPE_BERSERK as usize] != 0);
                if c.super_attack_left > 0 && mode >= 20 {
                    // critical: + level% of the damage, + 10%, and +130 hit ratio (+ the custom attack value)
                    let lv = c.level as f64 / 100.0;
                    ap_sm += (ap_sm as f64 * lv + 0.5) as i32;
                    ap_l += (ap_l as f64 * lv + 0.5) as i32;
                    ap_sm += ap_sm / 10;
                    ap_l += ap_l / 10;
                    ahr += 130 + f.custom_attack;
                }
                if dash {
                    ahr += 30;
                }
                ahp = c.hp;
                ahr += f.add_ar;
                apos = (c.x, c.y);
                party = c.party_id;
            }
            OWNERTYPE_NPC => {
                let Some(n) = self.npcs.get(ah).and_then(|n| n.as_ref()) else { return 0 };
                if !self.maps[n.map_index].is_attack_enabled {
                    return 0;
                }
                let n = self.npcs[ah].as_mut().unwrap();
                if n.status & STATUS_INVISIBILITY != 0 {
                    n.status &= !STATUS_INVISIBILITY;
                    n.magic_effect_status[MAGICTYPE_INVISIBILITY as usize] = 0;
                    self.remove_delay_events(ah, OWNERTYPE_NPC, MAGICTYPE_INVISIBILITY);
                }
                let n = self.npcs[ah].as_ref().unwrap();
                aside = n.side;
                let (dt, dr) = (n.attack_dice_throw, n.attack_dice_range);
                (ap_sm, ap_l) = (0, 0);
                if dt != 0 {
                    let d = self.rng.dice(dt, dr);
                    (ap_sm, ap_l) = (d, d);
                }
                let n = self.npcs[ah].as_ref().unwrap();
                (ahr, adir, berserk, ahp, apos, party) = (n.hit_ratio, n.dir, n.magic_effect_status[MAGICTYPE_BERSERK as usize] != 0, n.hp, (n.x, n.y), 0);
                asa = n.special_ability;
            }
            _ => return 0,
        }
        let _ = ahp;
        let (tdr, tdir, tpos, protect);
        match tt {
            OWNERTYPE_PLAYER => {
                let Some(t) = self.client(th) else { return 0 };
                if t.is_killed || (t.x, t.y) != (tdx, tdy) || t.is_invincible() {
                    return 0;
                }
                if at == OWNERTYPE_PLAYER && self.client(ah).unwrap().is_neutral && t.pk_count == 0 {
                    return 0;
                }
                if t.party_id != 0 && party == t.party_id {
                    return 0;
                }
                if t.status & STATUS_REDSLATE != 0 {
                    return 0;
                }
                (tdir, protect) = (t.dir, t.magic_effect_status[MAGICTYPE_PROTECT as usize]);
                let mut dr = t.defense_ratio;
                if at == OWNERTYPE_PLAYER && self.client(ah).unwrap().is_safe_attack_mode {
                    // safe attack mode only lets enemies be hit (at half damage in fight zones it is per guild)
                    let a = self.client(ah).unwrap();
                    if !self.is_player_hostile(ah, th) {
                        if self.maps[a.map_index].is_fight_zone && a.guild_guid != t.guild_guid {
                            ap_sm /= 2;
                            ap_l /= 2;
                        } else {
                            return 0;
                        }
                    } else {
                        // GetPlayerRelationship 2/6/7 (enemy / criminal): half damage
                        ap_sm /= 2;
                        ap_l /= 2;
                    }
                }
                dr += t.fx.add_dr;
                tdr = dr;
                tpos = (t.x, t.y);
            }
            OWNERTYPE_NPC => {
                let Some(n) = self.npcs.get(th).and_then(|n| n.as_ref()) else { return 0 };
                if n.hp <= 0 || (n.x, n.y) != (tdx, tdy) {
                    return 0;
                }
                (tdir, tdr, protect, tpos) = (n.dir, n.defense_ratio, n.magic_effect_status[MAGICTYPE_PROTECT as usize], (n.x, n.y));
                if at == OWNERTYPE_PLAYER {
                    let c = self.client(ah).unwrap();
                    if matches!(n.type_, crate::crusade::NPC_ESG | crate::crusade::NPC_GMG) && (c.side == NEUTRAL || n.side == c.side) {
                        return 0;
                    }
                    if weapon_type == 25 && n.action_limit == 5 && n.build_count > 0 && (c.crusade_duty == 2 || self.ast.ev.is_some() || c.admin_user_level != 0) {
                        // a constructor's hammer builds (no damage); anyone else's hits it like any weapon
                        self.hammer_structure(ah, th);
                        return 0;
                    }
                    if weapon_type == 27 && n.crops != 0 && n.action_limit == 5 && n.build_count > 0 {
                        // a hoe on a crop (farming.rs); any other weapon hits it like an NPC
                        self.hoe_crop(ah, th);
                        return 0;
                    }
                    let c = self.client(ah).unwrap();
                    let w = Game::weapon_index(c);
                    if w != -1 && c.equip_status[EQUIPPOS_TWOHAND as usize] == w && c.items[w as usize].as_ref().is_some_and(|i| i.cfg.id_num == 616) && n.type_ == 31 {
                        ap_l += 5; // Demon Slayer against demons
                    }
                }
            }
            _ => return 0,
        }
        if at == OWNERTYPE_PLAYER && tt == OWNERTYPE_PLAYER {
            let m = self.client(th).unwrap().map_index;
            if self.maps[m].attribute(apos.0, apos.1, 0x06) != 0 || self.maps[m].attribute(tpos.0, tpos.1, 0x06) != 0 {
                return 0;
            }
        }
        if at == OWNERTYPE_PLAYER {
            let d = self.client(ah).unwrap().get_dex();
            if d > 50 {
                ahr += d - 50;
            }
            if weapon_type >= 40 {
                let wx = self.maps[self.client(ah).unwrap().map_index].weather;
                ahr -= match wx {
                    1 => ahr / 20,
                    2 => ahr / 10,
                    3 => ahr / 4,
                    _ => 0,
                };
            }
        }
        ahr = ahr.max(0);
        let mut tdr = tdr;
        if at == OWNERTYPE_PLAYER {
            let mut bow = false;
            let c = self.client(ah).unwrap();
            let two = c.equip_status[EQUIPPOS_TWOHAND as usize];
            if two != -1 {
                let Some(weapon2) = c.items[two as usize].as_ref() else {
                    // the C++ disconnects a player whose two-hand slot points at nothing
                    self.client_mut(ah).unwrap().is_item_equipped[two as usize] = false;
                    self.delete_client(ah, true, true, true, false);
                    return 0;
                };
                if weapon2.cfg.item_effect_type == ITEMEFFECTTYPE_ATTACK_ARROW {
                    let ai = c.cs.arrow_index;
                    if ai == -1 || c.items[ai as usize].is_none() {
                        return 0;
                    }
                    let left = {
                        let it = self.client_mut(ah).unwrap().items[ai as usize].as_mut().unwrap();
                        it.count = it.count.saturating_sub(1);
                        it.count
                    };
                    if left == 0 {
                        self.item_deplete(ah, ai as usize, false);
                    } else {
                        self.notify(ah, NOTIFY_SETITEMCOUNT, &[ai, left as i32, 0], None);
                        self.calc_total_weight(ah);
                    }
                    if protect == MAGICPROTECT_PFA {
                        return 0;
                    }
                    bow = true;
                }
            }
            // fix: the C++ only added the defense shields when the attacker held a two-handed non-bow weapon
            if !bow {
                tdr += match protect {
                    MAGICPROTECT_DS => 40,
                    MAGICPROTECT_GDS => 100,
                    _ => 0,
                };
            }
        } else {
            match protect {
                MAGICPROTECT_PFA if self.npcs[ah].as_ref().unwrap().type_ as i32 == NPC_DARK_ELF => {
                    if (tpos.0 - apos.0).abs() >= 1 || (tpos.1 - apos.1).abs() >= 1 {
                        return 0;
                    }
                }
                MAGICPROTECT_DS => tdr += 40,
                MAGICPROTECT_GDS => tdr += 100,
                _ => {}
            }
        }
        if tdr < 0 {
            tdr = 1;
        }
        let chance = hit_chance(ahr, tdr, adir == tdir);
        if berserk && mode < 20 {
            ap_sm *= 2;
            ap_l *= 2;
        }
        if at == OWNERTYPE_PLAYER {
            let a = self.client(ah).unwrap().fx.add_physical_damage;
            ap_sm += a;
            ap_l += a;
        }
        if near {
            ap_sm /= 2;
            ap_l /= 2;
        }
        if tt == OWNERTYPE_PLAYER {
            let v = self.client(th).unwrap().vit;
            ap_sm -= self.rng.dice(1, v / 10) - 1;
            ap_l -= self.rng.dice(1, v / 10) - 1;
        }
        if at == OWNERTYPE_PLAYER {
            let c = self.client(ah).unwrap();
            let two = c.equip_status[EQUIPPOS_TWOHAND as usize];
            if let Some(it) = (two != -1).then(|| c.items[two as usize].as_ref()).flatten() {
                // Dark Executor at night, Lighting Blade by day
                if (self.day_or_night == 2 && it.cfg.id_num == 851) || (self.day_or_night == 1 && it.cfg.id_num == 852) {
                    ap_sm += 10;
                    ap_l += 10;
                }
            }
            ap_sm = ap_sm.max(1);
            ap_l = ap_l.max(1);
        } else {
            ap_sm = ap_sm.max(0);
            ap_l = ap_l.max(0);
        }
        let mut killed = false;
        let mut killed_dice = 0;
        if self.rng.dice(1, 100) > chance {
            if at == OWNERTYPE_PLAYER {
                self.client_mut(ah).unwrap().cs.combo = 0;
            }
            return 0;
        }
        if at == OWNERTYPE_PLAYER {
            let c = self.client(ah).unwrap();
            let (hungry, mi) = (c.hunger_status <= 10 || c.sp <= 0, c.map_index);
            if hungry && self.rng.dice(1, 10) == 5 {
                return 0;
            }
            let fz = self.maps[mi].is_fight_zone;
            let c = self.clients[ah].as_mut().unwrap();
            c.cs.combo += 1;
            if c.cs.combo > 4 {
                c.cs.combo = 1;
            }
            let mut cb = combo_bonus(weapon_skill(weapon_type), c.cs.combo);
            if c.cs.combo > 1 && c.fx.add_cd != 0 {
                cb += c.fx.add_cd;
            }
            ap_sm += cb;
            ap_l += cb;
            match c.fx.swe_type {
                ITEMSTAT_CRITICAL if c.super_attack_left > 0 && mode >= 20 => {
                    ap_sm += c.fx.swe_value;
                    ap_l += c.fx.swe_value;
                }
                ITEMSTAT_POISONING => (asa, asa_value) = (61, c.fx.swe_value * 5),
                ITEMSTAT_RIGHTEOUS => asa = 62,
                _ => {}
            }
            if fz {
                ap_sm += ap_sm / 3;
                ap_l += ap_l / 3;
            }
            if self.is_apocalypse_map(mi) {
                (ap_sm, ap_l) = ((ap_sm as f64 * 1.2) as i32, (ap_l as f64 * 1.2) as i32);
            }
            if self.check_nearby_flags(ah) {
                (ap_sm, ap_l) = ((ap_sm as f64 * 1.3) as i32, (ap_l as f64 * 1.3) as i32);
            }
            let c = self.client(ah).unwrap();
            if tt == OWNERTYPE_PLAYER && self.cr.mode && c.crusade_duty == 1 {
                // a fighter's bonus against players
                match c.level {
                    ..=80 => (ap_sm, ap_l) = (ap_sm * 2, ap_l * 2),
                    ..=100 => (ap_sm, ap_l) = (ap_sm + ap_sm * 7 / 10, ap_l + ap_l * 7 / 10),
                    _ => (ap_sm, ap_l) = (ap_sm + ap_sm / 3, ap_l + ap_l / 3),
                }
            }
        }
        let weapon = if at == OWNERTYPE_PLAYER { weapon_type } else { 1 };
        match tt {
            OWNERTYPE_PLAYER => {
                self.clear_skill_using_status(th);
                let t = self.client(th).unwrap();
                if now.wrapping_sub(t.time) > RAGPROTECTIONTIME as u32 {
                    return 0; // a lagging player cannot be hit
                }
                if asa == 62 && t.reputation < 0 {
                    ap_sm += (t.reputation.abs() / 10).min(10);
                }
                let r = self.rng.dice(1, 10000);
                let t = self.client(th).unwrap();
                let hit_point = match r {
                    ..=4999 => 1,
                    5000..=7499 => 2,
                    7500..=8999 => 3,
                    _ => 4,
                };
                let f = &t.fx;
                let abs = match hit_point {
                    1 => f.abs_armor[EQUIPPOS_BODY as usize],
                    2 => f.abs_armor[EQUIPPOS_PANTS as usize] + f.abs_armor[EQUIPPOS_LEGGINGS as usize],
                    3 => f.abs_armor[EQUIPPOS_ARMS as usize],
                    _ => f.abs_armor[EQUIPPOS_HEAD as usize],
                };
                // the shield absorption is commented out in the C++ (only the armour absorbs)
                ap_sm = (ap_sm - armor_absorb(abs, ap_sm)).max(1);
                if at == OWNERTYPE_PLAYER && self.client(ah).unwrap().cs.sa_enabled {
                    match self.client(ah).unwrap().fx.sa_type {
                        1 => ap_sm = ap_sm.max(t.hp / 2).max(1),
                        4 => ap_sm = t.hp,
                        5 => {
                            let a = self.client_mut(ah).unwrap();
                            a.hp = (a.hp + ap_sm).min(a.get_max_hp());
                            self.notify(ah, NOTIFY_HP, &[], None);
                        }
                        2 if t.magic_effect_status[MAGICTYPE_ICE as usize] == 0 => {
                            let t = self.client_mut(th).unwrap();
                            t.magic_effect_status[MAGICTYPE_ICE as usize] = 1;
                            t.set_status_flag(STATUS_FROZEN, true);
                            self.send_event_near_type_a(th, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
                            self.register_delay_event(DELAYEVENTTYPE_MAGICRELEASE, MAGICTYPE_ICE, now.wrapping_add(30 * 1000), th, OWNERTYPE_PLAYER);
                            self.notify(th, NOTIFY_MAGICEFFECTON, &[MAGICTYPE_ICE, 1], None);
                        }
                        3 if t.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] == 0 => {
                            self.client_mut(th).unwrap().magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] = 2;
                            self.register_delay_event(DELAYEVENTTYPE_MAGICRELEASE, MAGICTYPE_HOLDOBJECT, now.wrapping_add(10 * 1000), th, OWNERTYPE_PLAYER);
                            self.notify(th, NOTIFY_MAGICEFFECTON, &[MAGICTYPE_HOLDOBJECT, 10], None);
                        }
                        _ => {}
                    }
                }
                let t = self.client(th).unwrap();
                if at == OWNERTYPE_PLAYER && t.cs.sa_enabled {
                    match t.fx.sa_type {
                        50 => {
                            let w = Game::weapon_index(self.client(ah).unwrap());
                            if let Some(it) = (w != -1).then(|| self.client_mut(ah).unwrap().items[w as usize].as_mut()).flatten() {
                                it.cur_life_span = 0;
                            }
                        }
                        51 if hit_point == t.fx.sa_equip_pos => ap_sm = 0,
                        52 => ap_sm = 0,
                        _ => {}
                    }
                }
                let (lucky, thp) = (self.client(th).unwrap().fx.is_lucky_effect, self.client(th).unwrap().hp);
                if lucky && self.rng.dice(1, 10) < 5 && thp <= ap_sm {
                    ap_sm = 1;
                }
                let t = self.client(th).unwrap();
                let es = &t.equip_status;
                let strip: Vec<(i16, i32)> = match hit_point {
                    1 => vec![(if es[EQUIPPOS_BODY as usize] == -1 { es[EQUIPPOS_FULLBODY as usize] } else { es[EQUIPPOS_BODY as usize] }, 2000)],
                    2 => vec![(es[EQUIPPOS_PANTS as usize], 2000), (es[EQUIPPOS_LEGGINGS as usize], 2000)],
                    3 => vec![(es[EQUIPPOS_ARMS as usize], 2000)],
                    _ => vec![(es[EQUIPPOS_HEAD as usize], 3000)],
                };
                for (k, _) in strip {
                    self.endu_stripping(th, k);
                }
                if asa == 2 && self.client(th).unwrap().magic_effect_status[MAGICTYPE_PROTECT as usize] != 0 {
                    let p = self.client(th).unwrap().magic_effect_status[MAGICTYPE_PROTECT as usize];
                    self.notify(th, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_PROTECT, p], None);
                    let t = self.client_mut(th).unwrap();
                    let flag = match p {
                        MAGICPROTECT_PFA => STATUS_PFA,
                        MAGICPROTECT_PFM | MAGICPROTECT_AMP => STATUS_PFM,
                        _ => STATUS_DEFENSESHIELD,
                    };
                    t.set_status_flag(flag, false);
                    t.magic_effect_status[MAGICTYPE_PROTECT as usize] = 0;
                }
                if !self.client(th).unwrap().cs.is_poisoned && matches!(asa, 5 | 6 | 61) && !self.resist_poison(th, OWNERTYPE_PLAYER) {
                    let t = self.client_mut(th).unwrap();
                    t.cs.is_poisoned = true;
                    t.cs.poison_level = match asa {
                        5 => 15,
                        6 => 40,
                        _ => asa_value,
                    };
                    t.cs.poison_time = now;
                    t.set_status_flag(STATUS_POISON, true);
                    let l = t.cs.poison_level;
                    self.notify(th, NOTIFY_MAGICEFFECTON, &[MAGICTYPE_POISON, l], None);
                }
                let t = self.client_mut(th).unwrap();
                t.hp -= ap_sm;
                t.cs.last_damage_time = now;
                if t.hp <= 0 {
                    if at == OWNERTYPE_PLAYER {
                        let p = (t.x, t.y);
                        self.analyze_criminal_action(ah, p.0, p.1, false);
                    }
                    let t = self.client(th).unwrap();
                    killed_dice = t.level;
                    self.killed_handler(th, ah, at, ap_sm);
                    killed = true;
                } else if ap_sm > 0 {
                    if t.fx.add_trans_mana > 0 {
                        let cap = 2 * t.get_mag() + 2 * t.level + t.get_int() / 2;
                        t.mp = (t.mp + (t.fx.add_trans_mana as f64 / 100.0 * ap_sm as f64) as i32).min(cap);
                    }
                    if t.fx.add_charge_critical > 0 {
                        let cc = t.fx.add_charge_critical;
                        if self.rng.dice(1, 100) < cc {
                            let t = self.client_mut(th).unwrap();
                            if t.super_attack_left < t.level / 10 {
                                t.super_attack_left += 1;
                            }
                            self.notify(th, NOTIFY_SUPERATTACKLEFT, &[], None);
                        }
                    }
                    self.notify(th, NOTIFY_HP, &[], None);
                    let fz = at == OWNERTYPE_PLAYER && self.maps[self.client(ah).unwrap().map_index].is_fight_zone;
                    let move_damage = if fz { 60 } else { 40 };
                    let dir = damage_move_dir(apos, tpos);
                    if ap_sm >= move_damage && dir != 0 {
                        self.client_mut(th).unwrap().cs.last_damage = ap_sm;
                        self.notify(th, NOTIFY_DAMAGEMOVE, &[dir as i32, ap_sm, weapon], None);
                    } else if self.rng.dice(1, 10000) >= 500 {
                        self.send_event_near_type_a(th, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, ap_sm as i16, weapon as i16, 0);
                    }
                    let t = self.client_mut(th).unwrap();
                    if t.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] == 1 {
                        t.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] = 0;
                        self.notify(th, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_HOLDOBJECT, 1], None);
                        self.remove_delay_events(th, OWNERTYPE_PLAYER, MAGICTYPE_HOLDOBJECT);
                    }
                    let t = self.client_mut(th).unwrap();
                    t.cs.super_attack_count += 1;
                    if t.cs.super_attack_count > 14 {
                        t.cs.super_attack_count = 0;
                        if t.super_attack_left < t.level / 10 {
                            t.super_attack_left += 1;
                        }
                        self.notify(th, NOTIFY_SUPERATTACKLEFT, &[], None);
                    }
                }
            }
            _ => {
                let n = self.npcs[th].as_ref().unwrap();
                if n.behavior == BEHAVIOR_DEAD || n.is_killed {
                    return 0;
                }
                if self.cr.mode {
                    use crate::crusade::*;
                    if aside == n.side {
                        if matches!(n.type_, NPC_ESG | NPC_GMG | NPC_LWB | NPC_GHK | NPC_GHKABS | NPC_TK | NPC_BG | NPC_CP) {
                            return 0;
                        }
                    } else if n.type_ == NPC_GMG {
                        self.gmg_hit(th, aside, ap_l);
                    }
                }
                let n = self.npcs[th].as_ref().unwrap();
                if matches!(n.action_limit, 1 | 2 | 6) {
                    return 0;
                }
                let mut dmg = if n.size == 0 { ap_sm } else { ap_l };
                if n.abs_damage < 0 {
                    let d = dmg as f64;
                    dmg = ((d - d * (n.abs_damage.abs() as f64 / 100.0)) as i32).max(1);
                }
                let n = self.npcs[th].as_mut().unwrap();
                if asa == 2 && n.magic_effect_status[MAGICTYPE_PROTECT as usize] != 0 {
                    let flag = match n.magic_effect_status[MAGICTYPE_PROTECT as usize] {
                        MAGICPROTECT_PFA => STATUS_PFA,
                        MAGICPROTECT_PFM | MAGICPROTECT_AMP => STATUS_PFM,
                        _ => STATUS_DEFENSESHIELD,
                    };
                    n.status &= !flag;
                    n.magic_effect_status[MAGICTYPE_PROTECT as usize] = 0;
                }
                if matches!(n.action_limit, 0 | 3 | 5) {
                    n.hp -= dmg;
                }
                if n.hp <= 0 {
                    killed_dice = n.hit_dice;
                    self.npc_killed(ah, at, th, dmg);
                    killed = true;
                } else {
                    let skip = (!matches!(n.type_, 21 | 55 | 56) && n.side == aside) || n.action_limit != 0 || n.is_perm_attack_mode || (n.is_summoned && n.summon_control_mode == 1) || n.type_ as i32 == NPC_CP;
                    if !skip && self.rng.dice(1, 3) == 2 {
                        let n = self.npcs[th].as_ref().unwrap();
                        let switch = if n.behavior == BEHAVIOR_ATTACK {
                            let (tx, ty) = self.unit_pos(n.target, n.target_type).unwrap_or((0, 0));
                            let d1 = (n.x - tx) as i32 * (n.x - tx) as i32 + (n.y - ty) as i32 * (n.y - ty) as i32;
                            let (ax, ay) = self.unit_pos(ah, at).unwrap_or((0, 0));
                            let d2 = (n.x - ax) as i32 * (n.x - ax) as i32 + (n.y - ay) as i32 * (n.y - ay) as i32;
                            d2 <= d1
                        } else {
                            true
                        };
                        if switch {
                            let n = self.npcs[th].as_mut().unwrap();
                            (n.behavior, n.behavior_turn_count, n.target, n.target_type) = (BEHAVIOR_ATTACK, 0, ah, at);
                        }
                    }
                    // gold drop value is never loaded (commented out in the C++ decoder), so this is dice(1, 3)
                    if self.rng.dice(1, 3) == 1 && self.npcs[th].as_ref().unwrap().action_limit == 0 {
                        self.npcs[th].as_mut().unwrap().time = now;
                    }
                    let n = self.npcs[th].as_ref().unwrap();
                    if weapon_type < 40 && n.action_limit == 4 {
                        self.npc_damage_move(th, apos, Some((ah, at)));
                    } else {
                        self.send_event_near_npc_v(th, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, dmg as i16, weapon as i16, 0);
                    }
                    let n = self.npcs[th].as_ref().unwrap();
                    let hold = n.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize];
                    if hold == 1 || (hold == 2 && n.hit_dice > 50 && self.rng.dice(1, 10) == 5) {
                        self.npcs[th].as_mut().unwrap().magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] = 0;
                        self.remove_delay_events(th, OWNERTYPE_NPC, MAGICTYPE_HOLDOBJECT);
                    }
                    let n = self.npcs[th].as_mut().unwrap();
                    if n.no_die_remain_exp > 0 && !n.is_summoned && at == OWNERTYPE_PLAYER && self.clients[ah].is_some() {
                        let e = if n.no_die_remain_exp > dmg {
                            n.no_die_remain_exp -= dmg;
                            dmg
                        } else {
                            std::mem::take(&mut n.no_die_remain_exp)
                        };
                        let ty = n.type_;
                        let c = self.client(ah).unwrap();
                        exp = e + (c.fx.add_exp as f64 / 100.0 * e as f64) as i32;
                        if self.cr.mode {
                            exp /= 3;
                        }
                        if c.level > 100 && matches!(ty, 55 | 56) {
                            exp = 0;
                        }
                    }
                }
            }
        }
        if at == OWNERTYPE_PLAYER && self.client(ah).is_some() {
            let c = self.client(ah).unwrap();
            let w = Game::weapon_index(c);
            if w != -1 {
                if let Some(it) = c.items[w as usize].as_ref() {
                    let (id, skill, max_life) = (it.cfg.id_num, it.cfg.related_skill, it.max_life_span);
                    if id != ITEM_PICKAXE {
                        let v = if !killed {
                            1
                        } else if c.hp <= 3 {
                            self.rng.dice(1, killed_dice) * 2
                        } else {
                            self.rng.dice(1, killed_dice)
                        };
                        self.calc_ssn_skill(ah, skill, v);
                    }
                    if max_life != 0 {
                        let mut off = 1;
                        if (1..40).contains(&weapon_type) {
                            match self.maps[self.client(ah).unwrap().map_index].weather {
                                1 if self.rng.dice(1, 3) == 1 => off += 1,
                                2 if self.rng.dice(1, 2) == 1 => off += self.rng.dice(1, 2),
                                3 if self.rng.dice(1, 2) == 1 => off += self.rng.dice(1, 3),
                                _ => {}
                            }
                        }
                        let c = self.client_mut(ah).unwrap();
                        let neutral = c.is_neutral;
                        if let Some(it) = c.items[w as usize].as_mut() {
                            if !neutral {
                                it.cur_life_span = it.cur_life_span.saturating_sub(off as u16);
                            }
                            if it.cur_life_span == 0 {
                                let pos = it.cfg.equip_pos;
                                self.notify(ah, NOTIFY_ITEMLIFESPANEND, &[pos, w as i32], None);
                                self.release_item(ah, w as usize, true);
                            }
                        }
                    }
                }
            }
        }
        exp
    }

    /// The two-step knock-back of an action-limit-4 NPC (dummies) hit by a melee weapon.
    /// `attacker`: the melee hitter, for bCheckEnergySphereDestination (the C++ checks after a melee hit only)
    pub fn npc_damage_move(&mut self, th: usize, apos: (i16, i16), attacker: Option<(usize, u8)>) {
        let mut dir = damage_move_dir(apos, self.unit_pos(th, OWNERTYPE_NPC).unwrap()) as i32;
        if dir == 0 {
            return;
        }
        for step in 0..2 {
            let n = self.npcs[th].as_ref().unwrap();
            let (m, x, y) = (n.map_index, n.x, n.y);
            let (mut dx, mut dy) = (x + DIR_X[dir as usize], y + DIR_Y[dir as usize]);
            if !self.maps[m].get_moveable(dx, dy).0 {
                dir = self.rng.dice(1, 8);
                (dx, dy) = (x + DIR_X[dir as usize], y + DIR_Y[dir as usize]);
                if !self.maps[m].get_moveable(dx, dy).0 {
                    return;
                }
            }
            self.maps[m].clear_owner(th as i16, OWNERTYPE_NPC, x, y);
            self.maps[m].set_owner(th as i16, OWNERTYPE_NPC, dx, dy);
            let n = self.npcs[th].as_mut().unwrap();
            (n.x, n.y, n.dir) = (dx, dy, dir as u8);
            self.send_event_near_npc(th, MSGID_EVENT_MOTION, OBJECTMOVE as u16);
            if let (1, Some((ah, at))) = (step, attacker) {
                if self.energy_sphere_check(th, ah, at) {
                    self.delete_npc(th);
                }
            }
        }
    }

    /// `checkResistingPoisonSuccess`
    pub fn resist_poison(&mut self, h: usize, t: u8) -> bool {
        let r = if t == OWNERTYPE_PLAYER { self.client(h).map_or(0, |c| c.fx.add_pr) } else { 0 };
        self.rng.dice(1, 100) < r
    }

    /// `checkResistingIceSuccess` (the attacker's direction and hit ratio are unused in the C++ too)
    pub fn resist_ice(&mut self, h: usize, t: u8) -> bool {
        let r = if t == OWNERTYPE_PLAYER {
            let Some(c) = self.client(h) else { return false };
            if c.is_invincible() {
                return true;
            }
            let body = c.equip_status[EQUIPPOS_BODY as usize];
            // fix: the C++ compared the body slot's item index (not its id) with 770/771, so Santa costumes never protected
            if body >= 0 && c.items[body as usize].as_ref().is_some_and(|i| matches!(i.cfg.id_num, 770 | 771)) {
                return true;
            }
            if c.cs.warm_time != 0 && self.now().wrapping_sub(c.cs.warm_time) < 30_000 {
                return true; // a warm potion drunk less than 30 s ago
            }
            c.fx.add_abs[3] * 2
        } else {
            let Some(n) = self.npcs.get(h).and_then(|n| n.as_ref()) else { return false };
            if n.attribute == ATTRIBUTE_WATER {
                return true;
            }
            n.resist_magic - n.resist_magic / 3
        };
        self.rng.dice(1, 100) <= r.max(1)
    }

    /// `EnduStrippingDamage`: armour loses 1 endurance per hit taken (the strip chance is always 0 in the C++).
    fn endu_stripping(&mut self, th: usize, k: i16) {
        if k == -1 {
            return;
        }
        let t = self.client_mut(th).unwrap();
        let neutral = t.is_neutral;
        let Some(it) = t.items[k as usize].as_mut() else { return };
        if !neutral && it.cur_life_span > 0 && !it.cfg.name.starts_with(b"Legion") {
            it.cur_life_span -= 1;
        }
        if it.cur_life_span == 0 {
            let pos = it.cfg.equip_pos;
            self.notify(th, NOTIFY_ITEMLIFESPANEND, &[pos, k as i32], None);
            self.release_item(th, k as usize, true);
        }
    }

    /// `iClientMotion_Attack_Handler`: 1 = done, 2 = position mismatch, 0 = rejected.
    #[allow(clippy::too_many_arguments)]
    pub fn motion_attack(&mut self, h: usize, sx: i16, sy: i16, mut dx: i16, mut dy: i16, mut ty: i32, dir: u8, target: u16, response: bool, dash: bool) -> i32 {
        let now = self.now();
        let Some(c) = self.client_mut(h) else { return 0 };
        if dir == 0 || dir > 8 || !c.is_init_complete || c.is_killed {
            return 0;
        }
        c.last_action_time = now;
        c.cs.attack_msg_count += 1;
        if c.cs.attack_msg_count >= 7 {
            if c.cs.attack_lat != 0 && now.wrapping_sub(c.cs.attack_lat) < 80 * 8 * 7 - 3000 {
                self.delete_client(h, true, true, true, false);
                return 0;
            }
            let c = self.client_mut(h).unwrap();
            (c.cs.attack_lat, c.cs.attack_msg_count) = (now, 0);
        }
        let m = self.client(h).unwrap().map_index;
        let mut near = false;
        let mut tgt_dist = 1;
        if target != 0 && ty != 2 {
            let mut td = None;
            if (target as usize) < MAXCLIENTS as usize {
                td = self.client(target as usize).map(|t| (t.x, t.y));
            } else if target > 10000 && (target as usize) < 10000 + MAXNPCS {
                if let Some(n) = self.npcs[target as usize - 10000].as_ref() {
                    td = Some((n.x, n.y));
                    tgt_dist = if n.size < 2 { 1 } else { n.size as i16 };
                }
            }
            // fix: the C++ used the target's (0, 0) and m_pNpcList[sOwner] without checking it exists
            if let Some((tdx, tdy)) = td {
                let (o, ot) = self.maps[m].get_owner(dx, dy);
                if ot == OWNERTYPE_NPC && o as i32 == target as i32 - 10000 {
                    (dx, dy) = (tdx, tdy);
                } else if (tdx, tdy) != (dx, dy) && (tdx - dx).abs() <= 1 && (tdy - dy).abs() <= 1 {
                    (dx, dy, near) = (tdx, tdy, true);
                }
            }
        }
        if dx < 0 || dx >= self.maps[m].size_x || dy < 0 || dy >= self.maps[m].size_y {
            return 0;
        }
        let c = self.client(h).unwrap();
        if (sx, sy) != (c.x, c.y) {
            return 2;
        }
        self.inc_player_activity(h);
        let c = self.client(h).unwrap();
        let (ax, ay) = ((sx - dx).abs(), (sy - dy).abs());
        if ty != 2 && ty < 20 {
            let two = c.equip_status[EQUIPPOS_TWOHAND as usize];
            let reach = if two != -1 {
                let Some(it) = c.items[two as usize].as_ref() else { return 0 };
                if it.cfg.id_num == ITEM_STORMBRINGER { 3 + tgt_dist } else { tgt_dist }
            } else {
                tgt_dist
            };
            if ax > reach || ay > reach {
                ty = 0;
            }
        }
        self.pretend_corpse_reown(h, sx, sy, false);
        self.clear_skill_using_status(h);
        self.client_mut(h).unwrap().dir = dir;
        let mut exp = 0;
        let (o, ot) = self.maps[m].get_owner(dx, dy);
        if o != 0 {
            let c = self.client(h).unwrap();
            if ty != 0 && now.wrapping_sub(c.recent_attack_time) > 100 {
                let two = c.equip_status[EQUIPPOS_TWOHAND as usize];
                let bow = if c.is_in_building || two == -1 { 0 } else { c.items[two as usize].as_ref().map_or(0, |i| i.cfg.id_num) };
                let in_combat = c.appr[1] as u16 & 0xF000 != 0;
                if bow == ITEM_DIRECTION_BOW {
                    // the arrow also hits (3 in 4) whoever stands on the line to the target
                    for i in 1..(sx - dx).abs().max((sy - dy).abs()) {
                        if self.rng.dice(1, 4) <= 3 {
                            let (tx, tty) = crate::magic::get_point2(sx, sy, dx, dy, i as i32);
                            let (o2, ot2) = self.maps[m].get_owner(tx, tty);
                            if ot2 != 0 {
                                exp += self.attack_effect(o2 as usize, ot2, h, OWNERTYPE_PLAYER, tx, tty, ty, near, dash);
                            }
                        }
                    }
                    let (o2, ot2) = self.maps[m].get_owner(dx, dy);
                    if exp == 0 && ot2 == OWNERTYPE_NPC && self.rng.dice(1, 2) == 1 {
                        exp += self.attack_effect(o2 as usize, ot2, h, OWNERTYPE_PLAYER, dx, dy, ty, near, dash);
                    }
                    exp += self.attack_effect(o2 as usize, ot2, h, OWNERTYPE_PLAYER, dx, dy, ty, near, dash);
                } else if bow == ITEM_FIRE_BOW {
                    if in_combat {
                        // iGetAttribute is -1 (set) within 20 tiles of the map border
                        let mp = &self.maps[m];
                        let blocked = sx < 20 || sx >= mp.size_x - 20 || sy < 20 || sy >= mp.size_y - 20 || mp.attribute(sx, sy, 0x06) != 0;
                        if !blocked && ((dx - sx).abs() > 1 || (dy - sy).abs() > 1) {
                            let life = (self.rng.dice(1, 7) as u32 + 3) * 1000;
                            self.add_dynamic_object(h, 3, DYNAMICOBJECT_FIRE, m, dx, dy, life, 8); // OWNERTYPE_PLAYER_INDIRECT
                        }
                        exp += self.attack_effect(o as usize, ot, h, OWNERTYPE_PLAYER, dx, dy, ty, near, dash);
                    }
                } else {
                    exp += self.attack_effect(o as usize, ot, h, OWNERTYPE_PLAYER, dx, dy, ty, near, dash);
                }
                let Some(c) = self.client_mut(h) else { return 0 };
                c.recent_attack_time = now;
            }
        } else {
            self.check_mining_action(h, dx, dy);
        }
        if exp != 0 {
            self.get_exp(h, exp, true);
        }
        if response {
            let mut b = vec![0u8; 6];
            W(&mut b).u32(0, MSGID_RESPONSE_MOTION);
            W(&mut b).u16(4, OBJECTMOTION_ATTACK_CONFIRM as u16);
            self.send(h, &b);
        }
        1
    }

    /// The OBJECTATTACK / OBJECTATTACKMOVE arms of ClientMotionHandler.
    pub fn client_motion_attack(&mut self, h: usize, d: &[u8], cmd: i32) {
        let r = Rd::new(d);
        let (sx, sy, dir) = (r.u16(6) as i16, r.u16(8) as i16, r.u8(10));
        let (dx, dy, mut ty, target) = (r.u16(15) as i16, r.u16(17) as i16, r.u16(19) as i16 as i32, r.u16(21));
        if cmd == OBJECTATTACKMOVE {
            if self.motion_move(h, sx, sy, dir, false) == 1 && self.client(h).is_some() {
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTATTACKMOVE as u16, 0, 0, 0);
                let (x, y) = self.unit_pos(h, OWNERTYPE_PLAYER).unwrap();
                self.motion_attack(h, x, y, dx, dy, 1, dir, target, false, true);
            }
            if self.client(h).is_some_and(|c| c.hp <= 0) {
                self.killed_handler(h, 0, 0, 1);
            }
            return;
        }
        // _CheckAttackType
        let c = self.client(h).unwrap();
        let w = ((c.appr[1] as u16 & 0x0FF0) >> 4) as i32;
        match ty {
            2 | 25 => {
                if ty == 25 && c.super_attack_left <= 0 {
                    ty = 2;
                }
                if c.cs.arrow_index == -1 {
                    ty = 0;
                }
                if w < 40 {
                    ty = 1;
                }
            }
            20..=27 if c.super_attack_left <= 0 => ty = 1,
            _ => {}
        }
        match self.motion_attack(h, sx, sy, dx, dy, ty, dir, target, true, false) {
            1 => {
                if ty >= 20 {
                    let c = self.client_mut(h).unwrap();
                    c.super_attack_left = (c.super_attack_left - 1).max(0);
                }
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTATTACK as u16, dx, dy, ty as i16);
            }
            2 => self.send_motion_reject(h),
            _ => {}
        }
        // bCheckClientAttackFrequency only tracks the time (like the move check)
    }
}

impl Game {
    /// `ClientCommonHandler`: the combat toggles (the item/shop/guild commands come with their layers).
    pub fn client_common(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client_mut(h) else { return };
        if !c.is_init_complete || c.is_killed {
            return;
        }
        let r = Rd::new(d);
        match r.u16(4) as i32 {
            // layout: x 6, y 8, dir 10, v1 11, v2 15, v3 19, string 23 (30 bytes), v4 53
            COMMONTYPE_ITEMDROP => {
                let k = r.u32(11) as i16;
                if k >= 0 {
                    self.drop_item(h, k as usize, r.u32(15) as i32, &r.raw(23, 30));
                }
            }
            COMMONTYPE_TOGGLECOMBATMODE => {
                // fix: the C++ tested m_bSkillUsingStatus[19], past the 11-skill array (pretend corpse is 10)
                if c.skill_using[SKILL_PRETENDCORPSE as usize] {
                    return;
                }
                c.is_attack_mode_change = true;
                c.appr[1] = if c.appr[1] as u16 & 0xF000 == 0 { (c.appr[1] as u16 | 0xF000) as i16 } else { (c.appr[1] as u16 & 0x0FFF) as i16 };
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
            }
            COMMONTYPE_EQUIPITEM => {
                let k = r.u32(11) as i32;
                if (0..MAXITEMS as i32).contains(&k) {
                    crate::entry::equip_item(self, h, k as usize, true, true);
                }
            }
            COMMONTYPE_RELEASEITEM => {
                let k = r.u32(11) as i32;
                if (0..MAXITEMS as i32).contains(&k) {
                    self.release_item(h, k as usize, true);
                }
            }
            COMMONTYPE_REQ_USEITEM => self.use_item(h, r.u32(11) as i16 as i32, r.u32(15) as i16, r.u32(19) as i16, r.u32(53) as i16 as i32),
            // the crafting handlers read fixed offsets up to 37: short messages are padded with zeros
            COMMONTYPE_REQ_CREATEPOTION | COMMONTYPE_CRAFTITEM | COMMONTYPE_BUILDITEM => {
                let mut b = d.to_vec();
                b.resize(b.len().max(64), 0);
                match r.u16(4) as i32 {
                    COMMONTYPE_REQ_CREATEPOTION => self.create_potion(h, &b),
                    COMMONTYPE_CRAFTITEM => self.create_crafting(h, &b),
                    _ => self.build_item(h, &b),
                }
            }
            COMMONTYPE_EXCHANGEITEMTOCHAR => self.exchange_item_to_char(h, r.u8(10) as i8 as i32, r.u32(11) as i32, r.u32(15) as i16, r.u32(19) as i16, r.u32(53) as u16),
            COMMONTYPE_SETEXCHANGEITEM => self.set_exchange_item(h, r.u32(11) as i32, r.u32(15) as i32),
            COMMONTYPE_CONFIRMEXCHANGEITEM => self.confirm_exchange_item(h),
            COMMONTYPE_CANCELEXCHANGEITEM => self.cancel_exchange_item(h),
            COMMONTYPE_GIVEITEMTOCHAR => self.give_item(h, r.u8(10) as i8 as i32, r.u32(11) as i32, r.u32(15) as i16, r.u32(19) as i16, r.u32(53) as u16, &r.raw(23, 30)),
            COMMONTYPE_REQ_PURCHASEITEM => self.purchase_item(h, &r.raw(23, 30), r.u32(11) as i32),
            COMMONTYPE_REQ_SELLITEM => self.sell_item_offer(h, r.u32(11) as i8 as i32, r.u32(15) as i8 as i32, r.u32(19) as i32),
            COMMONTYPE_REQ_SELLITEMCONFIRM => self.sell_item_confirm(h, r.u32(11) as i8 as i32, r.u32(15) as i32),
            COMMONTYPE_REQ_REPAIRITEM => self.repair_item_offer(h, r.u32(11) as i8 as i32, r.u32(15) as i8 as i32),
            COMMONTYPE_REQ_REPAIRITEMCONFIRM => self.repair_item_confirm(h, r.u32(11) as i8 as i32),
            COMMONTYPE_REQ_STUDYMAGIC => self.study_magic(h, &r.raw(23, 30), true),
            COMMONTYPE_REQUEST_ACTIVATESPECABLTY => self.activate_special_ability(h),
            COMMONTYPE_REQ_USESKILL => self.use_skill(h, r.u32(11) as i32),
            COMMONTYPE_REQ_SETDOWNSKILLINDEX => self.set_down_skill_index(h, r.u32(11) as i32),
            COMMONTYPE_REQ_GETFISHTHISTIME => self.get_fish_this_time(h),
            COMMONTYPE_UPGRADEITEM => self.request_item_upgrade(h, r.u32(11) as i32),
            COMMONTYPE_REQ_CREATESLATE => {
                let b = d.to_vec();
                self.create_slate(h, &b);
            }
            COMMONTYPE_MAGIC => self.common_magic(h, r.u32(11) as i32, r.u32(15) as i32, r.u32(19) as i32),
            COMMONTYPE_REQUEST_ACCEPTJOINPARTY => self.accept_join_party(h, r.u32(11) as i32),
            COMMONTYPE_REQUEST_JOINPARTY => self.join_party_cmd(h, r.u32(11) as i32, &r.s(23, 30)),
            COMMONTYPE_TALKTONPC => self.npc_talk(h, r.u32(11) as i32),
            COMMONTYPE_QUESTACCEPTED => self.quest_accepted(h),
            COMMONTYPE_REQUEST_CANCELQUEST => self.cancel_quest(h),
            COMMONTYPE_REQGUILDNAME => self.request_guild_name(h, r.u32(11) as i32, r.u32(15) as i32),
            COMMONTYPE_JOINGUILDAPPROVE | COMMONTYPE_JOINGUILDREJECT | COMMONTYPE_DISMISSGUILDAPPROVE | COMMONTYPE_DISMISSGUILDREJECT => {
                self.guild_answer(h, r.u16(4) as i32, &r.s(23, 30))
            }
            COMMONTYPE_SETGUILDTELEPORTLOC | COMMONTYPE_SETGUILDCONSTRUCTLOC => {
                let (construct, guid) = (r.u16(4) as i32 == COMMONTYPE_SETGUILDCONSTRUCTLOC, c.guild_guid);
                self.set_guild_loc(h, construct, r.u32(11) as i32, r.u32(15) as i32, guid, &r.s(23, 30), true)
            }
            COMMONTYPE_GUILDTELEPORT => self.guild_teleport(h),
            COMMONTYPE_REQ_GETOCCUPYFLAG => self.get_occupy_flag(h),
            COMMONTYPE_REQ_GETOCCUPYFIGHTZONETICKET => self.get_fightzone_ticket(h),
            COMMONTYPE_REQ_GETREWARDMONEY => self.get_reward_money(h),
            COMMONTYPE_REQ_GETHEROMANTLE => self.get_hero_mantle(h, r.u32(11) as i32),
            // the C++ handler takes chars: the type, count and mode wrap at 256
            COMMONTYPE_SUMMONWARUNIT => self.summon_war_unit(h, r.u16(6) as i16, r.u16(8) as i16, r.u32(11) as i8 as i32, r.u32(15) as i8 as i32, r.u32(19) as i8 as i32),
            COMMONTYPE_REQUEST_HELP => self.request_help(h),
            COMMONTYPE_REQUEST_MAPSTATUS => self.map_status(h, r.u32(11) as i32, &r.s(23, 30)),
            COMMONTYPE_REQUEST_SELECTCRUSADEDUTY => self.select_crusade_duty(h, r.u32(11) as i32),
            COMMONTYPE_TOGGLESAFEATTACKMODE => {
                c.is_safe_attack_mode = !c.is_safe_attack_mode;
                self.notify(h, NOTIFY_SAFEATTACKMODE, &[], None);
            }
            t => crate::log::log(&format!("<{h}> not ported yet: common type {t:04x}")),
        }
    }
}

impl Game {
    /// `ActivateSpecialAbilityHandler` (GUILDWARMODE is not defined)
    pub fn activate_special_ability(&mut self, h: usize) {
        let now = self.now();
        let Some(c) = self.client_mut(h) else { return };
        let (sa, last) = (c.fx.sa_type, c.fx.sa_last_sec);
        if c.special_ability_time != 0 || sa == 0 || c.cs.sa_enabled {
            return;
        }
        c.cs.sa_enabled = true;
        c.cs.sa_start_time = now;
        c.special_ability_time = SPECABLTYTIMESEC;
        let bits: u16 = match sa {
            1..=5 => 0x0010,
            50..=54 => 0x0020,
            _ => 0,
        };
        c.appr[3] = ((c.appr[3] as u16 & 0xFF0F) | bits) as i16;
        self.notify(h, NOTIFY_SPECIALABILITYSTATUS, &[1, sa, last], None);
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
    }

    /// The SA part of the 3-second player timer: the activated ability runs out after its last-seconds.
    pub fn special_ability_timer(&mut self, h: usize, now: u32) {
        let Some(c) = self.client_mut(h) else { return };
        if !c.cs.sa_enabled || now.wrapping_sub(c.cs.sa_start_time) / 1000 <= c.fx.sa_last_sec as u32 {
            return;
        }
        c.cs.sa_enabled = false;
        c.special_ability_time = SPECABLTYTIMESEC;
        c.appr[3] = (c.appr[3] as u16 & 0xFF0F) as i16;
        self.notify(h, NOTIFY_SPECIALABILITYSTATUS, &[3], None);
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
    }
}

/// `_iGetArrowItemIndex`
pub fn arrow_index(c: &Client) -> i32 {
    c.items.iter().position(|i| i.as_ref().is_some_and(|i| i.cfg.item_type == ITEMTYPE_ARROW && i.count > 0)).map_or(-1, |k| k as i32)
}

/// The direction a hit pushes the target (away from the attacker); 0 when they share the tile.
pub fn damage_move_dir(a: (i16, i16), t: (i16, i16)) -> u8 {
    use std::cmp::Ordering::*;
    match (t.0.cmp(&a.0), t.1.cmp(&a.1)) {
        (Equal, Equal) => 0,
        (Equal, Greater) => 5,
        (Equal, Less) => 1,
        (Greater, Equal) => 3,
        (Greater, Greater) => 4,
        (Greater, Less) => 2,
        (Less, Equal) => 7,
        (Less, Greater) => 6,
        (Less, Less) => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas() {
        // hit chance: 80 * hr / dr clamped 15..99, halved defense from behind
        assert_eq!(hit_chance(100, 100, false), 80);
        assert_eq!(hit_chance(100, 100, true), 99);
        assert_eq!(hit_chance(10, 1000, false), 15);
        assert_eq!(hit_chance(50, 0, false), 99);
        assert_eq!(hit_chance(60, 100, false), 48);
        // strength: 10 dmg with 50 str -> 10 + 10 * 10% = 11
        assert_eq!(str_scaled(10, 50), 11);
        assert_eq!(str_scaled(7, 0), 7);
        assert_eq!(armor_absorb(25, 40), 10);
        assert_eq!(armor_absorb(95, 100), 80);
        assert_eq!(armor_absorb(0, 100), 0);
        assert_eq!((combo_bonus(8, 3), combo_bonus(9, 4), combo_bonus(5, 1), combo_bonus(6, 4)), (3, 8, 0, 0));
        assert_eq!((weapon_skill(0), weapon_skill(1), weapon_skill(7), weapon_skill(8), weapon_skill(45)), (5, 7, 9, 8, 6));
        assert_eq!((skill_ssn_point(0), skill_ssn_point(20), skill_ssn_point(70)), (1, 20, 700));
        assert_eq!((damage_move_dir((5, 5), (6, 6)), damage_move_dir((5, 5), (5, 4)), damage_move_dir((5, 5), (5, 5))), (4, 1, 0));
    }
}
