//! AS-286 — under `TeamPlan` the Paladin casts Holy Light, its big heal, when
//! there is time to cast it.
//!
//! Flash of Light (cheaper, quicker, on a wider trigger) is tried first under
//! `Legacy`, so Holy Light is never cast there. Under `TeamPlan` Holy Light is
//! tried ahead of it, on the lowest ally in its band (50-85%), when neither the
//! Paladin nor that ally is pressured (`paladin::holy_light_has_time`). Each
//! scene is paired with its one condition removed.

use arenasim::states::match_config::CharacterClass::{Paladin, Warrior};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::{Aura, AuraType};

const PALADIN: &str = "paladin";
const ALLY: &str = "ally";
const ENEMY: &str = "enemy";

/// In Holy Light's band: hurt, not low.
const HURT: f32 = 0.7;

/// Where the enemy Warrior stands, and whom it attacks.
#[derive(Clone, Copy)]
enum Threat {
    /// 40yd off, attacking the ally: beyond `threat_intent_radius` of both,
    /// so nobody is pressured.
    Distant,
    /// Beside the ally, attacking it.
    OnAlly,
    /// Beside the Paladin, attacking it.
    OnPaladin,
}

/// A Paladin at the origin with its aura up and Hammer of Justice down, its
/// teammate Warrior 5yd east at `ally_hp`, and an enemy Warrior placed by
/// `threat`.
fn scene(profile: AiProfile, ally_hp: f32, threat: Threat) -> Scene {
    let (at, whom) = match threat {
        Threat::Distant => ((0.0, 40.0), ALLY),
        Threat::OnAlly => ((7.0, 0.0), ALLY),
        Threat::OnPaladin => ((-2.0, 0.0), PALADIN),
    };
    Scene::new()
        .ai_profile(1, profile)
        .unit(PALADIN, 1, Paladin)
        .cooldown(PALADIN, AbilityType::HammerOfJustice, 30.0)
        .unit(ALLY, 1, Warrior)
        .at(5.0, 0.0)
        .health(ally_hp)
        .unit(ENEMY, 2, Warrior)
        .at(at.0, at.1)
        .targeting(whom)
        .aura_full(
            PALADIN,
            Aura {
                effect_type: AuraType::DamageTakenReduction,
                ability_name: "Devotion Aura".into(),
                duration: 600.0,
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        )
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

fn note(view: &DecisionView) -> Option<String> {
    match view.rejected(AbilityType::HolyLight) {
        Some(RejectionReason::PreconditionUnmet { note }) => Some(note.to_string()),
        _ => None,
    }
}

/// With time to cast it, a `TeamPlan` Paladin heals a hurt ally with Holy
/// Light; the same scene under `Legacy` heals it with Flash of Light.
#[test]
fn a_teamplan_paladin_with_time_casts_holy_light() {
    let mut s = scene(AiProfile::TeamPlan, HURT, Threat::Distant);
    let (ally, paladin) = (s.entity(ALLY), s.entity(PALADIN));
    let situation = s.reclassify().for_team(1);
    assert!(
        !situation.is_pressured(ally) && !situation.is_pressured(paladin),
        "nobody is pressured: {situation:?}"
    );
    let view = s.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::HolyLight, Some(ALLY))),
        "{}",
        describe(&view)
    );

    let legacy = scene(AiProfile::Legacy, HURT, Threat::Distant).decide(PALADIN);
    assert_eq!(
        legacy.chose(),
        Some((AbilityType::FlashOfLight, Some(ALLY))),
        "{}",
        describe(&legacy)
    );
}

/// An ally under attack gets Flash of Light: Holy Light lands too late.
#[test]
fn a_pressured_ally_gets_flash_of_light() {
    let mut s = scene(AiProfile::TeamPlan, HURT, Threat::OnAlly);
    let (ally, paladin) = (s.entity(ALLY), s.entity(PALADIN));
    let situation = s.reclassify().for_team(1);
    assert!(
        situation.is_pressured(ally) && !situation.is_pressured(paladin),
        "the ally alone is pressured: {situation:?}"
    );
    let view = s.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FlashOfLight, Some(ALLY))),
        "{}",
        describe(&view)
    );
    assert_eq!(
        note(&view).as_deref(),
        Some("no time for Holy Light: its target is pressured"),
        "{}",
        describe(&view)
    );
}

/// A Paladin under attack casts Flash of Light: a 2.5s cast is the one an
/// interrupt or a melee reaches.
#[test]
fn a_pressured_paladin_casts_flash_of_light() {
    let mut s = scene(AiProfile::TeamPlan, HURT, Threat::OnPaladin);
    let (ally, paladin) = (s.entity(ALLY), s.entity(PALADIN));
    let situation = s.reclassify().for_team(1);
    assert!(
        situation.is_pressured(paladin) && !situation.is_pressured(ally),
        "the Paladin alone is pressured: {situation:?}"
    );
    let view = s.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FlashOfLight, Some(ALLY))),
        "{}",
        describe(&view)
    );
    assert_eq!(
        note(&view).as_deref(),
        Some("no time for Holy Light: the Paladin is pressured"),
        "{}",
        describe(&view)
    );
}

/// A deficit smaller than Holy Light's smallest heal is Flash of Light's: the
/// rest of a Holy Light would be overheal. The ally is in Holy Light's band
/// (below 85%), so only the deficit turns it away.
#[test]
fn a_small_deficit_gets_flash_of_light() {
    let mut s = scene(AiProfile::TeamPlan, 0.84, Threat::Distant);
    let ally = s.combatant_mut(ALLY);
    let deficit = ally.max_health - ally.current_health;
    let paladin_sp = s.combatant_mut(PALADIN).spell_power;
    let defs = arenasim::states::play_match::load_ability_definitions().expect("abilities.ron");
    let hl = defs.get(&AbilityType::HolyLight).expect("Holy Light");
    let smallest = hl.healing_base_min + paladin_sp * hl.healing_coefficient;
    assert!(
        deficit < smallest,
        "the scene's premise: a {deficit} deficit under a {smallest} Holy Light"
    );

    let view = s.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FlashOfLight, Some(ALLY))),
        "{}",
        describe(&view)
    );
    assert_eq!(
        note(&view).as_deref(),
        Some("Holy Light would overheal: the deficit is Flash of Light's"),
        "{}",
        describe(&view)
    );
}

/// Below Holy Light's band the quick heal goes out, time or not.
#[test]
fn a_low_ally_gets_flash_of_light() {
    let view = scene(AiProfile::TeamPlan, 0.45, Threat::Distant).decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FlashOfLight, Some(ALLY))),
        "{}",
        describe(&view)
    );
}
