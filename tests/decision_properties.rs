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
//! - **upkeep**: nothing up; the kit's own buffs (and a Shaman's totems)
//!   up; or those plus its shields and heals over time, and its own damage
//!   over time on its target (and its curse on every enemy) — so the
//!   decision reaches past "refresh the buff" and "apply the DoT".
//!
//! Over a handful of situations the grid also walks each class's per-match
//! CHOICES ([`kit_choices`]): the shout, the armor, the opener, the Paladin
//! aura and the curse a match config can give it, and for the Shaman which
//! totem is still to drop.
//!
//! The situations vary health, death, auras, an enemy cast, the classes of
//! the ally and the target, whom the enemy attacks, a focus on the enemy
//! healer and the unit's own cast locks — one condition at a time, with the
//! combinations a property needs to be reachable (a heal is only cast on an
//! injured ally, so "ally cycloned" is also "ally injured") — and put the
//! unit and its ally just either side of every healer's guaranteed-heal
//! bar. The Rogue meets each in and out of stealth. Every situation is under
//! `Legacy` but three, which put the unit's team under `TeamPlan` with its
//! ally dying under attack — the Peel play's situation — so every property
//! also judges the gated plays' choices
//! ([`every_peel_tool_is_reached_and_every_peel_is_aimed_at_the_attacker`]). The roster is the unit,
//! an ally, the enemy target and an enemy Priest. The unit's CC target is the
//! one `acquire_targets` would leave it ([`cc_target`]). About 160,000
//! scenes, built in one to two seconds in the test profile.
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
//! Three kinds of decision are made outside `dispatch_class_decision`, so no
//! scene reaches them, and [`UNREACHED`] names each:
//! - the interrupts (Pummel, Kick, Wind Shear) are decided by
//!   `check_interrupts`;
//! - a weapon poison is applied by the auto-attack;
//! - a cooldown used from inside crowd control (Berserker Rage, Divine
//!   Shield) is decided by `decide_abilities`' CC-break branch.
//!
//! The same branch is where incapacitation (stun, fear, polymorph,
//! incapacitate, cyclone) is gated, BEFORE the class dispatch, so a
//! decision-level scene cannot test "never casts while incapacitated" —
//! `Scene::decide` refuses, as the frame loop does; that belongs to a staged
//! match. What IS reachable from inside the dispatch, the non-incapacitating
//! cast locks (Silence, a school lockout, Travel Form), is
//! [`never_casts_through_a_silence_lockout_or_shapeshift`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use bevy::prelude::{Entity, Vec2, Vec3};

use arenasim::states::match_config::{
    CharacterClass, MageArmor, PaladinAura, RogueOpener, WarlockCurse, WarriorShout,
};
use arenasim::states::play_match::ability_config::{load_ability_definitions, AbilityDefinitions};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::TrapType;
use arenasim::states::play_match::components::{DRCategory, DRTracker, DispelType};
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
    /// The ally's class: a Warrior, or a mana user.
    ally_class: CharacterClass,
    /// The ally carries none of the unit's upkeep (it has just come in).
    ally_unbuffed: bool,
    /// `None`: the target is dead.
    target_health: Option<f32>,
    target_aura: Hold,
    /// The target carries a purgeable buff (a Power Word: Shield).
    target_shielded: bool,
    /// The target's class: a Warrior (a melee, and a rage user), or a Mage.
    target_class: CharacterClass,
    /// The target attacks the ally instead of the unit.
    target_attacks_ally: bool,
    /// The unit (and its ally) focus the enemy healer — a kill target set on
    /// it — while the enemy melee is loose.
    focus_healer: bool,
    healer_aura: Hold,
    my_aura: SelfLock,
    /// The enemy healer is casting a heal (an interrupt's moment).
    healer_casting: bool,
    /// The target is mid this cast.
    target_casting: Option<AbilityType>,
    /// Both enemies are immune to Fear by diminishing returns.
    fear_diminished: bool,
    /// The unit has broken stealth (the Rogue starts every match in it).
    opened: bool,
    /// The unit's team runs `TeamPlan` — the profile the gated plays (the
    /// Peel play) run under. `Legacy` otherwise.
    team_plan: bool,
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
    ally_class: CharacterClass::Warrior,
    ally_unbuffed: false,
    target_health: Some(1.0),
    target_aura: Hold::Free,
    target_shielded: false,
    target_class: CharacterClass::Warrior,
    target_attacks_ally: false,
    focus_healer: false,
    healer_aura: Hold::Free,
    my_aura: SelfLock::Free,
    healer_casting: false,
    target_casting: None,
    fear_diminished: false,
    opened: false,
    team_plan: false,
};

/// Each healer's guaranteed-heal bar ([`heal_guarantee`]): the grid puts the
/// ally and the unit itself just either side of every one.
fn heal_bars(movement: &MovementConfig) -> Vec<f32> {
    let mut bars: Vec<f32> = [
        CharacterClass::Priest,
        CharacterClass::Paladin,
        CharacterClass::Shaman,
        CharacterClass::Druid,
    ]
    .into_iter()
    .filter_map(|c| heal_guarantee(c, movement).map(|g| g.bar))
    .collect();
    bars.sort_by(f32::total_cmp);
    bars.dedup();
    bars
}

fn situations(class: CharacterClass, bars: &[f32]) -> Vec<Situation> {
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
            name: "enemy healer polymorphed by my team, me injured",
            healer_aura: Hold::FriendlyPolymorph,
            my_health: 0.3,
            ..CALM
        },
        Situation {
            name: "enemy healer divine shield",
            healer_aura: Hold::DivineShield,
            ..CALM
        },
        Situation {
            name: "enemies immune to fear by diminishing returns, me injured",
            fear_diminished: true,
            my_health: 0.3,
            ..CALM
        },
        Situation {
            name: "enemy healer divine shield, me injured",
            healer_aura: Hold::DivineShield,
            my_health: 0.3,
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
    let caster_ally = Situation {
        name: "a mana-using ally",
        ally_class: CharacterClass::Priest,
        ally_unbuffed: true,
        ..CALM
    };
    let caster_target = Situation {
        name: "a caster target",
        target_class: CharacterClass::Mage,
        ..CALM
    };
    list.push(caster_target);
    list.push(Situation {
        name: "a caster target, me injured",
        my_health: 0.3,
        ..caster_target
    });
    // The target mid a cast: one no interrupt takes (Aimed Shot, the book's
    // uninterruptible cast, on a caster target so a Kick would otherwise be
    // owed), and its pair, an interruptible one.
    list.push(Situation {
        name: "a caster target mid an uninterruptible cast",
        target_casting: Some(AbilityType::AimedShot),
        ..caster_target
    });
    list.push(Situation {
        name: "a caster target mid an interruptible cast",
        target_casting: Some(AbilityType::Frostbolt),
        ..caster_target
    });
    list.push(Situation {
        name: "ally dying under attack",
        ally_health: Some(0.3),
        target_attacks_ally: true,
        ..CALM
    });
    list.push(Situation {
        name: "focusing the healer, me dying",
        my_health: 0.3,
        focus_healer: true,
        ..CALM
    });
    list.push(Situation {
        name: "focusing the healer, ally dying under attack",
        ally_health: Some(0.3),
        target_attacks_ally: true,
        focus_healer: true,
        ..CALM
    });
    // Under TeamPlan the ally in danger calls the Peel play: the unit's peel
    // step heads its chain, on the enemy target attacking the ally.
    for (name, target_class, focus_healer) in [
        (
            "ally dying under attack, TeamPlan",
            CharacterClass::Warrior,
            false,
        ),
        (
            "focusing the healer, ally dying under attack, TeamPlan",
            CharacterClass::Warrior,
            true,
        ),
        (
            "ally dying under a caster's attack, TeamPlan",
            CharacterClass::Mage,
            false,
        ),
    ] {
        list.push(Situation {
            name,
            ally_health: Some(0.3),
            target_attacks_ally: true,
            target_class,
            focus_healer,
            team_plan: true,
            ..CALM
        });
    }
    list.push(caster_ally);
    list.push(Situation {
        name: "a mana-using ally, injured",
        ally_health: Some(0.4),
        ..caster_ally
    });
    list.push(Situation {
        name: "target shielded",
        target_shielded: true,
        ..CALM
    });
    for &bar in bars {
        for hp in [bar - 0.02, bar + 0.02] {
            list.push(Situation {
                name: "ally at a healer's bar",
                ally_health: Some(hp),
                ..CALM
            });
            list.push(Situation {
                name: "me at a healer's bar",
                my_health: hp,
                ..CALM
            });
        }
    }
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
    upkeep: Upkeep,
    /// The class's per-match choice ([`kit_choices`]): 0 is the default.
    choice: u8,
    situation: Situation,
}

/// How much of the class's own upkeep is already done when it decides.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Upkeep {
    /// Nothing up: the first priorities (buff, armor, totems) decide.
    None,
    /// Its buffs and armors up on itself and its ally (and a Shaman's
    /// totems down), but no shield or heal over time and no damage over time
    /// on its target.
    Buffs,
    /// Everything: buffs, shields and heals over time on itself and its
    /// ally, and its own damage over time on its target.
    Full,
}

impl Point {
    fn describe(&self) -> String {
        format!(
            "{:?}: target at {}yd, resource {}, cooldowns {:?}, {}, upkeep {:?}, \
             choice {}, situation \"{}\" (me at {}, ally {})",
            self.class,
            self.distance,
            self.resource,
            self.cooldowns,
            if self.pillar {
                "pillar between"
            } else {
                "clear sight"
            },
            self.upkeep,
            self.choice,
            self.situation.name,
            self.situation.my_health,
            match self.situation.ally_health {
                Some(hp) => format!("a {:?} at {hp}", self.situation.ally_class),
                None => "dead".to_string(),
            },
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
/// absorbs and heals over time — which the `upkeep` axis puts up in advance
/// so the decision reaches past "refresh the buff" into the rest of the
/// priority chain. Not a shapeshift or an immunity: those are states, not
/// upkeep.
fn upkeep_auras(
    defs: &AbilityDefinitions,
    class: CharacterClass,
    caster: Entity,
    upkeep: Upkeep,
) -> Vec<Aura> {
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
                )
                && (upkeep == Upkeep::Full
                    || !matches!(
                        effect.aura_type,
                        AuraType::Absorb | AuraType::HealingOverTime
                    ));
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

/// The damage over time the class's kit applies, as it stands on a target.
fn own_dots(
    defs: &AbilityDefinitions,
    class: CharacterClass,
    caster: Entity,
    curse: AbilityType,
) -> Vec<Aura> {
    kit(defs, class)
        .into_iter()
        .filter_map(|a| {
            let d = defs.get(&a).unwrap();
            let effect = d.applies_aura.as_ref()?;
            // Of the curses, only the one the loadout prefers is ever placed.
            let curse_of_another = effect.dispel_type == DispelType::Curse && a != curse;
            (effect.aura_type == AuraType::DamageOverTime && !curse_of_another).then(|| Aura {
                effect_type: AuraType::DamageOverTime,
                duration: 12.0,
                magnitude: effect.magnitude,
                tick_interval: 3.0,
                time_until_next_tick: 3.0,
                break_on_damage_threshold: -1.0,
                caster: Some(caster),
                ability_name: d.name.clone(),
                spell_school: Some(d.spell_school),
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

/// How many per-match choices the class has: its loadout alternatives (the
/// shout, the armor, the opener, the aura, the curse — each a match-config
/// choice, set on `Combatant` at spawn), or for the Shaman, which totem is
/// left to drop under part upkeep. Choice 0 is the default.
fn kit_choices(class: CharacterClass) -> u8 {
    match class {
        CharacterClass::Warrior
        | CharacterClass::Mage
        | CharacterClass::Warlock
        | CharacterClass::Paladin => 3,
        CharacterClass::Rogue | CharacterClass::Shaman => 2,
        CharacterClass::Priest | CharacterClass::Hunter | CharacterClass::Druid => 1,
    }
}

/// The situations the non-default choices meet (the default meets every
/// one), so the grid stays near a second: enough to reach every alternative
/// both calm and under pressure.
const CHOICE_SITUATIONS: &[&str] = &[
    "calm",
    "target polymorphed by my team",
    "target low, divine shield",
    "target dead",
    "ally injured",
    "me injured",
    "target low",
    "a caster target",
    "a caster target, me injured",
    "enemy healer polymorphed by my team",
    "silenced, ally injured",
];

fn points(
    defs: &AbilityDefinitions,
    movement: &MovementConfig,
    class: CharacterClass,
) -> Vec<Point> {
    let mut out = Vec::new();
    let lockable = main_school(defs, class).is_some();
    for situation in situations(class, &heal_bars(movement)) {
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
                        for upkeep in [Upkeep::None, Upkeep::Buffs, Upkeep::Full] {
                            let alternatives = if CHOICE_SITUATIONS.contains(&situation.name) {
                                kit_choices(class)
                            } else {
                                1
                            };
                            for choice in 0..alternatives {
                                out.push(Point {
                                    class,
                                    distance,
                                    resource,
                                    cooldowns,
                                    pillar,
                                    upkeep,
                                    choice,
                                    situation,
                                });
                            }
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

/// The unit's CC target, as `acquire_targets` leaves it.
///
/// One rule is the grid's own, and it is `acquire_targets`' validity check
/// applied to a history the scene cannot hold: a CC target STAYS while it is
/// alive, visible and not immune, so an enemy healer the unit picked before
/// its ally polymorphed it is still its CC target (the "enemy healer
/// polymorphed by my team" situations).
///
/// Otherwise the target is re-picked by production's own
/// `select_cc_target_heuristic`, from the same enemy rows, aura map and
/// visibility rule `acquire_targets` builds.
fn cc_target(scene: &mut Scene, p: &Point) -> Option<Entity> {
    use arenasim::states::play_match::class_ai::{grants_damage_immunity, stealth_visible};
    use arenasim::states::play_match::combat_ai::select_cc_target_heuristic;
    if p.situation.healer_aura == Hold::FriendlyPolymorph {
        return Some(scene.entity(HEALER));
    }
    let mut auras: std::collections::HashMap<Entity, Vec<Aura>> = Default::default();
    let mut enemies = Vec::new();
    for who in [TARGET, HEALER] {
        let entity = scene.entity(who);
        let on_it = scene.auras_of(who).to_vec();
        let immune = on_it.iter().any(|a| grants_damage_immunity(a.effect_type));
        auras.insert(entity, on_it);
        let c = scene.combatant_mut(who);
        if !c.is_alive() {
            continue;
        }
        enemies.push((
            entity,
            pos3(who, p),
            c.stealthed,
            false,
            c.class,
            c.current_health,
            immune,
            false,
        ));
    }
    // Nobody in the grid holds Shadow Sight.
    let can_see =
        |stealthed: bool, enemy_has_sight: bool| stealth_visible(stealthed, false, enemy_has_sight);
    let kill_target = Some(scene.entity(whom(p)));
    select_cc_target_heuristic(&enemies, kill_target, &auras, &can_see)
}

/// Whom the enemy target attacks.
fn attacked(p: &Point) -> &'static str {
    if p.situation.target_attacks_ally {
        ALLY
    } else {
        ME
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
    if valid && !s.focus_healer {
        TARGET
    } else {
        HEALER
    }
}

fn build(defs: &AbilityDefinitions, movement: MovementConfig, p: &Point) -> Scene {
    let s = &p.situation;
    let at = |name| position(name, p);
    let profile = if s.team_plan {
        AiProfile::TeamPlan
    } else {
        AiProfile::Legacy
    };
    let mut scene = Scene::with_config(defs.clone(), movement)
        .ai_profile(1, profile)
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
        .unit(ALLY, 1, s.ally_class)
        .at(at(ALLY).x, at(ALLY).y)
        .health(s.ally_health.unwrap_or(0.0))
        .targeting(whom(p))
        .unit(TARGET, 2, s.target_class)
        .at(at(TARGET).x, at(TARGET).y)
        .health(s.target_health.unwrap_or(0.0))
        .targeting(attacked(p))
        .unit(HEALER, 2, CharacterClass::Priest)
        .at(at(HEALER).x, at(HEALER).y);
    if s.opened {
        scene.combatant_mut(ME).stealthed = false;
    }
    let healer = scene.entity(HEALER);
    // The class's per-match choice. A Warlock whose config names no curses
    // gets the default curse on each enemy (`parse_warlock_curse_prefs`).
    {
        let me = scene.combatant_mut(ME);
        let k = p.choice as usize;
        me.warrior_shout = [
            WarriorShout::BattleShout,
            WarriorShout::DemoralizingShout,
            WarriorShout::CommandingShout,
        ][k % 3];
        me.mage_armor = [
            MageArmor::FrostArmor,
            MageArmor::MageArmor,
            MageArmor::MoltenArmor,
        ][k % 3];
        me.paladin_aura = [
            PaladinAura::DevotionAura,
            PaladinAura::ShadowResistanceAura,
            PaladinAura::ConcentrationAura,
        ][k % 3];
        me.rogue_opener = [RogueOpener::CheapShot, RogueOpener::Ambush][k % 2];
        let curse = [
            WarlockCurse::Agony,
            WarlockCurse::Weakness,
            WarlockCurse::Tongues,
        ][k % 3];
        me.warlock_curse_prefs = vec![curse; 2];
    }
    if s.target_shielded {
        scene = scene.aura_full(
            TARGET,
            Aura {
                effect_type: AuraType::Absorb,
                duration: 20.0,
                magnitude: 50.0,
                break_on_damage_threshold: -1.0,
                caster: Some(healer),
                ability_name: "Power Word: Shield".into(),
                spell_school: Some(SpellSchool::Holy),
                ..Default::default()
            },
        );
    }
    if s.fear_diminished {
        let mut tracker = DRTracker::default();
        while !tracker.is_immune(DRCategory::Fears) {
            tracker.apply(DRCategory::Fears);
        }
        scene = scene
            .dr_tracker(TARGET, tracker.clone())
            .dr_tracker(HEALER, tracker);
    }
    if s.healer_casting {
        scene = scene.casting(HEALER, AbilityType::FlashHeal);
    }
    if let Some(cast) = s.target_casting {
        scene = scene.casting(TARGET, cast);
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
    if p.upkeep != Upkeep::None {
        for aura in upkeep_auras(defs, p.class, scene.entity(ME), p.upkeep) {
            scene = scene.aura_full(ME, aura.clone());
            if !s.ally_unbuffed {
                scene = scene.aura_full(ALLY, aura);
            }
        }
        // ...and the unit's own damage over time already on its target
        // (never on one its team holds in a Polymorph, which it would break).
        if p.upkeep == Upkeep::Full && hold_of(p, whom(p)) != Hold::FriendlyPolymorph {
            let curse = scene.combatant_mut(ME).warlock_curse_prefs[0].ability();
            for aura in own_dots(defs, p.class, scene.entity(ME), curse) {
                scene = scene.aura_full(whom(p), aura);
            }
        }
        // ...and its curse on every enemy it spreads to.
        for enemy in [TARGET, HEALER] {
            let cursed = enemy == whom(p) || hold_of(p, enemy) == Hold::FriendlyPolymorph;
            if p.upkeep == Upkeep::Full && !cursed && alive(p, enemy) {
                let curse = scene.combatant_mut(ME).warlock_curse_prefs[0].ability();
                for aura in own_dots(defs, p.class, scene.entity(ME), curse) {
                    if aura.ability_name == defs.get(&curse).unwrap().name {
                        scene = scene.aura_full(enemy, aura);
                    }
                }
            }
        }
        // A Shaman's upkeep is its totems (Air, Water, Earth, Fire): all four
        // down in full upkeep; in part, Water and Fire still to drop (choice
        // 0) or only Fire (choice 1).
        if p.class == CharacterClass::Shaman {
            let totems = match (p.upkeep, p.choice) {
                (Upkeep::Full, _) => [60.0; 4],
                (_, 0) => [60.0, 0.0, 60.0, 0.0],
                _ => [60.0, 60.0, 60.0, 0.0],
            };
            scene = scene.totems(ME, totems);
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
    // Last, once every aura is on: the CC target reads them.
    let cc = cc_target(&mut scene, p);
    scene.combatant_mut(ME).cc_target = cc;
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
                        points(defs, &movement, class)
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
    CcNotStacked,
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

/// Why an ability of a class's own kit is never chosen anywhere in the grid.
/// Every kit ability is either chosen somewhere or named in [`UNREACHED`]
/// with one of these, so an ability the grid stops reaching — or a new one
/// it never reached — fails [`every_kit_ability_is_chosen_or_named`] until
/// someone says why.
#[derive(Clone, Copy, Debug)]
enum Unreached {
    /// Decided outside `dispatch_class_decision`, at the named site, so no
    /// scene can choose it: the interrupts (`check_interrupts`), a weapon
    /// poison (the auto-attack), a cooldown used from inside crowd control
    /// (`decide_abilities`' CC-break branch).
    DecidedOutside(&'static str),
    /// Reachable, but only from a state the grid has no knob for (named).
    NeedsKnob(&'static str),
    /// Never chosen from ANY state: an earlier priority that is always
    /// castable when this one is wins first (named).
    Shadowed(&'static str),
}

impl Unreached {
    fn why(&self) -> String {
        match self {
            Unreached::DecidedOutside(site) => format!("decided by {site}, outside the dispatch"),
            Unreached::NeedsKnob(knob) => format!("needs a knob: {knob}"),
            Unreached::Shadowed(by) => format!("shadowed by {by}"),
        }
    }
}

const UNREACHED: &[(AbilityType, Unreached)] = &[
    (
        AbilityType::Pummel,
        Unreached::DecidedOutside("check_interrupts"),
    ),
    (
        AbilityType::Kick,
        Unreached::DecidedOutside("check_interrupts"),
    ),
    (
        AbilityType::WindShear,
        Unreached::DecidedOutside("check_interrupts"),
    ),
    (
        AbilityType::CripplingPoison,
        Unreached::DecidedOutside("the auto-attack's weapon poison"),
    ),
    (
        AbilityType::BerserkerRage,
        Unreached::DecidedOutside("decide_abilities' CC-break branch, while feared"),
    ),
    (
        AbilityType::Flare,
        Unreached::NeedsKnob("an enemy in stealth the Hunter cannot see"),
    ),
    (
        AbilityType::Lifebloom,
        Unreached::NeedsKnob(
            "the Druid's Rejuvenation up and no Lifebloom on a dying ally under attack",
        ),
    ),
    (
        AbilityType::HolyLight,
        Unreached::Shadowed(
            "Flash of Light: same range and school, cheaper, no cooldown, wider trigger (AS-286)",
        ),
    ),
];

/// Every ability of every class's own kit is chosen somewhere in the grid, or
/// is named in [`UNREACHED`] with its reason — set equality, both ways.
#[test]
fn every_kit_ability_is_chosen_or_named() {
    let g = grid();
    let chosen: BTreeSet<(String, String)> = choices()
        .map(|(r, ability, _)| (format!("{:?}", r.point.class), format!("{ability:?}")))
        .collect();
    let mut unreached = BTreeSet::new();
    for class in CharacterClass::all() {
        let kit = kit(&g.defs, *class);
        let mut missing = Vec::new();
        for a in &kit {
            if !chosen.contains(&(format!("{class:?}"), format!("{a:?}"))) {
                missing.push(*a);
                unreached.insert(format!("{a:?}"));
            }
        }
        eprintln!(
            "{class:?}: {} scenes, {}/{} of its kit chosen",
            g.records.iter().filter(|r| r.point.class == *class).count(),
            kit.len() - missing.len(),
            kit.len(),
        );
        for a in &missing {
            let why = UNREACHED
                .iter()
                .find(|(u, _)| u == a)
                .map_or("NOT NAMED".to_string(), |(_, why)| why.why());
            eprintln!("  never chosen: {a:?} — {why}");
        }
    }
    let named: BTreeSet<String> = UNREACHED.iter().map(|(a, _)| format!("{a:?}")).collect();
    let unnamed: Vec<_> = unreached.difference(&named).collect();
    let reached_now: Vec<_> = named.difference(&unreached).collect();
    assert!(
        unnamed.is_empty() && reached_now.is_empty(),
        "never chosen but not named in UNREACHED (say why): {unnamed:?}; \
         named but now chosen (delete the row): {reached_now:?}"
    );
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
        (Property::CcNotStacked, cc_not_stacked()),
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

// ---------------------------------------------------------------------------
// A dying ally before damage
// ---------------------------------------------------------------------------

/// What a healer's rotation guarantees: an ally below `bar` that `heal`
/// reaches, and that the healer could heal now, is healed before the healer
/// deals any damage. Each bar is the class's own constant (or the shared
/// urgency bar its rotation reads), never restated here.
struct HealGuarantee {
    bar: f32,
    heal: AbilityType,
}

fn heal_guarantee(class: CharacterClass, movement: &MovementConfig) -> Option<HealGuarantee> {
    use arenasim::states::play_match::class_ai::{priest, shaman};
    use arenasim::states::play_match::constants::HEALTHY_HP_THRESHOLD;
    match class {
        // Flash Heal (the lowest ally in reach below its trigger) comes before
        // Mind Blast.
        CharacterClass::Priest => Some(HealGuarantee {
            bar: priest::FLASH_HEAL_HP,
            heal: AbilityType::FlashHeal,
        }),
        // Holy Shock's damage waits for a team at or above the healthy bar.
        CharacterClass::Paladin => Some(HealGuarantee {
            bar: HEALTHY_HP_THRESHOLD,
            heal: AbilityType::FlashOfLight,
        }),
        // The emergency Lesser Healing Wave; above it the Shaman's
        // offense-first order is by design.
        CharacterClass::Shaman => Some(HealGuarantee {
            bar: shaman::SHAMAN_EMERGENCY_HP,
            heal: AbilityType::LesserHealingWave,
        }),
        // A dying teammate gets the Druid's own Rejuvenation before any
        // damage (when it already carries one, what else is owed depends on
        // the Lifebloom and Swiftmend rules, and the damage may go out).
        CharacterClass::Druid => Some(HealGuarantee {
            bar: movement.shared.urgency_hp_threshold,
            heal: AbilityType::Rejuvenation,
        }),
        CharacterClass::Warrior
        | CharacterClass::Mage
        | CharacterClass::Rogue
        | CharacterClass::Warlock
        | CharacterClass::Hunter => None,
    }
}

fn health_of(p: &Point, name: &str) -> Option<f32> {
    match name {
        ME => Some(p.situation.my_health),
        ALLY => p.situation.ally_health,
        TARGET => p.situation.target_health,
        _ => Some(1.0),
    }
}

/// The ally `g.heal` is guaranteed on in this scene, if any: the lowest-health
/// living, uncycloned ally (the healer included) below the bar that the heal
/// reaches and that the healer can cast it on right now.
fn owed_heal(g: &Grid, r: &Record, guarantee: &HealGuarantee) -> Option<&'static str> {
    let p = &r.point;
    let def = g.defs.get(&guarantee.heal).unwrap();
    let mana_user = !matches!(p.class, CharacterClass::Warrior | CharacterClass::Rogue);
    let caster_free = match p.situation.my_aura {
        SelfLock::Free => true,
        SelfLock::Silence => !(mana_user && def.mana_cost > 0.0),
        SelfLock::SchoolLockout => Some(def.spell_school) != main_school(&g.defs, p.class),
        SelfLock::TravelForm => false,
    };
    if !caster_free || r.my_resource < def.mana_cost || r.my_cooldowns.contains(&guarantee.heal) {
        return None;
    }
    let obstacles: Vec<_> = pillar(p).into_iter().collect();
    let lowest = [ME, ALLY]
        .into_iter()
        .filter(|who| hold_of(p, who) != Hold::Cyclone)
        .filter_map(|who| health_of(p, who).map(|hp| (who, hp)))
        .filter(|(_, hp)| *hp > 0.0 && *hp < guarantee.bar)
        .filter(|(who, _)| {
            let at = pos3(who, p);
            pos3(ME, p).distance(at) <= def.range && has_line_of_sight(&obstacles, pos3(ME, p), at)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    // A heal over time already up on it is not owed again (the scene puts
    // its upkeep up for a full minute, far outside any refresh window).
    let applied = def.applies_aura.is_some() && p.upkeep == Upkeep::Full;
    (!applied).then_some(lowest.0)
}

/// A healer that owes a dying ally its guaranteed heal (each class's own bar:
/// [`heal_guarantee`]) does not deal damage instead — unless the damage is a
/// PEEL on an enemy attacking a dying ally (Frost Shock, Entangling Roots),
/// which counts as answering the danger.
#[test]
fn a_healer_heals_a_dying_ally_before_dealing_damage() {
    let g = grid();
    let movement = load_movement_config().expect("movement.ron loads");
    let mut judged = 0;
    let mut v = Vec::new();
    for (r, ability, target) in choices() {
        let Some(guarantee) = heal_guarantee(r.point.class, &movement) else {
            continue;
        };
        let Some(owed) = owed_heal(g, r, &guarantee) else {
            continue;
        };
        judged += 1;
        let def = g.defs.get(&ability).unwrap();
        let Some(t) = target.filter(|t| team(t) == 2) else {
            continue;
        };
        let damages = def.damage_base_max > 0.0
            || def
                .applies_aura
                .as_ref()
                .is_some_and(|a| a.aura_type == AuraType::DamageOverTime);
        // The grid's enemy target attacks the unit or its ally.
        let controls = def.applies_aura.as_ref().is_some_and(|a| {
            matches!(
                a.aura_type,
                AuraType::Root
                    | AuraType::MovementSpeedSlow
                    | AuraType::Stun
                    | AuraType::Incapacitate
                    | AuraType::Fear
                    | AuraType::Cyclone
            )
        });
        let peel = controls
            && t == TARGET
            && health_of(&r.point, attacked(&r.point)).is_some_and(|hp| hp < guarantee.bar);
        if damages && !peel {
            v.push(format!(
                "{} (owed {:?} on {owed} below {})",
                describe(r, ability, target),
                guarantee.heal,
                guarantee.bar
            ));
        }
    }
    eprintln!("dying ally first: judged {judged} choices made while a heal was owed");
    assert!(judged > 0, "no healer ever owed a heal in the grid");
    assert!(v.is_empty(), "damage dealt while a heal was owed: {v:#?}");
}

fn cc_not_stacked() -> Finding {
    use arenasim::states::play_match::utils::is_incapacitating;
    let g = grid();
    let mut judged = 0;
    let mut violations = Vec::new();
    for (r, ability, target) in choices() {
        let Some(t) = target.filter(|t| team(t) == 2) else {
            continue;
        };
        let def = g.defs.get(&ability).unwrap();
        let locks = def
            .applies_aura
            .as_ref()
            .is_some_and(|a| is_incapacitating(&a.aura_type));
        if !locks {
            continue;
        }
        judged += 1;
        // The grid's only own-team hard CC is the ally's Polymorph.
        if hold_of(&r.point, t) == Hold::FriendlyPolymorph {
            violations.push((ability, describe(r, ability, target)));
        }
    }
    Finding { judged, violations }
}

/// Hard crowd control — a stun, fear, polymorph, incapacitate or cyclone —
/// is never cast on an enemy the deciding unit's own team already holds in
/// one: the cast is wasted, and it spends diminishing returns on a target
/// that is not acting anyway.
#[test]
fn never_stacks_hard_cc_on_a_unit_its_team_holds() {
    verdict(Property::CcNotStacked, Scope::Guard, cc_not_stacked());
}

/// The grid reaches the Peel play, and every peel it makes is a peel, so
/// every property above also judges the play's choices. Under `TeamPlan`,
/// with the ally dying under the enemy target's attack:
///
/// - the (class, tool) pairs the grid's peels used are exactly the pairs
///   `peel_tools` names: set equality, so a class whose peel step stops
///   reaching ANY of its tools — its second as much as its first — fails
///   here, and so does a step that peels with a tool it does not list;
/// - every peel is aimed at the ally's attacker (the enemy target; nothing
///   else in the grid attacks the ally): a targeted peel names it, a
///   self-centred area (Frost Nova, Psychic Scream) holds it inside its
///   radius, and a Frost Trap names it and lands within trigger range of it.
///   A peel step that casts on any other unit fails here;
/// - no peel is made outside `TeamPlan`.
#[test]
fn every_peel_tool_is_reached_and_every_peel_is_aimed_at_the_attacker() {
    use arenasim::states::play_match::class_ai::peel::peel_tools;
    use arenasim::states::play_match::constants::TRAP_TRIGGER_RADIUS;
    use arenasim::states::play_match::decision_trace::PlayStep;
    let g = grid();
    let mut peeled: BTreeSet<(String, String)> = BTreeSet::new();
    let mut misaimed = Vec::new();
    for r in &g.records {
        if r.view.situation.and_then(|s| s.play_step) != Some(PlayStep::Peel) {
            continue;
        }
        let p = &r.point;
        assert!(
            p.situation.team_plan,
            "a peel outside TeamPlan: {}",
            p.describe()
        );
        let (ability, target) = r.view.chosen.expect("a peel chose something");
        peeled.insert((format!("{:?}", p.class), format!("{ability:?}")));
        // The ally's attacker: the enemy target attacks the ally in every
        // situation the play runs in.
        assert!(p.situation.target_attacks_ally, "{}", p.describe());
        let attacker = pos3(TARGET, p);
        let def = g.defs.get(&ability).unwrap();
        let aimed = match ability {
            AbilityType::FrostTrap => {
                target == Some(TARGET)
                    && r.view.trap_landings.iter().any(|(_, at)| {
                        Vec2::new(at.x, at.z).distance(Vec2::new(attacker.x, attacker.z))
                            <= TRAP_TRIGGER_RADIUS
                    })
            }
            // Centred on the caster, with its `range` as the radius; the
            // trace names no target.
            AbilityType::FrostNova | AbilityType::PsychicScream => {
                target.is_none() && pos3(ME, p).distance(attacker) <= def.range
            }
            _ => target == Some(TARGET),
        };
        if !aimed {
            misaimed.push(describe(r, ability, target));
        }
    }
    eprintln!("peels chosen in the grid: {peeled:?}");
    assert!(
        misaimed.is_empty(),
        "peels not aimed at the dying ally's attacker: {misaimed:#?}"
    );
    let expected: BTreeSet<(String, String)> = CharacterClass::all()
        .iter()
        .flat_map(|c| {
            peel_tools(*c)
                .iter()
                .map(move |a| (format!("{c:?}"), format!("{a:?}")))
        })
        .collect();
    assert_eq!(peeled, expected, "(class, tool) pairs the grid peels with");
}

// ---------------------------------------------------------------------------
// Uninterruptible casts
// ---------------------------------------------------------------------------

/// Whether a candidate was held for an interrupt: a crowd control the
/// decision left for Kick to do instead (`plan_kidney_shot`'s hold).
fn held_for_an_interrupt(reason: &Option<RejectionReason>) -> bool {
    matches!(
        reason,
        Some(RejectionReason::PreconditionUnmet { note }) if note.contains("Kick will interrupt")
    )
}

/// The cast the scene's target is mid, and whether an interrupt can take it.
fn target_cast(r: &Record) -> Option<bool> {
    r.point
        .situation
        .target_casting
        .map(|cast| grid().defs.get_unchecked(&cast).can_be_interrupted())
}

/// No decision holds its crowd control for an interrupt when the target's
/// cast is `uninterruptible` (AS-287): no interrupt will take it, so the CC is
/// the only thing that stops it. Interrupts themselves are decided outside the
/// dispatch ([`UNREACHED`]); `tests/uninterruptible_casts.rs` holds them to the
/// same rule.
///
/// Paired inside the grid: the same scenes with an interruptible cast DO hold
/// for the interrupt somewhere, so the hold this property looks for is real.
/// Mutant killed: `plan_kidney_shot` ignores the flag, and the Rogue holds
/// Kidney Shot for a Kick that cannot land.
#[test]
fn crowd_control_is_never_held_for_an_interrupt_an_uninterruptible_cast_refuses() {
    let mut judged = 0;
    let mut control_holds = 0;
    let mut v = Vec::new();
    for r in &grid().records {
        let Some(interruptible) = target_cast(r) else {
            continue;
        };
        for c in &r.view.candidates {
            if !held_for_an_interrupt(&c.reason) {
                judged += usize::from(!interruptible);
                continue;
            }
            if interruptible {
                control_holds += 1;
            } else {
                v.push(format!(
                    "{} -> held {:?} for an interrupt",
                    r.point.describe(),
                    c.ability
                ));
            }
        }
    }
    eprintln!(
        "uninterruptible: judged {judged} candidates; the interruptible pair held {control_holds}"
    );
    assert!(judged > 0, "no decision met an uninterruptible cast");
    assert!(
        control_holds > 0,
        "no decision ever held for an interrupt, so this property cannot see one"
    );
    assert!(
        v.is_empty(),
        "held for an interrupt that cannot land: {v:#?}"
    );
}
