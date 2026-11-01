//! The damage pipeline's talent mechanisms (AS-408): damage dealt and taken
//! modifiers, percentage armor, damage transfer, killing-blow saves and
//! reflected damage. Each mechanism is pinned in the one funnel
//! (`combat_core::apply_damage_with_absorb`) as a pair — the case and its
//! control with the one condition removed — and the follow-ups it queues are
//! landed by `process_damage_follow_ups` in a world of their own.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::{CombatLog, StructuredEventData};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::combat_core::{
    apply_damage_with_absorb, damage_dealt_multiplier, effective_armor,
    process_damage_follow_ups, DamageFollowUp, DamageFollowUps, DealtMods, Hit, HitKind,
    KillingBlowPrevented, PendingDamageFollowUp, TransferReceiver, CHEAT_DEATH_NAME,
};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraDamageMods, AuraStacks, AuraType, Combatant,
    DamageScope, Pet, PetType, StackScope,
};
use arenasim::states::play_match::talent_mechanics::{TalentMechanic, TalentMechanics};

// ---------------------------------------------------------------------------
// Scaffolding
// ---------------------------------------------------------------------------

/// A Warrior with `health` and no armor or resistance, so a hit's number is
/// the pipeline's alone.
fn bare(health: f32) -> Combatant {
    let mut c = Combatant::new(1, 0, CharacterClass::Warrior);
    c.max_health = health;
    c.current_health = health;
    c.armor = 0.0;
    c.fire_resistance = 0.0;
    c.frost_resistance = 0.0;
    c.shadow_resistance = 0.0;
    c.arcane_resistance = 0.0;
    c.nature_resistance = 0.0;
    c.holy_resistance = 0.0;
    c
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

fn scoped(kind: AuraType, magnitude: f32, scope: DamageScope) -> Aura {
    Aura {
        damage_mods: AuraDamageMods {
            scope: Some(scope),
            ..Default::default()
        },
        ..aura(kind, magnitude)
    }
}

fn auras(list: Vec<Aura>) -> ActiveAuras {
    ActiveAuras { auras: list }
}

/// Two distinct entities, for hits that name an attacker and a victim.
fn entities() -> (Entity, Entity, Entity) {
    let mut world = World::new();
    (
        world.spawn_empty().id(),
        world.spawn_empty().id(),
        world.spawn_empty().id(),
    )
}

/// Land `hit` on `target` and return (to health, absorbed, follow-ups).
fn land(
    hit: Hit,
    target: &mut Combatant,
    target_auras: Option<&mut ActiveAuras>,
) -> (f32, f32, Vec<DamageFollowUp>) {
    let mut follow_ups = DamageFollowUps::default();
    let (to_health, absorbed) = apply_damage_with_absorb(
        hit,
        target,
        target_auras,
        &mut follow_ups,
        &mut CombatLog::default(),
    );
    (to_health, absorbed, follow_ups.as_slice().to_vec())
}

fn direct(amount: f32, school: SpellSchool, attacker: Option<Entity>, victim: Entity) -> Hit<'static> {
    Hit::new(amount, school, "Probe", attacker, None, victim)
}

// ---------------------------------------------------------------------------
// Damage dealt
// ---------------------------------------------------------------------------

#[test]
fn a_damage_dealt_increase_raises_hits_in_its_scope_only() {
    let (a, v, _) = entities();
    let attacker = auras(vec![scoped(
        AuraType::DamageDealtIncrease,
        0.15,
        DamageScope::School(SpellSchool::Shadow),
    )]);
    let shadow = Hit::new(100.0, SpellSchool::Shadow, "Probe", Some(a), Some(&attacker), v);
    let fire = Hit::new(100.0, SpellSchool::Fire, "Probe", Some(a), Some(&attacker), v);
    assert_eq!(land(shadow, &mut bare(500.0), None).0, 115.0);
    assert_eq!(land(fire, &mut bare(500.0), None).0, 100.0, "out of scope");
    // Control: no aura on the attacker.
    let plain = Hit::new(100.0, SpellSchool::Shadow, "Probe", Some(a), None, v);
    assert_eq!(land(plain, &mut bare(500.0), None).0, 100.0);
}

/// The stacking simplification: same-kind effects do not compound.
#[test]
fn only_the_strongest_damage_dealt_increase_applies() {
    let both = auras(vec![
        aura(AuraType::DamageDealtIncrease, 0.10),
        aura(AuraType::DamageDealtIncrease, 0.15),
    ]);
    assert_eq!(damage_dealt_multiplier(Some(&both), SpellSchool::Fire), 1.15);
    assert_eq!(DealtMods::of(Some(&both)).multiplier(SpellSchool::Fire), 1.15);
    assert_eq!(damage_dealt_multiplier(None, SpellSchool::Fire), 1.0);
    // The read-ahead copy honours scope as the live read does.
    let shadow_only = auras(vec![scoped(
        AuraType::DamageDealtIncrease,
        0.15,
        DamageScope::Spells,
    )]);
    let mods = DealtMods::of(Some(&shadow_only));
    assert_eq!(mods.multiplier(SpellSchool::Shadow), 1.15);
    assert_eq!(mods.multiplier(SpellSchool::Physical), 1.0);
}

// ---------------------------------------------------------------------------
// Damage taken
// ---------------------------------------------------------------------------

#[test]
fn the_strongest_damage_taken_increase_in_scope_applies_per_stack() {
    let (_, v, _) = entities();
    // Hemorrhage's shape: +2% Physical a stack, at five stacks.
    let mut hemo = scoped(
        AuraType::DamageTakenIncrease,
        0.02,
        DamageScope::School(SpellSchool::Physical),
    );
    hemo.stacks = Some(AuraStacks {
        count: 5,
        max: 5,
        scope: StackScope::PerTarget,
    });
    let rider = aura(AuraType::DamageTakenIncrease, 0.05);
    let mut target_auras = auras(vec![hemo, rider]);
    let (to_health, ..) = land(
        direct(100.0, SpellSchool::Physical, None, v),
        &mut bare(500.0),
        Some(&mut target_auras),
    );
    assert!((to_health - 110.0).abs() < 1e-3, "5 stacks of 2% beat 5%: {to_health}");
    // A Fire hit: only the unscoped 5% reaches it.
    let (to_health, ..) = land(
        direct(100.0, SpellSchool::Fire, None, v),
        &mut bare(500.0),
        Some(&mut target_auras),
    );
    assert!((to_health - 105.0).abs() < 1e-3, "{to_health}");
    // Control.
    assert_eq!(
        land(direct(100.0, SpellSchool::Fire, None, v), &mut bare(500.0), None).0,
        100.0
    );
}

/// Shadow Vulnerability's shape: four hits or its duration. A direct hit
/// spends a charge; a periodic tick neither spends one nor is raised.
#[test]
fn a_charged_vulnerability_is_spent_by_direct_hits_only() {
    let (_, v, _) = entities();
    let mut vuln = scoped(
        AuraType::DamageTakenIncrease,
        0.10,
        DamageScope::School(SpellSchool::Shadow),
    );
    vuln.damage_mods.charges = Some(2);
    let mut target_auras = auras(vec![vuln]);
    let mut target = bare(1000.0);

    let tick = direct(100.0, SpellSchool::Shadow, None, v).of_kind(HitKind::Periodic);
    let (dealt, ..) = land(tick, &mut target, Some(&mut target_auras));
    assert_eq!(dealt, 100.0, "a tick is not raised");
    assert_eq!(target_auras.auras[0].damage_mods.charges, Some(2));

    let (dealt, ..) = land(direct(100.0, SpellSchool::Shadow, None, v), &mut target, Some(&mut target_auras));
    assert!((dealt - 110.0).abs() < 1e-3);
    assert_eq!(target_auras.auras[0].damage_mods.charges, Some(1));

    let (dealt, ..) = land(direct(100.0, SpellSchool::Shadow, None, v), &mut target, Some(&mut target_auras));
    assert!((dealt - 110.0).abs() < 1e-3);
    assert!(target_auras.auras.is_empty(), "the last charge ends it");

    let (dealt, ..) = land(direct(100.0, SpellSchool::Shadow, None, v), &mut target, Some(&mut target_auras));
    assert_eq!(dealt, 100.0, "spent");
}

/// Shadowform's shape: -15% Physical taken, nothing else.
#[test]
fn a_scoped_damage_taken_reduction_reaches_only_its_school() {
    let (_, v, _) = entities();
    let mut target_auras = auras(vec![scoped(
        AuraType::DamageTakenReduction,
        0.15,
        DamageScope::School(SpellSchool::Physical),
    )]);
    let (phys, ..) = land(direct(100.0, SpellSchool::Physical, None, v), &mut bare(500.0), Some(&mut target_auras));
    assert!((phys - 85.0).abs() < 1e-3);
    let (shadow, ..) = land(direct(100.0, SpellSchool::Shadow, None, v), &mut bare(500.0), Some(&mut target_auras));
    assert_eq!(shadow, 100.0);
    // "Spell damage" (Master Demonologist): every magic school, not Physical.
    let mut spells = auras(vec![scoped(AuraType::DamageTakenReduction, 0.04, DamageScope::Spells)]);
    let (shadow, ..) = land(direct(100.0, SpellSchool::Shadow, None, v), &mut bare(500.0), Some(&mut spells));
    assert!((shadow - 96.0).abs() < 1e-3);
    let (phys, ..) = land(direct(100.0, SpellSchool::Physical, None, v), &mut bare(500.0), Some(&mut spells));
    assert_eq!(phys, 100.0);
}

// ---------------------------------------------------------------------------
// Percentage armor
// ---------------------------------------------------------------------------

#[test]
fn percentage_armor_auras_scale_the_armor_a_hit_meets() {
    let mut target = bare(500.0);
    target.armor = 1000.0;
    assert_eq!(effective_armor(&target, None), 1000.0);
    let expose = auras(vec![aura(AuraType::ArmorPercentReduction, 0.5)]);
    assert_eq!(effective_armor(&target, Some(&expose)), 500.0);
    let inspiration = auras(vec![aura(AuraType::ArmorPercentIncrease, 0.12)]);
    assert!((effective_armor(&target, Some(&inspiration)) - 1120.0).abs() < 1e-3);
    // Strongest of each kind; the kinds multiply.
    let all = auras(vec![
        aura(AuraType::ArmorPercentReduction, 0.2),
        aura(AuraType::ArmorPercentReduction, 0.5),
        aura(AuraType::ArmorPercentIncrease, 0.12),
    ]);
    assert!((effective_armor(&target, Some(&all)) - 560.0).abs() < 1e-3);
    // The armor stat itself is never written.
    assert_eq!(target.armor, 1000.0);

    // And the funnel reads it: Expose Armor makes a Physical hit land harder.
    let (_, v, _) = entities();
    let mut exposed = auras(vec![aura(AuraType::ArmorPercentReduction, 0.5)]);
    let (with, ..) = land(direct(100.0, SpellSchool::Physical, None, v), &mut target.clone(), Some(&mut exposed));
    let (without, ..) = land(direct(100.0, SpellSchool::Physical, None, v), &mut target.clone(), None);
    assert!((without - 100.0 * (1.0 - 1000.0 / 6500.0)).abs() < 1e-3);
    assert!((with - 100.0 * (1.0 - 500.0 / 6000.0)).abs() < 1e-3);
}

// ---------------------------------------------------------------------------
// Damage transfer
// ---------------------------------------------------------------------------

/// Blessing of Sacrifice's shape: 30% to the aura's caster, after the
/// holder's mitigation and before its absorbs.
#[test]
fn a_transfer_aura_sends_its_share_to_its_caster() {
    let (attacker, holder, paladin) = entities();
    let mut sac = aura(AuraType::DamageTransfer, 0.25);
    sac.caster = Some(paladin);
    sac.ability_name = "Blessing of Sacrifice".into();
    let mut shield = aura(AuraType::Absorb, 50.0);
    shield.ability_name = "Shield".into();
    let mut target_auras = auras(vec![sac.clone(), shield]);
    let (to_health, absorbed, follow_ups) = land(
        direct(100.0, SpellSchool::Fire, Some(attacker), holder),
        &mut bare(500.0),
        Some(&mut target_auras),
    );
    assert_eq!((to_health, absorbed), (25.0, 50.0), "25 leaves before the shield");
    assert_eq!(
        follow_ups,
        vec![DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::Unit(paladin),
            amount: 25.0,
            dealer: Some(attacker),
            by: "Blessing of Sacrifice".into(),
        }]
    );
    // Control: no transfer aura.
    let (to_health, _, follow_ups) = land(
        direct(100.0, SpellSchool::Fire, Some(attacker), holder),
        &mut bare(500.0),
        None,
    );
    assert_eq!((to_health, follow_ups.len()), (100.0, 0));
    // A secondary hit is never transferred again.
    let mut target_auras = auras(vec![sac]);
    let reflected = direct(100.0, SpellSchool::Fire, Some(attacker), holder).of_kind(HitKind::Reflected);
    let (to_health, _, follow_ups) = land(reflected, &mut bare(500.0), Some(&mut target_auras));
    assert_eq!((to_health, follow_ups.len()), (100.0, 0));
}

/// Soul Link's shape: the holder's mechanic sends its share to its own pet.
#[test]
fn soul_link_sends_its_share_to_the_pet() {
    let (attacker, holder, _) = entities();
    let mut warlock = bare(500.0);
    warlock.talent_mechanics = TalentMechanics::of(&[TalentMechanic::SoulLink { share: 0.12 }]);
    let (to_health, _, follow_ups) = land(
        direct(100.0, SpellSchool::Fire, Some(attacker), holder),
        &mut warlock,
        None,
    );
    assert!((to_health - 88.0).abs() < 1e-3);
    assert!(matches!(
        follow_ups.as_slice(),
        [DamageFollowUp::Transfer { receiver: TransferReceiver::OwnPet, amount, by, .. }]
            if (*amount - 12.0).abs() < 1e-3 && by == "Soul Link"
    ));
}

// ---------------------------------------------------------------------------
// Killing-blow saves
// ---------------------------------------------------------------------------

#[test]
fn a_save_aura_prevents_the_killing_blow_and_is_spent() {
    let (attacker, holder, priest) = entities();
    let mut spirit = aura(AuraType::KillingBlowSave, 0.30);
    spirit.caster = Some(priest);
    spirit.ability_name = "Guardian Spirit".into();
    // A blow that does not kill leaves the save in place.
    let mut target_auras = auras(vec![spirit.clone()]);
    let mut target = bare(500.0);
    target.current_health = 50.0;
    let (dealt, _, follow_ups) = land(direct(30.0, SpellSchool::Fire, Some(attacker), holder), &mut target, Some(&mut target_auras));
    assert_eq!((dealt, target.current_health, follow_ups.len()), (30.0, 20.0, 0));
    assert_eq!(target_auras.auras.len(), 1);
    // The killing blow is prevented: health stays, the save is spent and its
    // heal queued.
    let (dealt, _, follow_ups) = land(direct(80.0, SpellSchool::Fire, Some(attacker), holder), &mut target, Some(&mut target_auras));
    assert_eq!((dealt, target.current_health), (0.0, 20.0));
    assert!(target_auras.auras.is_empty());
    assert!(matches!(
        follow_ups.as_slice(),
        [DamageFollowUp::Saved { by, caster: Some(c), heal_fraction: Some(f), .. }]
            if by == "Guardian Spirit" && *c == priest && *f == 0.30
    ));
    // Spent: the next killing blow kills.
    land(direct(80.0, SpellSchool::Fire, Some(attacker), holder), &mut target, Some(&mut target_auras));
    assert_eq!(target.current_health, 0.0);
}

#[test]
fn cheat_death_leaves_the_rogue_at_its_floor_once_a_match() {
    let (attacker, holder, _) = entities();
    let mut rogue = bare(400.0);
    rogue.current_health = 100.0;
    rogue.talent_mechanics = TalentMechanics::of(&[TalentMechanic::CheatDeath {
        health: 0.10,
        reduction: 0.90,
        duration: 3.0,
    }]);
    let mut target_auras = auras(vec![]);
    let (dealt, _, follow_ups) = land(direct(300.0, SpellSchool::Fire, Some(attacker), holder), &mut rogue, Some(&mut target_auras));
    assert_eq!((dealt, rogue.current_health), (60.0, 40.0), "left at 10% of 400");
    assert!(rogue.killing_blow_save_spent);
    assert_eq!(target_auras.auras.len(), 1);
    let cut = &target_auras.auras[0];
    assert_eq!(
        (cut.effect_type, cut.magnitude, cut.duration, cut.ability_name.as_str()),
        (AuraType::DamageTakenReduction, 0.90, 3.0, CHEAT_DEATH_NAME)
    );
    assert!(matches!(
        follow_ups.as_slice(),
        [DamageFollowUp::Saved { heal_fraction: None, aura: None, .. }]
    ));
    // The cut applies: a 100 hit lands for 10.
    let (dealt, ..) = land(direct(100.0, SpellSchool::Fire, Some(attacker), holder), &mut rogue, Some(&mut target_auras));
    assert!((dealt - 10.0).abs() < 1e-3);
    // Once a match: the next killing blow kills.
    target_auras.auras.clear();
    land(direct(300.0, SpellSchool::Fire, Some(attacker), holder), &mut rogue, Some(&mut target_auras));
    assert_eq!(rogue.current_health, 0.0);

    // Below the floor already: the blow is spent and the Rogue keeps what it
    // had — never a health raise.
    let mut low = bare(400.0);
    low.current_health = 20.0;
    low.talent_mechanics = rogue.talent_mechanics.clone();
    let (dealt, ..) = land(direct(300.0, SpellSchool::Fire, Some(attacker), holder), &mut low, None);
    assert_eq!((dealt, low.current_health), (0.0, 20.0));

    // Control: no Cheat Death, the blow kills.
    let mut plain = bare(400.0);
    plain.current_health = 100.0;
    land(direct(300.0, SpellSchool::Fire, Some(attacker), holder), &mut plain, None);
    assert_eq!(plain.current_health, 0.0);
}

// ---------------------------------------------------------------------------
// Reflect
// ---------------------------------------------------------------------------

/// Reflective Shield's shape: 30% of what the shield soaks, to the attacker,
/// credited to the shield's caster.
#[test]
fn a_reflecting_shield_returns_its_share_of_what_it_soaks() {
    let (attacker, holder, priest) = entities();
    let mut shield = aura(AuraType::Absorb, 50.0);
    shield.caster = Some(priest);
    shield.spell_school = Some(SpellSchool::Holy);
    shield.ability_name = "Power Word: Shield".into();
    shield.damage_mods.reflect_absorbed = Some(0.25);
    let mut target_auras = auras(vec![shield.clone()]);
    let (_, absorbed, follow_ups) = land(
        direct(100.0, SpellSchool::Fire, Some(attacker), holder),
        &mut bare(500.0),
        Some(&mut target_auras),
    );
    assert_eq!(absorbed, 50.0);
    assert_eq!(
        follow_ups,
        vec![DamageFollowUp::Reflect {
            to: attacker,
            from: holder,
            dealer: Some(priest),
            amount: 12.5,
            school: SpellSchool::Holy,
            source: "Power Word: Shield".into(),
        }]
    );
    // Control: a plain shield reflects nothing.
    shield.damage_mods.reflect_absorbed = None;
    let mut target_auras = auras(vec![shield]);
    let (_, absorbed, follow_ups) = land(
        direct(100.0, SpellSchool::Fire, Some(attacker), holder),
        &mut bare(500.0),
        Some(&mut target_auras),
    );
    assert_eq!((absorbed, follow_ups.len()), (50.0, 0));
}

/// Retribution Aura's shape: a flat Holy hit back at a melee attacker.
#[test]
fn a_reflect_aura_answers_melee_hits_only_and_never_reflects_again() {
    let (attacker, holder, paladin) = entities();
    let mut ret = aura(AuraType::DamageReflect, 6.0);
    ret.caster = Some(paladin);
    ret.spell_school = Some(SpellSchool::Holy);
    ret.ability_name = "Retribution Aura".into();
    let mut target_auras = auras(vec![ret]);
    let melee = direct(50.0, SpellSchool::Physical, Some(attacker), holder).of_kind(HitKind::Melee);
    let (.., follow_ups) = land(melee, &mut bare(500.0), Some(&mut target_auras));
    assert_eq!(
        follow_ups,
        vec![DamageFollowUp::Reflect {
            to: attacker,
            from: holder,
            dealer: Some(paladin),
            amount: 6.0,
            school: SpellSchool::Holy,
            source: "Retribution Aura".into(),
        }]
    );
    // A spell is not a melee strike.
    let spell = direct(50.0, SpellSchool::Physical, Some(attacker), holder);
    assert!(land(spell, &mut bare(500.0), Some(&mut target_auras)).2.is_empty());
    // Reflected damage never reflects again.
    let back = direct(50.0, SpellSchool::Physical, Some(attacker), holder).of_kind(HitKind::Reflected);
    assert!(land(back, &mut bare(500.0), Some(&mut target_auras)).2.is_empty());
    // Control: no aura, no reflect.
    let melee = direct(50.0, SpellSchool::Physical, Some(attacker), holder).of_kind(HitKind::Melee);
    assert!(land(melee, &mut bare(500.0), None).2.is_empty());
}

/// Transferred damage cannot be prevented: it skips its receiver's immunity,
/// mitigation and absorbs.
#[test]
fn transferred_damage_cannot_be_prevented() {
    let (_, v, _) = entities();
    let mut target = bare(500.0);
    target.armor = 3000.0;
    let mut target_auras = auras(vec![
        aura(AuraType::DamageImmunity, 1.0),
        aura(AuraType::Absorb, 100.0),
        aura(AuraType::DamageTakenReduction, 0.5),
    ]);
    let hit = direct(30.0, SpellSchool::Physical, None, v).of_kind(HitKind::Transferred);
    let (dealt, absorbed, _) = land(hit, &mut target, Some(&mut target_auras));
    assert_eq!((dealt, absorbed, target.current_health), (30.0, 0.0, 470.0));
    // Control: the same hit as a direct one is blocked.
    let hit = direct(30.0, SpellSchool::Physical, None, v);
    assert_eq!(land(hit, &mut target, Some(&mut target_auras)).0, 0.0);
}

// ---------------------------------------------------------------------------
// Follow-ups, landed
// ---------------------------------------------------------------------------

struct Units {
    world: World,
    holder: Entity,
    receiver: Entity,
    attacker: Entity,
}

/// A holder (team 1 Priest), a receiver (team 1 Paladin) and an attacker
/// (team 2 Rogue), each with an empty aura list.
fn units() -> Units {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    let mut priest = bare(400.0);
    priest.team = 1;
    priest.slot = 0;
    priest.class = CharacterClass::Priest;
    let mut paladin = bare(400.0);
    paladin.team = 1;
    paladin.slot = 1;
    paladin.class = CharacterClass::Paladin;
    let mut rogue = Combatant::new(2, 0, CharacterClass::Rogue);
    rogue.max_health = 100.0;
    rogue.current_health = 100.0;
    rogue.armor = 0.0;
    let holder = world.spawn((priest, ActiveAuras::default())).id();
    let receiver = world.spawn((paladin, ActiveAuras::default())).id();
    let attacker = world.spawn((rogue, ActiveAuras::default())).id();
    Units {
        world,
        holder,
        receiver,
        attacker,
    }
}

fn run(world: &mut World, follow_up: DamageFollowUp) {
    world.spawn(PendingDamageFollowUp(follow_up));
    world.run_system_once(process_damage_follow_ups).expect("ran");
}

fn health(world: &World, e: Entity) -> f32 {
    world.get::<Combatant>(e).unwrap().current_health
}

fn deaths(world: &World) -> Vec<(String, Option<String>)> {
    world
        .resource::<CombatLog>()
        .entries
        .iter()
        .filter_map(|e| match &e.structured_data {
            Some(StructuredEventData::Death { victim, killer }) => Some((victim.clone(), killer.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_transfer_lands_on_its_receiver_and_credits_the_dealer() {
    let Units { mut world, holder, receiver, attacker } = units();
    run(
        &mut world,
        DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::Unit(receiver),
            amount: 30.0,
            dealer: Some(attacker),
            by: "Blessing of Sacrifice".into(),
        },
    );
    assert_eq!(health(&world, receiver), 370.0);
    assert_eq!(health(&world, holder), 400.0);
    assert_eq!(world.get::<Combatant>(attacker).unwrap().damage_dealt, 30.0);
    assert!(
        world.query::<&PendingDamageFollowUp>().iter(&world).next().is_none(),
        "the follow-up is spent"
    );
}

/// A transfer never kills its receiver: a share it cannot bear stays with the
/// holder, and the transfer aura ends.
#[test]
fn an_unbearable_transfer_returns_to_the_holder_and_ends_the_aura() {
    let Units { mut world, holder, receiver, attacker } = units();
    world.get_mut::<Combatant>(receiver).unwrap().current_health = 20.0;
    let mut sac = aura(AuraType::DamageTransfer, 0.3);
    sac.caster = Some(receiver);
    world.get_mut::<ActiveAuras>(holder).unwrap().auras.push(sac);
    run(
        &mut world,
        DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::Unit(receiver),
            amount: 30.0,
            dealer: Some(attacker),
            by: "Blessing of Sacrifice".into(),
        },
    );
    assert_eq!(health(&world, receiver), 20.0, "the receiver is spared");
    assert_eq!(health(&world, holder), 370.0, "the holder keeps the share");
    assert!(world.get::<ActiveAuras>(holder).unwrap().auras.is_empty(), "the transfer ends");
    assert!(deaths(&world).is_empty());
}

#[test]
fn a_soul_link_share_finds_the_holders_pet() {
    let Units { mut world, holder, attacker, .. } = units();
    let mut felhunter = bare(150.0);
    felhunter.team = 1;
    felhunter.slot = 10;
    felhunter.class = CharacterClass::Warlock;
    let pet = world
        .spawn((felhunter, ActiveAuras::default(), Pet { owner: holder, pet_type: PetType::Felhunter }))
        .id();
    run(
        &mut world,
        DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::OwnPet,
            amount: 12.0,
            dealer: Some(attacker),
            by: "Soul Link".into(),
        },
    );
    assert_eq!(health(&world, pet), 138.0);
    assert_eq!(health(&world, holder), 400.0);
}

/// Reflected damage is damage: it reveals a stealthed attacker, can kill it,
/// and the death is logged with the reflect's dealer as the killer.
#[test]
fn a_reflect_reveals_and_can_kill_the_attacker() {
    let Units { mut world, holder, receiver, attacker } = units();
    assert!(world.get::<Combatant>(attacker).unwrap().stealthed);
    run(
        &mut world,
        DamageFollowUp::Reflect {
            to: attacker,
            from: holder,
            dealer: Some(receiver),
            amount: 150.0,
            school: SpellSchool::Holy,
            source: "Retribution Aura".into(),
        },
    );
    let rogue = world.get::<Combatant>(attacker).unwrap();
    assert!(!rogue.stealthed, "the reflect revealed it");
    assert!(rogue.is_dead);
    assert_eq!(
        deaths(&world),
        vec![("Team 2 Rogue #1".to_string(), Some("Team 1 Paladin #2".to_string()))]
    );
    assert_eq!(world.get::<Combatant>(receiver).unwrap().damage_dealt, 100.0);
}

/// A reflected hit on a unit that itself reflects is not reflected back.
#[test]
fn a_reflect_on_a_reflecting_unit_stops_there() {
    let Units { mut world, holder, receiver, attacker } = units();
    let mut ret = aura(AuraType::DamageReflect, 6.0);
    ret.caster = Some(attacker);
    world.get_mut::<ActiveAuras>(attacker).unwrap().auras.push(ret);
    run(
        &mut world,
        DamageFollowUp::Reflect {
            to: attacker,
            from: holder,
            dealer: Some(receiver),
            amount: 10.0,
            school: SpellSchool::Holy,
            source: "Retribution Aura".into(),
        },
    );
    assert_eq!(health(&world, holder), 400.0);
    assert_eq!(health(&world, receiver), 400.0);
    assert_eq!(health(&world, attacker), 90.0);
}

/// A save's heal lands through `apply_healing`: dampening cuts it, and the
/// "Saved" marker is spawned for the shared cue.
#[test]
fn a_saves_heal_goes_through_apply_healing() {
    let Units { mut world, holder, receiver, .. } = units();
    world.get_mut::<Combatant>(holder).unwrap().current_health = 10.0;
    world.resource_mut::<ArenaDampening>().reduction = 0.5;
    run(
        &mut world,
        DamageFollowUp::Saved {
            holder,
            by: "Guardian Spirit".into(),
            caster: Some(receiver),
            heal_fraction: Some(0.25),
            aura: None,
        },
    );
    // 25% of 400 is 100, halved by dampening.
    assert_eq!(health(&world, holder), 60.0);
    assert_eq!(world.get::<Combatant>(receiver).unwrap().healing_done, 50.0);
    let marks: Vec<KillingBlowPrevented> = world
        .query::<&KillingBlowPrevented>()
        .iter(&world)
        .cloned()
        .collect();
    assert_eq!(
        marks,
        vec![KillingBlowPrevented { holder, by: "Guardian Spirit".into() }]
    );
}
