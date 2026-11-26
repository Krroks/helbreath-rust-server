//! `CClient` (HG/char/Client.h/.cpp, Unit.h): one connected player. Fields keep the C++ names in snake_case.
use crate::gamedata::ItemCfg;
use crate::net::Tx;

pub const MAXITEMS: usize = 50;
pub const MAXBANKITEMS: usize = 120;
pub const MAXSKILLTYPE: usize = 11;
pub const MAXMAGICTYPE: usize = 100;
pub const MAXITEMEQUIPPOS: usize = 15;
pub const MAXMAGICEFFECTS: usize = 100;

// Side (Shared/common.h)
pub const NEUTRAL: i32 = 0;
pub const ARESDEN: i32 = 1;
pub const ELVINE: i32 = 2;
pub const ISTRIA: i32 = 3;
pub const SIDE_MAP: [&[u8]; 4] = [b"default", b"aresden", b"elvine", b"default2"];
pub const SIDE_MAP_JAIL: [&[u8]; 4] = [b"default", b"arejail", b"elvjail", b"default2"];
pub const SIDE_MAP_FARM: [&[u8]; 4] = [b"default", b"arefarm", b"elvfarm", b"default2"];
pub const SIDE_MAP_RES: [&[u8]; 4] = [b"default", b"resurr1", b"resurr2", b"default2"];

// StatusFlags (Shared/common.h)
pub const STATUS_INVISIBILITY: i32 = 1 << 4;
pub const STATUS_BERSERK: i32 = 1 << 5;
pub const STATUS_FROZEN: i32 = 1 << 6;
pub const STATUS_POISON: i32 = 1 << 7;
pub const STATUS_ANGELSTR: i32 = 1 << 12;
pub const STATUS_ANGELS: i32 = 0xF << 12; // STR, DEX, INT, MAG
pub const STATUS_GREENSLATE: i32 = 1 << 16;
pub const STATUS_PK: i32 = 1 << 20;
pub const STATUS_REDSLATE: i32 = 1 << 22;
pub const STATUS_BLUESLATE: i32 = 1 << 23;
pub const STATUS_ILLUSION: i32 = 1 << 24;
pub const STATUS_DEFENSESHIELD: i32 = 1 << 25;
pub const STATUS_PFM: i32 = 1 << 26;
pub const STATUS_PFA: i32 = 1 << 27;
pub const STATUS_ENEMYFLAGS: i32 = !(STATUS_PFA | STATUS_PFM | STATUS_DEFENSESHIELD | STATUS_ILLUSION | STATUS_POISON);
pub const GMFLAG_INVENCIBLE: i32 = 1;
pub const GMFLAG_NOAGGRO: i32 = 2;

/// A carried or banked item: the config it was made from plus the per-instance fields of the player data.
#[derive(Clone, Debug, Default)]
pub struct Item {
    pub cfg: ItemCfg,
    pub count: u32,
    pub touch_effect_type: i16,
    pub touch_effect_value: [i16; 3],
    pub item_color: u8,
    pub item_spec_effect_value: [i16; 3],
    pub cur_life_span: u16,
    pub max_life_span: u16,
    pub weight: u16,
    pub speed: i32,
    pub price: u16,
    pub attribute: u32,
    pub unique_id: u64,
    /// AddGroundItem's handle while it lies on the ground (0 = none; m_stGroundNpcItem[].item)
    pub ground_tag: u32,
}

impl Item {
    pub fn new(cfg: &ItemCfg) -> Item {
        Item {
            cfg: cfg.clone(),
            // (CItem's constructor: m_dwCount = 1; the summon scrolls, flags and relic made here had count 0)
            count: 1,
            item_color: cfg.item_color as u8,
            cur_life_span: cfg.max_life_span,
            max_life_span: cfg.max_life_span,
            weight: cfg.weight,
            speed: cfg.speed,
            price: cfg.price,
            ..Default::default()
        }
    }

    /// `CItem::AdjustByStat`: the secondary stat nibble (5) changes speed, weight or durability.
    pub fn adjust_by_stat(&mut self) {
        let t = (self.attribute >> 20) & 0xF;
        let v = (self.attribute >> 16) & 0xF;
        match t {
            5 => self.speed = (self.speed - 1).max(0), // ITEMSTAT_AGILE
            6 => {
                // ITEMSTAT_LIGHT
                let d = (v * 4) as f64 / 100.0 * self.weight as f64;
                self.weight = (self.weight as i32 - d as i32).max(1) as u16;
            }
            8 | 9 => {
                // ITEMSTAT_STRONG / ANCIENT
                let d = (v * 7) as f64 / 100.0 * self.max_life_span as f64;
                self.max_life_span = self.max_life_span.wrapping_add(d as u16);
            }
            _ => {}
        }
    }

    /// `iGetItemWeight`
    pub fn weight_of(&self, count: i64) -> i32 {
        let c = if count < 0 { 1 } else { count };
        let mut w = (self.weight as i64 * c) as i32;
        if self.cfg.id_num == 90 {
            w /= 20; // ITEM_GOLD
        }
        w.max(1)
    }
}

pub struct Client {
    pub handle: usize,
    pub conn: u64,
    pub tx: Tx,
    pub ip_address: Vec<u8>,
    pub char_name: Vec<u8>,
    pub account_name: Vec<u8>,
    pub account_password: Vec<u8>,
    pub is_init_complete: bool,
    pub is_observer_mode: bool,
    pub is_killed: bool,
    pub is_on_server_change: bool,
    pub is_on_waiting_process: bool,
    pub is_manager: bool,
    /// trade window (m_isExchangeMode, m_isExchangeConfirm, m_exchangeH, m_exchangeName, m_exchangeCount,
    /// m_exchangeItems: slot, amount, name)
    pub exchange_mode: bool,
    /// the angelic pendants' stat bonus: str, dex, int, mag (_angelStr...)
    pub angel: [i32; 4],
    /// m_bIsBeingResurrected (a resurrection spell asked this dead player yes/no)
    pub is_being_resurrected: bool,
    pub exchange_confirm: bool,
    pub exchange_h: usize,
    pub exchange_name: Vec<u8>,
    pub exchange_count: usize,
    pub exchange_items: [(i32, i32, Vec<u8>); 4],

    pub char_id: u32,
    pub char_id_num: [i16; 3],
    pub level: i32,
    pub str_: i32,
    pub vit: i32,
    pub dex: i32,
    pub int_: i32,
    pub mag: i32,
    pub range: i32,
    pub luck: i32,
    pub exp: u32,
    pub sex: u8,
    pub skin: u8,
    pub hair_style: u8,
    pub hair_color: u8,
    pub underwear: u8,
    pub location: Vec<u8>,
    pub map_name: Vec<u8>,
    pub map_index: usize,
    pub x: i16,
    pub y: i16,
    pub dir: u8,
    pub type_: i16,
    /// m_sOriginalType: the type before a /polymorph (0 = never changed)
    pub original_type: i16,
    /// m_nextRecallPoint: the map's initial point a recall goes to (MSGID_REQUEST_SETRECALLPNT; 0 = the plain recall)
    pub next_recall_point: i32,
    pub appr: [i16; 4],
    pub appr_color: i32,
    pub status: i32,
    pub side: i32,
    pub is_neutral: bool,
    pub is_on_town_nonpk: bool,
    pub is_on_shop: bool,
    pub is_on_warehouse: bool,
    pub is_on_tower: bool,
    pub contribution: i32,
    pub special_ability_time: i32,
    pub locked_map_name: Vec<u8>,
    pub locked_map_time: i32,
    pub penalty_block: [i32; 3],
    pub guild_name: Vec<u8>,
    pub guild_guid: i32,
    pub guild_rank: i32,
    /// a ticket request waiting for that guild master's answer
    pub guild_request: Option<(crate::guild::GuildReq, [u8; 10])>,
    pub fightzone_number: i32,
    pub reserve_time: i32,
    pub fightzone_ticket_number: i32,
    pub quest: i32,
    pub quest_id: u32,
    pub cur_quest_count: i32,
    pub quest_reward_type: i32,
    pub quest_reward_amount: i32,
    pub is_quest_completed: bool,
    /// m_iAskedQuest: the quest the last npc talk offered
    pub asked_quest: i32,
    /// m_bQuestMatchFlag_Loc: on the quest's target map
    pub quest_match_loc: bool,
    pub special_event_id: i32,
    pub war_contribution: i32,
    pub crusade_duty: i32,
    pub crusade_guid: u32,
    pub construction_point: i32,
    /// the crusade map / construction state (crusade.rs)
    pub cr: crate::crusade::ClientCrusade,
    pub reputation: i32,
    pub hp: i32,
    pub mp: i32,
    pub sp: i32,
    pub enemy_kill_count: i32,
    pub pk_count: i32,
    pub reward_gold: i32,
    pub down_skill_index: i32,
    pub hunger_status: i32,
    pub super_attack_left: i32,
    pub time_left_shutup: i32,
    /// v8.00 starter bonus (events.rs): minutes of play left with Double EXP + Double EK; saved in the top byte of LeftShutupTime
    pub newbie_min: i32,
    /// `Game::now` of the last MSGID_REQUEST_TYPING accepted (a client may not flood its neighbours)
    pub typing_time: u32,
    /// the saved client settings from the Login (None until it answers) and when the client last saved them (client_settings.rs)
    pub client_settings: Option<Vec<u8>>,
    pub settings_time: u32,
    /// v8.00 client toggles, effective values: UI_TYPING | UI_LOWHEALTH | UI_ITEMNAMES (saved in the top bits of `LeftPopTime`)
    pub ui_flags: u8,
    /// `Game::now` of the last minute taken off `newbie_min` (0 = not started)
    pub newbie_time: u32,
    pub time_left_rating: i32,
    pub time_left_force_recall: i32,
    pub time_left_firm_stamina: i32,
    pub dead_penalty_time: i32,
    pub party_id: i32,
    pub party_status: i32,
    /// the other side of a pending party query (m_iReqJoinPartyClientH / m_cReqJoinPartyName)
    pub req_join_party_h: usize,
    pub req_join_party_name: Vec<u8>,
    pub party_hp_count: i32,
    /// CLIENTFEATURE_* bits the client announced (0 for the original clients)
    pub client_features: u8,
    /// what the party frame was last told about this player, and the party roster this player's frame was last given
    pub hud_sent: crate::party::HudState,
    pub hud_sig: u64,
    pub party_mp_count: i32,
    pub gizon_item_upgrade_left: i32,
    pub elo: i32,
    pub admin_user_level: i32,
    pub gm_flags: i32,
    pub magic_mastery: [u8; MAXMAGICTYPE],
    pub profile: Vec<u8>,
    pub skill_mastery: [u8; MAXSKILLTYPE],
    pub skill_ssn: [u32; MAXSKILLTYPE],
    pub items: Vec<Option<Item>>,
    pub item_pos: [(i16, i16); MAXITEMS],
    pub is_item_equipped: [bool; MAXITEMS],
    pub bank: Vec<Option<Item>>,
    pub is_bank_modified: bool,
    pub equip_status: [i16; MAXITEMEQUIPPOS],
    pub magic_effect_status: [i32; MAXMAGICEFFECTS],
    pub hit_ratio: i32,
    pub defense_ratio: i32,
    pub lu_pool: i32,
    pub next_level_exp: u32,
    pub cur_weight_load: i32,
    pub is_safe_attack_mode: bool,
    pub is_war_location: bool,
    pub is_in_building: bool,
    pub war_begin_time: u32,
    pub whisper_player_index: i32,
    pub whisper_player_name: Vec<u8>,
    pub is_checking_whisper_player: bool,

    // timers (timeGetTime ms)
    pub time: u32,
    pub hunger_time: u32,
    pub auto_save_time: u32,
    pub hp_time: u32,
    pub mp_time: u32,
    pub sp_time: u32,
    pub exp_stock_time: u32,
    pub auto_exp_time: u32,
    pub recent_attack_time: u32,
    /// last "safe zone" notice shown to this player (Rust only), so a held attack does not flood the chat
    pub safe_zone_msg_time: u32,
    pub last_action_time: u32,
    pub logout_hack_check: u32,
    pub init_cc_time_rcv: u32,
    pub init_cc_time: u32,
    pub connection_check: i32,
    pub move_msg_recv_count: i32,
    pub move_lat: u32,
    pub run_msg_recv_count: i32,
    pub run_lat: u32,
    pub rejected_moves: i32,
    pub move_freq_time: u32,
    pub is_move_blocked: bool,
    pub is_attack_mode_change: bool,
    pub skill_msg_recv_count: i32,
    /// m_bSkillUsingStatus / m_iSkillUsingTimeID (a skill item's pending delay event)
    pub skill_using: [bool; MAXSKILLTYPE],
    pub skill_using_time_id: [u32; MAXSKILLTYPE],
    /// m_iAllocatedFish (0 = none) and m_iFishChance
    pub allocated_fish: usize,
    pub fish_chance: i32,
    pub party_coord_steps: i32,
    pub fx: crate::combat::ItemFx,
    pub cs: crate::combat::Cs,
}

impl Client {
    pub fn new(handle: usize, conn: u64, tx: Tx, ip: Vec<u8>, now: u32) -> Client {
        Client {
            handle,
            conn,
            tx,
            ip_address: ip,
            char_name: Vec::new(),
            account_name: Vec::new(),
            account_password: Vec::new(),
            is_init_complete: false,
            is_observer_mode: false,
            is_killed: false,
            is_on_server_change: false,
            is_on_waiting_process: false,
            is_manager: false,
            exchange_mode: false,
            angel: [0; 4],
            is_being_resurrected: false,
            exchange_confirm: false,
            exchange_h: 0,
            exchange_name: Vec::new(),
            exchange_count: 0,
            exchange_items: Default::default(),
            char_id: 0,
            char_id_num: [0; 3],
            level: 0,
            str_: 0,
            vit: 0,
            dex: 0,
            int_: 0,
            mag: 0,
            range: 0,
            luck: 0,
            exp: 0,
            sex: 0,
            skin: 0,
            hair_style: 0,
            hair_color: 0,
            underwear: 0,
            location: b"NONE".to_vec(),
            map_name: Vec::new(),
            map_index: 0,
            x: -1,
            y: -1,
            dir: 5,
            type_: 0,
            original_type: 0,
            next_recall_point: 0,
            appr: [0; 4],
            appr_color: 0,
            status: 0,
            side: NEUTRAL,
            is_neutral: false,
            is_on_town_nonpk: false,
            is_on_shop: false,
            is_on_warehouse: false,
            is_on_tower: false,
            contribution: 0,
            special_ability_time: 0,
            locked_map_name: b"NONE".to_vec(),
            locked_map_time: 0,
            penalty_block: [0; 3],
            guild_name: b"NONE".to_vec(),
            guild_guid: -1,
            guild_rank: -1,
            guild_request: None,
            fightzone_number: 0,
            reserve_time: 0,
            fightzone_ticket_number: 0,
            quest: 0,
            quest_id: 0,
            cur_quest_count: 0,
            quest_reward_type: 0,
            quest_reward_amount: 0,
            is_quest_completed: false,
            asked_quest: 0,
            quest_match_loc: false,
            special_event_id: 0,
            war_contribution: 0,
            crusade_duty: 0,
            crusade_guid: 0,
            construction_point: 0,
            cr: Default::default(),
            reputation: 0,
            hp: 0,
            mp: 0,
            sp: 0,
            enemy_kill_count: 0,
            pk_count: 0,
            reward_gold: 0,
            down_skill_index: -1,
            hunger_status: 100,
            super_attack_left: 0,
            time_left_shutup: 0,
            newbie_min: 0,
            typing_time: 0,
            client_settings: None,
            settings_time: 0,
            ui_flags: crate::events::UI_DEFAULT,
            newbie_time: 0,
            time_left_rating: 0,
            time_left_force_recall: 0,
            time_left_firm_stamina: 0,
            dead_penalty_time: 0,
            party_id: 0,
            party_status: 0,
            req_join_party_h: 0,
            req_join_party_name: Vec::new(),
            party_hp_count: 0,
            client_features: 0,
            hud_sent: Default::default(),
            hud_sig: 0,
            party_mp_count: 0,
            gizon_item_upgrade_left: 0,
            elo: 0,
            admin_user_level: 0,
            gm_flags: 0,
            magic_mastery: [0; MAXMAGICTYPE],
            profile: Vec::new(),
            skill_mastery: [0; MAXSKILLTYPE],
            skill_ssn: [0; MAXSKILLTYPE],
            items: vec![None; MAXITEMS],
            item_pos: [(40, 30); MAXITEMS],
            is_item_equipped: [false; MAXITEMS],
            bank: vec![None; MAXBANKITEMS],
            is_bank_modified: false,
            equip_status: [-1; MAXITEMEQUIPPOS],
            magic_effect_status: [0; MAXMAGICEFFECTS],
            hit_ratio: 0,
            defense_ratio: 0,
            lu_pool: 0,
            next_level_exp: 0,
            cur_weight_load: 0,
            is_safe_attack_mode: false,
            is_war_location: false,
            is_in_building: false,
            war_begin_time: 0,
            whisper_player_index: -1,
            whisper_player_name: Vec::new(),
            is_checking_whisper_player: false,
            time: now,
            hunger_time: now,
            auto_save_time: now,
            hp_time: now,
            mp_time: now,
            sp_time: now,
            exp_stock_time: now,
            auto_exp_time: now,
            recent_attack_time: now,
            safe_zone_msg_time: 0,
            last_action_time: now,
            logout_hack_check: 0,
            init_cc_time_rcv: 0,
            init_cc_time: 0,
            connection_check: 0,
            move_msg_recv_count: 0,
            move_lat: 0,
            run_msg_recv_count: 0,
            run_lat: 0,
            rejected_moves: 0,
            move_freq_time: 0,
            is_move_blocked: false,
            is_attack_mode_change: false,
            skill_msg_recv_count: 0,
            skill_using: [false; MAXSKILLTYPE],
            skill_using_time_id: [0; MAXSKILLTYPE],
            allocated_fish: 0,
            fish_chance: 0,
            party_coord_steps: 0,
            fx: Default::default(),
            cs: Default::default(),
        }
    }

    // stats with the angelic pendants' bonus (_angelStr/Dex/Int/Mag)
    pub fn get_str(&self) -> i32 {
        self.str_ + self.angel[0]
    }
    pub fn get_dex(&self) -> i32 {
        self.dex + self.angel[1]
    }
    pub fn get_int(&self) -> i32 {
        self.int_ + self.angel[2]
    }
    pub fn get_mag(&self) -> i32 {
        self.mag + self.angel[3]
    }
    pub fn is_gm(&self) -> bool {
        self.admin_user_level != 0
    }
    pub fn is_neutral(&self) -> bool {
        self.side == NEUTRAL
    }
    pub fn is_dead(&self) -> bool {
        self.hp <= 0
    }
    pub fn is_invincible(&self) -> bool {
        self.gm_flags & GMFLAG_INVENCIBLE != 0
    }
    pub fn get_max_hp(&self) -> i32 {
        let r = self.vit * 3 + self.level * 2 + self.get_str() / 2;
        if self.fx.side_effect_max_hp_down != 0 { r - r / self.fx.side_effect_max_hp_down } else { r }
    }
    pub fn get_max_mp(&self) -> i32 {
        2 * self.get_mag() + 2 * self.level + self.get_int() / 2
    }
    pub fn get_max_sp(&self) -> i32 {
        2 * self.get_str() + 2 * self.level
    }
    pub fn set_status_flag(&mut self, flag: i32, on: bool) {
        if on {
            self.status |= flag;
        } else {
            self.status &= !flag;
        }
    }
    /// `Unit::SetSideFlag`: nibble 7 of the status.
    pub fn set_side_flag(&mut self, side: i32) {
        self.status = (self.status & 0x0FFF_FFFF) | (side << 28);
    }
    /// `CClient::HasItem` by id: the bag index.
    pub fn has_item(&self, id: i32) -> Option<usize> {
        self.items.iter().position(|i| i.as_ref().is_some_and(|i| i.cfg.id_num == id))
    }
}

/// `CMisc::bCheckValidName`
pub fn valid_name(s: &[u8]) -> bool {
    !crate::util::cstr(s).iter().any(|c| b",= \n\t\\/:*?<>|\"".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_flag_and_max() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut c = Client::new(1, 1, tx, b"1.2.3.4".to_vec(), 0);
        c.status = 0x0012_3456;
        c.set_side_flag(ELVINE);
        assert_eq!(c.status as u32, 0x2012_3456);
        c.set_status_flag(STATUS_PK, false);
        assert_eq!(c.status as u32, 0x2002_3456);
        (c.vit, c.level, c.str_) = (10, 1, 14);
        assert_eq!(c.get_max_hp(), 39);
        assert!(valid_name(b"RtHg1") && !valid_name(b"a b") && !valid_name(b"a?b"));
    }
}

// MagicTypes (HG/char/Magic.h)
pub const MAGICTYPE_PROTECT: i32 = 11;
pub const MAGICTYPE_HOLDOBJECT: i32 = 12;
pub const MAGICTYPE_INVISIBILITY: i32 = 13;
pub const MAGICTYPE_POISON: i32 = 17;
pub const MAGICTYPE_BERSERK: i32 = 18;
pub const MAGICTYPE_CONFUSE: i32 = 16;

pub const MAGICTYPE_ICE: i32 = 23;
