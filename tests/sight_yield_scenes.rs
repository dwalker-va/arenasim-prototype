//! AS-386 — the team sight yield, one classification at a time (tier 1).
//!
//! Each DPS–healer pair names which of the two must not break the pair's
//! line of sight (`situation::SightPair`). The first rule that holds decides:
//!
//! 1. a unit that cannot move leads (the healer: its DPS yields; the DPS: its
//!    healer follows);
//! 2. a pressured healer leads (its DPS yields);
//! 3. the team stance leads (read off `TeamSituation::pillar_stance`, AS-387;
//!    its scene is `pillar_stance_scenes::the_stance_makes_a_healthy_dps_yield_in_the_same_classification`);
//! 4. a hurt DPS (below 80%, held until 90%) yields;
//! 5. otherwise the DPS leads and the healer follows.
//!
//! Every rule is a pair — the situation, and its control with the one
//! condition removed — and the order between neighbours is pinned where the
//! two can both hold. The anti-strobe (the Schmitt pair and the 2s dwell, cut
//! short only by rule 1) is read across consecutive classifications. Then the
//! kiter's half: a yielding Hunter's KITE step keeps its Priest in sight, and
//! one already blind closes on it (`dps_postures::sight_yield_anchor`).

use arenasim::states::match_config::CharacterClass::{Hunter, Mage, Priest, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::Scene;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::states::play_match::situation::{
    SightPair, YieldRule, SIGHT_YIELD_DWELL, SIGHT_YIELD_HP, SIGHT_YIELD_RELEASE_HP,
};
use arenasim::states::play_match::{AbilityType, AuraType};
use bevy::prelude::*;

const DPS: &str = "dps";
const HEALER: &str = "healer";
const ENEMY: &str = "enemy";
const ENEMY_HEALER: &str = "enemy_healer";

/// Our Warrior and Priest 15yd apart in the open, an enemy Warrior and Priest
/// 30yd off. Nobody targets anybody near enough to pressure, under `TeamPlan`.
fn scene() -> Scene {
    Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .unit(DPS, 1, Warrior)
        .at(0.0, 0.0)
        .unit(HEALER, 1, Priest)
        .at(-15.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(30.0, 0.0)
        .unit(ENEMY_HEALER, 2, Priest)
        .at(40.0, 0.0)
}

/// Team 1's one pair, classified (one frame passing).
fn pair(s: &mut Scene) -> SightPair {
    let pairs = s.reclassify().teams[0].sight_yield.clone();
    assert_eq!(pairs.len(), 1, "team 1 has one DPS–healer pair: {pairs:?}");
    pairs[0]
}

fn rule(mut s: Scene) -> YieldRule {
    pair(&mut s).rule
}

// ---------------------------------------------------------------------------
// The rules
// ---------------------------------------------------------------------------

/// Rule 5, and the switch: a healthy DPS with a free healer leads. The
/// control: `Legacy`, and `TeamPlan` with the switch off, classify no pair.
#[test]
fn a_healthy_dps_leads_and_only_teamplan_classifies() {
    assert_eq!(rule(scene()), YieldRule::DpsLeads);
    assert!(!YieldRule::DpsLeads.dps_yields());
    let mut legacy = scene().ai_profile(1, AiProfile::Legacy);
    assert!(legacy.reclassify().teams[0].sight_yield.is_empty());
    let mut off = scene().without_sight_yield(1);
    assert!(off.reclassify().teams[0].sight_yield.is_empty());
    // The switch is per team: team 2 still classifies its own pair.
    assert_eq!(off.reclassify().teams[1].sight_yield.len(), 1);
}

/// Rule 1, the healer's half: a healer that cannot move — stunned, rooted,
/// or casting — leads, and its healthy DPS yields. The control is the scene.
#[test]
fn a_healer_that_cannot_move_makes_its_dps_yield() {
    for (label, s) in [
        ("stunned", scene().aura(HEALER, AuraType::Stun, 4.0)),
        ("rooted", scene().aura(HEALER, AuraType::Root, 4.0)),
        ("feared", scene().aura(HEALER, AuraType::Fear, 4.0)),
        ("casting", scene().casting(HEALER, AbilityType::FlashHeal)),
    ] {
        assert_eq!(rule(s), YieldRule::HealerCannotMove, "{label}");
    }
    assert!(YieldRule::HealerCannotMove.dps_yields());
}

/// Rule 1, the DPS's half, ahead of rule 4: a hurt DPS that cannot move
/// leads (its healer follows). The control: the same hurt DPS free yields.
/// A DPS's own cast does not count: a hurt Mage casting still yields.
#[test]
fn a_dps_that_cannot_move_leads_even_hurt() {
    let hurt = || scene().health_of(DPS, 0.5);
    assert_eq!(
        rule(hurt().aura(DPS, AuraType::Root, 4.0)),
        YieldRule::DpsCannotMove
    );
    assert_eq!(rule(hurt()), YieldRule::DpsHurt);
    let casting_mage = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(DPS, 1, Mage)
        .health(0.5)
        .unit(HEALER, 1, Priest)
        .at(-15.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(30.0, 0.0)
        .casting(DPS, AbilityType::Frostbolt);
    assert_eq!(rule(casting_mage), YieldRule::DpsHurt);
}

/// Rule 1 before rule 2: a pressured healer that is also stunned leads by
/// rule 1 (the trace names the rule that decided).
#[test]
fn rule_one_comes_before_rule_two() {
    let s =
        scene()
            .target(ENEMY, HEALER)
            .place(ENEMY, -13.0, 0.0)
            .aura(HEALER, AuraType::Stun, 4.0);
    assert_eq!(rule(s), YieldRule::HealerCannotMove);
}

/// Rule 2: an enemy Warrior on our healer pressures it, and its healthy DPS
/// yields. The control: the same Warrior on our DPS instead, which leads.
#[test]
fn a_pressured_healer_makes_its_dps_yield() {
    let on_healer = scene().target(ENEMY, HEALER).place(ENEMY, -13.0, 0.0);
    assert_eq!(rule(on_healer), YieldRule::HealerPressured);
    let on_dps = scene().target(ENEMY, DPS).place(ENEMY, 2.0, 0.0);
    assert_eq!(rule(on_dps), YieldRule::DpsLeads);
}

/// Rule 2 before rule 4: a hurt DPS whose healer is pressured yields by rule
/// 2.
#[test]
fn rule_two_comes_before_rule_four() {
    let s = scene()
        .health_of(DPS, 0.5)
        .target(ENEMY, HEALER)
        .place(ENEMY, -13.0, 0.0);
    assert_eq!(rule(s), YieldRule::HealerPressured);
}

/// Rule 4 and its Schmitt pair: below 80% the DPS yields; at 85% a DPS that
/// was not hurt leads, and one that was still yields; above 90% it lets go.
#[test]
fn a_hurt_dps_yields_across_the_schmitt_band() {
    assert_eq!(
        rule(scene().health_of(DPS, SIGHT_YIELD_HP - 0.05)),
        YieldRule::DpsHurt
    );
    let band = (SIGHT_YIELD_HP + SIGHT_YIELD_RELEASE_HP) / 2.0;
    assert_eq!(rule(scene().health_of(DPS, band)), YieldRule::DpsLeads);

    let mut s = scene().health_of(DPS, SIGHT_YIELD_HP - 0.05);
    assert_eq!(pair(&mut s).rule, YieldRule::DpsHurt);
    // Healed into the band, after the dwell: the latch still holds.
    s.set_health(DPS, band);
    let mut s = s.now(30.0 + SIGHT_YIELD_DWELL + 0.5);
    assert_eq!(
        pair(&mut s).rule,
        YieldRule::DpsHurt,
        "released inside the band"
    );
    // Above the release: let go.
    s.set_health(DPS, SIGHT_YIELD_RELEASE_HP + 0.02);
    let mut s = s.now(30.0 + SIGHT_YIELD_DWELL + 1.0);
    assert_eq!(pair(&mut s).rule, YieldRule::DpsLeads);
}

/// The dwell: a yield healed off at once still holds until it is
/// `SIGHT_YIELD_DWELL` old, then lets go. Its onset is carried, not reset.
#[test]
fn a_yield_holds_its_dwell() {
    let mut s = scene().health_of(DPS, 0.5);
    let onset = pair(&mut s);
    assert_eq!(onset.rule, YieldRule::DpsHurt);
    s.set_health(DPS, 1.0);
    let mut s = s.now(30.0 + SIGHT_YIELD_DWELL - 0.5);
    let held = pair(&mut s);
    assert_eq!(held.rule, YieldRule::DpsHurt, "let go inside the dwell");
    assert_eq!(held.since, onset.since, "the onset moved");
    let mut s = s.now(30.0 + SIGHT_YIELD_DWELL + 0.1);
    assert_eq!(
        pair(&mut s).rule,
        YieldRule::DpsLeads,
        "held past the dwell"
    );
}

/// Only rule 1 cuts the dwell short: the same yield, its DPS rooted inside
/// the dwell, leads at once. The control is the dwell test above.
#[test]
fn rule_one_cuts_the_dwell_short() {
    let mut s = scene().health_of(DPS, 0.5);
    assert_eq!(pair(&mut s).rule, YieldRule::DpsHurt);
    s.set_health(DPS, 1.0);
    let mut s = s.now(30.5).aura(DPS, AuraType::Root, 4.0);
    assert_eq!(pair(&mut s).rule, YieldRule::DpsCannotMove);
}

/// Three-a-side: each DPS has its own pair with the one healer, decided
/// apart — the hurt one yields, the healthy one leads.
#[test]
fn each_dps_has_its_own_pair() {
    let mut s = scene().unit("second", 1, Mage).at(5.0, 5.0).health(0.5);
    let pairs = s.reclassify().teams[0].sight_yield.clone();
    let rules: Vec<_> = pairs.iter().map(|p| p.rule).collect();
    assert_eq!(rules, vec![YieldRule::DpsLeads, YieldRule::DpsHurt]);
}

// ---------------------------------------------------------------------------
// The kiter's mask
// ---------------------------------------------------------------------------

const HUNTER: &str = "hunter";
const ROGUE: &str = "rogue";
const PRIEST: &str = "priest";

/// A TwinPillars-sized pillar at the origin.
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::ZERO,
        radius: 3.0,
        base_y: 0.0,
        height: 5.0,
    }]
}

fn eye(x: f32, z: f32) -> Vec3 {
    Vec3::new(x, EYE_HEIGHT, z)
}

/// Our Hunter at `hunter`, hurt (it yields), kiting an enemy Warrior `rogue`
/// that is on it; our Priest north of the pillar at (0, 15).
fn kite(hunter: (f32, f32), rogue: (f32, f32), hp: f32) -> Scene {
    Scene::new()
        .obstacles(pillar())
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .unit(HUNTER, 1, Hunter)
        .at(hunter.0, hunter.1)
        .health(hp)
        .targeting(ROGUE)
        .unit(PRIEST, 1, Priest)
        .at(0.0, 15.0)
        .unit(ROGUE, 2, Warrior)
        .at(rogue.0, rogue.1)
        .targeting(HUNTER)
}

/// The KITE step the Hunter chose, as the 2yd scorer lookahead.
fn kite_step(s: &Scene, from: (f32, f32)) -> (f32, f32) {
    let view = s.decide(HUNTER);
    let dir = view
        .movement
        .iter()
        .rev()
        .find_map(|m| m.chosen_direction)
        .expect("a scored KITE direction");
    (from.0 + dir[0] * 2.0, from.1 + dir[1] * 2.0)
}

/// The mask: a hurt Hunter south-east of the pillar, a Warrior east of it, its
/// Priest north. Fleeing straight west walks it into the pillar's shadow
/// from the Priest; yielding, it steps where the Priest still sees it. The
/// control: a healthy Hunter (it leads, no mask) takes the shadowed step.
#[test]
fn a_yielding_kiter_keeps_its_healer_in_sight() {
    let at = (5.0, -3.0);
    let rogue = (8.0, -3.0);
    assert!(has_line_of_sight(
        &pillar(),
        eye(at.0, at.1),
        eye(0.0, 15.0)
    ));
    let yielding = kite_step(&kite(at, rogue, 0.6), at);
    let leading = kite_step(&kite(at, rogue, 1.0), at);
    println!("yielding step {yielding:?}, leading step {leading:?}");
    assert!(
        has_line_of_sight(&pillar(), eye(yielding.0, yielding.1), eye(0.0, 15.0)),
        "the yielding Hunter stepped into its Priest's shadow: {yielding:?}"
    );
    assert!(
        !has_line_of_sight(&pillar(), eye(leading.0, leading.1), eye(0.0, 15.0)),
        "the leading Hunter kept sight: the stage no longer tests the mask ({leading:?})"
    );
    // The mask keeps the escape: the yielding step still opens distance on
    // the Warrior.
    let gap = |p: (f32, f32)| ((p.0 - rogue.0).powi(2) + (p.1 - rogue.1).powi(2)).sqrt();
    assert!(
        gap(yielding) > gap(at),
        "the yielding step closed on the Warrior"
    );
}

/// A kiter already blind with no step that regains sight closes on its
/// healer (`close_when_out`) rather than fleeing on, further out of sight:
/// deep in the pillar's shadow, a Warrior between it and the pillar. The
/// control: the healthy Hunter flees away from the Priest.
#[test]
fn a_blind_yielding_kiter_closes_on_its_healer() {
    let at = (0.0, -9.0);
    let rogue = (0.0, -6.0);
    let healer = Vec2::new(0.0, 15.0);
    assert!(!has_line_of_sight(
        &pillar(),
        eye(at.0, at.1),
        eye(0.0, 15.0)
    ));
    let dist = |p: (f32, f32)| Vec2::new(p.0, p.1).distance(healer);
    let yielding = kite_step(&kite(at, rogue, 0.6), at);
    let leading = kite_step(&kite(at, rogue, 1.0), at);
    println!("yielding step {yielding:?}, leading step {leading:?}");
    assert!(
        dist(yielding) < dist(at),
        "the blind yielding Hunter did not close on its Priest: {yielding:?}"
    );
    assert!(
        dist(leading) > dist(at),
        "the leading Hunter closed anyway: the stage no longer tests the rung ({leading:?})"
    );
}

/// The mask's reach is unbounded: it holds SIGHT only, and heal range stays
/// the soft `healer_leash` term. A hurt Hunter 41yd from its Priest, in its
/// sight, a Warrior on it from the Priest's side: it flees straight away,
/// out past 40yd, exactly as with the yield off — every step it had free
/// stays free. A finite reach (40yd, the healers' anchor) would keep only the
/// steps that end inside it, toward the Priest and the Warrior. The pillar
/// stands well off the line, so nothing is occluded.
#[test]
fn a_yielding_kiter_out_of_heal_range_keeps_its_free_steps() {
    let far = |yield_on: bool| {
        let s = Scene::new()
            .obstacles(vec![ObstacleVolume::Cylinder {
                center_xz: Vec2::new(0.0, 25.0),
                radius: 3.0,
                base_y: 0.0,
                height: 5.0,
            }])
            .ai_profile(1, AiProfile::TeamPlan)
            .ai_profile(2, AiProfile::TeamPlan)
            .unit(HUNTER, 1, Hunter)
            .at(21.0, 0.0)
            .health(0.6)
            .targeting(ROGUE)
            .unit(PRIEST, 1, Priest)
            .at(-20.0, 0.0)
            .unit(ROGUE, 2, Warrior)
            .at(18.0, 0.0)
            .targeting(HUNTER);
        if yield_on {
            s
        } else {
            s.without_sight_yield(1)
        }
    };
    let at = (21.0, 0.0);
    let healer = Vec2::new(-20.0, 0.0);
    let dist = |p: (f32, f32)| Vec2::new(p.0, p.1).distance(healer);
    assert!(dist(at) > 40.0);

    let mut yielding = far(true);
    let pairs = yielding.reclassify().teams[0].sight_yield.clone();
    assert_eq!(pairs[0].rule, YieldRule::DpsHurt, "the Hunter must yield");
    let mut off = far(false);
    assert!(off.reclassify().teams[0].sight_yield.is_empty());

    let with_mask = kite_step(&yielding, at);
    let without = kite_step(&off, at);
    println!("yielding step {with_mask:?}, unmasked step {without:?}");
    assert!(
        dist(without) > dist(at),
        "the unmasked Hunter did not flee outward: the stage no longer tests the reach ({without:?})"
    );
    assert!(
        dist(with_mask) > 40.0 && dist(with_mask) > dist(at),
        "the yielding Hunter was held inside 40yd of its Priest: {with_mask:?}"
    );
    assert!(
        (with_mask.0 - without.0).abs() < 1e-4 && (with_mask.1 - without.1).abs() < 1e-4,
        "the mask took a free step: {with_mask:?} against {without:?}"
    );
}

/// The measurement variant (AS-385 package): with the healer's cast left out
/// of rule 1, a casting healer no longer makes its healthy DPS yield; a
/// stunned one still does. The control is the shipped rule, which counts the
/// cast.
#[test]
fn the_variant_leaves_the_healers_cast_out_of_rule_one() {
    let casting = || scene().casting(HEALER, AbilityType::FlashHeal);
    assert_eq!(rule(casting()), YieldRule::HealerCannotMove);
    assert_eq!(rule(casting().yield_cast_moves(1)), YieldRule::DpsLeads);
    assert_eq!(
        rule(
            scene()
                .aura(HEALER, AuraType::Stun, 4.0)
                .yield_cast_moves(1)
        ),
        YieldRule::HealerCannotMove
    );
}
