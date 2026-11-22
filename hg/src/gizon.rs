//! The options of a character at the level cap (HG.cpp): StateChangeHandler (MSGID_STATECHANGEPOINT: three stat points
//! moved for one Gizon / majestic point), ChangeState, CheckMagicInt and GetDKItemHandler (MSGID_REQUEST_DKITEM: Gail's
//! Dark Knight / Dark Mage armour and weapons).
use crate::client::{Item, MAXMAGICTYPE};
use crate::consts::*;
use crate::game::Game;
use crate::log::log;
use crate::util::Rd;

/// `ChangeState`: the stat a MJSTAT_* byte names (an index of the array above), None for any other byte.
pub fn change_state(code: u8) -> Option<usize> {
    [MJSTAT_STR, MJSTAT_VIT, MJSTAT_DEX, MJSTAT_INT, MJSTAT_MAG, MJSTAT_CHR].iter().position(|&m| m as u8 == code)
}

/// The checks of `StateChangeHandler` after the three bytes were read: `base` is (str, vit, dex, int, mag, chr),
/// `delta` the points taken off each. The C++ answered none of the refusals; the one for a stat sum other than the cap's
/// 607 it did not even log.
pub fn respec_check(base: [i32; 6], delta: [i32; 6], guild_master: bool, limit: i32) -> bool {
    // a guild master keeps the 20 charisma the guild needs
    !(guild_master && base[5] - delta[5] < 20)
        && base.iter().sum::<i32>() == 179 * 3 + 70
        && delta.iter().all(|&d| d >= 0)
        && delta.iter().sum::<i32>() == 3
        && base.iter().zip(&delta).all(|(b, d)| b - d <= limit && b - d >= 10)
}

impl Game {
    /// `StateChangeHandler` (MSGID_STATECHANGEPOINT: three bytes after the header, each a MJSTAT_* stat to lower by 1):
    /// a character whose six stats add up to the level cap's 607 trades three points for one Gizon point. The three
    /// points go back to the level-up pool (at least 3 there) and are spent again with MSGID_LEVELUPSETTINGS.
    pub fn state_change(&mut self, h: usize, d: &[u8]) {
        let limit = self.settings.char_stat_limit;
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        // fix: the C++ answered none of its refusals (SendNotifyMsg has no case for NOTIFY_STATECHANGE_FAILED), so the
        // client kept the stat changes it had made on its own side; here every refusal is answered
        let fail = |g: &mut Game| g.notify(h, NOTIFY_STATECHANGE_FAILED, &[], None);
        if c.gizon_item_upgrade_left <= 0 {
            return fail(self);
        }
        let r = Rd::new(d);
        let mut delta = [0i32; 6];
        for k in 0..3 {
            match change_state(r.u8(6 + k)) {
                Some(s) => delta[s] += 1,
                None => return fail(self),
            }
        }
        let c = self.client(h).unwrap();
        let base = [c.str_, c.vit, c.dex, c.int_, c.mag, c.range];
        if !respec_check(base, delta, c.guild_rank == 0, limit) {
            return fail(self);
        }
        let c = self.client_mut(h).unwrap();
        if c.lu_pool < 3 {
            c.lu_pool = 3;
        }
        c.gizon_item_upgrade_left -= 1;
        // SetStr: the bars are clamped (and sent) with the new strength, the vitality comes after
        c.str_ -= delta[0];
        let (mut hp, mut sp) = (false, false);
        if c.hp > c.get_max_hp() {
            (c.hp, hp) = (c.get_max_hp(), true);
        }
        if c.sp > c.get_max_sp() {
            (c.sp, sp) = (c.get_max_sp(), true);
        }
        for (f, t) in [(hp, NOTIFY_HP), (sp, NOTIFY_SP)] {
            if f {
                self.notify(h, t, &[], None);
            }
        }
        let c = self.client_mut(h).unwrap();
        c.vit -= delta[1];
        if c.hp > c.get_max_hp() {
            c.hp = c.get_max_hp();
        }
        c.dex -= delta[2];
        // SetInt, then SetMag: the mana is clamped (and sent) after each
        c.int_ -= delta[3];
        let mut mp = c.mp > c.get_max_mp();
        if mp {
            c.mp = c.get_max_mp();
            self.notify(h, NOTIFY_MP, &[], None);
        }
        let cleared = delta[3] > 0 && self.check_magic_int(h);
        let c = self.client_mut(h).unwrap();
        c.mag -= delta[4];
        mp = c.mp > c.get_max_mp();
        if mp {
            c.mp = c.get_max_mp();
            self.notify(h, NOTIFY_MP, &[], None);
        }
        let c = self.client_mut(h).unwrap();
        c.range -= delta[5];
        self.validate_skills(h);
        let left = self.client(h).unwrap().gizon_item_upgrade_left;
        self.notify(h, NOTIFY_GIZONITEMUPGRADELEFT, &[left], None);
        self.notify(h, NOTIFY_SETTING_SUCCESS, &[], None);
        if cleared {
            // fix: the C++ cleared the spells the new intelligence cannot cast without telling the client, whose spell
            // list kept showing them until the next login (NOTIFY_SPELL_SKILL is the silent reload of both lists)
            self.notify(h, NOTIFY_SPELL_SKILL, &[], None);
        }
    }

    /// `CheckMagicInt`: the spells whose intelligence limit is above the player's are forgotten. True when one was.
    fn check_magic_int(&mut self, h: usize) -> bool {
        let int = self.client(h).unwrap().get_int();
        let mut cleared = false;
        for i in 0..MAXMAGICTYPE {
            let limit = self.data.magic.get(i).and_then(|r| r.as_ref()).and_then(|f| f.get(16)).map(|t| crate::util::atoi(t));
            if limit.is_some_and(|l| l > int) {
                let c = self.client_mut(h).unwrap();
                cleared |= c.magic_mastery[i] != 0;
                c.magic_mastery[i] = 0;
            }
        }
        cleared
    }

    /// `GetDKItemHandler` (MSGID_REQUEST_DKITEM: Gail's name at 6, the item number as an int at 26): the Dark Knight
    /// (1-4: helm, hauberk, plate mail, leggings; 11-13: rapier, great sword, flameberge) or Dark Mage (5-9: hat,
    /// hauberk, robe, chain mail, leggings; 10: magic staff) piece for the player's sex, bound to him (ITET_UNIQUE_OWNER).
    /// The C++ asks for nothing in return and checks nothing (the client only offers it at the level cap).
    pub fn get_dk_item(&mut self, h: usize, d: &[u8]) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        if self.item_space_left(h) == 0 {
            self.send_cannot_carry(h);
            return;
        }
        let which = Rd::new(d).u32(26) as i32;
        let sex = c.sex;
        let name = match dk_item_name(which, sex) {
            Some(n) => n,
            None => {
                log("Gail asked to create a wrong item!");
                // fix: the C++ went on with an uninitialised item name (a sex that is neither MALE nor FEMALE did the same)
                return;
            }
        };
        let Some(cfg) = self.data.item_by_name(name.as_bytes()).cloned() else { return };
        let mut it = Item::new(&cfg);
        (it.touch_effect_type, it.touch_effect_value) = (ITET_UNIQUE_OWNER as i16, self.client(h).unwrap().char_id_num);
        self.add_item(h, it);
    }
}

/// The item of GetDKItemHandler's switch: `which` 1..13, `sex` 1 (male) / 2 (female) (the armour has a (M) and a (W) form).
pub fn dk_item_name(which: i32, sex: u8) -> Option<String> {
    let base = match which {
        1 => "DKFullHelm",
        2 => "DKHauberk",
        3 => "DKPlateMail",
        4 => "DKLeggings",
        5 => "DMHat",
        6 => "DMHauberk",
        7 => "DMRobe",
        8 => "DMChainMail",
        9 => "DMLeggings",
        10 => return Some("DMMagicStaff".into()),
        11 => return Some("DKRapier".into()),
        12 => return Some("DKGreatSword".into()),
        13 => return Some("DKFlameberg".into()),
        _ => return None,
    };
    match sex {
        1 => Some(format!("{base}(M)")),
        2 => Some(format!("{base}(W)")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_bytes() {
        assert_eq!(change_state(MJSTAT_STR as u8), Some(0));
        assert_eq!(change_state(MJSTAT_CHR as u8), Some(5));
        assert_eq!(change_state(0), None);
        assert_eq!(change_state(MJSTAT_CHR as u8 + 1), None);
    }

    #[test]
    fn respec_rules() {
        let base = [200, 150, 100, 57, 50, 50]; // 607
        assert!(respec_check(base, [1, 1, 1, 0, 0, 0], false, 200));
        // not 3 points / a negative one / below 10 / above the limit after the change
        assert!(!respec_check(base, [1, 1, 0, 0, 0, 0], false, 200));
        assert!(!respec_check(base, [4, -1, 0, 0, 0, 0], false, 200));
        assert!(respec_check(base, [0, 0, 0, 3, 0, 0], false, 200));
        assert!(!respec_check([200, 150, 100, 10, 97, 50], [0, 0, 0, 3, 0, 0], false, 200));
        assert!(!respec_check([201, 149, 100, 57, 50, 50], [0, 3, 0, 0, 0, 0], false, 200));
        // the cap's 607 only; a guild master keeps 20 charisma
        assert!(!respec_check([100, 100, 100, 100, 100, 100], [1, 1, 1, 0, 0, 0], false, 200));
        assert!(respec_check(base, [0, 0, 0, 0, 0, 3], true, 200));
        assert!(!respec_check([200, 150, 100, 85, 50, 22], [0, 0, 0, 0, 0, 3], true, 200));
    }

    #[test]
    fn dk_items() {
        assert_eq!(dk_item_name(1, 1).as_deref(), Some("DKFullHelm(M)"));
        assert_eq!(dk_item_name(9, 2).as_deref(), Some("DMLeggings(W)"));
        assert_eq!(dk_item_name(10, 0).as_deref(), Some("DMMagicStaff"));
        assert_eq!(dk_item_name(13, 7).as_deref(), Some("DKFlameberg"));
        assert_eq!(dk_item_name(14, 1), None);
        assert_eq!(dk_item_name(1, 0), None);
    }
}
