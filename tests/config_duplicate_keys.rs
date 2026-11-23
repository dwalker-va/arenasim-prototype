//! Every config loader refuses a map that names a key twice (AS-504).
//!
//! RON keeps the last value of a repeated map key and says nothing, so a
//! second `Paladin:` block in `reference_builds` used to replace the first
//! silently. Each loader now parses through `arenasim::config_ron`, which
//! refuses the repeat by name, by the path to its map and by line.
//!
//! Each case plants its duplicate by editing the SHIPPED file: it inserts a
//! copy of a key just before a key the file already holds, so the edit can
//! never drift from the real file, and the key RON would have kept is the
//! shipped one. Without the check every map case would load cleanly; the
//! plain-RON controls below show that for the files whose root type is
//! public. maps.ron, movement.ron and banter.ron hold no map today, so their
//! cases repeat a struct field: serde's derive refused that already, and the
//! case pins that the loader still parses through the strict path, which is
//! what covers a map field added to them later.

use arenasim::states::play_match::equipment::{
    load_item_definitions, parse_default_loadouts, parse_item_definitions, ItemsConfig,
    LoadoutsConfig,
};
use arenasim::states::play_match::talents::TalentDefinitions;
use arenasim::states::play_match::{
    parse_ability_definitions, parse_banter_config, parse_map_geometry_config,
    parse_movement_config, AbilitiesConfig, AbilityDefinitions,
};

#[derive(Clone, Copy, Debug)]
enum File {
    Talents,
    Abilities,
    Items,
    Loadouts,
    Maps,
    Movement,
    Banter,
}

impl File {
    fn name(self) -> &'static str {
        match self {
            File::Talents => "talents.ron",
            File::Abilities => "abilities.ron",
            File::Items => "items.ron",
            File::Loadouts => "loadouts.ron",
            File::Maps => "maps.ron",
            File::Movement => "movement.ron",
            File::Banter => "banter.ron",
        }
    }

    fn shipped(self) -> String {
        let path = arenasim::paths::asset_path(&format!("config/{}", self.name()));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
    }

    /// The file's own loader, on `text`.
    fn load(self, text: &str) -> Result<(), String> {
        let origin = self.name();
        match self {
            File::Talents => {
                TalentDefinitions::from_ron(text, AbilityDefinitions::shipped()).map(drop)
            }
            File::Abilities => parse_ability_definitions(text, origin).map(drop),
            File::Items => parse_item_definitions(text, origin).map(drop),
            File::Loadouts => {
                let items = load_item_definitions().expect("items.ron loads");
                parse_default_loadouts(text, origin, &items).map(drop)
            }
            File::Maps => parse_map_geometry_config(text, origin).map(drop),
            File::Movement => parse_movement_config(text, origin).map(drop),
            File::Banter => parse_banter_config(text, origin).map(drop),
        }
    }
}

const ALL_FILES: [File; 7] = [
    File::Talents,
    File::Abilities,
    File::Items,
    File::Loadouts,
    File::Maps,
    File::Movement,
    File::Banter,
];

/// A duplicate planted in a shipped file: `inserted` goes in just before
/// `anchor`, which occurs once in the file and starts with `key`.
struct Plant {
    file: File,
    anchor: &'static str,
    inserted: String,
    key: &'static str,
    /// The path to the map the error must name.
    map: &'static str,
}

impl Plant {
    fn new(
        file: File,
        anchor: &'static str,
        inserted: &str,
        key: &'static str,
        map: &'static str,
    ) -> Self {
        Self {
            file,
            anchor,
            inserted: inserted.to_string(),
            key,
            map,
        }
    }

    /// The shipped file with the duplicate in it, and the line of the
    /// repeated key (the shipped one, read second).
    fn text(&self) -> (String, usize) {
        let shipped = self.file.shipped();
        assert_eq!(
            shipped.matches(self.anchor).count(),
            1,
            "{} has one {:?}",
            self.file.name(),
            self.anchor
        );
        assert!(
            self.anchor.trim_start().starts_with(self.key),
            "the anchor starts with its key"
        );
        let at = shipped.find(self.anchor).unwrap();
        let text = format!("{}{}{}", &shipped[..at], self.inserted, &shipped[at..]);
        let second = at + self.inserted.len();
        let line = 1 + text[..second].matches('\n').count();
        (text, line)
    }

    /// Whether the file's loader refuses the planted duplicate by name, map
    /// and line; `Err` says how it fell short.
    fn check(&self) -> Result<(), String> {
        let (text, line) = self.text();
        let at = format!("{} `{}` in `{}`", self.file.name(), self.key, self.map);
        let err = match self.file.load(&text) {
            Ok(()) => return Err(format!("{at}: the duplicate loaded")),
            Err(err) => err,
        };
        let expected = format!("duplicate key `{}` in `{}`", self.key, self.map);
        if !err.contains(&expected) {
            return Err(format!("{at}: expected {expected:?} in {err}"));
        }
        if !err.contains(&format!(" {line}:")) {
            return Err(format!("{at}: the error does not name line {line}: {err}"));
        }
        Ok(())
    }
}

/// Every plant is refused. Each is checked before any failure is reported, so
/// a failure lists every plant that fell short, not just the first.
fn assert_refused(plants: &[Plant]) {
    let failures: Vec<String> = plants.iter().filter_map(|p| p.check().err()).collect();
    assert!(
        failures.is_empty(),
        "{} of {} plants fell short:\n{}",
        failures.len(),
        plants.len(),
        failures.join("\n")
    );
}

/// The whole `(...)` entry that `opener` begins, from the shipped text: up to
/// the first line that closes it at the opener's indent.
fn entry(file: File, opener: &str) -> String {
    let text = file.shipped();
    assert_eq!(
        text.matches(opener).count(),
        1,
        "{} has one {opener:?}",
        file.name()
    );
    let start = text.find(opener).unwrap();
    let indent = &opener[1..opener.len() - opener.trim_start().len()];
    let close = format!("\n{indent}),\n");
    let end = start + text[start..].find(&close).expect("the entry closes") + close.len();
    // Skip the newline the opener starts with; keep the one the entry ends with.
    text[start + 1..end].to_string()
}

#[test]
fn every_shipped_config_holds_no_duplicate_key() {
    for file in ALL_FILES {
        file.load(&file.shipped())
            .unwrap_or_else(|e| panic!("{}: {e}", file.name()));
    }
}

#[test]
fn talents_refuse_a_duplicate_in_every_map() {
    let talent = entry(File::Talents, "\n        ImprovedDrainLife: (");
    assert_refused(&[
        Plant::new(
            File::Talents,
            "Warlock: [\n            (id: Affliction",
            "Warlock: [],\n        ",
            "Warlock",
            "trees",
        ),
        Plant::new(
            File::Talents,
            "        ImprovedDrainLife: (",
            &talent,
            "ImprovedDrainLife",
            "talents",
        ),
        Plant::new(
            File::Talents,
            "Warlock: {\n            Head: ArcanistCrown",
            "Warlock: {},\n        ",
            "Warlock",
            "reference",
        ),
        Plant::new(
            File::Talents,
            "Head: JudgementCrown,",
            "Head: JudgementCrown,\n            ",
            "Head",
            "reference.Paladin",
        ),
        Plant::new(
            File::Talents,
            "Warlock: [\n            // Siphon Life",
            "Warlock: [],\n        ",
            "Warlock",
            "reference_builds",
        ),
        Plant::new(
            File::Talents,
            "ImprovedDrainLife: 2, Nightfall: 2, SiphonLife: 1",
            "ImprovedDrainLife: 2, ",
            "ImprovedDrainLife",
            "reference_builds.Warlock[0].build",
        ),
    ]);
}

#[test]
fn a_second_class_block_in_reference_builds_is_refused() {
    // Planted ahead of the shipped Paladin block, which RON would have kept,
    // so without the check the load passes with the shipped builds intact.
    let anchor_text = File::Talents.shipped();
    let at = anchor_text.find("reference_builds: {").expect("the map");
    let paladin = anchor_text[at..]
        .find("\n        Paladin: [")
        .map(|i| at + i + 1)
        .expect("the shipped Paladin block");
    let text = format!(
        "{}Paladin: [],\n        {}",
        &anchor_text[..paladin],
        &anchor_text[paladin..]
    );
    let err = File::Talents.load(&text).expect_err("refused");
    assert!(
        err.contains("duplicate key `Paladin` in `reference_builds`"),
        "{err}"
    );
}

#[test]
fn abilities_refuse_a_duplicate_ability() {
    let frostbolt = entry(File::Abilities, "\n        Frostbolt: (");
    let plant = Plant::new(
        File::Abilities,
        "        Frostbolt: (",
        &frostbolt,
        "Frostbolt",
        "abilities",
    );
    assert_refused(std::slice::from_ref(&plant));
    // Control: plain RON takes the same text and keeps the last copy.
    let (text, _) = plant.text();
    ron::from_str::<AbilitiesConfig>(&text).expect("plain RON accepts the repeat");
}

#[test]
fn items_refuse_a_duplicate_item() {
    let helm = entry(File::Items, "\n        LionheartHelm: (");
    let plant = Plant::new(
        File::Items,
        "        LionheartHelm: (",
        &helm,
        "LionheartHelm",
        "items",
    );
    assert_refused(std::slice::from_ref(&plant));
    let (text, _) = plant.text();
    ron::from_str::<ItemsConfig>(&text).expect("plain RON accepts the repeat");
}

#[test]
fn loadouts_refuse_a_duplicate_class_or_slot() {
    let plants = [
        Plant::new(
            File::Loadouts,
            "Warrior: {\n            Head: LionheartHelm",
            "Warrior: {},\n        ",
            "Warrior",
            "loadouts",
        ),
        Plant::new(
            File::Loadouts,
            "Head: LionheartHelm,",
            "Head: LionheartHelm,\n            ",
            "Head",
            "loadouts.Warrior",
        ),
    ];
    assert_refused(&plants);
    for plant in &plants {
        let (text, _) = plant.text();
        ron::from_str::<LoadoutsConfig>(&text).expect("plain RON accepts the repeat");
    }
}

#[test]
fn maps_movement_and_banter_refuse_a_repeated_field() {
    assert_refused(&[
        Plant::new(
            File::Maps,
            "volumes: [],",
            "volumes: [],\n        ",
            "volumes",
            "basic_arena",
        ),
        Plant::new(
            File::Movement,
            "danger_radius: 12.0,",
            "danger_radius: 12.0,\n        ",
            "danger_radius",
            "shared",
        ),
        Plant::new(
            File::Banter,
            "opening_start: 1.5,",
            "opening_start: 1.5,\n        ",
            "opening_start",
            "timing",
        ),
    ]);
}
