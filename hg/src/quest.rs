//! Quests (HG.cpp ~26531 NpcTalkHandler, ~28209.._ClearQuestStatus, CancelQuestHandler ~33031, the monster-hunt
//! counter of NpcKilledHandler ~7136, the move check of iClientMotion_Move_Handler ~979). Only the city hall (npc
//! 4) gives quests; the guard (21) answers with a fixed line; every other npc says "nothing for you" (130 + npc).
//! Only MONSTERHUNT and GOPLACE can complete (as in the C++: the other types are accepted but never finish).
use crate::client::{Item, ARESDEN, ELVINE, NEUTRAL, SIDE_MAP};
use crate::consts::*;
use crate::game::Game;
use crate::util::{atoi, cstr};

pub const QUESTTYPE_MONSTERHUNT: i32 = 1;
pub const QUESTTYPE_GOPLACE: i32 = 7;
/// the name the C++ sends for an exp reward: "경험치" in CP949, space padded to 20
const EXP_NAME: &[u8; 20] = b"\xb0\xe6\xc7\xe8\xc4\xa1              ";

/// One Quest.cfg record (CQuest), from the 27 raw tokens `quest = n side type ...`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuestCfg {
    pub side: i32,
    pub ty: i32,
    pub target_type: i32,
    pub max_count: i32,
    pub from: i32,
    pub min_level: i32,
    pub max_level: i32,
    pub req_skill: i32,
    pub req_skill_level: i32,
    pub assign_type: i32,
    /// slots 1..3 (index 0 unused, as m_iRewardType[4])
    pub reward_type: [i32; 4],
    pub reward_amount: [i32; 4],
    pub contribution: i32,
    pub contribution_limit: i32,
    pub response_mode: i32,
    pub target_name: Vec<u8>,
    pub x: i32,
    pub y: i32,
    pub range: i32,
    pub quest_id: i32,
    pub req_contribution: i32,
}

impl QuestCfg {
    pub fn parse(r: &[Vec<u8>]) -> QuestCfg {
        let n = |i: usize| atoi(&r[i]);
        QuestCfg {
            side: n(1) as i8 as i32, // m_side is a char
            ty: n(2),
            target_type: n(3),
            max_count: n(4),
            from: n(5),
            min_level: n(6),
            max_level: n(7),
            req_skill: n(8),
            req_skill_level: n(9),
            assign_type: n(11),
            reward_type: [0, n(12), n(14), n(16)],
            reward_amount: [0, n(13), n(15), n(17)],
            contribution: n(18),
            contribution_limit: n(19),
            response_mode: n(20),
            target_name: r[21][..r[21].len().min(20)].to_vec(),
            x: n(22),
            y: n(23),
            range: n(24),
            quest_id: n(25),
            req_contribution: n(26),
        }
    }
}

/// a name compared as the C++ memcmp of a zero-padded buffer of `n` bytes
fn padded(s: &[u8], n: usize) -> Vec<u8> {
    let mut v = cstr(s).to_vec();
    v.resize(n.max(v.len()), 0);
    v.truncate(n);
    v
}

/// what NpcTalkHandler sends after a successful search
struct Offer {
    quest: i32,
    cfg: QuestCfg,
    reward_type: i32,
    reward_amount: i32,
}

impl Game {
    pub fn quest_cfg(&self, i: i32) -> Option<QuestCfg> {
        let r = self.data.quests.get(usize::try_from(i).ok()?)?.as_ref()?;
        Some(QuestCfg::parse(r))
    }

    /// `NpcTalkHandler` (COMMONTYPE_TALKTONPC, iV1 = the npc type)
    pub fn npc_talk(&mut self, h: usize, who: i32) {
        if self.client(h).is_none() {
            return;
        }
        let res = match who {
            4 => self.talk_cityhall(h),
            21 => {
                if self.talk_guard(h) {
                    return;
                }
                Err(0)
            }
            _ => Err(0),
        };
        match res {
            Ok(o) => {
                // fix: the C++ read m_pItemConfigList[iRewardType]->m_cName unchecked (crash on a missing item)
                let name = if o.reward_type > 1 {
                    self.data.items.get(o.reward_type as usize).and_then(|i| i.as_ref()).map(|i| i.name.clone()).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let c = self.client_mut(h).unwrap();
                (c.asked_quest, c.quest_reward_type, c.quest_reward_amount) = (o.quest, o.reward_type, o.reward_amount);
                let q = &o.cfg;
                let v = [q.ty, q.response_mode, o.reward_amount, q.contribution, q.target_type, q.max_count, q.x, q.y, q.range];
                let target = q.target_name.clone();
                self.notify2(h, NOTIFY_NPCTALK, &v, Some(&name), Some(&target));
            }
            Err(0) => self.notify(h, NOTIFY_NPCTALK, &[who + 130], None),
            Err(e @ -4..=-1) => self.notify(h, NOTIFY_NPCTALK, &[e.abs() + 100], None),
            Err(_) => {} // -5: the reward was handed out
        }
    }

    /// `_iTalkToNpcResult_Guard`: true when the guard answered (the C++ codes 1000..1006)
    fn talk_guard(&mut self, h: usize) -> bool {
        let c = self.client(h).unwrap();
        let on = |s: &[u8]| c.map_name.starts_with(s);
        let (a, e, n) = (SIDE_MAP[ARESDEN as usize], SIDE_MAP[ELVINE as usize], SIDE_MAP[NEUTRAL as usize]);
        let line = match c.side {
            ARESDEN if on(a) => 200,
            ARESDEN if on(e) => 201,
            ELVINE if on(a) => 202,
            ELVINE if on(e) => 203,
            NEUTRAL if on(a) => 204,
            NEUTRAL if on(e) => 205,
            NEUTRAL if on(n) => 206,
            _ => return false,
        };
        self.notify(h, NOTIFY_NPCTALK, &[line], None);
        true
    }

    /// `_iTalkToNpcResult_Cityhall`: a new quest, a reward for a completed one, or a return code (-1 not done
    /// yet, -2 not in a safe town, -3 criminal, -4 nothing, -5 reward handled).
    fn talk_cityhall(&mut self, h: usize) -> Result<Offer, i32> {
        let c = self.client(h).unwrap();
        if c.quest != 0 {
            let Some(q) = self.quest_cfg(c.quest) else { return Err(-4) };
            if q.from != 4 {
                return Err(-4);
            }
            if !c.is_quest_completed {
                return Err(-1);
            }
            self.quest_reward(h, &q);
            return Err(-5);
        }
        if !c.is_on_town_nonpk {
            return Err(-2);
        }
        if c.pk_count > 0 {
            return Err(-3);
        }
        self.search_for_quest(h, 4).ok_or(-4)
    }

    /// the reward part of `_iTalkToNpcResult_Cityhall`
    fn quest_reward(&mut self, h: usize, q: &QuestCfg) {
        let c = self.client(h).unwrap();
        let (rt, ra) = (c.quest_reward_type, c.quest_reward_amount);
        let item_cfg = (rt > 0).then(|| self.data.items.get(rt as usize).and_then(|i| i.clone())).flatten();
        let (amount, name) = if let Some(cfg) = item_cfg {
            let mut it = Item::new(&cfg);
            it.count = ra as u32;
            if !self.item_receive_condition(h, &it) {
                self.send_cannot_carry(h);
                let cont = self.client(h).unwrap().contribution;
                self.notify(h, NOTIFY_QUESTREWARD, &[4, 0, ra, cont], Some(&cfg.name));
                return; // the quest stays completed: the reward can be asked again
            }
            self.add_client_item_list(h, &it);
            self.send_item_obtained(h, &it);
            (ra, cfg.name.clone())
        } else if rt == -1 {
            self.get_exp(h, ra, false);
            (ra, EXP_NAME.to_vec())
        } else if rt == -2 {
            let lv = self.client(h).unwrap().level;
            let exp = self.dice(1, 10 * lv) * ra;
            self.get_exp(h, exp, false);
            (exp, EXP_NAME.to_vec())
        } else {
            (0, vec![b' '; 20])
        };
        let c = self.client_mut(h).unwrap();
        c.contribution += q.contribution;
        let cont = c.contribution;
        self.notify(h, NOTIFY_QUESTREWARD, &[4, 1, amount, cont], Some(&name));
        self.clear_quest_status(h);
    }

    /// `_bCheckItemReceiveCondition`: the item's weight fits and there is an empty bag slot
    fn item_receive_condition(&self, h: usize, it: &Item) -> bool {
        let c = self.client(h).unwrap();
        let max_load = c.get_str() * 500 + c.level * 500;
        c.cur_weight_load + it.weight_of(it.count as i64) <= max_load && c.items.iter().any(|i| i.is_none())
    }

    /// `__iSearchForQuest`: a random quest of this npc that fits the player.
    fn search_for_quest(&mut self, h: usize, who: i32) -> Option<Offer> {
        let c = self.client(h).unwrap();
        let crusade = self.cr.mode;
        let fits: Vec<(i32, QuestCfg)> = (1..MAXQUESTTYPE)
            .filter_map(|i| self.quest_cfg(i).map(|q| (i, q)))
            .filter(|(_, q)| {
                q.from == who
                    && q.side == c.side
                    && q.min_level <= c.level
                    && q.max_level >= c.level
                    && q.req_contribution <= c.contribution
                    // fix: the C++ indexed m_cSkillMastery with the config's skill number unchecked
                    && (q.req_skill == -1 || c.skill_mastery.get(q.req_skill as usize).is_some_and(|&m| m as i32 >= q.req_skill_level))
                    && (crusade == (q.assign_type == 1))
                    && q.contribution_limit >= c.contribution
            })
            .collect();
        if fits.is_empty() {
            return None;
        }
        let k = self.dice(1, fits.len() as i32) - 1;
        let (quest, cfg) = fits[k as usize].clone();
        let r = self.dice(1, 3) as usize;
        Some(Offer { quest, reward_type: cfg.reward_type[r], reward_amount: cfg.reward_amount[r], cfg })
    }

    /// `QuestAcceptedHandler` (COMMONTYPE_QUESTACCEPTED)
    pub fn quest_accepted(&mut self, h: usize) {
        let Some(c) = self.client_mut(h) else { return };
        // fix: the C++ read m_pQuestConfigList[m_iAskedQuest] unchecked (crash on an accept without a talk) and
        // never cleared the asked quest, so a repeated accept restarted a quest without the search's level, side
        // and contribution-limit checks
        let asked = std::mem::take(&mut c.asked_quest);
        let Some(q) = self.quest_cfg(asked).filter(|_| asked != 0) else { return };
        if q.assign_type == 1 && q.ty == 10 {
            // QUESTTYPE_STRATEGICSTRIKE (crusade only): a teleport to the target instead of a quest
            self.clear_quest_status(h);
            self.request_teleport(h, 2, Some(&q.target_name), q.x as i16, q.y as i16);
            return;
        }
        let c = self.client_mut(h).unwrap();
        c.quest = asked;
        c.quest_id = q.quest_id as u32;
        c.cur_quest_count = 0;
        c.is_quest_completed = false;
        self.check_quest_environment(h);
        self.send_quest_contents(h);
    }

    /// `CancelQuestHandler` (COMMONTYPE_REQUEST_CANCELQUEST)
    pub fn cancel_quest(&mut self, h: usize) {
        if self.client(h).is_none() {
            return;
        }
        self.clear_quest_status(h);
        self.notify(h, NOTIFY_QUESTABORTED, &[], None);
    }

    /// `_ClearQuestStatus`
    pub fn clear_quest_status(&mut self, h: usize) {
        if let Some(c) = self.client_mut(h) {
            (c.quest, c.quest_id, c.quest_reward_type, c.quest_reward_amount, c.is_quest_completed) = (0, 0, 0, 0, false);
        }
    }

    /// `_SendQuestContents`
    pub fn send_quest_contents(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        if c.quest == 0 {
            self.notify(h, NOTIFY_QUESTCONTENTS, &[], None);
            return;
        }
        let Some(q) = self.quest_cfg(c.quest) else { return };
        let v = [q.from, q.ty, q.contribution, q.target_type, q.max_count, q.x, q.y, q.range, c.is_quest_completed as i32];
        let remain = q.max_count - c.cur_quest_count;
        self.notify(h, NOTIFY_QUESTCONTENTS, &v, Some(&q.target_name));
        self.notify(h, NOTIFY_QUESTCOUNTER, &[remain], None);
    }

    /// `_CheckQuestEnvironment` (map entry, accept): drops an outdated quest, sets the location flag.
    pub fn check_quest_environment(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        let i = c.quest;
        if i == 0 {
            return;
        }
        let q = self.quest_cfg(i);
        // quests 35..40 are always dropped (as the C++)
        if (35..=40).contains(&i) || q.as_ref().is_some_and(|q| q.quest_id as u32 != c.quest_id) {
            let c = self.client_mut(h).unwrap();
            (c.quest, c.quest_id, c.quest_reward_amount, c.quest_reward_type) = (0, 0, 0, 0);
            self.notify(h, NOTIFY_QUESTABORTED, &[], None);
            return;
        }
        let Some(q) = q else { return };
        if q.ty == QUESTTYPE_MONSTERHUNT || q.ty == QUESTTYPE_GOPLACE {
            let on = padded(&self.maps[c.map_index].name, 10) == padded(&q.target_name, 10);
            self.client_mut(h).unwrap().quest_match_loc = on;
        }
    }

    /// `_bCheckIsQuestCompleted`
    pub fn check_quest_completed(&mut self, h: usize) -> bool {
        let Some(c) = self.client(h) else { return false };
        if c.is_quest_completed || c.quest == 0 {
            return false;
        }
        let Some(q) = self.quest_cfg(c.quest) else { return false };
        let (x, y) = (c.x as i32, c.y as i32);
        let done = c.quest_match_loc
            && match q.ty {
                QUESTTYPE_MONSTERHUNT => c.cur_quest_count >= q.max_count,
                QUESTTYPE_GOPLACE => (q.x - q.range..=q.x + q.range).contains(&x) && (q.y - q.range..=q.y + q.range).contains(&y),
                _ => false,
            };
        if done {
            self.client_mut(h).unwrap().is_quest_completed = true;
            self.notify(h, NOTIFY_QUESTCOMPLETED, &[], None);
        }
        done
    }

    /// NpcKilledHandler's monster-hunt counter (the killer only)
    pub fn quest_kill(&mut self, h: usize, npc_type: i32) {
        let Some(c) = self.client(h) else { return };
        let Some(q) = self.quest_cfg(c.quest).filter(|_| c.quest != 0) else { return };
        if q.ty != QUESTTYPE_MONSTERHUNT || !c.quest_match_loc || q.target_type != npc_type {
            return;
        }
        let c = self.client_mut(h).unwrap();
        c.cur_quest_count += 1;
        let remain = q.max_count - c.cur_quest_count;
        self.notify(h, NOTIFY_QUESTCOUNTER, &[remain], None);
        self.check_quest_completed(h);
    }

    /// InitPlayerData's tail: the counter and the completion check for a quest in progress
    pub fn quest_init(&mut self, h: usize) {
        let Some(c) = self.client(h) else { return };
        // fix: the C++ read m_pQuestConfigList[m_iQuest] unchecked here (crash on a quest missing from Quest.cfg)
        let Some(q) = self.quest_cfg(c.quest).filter(|_| c.quest != 0) else { return };
        let remain = q.max_count - c.cur_quest_count;
        self.notify(h, NOTIFY_QUESTCOUNTER, &[remain], None);
        self.check_quest_completed(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quest_cfg_line() {
        let line = b"quest\t=\t1\t1\t1\t56\t20\t4\t11\t20\t-1\t-1\t-1\t-1\t-1\t450\t90\t550\t90\t500\t2\t20\t1\tarefarm\t0\t0\t0\t354\t0";
        let toks: Vec<Vec<u8>> = line.split(|&b| b == b'\t').skip(2).map(|t| t.to_vec()).collect();
        assert_eq!(toks.len(), 27);
        let q = QuestCfg::parse(&toks);
        assert_eq!((q.side, q.ty, q.target_type, q.max_count, q.from), (1, 1, 56, 20, 4));
        assert_eq!((q.min_level, q.max_level, q.req_skill, q.assign_type), (11, 20, -1, -1));
        assert_eq!(q.reward_type, [0, -1, 90, 90]);
        assert_eq!(q.reward_amount, [0, 450, 550, 500]);
        assert_eq!((q.contribution, q.contribution_limit, q.response_mode), (2, 20, 1));
        assert_eq!(q.target_name, b"arefarm");
        assert_eq!((q.quest_id, q.req_contribution), (354, 0));
    }

    #[test]
    fn padded_compare_is_the_memcmp_of_10() {
        assert_eq!(padded(b"arefarm", 10), padded(b"arefarm\0\0", 10));
        assert_ne!(padded(b"arefarm", 10), padded(b"arefarm2", 10));
        assert_eq!(padded(b"middleland1", 10), padded(b"middleland2", 10)); // only 10 bytes count
    }
}
