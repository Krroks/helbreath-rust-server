//! The energy sphere (HG.cpp): EnergySphereProcessor and bCheckEnergySphereDestination. /energysphere creates an
//! Energy-Sphere NPC (action limit 4: melee hits knock it back) at a random creation point of the GM's map and picks
//! a goal point; knocking it next to the goal's aresden or elvine spot announces NOTIFY_ENERGYSPHEREGOALIN and
//! removes it (middleland: +5 contribution to the scorer of his own side's goal, -10 otherwise; other maps with
//! energy-sphere-auto-creation make a new one). The random creation of the 3 s timer is commented out in the C++,
//! so only the GM path is ported.
use crate::consts::*;
use crate::game::Game;
use crate::log::{log, log_file};
use crate::map::OWNERTYPE_PLAYER;
use crate::npc::MOVETYPE_RANDOM;

impl Game {
    /// `EnergySphereProcessor(TRUE, iClientH)`: `who` > 0 is a client (his map), <= 0 the map -who
    pub fn energy_sphere_create(&mut self, who: i32) {
        let m = if who > 0 {
            let Some(c) = self.client(who as usize) else { return };
            c.map_index
        } else {
            (-who) as usize
        };
        let Some(mp) = self.maps.get(m) else { return };
        if mp.es_cur_goal >= 0 {
            return;
        }
        let ci = self.rng.dice(1, mp.es_total_create) as usize;
        let Some(&(ty, x, y)) = self.maps[m].es_create.get(ci) else { return };
        if ty == 0 {
            return;
        }
        // fix: with no naming value left the C++ created no sphere but still set a goal and announced it
        let Some(nv) = self.maps[m].get_empty_naming_value() else { return };
        let name = format!("_{}{nv}", (m as u8 + 65) as char).into_bytes();
        let Some(i) = self.create_npc(b"Energy-Sphere", &name, m, 0, MOVETYPE_RANDOM, Some((x as i16, y as i16)), &[-1; 10], None, 0) else {
            self.maps[m].set_naming_value_empty(nv);
            return;
        };
        let (px, py) = { let n = self.npcs[i].as_ref().unwrap(); (n.x as i32, n.y as i32) };
        let gi = self.rng.dice(1, self.maps[m].es_total_goal) as usize;
        // as in the C++ an empty goal point leaves the sphere without a goal (it cannot score); fix: no read past
        // the 10 points
        if self.maps[m].es_goal.get(gi).is_none_or(|g| g.0 == 0) {
            return;
        }
        self.maps[m].es_cur_goal = gi as i32;
        for h in self.handles() {
            if self.client(h).unwrap().is_init_complete {
                self.notify(h, NOTIFY_ENERGYSPHERECREATED, &[px, py], None);
            }
        }
        let t = format!("(!) Admin Energy Sphere Created! ({px}, {py})");
        log(&t);
        log_file(&t);
    }

    /// `bCheckEnergySphereDestination`, after the knock-back of NPC `n` by the attacker (`ah`, `at`)
    pub fn energy_sphere_check(&mut self, n: usize, ah: usize, at: u8) -> bool {
        let Some(np) = self.npcs[n].as_ref() else { return false };
        let (m, sx, sy) = (np.map_index, np.x as i32, np.y as i32);
        let gi = self.maps[m].es_cur_goal;
        if gi < 0 {
            return false;
        }
        let middle = self.middleland_map() == Some(m);
        let (result, ax, ay, ex, ey) = self.maps[m].es_goal[gi as usize];
        // the aresden spot answers 2, the elvine one 1 (sV3)
        let Some(spot) = [(ax, ay, 1, 2), (ex, ey, 2, 1)].into_iter().find(|&(dx, dy, _, _)| (sx - dx).abs() <= 1 && (sy - dy).abs() <= 1) else { return false };
        self.maps[m].es_cur_goal = -1;
        if at == OWNERTYPE_PLAYER {
            if let Some(c) = self.client_mut(ah) {
                let (own, side, name) = (c.side == spot.2, c.side, c.char_name.clone());
                if middle {
                    c.contribution += if own { 5 } else { -10 };
                }
                if own {
                    log_file(&format!("(!) Energy sphere goal in ({})", crate::util::l1(&name)));
                }
                for h in self.handles() {
                    if self.client(h).unwrap().is_init_complete {
                        self.notify(h, NOTIFY_ENERGYSPHEREGOALIN, &[result, side as i32, spot.3], Some(&name));
                    }
                }
            }
        }
        if !middle && self.maps[m].is_energy_sphere_auto_creation {
            self.energy_sphere_create(-(m as i32));
        }
        true
    }
}
