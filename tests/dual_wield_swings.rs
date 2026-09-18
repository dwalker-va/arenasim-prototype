//! Durable guard: a second weapon in the off hand actually SWINGS.
//!
//! The defect this closes is the one AS-60 named — before it, an off-hand
//! weapon contributed its stat bonuses and zero damage, so "any one-hander
//! fits either hand" would have been a cosmetic change. These probes drive the
//! real `combat_auto_attack` system in a minimal Bevy App (the
//! `auto_attack_los.rs` harness pattern: MinimalPlugins clock, gates forced
//! open, only that one system registered) so they pin the WIRING — the second
//! timer, the halved damage, the dual-wield miss roll — and not a helper in
//! isolation.
//!
//! Every assertion here is on damage that actually landed on a victim.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::MinimalPlugins;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::combat_core::combat_auto_attack;
use arenasim::states::play_match::components::{Combatant, GameRng, MatchCountdown};
use arenasim::states::play_match::constants::DUAL_WIELD_MISS_CHANCE;
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::AbilityDefinitions;
use arenasim::CharacterClass;

const TICKS_PER_SEC: u32 = 60;
/// Long enough that the miss rate averages out: ~60 swings per hand, so a
/// ratio claim below is a couple of standard deviations wide rather than a
/// coin flip.
const WINDOW_TICKS: u32 = 60 * TICKS_PER_SEC;

/// Minimal App running only `combat_auto_attack`, obstacle-free, gates open,
/// on a manual 1/60s clock (the headless runner's strategy).
fn harness_app(seed: u64) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / f64::from(TICKS_PER_SEC),
        )))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(GameRng::from_seed(seed))
        .insert_resource(AbilityDefinitions::default())
        .insert_resource(ActiveMapGeometry {
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        })
        .add_systems(Update, combat_auto_attack);
    app
}

/// A melee attacker with crit switched OFF, so every landed swing deals
/// exactly its weapon damage and the only variance left in the probe is the
/// dual-wield miss roll — the thing under test.
fn spawn_attacker(app: &mut App, main_damage: f32, main_speed: f32) -> Entity {
    let mut combatant = Combatant::new(1, 0, CharacterClass::Warrior);
    combatant.attack_damage = main_damage;
    combatant.attack_speed = main_speed;
    combatant.crit_chance = 0.0;
    app.world_mut()
        .spawn((Transform::from_translation(Vec3::ZERO), combatant))
        .id()
}

/// A stationary victim in melee range with a health pool deep enough that it
/// survives the whole window (a death would silently truncate the probe).
fn spawn_victim(app: &mut App) -> Entity {
    let mut combatant = Combatant::new(2, 0, CharacterClass::Warrior);
    combatant.max_health = 1_000_000.0;
    combatant.current_health = 1_000_000.0;
    // Never swing back: this probe measures one direction only.
    combatant.attack_speed = 0.000_01;
    app.world_mut()
        .spawn((
            Transform::from_translation(Vec3::new(0.0, 0.0, 1.0)),
            combatant,
        ))
        .id()
}

fn arm_off_hand(app: &mut App, attacker: Entity, damage: f32, speed: f32) {
    let mut c = app.world_mut().get_mut::<Combatant>(attacker).unwrap();
    c.offhand_damage = damage;
    c.offhand_speed = speed;
}

fn set_target(app: &mut App, attacker: Entity, target: Entity) {
    app.world_mut()
        .get_mut::<Combatant>(attacker)
        .unwrap()
        .target = Some(target);
}

fn damage_dealt(app: &App, entity: Entity) -> f32 {
    app.world().get::<Combatant>(entity).unwrap().damage_dealt
}

fn run(app: &mut App, ticks: u32) {
    for _ in 0..ticks {
        app.update();
    }
}

/// Damage a single-wielding attacker deals over the window: the control, and
/// the number every dual-wield claim below is relative to.
fn single_wield_damage(seed: u64, main_damage: f32, main_speed: f32) -> f32 {
    let mut app = harness_app(seed);
    let attacker = spawn_attacker(&mut app, main_damage, main_speed);
    let victim = spawn_victim(&mut app);
    set_target(&mut app, attacker, victim);
    run(&mut app, WINDOW_TICKS);
    damage_dealt(&app, attacker)
}

/// Damage a dual-wielding attacker deals over the same window at the same seed.
fn dual_wield_damage(
    seed: u64,
    main_damage: f32,
    main_speed: f32,
    off_damage: f32,
    off_speed: f32,
) -> f32 {
    let mut app = harness_app(seed);
    let attacker = spawn_attacker(&mut app, main_damage, main_speed);
    let victim = spawn_victim(&mut app);
    arm_off_hand(&mut app, attacker, off_damage, off_speed);
    set_target(&mut app, attacker, victim);
    run(&mut app, WINDOW_TICKS);
    damage_dealt(&app, attacker)
}

/// The claim the card is actually about: an off-hand weapon adds damage.
///
/// Stated as a comparison against the SAME attacker without one, at the same
/// seed, because an absolute number here would be pinning the miss rolls of
/// one seed rather than the mechanism.
#[test]
fn an_off_hand_weapon_adds_damage() {
    let single = single_wield_damage(1, 20.0, 1.0);
    let dual = dual_wield_damage(1, 20.0, 1.0, 10.0, 1.0);

    assert!(
        single > 0.0,
        "the control dealt no damage — the probe proves nothing"
    );
    assert!(
        dual > single,
        "a second weapon must add damage: single-wield {single}, dual-wield {dual}"
    );
}

/// The off hand swings at HALF damage, not full.
///
/// Both hands at the same speed and the same listed damage, so what separates
/// this ratio from a doubled main hand is the off-hand penalty — and what
/// separates it from `1 + OFFHAND_DAMAGE_MULTIPLIER` is the miss penalty,
/// which the single-wield control does not pay. Both are folded into the
/// expectation; getting either wrong lands outside the band.
///
/// The band is wide because ~120 miss rolls sit inside it, and narrow enough
/// to exclude the two mistakes it is here to catch: an off hand at FULL damage
/// reads 1.62, and one at a quarter reads 1.01.
///
/// The off-hand damage is the LITERAL half of the main hand's, not
/// `20.0 * OFFHAND_DAMAGE_MULTIPLIER`. Reading that constant here would put it
/// on both sides of the comparison — it would set the input and the
/// expectation together, and retuning it would move both and prove nothing.
/// That the constant is what `apply_equipment` actually applies is pinned
/// separately, by `apply_equipment_offhand_weapon_arms_the_second_swing`.
#[test]
fn the_off_hand_swings_at_half_damage() {
    let single = single_wield_damage(7, 20.0, 1.0);
    let dual = dual_wield_damage(7, 20.0, 1.0, 10.0, 1.0);

    // 1.5x the main hand's contribution, less the miss rate the control does
    // not pay. The miss rate IS read from its constant: it is a different
    // mechanism this probe has to compensate for, not the one under test.
    let expected_ratio = 1.5 * (1.0 - DUAL_WIELD_MISS_CHANCE);
    let ratio = dual / single;
    assert!(
        (ratio - expected_ratio).abs() < 0.15,
        "dual-wield damage should be about {expected_ratio}x single-wield, got {ratio}x \
         (single {single}, dual {dual})"
    );
}

/// The off hand keeps its OWN swing timer, off its own weapon's speed.
///
/// A faster off-hand weapon of the same listed damage must land more of it. If
/// the second hand were driven off the main hand's timer this reads as a tie.
#[test]
fn the_off_hand_runs_on_its_own_timer() {
    let slow = dual_wield_damage(3, 20.0, 1.0, 10.0, 1.0);
    let fast = dual_wield_damage(3, 20.0, 1.0, 10.0, 3.0);

    assert!(
        fast > slow * 1.3,
        "a 3x faster off hand must land substantially more damage: \
         slow off hand {slow}, fast off hand {fast}"
    );
}

/// Dual wield costs accuracy, and the cost is real rather than declared.
///
/// The "never misses" figure is not an analytic ceiling — it is COUNTED off
/// the single-wield control, which misses nothing and does not crit, so its
/// damage divided by its weapon damage is exactly the number of swings the
/// window afforded. At equal speeds the off hand gets the same count. A loose
/// ceiling here would let the upper bound pass with zero misses, which is the
/// vacuity this probe exists to rule out.
///
/// Upper bound: the dual wielder lands strictly less than that. Over ~120
/// swings at `DUAL_WIELD_MISS_CHANCE`, a run with no misses has probability
/// about `(1 - p)^120`, far below any flake floor.
///
/// Lower bound: it lands at least the share the stated miss rate predicts,
/// less a wide margin — so the penalty is not just present but approximately
/// the size claimed.
#[test]
fn dual_wield_misses_cost_damage() {
    let main_damage = 20.0;
    let main_speed = 1.0;
    // Literal, not `main_damage * OFFHAND_DAMAGE_MULTIPLIER`: the ceiling
    // below is built from this number, so reading the constant would set the
    // input and the bound together.
    let off_damage = 10.0;
    let off_speed = main_speed;

    let single = single_wield_damage(11, main_damage, main_speed);
    let dual = dual_wield_damage(11, main_damage, main_speed, off_damage, off_speed);

    let swings = (single / main_damage).round();
    assert!(
        swings >= 50.0,
        "the control only afforded {swings} swings — too few for this claim"
    );
    let never_misses = swings * main_damage + swings * off_damage;

    assert!(
        dual < never_misses,
        "a dual wielder that never missed would deal {never_misses} over {swings} \
         swings per hand; it dealt {dual}, so no swing missed — the penalty is not \
         being applied"
    );
    let predicted = never_misses * (1.0 - DUAL_WIELD_MISS_CHANCE);
    assert!(
        dual > predicted * 0.85,
        "dual-wield damage {dual} is far below the {predicted} the stated \
         {DUAL_WIELD_MISS_CHANCE} miss rate predicts — swings are being lost to \
         something other than the miss roll"
    );
}

/// The miss penalty is charged to the MAIN hand too, as it is in Classic.
///
/// Arming an off hand that deals no damage at all leaves the main hand's
/// swings untouched in every respect except the new roll, so a main hand that
/// still lands everything reads here as a tie.
#[test]
fn the_miss_penalty_reaches_the_main_hand() {
    let single = single_wield_damage(5, 20.0, 1.0);
    // A real second weapon, but one whose own contribution is negligible: a
    // very slow off hand swings about twice in the window.
    let dual_slow_off_hand = dual_wield_damage(5, 20.0, 1.0, 0.01, 0.1);

    assert!(
        dual_slow_off_hand < single,
        "dual wielding must cost the main hand accuracy: single-wield {single}, \
         dual-wield with a negligible off hand {dual_slow_off_hand}"
    );
}

/// How many values the app's `GameRng` has drawn, recovered by replaying a
/// fresh RNG at the same seed until its stream lines up with the app's.
///
/// This is the independent observation the probe below needs: it interrogates
/// the RNG, not the system under test, so no arithmetic inside
/// `combat_auto_attack` can make it agree by construction. Matches on TWO
/// consecutive values, so a chance `f32` collision cannot report a wrong
/// position. `None` means the stream did not line up anywhere in range — a
/// failure to measure, which the caller must treat as a failure.
fn rng_draws_taken(next_two: (f32, f32), seed: u64, search_limit: u32) -> Option<u32> {
    for n in 0..=search_limit {
        let mut reference = GameRng::from_seed(seed);
        for _ in 0..n {
            reference.random_f32();
        }
        if (reference.random_f32(), reference.random_f32()) == next_two {
            return Some(n);
        }
    }
    None
}

/// A combatant with no second weapon runs the old path exactly: no off-hand
/// timer, and — the property every existing balance baseline rests on — NO
/// EXTRA RNG DRAW.
///
/// **Counting damage cannot show this, and an earlier version of this probe
/// wrongly claimed it could.** It argued that a single wielder's total stays an
/// exact multiple of its weapon damage because nothing missed. But a missed
/// swing contributes zero, so the total stays an exact multiple either way, and
/// the assertion could not fail for the reason it gave: removing the
/// `dual_wielding &&` gate so every attacker rolls to miss failed fifteen other
/// tests and left this one green.
///
/// So count the DRAWS instead. `roll_crit` draws unconditionally — crit chance
/// zero still consumes a value — and nothing else in this harness touches the
/// RNG, so a correct single wielder advances the stream exactly once per landed
/// swing. Ungate the miss roll and it advances twice per swing while landing
/// fewer, and the two numbers separate immediately.
#[test]
fn a_single_wielder_draws_no_dual_wield_rng() {
    const SEED: u64 = 42;
    let mut app = harness_app(SEED);
    let attacker = spawn_attacker(&mut app, 20.0, 1.0);
    let victim = spawn_victim(&mut app);
    set_target(&mut app, attacker, victim);
    run(&mut app, WINDOW_TICKS);

    let combatant = app.world().get::<Combatant>(attacker).unwrap();
    assert!(
        !combatant.is_dual_wielding(),
        "the control must not be dual wielding"
    );
    assert_eq!(
        combatant.offhand_timer, 0.0,
        "a single wielder must not tick an off-hand timer"
    );

    let dealt = damage_dealt(&app, attacker);
    let landed_swings = (dealt / 20.0).round() as u32;
    // Non-vacuity only: enough swings that one draw each is plainly distinct
    // from two. Deliberately well BELOW the ~59 a correct build lands and below
    // the ~48 an ungated miss roll would leave, so this guard can never
    // preempt the draw comparison below and report a short window when the
    // real finding is an extra roll.
    assert!(
        landed_swings >= 30,
        "the control only landed {landed_swings} swings — too few to separate \
         one draw per swing from two"
    );

    // Drawing these two advances the app's RNG, so take them last.
    let next_two = {
        let mut rng = app.world_mut().resource_mut::<GameRng>();
        (rng.random_f32(), rng.random_f32())
    };
    // Generous ceiling: twice a swing-per-tick upper bound, so a build that
    // draws far MORE than expected is still located and reported rather than
    // failing as "could not measure".
    let draws = rng_draws_taken(next_two, SEED, WINDOW_TICKS * 2)
        .expect("could not locate the app's RNG position — the probe cannot measure");

    assert_eq!(
        draws, landed_swings,
        "a single wielder must draw exactly once per landed swing (the crit \
         roll): it landed {landed_swings} swings and drew {draws} values, so \
         something else is rolling — the dual-wield miss gate is the suspect"
    );
}
