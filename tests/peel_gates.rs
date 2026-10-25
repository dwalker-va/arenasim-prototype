//! The Peel play's context gates (`class_ai::peel::PEEL_GATES`): the
//! peeler-for-teammate contexts the play is switched off in because it
//! measured negative there (`docs/design/balance/2026-10-12-as290-peel-context-gates.md`).
//!
//! Two tests, so that adding or removing a gate is a visible, deliberate edit:
//!
//! - [`the_gated_contexts_are_exactly_these`] names every gate by SET
//!   EQUALITY, and holds each to a measurement line and its findings note;
//! - [`every_peeler_peels_for_every_teammate_but_the_gated_ones`] plays the
//!   same scene for every peeling class and every class of teammate in
//!   danger, and asserts the peel happens exactly where no gate stands — so
//!   un-gating a gated context, and gating an enabled one, each fail it on
//!   behaviour, not only on the list. Its control lifts the team's gates
//!   (`Scene::lift`, the milestone sweep's re-check), and every gated
//!   context then peels: the gate is what stops it.

use std::collections::BTreeSet;

use arenasim::states::match_config::CharacterClass::{
    self, Druid, Hunter, Mage, Paladin, Priest, Rogue, Shaman, Warlock, Warrior,
};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::peel::{
    peel_tools, PeelContext, MILESTONE_08_FINDINGS, PEEL_FORMAT_GATES, PEEL_FORMAT_GATES_FINDINGS,
    PEEL_GATES, PEEL_GATES_FINDINGS,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::{PlayStep, PlayTag};
use arenasim::states::play_match::situation::{Play, PlaySet, CHAIN, PEEL};

/// Every gate, as (peeler, teammate in danger). Edit this list together with
/// `PEEL_GATES`, and only on a measurement.
const GATED: &[(CharacterClass, CharacterClass)] = &[
    (Mage, Hunter),
    (Mage, Warlock),
    (Druid, Hunter),
    (Druid, Warlock),
    (Druid, Warrior),
    (Shaman, Warlock),
    (Rogue, Hunter),
    (Rogue, Warrior),
    (Rogue, Priest),
    // The 0.8 milestone sweep (2026-10-25-0.8-milestone-sweep.md §3.1).
    (Paladin, Hunter),
    (Hunter, Priest),
];

#[test]
fn the_gated_contexts_are_exactly_these() {
    let shipped: BTreeSet<(String, String)> = PEEL_GATES
        .iter()
        .map(|g| {
            (
                format!("{:?}", g.context.peeler),
                format!("{:?}", g.context.ally),
            )
        })
        .collect();
    let named: BTreeSet<(String, String)> = GATED
        .iter()
        .map(|(p, a)| (format!("{p:?}"), format!("{a:?}")))
        .collect();
    assert_eq!(
        shipped, named,
        "PEEL_GATES against the gates this test names"
    );
    assert_eq!(
        PEEL_GATES.len(),
        GATED.len(),
        "a context gated twice in PEEL_GATES"
    );
    for gate in PEEL_GATES {
        let PeelContext { peeler, ally } = gate.context;
        assert!(
            !peel_tools(peeler).is_empty(),
            "{peeler:?} has no peel tool to gate"
        );
        assert!(
            gate.measured.contains("pt") && gate.measured.contains("z="),
            "{peeler:?} for {ally:?}: the gate's line names no measurement: {}",
            gate.measured
        );
    }
    let read = |path: &str| {
        let note = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        std::fs::read_to_string(&note)
            .unwrap_or_else(|e| panic!("the gates' findings note {}: {e}", note.display()))
    };
    let (as290, milestone_08) = (read(PEEL_GATES_FINDINGS), read(MILESTONE_08_FINDINGS));
    for gate in PEEL_GATES {
        let PeelContext { peeler, ally } = gate.context;
        let context = format!("{peeler:?}@{ally:?}");
        // A gate the 0.8 sweep added says so, and is recorded in its note.
        let (name, text) = if gate.measured.starts_with("0.8 sweep") {
            (MILESTONE_08_FINDINGS, &milestone_08)
        } else {
            (PEEL_GATES_FINDINGS, &as290)
        };
        assert!(text.contains(&context), "{name} does not record {context}");
    }
}

const ME: &str = "me";
const ALLY: &str = "ally";
const ATTACKER: &str = "enemy warrior";
const ENEMY_HEALER: &str = "enemy priest";

/// `tests/peel_scenes.rs`'s scene, with the teammate in danger of class
/// `ally`: the peeler at `x` (where its tool reaches the attacker), the ally
/// at (3, 0) at 30% health, an enemy Warrior on it at (4.5, 0), and the enemy
/// Priest — the peeler's own target — 30yd off.
fn scene(peeler: CharacterClass, ally: CharacterClass) -> Scene {
    let x = match peeler {
        // Beyond the Warlock's own Death Coil self-peel radius, and the
        // Hunter's dead zone.
        Warlock | Hunter => -10.0,
        // In melee with the Warrior.
        Rogue => 6.0,
        _ => 0.0,
    };
    let mut s = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ME, 1, peeler)
        .at(x, 0.0)
        .targeting(ENEMY_HEALER)
        .unit(ALLY, 1, ally)
        .at(3.0, 0.0)
        .health(0.3)
        .targeting(ENEMY_HEALER)
        .unit(ATTACKER, 2, Warrior)
        .at(4.5, 0.0)
        .targeting(ALLY)
        .unit(ENEMY_HEALER, 2, Priest)
        .at(x, 30.0);
    s.combatant_mut(ME).stealthed = false;
    s.combatant_mut(ALLY).stealthed = false;
    s
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; situation {:?}; candidates {:?}",
        view.chosen,
        view.situation,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// Whether the decision peeled, and that a peel is one of the peeler's tools
/// aimed at the attacker (or a self-centred area, which names no target).
fn peeled(peeler: CharacterClass, view: &DecisionView) -> bool {
    let situation = view.situation.as_ref().expect("a situation");
    assert!(
        situation.ally_in_danger.is_some(),
        "{peeler:?}: no ally in danger: {}",
        describe(view)
    );
    if situation.play_step != Some(PlayStep::Peel) {
        return false;
    }
    let (tool, target) = view.chose().expect("a peel chose something");
    assert!(
        peel_tools(peeler).contains(&tool),
        "{peeler:?} peeled with {tool:?}"
    );
    assert!(
        target.is_none_or(|t| t == ATTACKER),
        "{peeler:?} peeled {target:?}, not the attacker"
    );
    true
}

/// For every class with a peel tool and every class of teammate in danger:
/// the peeler peels exactly when the context is not gated, and runs its
/// fallback play (no `Peel` tag) when it is. With the team's gates lifted,
/// every context peels — the paired control.
#[test]
fn every_peeler_peels_for_every_teammate_but_the_gated_ones() {
    let mut seen_gated = 0;
    let mut wrong = Vec::new();
    for &peeler in CharacterClass::all() {
        if peel_tools(peeler).is_empty() {
            continue;
        }
        for &ally in CharacterClass::all() {
            let is_gated = GATED.contains(&(peeler, ally));
            seen_gated += usize::from(is_gated);

            let view = scene(peeler, ally).decide(ME);
            let did = peeled(peeler, &view);
            let tagged = matches!(
                view.situation.as_ref().and_then(|s| s.play),
                Some(PlayTag::Peel { .. })
            );
            if did == is_gated || tagged == is_gated {
                wrong.push(format!(
                    "{peeler:?} for {ally:?} (gated: {is_gated}): peeled {did}, tagged Peel \
                     {tagged}: {}",
                    describe(&view)
                ));
            }

            let lifted = scene(peeler, ally).lift(1, PlaySet::of(&[PEEL])).decide(ME);
            if !peeled(peeler, &lifted) {
                wrong.push(format!(
                    "{peeler:?} for {ally:?}, gates lifted: no peel: {}",
                    describe(&lifted)
                ));
            }
        }
    }
    assert_eq!(
        seen_gated,
        GATED.len(),
        "every gate is reached by the scene"
    );
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// A Mage gated for its dying Hunter, the Hunter's attacker low and in a kill
/// window, the enemy Priest stunned for `stun` seconds.
fn gated_mage_scene(stun: f32) -> Scene {
    use arenasim::states::play_match::AuraType;
    let mut s = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ME, 1, Mage)
        .at(0.0, 0.0)
        .targeting(ATTACKER)
        .unit(ALLY, 1, Hunter)
        .at(-20.0, 0.0)
        .health(0.3)
        .targeting(ENEMY_HEALER)
        .unit(ATTACKER, 2, Warrior)
        .at(-18.5, 0.0)
        .health(0.2)
        .targeting(ALLY)
        .unit(ENEMY_HEALER, 2, Priest)
        .at(0.0, 30.0)
        .aura(ENEMY_HEALER, AuraType::Stun, stun);
    s.combatant_mut(ALLY).stealthed = false;
    s
}

/// A gated peeler falls back to its fallback play, and the Burst play's
/// exceptions then apply to THAT: a Mage gated for its dying Hunter, with the
/// Hunter's attacker low and the enemy Priest stunned (a kill window), runs
/// the Burst play; with the Mage named a Burst exception it runs the default
/// chain, not Burst. With its gates lifted it peels instead.
#[test]
fn a_gated_peeler_falls_back_to_burst_unless_it_is_excepted() {
    use arenasim::states::play_match::situation::ClassSet;
    assert!(GATED.contains(&(Mage, Hunter)));
    // The stun outlasts the Chain play's horizon, so no lock is ending: this
    // pins Peel's gate against Burst's exceptions alone (the Chain between
    // them is the next test's).
    let build = || gated_mage_scene(6.0);
    let tag = |view: &DecisionView| {
        let s = view.situation.as_ref().expect("a situation");
        assert!(s.ally_in_danger.is_some(), "{}", describe(view));
        assert!(s.kill_window.is_some(), "{}", describe(view));
        s.play
    };
    let gated = build().decide(ME);
    assert_eq!(tag(&gated), Some(PlayTag::Burst), "{}", describe(&gated));
    let excepted = build()
        .exceptions(Play::Burst, ClassSet::of(&[Mage]))
        .decide(ME);
    assert_eq!(tag(&excepted), None, "{}", describe(&excepted));
    let lifted = build().lift(1, PlaySet::of(&[PEEL])).decide(ME);
    assert!(
        matches!(tag(&lifted), Some(PlayTag::Peel { .. })),
        "{}",
        describe(&lifted)
    );
}

/// The composition order of `CombatContext::play`: Peel's gate, then Chain,
/// then Burst's exceptions. A gated peeler passes to the next stage of the
/// pipeline (`class_ai::plays::PLAY_ORDER`), so with the enemy Priest's stun
/// ending inside the Chain play's horizon the gated Mage CHAINS (onto the
/// Priest); named a Chain exception it falls to the Burst play (the kill
/// window); named a Burst exception too, to the default chain. With its gates
/// lifted it peels, ahead of all of them.
#[test]
fn a_gated_peeler_still_chains_when_a_lock_is_ending() {
    use arenasim::states::play_match::situation::ClassSet;
    assert!(GATED.contains(&(Mage, Hunter)));
    let build = || gated_mage_scene(1.7);
    let priest = build().entity(ENEMY_HEALER).index();
    let tag = |view: &DecisionView| {
        let s = view.situation.as_ref().expect("a situation");
        assert!(s.ally_in_danger.is_some(), "{}", describe(view));
        assert!(s.kill_window.is_some(), "{}", describe(view));
        s.play
    };
    let gated = build().decide(ME);
    assert_eq!(
        tag(&gated),
        Some(PlayTag::Chain { subject: priest }),
        "{}",
        describe(&gated)
    );
    let no_chain = build().exceptions(CHAIN, ClassSet::of(&[Mage])).decide(ME);
    assert_eq!(
        tag(&no_chain),
        Some(PlayTag::Burst),
        "{}",
        describe(&no_chain)
    );
    let neither = build()
        .exceptions(CHAIN, ClassSet::of(&[Mage]))
        .exceptions(Play::Burst, ClassSet::of(&[Mage]))
        .decide(ME);
    assert_eq!(tag(&neither), None, "{}", describe(&neither));
    let lifted = build().lift(1, PlaySet::of(&[PEEL])).decide(ME);
    assert!(
        matches!(tag(&lifted), Some(PlayTag::Peel { .. })),
        "{}",
        describe(&lifted)
    );
}

// ---------------------------------------------------------------------------
// Format gates (`PEEL_FORMAT_GATES`): a peeler that does not peel at all on a
// team of three
// ---------------------------------------------------------------------------

/// Every format gate, as (peeler, team size). Edit together with
/// `PEEL_FORMAT_GATES`, and only on a measurement.
const FORMAT_GATED: &[(CharacterClass, u8)] = &[(Mage, 3), (Druid, 3)];

#[test]
fn the_format_gates_are_exactly_these() {
    let shipped: BTreeSet<(String, u8)> = PEEL_FORMAT_GATES
        .iter()
        .map(|g| (format!("{:?}", g.peeler), g.team_size))
        .collect();
    let named: BTreeSet<(String, u8)> = FORMAT_GATED
        .iter()
        .map(|(p, n)| (format!("{p:?}"), *n))
        .collect();
    assert_eq!(shipped, named, "PEEL_FORMAT_GATES against this test's list");
    assert_eq!(
        PEEL_FORMAT_GATES.len(),
        FORMAT_GATED.len(),
        "a format gated twice"
    );
    let note = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PEEL_FORMAT_GATES_FINDINGS);
    let text = std::fs::read_to_string(&note)
        .unwrap_or_else(|e| panic!("the format gates' findings note {}: {e}", note.display()));
    for gate in PEEL_FORMAT_GATES {
        assert!(
            !peel_tools(gate.peeler).is_empty(),
            "{:?} has no peel tool to gate",
            gate.peeler
        );
        assert!(
            gate.measured.contains("pt") && gate.measured.contains("z="),
            "{:?}@{}v{}: the gate's line names no measurement: {}",
            gate.peeler,
            gate.team_size,
            gate.team_size,
            gate.measured
        );
        let context = format!("{:?}@{}v{}", gate.peeler, gate.team_size, gate.team_size);
        assert!(
            text.contains(&context),
            "{PEEL_FORMAT_GATES_FINDINGS} does not record {context}"
        );
    }
}

/// [`scene`] on a team of three: a second teammate, healthy and far off.
fn scene3(peeler: CharacterClass, ally: CharacterClass) -> Scene {
    let mut s = scene(peeler, ally)
        .unit("mate", 1, Warrior)
        .at(-25.0, -25.0)
        .targeting(ENEMY_HEALER);
    s.combatant_mut("mate").stealthed = false;
    s
}

/// On a team of three, every peeler peels for a Paladin (a teammate no
/// context gate names) exactly when no format gate names it; on a team of
/// two, every one does. With the team's gates lifted, every one peels on a
/// team of three too — the gate is what stops it.
#[test]
fn a_format_gated_peeler_does_not_peel_on_a_team_of_three() {
    assert!(
        GATED.iter().all(|(_, ally)| *ally != Paladin),
        "the scene's teammate must be one no context gate names"
    );
    let mut wrong = Vec::new();
    let mut seen_gated = 0;
    for &peeler in CharacterClass::all() {
        if peel_tools(peeler).is_empty() {
            continue;
        }
        let gated = FORMAT_GATED.contains(&(peeler, 3));
        seen_gated += usize::from(gated);
        let two = scene(peeler, Paladin).decide(ME);
        if !peeled(peeler, &two) {
            wrong.push(format!(
                "{peeler:?}, a team of two: no peel: {}",
                describe(&two)
            ));
        }
        let three = scene3(peeler, Paladin).decide(ME);
        if peeled(peeler, &three) == gated {
            wrong.push(format!(
                "{peeler:?}, a team of three (gated: {gated}): {}",
                describe(&three)
            ));
        }
        // A 3v3 that has lost a unit is still a 3v3.
        let mut lost = scene3(peeler, Paladin);
        lost.combatant_mut("mate").is_dead = true;
        lost.combatant_mut("mate").current_health = 0.0;
        let lost = lost.decide(ME);
        if peeled(peeler, &lost) == gated {
            wrong.push(format!(
                "{peeler:?}, a team of three, one dead (gated: {gated}): {}",
                describe(&lost)
            ));
        }
        let lifted = scene3(peeler, Paladin)
            .lift(1, PlaySet::of(&[PEEL]))
            .decide(ME);
        if !peeled(peeler, &lifted) {
            wrong.push(format!(
                "{peeler:?}, a team of three, gates lifted: no peel: {}",
                describe(&lifted)
            ));
        }
    }
    assert_eq!(
        seen_gated,
        FORMAT_GATED.len(),
        "every format gate is reached"
    );
    assert!(wrong.is_empty(), "{wrong:#?}");
}
