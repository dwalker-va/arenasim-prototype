//! The shield casts' hands observed in a REAL match: every Power Word: Shield,
//! Divine Shield and Ice Barrier flashes its caster's two spell hands exactly
//! ONCE — two launch-phase hand rigs per cast, never four.
//!
//! ## Why this exists
//!
//! None of the three has a cast state, so each plays its hands off a signal:
//! Power Word: Shield and Ice Barrier off their `InstantAbilityFired` marker
//! (`spawn_instant_cast_hands`), Divine Shield off its `DivineShieldPending`
//! (`spawn_divine_shield_cast_hands`, at the head of the next fixed tick). A
//! signal read by two hand systems plays the hands twice per cast, and only
//! the match path can show it: the animation sandbox's Divine Shield pending is
//! consumed by Phase 1 before a second reader would see it, and the
//! Ice Barrier and PW:S probes replay one decision through the hand system in
//! a bare app. So this boots `PlayMatch` through the real `StatesPlugin`
//! schedule the way `--replay` does (the `hit_reaction_match_flow.rs` shape)
//! and counts, per cast, the launch rigs that cast's caster grew.
//!
//! Runs with no window or GPU. Visual-only systems never touch the sim, so the
//! matches are the seeded ones, but for one staged fact: on the frame the gates
//! open the Paladin is set to a quarter of its health, below the Divine Shield
//! trigger, so every match of the bank has a bubble in it by construction.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::components::{
    Combatant, DivineShieldPending, HealCastHand, HealCastKind, HealCastPhase, InstantAbilityFired,
    MatchCountdown,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::systems::CombatSystemPhase;
use arenasim::states::play_match::{
    consume_instant_ability_signals, AbilityConfigPlugin, GameRng, MapConfigPlugin,
    MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::{CharacterClass, HeadlessMatchConfig};

/// One rendered frame at 60 fps — one sim tick, the client's own cadence.
const FRAME: Duration = Duration::from_micros(16_667);
/// Long enough for the seeded matches below to reach their end.
const MAX_FRAMES: usize = 60 * 90;
/// A cast's hands are spawned on its own frame (an `InstantAbilityFired`) or
/// at the head of the next fixed tick (a `DivineShieldPending`); this many
/// frames after the cast is generous for both, and far shorter than any
/// caster's next shield.
const HANDS_WITHIN: usize = 3;

/// The three shields under test.
const SHIELDS: [AbilityType; 3] = [
    AbilityType::PowerWordShield,
    AbilityType::DivineShield,
    AbilityType::IceBarrier,
];

/// What the observers saw. Frames count from the first update.
#[derive(Resource, Default)]
struct Seen {
    frame: usize,
    /// `(frame, caster, ability)` per shield cast, read off its signal.
    casts: Vec<(usize, Entity, AbilityType)>,
    /// `(frame, caster, kind)` per hand rig spawned in its launch phase.
    launches: Vec<(usize, Entity, HealCastKind)>,
}

fn boot(cfg: &str) -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(cfg).unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(BevyStatesPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .add_plugins(bevy::transform::TransformPlugin)
        .init_resource::<bevy_egui::EguiUserTextures>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        .init_asset::<bevy::scene::Scene>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<bevy::gltf::GltfNode>()
        .init_asset::<bevy::gltf::GltfMesh>()
        .init_asset::<bevy::gltf::GltfPrimitive>()
        .add_plugins((
            AbilityConfigPlugin,
            MovementConfigPlugin,
            MapConfigPlugin,
            EquipmentPlugin,
            CombatPlugin,
            arenasim::settings::SettingsPlugin,
            StatesPlugin,
        ));
    // The `--replay` entrance: straight into the match with the seed set.
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>();

    // The marker lives from the class AI's application to
    // `consume_instant_ability_signals` in the same tick.
    app.add_systems(
        FixedUpdate,
        observe_fired
            .after(CombatSystemPhase::CombatResolution)
            .before(consume_instant_ability_signals),
    )
    // The pending lives from the Paladin AI's queue to Phase 1 of the next
    // tick, so every frame's `Last` sees it once, independent of either hand
    // system's place in the schedule.
    .add_systems(PreUpdate, stage_low_paladin)
    .add_systems(Last, (observe_bubbles, observe_rigs))
    .add_systems(
        Last,
        (|mut seen: ResMut<Seen>| seen.frame += 1).after(observe_rigs),
    );
    app
}

/// The staged fact: on the first frame after the gates open, team 1's Paladin
/// stands at a quarter of its health, below `DIVINE_SHIELD_HP_THRESHOLD`, so its
/// next decision bubbles. Applied once, before that frame's fixed ticks.
fn stage_low_paladin(
    mut done: Local<bool>,
    countdown: Option<Res<MatchCountdown>>,
    mut combatants: Query<&mut Combatant>,
) {
    if *done || !countdown.is_some_and(|c| c.gates_opened) {
        return;
    }
    for mut c in combatants.iter_mut() {
        if c.team == 1 && c.class == CharacterClass::Paladin {
            c.current_health = 0.25 * c.max_health;
            *done = true;
        }
    }
    assert!(*done, "the staged match has no team 1 Paladin");
}

fn observe_fired(mut seen: ResMut<Seen>, fired: Query<&InstantAbilityFired>) {
    let frame = seen.frame;
    for f in fired.iter() {
        if SHIELDS.contains(&f.ability) {
            seen.casts.push((frame, f.caster, f.ability));
        }
    }
}

fn observe_bubbles(
    mut seen: ResMut<Seen>,
    bubbles: Query<&DivineShieldPending, Added<DivineShieldPending>>,
) {
    let frame = seen.frame;
    for p in bubbles.iter() {
        seen.casts
            .push((frame, p.caster, AbilityType::DivineShield));
    }
}

fn observe_rigs(mut seen: ResMut<Seen>, rigs: Query<&HealCastHand, Added<HealCastHand>>) {
    let frame = seen.frame;
    for rig in rigs.iter() {
        if matches!(rig.phase, HealCastPhase::Flare { .. }) {
            seen.launches.push((frame, rig.caster, rig.kind));
        }
    }
}

/// Play the seeded match `cfg` to its end and hand back what was observed.
fn play_seen(cfg: &str) -> Seen {
    let mut app = boot(cfg);
    for _ in 0..MAX_FRAMES {
        app.update();
        if app.world().resource::<State<GameState>>().get() != &GameState::PlayMatch {
            break;
        }
    }
    std::mem::take(&mut *app.world_mut().resource_mut::<Seen>())
}

fn kind(ability: AbilityType) -> HealCastKind {
    HealCastKind::for_ability(ability).expect("a shield routed to a hand family")
}

/// Paladin, Priest and Mage against a melee team on the Paladin: every
/// shield's caster is in the match. The Paladin is staged below the bubble
/// trigger at the gates (`stage_low_paladin`), so every match casts a Divine
/// Shield; over the bank, each shield is cast at least once. (Its bubble used
/// to come from the seeds alone, until AS-303's gear and the Rogue poisons (#380)
/// together moved both seeds off it.)
const BANK: [u64; 2] = [1, 3];

#[test]
fn each_shield_cast_flashes_its_hands_exactly_once_in_a_real_match() {
    let mut cast_counts = [0usize; SHIELDS.len()];
    for seed in BANK {
        let seen = play_seen(&format!(
            r#"{{"team1":["Paladin","Priest","Mage"],"team2":["Warrior","Rogue","Hunter"],
                "team2_kill_target":0,"random_seed":{seed}}}"#
        ));
        for (i, ability) in SHIELDS.iter().enumerate() {
            cast_counts[i] += seen.casts.iter().filter(|c| c.2 == *ability).count();
        }
        assert!(
            seen.casts.iter().any(|c| c.2 == AbilityType::DivineShield),
            "seed {seed}: the Paladin staged below its bubble trigger never bubbled"
        );

        // Per cast: the two hands, once.
        for &(frame, caster, ability) in &seen.casts {
            let rigs = seen
                .launches
                .iter()
                .filter(|(f, c, k)| {
                    *c == caster
                        && *k == kind(ability)
                        && (frame..=frame + HANDS_WITHIN).contains(f)
                })
                .count();
            assert_eq!(
                rigs, 2,
                "seed {seed}: the {ability:?} cast at frame {frame} grew {rigs} launch hand \
                 rigs, not one per hand"
            );
        }

        // ...and nothing else: every launch rig these casters grew belongs to
        // one of their shield casts, so a second reader that fires a frame
        // later than the window cannot hide outside it.
        let casters: Vec<(Entity, HealCastKind)> =
            seen.casts.iter().map(|&(_, c, a)| (c, kind(a))).collect();
        for &(caster, k) in &casters {
            let casts = seen
                .casts
                .iter()
                .filter(|&&(_, c, a)| c == caster && kind(a) == k)
                .count();
            let rigs = seen
                .launches
                .iter()
                .filter(|&&(_, c, rk)| c == caster && rk == k)
                .count();
            assert_eq!(
                rigs,
                2 * casts,
                "seed {seed}: a caster grew {rigs} {k:?} launch rigs over {casts} shield casts"
            );
        }
        println!(
            "seed {seed}: {} shield casts, {} launch rigs",
            seen.casts.len(),
            seen.launches.len()
        );
    }
    for (ability, n) in SHIELDS.iter().zip(cast_counts) {
        println!("{ability:?}: {n} casts over the bank");
        assert!(n >= 1, "vacuous: no {ability:?} cast over the bank");
    }
}
