//! One caster holds ONE instance of a unique crowd control at a time.
//!
//! Nothing enforced this, so a single Mage could keep two enemies polymorphed
//! simultaneously. The priced CC model found the exploit immediately — in
//! `Mage+Priest vs Rogue+Priest` seed 6 it sheeped BOTH enemies (Priest at
//! 23.5s post-gates, Rogue at 26.3s) and then dealt no damage for seven
//! seconds, because `pre_cast_ok`'s friendly-CC guard stops the team attacking
//! anything carrying a break-on-damage aura. Two crowd controls, zero offence.
//!
//! The constraint is keyed per ABILITY rather than per aura type, which is what
//! keeps AoE crowd control working: Psychic Scream fears a whole group and
//! shares `AuraType::Fear` with the Warlock's single-target Fear, but they are
//! different abilities and must not supersede one another.

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::components::AuraType;

/// Polymorph and Fear are marked unique in config; Psychic Scream is not.
#[test]
fn config_marks_single_target_cc_unique_and_leaves_aoe_alone() {
    let defs = load_ability_definitions().expect("abilities.ron loads");
    for ability in [AbilityType::Polymorph, AbilityType::Fear] {
        let d = defs.get(&ability).expect("ability defined");
        let aura = d.applies_aura.as_ref().expect("applies an aura");
        assert!(
            aura.unique_per_caster,
            "{ability:?} is single-target hard CC and must be unique per caster"
        );
    }
    let scream = defs
        .get(&AbilityType::PsychicScream)
        .expect("Psychic Scream defined");
    let aura = scream.applies_aura.as_ref().expect("applies an aura");
    assert!(
        !aura.unique_per_caster,
        "Psychic Scream is AoE — marking it unique would let it fear only one \
         target, since every application shares one caster"
    );
}

/// The behavioural guard: across a match, no caster ever holds two live
/// instances of the same unique ability at once.
#[test]
fn no_caster_ever_holds_two_polymorphs_at_once() {
    // The exact cell and seed that exposed the bug, plus neighbours that also
    // flipped, so a regression cannot hide behind one seed.
    for seed in [6u64, 14, 17, 22] {
        let cfg = HeadlessMatchConfig {
            team1: vec!["Mage".into(), "Priest".into()],
            team2: vec!["Rogue".into(), "Priest".into()],
            random_seed: Some(seed),
            cc_policy: Some("Priced".into()),
            ..Default::default()
        };

        let mut worst = 0usize;
        run_headless_match_observed(cfg, true, None, |frame| {
            // caster -> how many live Polymorphs it owns this frame.
            let mut owned: std::collections::BTreeMap<bevy::prelude::Entity, usize> =
                Default::default();
            for obs in frame.combatants.values() {
                for aura in &obs.auras {
                    if aura.effect_type != AuraType::Polymorph {
                        continue;
                    }
                    if let Some(caster) = aura.caster {
                        *owned.entry(caster).or_default() += 1;
                    }
                }
            }
            worst = worst.max(owned.values().copied().max().unwrap_or(0));
        })
        .expect("match should run");

        assert!(
            worst <= 1,
            "seed {seed}: a caster held {worst} simultaneous Polymorphs; one \
             caster may hold at most one"
        );
    }
}
