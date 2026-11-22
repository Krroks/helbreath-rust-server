//! HG/DropManager.cpp: NPC drop tables from `..\configs\droplist.xml` (xsd/xerces in the C++, roxmltree here).
//! Note: the C++ never calls `InitDrops` (commented out in OnTimer), so its tables stay empty and the
//! `m_drops` paths never drop anything; the world decides whether to load this (see PORTING.md).
use crate::gamedata::ITEM_NONE;
use crate::rng::Rng;

pub const MAXNPCDROPTIERS: usize = 3;
pub const MAXNPCDROPS: usize = 40;
pub const ONNPCDEATH: u8 = 0;
pub const ONNPCDELETE: u8 = 1;

#[derive(Clone, Default, Debug)]
pub struct Tier {
    pub prob: i32,
    pub drop_prob_sum: i32,
    pub drops: Vec<(i32, i32)>, // (item id, prob)
}

#[derive(Clone, Default, Debug)]
pub struct NpcDrop {
    pub tier_prob_sum: i32,
    pub tiers: Vec<Tier>,
    pub sec_drop_num: u32,
}

fn kids<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &'static str) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    n.children().filter(move |c| c.has_tag_name(tag))
}

pub struct DropManager {
    pub primary: Vec<NpcDrop>,
    pub secondary: Vec<NpcDrop>,
    pub drop_count: u32,
}

impl DropManager {
    pub fn new(max_npc_types: usize) -> Self {
        DropManager { primary: vec![NpcDrop::default(); max_npc_types], secondary: vec![NpcDrop::default(); max_npc_types], drop_count: 0 }
    }

    /// `_ParseXML`; returns the errors the C++ sends to LogError.
    pub fn parse(&mut self, xml: &str, find_npc: impl Fn(&str) -> Option<usize>, find_item: impl Fn(&str) -> Option<i32>) -> Vec<String> {
        let mut err = Vec::new();
        let doc = match roxmltree::Document::parse(xml) {
            Ok(d) => d,
            Err(e) => return vec![format!("XML ERROR: {e}")],
        };
        for npc in kids(doc.root_element(), "npc_t") {
            let name = npc.attribute("name").unwrap_or("");
            let Some(t) = find_npc(name).filter(|&t| t < self.primary.len()) else {
                err.push(format!("Unknown NPC in Droplist: {name}"));
                continue;
            };
            if !self.primary[t].tiers.is_empty() || !self.secondary[t].tiers.is_empty() {
                err.push(format!("Duplicated NPC in Droplist: {name}"));
                continue;
            }
            let num = npc.attribute("num").and_then(|v| v.parse().ok()).unwrap_or(1);
            for tier in kids(npc, "tier_t") {
                let drop = if tier.attribute("type") == Some("primary") { &mut self.primary[t] } else { &mut self.secondary[t] };
                drop.sec_drop_num = num;
                if drop.tiers.len() >= MAXNPCDROPTIERS {
                    err.push(format!("Too many a Tier in NPC({name})"));
                    continue;
                }
                let prob: i32 = tier.attribute("prob").and_then(|v| v.parse().ok()).unwrap_or(0);
                drop.tier_prob_sum += prob;
                let mut tr = Tier { prob, ..Default::default() };
                for item in kids(tier, "item_t") {
                    let iname = item.attribute("name").unwrap_or("");
                    let Some(id) = (if iname == "nothing" { Some(ITEM_NONE) } else { find_item(iname) }) else {
                        err.push(format!("Unknown item({iname}) in NPC({name})"));
                        continue;
                    };
                    if tr.drops.len() >= MAXNPCDROPS {
                        err.push(format!("Too many a Drop({iname}) in NPC({name})"));
                        continue;
                    }
                    let p: i32 = item.attribute("prob").and_then(|v| v.parse().ok()).unwrap_or(0);
                    tr.drop_prob_sum += p;
                    tr.drops.push((id, p));
                    self.drop_count += 1;
                }
                drop.tiers.push(tr);
            }
        }
        err
    }

    pub fn has_primary(&self, npc_type: usize) -> bool {
        self.primary.get(npc_type).is_some_and(|d| !d.tiers.is_empty())
    }
    pub fn has_secondary(&self, npc_type: usize) -> bool {
        self.secondary.get(npc_type).is_some_and(|d| !d.tiers.is_empty())
    }
    pub fn sec_drop_num(&self, npc_type: usize) -> u32 {
        self.secondary.get(npc_type).map_or(0, |d| d.sec_drop_num)
    }

    /// `Roll`. `factor` scales the first drop's chance (killer drop factor, x0.9 in middleland) and is only
    /// applied to ONNPCDELETE rolls, as in the C++; the caller computes it.
    pub fn roll(&self, npc_type: usize, drop_type: u8, factor: f32, rng: &mut Rng) -> i32 {
        let (drop, factor) = match drop_type {
            ONNPCDEATH => (self.primary.get(npc_type), 1.0),
            ONNPCDELETE => (self.secondary.get(npc_type), factor),
            _ => return ITEM_NONE,
        };
        let Some(drop) = drop else { return ITEM_NONE };
        let mut tier_roll = rng.dice(1, drop.tier_prob_sum);
        let mut tier = None;
        for t in &drop.tiers {
            if tier_roll <= t.prob {
                tier = Some(t);
                break;
            }
            tier_roll -= t.prob;
        }
        let Some(tier) = tier.filter(|t| !t.drops.is_empty()) else { return ITEM_NONE };
        let first = (tier.drops[0].1 as f32 * factor) as i32;
        let mut roll = rng.dice(1, tier.drop_prob_sum - (tier.drops[0].1 - first));
        if roll <= first {
            return tier.drops[0].0;
        }
        roll -= first;
        for &(id, p) in &tier.drops[1..] {
            if roll <= p {
                return id;
            }
            roll -= p;
        }
        ITEM_NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0"?><droplist><npc_t name="Slime" num="2">
        <tier_t type="primary" prob="10"><item_t name="nothing" prob="1"/><item_t name="Gold" prob="9"/></tier_t>
        <tier_t type="secondary" prob="5"><item_t name="Dagger" prob="5"/><item_t name="Nope" prob="5"/></tier_t>
        </npc_t><npc_t name="Ghost"/><npc_t name="Slime"/></droplist>"#;

    fn dm() -> (DropManager, Vec<String>) {
        let mut d = DropManager::new(200);
        let npc = |n: &str| (n == "Slime").then_some(10);
        let item = |n: &str| match n {
            "Gold" => Some(90),
            "Dagger" => Some(1),
            _ => None,
        };
        let e = d.parse(XML, npc, item);
        (d, e)
    }

    #[test]
    fn parse_and_roll() {
        let (d, e) = dm();
        assert_eq!(e, ["Unknown item(Nope) in NPC(Slime)", "Unknown NPC in Droplist: Ghost", "Duplicated NPC in Droplist: Slime"]);
        assert!(d.has_primary(10) && d.has_secondary(10) && !d.has_primary(11));
        assert_eq!((d.sec_drop_num(10), d.drop_count), (2, 3));
        let mut rng = Rng::new(1);
        let rolls: Vec<i32> = (0..200).map(|_| d.roll(10, ONNPCDEATH, 1.0, &mut rng)).collect();
        assert!(rolls.iter().all(|&r| r == 90 || r == ITEM_NONE) && rolls.iter().filter(|&&r| r == 90).count() > 150);
        assert_eq!(d.roll(10, ONNPCDELETE, 1.0, &mut rng), 1);
        assert_eq!(d.roll(11, ONNPCDEATH, 1.0, &mut rng), ITEM_NONE);
    }

    #[test]
    fn real_droplist() {
        let p = r"G:\DEV\hb\run\server\Maps\configs\droplist.xml";
        let cfg = std::path::Path::new(r"G:\DEV\hb\run\server\Login\Config");
        let Ok(xml) = std::fs::read_to_string(p) else { return };
        let mut g = crate::gamedata::GameData::new();
        for f in ["Item.cfg", "Item2.cfg", "Item3.cfg", "Item4.cfg"] {
            g.decode_items(&crate::util::text_mode(&std::fs::read(cfg.join(f)).unwrap_or_default()));
        }
        g.decode_npcs(&crate::util::text_mode(&std::fs::read(cfg.join("NPC.cfg")).unwrap_or_default()));
        let mut d = DropManager::new(200);
        let e = d.parse(&xml, |n| g.find_npc(n), |n| g.find_item(n));
        println!("drops {} errors {} first {:?}", d.drop_count, e.len(), e.first());
        assert!(d.drop_count > 0);
    }
}
