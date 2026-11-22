//! The two reward commands of the client (HG.cpp): GetRewardMoneyHandler (COMMONTYPE_REQ_GETREWARDMONEY: the reward gold
//! of enemy kills and PK captures handed over as a Gold item) and GetHeroMantleHandler (COMMONTYPE_REQ_GETHEROMANTLE: the
//! hero cape and armour, paid with enemy kills and contribution).
use crate::client::Item;
use crate::consts::*;
use crate::game::Game;
use crate::log::log;

/// `HeroItemID[HR_MAX][MAXSIDES-1][2]` (Shared/common.h): per piece (cape, helm, cap, plate, robe, hauberk, leggings),
/// per side (Aresden, Elvine, Istria), per sex (male, female).
pub const HERO_ITEM_ID: [[[i32; 2]; 3]; 7] = [
    [[400, 400], [401, 401], [1009, 1009]],
    [[403, 404], [405, 406], [1010, 1011]],
    [[407, 408], [409, 410], [1012, 1013]],
    [[411, 412], [413, 414], [1014, 1015]],
    [[415, 416], [417, 418], [1016, 1017]],
    [[419, 420], [421, 422], [1018, 1019]],
    [[423, 424], [425, 426], [1020, 1021]],
];

/// `heroPrice[HR_MAX]`: (enemy kills, contribution) per piece.
pub const HERO_PRICE: [(i32, i32); 7] = [(300, 0), (150, 20), (100, 20), (300, 30), (200, 20), (100, 10), (150, 15)];

/// The Gold a reward of `reward` can be paid with when `weight_left` (half the free load) is available, and what stays:
/// `weight_per_gold` is iGetItemWeight(Gold, 1).
pub fn reward_split(weight_left: i32, weight_per_gold: i32, reward: i32) -> (i32, i32) {
    if weight_left / weight_per_gold >= reward {
        (reward, 0)
    } else {
        let n = weight_left / weight_per_gold;
        (n, reward - n)
    }
}

impl Game {
    /// `GetRewardMoneyHandler`: m_iRewardGold as a Gold item, as much as half the free load carries, then NOTIFY_REWARDGOLD.
    pub fn get_reward_money(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if !c.is_init_complete {
            return;
        }
        // (iCalcTotalWeight refreshes the load first)
        self.calc_total_weight(h);
        let c = self.client(h).unwrap();
        let left = (c.get_str() * 500 + c.level * 500 - c.cur_weight_load) / 2;
        if left <= 0 || c.reward_gold <= 0 {
            // fix: the C++ went on with no reward gold and handed over a Gold item of count 0 (an empty slot of the bag)
            return;
        }
        let Some(cfg) = self.data.item_by_name(b"Gold").cloned() else { return };
        let mut it = Item::new(&cfg);
        let (n, rest) = reward_split(left, it.weight_of(1), c.reward_gold);
        it.count = n as u32;
        if self.add_client_item_list(h, &it) {
            self.client_mut(h).unwrap().reward_gold = rest;
            self.send_item_obtained(h, &it);
            self.notify(h, NOTIFY_REWARDGOLD, &[], None);
        }
    }

    /// `GetHeroMantleHandler` (`req` = the piece, an HR_* index): needs 100 enemy kills and a side, then the piece's price.
    pub fn get_hero_mantle(&mut self, h: usize, req: i32) {
        let Some(c) = self.client(h) else { return };
        if c.enemy_kill_count < 100 || c.is_neutral() {
            return;
        }
        if self.item_space_left(h) == 0 {
            self.send_cannot_carry(h);
            return;
        }
        if !(0..HERO_ITEM_ID.len() as i32).contains(&req) {
            return;
        }
        let c = self.client(h).unwrap();
        // fix: the C++ indexed the table with side - 1 and sex - 1 unchecked (the hostile side and a sex of 0 read other entries)
        if !(1..=3).contains(&c.side) || !(1..=2).contains(&c.sex) {
            return;
        }
        let id = HERO_ITEM_ID[req as usize][c.side as usize - 1][c.sex as usize - 1];
        let (ek, contrib) = HERO_PRICE[req as usize];
        // fix: the C++ compared the (signed) contribution with an unsigned price, so a negative contribution (the energy
        // sphere's -10) always paid and went further below zero
        if c.enemy_kill_count < ek || c.contribution < contrib {
            return;
        }
        let Some(name) = self.data.item_by_id(id).map(|f| f.name.clone()) else { return };
        let Some(cfg) = self.data.item_by_name(&name).cloned() else { return };
        let mut it = Item::new(&cfg);
        // (the C++ set these on the item after adding it: it is the very object in the bag)
        (it.touch_effect_type, it.touch_effect_value) = (ITET_UNIQUE_OWNER as i16, c.char_id_num);
        if self.add_client_item_list(h, &it) {
            let c = self.client_mut(h).unwrap();
            c.enemy_kill_count -= ek;
            c.contribution -= contrib;
            c.cur_weight_load = c.cur_weight_load.max(0);
            log(&format!(
                "(*) Get HeroItem : Char({}) Player-EK({}) Player-Contr({}) Hero Obtained({})",
                String::from_utf8_lossy(crate::util::cstr(&c.char_name)),
                c.enemy_kill_count,
                c.contribution,
                String::from_utf8_lossy(crate::util::cstr(&name))
            ));
            let ek_left = c.enemy_kill_count;
            self.send_item_obtained(h, &it);
            self.notify(h, NOTIFY_ENEMYKILLS, &[ek_left], None);
        } else {
            self.calc_total_weight(h);
            self.send_cannot_carry(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split() {
        // everything fits / only part of it / nothing
        assert_eq!(reward_split(500, 1, 120), (120, 0));
        assert_eq!(reward_split(100, 1, 120), (100, 20));
        assert_eq!(reward_split(100, 2, 120), (50, 70));
        assert_eq!(reward_split(1, 2, 120), (0, 120));
    }

    #[test]
    fn tables() {
        // the male / female ids of the helm: Aresden 403 / 404, Istria 1010 / 1011; the cape is the same for both sexes
        assert_eq!(HERO_ITEM_ID[1][0], [403, 404]);
        assert_eq!(HERO_ITEM_ID[1][2], [1010, 1011]);
        assert_eq!(HERO_ITEM_ID[0][1], [401, 401]);
        assert_eq!(HERO_PRICE[3], (300, 30));
    }
}
