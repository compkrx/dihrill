use super::{BlockStateId, Game};
use super::ore_miner::{harvestable, safe_block};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(super) const ID: &str = "ore_miner";
pub(super) const MAX_TARGETS: usize = 64;
pub(super) const MAX_SHOP_DIAMONDS: u32 = 36 * 64;
pub(super) const MAX_SHOP_INTERVAL: u32 = 3_600;
pub(super) const MAX_SHAFT_LENGTH: u32 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ShaftDirection {
    Down,
    Up,
}
pub(super) const JUNK: &[&str] = &[
    "minecraft:cobblestone",
    "minecraft:cobbled_deepslate",
    "minecraft:dirt",
    "minecraft:gravel",
    "minecraft:netherrack",
    "minecraft:tuff",
    "minecraft:andesite",
    "minecraft:diorite",
    "minecraft:granite",
    "minecraft:calcite",
    "minecraft:blackstone",
];

pub(super) const GROUPS: &[(&str, &[&str])] = &[
    ("diamond", &["diamond_ore", "deepslate_diamond_ore"]),
    ("iron", &["iron_ore", "deepslate_iron_ore"]),
    ("copper", &["copper_ore", "deepslate_copper_ore"]),
    ("coal", &["coal_ore", "deepslate_coal_ore"]),
    ("gold", &["gold_ore", "deepslate_gold_ore", "nether_gold_ore"]),
    ("redstone", &["redstone_ore", "deepslate_redstone_ore"]),
    ("lapis", &["lapis_ore", "deepslate_lapis_ore"]),
    ("emerald", &["emerald_ore", "deepslate_emerald_ore"]),
    ("quartz", &["nether_quartz_ore"]),
    ("netherite", &["ancient_debris"]),
    ("debris", &["ancient_debris"]),
];

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Profile {
    #[default]
    Hunter,
    Tunnel,
    Branch,
    Cave,
    Area,
    Quantity,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Pace {
    Calm,
    #[default]
    Normal,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Tool {
    #[default]
    Fast,
    Fortune,
    Silk,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Goal {
    #[default]
    Unlimited,
    Blocks(u32),
    Items(String, u32),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct Settings {
    pub targets: Vec<String>,
    pub radius: i32,
    pub diagonals: bool,
    pub max_fall: u8,
    pub shallow_water: bool,
    pub avoid_caves: bool,
    pub avoid_mobs: bool,
    pub mob_radius: u8,
    pub profile: Profile,
    pub explore: bool,
    pub explore_limit: u32,
    pub continuous_explore: bool,
    pub branch_spacing: u32,
    pub tunnel_width: u8,
    pub tunnel_height: u8,
    pub turn_speed: u8,
    pub sprint: bool,
    pub pace: Pace,
    pub durability: u32,
    pub tool: Tool,
    pub junk: Vec<String>,
    pub protected: Vec<String>,
    pub alert_inventory: bool,
    pub goal: Goal,
    pub run_seconds: u32,
    pub shaft_length: u32,
    pub area: Option<([i32; 3], [i32; 3])>,
    pub storage: Option<[i32; 3]>,
    pub diamond_shop: Option<ShopMenu>,
    pub shop_min_diamonds: u32,
    pub shop_interval_seconds: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct ShopMenu {
    pub address: String,
    pub title: String,
    pub menu: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            radius: 24,
            diagonals: false,
            max_fall: 1,
            shallow_water: false,
            avoid_caves: true,
            avoid_mobs: true,
            mob_radius: 12,
            profile: Profile::Hunter,
            explore: true,
            explore_limit: 96,
            continuous_explore: false,
            branch_spacing: 12,
            tunnel_width: 1,
            tunnel_height: 2,
            turn_speed: 12,
            sprint: false,
            pace: Pace::Normal,
            durability: 8,
            tool: Tool::Fast,
            junk: Vec::new(),
            protected: Vec::new(),
            alert_inventory: true,
            goal: Goal::Unlimited,
            run_seconds: 0,
            shaft_length: 8,
            area: None,
            storage: None,
            diamond_shop: None,
            shop_min_diamonds: 64,
            shop_interval_seconds: 60,
        }
    }
}

impl Settings {
    pub fn load(game: &Game<'_>) -> Self {
        let mut settings = match game.native_setting(ID, "settings.json") {
            Some(value) => serde_json::from_str(&value).unwrap_or_else(|error| {
                eprintln!("StrataDrill could not read settings.json: {error}");
                Self::default()
            }),
            None => {
                let targets = game.native_setting(ID, "targets").and_then(|value|
                    parse_targets(game, &value.split_whitespace().collect::<Vec<_>>()).ok()
                );
                let radius = game.native_setting(ID, "radius").and_then(|value| value.parse().ok());
                Self {
                    targets: targets.unwrap_or_default(),
                    radius: radius.unwrap_or(Self::default().radius),
                    ..Self::default()
                }
            }
        };
        settings.normalize();
        settings.targets.retain(|id| target_is_valid(game,id));
        settings.protected.retain(|id| game.res.items().get(id).is_some());
        if let Goal::Items(item, _) = &settings.goal
            && game.res.items().get(item).is_none()
        {
            settings.goal = Goal::Unlimited;
        }
        settings
    }

    pub fn save(&self, game: &Game<'_>) {
        let directory = game.native_data_dir(ID);
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&directory)?;
            let json = serde_json::to_vec_pretty(self)?;
            let pending = directory.join(format!(".settings-{}.tmp", std::process::id()));
            std::fs::write(&pending, json)?;
            std::fs::rename(pending, directory.join("settings.json"))
        })();
        if let Err(error) = result {
            eprintln!("StrataDrill could not save settings.json: {error}");
        }
    }

    pub fn normalize(&mut self) {
        fn normalize_ids(ids: &mut Vec<String>, limit: usize) {
            let mut seen = HashSet::new();
            ids.retain_mut(|id| {
                *id = namespaced(id);
                valid_id(id) && seen.insert(id.clone())
            });
            ids.truncate(limit);
        }
        self.radius = self.radius.clamp(8, 48);
        self.shaft_length = self.shaft_length.clamp(1, MAX_SHAFT_LENGTH);
        self.max_fall = self.max_fall.clamp(1, 3);
        self.mob_radius = self.mob_radius.clamp(6, 24);
        self.shop_min_diamonds = self.shop_min_diamonds.clamp(1, MAX_SHOP_DIAMONDS);
        self.shop_interval_seconds = self.shop_interval_seconds.clamp(1, MAX_SHOP_INTERVAL);
        self.explore_limit = self.explore_limit.clamp(8, 256);
        self.branch_spacing = self.branch_spacing.clamp(4, 32);
        self.tunnel_width = self.tunnel_width.clamp(1, 3);
        self.tunnel_height = self.tunnel_height.clamp(2, 3);
        self.turn_speed = self.turn_speed.clamp(3, 45);
        self.durability = self.durability.clamp(1, 1000);
        normalize_ids(&mut self.targets, MAX_TARGETS);
        normalize_ids(&mut self.junk, JUNK.len());
        self.junk.retain(|id| JUNK.contains(&id.as_str()));
        normalize_ids(&mut self.protected, MAX_TARGETS);
        match &mut self.goal {
            Goal::Unlimited => (),
            Goal::Blocks(count) => *count = (*count).clamp(1,100_000),
            Goal::Items(id, count) => {
                *id = namespaced(id);
                *count = (*count).clamp(1,100_000);
                if !valid_id(id) { self.goal = Goal::Unlimited; }
            }
        }
        if let Some((a,b)) = self.area {
            self.area = Some((
                std::array::from_fn(|i| a[i].min(b[i])),
                std::array::from_fn(|i| a[i].max(b[i])),
            ));
        }
    }
}

pub(super) fn namespaced(raw: &str) -> String {
    let id = raw.trim().to_ascii_lowercase();
    if id.contains(':') { id } else { format!("minecraft:{id}") }
}

fn valid_id(value: &str) -> bool {
    let Some((namespace, path)) = value.split_once(':') else { return false };
    !namespace.is_empty() && !path.is_empty() && value.len() <= 128
        && namespace.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()
            || matches!(b, b'_' | b'-' | b'.'))
        && path.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()
            || matches!(b, b'_' | b'-' | b'.' | b'/'))
}

pub(super) fn valid_item(game: &Game<'_>, value: &str) -> Result<String,String> {
    let id = namespaced(value);
    if valid_id(&id) && id != "minecraft:air" && game.res.items().get(&id).is_some() {
        Ok(id)
    } else {
        Err(format!("Unknown item {value}"))
    }
}

fn target_is_valid(game: &Game<'_>, id: &str) -> bool {
    game.pack.assets.registry.default_state(id).is_some_and(|state| {
        let facts = game.res.facts().get(state);
        !game.res.facts().is_air(state) && safe_block(facts) && harvestable(facts)
    })
}

pub(super) fn target_catalog(game: &Game<'_>) -> Vec<String> {
    let registry = game.pack.assets.registry;
    (0..registry.len()).filter_map(|index| {
        let state = BlockStateId(index as u32);
        let facts = game.res.facts().get(state);
        (registry.default_state(&facts.name) == Some(state) &&
            !game.res.facts().is_air(state) && safe_block(facts) && harvestable(facts))
            .then(|| facts.name.to_string())
    }).collect()
}

pub(super) fn parse_targets(game: &Game<'_>, tokens: &[&str]) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for token in tokens.iter().flat_map(|s| s.split(',')).filter(|s| !s.is_empty()) {
        let alias = token.to_ascii_lowercase();
        let group = GROUPS.iter().find(|(name,_)| *name == alias);
        let candidates = if let Some((_, blocks)) = group {
            blocks.iter().map(|id| namespaced(id)).collect()
        } else if alias == "ores" || alias.starts_with('#') {
            let mut catalog = target_catalog(game);
            if let Some(tag) = alias.strip_prefix('#') {
                let tag = namespaced(tag);
                if !valid_id(&tag) { return Err(format!("Invalid block tag {token}")); }
                catalog.retain(|id| game.res.items().block_in_tag(&tag,id));
            } else {
                catalog.retain(|id| id.ends_with("_ore") || id == "minecraft:ancient_debris");
            }
            catalog.sort_unstable();
            if catalog.is_empty() {
                return Err(format!("Block group {token} has no eligible blocks"));
            }
            catalog
        } else {
            let id = namespaced(token);
            if !valid_id(&id) || game.pack.assets.registry.default_state(&id).is_none() {
                return Err(format!("Unknown block {token}"));
            }
            if !target_is_valid(game,&id) {
                return Err(format!("StrataDrill cannot safely mine {id}"));
            }
            vec![id]
        };
        for id in candidates {
            if seen.insert(id.clone()) {
                result.push(id);
                if result.len() > MAX_TARGETS {
                    return Err(format!("Choose at most {MAX_TARGETS} blocks after expanding groups"));
                }
            }
        }
    }
    if result.is_empty() { return Err("Choose at least one target block".into()); }
    Ok(result)
}

pub(super) fn may_discard(settings: &Settings, item: &str) -> bool {
    if !JUNK.contains(&item) || !settings.junk.iter().any(|id| id == item)
        || settings.protected.iter().any(|id| id == item)
        || matches!(&settings.goal, Goal::Items(id, _) if id == item)
    {
        return false;
    }
    !settings.targets.iter().any(|target| {
        let target = target.as_str();
        target == item || match item {
            "minecraft:cobblestone" => matches!(target, "minecraft:stone" | "minecraft:infested_stone"
                | "minecraft:cobblestone_slab" | "minecraft:cobblestone_stairs" | "minecraft:cobblestone_wall"),
            "minecraft:cobbled_deepslate" => target == "minecraft:deepslate",
            "minecraft:dirt" => matches!(target, "minecraft:grass_block" | "minecraft:coarse_dirt"
                | "minecraft:rooted_dirt" | "minecraft:farmland" | "minecraft:dirt_path"
                | "minecraft:mycelium" | "minecraft:podzol"),
            _ => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_targets_and_defaults_survive_settings_round_trip() {
        let mut settings: Settings = serde_json::from_str(r#"{
            "targets": ["Diamond_ore", "iron_ore", "minecraft:diamond_ore"],
            "radius": 500,
            "area": [[4, 8, 12], [0, 7, 14]],
            "tool": "fortune",
            "goal": {"items": ["diamond", 5]}
        }"#).unwrap();
        settings.normalize();
        assert_eq!(settings.targets, ["minecraft:diamond_ore", "minecraft:iron_ore"]);
        assert_eq!(settings.area, Some(([0,7,12], [4,8,14])));
        assert_eq!(settings.radius, 48);
        assert!(settings.junk.is_empty());
        let restored: Settings = serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(restored.targets, settings.targets);
        assert_eq!(restored.goal, Goal::Items("minecraft:diamond".into(), 5));
        assert_eq!(restored.tool, Tool::Fortune);
    }

    #[test]
    fn only_explicit_filler_can_be_discarded_and_objectives_win() {
        let mut settings = Settings::default();
        assert!(!may_discard(&settings, "minecraft:cobblestone"));
        settings.junk = vec!["minecraft:cobblestone".into(), "minecraft:diamond".into(),
                             "minecraft:dirt".into(), "minecraft:tuff".into()];
        assert!(may_discard(&settings, "minecraft:cobblestone"));
        assert!(!may_discard(&settings, "minecraft:diamond"));
        settings.targets = vec!["minecraft:stone".into()];
        settings.goal = Goal::Items("minecraft:dirt".into(), 10);
        settings.protected = vec!["minecraft:tuff".into()];
        assert!(!may_discard(&settings, "minecraft:cobblestone"));
        assert!(!may_discard(&settings, "minecraft:dirt"));
        assert!(!may_discard(&settings, "minecraft:tuff"));
    }
}
