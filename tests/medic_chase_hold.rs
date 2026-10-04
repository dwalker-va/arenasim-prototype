//! AS-205 — the medic chase holds for its commit window once sight returns.
//!
//! The medic chase walks a healer round cover to a dying teammate it cannot
//! see. It used to let go the frame sight returned, handing movement back to
//! the posture tick; when the posture's own goal (a formation point behind the
//! same pillar) stepped the healer straight back out of sight, the chase
//! re-armed the next frame, and on a pillar edge the two traded the healer
//! every frame — `SeekLos` and `FormationShift` alternating at 60Hz.
//!
//! The rule now: a chase that is live keeps walking to its teammate until its
//! directive's commit window (`commit_window`) has run, while that teammate is
//! still alive and dying. Only the RELEASE waits. The onset never does: a dying
//! teammate the healer cannot see starts a chase on the frame it qualifies,
//! whatever the posture was doing, and a more-injured one takes the chase over
//! at once.
//!
//! Two kinds of test:
//! - scene tests drive the real posture ticks of every healer machine (Priest,
//!   Paladin, and the caster-healer machine the Shaman and Druid share) over a
//!   hand-built scene, frame by frame;
//! - `the_repro_druid_commits_to_one_goal` plays the match the defect was
//!   found in and counts the Druid's goal switches per second.

use std::collections::BTreeMap;

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::caster_healer_posture::evaluate_caster_healer_posture;
use arenasim::states::play_match::class_ai::paladin::evaluate_paladin_posture;
use arenasim::states::play_match::class_ai::priest::evaluate_priest_posture;
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
use arenasim::states::play_match::decision_trace::DecisionTrace;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::{
    AbilityDefinitions, AbilityType, Aura, Combatant, DispelWalkPhase, HealerPosture,
    MovementConfig, MovementDirective, MovementGoal,
};

const HEALER_POS: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const WARRIOR_POS: Vec3 = Vec3::new(16.0, 1.0, 0.0);
const MAGE_POS: Vec3 = Vec3::new(0.0, 1.0, 16.0);
const T0: f32 = 20.0;
/// Below the shipped `urgency_hp_threshold` (0.5).
const DYING: f32 = 0.3;

static EMPTY_DR: BTreeMap<Entity, arenasim::states::play_match::DRTracker> = BTreeMap::new();
static EMPTY_CD: BTreeMap<Entity, BTreeMap<AbilityType, f32>> = BTreeMap::new();

/// Every healer posture machine the medic chase runs in.
const HEALERS: [CharacterClass; 4] = [
    CharacterClass::Priest,
    CharacterClass::Paladin,
    CharacterClass::Shaman,
    CharacterClass::Druid,
];

fn info(entity: Entity, team: u8, class: CharacterClass, position: Vec3, hp: f32) -> CombatantInfo {
    CombatantInfo {
        entity,
        team,
        slot: 0,
        class,
        current_health: 100.0 * hp,
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

/// The pillar between the healer and the Warrior.
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(8.0, 0.0),
        radius: 2.0,
        base_y: 0.0,
        height: 10.0,
    }]
}

/// A healer of `class` at the origin, a dying Warrior behind the pillar, a
/// Mage at 70% in sight, and an enemy Hunter beside the Warrior attacking it —
/// far outside the healer's danger radius, so the healer stays FREE.
struct Scene {
    class: CharacterClass,
    world: World,
    healer: Entity,
    warrior: Entity,
    roster: BTreeMap<Entity, CombatantInfo>,
    auras: BTreeMap<Entity, Vec<Aura>>,
    combatant: Combatant,
    obstacles: Vec<ObstacleVolume>,
    state: HealerPosture,
    /// The last tick's cast deferral: the plan's `escape_defer` (Priest,
    /// Shaman, Druid) or `cast_defer` (Paladin). A heal on a target ABOVE it
    /// is deferred; one at or below it fires.
    defer: Option<f32>,
}

fn scene(class: CharacterClass) -> Scene {
    let mut world = World::new();
    let healer = world.spawn_empty().id();
    let warrior = world.spawn_empty().id();
    let mage = world.spawn_empty().id();
    let hunter = world.spawn_empty().id();

    let mut combatant = Combatant::new(1, 0, class);
    combatant.target = Some(hunter);
    let mut me = info(healer, 1, class, HEALER_POS, 1.0);
    me.current_mana = combatant.current_mana;
    me.max_mana = combatant.max_mana;
    let mut enemy = info(
        hunter,
        2,
        CharacterClass::Hunter,
        WARRIOR_POS + Vec3::new(5.0, 0.0, 0.0),
        1.0,
    );
    enemy.target = Some(warrior);
    let mut warrior_info = info(warrior, 1, CharacterClass::Warrior, WARRIOR_POS, DYING);
    warrior_info.target = Some(hunter);
    let mut mage_info = info(mage, 1, CharacterClass::Mage, MAGE_POS, 0.7);
    mage_info.target = Some(hunter);
    let roster = [me, warrior_info, mage_info, enemy]
        .into_iter()
        .map(|u| (u.entity, u))
        .collect();

    Scene {
        class,
        world,
        healer,
        warrior,
        roster,
        auras: BTreeMap::new(),
        combatant,
        obstacles: pillar(),
        state: HealerPosture::new(0.0),
        defer: None,
    }
}

impl Scene {
    /// One posture tick of this healer's machine at `now`, against the
    /// directive the previous tick left in the world. Returns the movement
    /// triggers it traced, and records the plan's cast deferral in `defer`.
    fn tick(&mut self, now: f32) -> Vec<String> {
        let abilities = AbilityDefinitions::default();
        let movement = MovementConfig::default();
        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        let directive = self.world.get::<MovementDirective>(self.healer).copied();
        let defer = {
            let ctx = CombatContext::new(
                self.healer,
                1,
                &self.roster,
                &self.auras,
                &EMPTY_DR,
                &EMPTY_CD,
                &self.obstacles,
                Default::default(),
                Default::default(),
            );
            let mut commands = Commands::new(&mut queue, &self.world);
            let state = Some(&mut self.state);
            match self.class {
                CharacterClass::Priest => {
                    evaluate_priest_posture(
                        &mut commands,
                        self.healer,
                        &self.combatant,
                        HEALER_POS,
                        &ctx,
                        &abilities,
                        None,
                        state,
                        directive.as_ref(),
                        &movement,
                        now,
                        &mut trace,
                    )
                    .escape_defer
                }
                CharacterClass::Paladin => {
                    evaluate_paladin_posture(
                        &mut commands,
                        &abilities,
                        self.healer,
                        &self.combatant,
                        HEALER_POS,
                        None,
                        &ctx,
                        state,
                        directive.as_ref(),
                        &movement,
                        now,
                        &mut trace,
                    )
                    .cast_defer
                }
                CharacterClass::Shaman | CharacterClass::Druid => {
                    let block = if self.class == CharacterClass::Shaman {
                        &movement.shaman
                    } else {
                        &movement.druid
                    };
                    evaluate_caster_healer_posture(
                        &mut commands,
                        self.healer,
                        &self.combatant,
                        HEALER_POS,
                        &ctx,
                        state,
                        directive.as_ref(),
                        &movement,
                        block,
                        false,
                        now,
                        &mut trace,
                    )
                    .escape_defer
                }
                other => panic!("{other:?} runs no healer posture"),
            }
        };
        self.defer = defer;
        queue.apply(&mut self.world);
        trace
            .pending_events
            .drain(..)
            .filter_map(|e| {
                let v = serde_json::to_value(&e).ok()?;
                Some(v["trigger"].as_str()?.to_string())
            })
            .collect()
    }

    /// The point the healer is walking to, if its directive is a point walk.
    fn walking_to(&self) -> Option<Vec3> {
        self.world
            .get::<MovementDirective>(self.healer)
            .and_then(|d| match d.goal {
                MovementGoal::Point(p) => Some(p),
                _ => None,
            })
    }

    fn chasing(&self) -> bool {
        self.state.medic_target == Some(self.warrior) && self.walking_to() == Some(WARRIOR_POS)
    }

    fn set_warrior_hp(&mut self, hp: f32) {
        self.roster.get_mut(&self.warrior).unwrap().current_health = 100.0 * hp;
    }

    /// Start a chase at `T0`: the Warrior is dying behind the pillar.
    fn start_chase(&mut self) {
        let triggers = self.tick(T0);
        assert_eq!(triggers, vec!["SeekLos".to_string()], "{:?}", self.class);
        assert!(self.chasing(), "{:?}: the chase starts at once", self.class);
    }
}

fn commit_window() -> f32 {
    MovementConfig::default().shared.commit_window
}

/// Sight returns mid-window: the chase keeps walking to its teammate, silently,
/// until the commit window has run — then lets go to the posture.
#[test]
fn a_chase_that_regains_sight_holds_its_commit_window() {
    for class in HEALERS {
        let mut s = scene(class);
        s.start_chase();

        // Sight is back (the pillar is gone), every frame up to the window's end.
        s.obstacles.clear();
        let window_end = T0 + commit_window();
        let mut now = T0 + 1.0 / 60.0;
        while now < window_end - 1e-3 {
            let triggers = s.tick(now);
            assert!(triggers.is_empty(), "{class:?} at {now}: {triggers:?}");
            assert!(
                s.chasing(),
                "{class:?} let go of the chase at {now}, inside its window"
            );
            now += 1.0 / 60.0;
        }

        // The window has run with the teammate in sight: the posture takes over.
        let triggers = s.tick(window_end);
        assert!(
            !triggers.contains(&"SeekLos".to_string()),
            "{class:?}: {triggers:?}"
        );
        assert_eq!(
            s.state.medic_target, None,
            "{class:?}: released at the window's end"
        );
        assert_ne!(s.walking_to(), Some(WARRIOR_POS), "{class:?}");
    }
}

/// The hold delays only the release. Once the posture has the healer again, a
/// dying teammate going out of sight starts a chase on that very frame.
#[test]
fn the_onset_is_never_held() {
    for class in HEALERS {
        let mut s = scene(class);
        s.obstacles.clear();
        // The posture has the healer: nothing to chase in open ground.
        let triggers = s.tick(T0);
        assert!(
            !triggers.contains(&"SeekLos".to_string()),
            "{class:?}: {triggers:?}"
        );
        assert_eq!(s.state.medic_target, None);

        // One frame later the Warrior is behind the pillar.
        s.obstacles = pillar();
        let triggers = s.tick(T0 + 1.0 / 60.0);
        assert_eq!(triggers, vec!["SeekLos".to_string()], "{class:?}");
        assert!(
            s.chasing(),
            "{class:?}: the chase starts on the frame it qualifies"
        );
    }
}

/// Nor does the hold keep the chase on its teammate when another is worse off:
/// a more-injured teammate out of sight takes the chase over on the frame it
/// qualifies, mid-window.
#[test]
fn a_more_injured_teammate_takes_a_held_chase_over_at_once() {
    for class in HEALERS {
        let mut s = scene(class);
        s.start_chase();
        // Sight of the Warrior is back; the chase holds on it.
        s.obstacles.clear();
        s.tick(T0 + 0.1);
        assert!(s.chasing(), "{class:?}");

        // The Mage drops below the Warrior, behind a pillar of its own.
        let mage = *s
            .roster
            .iter()
            .find(|(_, u)| u.class == CharacterClass::Mage)
            .unwrap()
            .0;
        s.roster.get_mut(&mage).unwrap().current_health = 10.0;
        s.obstacles = vec![ObstacleVolume::Cylinder {
            center_xz: Vec2::new(0.0, 8.0),
            radius: 2.0,
            base_y: 0.0,
            height: 10.0,
        }];
        let triggers = s.tick(T0 + 0.1 + 1.0 / 60.0);
        assert_eq!(triggers, vec!["SeekLos".to_string()], "{class:?}");
        assert_eq!(s.state.medic_target, Some(mage), "{class:?}");
        assert_eq!(s.walking_to(), Some(MAGE_POS), "{class:?}");
    }
}

/// The hold is for a teammate still dying: one healed just past the urgency
/// line, or dead, releases the chase at once, mid-window — and one a hair
/// below the line keeps it.
#[test]
fn a_held_chase_lets_go_at_once_when_its_teammate_recovers_or_dies() {
    let threshold = MovementConfig::default().shared.urgency_hp_threshold;
    for class in HEALERS {
        for outcome in [
            "stays just below the line",
            "recovers just past the line",
            "dies",
        ] {
            let mut s = scene(class);
            s.start_chase();
            s.obstacles.clear();
            s.tick(T0 + 0.1);
            assert!(s.chasing(), "{class:?}: held while the Warrior is dying");

            match outcome {
                "stays just below the line" => s.set_warrior_hp(threshold - 0.01),
                "recovers just past the line" => s.set_warrior_hp(threshold + 0.01),
                _ => {
                    s.set_warrior_hp(0.0);
                    s.roster.get_mut(&s.warrior).unwrap().is_alive = false;
                }
            }
            s.tick(T0 + 0.2);
            if outcome == "stays just below the line" {
                assert!(
                    s.chasing(),
                    "{class:?}: a teammate at {} still holds the chase",
                    threshold - 0.01
                );
            } else {
                assert_eq!(
                    s.state.medic_target, None,
                    "{class:?}: a teammate who {outcome} releases the chase mid-window"
                );
            }
        }
    }
}

/// A hold walks the healer to a teammate it can SEE and who is dying, so the
/// heal that teammate needs must fire: the hold's cast deferral leaves a heal
/// on a target at or below the urgency line alone, exactly as a live chase's
/// does. (It defers only heals on targets above that line.)
#[test]
fn a_critical_heal_fires_inside_a_hold() {
    let threshold = MovementConfig::default().shared.urgency_hp_threshold;
    for class in HEALERS {
        let mut s = scene(class);
        s.start_chase();
        let chasing_defer = s.defer;
        assert_eq!(
            chasing_defer,
            Some(threshold),
            "{class:?}: the chase's own deferral"
        );

        // Inside the window, the dying Warrior in sight: the chase holds.
        s.obstacles.clear();
        s.set_warrior_hp(threshold - 0.01);
        s.tick(T0 + 0.1);
        assert!(s.chasing(), "{class:?}: the chase holds");
        let warrior_hp = threshold - 0.01;
        assert!(
            s.defer.is_none_or(|t| warrior_hp <= t),
            "{class:?}: the hold defers a heal on the dying Warrior it is walking to \
             (defer {:?}, Warrior at {warrior_hp})",
            s.defer
        );
        assert_eq!(
            s.defer, chasing_defer,
            "{class:?}: a hold defers as the chase does"
        );
    }
}

/// `medic_target` also marks a live dispel walk. The hold is the medic chase's
/// alone: a dispel walk whose teammate is in sight is not kept walking by it.
#[test]
fn the_hold_does_not_keep_a_dispel_walk() {
    for class in [CharacterClass::Priest, CharacterClass::Paladin] {
        let mut s = scene(class);
        s.start_chase();
        // The same walk, marked as the dispel walk's, with nothing owed.
        s.state.dispel_walk = DispelWalkPhase::Walking;
        s.obstacles.clear();
        s.tick(T0 + 0.1);
        assert_eq!(s.state.medic_target, None, "{class:?}");
        assert_ne!(s.walking_to(), Some(WARRIOR_POS), "{class:?}");
    }
}

// ---------------------------------------------------------------------------
// The played repro
// ---------------------------------------------------------------------------

/// Hunter+Paladin vs Mage+Druid on Nagrand (`TeamPlan`), seed 2: the match the
/// defect was found in. From 94.16s (combat time) the Mage sits at 40% behind
/// a pillar edge from the Druid, whose formation point lies on the occluded
/// side of that edge — so the formation walk takes sight away and the medic
/// chase gives it back.
fn repro_config() -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Hunter".into(), "Paladin".into()],
        team2: vec!["Mage".into(), "Druid".into()],
        map: "PillaredArena".to_string(),
        ai_profile: Some("TeamPlan".to_string()),
        random_seed: Some(2),
        ..Default::default()
    }
}

/// The Druid's movement decisions as `(combat time, trigger)`.
fn druid_movement(config: HeadlessMatchConfig) -> Vec<(f32, String)> {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    run_headless_match_observed(
        config,
        true,
        Some(TraceConfig {
            output_path: path.clone(),
        }),
        |_| {},
    )
    .expect("headless match");
    let body = std::fs::read_to_string(&path).expect("read trace");
    let _ = std::fs::remove_file(&path);
    body.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|e| e["kind"] == "movement_decision" && e["actor"]["class"] == "Druid")
        .map(|e| {
            (
                e["sim_time"].as_f64().unwrap() as f32,
                e["trigger"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// The most goal switches — consecutive movement decisions with different
/// triggers — that fall inside any one second.
fn max_switches_per_second(events: &[(f32, String)]) -> usize {
    let switches: Vec<f32> = events
        .windows(2)
        .filter(|w| w[0].1 != w[1].1)
        .map(|w| w[1].0)
        .collect();
    (0..switches.len())
        .map(|i| switches[i..].partition_point(|&t| t < switches[i] + 1.0))
        .max()
        .unwrap_or(0)
}

/// The guard. Before the hold the Druid switched goal every frame from 94.16s
/// to the end of the match — about 60 switches a second. A chase now runs at
/// least a commit window (0.6s) before the formation point can have the Druid
/// back, so a chase-formation-chase cycle takes over 0.6s: at most four
/// switches fit in any second.
#[test]
fn the_repro_druid_commits_to_one_goal() {
    let events = druid_movement(repro_config());

    // The scenario still happens: the Druid chases the Mage round the pillar
    // edge, more than once, in the repro window — until its heal lifts the
    // Mage back over the urgency threshold and the chase has no teammate.
    let chases_in_window = events
        .iter()
        .filter(|(t, trig)| *t >= 94.0 && trig == "SeekLos")
        .count();
    assert!(
        chases_in_window >= 2,
        "the repro no longer reaches the pillar edge: {chases_in_window} SeekLos at/after 94s \
         — re-seed this probe"
    );

    let worst = max_switches_per_second(&events);
    assert!(
        worst <= 4,
        "the Druid switched movement goal {worst} times inside one second \
         (the medic chase and the formation point trading it every frame)"
    );
}

/// The window counter itself: a frame-by-frame alternation reads as ~60 a
/// second, and a 0.6s cadence as at most 4.
#[test]
fn max_switches_per_second_counts_a_sliding_second() {
    let frame = 1.0 / 60.0;
    let flicker: Vec<(f32, String)> = (0..120)
        .map(|i| {
            let trig = if i % 2 == 0 {
                "SeekLos"
            } else {
                "FormationShift"
            };
            (i as f32 * frame, trig.to_string())
        })
        .collect();
    assert!(max_switches_per_second(&flicker) >= 59);

    let cadence: Vec<(f32, String)> = (0..10)
        .map(|i| {
            let trig = if i % 2 == 0 {
                "SeekLos"
            } else {
                "FormationShift"
            };
            (
                i as f32 * 0.6 + if i % 2 == 0 { 0.0 } else { frame },
                trig.to_string(),
            )
        })
        .collect();
    assert!(max_switches_per_second(&cadence) <= 4);
}
