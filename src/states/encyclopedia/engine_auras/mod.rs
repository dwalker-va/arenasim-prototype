//! The engine aura registry — one FILE per family of auras the engine applies
//! from code, which the `abilities.ron` walk cannot see.
//!
//! ## Adding a family is adding a file
//!
//! Every `.rs` file in this directory other than this one is a family. The
//! build script lists the directory and generates the module declarations,
//! the [`EngineFamily`] enum (one variant per file, named after it:
//! `frost_trap_slow.rs` is `EngineFamily::FrostTrapSlow`) and its dispatch
//! (`build.rs`, `engine_aura_families`). So a new family is a new file and
//! nothing else: no shared list to edit, so two cards that each add one
//! cannot collide in it.
//!
//! A family file answers two questions, as two free functions:
//!
//! - `pub fn members(source: &CatalogSource) -> Vec<EngineKey>` — the auras of
//!   this family that `source` holds. Read from the data its auras come from,
//!   so a family whose source is absent lists nothing: Weakened Soul is listed
//!   only while the ability table holds Power Word: Shield, a talent's aura
//!   only while the talent table holds the talent. That is what lets a
//!   catalogue over a PINNED table (the encyclopedia snapshots) leave out an
//!   aura a later card adds.
//! - `pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec>`
//!   — the entry one member resolves to, read from the constant, shared
//!   constructor or spec the apply site reads, so a page cannot state a
//!   number the simulation does not use. `None` for a key that is not a
//!   member (a stale navigation stack can still hold one).
//!
//! A missing function, or one with the wrong signature, fails the build at
//! the generated dispatch, naming the file.
//!
//! ## Name collisions
//!
//! Three engine auras carry a `frame_name` that is NOT their catalog name,
//! because the engine hangs two distinct DEBUFFS under one name and a player
//! reading their frames cannot tell which is which:
//!
//! - "Crippling Poison" is both the Rogue's own coating marker and the slow it
//!   puts on the target.
//! - "Unstable Affliction" is both an 18-second Shadow damage-over-time and the
//!   silence its dispel backlash inflicts.
//! - "Frost Armor" is the Mage's self-buff AND the chill it hangs on melee
//!   attackers.
//!
//! Each gets its own entry with a parenthetical qualifier, and its page says
//! what the frames call it. The alternative — one entry per NAME — sends a
//! player from their own gold-bordered buff to the enemy debuff's page.
//!
//! **A collision is two debuffs sharing a name, never two EFFECTS of one
//! debuff.** Frost Armor's chill used to be listed twice, once per effect, and
//! the two rows wore different removal badges over what a player experiences as
//! one thing. That is fixed upstream in the engine rather than papered over
//! here: the chill's effects are bound into a [`CompoundDebuff`](crate::states::play_match::components::CompoundDebuff), so it is one
//! entry that states both (see [`NamedAura::riders`](super::auras::NamedAura::riders)).
//!
//! ## Formatting
//!
//! `cargo fmt` reaches a module only through a `mod` item it can read in the
//! source, and these are declared in generated code, so it skips them.
//! `family_files_are_rustfmt_clean` below holds them to `rustfmt` instead:
//! run `rustfmt --edition 2021 src/states/encyclopedia/engine_auras/*.rs`
//! after editing one.

use crate::states::match_config::RoguePoison;
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::components::{Aura, AuraType, DispelType, TotemElement};
use crate::states::play_match::equipment::ItemId;
use crate::states::play_match::talents::TalentId;

use super::auras::{AuraArt, AuraSource, CatalogSource, Persistence};

include!(concat!(env!("OUT_DIR"), "/engine_aura_families.rs"));

/// Which member of a family an address names. A family of one aura is
/// [`EngineKey::Unit`]; a family that expands from a source names the source.
///
/// This is the one shared list here, and it grows only with a new KIND of
/// source, never with a new family or a new member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKey {
    /// The family's only aura.
    Unit,
    /// One per ability (an interrupt's lockout).
    Ability(AbilityType),
    /// One per item (a proc trinket's buff).
    Item(ItemId),
    /// One per totem element.
    Totem(TotemElement),
    /// One per Rogue poison.
    Poison(RoguePoison),
    /// A talent's proc, by its index among the talent's `Proc` effects.
    TalentProc(TalentId, u8),
}

/// Everything a family's `entry` answers. A struct rather than a long tuple
/// because two of the fields (the sample's break-on-damage threshold, and the
/// frame name) are exactly the ones a wrong default gets silently wrong.
pub struct EngineSpec {
    /// The catalog's name for the entry.
    pub name: String,
    /// What the actor frames call it. `None` means "same as `name`".
    pub frame_name: Option<String>,
    pub mechanic: AuraType,
    pub source: AuraSource,
    /// Where its icon comes from. Not defaulted: a wrong borrow renders a
    /// plausible icon that means something else, and a missing one renders
    /// the placeholder tile.
    pub art: AuraArt,
    pub magnitude: f32,
    pub school: Option<SpellSchool>,
    /// Read from the constant the apply site reads. NOT defaulted: Shadow Sight
    /// once applied `0.0` (breaks on any damage) where every other engine aura
    /// applies `-1.0`, and a shared default printed "Never" on a page for an
    /// aura the first hit removed. Every entry names its own value so the page
    /// can only ever say what the engine does.
    pub break_on_damage: f32,
    /// The removal class, read from the constructor the apply site uses
    /// wherever there is one. Not defaulted, for the same reason as
    /// `break_on_damage`: a proc trinket's buff is `Physical`, and a page
    /// defaulting it to `Auto` would advertise a purge that cannot happen.
    pub dispel_type: DispelType,
    pub persistence: Persistence,
    /// The RIDER effects of a compound debuff — the effects this entry covers
    /// beyond its face. Empty for every ordinary aura. See
    /// [`CompoundDebuff`](crate::states::play_match::components::CompoundDebuff);
    /// the family builds these from the same constructor the apply site uses,
    /// so the page lists exactly what lands.
    pub riders: Vec<Aura>,
    pub provenance: String,
}

impl EngineSpec {
    /// The riders of every family that is not a compound debuff. Spelled out
    /// as a helper rather than a `Default` impl so the other fields stay
    /// mandatory — `break_on_damage` in particular must never default.
    pub fn no_riders() -> Vec<Aura> {
        Vec::new()
    }
}

/// The address of one engine-applied aura: its family and which member.
/// `Copy` and cheap to compare, so it slots into `Topic` like every other
/// address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineAura {
    pub family: EngineFamily,
    pub key: EngineKey,
}

impl EngineAura {
    pub fn new(family: EngineFamily, key: EngineKey) -> Self {
        Self { family, key }
    }

    /// The only aura of a one-aura family.
    pub fn unit(family: EngineFamily) -> Self {
        Self::new(family, EngineKey::Unit)
    }

    /// Every engine aura `source` holds, family by family in the order of
    /// its `families`.
    pub fn all(source: &CatalogSource) -> Vec<EngineAura> {
        source
            .families
            .iter()
            .flat_map(|family| {
                family
                    .members(source)
                    .into_iter()
                    .map(|key| EngineAura::new(*family, key))
            })
            .collect()
    }

    /// The spec this address resolves to, or `None` for an address `source`
    /// does not hold.
    pub fn spec(self, source: &CatalogSource) -> Option<EngineSpec> {
        if !source.families.contains(&self.family) {
            return None;
        }
        self.family.entry(self.key, source)
    }
}

/// Whether `source` holds `ability`: the gate of every family an ability
/// applies from code.
pub fn has_ability(source: &CatalogSource, ability: AbilityType) -> bool {
    source.abilities.get(&ability).is_some()
}

/// `vec![EngineKey::Unit]` when `present`, else nothing: the members of a
/// one-aura family.
pub fn unit_if(present: bool) -> Vec<EngineKey> {
    if present {
        vec![EngineKey::Unit]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family_files() -> Vec<std::path::PathBuf> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/states/encyclopedia/engine_auras");
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
            .map(|entry| entry.expect("a directory entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .collect();
        files.sort();
        files
    }

    /// The generated enum has exactly one variant per family file, named
    /// after it — the build script's whole job, checked against the directory
    /// itself rather than against the script's own reading of it.
    #[test]
    fn every_family_file_is_a_variant() {
        let stems: Vec<String> = family_files()
            .iter()
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .filter(|stem| stem != "mod")
            .collect();
        let variants: Vec<String> = EngineFamily::ALL
            .iter()
            .map(|family| family.file_stem().to_string())
            .collect();
        assert_eq!(variants, stems);
        for family in EngineFamily::ALL {
            let camel: String = family
                .file_stem()
                .split('_')
                .map(|word| {
                    let mut chars = word.chars();
                    chars.next().map_or(String::new(), |first| {
                        first.to_ascii_uppercase().to_string() + chars.as_str()
                    })
                })
                .collect();
            assert_eq!(format!("{family:?}"), camel);
        }
    }

    /// `cargo fmt` cannot see the family files (they are declared in
    /// generated code), so this is what holds them to `rustfmt`: the same
    /// check `cargo fmt --check` makes of every other file, run on these.
    #[test]
    fn family_files_are_rustfmt_clean() {
        let files = family_files();
        assert!(files.len() > 1, "no family files found");
        let output = std::process::Command::new("rustfmt")
            .args(["--edition", "2021", "--check"])
            .args(&files)
            .output()
            .expect("rustfmt must be installed (rust-toolchain.toml lists it)");
        assert!(
            output.status.success(),
            "an engine aura family file is not rustfmt-clean; run \
             `rustfmt --edition 2021 src/states/encyclopedia/engine_auras/*.rs`:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
