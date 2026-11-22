//! Player magic (HG.cpp ~12367-13816): `iClientMotion_Magic_Handler` (the precast motion), ClientCommonHandler's
//! COMMONTYPE_MAGIC, `PlayerMagicHandler`, `CheckResistingMagicSuccess` (char/combat.cpp), `Effect_Damage_Spot` /
//! `Effect_Damage_Spot_DamageMove` / `Effect_HpUp_Spot` / `Effect_SpDown_Spot`, `CMagic::Effect_RecSP`,
//! `ArmorLifeDecrement`, `iGetWhetherMagicBonusEffect`, `getPlayerNum`, `ShowClientMsg`, `CMisc::GetPoint2`.
//! Not ported: the dynamic objects of type 14 (fire wall/field, poison cloud, spike field, ice storm: only their
//! bAnalyzeCriminalAction calls are), resurrection (32: wands), and the types no spell in Magic.cfg uses
//! (polymorph 20, 30 linear+sp-down, 33 area-move). Crusade damage bonuses come with crusades.
//! HG_MAGIC_DEBUG=1 prints the owner's "(MAGIA)" trace of the C++.
use crate::client::*;
use crate::combat::{MAGICPROTECT_AMP, MAGICPROTECT_PFM};
use crate::consts::*;
use crate::delay::{magic_flag, MAGICTYPE_ICE_LINEAR};
use crate::game::Game;
use crate::map::{OWNERTYPE_NPC, OWNERTYPE_PLAYER};
use crate::npc::{BEHAVIOR_ATTACK, MOVETYPE_FOLLOW, MOVETYPE_RANDOM};
use crate::util::{atoi, W};

pub const MAGICTYPE_DAMAGE_SPOT: i32 = 1;
pub const MAGICTYPE_HPUP_SPOT: i32 = 2;
pub const MAGICTYPE_DAMAGE_AREA: i32 = 3;
pub const MAGICTYPE_SPDOWN_AREA: i32 = 5;
pub const MAGICTYPE_SPUP_AREA: i32 = 7;
pub const MAGICTYPE_TELEPORT: i32 = 8;
pub const MAGICTYPE_SUMMON: i32 = 9;
pub const MAGICTYPE_CREATE: i32 = 10;
pub const MAGICTYPE_CREATE_DYNAMIC: i32 = 14;
pub const MAGICTYPE_POSSESSION: i32 = 15;
pub const MAGICTYPE_DAMAGE_LINEAR: i32 = 19;
pub const MAGICTYPE_POLYMORPH: i32 = 20;
pub const MAGICTYPE_DAMAGE_AREA_NOSPOT: i32 = 21;
pub const MAGICTYPE_TREMOR: i32 = 22;
pub const MAGICTYPE_DAMAGE_SPOT_SPDOWN: i32 = 24;
pub const MAGICTYPE_DAMAGE_AREA_NOSPOT_SPDOWN: i32 = 25;
pub const MAGICTYPE_DAMAGE_AREA_ARMOR_BREAK: i32 = 26;
pub const MAGICTYPE_CANCELLATION: i32 = 28;
pub const MAGICTYPE_INHIBITION: i32 = 29;
pub const MAGICTYPE_SCAN: i32 = 31;
pub const MAGICTYPE_RESURRECTION: i32 = 32;
const ITEMSTAT_CASTPROB: i32 = 10;
const ITEM_BAGUETTE: i32 = 98;
const ITEM_MEAT: i32 = 99;
const CHAT_GM: u8 = 10;
const DYNAMICOBJECT_ICESTORM: i32 = 8;

/// `CMagic` (Magic.cfg: index, name, type, delay, last time, mana, h range, v range, values 0..7, int, cost,
/// category, attribute). The ranges are halved at load like the C++.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spell {
    pub ty: i32,
    pub delay: i32,
    pub last_time: u32,
    pub mana: i32,
    pub h: i16,
    pub v: i16,
    pub val: [i32; 8],
    /// gold cost: negative = not for sale (the GM-Kill spells)
    pub cost: i32,
    pub category: i32,
    pub attr: i32,
}

impl Spell {
    pub fn parse(f: &[Vec<u8>]) -> Option<Spell> {
        if f.len() < 20 {
            return None;
        }
        let n = |i: usize| atoi(&f[i]);
        Some(Spell {
            ty: n(2),
            delay: n(3),
            last_time: n(4).max(0) as u32,
            mana: n(5),
            h: (n(6) / 2) as i16,
            v: (n(7) / 2) as i16,
            val: std::array::from_fn(|k| n(8 + k)),
            cost: n(17),
            category: n(18),
            attr: n(19),
        })
    }
    fn main(&self, wb: i32) -> (i32, i32, i32) {
        (self.val[0], self.val[1], self.val[2] + wb)
    }
    fn linear(&self, wb: i32) -> (i32, i32, i32) {
        (self.val[3], self.val[4], self.val[5] + wb)
    }
}

const MC_PROB: [i32; 11] = [0, 300, 250, 200, 150, 100, 80, 70, 60, 50, 40];
const ML_PENALTY: [i32; 11] = [0, 5, 5, 8, 8, 10, 14, 28, 32, 36, 40];

/// The spell circle against the caster's level/10, applied twice by PlayerMagicHandler.
pub fn level_adjust(circle: i32, level: i32) -> i32 {
    let lm = level / 10;
    if circle > lm {
        let d = circle - lm;
        let p = d * ML_PENALTY[circle as usize];
        let v4 = ((level - lm * 10) as f64 / (d * 10) as f64) * p as f64;
        -(p - v4 as i32).abs()
    } else {
        5 * (lm - circle)
    }
}

/// The cast probability (in %, the dice(1, 100) roll must not exceed it).
pub fn cast_probability(stype: i32, int: i32, level: i32, weather: i32, castprob: Option<i32>) -> i32 {
    let circle = stype / 10 + 1;
    let mut r = MC_PROB[circle as usize];
    if int > 50 {
        r += (int - 50) / 2;
    }
    r += level_adjust(circle, level);
    r -= match weather {
        1 => r / 24,
        2 => r / 12,
        3 => r / 5,
        _ => 0,
    };
    if let Some(v) = castprob {
        r += v * 3;
    }
    r.max(1)
}

/// The hit ratio against the target's magic resistance (after the mana check).
pub fn magic_hit_ratio(r: i32, mag: i32, level: i32, stype: i32, add_ar: i32) -> i32 {
    let mut r = r;
    if mag > 50 {
        r += mag - 50;
    }
    r += level_adjust(stype / 10 + 1, level) + add_ar;
    r = r.max(1);
    if stype >= 80 {
        r += 10000;
    }
    r
}

/// Mana cost: +10 % in safe-attack mode outside fight zones, minus the mana-save ratio (at least 1).
pub fn mana_cost(base: i32, safe_mode_penalty: bool, save_ratio: i32) -> i32 {
    let mut c = base;
    if safe_mode_penalty {
        c = (c as f64 * 1.1) as i32;
    }
    if save_ratio > 0 {
        c = (c as f64 - save_ratio as f64 / 100.0 * c as f64) as i32;
        c = c.max(1);
    }
    c
}

/// CheckResistingMagicSuccess's destination ratio: below it the dice(1, 100) roll lands (not resisted).
pub fn magic_dest_ratio(hit: i32, resist: i32) -> i32 {
    let hit = if hit >= 10000 { hit - 10000 } else { hit };
    ((hit as f64 / resist.max(1) as f64 * 50.0) as i32).clamp(MINIMUMHITRATIO, MAXIMUMHITRATIO)
}

/// The caster's magic scaling of a damage roll: + mag/3.3 %.
pub fn magic_scale(dmg: i32, mag: i32) -> i32 {
    let d = dmg as f64;
    let m = if mag <= 0 { 1.0 } else { mag as f64 } / 3.3f32 as f64;
    (d + d * (m / 100.0) + 0.5) as i32
}

/// `iGetWhetherMagicBonusEffect`: rain helps lightning, hurts fire.
pub fn weather_bonus(stype: i32, weather: i32) -> i32 {
    match (weather, stype) {
        (1..=3, 10 | 37 | 43 | 51) => 1,
        (1..=3, 20 | 30) => -1,
        _ => 0,
    }
}

/// `CMisc::GetPoint2`: the `count`-th step of the line (x0, y0) -> (x1, y1) (the error starts at 0).
pub fn get_point2(x0: i16, y0: i16, x1: i16, y1: i16, count: i32) -> (i16, i16) {
    if (x0, y0) == (x1, y1) {
        return (x0, y0);
    }
    let (mut dx, mut dy) = ((x1 - x0) as i32, (y1 - y0) as i32);
    let xi = if dx >= 0 { 1 } else { dx = -dx; -1 };
    let yi = if dy >= 0 { 1 } else { dy = -dy; -1 };
    let (mut rx, mut ry, mut e, mut n) = (x0 as i32, y0 as i32, 0, 0);
    let (major, minor) = if dx > dy { (dx, dy) } else { (dy, dx) };
    for _ in 0..=major {
        e += minor;
        if e > major {
            e -= major;
            if dx > dy { ry += yi } else { rx += xi }
        }
        if dx > dy { rx += xi } else { ry += yi }
        n += 1;
        if n >= count {
            break;
        }
    }
    (rx as i16, ry as i16)
}

fn debug() -> bool {
    static D: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *D.get_or_init(|| std::env::var_os("HG_MAGIC_DEBUG").is_some())
}

pub fn mlog(msg: &str) {
    if debug() {
        crate::log::log(msg);
    }
}

/// How a tile's unit is hit: the damage values, the exp flag and the damage-move origin (None: Effect_Damage_Spot).
#[derive(Clone, Copy)]
struct Dmg {
    v: (i32, i32, i32),
    exp: bool,
    mv: Option<(i16, i16)>,
}

/// What follows a hit that was not resisted.
#[derive(Clone, Copy)]
enum Post {
    None,
    SpDown((i32, i32, i32)),
    Armor(i32),
    /// effect type, seconds, skip neutral players (MAGICTYPE_ICE)
    Ice(i32, u32, bool),
}

impl Game {
    /// `iClientMotion_Magic_Handler`: the precast. 1 = confirmed (the caller broadcasts OBJECTMAGIC), 2 = reject.
    pub fn motion_magic(&mut self, h: usize, sx: i16, sy: i16, dir: u8) -> i32 {
        let Some(c) = self.client_mut(h) else { return 0 };
        if c.is_killed || !c.is_init_complete {
            return 0;
        }
        if (sx, sy) != (c.x, c.y) {
            return 2;
        }
        self.inc_player_activity(h);
        self.pretend_corpse_reown(h, sx, sy, false);
        self.clear_skill_using_status(h);
        self.remove_magic_effect(h, OWNERTYPE_PLAYER, MAGICTYPE_INVISIBILITY);
        self.client_mut(h).unwrap().dir = dir;
        let mut b = vec![0u8; 6];
        W(&mut b).u32(0, MSGID_RESPONSE_MOTION);
        W(&mut b).u16(4, OBJECTMOTION_CONFIRM as u16);
        self.send(h, &b);
        1
    }

    /// ClientCommonHandler COMMONTYPE_MAGIC: v1, v2 = target tile, v3 = spell + 100; needs the precast (or a GM).
    pub fn common_magic(&mut self, h: usize, v1: i32, v2: i32, v3: i32) {
        let Some(c) = self.client(h) else { return };
        if c.cs.has_precasted || c.is_gm() {
            self.player_magic(h, v1, v2, (v3 - 100) as i16 as i32, false, 0);
        } else {
            mlog("(MAGIA) ignorado: llego el lanzamiento sin la fase previa (precast)");
        }
    }

    /// `PlayerMagicHandler` (bItemEffect: cast by an item, no mastery needed; v1: the summon kind).
    pub fn player_magic(&mut self, h: usize, dx: i32, dy: i32, stype: i32, item_effect: bool, v1: i32) {
        let now = self.now();
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            mlog("(MAGIA) rechazado: jugador no inicializado");
            return;
        }
        let m = c.map_index;
        if dx < 0 || dx >= self.maps[m].size_x as i32 || dy < 0 || dy >= self.maps[m].size_y as i32 {
            return;
        }
        let (dx, dy) = (dx as i16, dy as i16);
        let c = self.client_mut(h).unwrap();
        c.cs.has_precasted = false;
        if (dx - c.x).abs() > 11 || (dy - c.y).abs() > 11 {
            let msg = format!("Player ({}) casted further than maximum range ({},{})->({},{})", String::from_utf8_lossy(crate::util::cstr(&c.char_name)), c.x, c.y, dx, dy);
            crate::log::log(&msg);
            crate::log::log_file(&msg);
            return;
        }
        let mastery = if (0..100).contains(&stype) { c.magic_mastery[stype as usize] as i32 } else { -1 };
        let cm = c.map_index;
        let attack_on = self.maps[cm].is_attack_enabled;
        mlog(&format!("(MAGIA) intento: tipo={stype} destino=({dx},{dy}) maestria={mastery} mapaAtaque={}", attack_on as i32));
        if !(0..100).contains(&stype) {
            mlog("(MAGIA) rechazado: tipo fuera de rango");
            return;
        }
        let Some(sp) = self.data.magic.get(stype as usize).and_then(|r| r.as_deref()).and_then(Spell::parse) else {
            mlog("(MAGIA) rechazado: hechizo sin configuracion en el servidor");
            return;
        };
        let c = self.client(h).unwrap();
        // Rust only: GM spells (cost < 0) are refused to players even if their mastery byte says learned
        if sp.cost < 0 && !c.is_gm() {
            mlog("(MAGIA) rechazado: hechizo de GM");
            return;
        }
        if !item_effect && mastery != 1 {
            mlog("(MAGIA) rechazado: el jugador no tiene aprendido ese hechizo");
            return;
        }
        let mp = &self.maps[c.map_index];
        if !mp.is_attack_enabled && sp.ty != MAGICTYPE_CREATE && sp.ty != MAGICTYPE_TELEPORT && !c.is_gm() {
            mlog("(MAGIA) rechazado: mapa con ataque desactivado");
            return;
        }
        if mp.magic_limited[stype as usize] && !c.is_gm() {
            mlog("(MAGIA) rechazado: hechizo limitado en este mapa");
            return;
        }
        if c.magic_effect_status[MAGICTYPE_INHIBITION as usize] != 0 {
            mlog("(MAGIA) rechazado: inhibicion activa");
            return;
        }
        if c.equip_status[EQUIPPOS_RHAND as usize] != -1 {
            let wt = (c.appr[1] as u16 & 0x0FF0) >> 4;
            if !(34..=39).contains(&wt) {
                mlog("(MAGIA) rechazado: arma equipada que no es de magia");
                return;
            }
        }
        if c.equip_status[EQUIPPOS_LHAND as usize] != -1 || c.equip_status[EQUIPPOS_TWOHAND as usize] != -1 {
            mlog("(MAGIA) rechazado: escudo o arma a dos manos equipada");
            return;
        }
        if now.wrapping_sub(c.recent_attack_time) < 100 {
            mlog("(MAGIA) rechazado: demasiado rapido tras otra accion");
            return;
        }
        let c = self.client_mut(h).unwrap();
        (c.recent_attack_time, c.last_action_time) = (now, now);
        let (sx, sy) = (c.x, c.y);
        let weather = self.maps[m].weather as i32;
        let c = self.client(h).unwrap();
        let castprob = (c.fx.swe_type == ITEMSTAT_CASTPROB).then_some(c.fx.swe_value);
        let mut result = cast_probability(stype, c.get_int(), c.level, weather, castprob);
        let wb = weather_bonus(stype, weather);
        let mana = mana_cost(sp.mana, c.is_safe_attack_mode && !self.maps[m].is_fight_zone, c.fx.mp_save_ratio);
        if result < 100 && result < self.rng.dice(1, 100) {
            mlog(&format!("(MAGIA) fallo: probabilidad {result}"));
            self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, 0, -1, 0);
            return;
        }
        let c = self.client(h).unwrap();
        if (c.hunger_status <= 10 || c.sp <= 0) && self.rng.dice(1, 1000) <= 100 {
            self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, 0, -1, 0);
            return;
        }
        let c = self.client(h).unwrap();
        if c.mp < mana {
            mlog(&format!("(MAGIA) rechazado: mana {} < {mana}", c.mp));
            return;
        }
        result = magic_hit_ratio(result, c.get_mag(), c.level, stype, c.fx.add_ar);
        if sp.category == 1 && self.maps[m].attribute(sx, sy, 0x05) != 0 {
            return;
        }
        self.remove_magic_effect(h, OWNERTYPE_PLAYER, MAGICTYPE_INVISIBILITY);
        // the C++ leaves the spells with a delay time (none in Magic.cfg) without effect
        if sp.delay == 0 && !self.magic_effect(h, &sp, stype, m, (sx, sy), (dx, dy), result, wb, v1) {
            return;
        }
        // MAGIC_NOEFFECT: the mana slate makes the spell free
        let Some(c) = self.client_mut(h) else { return };
        let mana = if c.status & STATUS_BLUESLATE != 0 { 0 } else { mana };
        c.mp = (c.mp - mana).max(0);
        let (cm, cx, cy, ct) = (c.map_index, c.x, c.y, c.type_);
        self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_MAGIC as u16, cm, cx, cy, [dx, dy, (stype + 100) as i16, ct]);
        self.notify(h, NOTIFY_MP, &[], None);
    }

    /// The live owner of a tile.
    fn owner_at(&self, m: usize, x: i16, y: i16) -> Option<(usize, u8)> {
        let (o, t) = self.maps[m].get_owner(x, y);
        let ok = match t {
            OWNERTYPE_PLAYER => self.client(o as usize).is_some(),
            OWNERTYPE_NPC => self.npcs.get(o as usize).is_some_and(|n| n.is_some()),
            _ => false,
        };
        (o != 0 && ok).then_some((o as usize, t))
    }

    /// The dead-owner slot of a tile when it holds a player with hp > 0 (the C++ hits those too).
    fn dead_player_at(&self, m: usize, x: i16, y: i16) -> Option<usize> {
        let (o, t) = self.maps[m].get_dead_owner(x, y);
        (t == OWNERTYPE_PLAYER && self.client(o as usize).is_some_and(|c| c.hp > 0)).then_some(o as usize)
    }

    /// One unit: resist, damage, then the follow-up effect.
    fn magic_hit(&mut self, a: (usize, u8), t: usize, tt: u8, r: i32, attr: i32, d: Dmg, post: Post) {
        if self.resist_magic(t, tt, r) {
            return;
        }
        self.effect_damage(a.0, a.1, t, tt, d.v, d.exp, attr, d.mv);
        match post {
            Post::None => {}
            Post::SpDown(v) => self.effect_sp_down(a.0, a.1, t, tt, v),
            Post::Armor(v) => self.armor_life_decrement(a.0, t, tt, v),
            Post::Ice(e, secs, skip_neutral) => {
                // fix: MAGICTYPE_ICE jumped to MAGIC_NOEFFECT on a neutral player, ending the whole area loop
                if skip_neutral && tt == OWNERTYPE_PLAYER && self.client(t).is_some_and(|c| c.is_neutral()) {
                    return;
                }
                if !self.target_dead(t, tt) && !self.resist_ice(t, tt) {
                    self.add_magic_effect(t, tt, e, secs, 1);
                }
            }
        }
    }

    /// A tile: its live owner, then a player standing on its dead-owner slot.
    #[allow(clippy::too_many_arguments)]
    fn magic_hit_tile(&mut self, a: (usize, u8), m: usize, x: i16, y: i16, r: i32, attr: i32, live: Option<(Dmg, Post)>, dead: Option<(Dmg, Post)>) {
        if let (Some((d, p)), Some((t, tt))) = (live, self.owner_at(m, x, y)) {
            self.magic_hit(a, t, tt, r, attr, d, p);
        }
        if let (Some((d, p)), Some(t)) = (dead, self.dead_player_at(m, x, y)) {
            self.magic_hit(a, t, OWNERTYPE_PLAYER, r, attr, d, p);
        }
    }

    /// The (y outer, x inner) tiles of a spell's area around (dx, dy).
    fn area(sp: &Spell, dx: i16, dy: i16) -> Vec<(i16, i16)> {
        let mut v = Vec::new();
        for y in dy - sp.v..=dy + sp.v {
            for x in dx - sp.h..=dx + sp.h {
                v.push((x, y));
            }
        }
        v
    }

    /// The switch of PlayerMagicHandler. false: a case that returns before the mana is spent.
    #[allow(clippy::too_many_arguments)]
    fn magic_effect(&mut self, h: usize, sp: &Spell, stype: i32, m: usize, s: (i16, i16), d: (i16, i16), r: i32, wb: i32, v1: i32) -> bool {
        let now = self.now();
        let (dx, dy) = d;
        let attr = sp.attr;
        let target = self.owner_at(m, dx, dy);
        let spot = |v, exp| Dmg { v, exp, mv: None };
        let mv = |v, exp, p| Dmg { v, exp, mv: Some(p) };
        let caster_side = self.client(h).unwrap().side;
        let pa = (h, OWNERTYPE_PLAYER);
        match sp.ty {
            MAGICTYPE_DAMAGE_SPOT => {
                let a = Some((spot(sp.main(wb), true), Post::None));
                self.magic_hit_tile(pa, m, dx, dy, r, attr, a, a);
            }
            MAGICTYPE_DAMAGE_SPOT_SPDOWN => {
                let a = Some((spot(sp.main(wb), true), Post::SpDown(sp.linear(0))));
                self.magic_hit_tile(pa, m, dx, dy, r, attr, a, a);
            }
            MAGICTYPE_DAMAGE_LINEAR | MAGICTYPE_ICE_LINEAR => {
                let ice = if sp.ty == MAGICTYPE_ICE_LINEAR { Post::Ice(MAGICTYPE_ICE_LINEAR, sp.val[6].max(0) as u32, false) } else { Post::None };
                const CROSS: [(i16, i16); 5] = [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)];
                for i in 2..10 {
                    let (tx, ty) = get_point2(s.0, s.1, dx, dy, i);
                    for (cx, cy) in CROSS {
                        let live = Some((mv(sp.linear(wb), false, s), ice));
                        self.magic_hit_tile(pa, m, tx + cx, ty + cy, r, attr, live, Some((spot(sp.linear(wb), false), ice)));
                    }
                    if (tx - dx).abs() <= 1 && (ty - dy).abs() <= 1 {
                        break;
                    }
                }
                for (x, y) in Game::area(sp, dx, dy) {
                    let a = Some((mv(sp.linear(wb), false, d), ice));
                    self.magic_hit_tile(pa, m, x, y, r, attr, a, a);
                }
                let a = Some((spot(sp.main(wb), true), ice));
                self.magic_hit_tile(pa, m, dx, dy, r, attr, a, a);
            }
            MAGICTYPE_CANCELLATION => {
                if let Some((t, tt)) = target {
                    for e in [MAGICTYPE_INVISIBILITY, MAGICTYPE_PROTECT, MAGICTYPE_HOLDOBJECT, MAGICTYPE_CONFUSE, MAGICTYPE_BERSERK, MAGICTYPE_ICE, MAGICTYPE_POLYMORPH, MAGICTYPE_INHIBITION] {
                        self.remove_magic_effect(t, tt, e);
                    }
                }
            }
            MAGICTYPE_INHIBITION => {
                if let Some((t, OWNERTYPE_PLAYER)) = target {
                    if !self.client(t).unwrap().is_invincible() {
                        self.add_magic_effect(t, OWNERTYPE_PLAYER, sp.ty, sp.last_time, 1);
                    }
                }
            }
            MAGICTYPE_HPUP_SPOT => {
                if let Some((t, tt)) = target {
                    self.effect_hp_up(h, t, tt, sp.main(0));
                }
            }
            MAGICTYPE_TREMOR => {
                let a = Some((spot(sp.main(wb), true), Post::None));
                self.magic_hit_tile(pa, m, dx, dy, r, attr, a, a);
                for (x, y) in Game::area(sp, dx, dy) {
                    let a = Some((spot(sp.linear(wb), false), Post::None));
                    self.magic_hit_tile(pa, m, x, y, r, attr, a, a);
                }
            }
            MAGICTYPE_DAMAGE_AREA => {
                let a = Some((spot(sp.main(wb), true), Post::None));
                self.magic_hit_tile(pa, m, dx, dy, r, attr, a, a);
                for (x, y) in Game::area(sp, dx, dy) {
                    let a = Some((mv(sp.linear(wb), false, d), Post::None));
                    self.magic_hit_tile(pa, m, x, y, r, attr, a, a);
                }
            }
            MAGICTYPE_DAMAGE_AREA_NOSPOT => {
                for (x, y) in Game::area(sp, dx, dy) {
                    let a = Some((mv(sp.linear(wb), false, d), Post::None));
                    self.magic_hit_tile(pa, m, x, y, r, attr, a, a);
                }
            }
            MAGICTYPE_DAMAGE_AREA_NOSPOT_SPDOWN => {
                for (x, y) in Game::area(sp, dx, dy) {
                    let a = Some((mv(sp.main(wb), false, d), Post::SpDown(sp.linear(0))));
                    self.magic_hit_tile(pa, m, x, y, r, attr, a, a);
                }
            }
            MAGICTYPE_DAMAGE_AREA_ARMOR_BREAK => {
                for (x, y) in Game::area(sp, dx, dy) {
                    let a = Some((mv(sp.linear(wb), false, d), Post::Armor(sp.val[6])));
                    self.magic_hit_tile(pa, m, x, y, r, attr, a, a);
                }
            }
            MAGICTYPE_ICE => {
                let secs = sp.val[6].max(0) as u32;
                for (x, y) in Game::area(sp, dx, dy) {
                    let live = Some((mv(sp.main(wb), true, d), Post::Ice(MAGICTYPE_ICE, secs, true)));
                    self.magic_hit_tile(pa, m, x, y, r, attr, live, Some((spot(sp.main(wb), true), Post::Ice(MAGICTYPE_ICE, secs, false))));
                }
            }
            MAGICTYPE_SPDOWN_AREA => {
                if let Some((t, tt)) = target {
                    if !self.resist_magic(t, tt, r) {
                        self.effect_sp_down(h, OWNERTYPE_PLAYER, t, tt, sp.main(0));
                    }
                }
                for (x, y) in Game::area(sp, dx, dy) {
                    if let Some((t, tt)) = self.owner_at(m, x, y) {
                        if !self.resist_magic(t, tt, r) {
                            self.effect_sp_down(h, OWNERTYPE_PLAYER, t, tt, sp.linear(0));
                        }
                    }
                }
            }
            MAGICTYPE_SPUP_AREA => {
                if let Some((t, tt)) = target {
                    self.effect_rec_sp(t, tt, sp.main(0));
                }
                for (x, y) in Game::area(sp, dx, dy) {
                    if let Some((t, tt)) = self.owner_at(m, x, y) {
                        // fix: CMagic::Effect_RecSP ignored its values and always restored the spot amount
                        self.effect_rec_sp(t, tt, sp.linear(0));
                    }
                }
            }
            MAGICTYPE_TELEPORT => {
                if sp.val[0] == 1 && target == Some((h, OWNERTYPE_PLAYER)) {
                    let c = self.client(h).unwrap();
                    if now.wrapping_sub(c.cs.last_damage_time) > 10000 || c.is_gm() {
                        // a recall point chosen with MSGID_REQUEST_SETRECALLPNT (in the home town) is a scripted teleport
                        let point = (c.next_recall_point != 0 && crate::util::cstr(&c.map_name) == SIDE_MAP[c.side.clamp(0, 3) as usize])
                            .then(|| self.maps[c.map_index].initial_points.get(c.next_recall_point as usize).copied().flatten())
                            .flatten();
                        match point {
                            Some((px, py)) => {
                                let name = c.map_name.clone();
                                self.request_teleport(h, 3, Some(&name), px, py);
                            }
                            None => self.request_teleport(h, 1, None, -1, -1),
                        }
                    } else {
                        self.notify(h, NOTIFY_CANNOTRECALL, &[], None);
                    }
                }
            }
            MAGICTYPE_SUMMON => {
                if self.maps[m].is_fight_zone || (self.hd.mode && self.is_heldenian_map(m)) {
                    return false;
                }
                if let Some((t, OWNERTYPE_PLAYER)) = target {
                    if self.client(t).unwrap().side == caster_side {
                        self.summon(m, dx, dy, t, caster_side, v1);
                    }
                }
            }
            MAGICTYPE_CREATE => {
                if !self.maps[m].is_move_allowed(dx, dy) {
                    return true;
                }
                // fix: a value other than 1 used an uninitialised item id
                if sp.val[0] == 1 {
                    let id = if self.rng.dice(1, 2) == 1 { ITEM_MEAT } else { ITEM_BAGUETTE };
                    if let Some(cfg) = self.data.item_by_id(id).cloned() {
                        let mut it = Item::new(&cfg);
                        it.count = 1; // (a port bug until now: _bInitItemAttr gives count 1)
                        it.touch_effect_type = ITET_ID as i16;
                        it.touch_effect_value = [self.rng.dice(1, 100000) as i16, self.rng.dice(1, 100000) as i16, now as i16];
                        let logged = it.clone();
                        if self.drop_ground_item(m, dx, dy, it) {
                            self.item_log(ITEMLOG_DROP, h, -1, &logged, false);
                        }
                    }
                }
            }
            MAGICTYPE_PROTECT => {
                if let Some((t, tt)) = target {
                    let npc_busy = tt == OWNERTYPE_NPC && self.npcs[t].as_ref().unwrap().action_limit != 0;
                    if self.unit_effect(t, tt, MAGICTYPE_PROTECT) == 0 && !npc_busy {
                        self.set_magic_effect(t, tt, MAGICTYPE_PROTECT, sp.val[0], sp.last_time, None);
                    }
                }
            }
            MAGICTYPE_HOLDOBJECT => {
                let Some((t, tt)) = target else { return true };
                if self.resist_magic(t, tt, r) {
                    return true;
                }
                let ok = if tt == OWNERTYPE_PLAYER {
                    let (c, p) = (self.client(h).unwrap(), self.client(t).unwrap());
                    let map = &self.maps[p.map_index];
                    let towns = [SIDE_MAP[ARESDEN as usize], SIDE_MAP[ELVINE as usize], SIDE_MAP[ISTRIA as usize], SIDE_MAP[NEUTRAL as usize], SIDE_MAP_FARM[ARESDEN as usize], SIDE_MAP_FARM[ELVINE as usize], SIDE_MAP_FARM[ISTRIA as usize]];
                    let town = towns.contains(&crate::util::cstr(&self.maps[m].name));
                    p.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] == 0
                        && map.attribute(s.0, s.1, 0x06) == 0
                        && map.attribute(dx, dy, 0x06) == 0
                        && !(c.party_id != 0 && p.party_id == c.party_id)
                        && p.fx.add_pr < 500
                        && !(c.side == p.side && !self.cr.mode && c.admin_user_level == 0 && town)
                } else {
                    let n = self.npcs[t].as_ref().unwrap();
                    n.magic_level < 6 && n.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] == 0
                };
                if ok {
                    self.set_magic_effect(t, tt, MAGICTYPE_HOLDOBJECT, sp.val[0], sp.last_time, None);
                }
            }
            MAGICTYPE_INVISIBILITY => match sp.val[0] {
                1 => {
                    let Some((t, tt)) = target else { return true };
                    if self.unit_effect(t, tt, MAGICTYPE_INVISIBILITY) != 0 {
                        return true;
                    }
                    if tt == OWNERTYPE_PLAYER && self.is_relic_holder(t) {
                        return true; // the relic holder cannot turn invisible
                    }
                    if tt == OWNERTYPE_PLAYER || self.npcs[t].as_ref().unwrap().action_limit == 0 {
                        self.set_magic_effect(t, tt, MAGICTYPE_INVISIBILITY, sp.val[0], sp.last_time, None);
                        self.remove_from_target(t, tt, MAGICTYPE_INVISIBILITY);
                    } else {
                        // an NPC with an action limit only gets the release event, like the C++
                        self.register_delay_event(DELAYEVENTTYPE_MAGICRELEASE, MAGICTYPE_INVISIBILITY, now.wrapping_add(sp.last_time * 1000), t, tt);
                    }
                }
                2 => {
                    for x in dx - 8..=dx + 8 {
                        for y in dy - 8..=dy + 8 {
                            if let Some((t, tt)) = self.owner_at(m, x, y) {
                                if self.unit_effect(t, tt, MAGICTYPE_INVISIBILITY) != 0 {
                                    self.unit_mut(t, tt).unwrap().0[MAGICTYPE_INVISIBILITY as usize] = 0;
                                    self.set_unit_status_flag(t, tt, STATUS_INVISIBILITY, false);
                                    self.remove_delay_events(t, tt, MAGICTYPE_INVISIBILITY);
                                }
                            }
                        }
                    }
                }
                _ => {}
            },
            MAGICTYPE_CREATE_DYNAMIC => {
                // iAddDynamicObjectList for every tile of the wall / field (with its bAnalyzeCriminalAction)
                let town = [SIDE_MAP[ARESDEN as usize], SIDE_MAP[ELVINE as usize], SIDE_MAP[ISTRIA as usize], SIDE_MAP_FARM[ARESDEN as usize], SIDE_MAP_FARM[ELVINE as usize], SIDE_MAP_FARM[ISTRIA as usize]];
                if !self.cr.mode && town.contains(&crate::util::cstr(&self.maps[m].name)) {
                    return false;
                }
                if !matches!(sp.val[6], DYNAMICOBJECT_ICESTORM | 1 | 9 | 10) {
                    // no other object type is created by a spell
                } else if sp.val[6] != DYNAMICOBJECT_ICESTORM {
                    match sp.val[7] {
                        1 => {
                            let (rx, ry) = match crate::npc::misc_dir(s.0, s.1, dx, dy) {
                                1 | 5 => (1, 0),
                                2 => (1, 1),
                                3 => (0, 1),
                                4 => (-1, 1),
                                6 => (-1, -1),
                                7 => (0, -1),
                                8 => (1, -1),
                                _ => (0, 0),
                            };
                            let ms = sp.last_time * 1000;
                            self.add_dynamic_object(h, 3, sp.val[6], m, dx, dy, ms, 0);
                            self.analyze_criminal_action(h, dx, dy, false);
                            for i in 1..=sp.h {
                                self.add_dynamic_object(h, 3, sp.val[6], m, dx + i * rx, dy + i * ry, ms, 0);
                                self.analyze_criminal_action(h, dx + i * rx, dy + i * ry, false);
                                self.add_dynamic_object(h, 3, sp.val[6], m, dx - i * rx, dy - i * ry, ms, 0);
                                self.analyze_criminal_action(h, dx - i * rx, dy - i * ry, false);
                            }
                        }
                        2 => {
                            for (x, y) in Game::area(sp, dx, dy) {
                                self.add_dynamic_object(h, 3, sp.val[6], m, x, y, sp.last_time * 1000, sp.val[0]);
                            }
                            // fix: the C++ checked the tile past each row (ix after its loop) instead of the area
                            let hit = Game::area(sp, dx, dy).into_iter().rfind(|&(x, y)| self.analyze_criminal_action(h, x, y, true));
                            if let Some((x, y)) = hit {
                                self.analyze_criminal_action(h, x, y, false);
                            }
                        }
                        _ => {}
                    }
                } else {
                    self.add_dynamic_object(h, 3, DYNAMICOBJECT_ICESTORM, m, dx, dy, sp.last_time * 1000, 100);
                }
            }
            MAGICTYPE_POSSESSION => {
                if self.player_num(m, dx, dy, 1) == 0 {
                    if let Some(it) = self.take_ground_item(m, dx, dy) {
                        if self.add_client_item_list(h, &it) {
                            if self.is_genuine_relic(&it) {
                                self.player_get_relic(h);
                            }
                            let top = self.maps[m].tile(dx, dy).and_then(|t| t.top_item).unwrap_or((0, 0, 0));
                            self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_SETITEM as u16, m, dx, dy, [top.0, top.1, top.2 as i16, 0]);
                            self.send_item_obtained(h, &it);
                        } else {
                            // bSetItem: back on the tile (with its ITEMDROP event, unlike the get-item motion)
                            self.drop_ground_item(m, dx, dy, it);
                            self.send_cannot_carry(h);
                        }
                    }
                }
            }
            MAGICTYPE_CONFUSE => {
                let k = sp.val[0];
                if (1..=4).contains(&k) {
                    for (x, y) in Game::area(sp, dx, dy) {
                        let Some((t, OWNERTYPE_PLAYER)) = self.owner_at(m, x, y) else { continue };
                        if self.client(t).unwrap().side == caster_side || self.resist_magic(t, OWNERTYPE_PLAYER, r) || self.unit_effect(t, OWNERTYPE_PLAYER, MAGICTYPE_CONFUSE) != 0 {
                            continue;
                        }
                        self.set_magic_effect(t, OWNERTYPE_PLAYER, MAGICTYPE_CONFUSE, k, sp.last_time, (k >= 3).then_some(h as i32));
                    }
                }
            }
            MAGICTYPE_POLYMORPH => {
                // no resistance check (`if (1)` in the C++); the target becomes type 18 until the effect runs out
                // fix: with nobody on the tile the C++ still registered the release event (of owner type 0, read through a
                // stale pointer by the delay processor); nothing is registered here
                let Some((t, tt)) = target else { return true };
                let kind = sp.val[0] as i8 as i32;
                if self.unit_effect(t, tt, MAGICTYPE_POLYMORPH) != 0 {
                    return true;
                }
                if tt == OWNERTYPE_PLAYER {
                    let c = self.client_mut(t).unwrap();
                    (c.original_type, c.type_) = (c.type_, 18);
                } else {
                    let n = self.npcs[t].as_mut().unwrap();
                    (n.original_type, n.type_) = (n.type_, 18);
                }
                self.send_event_near_unit(t, tt);
                self.set_magic_effect(t, tt, MAGICTYPE_POLYMORPH, kind, sp.last_time, None);
            }
            MAGICTYPE_POISON => {
                let Some((t, tt)) = target else { return true };
                if sp.val[0] == 1 {
                    if tt == OWNERTYPE_PLAYER {
                        self.analyze_criminal_action(h, dx, dy, false);
                    }
                    if self.client(h).is_none() || self.unit_mut(t, tt).is_none() {
                        return true;
                    }
                    if !self.resist_magic(t, tt, r) && !self.resist_poison(t, tt) && tt == OWNERTYPE_PLAYER {
                        let p = self.client_mut(t).unwrap();
                        (p.cs.is_poisoned, p.cs.poison_level, p.cs.poison_time) = (true, sp.val[1], now);
                        self.set_unit_status_flag(t, tt, STATUS_POISON, true);
                        self.notify(t, NOTIFY_MAGICEFFECTON, &[MAGICTYPE_POISON, sp.val[1]], None);
                    }
                } else if sp.val[0] == 0 && tt == OWNERTYPE_PLAYER && self.client(t).unwrap().cs.is_poisoned {
                    self.client_mut(t).unwrap().cs.is_poisoned = false;
                    self.set_unit_status_flag(t, tt, STATUS_POISON, false);
                    self.notify(t, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_POISON, 0], None);
                }
            }
            MAGICTYPE_BERSERK => {
                if sp.val[0] == 1 {
                    let Some((t, tt)) = target else { return true };
                    let ok = self.unit_effect(t, tt, MAGICTYPE_BERSERK) == 0
                        && (tt == OWNERTYPE_PLAYER || self.npcs[t].as_ref().is_some_and(|n| n.action_limit == 0 && n.side == caster_side));
                    if ok {
                        self.set_magic_effect(t, tt, MAGICTYPE_BERSERK, 1, sp.last_time, None);
                    }
                }
            }
            MAGICTYPE_RESURRECTION => {
                // GetDeadOwner: a dead player of the caster's side (a GM resurrects anyone); a non-GM caster needs
                // his special ability ready, uses it for SPECABLTYTIMESEC and loses the spell (it came with the wand)
                let dead = self.maps[m].tile(dx, dy).filter(|t| t.dead_owner != 0 && t.dead_owner_class == OWNERTYPE_PLAYER).map(|t| t.dead_owner as usize);
                let Some(t) = dead.filter(|&t| self.client(t).is_some_and(|p| p.is_killed)) else { return true };
                let c = self.client(h).unwrap();
                if !c.is_gm() {
                    if c.side != self.client(t).unwrap().side || c.special_ability_time != 0 {
                        return true;
                    }
                    let c = self.client_mut(h).unwrap();
                    c.special_ability_time = SPECABLTYTIMESEC;
                    c.magic_mastery[94] = 0;
                    self.notify(h, NOTIFY_SPELL_SKILL, &[], None);
                    self.notify(h, NOTIFY_SPECIALABILITYSTATUS, &[1, 55, SPECABLTYTIMESEC], None);
                }
                self.client_mut(t).unwrap().is_being_resurrected = true;
                self.notify(t, NOTIFY_RESURRECTPLAYER, &[], None);
            }
            MAGICTYPE_SCAN => {
                if let Some((t, tt)) = target {
                    if !self.resist_magic(t, tt, r) {
                        let msg = if tt == OWNERTYPE_PLAYER {
                            let p = self.client(t).unwrap();
                            format!(" Player: {} HP:{} MP:{}.", String::from_utf8_lossy(crate::util::cstr(&p.char_name)), p.hp, p.mp)
                        } else {
                            let n = self.npcs[t].as_ref().unwrap();
                            format!(" NPC: {} HP:{} MP:{}", String::from_utf8_lossy(crate::util::cstr(&n.npc_name)), n.hp, n.mp)
                        };
                        self.show_client_msg(h, msg.as_bytes());
                        let c = self.client(h).unwrap();
                        let (cx, cy) = (c.x, c.y);
                        self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_MAGIC as u16, m, cx, cy, [dx, dy, 10, 10]);
                    }
                }
            }
            t => mlog(&format!("(MAGIA) tipo de efecto {t} no portado (hechizo {stype})")),
        }
        true
    }

    /// `CNpc::magicHandler`: an NPC casts `stype` at (dx, dy) with its m_iMagicHitRatio.
    pub fn npc_magic(&mut self, i: usize, dx: i16, dy: i16, stype: i32) {
        let Some(n) = self.npcs.get(i).and_then(|n| n.as_ref()) else { return };
        let (m, s, r, nty) = (n.map_index, (n.x, n.y), n.magic_hit_ratio, n.type_);
        if !self.maps[m].is_attack_enabled || !self.maps[m].valid(dx, dy) || !(0..100).contains(&stype) {
            return;
        }
        let Some(sp) = self.data.magic.get(stype as usize).and_then(|r| r.as_deref()).and_then(Spell::parse) else { return };
        let wb = weather_bonus(stype, self.maps[m].weather as i32);
        let (a, attr, d) = ((i, OWNERTYPE_NPC), sp.attr, (dx, dy));
        let target = self.owner_at(m, dx, dy);
        let spot = |v, exp| Dmg { v, exp, mv: None };
        let mv = |v, exp, p| Dmg { v, exp, mv: Some(p) };
        if sp.delay == 0 {
            match sp.ty {
                MAGICTYPE_INVISIBILITY => match sp.val[0] {
                    1 => {
                        if let Some((t, tt)) = target {
                            self.add_magic_effect(t, tt, MAGICTYPE_INVISIBILITY, sp.last_time, 1);
                        }
                    }
                    2 => {
                        for x in dx - 8..=dx + 8 {
                            for y in dy - 8..=dy + 8 {
                                if let Some((t, tt)) = self.owner_at(m, x, y) {
                                    self.remove_magic_effect(t, tt, MAGICTYPE_INVISIBILITY);
                                    // the guild-NPC part of RemoveFromTarget(Unit*) comes with crusades
                                    self.remove_from_target(t, tt, 0);
                                }
                            }
                        }
                    }
                    _ => {}
                },
                MAGICTYPE_HOLDOBJECT => {
                    if let Some((t, tt)) = target {
                        if !self.resist_magic(t, tt, r) && !(tt == OWNERTYPE_NPC && self.npcs[t].as_ref().unwrap().magic_level >= 6) {
                            self.add_magic_effect(t, tt, MAGICTYPE_HOLDOBJECT, sp.last_time, 1);
                        }
                    }
                }
                MAGICTYPE_DAMAGE_LINEAR | MAGICTYPE_ICE_LINEAR => {
                    let il = sp.ty == MAGICTYPE_ICE_LINEAR;
                    let ice = if il { Post::Ice(MAGICTYPE_ICE_LINEAR, sp.val[6].max(0) as u32, false) } else { Post::None };
                    // the NPC lightning (19) hits every tile with Effect_Damage_Spot; the ice one pushes from the caster
                    let line = if il { mv(sp.linear(wb), false, s) } else { spot(sp.linear(wb), false) };
                    const CROSS: [(i16, i16); 5] = [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)];
                    for k in 2..10 {
                        let (tx, ty) = get_point2(s.0, s.1, dx, dy, k);
                        for (cx, cy) in CROSS {
                            // fix: the C++ NPC lightning used the linear range as the bonus of its dead-tile and area hits
                            self.magic_hit_tile(a, m, tx + cx, ty + cy, r, attr, Some((line, ice)), Some((spot(sp.linear(wb), false), ice)));
                        }
                        if (tx - dx).abs() <= 1 && (ty - dy).abs() <= 1 {
                            break;
                        }
                    }
                    for (x, y) in Game::area(&sp, dx, dy) {
                        self.magic_hit_tile(a, m, x, y, r, attr, Some((line, ice)), Some((spot(sp.linear(wb), false), ice)));
                    }
                    let c = Some((spot(sp.main(wb), il), ice));
                    self.magic_hit_tile(a, m, dx, dy, r, attr, c, c);
                }
                MAGICTYPE_DAMAGE_SPOT => {
                    let c = Some((spot(sp.main(wb), true), Post::None));
                    self.magic_hit_tile(a, m, dx, dy, r, attr, c, c);
                }
                MAGICTYPE_HPUP_SPOT => {
                    if let Some((t, tt)) = target {
                        self.effect_hp_up(i, t, tt, sp.main(0));
                    }
                }
                MAGICTYPE_DAMAGE_AREA | MAGICTYPE_DAMAGE_AREA_NOSPOT => {
                    if sp.ty == MAGICTYPE_DAMAGE_AREA {
                        let c = Some((spot(sp.main(wb), true), Post::None));
                        self.magic_hit_tile(a, m, dx, dy, r, attr, c, c);
                    }
                    for (x, y) in Game::area(&sp, dx, dy) {
                        let c = Some((mv(sp.linear(wb), false, d), Post::None));
                        self.magic_hit_tile(a, m, x, y, r, attr, c, c);
                    }
                }
                MAGICTYPE_SPDOWN_AREA => {
                    if let Some((t, tt)) = target {
                        if !self.resist_magic(t, tt, r) {
                            self.effect_sp_down(i, OWNERTYPE_NPC, t, tt, sp.main(0));
                        }
                    }
                    for (x, y) in Game::area(&sp, dx, dy) {
                        if let Some((t, tt)) = self.owner_at(m, x, y) {
                            if !self.resist_magic(t, tt, r) {
                                self.effect_sp_down(i, OWNERTYPE_NPC, t, tt, sp.linear(0));
                            }
                        }
                    }
                }
                MAGICTYPE_SPUP_AREA => {
                    if let Some((t, tt)) = target {
                        self.effect_rec_sp(t, tt, sp.main(0));
                    }
                    for (x, y) in Game::area(&sp, dx, dy) {
                        if let Some((t, tt)) = self.owner_at(m, x, y) {
                            self.effect_rec_sp(t, tt, sp.linear(0));
                        }
                    }
                }
                MAGICTYPE_ICE => {
                    // NPCs freeze non-neutral players and summons only, and roll both resistances before the damage
                    let secs = sp.val[6].max(0) as u32;
                    for (x, y) in Game::area(&sp, dx, dy) {
                        let live = self.owner_at(m, x, y).filter(|&(t, tt)| {
                            !self.target_dead(t, tt)
                                && match tt {
                                    OWNERTYPE_PLAYER => !self.client(t).unwrap().is_neutral(),
                                    _ => self.npcs[t].as_ref().unwrap().is_summoned,
                                }
                        });
                        let dead = self.dead_player_at(m, x, y).filter(|&t| !self.client(t).unwrap().is_neutral()).map(|t| (t, OWNERTYPE_PLAYER));
                        for (k, hit) in [live, dead].into_iter().enumerate() {
                            let Some((t, tt)) = hit else { continue };
                            if self.resist_magic(t, tt, r) || self.resist_ice(t, tt) {
                                continue;
                            }
                            self.effect_damage(i, OWNERTYPE_NPC, t, tt, sp.main(wb), true, attr, (k == 0).then_some(d));
                            if !self.target_dead(t, tt) {
                                self.add_magic_effect(t, tt, MAGICTYPE_ICE, secs, 1);
                            }
                        }
                    }
                }
                t => mlog(&format!("(MAGIA) NPC: tipo de efecto {t} no portado (hechizo {stype})")),
            }
        }
        // NMH_NOEFFECT
        let Some(n) = self.npcs.get_mut(i).and_then(|n| n.as_mut()) else { return };
        n.mp = (n.mp - sp.mana).max(0);
        let (nx, ny) = (n.x, n.y);
        self.send_event_type_b(MSGID_EVENT_COMMON, COMMONTYPE_MAGIC as u16, m, nx, ny, [dx, dy, (stype + 100) as i16, nty]);
    }

    /// The spell an NPC of magic level `ml` with `mp` mana picks (behavior_attack); None: none affordable.
    pub fn npc_pick_spell(&mut self, mp: i32, ml: i32) -> Option<i32> {
        // fix: a spell missing from Magic.cfg (96 for level 13) was dereferenced (NULL) instead of skipped
        let ok = |g: &Game, t: i32| g.data.magic.get(t as usize).and_then(|r| r.as_deref()).and_then(Spell::parse).is_some_and(|s| s.mana <= mp);
        let first = |g: &Game, l: &[i32]| l.iter().copied().find(|&t| ok(g, t));
        match ml {
            1 => first(self, &[0]),
            2 => first(self, &[10, 0]),
            3 => first(self, &[20, 10]),
            4 => first(self, &[30, 37, 20, 10]),
            5 => first(self, &[43, 30, 37, 20, 10]),
            6 => first(self, &[51, 43, 30, 37, 20, 10]),
            7 | 8 | 9 | 10 | 12 | 13 => {
                let (rare, odds, hit, rest): (i32, i32, i32, &[i32]) = match ml {
                    7 => (70, 5, 3, &[61, 60, 51, 43]),
                    8 => (35, 3, 2, &[60, 51, 43]),
                    9 => (74, 3, 2, &[]),
                    10 => (57, 3, 2, &[]),
                    12 => (91, 3, 2, &[63]),
                    _ => (96, 3, 2, &[81]),
                };
                if ok(self, rare) && self.rng.dice(1, odds) == hit {
                    Some(rare)
                } else {
                    first(self, rest)
                }
            }
            m if m < 0 => first(self, &[43, 37, 0]),
            _ => None,
        }
    }

    /// Status, its flag broadcast, the release event and (players) MAGICEFFECTON, in the C++ order of the
    /// protect/hold/invisibility/berserk/confuse cases. `v3`: the caster for illusions.
    fn set_magic_effect(&mut self, t: usize, tt: u8, effect: i32, kind: i32, secs: u32, v3: Option<i32>) {
        let now = self.now();
        let Some((m, _)) = self.unit_mut(t, tt) else { return };
        m[effect as usize] = kind;
        if let Some(f) = magic_flag(effect, kind).filter(|&f| f != 0) {
            self.set_unit_status_flag(t, tt, f, true);
        }
        self.register_delay_event(DELAYEVENTTYPE_MAGICRELEASE, effect, now.wrapping_add(secs * 1000), t, tt);
        if tt == OWNERTYPE_PLAYER {
            let mut v = vec![effect, kind];
            v.extend(v3);
            self.notify(t, NOTIFY_MAGICEFFECTON, &v, None);
        }
    }

    /// MAGICTYPE_SUMMON: an "_<map><n>" NPC of the caster's side following the target player.
    fn summon(&mut self, m: usize, x: i16, y: i16, master: usize, side: i32, v1: i32) {
        // iGetFollowerNumber is computed by the C++ but never limits anything
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return };
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        let kind: &[u8] = match v1 {
            0 | 1 => b"Orc",
            2 => b"Skeleton",
            3 => b"Clay-Golem",
            4 => b"Stone-Golem",
            5 => b"Hellbound",
            6 => b"Cyclops",
            7 => b"Troll",
            8 => b"Orge",
            _ => b"",
        };
        let Some(i) = self.create_npc(kind, &name, m, 0, MOVETYPE_RANDOM, Some((x, y)), &[-1; 10], None, -1) else {
            self.maps[m].set_naming_value_empty(nv);
            return;
        };
        let now = self.now();
        let follow_side = self.client(master).map_or(side, |c| c.side);
        let n = self.npcs[i].as_mut().unwrap();
        (n.is_summoned, n.summoned_time) = (true, now);
        n.status = (n.status & 0x0FFF_FFFF) | (side << 28);
        // bSetNpcFollowMode (by handle here)
        (n.move_type, n.follow_owner, n.follow_owner_type, n.side) = (MOVETYPE_FOLLOW, master, OWNERTYPE_PLAYER, follow_side);
    }

    /// `CheckResistingMagicSuccess`: true = resisted.
    pub fn resist_magic(&mut self, t: usize, tt: u8, hit: i32) -> bool {
        let (resist, protect) = match tt {
            OWNERTYPE_PLAYER => {
                let Some(c) = self.client(t) else { return false };
                if c.is_invincible() {
                    return true;
                }
                let mut r = c.fx.add_mr + c.fx.add_resist_magic;
                if c.get_mag() > 50 {
                    r += c.get_mag() - 50;
                }
                if c.status & STATUS_REDSLATE != 0 {
                    return true;
                }
                (r, c.magic_effect_status[MAGICTYPE_PROTECT as usize])
            }
            OWNERTYPE_NPC => {
                let Some(n) = self.npcs.get(t).and_then(|n| n.as_ref()) else { return false };
                (n.resist_magic, n.magic_effect_status[MAGICTYPE_PROTECT as usize])
            }
            // fix: an empty tile read uninitialised resistance/protection values (and usually rolled a dice)
            _ => return false,
        };
        if protect == MAGICPROTECT_AMP || (hit < 1000 && protect == MAGICPROTECT_PFM) {
            return true;
        }
        let dest = magic_dest_ratio(hit, resist);
        dest < 100 && self.rng.dice(1, 100) > dest
    }

    /// `Effect_Damage_Spot` (mv None) and `Effect_Damage_Spot_DamageMove` (mv = the push origin).
    #[allow(clippy::too_many_arguments)]
    pub fn effect_damage(&mut self, ah: usize, at: u8, th: usize, tt: u8, v: (i32, i32, i32), exp: bool, attr: i32, mv: Option<(i16, i16)>) {
        let now = self.now();
        let alive = match at {
            OWNERTYPE_PLAYER => self.client(ah).is_some(),
            OWNERTYPE_NPC => self.npcs.get(ah).is_some_and(|n| n.is_some()),
            _ => false,
        };
        if !alive {
            return;
        }
        let mut dmg = (self.rng.dice(v.0, v.1) + v.2).max(0);
        let (aside, apos, party, afz);
        if at == OWNERTYPE_PLAYER {
            let a = self.client(ah).unwrap();
            dmg = magic_scale(dmg, a.get_mag());
            dmg += a.fx.add_magical_damage;
            dmg += dmg * a.fx.add_magic_dmg_pct / 100;
            dmg = dmg.max(0);
            afz = self.maps[a.map_index].is_fight_zone;
            if afz {
                dmg += dmg / 3;
            }
            if tt == OWNERTYPE_PLAYER && self.cr.mode && a.crusade_duty == 1 {
                // a fighter's bonus against players
                dmg += match a.level {
                    ..=80 => dmg * 7 / 10,
                    ..=100 => dmg / 2,
                    _ => dmg / 3,
                };
            }
            (aside, apos, party) = (a.side, (a.x, a.y), a.party_id);
        } else {
            let n = self.npcs[ah].as_ref().unwrap();
            (aside, apos, party, afz) = (n.side, (n.x, n.y), 0, false);
        }
        let apos = mv.unwrap_or(apos);
        match tt {
            OWNERTYPE_PLAYER => {
                let Some(t) = self.client(th) else { return };
                if !t.is_init_complete || t.is_killed {
                    return;
                }
                if !self.maps[t.map_index].is_attack_enabled {
                    return;
                }
                // BEGINNERTOWNPROTECT
                if at == OWNERTYPE_PLAYER && t.is_neutral && t.pk_count == 0 && t.is_on_town_nonpk {
                    return;
                }
                if now.wrapping_sub(t.time) > RAGPROTECTIONTIME as u32 {
                    return;
                }
                if at == OWNERTYPE_PLAYER && self.client(ah).unwrap().is_neutral && t.pk_count == 0 {
                    return;
                }
                if t.party_id != 0 && party == t.party_id {
                    return;
                }
                if t.status & STATUS_REDSLATE != 0 {
                    return;
                }
                if at == OWNERTYPE_PLAYER {
                    let a = self.client(ah).unwrap();
                    if a.is_safe_attack_mode && !self.is_player_hostile(ah, th) && !(afz && a.guild_guid != t.guild_guid) {
                        return;
                    }
                    if self.maps[t.map_index].attribute(t.x, t.y, 0x05) != 0 {
                        return;
                    }
                }
                self.clear_skill_using_status(th);
                let t = self.client(th).unwrap();
                let abs = match attr {
                    1 => t.fx.add_abs[1],
                    2 => t.fx.add_abs[0],
                    3 => t.fx.add_abs[2],
                    4 => t.fx.add_abs[3],
                    _ => 0,
                };
                if abs != 0 {
                    dmg = (dmg - (abs as f64 / 100.0 * dmg as f64) as i32).max(0);
                }
                let k = t.fx.magic_damage_save_item_index;
                if (0..MAXITEMS as i32).contains(&k) {
                    if let Some(it) = t.items[k as usize].as_ref() {
                        let cut = match it.cfg.id_num {
                            335 => 0.2f32 as f64,
                            337 => 0.1f32 as f64,
                            _ => 0.0,
                        };
                        if cut > 0.0 {
                            dmg = (dmg as f64 - dmg as f64 * cut + 0.5) as i32;
                        }
                        dmg = dmg.max(0);
                        if it.cur_life_span as i32 <= dmg {
                            self.item_deplete(th, k as usize, true);
                        } else {
                            self.client_mut(th).unwrap().items[k as usize].as_mut().unwrap().cur_life_span -= dmg as u16;
                        }
                    }
                }
                let t = self.client(th).unwrap();
                if t.fx.add_abs_md != 0 {
                    dmg -= (t.fx.add_abs_md as f64 / 100.0 * dmg as f64) as i32;
                }
                let vit = t.vit;
                dmg = (dmg - (self.rng.dice(1, vit / 10) - 1)).max(0);
                let pfm = |d: i32, t: &Client| if t.magic_effect_status[MAGICTYPE_PROTECT as usize] == MAGICPROTECT_PFM { (d as f64 * 0.5) as i32 } else { d };
                if mv.is_some() {
                    dmg = pfm(dmg, self.client(th).unwrap());
                }
                let t = self.client(th).unwrap();
                if t.fx.is_lucky_effect && self.rng.dice(1, 10) < 5 && self.client(th).unwrap().hp <= dmg {
                    dmg = 1;
                }
                if mv.is_none() {
                    dmg = pfm(dmg, self.client(th).unwrap());
                }
                let t = self.client(th).unwrap();
                if at == OWNERTYPE_PLAYER && t.cs.sa_enabled && matches!(t.fx.sa_type, 51 | 52) {
                    return;
                }
                let t = self.client_mut(th).unwrap();
                t.hp -= dmg;
                t.cs.last_damage_time = now;
                if t.hp <= 0 {
                    self.killed_handler(th, ah, at, dmg);
                    return;
                }
                if dmg <= 0 {
                    return;
                }
                if t.fx.add_trans_mana > 0 {
                    let cap = 2 * t.get_mag() + 2 * t.level + t.get_int() / 2;
                    t.mp = (t.mp + (t.fx.add_trans_mana as f64 / 100.0 * dmg as f64 + 1.0) as i32).min(cap);
                }
                let cc = t.fx.add_charge_critical;
                if cc > 0 && self.rng.dice(1, 100) < cc {
                    let t = self.client_mut(th).unwrap();
                    if t.super_attack_left < t.level / 10 {
                        t.super_attack_left += 1;
                    }
                    self.notify(th, NOTIFY_SUPERATTACKLEFT, &[], None);
                }
                let t = self.client(th).unwrap();
                let dir = crate::combat::damage_move_dir(apos, (t.x, t.y));
                let move_damage = if at == OWNERTYPE_PLAYER && afz { 80 } else { 50 };
                self.notify(th, NOTIFY_HP, &[], None);
                if mv.is_some() && dmg >= move_damage && dir != 0 {
                    self.client_mut(th).unwrap().cs.last_damage = dmg;
                    self.notify(th, NOTIFY_DAMAGEMOVE, &[dir as i32, dmg], None);
                } else {
                    self.send_event_near_type_a(th, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, dmg as i16, 0, 0);
                }
                // a pretend corpse that is hit stands up (ClearSkillUsingStatus above: always re-owned)
                let t = self.client(th).unwrap();
                let (tm, tx, ty) = (t.map_index, t.x, t.y);
                self.maps[tm].clear_owner(th as i16, OWNERTYPE_PLAYER, tx, ty);
                self.maps[tm].set_owner(th as i16, OWNERTYPE_PLAYER, tx, ty);
                let hold = self.client(th).unwrap().magic_effect_status[MAGICTYPE_HOLDOBJECT as usize];
                if hold != 0 {
                    self.notify(th, NOTIFY_MAGICEFFECTOFF, &[MAGICTYPE_HOLDOBJECT, hold], None);
                    self.client_mut(th).unwrap().magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] = 0;
                    self.remove_delay_events(th, OWNERTYPE_PLAYER, MAGICTYPE_HOLDOBJECT);
                }
            }
            OWNERTYPE_NPC => {
                let Some(n) = self.npcs.get(th).and_then(|n| n.as_ref()) else { return };
                if n.hp <= 0 {
                    return;
                }
                if self.cr.mode && aside == n.side {
                    return;
                }
                match n.action_limit {
                    1 | 2 | 6 => return,
                    4 => {
                        if mv.is_none() {
                            self.npc_damage_move(th, apos, None);
                        }
                        return;
                    }
                    _ => {}
                }
                if at == OWNERTYPE_PLAYER && matches!(n.type_, 40 | 41) {
                    let a = self.client(ah).unwrap();
                    if a.side == NEUTRAL || n.side == a.side {
                        return;
                    }
                }
                if n.abs_damage > 0 {
                    let d = dmg as f64;
                    dmg = (d - d * (n.abs_damage as f64 / 100.0)) as i32;
                    if dmg < 0 {
                        dmg = 1;
                    }
                }
                if n.magic_effect_status[MAGICTYPE_PROTECT as usize] == MAGICPROTECT_PFM {
                    dmg = (dmg as f64 * 0.5) as i32;
                }
                let n = self.npcs[th].as_mut().unwrap();
                n.hp -= dmg;
                if n.hp <= 0 {
                    self.npc_killed(ah, at, th, dmg);
                    return;
                }
                let (nty, nside) = (n.type_, n.side);
                if at == OWNERTYPE_PLAYER && !matches!(nty, 21 | 55 | 56) && nside == aside {
                    return;
                }
                if at == OWNERTYPE_NPC && aside == nside {
                    return;
                }
                self.send_event_near_npc_v(th, MSGID_EVENT_MOTION, OBJECTDAMAGE as u16, dmg as i16, 0, 0);
                if self.rng.dice(1, 3) != 2 || self.npcs[th].as_ref().unwrap().action_limit != 0 {
                    return;
                }
                if at == OWNERTYPE_NPC && self.npcs[ah].as_ref().unwrap().type_ == nty {
                    return; // (same side already returned above)
                }
                let n = self.npcs[th].as_mut().unwrap();
                (n.behavior, n.behavior_turn_count, n.target, n.target_type, n.time) = (BEHAVIOR_ATTACK, 0, ah, at, now);
                if n.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] != 0 {
                    n.magic_effect_status[MAGICTYPE_HOLDOBJECT as usize] = 0;
                    self.remove_delay_events(th, OWNERTYPE_NPC, MAGICTYPE_HOLDOBJECT);
                }
                let n = self.npcs[th].as_mut().unwrap();
                if n.no_die_remain_exp > 0 && !n.is_summoned && at == OWNERTYPE_PLAYER && self.clients[ah].is_some() {
                    let mut e = if n.no_die_remain_exp > dmg { dmg } else { n.no_die_remain_exp };
                    n.no_die_remain_exp = if n.no_die_remain_exp > dmg { n.no_die_remain_exp - dmg } else { 0 };
                    if self.cr.mode {
                        e = e.min(10);
                    }
                    let a = self.client(ah).unwrap();
                    if a.fx.add_exp > 0 {
                        e += (a.fx.add_exp as f64 / 100.0 * e as f64) as i32;
                    }
                    if a.level > 100 && matches!(nty, 55 | 56) {
                        e = 0;
                    }
                    self.get_exp(ah, if exp { e } else { e / 2 }, true);
                }
            }
            _ => {}
        }
    }

    /// `Effect_HpUp_Spot`
    fn effect_hp_up(&mut self, _h: usize, t: usize, tt: u8, v: (i32, i32, i32)) {
        let hp = self.rng.dice(v.0, v.1) + v.2;
        match tt {
            OWNERTYPE_PLAYER => {
                let Some(c) = self.client_mut(t) else { return };
                if c.is_killed {
                    return;
                }
                // CClient::AddHP
                c.hp = (c.hp + hp).min(c.get_max_hp());
                self.notify(t, NOTIFY_HP, &[], None);
            }
            OWNERTYPE_NPC => {
                let Some(n) = self.npcs.get_mut(t).and_then(|n| n.as_mut()) else { return };
                let max = n.hit_dice * 4;
                if n.hp > 0 && !n.is_killed && n.hp < max {
                    n.hp = (n.hp + hp).min(max).max(1);
                }
            }
            _ => {}
        }
    }

    /// `Effect_SpDown_Spot` (players only)
    pub fn effect_sp_down(&mut self, ah: usize, at: u8, t: usize, tt: u8, v: (i32, i32, i32)) {
        let sp = self.rng.dice(v.0, v.1) + v.2;
        if tt != OWNERTYPE_PLAYER {
            return;
        }
        let Some(p) = self.client(t) else { return };
        if p.is_killed || p.status & STATUS_REDSLATE != 0 {
            return;
        }
        if at == OWNERTYPE_PLAYER {
            let Some(a) = self.client(ah) else { return };
            if p.party_id != 0 && a.party_id == p.party_id {
                return;
            }
            if a.is_safe_attack_mode && !self.is_player_hostile(ah, t) && !(self.maps[a.map_index].is_fight_zone && a.guild_guid != p.guild_guid) {
                return;
            }
        }
        let p = self.client_mut(t).unwrap();
        if p.sp > 0 && p.time_left_firm_stamina == 0 {
            p.sp = (p.sp - sp).max(0);
            self.notify(t, NOTIFY_SP, &[], None);
        }
    }

    /// `CMagic::Effect_RecSP`
    fn effect_rec_sp(&mut self, t: usize, tt: u8, v: (i32, i32, i32)) {
        let sp = self.rng.dice(v.0, v.1) + v.2;
        if tt != OWNERTYPE_PLAYER {
            return;
        }
        let Some(p) = self.client_mut(t) else { return };
        let max = p.get_max_sp();
        if !p.is_killed && p.sp < max {
            p.sp = (p.sp + sp).min(max);
            self.notify(t, NOTIFY_SP, &[], None);
        }
    }

    /// `ArmorLifeDecrement`: body, pants, leggings, arms and head lose `v` endurance.
    fn armor_life_decrement(&mut self, ah: usize, t: usize, tt: u8, v: i32) {
        if tt != OWNERTYPE_PLAYER {
            return;
        }
        let (Some(a), Some(p)) = (self.client(ah), self.client(t)) else { return };
        if (a.side == p.side && !self.maps[a.map_index].is_fight_zone) || p.side == NEUTRAL {
            return;
        }
        for pos in [EQUIPPOS_BODY, EQUIPPOS_PANTS, EQUIPPOS_LEGGINGS, EQUIPPOS_ARMS, EQUIPPOS_HEAD] {
            let k = self.client(t).unwrap().equip_status[pos as usize];
            if k == -1 {
                continue;
            }
            let Some(it) = self.client_mut(t).unwrap().items[k as usize].as_mut() else { continue };
            // fix: m_wCurLifeSpan is a WORD: less endurance than the decrement wrapped to ~65000 instead of breaking
            it.cur_life_span = (it.cur_life_span as i32 - v).max(0) as u16;
            if it.cur_life_span == 0 {
                let ep = it.cfg.equip_pos;
                self.notify(t, NOTIFY_ITEMLIFESPANEND, &[ep, k as i32], None);
                self.release_item(t, k as usize, true);
            }
        }
    }

    /// `getPlayerNum`: players on the tiles within `r` of (x, y).
    fn player_num(&self, m: usize, x: i16, y: i16, r: i16) -> i32 {
        let mut n = 0;
        for xx in x - r..=x + r {
            for yy in y - r..=y + r {
                let (o, t) = self.maps[m].get_owner(xx, yy);
                if o != 0 && t == OWNERTYPE_PLAYER {
                    n += 1;
                }
            }
        }
        n
    }

    /// `ShowClientMsg`: a "Server" GM chat line (at most 50 bytes).
    pub fn show_client_msg(&mut self, h: usize, msg: &[u8]) {
        let n = msg.len().min(50);
        let mut b = vec![0u8; 22 + n];
        let mut w = W(&mut b);
        w.u32(0, MSGID_COMMAND_CHATMSG);
        // fix: the C++ copied 8 bytes from the 7-byte literal "Server"
        w.bytes(10, b"Server");
        w.u8(20, CHAT_GM);
        w.bytes(21, &msg[..n]);
        self.send(h, &b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas() {
        // circle 1 at level 1: 300 + 5*1 (two circles below? no: lm 0 < circle 1 -> penalty)
        assert_eq!(level_adjust(1, 10), 0);
        assert_eq!(level_adjust(1, 30), 10);
        // circle 3 at level 15: d 2, p 2*5... ML_PENALTY[3] = 8 -> p 16, v4 = 5/20*16 = 4 -> -(16-4)
        assert_eq!(level_adjust(3, 15), -12);
        assert_eq!(cast_probability(0, 10, 30, 0, None), 310);
        assert_eq!(cast_probability(90, 200, 30, 3, None), {
            // circle 10, lm 3: d 7, p 7*40 = 280, v4 = 0 -> -280; 40 + 75 - 280 < 0 -> 1
            1
        });
        assert_eq!(cast_probability(20, 60, 25, 2, Some(2)), {
            let r = 200 + 5 + level_adjust(3, 25);
            r - r / 12 + 6
        });
        assert_eq!(mana_cost(15, false, 0), 15);
        assert_eq!(mana_cost(15, true, 0), 16);
        assert_eq!(mana_cost(27, true, 20), 23);
        assert_eq!(mana_cost(1, false, 80), 1);
        assert_eq!(magic_hit_ratio(100, 60, 30, 85, 5), 1 + 10000); // 100 + 10 - 216 + 5 < 1
        assert_eq!(magic_hit_ratio(310, 60, 30, 5, 5), 310 + 10 + 10 + 5);
        assert_eq!(magic_dest_ratio(100, 50), 99);
        assert_eq!(magic_dest_ratio(10100, 200), 25);
        assert_eq!(magic_dest_ratio(10, 100), 15);
        assert_eq!(magic_dest_ratio(60, 0), 99);
        // 10 dmg, mag 33: 33/3.3 = 10 % -> 11
        assert_eq!(magic_scale(10, 33), 11);
        assert_eq!(magic_scale(8, 0), 8);
        assert_eq!((weather_bonus(10, 2), weather_bonus(20, 1), weather_bonus(10, 0), weather_bonus(0, 3)), (1, -1, 0, 0));
        assert_eq!(get_point2(10, 10, 20, 10, 2), (12, 10));
        assert_eq!(get_point2(10, 10, 13, 16, 3), (11, 13));
        assert_eq!(get_point2(5, 5, 6, 5, 9), (7, 5));
        let f: Vec<Vec<u8>> = "20 Fire-Ball 3 0 0 27 3 3 2 6 2 2 6 2 0 0 26 100 1 3".split(' ').map(|s| s.as_bytes().to_vec()).collect();
        let s = Spell::parse(&f).unwrap();
        assert_eq!((s.ty, s.mana, s.h, s.v, s.val[1], s.category, s.attr), (3, 27, 1, 1, 6, 1, 3));
        assert_eq!((s.main(1), s.linear(0)), ((2, 6, 3), (2, 6, 2)));
    }
}
