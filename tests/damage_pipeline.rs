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
    apply_damage_with_absorb, damage_dealt_multiplier, effective_armor, end_transfers_to_the_dead,
    process_damage_follow_ups, share_to_health, DamageFollowUp, DamageFollowUps, DealtMods, Hit,
    HitKind, KillingBlowPrevented, PendingDamageFollowUp, TransferReceiver, CHEAT_DEATH_NAME,
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

fn direct(
    amount: f32,
    school: SpellSchool,
    attacker: Option<Entity>,
    victim: Entity,
) -> Hit<'static> {
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
    let shadow = Hit::new(
        100.0,
        SpellSchool::Shadow,
        "Probe",
        Some(a),
        Some(&attacker),
        v,
    );
    let fire = Hit::new(
        100.0,
        SpellSchool::Fire,
        "Probe",
        Some(a),
        Some(&attacker),
        v,
    );
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
    assert_eq!(
        damage_dealt_multiplier(Some(&both), SpellSchool::Fire),
        1.15
    );
    assert_eq!(
        DealtMods::of(Some(&both)).multiplier(SpellSchool::Fire),
        1.15
    );
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
        one_target: false,
    });
    let rider = aura(AuraType::DamageTakenIncrease, 0.05);
    let mut target_auras = auras(vec![hemo, rider]);
    let (to_health, ..) = land(
        direct(100.0, SpellSchool::Physical, None, v),
        &mut bare(500.0),
        Some(&mut target_auras),
    );
    assert!(
        (to_health - 110.0).abs() < 1e-3,
        "5 stacks of 2% beat 5%: {to_health}"
    );
    // A Fire hit: only the unscoped 5% reaches it.
    let (to_health, ..) = land(
        direct(100.0, SpellSchool::Fire, None, v),
        &mut bare(500.0),
        Some(&mut target_auras),
    );
    assert!((to_health - 105.0).abs() < 1e-3, "{to_health}");
    // Control.
    assert_eq!(
        land(
            direct(100.0, SpellSchool::Fire, None, v),
            &mut bare(500.0),
            None
        )
        .0,
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

    let (dealt, ..) = land(
        direct(100.0, SpellSchool::Shadow, None, v),
        &mut target,
        Some(&mut target_auras),
    );
    assert!((dealt - 110.0).abs() < 1e-3);
    assert_eq!(target_auras.auras[0].damage_mods.charges, Some(1));

    let (dealt, ..) = land(
        direct(100.0, SpellSchool::Shadow, None, v),
        &mut target,
        Some(&mut target_auras),
    );
    assert!((dealt - 110.0).abs() < 1e-3);
    assert!(target_auras.auras.is_empty(), "the last charge ends it");

    let (dealt, ..) = land(
        direct(100.0, SpellSchool::Shadow, None, v),
        &mut target,
        Some(&mut target_auras),
    );
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
    let (phys, ..) = land(
        direct(100.0, SpellSchool::Physical, None, v),
        &mut bare(500.0),
        Some(&mut target_auras),
    );
    assert!((phys - 85.0).abs() < 1e-3);
    let (shadow, ..) = land(
        direct(100.0, SpellSchool::Shadow, None, v),
        &mut bare(500.0),
        Some(&mut target_auras),
    );
    assert_eq!(shadow, 100.0);
    // "Spell damage" (Master Demonologist): every magic school, not Physical.
    let mut spells = auras(vec![scoped(
        AuraType::DamageTakenReduction,
        0.04,
        DamageScope::Spells,
    )]);
    let (shadow, ..) = land(
        direct(100.0, SpellSchool::Shadow, None, v),
        &mut bare(500.0),
        Some(&mut spells),
    );
    assert!((shadow - 96.0).abs() < 1e-3);
    let (phys, ..) = land(
        direct(100.0, SpellSchool::Physical, None, v),
        &mut bare(500.0),
        Some(&mut spells),
    );
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
    let (with, ..) = land(
        direct(100.0, SpellSchool::Physical, None, v),
        &mut target.clone(),
        Some(&mut exposed),
    );
    let (without, ..) = land(
        direct(100.0, SpellSchool::Physical, None, v),
        &mut target.clone(),
        None,
    );
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
    assert_eq!(
        (to_health, absorbed),
        (25.0, 50.0),
        "25 leaves before the shield"
    );
    assert_eq!(
        follow_ups,
        vec![DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::Unit(paladin),
            amount: 25.0,
            school: SpellSchool::Fire,
            preventable: true,
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
    let reflected =
        direct(100.0, SpellSchool::Fire, Some(attacker), holder).of_kind(HitKind::Reflected);
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
    let (dealt, _, follow_ups) = land(
        direct(30.0, SpellSchool::Fire, Some(attacker), holder),
        &mut target,
        Some(&mut target_auras),
    );
    assert_eq!(
        (dealt, target.current_health, follow_ups.len()),
        (30.0, 20.0, 0)
    );
    assert_eq!(target_auras.auras.len(), 1);
    // The killing blow is prevented: health stays, the save is spent and its
    // heal queued.
    let (dealt, _, follow_ups) = land(
        direct(80.0, SpellSchool::Fire, Some(attacker), holder),
        &mut target,
        Some(&mut target_auras),
    );
    assert_eq!((dealt, target.current_health), (0.0, 20.0));
    assert!(target_auras.auras.is_empty());
    assert!(matches!(
        follow_ups.as_slice(),
        [DamageFollowUp::Saved { by, caster: Some(c), heal_fraction: Some(f), .. }]
            if by == "Guardian Spirit" && *c == priest && *f == 0.30
    ));
    // Spent: the next killing blow kills.
    land(
        direct(80.0, SpellSchool::Fire, Some(attacker), holder),
        &mut target,
        Some(&mut target_auras),
    );
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
    let (dealt, _, follow_ups) = land(
        direct(300.0, SpellSchool::Fire, Some(attacker), holder),
        &mut rogue,
        Some(&mut target_auras),
    );
    assert_eq!(
        (dealt, rogue.current_health),
        (60.0, 40.0),
        "left at 10% of 400"
    );
    assert!(rogue.killing_blow_save_spent);
    assert_eq!(target_auras.auras.len(), 1);
    let cut = &target_auras.auras[0];
    assert_eq!(
        (
            cut.effect_type,
            cut.magnitude,
            cut.duration,
            cut.ability_name.as_str()
        ),
        (AuraType::DamageTakenReduction, 0.90, 3.0, CHEAT_DEATH_NAME)
    );
    assert!(matches!(
        follow_ups.as_slice(),
        [DamageFollowUp::Saved {
            heal_fraction: None,
            aura: None,
            ..
        }]
    ));
    // The cut applies: a 100 hit lands for 10.
    let (dealt, ..) = land(
        direct(100.0, SpellSchool::Fire, Some(attacker), holder),
        &mut rogue,
        Some(&mut target_auras),
    );
    assert!((dealt - 10.0).abs() < 1e-3);
    // Once a match: the next killing blow kills.
    target_auras.auras.clear();
    land(
        direct(300.0, SpellSchool::Fire, Some(attacker), holder),
        &mut rogue,
        Some(&mut target_auras),
    );
    assert_eq!(rogue.current_health, 0.0);

    // Below the floor already: the blow is spent and the Rogue keeps what it
    // had — never a health raise.
    let mut low = bare(400.0);
    low.current_health = 20.0;
    low.talent_mechanics = rogue.talent_mechanics.clone();
    let (dealt, ..) = land(
        direct(300.0, SpellSchool::Fire, Some(attacker), holder),
        &mut low,
        None,
    );
    assert_eq!((dealt, low.current_health), (0.0, 20.0));

    // Control: no Cheat Death, the blow kills.
    let mut plain = bare(400.0);
    plain.current_health = 100.0;
    land(
        direct(300.0, SpellSchool::Fire, Some(attacker), holder),
        &mut plain,
        None,
    );
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
    assert!(land(spell, &mut bare(500.0), Some(&mut target_auras))
        .2
        .is_empty());
    // Reflected damage never reflects again.
    let back =
        direct(50.0, SpellSchool::Physical, Some(attacker), holder).of_kind(HitKind::Reflected);
    assert!(land(back, &mut bare(500.0), Some(&mut target_auras))
        .2
        .is_empty());
    // Control: no aura, no reflect.
    let melee = direct(50.0, SpellSchool::Physical, Some(attacker), holder).of_kind(HitKind::Melee);
    assert!(land(melee, &mut bare(500.0), None).2.is_empty());
}

/// A transferred share meets its receiver's defences by its source (TBC):
/// Soul Link's cannot be prevented (tooltip 19028: "That damage cannot be
/// prevented"), so it skips its receiver's immunity, modifiers and absorbs;
/// Blessing of Sacrifice's is damage the receiver takes, so its immunity
/// blocks it and its modifiers and absorbs meet it. Neither meets the
/// receiver's armor, which the share met on the unit it left. Control: the
/// same hit struck directly is blocked.
#[test]
fn a_share_meets_its_receivers_defences_unless_its_source_is_unpreventable() {
    let (_, v, _) = entities();
    let defended = || {
        auras(vec![
            aura(AuraType::DamageImmunity, 1.0),
            aura(AuraType::Absorb, 100.0),
            aura(AuraType::DamageTakenReduction, 0.5),
        ])
    };
    let armored = || {
        let mut c = bare(500.0);
        c.armor = 3000.0;
        c
    };
    // Soul Link: all of it, through everything.
    let mut target = armored();
    let mut target_auras = defended();
    let hit =
        direct(30.0, SpellSchool::Physical, None, v).of_kind(HitKind::TransferredUnpreventable);
    let (dealt, absorbed, _) = land(hit, &mut target, Some(&mut target_auras));
    assert_eq!((dealt, absorbed, target.current_health), (30.0, 0.0, 470.0));
    // Blessing of Sacrifice: the immunity blocks it.
    let mut target = armored();
    let hit = direct(30.0, SpellSchool::Physical, None, v).of_kind(HitKind::Transferred);
    let (dealt, absorbed, _) = land(hit, &mut target, Some(&mut defended()));
    assert_eq!((dealt, absorbed, target.current_health), (0.0, 0.0, 500.0));
    // ...and with no immunity, the reduction halves it and the shield soaks
    // the half; no armor met it.
    let mut no_immunity = defended();
    no_immunity.auras.remove(0);
    let (dealt, absorbed, _) = land(hit, &mut target, Some(&mut no_immunity));
    assert_eq!((dealt, absorbed), (0.0, 15.0));
    assert_eq!(target.damage_mitigated_by_armor, 0.0);
    // Control: the same hit as a direct one is blocked.
    let hit = direct(30.0, SpellSchool::Physical, None, v);
    assert_eq!(land(hit, &mut target, Some(&mut defended())).0, 0.0);
}

/// A share its receiver takes, against each of the receiver's defences in
/// turn, each beside the plain receiver: a Cyclone and a damage immunity
/// block it, Divine Protection's reduction cuts it, a shield soaks it, and an
/// uncharged vulnerability raises it.
#[test]
fn each_receiver_defence_meets_a_sacrifice_share() {
    let (_, v, _) = entities();
    let share = || direct(30.0, SpellSchool::Physical, None, v).of_kind(HitKind::Transferred);
    let mut cyclone = aura(AuraType::Cyclone, 1.0);
    cyclone.ability_name = "Cyclone".into();
    let mut protection = aura(AuraType::DamageTakenReduction, 0.3);
    protection.ability_name = "Divine Protection".into();
    for (label, defence, expected) in [
        (
            "divine shield",
            Some(aura(AuraType::DamageImmunity, 1.0)),
            (0.0, 0.0),
        ),
        ("cyclone", Some(cyclone), (0.0, 0.0)),
        ("divine protection", Some(protection), (21.0, 0.0)),
        ("shield", Some(aura(AuraType::Absorb, 20.0)), (10.0, 20.0)),
        (
            "vulnerable",
            Some(aura(AuraType::DamageTakenIncrease, 0.5)),
            (45.0, 0.0),
        ),
        ("plain receiver", None, (30.0, 0.0)),
    ] {
        let mut receiver = auras(defence.into_iter().collect());
        let (dealt, absorbed, follow_ups) = land(share(), &mut bare(500.0), Some(&mut receiver));
        assert!(
            (dealt - expected.0).abs() < 1e-3 && absorbed == expected.1,
            "{label}: ({dealt}, {absorbed})"
        );
        assert!(follow_ups.is_empty(), "{label}");
    }
}

/// A share meets its receiver's modifiers, but not its sender's attacker's
/// damage-dealt increase, nor the receiver's armor or resistance (TBC: it met
/// them on the unit it left). A charged vulnerability neither raises it nor
/// spends a charge on it. Controls: the same hit struck directly meets each.
#[test]
fn a_share_skips_the_steps_its_holder_met() {
    let (_, v, _) = entities();
    let resistant = || {
        let mut c = bare(500.0);
        c.armor = 3000.0;
        c.fire_resistance = 150.0;
        c
    };
    for school in [SpellSchool::Physical, SpellSchool::Fire] {
        let share = direct(30.0, school, None, v)
            .with_dealt(2.0)
            .of_kind(HitKind::Transferred);
        assert_eq!(land(share, &mut resistant(), None).0, 30.0, "{school:?}");
        // Control: struck directly, the increase and the mitigation apply.
        let hit = direct(30.0, school, None, v).with_dealt(2.0);
        let dealt = land(hit, &mut resistant(), None).0;
        assert!(dealt > 30.0 && dealt < 60.0, "{school:?}: {dealt}");
    }
    let charged = || {
        let mut a = aura(AuraType::DamageTakenIncrease, 0.5);
        a.damage_mods.charges = Some(2);
        auras(vec![a])
    };
    let mut receiver = charged();
    let share = direct(30.0, SpellSchool::Physical, None, v).of_kind(HitKind::Transferred);
    assert_eq!(land(share, &mut bare(500.0), Some(&mut receiver)).0, 30.0);
    assert_eq!(
        receiver.auras[0].damage_mods.charges,
        Some(2),
        "no charge spent"
    );
    // Control: a direct hit is raised and spends one.
    let mut receiver = charged();
    let hit = direct(30.0, SpellSchool::Physical, None, v);
    assert_eq!(land(hit, &mut bare(500.0), Some(&mut receiver)).0, 45.0);
    assert_eq!(receiver.auras[0].damage_mods.charges, Some(1));
}

/// A share is never transferred onward, by a transfer aura or a Soul Link on
/// its receiver, and the receiver's reflecting shield that soaks it reflects
/// nothing — for both kinds of share. Control: a direct hit on the same
/// receiver transfers and reflects.
#[test]
fn a_share_is_never_transferred_onward_or_reflected() {
    let (attacker, v, other) = entities();
    let receiver = || {
        let mut sac = aura(AuraType::DamageTransfer, 0.3);
        sac.caster = Some(other);
        let mut shield = aura(AuraType::Absorb, 10.0);
        shield.damage_mods.reflect_absorbed = Some(0.5);
        auras(vec![sac, shield])
    };
    let linked = || {
        let mut c = bare(500.0);
        c.talent_mechanics = TalentMechanics::of(&[TalentMechanic::SoulLink { share: 0.2 }]);
        c
    };
    for kind in [HitKind::Transferred, HitKind::TransferredUnpreventable] {
        let share = direct(30.0, SpellSchool::Fire, Some(attacker), v).of_kind(kind);
        let (_, _, follow_ups) = land(share, &mut linked(), Some(&mut receiver()));
        assert!(follow_ups.is_empty(), "{kind:?}: {follow_ups:?}");
    }
    let hit = direct(30.0, SpellSchool::Fire, Some(attacker), v);
    let (_, _, follow_ups) = land(hit, &mut linked(), Some(&mut receiver()));
    assert!(
        matches!(
            follow_ups.as_slice(),
            [
                DamageFollowUp::Transfer { .. },
                DamageFollowUp::Reflect { .. }
            ]
        ),
        "{follow_ups:?}"
    );
}

/// The secondary hits, named: damage another hit caused, which never
/// transfers or reflects. An unpreventable share also skips the transfer and
/// absorb steps outright, so this is what keeps it secondary if that changes.
#[test]
fn the_secondary_hits_are_the_reflected_transferred_and_returned_ones() {
    let secondary: Vec<HitKind> = [
        HitKind::Direct,
        HitKind::Melee,
        HitKind::Periodic,
        HitKind::Reflected,
        HitKind::Transferred,
        HitKind::TransferredUnpreventable,
        HitKind::Returned,
    ]
    .into_iter()
    .filter(|k| k.is_secondary())
    .collect();
    assert_eq!(
        secondary,
        vec![
            HitKind::Reflected,
            HitKind::Transferred,
            HitKind::TransferredUnpreventable,
            HitKind::Returned
        ]
    );
    assert_eq!(HitKind::share(true), HitKind::Transferred);
    assert_eq!(HitKind::share(false), HitKind::TransferredUnpreventable);
}

/// The share carries its kind from its source: a transfer aura's (Blessing
/// of Sacrifice) is preventable, a Soul Link's is not; both carry the hit's
/// school.
#[test]
fn a_share_carries_its_sources_rule_and_its_hits_school() {
    let (attacker, holder, paladin) = entities();
    let mut sac = aura(AuraType::DamageTransfer, 0.3);
    sac.caster = Some(paladin);
    let (_, _, follow_ups) = land(
        direct(100.0, SpellSchool::Shadow, Some(attacker), holder),
        &mut bare(500.0),
        Some(&mut auras(vec![sac])),
    );
    assert!(matches!(
        follow_ups.as_slice(),
        [DamageFollowUp::Transfer {
            preventable: true,
            school: SpellSchool::Shadow,
            ..
        }]
    ));
    let mut warlock = bare(500.0);
    warlock.talent_mechanics = TalentMechanics::of(&[TalentMechanic::SoulLink { share: 0.12 }]);
    let (_, _, follow_ups) = land(
        direct(100.0, SpellSchool::Shadow, Some(attacker), holder),
        &mut warlock,
        None,
    );
    assert!(matches!(
        follow_ups.as_slice(),
        [DamageFollowUp::Transfer {
            preventable: false,
            school: SpellSchool::Shadow,
            ..
        }]
    ));
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
    world
        .run_system_once(process_damage_follow_ups)
        .expect("ran");
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
            Some(StructuredEventData::Death { victim, killer }) => {
                Some((victim.clone(), killer.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_transfer_lands_on_its_receiver_and_credits_the_dealer() {
    let Units {
        mut world,
        holder,
        receiver,
        attacker,
    } = units();
    run(
        &mut world,
        DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::Unit(receiver),
            amount: 30.0,
            school: SpellSchool::Physical,
            preventable: true,
            dealer: Some(attacker),
            by: "Blessing of Sacrifice".into(),
        },
    );
    assert_eq!(health(&world, receiver), 370.0);
    assert_eq!(health(&world, holder), 400.0);
    assert_eq!(world.get::<Combatant>(attacker).unwrap().damage_dealt, 30.0);
    assert!(
        world
            .query::<&PendingDamageFollowUp>()
            .iter(&world)
            .next()
            .is_none(),
        "the follow-up is spent"
    );
}

/// A transfer never kills its receiver: a share it cannot bear stays with the
/// holder, and the transfer aura ends.
#[test]
fn an_unbearable_transfer_returns_to_the_holder_and_ends_the_aura() {
    let Units {
        mut world,
        holder,
        receiver,
        attacker,
    } = units();
    world.get_mut::<Combatant>(receiver).unwrap().current_health = 20.0;
    let mut sac = aura(AuraType::DamageTransfer, 0.3);
    sac.caster = Some(receiver);
    world
        .get_mut::<ActiveAuras>(holder)
        .unwrap()
        .auras
        .push(sac);
    run(
        &mut world,
        DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::Unit(receiver),
            amount: 30.0,
            school: SpellSchool::Physical,
            preventable: true,
            dealer: Some(attacker),
            by: "Blessing of Sacrifice".into(),
        },
    );
    assert_eq!(health(&world, receiver), 20.0, "the receiver is spared");
    assert_eq!(health(&world, holder), 370.0, "the holder keeps the share");
    assert!(
        world.get::<ActiveAuras>(holder).unwrap().auras.is_empty(),
        "the transfer ends"
    );
    assert!(deaths(&world).is_empty());
}

/// A share handed back to its holder meets the holder's absorbs, for every
/// kind of transfer: a Blessing of Sacrifice whose caster is dead, and a Soul
/// Link with no living pet. Control: a living receiver takes it.
#[test]
fn a_returned_share_is_soaked_by_the_holders_absorbs() {
    let shield = || {
        let mut s = aura(AuraType::Absorb, 1000.0);
        s.ability_name = "Power Word: Shield".into();
        s
    };
    for receiver_kind in ["Blessing of Sacrifice", "Soul Link"] {
        let Units {
            mut world,
            holder,
            receiver,
            attacker,
        } = units();
        world
            .get_mut::<ActiveAuras>(holder)
            .unwrap()
            .auras
            .push(shield());
        let to = if receiver_kind == "Soul Link" {
            // Its only pet is dead.
            let mut felhunter = bare(150.0);
            felhunter.current_health = 0.0;
            felhunter.is_dead = true;
            world.spawn((
                felhunter,
                Pet {
                    owner: holder,
                    pet_type: PetType::Felhunter,
                },
            ));
            TransferReceiver::OwnPet
        } else {
            let mut paladin = world.get_mut::<Combatant>(receiver).unwrap();
            paladin.current_health = 0.0;
            paladin.is_dead = true;
            TransferReceiver::Unit(receiver)
        };
        run(
            &mut world,
            DamageFollowUp::Transfer {
                holder,
                receiver: to,
                amount: 30.0,
                school: SpellSchool::Physical,
                preventable: receiver_kind != "Soul Link",
                dealer: Some(attacker),
                by: receiver_kind.into(),
            },
        );
        assert_eq!(health(&world, holder), 400.0, "{receiver_kind}: soaked");
        let left = world.get::<ActiveAuras>(holder).unwrap().auras[0].magnitude;
        assert_eq!(left, 970.0, "{receiver_kind}: the shield took it");
    }
    // Control: a living receiver takes the share; the holder's shield is
    // untouched.
    let Units {
        mut world,
        holder,
        receiver,
        attacker,
    } = units();
    world
        .get_mut::<ActiveAuras>(holder)
        .unwrap()
        .auras
        .push(shield());
    run(
        &mut world,
        DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::Unit(receiver),
            amount: 30.0,
            school: SpellSchool::Physical,
            preventable: true,
            dealer: Some(attacker),
            by: "Blessing of Sacrifice".into(),
        },
    );
    assert_eq!(health(&world, receiver), 370.0);
    assert_eq!(
        world.get::<ActiveAuras>(holder).unwrap().auras[0].magnitude,
        1000.0
    );
}

/// A returned share was mitigated on its holder already: neither immunity
/// nor a damage-taken reduction meets it again.
#[test]
fn a_returned_share_is_not_mitigated_twice() {
    for (label, holder_aura) in [
        ("immune", aura(AuraType::DamageImmunity, 1.0)),
        ("reduced", aura(AuraType::DamageTakenReduction, 0.5)),
    ] {
        let Units {
            mut world,
            holder,
            receiver,
            attacker,
        } = units();
        world
            .get_mut::<ActiveAuras>(holder)
            .unwrap()
            .auras
            .push(holder_aura);
        world.get_mut::<Combatant>(receiver).unwrap().current_health = 0.0;
        run(
            &mut world,
            DamageFollowUp::Transfer {
                holder,
                receiver: TransferReceiver::Unit(receiver),
                amount: 30.0,
                school: SpellSchool::Physical,
                preventable: true,
                dealer: Some(attacker),
                by: "Blessing of Sacrifice".into(),
            },
        );
        assert_eq!(health(&world, holder), 370.0, "{label}");
    }
}

/// A returned share is secondary: the reflecting shield that soaks it sends
/// nothing back. Control: the same shield soaking a direct hit reflects.
#[test]
fn a_returned_share_is_never_reflected() {
    let reflecting = || {
        let mut s = aura(AuraType::Absorb, 1000.0);
        s.ability_name = "Power Word: Shield".into();
        s.damage_mods.reflect_absorbed = Some(0.5);
        s
    };
    let Units {
        mut world,
        holder,
        receiver,
        attacker,
    } = units();
    world
        .get_mut::<ActiveAuras>(holder)
        .unwrap()
        .auras
        .push(reflecting());
    world.get_mut::<Combatant>(receiver).unwrap().current_health = 0.0;
    run(
        &mut world,
        DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::Unit(receiver),
            amount: 30.0,
            school: SpellSchool::Physical,
            preventable: true,
            dealer: Some(attacker),
            by: "Blessing of Sacrifice".into(),
        },
    );
    assert_eq!(
        world.get::<ActiveAuras>(holder).unwrap().auras[0].magnitude,
        970.0,
        "the shield soaked the share"
    );
    assert_eq!(health(&world, attacker), 100.0, "and reflected none of it");
    // Control: a direct hit on the same shield reflects.
    let (_, holder_e, _) = entities();
    let mut shielded = auras(vec![reflecting()]);
    let (_, absorbed, follow_ups) = land(
        direct(30.0, SpellSchool::Fire, Some(attacker), holder_e),
        &mut bare(400.0),
        Some(&mut shielded),
    );
    assert_eq!(absorbed, 30.0);
    assert!(matches!(
        follow_ups.as_slice(),
        [DamageFollowUp::Reflect { .. }]
    ));
}

/// A severed Soul Link takes no share; the control with the link intact does.
#[test]
fn a_severed_soul_link_takes_no_share() {
    let (attacker, holder, _) = entities();
    let mut warlock = bare(500.0);
    warlock.talent_mechanics = TalentMechanics::of(&[TalentMechanic::SoulLink { share: 0.12 }]);
    warlock.soul_link_severed = true;
    let (to_health, _, follow_ups) = land(
        direct(100.0, SpellSchool::Fire, Some(attacker), holder),
        &mut warlock,
        None,
    );
    assert_eq!((to_health, follow_ups.len()), (100.0, 0));
    warlock.soul_link_severed = false;
    let (to_health, _, follow_ups) = land(
        direct(100.0, SpellSchool::Fire, Some(attacker), holder),
        &mut warlock,
        None,
    );
    assert!((to_health - 88.0).abs() < 1e-3);
    assert_eq!(follow_ups.len(), 1);
}

/// `end_transfers_to_the_dead`: a transfer aura whose caster is dead ends, and
/// a Soul Link whose pet is dead is severed. Controls: a living caster's aura
/// stays, and a living pet keeps the link.
#[test]
fn transfers_to_the_dead_end_before_the_frames_damage() {
    let run_end = |world: &mut World| {
        world
            .run_system_once(end_transfers_to_the_dead)
            .expect("ran");
    };
    for caster_dead in [true, false] {
        let Units {
            mut world,
            holder,
            receiver,
            ..
        } = units();
        let mut sac = aura(AuraType::DamageTransfer, 0.3);
        sac.caster = Some(receiver);
        world
            .get_mut::<ActiveAuras>(holder)
            .unwrap()
            .auras
            .push(sac);
        if caster_dead {
            world.get_mut::<Combatant>(receiver).unwrap().current_health = 0.0;
        }
        run_end(&mut world);
        let kept = world.get::<ActiveAuras>(holder).unwrap().auras.len();
        assert_eq!(
            kept,
            usize::from(!caster_dead),
            "caster dead: {caster_dead}"
        );
    }
    for pet_dead in [true, false] {
        let Units {
            mut world, holder, ..
        } = units();
        world.get_mut::<Combatant>(holder).unwrap().talent_mechanics =
            TalentMechanics::of(&[TalentMechanic::SoulLink { share: 0.12 }]);
        let mut felhunter = bare(150.0);
        if pet_dead {
            felhunter.current_health = 0.0;
        }
        world.spawn((
            felhunter,
            Pet {
                owner: holder,
                pet_type: PetType::Felhunter,
            },
        ));
        run_end(&mut world);
        assert_eq!(
            world.get::<Combatant>(holder).unwrap().soul_link_severed,
            pet_dead
        );
    }
}

#[test]
fn a_soul_link_share_finds_the_holders_pet() {
    let Units {
        mut world,
        holder,
        attacker,
        ..
    } = units();
    let mut felhunter = bare(150.0);
    felhunter.team = 1;
    felhunter.slot = 10;
    felhunter.class = CharacterClass::Warlock;
    let pet = world
        .spawn((
            felhunter,
            ActiveAuras::default(),
            Pet {
                owner: holder,
                pet_type: PetType::Felhunter,
            },
        ))
        .id();
    run(
        &mut world,
        DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::OwnPet,
            amount: 12.0,
            school: SpellSchool::Physical,
            preventable: false,
            dealer: Some(attacker),
            by: "Soul Link".into(),
        },
    );
    assert_eq!(health(&world, pet), 138.0);
    assert_eq!(health(&world, holder), 400.0);
}

// ---------------------------------------------------------------------------
// A share, landed on a receiver with its own defences (AS-459)
// ---------------------------------------------------------------------------

/// A 30 Physical Blessing of Sacrifice share from the holder to the receiver,
/// the holder wearing the transfer aura.
fn sacrifice(world: &mut World, holder: Entity, receiver: Entity, attacker: Entity) {
    let mut sac = aura(AuraType::DamageTransfer, 0.3);
    sac.caster = Some(receiver);
    sac.ability_name = "Blessing of Sacrifice".into();
    world
        .get_mut::<ActiveAuras>(holder)
        .unwrap()
        .auras
        .push(sac);
    run(
        world,
        DamageFollowUp::Transfer {
            holder,
            receiver: TransferReceiver::Unit(receiver),
            amount: 30.0,
            school: SpellSchool::Physical,
            preventable: true,
            dealer: Some(attacker),
            by: "Blessing of Sacrifice".into(),
        },
    );
}

fn transfer_lines(world: &World) -> Vec<String> {
    world
        .resource::<CombatLog>()
        .entries
        .iter()
        .filter(|e| e.message.contains("[TRANSFER]"))
        .map(|e| e.message.clone())
        .collect()
}

fn holds_sacrifice(world: &World, holder: Entity) -> bool {
    world
        .get::<ActiveAuras>(holder)
        .unwrap()
        .auras
        .iter()
        .any(|a| a.effect_type == AuraType::DamageTransfer)
}

/// The receiver at `hp` health wearing `defence`, sent a Blessing of Sacrifice
/// share: (receiver health, holder health, the holder still holds the
/// transfer, the transfer log lines, the receiver's auras).
fn sacrifice_onto(hp: f32, defence: Option<Aura>) -> (f32, f32, bool, Vec<String>, Vec<Aura>) {
    let Units {
        mut world,
        holder,
        receiver,
        attacker,
    } = units();
    world.get_mut::<Combatant>(receiver).unwrap().current_health = hp;
    if let Some(defence) = defence {
        world
            .get_mut::<ActiveAuras>(receiver)
            .unwrap()
            .auras
            .push(defence);
    }
    sacrifice(&mut world, holder, receiver, attacker);
    (
        health(&world, receiver),
        health(&world, holder),
        holds_sacrifice(&world, holder),
        transfer_lines(&world),
        world.get::<ActiveAuras>(receiver).unwrap().auras.clone(),
    )
}

fn named(kind: AuraType, magnitude: f32, name: &str) -> Aura {
    Aura {
        ability_name: name.into(),
        ..aura(kind, magnitude)
    }
}

/// A Paladin under Divine Shield takes none of its Blessing of Sacrifice
/// share, and the holder none of it either; at low health it still bears the
/// share it is immune to, and the blessing holds. Control: the plain Paladin
/// takes the share, and at low health hands it back and ends the blessing.
#[test]
fn a_paladin_under_divine_shield_takes_no_sacrifice_share() {
    let shield = || Some(named(AuraType::DamageImmunity, 1.0, "Divine Shield"));
    let (paladin, holder, holds, lines, _) = sacrifice_onto(400.0, shield());
    assert_eq!((paladin, holder, holds), (400.0, 400.0, true));
    assert_eq!(
        lines,
        vec!["[TRANSFER] Team 1 Paladin #2 is immune to Team 1 Priest #1's damage (Blessing of Sacrifice)"]
    );
    let (paladin, holder, holds, _, _) = sacrifice_onto(20.0, shield());
    assert_eq!((paladin, holder, holds), (20.0, 400.0, true), "low health");
    // Control: the plain receiver.
    let (paladin, holder, holds, lines, _) = sacrifice_onto(400.0, None);
    assert_eq!((paladin, holder, holds), (370.0, 400.0, true));
    assert_eq!(
        lines,
        vec!["[TRANSFER] Team 1 Paladin #2 takes 30 of Team 1 Priest #1's damage (Blessing of Sacrifice)"]
    );
    let (paladin, holder, holds, _, _) = sacrifice_onto(20.0, None);
    assert_eq!((paladin, holder, holds), (20.0, 370.0, false), "low health");
}

/// A Paladin under Divine Protection takes its reduced share, and bears it
/// at a health the whole share would not leave it. Control: the plain
/// Paladin takes all 30, and at that health hands it back.
#[test]
fn a_paladin_under_divine_protection_takes_the_reduced_share() {
    let protection = named(AuraType::DamageTakenReduction, 0.3, "Divine Protection");
    let (paladin, holder, _, _, _) = sacrifice_onto(400.0, Some(protection.clone()));
    assert!((paladin - 379.0).abs() < 1e-3, "{paladin}");
    assert_eq!(holder, 400.0);
    assert_eq!(sacrifice_onto(400.0, None).0, 370.0);
    // At 25 health the reduced share (21) is borne; the plain receiver hands
    // its 30 back.
    let (paladin, holder, holds, _, _) = sacrifice_onto(25.0, Some(protection));
    assert!((paladin - 4.0).abs() < 1e-3, "{paladin}");
    assert_eq!((holder, holds), (400.0, true));
    let (paladin, holder, holds, _, _) = sacrifice_onto(25.0, None);
    assert_eq!((paladin, holder, holds), (25.0, 370.0, false));
}

/// The receiver's shield soaks its share, and a share the shield keeps below
/// a low receiver's health is borne. Control: the plain receiver.
#[test]
fn a_receivers_absorb_soaks_its_share() {
    let shield = || Some(named(AuraType::Absorb, 50.0, "Power Word: Shield"));
    let (paladin, holder, holds, lines, auras) = sacrifice_onto(400.0, shield());
    assert_eq!((paladin, holder, holds), (400.0, 400.0, true));
    assert_eq!(auras[0].magnitude, 20.0, "the shield took 30");
    assert_eq!(
        lines,
        vec![
            "[TRANSFER] Team 1 Paladin #2 takes 0 of Team 1 Priest #1's damage \
             (Blessing of Sacrifice, 30 absorbed)"
        ]
    );
    let (paladin, holder, holds, _, _) = sacrifice_onto(20.0, shield());
    assert_eq!((paladin, holder, holds), (20.0, 400.0, true), "low health");
    // Control: no shield.
    let (paladin, _, _, _, auras) = sacrifice_onto(400.0, None);
    assert_eq!((paladin, auras.len()), (370.0, 0));
}

/// A transfer never kills its receiver, judged on what the share would do:
/// a vulnerability that raises a 30 share to 45 makes it unbearable at 40
/// health, so it returns to the holder. Control: the plain receiver at 40
/// takes the 30.
#[test]
fn a_share_its_receiver_could_not_survive_raised_returns() {
    let vulnerable = named(AuraType::DamageTakenIncrease, 0.5, "Vulnerable");
    let (paladin, holder, holds, _, _) = sacrifice_onto(40.0, Some(vulnerable));
    assert_eq!((paladin, holder, holds), (40.0, 370.0, false));
    let (paladin, holder, holds, _, _) = sacrifice_onto(40.0, None);
    assert_eq!((paladin, holder, holds), (10.0, 400.0, true));
}

/// A share equal to its receiver's health is one it could not survive: it
/// returns to the holder and the blessing ends. One point of health more and
/// the receiver bears it. Judged on what the share would put on its health,
/// so a 10 absorb moves the line from 30 to 20.
#[test]
fn a_share_equal_to_its_receivers_health_returns_to_the_holder() {
    assert_eq!(
        drop_last(sacrifice_onto(30.0, None)),
        (30.0, 370.0, false),
        "plain, at 30"
    );
    assert_eq!(
        drop_last(sacrifice_onto(31.0, None)),
        (1.0, 400.0, true),
        "plain, at 31"
    );
    let shield = || Some(named(AuraType::Absorb, 10.0, "Power Word: Shield"));
    assert_eq!(
        drop_last(sacrifice_onto(20.0, shield())),
        (20.0, 370.0, false),
        "shielded, at 20"
    );
    assert_eq!(
        drop_last(sacrifice_onto(21.0, shield())),
        (1.0, 400.0, true),
        "shielded, at 21"
    );
}

/// (receiver health, holder health, the holder still holds the transfer).
fn drop_last(
    (receiver, holder, holds, _, _): (f32, f32, bool, Vec<String>, Vec<Aura>),
) -> (f32, f32, bool) {
    (receiver, holder, holds)
}

/// The preview the bearability check trusts (`share_to_health`) is exactly
/// what the funnel takes from a receiver's health, for both kinds of share,
/// over every combination of the receiver's defences: an immunity, a Cyclone,
/// a reduction, a spell-scoped reduction, an uncharged and a charged
/// increase, and two absorbs, against a Physical and a Shadow share.
#[test]
fn share_to_health_is_what_the_funnel_takes() {
    let (_, v, _) = entities();
    let mut charged = aura(AuraType::DamageTakenIncrease, 0.5);
    charged.damage_mods.charges = Some(2);
    let pieces = [
        aura(AuraType::DamageImmunity, 1.0),
        aura(AuraType::Cyclone, 1.0),
        aura(AuraType::DamageTakenReduction, 0.3),
        scoped(AuraType::DamageTakenReduction, 0.5, DamageScope::Spells),
        aura(AuraType::DamageTakenIncrease, 0.2),
        charged,
        aura(AuraType::Absorb, 10.0),
        aura(AuraType::Absorb, 25.0),
    ];
    let mut cases = 0;
    let mut previews = std::collections::BTreeSet::new();
    for mask in 0u32..(1 << pieces.len()) {
        let set = auras(
            pieces
                .iter()
                .enumerate()
                .filter(|(i, _)| mask >> i & 1 == 1)
                .map(|(_, a)| a.clone())
                .collect(),
        );
        for kind in [HitKind::Transferred, HitKind::TransferredUnpreventable] {
            for school in [SpellSchool::Physical, SpellSchool::Shadow] {
                let preview = share_to_health(kind, 30.0, school, Some(&set));
                let mut landed = auras(set.auras.clone());
                let hit = direct(30.0, school, None, v).of_kind(kind);
                let (to_health, _, _) = land(hit, &mut bare(1000.0), Some(&mut landed));
                assert_eq!(
                    preview.to_bits(),
                    to_health.to_bits(),
                    "{kind:?} {school:?} over {:?}: preview {preview}, funnel {to_health}",
                    set.auras
                        .iter()
                        .map(|a| (a.effect_type, a.magnitude))
                        .collect::<Vec<_>>()
                );
                previews.insert(preview.to_bits());
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 1024);
    // Not vacuous: the defences move the number, from nothing to all of it.
    assert!(previews.contains(&0.0f32.to_bits()) && previews.contains(&30.0f32.to_bits()));
    assert!(previews.len() > 10, "{} distinct previews", previews.len());
}

/// A Soul Link share cannot be prevented, so the Felhunter's own defences do
/// not make one it could not survive bearable: at 10 health, under a 100
/// absorb and Master Demonologist, its 12 share returns to the Warlock.
/// Control: at 20 health the Felhunter takes all 12 through the same
/// defences, and the absorb is untouched.
#[test]
fn a_soul_link_share_the_felhunter_cannot_survive_returns_whatever_it_wears() {
    use arenasim::states::play_match::demonology::master_demonologist_aura;
    let link = |hp: f32| {
        let Units {
            mut world,
            holder,
            attacker,
            ..
        } = units();
        let mut felhunter = bare(150.0);
        felhunter.current_health = hp;
        felhunter.team = 1;
        felhunter.slot = 10;
        felhunter.class = CharacterClass::Warlock;
        let pet = world
            .spawn((
                felhunter,
                ActiveAuras {
                    auras: vec![
                        named(AuraType::Absorb, 100.0, "Sacrifice"),
                        master_demonologist_aura(0.2, holder),
                    ],
                },
                Pet {
                    owner: holder,
                    pet_type: PetType::Felhunter,
                },
            ))
            .id();
        run(
            &mut world,
            DamageFollowUp::Transfer {
                holder,
                receiver: TransferReceiver::OwnPet,
                amount: 12.0,
                school: SpellSchool::Shadow,
                preventable: false,
                dealer: Some(attacker),
                by: "Soul Link".into(),
            },
        );
        let shield = world.get::<ActiveAuras>(pet).unwrap().auras[0].magnitude;
        (health(&world, pet), health(&world, holder), shield)
    };
    assert_eq!(link(10.0), (10.0, 388.0, 100.0), "returned to the Warlock");
    assert_eq!(
        link(20.0),
        (8.0, 400.0, 100.0),
        "taken through its defences"
    );
}

/// Soul Link's share cannot be prevented (TBC tooltip 19028): the Felhunter's
/// own Master Demonologist does not reduce a spell share it takes. Controls:
/// the same aura reduces a direct spell hit on the Felhunter, and a share its
/// receiver may prevent (Blessing of Sacrifice's kind) of the same school.
#[test]
fn master_demonologist_does_not_reduce_a_soul_link_share() {
    use arenasim::states::play_match::demonology::master_demonologist_aura;
    let felhunter_of = |world: &mut World, holder: Entity| {
        let mut felhunter = bare(150.0);
        felhunter.team = 1;
        felhunter.slot = 10;
        felhunter.class = CharacterClass::Warlock;
        world
            .spawn((
                felhunter,
                ActiveAuras {
                    auras: vec![master_demonologist_aura(0.2, holder)],
                },
                Pet {
                    owner: holder,
                    pet_type: PetType::Felhunter,
                },
            ))
            .id()
    };
    let share = |preventable: bool, school: SpellSchool| {
        let Units {
            mut world,
            holder,
            attacker,
            ..
        } = units();
        let pet = felhunter_of(&mut world, holder);
        let receiver = if preventable {
            TransferReceiver::Unit(pet)
        } else {
            TransferReceiver::OwnPet
        };
        run(
            &mut world,
            DamageFollowUp::Transfer {
                holder,
                receiver,
                amount: 30.0,
                school,
                preventable,
                dealer: Some(attacker),
                by: if preventable {
                    "Blessing of Sacrifice"
                } else {
                    "Soul Link"
                }
                .into(),
            },
        );
        150.0 - health(&world, pet)
    };
    assert_eq!(
        share(false, SpellSchool::Shadow),
        30.0,
        "Soul Link: all of it"
    );
    // Controls: a preventable spell share is reduced, a Physical one is not.
    assert!((share(true, SpellSchool::Shadow) - 24.0).abs() < 1e-3);
    assert_eq!(share(true, SpellSchool::Physical), 30.0);
    // Control: a direct spell hit on the Felhunter is reduced.
    let (_, pet, _) = entities();
    let mut pet_auras = auras(vec![master_demonologist_aura(0.2, pet)]);
    let (dealt, _, _) = land(
        direct(30.0, SpellSchool::Shadow, None, pet),
        &mut bare(150.0),
        Some(&mut pet_auras),
    );
    assert!((dealt - 24.0).abs() < 1e-3, "{dealt}");
}

/// Reflected damage is damage: it reveals a stealthed attacker, can kill it,
/// and the death is logged with the reflect's dealer as the killer.
#[test]
fn a_reflect_reveals_and_can_kill_the_attacker() {
    let Units {
        mut world,
        holder,
        receiver,
        attacker,
    } = units();
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
        vec![(
            "Team 2 Rogue #1".to_string(),
            Some("Team 1 Paladin #2".to_string())
        )]
    );
    assert_eq!(
        world.get::<Combatant>(receiver).unwrap().damage_dealt,
        100.0
    );
}

/// A reflected hit on a unit that itself reflects is not reflected back.
#[test]
fn a_reflect_on_a_reflecting_unit_stops_there() {
    let Units {
        mut world,
        holder,
        receiver,
        attacker,
    } = units();
    let mut ret = aura(AuraType::DamageReflect, 6.0);
    ret.caster = Some(attacker);
    world
        .get_mut::<ActiveAuras>(attacker)
        .unwrap()
        .auras
        .push(ret);
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
    let Units {
        mut world,
        holder,
        receiver,
        ..
    } = units();
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
        vec![KillingBlowPrevented {
            holder,
            by: "Guardian Spirit".into()
        }]
    );
}

// ---------------------------------------------------------------------------
// The exact-lethal boundary, and the strongest damage-taken reduction
// ---------------------------------------------------------------------------

/// A target with `health` left and the given save, struck for `blow`.
/// Returns its health after, and whether its save was spent.
fn strike(blow: f32, health: f32, cheat_death: bool) -> (f32, bool) {
    let (attacker, holder, priest) = entities();
    let mut target = bare(400.0);
    target.current_health = health;
    let mut target_auras = auras(vec![]);
    if cheat_death {
        target.talent_mechanics = TalentMechanics::of(&[TalentMechanic::CheatDeath {
            health: 0.01,
            reduction: 0.9,
            duration: 3.0,
        }]);
    } else {
        let mut spirit = aura(AuraType::KillingBlowSave, 0.25);
        spirit.caster = Some(priest);
        target_auras.auras.push(spirit);
    }
    land(
        direct(blow, SpellSchool::Fire, Some(attacker), holder),
        &mut target,
        Some(&mut target_auras),
    );
    let spent = if cheat_death {
        target.killing_blow_save_spent
    } else {
        target_auras
            .auras
            .iter()
            .all(|a| a.effect_type != AuraType::KillingBlowSave)
    };
    (target.current_health, spent)
}

/// A blow of EXACTLY the holder's health is a killing blow, and the save
/// takes it; one point less is not, and the save stays unspent.
#[test]
fn a_blow_of_exactly_the_holders_health_is_saved() {
    // Guardian Spirit's shape: health left where it stood.
    assert_eq!(strike(50.0, 50.0, false), (50.0, true));
    assert_eq!(strike(49.0, 50.0, false), (1.0, false));
    // Cheat Death: left at 1% of 400 = 4.
    assert_eq!(strike(50.0, 50.0, true), (4.0, true));
    assert_eq!(strike(49.0, 50.0, true), (1.0, false));
}

/// Only the strongest damage-taken reduction applies: Cheat Death's 90% cut
/// under Devotion Aura's 10% is 90%, not 91%.
#[test]
fn only_the_strongest_damage_taken_reduction_applies() {
    let (_, v, _) = entities();
    let hit = || direct(100.0, SpellSchool::Fire, None, v);
    let devotion = aura(AuraType::DamageTakenReduction, 0.10);
    let cut = arenasim::states::play_match::combat_core::cheat_death_aura(0.90, 3.0);
    let mut both = auras(vec![devotion.clone(), cut]);
    let (dealt, ..) = land(hit(), &mut bare(500.0), Some(&mut both));
    assert!((dealt - 10.0).abs() < 1e-3, "{dealt}");
    // Order does not matter.
    both.auras.reverse();
    let (dealt, ..) = land(hit(), &mut bare(500.0), Some(&mut both));
    assert!((dealt - 10.0).abs() < 1e-3, "{dealt}");
    // Control: Devotion alone cuts exactly its 10%.
    let mut alone = auras(vec![devotion]);
    assert_eq!(land(hit(), &mut bare(500.0), Some(&mut alone)).0, 90.0);
    // An out-of-scope reduction does not outrank an in-scope one.
    let mut scoped_big = auras(vec![
        aura(AuraType::DamageTakenReduction, 0.10),
        scoped(
            AuraType::DamageTakenReduction,
            0.5,
            DamageScope::School(SpellSchool::Physical),
        ),
    ]);
    assert_eq!(land(hit(), &mut bare(500.0), Some(&mut scoped_big)).0, 90.0);
}
