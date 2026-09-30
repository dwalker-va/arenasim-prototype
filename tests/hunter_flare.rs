//! AS-166 — the Hunter's Flare, driven through the Hunter's own decider and
//! through whole matches.
//!
//! `class_ai_decisions.rs` pins `flare_plan` (where and when); this file pins
//! what `decide_hunter_action` DOES with the plan:
//!
//! - the GCD reservation: with a Flare due within one GCD the Hunter starts
//!   nothing, so the light goes down the frame it comes due;
//! - the target-less path: in 1v1 against a Rogue the stealthed Rogue is the
//!   only enemy, so the Hunter has no target at all, and a Flare lit from
//!   that path is the only answer it has to the opener.

use std::collections::BTreeMap;

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::hunter::decide_hunter_action;
use arenasim::states::play_match::class_ai::hunter_dip::HunterDipPlan;
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo, QueuedInstantAttack};
use arenasim::states::play_match::decision_trace::DecisionTrace;
use arenasim::states::play_match::{
    AbilityDefinitions, AbilityType, Aura, CastingState, Combatant, FlareZone, GameRng,
    MovementConfig,
};

fn info(entity: Entity, team: u8, class: CharacterClass, position: Vec3) -> CombatantInfo {
    CombatantInfo {
        entity,
        team,
        slot: 0,
        class,
        current_health: 100.0,
        max_health: 100.0,
        current_mana: 100.0,
        max_mana: 100.0,
        position,
        velocity: Vec3::ZERO,
        is_alive: true,
        stealthed: false,
        target: None,
        is_pet: false,
        casting_ability: None,
        pet_type: None,
        pet: None,
    }
}

/// What one call of the Hunter's decider did.
struct Decision {
    acted: bool,
    flare_lit: bool,
    began_cast: bool,
    gcd: f32,
    mana_spent: f32,
    cooldowns: Vec<AbilityType>,
}

/// One decision by a Hunter at (-30, 0, 0) on team 1, `since_gates` seconds
/// after the gates, with a visible enemy Priest 30yd away as its target (safe
/// range: every shot in its rotation is available) and an enemy Rogue in
/// stealth. The Hunter is its team's only member, so the Flare guards it:
/// centred 5yd toward team 2's gate at +35, with its edge 50yd from that gate
/// — a Rogue at base speed (6yd/s) could first reach it 8.33s after the gates.
fn decide_at(since_gates: f32) -> Decision {
    let mut world = World::new();
    let hunter = world.spawn_empty().id();
    let priest = world.spawn_empty().id();
    let rogue = world.spawn_empty().id();

    let abilities = AbilityDefinitions::default();
    let movement = MovementConfig::default();
    let mut combat_log = CombatLog::default();
    let mut game_rng = GameRng::from_seed(7);
    let mut trace = DecisionTrace::default();
    trace.current_sim_time = since_gates;

    let my_pos = Vec3::new(-30.0, 0.0, 0.0);
    let mut combatant = Combatant::new(1, 0, CharacterClass::Hunter);
    combatant.current_mana = combatant.max_mana;
    combatant.target = Some(priest);
    let mana_before = combatant.current_mana;

    let mut roster = BTreeMap::new();
    let mut me = info(hunter, 1, CharacterClass::Hunter, my_pos);
    me.target = Some(priest);
    roster.insert(hunter, me);
    roster.insert(
        priest,
        info(priest, 2, CharacterClass::Priest, Vec3::new(0.0, 0.0, 0.0)),
    );
    let mut hidden = info(rogue, 2, CharacterClass::Rogue, Vec3::new(20.0, 0.0, 0.0));
    hidden.stealthed = true;
    roster.insert(rogue, hidden);

    let auras: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();
    let dr = BTreeMap::new();
    let cooldowns = BTreeMap::new();
    let ctx = CombatContext::new(
        hunter,
        1,
        &roster,
        &auras,
        &dr,
        &cooldowns,
        &[],
        Default::default(),
        Default::default(),
    );

    let mut queue = CommandQueue::default();
    let mut commands = Commands::new(&mut queue, &world);
    let mut instant_attacks: Vec<QueuedInstantAttack> = Vec::new();
    let acted = decide_hunter_action(
        &mut commands,
        &mut combat_log,
        &mut game_rng,
        &abilities,
        hunter,
        &mut combatant,
        my_pos,
        None,
        &ctx,
        &mut instant_attacks,
        HunterDipPlan::default(),
        &movement.hunter.weights,
        &[],
        None,
        &mut trace,
    );
    queue.apply(&mut world);

    let flare_lit = world.query::<&FlareZone>().iter(&world).count() > 0;
    let began_cast = world.get::<CastingState>(hunter).is_some();
    Decision {
        acted,
        flare_lit,
        began_cast,
        gcd: combatant.global_cooldown,
        mana_spent: mana_before - combatant.current_mana,
        cooldowns: combatant.ability_cooldowns.keys().copied().collect(),
    }
}

/// Within one GCD of the Flare coming due, the Hunter with a live target
/// takes no action at all — no shot, no cast, no GCD, no mana — and on the
/// first frame the Flare is due it lights it. Outside that window the same
/// Hunter does act (the rotation's Serpent Sting, say), so the stillness is
/// the reservation, not a Hunter with nothing to do.
#[test]
fn a_flare_due_within_a_gcd_holds_the_gcd_and_lights_the_frame_it_is_due() {
    let due = 50.0 / 6.0;

    let early = decide_at(due - 2.5);
    assert!(
        early.acted && early.gcd > 0.0 && !early.flare_lit,
        "more than a GCD out, the Hunter shoots: acted={}, gcd={}, flare={}",
        early.acted,
        early.gcd,
        early.flare_lit
    );

    for before in [1.4, 0.7, 0.05] {
        let held = decide_at(due - before);
        assert!(
            !held.acted
                && held.gcd == 0.0
                && held.mana_spent == 0.0
                && held.cooldowns.is_empty()
                && !held.began_cast
                && !held.flare_lit,
            "{before}s before the Flare is due the Hunter must start nothing: \
             acted={}, gcd={}, mana spent={}, cooldowns={:?}, cast={}, flare={}",
            held.acted,
            held.gcd,
            held.mana_spent,
            held.cooldowns,
            held.began_cast,
            held.flare_lit
        );
    }

    let lit = decide_at(due + 0.02);
    assert!(
        lit.acted && lit.flare_lit && lit.cooldowns == vec![AbilityType::Flare],
        "the frame the Flare is due it is lit, and nothing else: acted={}, flare={}, \
         cooldowns={:?}",
        lit.acted,
        lit.flare_lit,
        lit.cooldowns
    );
}

/// 1v1 against a Rogue: the Rogue in stealth is the only enemy, so the Hunter
/// has no target until it is found, and every Flare it lights comes from the
/// target-less path. Pinned seeds: the Flare finds the Rogue and the Rogue
/// never opens from stealth, in both slot orders.
#[test]
fn a_hunter_with_no_target_flares_the_lone_rogue_before_it_opens() {
    for (team1, team2, rogue_team) in [(["Hunter"], ["Rogue"], 2u8), (["Rogue"], ["Hunter"], 1u8)] {
        for seed in [0u64, 1, 2] {
            let dir = tempfile::tempdir().unwrap();
            let log_path = dir.path().join("match.txt");
            let cfg = HeadlessMatchConfig {
                team1: team1.iter().map(|s| s.to_string()).collect(),
                team2: team2.iter().map(|s| s.to_string()).collect(),
                random_seed: Some(seed),
                max_duration_secs: 60.0,
                output_path: Some(log_path.to_string_lossy().into_owned()),
                ..Default::default()
            };
            run_headless_match_with(cfg, false, None).expect("headless match");
            let log = std::fs::read_to_string(&log_path).expect("read log");
            let rogue = format!("Team {rogue_team} Rogue #1");
            assert!(
                log.lines()
                    .any(|l| l.contains(&format!("[STEALTH] {rogue} is revealed by Flare"))),
                "{team1:?} vs {team2:?} seed {seed}: no Flare found the Rogue"
            );
            let opened = log.lines().any(|l| {
                l.contains(&format!("[CAST] {rogue} uses Cheap Shot on"))
                    || l.contains(&format!("[CAST] {rogue} uses Ambush on"))
            });
            assert!(
                !opened,
                "{team1:?} vs {team2:?} seed {seed}: the Rogue opened from stealth"
            );
        }
    }
}
