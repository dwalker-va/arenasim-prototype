//! Decision properties (tier 2 of the AI testing pyramid,
//! `docs/design/proposals/2026-10-11-ai-architecture.md` §3.2): invariants
//! every class decision must hold in EVERY situation, checked over a
//! deterministic grid of scenes for all nine classes.
//!
//! Each scene runs the production class dispatch for one tick
//! ([`Scene::decide`] calls `dispatch_class_decision`, the entry point
//! `decide_abilities` runs), and each property below is checked against every
//! ability the decision chose. The classes are trusted with nothing: one grid
//! drives all nine, and a property names no class, so a new `try_*` that
//! forgets a gate fails here without anyone writing a test for it.
//!
//! # The grid
//!
//! Per class, the full product of five CORE axes, times a list of SITUATIONS:
//!
//! - **distance** to the unit's target, straddling every range edge the
//!   class's own kit declares (each `range` and `min_range`, 0.05yd either
//!   side) plus point-blank — derived from `abilities.ron`, so a new ability's
//!   range edge joins the grid by itself;
//! - **resource**: full, 15%, empty;
//! - **cooldowns**: none, or alternate halves of the kit's cooldown abilities
//!   cooling down;
//! - **sight**: clear, or a pillar between the unit and its target (only where
//!   one fits — from 4yd out);
//! - **upkeep**: unbuffed, or the kit's own buffs (and a Shaman's totems)
//!   already up, so the decision reaches past "refresh the buff".
//!
//! The situations vary health, death, auras, an enemy cast and the unit's own
//! cast locks one condition at a time, with the combinations a property needs
//! to be reachable (a heal is only cast on an injured ally, so "ally cycloned"
//! is also "ally injured"); the Rogue meets each in and out of stealth. The
//! roster is always the unit, an allied Warrior, an enemy Warrior (the unit's
//! target) and an enemy Priest. About 41,000 scenes, built in well under a
//! second in the test profile.
//!
//! The grid draws no randomness: a scene has no seed, and no class DECISION
//! reads `GameRng` (§3.3). It is computed once per test binary, in parallel
//! across classes, and every property reads the same records.
//!
//! # Known defects
//!
//! A property that fails on `main` is a correctness card, never an
//! exemption. [`KNOWN`] names each failing (property, ability) pair and its
//! card; the property's own test guards every OTHER ability, an `#[ignore]`d
//! test per card checks exactly the named ones, and
//! [`every_known_defect_is_live`] fails once a named pair stops failing — so
//! the table can never hide more than it names, and a fix deletes its row.
//!
//! # What is not here
//!
//! Incapacitation (stun, fear, polymorph, incapacitate, cyclone) is gated in
//! `decide_abilities` BEFORE the class dispatch, so a decision-level scene
//! cannot reach a dispatch from inside it — `Scene::decide` refuses, as the
//! frame loop does. What IS reachable from inside the dispatch, the
//! non-incapacitating cast locks (Silence, a school lockout, Travel Form), is
//! [`never_casts_through_a_silence_lockout_or_shapeshift`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use bevy::prelude::{Entity, Vec2, Vec3};

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::{load_ability_definitions, AbilityDefinitions};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::TrapType;
use arenasim::states::play_match::decision_trace::{CandidateStatus, RejectionReason};
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume};
use arenasim::states::play_match::movement_config::{load_movement_config, MovementConfig};
use arenasim::states::play_match::{AbilityType, Aura, AuraType, SpellSchool};

const ME: &str = "me";
const ALLY: &str = "ally";
const TARGET: &str = "target";
const HEALER: &str = "enemy healer";

/// Every scene unit stands at this height, as `Scene` places them.
const UNIT_Y: f32 = 1.0;
/// The deciding unit's spot, clear of the walls in every direction the grid
/// places another unit.
const ORIGIN: Vec2 = Vec2::new(-25.0, -15.0);
/// The ally stands along this bearing from the unit, off the line to the
/// target so the target's pillar never hides it.
const ALLY_BEARING: Vec2 = Vec2::new(0.6, 0.8);
/// Seconds left on a cooldown the grid sets.
const COOLDOWN_LEFT: f32 = 5.0;

// ---------------------------------------------------------------------------
// The grid
// ---------------------------------------------------------------------------

/// One condition the scene holds, besides the core axes.
#[derive(Clone, Copy, Debug)]
struct Situation {
    name: &'static str,
    my_health: f32,
    /// `None`: the ally is dead.
    ally_health: Option<f32>,
    ally_distance: f32,
    ally_aura: Hold,
    /// `None`: the target is dead.
    target_health: Option<f32>,
    target_aura: Hold,
    healer_aura: Hold,
    my_aura: SelfLock,
    /// The enemy healer is casting a heal (an interrupt's moment).
    healer_casting: bool,
    /// The unit has broken stealth (the Rogue starts every match in it).
    opened: bool,
}

/// An aura the grid puts on a unit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hold {
    Free,
    /// Divine Shield: immune to damage and harmful spells.
    DivineShield,
    /// Cyclone: immune to everything, friendly spells included.
    Cyclone,
    /// A Polymorph from the OTHER team (breaks on any damage).
    EnemyPolymorph,
    /// A Polymorph from the deciding unit's OWN team (breaks on any damage).
    FriendlyPolymorph,
}

/// A cast lock on the deciding unit that does not incapacitate it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SelfLock {
    Free,
    Silence,
    /// Locked out of the school most of its kit is in.
    SchoolLockout,
    /// Travel Form (applied to the Druid only).
    TravelForm,
}

const CALM: Situation = Situation {
    name: "calm",
    my_health: 1.0,
    ally_health: Some(1.0),
    ally_distance: 6.0,
    ally_aura: Hold::Free,
    target_health: Some(1.0),
    target_aura: Hold::Free,
    healer_aura: Hold::Free,
    my_aura: SelfLock::Free,
    healer_casting: false,
    opened: false,
};

fn situations(class: CharacterClass) -> Vec<Situation> {
    let injured = Situation {
        name: "ally injured",
        ally_health: Some(0.4),
        ..CALM
    };
    let mut list = vec![
        CALM,
        injured,
        Situation {
            name: "ally dying",
            ally_health: Some(0.12),
            ..CALM
        },
        Situation {
            name: "ally dead",
            ally_health: None,
            ..CALM
        },
        Situation {
            name: "ally dead, me injured",
            ally_health: None,
            my_health: 0.3,
            ..CALM
        },
        Situation {
            name: "ally injured and cycloned",
            ally_aura: Hold::Cyclone,
            ..injured
        },
        Situation {
            name: "ally injured and polymorphed",
            ally_aura: Hold::EnemyPolymorph,
            ..injured
        },
        Situation {
            name: "ally injured, just inside 40yd",
            ally_distance: 39.95,
            ..injured
        },
        Situation {
            name: "ally injured, just past 40yd",
            ally_distance: 40.05,
            ..injured
        },
        Situation {
            name: "me injured",
            my_health: 0.3,
            ..CALM
        },
        Situation {
            name: "target low",
            target_health: Some(0.15),
            ..CALM
        },
        Situation {
            name: "target dead",
            target_health: None,
            ..CALM
        },
        Situation {
            name: "target low, divine shield",
            target_health: Some(0.15),
            target_aura: Hold::DivineShield,
            ..CALM
        },
        Situation {
            name: "target low, cycloned",
            target_health: Some(0.15),
            target_aura: Hold::Cyclone,
            ..CALM
        },
        Situation {
            name: "target polymorphed by my team",
            target_aura: Hold::FriendlyPolymorph,
            ..CALM
        },
        Situation {
            name: "target low, polymorphed by my team",
            target_health: Some(0.15),
            target_aura: Hold::FriendlyPolymorph,
            ..CALM
        },
        Situation {
            name: "enemy healer polymorphed by my team",
            healer_aura: Hold::FriendlyPolymorph,
            ..CALM
        },
        Situation {
            name: "enemy healer divine shield",
            healer_aura: Hold::DivineShield,
            ..CALM
        },
        Situation {
            name: "enemy healer casting",
            healer_casting: true,
            ..CALM
        },
        Situation {
            name: "enemy healer casting, ally injured",
            healer_casting: true,
            ..injured
        },
        Situation {
            name: "silenced, ally injured",
            my_aura: SelfLock::Silence,
            ..injured
        },
        Situation {
            name: "school locked, ally injured",
            my_aura: SelfLock::SchoolLockout,
            ..injured
        },
    ];
    if class == CharacterClass::Druid {
        list.push(Situation {
            name: "travel form, ally injured",
            my_aura: SelfLock::TravelForm,
            ..injured
        });
    }
    if class == CharacterClass::Rogue {
        let opened: Vec<_> = list
            .iter()
            .map(|s| Situation { opened: true, ..*s })
            .collect();
        list.extend(opened);
    }
    list
}

#[derive(Clone, Copy, Debug)]
enum Cooldowns {
    None,
    /// Every other ability of the kit that has a cooldown (in name order),
    /// from the first.
    Even,
    /// The rest.
    Odd,
}

/// One scene's coordinates: enough to rebuild it with [`build`].
#[derive(Clone, Copy, Debug)]
struct Point {
    class: CharacterClass,
    distance: f32,
    resource: f32,
    cooldowns: Cooldowns,
    pillar: bool,
    /// The kit's upkeep auras already up on the unit and its ally.
    buffed: bool,
    situation: Situation,
}

impl Point {
    fn describe(&self) -> String {
        format!(
            "{:?}: target at {}yd, resource {}, cooldowns {:?}, {}, {}, situation \"{}\"",
            self.class,
            self.distance,
            self.resource,
            self.cooldowns,
            if self.pillar {
                "pillar between"
            } else {
                "clear sight"
            },
            if self.buffed { "buffed" } else { "unbuffed" },
            self.situation.name,
        ) + if self.situation.opened {
            ", out of stealth"
        } else {
            ""
        }
    }
}

/// The class's own kit (no pet abilities), in name order.
fn kit(defs: &AbilityDefinitions, class: CharacterClass) -> Vec<AbilityType> {
    let mut kit: Vec<AbilityType> = defs
        .iter()
        .filter(|(_, d)| d.class == class && d.pet.is_none())
        .map(|(a, _)| *a)
        .collect();
    kit.sort_by_key(|a| format!("{a:?}"));
    kit
}

/// The school the lockout situation locks: the one most of the class's
/// CASTS are in (cast-time and channelled abilities; ties broken by
/// declaration order). An interrupt locks the school of the cast it
/// interrupts, so a school the class never casts in is a lockout no match
/// reaches, and a class that never casts (`None`) is never locked out.
fn main_school(defs: &AbilityDefinitions, class: CharacterClass) -> Option<SpellSchool> {
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    for a in kit(defs, class) {
        let d = defs.get(&a).unwrap();
        if d.cast_time <= 0.0 && d.channel_duration.is_none() {
            continue;
        }
        let school = d.spell_school;
        let i = SpellSchool::all()
            .iter()
            .position(|s| *s == school)
            .unwrap();
        *counts.entry(i).or_default() += 1;
    }
    counts
        .iter()
        .max_by_key(|(i, n)| (**n, std::cmp::Reverse(**i)))
        .map(|(i, _)| SpellSchool::all()[*i])
}

/// The kit abilities that go on a cooldown when used: only these can be
/// cooling down in a match.
fn cooling_kit(defs: &AbilityDefinitions, class: CharacterClass) -> Vec<AbilityType> {
    kit(defs, class)
        .into_iter()
        .filter(|a| {
            let d = defs.get(a).unwrap();
            d.cooldown > 0.0 || d.cooldown_category.is_some()
        })
        .collect()
}

/// The auras the class's kit maintains on its own side — its buffs, armors,
/// absorbs and heals over time — which the `buffed` axis puts up in advance
/// so the decision reaches past "refresh the buff" into the rest of the
/// priority chain. Not a shapeshift or an immunity: those are states, not
/// upkeep.
fn upkeep_auras(defs: &AbilityDefinitions, class: CharacterClass, caster: Entity) -> Vec<Aura> {
    kit(defs, class)
        .into_iter()
        .filter_map(|a| {
            let d = defs.get(&a).unwrap();
            let effect = d.applies_aura.as_ref()?;
            let upkeep = !effect.aura_type.is_hostile_effect()
                && d.damage_base_max == 0.0
                && !matches!(
                    effect.aura_type,
                    AuraType::TravelForm | AuraType::DamageImmunity
                );
            upkeep.then(|| Aura {
                effect_type: effect.aura_type,
                duration: 60.0,
                magnitude: effect.magnitude,
                break_on_damage_threshold: -1.0,
                caster: Some(caster),
                ability_name: d.name.clone(),
                ..Default::default()
            })
        })
        .collect()
}

/// The distances the grid puts the target at: point-blank, and both sides of
/// every range edge in the class's kit.
fn distances(defs: &AbilityDefinitions, class: CharacterClass) -> Vec<f32> {
    let mut edges = vec![1.0_f32];
    for a in kit(defs, class) {
        let d = defs.get(&a).unwrap();
        for edge in [Some(d.range), d.min_range].into_iter().flatten() {
            if edge > 0.0 && edge <= 45.0 {
                edges.push(edge - 0.05);
                edges.push(edge + 0.05);
            }
        }
    }
    edges.sort_by(f32::total_cmp);
    edges.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    edges
}

fn points(defs: &AbilityDefinitions, class: CharacterClass) -> Vec<Point> {
    let mut out = Vec::new();
    let lockable = main_school(defs, class).is_some();
    for situation in situations(class) {
        if situation.my_aura == SelfLock::SchoolLockout && !lockable {
            continue;
        }
        for &distance in &distances(defs, class) {
            for resource in [1.0, 0.15, 0.0] {
                for cooldowns in [Cooldowns::None, Cooldowns::Even, Cooldowns::Odd] {
                    for pillar in [false, true] {
                        if pillar && distance < 4.0 {
                            continue;
                        }
                        for buffed in [false, true] {
                            out.push(Point {
                                class,
                                distance,
                                resource,
                                cooldowns,
                                pillar,
                                buffed,
                                situation,
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

fn position(name: &str, p: &Point) -> Vec2 {
    match name {
        ME => ORIGIN,
        ALLY => ORIGIN + ALLY_BEARING * p.situation.ally_distance,
        TARGET => ORIGIN + Vec2::new(p.distance, 0.0),
        HEALER => ORIGIN + Vec2::new(p.distance + 5.0, 10.0),
        _ => unreachable!(),
    }
}

fn pillar(p: &Point) -> Option<ObstacleVolume> {
    p.pillar.then(|| ObstacleVolume::Cylinder {
        center_xz: ORIGIN + Vec2::new(p.distance / 2.0, 0.0),
        radius: (p.distance / 4.0).min(1.5),
        base_y: 0.0,
        height: 10.0,
    })
}

fn hold(scene: Scene, who: &'static str, hold: Hold) -> Scene {
    let polymorph = |caster: Entity| Aura {
        effect_type: AuraType::Polymorph,
        duration: 6.0,
        break_on_damage_threshold: 0.0,
        caster: Some(caster),
        ability_name: "Polymorph".into(),
        ..Default::default()
    };
    let immunity = |effect_type| Aura {
        effect_type,
        duration: 6.0,
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    match hold {
        Hold::Free => scene,
        Hold::DivineShield => scene.aura_full(who, immunity(AuraType::DamageImmunity)),
        Hold::Cyclone => scene.aura_full(who, immunity(AuraType::Cyclone)),
        // A friendly Polymorph is the ally's; an enemy one, the enemy healer's.
        Hold::FriendlyPolymorph => {
            let caster = scene.entity(ALLY);
            scene.aura_full(who, polymorph(caster))
        }
        Hold::EnemyPolymorph => {
            let caster = scene.entity(HEALER);
            scene.aura_full(who, polymorph(caster))
        }
    }
}

/// Whom the unit and its ally target: what `acquire_targets` leaves them on
/// when the decision runs. It runs first every frame and drops a dead or
/// immune target for the nearest valid enemy, so a dead, shielded or
/// cycloned target is never the decision's own; it stays in the scene as a
/// bystander nothing may be cast on.
fn whom(p: &Point) -> &'static str {
    let s = &p.situation;
    let valid =
        s.target_health.is_some() && !matches!(s.target_aura, Hold::DivineShield | Hold::Cyclone);
    if valid {
        TARGET
    } else {
        HEALER
    }
}

fn build(defs: &AbilityDefinitions, movement: MovementConfig, p: &Point) -> Scene {
    let s = &p.situation;
    let at = |name| position(name, p);
    let mut scene = Scene::with_config(defs.clone(), movement)
        .unit(ME, 1, p.class)
        .at(at(ME).x, at(ME).y)
        .health(s.my_health)
        .targeting(whom(p));
    scene = if p.class == CharacterClass::Warrior {
        scene.rage(100.0 * p.resource)
    } else {
        scene.mana(p.resource)
    };
    scene = scene
        .unit(ALLY, 1, CharacterClass::Warrior)
        .at(at(ALLY).x, at(ALLY).y)
        .health(s.ally_health.unwrap_or(0.0))
        .targeting(whom(p))
        .unit(TARGET, 2, CharacterClass::Warrior)
        .at(at(TARGET).x, at(TARGET).y)
        .health(s.target_health.unwrap_or(0.0))
        .targeting(ME)
        .unit(HEALER, 2, CharacterClass::Priest)
        .at(at(HEALER).x, at(HEALER).y);
    if s.opened {
        scene.combatant_mut(ME).stealthed = false;
    }
    if s.healer_casting {
        scene = scene.casting(HEALER, AbilityType::FlashHeal);
    }
    for (who, alive) in [(ALLY, s.ally_health), (TARGET, s.target_health)] {
        if alive.is_none() {
            scene.combatant_mut(who).is_dead = true;
        }
    }
    scene = hold(scene, ALLY, s.ally_aura);
    scene = hold(scene, TARGET, s.target_aura);
    scene = hold(scene, HEALER, s.healer_aura);
    scene = match s.my_aura {
        SelfLock::Free => scene,
        SelfLock::Silence => scene.aura(ME, AuraType::Silence, 4.0),
        SelfLock::SchoolLockout => scene.aura_full(
            ME,
            Aura {
                effect_type: AuraType::SpellSchoolLockout,
                duration: 4.0,
                magnitude: main_school(defs, p.class)
                    .expect("a lockout scene has a magic school to lock")
                    .to_lockout_magnitude(),
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        ),
        SelfLock::TravelForm => scene.aura_full(
            ME,
            Aura {
                effect_type: AuraType::TravelForm,
                duration: f32::MAX,
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        ),
    };
    if p.buffed {
        for aura in upkeep_auras(defs, p.class, scene.entity(ME)) {
            scene = scene.aura_full(ME, aura.clone()).aura_full(ALLY, aura);
        }
        // A Shaman's upkeep is its totems: one of each element down.
        if p.class == CharacterClass::Shaman {
            scene = scene.totems(ME, [60.0; 4]);
        }
    }
    for (i, ability) in cooling_kit(defs, p.class).iter().enumerate() {
        let cooling = match p.cooldowns {
            Cooldowns::None => false,
            Cooldowns::Even => i % 2 == 0,
            Cooldowns::Odd => i % 2 == 1,
        };
        if cooling {
            scene = scene.cooldown(ME, *ability, COOLDOWN_LEFT);
        }
    }
    if let Some(pillar) = pillar(p) {
        scene = scene.obstacles(vec![pillar]);
    }
    scene
}

// ---------------------------------------------------------------------------
// The records every property reads
// ---------------------------------------------------------------------------

/// What one scene held and what its decision chose.
struct Record {
    point: Point,
    view: DecisionView,
    /// The deciding unit's resource and cooldowns, as the scene set them.
    my_resource: f32,
    my_cooldowns: BTreeSet<AbilityType>,
}

struct Grid {
    defs: AbilityDefinitions,
    records: Vec<Record>,
    seconds: f32,
}

fn grid() -> &'static Grid {
    static GRID: OnceLock<Grid> = OnceLock::new();
    GRID.get_or_init(|| {
        let start = std::time::Instant::now();
        let defs = load_ability_definitions().expect("abilities.ron loads");
        let movement = load_movement_config().expect("movement.ron loads");
        let records = std::thread::scope(|scope| {
            let handles: Vec<_> = CharacterClass::all()
                .iter()
                .map(|&class| {
                    let defs = &defs;
                    scope.spawn(move || {
                        points(defs, class)
                            .into_iter()
                            .map(|point| {
                                let mut scene = build(defs, movement, &point);
                                let me = scene.combatant_mut(ME);
                                let my_resource = me.current_mana;
                                let my_cooldowns = me.ability_cooldowns.keys().copied().collect();
                                let view = scene.decide(ME);
                                Record {
                                    point,
                                    view,
                                    my_resource,
                                    my_cooldowns,
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("a class's grid panicked"))
                .collect::<Vec<_>>()
        });
        let seconds = start.elapsed().as_secs_f32();
        Grid {
            defs,
            records,
            seconds,
        }
    })
}

fn team(name: &str) -> u8 {
    match name {
        ME | ALLY => 1,
        _ => 2,
    }
}

fn hold_of(p: &Point, name: &str) -> Hold {
    match name {
        ALLY => p.situation.ally_aura,
        TARGET => p.situation.target_aura,
        HEALER => p.situation.healer_aura,
        _ => Hold::Free,
    }
}

fn alive(p: &Point, name: &str) -> bool {
    match name {
        ALLY => p.situation.ally_health.is_some(),
        TARGET => p.situation.target_health.is_some(),
        _ => true,
    }
}

fn pos3(name: &str, p: &Point) -> Vec3 {
    let xz = position(name, p);
    Vec3::new(xz.x, UNIT_Y, xz.y)
}

/// Every (scene, choice) pair the grid produced.
fn choices() -> impl Iterator<Item = (&'static Record, AbilityType, Option<&'static str>)> {
    grid().records.iter().flat_map(|r| {
        r.view
            .choices
            .iter()
            .map(move |(ability, target)| (r, *ability, *target))
    })
}

fn describe(r: &Record, ability: AbilityType, target: Option<&str>) -> String {
    format!(
        "{} -> chose {ability:?} on {}",
        r.point.describe(),
        target.unwrap_or("<none>")
    )
}

// ---------------------------------------------------------------------------
// Known defects
// ---------------------------------------------------------------------------

/// The properties, by name, so a known defect can name the one it breaks.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Property {
    InRange,
    InSight,
    NotImmune,
    FriendlyCcKept,
    NotSilenced,
}

/// A property that fails on `main` for these abilities, and the correctness
/// card that fixes it. The property's own test leaves exactly these
/// (property, ability) pairs out, so it still guards every other ability;
/// the card's `#[ignore]`d test checks exactly them, and passes once the card
/// lands — when this row and that test are deleted.
struct Known {
    card: &'static str,
    property: Property,
    abilities: &'static [AbilityType],
}

const KNOWN: &[Known] = &[
    // Rotation and dip Hammer of Justice gate range by hand and never ask
    // for sight.
    Known {
        card: "AS-279",
        property: Property::InSight,
        abilities: &[AbilityType::HammerOfJustice],
    },
    // The Death Coil peel horrifies a melee its own team has polymorphed.
    Known {
        card: "AS-280",
        property: Property::FriendlyCcKept,
        abilities: &[AbilityType::DeathCoil],
    },
    // Concussive Shot and the Frost Trap peel are aimed at a Divine-Shielded
    // melee, and trap throws land where the Hunter cannot see.
    Known {
        card: "AS-281",
        property: Property::NotImmune,
        abilities: &[AbilityType::ConcussiveShot, AbilityType::FrostTrap],
    },
    Known {
        card: "AS-281",
        property: Property::InSight,
        abilities: &[AbilityType::FrostTrap],
    },
    // Casts that skip the caster's own silence and school-lockout gates.
    Known {
        card: "AS-282",
        property: Property::NotSilenced,
        abilities: &[
            AbilityType::IceBarrier,
            AbilityType::FrostArmor,
            AbilityType::ConcussiveShot,
            AbilityType::Disengage,
            AbilityType::FreezingTrap,
            AbilityType::FrostTrap,
            AbilityType::AirTotem,
            AbilityType::EarthTotem,
        ],
    },
];

fn known_card(property: Property, ability: AbilityType) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|k| k.property == property && k.abilities.contains(&ability))
        .map(|k| k.card)
}

/// What a property found: how many choices it judged, and which broke it.
struct Finding {
    judged: usize,
    violations: Vec<(AbilityType, String)>,
}

/// Which of a property's findings a test checks.
#[derive(Clone, Copy)]
enum Scope {
    /// Everything except the known defects.
    Guard,
    /// Only the known defects one card carries.
    Card(&'static str),
}

/// Fail with every violation in `scope`, grouped by ability, the first scene
/// of each shown in full. Fails too if the property judged nothing — a
/// property that never meets its situation proves nothing.
fn verdict(property: Property, scope: Scope, finding: Finding) {
    let g = grid();
    eprintln!(
        "{property:?}: judged {} choices over {} scenes (grid built in {:.2}s)",
        finding.judged,
        g.records.len(),
        g.seconds
    );
    assert!(
        finding.judged > 0,
        "{property:?} judged nothing: the grid never reached it"
    );
    let in_scope: Vec<_> = finding
        .violations
        .into_iter()
        .filter(|(ability, _)| match scope {
            Scope::Guard => known_card(property, *ability).is_none(),
            Scope::Card(card) => known_card(property, *ability) == Some(card),
        })
        .collect();
    if in_scope.is_empty() {
        return;
    }
    let mut by_ability: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (ability, why) in &in_scope {
        by_ability
            .entry(format!("{ability:?}"))
            .or_default()
            .push(why.clone());
    }
    let mut msg = format!("{property:?}: {} violations\n", in_scope.len());
    for (ability, whys) in by_ability {
        msg += &format!("  {ability}: {} scenes, first: {}\n", whys.len(), whys[0]);
    }
    panic!("{msg}");
}

/// The grid is not vacuous: every class chooses something somewhere in it.
#[test]
fn the_grid_exercises_every_class() {
    let g = grid();
    let mut chosen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (r, ability, _) in choices() {
        chosen
            .entry(format!("{:?}", r.point.class))
            .or_default()
            .insert(format!("{ability:?}"));
    }
    for class in CharacterClass::all() {
        let kit = kit(&g.defs, *class);
        let reached = chosen.get(&format!("{class:?}")).map_or(0, BTreeSet::len);
        eprintln!(
            "{class:?}: {} scenes, {reached}/{} of its kit chosen: {:?}",
            g.records.iter().filter(|r| r.point.class == *class).count(),
            kit.len(),
            chosen.get(&format!("{class:?}"))
        );
        assert!(reached > 0, "{class:?} chose nothing anywhere in the grid");
    }
}

/// Every row of [`KNOWN`] is a live failure: each (property, ability) pair it
/// leaves out of a guard really does break that property somewhere in the
/// grid. So the table cannot leave out more than the defects it names, and a
/// card that fixes one fails here until its row is deleted.
#[test]
fn every_known_defect_is_live() {
    let live: BTreeSet<(String, String)> = [
        (Property::InRange, in_range()),
        (Property::InSight, in_sight()),
        (Property::NotImmune, not_immune()),
        (Property::FriendlyCcKept, friendly_cc_kept()),
        (Property::NotSilenced, not_silenced()),
    ]
    .into_iter()
    .flat_map(|(p, f)| {
        f.violations
            .into_iter()
            .map(move |(a, _)| (format!("{p:?}"), format!("{a:?}")))
    })
    .collect();
    let stale: Vec<String> = KNOWN
        .iter()
        .flat_map(|k| {
            k.abilities
                .iter()
                .map(move |a| (k.card, format!("{:?}", k.property), format!("{a:?}")))
        })
        .filter(|(_, p, a)| !live.contains(&(p.clone(), a.clone())))
        .map(|(card, p, a)| format!("{card}: {p} {a}"))
        .collect();
    assert!(
        stale.is_empty(),
        "known defects the grid no longer finds (fixed? delete the row and its ignored test): {stale:?}"
    );
}

// ---------------------------------------------------------------------------
// The properties
// ---------------------------------------------------------------------------

/// An ability with no range is centred on its caster (Disengage, Frost Nova,
/// a totem): a target its trace names is what the decision AIMED it at, not
/// where it is cast, so it has no range or sight to its target to keep.
fn self_centred(def: &arenasim::states::play_match::ability_config::AbilityConfig) -> bool {
    def.range == 0.0
}

/// Where a chosen ability is CAST: its target's position — or, for a trap,
/// the point it lands on, which is where the throw has to reach and see. (A
/// trap's traced target is the enemy it was aimed at.) `None` for a choice
/// with no point to reach: self-cast, self-centred, or a trap the decision
/// did not actually put down.
fn cast_point(
    r: &Record,
    ability: AbilityType,
    target: Option<&str>,
    def: &arenasim::states::play_match::ability_config::AbilityConfig,
) -> Option<Vec3> {
    let trap = match ability {
        AbilityType::FreezingTrap => Some(TrapType::Freezing),
        AbilityType::FrostTrap => Some(TrapType::Frost),
        _ => None,
    };
    if let Some(trap) = trap {
        return r
            .view
            .trap_landings
            .iter()
            .find(|(t, _)| *t == trap)
            .map(|(_, at)| Vec3::new(at.x, UNIT_Y, at.z));
    }
    if self_centred(def) {
        return None;
    }
    target.filter(|t| *t != ME).map(|t| pos3(t, &r.point))
}

/// A chosen ability is the deciding unit's own (its class's, not a pet's).
#[test]
fn chooses_only_its_own_abilities() {
    let g = grid();
    let mut judged = 0;
    let mut v = Vec::new();
    for (r, ability, target) in choices() {
        judged += 1;
        let def = g.defs.get(&ability).unwrap();
        if def.class != r.point.class || def.pet.is_some() {
            v.push((ability, describe(r, ability, target)));
        }
    }
    assert!(judged > 0);
    assert!(v.is_empty(), "abilities of another class or a pet: {v:?}");
}

fn in_range() -> Finding {
    let g = grid();
    let mut judged = 0;
    let mut violations = Vec::new();
    for (r, ability, target) in choices() {
        let def = g.defs.get(&ability).unwrap();
        let Some(at) = cast_point(r, ability, target, def) else {
            continue;
        };
        judged += 1;
        let d = pos3(ME, &r.point).distance(at);
        let short = def.min_range.is_some_and(|min| d < min);
        if d > def.range || short {
            violations.push((
                ability,
                format!(
                    "{} (distance {d:.2}, range {}, min {:?})",
                    describe(r, ability, target),
                    def.range,
                    def.min_range
                ),
            ));
        }
    }
    Finding { judged, violations }
}

/// A chosen ability is cast within its range and outside its minimum range
/// (`can_cast_config`'s edges, read from `abilities.ron`).
#[test]
fn a_targeted_choice_is_in_range() {
    verdict(Property::InRange, Scope::Guard, in_range());
}

fn in_sight() -> Finding {
    let g = grid();
    let mut judged = 0;
    let mut violations = Vec::new();
    for (r, ability, target) in choices() {
        let def = g.defs.get(&ability).unwrap();
        let Some(at) = cast_point(r, ability, target, def) else {
            continue;
        };
        let obstacles: Vec<_> = pillar(&r.point).into_iter().collect();
        if obstacles.is_empty() {
            continue;
        }
        judged += 1;
        if !has_line_of_sight(&obstacles, pos3(ME, &r.point), at) {
            violations.push((ability, describe(r, ability, target)));
        }
    }
    Finding { judged, violations }
}

/// A chosen ability is cast where its caster can see: its target, or the
/// spot a trap lands on.
#[test]
fn a_targeted_choice_is_in_sight() {
    verdict(Property::InSight, Scope::Guard, in_sight());
}

#[test]
#[ignore = "AS-279"]
fn hammer_of_justice_is_cast_in_sight() {
    verdict(Property::InSight, Scope::Card("AS-279"), in_sight());
}

#[test]
#[ignore = "AS-281"]
fn traps_are_thrown_in_sight_of_their_landing() {
    verdict(Property::InSight, Scope::Card("AS-281"), in_sight());
}

/// A chosen ability is off cooldown.
#[test]
fn never_chooses_an_ability_on_cooldown() {
    let mut judged = 0;
    let mut v = Vec::new();
    for (r, ability, target) in choices() {
        if r.my_cooldowns.is_empty() {
            continue;
        }
        judged += 1;
        if r.my_cooldowns.contains(&ability) {
            v.push((ability, describe(r, ability, target)));
        }
    }
    assert!(judged > 0);
    assert!(v.is_empty(), "chosen while cooling down: {v:#?}");
}

/// A chosen ability is affordable.
#[test]
fn never_chooses_an_ability_it_cannot_afford() {
    let g = grid();
    let mut judged = 0;
    let mut v = Vec::new();
    for (r, ability, target) in choices() {
        let cost = g.defs.get(&ability).unwrap().mana_cost;
        if cost <= 0.0 {
            continue;
        }
        judged += 1;
        if r.my_resource < cost {
            v.push(format!(
                "{} (has {}, costs {cost})",
                describe(r, ability, target),
                r.my_resource
            ));
        }
    }
    assert!(judged > 0);
    assert!(v.is_empty(), "chosen without the resource: {v:#?}");
}

fn not_immune() -> Finding {
    let mut judged = 0;
    let mut violations = Vec::new();
    for (r, ability, target) in choices() {
        let Some(t) = target.filter(|t| *t != ME) else {
            continue;
        };
        judged += 1;
        let held = hold_of(&r.point, t);
        let hostile = team(t) != 1;
        if held == Hold::Cyclone || (hostile && held == Hold::DivineShield) {
            violations.push((ability, describe(r, ability, target)));
        }
    }
    Finding { judged, violations }
}

/// Nothing is cast on an enemy that is immune — Divine Shield or Cyclone —
/// and nothing at all, friendly or hostile, on a cycloned unit.
#[test]
fn never_targets_an_immune_or_untargetable_unit() {
    verdict(Property::NotImmune, Scope::Guard, not_immune());
}

#[test]
#[ignore = "AS-281"]
fn hunter_peels_skip_a_divine_shielded_melee() {
    verdict(Property::NotImmune, Scope::Card("AS-281"), not_immune());
}

/// Nothing is cast on a dead unit — no heal, no buff, no damage.
#[test]
fn never_targets_a_dead_unit() {
    let mut judged = 0;
    let mut v = Vec::new();
    for (r, ability, target) in choices() {
        let Some(t) = target else {
            continue;
        };
        judged += 1;
        if !alive(&r.point, t) {
            v.push(describe(r, ability, target));
        }
    }
    assert!(judged > 0);
    assert!(v.is_empty(), "cast on a dead unit: {v:#?}");
}

/// A heal lands on the deciding unit's own side, and damage on the other.
/// Judged only for abilities that are purely one or the other: an ability
/// with both a heal and a damage component (Holy Shock) may go either way.
#[test]
fn heals_go_to_allies_and_damage_to_enemies() {
    let g = grid();
    let mut judged = 0;
    let mut v = Vec::new();
    for (r, ability, target) in choices() {
        let Some(t) = target else {
            continue;
        };
        let def = g.defs.get(&ability).unwrap();
        let heals = def.healing_base_max > 0.0;
        let damages = def.damage_base_max > 0.0;
        if heals == damages {
            continue;
        }
        judged += 1;
        let friendly_target = team(t) == 1;
        if heals != friendly_target {
            v.push(describe(r, ability, target));
        }
    }
    assert!(judged > 0);
    assert!(
        v.is_empty(),
        "a heal on an enemy or damage on an ally: {v:#?}"
    );
}

fn friendly_cc_kept() -> Finding {
    let g = grid();
    let mut judged = 0;
    let mut violations = Vec::new();
    for (r, ability, target) in choices() {
        let Some(t) = target.filter(|t| team(t) == 2) else {
            continue;
        };
        let def = g.defs.get(&ability).unwrap();
        let damages = def.damage_base_max > 0.0
            || def
                .applies_aura
                .as_ref()
                .is_some_and(|a| a.aura_type == AuraType::DamageOverTime);
        if !damages {
            continue;
        }
        judged += 1;
        if hold_of(&r.point, t) == Hold::FriendlyPolymorph {
            violations.push((ability, describe(r, ability, target)));
        }
    }
    Finding { judged, violations }
}

/// Damage — direct or over time — is never aimed at an enemy held in a
/// crowd control of the deciding unit's own team that breaks on any damage.
#[test]
fn never_breaks_a_friendly_breakable_cc() {
    verdict(Property::FriendlyCcKept, Scope::Guard, friendly_cc_kept());
}

#[test]
#[ignore = "AS-280"]
fn death_coil_keeps_a_friendly_polymorph() {
    verdict(
        Property::FriendlyCcKept,
        Scope::Card("AS-280"),
        friendly_cc_kept(),
    );
}

fn not_silenced() -> Finding {
    let g = grid();
    let mut judged = 0;
    let mut violations = Vec::new();
    for (r, ability, target) in choices() {
        let def = g.defs.get(&ability).unwrap();
        let mana_user = !matches!(
            r.point.class,
            CharacterClass::Warrior | CharacterClass::Rogue
        );
        let blocked = match r.point.situation.my_aura {
            SelfLock::Free => continue,
            SelfLock::Silence => {
                mana_user && def.mana_cost > 0.0 && ability != AbilityType::DivineShield
            }
            SelfLock::SchoolLockout => {
                Some(def.spell_school) == main_school(&g.defs, r.point.class)
            }
            SelfLock::TravelForm => true,
        };
        judged += 1;
        if blocked {
            violations.push((ability, describe(r, ability, target)));
        }
    }
    Finding { judged, violations }
}

/// A silenced caster casts nothing that costs mana (`is_silenced`: mana
/// users only; Divine Shield is the one ability that bypasses it), a
/// locked-out school casts nothing of that school, and a Druid in Travel
/// Form casts nothing — the shift out is not a cast.
#[test]
fn never_casts_through_a_silence_lockout_or_shapeshift() {
    verdict(Property::NotSilenced, Scope::Guard, not_silenced());
}

#[test]
#[ignore = "AS-282"]
fn own_state_gates_hold_for_every_cast() {
    verdict(Property::NotSilenced, Scope::Card("AS-282"), not_silenced());
}

/// The trace never contradicts the choice: an ability the decision rejected
/// for a reason that is the CASTER's alone — on cooldown, short of resource,
/// silenced or locked out — is not then chosen in the same decision, whatever
/// the target. (A rejection for a target's reason — range, sight, immunity —
/// may be followed by the same ability on another target.)
#[test]
fn never_chooses_an_ability_its_own_trace_rejected_for_a_caster_reason() {
    let mut judged = 0;
    let mut v = Vec::new();
    for r in &grid().records {
        for c in &r.view.candidates {
            if c.status != CandidateStatus::Rejected {
                continue;
            }
            let caster_reason = matches!(
                c.reason,
                Some(
                    RejectionReason::OnCooldown { .. }
                        | RejectionReason::InsufficientMana { .. }
                        | RejectionReason::InsufficientResource { .. }
                        | RejectionReason::SilencedOrLocked { .. }
                )
            );
            if !caster_reason {
                continue;
            }
            judged += 1;
            if let Some((_, target)) = r.view.choices.iter().find(|(a, _)| *a == c.ability) {
                v.push(format!(
                    "{} (also rejected: {:?})",
                    describe(r, c.ability, *target),
                    c.reason
                ));
            }
        }
    }
    assert!(judged > 0);
    assert!(v.is_empty(), "chosen after a caster-side rejection: {v:#?}");
}
