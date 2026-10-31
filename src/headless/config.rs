//! JSON configuration parsing for headless mode
//!
//! Parses JSON match configurations and converts them to the game's MatchConfig format.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::str::FromStr;

use crate::states::play_match::talents::{Build, TalentDefinitions};

use crate::states::match_config::{
    ArenaMap, CharacterClass, HunterPetType, MageArmor, MatchConfig, PaladinAura, RogueOpener,
    RoguePoison, WarlockCurse, WarriorShout,
};
use crate::states::play_match::equipment::{
    load_item_definitions, validate_class_restrictions, ItemId, ItemSlot, Loadout,
};

/// Headless match configuration loaded from JSON
///
/// `deny_unknown_fields` because every field here has a serde default: a
/// mistyped key ("seed" instead of "random_seed", a misspelled ai_profile
/// key) would otherwise silently degrade to the default — which once burned a
/// byte-identity comparison by running a "seeded" match randomly seeded. A
/// typo must fail at load with the offending key named. This is the ONLY
/// struct in the config tree (all fields are vectors/maps/options of
/// primitives), so nothing nested needs its own attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadlessMatchConfig {
    /// Team 1 composition (1-3 class names)
    pub team1: Vec<String>,
    /// Team 2 composition (1-3 class names)
    pub team2: Vec<String>,
    /// Arena map name (default: "BasicArena")
    #[serde(default = "default_map")]
    pub map: String,
    /// Team 1's kill target priority (0-based index into enemy team)
    #[serde(default)]
    pub team1_kill_target: Option<usize>,
    /// Team 2's kill target priority (0-based index into enemy team)
    #[serde(default)]
    pub team2_kill_target: Option<usize>,
    /// Team 1's CC target priority (0-based index into enemy team, None = use heuristics)
    #[serde(default)]
    pub team1_cc_target: Option<usize>,
    /// Team 2's CC target priority (0-based index into enemy team, None = use heuristics)
    #[serde(default)]
    pub team2_cc_target: Option<usize>,
    /// Custom output path for match log (optional)
    #[serde(default)]
    pub output_path: Option<String>,
    /// Maximum match duration in seconds (default: 300)
    #[serde(default = "default_max_duration")]
    pub max_duration_secs: f32,
    /// Random seed for deterministic match reproduction
    /// If provided, the match will use a seeded RNG for reproducible results
    #[serde(default)]
    pub random_seed: Option<u64>,
    /// Which AI implementation to run this match under: "Legacy" (default, what
    /// every balance baseline is calibrated against) or "TeamPlan". Omit for
    /// Legacy. See `src/states/play_match/ai_profile.rs`.
    #[serde(default)]
    pub ai_profile: Option<String>,
    /// Per-team overrides for `ai_profile`. Setting these differently pits the
    /// two AI implementations directly against each other on one seed, which a
    /// match-wide profile cannot do — see `AiProfiles`.
    #[serde(default)]
    pub team1_ai_profile: Option<String>,
    #[serde(default)]
    pub team2_ai_profile: Option<String>,
    /// GATED plays (`"Pillar"` — each play stage's config key;
    /// `PlayId::is_gated`) to DENY each team, for measurement: a team under
    /// `TeamPlan` with a play withheld is the paired control the play is
    /// measured against (`situation::WithheldPlays`). A graduated play (Peel,
    /// Hunt, Chain, Burst) is refused: it cannot be withheld, though its
    /// exceptions can still be replaced (`team1_exceptions`). Empty — the
    /// default — withholds nothing.
    #[serde(default)]
    pub team1_withheld_plays: Vec<String>,
    #[serde(default)]
    pub team2_withheld_plays: Vec<String>,
    /// `false` turns the healer pillar modes off for that team, for
    /// measurement (AS-373, `class_ai::pillar_mode`): a `TeamPlan` team
    /// without them plays as `TeamPlan` did before them, so a head-to-head
    /// with one side on and one off measures the modes alone. Omitted — the
    /// default — leaves them on (they run only under `TeamPlan`).
    #[serde(default)]
    pub team1_pillar_modes: Option<bool>,
    #[serde(default)]
    pub team2_pillar_modes: Option<bool>,
    /// `false` turns the team sight yield off for that team, for measurement
    /// (AS-386, `situation::SightPair`): a `TeamPlan` team without it plays
    /// as `TeamPlan` did before it. Omitted — the default — leaves it on (it
    /// runs only under `TeamPlan`).
    #[serde(default)]
    pub team1_sight_yield: Option<bool>,
    #[serde(default)]
    pub team2_sight_yield: Option<bool>,
    /// `false` turns the team target focus off for that team, for measurement
    /// (AS-389, `class_ai::target_focus`): a `TeamPlan` team without it keeps
    /// a target it cannot see, as `TeamPlan` did before it. Omitted — the
    /// default — leaves it on (it runs only under `TeamPlan`).
    #[serde(default)]
    pub team1_target_focus: Option<bool>,
    #[serde(default)]
    pub team2_target_focus: Option<bool>,
    /// `false` makes that team's sight yield leave its healer's cast in
    /// progress out of rule 1's "cannot move" (AS-385's package
    /// measurement, the variant arm). Omitted — the default — counts it.
    #[serde(default)]
    pub team1_yield_healer_cast: Option<bool>,
    #[serde(default)]
    pub team2_yield_healer_cast: Option<bool>,
    /// Per gated play, the exceptions table to run IN PLACE of the shipped
    /// one for each team, for measurement: `{"Peel": []}` lifts every Peel
    /// context gate, `{"Burst": ["Hunter"]}` stops the Hunter bursting. An
    /// entry is a class, or for a play switched off by context (Peel) a class
    /// in a context: `"Mage/Hunter"`, a Mage peeling for a Hunter. Omitted,
    /// the shipped tables hold.
    #[serde(default)]
    pub team1_exceptions: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub team2_exceptions: BTreeMap<String, Vec<String>>,
    /// DEPRECATED — `team1_exceptions: {"Burst": [...]}` and team 2's. Kept so
    /// the committed sweep generators still run: the classes that do not run
    /// the Burst play, for both teams.
    #[serde(default)]
    pub burst_exceptions: Option<Vec<String>>,
    /// DEPRECATED — `team1_exceptions: {"<play>": []}`. Kept so the committed
    /// sweep generators still run: each play named has every exception lifted
    /// for the team.
    #[serde(default)]
    pub team1_lifted_gates: Vec<String>,
    #[serde(default)]
    pub team2_lifted_gates: Vec<String>,
    /// DEPRECATED — `team1_exceptions: {"Chain": [...]}` and team 2's, as
    /// `burst_exceptions`.
    #[serde(default)]
    pub chain_exceptions: Option<Vec<String>>,
    /// Optional grouping label echoed verbatim into batch-runner output. Lets a
    /// strategy-var sweep keep variants distinct (e.g. "Hunter+Spider vs Mage"
    /// vs "Hunter+Boar vs Mage") when two configs share the same class lists.
    /// Ignored by the single-match and matrix paths.
    #[serde(default)]
    pub label: Option<String>,
    /// Team 1's rogue opener preferences (one per slot: "Ambush" or "CheapShot")
    #[serde(default)]
    pub team1_rogue_openers: Vec<String>,
    /// Team 2's rogue opener preferences (one per slot: "Ambush" or "CheapShot")
    #[serde(default)]
    pub team2_rogue_openers: Vec<String>,
    /// Team 1's rogue weapon-poison preferences (one per slot: "Crippling")
    #[serde(default)]
    pub team1_rogue_poisons: Vec<String>,
    /// Team 2's rogue weapon-poison preferences (one per slot: "Crippling")
    #[serde(default)]
    pub team2_rogue_poisons: Vec<String>,
    /// Team 1's warlock curse preferences: outer vec indexed by slot, inner vec indexed by enemy target
    /// Values: "Agony", "Weakness", "Tongues" (defaults to Agony)
    #[serde(default)]
    pub team1_warlock_curse_prefs: Vec<Option<Vec<String>>>,
    /// Team 2's warlock curse preferences: outer vec indexed by slot, inner vec indexed by enemy target
    #[serde(default)]
    pub team2_warlock_curse_prefs: Vec<Option<Vec<String>>>,
    /// Team 1's hunter pet type preferences (one per slot: "Spider", "Boar", "Bird")
    #[serde(default)]
    pub team1_hunter_pet_types: Vec<String>,
    /// Team 2's hunter pet type preferences (one per slot: "Spider", "Boar", "Bird")
    #[serde(default)]
    pub team2_hunter_pet_types: Vec<String>,
    /// Team 1's equipment overrides: outer vec indexed by slot, inner map is slot_name -> item_name
    #[serde(default)]
    pub team1_equipment: Vec<HashMap<String, String>>,
    /// Team 2's equipment overrides: outer vec indexed by slot, inner map is slot_name -> item_name
    #[serde(default)]
    pub team2_equipment: Vec<HashMap<String, String>>,
    /// Team 1's warrior shout preferences (one per slot)
    #[serde(default)]
    pub team1_warrior_shouts: Vec<String>,
    /// Team 2's warrior shout preferences (one per slot)
    #[serde(default)]
    pub team2_warrior_shouts: Vec<String>,
    /// Team 1's mage armor preferences (one per slot)
    #[serde(default)]
    pub team1_mage_armors: Vec<String>,
    /// Team 2's mage armor preferences (one per slot)
    #[serde(default)]
    pub team2_mage_armors: Vec<String>,
    /// Team 1's paladin aura preferences (one per slot)
    #[serde(default)]
    pub team1_paladin_auras: Vec<String>,
    /// Team 2's paladin aura preferences (one per slot)
    #[serde(default)]
    pub team2_paladin_auras: Vec<String>,
    /// Team 1's talent builds, one per slot, each a map of talent to rank:
    /// `[{"DemonicEmbrace": 2, "FelStamina": 2}]`. A slot with no entry (or
    /// `{}`) takes no talents. Checked against the slot's class and the tree
    /// rules (`talents.ron`) before the match.
    #[serde(default)]
    pub team1_builds: Vec<Build>,
    /// Team 2's talent builds, one per slot.
    #[serde(default)]
    pub team2_builds: Vec<Build>,
}

fn default_map() -> String {
    "BasicArena".to_string()
}

fn default_max_duration() -> f32 {
    300.0
}

/// Mirrors the serde per-field defaults: empty teams (callers must fill them
/// in — `validate()` rejects empty teams), `BasicArena`, 300s timeout, no
/// seed, and empty per-class strategy/equipment preference vectors. Lets test
/// helpers write `HeadlessMatchConfig { team1, team2, ..Default::default() }`
/// instead of the full 24-field struct literal.
impl Default for HeadlessMatchConfig {
    fn default() -> Self {
        Self {
            team1: Vec::new(),
            team2: Vec::new(),
            label: None,
            map: default_map(),
            team1_kill_target: None,
            team2_kill_target: None,
            team1_cc_target: None,
            team2_cc_target: None,
            output_path: None,
            max_duration_secs: default_max_duration(),
            random_seed: None,
            ai_profile: None,
            team1_ai_profile: None,
            team2_ai_profile: None,
            team1_withheld_plays: Vec::new(),
            team2_withheld_plays: Vec::new(),
            team1_pillar_modes: None,
            team2_pillar_modes: None,
            team1_sight_yield: None,
            team2_sight_yield: None,
            team1_target_focus: None,
            team2_target_focus: None,
            team1_yield_healer_cast: None,
            team2_yield_healer_cast: None,
            team1_exceptions: BTreeMap::new(),
            team2_exceptions: BTreeMap::new(),
            burst_exceptions: None,
            team1_lifted_gates: Vec::new(),
            team2_lifted_gates: Vec::new(),
            chain_exceptions: None,
            team1_rogue_openers: Vec::new(),
            team2_rogue_openers: Vec::new(),
            team1_rogue_poisons: Vec::new(),
            team2_rogue_poisons: Vec::new(),
            team1_warlock_curse_prefs: Vec::new(),
            team2_warlock_curse_prefs: Vec::new(),
            team1_hunter_pet_types: Vec::new(),
            team2_hunter_pet_types: Vec::new(),
            team1_equipment: Vec::new(),
            team2_equipment: Vec::new(),
            team1_warrior_shouts: Vec::new(),
            team2_warrior_shouts: Vec::new(),
            team1_mage_armors: Vec::new(),
            team2_mage_armors: Vec::new(),
            team1_paladin_auras: Vec::new(),
            team2_paladin_auras: Vec::new(),
            team1_builds: Vec::new(),
            team2_builds: Vec::new(),
        }
    }
}

impl HeadlessMatchConfig {
    /// Load configuration from a JSON file
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let config: HeadlessMatchConfig =
            serde_json::from_str(&contents).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        config.validate()?;
        Ok(config)
    }

    /// Resolve the three profile fields to a per-team [`AiProfiles`].
    ///
    /// THE single authority on the precedence rule: a bare `ai_profile` means
    /// both teams (so every pre-existing config and baseline keeps its
    /// meaning), and `team1_ai_profile` / `team2_ai_profile` override it per
    /// side. Before this existed, the same merge was hand-written in the
    /// headless runner and the replay launcher, and `validate()` checked a
    /// different field list than the runner consumed — the exact class of
    /// drift a second implementation invites.
    pub fn ai_profiles(&self) -> Result<crate::states::play_match::ai_profile::AiProfiles, String> {
        use crate::states::play_match::ai_profile::{AiProfile, AiProfiles};
        let parse = |s: Option<&str>| s.map(AiProfile::parse).transpose();
        let base = parse(self.ai_profile.as_deref())?.unwrap_or_default();
        Ok(AiProfiles {
            team1: parse(self.team1_ai_profile.as_deref())?.unwrap_or(base),
            team2: parse(self.team2_ai_profile.as_deref())?.unwrap_or(base),
        })
    }

    /// Resolve `team1_withheld_plays` / `team2_withheld_plays` and
    /// `team1_exceptions` / `team2_exceptions` to the
    /// [`WithheldPlays`](crate::states::play_match::situation::WithheldPlays)
    /// resource — the one authority, as [`Self::ai_profiles`] is for the
    /// profiles. The deprecated keys (`team{1,2}_lifted_gates`,
    /// `burst_exceptions`, `chain_exceptions`) are read into the same
    /// overrides; a play's exceptions set twice for one team is an error.
    pub fn withheld_plays(
        &self,
    ) -> Result<crate::states::play_match::situation::WithheldPlays, String> {
        use crate::states::play_match::class_ai::plays::parse_play;
        use crate::states::play_match::situation::{ExceptionSet, PlayId, WithheldPlays};
        let mut out = WithheldPlays {
            team1: WithheldPlays::parse_set(&self.team1_withheld_plays)?,
            team2: WithheldPlays::parse_set(&self.team2_withheld_plays)?,
            team1_no_pillar_modes: self.team1_pillar_modes == Some(false),
            team2_no_pillar_modes: self.team2_pillar_modes == Some(false),
            team1_no_sight_yield: self.team1_sight_yield == Some(false),
            team2_no_sight_yield: self.team2_sight_yield == Some(false),
            team1_no_target_focus: self.team1_target_focus == Some(false),
            team2_no_target_focus: self.team2_target_focus == Some(false),
            team1_yield_cast_moves: self.team1_yield_healer_cast == Some(false),
            team2_yield_cast_moves: self.team2_yield_healer_cast == Some(false),
            ..Default::default()
        };
        let mut set_once = |team: u8, id: PlayId, set: ExceptionSet| -> Result<(), String> {
            let overrides = out.exceptions_mut(team);
            if overrides.get(id).is_some() {
                return Err(format!(
                    "Team {team}'s {id:?} exceptions are set twice (a deprecated key and \
                     team{team}_exceptions, or two deprecated keys)"
                ));
            }
            overrides.set(id, set);
            Ok(())
        };
        for (team, tables) in [(1, &self.team1_exceptions), (2, &self.team2_exceptions)] {
            for (play, entries) in tables {
                let id = parse_play(play)?;
                set_once(
                    team,
                    id,
                    WithheldPlays::parse_exceptions(id, entries, Self::parse_class)?,
                )?;
            }
        }
        // The deprecated keys.
        for (team, plays) in [(1, &self.team1_lifted_gates), (2, &self.team2_lifted_gates)] {
            for play in plays {
                set_once(team, parse_play(play)?, ExceptionSet::default())?;
            }
        }
        for (id, classes) in [
            (PlayId::Burst, &self.burst_exceptions),
            (PlayId::Chain, &self.chain_exceptions),
        ] {
            if let Some(classes) = classes {
                let set = WithheldPlays::parse_exceptions(id, classes, Self::parse_class)?;
                for team in [1, 2] {
                    set_once(team, id, set)?;
                }
            }
        }
        Ok(out)
    }

    /// Validate the configuration
    // `pub(crate)` so the batch runner can fail fast on a bad config instead of
    // panicking a worker mid-sweep after the matches have been paid for.
    pub(crate) fn validate(&self) -> Result<(), String> {
        // Validate team sizes
        if self.team1.is_empty() || self.team1.len() > 3 {
            return Err("team1 must have 1-3 members".to_string());
        }
        if self.team2.is_empty() || self.team2.len() > 3 {
            return Err("team2 must have 1-3 members".to_string());
        }

        // Validate class names
        for class_name in self.team1.iter().chain(self.team2.iter()) {
            Self::parse_class(class_name)?;
        }

        // Validate map name
        Self::parse_map(&self.map)?;

        // Validate the AI profile here, not at plugin build: `HeadlessPlugin`
        // `.expect()`s them, so without this a typo'd profile panics with a
        // backtrace instead of getting the clean error every other string field in
        // this config gets. Delegates to `ai_profiles()` so validation and
        // resolution can never disagree on which fields exist — the review that
        // added this check found exactly that divergence once already.
        self.ai_profiles()?;
        self.withheld_plays()?;

        // Validate kill targets
        if let Some(target) = self.team1_kill_target {
            if target >= self.team2.len() {
                return Err(format!(
                    "team1_kill_target {} is out of range (team2 has {} members)",
                    target,
                    self.team2.len()
                ));
            }
        }
        if let Some(target) = self.team2_kill_target {
            if target >= self.team1.len() {
                return Err(format!(
                    "team2_kill_target {} is out of range (team1 has {} members)",
                    target,
                    self.team1.len()
                ));
            }
        }

        // Validate CC targets
        if let Some(target) = self.team1_cc_target {
            if target >= self.team2.len() {
                return Err(format!(
                    "team1_cc_target {} is out of range (team2 has {} members)",
                    target,
                    self.team2.len()
                ));
            }
        }
        if let Some(target) = self.team2_cc_target {
            if target >= self.team1.len() {
                return Err(format!(
                    "team2_cc_target {} is out of range (team1 has {} members)",
                    target,
                    self.team1.len()
                ));
            }
        }

        // Validate max duration
        if self.max_duration_secs <= 0.0 {
            return Err("max_duration_secs must be positive".to_string());
        }

        // Validate equipment overrides against the class that will wear them
        self.validate_equipment()?;

        Ok(())
    }

    /// Check every equipment override against the class in that slot: the item
    /// must exist, the socket must accept its kind, and the class must be able
    /// to equip it (armor type, class list, weapon proficiency).
    ///
    /// A config file is the one place a human writes a loadout by hand, so an
    /// illegal entry is REJECTED here with the offending socket named, rather
    /// than silently dropped at spawn the way a stale in-app override is. The
    /// rule is the same one the equipment picker enforces — `socket_rejection`,
    /// which also answers the question only a named socket can ask: whether the
    /// class may hold a one-hander in its OFF hand — so
    /// headless and graphical agree on what is legal; they differ only in what
    /// they do about it, and only because only one of them has a file to point
    /// at.
    ///
    /// Reads `items.ron` only when an override actually exists, so the
    /// override-free paths (every matrix and sweep run) pay nothing.
    fn validate_equipment(&self) -> Result<(), String> {
        if self.team1_equipment.is_empty() && self.team2_equipment.is_empty() {
            return Ok(());
        }
        let items = load_item_definitions()?;
        for (team, classes, equipment) in [
            (1, &self.team1, &self.team1_equipment),
            (2, &self.team2, &self.team2_equipment),
        ] {
            for (slot_index, raw) in equipment.iter().enumerate() {
                if raw.is_empty() {
                    continue;
                }
                let Some(class_name) = classes.get(slot_index) else {
                    return Err(format!(
                        "team{}_equipment has an entry for slot {} but team{} has only {} members",
                        team,
                        slot_index,
                        team,
                        classes.len()
                    ));
                };
                let class = Self::parse_class(class_name)?;
                let overrides = Self::parse_equipment_map(raw)?;
                validate_class_restrictions(class, &overrides, &items).map_err(|e| {
                    format!("team{} slot {} ({}): {}", team, slot_index, class_name, e)
                })?;
            }
        }
        Ok(())
    }

    /// Parse a class name string into CharacterClass
    fn parse_class(name: &str) -> Result<CharacterClass, String> {
        match name {
            "Warrior" => Ok(CharacterClass::Warrior),
            "Mage" => Ok(CharacterClass::Mage),
            "Rogue" => Ok(CharacterClass::Rogue),
            "Priest" => Ok(CharacterClass::Priest),
            "Warlock" => Ok(CharacterClass::Warlock),
            "Paladin" => Ok(CharacterClass::Paladin),
            "Hunter" => Ok(CharacterClass::Hunter),
            "Shaman" => Ok(CharacterClass::Shaman),
            "Druid" => Ok(CharacterClass::Druid),
            _ => Err(format!(
                "Unknown class: '{}'. Valid classes: Warrior, Mage, Rogue, Priest, Warlock, Paladin, Hunter, Shaman, Druid",
                name
            )),
        }
    }

    /// Parse a map name string into ArenaMap.
    ///
    /// Public so the matrix runner can validate its `--matrix-map` lever with
    /// the same parser the single-match path uses. NOTE: this accepts
    /// `TestVerticality` (a test asset) — callers that must exclude it from
    /// real play (the matrix runner) reject that variant explicitly after
    /// parsing.
    pub fn parse_map(name: &str) -> Result<ArenaMap, String> {
        match name {
            "BasicArena" => Ok(ArenaMap::BasicArena),
            "PillaredArena" => Ok(ArenaMap::PillaredArena),
            "TwinPillars" => Ok(ArenaMap::TwinPillars),
            "TestVerticality" => Ok(ArenaMap::TestVerticality),
            _ => Err(format!(
                "Unknown map: '{}'. Valid maps: BasicArena, PillaredArena, TwinPillars, TestVerticality",
                name
            )),
        }
    }

    /// Parse a rogue opener name string into RogueOpener
    fn parse_rogue_opener(name: &str) -> RogueOpener {
        match name {
            "CheapShot" | "Cheap Shot" => RogueOpener::CheapShot,
            _ => RogueOpener::Ambush, // Default to Ambush for unknown values
        }
    }

    /// Parse a rogue poison name string into RoguePoison: the variant name
    /// (`"Deadly"`) or the display name (`"Deadly Poison"`). An unknown name is
    /// an error rather than a silent Crippling, so a misspelt sweep arm cannot
    /// measure the default poison under another poison's label.
    pub(crate) fn parse_rogue_poison(name: &str) -> Result<RoguePoison, String> {
        RoguePoison::ALL
            .into_iter()
            .find(|poison| name == format!("{poison:?}") || name == poison.name())
            .ok_or_else(|| {
                let known: Vec<String> = RoguePoison::ALL
                    .iter()
                    .map(|poison| format!("{poison:?}"))
                    .collect();
                format!(
                    "Unknown rogue poison: {name:?} (expected one of {})",
                    known.join(", ")
                )
            })
    }

    /// Parse a hunter pet type name string into HunterPetType
    fn parse_hunter_pet_type(name: &str) -> HunterPetType {
        match name {
            "Boar" => HunterPetType::Boar,
            "Bird" => HunterPetType::Bird,
            _ => HunterPetType::Spider, // Default to Spider for unknown values
        }
    }

    /// Parse a warrior shout name string into WarriorShout
    fn parse_warrior_shout(name: &str) -> WarriorShout {
        match name.to_lowercase().as_str() {
            "battle" | "battleshout" | "battle_shout" | "battle shout" => WarriorShout::BattleShout,
            "demoralizing" | "demoralizingshout" | "demoralizing_shout" | "demoralizing shout" => {
                WarriorShout::DemoralizingShout
            }
            "commanding" | "commandingshout" | "commanding_shout" | "commanding shout" => {
                WarriorShout::CommandingShout
            }
            _ => WarriorShout::default(),
        }
    }

    /// Parse a mage armor name string into MageArmor
    fn parse_mage_armor(name: &str) -> MageArmor {
        match name.to_lowercase().as_str() {
            "frost" | "frostarmor" | "frost_armor" | "frost armor" => MageArmor::FrostArmor,
            "mage" | "magearmor" | "mage_armor" | "mage armor" => MageArmor::MageArmor,
            "molten" | "moltenarmor" | "molten_armor" | "molten armor" => MageArmor::MoltenArmor,
            _ => MageArmor::default(),
        }
    }

    /// Parse a paladin aura name string into PaladinAura
    fn parse_paladin_aura(name: &str) -> PaladinAura {
        match name.to_lowercase().as_str() {
            "devotion" | "devotionaura" | "devotion_aura" | "devotion aura" => {
                PaladinAura::DevotionAura
            }
            "shadow"
            | "shadowresistance"
            | "shadow_resistance"
            | "shadow resistance"
            | "shadow resistance aura"
            | "shadowresistanceaura" => PaladinAura::ShadowResistanceAura,
            "concentration" | "concentrationaura" | "concentration_aura" | "concentration aura" => {
                PaladinAura::ConcentrationAura
            }
            _ => PaladinAura::default(),
        }
    }

    /// Parse a warlock curse name string into WarlockCurse
    fn parse_warlock_curse(name: &str) -> WarlockCurse {
        match name {
            "Weakness" | "CurseOfWeakness" | "Curse of Weakness" => WarlockCurse::Weakness,
            "Tongues" | "CurseOfTongues" | "Curse of Tongues" => WarlockCurse::Tongues,
            _ => WarlockCurse::Agony, // Default to Agony for unknown values
        }
    }

    /// Parse an item slot name string into ItemSlot
    fn parse_item_slot(name: &str) -> Result<ItemSlot, String> {
        match name {
            "Head" => Ok(ItemSlot::Head),
            "Neck" => Ok(ItemSlot::Neck),
            "Shoulders" => Ok(ItemSlot::Shoulders),
            "Back" => Ok(ItemSlot::Back),
            "Chest" => Ok(ItemSlot::Chest),
            "Wrists" => Ok(ItemSlot::Wrists),
            "Hands" => Ok(ItemSlot::Hands),
            "Waist" => Ok(ItemSlot::Waist),
            "Legs" => Ok(ItemSlot::Legs),
            "Feet" => Ok(ItemSlot::Feet),
            "Ring1" => Ok(ItemSlot::Ring1),
            "Ring2" => Ok(ItemSlot::Ring2),
            "Trinket1" => Ok(ItemSlot::Trinket1),
            "Trinket2" => Ok(ItemSlot::Trinket2),
            "MainHand" => Ok(ItemSlot::MainHand),
            "OffHand" => Ok(ItemSlot::OffHand),
            "Ranged" => Ok(ItemSlot::Ranged),
            _ => Err(format!(
                "Unknown item slot: '{}'. Valid slots: Head, Neck, Shoulders, Back, Chest, Wrists, Hands, Waist, Legs, Feet, Ring1, Ring2, Trinket1, Trinket2, MainHand, OffHand, Ranged",
                name
            )),
        }
    }

    /// Parse a string-keyed equipment map into typed ItemSlot/ItemId map
    fn parse_equipment_map(map: &HashMap<String, String>) -> Result<Loadout, String> {
        let mut result = Loadout::new();
        for (slot_str, item_str) in map {
            let slot = Self::parse_item_slot(slot_str)?;
            // `ItemId`'s own `FromStr`, not a parser local to this module: the
            // variant name is the id's canonical string form, so this accepts
            // exactly the keys `items.ron` defines. A hand-maintained match
            // here is what let every tier-1 item be unnameable from a config.
            let item = ItemId::from_str(item_str)?;
            result.insert(slot, item);
        }
        Ok(result)
    }

    /// Parse equipment overrides from JSON, resizing to team size with empty defaults
    fn parse_equipment_overrides(
        raw: &[HashMap<String, String>],
        team_size: usize,
    ) -> Result<Vec<Loadout>, String> {
        let mut result = Vec::with_capacity(team_size);
        for (i, map) in raw.iter().enumerate() {
            if i >= team_size {
                break;
            }
            result.push(Self::parse_equipment_map(map)?);
        }
        // Pad remaining slots with empty maps
        result.resize(team_size, Loadout::new());
        Ok(result)
    }

    /// Parse warlock curse preferences from JSON format
    /// Outer vec indexed by slot, inner vec indexed by enemy target
    fn parse_warlock_curse_prefs(
        prefs: &[Option<Vec<String>>],
        team_size: usize,
        enemy_size: usize,
    ) -> Vec<Vec<WarlockCurse>> {
        let mut result = Vec::with_capacity(team_size);
        for slot in 0..team_size {
            let slot_prefs = prefs
                .get(slot)
                .and_then(|opt| opt.as_ref())
                .map(|curses| {
                    let mut parsed: Vec<WarlockCurse> = curses
                        .iter()
                        .map(|s| Self::parse_warlock_curse(s))
                        .collect();
                    parsed.resize(enemy_size, WarlockCurse::default());
                    parsed
                })
                .unwrap_or_else(|| vec![WarlockCurse::default(); enemy_size]);
            result.push(slot_prefs);
        }
        result
    }

    /// Convert to the game's MatchConfig format
    pub fn to_match_config(&self) -> Result<MatchConfig, String> {
        let team1: Vec<Option<CharacterClass>> = self
            .team1
            .iter()
            .map(|s| Self::parse_class(s).map(Some))
            .collect::<Result<Vec<_>, _>>()?;

        let team2: Vec<Option<CharacterClass>> = self
            .team2
            .iter()
            .map(|s| Self::parse_class(s).map(Some))
            .collect::<Result<Vec<_>, _>>()?;

        let map = Self::parse_map(&self.map)?;

        // Parse rogue openers, defaulting to Ambush for missing entries
        let mut team1_rogue_openers: Vec<RogueOpener> = self
            .team1_rogue_openers
            .iter()
            .map(|s| Self::parse_rogue_opener(s))
            .collect();
        team1_rogue_openers.resize(team1.len(), RogueOpener::default());

        let mut team2_rogue_openers: Vec<RogueOpener> = self
            .team2_rogue_openers
            .iter()
            .map(|s| Self::parse_rogue_opener(s))
            .collect();
        team2_rogue_openers.resize(team2.len(), RogueOpener::default());

        // Parse rogue poisons, defaulting to Crippling for missing entries
        let mut team1_rogue_poisons: Vec<RoguePoison> = self
            .team1_rogue_poisons
            .iter()
            .map(|s| Self::parse_rogue_poison(s))
            .collect::<Result<_, _>>()?;
        team1_rogue_poisons.resize(team1.len(), RoguePoison::default());
        let mut team2_rogue_poisons: Vec<RoguePoison> = self
            .team2_rogue_poisons
            .iter()
            .map(|s| Self::parse_rogue_poison(s))
            .collect::<Result<_, _>>()?;
        team2_rogue_poisons.resize(team2.len(), RoguePoison::default());

        // Parse warlock curse preferences
        let team1_warlock_curse_prefs = Self::parse_warlock_curse_prefs(
            &self.team1_warlock_curse_prefs,
            team1.len(),
            team2.len(),
        );
        let team2_warlock_curse_prefs = Self::parse_warlock_curse_prefs(
            &self.team2_warlock_curse_prefs,
            team2.len(),
            team1.len(),
        );

        // Parse hunter pet types, defaulting to Spider for missing entries
        let mut team1_hunter_pet_types: Vec<HunterPetType> = self
            .team1_hunter_pet_types
            .iter()
            .map(|s| Self::parse_hunter_pet_type(s))
            .collect();
        team1_hunter_pet_types.resize(team1.len(), HunterPetType::default());

        let mut team2_hunter_pet_types: Vec<HunterPetType> = self
            .team2_hunter_pet_types
            .iter()
            .map(|s| Self::parse_hunter_pet_type(s))
            .collect();
        team2_hunter_pet_types.resize(team2.len(), HunterPetType::default());

        // Parse warrior shout preferences, defaulting to BattleShout for missing entries
        let mut team1_warrior_shouts: Vec<WarriorShout> = self
            .team1_warrior_shouts
            .iter()
            .map(|s| Self::parse_warrior_shout(s))
            .collect();
        team1_warrior_shouts.resize(team1.len(), WarriorShout::default());

        let mut team2_warrior_shouts: Vec<WarriorShout> = self
            .team2_warrior_shouts
            .iter()
            .map(|s| Self::parse_warrior_shout(s))
            .collect();
        team2_warrior_shouts.resize(team2.len(), WarriorShout::default());

        // Parse mage armor preferences, defaulting to FrostArmor for missing entries
        let mut team1_mage_armors: Vec<MageArmor> = self
            .team1_mage_armors
            .iter()
            .map(|s| Self::parse_mage_armor(s))
            .collect();
        team1_mage_armors.resize(team1.len(), MageArmor::default());

        let mut team2_mage_armors: Vec<MageArmor> = self
            .team2_mage_armors
            .iter()
            .map(|s| Self::parse_mage_armor(s))
            .collect();
        team2_mage_armors.resize(team2.len(), MageArmor::default());

        // Parse paladin aura preferences, defaulting to DevotionAura for missing entries
        let mut team1_paladin_auras: Vec<PaladinAura> = self
            .team1_paladin_auras
            .iter()
            .map(|s| Self::parse_paladin_aura(s))
            .collect();
        team1_paladin_auras.resize(team1.len(), PaladinAura::default());

        let mut team2_paladin_auras: Vec<PaladinAura> = self
            .team2_paladin_auras
            .iter()
            .map(|s| Self::parse_paladin_aura(s))
            .collect();
        team2_paladin_auras.resize(team2.len(), PaladinAura::default());

        // Talent builds, defaulting to empty for missing entries.
        let team1_builds = Self::parse_builds(1, &team1, &self.team1_builds)?;
        let team2_builds = Self::parse_builds(2, &team2, &self.team2_builds)?;

        // Parse equipment overrides, defaulting to empty maps for missing entries
        self.validate_equipment()?;
        let team1_equipment = Self::parse_equipment_overrides(&self.team1_equipment, team1.len())?;
        let team2_equipment = Self::parse_equipment_overrides(&self.team2_equipment, team2.len())?;

        Ok(MatchConfig {
            team1_size: team1.len(),
            team2_size: team2.len(),
            team1,
            team2,
            map,
            team1_kill_target: self.team1_kill_target,
            team2_kill_target: self.team2_kill_target,
            team1_cc_target: self.team1_cc_target,
            team2_cc_target: self.team2_cc_target,
            team1_rogue_openers,
            team2_rogue_openers,
            team1_rogue_poisons,
            team2_rogue_poisons,
            team1_warlock_curse_prefs,
            team2_warlock_curse_prefs,
            team1_hunter_pet_types,
            team2_hunter_pet_types,
            team1_warrior_shouts,
            team2_warrior_shouts,
            team1_mage_armors,
            team2_mage_armors,
            team1_paladin_auras,
            team2_paladin_auras,
            team1_equipment,
            team2_equipment,
            team1_builds,
            team2_builds,
        })
    }

    /// One build per slot of `team`, each checked against its slot's class
    /// and the tree rules — the same check a spawn's resolve makes, so an
    /// illegal build is rejected here with its slot named, never at spawn.
    /// Reads the talent table only when a build takes a talent.
    fn parse_builds(
        team: u8,
        classes: &[Option<CharacterClass>],
        builds: &[Build],
    ) -> Result<Vec<Build>, String> {
        if builds.len() > classes.len() {
            return Err(format!(
                "team{team}_builds has {} entries but team{team} has only {} members",
                builds.len(),
                classes.len()
            ));
        }
        for (slot, build) in builds.iter().enumerate() {
            if build.is_empty() {
                continue;
            }
            let Some(class) = classes[slot] else {
                continue;
            };
            TalentDefinitions::shipped()
                .validate_build(class, build)
                .map_err(|e| format!("team{team}_builds slot {slot} ({class:?}): {e}"))?;
        }
        let mut parsed = builds.to_vec();
        parsed.resize(classes.len(), Build::default());
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_withheld(json: &str) -> crate::states::play_match::situation::WithheldPlays {
        serde_json::from_str::<HeadlessMatchConfig>(json)
            .unwrap()
            .withheld_plays()
            .unwrap()
    }

    fn withheld_err(json: &str) -> String {
        serde_json::from_str::<HeadlessMatchConfig>(json)
            .unwrap()
            .withheld_plays()
            .unwrap_err()
    }

    /// `team{1,2}_exceptions` reach `WithheldPlays` on their own team, and
    /// only there, keyed by play: an empty table lifts the play's exceptions,
    /// a named one replaces them, and neither withholds a play.
    #[test]
    fn exceptions_reach_their_own_team_by_play() {
        use crate::states::play_match::situation::{ExceptionKey, ExceptionSet, PlayId, PlaySet};
        use CharacterClass::*;
        let one = parse_withheld(
            r#"{"team1":["Mage"],"team2":["Warrior"],
                "team1_exceptions":{"Peel":[],"Burst":["Hunter","Mage"]}}"#,
        );
        let team1 = one.exceptions_for_team(1);
        assert_eq!(team1.get(PlayId::Peel), Some(ExceptionSet::default()));
        assert_eq!(
            team1.get(PlayId::Burst),
            Some(ExceptionSet::of(&[
                ExceptionKey::class(Hunter),
                ExceptionKey::class(Mage)
            ]))
        );
        assert_eq!(team1.get(PlayId::Chain), None);
        assert_eq!(one.exceptions_for_team(2), Default::default());
        assert_eq!(
            (one.for_team(1), one.for_team(2)),
            (PlaySet::default(), PlaySet::default())
        );
        let two = parse_withheld(
            r#"{"team1":["Mage"],"team2":["Warrior"],
                "team2_exceptions":{"Peel":["Mage/Hunter"]}}"#,
        );
        assert_eq!(two.exceptions_for_team(1), Default::default());
        assert_eq!(
            two.exceptions_for_team(2).get(PlayId::Peel),
            Some(ExceptionSet::of(&[ExceptionKey::in_context(Mage, Hunter)]))
        );
        let none = parse_withheld(r#"{"team1":["Mage"],"team2":["Warrior"]}"#);
        assert_eq!(none, Default::default());
        // An unknown play, an unknown class, and an entry of the wrong shape.
        withheld_err(r#"{"team1":["Mage"],"team2":["Warrior"],"team1_exceptions":{"Pell":[]}}"#);
        withheld_err(
            r#"{"team1":["Mage"],"team2":["Warrior"],"team1_exceptions":{"Burst":["Mgae"]}}"#,
        );
        withheld_err(
            r#"{"team1":["Mage"],"team2":["Warrior"],"team1_exceptions":{"Peel":["Mage"]}}"#,
        );
        withheld_err(
            r#"{"team1":["Mage"],"team2":["Warrior"],"team1_exceptions":{"Burst":["Mage/Hunter"]}}"#,
        );
        // A format key (Peel's 3v3 gates): only for a play keyed by format,
        // and only a team of two or three.
        let format = parse_withheld(
            r#"{"team1":["Mage"],"team2":["Warrior"],
                "team1_exceptions":{"Peel":["Mage@3v3","Druid/Hunter"]}}"#,
        );
        assert_eq!(
            format.exceptions_for_team(1).get(PlayId::Peel),
            Some(ExceptionSet::of(&[
                ExceptionKey::in_format(Mage, 3),
                ExceptionKey::in_context(Druid, Hunter)
            ]))
        );
        assert!(!ExceptionSet::of(&[ExceptionKey::in_format(Mage, 3)])
            .contains(ExceptionKey::in_format(Mage, 2)));
        withheld_err(
            r#"{"team1":["Mage"],"team2":["Warrior"],"team1_exceptions":{"Burst":["Mage@3v3"]}}"#,
        );
        withheld_err(
            r#"{"team1":["Mage"],"team2":["Warrior"],"team1_exceptions":{"Peel":["Mage@4v4"]}}"#,
        );
    }

    /// The deprecated keys still parse, into the same overrides the generic
    /// key sets: `team{1,2}_lifted_gates` lifts each named play's exceptions
    /// on its own team, and `burst_exceptions` / `chain_exceptions` replace
    /// the play's table on both. Setting one play's table twice for a team
    /// is an error.
    #[test]
    fn deprecated_exception_keys_are_the_generic_overrides() {
        use crate::states::play_match::situation::{Play, PlaySet};
        let lifted = |team: u8| {
            parse_withheld(&format!(
                r#"{{"team1":["Mage"],"team2":["Warrior"],"team{team}_lifted_gates":["Peel"]}}"#
            ))
        };
        for team in [1u8, 2] {
            let old = lifted(team);
            let new = parse_withheld(&format!(
                r#"{{"team1":["Mage"],"team2":["Warrior"],"team{team}_exceptions":{{"Peel":[]}}}}"#
            ));
            assert_eq!(old, new, "team {team}");
            assert_eq!(
                (old.for_team(1), old.for_team(2)),
                (PlaySet::default(), PlaySet::default())
            );
            assert_ne!(old, Default::default());
        }
        let old = parse_withheld(
            r#"{"team1":["Mage"],"team2":["Warrior"],"burst_exceptions":["Hunter"],
                "chain_exceptions":[]}"#,
        );
        let new = parse_withheld(
            r#"{"team1":["Mage"],"team2":["Warrior"],
                "team1_exceptions":{"Burst":["Hunter"],"Chain":[]},
                "team2_exceptions":{"Burst":["Hunter"],"Chain":[]}}"#,
        );
        assert_eq!(old, new);
        withheld_err(
            r#"{"team1":["Mage"],"team2":["Warrior"],"team1_lifted_gates":["Peel"],
                "team1_exceptions":{"Peel":[]}}"#,
        );
        withheld_err(
            r#"{"team1":["Mage"],"team2":["Warrior"],"burst_exceptions":[],
                "team2_exceptions":{"Burst":[]}}"#,
        );
        withheld_err(r#"{"team1":["Mage"],"team2":["Warrior"],"team1_lifted_gates":["Pell"]}"#);
        // Withholding stays its own key.
        let withheld = parse_withheld(
            r#"{"team1":["Mage"],"team2":["Warrior"],"team2_withheld_plays":["Pillar"]}"#,
        );
        assert_eq!(
            (withheld.for_team(1), withheld.for_team(2)),
            (PlaySet::default(), PlaySet::of(&[Play::Pillar]))
        );
    }

    /// Only a GATED play can be withheld: a graduated one (Peel, Hunt, Chain,
    /// Burst) is refused by name with the override that is left, and so is a
    /// deleted one (TakeCover). Its exceptions can still be lifted.
    #[test]
    fn a_graduated_play_cannot_be_withheld() {
        for play in ["Peel", "Hunt", "Chain", "Burst"] {
            for team in [1, 2] {
                let err = withheld_err(&format!(
                    r#"{{"team1":["Mage"],"team2":["Warrior"],"team{team}_withheld_plays":["{play}"]}}"#
                ));
                assert!(err.contains("graduated"), "{play}: {err}");
                assert!(err.contains("team1_exceptions"), "{play}: {err}");
            }
            parse_withheld(&format!(
                r#"{{"team1":["Mage"],"team2":["Warrior"],"team1_exceptions":{{"{play}":[]}}}}"#
            ));
        }
        let err = withheld_err(
            r#"{"team1":["Mage"],"team2":["Warrior"],"team1_withheld_plays":["TakeCover"]}"#,
        );
        assert!(err.contains("Unknown play"), "{err}");
        // `validate` refuses the config, so a batch fails fast on it.
        let cfg: HeadlessMatchConfig = serde_json::from_str(
            r#"{"team1":["Mage"],"team2":["Warrior"],"team1_withheld_plays":["Burst"]}"#,
        )
        .unwrap();
        assert!(cfg.validate().is_err());
    }

    /// Scenario 5: the headless map-name parser accepts "TestVerticality".
    #[test]
    fn parse_map_accepts_test_verticality() {
        assert_eq!(
            HeadlessMatchConfig::parse_map("TestVerticality").unwrap(),
            ArenaMap::TestVerticality
        );
    }

    /// An unknown map name still errors, and the error lists TestVerticality
    /// among the valid options.
    #[test]
    fn parse_map_rejects_unknown_and_lists_test_verticality() {
        let err = HeadlessMatchConfig::parse_map("Nonsense").unwrap_err();
        assert!(
            err.contains("TestVerticality"),
            "error should list the new map: {}",
            err
        );
    }

    /// The actual incident: `"seed"` instead of `"random_seed"` used to parse
    /// fine and run the match randomly seeded — silently invalidating a
    /// byte-identity comparison. It must now fail at parse time with the
    /// offending key named in the error.
    #[test]
    fn unknown_key_fails_to_parse_and_names_the_key() {
        let json = r#"{"team1":["Warrior"],"team2":["Mage"],"seed":42}"#;
        let err = serde_json::from_str::<HeadlessMatchConfig>(json)
            .expect_err("a config with an unknown key must not parse");
        let msg = err.to_string();
        assert!(
            msg.contains("seed"),
            "error must name the offending key: {}",
            msg
        );
    }

    /// A valid config parses identically to before the deny_unknown_fields
    /// change: explicit keys land in their fields, omitted keys take their
    /// serde defaults.
    #[test]
    fn valid_config_parses_identically() {
        let json = r#"{"team1":["Warrior","Priest"],"team2":["Mage"],"map":"TwinPillars","random_seed":42,"team2_kill_target":1,"ai_profile":"Legacy"}"#;
        let config: HeadlessMatchConfig =
            serde_json::from_str(json).expect("valid config must still parse");
        assert_eq!(config.team1, vec!["Warrior", "Priest"]);
        assert_eq!(config.team2, vec!["Mage"]);
        assert_eq!(config.map, "TwinPillars");
        assert_eq!(config.random_seed, Some(42));
        assert_eq!(config.team2_kill_target, Some(1));
        assert_eq!(config.ai_profile.as_deref(), Some("Legacy"));
        // Omitted keys still take their serde defaults.
        assert_eq!(config.max_duration_secs, default_max_duration());
        assert_eq!(config.team1_kill_target, None);
        assert!(config.team1_hunter_pet_types.is_empty());
        config.validate().expect("valid config must still validate");
    }

    /// A config file is the one place a loadout is written by hand, so an
    /// illegal override is rejected at load with the socket named — the same
    /// treatment a bad class name or map name gets.
    #[test]
    fn an_override_the_class_cannot_equip_is_rejected() {
        // The AS-47 Tester's case: a helm in a ring socket.
        let json = r#"{"team1":["Warrior"],"team2":["Mage"],"team1_equipment":[{"Ring1":"LionheartHelm"}]}"#;
        let config: HeadlessMatchConfig = serde_json::from_str(json).expect("parses");
        let err = config
            .validate()
            .expect_err("a helm in a ring socket must not validate");
        assert!(err.contains("Ring1"), "error must name the socket: {}", err);
        config
            .to_match_config()
            .expect_err("the conversion must reject it too");

        // The card's case: a Mage with a two-handed axe.
        let json = r#"{"team1":["Mage"],"team2":["Warrior"],"team1_equipment":[{"MainHand":"ArcaniteReaper"}]}"#;
        let config: HeadlessMatchConfig = serde_json::from_str(json).expect("parses");
        let err = config.validate().expect_err("a Mage may not wield an axe");
        assert!(
            err.contains("Mage cannot use two-handed axes"),
            "error must name the failing gate: {}",
            err
        );
    }

    /// The card's regression: a tier-1 item that `items.ron` defines and the
    /// client equips used to be rejected on its NAME, before any equip rule
    /// ran, because the parser carried its own list of items.
    #[test]
    fn a_tier_one_item_parses_and_reaches_the_equip_gates() {
        let json = r#"{"team1":["Warrior"],"team2":["Mage"],"team1_equipment":[{"MainHand":"BloodlordsBattleaxe"}]}"#;
        let config: HeadlessMatchConfig = serde_json::from_str(json).expect("parses");
        config.validate().expect("a Warrior may wield a tier-1 axe");
        let match_config = config.to_match_config().expect("converts");
        assert_eq!(
            match_config.team1_equipment[0].get(&ItemSlot::MainHand),
            Some(&ItemId::BloodlordsBattleaxe)
        );
    }

    /// And when a tier-1 item IS illegal, the error is now about the item
    /// rather than about the name — the AS-59 gates get to speak at all.
    #[test]
    fn an_illegal_tier_one_item_fails_on_the_gate_not_the_name() {
        let json = r#"{"team1":["Mage"],"team2":["Warrior"],"team1_equipment":[{"MainHand":"BloodlordsBattleaxe"}]}"#;
        let config: HeadlessMatchConfig = serde_json::from_str(json).expect("parses");
        let err = config.validate().expect_err("a Mage may not wield an axe");
        assert!(
            err.contains("Mage cannot use two-handed axes"),
            "error must name the failing gate, not the item name: {}",
            err
        );
    }

    /// Every item the game defines is nameable from a config. The parser has
    /// no list of its own, so this can only fail if `ItemId`'s string form
    /// stops matching the enum — which is the point.
    #[test]
    fn every_item_id_is_nameable_from_a_config() {
        for id in ItemId::all() {
            let mut map = HashMap::new();
            map.insert("Head".to_string(), id.as_str().to_string());
            let parsed = HeadlessMatchConfig::parse_equipment_map(&map)
                .unwrap_or_else(|e| panic!("{:?} is not nameable from a config: {}", id, e));
            assert_eq!(parsed.get(&ItemSlot::Head), Some(id));
        }
    }

    /// A legal override still loads, and lands in the parsed config.
    #[test]
    fn a_legal_override_still_loads() {
        let json = r#"{"team1":["Mage"],"team2":["Warrior"],"team1_equipment":[{"MainHand":"CrescentStaff"}]}"#;
        let config: HeadlessMatchConfig = serde_json::from_str(json).expect("parses");
        config.validate().expect("a Mage may wield a staff");
        let match_config = config.to_match_config().expect("converts");
        assert_eq!(
            match_config.team1_equipment[0].get(&ItemSlot::MainHand),
            Some(&ItemId::CrescentStaff)
        );
    }
}
