//! The Frost Armor chill is ONE debuff, and a dispel takes all of it.
//!
//! The unit tests beside `ActiveAuras` prove the removal helpers take a whole
//! compound. This drives the real `process_dispels` system in a Bevy app
//! instead, because the helper is only half the claim: the other half is that
//! the dispel PATH calls it, on the aura it actually rolled, and that the
//! rider — which is not dispellable in its own right and never appears in the
//! candidate set — leaves with it.
//!
//! That asymmetry is the bug the card names. Before the chill's two effects
//! were bound into one debuff, the movement slow was a dispel candidate and
//! the attack-speed slow was not, so Devour Magic or Master's Call lifted half
//! a debuff and left the other half standing under the same name.

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::combat_core::{
    compound_riders, effective_attack_interval, frost_armor_chill_auras,
    frost_armor_movement_slow_aura, FROST_ARMOR_PROC_DURATION,
};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, Combatant, CompoundDebuff,
    DRCategory, DRTracker, DispelPending, GameRng,
};
use arenasim::states::play_match::effects::process_dispels;
use arenasim::CharacterClass;

/// A minimal app holding just what `process_dispels` reads.
fn harness() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(CombatLog::default());
    app.insert_resource(GameRng::from_seed(7));
    app.add_systems(Update, process_dispels);
    app
}

fn chilled_victim(app: &mut App, extra: Vec<Aura>) -> Entity {
    let mut auras: Vec<Aura> = frost_armor_chill_auras().into();
    auras.extend(extra);
    app.world_mut()
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warrior),
            ActiveAuras { auras },
        ))
        .id()
}

fn queue_dispel(app: &mut App, target: Entity, filter: Option<Vec<AuraType>>) {
    let dispeller = app
        .world_mut()
        .spawn(Combatant::new(1, 1, CharacterClass::Warlock))
        .id();
    app.world_mut().spawn(DispelPending {
        target,
        dispeller,
        log_prefix: "[DEVOUR]",
        caster_class: CharacterClass::Warlock,
        heal_on_success: None,
        aura_type_filter: filter,
        removes_poison: false,
    });
}

fn remaining(app: &App, entity: Entity) -> Vec<AuraType> {
    app.world()
        .entity(entity)
        .get::<ActiveAuras>()
        .map(|auras| auras.auras.iter().map(|a| a.effect_type).collect())
        .unwrap_or_default()
}

/// The headline. An unfiltered dispel (Dispel Magic, Cleanse, Devour Magic)
/// can only ever roll the chill's movement slow — the attack-speed rider is
/// not a candidate — and both effects come off together.
#[test]
fn devour_magic_takes_the_whole_chill() {
    let mut app = harness();
    let victim = chilled_victim(&mut app, vec![]);
    queue_dispel(&mut app, victim, None);
    app.update();

    assert!(
        remaining(&app, victim).is_empty(),
        "a dispel left {:?} of the chill behind",
        remaining(&app, victim)
    );
}

/// Master's Call pins its filter to movement impairments, so it reaches the
/// chill by its FACE and nothing else. It still takes the whole debuff: the
/// rider is part of the thing being removed, not a separate debuff that
/// happens to share a name.
#[test]
fn masters_call_takes_the_whole_chill() {
    let mut app = harness();
    let victim = chilled_victim(&mut app, vec![]);
    queue_dispel(
        &mut app,
        victim,
        Some(vec![AuraType::Root, AuraType::MovementSpeedSlow]),
    );
    app.update();

    assert!(
        remaining(&app, victim).is_empty(),
        "Master's Call left {:?} of the chill behind",
        remaining(&app, victim)
    );
}

/// The effect the rider carries is real, and removing it is what the dispel
/// now buys: a chilled attacker swings slower, and a dispelled one does not.
/// Asserted on the swing timer rather than on the aura list, so the test would
/// fail if the rider were removed from the debuff outright instead of bound
/// into it.
#[test]
fn the_rider_is_what_slows_the_swing() {
    let mut app = harness();
    let victim = chilled_victim(&mut app, vec![]);

    let chilled = {
        let entity = app.world().entity(victim);
        effective_attack_interval(
            entity.get::<Combatant>().expect("combatant"),
            entity.get::<ActiveAuras>(),
        )
    };

    queue_dispel(&mut app, victim, None);
    app.update();

    let entity = app.world().entity(victim);
    let combatant = entity.get::<Combatant>().expect("combatant");
    let freed = effective_attack_interval(combatant, entity.get::<ActiveAuras>());

    assert!(
        chilled > freed,
        "the chill must stretch the swing interval ({chilled} vs {freed})"
    );
    assert_eq!(
        freed,
        1.0 / combatant.attack_speed,
        "after the dispel the attacker swings at its base cadence"
    );
}

/// A dispel takes ONE debuff, not every debuff. The compound must not become a
/// vacuum that clears unrelated auras sharing the target.
#[test]
fn only_the_rolled_debuff_leaves() {
    let mut app = harness();
    let unrelated = Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 12.0,
        magnitude: 10.0,
        ability_name: "Rend".to_string(),
        ..Default::default()
    };
    let victim = chilled_victim(&mut app, vec![unrelated]);
    queue_dispel(&mut app, victim, Some(vec![AuraType::MovementSpeedSlow]));
    app.update();

    assert_eq!(remaining(&app, victim), vec![AuraType::DamageOverTime]);
}

// ---------------------------------------------------------------------------
// The riders' lifetime, driven through the real `apply_pending_auras`
// ---------------------------------------------------------------------------
//
// A rider is not queued as a pending of its own: `apply_pending_auras` pulls it
// in when the FACE lands, stamped with the face's post-diminishing-returns
// duration. Both halves of that need a guard that names them, because both were
// live defects on main:
//
//   - the rider carried no DR category, so it kept its full 5 seconds while a
//     diminished chill expired after 1.2 — half a debuff outliving the icon
//     that represents it;
//   - and it survived a DR-IMMUNE rejection its face did not, landing alone on
//     a target with no Frost Armor on its frames and nothing for a dispel to
//     take hold of.
//
// The suite used to prove neither. `compound_members_share_their_lifetime`
// deliberately pins equality at the CONSTRUCTORS — which is the right thing for
// the encyclopedia to read — and its doc comment then described the in-play
// rule without pinning it. Deleting `rider.duration = face_duration` left the
// whole default suite green except for behavioural pins that the next person
// would simply re-record.

/// Drive the real aura-application system, with everything it reads.
fn apply_harness() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(CombatLog::default());
    app.insert_resource(ArenaDampening::default());
    app.add_systems(Update, apply_pending_auras);
    app
}

/// A target that can be diminished: `DRTracker` is what makes the chill's
/// `Slows` category scale, and `Transform` is required by the system's query.
fn dr_victim(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::default(),
            DRTracker::default(),
        ))
        .id()
}

/// Queue the chill the way the proc site does: ONE pending, for the face.
fn proc_chill(app: &mut App, target: Entity) {
    let aura = frost_armor_movement_slow_aura();
    app.world_mut().spawn(AuraPending { target, aura });
    app.update();
}

fn durations(app: &App, entity: Entity) -> Vec<(AuraType, f32)> {
    app.world()
        .entity(entity)
        .get::<ActiveAuras>()
        .map(|a| {
            a.auras
                .iter()
                .map(|x| (x.effect_type, x.duration))
                .collect()
        })
        .unwrap_or_default()
}

/// A DIMINISHED chill is short in BOTH of its effects. The rider inherits the
/// face's scaled duration, not the constant it was built from.
#[test]
fn a_diminished_chill_shortens_its_rider_too() {
    let mut app = apply_harness();
    let victim = dr_victim(&mut app);

    // First application is undiminished: both effects run the full duration.
    proc_chill(&mut app, victim);
    for (effect, duration) in durations(&app, victim) {
        assert_eq!(
            duration, FROST_ARMOR_PROC_DURATION,
            "{effect:?} should land at full duration while DR is fresh"
        );
    }

    // Second lands at 50%. The face carries the `Slows` DR category; the rider
    // carries none, which is exactly why it needs the face to hand it a
    // duration rather than keeping its own.
    proc_chill(&mut app, victim);
    let after = durations(&app, victim);
    assert_eq!(after.len(), 2, "the chill is two effects: {after:?}");

    let face = after
        .iter()
        .find(|(e, _)| *e == AuraType::MovementSpeedSlow)
        .expect("face");
    let rider = after
        .iter()
        .find(|(e, _)| *e == AuraType::AttackSpeedSlow)
        .expect("rider");

    assert!(
        face.1 < FROST_ARMOR_PROC_DURATION,
        "the second chill must be diminished, or this proves nothing: {face:?}"
    );
    assert_eq!(
        rider.1, face.1,
        "the rider must inherit the face's POST-DR duration ({}s), not its own \
         undiminished {FROST_ARMOR_PROC_DURATION}s — a rider that outlives its \
         face is a debuff with no icon and nothing to dispel",
        face.1
    );
}

/// A chill the target is IMMUNE to lands nothing at all — not even the half
/// that carries no diminishing-returns category of its own.
#[test]
fn a_dr_immune_chill_leaves_no_rider_behind() {
    let mut app = apply_harness();
    let victim = dr_victim(&mut app);

    // 100% -> 50% -> 25%, and the fourth is resisted outright.
    for _ in 0..3 {
        proc_chill(&mut app, victim);
    }
    assert!(
        app.world()
            .entity(victim)
            .get::<DRTracker>()
            .expect("tracker")
            .is_immune(DRCategory::Slows),
        "the target must be DR-immune by now, or the rejection is never exercised"
    );

    // Let the third chill expire so the vector is empty, then proc into immunity.
    if let Some(mut auras) = app.world_mut().entity_mut(victim).get_mut::<ActiveAuras>() {
        auras.auras.clear();
    }
    proc_chill(&mut app, victim);

    assert_eq!(
        durations(&app, victim),
        vec![],
        "the face was resisted, so NOTHING should have landed — a rider alone \
         is the exact defect this compound exists to make unreachable"
    );
}

/// The debuff the catalog describes and the debuff the simulation applies are
/// the same debuff.
///
/// Two paths produce it: `frost_armor_chill_auras` (the whole thing, which the
/// encyclopedia page and these tests read) and `compound_riders` (what
/// `apply_pending_auras` pulls in behind the face). A rider present in one and
/// not the other is a page that promises an effect the proc never lands, or a
/// proc that lands one the page never mentions.
#[test]
fn the_catalog_and_the_apply_path_agree_on_the_riders() {
    let [_, rider] = frost_armor_chill_auras();
    let applied = compound_riders(CompoundDebuff::FrostArmorChill);

    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].effect_type, rider.effect_type);
    assert_eq!(applied[0].magnitude, rider.magnitude);
    assert_eq!(applied[0].ability_name, rider.ability_name);
    assert_eq!(applied[0].compound, rider.compound);
}

/// The apply site hands out both halves bound to the same debuff. Pinned here
/// as well as in the components' own tests because this file is where a reader
/// looking for "what makes the dispel take both" lands.
#[test]
fn the_proc_applies_one_debuff_with_two_effects() {
    let [face, rider] = frost_armor_chill_auras();
    assert_eq!(face.compound, Some(CompoundDebuff::FrostArmorChill));
    assert_eq!(rider.compound, face.compound);
    assert_eq!(face.ability_name, rider.ability_name);
    assert!(!face.is_compound_rider());
    assert!(rider.is_compound_rider());
    assert!(face.can_be_dispelled(), "the chill comes off by its face");
    assert!(
        !rider.can_be_dispelled(),
        "the rider is not independently dispellable — widening \
         is_magic_dispellable for AttackSpeedSlow was the alternative, and it \
         would have made Demoralizing Shout dispellable as magic too"
    );
}
