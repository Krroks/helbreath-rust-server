//! A player's life cycle: `CClient::KilledHandler`, `ApplyCombatKilledPenalty`, `DecPKCount`,
//! `RequestRestartHandler`, `GetExp`, `CalcExpStock`, `bCheckLevelUp`, `LevelUpSettingsHandler`,
//! `TimeHitPointsUp` / `TimeManaPointsUp` / `TimeStaminarPointsUp`, hunger and `PoisonEffect` (the per-player
//! timers of `CheckClientResponseTime`), `bAddItem`.
//! Not ported: the PK log lines (_bPKLog), CheckUniqueItemEquipment.
use crate::client::*;
use crate::combat::ITEMTYPE_ARROW;
use crate::consts::*;
use crate::game::Game;
use crate::map::{OWNERTYPE_NPC, OWNERTYPE_PLAYER};
use crate::util::{cstr, Rd, W};

pub const ITEMTYPE_CONSUME: i32 = 5;

/// `GetExp` without parties: exp / 100, then a bonus of 2.5% per level under 80 (+2.5%).
pub fn exp_gain(level: i32, exp: i32) -> i32 {
    let mut e = exp / 100;
    if level <= 80 {
        // the C++ constants are floats (0.025f, 1.025f): level 41 gives 1.99999999, not 2
        e = (((80 - level) as f64 * 0.025f32 as f64 + 1.025f32 as f64) * e as f64) as i32;
    }
    e
}

/// `TimeHitPointsUp`'s amount before the stocks: 3d(vit/3), at least vit/2, minus the max-HP-down side effect.
pub fn hp_regen(roll: i32, vit: i32, hp_down: i32) -> i32 {
    let mut t = roll.max(vit / 2);
    if hp_down != 0 {
        t -= t / hp_down;
    }
    t
}

/// The level bonus of `TimeStaminarPointsUp`.
pub fn sp_level_bonus(level: i32) -> i32 {
    match level {
        ..=20 => 15,
        21..=40 => 10,
        41..=60 => 5,
        _ => 0,
    }
}

/// The extra regen delay of a hungry player: (30 - hunger) seconds below 30.
pub fn hunger_delay(hunger: i32) -> u32 {
    if (0..=30).contains(&hunger) {
        (30 - hunger) as u32 * 1000
    } else {
        0
    }
}

fn pct(v: i32, p: i32) -> i32 {
    if p == 0 {
        v
    } else {
        v + (p as f64 / 100.0 * v as f64) as i32
    }
}

/// What a player kill does to the killer (KilledHandler's PvP switch).
#[derive(Debug, PartialEq)]
pub enum Pvp {
    EnemyKill,
    PkCaptured,
    PkPenalty,
}

/// KilledHandler: victim (neutral?, PK count, side), killer (neutral?, side). Kept as in the C++: killing a
/// PK-free traveller, or a PK-free citizen as a traveller, counts as an enemy kill.
pub fn pvp_outcome(vn: bool, vpk: i32, vside: i32, an: bool, aside: i32) -> Pvp {
    match (vn, an) {
        _ if (vn || an) && vpk == 0 => Pvp::EnemyKill,
        (true, _) => Pvp::PkCaptured,
        (false, true) => Pvp::PkPenalty,
        _ if aside != vside => Pvp::EnemyKill,
        _ if vpk == 0 => Pvp::PkPenalty,
        _ => Pvp::PkCaptured,
    }
}

/// `iGetExpLevel` over a level-exp table
pub fn exp_level_in(t: &[u32], exp: u32) -> i32 {
    (1..890.min(t.len().saturating_sub(1))).find(|&i| t[i] <= exp && t[i + 1] > exp).unwrap_or(0) as i32
}

/// `CClient::ApplyElo` (K = 16, in the C++'s float arithmetic): the new (killer, victim) ratings. The level factor is
/// 1 - log180(killer level - victim level) for a higher killer and log180(victim level - killer level) for a lower one.
pub fn elo_update(level: i32, elo: i32, foe_level: i32, foe_elo: i32) -> (i32, i32) {
    let logb = |n: i32| ((n as f32).ln() as f64 / 180f64.ln()) as f32;
    let lvl = match level.cmp(&foe_level) {
        std::cmp::Ordering::Greater => (1.0 - logb(level - foe_level) as f64) as f32,
        std::cmp::Ordering::Less => logb(foe_level - level),
        std::cmp::Ordering::Equal => 1.0,
    };
    let e = |a: i32, b: i32| 1.0f32 / (1.0 + 10f32.powf(((a - b) as f64 / 400.0) as f32));
    // fix: the C++ wrote `m_elo = lvlFactor * K * (1 - E)` for the killer (his rating became the gain, not
    // rating + gain) and `+=` for the victim
    let elo = (elo as f32 + lvl * 16.0 * (1.0 - e(foe_elo, elo))) as i32;
    let foe = (foe_elo as f32 + lvl * 16.0 * (0.0 - e(elo, foe_elo))) as i32;
    (elo, foe)
}

/// EnemyKillRewardHandler's exp: (3d(3L) + L) / 3
pub fn ek_reward_exp(r: &mut crate::rng::Rng, l: i32) -> i32 {
    (r.dice(3, 3 * l) + l) / 3
}

impl Game {
    /// `CClient::KilledHandler`. `ah`/`at` = the attacker (0, 0 for none).
    pub fn killed_handler(&mut self, h: usize, ah: usize, at: u8, damage: i32) {
        let Some(c) = self.client_mut(h) else { return };
        if !c.is_init_complete || c.is_killed {
            return;
        }
        // fight zone dead time: ResurrectPlayer is commented out in the C++
        (c.is_killed, c.hp) = (true, 0);
        self.cancel_exchange_if_open(h);
        self.remove_from_target(h, OWNERTYPE_PLAYER, 0);
        let name: Vec<u8> = match at {
            OWNERTYPE_PLAYER => self.client(ah).map(|a| cstr(&a.char_name).to_vec()).unwrap_or_default(),
            OWNERTYPE_NPC => self.npcs.get(ah).and_then(|n| n.as_ref()).map(|n| cstr(&n.npc_name).to_vec()).unwrap_or_default(),
            _ => Vec::new(),
        };
        self.notify(h, NOTIFY_KILLED, &[], Some(&name));
        let weapon = if at == OWNERTYPE_PLAYER { self.client(ah).map_or(1, |a| (a.appr[1] as i32 & 0x0FF0) >> 4) } else { 1 };
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTDYING as u16, damage as i16, weapon as i16, 0);
        let capture = self.ast.ev.as_ref().is_some_and(|e| e.ty == crate::astoria::ET_CAPTURE);
        // fix: the C++ tested `(itemInd = HasItem(ITEM_RELIC))`: ITEM_NONE (-1) counted as found and a relic in
        // slot 0 as missing
        if let Some(k) = self.client(h).unwrap().has_item(crate::astoria::ITEM_RELIC).filter(|_| capture) {
            let n = self.client(h).unwrap().items[k].as_ref().unwrap().cfg.name.clone();
            self.drop_item(h, k, 1, &n);
        }
        let c = self.client(h).unwrap();
        let (m, x, y) = (c.map_index, c.x, c.y);
        self.maps[m].clear_owner(h as i16, OWNERTYPE_PLAYER, x, y);
        self.maps[m].set_dead_owner(h as i16, OWNERTYPE_PLAYER, x, y);
        if self.maps[m].map_type == MAPTYPE_NOPENALTY_NOREWARD {
            return;
        }
        let pk = self.client(h).unwrap().pk_count;
        let level = match pk {
            0 => 1,
            1..=3 => 3,
            4..=11 => 6,
            _ => 12,
        };
        match at {
            OWNERTYPE_PLAYER => {
                let sa = self.client(ah).is_some_and(|a| (1..=5).contains(&a.fx.sa_type));
                if ah == h {
                    return;
                }
                let a = self.client(ah).map(|a| (a.is_neutral(), a.side));
                let c = self.client(h).unwrap();
                // the killer's reward or PK penalty
                if let Some((an, aside)) = a {
                    let (vn, vside, vm) = (c.is_neutral(), c.side, c.map_index);
                    if !vn && !an {
                        self.astoria_player_kill(aside, vside);
                    }
                    if !vn && !an && self.hd.mode && self.is_heldenian_map(vm) {
                        self.heldenian_player_kill(vside);
                    }
                    let c = self.client(h).unwrap();
                    match pvp_outcome(c.is_neutral(), pk, c.side, an, aside) {
                        Pvp::EnemyKill => self.enemy_kill_reward(ah, h),
                        Pvp::PkCaptured => self.pk_kill_reward(ah, h),
                        Pvp::PkPenalty => self.apply_pk_penalty(ah, h),
                    }
                }
                let c = self.client(h).unwrap();
                if pk == 0 {
                    if let Some((false, side)) = a {
                        if side != c.side {
                            self.combat_killed_penalty(h, 2, sa);
                        }
                    }
                } else {
                    self.combat_killed_penalty(h, level, sa);
                }
            }
            OWNERTYPE_NPC => {
                self.combat_killed_penalty(h, level, false);
                // a guild's war unit that kills an enemy gives its guild's builders level / 2 construction points
                let Some((guid, nside)) = self.npcs.get(ah).and_then(|n| n.as_ref()).map(|n| (n.guild_guid, n.side)) else { return };
                let c = self.client(h).unwrap();
                let pts = c.level / 2;
                if guid != 0 && nside != c.side {
                    for i in self.handles() {
                        let o = self.client_mut(i).unwrap();
                        if o.guild_guid == guid && o.crusade_duty == 3 {
                            o.construction_point = (o.construction_point + pts).min(MAXCONSTRUCTIONPOINT);
                            let (cp, wc) = (o.construction_point, o.war_contribution);
                            self.notify(i, NOTIFY_CONSTRUCTIONPOINT, &[cp, wc, 0], None);
                            return;
                        }
                    }
                    let mut d = vec![GSM_CONSTRUCTIONPOINT as u8];
                    d.extend_from_slice(&guid.to_le_bytes());
                    d.extend_from_slice(&pts.to_le_bytes());
                    self.stock_msg(&d);
                }
            }
            _ => {}
        }
    }

    /// `ApplyCombatKilledPenalty`: lose 1d(5 * penalty * level) exp (a third for travellers), then a citizen
    /// drops items (PenaltyItemDrop: penalty - 1 of them below level 80, at least 1). In a crusade only DecPKCount
    /// (the same in the apocalypse and heldenian maps and in the astoria event).
    pub fn combat_killed_penalty(&mut self, h: usize, penalty: i32, sa_attacked: bool) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        self.dec_pk_count(h);
        let m = self.client(h).unwrap().map_index;
        if self.cr.mode || (self.ast.ev.is_some() && self.is_astoria_map(m)) || (self.ap.mode && self.is_apocalypse_map(m)) || (self.hd.mode && self.is_heldenian_map(m)) {
            return;
        }
        let c = self.client(h).unwrap();
        let mut e = self.rng.dice(1, 5 * penalty * c.level);
        let c = self.client_mut(h).unwrap();
        if c.is_neutral {
            e /= 3;
        }
        c.exp = c.exp.saturating_sub(e.max(0) as u32);
        self.notify(h, NOTIFY_EXP, &[], None);
        let c = self.client(h).unwrap();
        if !c.is_neutral {
            let n = if c.level < 80 { (penalty - 1).max(1) } else { penalty };
            self.penalty_item_drop(h, n, sa_attacked);
        }
    }

    /// `iGetExpLevel`: the level an exp amount belongs to (0 below level 1 or past the table).
    pub fn exp_level(&self, exp: u32) -> i32 {
        exp_level_in(&self.level_exp_table, exp)
    }

    /// `bCheckLimitedUser`: travellers stop one exp point short of level 51 (NOTIFY_TRAVELERLIMITEDLEVEL).
    pub fn check_limited_user(&mut self, h: usize) -> bool {
        let cap = self.level_exp_table[51];
        let Some(c) = self.clients[h].as_mut() else { return false };
        if c.is_neutral() && c.exp >= cap && !c.is_gm() {
            c.exp = cap - 1;
            self.notify(h, NOTIFY_TRAVELERLIMITEDLEVEL, &[], None);
            return true;
        }
        false
    }

    /// `EnemyKillRewardHandler` (outside crusade; both guild-rank branches of the C++ are the same): a killer without
    /// PK gets (3d(3L) + L) / 3 exp, L = the victim's exp level, and from level 30 (80 for killers of level 80+)
    /// an EK (with the Elo update) and 1dL reward gold; the side's city win counter goes up.
    pub fn enemy_kill_reward(&mut self, ah: usize, h: usize) {
        let (Some(a), Some(v)) = (self.client(ah), self.client(h)) else { return };
        let ek_level = if a.level >= 80 { 80 } else { 30 };
        if a.pk_count != 0 {
            return;
        }
        let (l, vlevel) = (self.exp_level(v.exp), v.level);
        let reward = ek_reward_exp(&mut self.rng, l);
        let adjust = self.settings.enemy_kill_adjust;
        let crusade = self.cr.mode;
        let a = self.client_mut(ah).unwrap();
        a.exp = a.exp.wrapping_add(reward as u32);
        if crusade {
            a.war_contribution = (a.war_contribution + reward * 5).min(MAXWARCONTRIBUTION);
            a.construction_point = (a.construction_point + vlevel / 2).min(MAXCONSTRUCTIONPOINT);
            let (cp, wc) = (a.construction_point, a.war_contribution);
            self.notify(ah, NOTIFY_CONSTRUCTIONPOINT, &[cp, wc, 0], None);
        }
        if l >= ek_level {
            // fix (gameplay-visible): the C++ wrote `m_iEnemyKillCount + m_iEnemyKillAdjust;`, so the EK count never rose
            self.client_mut(ah).unwrap().enemy_kill_count += adjust;
            self.apply_elo(ah, h);
        }
        // fix: the port gave the reward gold only at EK level; the C++ gives 1d(victim level) on every enemy kill
        let g = self.rng.dice(1, l);
        let a = self.client_mut(ah).unwrap();
        a.reward_gold = (a.reward_gold + g).clamp(0, MAXREWARDGOLD);
        self.notify(ah, NOTIFY_ENEMYKILLREWARD, &[h as i32], None);
        if !self.check_limited_user(ah) {
            self.notify(ah, NOTIFY_EXP, &[], None);
        }
        self.check_level_up(ah);
        let side = self.client(ah).unwrap().side as usize;
        if let Some(w) = self.city_wins.get_mut(side) {
            *w += 1;
        }
    }

    /// `CClient::ApplyElo`: the killer `ah` beat `h`; both ratings move.
    pub fn apply_elo(&mut self, ah: usize, h: usize) {
        let (Some(a), Some(v)) = (self.client(ah), self.client(h)) else { return };
        let (ea, ev) = elo_update(a.level, a.elo, v.level, v.elo);
        self.client_mut(ah).unwrap().elo = ea;
        self.client_mut(h).unwrap().elo = ev;
    }

    /// `PK_KillRewardHandler`: killing a criminal gives a PK-free killer 3 * L reward gold (NOTIFY_PKCAPTURED).
    pub fn pk_kill_reward(&mut self, ah: usize, h: usize) {
        let (Some(a), Some(v)) = (self.client(ah), self.client(h)) else { return };
        if a.pk_count != 0 {
            return;
        }
        let (vpk, vlevel, vname) = (v.pk_count, v.level, cstr(&v.char_name).to_vec());
        let g = self.exp_level(v.exp) * 3;
        let a = self.client_mut(ah).unwrap();
        a.reward_gold = (a.reward_gold + g).clamp(0, MAXREWARDGOLD);
        self.notify(ah, NOTIFY_PKCAPTURED, &[vpk, vlevel], Some(&vname));
    }

    /// `CClient::ApplyPKPenalty` on the killer `h` of `v`: loses (v.L/2+1)d50 + (L/2+1)d50 exp, one more PK,
    /// 5 reputation, and a citizen who did it in his own town goes to its jail for 3 minutes.
    pub fn apply_pk_penalty(&mut self, h: usize, v: usize) {
        let (Some(c), Some(vc)) = (self.client(h), self.client(v)) else { return };
        if c.is_safe_attack_mode && c.pk_count == 0 {
            return;
        }
        let (vl, l) = (vc.level, c.level);
        let v1 = self.rng.dice(vl / 2 + 1, 50);
        let v2 = self.rng.dice(l / 2 + 1, 50);
        let c = self.client_mut(h).unwrap();
        c.exp = c.exp.saturating_sub(v1.max(0) as u32).saturating_sub(v2.max(0) as u32);
        self.inc_pk_count(h);
        let c = self.client_mut(h).unwrap();
        c.reputation -= 5;
        let (side, m) = (c.side, c.map_index);
        if let Some(n) = self.city_crimes.get_mut(side as usize) {
            *n += 1;
        }
        if side == NEUTRAL {
            return;
        }
        let home = SIDE_MAP.get(side as usize).copied().unwrap_or(b"");
        if cstr(&self.maps[m].location_name) != home {
            return;
        }
        let jail = SIDE_MAP_JAIL.get(side as usize).copied().unwrap_or(b"default").to_vec();
        let c = self.client_mut(h).unwrap();
        (c.locked_map_name, c.locked_map_time) = (jail.clone(), 60 * 3);
        self.request_teleport(h, 2, Some(&jail), -1, -1);
    }

    /// `CClient::IncPKCount`
    pub fn inc_pk_count(&mut self, h: usize) {
        let c = self.client_mut(h).unwrap();
        c.pk_count += 1;
        c.set_status_flag(STATUS_PK, true);
        self.notify(h, NOTIFY_PKPENALTY, &[], None);
        self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
    }

    /// `CClient::DecPKCount`
    pub fn dec_pk_count(&mut self, h: usize) {
        let c = self.client_mut(h).unwrap();
        if c.pk_count > 0 {
            c.pk_count -= 1;
            let zero = c.pk_count == 0;
            if zero {
                c.set_status_flag(STATUS_PK, false);
            }
            self.notify(h, NOTIFY_PKPENALTY, &[], None);
            if zero {
                self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
            }
        }
    }

    /// `RequestResurrectPlayer`: the dead player's answer to NOTIFY_RESURRECTPLAYER. Yes: alive with half the
    /// maximum HP, re-entered on the same spot (RequestTeleportHandler type 2). A yes nobody asked for is a hack:
    /// the player is disconnected.
    pub fn request_resurrect_player(&mut self, h: usize, yes: bool) {
        let Some(c) = self.client_mut(h) else { return };
        if !yes {
            c.is_being_resurrected = false;
            return;
        }
        if !c.is_being_resurrected {
            crate::log::log(&format!("(!!!) {}({}) Tried To Use Resurrection Hack", String::from_utf8_lossy(cstr(&c.char_name)), String::from_utf8_lossy(cstr(&c.ip_address))));
            self.delete_client(h, true, true, true, true);
            return;
        }
        c.is_being_resurrected = false;
        // fix: the C++ also "resurrected" a player who had restarted meanwhile (HP set to half the maximum, teleport)
        if !c.is_killed {
            return;
        }
        c.is_killed = false;
        c.hp = c.get_max_hp() / 2;
        let (map, x, y) = (c.map_name.clone(), c.x, c.y);
        self.request_teleport(h, 2, Some(&map), x, y);
    }

    /// `RequestRestartHandler`: back to life at the side's farm (resurrection map above level 80) or the
    /// jail when killed in the enemy town.
    pub fn request_restart(&mut self, h: usize) {
        let Some(c) = self.client_mut(h) else { return };
        if !c.is_killed {
            return;
        }
        let tmp = cstr(&c.map_name).to_vec();
        c.magic_effect_status[MAGICTYPE_CONFUSE as usize] = 0;
        let side = c.side;
        let dest: &[u8] = if side == NEUTRAL {
            SIDE_MAP[NEUTRAL as usize]
        } else if let Some(&e) = [ARESDEN, ELVINE, ISTRIA].iter().find(|&&e| e != side && tmp == SIDE_MAP.get(e as usize).copied().unwrap_or(b"")) {
            let jail = SIDE_MAP_JAIL.get(e as usize).copied().unwrap_or(b"default");
            c.locked_map_name = jail.to_vec();
            c.locked_map_time = 60 * 3;
            jail
        } else if c.level > 80 {
            SIDE_MAP_RES[side as usize % 4]
        } else {
            SIDE_MAP_FARM[side as usize % 4]
        };
        c.map_name = dest.to_vec();
        c.is_killed = false;
        c.hp = c.get_max_hp();
        c.hunger_status = 100;
        self.request_teleport(h, 2, Some(dest), -1, -1);
    }

    /// `GetExp` (no party share): into the exp stock, a third at the max level.
    pub fn get_exp(&mut self, h: usize, exp: i32, attacker_own: bool) {
        let max_level = self.settings.max_player_level;
        let Some(c) = self.client(h) else { return };
        if exp <= 0 {
            return;
        }
        let mut e = exp_gain(c.level, exp);
        if c.level >= 80 && (self.maps[c.map_index].name.starts_with(b"arefarm") || self.maps[c.map_index].name.starts_with(b"elvfarm")) {
            e /= 2;
        }
        if self.party_get_exp(h, e, attacker_own) {
            return;
        }
        let c = self.client_mut(h).unwrap();
        if c.status & STATUS_GREENSLATE != 0 {
            e *= 3;
        }
        if c.level == max_level {
            e /= 3;
        }
        c.cs.exp_stock += e;
    }

    /// `CalcExpStock`
    pub fn calc_exp_stock(&mut self, h: usize) {
        let Some(c) = self.clients[h].as_mut() else { return };
        if !c.is_init_complete || c.cs.exp_stock <= 0 {
            return;
        }
        if self.maps[c.map_index].map_type == MAPTYPE_NOPENALTY_NOREWARD {
            c.cs.exp_stock = 0;
            return;
        }
        c.exp = c.exp.wrapping_add(c.cs.exp_stock as u32);
        c.cs.auto_exp_amount += c.cs.exp_stock;
        c.cs.exp_stock = 0;
        if !self.check_limited_user(h) {
            self.notify(h, NOTIFY_EXP, &[], None);
        }
        let up = self.check_level_up(h);
        let level = self.client(h).unwrap().level;
        if up && level <= 20 {
            self.add_gold(h, if level <= 5 { 100 } else { 3000 });
        }
    }

    fn add_gold(&mut self, h: usize, n: u32) {
        let Some(cfg) = self.data.item_by_name(b"Gold").cloned() else { return };
        let mut it = Item::new(&cfg);
        it.count = n;
        self.add_item(h, it);
    }

    /// `bCheckLevelUp`
    pub fn check_level_up(&mut self, h: usize) -> bool {
        let (max, limit) = (self.settings.max_player_level, self.settings.char_stat_limit);
        let c = self.client(h).unwrap();
        if c.level >= max {
            self.max_level_exp(h);
            return false;
        }
        if c.exp < c.next_level_exp {
            return false;
        }
        for _ in 0..=100 {
            let c = self.client(h).unwrap();
            if c.exp < c.next_level_exp {
                return true;
            }
            if c.level >= max {
                self.max_level_exp(h);
                return false;
            }
            let next = self.level_exp_table.get((c.level + 2).clamp(0, 899) as usize).copied().unwrap_or(u32::MAX);
            let c = self.client_mut(h).unwrap();
            c.level += 1;
            c.lu_pool += 3;
            let lv = c.level;
            for s in [&mut c.str_, &mut c.dex, &mut c.vit, &mut c.int_, &mut c.mag, &mut c.range] {
                *s = (*s).min(limit);
            }
            if (1..=50).contains(&lv) {
                self.add_gold(h, 500);
            }
            self.notify(h, NOTIFY_LEVELUP, &[], None);
            let c = self.client_mut(h).unwrap();
            (c.hp, c.mp, c.sp) = (c.get_max_hp(), c.get_max_mp(), c.get_max_sp());
            c.super_attack_left = c.level / 10;
            c.next_level_exp = next;
            self.calc_total_item_effect(h, -1, false);
        }
        false
    }

    /// The max-level branch of `bCheckLevelUp`: exp above the cap becomes Gizon upgrade points.
    fn max_level_exp(&mut self, h: usize) {
        let max = self.settings.max_player_level as usize;
        let (a, b) = (self.level_exp_table[(max + 1).min(899)], self.level_exp_table[max.min(899)]);
        while self.client(h).unwrap().exp >= a && a > b {
            let c = self.client_mut(h).unwrap();
            c.exp -= a - b;
            c.gizon_item_upgrade_left += 1;
            let g = c.gizon_item_upgrade_left;
            self.notify(h, NOTIFY_EXP, &[], None);
            self.notify(h, NOTIFY_GIZONITEMUPGRADELEFT, &[g, 1], None);
        }
    }

    /// `LevelUpSettingsHandler`: str, vit, dex, int, mag, chr bytes after the header.
    pub fn level_up_settings(&mut self, h: usize, d: &[u8]) {
        let limit = self.settings.char_stat_limit;
        let Some(c) = self.client_mut(h) else { return };
        if !c.is_init_complete {
            return;
        }
        if c.lu_pool <= 0 || d.len() < 12 {
            self.notify(h, NOTIFY_SETTING_FAILED, &[], None);
            return;
        }
        let r = Rd::new(d);
        let v: Vec<i32> = (0..6).map(|k| r.u8(6 + k) as i8 as i32).collect();
        let sum: i32 = v.iter().sum();
        if sum > c.lu_pool {
            self.notify(h, NOTIFY_SETTING_FAILED, &[], None);
            return;
        }
        let cur = [c.str_, c.vit, c.dex, c.int_, c.mag, c.range];
        if cur.iter().zip(&v).any(|(s, a)| s + a > limit || *a < 0) {
            return;
        }
        let total: i32 = cur.iter().sum();
        let cap = (c.level - 1) * 3 + 70;
        if total + c.lu_pool - 3 > cap {
            c.lu_pool = (3 + cap - total).max(3);
            self.notify(h, NOTIFY_SETTING_FAILED, &[], None);
            return;
        }
        if total + sum > cap {
            self.notify(h, NOTIFY_SETTING_FAILED, &[], None);
            return;
        }
        c.lu_pool -= sum;
        (c.str_, c.vit, c.dex, c.int_, c.mag, c.range) = (cur[0] + v[0], cur[1] + v[1], cur[2] + v[2], cur[3] + v[3], cur[4] + v[4], cur[5] + v[5]);
        // SetStr/SetMag/SetInt(check): clamp the bars to the new maxima
        let (mut hpn, mut spn, mut mpn) = (false, false, false);
        if c.hp > c.get_max_hp() {
            (c.hp, hpn) = (c.get_max_hp(), true);
        }
        if c.sp > c.get_max_sp() {
            (c.sp, spn) = (c.get_max_sp(), true);
        }
        if c.mp > c.get_max_mp() {
            (c.mp, mpn) = (c.get_max_mp(), true);
        }
        for (f, t) in [(hpn, NOTIFY_HP), (spn, NOTIFY_SP), (mpn, NOTIFY_MP)] {
            if f {
                self.notify(h, t, &[], None);
            }
        }
        self.calc_total_item_effect(h, -1, false);
        self.notify(h, NOTIFY_SETTING_SUCCESS, &[], None);
    }

    /// The regen/hunger/poison/exp part of `CheckClientResponseTime` for one initialised player.
    pub fn player_life_timers(&mut self, h: usize) {
        let now = self.now();
        let c = self.client_mut(h).unwrap();
        if now.wrapping_sub(c.hunger_time) > HUNGERTIME as u32 && !c.is_killed {
            if c.level >= LEVELLIMIT && c.admin_user_level < 1 {
                c.hunger_status -= 1;
            }
            c.hunger_status = c.hunger_status.max(0);
            c.hunger_time = now;
            if c.hp > 0 && c.hunger_status < 30 {
                let hs = c.hunger_status;
                self.notify(h, NOTIFY_HUNGER, &[hs], None);
            }
        }
        let c = self.client_mut(h).unwrap();
        let plus = hunger_delay(c.hunger_status);
        if now.wrapping_sub(c.hp_time) > HPUPTIME as u32 + plus {
            c.hp_time = now;
            self.time_hp_up(h);
        }
        let c = self.client_mut(h).unwrap();
        if now.wrapping_sub(c.mp_time) > MPUPTIME as u32 + plus {
            c.mp_time = now;
            self.time_mp_up(h);
        }
        let c = self.client_mut(h).unwrap();
        if now.wrapping_sub(c.sp_time) > SPUPTIME as u32 + plus {
            c.sp_time = now;
            self.time_sp_up(h);
        }
        let c = self.client_mut(h).unwrap();
        if c.cs.is_poisoned && now.wrapping_sub(c.cs.poison_time) > POISONTIME as u32 {
            c.cs.poison_time = now;
            self.poison_effect(h);
        }
        let c = self.client_mut(h).unwrap();
        if now.wrapping_sub(c.exp_stock_time) > EXPSTOCKTIME as u32 {
            c.exp_stock_time = now;
            self.calc_exp_stock(h);
            // CheckUniqueItemEquipment: not ported
            self.check_crusade_result(h);
            self.check_heldenian_result(h);
        }
        let c = self.client(h).unwrap();
        if now.wrapping_sub(c.auto_exp_time) > AUTOEXPTIME as u32 {
            let v = c.level.max(1);
            let next = self.level_exp_table.get((c.level + 1).clamp(0, 899) as usize).copied().unwrap_or(0);
            if c.cs.auto_exp_amount < v && (c.exp as u64 + v as u64) < next as u64 {
                self.get_exp(h, v, false);
                self.calc_exp_stock(h);
            }
            let c = self.client_mut(h).unwrap();
            c.cs.auto_exp_amount = 0;
            c.auto_exp_time = now;
        }
    }

    /// `TimeHitPointsUp`
    fn time_hp_up(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        // fix: the C++ tested m_bSkillUsingStatus[19] (past the 11-skill array); pretend corpse is skill 10
        if !c.is_init_complete || c.hunger_status <= 0 || c.is_killed || c.skill_using[crate::combat::SKILL_PRETENDCORPSE as usize] {
            return;
        }
        if c.hp < c.get_max_hp() {
            let (vit, down, stock, stat, add) = (c.vit, c.fx.side_effect_max_hp_down, c.cs.hp_stock, c.fx.hp_static_stock, c.fx.add_hp);
            let roll = self.rng.dice(3, vit / 3);
            let total = pct(hp_regen(roll, vit, down) + stock + stat, add);
            let c = self.client_mut(h).unwrap();
            c.hp = (c.hp + total).min(c.get_max_hp());
            self.notify(h, NOTIFY_HP, &[], None);
        }
        self.client_mut(h).unwrap().cs.hp_stock = 0;
    }

    /// `TimeManaPointsUp`
    fn time_mp_up(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        if c.is_killed || !c.is_init_complete || c.hunger_status <= 0 || c.skill_using[crate::combat::SKILL_PRETENDCORPSE as usize] {
            return;
        }
        if c.mp < c.get_max_mp() {
            let (mag, add) = (c.get_mag(), c.fx.add_mp);
            let t = pct(self.rng.dice(3, mag / 3), add);
            let c = self.client_mut(h).unwrap();
            c.mp = (c.mp + t).min(c.get_max_mp());
            self.notify(h, NOTIFY_MP, &[], None);
        }
    }

    /// `TimeStaminarPointsUp`
    fn time_sp_up(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        if c.is_killed || !c.is_init_complete || c.hunger_status <= 0 || c.skill_using[crate::combat::SKILL_PRETENDCORPSE as usize] {
            return;
        }
        if c.sp < c.get_max_sp() {
            let (vit, add, level) = (c.vit, c.fx.add_sp, c.level);
            let t = pct(10 + self.rng.dice(4, vit / 17), add) + sp_level_bonus(level);
            let c = self.client_mut(h).unwrap();
            c.sp = (c.sp + t).min(c.get_max_sp());
            self.notify(h, NOTIFY_SP, &[], None);
        }
    }

    /// `PoisonEffect`
    fn poison_effect(&mut self, h: usize) {
        let c = self.client(h).unwrap();
        if c.is_killed || !c.is_init_complete {
            return;
        }
        let d = self.rng.dice(1, c.cs.poison_level);
        let c = self.client_mut(h).unwrap();
        let prev = c.hp;
        c.hp = (c.hp - d).max(1);
        if prev != c.hp {
            self.notify(h, NOTIFY_HP, &[], None);
        }
        let p = (10 + self.client(h).unwrap().fx.add_pr).max(10);
        if self.rng.dice(1, 100) <= p {
            let c = self.client_mut(h).unwrap();
            c.cs.is_poisoned = false;
            c.set_status_flag(STATUS_POISON, false);
            self.notify(h, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_POISON], None);
        }
    }

    /// `bAddItem` + `_bAddClientItemList`: stacks consumables and arrows, else the first free slot; when the
    /// player cannot carry it the item falls on their tile.
    pub fn add_item(&mut self, h: usize, it: Item) -> bool {
        if self.client(h).is_none() {
            return false;
        }
        if self.add_client_item_list(h, &it) {
            self.send_item_obtained(h, &it);
        } else {
            let c = self.client(h).unwrap();
            let (m, x, y) = (c.map_index, c.x, c.y);
            self.drop_ground_item(m, x, y, it);
            self.send_cannot_carry(h);
        }
        true
    }

    /// `_bAddClientItemList`: false when too heavy or the bag is full.
    pub fn add_client_item_list(&mut self, h: usize, it: &Item) -> bool {
        let Some(c) = self.client_mut(h) else { return false };
        let stack = it.cfg.item_type == ITEMTYPE_CONSUME || it.cfg.item_type == ITEMTYPE_ARROW;
        let w = it.weight_of(if stack { it.count as i64 } else { 1 });
        let max_load = c.get_str() * 500 + c.level * 500;
        if c.cur_weight_load + w > max_load {
            return false;
        }
        if let Some(k) = stack.then(|| c.items.iter().position(|i| i.as_ref().is_some_and(|i| cstr(&i.cfg.name) == cstr(&it.cfg.name)))).flatten() {
            c.items[k].as_mut().unwrap().count += it.count;
        } else if let Some(k) = c.items.iter().position(|i| i.is_none()) {
            c.items[k] = Some(it.clone());
            c.item_pos[k] = (40, 30);
            if it.cfg.item_type == ITEMTYPE_ARROW {
                c.cs.arrow_index = crate::combat::arrow_index(c);
            }
        } else {
            return false;
        }
        self.calc_total_weight(h);
        true
    }

    pub fn send_cannot_carry(&mut self, h: usize) {
        let mut b = vec![0u8; 6];
        W(&mut b).u32(0, MSGID_NOTIFY);
        W(&mut b).u16(4, NOTIFY_CANNOTCARRYMOREITEM as u16);
        self.send(h, &b);
    }

    /// SendItemNotifyMsg(NOTIFY_ITEMOBTAINED): 1, WriteItemData, spec value 2, attribute (53 bytes).
    pub fn send_item_obtained(&mut self, h: usize, it: &Item) {
        let mut b = vec![0u8; 53];
        let mut w = W(&mut b);
        w.u32(0, MSGID_NOTIFY);
        w.u16(4, NOTIFY_ITEMOBTAINED as u16);
        w.u8(6, 1);
        crate::entry::item_entry(&mut w, 7, it, Some(false));
        // fix: the C++ sent 2 uninitialised stack bytes after the attribute (zero here)
        self.send(h, &b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn life_formulas() {
        // level 1: (79 * 0.025 + 1.025) = 3.0 -> 300000 exp / 100 * 3
        assert_eq!(exp_gain(1, 300_000), 9000);
        assert_eq!(exp_gain(80, 1000), 10);
        assert_eq!(exp_gain(100, 1000), 10);
        assert_eq!(exp_gain(1, 99), 0);
        assert_eq!(hp_regen(3, 20, 0), 10);
        assert_eq!(hp_regen(15, 20, 0), 15);
        assert_eq!(hp_regen(16, 20, 4), 12);
        assert_eq!((sp_level_bonus(1), sp_level_bonus(40), sp_level_bonus(60), sp_level_bonus(61)), (15, 10, 5, 0));
        assert_eq!((hunger_delay(100), hunger_delay(30), hunger_delay(0)), (0, 0, 30_000));
    }

    #[test]
    fn elo() {
        // equal level and rating: the killer wins 8, the victim loses 7 (his E uses the killer's new rating)
        assert_eq!(elo_update(50, 0, 50, 0), (8, -7));
        // level 60 over 40: 1 - log180(20) = 0.423
        assert_eq!(elo_update(60, 0, 40, 0), (3, -3));
        // a victim 400 points above: the killer gains 14, the victim loses 14.4
        assert_eq!(elo_update(50, 0, 50, 400), (14, 385));
        // one level of difference below the killer's gives log180(1) = 0: nothing moves
        assert_eq!(elo_update(40, 100, 41, 200), (100, 200));
    }
}

#[cfg(test)]
mod pvp_tests {
    use super::*;

    #[test]
    fn outcomes() {
        // enemy towns
        assert_eq!(pvp_outcome(false, 0, ARESDEN, false, ELVINE), Pvp::EnemyKill);
        assert_eq!(pvp_outcome(false, 2, ARESDEN, false, ELVINE), Pvp::EnemyKill);
        // same town: innocent -> PK, criminal -> captured
        assert_eq!(pvp_outcome(false, 0, ARESDEN, false, ARESDEN), Pvp::PkPenalty);
        assert_eq!(pvp_outcome(false, 1, ARESDEN, false, ARESDEN), Pvp::PkCaptured);
        // travellers
        assert_eq!(pvp_outcome(true, 0, NEUTRAL, false, ARESDEN), Pvp::EnemyKill);
        assert_eq!(pvp_outcome(true, 3, NEUTRAL, false, ARESDEN), Pvp::PkCaptured);
        assert_eq!(pvp_outcome(false, 0, ARESDEN, true, NEUTRAL), Pvp::EnemyKill);
        assert_eq!(pvp_outcome(false, 1, ARESDEN, true, NEUTRAL), Pvp::PkPenalty);
    }

    #[test]
    fn exp_level_and_reward() {
        let t: Vec<u32> = (0..900).map(crate::game::level_exp).collect();
        assert_eq!(exp_level_in(&t, 0), 0);
        assert_eq!(exp_level_in(&t, t[30]), 30);
        assert_eq!(exp_level_in(&t, t[31] - 1), 30);
        let mut r = crate::rng::Rng::new(5);
        for _ in 0..1000 {
            let e = ek_reward_exp(&mut r, 30);
            assert!((31 / 3..=(270 + 30) / 3).contains(&e)); // 3d90 + 30, over 3
        }
    }
}
