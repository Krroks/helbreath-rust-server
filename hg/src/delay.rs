//! Delay events: `RegisterDelayEvent`, `RemoveFromDelayEventList`, `DelayEventProcessor` (every second) and
//! `Unit::RemoveMagicEffect` / `SetMagicFlag`. MAGICRELEASE (ice, hold, invisibility...),
//! ANCIENT_TABLET (slates), USEITEM_SKILL (fishing rods) and the three meteor events (crusade.rs).
use crate::client::*;
use crate::combat::{MAGICPROTECT_AMP, MAGICPROTECT_DS, MAGICPROTECT_GDS, MAGICPROTECT_PFA, MAGICPROTECT_PFM};
use crate::consts::*;
use crate::game::Game;
use crate::map::{OWNERTYPE_NPC, OWNERTYPE_PLAYER};

pub const MAGICTYPE_ICE_LINEAR: i32 = 27;
const STATUS_ILLUSIONMOVEMENT: i32 = 1 << 21;

#[derive(Clone, Debug)]
pub struct DelayEvent {
    pub ty: i32,
    pub effect: i32,
    pub trigger: u32,
    pub target: usize,
    pub tt: u8,
    /// USEITEM_SKILL: the map and tile, V1 (skill mastery) and V2 (the using time id)
    pub m: usize,
    pub dx: i16,
    pub dy: i16,
    pub v1: i32,
    pub v2: u32,
}

/// `Unit::SetMagicFlag`: the status bit a magic effect shows (None: the C++ returns without a broadcast).
pub fn magic_flag(effect: i32, status: i32) -> Option<i32> {
    Some(match effect {
        MAGICTYPE_INVISIBILITY => STATUS_INVISIBILITY,
        MAGICTYPE_ICE | MAGICTYPE_ICE_LINEAR => STATUS_FROZEN,
        MAGICTYPE_BERSERK => STATUS_BERSERK,
        MAGICTYPE_PROTECT => match status {
            MAGICPROTECT_PFA => STATUS_PFA,
            MAGICPROTECT_PFM | MAGICPROTECT_AMP => STATUS_PFM,
            MAGICPROTECT_DS | MAGICPROTECT_GDS => STATUS_DEFENSESHIELD,
            _ => 0,
        },
        MAGICTYPE_CONFUSE => match status {
            3 => STATUS_ILLUSION,
            4 => STATUS_ILLUSIONMOVEMENT,
            _ => 0,
        },
        _ => return None,
    })
}

impl Game {
    /// A unit's (magic effect status, status) for the Unit methods shared by players and NPCs.
    pub fn unit_mut(&mut self, h: usize, tt: u8) -> Option<(&mut [i32], &mut i32)> {
        match tt {
            OWNERTYPE_PLAYER => self.client_mut(h).map(|c| (&mut c.magic_effect_status[..], &mut c.status)),
            OWNERTYPE_NPC => self.npcs.get_mut(h).and_then(|n| n.as_mut()).map(|n| (&mut n.magic_effect_status[..], &mut n.status)),
            _ => None,
        }
    }

    pub fn unit_effect(&mut self, h: usize, tt: u8, effect: i32) -> i32 {
        self.unit_mut(h, tt).map_or(0, |(m, _)| m[effect as usize])
    }

    /// `Unit::SetStatusFlag` (with its broadcast)
    pub fn set_unit_status_flag(&mut self, h: usize, tt: u8, flag: i32, on: bool) {
        let Some((_, st)) = self.unit_mut(h, tt) else { return };
        if on {
            *st |= flag;
        } else {
            *st &= !flag;
        }
        self.send_event_near_unit(h, tt);
    }

    /// OBJECTNULLACTION to the viewers of a player or an NPC.
    pub fn send_event_near_unit(&mut self, h: usize, tt: u8) {
        if tt == OWNERTYPE_PLAYER {
            self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
        } else {
            self.send_event_near_npc(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16);
        }
    }

    /// `Unit::AddMagicEffect`: `secs` seconds of `effect` at level `kind`; false if it is already on.
    pub fn add_magic_effect(&mut self, h: usize, tt: u8, effect: i32, secs: u32, kind: i32) -> bool {
        let now = self.now();
        let Some((m, _)) = self.unit_mut(h, tt) else { return false };
        if m[effect as usize] != 0 {
            return false;
        }
        m[effect as usize] = kind;
        self.register_delay_event(DELAYEVENTTYPE_MAGICRELEASE, effect, now.wrapping_add(secs * 1000), h, tt);
        if tt == OWNERTYPE_PLAYER {
            self.notify(h, NOTIFY_MAGICEFFECTON, &[effect, kind], None);
        }
        if let Some(f) = magic_flag(effect, kind) {
            self.set_unit_status_flag(h, tt, f, true);
        }
        true
    }

    /// `RegisterDelayEvent` (the V1..V3 values and the map/position are unused by MAGICRELEASE)
    pub fn register_delay_event(&mut self, ty: i32, effect: i32, trigger: u32, target: usize, tt: u8) -> bool {
        if self.delay_events.len() >= MAXDELAYEVENTS as usize {
            return false;
        }
        self.delay_events.push(DelayEvent { ty, effect, trigger, target, tt, m: 0, dx: 0, dy: 0, v1: 0, v2: 0 });
        true
    }

    /// `RegisterDelayEvent` with the map/position and V1/V2 (DELAYEVENTTYPE_USEITEM_SKILL).
    pub fn register_delay_event_at(&mut self, e: DelayEvent) -> bool {
        if self.delay_events.len() >= MAXDELAYEVENTS as usize {
            return false;
        }
        self.delay_events.push(e);
        true
    }

    /// `RemoveFromDelayEventList`: `effect` 0 = every event of that unit.
    pub fn remove_delay_events(&mut self, target: usize, tt: u8, effect: i32) {
        self.delay_events.retain(|e| !(e.target == target && e.tt == tt && (effect == 0 || e.effect == effect)));
    }

    /// `DelayEventProcessor`
    pub fn delay_event_processor(&mut self) {
        let now = self.now();
        let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.delay_events).into_iter().partition(|e| now.wrapping_sub(e.trigger) as i32 > 0);
        self.delay_events = keep;
        for e in due {
            match e.ty {
                DELAYEVENTTYPE_MAGICRELEASE => {
                    self.remove_magic_effect(e.target, e.tt, e.effect);
                }
                DELAYEVENTTYPE_ANCIENT_TABLET => self.slate_expired(e.target, e.effect),
                DELAYEVENTTYPE_USEITEM_SKILL => self.skill_item_due(&e),
                DELAYEVENTTYPE_KILL_ABADDON | DELAYEVENTTYPE_END_APOCALYPSE => self.apocalypse_delay_event(e.ty, e.target),
                DELAYEVENTTYPE_METEORSTRIKE | DELAYEVENTTYPE_DOMETEORSTRIKEDAMAGE | DELAYEVENTTYPE_CALCMETEORSTRIKEEFFECT => self.meteor_delay_event(e.ty, e.m),
                _ => {}
            }
        }
    }

    /// `Unit::RemoveMagicEffect`
    pub fn remove_magic_effect(&mut self, h: usize, tt: u8, effect: i32) -> bool {
        let k = effect as usize;
        let status = match tt {
            OWNERTYPE_PLAYER => self.client(h).map_or(0, |c| c.magic_effect_status[k]),
            OWNERTYPE_NPC => self.npcs.get(h).and_then(|n| n.as_ref()).map_or(0, |n| n.magic_effect_status[k]),
            _ => 0,
        };
        if status == 0 {
            return false;
        }
        if tt == OWNERTYPE_PLAYER {
            self.notify(h, NOTIFY_MAGICEFFECTOFF, &[effect, status], None);
        }
        let flag = magic_flag(effect, status);
        match tt {
            OWNERTYPE_PLAYER => {
                let c = self.client_mut(h).unwrap();
                if let Some(f) = flag {
                    c.status &= !f;
                }
                c.magic_effect_status[k] = 0;
                if flag.is_some() {
                    self.send_event_near_type_a(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16, 0, 0, 0);
                }
            }
            _ => {
                let n = self.npcs[h].as_mut().unwrap();
                if let Some(f) = flag {
                    n.status &= !f;
                }
                n.magic_effect_status[k] = 0;
                if flag.is_some() {
                    self.send_event_near_npc(h, MSGID_EVENT_MOTION, OBJECTNULLACTION as u16);
                }
            }
        }
        // fix (gameplay-visible): the C++ never gave a polymorphed unit its type back (no status flag, so nothing
        // restored m_sOriginalType and nothing told the viewers): it stayed type 18. Here it is restored and shown.
        if effect == crate::magic::MAGICTYPE_POLYMORPH {
            let ty = match tt {
                OWNERTYPE_PLAYER => self.client_mut(h).map(|c| (&mut c.type_, c.original_type)),
                _ => self.npcs.get_mut(h).and_then(|n| n.as_mut()).map(|n| (&mut n.type_, n.original_type)),
            };
            if let Some((t, orig)) = ty.filter(|&(_, o)| o != 0) {
                *t = orig;
                self.send_event_near_unit(h, tt);
            }
        }
        self.remove_delay_events(h, tt, effect);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags() {
        assert_eq!(magic_flag(MAGICTYPE_ICE, 1), Some(STATUS_FROZEN));
        assert_eq!(magic_flag(MAGICTYPE_PROTECT, MAGICPROTECT_GDS), Some(STATUS_DEFENSESHIELD));
        assert_eq!(magic_flag(MAGICTYPE_HOLDOBJECT, 2), None);
    }
}
