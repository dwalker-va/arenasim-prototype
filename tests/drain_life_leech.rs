//! AS-461 — Drain Life heals off the damage each tick lands.
//!
//! TBC's Drain Life is a periodic leech (aura 53, multiplier 1): each tick
//! heals its caster by the damage that tick dealt. The channel declares it as
//! a `channel_leech` and lands it through `combat_core::leech`, the mechanism
//! Death Coil and Siphon Life heal through, so a tick a shield soaks, a cut
//! halves or an immunity stops heals that much less, and a tick that lands
//! nothing heals nothing.
//!
//! The tick's other numbers follow the landed damage too: the channeler's
//! `damage_dealt` (the Results screen's total) agrees with the log's
//! per-ability rows, the floating damage number is what landed, and a heal
//! that landed nothing floats no "+0".
//!
//! One tick decides each of these, so most run `process_channeling` once on a
//! built world; the last pair plays the tick in a staged match, through the
//! production schedule.

mod common;

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::combat_core::leech::{Leech, LeechRecipient, LeechSource};
use arenasim::states::play_match::combat_core::process_channeling;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraType, ChannelingState, Combatant, FloatingCombatText,
};
use arenasim::states::play_match::AbilityType;
use common::staged::play;

const WARLOCK: &str = "Team 1 Warlock #1";
const WARRIOR: &str = "Team 2 Warrior #1";

/// Drain Life's tick (`abilities.ron`), against no resistance.
const TICK: f32 = 10.0;

/// The channeler's health before the tick: hurt, so a whole tick's heal lands.
const HURT: f32 = 100.0;

/// What one tick left behind.
struct Ticked {
    log: CombatLog,
    warlock: Combatant,
    /// Every floating number the tick spawned, as (text, over the Warlock).
    floats: Vec<(String, bool)>,
}

impl Ticked {
    fn heal_lines(&self) -> Vec<&str> {
        self.log
            .entries
            .iter()
            .map(|e| e.message.as_str())
            .filter(|m| m.contains("heals"))
            .collect()
    }

    fn floats_over_warlock(&self) -> Vec<&str> {
        self.floats
            .iter()
            .filter(|(_, over)| *over)
            .map(|(t, _)| t.as_str())
            .collect()
    }

    fn floats_over_target(&self) -> Vec<&str> {
        self.floats
            .iter()
            .filter(|(_, over)| !*over)
            .map(|(t, _)| t.as_str())
            .collect()
    }
}

/// One Drain Life tick from a Warlock at `warlock_health` on a Warrior at 400
/// health carrying `auras`.
fn tick(warlock_health: f32, auras: Vec<Aura>) -> Ticked {
    tick_with(warlock_health, auras, |_| Vec::new())
}

/// [`tick`], with `caster_auras` building auras on the target that name the
/// Warlock as their caster.
fn tick_with(
    warlock_health: f32,
    auras: Vec<Aura>,
    caster_auras: impl FnOnce(Entity) -> Vec<Aura>,
) -> Ticked {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
    world.insert_resource(time);
    let mut warrior = Combatant::new(2, 0, CharacterClass::Warrior);
    warrior.current_health = 400.0;
    warrior.shadow_resistance = 0.0;
    let target = world
        .spawn((
            warrior,
            Transform::from_xyz(20.0, 0.0, 0.0),
            ActiveAuras { auras },
        ))
        .id();
    let mut warlock = Combatant::new(1, 0, CharacterClass::Warlock);
    warlock.current_health = warlock_health;
    let caster = world
        .spawn((
            warlock,
            Transform::default(),
            ChannelingState {
                ability: AbilityType::DrainLife,
                duration_remaining: 5.0,
                time_until_next_tick: 0.0,
                tick_interval: 1.0,
                target,
                interrupted: false,
                interrupted_display_time: 0.0,
                ticks_applied: 0,
            },
        ))
        .id();
    let mut more = caster_auras(caster);
    world
        .get_mut::<ActiveAuras>(target)
        .unwrap()
        .auras
        .append(&mut more);
    world.run_system_once(process_channeling).unwrap();
    let warlock = world.get::<Combatant>(caster).unwrap().clone();
    let mut q = world.query::<&FloatingCombatText>();
    let floats = q
        .iter(&world)
        .map(|f| (f.text.clone(), f.world_position.x < 10.0))
        .collect();
    Ticked {
        log: world.remove_resource::<CombatLog>().unwrap(),
        warlock,
        floats,
    }
}

fn aura(kind: AuraType, magnitude: f32) -> Aura {
    Aura {
        effect_type: kind,
        duration: 30.0,
        magnitude,
        break_on_damage_threshold: -1.0,
        ability_name: format!("{kind:?}"),
        ..Default::default()
    }
}

/// A tick that lands whole heals for all of it.
#[test]
fn a_tick_heals_the_damage_it_landed() {
    let t = tick(HURT, vec![]);
    assert_eq!(t.warlock.current_health, HURT + TICK);
    assert_eq!(t.warlock.healing_done, TICK);
    assert_eq!(
        t.heal_lines(),
        vec![format!("{WARLOCK}'s Drain Life heals for 10")]
    );
    assert_eq!(t.floats_over_warlock(), vec!["+10"]);
}

/// A shield soaks part of the tick: the heal is the part that got past it.
#[test]
fn a_tick_into_a_shield_heals_only_what_got_past_it() {
    let t = tick(HURT, vec![aura(AuraType::Absorb, 4.0)]);
    assert_eq!(t.warlock.current_health, HURT + 6.0);
    assert_eq!(t.warlock.healing_done, 6.0);
    assert_eq!(
        t.heal_lines(),
        vec![format!("{WARLOCK}'s Drain Life heals for 6")]
    );
    assert_eq!(t.floats_over_warlock(), vec!["+6"]);
    // Control: no shield, the whole tick.
    assert_eq!(tick(HURT, vec![]).warlock.current_health, HURT + TICK);
}

/// A damage-taken cut halves the tick, and the heal with it.
#[test]
fn a_tick_its_modifiers_cut_heals_the_cut_tick() {
    let t = tick(HURT, vec![aura(AuraType::DamageTakenReduction, 0.5)]);
    assert_eq!(t.warlock.current_health, HURT + 5.0);
    // Control: no cut.
    assert_eq!(tick(HURT, vec![]).warlock.current_health, HURT + TICK);
}

/// An immune target takes nothing, so the tick heals nothing: no heal line,
/// no heal credited, no number over the channeler.
#[test]
fn a_tick_into_an_immune_target_heals_nothing() {
    for immunity in [AuraType::DamageImmunity, AuraType::Cyclone] {
        let t = tick(HURT, vec![aura(immunity, 0.0)]);
        assert_eq!(t.warlock.current_health, HURT, "{immunity:?}");
        assert_eq!(t.warlock.healing_done, 0.0, "{immunity:?}");
        assert!(
            t.heal_lines().is_empty(),
            "{immunity:?}: {:?}",
            t.heal_lines()
        );
        assert!(
            t.floats_over_warlock().is_empty(),
            "{immunity:?}: {:?}",
            t.floats_over_warlock()
        );
    }
    // Control: a target without the immunity is drained.
    assert_eq!(tick(HURT, vec![]).warlock.current_health, HURT + TICK);
}

/// A leech the Warlock placed on its target that its Shadow damage feeds (a
/// Vampiric Embrace's shape) is fed by the tick too, beside the channel's
/// own: both heal the Warlock off the damage that landed.
#[test]
fn a_tick_feeds_its_casters_leech_on_the_target_too() {
    let embrace = |caster| {
        vec![Aura {
            caster: Some(caster),
            leech: Some(Leech {
                fraction: 0.2,
                to: LeechRecipient::Caster,
                of: LeechSource::CasterSchool(SpellSchool::Shadow),
            }),
            ..aura(AuraType::DamageTakenIncrease, 0.0)
        }]
    };
    let t = tick_with(HURT, vec![], embrace);
    assert_eq!(t.warlock.current_health, HURT + TICK + 2.0);
    assert_eq!(t.warlock.healing_done, TICK + 2.0);
    // Control: the tick's own leech alone.
    assert_eq!(tick(HURT, vec![]).warlock.current_health, HURT + TICK);
}

/// A heal that lands nothing — the channeler is at full health — floats no
/// "+0" and logs no heal line.
#[test]
fn a_heal_that_lands_nothing_floats_nothing() {
    let full = Combatant::new(1, 0, CharacterClass::Warlock).max_health;
    let t = tick(full, vec![]);
    assert_eq!(t.warlock.current_health, full);
    assert!(t.heal_lines().is_empty(), "{:?}", t.heal_lines());
    assert!(
        t.floats_over_warlock().is_empty(),
        "{:?}",
        t.floats_over_warlock()
    );
    // Control: a hurt channeler is healed, and the number floats.
    assert_eq!(tick(HURT, vec![]).floats_over_warlock(), vec!["+10"]);
}

/// The floating damage number is the damage that landed, beside what a shield
/// took.
#[test]
fn the_damage_number_is_what_landed() {
    let mut floats = tick(HURT, vec![aura(AuraType::Absorb, 4.0)])
        .floats_over_target()
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    floats.sort();
    assert_eq!(floats, vec!["4 absorbed", "6"]);
    // Control: a whole tick floats whole.
    assert_eq!(tick(HURT, vec![]).floats_over_target(), vec!["10"]);
}

/// The channeler's damage and healing totals — what the Results screen's row
/// shows — are the sums of its per-ability breakdown, shield or no shield.
#[test]
fn the_results_totals_are_the_breakdowns_sums() {
    for (auras, landed) in [
        (vec![aura(AuraType::Absorb, 4.0)], 6.0),
        // Control: a whole tick, where the two always agreed.
        (vec![], TICK),
    ] {
        let t = tick(HURT, auras);
        assert_eq!(t.warlock.damage_dealt, landed);
        assert_eq!(t.log.total_damage_dealt(WARLOCK), landed);
        assert_eq!(t.warlock.healing_done, landed);
        assert_eq!(t.log.total_healing_done(WARLOCK), landed);
        // The target took what the Warlock was credited with.
        assert!(t.log.entries.iter().any(|e| e.message.starts_with(&format!(
            "{WARLOCK}'s Drain Life ticks on {WARRIOR} for {landed:.0} damage"
        ))));
    }
}

// ---------------------------------------------------------------------------
// The tick in a match
// ---------------------------------------------------------------------------

const LOCK_1: Unit = Unit::new(1, 0);
const LOCK_2: Unit = Unit::new(2, 0);

fn config() -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warlock".into()],
        map: "BasicArena".into(),
        random_seed: Some(461),
        max_duration_secs: 5.0,
        ..Default::default()
    }
}

/// `LOCK_1`, hurt, drains `LOCK_2`, which carries `shield` (if any). Both
/// stand still with no mana and no resistance; both pets are benched out of
/// the way.
fn drain_stage(shield: Option<f32>) -> Stage {
    let mut stage = Stage::new();
    for (lock, x, health) in [(LOCK_1, -10.0, HURT), (LOCK_2, 10.0, 400.0)] {
        stage = stage
            .place(lock, x, 0.0)
            .clear_auras(lock)
            .mana(lock, 0.0)
            .combatant(lock, move |c| {
                c.current_health = health;
                c.shadow_resistance = 0.0;
            });
        let pet = Unit::pet_of(lock.team, lock.slot);
        stage = stage.place(pet, x, 60.0).mana(pet, 0.0);
    }
    if let Some(amount) = shield {
        stage = stage.aura(LOCK_2, aura(AuraType::Absorb, amount));
    }
    stage.world(|world| {
        let target = LOCK_2.entity(world);
        let caster = LOCK_1.entity(world);
        world.entity_mut(caster).insert(ChannelingState {
            ability: AbilityType::DrainLife,
            duration_remaining: 5.0,
            time_until_next_tick: 0.0,
            tick_interval: 1.0,
            target,
            interrupted: false,
            interrupted_display_time: 0.0,
            ticks_applied: 0,
        });
    })
}

/// The draining Warlock's health on the frame its tick resolved.
fn drainer_health_on_tick_frame(shield: Option<f32>) -> f32 {
    let played = play(config(), drain_stage(shield));
    assert!(
        played.log.contains("Drain Life ticks on"),
        "the tick resolved:\n{}",
        played.log
    );
    let frame = played.staged_frames().next().expect("a staged frame");
    let (_, c) = frame
        .combatants
        .iter()
        .find(|(_, c)| c.team == 1 && c.slot == 0 && !c.is_pet)
        .expect("the drainer");
    c.current_health
}

/// In a match, a tick into a shield heals the drainer for the part of it that
/// got past the shield.
#[test]
fn in_a_match_a_tick_into_a_shield_heals_the_part_that_landed() {
    assert_eq!(drainer_health_on_tick_frame(Some(4.0)), HURT + 6.0);
}

/// The control: no shield, the whole tick's heal.
#[test]
fn in_a_match_a_whole_tick_heals_whole() {
    assert_eq!(drainer_health_on_tick_frame(None), HURT + TICK);
}
