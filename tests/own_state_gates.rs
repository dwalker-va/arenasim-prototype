//! A caster's own gates hold in a played match (AS-282).
//!
//! The decision-property grid (`tests/decision_properties.rs`) checks every
//! choice against silence, school lockout and Travel Form one decision at a
//! time. This plays the case a real match produces: a Mage interrupted on a
//! Frostbolt is locked out of Frost, and Ice Barrier and Frost Armor are Frost
//! spells. Locked out, it casts neither; the moment the lockout ends, it casts
//! both — so the lockout, and nothing else in the scene, is what held them.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::MageArmor;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::{AbilityType, Aura, AuraType};
use common::staged::{entity_id, play, sim_time};

const MAGE: Unit = Unit::new(1, 0);
const WARRIOR: Unit = Unit::new(2, 0);

/// How long the staged Frost lockout lasts.
const LOCKOUT: f32 = 2.0;

#[test]
fn a_mage_locked_out_of_frost_casts_neither_ice_barrier_nor_frost_armor() {
    let config = HeadlessMatchConfig {
        team1: vec!["Mage".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(1),
        max_duration_secs: 8.0,
        ..Default::default()
    };
    let lockout = Aura {
        effect_type: AuraType::SpellSchoolLockout,
        duration: LOCKOUT,
        magnitude: SpellSchool::Frost.to_lockout_magnitude(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    // The Warrior starts far enough off that nothing touches the Mage for the
    // whole window and the one after it: the Mage stays at full health, so
    // Ice Barrier's own trigger holds throughout.
    let played = play(
        config,
        Stage::new()
            .place(MAGE, -40.0, 0.0)
            .place(WARRIOR, 40.0, 0.0)
            .clear_auras(MAGE)
            .aura(MAGE, lockout)
            .combatant(MAGE, |c| {
                c.mage_armor = MageArmor::FrostArmor;
                c.ability_cooldowns.remove(&AbilityType::IceBarrier);
                c.global_cooldown = 0.0;
            }),
    );
    let landed = played.landed().combat;
    let ends = landed + LOCKOUT;
    let mage = entity_id(&played, 1, 0);
    let mine: Vec<_> = played
        .staged_trace()
        .filter(|e| e["kind"] == "ability_decision" && e["actor"]["entity_id"] == mage)
        .collect();

    for ability in ["IceBarrier", "FrostArmor"] {
        let candidates = |during: bool| {
            mine.iter()
                .filter(move |e| (sim_time(e) < ends - 0.05) == during)
                .flat_map(move |e| {
                    e["candidates"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(move |c| c["ability"] == ability)
                        .map(move |c| (sim_time(e), c))
                })
                .collect::<Vec<_>>()
        };
        let during = candidates(true);
        assert!(
            !during.is_empty(),
            "the Mage never considered {ability} while locked out"
        );
        for (t, c) in &during {
            assert_eq!(
                c["status"], "rejected",
                "{ability} chosen at {t:.2}s, inside the Frost lockout ({landed:.2}..{ends:.2}s)"
            );
            assert_eq!(
                c["reason"]["SilencedOrLocked"]["school"], "Frost",
                "{ability} at {t:.2}s refused for {}, not the lockout",
                c["reason"]
            );
        }
        let chosen = candidates(false)
            .into_iter()
            .find(|(_, c)| c["status"] == "chosen")
            .map(|(t, _)| t);
        assert!(
            chosen.is_some_and(|t| t < ends + 3.0),
            "{ability} not cast once the lockout ended at {ends:.2}s: \
             the lockout was not what held it"
        );
    }
}
