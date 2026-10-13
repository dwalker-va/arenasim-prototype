//! Interrupts need sight (AS-293).
//!
//! Kick, Pummel, Wind Shear and the Felhunter's Spell Lock choose their target
//! by range, then line of sight (`class_ai::cast_reach`), as every targeted
//! cast does at cast start, and no interrupt lands on a caster its
//! interrupter cannot see.
//!
//! - **Each interrupter** (scenes): a pillar between it and an enemy mid a
//!   cast means no interrupt, traced `LosBlocked`; the same scene without the
//!   pillar interrupts.
//! - **The scans** (scenes): an interrupter that picks among casters passes
//!   over one behind a pillar for one it can see — the Shaman's healer-first
//!   pick, every interrupter's Cyclone-first pick, and Spell Lock's heal-first
//!   pick — and, without the pillar, picks the one it prefers.
//! - **The use site** (`check_interrupts`, one pass in a world with a pillar):
//!   no interrupt queued, no cooldown spent; without the pillar, one queued.
//! - **Resolution** (`process_interrupts`): an interrupt queued at a caster
//!   behind a pillar cuts nothing and locks nothing; the same one in the open
//!   does both.
//! - **The list**: every `is_interrupt` ability in `abilities.ron` is either a
//!   class's own interrupt (`class_interrupt`) or the Felhunter's Spell Lock.

use std::collections::BTreeSet;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::interrupts::class_interrupt;
use arenasim::states::play_match::class_ai::scene::{Scene, Who};
use arenasim::states::play_match::components::{
    AuraPending, AuraType, CastingState, Combatant, InterruptPending, PetType,
};
use arenasim::states::play_match::decision_trace::{DecisionTrace, RejectionReason};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::systems::{check_interrupts, process_interrupts};
use arenasim::states::play_match::{AbilityDefinitions, AbilityType};

const ME: Who = "interrupter";
const CASTER: Who = "caster";
const HEALER: Who = "healer";
const DRUID: Who = "druid";

/// A pillar centred at (`x`, `z`).
fn pillar(x: f32, z: f32, radius: f32) -> ObstacleVolume {
    ObstacleVolume::Cylinder {
        center_xz: Vec2::new(x, z),
        radius,
        base_y: 0.0,
        height: 10.0,
    }
}

/// A pillar halfway along the line from the origin to (`x`, 0), narrow
/// enough to stand between two units in melee.
fn pillar_between(x: f32) -> ObstacleVolume {
    pillar(x / 2.0, 0.0, (x / 4.0).min(1.5))
}

fn is_los_blocked(reason: Option<&RejectionReason>) -> bool {
    matches!(reason, Some(RejectionReason::LosBlocked))
}

/// An interrupter of `class` at the origin, ready, its kill target an enemy
/// Mage mid-Frostbolt just inside the interrupt's range — with a pillar
/// between them or not.
fn interrupter_scene(class: CharacterClass, pillared: bool) -> (Scene, AbilityType) {
    let defs = load_ability_definitions().expect("abilities.ron loads");
    let ability = class_interrupt(class).expect("an interrupter");
    let distance = defs.get_unchecked(&ability).range - 0.1;
    let mut scene = Scene::new().unit(ME, 1, class).targeting(CASTER);
    scene = if class == CharacterClass::Warrior {
        scene.rage(100.0)
    } else {
        scene.mana(1.0)
    };
    scene = scene
        .unit(CASTER, 2, CharacterClass::Mage)
        .at(distance, 0.0)
        .casting(CASTER, AbilityType::Frostbolt);
    scene.combatant_mut(ME).stealthed = false;
    if pillared {
        scene = scene.obstacles(vec![pillar_between(distance)]);
    }
    (scene, ability)
}

/// Each interrupter: a pillar between it and its target's cast means no
/// interrupt, traced `LosBlocked`; the same scene in the open interrupts.
///
/// Mutant killed: `cast_reach` (or the gate's sight step) dropped from
/// `decide_interrupt` — the pillared scene interrupts.
#[test]
fn a_pillar_between_means_no_interrupt() {
    for class in [
        CharacterClass::Warrior,
        CharacterClass::Rogue,
        CharacterClass::Shaman,
    ] {
        let (open, ability) = interrupter_scene(class, false);
        assert_eq!(
            open.interrupt(ME).chose(),
            Some((ability, Some(CASTER))),
            "{class:?} in the open interrupts its target"
        );
        let (pillared, _) = interrupter_scene(class, true);
        let view = pillared.interrupt(ME);
        assert_eq!(view.chose(), None, "{class:?} interrupts through a pillar");
        assert!(
            is_los_blocked(view.rejected(ability)),
            "{class:?}: {ability:?} not traced LosBlocked: {:?}",
            view.candidates
        );
    }
}

/// The Felhunter: Spell Lock takes the Mage's Frostbolt in the open, and
/// nobody's through a pillar, traced `LosBlocked`.
///
/// Mutant killed: `cast_reach` dropped from `spell_lock_pick`.
#[test]
fn spell_lock_needs_sight() {
    let felhunter = |pillared: bool| {
        let mut s = Scene::new()
            .unit(ME, 1, CharacterClass::Warlock)
            .unit(CASTER, 2, CharacterClass::Mage)
            .at(20.0, 0.0)
            .casting(CASTER, AbilityType::Frostbolt);
        if pillared {
            s = s.obstacles(vec![pillar_between(20.0)]);
        }
        s
    };
    assert!(matches!(felhunter(false).spell_lock(ME), Ok(CASTER)));
    let refused = felhunter(true).spell_lock(ME);
    assert!(
        matches!(refused, Err(RejectionReason::LosBlocked)),
        "{refused:?}"
    );
    // A channel too: Drain Life behind the pillar.
    let channel = |pillared: bool| {
        let mut s = Scene::new()
            .unit(ME, 1, CharacterClass::Warlock)
            .unit(CASTER, 2, CharacterClass::Warlock)
            .at(20.0, 0.0)
            .casting(CASTER, AbilityType::DrainLife);
        if pillared {
            s = s.obstacles(vec![pillar_between(20.0)]);
        }
        s
    };
    assert!(matches!(channel(false).spell_lock(ME), Ok(CASTER)));
    assert!(matches!(
        channel(true).spell_lock(ME),
        Err(RejectionReason::LosBlocked)
    ));
}

/// A Shaman with two enemies casting in Wind Shear range: the healer it
/// prefers mid-Flash Heal, behind a pillar, and a Mage mid-Frostbolt in sight.
/// It Wind Shears the Mage; with no pillar, the healer.
///
/// Mutant killed: the scan ranks by range alone — it picks the hidden healer
/// and is then refused, interrupting nobody.
#[test]
fn wind_shear_passes_over_a_hidden_healer_for_a_caster_in_sight() {
    let scene = |pillared: bool| {
        let mut s = Scene::new()
            .unit(ME, 1, CharacterClass::Shaman)
            .mana(1.0)
            .unit(HEALER, 2, CharacterClass::Priest)
            .at(20.0, 6.0)
            .casting(HEALER, AbilityType::FlashHeal)
            .unit(CASTER, 2, CharacterClass::Mage)
            .at(20.0, -8.0)
            .casting(CASTER, AbilityType::Frostbolt);
        if pillared {
            s = s.obstacles(vec![pillar(10.0, 3.0, 1.5)]);
        }
        s
    };
    assert_eq!(
        scene(false).interrupt(ME).chose(),
        Some((AbilityType::WindShear, Some(HEALER))),
        "control: in the open the healer comes first"
    );
    assert_eq!(
        scene(true).interrupt(ME).chose(),
        Some((AbilityType::WindShear, Some(CASTER))),
        "the hidden healer yields to the Mage in sight"
    );
}

/// A Warrior whose kill target, a Mage mid-Frostbolt, is in sight, beside an
/// enemy Druid mid-Cyclone — the cast every interrupter takes first — behind
/// a pillar. It Pummels the Mage; with no pillar, the Druid.
///
/// Mutant killed: the Cyclone-first scan ranks by range alone — it picks the
/// hidden Druid and is then refused, so the Mage's cast goes uncut.
#[test]
fn the_cyclone_first_pick_passes_over_a_hidden_druid() {
    let scene = |pillared: bool| {
        let mut s = Scene::new()
            .unit(ME, 1, CharacterClass::Warrior)
            .rage(100.0)
            .targeting(CASTER)
            .unit(CASTER, 2, CharacterClass::Mage)
            .at(2.4, 0.0)
            .casting(CASTER, AbilityType::Frostbolt)
            .unit(DRUID, 2, CharacterClass::Druid)
            .at(-2.4, 0.0)
            .casting(DRUID, AbilityType::Cyclone);
        if pillared {
            s = s.obstacles(vec![pillar(-1.2, 0.0, 0.5)]);
        }
        s
    };
    assert_eq!(
        scene(false).interrupt(ME).chose(),
        Some((AbilityType::Pummel, Some(DRUID))),
        "control: in the open the Cyclone comes first"
    );
    assert_eq!(
        scene(true).interrupt(ME).chose(),
        Some((AbilityType::Pummel, Some(CASTER))),
        "the hidden Cyclone yields to the kill target in sight"
    );
}

/// The Felhunter: an enemy Priest mid-Flash Heal behind a pillar and a Mage
/// mid-Frostbolt in sight. It Spell Locks the Mage; with no pillar, the heal.
///
/// Mutant killed: Spell Lock's scan ranks by range alone.
#[test]
fn spell_lock_passes_over_a_hidden_heal_for_a_cast_in_sight() {
    let scene = |pillared: bool| {
        let mut s = Scene::new()
            .unit(ME, 1, CharacterClass::Warlock)
            .unit(HEALER, 2, CharacterClass::Priest)
            .at(20.0, 6.0)
            .casting(HEALER, AbilityType::FlashHeal)
            .unit(CASTER, 2, CharacterClass::Mage)
            .at(20.0, -8.0)
            .casting(CASTER, AbilityType::Frostbolt);
        if pillared {
            s = s.obstacles(vec![pillar(10.0, 3.0, 1.5)]);
        }
        s
    };
    assert!(matches!(scene(false).spell_lock(ME), Ok(HEALER)));
    assert!(matches!(scene(true).spell_lock(ME), Ok(CASTER)));
}

// ── the use site ────────────────────────────────────────────────────────────

/// A Rogue in Kick range of a Mage mid-Frostbolt, with a pillar between them
/// or not: one pass of `check_interrupts`. Returns (interrupts queued, Kick on
/// cooldown, the rejection traced for Kick).
fn kick_pass(pillared: bool) -> (usize, bool, Option<RejectionReason>) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(DecisionTrace::default());
    let defs = AbilityDefinitions::default();
    let cast_time = defs.get_unchecked(&AbilityType::Frostbolt).cast_time;
    world.insert_resource(defs);
    world.insert_resource(ActiveMapGeometry {
        volumes: if pillared {
            vec![pillar_between(2.4)]
        } else {
            Vec::new()
        },
        ..Default::default()
    });
    let mut rogue = Combatant::new(1, 0, CharacterClass::Rogue);
    rogue.stealthed = false;
    let rogue = world
        .spawn((rogue, Transform::from_xyz(0.0, 1.0, 0.0)))
        .id();
    let mage = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Mage),
            Transform::from_xyz(2.4, 1.0, 0.0),
            CastingState::new(AbilityType::Frostbolt, rogue, cast_time),
        ))
        .id();
    world.get_mut::<Combatant>(rogue).unwrap().target = Some(mage);
    world.run_system_once(check_interrupts).unwrap();

    let queued = world.query::<&InterruptPending>().iter(&world).count();
    let on_cooldown = world
        .get::<Combatant>(rogue)
        .unwrap()
        .ability_cooldowns
        .contains_key(&AbilityType::Kick);
    let traced = world
        .resource::<DecisionTrace>()
        .pending_events
        .iter()
        .find_map(|e| match &e.payload {
            arenasim::states::play_match::decision_trace::EventPayload::Ability {
                candidates,
                ..
            } => candidates
                .iter()
                .find(|c| c.ability == AbilityType::Kick)
                .and_then(|c| c.reason.clone()),
            _ => None,
        });
    (queued, on_cooldown, traced)
}

/// `check_interrupts` itself, on the map's own geometry: through a pillar the
/// Rogue queues nothing and keeps Kick, tracing `LosBlocked`; in the open it
/// kicks.
///
/// Mutant killed: `check_interrupts` builds its context without the map's
/// obstacles.
#[test]
fn check_interrupts_reads_the_map() {
    assert_eq!(
        kick_pass(false).0,
        1,
        "control: in the open the Rogue kicks"
    );
    assert!(kick_pass(false).1);
    let (queued, on_cooldown, traced) = kick_pass(true);
    assert_eq!(queued, 0, "no Kick through the pillar");
    assert!(!on_cooldown, "Kick is not spent");
    assert!(is_los_blocked(traced.as_ref()), "{traced:?}");
}

// ── resolution ──────────────────────────────────────────────────────────────

/// An enemy Mage mid-Frostbolt and a Pummel queued at it, with `volumes` on
/// the map (or no map at all). Returns (cast cut?, lockouts queued).
fn resolve_pummel(map: Option<Vec<ObstacleVolume>>) -> (bool, usize) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    let defs = AbilityDefinitions::default();
    let cast_time = defs.get_unchecked(&AbilityType::Frostbolt).cast_time;
    world.insert_resource(defs);
    if let Some(volumes) = map {
        world.insert_resource(ActiveMapGeometry {
            volumes,
            ..Default::default()
        });
    }
    let warrior = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warrior),
            Transform::from_xyz(0.0, 1.0, 0.0),
        ))
        .id();
    let mage = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Mage),
            Transform::from_xyz(2.4, 1.0, 0.0),
            CastingState::new(AbilityType::Frostbolt, warrior, cast_time),
        ))
        .id();
    world.spawn(InterruptPending {
        caster: warrior,
        target: mage,
        ability: AbilityType::Pummel,
        lockout_duration: 4.0,
    });
    world.run_system_once(process_interrupts).unwrap();

    let cut = world.get::<CastingState>(mage).unwrap().interrupted;
    let lockouts = world
        .query::<&AuraPending>()
        .iter(&world)
        .filter(|p| p.target == mage && p.aura.effect_type == AuraType::SpellSchoolLockout)
        .count();
    assert_eq!(
        world.query::<&InterruptPending>().iter(&world).count(),
        0,
        "the interrupt is consumed"
    );
    (cut, lockouts)
}

/// An interrupt resolving on a caster its interrupter cannot see cuts nothing
/// and locks nothing; the same interrupt in the open (or with no map) does
/// both.
///
/// Mutant killed: drop the sight check from `process_interrupts`.
#[test]
fn an_interrupt_resolving_through_a_pillar_does_nothing() {
    assert_eq!(
        resolve_pummel(Some(vec![pillar_between(2.4)])),
        (false, 0),
        "no cut and no lockout through a pillar"
    );
    assert_eq!(
        resolve_pummel(Some(Vec::new())),
        (true, 1),
        "control: the open map"
    );
    assert_eq!(resolve_pummel(None), (true, 1), "control: no map at all");
}

// ── the list ────────────────────────────────────────────────────────────────

/// Every `is_interrupt` ability in `abilities.ron` is decided somewhere that
/// asks for sight: a class's own interrupt is that class's `class_interrupt`
/// (`check_interrupts`), and the one pet interrupt is the Felhunter's Spell
/// Lock (its AI). Compared as sets, both ways, so a new interrupt fails here
/// until it is routed.
#[test]
fn every_interrupt_in_the_config_is_routed() {
    let defs = load_ability_definitions().expect("abilities.ron loads");
    let configured: BTreeSet<String> = defs
        .iter()
        .filter(|(_, d)| d.is_interrupt)
        .map(|(a, d)| format!("{a:?} ({:?}, pet {:?})", d.class, d.pet))
        .collect();
    let routed: BTreeSet<String> = CharacterClass::all()
        .iter()
        .filter_map(|&class| class_interrupt(class).map(|a| (class, a)))
        .map(|(class, a)| format!("{a:?} ({class:?}, pet None)"))
        .chain(std::iter::once(format!(
            "{:?} ({:?}, pet {:?})",
            AbilityType::SpellLock,
            CharacterClass::Warlock,
            Some(PetType::Felhunter)
        )))
        .collect();
    assert_eq!(configured, routed);
}
