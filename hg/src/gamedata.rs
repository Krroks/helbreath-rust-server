//! Config tables the HG receives from the Login as 14 text frames (HG.cpp MsgProcess, MSGFROM_LOGSERVER):
//! `_bDecode*ConfigFileContents` and the lookups over them (`_bInitItemAttr`, `FindNPC`, `FindItem`,
//! `_bCheckDupItemID`). Every decoder has the same shape in the C++: a token starting with the record
//! keyword (memcmp prefix) opens a record, the next N tokens are its fields in order, a missing tail is an error.
//! Items, NPCs, skills, teleports and dup-item ids are typed (layer 1/2 use them); magic, quest, build-item,
//! potion and crafting records are kept as raw field lists until the layers that use them type them.
use crate::consts::*;
use crate::util::{atoi, atoul, cstr, StrTok};

pub const ITEM_NONE: i32 = -1;
pub const ITEM_INVALID: i32 = -2;

/// `_bGetIsStringIsNumber`: only '-' and digits (an empty token passes).
pub fn is_number(t: &[u8]) -> bool {
    t.iter().all(|&c| c == b'-' || c.is_ascii_digit())
}

/// One record: the keyword token opens it, then `n` field tokens. `end` stops decoding (`[ENDITEMLIST]`, `[END]`).
/// Returns None when the text ends inside a record (the C++ "contents error").
fn records(text: &[u8], key: &[u8], n: usize, end: Option<&[u8]>) -> Option<Vec<Vec<Vec<u8>>>> {
    let mut t = StrTok::new(text, b"= \t\n");
    let (mut out, mut cur): (Vec<Vec<Vec<u8>>>, Option<Vec<Vec<u8>>>) = (Vec::new(), None);
    while let Some(tok) = t.get() {
        if let Some(r) = cur.as_mut() {
            r.push(tok);
            if r.len() == n {
                out.push(cur.take().unwrap());
            }
            continue;
        }
        if tok.starts_with(key) {
            cur = Some(Vec::new());
        } else if end.is_some_and(|e| tok.starts_with(e)) {
            break;
        }
    }
    cur.is_none().then_some(out)
}

fn name_of(t: &[u8], max: usize) -> Vec<u8> {
    t[..t.len().min(max)].to_vec()
}

#[derive(Clone, Debug, Default)]
pub struct ItemCfg {
    pub id_num: i32,
    pub name: Vec<u8>,
    pub item_type: i32,
    pub equip_pos: i32,
    pub item_effect_type: i32,
    pub item_effect_value: [i32; 6],
    pub max_life_span: u16,
    pub special_effect: i32,
    pub sprite: i32,
    pub sprite_frame: i32,
    pub price: u16,
    pub is_for_sale: bool,
    pub weight: u16,
    pub appr_value: i32,
    pub speed: i32,
    pub level_limit: i32,
    pub gender_limit: i32,
    pub special_effect_value1: i32,
    pub special_effect_value2: i32,
    pub related_skill: i32,
    pub category: i32,
    pub item_color: i32,
    pub is_logged: bool,
}

#[derive(Clone, Debug, Default)]
pub struct NpcCfg {
    pub name: Vec<u8>,
    pub type_: i32,
    pub hit_dice: i32,
    pub defense_ratio: i32,
    pub hit_ratio: i32,
    pub min_bravery: i32,
    pub exp_dice: i32,
    pub attack_dice_throw: i32,
    pub attack_dice_range: i32,
    pub size: i32,
    pub side: i32,
    pub action_limit: i32,
    pub action_time: u32,
    pub resist_magic: i32,
    pub magic_level: i32,
    pub day_of_week_limit: i32,
    pub chat_msg_presence: i32,
    pub target_search_range: i32,
    pub regen_time: u32,
    pub attribute: i32,
    pub abs_damage: i32,
    pub max_mana: i32,
    pub magic_hit_ratio: i32,
    pub attack_range: i32,
}

#[derive(Clone, Debug, Default)]
pub struct SkillCfg {
    pub name: Vec<u8>,
    pub type_: i32,
    pub value: [i32; 6],
}

#[derive(Clone, Debug, Default)]
pub struct TeleportCfg {
    /// added by /eventtp (William's cityhall teleports to an event map)
    pub admin_created: bool,
    pub npc_name: Vec<u8>,
    pub source_map: Vec<u8>,
    pub target_map: Vec<u8>,
    pub x: i32,
    pub y: i32,
    pub cost: u32,
    pub min_lvl: i32,
    pub max_lvl: i32,
    pub side: i32,
    pub neutral: bool,
    pub criminal: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DupItemId {
    pub touch_effect_type: i32,
    pub touch_effect_value: [i32; 3],
    pub price: u16,
}

/// Side values of the `Side` enum (Shared/common.h) the teleport list uses.
pub const NEUTRAL: i32 = 0;
pub const ARESDEN: i32 = 1;
pub const ELVINE: i32 = 2;
pub const ISTRIA: i32 = 3;

pub struct GameData {
    pub items: Vec<Option<ItemCfg>>,
    pub npcs: Vec<Option<NpcCfg>>,
    pub skills: Vec<Option<SkillCfg>>,
    pub teleports: Vec<TeleportCfg>,
    pub dup_item_ids: Vec<Option<DupItemId>>,
    /// Raw records (fields after the keyword), indexed by their first field where the C++ does.
    pub magic: Vec<Option<Vec<Vec<u8>>>>,
    pub quests: Vec<Option<Vec<Vec<u8>>>>,
    pub build_items: Vec<Vec<Vec<u8>>>,
    pub potions: Vec<Option<Vec<Vec<u8>>>>,
    pub crafting: Vec<Option<Vec<Vec<u8>>>>,
    pub noticement: Vec<u8>,
    pub item_ok: bool,
    pub npc_ok: bool,
    pub magic_ok: bool,
    pub skill_ok: bool,
    pub potion_ok: bool,
    pub quest_ok: bool,
    pub build_item_ok: bool,
}

/// Raw records indexed by their first field; an index outside the table is an error.
fn indexed(recs: Vec<Vec<Vec<u8>>>, max: i32) -> Option<Vec<Option<Vec<Vec<u8>>>>> {
    let mut v = vec![None; max as usize];
    for r in recs {
        let i = atoi(&r[0]);
        // fix: the C++ indexed its table with atoi(token) unchecked (out-of-bounds write on a bad id)
        if i < 0 || i >= max {
            return None;
        }
        v[i as usize] = Some(r);
    }
    Some(v)
}

impl GameData {
    pub fn new() -> Self {
        GameData {
            items: vec![None; MAXITEMTYPES as usize],
            npcs: vec![None; MAXNPCTYPES as usize],
            skills: vec![None; crate::client::MAXSKILLTYPE],
            teleports: Vec::new(),
            dup_item_ids: vec![None; MAXDUPITEMID as usize],
            magic: Vec::new(),
            quests: Vec::new(),
            build_items: Vec::new(),
            potions: Vec::new(),
            crafting: Vec::new(),
            noticement: Vec::new(),
            item_ok: false,
            npc_ok: false,
            magic_ok: false,
            skill_ok: false,
            potion_ok: false,
            quest_ok: false,
            build_item_ok: false,
        }
    }

    /// All the tables the game needs before `m_bIsGameStarted` (OnTimer's start condition).
    pub fn all_ok(&self) -> bool {
        self.item_ok && self.npc_ok && self.magic_ok && self.skill_ok && self.potion_ok && self.quest_ok && self.build_item_ok
    }

    /// The MSGFROM_LOGSERVER config cases of MsgProcess; `text` is the frame after its 6-byte header.
    pub fn on_config_frame(&mut self, msgid: u32, text: &[u8]) {
        let text = cstr(text);
        match msgid {
            MSGID_ITEMCONFIGURATIONCONTENTS => self.item_ok = self.decode_items(text),
            MSGID_NPCCONFIGURATIONCONTENTS => self.npc_ok = self.decode_npcs(text),
            MSGID_MAGICCONFIGURATIONCONTENTS => self.magic_ok = self.decode_raw(text, b"magic", 20, crate::client::MAXMAGICTYPE as i32, 0),
            MSGID_SKILLCONFIGURATIONCONTENTS => self.skill_ok = self.decode_skills(text),
            MSGID_QUESTCONFIGURATIONCONTENTS => self.quest_ok = self.decode_raw(text, b"quest", 27, MAXQUESTTYPE, 1),
            MSGID_POTIONCONFIGURATIONCONTENTS => self.potion_ok = self.decode_raw(text, b"potion", 16, MAXPOTIONTYPES, 2),
            MSGID_CRAFTINGCONFIGURATIONCONTENTS => {
                self.decode_raw(text, b"crafting", 16, MAXCRAFTING, 3);
            }
            MSGID_BUILDITEMCONFIGURATIONCONTENTS => {
                self.build_item_ok = match records(text, b"BuildItem", 23, None) {
                    Some(r) if r.len() <= MAXBUILDITEMS as usize => {
                        self.build_items = r;
                        true
                    }
                    _ => false,
                }
            }
            MSGID_DUPITEMIDFILECONTENTS => {
                self.decode_dup_ids(text);
            }
            MSGID_NOTICEMENTFILECONTENTS => self.noticement = text.to_vec(),
            MSGID_TELEPORTLISTCONTENTS => {
                self.decode_teleports(text);
            }
            _ => {}
        }
    }

    fn decode_raw(&mut self, text: &[u8], key: &[u8], n: usize, max: i32, which: u8) -> bool {
        let Some(t) = records(text, key, n, None).and_then(|r| indexed(r, max)) else { return false };
        match which {
            0 => self.magic = t,
            1 => self.quests = t,
            2 => self.potions = t,
            _ => self.crafting = t,
        }
        true
    }

    /// `_bDecodeItemConfigFileContents` (called once per Item*.cfg; the table accumulates).
    pub fn decode_items(&mut self, text: &[u8]) -> bool {
        let Some(recs) = records(text, b"Item", 26, Some(b"[ENDITEMLIST]")) else { return false };
        for f in recs {
            // every numeric field is checked with _bGetIsStringIsNumber; field 1 (name) is not
            if f.iter().enumerate().any(|(i, t)| i != 1 && !is_number(t)) {
                return false;
            }
            let n = |i: usize| atoi(&f[i]);
            let id = n(0);
            // fix: the C++ wrote m_pItemConfigList[id] without a range check
            if id < 0 || id >= MAXITEMTYPES || self.items[id as usize].is_some() {
                return false;
            }
            let price = n(15);
            self.items[id as usize] = Some(ItemCfg {
                id_num: id,
                name: name_of(&f[1], 20),
                item_type: n(2),
                equip_pos: n(3),
                item_effect_type: n(4),
                item_effect_value: [n(5), n(6), n(7), n(8), n(9), n(10)],
                max_life_span: n(11) as u16,
                special_effect: n(12),
                sprite: n(13),
                sprite_frame: n(14),
                is_for_sale: price >= 0,
                price: price.unsigned_abs() as u16,
                weight: n(16) as u16,
                appr_value: n(17),
                speed: n(18),
                level_limit: n(19),
                gender_limit: n(20),
                special_effect_value1: n(21),
                special_effect_value2: n(22),
                related_skill: n(23),
                category: n(24),
                item_color: n(25),
                is_logged: false,
            });
        }
        true
    }

    /// `_bDecodeNpcConfigFileContents`: records are numbered in file order.
    pub fn decode_npcs(&mut self, text: &[u8]) -> bool {
        let Some(recs) = records(text, b"Npc", 24, None) else { return false };
        for (i, f) in recs.iter().enumerate() {
            if f.iter().skip(1).any(|t| !is_number(t)) {
                return false;
            }
            // fix: the C++ kept creating entries past MAXNPCTYPES
            if i >= MAXNPCTYPES as usize {
                return false;
            }
            let n = |k: usize| atoi(&f[k]);
            self.npcs[i] = Some(NpcCfg {
                name: name_of(&f[0], 20),
                type_: n(1),
                hit_dice: n(2),
                defense_ratio: n(3),
                hit_ratio: n(4),
                min_bravery: n(5),
                exp_dice: n(6).wrapping_mul(100000),
                attack_dice_throw: n(7),
                attack_dice_range: n(8),
                size: n(9),
                side: n(10),
                action_limit: n(11),
                action_time: n(12) as u32,
                resist_magic: n(13),
                magic_level: n(14),
                day_of_week_limit: n(15),
                chat_msg_presence: n(16),
                target_search_range: n(17),
                regen_time: n(18) as u32,
                attribute: n(19),
                abs_damage: n(20),
                max_mana: n(21),
                magic_hit_ratio: n(22),
                attack_range: n(23),
            });
        }
        true
    }

    /// `_bDecodeSkillConfigFileContents`
    pub fn decode_skills(&mut self, text: &[u8]) -> bool {
        let Some(recs) = records(text, b"skill", 9, None) else { return false };
        for f in recs {
            let i = atoi(&f[0]);
            // fix: unchecked m_pSkillConfigList[atoi(token)] in the C++
            if i < 0 || i >= crate::client::MAXSKILLTYPE as i32 {
                return false;
            }
            let n = |k: usize| atoi(&f[k]);
            self.skills[i as usize] = Some(SkillCfg { name: name_of(&f[1], 20), type_: n(2), value: [n(3), n(4), n(5), n(6), n(7), n(8)] });
        }
        true
    }

    /// `_bDecodeTeleportListConfigFileContents`
    pub fn decode_teleports(&mut self, text: &[u8]) -> bool {
        let Some(recs) = records(text, b"teleport", 11, Some(b"[END]")) else { return false };
        let mut out = Vec::new();
        for f in recs {
            let side = match f[8].as_slice() {
                b"aresden" => ARESDEN,
                b"elvine" => ELVINE,
                b"istria" => ISTRIA,
                b"all" => ALLSIDES,
                b"attacker" => ATTACKER,
                b"defender" => DEFENDER,
                _ => return false,
            };
            let flag = |t: &[u8]| match t {
                b"TRUE" => Some(true),
                b"FALSE" => Some(false),
                _ => None,
            };
            let (Some(neutral), Some(criminal)) = (flag(&f[9]), flag(&f[10])) else { return false };
            // fix: the C++ kept writing past MAXTELEPORTLIST
            if out.len() >= MAXTELEPORTLIST as usize {
                break;
            }
            out.push(TeleportCfg {
                admin_created: false,
                npc_name: name_of(&f[0], 20),
                source_map: name_of(&f[1], 10),
                target_map: name_of(&f[2], 10),
                x: atoi(&f[3]),
                y: atoi(&f[4]),
                cost: atoul(&f[5]),
                min_lvl: atoi(&f[6]),
                max_lvl: atoi(&f[7]),
                side,
                neutral,
                criminal,
            });
        }
        self.teleports = out;
        true
    }

    /// `_bDecodeDupItemIDFileContents`
    pub fn decode_dup_ids(&mut self, text: &[u8]) -> bool {
        let Some(recs) = records(text, b"DupItemID", 6, None) else { return false };
        for f in recs {
            let i = atoi(&f[0]);
            // fix: unchecked m_pDupItemIDList[atoi(token)] in the C++
            if i < 0 || i >= MAXDUPITEMID {
                return false;
            }
            let n = |k: usize| atoi(&f[k]);
            self.dup_item_ids[i as usize] = Some(DupItemId { touch_effect_type: n(1), touch_effect_value: [n(2), n(3), n(4)], price: n(5) as u16 });
        }
        true
    }

    /// `_bInitItemAttr(pItem, name)`: memcmp of 20 bytes against the config names.
    pub fn item_by_name(&self, name: &[u8]) -> Option<&ItemCfg> {
        let name = cstr(name);
        if name.len() > 21 {
            return None;
        }
        let mut key = [0u8; 20];
        let n = name.len().min(20);
        key[..n].copy_from_slice(&name[..n]);
        self.items.iter().flatten().find(|c| {
            let mut k2 = [0u8; 20];
            k2[..c.name.len()].copy_from_slice(&c.name);
            k2 == key
        })
    }

    pub fn item_by_id(&self, id: i32) -> Option<&ItemCfg> {
        if id == ITEM_NONE || id == ITEM_INVALID {
            return None;
        }
        self.items.iter().flatten().find(|c| c.id_num == id)
    }

    /// `FindNPC`: the NPC type of the config with that exact name.
    pub fn find_npc(&self, name: &str) -> Option<usize> {
        self.npcs.iter().flatten().find(|c| c.name == name.as_bytes()).map(|c| c.type_ as usize)
    }

    /// `FindItem`
    pub fn find_item(&self, name: &str) -> Option<i32> {
        self.items.iter().flatten().find(|c| c.name == name.as_bytes()).map(|c| c.id_num)
    }

    /// `_bCheckDupItemID`: the replacement price when the touch effect matches a listed duplicate.
    pub fn dup_item_price(&self, touch_type: i32, v: [i32; 3]) -> Option<u16> {
        self.dup_item_ids.iter().flatten().find(|d| d.touch_effect_type == touch_type && d.touch_effect_value == v).map(|d| d.price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::text_mode;

    const ITEM: &[u8] = b"Item = 1 Dagger 1 8 1 1 4 0 0 0 0 300 0 1 0 -50 100 0 0 0 0 0 0 7 1 0\nItem = 2 Gold 5 0 0 0 0 0 0 0 0 0 0 6 0 1 1 0 0 0 0 0 0 0 0 0\n[ENDITEMLIST]\nItem = 3 Junk";

    #[test]
    fn items() {
        let mut g = GameData::new();
        assert!(g.decode_items(ITEM));
        let d = g.item_by_name(b"Dagger").unwrap();
        assert_eq!((d.id_num, d.equip_pos, d.max_life_span, d.price, d.is_for_sale, d.related_skill), (1, 8, 300, 50, false, 7));
        assert_eq!(g.find_item("Gold"), Some(2));
        assert!(g.item_by_id(3).is_none(), "records after the end marker are ignored");
        assert!(!g.decode_items(b"Item = 1 Again 0"), "truncated record");
        assert!(!GameData::new().decode_items(b"Item = 5000 X 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1"), "id out of range");
        assert!(!GameData::new().decode_items(b"Item = 1 X a 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1"), "not a number");
        let mut g2 = GameData::new();
        g2.decode_items(ITEM);
        assert!(!g2.decode_items(ITEM), "duplicate id across Item*.cfg");
    }

    #[test]
    fn npcs_skills_teleports_dups() {
        let mut g = GameData::new();
        let npc = b"Npc = Slime 10 2 10 10 1 1 1 4 0 0 0 1200 0 0 0 0 6 5000 0 0 0 0 1\nNpc = Orc 14 5 20 30 5 2 2 6 0 0 0 1000 0 0 0 0 6 5000 0 0 0 0 1";
        assert!(g.decode_npcs(npc));
        assert_eq!(g.find_npc("Orc"), Some(14));
        assert_eq!(g.npcs[0].as_ref().unwrap().exp_dice, 100000);
        assert!(!g.decode_npcs(b"Npc = Slime 10 2"));
        assert!(g.decode_skills(b"skill = 0 Mining 1 0 0 0 0 0 0"));
        assert!(!g.decode_skills(b"skill = 99 Bad 1 0 0 0 0 0 0"));
        assert!(g.decode_teleports(b"teleport = 1 aresden elvine 100 120 500 1 180 aresden TRUE FALSE\n[END]"));
        assert_eq!((g.teleports[0].cost, g.teleports[0].neutral, g.teleports[0].side), (500, true, ARESDEN));
        assert!(!g.decode_teleports(b"teleport = 1 a b 1 1 1 1 1 nowhere TRUE FALSE"));
        assert!(g.decode_dup_ids(b"DupItemID = 0 1 10 20 30 99"));
        assert_eq!(g.dup_item_price(1, [10, 20, 30]), Some(99));
        assert!(g.decode_raw(b"magic = 3 x 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1", b"magic", 20, crate::client::MAXMAGICTYPE as i32, 0));
        assert!(g.magic[3].is_some());
        assert!(!g.decode_raw(b"magic = 300 x 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1", b"magic", 20, crate::client::MAXMAGICTYPE as i32, 0));
    }

    #[test]
    fn real_login_configs() {
        let dir = std::path::Path::new(r"G:\DEV\hb\run\server\Login\Config");
        if !dir.exists() {
            return;
        }
        let mut g = GameData::new();
        let files: [(&str, u32); 14] = [
            ("Item.cfg", MSGID_ITEMCONFIGURATIONCONTENTS),
            ("Item2.cfg", MSGID_ITEMCONFIGURATIONCONTENTS),
            ("Item3.cfg", MSGID_ITEMCONFIGURATIONCONTENTS),
            ("Item4.cfg", MSGID_ITEMCONFIGURATIONCONTENTS),
            ("BuildItem.cfg", MSGID_BUILDITEMCONFIGURATIONCONTENTS),
            ("DupItemID.cfg", MSGID_DUPITEMIDFILECONTENTS),
            ("Magic.cfg", MSGID_MAGICCONFIGURATIONCONTENTS),
            ("noticement.txt", MSGID_NOTICEMENTFILECONTENTS),
            ("NPC.cfg", MSGID_NPCCONFIGURATIONCONTENTS),
            ("potion.cfg", MSGID_POTIONCONFIGURATIONCONTENTS),
            ("Quest.cfg", MSGID_QUESTCONFIGURATIONCONTENTS),
            ("Skill.cfg", MSGID_SKILLCONFIGURATIONCONTENTS),
            ("CraftItem.cfg", MSGID_CRAFTINGCONFIGURATIONCONTENTS),
            ("Teleport.cfg", MSGID_TELEPORTLISTCONTENTS),
        ];
        for (f, id) in files {
            let raw = std::fs::read(dir.join(f)).unwrap_or_default();
            g.on_config_frame(id, &text_mode(&raw));
        }
        let c = |v: &Vec<Option<Vec<Vec<u8>>>>| v.iter().flatten().count();
        println!(
            "items {} npcs {} skills {} magic {} quests {} potions {} crafting {} build {} teleports {} dups {}",
            g.items.iter().flatten().count(),
            g.npcs.iter().flatten().count(),
            g.skills.iter().flatten().count(),
            c(&g.magic),
            c(&g.quests),
            c(&g.potions),
            c(&g.crafting),
            g.build_items.len(),
            g.teleports.len(),
            g.dup_item_ids.iter().flatten().count()
        );
        let sc = g.build_items.iter().find(|f| cstr(&f[0]) == b"SuperCoal").expect("SuperCoal build record");
        assert_eq!((atoi(&sc[1]), atoi(&sc[2]), atoi(&sc[3]), atoi(&sc[20]), atoi(&sc[21])), (20, 355, 1, 0, 30));
        let rw = crate::crafting::find_recipe(g.crafting.iter(), &[358, 1, 351, 2, -1, 0, -1, 0, -1, 0, -1, 0]).expect("RubyWare");
        assert_eq!((rw.name.as_slice(), rw.limit, rw.difficulty), (&b"RubyWare"[..], 0, 70));
        assert!(g.all_ok(), "item {} npc {} magic {} skill {} potion {} quest {} build {}", g.item_ok, g.npc_ok, g.magic_ok, g.skill_ok, g.potion_ok, g.quest_ok, g.build_item_ok);
    }
}
