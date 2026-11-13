//! AS-415 — the Priest's Holy tree: the healer that lasts.
//!
//! - **the tree**, as the card writes it: every node, its place, its ranks,
//!   its effect, its price and its prerequisite; the two grants' values;
//! - **the spawn**: a Holy build reaches the spawned Priest — Holy spells'
//!   crit, Flash Heal's healing, mana regeneration, the procs and the grants;
//! - **the healing-received increase** (`combat_core::apply_healing`): its
//!   composition with healing reduction, the Mortal Wounds tell and
//!   dampening, strongest only;
//! - **Guardian Spirit's save**: it prevents the killing blow, heals 30% of
//!   maximum health, and spends the whole spirit;
//! - **Holy Nova**: it heals every teammate inside it and strikes every enemy
//!   inside it, on the global cooldown, for 50 mana; its critical heal is its
//!   Priest's `HealCrit`;
//! - **the AI rules**, in scene pairs (the situation, and its control with its
//!   one condition removed): Guardian Spirit's two triggers and its holds,
//!   and Holy Nova's threats and holds.

mod common;

use std::collections::BTreeSet;

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::build_headless_app;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{self, Mage, Priest, Warrior};
use arenasim::states::match_config::PriestForm;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::priest_holy::{
    SPIRIT_ANY_HP, SPIRIT_FOCUSED_HP, SPIRIT_FOCUS_ATTACKERS,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::combat_core::{
    apply_healing, compound_riders, GUARDIAN_SPIRIT_SAVE_HEAL,
};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraDamageMods, AuraType, Combatant, CompoundDebuff,
    DamageScope, DispelType, HealingRefused, Pet,
};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::talent_choices::{ChoiceOption, SlotChoices};
use arenasim::states::play_match::talent_procs::{
    ProcChance, ProcEffect, ProcRankScale, ProcTarget, TalentProc, TalentProcSlot, TalentTrigger,
};
use arenasim::states::play_match::talents::{
    AbilityField, Build, PatchOp, PatchTarget, TalentDefinitions, TalentEffect, TalentId,
    TalentStat, TreeId,
};
use arenasim::states::play_match::AbilityType::{self, FlashHeal, PowerWordShield};
use common::staged::play;
use TalentId::*;

const NOVA: AbilityType = AbilityType::HolyNova;
const SPIRIT: AbilityType = AbilityType::GuardianSpirit;

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

/// Every Holy node as the card writes it: its place, its ranks, what a rank
/// does, its declared price (Holy Concentration's is computed) and its
/// prerequisite.
#[test]
fn the_holy_tree_is_as_specified() {
    let talents = TalentDefinitions::shipped();
    let holy: BTreeSet<TalentId> = talents
        .talents_for_class(Priest)
        .filter(|(_, t)| t.tree == TreeId::Holy)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        holy,
        [
            HolySpecialization,
            SpiritualHealing,
            Inspiration,
            BlessedRecovery,
            HolyConcentration,
            TalentId::HolyNova,
            TalentId::GuardianSpirit,
        ]
        .into_iter()
        .collect()
    );
    // WoW's order: Discipline, Holy, Shadow ("2/6/0").
    assert_eq!(
        talents
            .trees(Priest)
            .iter()
            .map(|t| (t.id, t.name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (TreeId::Discipline, "Discipline"),
            (TreeId::Holy, "Holy"),
            (TreeId::Shadow, "Shadow")
        ]
    );

    let inspiration = TalentEffect::Proc(TalentProc {
        trigger: TalentTrigger::HealCrit,
        abilities: vec![FlashHeal],
        crit_only: false,
        chance: ProcChance::Chance(1.0),
        ranks_add: ProcRankScale::Effect,
        effect: ProcEffect::ApplyAura {
            on: ProcTarget::Other,
            aura_type: AuraType::ArmorPercentIncrease,
            magnitude: 0.12,
            duration: 15.0,
            tick_interval: 0.0,
            dispel_type: DispelType::Magic,
            name: None,
        },
        internal_cooldown: 0.0,
    });
    let recovery = TalentEffect::Proc(TalentProc {
        trigger: TalentTrigger::CritTaken,
        abilities: vec![],
        crit_only: false,
        chance: ProcChance::Chance(1.0),
        ranks_add: ProcRankScale::Effect,
        effect: ProcEffect::HealBack {
            fraction: 0.25,
            duration: 6.0,
            tick_interval: 2.0,
            name: None,
        },
        internal_cooldown: 0.0,
    });
    let school = |field, op| TalentEffect::Patch {
        on: PatchTarget::School(arenasim::states::play_match::abilities::SpellSchool::Holy),
        field,
        op,
    };
    type Row = (
        TalentId,
        u8,
        u8,
        u8,
        Vec<TalentEffect>,
        Option<f32>,
        Option<(TalentId, u8)>,
    );
    let expected: [Row; 7] = [
        (
            HolySpecialization,
            1,
            0,
            2,
            vec![school(AbilityField::CritChance, PatchOp::Add(0.025))],
            Some(11.0),
            None,
        ),
        (
            SpiritualHealing,
            1,
            2,
            2,
            vec![school(AbilityField::Healing, PatchOp::Mul(1.05))],
            Some(11.0),
            None,
        ),
        (Inspiration, 2, 0, 2, vec![inspiration], Some(11.0), None),
        (BlessedRecovery, 2, 2, 1, vec![recovery], Some(11.0), None),
        (
            TalentId::HolyNova,
            3,
            0,
            1,
            vec![TalentEffect::Grant(NOVA)],
            Some(11.0),
            None,
        ),
        (
            HolyConcentration,
            3,
            1,
            1,
            vec![TalentEffect::StatPercent(TalentStat::ManaRegen, 0.35)],
            // Computed at the reference Priest.
            None,
            None,
        ),
        (
            TalentId::GuardianSpirit,
            4,
            1,
            1,
            vec![TalentEffect::Grant(SPIRIT)],
            Some(22.0),
            Some((HolyConcentration, 1)),
        ),
    ];
    for (id, tier, column, ranks, effects, budget, requires) in expected {
        let node = talents.get(id).unwrap();
        assert_eq!(
            (
                node.tree,
                node.tier,
                node.column,
                node.max_rank,
                &node.per_rank,
                node.budget,
                node.requires
            ),
            (
                TreeId::Holy,
                tier,
                column,
                ranks,
                &effects,
                budget,
                requires
            ),
            "{id:?}"
        );
    }
}

/// The two grants carry the card's values. Holy Nova: 13-15 damage and
/// 25-29 healing within 10 yards, the whole of it (no spell-power share), 50
/// mana, no cooldown. Guardian Spirit: 40% more healing received for 10s,
/// the save healing 30% of maximum health, 60s, 17 mana, and nothing removes
/// it (Wrath 47788: no dispel type).
#[test]
fn the_grants_are_as_specified() {
    let defs = AbilityDefinitions::shipped();
    let nova = defs.get_unchecked(&NOVA);
    assert!(nova.talent);
    assert_eq!(
        (
            nova.damage_base_min,
            nova.damage_base_max,
            nova.damage_coefficient,
            nova.healing_base_min,
            nova.healing_base_max,
            nova.healing_coefficient,
        ),
        (13.0, 15.0, 0.0, 25.0, 29.0, 0.0)
    );
    assert_eq!(
        (nova.range, nova.mana_cost, nova.cooldown, nova.cast_time),
        (10.0, 50.0, 0.0, 0.0)
    );
    assert!(nova.applies_aura.is_none());

    let spirit = defs.get_unchecked(&SPIRIT);
    assert!(spirit.talent);
    assert_eq!(
        (
            spirit.range,
            spirit.mana_cost,
            spirit.cooldown,
            spirit.cast_time
        ),
        (40.0, 17.0, 60.0, 0.0)
    );
    let face = spirit.applies_aura.as_ref().unwrap();
    assert_eq!(
        (
            face.aura_type,
            face.magnitude,
            face.duration,
            face.dispel_type
        ),
        (
            AuraType::HealingReceivedIncrease,
            0.40,
            10.0,
            DispelType::Untyped
        )
    );
    assert_eq!(face.compound, Some(CompoundDebuff::GuardianSpirit));
    let riders = compound_riders(CompoundDebuff::GuardianSpirit);
    assert_eq!(riders.len(), 1);
    assert_eq!(
        (
            riders[0].effect_type,
            riders[0].magnitude,
            riders[0].dispel_type
        ),
        (AuraType::KillingBlowSave, 0.30, DispelType::Untyped)
    );
    assert_eq!(GUARDIAN_SPIRIT_SAVE_HEAL, 0.30);
}

/// The save is the compound's rider, which no numeric field states, so the
/// ability carries a written sentence: it names the rider's share.
#[test]
fn guardian_spirit_text_names_the_save() {
    let defs = AbilityDefinitions::shipped();
    let text = &defs.get_unchecked(&SPIRIT).description;
    let share = format!("{:.0}%", GUARDIAN_SPIRIT_SAVE_HEAL * 100.0);
    assert!(text.contains(&share), "{text}");
    assert!(text.contains("prevented"), "{text}");
}

/// Shadowform casts no heals: the form's choice locks Holy Nova as it locks
/// Flash Heal. Guardian Spirit, a save like Pain Suppression, stays castable.
#[test]
fn shadowform_locks_holy_nova_and_not_the_spirit() {
    let talents = TalentDefinitions::shipped();
    let form = talents.locked_by(&SlotChoices::of(
        &[ChoiceOption::PriestForm(PriestForm::Shadowform)],
        &[],
    ));
    assert!(form.contains(&NOVA), "{form:?}");
    assert!(form.contains(&FlashHeal), "{form:?}");
    assert!(!form.contains(&SPIRIT), "{form:?}");
    let none = talents.locked_by(&SlotChoices::of(
        &[ChoiceOption::PriestForm(PriestForm::NoForm)],
        &[],
    ));
    assert!(!none.contains(&NOVA), "{none:?}");
}

/// Spiritual Healing's field is a direct heal's: it is on Flash Heal and Holy
/// Nova, and not on a Holy spell that heals nothing (Power Word: Shield).
#[test]
fn the_healing_field_is_only_on_a_heal() {
    let defs = AbilityDefinitions::shipped();
    assert!(AbilityField::Healing.present_on(defs.get_unchecked(&FlashHeal)));
    assert!(AbilityField::Healing.present_on(defs.get_unchecked(&NOVA)));
    assert!(!AbilityField::Healing.present_on(defs.get_unchecked(&PowerWordShield)));
    assert_eq!(
        AbilityField::Healing.parts(),
        &[
            AbilityField::HealingBaseMin,
            AbilityField::HealingBaseMax,
            AbilityField::HealingCoefficient
        ]
    );
}

// ---------------------------------------------------------------------------
// The spawn
// ---------------------------------------------------------------------------

/// The Priest a headless match spawns (team 2, against a Warrior) with `build`.
fn spawned_priest(build: &Build) -> Combatant {
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into()],
        team2: vec!["Priest".into()],
        random_seed: Some(7),
        team2_builds: vec![build.clone()],
        ..Default::default()
    };
    let mut app = build_headless_app(&config, true, None);
    app.update();
    let world = app.world_mut();
    let mut q = world.query::<(&Combatant, Option<&Pet>)>();
    let priest = q
        .iter(world)
        .find(|(c, pet)| pet.is_none() && c.team == 2 && c.class == Priest)
        .map(|(c, _)| c.clone());
    priest.expect("a Priest spawned")
}

/// Every Holy node, 8 points: one rank each of the tier-1 nodes, both of
/// Inspiration, then the rest.
fn full_holy() -> Build {
    Build::of(&[
        (HolySpecialization, 1),
        (SpiritualHealing, 1),
        (Inspiration, 2),
        (BlessedRecovery, 1),
        (HolyConcentration, 1),
        (TalentId::HolyNova, 1),
        (TalentId::GuardianSpirit, 1),
    ])
}

/// A Holy build reaches the spawned Priest: Holy spells +2.5% crit a rank,
/// Flash Heal 5% bigger a rank, its base and spell-power share alike (the
/// card's 105 -> 110 -> 116; 108 -> 119 at today's starting kit), 35% more
/// mana regeneration, the two procs, and the two grants.
#[test]
fn a_holy_build_reaches_the_spawned_priest() {
    let base = spawned_priest(&Build::default());
    for a in [NOVA, SPIRIT] {
        assert!(base.book().get(&a).is_none(), "{a:?}");
    }
    assert!(base.talent_procs.is_empty());

    let tier1 = spawned_priest(&Build::of(&[
        (HolySpecialization, 2),
        (SpiritualHealing, 2),
    ]));
    let crit = |c: &Combatant, a| c.book().get_unchecked(&a).crit_chance;
    assert!((crit(&tier1, FlashHeal) - crit(&base, FlashHeal) - 0.05).abs() < 1e-6);
    assert!((crit(&tier1, PowerWordShield) - crit(&base, PowerWordShield) - 0.05).abs() < 1e-6);
    let heal = |c: &Combatant| {
        let d = c.book().get_unchecked(&FlashHeal);
        (d.healing_base_min + d.healing_base_max) / 2.0 + d.healing_coefficient * c.spell_power
    };
    assert!(
        (heal(&tier1) - heal(&base) * 1.10).abs() < 1e-3,
        "{} from {}",
        heal(&tier1),
        heal(&base)
    );

    let full = spawned_priest(&full_holy());
    for a in [NOVA, SPIRIT] {
        assert!(full.book().get(&a).is_some(), "{a:?}");
    }
    assert!(
        (full.mana_regen - base.mana_regen * 1.35).abs() < 1e-4,
        "{} from {}",
        full.mana_regen,
        base.mana_regen
    );
    let procs: Vec<(TalentId, TalentTrigger)> = full
        .talent_procs
        .slots
        .iter()
        .map(|s| (s.talent, s.proc.trigger))
        .collect();
    assert_eq!(
        procs,
        vec![
            (Inspiration, TalentTrigger::HealCrit),
            (BlessedRecovery, TalentTrigger::CritTaken)
        ]
    );
    // Inspiration's two ranks add: 24% armor.
    let inspiration = &full.talent_procs.slots[0].proc.effect;
    assert!(
        matches!(inspiration, ProcEffect::ApplyAura { magnitude, .. } if (*magnitude - 0.24).abs() < 1e-6),
        "{inspiration:?}"
    );
}

// ---------------------------------------------------------------------------
// The healing-received increase, in the one heal site
// ---------------------------------------------------------------------------

fn aura(effect_type: AuraType, magnitude: f32) -> Aura {
    Aura {
        effect_type,
        magnitude,
        duration: 10.0,
        ..Default::default()
    }
}

/// Land a 100-point heal on a Warrior with `auras` under `dampening`, and
/// return what it gained and the Mortal Wounds tell's refused fraction.
fn heal_100(auras: Vec<Aura>, dampening: f32) -> (f32, Option<f32>) {
    let mut world = World::new();
    let target = world.spawn_empty().id();
    let mut queue = CommandQueue::default();
    let mut warrior = Combatant::new(1, 0, Warrior);
    warrior.max_health = 10_000.0;
    warrior.current_health = 1_000.0;
    let auras = ActiveAuras { auras };
    let damp = ArenaDampening {
        reduction: dampening,
        ..Default::default()
    };
    let healed = {
        let mut commands = Commands::new(&mut queue, &world);
        apply_healing(
            &mut commands,
            target,
            &mut warrior,
            Some(&auras),
            &damp,
            100.0,
        )
    };
    queue.apply(&mut world);
    let tell = world
        .query::<&HealingRefused>()
        .iter(&world)
        .map(|r| r.refused_fraction)
        .next();
    (healed, tell)
}

/// The increase raises every heal by its share; with Mortal Strike's cut the
/// two multiply, and the tell still brackets the cut alone (35%); dampening
/// takes its share of the raised heal. Without the increase the same heal is
/// what it was.
#[test]
fn the_healing_increase_composes_with_the_cut_the_tell_and_dampening() {
    let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
    let (none, tell) = heal_100(vec![], 0.0);
    assert!(close(none, 100.0) && tell.is_none());

    let (raised, tell) = heal_100(vec![aura(AuraType::HealingReceivedIncrease, 0.40)], 0.0);
    assert!(close(raised, 140.0), "{raised}");
    assert!(tell.is_none());

    let cut = aura(AuraType::HealingReduction, 0.65);
    let (both, tell) = heal_100(
        vec![aura(AuraType::HealingReceivedIncrease, 0.40), cut.clone()],
        0.0,
    );
    assert!(close(both, 91.0), "{both}");
    assert!(tell.is_some_and(|f| close(f, 0.35)), "{tell:?}");
    let (cut_only, tell) = heal_100(vec![cut], 0.0);
    assert!(close(cut_only, 65.0), "{cut_only}");
    assert!(tell.is_some_and(|f| close(f, 0.35)), "{tell:?}");

    let (dampened, _) = heal_100(vec![aura(AuraType::HealingReceivedIncrease, 0.40)], 0.5);
    assert!(close(dampened, 70.0), "{dampened}");
}

/// Only the strongest increase applies (the stacking simplification): 40%
/// and 20% together are 40%.
#[test]
fn only_the_strongest_healing_increase_applies() {
    let (healed, _) = heal_100(
        vec![
            aura(AuraType::HealingReceivedIncrease, 0.20),
            aura(AuraType::HealingReceivedIncrease, 0.40),
        ],
        0.0,
    );
    assert!((healed - 140.0).abs() < 1e-3, "{healed}");
}

// ---------------------------------------------------------------------------
// Staged matches
// ---------------------------------------------------------------------------

const T1: Unit = Unit::new(1, 0);
const T1B: Unit = Unit::new(1, 1);
const T2: Unit = Unit::new(2, 0);
const T2B: Unit = Unit::new(2, 1);

/// The time of the first log line containing `needle`.
fn first_at(log: &str, needle: &str) -> Option<f32> {
    let line = log.lines().find(|l| l.contains(needle))?;
    let open = line.find('[')?;
    let close = line.find("s]")?;
    line[open + 1..close].trim().parse().ok()
}

fn lines<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines().filter(|l| l.contains(needle)).collect()
}

/// The number after `marker` on `line`.
fn amount(line: &str, marker: &str) -> f32 {
    let at = line.find(marker).unwrap() + marker.len();
    line[at..]
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn held(effect_type: AuraType, name: &str) -> Aura {
    Aura {
        effect_type,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: name.to_string(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

fn weakened() -> Aura {
    Aura {
        effect_type: AuraType::WeakenedSoul,
        duration: 15.0,
        ability_name: "Weakened Soul".into(),
        ..Default::default()
    }
}

/// A Mage beating on a Priest's Warrior teammate at 4% health, the Priest
/// silenced so no heal of its own lands: with Guardian Spirit on the
/// Warrior, the killing blow is prevented, the save heals exactly 30% of its
/// maximum health (no healing cut and no dampening yet — and the spirit's own
/// 40% is spent with it, not applied), and the spirit is gone, its healing
/// increase included. Without the spirit (the control), the Warrior dies.
#[test]
fn the_spirits_save_heals_thirty_percent_and_spends_the_whole_spirit() {
    let run = |spirited: bool| {
        let config = HeadlessMatchConfig {
            team1: vec!["Priest".into(), "Warrior".into()],
            team2: vec!["Mage".into()],
            random_seed: Some(3),
            max_duration_secs: 8.0,
            team1_builds: vec![full_holy()],
            ..Default::default()
        };
        let stage = Stage::new()
            .place(T1, -20.0, 0.0)
            .place(T1B, 0.0, 0.0)
            .place(T2, 20.0, 0.0)
            .health(T1B, 0.04)
            .target(T2, T1B)
            .target(T1B, T2)
            .aura(T1, held(AuraType::Silence, "Held"));
        let stage = if spirited {
            stage.land(T1B, SPIRIT, T1)
        } else {
            stage
        };
        play(config, stage)
    };
    let saved = run(true);
    let log = &saved.log;
    let prevented = first_at(
        log,
        "[SAVED] Guardian Spirit prevents a killing blow on Team 1 Warrior #2",
    )
    .unwrap_or_else(|| panic!("no save:\n{log}"));
    let heal = lines(log, "[SAVED] Guardian Spirit heals Team 1 Warrior #2 for");
    assert_eq!(heal.len(), 1, "{log}");
    // Its maximum health as the blow lands (Power Word: Fortitude raised it).
    let max = saved
        .frames
        .iter()
        .filter(|f| f.sim_time >= prevented - 0.05)
        .flat_map(|f| f.combatants.values())
        .find(|c| !c.is_pet && c.team == 1 && c.slot == 1)
        .unwrap()
        .max_health;
    assert_eq!(
        amount(heal[0], " for "),
        (max * 0.30).round(),
        "the save heals 30% of {max}:\n{log}"
    );
    // The spirit's face and rider are on the Warrior before the save, and
    // neither is after it.
    let warrior_auras = |t: f32| {
        saved
            .frames
            .iter()
            .filter(|f| f.sim_time >= t)
            .find_map(|f| {
                f.combatants
                    .values()
                    .find(|c| !c.is_pet && c.team == 1 && c.slot == 1)
                    .map(|c| c.aura_types.clone())
            })
            .unwrap()
    };
    let before = warrior_auras(saved.landed().app + 0.05);
    assert!(
        before.contains(&AuraType::HealingReceivedIncrease),
        "{before:?}"
    );
    assert!(before.contains(&AuraType::KillingBlowSave), "{before:?}");
    let after = warrior_auras(prevented + 0.1);
    assert!(
        !after.contains(&AuraType::HealingReceivedIncrease)
            && !after.contains(&AuraType::KillingBlowSave),
        "{after:?}\n{log}"
    );

    let control = run(false);
    assert!(
        control
            .log
            .contains("Team 1 Warrior #2 has been eliminated")
            && !control.log.contains("[SAVED]"),
        "{}",
        control.log
    );
}

/// The mana the Priest (team 1, slot 0) paid for a cast logged at `at`: its
/// largest one-frame drop in the frames round it.
fn mana_paid_at(played: &common::staged::Played, at: f32) -> f32 {
    let frames = &played.frames;
    let priest = |i: usize| {
        frames[i]
            .combatants
            .values()
            .find(|c| !c.is_pet && c.team == 1 && c.slot == 0)
            .cloned()
            .expect("the Priest is observed")
    };
    let landed = (0..frames.len())
        .find(|&i| frames[i].sim_time >= at - 1e-3)
        .unwrap_or_else(|| panic!("no frame at {at}:\n{}", played.log));
    (landed.saturating_sub(3)..=(landed + 1).min(frames.len() - 1))
        .filter(|&i| i > 0)
        .map(|i| priest(i - 1).current_mana - priest(i).current_mana)
        .fold(f32::MIN, f32::max)
}

/// The time of the Priest's first `[CAST]` line at or after `after` that is
/// not `ability`'s own.
fn next_priest_cast_after(log: &str, after: f32, ability: &str) -> Option<f32> {
    log.lines()
        .filter(|l| l.contains("[CAST] Team 1 Priest #1") && !l.contains(ability))
        .filter_map(|l| first_at(l, "Priest #1"))
        .find(|t| *t >= after)
}

/// A Priest beside its Warrior at 15%, two enemy Warriors on it: Guardian
/// Spirit goes out, for 17 mana, and the Priest's next cast (a Flash Heal)
/// follows within a few frames, not a global cooldown later (Wrath 47788:
/// no global cooldown).
#[test]
fn guardian_spirit_costs_seventeen_and_no_global_cooldown() {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Warrior".into(), "Warrior".into()],
        random_seed: Some(2),
        max_duration_secs: 4.0,
        team1_builds: vec![full_holy()],
        ..Default::default()
    };
    let stage = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T1B, 10.0, 0.0)
        .place(T2, 11.5, 0.0)
        .place(T2B, 11.5, 1.0)
        .health(T1B, 0.15)
        .target(T2, T1B)
        .target(T2B, T1B)
        .aura(T1, weakened())
        .aura(T1B, weakened());
    let played = play(config, stage);
    let log = &played.log;
    let spirit =
        first_at(log, "Priest #1 casts Guardian Spirit").unwrap_or_else(|| panic!("{log}"));
    let next = next_priest_cast_after(log, spirit, "Guardian Spirit")
        .unwrap_or_else(|| panic!("the Priest cast nothing after:\n{log}"));
    assert!(next - spirit < 0.2, "{spirit} -> {next}\n{log}");
    let paid = mana_paid_at(&played, spirit);
    assert!((paid - 17.0).abs() < 0.5, "paid {paid}\n{log}");
}

/// A Priest at 60% with an enemy Warrior on it, Pummel ready and the rage to
/// pay for it: Holy Nova goes out in place of the Flash Heal, for 50 mana,
/// healing the Priest and its Warrior beside it, striking the enemy Warrior
/// inside and not the second one outside, and the Priest's next cast waits a
/// global cooldown. The teammate 20 yards off is not healed.
#[test]
fn holy_nova_heals_and_strikes_inside_for_fifty_on_the_global_cooldown() {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into(), "Warrior".into()],
        team2: vec!["Warrior".into(), "Warrior".into()],
        random_seed: Some(2),
        max_duration_secs: 4.0,
        team1_builds: vec![Build::of(&[
            (SpiritualHealing, 2),
            (Inspiration, 1),
            (HolyConcentration, 1),
            (TalentId::HolyNova, 1),
        ])],
        ..Default::default()
    };
    const T1C: Unit = Unit::new(1, 2);
    let stage = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T1B, 4.0, 0.0)
        .place(T1C, 20.0, 0.0)
        .place(T2, 1.5, 0.0)
        .place(T2B, 25.0, 0.0)
        .health(T1, 0.6)
        .health(T1B, 0.8)
        .health(T1C, 0.8)
        .target(T2, T1)
        .resource(T2, 30.0)
        .cooldown(T1, AbilityType::PsychicScream, 30.0)
        .aura(T1, weakened())
        .aura(T1B, weakened())
        .aura(T1C, weakened())
        // Frost resistance enough to halve a Frost hit: the nova lands as
        // Holy, so it takes nothing off.
        .aura(
            T2,
            Aura {
                effect_type: AuraType::SpellResistanceBuff,
                duration: 300.0,
                magnitude: 600.0,
                spell_school: Some(arenasim::states::play_match::abilities::SpellSchool::Frost),
                ability_name: "Frost Ward".into(),
                ..Default::default()
            },
        )
        // A Holy damage increase on the Priest: the nova's hit is Holy, so
        // it doubles.
        .aura(
            T1,
            Aura {
                effect_type: AuraType::DamageDealtIncrease,
                duration: 300.0,
                magnitude: 1.0,
                damage_mods: AuraDamageMods {
                    scope: Some(DamageScope::School(
                        arenasim::states::play_match::abilities::SpellSchool::Holy,
                    )),
                    ..Default::default()
                },
                ability_name: "Holy Fervour".into(),
                ..Default::default()
            },
        );
    let played = play(config, stage);
    let log = &played.log;
    let nova = first_at(log, "Priest #1 casts Holy Nova").unwrap_or_else(|| panic!("{log}"));
    let at = |l: &&str| first_at(l, "]").is_some_and(|t| (t - nova).abs() < 1e-3);
    let heals: Vec<&str> = lines(log, "Priest #1's Holy Nova")
        .into_iter()
        .filter(at)
        .collect();
    let healed = |who: &str| {
        heals.iter().any(|l| {
            l.contains(&format!("heals {who} for"))
                || l.contains(&format!("CRITICALLY heals {who} for"))
        })
    };
    let struck = |who: &str| {
        heals.iter().any(|l| {
            l.contains(&format!("hits {who} for")) || l.contains(&format!("CRITS {who} for"))
        })
    };
    assert!(
        healed("Team 1 Priest #1") && healed("Team 1 Warrior #2"),
        "{log}"
    );
    assert!(!healed("Team 1 Warrior #3"), "{log}");
    assert!(
        struck("Team 2 Warrior #1") && !struck("Team 2 Warrior #2"),
        "{log}"
    );
    // Its damage is 13-15, doubled by the Holy increase and untouched by the
    // Frost resistance: it lands as Holy.
    let hits: Vec<&&str> = heals
        .iter()
        .filter(|l| l.contains("hits Team 2 Warrior #1"))
        .collect();
    assert!(!hits.is_empty(), "{log}");
    for l in hits {
        let n = amount(l, " for ");
        assert!((26.0..=30.0).contains(&n), "{l}");
    }
    // Its heal is 25-29 a target, 10% more at 2/2 Spiritual Healing, half
    // again on a crit.
    for l in heals
        .iter()
        .filter(|l| l.contains("heals") && !l.contains("CRITICALLY"))
    {
        let n = amount(l, " for ");
        assert!((27.0..=32.0).contains(&n), "{l}");
    }
    let paid = mana_paid_at(&played, nova);
    assert!((paid - 50.0).abs() < 0.5, "paid {paid}\n{log}");
    let next = log
        .lines()
        .filter(|l| l.contains("[CAST] Team 1 Priest #1"))
        .filter_map(|l| first_at(l, "Priest #1"))
        .find(|t| *t > nova + 1e-3)
        .unwrap_or_else(|| panic!("the Priest cast nothing after:\n{log}"));
    assert!(next - nova >= 1.4, "{nova} -> {next}\n{log}");
}

/// A certain proc on `trigger` naming `abilities`, giving its owner a marker
/// aura, logged as `name`.
fn marker_proc(trigger: TalentTrigger, abilities: Vec<AbilityType>, name: &str) -> TalentProc {
    TalentProc {
        trigger,
        abilities,
        crit_only: false,
        chance: ProcChance::Chance(1.0),
        ranks_add: ProcRankScale::Chance,
        effect: ProcEffect::ApplyAura {
            on: ProcTarget::Owner,
            aura_type: AuraType::AttackPowerIncrease,
            magnitude: 1.0,
            duration: 5.0,
            tick_interval: 0.0,
            dispel_type: DispelType::Physical,
            name: Some(name.to_string()),
        },
        internal_cooldown: 0.0,
    }
}

/// A Priest whose every spell crits, novaing in place of a threatened Flash
/// Heal: each critical Holy Nova heal is its `HealCrit`, so a proc naming
/// Holy Nova fires; Inspiration, which names only Flash Heal (TBC's list has
/// no Holy Nova), does not.
#[test]
fn a_critical_holy_nova_heal_is_its_priests_heal_crit() {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(2),
        max_duration_secs: 3.0,
        team1_builds: vec![Build::of(&[
            (HolySpecialization, 2),
            (Inspiration, 2),
            (HolyConcentration, 1),
            (TalentId::HolyNova, 1),
        ])],
        ..Default::default()
    };
    let stage = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T2, 1.5, 0.0)
        .health(T1, 0.6)
        .target(T2, T1)
        .resource(T2, 30.0)
        .cooldown(T1, AbilityType::PsychicScream, 30.0)
        .aura(T1, weakened())
        .combatant(T1, |c| {
            c.crit_chance = 1.0;
            c.talent_procs.slots.push(TalentProcSlot::new(
                FelStamina,
                "Test Talent",
                marker_proc(TalentTrigger::HealCrit, vec![NOVA], "Nova Crit"),
            ));
        });
    let log = play(config, stage).log;
    assert!(
        log.contains("Holy Nova CRITICALLY heals Team 1 Priest #1"),
        "{log}"
    );
    assert!(log.contains("Test Talent procs Nova Crit"), "{log}");
    assert!(!log.contains("procs Inspiration"), "{log}");
}

// ---------------------------------------------------------------------------
// The AI: scene pairs
// ---------------------------------------------------------------------------

const PRIEST: &str = "priest";
const ALLY: &str = "ally";
const ENEMY: &str = "enemy warrior";
const ENEMY2: &str = "second enemy";

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

fn precondition(view: &DecisionView, ability: AbilityType) -> bool {
    matches!(
        view.rejected(ability),
        Some(RejectionReason::PreconditionUnmet { .. })
    )
}

fn fortitude() -> Aura {
    Aura {
        effect_type: AuraType::MaxHealthIncrease,
        duration: 600.0,
        ability_name: "Power Word: Fortitude".into(),
        ..Default::default()
    }
}

/// A Priest (Weakened Soul, so it shields no one first) with a teammate of
/// `class` at `ally_hp` 10 yards off, `attackers` enemy Warriors on it.
fn spirit_scene(class: CharacterClass, ally_hp: f32, attackers: usize) -> Scene {
    let mut scene = Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .unit(ALLY, 1, class)
        .at(10.0, 0.0)
        .health(ally_hp)
        .aura_full(ALLY, fortitude())
        .aura(ALLY, AuraType::WeakenedSoul, 10.0)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Warrior)
        .at(11.5, 0.0);
    if attackers >= 1 {
        scene = scene.targeting(ALLY);
    }
    scene = scene.unit(ENEMY2, 2, Warrior).at(11.5, 1.5);
    if attackers >= 2 {
        scene = scene.targeting(ALLY);
    }
    scene.grant(PRIEST, SPIRIT)
}

/// Below 40% with two enemies on it, the teammate takes the spirit; with one
/// enemy on it (the control), the spirit is held.
#[test]
fn the_spirit_answers_a_focused_teammate_below_forty() {
    assert_eq!(SPIRIT_FOCUS_ATTACKERS, 2);
    let hp = SPIRIT_FOCUSED_HP - 0.05;
    let focused = spirit_scene(Warrior, hp, 2).decide(PRIEST);
    assert_eq!(
        focused.chose(),
        Some((SPIRIT, Some(ALLY))),
        "{}",
        describe(&focused)
    );
    let single = spirit_scene(Warrior, hp, 1).decide(PRIEST);
    assert!(precondition(&single, SPIRIT), "{}", describe(&single));
    // Two on it but above 40%: held as well.
    let healthy = spirit_scene(Warrior, SPIRIT_FOCUSED_HP + 0.05, 2).decide(PRIEST);
    assert!(precondition(&healthy, SPIRIT), "{}", describe(&healthy));
}

/// Below 25%, one enemy on it is enough (any source); at 30% with one (the
/// control) it is not.
#[test]
fn the_spirit_answers_any_teammate_below_twenty_five() {
    let dying = spirit_scene(Warrior, SPIRIT_ANY_HP - 0.05, 1).decide(PRIEST);
    assert_eq!(
        dying.chose(),
        Some((SPIRIT, Some(ALLY))),
        "{}",
        describe(&dying)
    );
    let hurt = spirit_scene(Warrior, SPIRIT_ANY_HP + 0.05, 1).decide(PRIEST);
    assert!(precondition(&hurt, SPIRIT), "{}", describe(&hurt));
}

/// A dying teammate immune to damage (a Divine Shield) is held off; the same
/// teammate exposed takes the spirit.
#[test]
fn the_spirit_holds_off_a_teammate_immune_to_damage() {
    let immune = spirit_scene(Warrior, 0.2, 2)
        .aura(ALLY, AuraType::DamageImmunity, 8.0)
        .decide(PRIEST);
    assert!(precondition(&immune, SPIRIT), "{}", describe(&immune));
    let exposed = spirit_scene(Warrior, 0.2, 2).decide(PRIEST);
    assert_eq!(exposed.chose(), Some((SPIRIT, Some(ALLY))));
}

/// A dying Rogue teammate hidden by stealth is held off; the same Rogue in
/// the open takes the spirit.
#[test]
fn the_spirit_holds_off_a_teammate_hidden_by_stealth() {
    let rogue = |stealthed: bool| {
        let mut scene = spirit_scene(CharacterClass::Rogue, 0.2, 2);
        scene.combatant_mut(ALLY).stealthed = stealthed;
        scene.decide(PRIEST)
    };
    let hidden = rogue(true);
    assert!(precondition(&hidden, SPIRIT), "{}", describe(&hidden));
    let open = rogue(false);
    assert_eq!(
        open.chose(),
        Some((SPIRIT, Some(ALLY))),
        "{}",
        describe(&open)
    );
}

/// A teammate already holding a spirit is not given a second.
#[test]
fn the_spirit_is_never_doubled() {
    let mut defs_scene = spirit_scene(Warrior, 0.2, 2);
    let defs = AbilityDefinitions::shipped();
    let face = arenasim::states::play_match::components::AuraPending::from_ability(
        defs_scene.entity(ALLY),
        defs_scene.entity(PRIEST),
        defs.get_unchecked(&SPIRIT),
    )
    .unwrap()
    .aura;
    defs_scene = defs_scene.aura_full(ALLY, face);
    let doubled = defs_scene.decide(PRIEST);
    assert!(
        matches!(
            doubled.rejected(SPIRIT),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&doubled)
    );
}

/// The Priest itself, dying under a melee, takes its own spirit; two
/// teammates in danger, the more endangered one takes it.
#[test]
fn the_spirit_goes_on_the_most_endangered_teammate_the_priest_included() {
    let me = Scene::new()
        .unit(PRIEST, 1, Priest)
        .health(0.2)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .unit(ENEMY, 2, Warrior)
        .at(1.5, 0.0)
        .targeting(PRIEST)
        .grant(PRIEST, SPIRIT)
        .decide(PRIEST);
    assert_eq!(
        me.chose(),
        Some((SPIRIT, Some(PRIEST))),
        "{}",
        describe(&me)
    );

    let both = |priest_hp: f32| {
        spirit_scene(Warrior, 0.2, 2)
            .health_of(PRIEST, priest_hp)
            .decide(PRIEST)
    };
    let ally_lower = both(0.24);
    assert_eq!(ally_lower.chose(), Some((SPIRIT, Some(ALLY))));
    let priest_lower = both(0.15);
    assert_eq!(
        priest_lower.chose(),
        Some((SPIRIT, Some(PRIEST))),
        "{}",
        describe(&priest_lower)
    );
}

/// The Shadowform chain runs the spirit too, as it runs Pain Suppression.
#[test]
fn a_shadowform_priest_casts_the_spirit() {
    let danger = spirit_scene(Warrior, 0.2, 2)
        .in_shadowform(PRIEST)
        .decide(PRIEST);
    assert_eq!(
        danger.chose(),
        Some((SPIRIT, Some(ALLY))),
        "{}",
        describe(&danger)
    );
    let calm = spirit_scene(Warrior, 0.9, 2)
        .in_shadowform(PRIEST)
        .decide(PRIEST);
    assert!(precondition(&calm, SPIRIT), "{}", describe(&calm));
}

/// A Priest at `priest_hp` with an enemy Warrior beside it targeting it,
/// carrying `rage`, and a Warrior teammate at `ally_hp` `ally_at` yards off.
/// Psychic Scream on cooldown and Weakened Soul all round, so neither the
/// scream nor a shield steps in first.
fn nova_scene(priest_hp: f32, rage: f32, ally_hp: f32, ally_at: f32) -> Scene {
    Scene::new()
        .unit(PRIEST, 1, Priest)
        .health(priest_hp)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .cooldown(PRIEST, AbilityType::PsychicScream, 20.0)
        .unit(ALLY, 1, Warrior)
        .at(ally_at, 0.0)
        .health(ally_hp)
        .aura_full(ALLY, fortitude())
        .aura(ALLY, AuraType::WeakenedSoul, 10.0)
        .unit(ENEMY, 2, Warrior)
        .at(1.5, 0.0)
        .rage(rage)
        .targeting(PRIEST)
        .grant(PRIEST, NOVA)
}

/// An enemy beside the Priest with its interrupt ready and the rage for it:
/// the Priest, hurt, novas in place of a Flash Heal it would lose. With
/// Pummel on cooldown, or no rage to pay for it, or the Warrior stunned, the
/// Flash Heal goes out.
#[test]
fn holy_nova_replaces_a_flash_heal_an_interrupt_would_take() {
    let threatened = nova_scene(0.6, 30.0, 1.0, 5.0).decide(PRIEST);
    assert_eq!(
        threatened.chose(),
        Some((NOVA, None)),
        "{}",
        describe(&threatened)
    );
    for (what, scene) in [
        (
            "Pummel cooling down",
            nova_scene(0.6, 30.0, 1.0, 5.0).cooldown(ENEMY, AbilityType::Pummel, 5.0),
        ),
        ("no rage", nova_scene(0.6, 0.0, 1.0, 5.0)),
        (
            "the Warrior stunned",
            nova_scene(0.6, 30.0, 1.0, 5.0).aura(ENEMY, AuraType::Stun, 3.0),
        ),
    ] {
        let calm = scene.decide(PRIEST);
        assert!(precondition(&calm, NOVA), "{what}: {}", describe(&calm));
        assert_eq!(
            calm.chose(),
            Some((FlashHeal, Some(PRIEST))),
            "{what}: {}",
            describe(&calm)
        );
    }
}

/// An interrupter out of its interrupt's reach (Pummel's 2.5 yards) is no
/// threat: the Flash Heal goes out.
#[test]
fn an_interrupter_out_of_reach_is_no_threat() {
    let mut far = nova_scene(0.6, 30.0, 1.0, 5.0);
    far.set_position(ENEMY, 6.0, 0.0);
    let view = far.decide(PRIEST);
    assert!(precondition(&view, NOVA), "{}", describe(&view));
    let near = nova_scene(0.6, 30.0, 1.0, 5.0).decide(PRIEST);
    assert_eq!(near.chose(), Some((NOVA, None)));
}

/// The Flash Heal's target must stand inside the nova: a teammate at 60%
/// 20 yards off is healed by Flash Heal; 5 yards off, by the nova.
#[test]
fn holy_nova_only_for_a_heal_target_inside_it() {
    let outside = nova_scene(1.0, 30.0, 0.6, 20.0).decide(PRIEST);
    assert!(precondition(&outside, NOVA), "{}", describe(&outside));
    assert_eq!(
        outside.chose(),
        Some((FlashHeal, Some(ALLY))),
        "{}",
        describe(&outside)
    );
    let inside = nova_scene(1.0, 30.0, 0.6, 5.0).decide(PRIEST);
    assert_eq!(inside.chose(), Some((NOVA, None)), "{}", describe(&inside));
}

/// An enemy inside the nova that the Priest's team holds in crowd control
/// any damage breaks (a teammate's Polymorph) holds the nova; without the
/// Polymorph the nova goes out.
#[test]
fn holy_nova_never_breaks_the_teams_crowd_control() {
    let sheeped = |polymorphed: bool| {
        let mut scene = nova_scene(0.6, 30.0, 1.0, 5.0)
            .unit("mage", 1, Mage)
            .at(-15.0, 0.0)
            .aura_full("mage", fortitude())
            .aura("mage", AuraType::WeakenedSoul, 10.0)
            .unit(ENEMY2, 2, Warrior)
            .at(-4.0, 0.0);
        if polymorphed {
            let mage = scene.entity("mage");
            scene = scene.aura_full(
                ENEMY2,
                Aura {
                    effect_type: AuraType::Polymorph,
                    duration: 8.0,
                    break_on_damage_threshold: 0.0,
                    caster: Some(mage),
                    ability_name: "Polymorph".into(),
                    ..Default::default()
                },
            );
        }
        scene.decide(PRIEST)
    };
    let held = sheeped(true);
    assert!(
        matches!(held.rejected(NOVA), Some(RejectionReason::PreconditionUnmet { note }) if note.contains("crowd control")),
        "{}",
        describe(&held)
    );
    let free = sheeped(false);
    assert_eq!(free.chose(), Some((NOVA, None)), "{}", describe(&free));
}

/// Holy Nova is not in the book without its talent: the same threatened
/// Priest casts its Flash Heal.
#[test]
fn without_the_talent_the_flash_heal_goes_out() {
    let mut scene = nova_scene(0.6, 30.0, 1.0, 5.0);
    let book = scene.combatant_mut(PRIEST).book().clone();
    assert!(book.get(&NOVA).is_some());
    let untalented = Scene::new()
        .unit(PRIEST, 1, Priest)
        .health(0.6)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .cooldown(PRIEST, AbilityType::PsychicScream, 20.0)
        .unit(ENEMY, 2, Warrior)
        .at(1.5, 0.0)
        .rage(30.0)
        .targeting(PRIEST)
        .decide(PRIEST);
    assert_eq!(untalented.chose(), Some((FlashHeal, Some(PRIEST))));
    assert!(untalented.rejected(NOVA).is_none());
}

/// A Priest at 70% whose Warrior teammate stands 35 yards off in an enemy
/// Mage's Polymorph, beyond Dispel Magic's reach: the dispel walk owns the
/// Priest's legs and defers its own Flash Heal (it is not critical), so the
/// nova heals it instead, with no interrupter anywhere. With the Warrior free
/// (the control) there is no walk, and the Flash Heal goes out.
#[test]
fn holy_nova_replaces_a_flash_heal_a_walk_defers() {
    let walk = |sheeped: bool| {
        let mut scene = Scene::new()
            .unit(PRIEST, 1, Priest)
            .health(0.7)
            .aura_full(PRIEST, fortitude())
            .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
            .cooldown(PRIEST, AbilityType::PsychicScream, 20.0)
            .unit(ALLY, 1, Warrior)
            .at(35.0, 0.0)
            .aura_full(ALLY, fortitude())
            .aura(ALLY, AuraType::WeakenedSoul, 10.0)
            .unit("enemy mage", 2, Mage)
            .at(60.0, 0.0)
            .grant(PRIEST, NOVA);
        if sheeped {
            let mage = scene.entity("enemy mage");
            scene = scene.aura_full(
                ALLY,
                Aura {
                    effect_type: AuraType::Polymorph,
                    duration: 8.0,
                    break_on_damage_threshold: 0.0,
                    caster: Some(mage),
                    ability_name: "Polymorph".into(),
                    ..Default::default()
                },
            );
        }
        scene.decide(PRIEST)
    };
    let walking = walk(true);
    assert_eq!(
        walking.chose(),
        Some((NOVA, None)),
        "{}",
        describe(&walking)
    );
    let still = walk(false);
    assert!(precondition(&still, NOVA), "{}", describe(&still));
    assert_eq!(
        still.chose(),
        Some((FlashHeal, Some(PRIEST))),
        "{}",
        describe(&still)
    );
}
