//! AS-416 — the Priest's Shadow tree: damage tools for a healer, and the way
//! to stop healing.
//!
//! - **the tree**, as the card writes it: every node, its place, its ranks,
//!   its effect, its price and its prerequisite; the three grants' values;
//!   Silence locked without Shadowform;
//! - **the spawn**: a Shadow build reaches the spawned Priest;
//! - **Blackout**: a Mind Blast that lands stuns its target for 3s;
//! - **Mind Flay**: a 3s channel, 10 a tick, whose slow ends with it; a
//!   Shadowform Priest closes to its reach;
//! - **Vampiric Embrace**: the Priest's Shadow damage to its bearer heals the
//!   Priest's party through `apply_healing` (dampening and healing reduction
//!   cut it; no pet is healed);
//! - **Silence**: it stops the cast it lands on, and a second silencer in the
//!   frame sees it;
//! - **Aura Mastery** answers an enemy's Silence as it answers an interrupt;
//! - **the AI rules**, in scene pairs (the situation, and its control with its
//!   one condition removed), for the healing Priest and the Shadowform
//!   rotation.

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::MinimalPlugins;

use arenasim::combat::log::CombatLog;
use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::build_headless_app;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{
    self, Hunter, Mage, Paladin, Priest, Warlock, Warrior,
};
use arenasim::states::match_config::PriestForm;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::cast_guard::{own_state_ok, PreCastOpts};
use arenasim::states::play_match::class_ai::priest_shadow::{
    shadow_pursuit_range, MIND_FLAY_PEEL_REACH, MIND_FLAY_RANGE_BUFFER,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::combat_core::leech::{Leech, LeechRecipient, LeechSource};
use arenasim::states::play_match::combat_core::process_channeling;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraType, ChannelingState, Combatant, DRCategory, DRTracker,
    DispelType, MatchCountdown, Pet,
};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::talent_choices::{ChoiceOption, SlotChoices};
use arenasim::states::play_match::talent_procs::{
    effect_aura, ProcChance, ProcEffect, ProcEvent, ProcRankScale, ProcTarget, TalentProc,
    TalentProcSlot, TalentTrigger,
};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentEffect, TalentId};
use arenasim::states::play_match::AbilityType::{
    self, FlashHeal, MindBlast, ShadowWordPain, Shadowbolt,
};
use common::staged::play;
use TalentId::*;

const FLAY: AbilityType = AbilityType::MindFlay;
const EMBRACE: AbilityType = AbilityType::VampiricEmbrace;
const SILENCE: AbilityType = AbilityType::Silence;

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

/// Blackout's one rank, as the card writes it: a Mind Blast that hits stuns
/// its target for 3s, 4% a rank, Magic (TBC 15269).
fn blackout() -> TalentProc {
    TalentProc {
        trigger: TalentTrigger::SpellHit,
        abilities: vec![MindBlast],
        crit_only: false,
        chance: ProcChance::Chance(0.04),
        ranks_add: ProcRankScale::Chance,
        effect: ProcEffect::ApplyAura {
            on: ProcTarget::Other,
            aura_type: AuraType::Stun,
            magnitude: 1.0,
            duration: 3.0,
            tick_interval: 0.0,
            scope: None,
            charges: None,
            dispel_type: DispelType::Magic,
            name: None,
        },
        internal_cooldown: 0.0,
    }
}

/// Every Shadow node as the card writes it (the first three are AS-413's):
/// its place, its ranks, what a rank does, its declared price and its
/// prerequisite.
#[test]
fn the_shadow_tree_is_as_specified() {
    use arenasim::states::play_match::talents::{AbilityField, PatchOp, PatchTarget, TreeId};
    let talents = TalentDefinitions::shipped();
    let shadow: BTreeSet<TalentId> = talents
        .talents_for_class(Priest)
        .filter(|(_, t)| t.tree == TreeId::Shadow)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        shadow,
        [
            ImprovedMindBlast,
            ImprovedPsychicScream,
            TalentId::Shadowform,
            Blackout,
            TalentId::MindFlay,
            TalentId::VampiricEmbrace,
            TalentId::Silence,
        ]
        .into_iter()
        .collect()
    );
    let cooldown = |ability, secs| TalentEffect::Patch {
        on: PatchTarget::Ability(ability),
        field: AbilityField::Cooldown,
        op: PatchOp::Add(secs),
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
    let expected: [Row; 6] = [
        (
            ImprovedMindBlast,
            1,
            0,
            2,
            vec![cooldown(MindBlast, -1.0)],
            Some(11.0),
            None,
        ),
        (
            ImprovedPsychicScream,
            1,
            2,
            2,
            vec![cooldown(AbilityType::PsychicScream, -2.0)],
            Some(11.0),
            None,
        ),
        (
            Blackout,
            2,
            2,
            2,
            vec![TalentEffect::Proc(blackout())],
            Some(11.0),
            None,
        ),
        (
            TalentId::MindFlay,
            3,
            0,
            1,
            vec![TalentEffect::Grant(FLAY)],
            Some(11.0),
            None,
        ),
        (
            TalentId::VampiricEmbrace,
            3,
            2,
            1,
            vec![TalentEffect::Grant(EMBRACE)],
            Some(11.0),
            None,
        ),
        (
            TalentId::Silence,
            4,
            1,
            1,
            vec![TalentEffect::Grant(SILENCE)],
            Some(22.0),
            Some((TalentId::Shadowform, 1)),
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
                TreeId::Shadow,
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
    // AS-413's form node, at tier 2 between the two tier-2 columns.
    let form = talents.get(TalentId::Shadowform).unwrap();
    assert_eq!((form.tier, form.column), (2, 1));
}

/// The three grants carry the card's values, and TBC's for what the card
/// leaves open: school, prevention, global cooldown and dispel class.
///
/// - Mind Flay: a 3s channel of 10 Shadow a second (30 in all) from 20 yards,
///   15 mana, no cooldown, and a 50% slow for its 3s that nothing dispels
///   (15407: DispelType 0).
/// - Vampiric Embrace: 30 yards, 6 mana, 10s cooldown; a 60s debuff whose
///   leech heals the caster's party for 20% of its Shadow damage; Magic.
/// - Silence: 20 yards, 25 mana, 45s cooldown; a 5s silence; Magic.
///
/// All three take the global cooldown (StartRecoveryCategory 133).
#[test]
fn the_grants_are_as_specified() {
    use arenasim::states::play_match::ability_config::Prevention;
    let defs = AbilityDefinitions::shipped();

    let flay = defs.get_unchecked(&FLAY);
    assert!(flay.talent && !flay.off_gcd);
    assert_eq!(
        (
            flay.range,
            flay.mana_cost,
            flay.cooldown,
            flay.cast_time,
            flay.damage_base_min,
            flay.damage_base_max,
            flay.damage_coefficient,
            flay.channel_duration,
            flay.channel_tick_interval,
        ),
        (20.0, 15.0, 0.0, 0.0, 10.0, 10.0, 0.0, Some(3.0), 1.0)
    );
    assert_eq!(
        (flay.spell_school, flay.prevention),
        (SpellSchool::Shadow, Prevention::Silence)
    );
    assert!(flay.channel_leech.is_none());
    let slow = flay.applies_aura.as_ref().unwrap();
    assert_eq!(
        (
            slow.aura_type,
            slow.magnitude,
            slow.duration,
            slow.dispel_type
        ),
        (AuraType::MovementSpeedSlow, 0.5, 3.0, DispelType::Untyped)
    );

    let embrace = defs.get_unchecked(&EMBRACE);
    assert!(embrace.talent && !embrace.off_gcd);
    assert_eq!(
        (
            embrace.range,
            embrace.mana_cost,
            embrace.cooldown,
            embrace.cast_time
        ),
        (30.0, 6.0, 10.0, 0.0)
    );
    assert_eq!(
        (embrace.spell_school, embrace.prevention),
        (SpellSchool::Shadow, Prevention::Silence)
    );
    let mark = embrace.applies_aura.as_ref().unwrap();
    assert_eq!(
        (mark.aura_type, mark.duration, mark.dispel_type),
        (AuraType::LeechMark, 60.0, DispelType::Auto)
    );
    assert_eq!(
        mark.leech,
        Some(Leech {
            fraction: 0.2,
            to: LeechRecipient::CasterParty,
            of: LeechSource::CasterSchool(SpellSchool::Shadow),
        })
    );

    let silence = defs.get_unchecked(&SILENCE);
    assert!(silence.talent && !silence.off_gcd);
    assert_eq!(
        (
            silence.range,
            silence.mana_cost,
            silence.cooldown,
            silence.cast_time
        ),
        (20.0, 25.0, 45.0, 0.0)
    );
    assert_eq!(
        (silence.spell_school, silence.prevention),
        (SpellSchool::Shadow, Prevention::Silence)
    );
    let aura = silence.applies_aura.as_ref().unwrap();
    assert_eq!(
        (aura.aura_type, aura.duration, aura.dispel_type),
        (AuraType::Silence, 5.0, DispelType::Auto)
    );
    // It diminishes in the Silence category, beside the backlash's.
    assert_eq!(
        DRCategory::from_aura_type(&aura.aura_type),
        Some(DRCategory::Silence)
    );
}

/// Silence requires the form: granted, with no form chosen it is locked and
/// the cast gate refuses it; with Shadowform chosen it is neither. Mind Flay
/// and Vampiric Embrace are castable either way.
#[test]
fn silence_requires_the_form() {
    let talents = TalentDefinitions::shipped();
    let defs = AbilityDefinitions::shipped();
    let build = Build::of(&[
        (ImprovedMindBlast, 2),
        (ImprovedPsychicScream, 1),
        (TalentId::Shadowform, 1),
        (TalentId::MindFlay, 1),
        (TalentId::VampiricEmbrace, 1),
        (TalentId::Silence, 1),
    ]);
    let book = |form| {
        talents
            .resolve_with_choices(
                defs,
                Priest,
                &build,
                &SlotChoices::of(&[ChoiceOption::PriestForm(form)], &[]),
            )
            .unwrap()
            .book()
            .clone()
    };
    let healer = book(PriestForm::NoForm);
    assert!(healer.is_locked(SILENCE));
    assert!(!healer.is_locked(FLAY) && !healer.is_locked(EMBRACE));
    let shadow = book(PriestForm::Shadowform);
    assert!(!shadow.is_locked(SILENCE));
    let gate = |book| {
        let c = Combatant::new(1, 0, Priest).with_book(book);
        own_state_ok(
            SILENCE,
            defs.get_unchecked(&SILENCE),
            &c,
            None,
            PreCastOpts::default(),
        )
    };
    assert!(!gate(healer));
    assert!(gate(shadow));
}

/// The Priest a headless match spawns (team 2, against a Warrior) with
/// `build`.
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

/// Every Shadow node but Improved Psychic Scream's second rank: 9 points is
/// one over the cap, so 8.
fn full_shadow() -> Build {
    Build::of(&[
        (ImprovedMindBlast, 1),
        (ImprovedPsychicScream, 1),
        (TalentId::Shadowform, 1),
        (Blackout, 2),
        (TalentId::MindFlay, 1),
        (TalentId::VampiricEmbrace, 1),
        (TalentId::Silence, 1),
    ])
}

/// A Shadow build reaches the spawned Priest: the three grants, and Blackout
/// at 2/2 an 8% chance on a Mind Blast hit.
#[test]
fn a_shadow_build_reaches_the_spawned_priest() {
    let base = spawned_priest(&Build::default());
    for a in [FLAY, EMBRACE, SILENCE] {
        assert!(base.book().get(&a).is_none(), "{a:?}");
    }
    assert!(base.talent_procs.is_empty());

    let full = spawned_priest(&full_shadow());
    for a in [FLAY, EMBRACE, SILENCE] {
        assert!(full.book().get(&a).is_some(), "{a:?}");
    }
    let slots: Vec<&TalentProcSlot> = full.talent_procs.slots.iter().collect();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].talent, Blackout);
    assert_eq!(slots[0].proc.trigger, TalentTrigger::SpellHit);
    assert_eq!(slots[0].proc.abilities, vec![MindBlast]);
    assert!(
        matches!(slots[0].proc.chance, ProcChance::Chance(c) if (c - 0.08).abs() < 1e-6),
        "{:?}",
        slots[0].proc.chance
    );
}

// ---------------------------------------------------------------------------
// Blackout
// ---------------------------------------------------------------------------

/// Blackout's effect is a 3s stun on the unit the Mind Blast struck, Magic,
/// that no damage breaks; only a Mind Blast's hit qualifies — not another
/// Shadow spell's, and not a Mind Flay tick.
#[test]
fn blackout_stuns_the_unit_a_mind_blast_struck() {
    let priest = Entity::from_raw(1);
    let victim = Entity::from_raw(2);
    let slot = TalentProcSlot::new(Blackout, "Blackout", blackout().at_rank(2));
    let hit = ProcEvent::new(priest, TalentTrigger::SpellHit)
        .ability(MindBlast)
        .other(victim);
    assert!(slot.proc.qualifies(&hit));
    for other in [
        ProcEvent::new(priest, TalentTrigger::SpellHit)
            .ability(Shadowbolt)
            .other(victim),
        ProcEvent::new(priest, TalentTrigger::SpellHit)
            .ability(ShadowWordPain)
            .other(victim),
        ProcEvent::new(priest, TalentTrigger::DotTick)
            .ability(FLAY)
            .other(victim),
    ] {
        assert!(!slot.proc.qualifies(&other), "{other:?}");
    }
    let landed = effect_aura(&slot, priest, &hit).unwrap();
    assert_eq!(landed.target, victim);
    assert_eq!(
        (
            landed.aura.effect_type,
            landed.aura.duration,
            landed.aura.dispel_type,
            landed.aura.break_on_damage_threshold,
            landed.aura.caster,
            landed.aura.ability_name.as_str(),
        ),
        (
            AuraType::Stun,
            3.0,
            DispelType::Magic,
            -1.0,
            Some(priest),
            "Blackout"
        )
    );
}

const T1: Unit = Unit::new(1, 0);
const T1B: Unit = Unit::new(1, 1);
const T1C: Unit = Unit::new(1, 2);
const T2: Unit = Unit::new(2, 0);
const T2B: Unit = Unit::new(2, 1);

fn lines<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines().filter(|l| l.contains(needle)).collect()
}

/// The time of the first log line containing `needle`.
fn first_at(log: &str, needle: &str) -> Option<f32> {
    let line = log.lines().find(|l| l.contains(needle))?;
    stamp(line)
}

fn stamp(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line.find("s]")?;
    line[open + 1..close].trim().parse().ok()
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

fn weakened() -> Aura {
    Aura {
        effect_type: AuraType::WeakenedSoul,
        duration: 15.0,
        ability_name: "Weakened Soul".into(),
        ..Default::default()
    }
}

/// A healing Priest whose Mind Blast lands on an enemy Mage, with its
/// Blackout's chance staged to certain (`certain`) or the proc taken off (the
/// control): the Mind Blast's landing stuns the Mage, and the control's
/// does not. The site (`process_casting`'s cast landing) pushes the Priest's
/// `SpellHit`.
#[test]
fn a_mind_blast_that_lands_stuns_through_blackout() {
    let run = |certain: bool| {
        let config = HeadlessMatchConfig {
            team1: vec!["Priest".into()],
            team2: vec!["Mage".into()],
            random_seed: Some(5),
            max_duration_secs: 4.0,
            team1_builds: vec![Build::of(&[(ImprovedMindBlast, 2), (Blackout, 2)])],
            ..Default::default()
        };
        let stage = Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T2, 25.0, 0.0)
            .target(T1, T2)
            .aura(T1, weakened())
            .combatant(T1, move |c| {
                if certain {
                    c.talent_procs.slots[0].proc.chance = ProcChance::Chance(1.0);
                } else {
                    c.talent_procs.slots.clear();
                }
            });
        play(config, stage).log
    };
    let log = run(true);
    let blast = lines(&log, "Priest #1's Mind Blast hits Team 2 Mage #1");
    assert!(!blast.is_empty(), "{log}");
    let proc =
        first_at(&log, "Priest #1's Blackout procs Blackout").unwrap_or_else(|| panic!("{log}"));
    assert!((proc - stamp(blast[0]).unwrap()).abs() < 0.1, "{log}");
    assert!(
        log.contains("[CC] Blackout on Team 2 Mage #1 (3.0s, DR: 100%)"),
        "{log}"
    );
    let control = run(false);
    assert!(
        !lines(&control, "Priest #1's Mind Blast hits").is_empty(),
        "{control}"
    );
    assert!(!control.contains("procs Blackout"), "{control}");
    assert!(!control.contains("[CC] Blackout"), "{control}");
}

// ---------------------------------------------------------------------------
// Scenes
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

fn considered(view: &DecisionView, ability: AbilityType) -> bool {
    view.candidates.iter().any(|c| c.ability == ability)
}

fn note(view: &DecisionView, ability: AbilityType) -> Option<String> {
    match view.rejected(ability) {
        Some(RejectionReason::PreconditionUnmet { note }) => Some(note.clone()),
        _ => None,
    }
}

fn fortitude() -> Aura {
    Aura {
        effect_type: AuraType::MaxHealthIncrease,
        duration: 600.0,
        ability_name: "Power Word: Fortitude".into(),
        ..Default::default()
    }
}

/// A healthy healing Priest (Fortitude up, Weakened Soul so it shields no
/// one) with a teammate of `class` 10 yards off and an enemy Warrior at
/// `chaser_x` targeting it; the Priest's own target is that Warrior.
fn peel_scene(class: CharacterClass, chaser_x: f32) -> Scene {
    Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .targeting(ENEMY)
        .unit(ALLY, 1, class)
        .at(10.0, 0.0)
        .aura_full(ALLY, fortitude())
        .aura(ALLY, AuraType::WeakenedSoul, 10.0)
        .unit(ENEMY, 2, Warrior)
        .at(chaser_x, 0.0)
        .targeting(ALLY)
}

// ---- Mind Flay, the healing Priest's peel --------------------------------

/// An enemy Warrior on the Priest's Mage: Mind Flay slows it. On its Warrior
/// teammate (the control), a melee wants no escape: the peel is not owed, and
/// Mind Blast goes out instead.
#[test]
fn mind_flay_peels_a_melee_off_a_ranged_teammate() {
    let on_mage = peel_scene(Mage, 11.5).grant(PRIEST, FLAY).decide(PRIEST);
    assert_eq!(
        on_mage.chose(),
        Some((FLAY, Some(ENEMY))),
        "{}",
        describe(&on_mage)
    );
    let on_warrior = peel_scene(Warrior, 11.5).grant(PRIEST, FLAY).decide(PRIEST);
    assert!(
        note(&on_warrior, FLAY).is_some_and(|n| n.contains("no enemy melee or pet")),
        "{}",
        describe(&on_warrior)
    );
    assert_eq!(on_warrior.chose(), Some((MindBlast, Some(ENEMY))));
}

/// The Priest is a ranged teammate of its own: a Warrior on it is peeled.
#[test]
fn mind_flay_peels_a_melee_off_the_priest() {
    let view = Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .cooldown(PRIEST, AbilityType::PsychicScream, 20.0)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Warrior)
        .at(1.5, 0.0)
        .targeting(PRIEST)
        .grant(PRIEST, FLAY)
        .decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((FLAY, Some(ENEMY))),
        "{}",
        describe(&view)
    );
}

/// Only a chaser ON its teammate (within the peel's reach of it) is slowed: a
/// Warrior 10 yards short of the Mage is not; one 7 yards short is.
#[test]
fn mind_flay_peels_only_a_chaser_on_its_teammate() {
    assert_eq!(MIND_FLAY_PEEL_REACH, 8.0);
    let near = peel_scene(Mage, 3.0).grant(PRIEST, FLAY).decide(PRIEST);
    assert_eq!(
        near.chose(),
        Some((FLAY, Some(ENEMY))),
        "{}",
        describe(&near)
    );
    let short = peel_scene(Mage, 0.0)
        .place(ENEMY, 0.0, 10.0)
        .grant(PRIEST, FLAY)
        .decide(PRIEST);
    assert!(note(&short, FLAY).is_some(), "{}", describe(&short));
}

/// An enemy caster on the Mage is not a chaser to slow; a Warrior is.
#[test]
fn mind_flay_peels_a_melee_not_a_caster() {
    let caster = Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .targeting("enemy mage")
        .unit(ALLY, 1, Mage)
        .at(10.0, 0.0)
        .aura_full(ALLY, fortitude())
        .aura(ALLY, AuraType::WeakenedSoul, 10.0)
        .unit("enemy mage", 2, Mage)
        .at(12.0, 0.0)
        .targeting(ALLY)
        .grant(PRIEST, FLAY)
        .decide(PRIEST);
    assert!(note(&caster, FLAY).is_some(), "{}", describe(&caster));
}

/// The nearest chaser is slowed: two Warriors on the Mage, the second nearer
/// the Priest.
#[test]
fn mind_flay_peels_the_nearest_chaser() {
    let view = peel_scene(Mage, 13.0)
        .unit(ENEMY2, 2, Warrior)
        .at(9.0, 1.0)
        .targeting(ALLY)
        .grant(PRIEST, FLAY)
        .decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((FLAY, Some(ENEMY2))),
        "{}",
        describe(&view)
    );
    let far = peel_scene(Mage, 9.0)
        .unit(ENEMY2, 2, Warrior)
        .at(13.0, 1.0)
        .targeting(ALLY)
        .grant(PRIEST, FLAY)
        .decide(PRIEST);
    assert_eq!(far.chose(), Some((FLAY, Some(ENEMY))), "{}", describe(&far));
}

/// A chaser already slowed, rooted, held in crowd control or immune to
/// damage is passed over; each against the plain chaser, which is slowed.
#[test]
fn mind_flay_passes_over_a_chaser_it_need_not_slow() {
    for (effect, why) in [
        (AuraType::MovementSpeedSlow, "slowed"),
        (AuraType::Root, "rooted"),
        (AuraType::Stun, "stunned"),
        (AuraType::DamageImmunity, "immune"),
    ] {
        let view = peel_scene(Mage, 11.5)
            .aura(ENEMY, effect, 5.0)
            .grant(PRIEST, FLAY)
            .decide(PRIEST);
        assert_ne!(
            view.chose().map(|(a, _)| a),
            Some(FLAY),
            "{why}: {}",
            describe(&view)
        );
    }
}

/// An immune chaser is passed over for the next: two Warriors on the Mage,
/// the nearer under Divine Shield, and the farther is slowed. With neither
/// immune (the control), the nearer is.
#[test]
fn mind_flay_passes_an_immune_chaser_for_the_next() {
    let run = |immune: bool| {
        let mut scene = peel_scene(Mage, 13.0)
            .unit(ENEMY2, 2, Warrior)
            .at(9.0, 1.0)
            .targeting(ALLY)
            .grant(PRIEST, FLAY);
        if immune {
            scene = scene.aura(ENEMY2, AuraType::DamageImmunity, 5.0);
        }
        scene.decide(PRIEST)
    };
    let immune = run(true);
    assert_eq!(
        immune.chose(),
        Some((FLAY, Some(ENEMY))),
        "{}",
        describe(&immune)
    );
    assert_eq!(run(false).chose(), Some((FLAY, Some(ENEMY2))));
}

/// A heal owed and not yet castable holds the peel: the Priest's Warrior
/// teammate below Flash Heal's trigger, 45 yards off (beyond its reach).
/// With the Warrior topped (the control), the peel goes.
#[test]
fn mind_flay_waits_while_a_heal_is_owed() {
    let run = |hp: f32| {
        peel_scene(Mage, 11.5)
            .unit("warrior", 1, Warrior)
            .at(-45.0, 0.0)
            .health(hp)
            .aura_full("warrior", fortitude())
            .aura("warrior", AuraType::WeakenedSoul, 10.0)
            .grant(PRIEST, FLAY)
            .decide(PRIEST)
    };
    let owed = run(0.8);
    assert!(
        note(&owed, FLAY).is_some_and(|n| n.contains("a heal is owed")),
        "{}",
        describe(&owed)
    );
    let topped = run(1.0);
    assert_eq!(
        topped.chose(),
        Some((FLAY, Some(ENEMY))),
        "{}",
        describe(&topped)
    );
}

/// An ESCAPE window owns the Priest's legs: the channel would plant it, so
/// the peel is deferred. FREE (the control), it goes.
#[test]
fn mind_flay_waits_while_a_walk_owns_the_priests_legs() {
    use arenasim::states::play_match::components::{HealerPosture, Posture};
    let run = |posture: Posture| {
        let mut state = HealerPosture::new(9.0);
        state.posture = posture;
        state.hold_until = 20.0;
        state.escape_until = 20.0;
        peel_scene(Mage, 11.5)
            .now(10.0)
            .healer_posture(PRIEST, state)
            .grant(PRIEST, FLAY)
            .decide(PRIEST)
    };
    let escaping = run(Posture::Escape);
    assert!(
        note(&escaping, FLAY).is_some_and(|n| n.contains("walk live")),
        "{}",
        describe(&escaping)
    );
    let free = run(Posture::Free);
    assert_eq!(
        free.chose(),
        Some((FLAY, Some(ENEMY))),
        "{}",
        describe(&free)
    );
}

/// A chaser beyond Mind Flay's 20 yards is reported out of range, not as no
/// chaser at all; in range (the control) it is slowed.
#[test]
fn mind_flay_reports_a_chaser_out_of_reach() {
    let far = peel_scene(Mage, 11.5)
        .place(ALLY, 24.0, 0.0)
        .place(ENEMY, 25.0, 0.0)
        .grant(PRIEST, FLAY)
        .decide(PRIEST);
    assert!(
        matches!(far.rejected(FLAY), Some(RejectionReason::OutOfRange { .. })),
        "{}",
        describe(&far)
    );
    let near = peel_scene(Mage, 11.5).grant(PRIEST, FLAY).decide(PRIEST);
    assert_eq!(near.chose(), Some((FLAY, Some(ENEMY))));
}

/// A chaser the Priest's team holds in a crowd control any damage breaks (a
/// Polymorph) is never flayed: the cast gate's friendly-CC guard.
#[test]
fn mind_flay_never_breaks_the_teams_crowd_control() {
    let mut scene = peel_scene(Mage, 11.5).grant(PRIEST, FLAY);
    let mage = scene.entity(ALLY);
    scene = scene.aura_full(
        ENEMY,
        Aura {
            effect_type: AuraType::Polymorph,
            duration: 2.0,
            break_on_damage_threshold: 0.0,
            caster: Some(mage),
            ability_name: "Polymorph".into(),
            ..Default::default()
        },
    );
    let view = scene.decide(PRIEST);
    assert_ne!(
        view.chose().map(|(a, _)| a),
        Some(FLAY),
        "{}",
        describe(&view)
    );
}

// ---- Mind Flay, the Shadowform filler --------------------------------------

/// A Shadowform Priest, topped, its kill target an enemy Mage at `x`.
fn rotation(x: f32) -> Scene {
    Scene::new()
        .unit(PRIEST, 1, Priest)
        .targeting("enemy mage")
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
        .unit("enemy mage", 2, Mage)
        .at(x, 0.0)
        .in_shadowform(PRIEST)
}

/// [`rotation`] with the Priest's own Shadow Word: Pain on the Mage.
fn pain_up(scene: Scene) -> Scene {
    let me = scene.entity(PRIEST);
    scene.aura_full(
        "enemy mage",
        Aura {
            effect_type: AuraType::DamageOverTime,
            duration: 15.0,
            magnitude: 10.0,
            tick_interval: 3.0,
            ability_name: "Shadow Word: Pain".into(),
            caster: Some(me),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        },
    )
}

/// With Shadow Word: Pain up and Mind Blast cooling down, Mind Flay is the
/// filler on the kill target; beyond its 20 yards (the control) it is out of
/// range, and the rotation walks on.
#[test]
fn mind_flay_is_the_shadowform_filler() {
    let run = |x: f32| {
        pain_up(rotation(x))
            .cooldown(PRIEST, MindBlast, 5.0)
            .grant(PRIEST, FLAY)
            .decide(PRIEST)
    };
    let near = run(18.0);
    assert_eq!(
        near.chose(),
        Some((FLAY, Some("enemy mage"))),
        "{}",
        describe(&near)
    );
    let far = run(25.0);
    assert!(
        matches!(far.rejected(FLAY), Some(RejectionReason::OutOfRange { .. })),
        "{}",
        describe(&far)
    );
    // Mind Blast ready goes first: the filler is below it.
    let blast = pain_up(rotation(18.0)).grant(PRIEST, FLAY).decide(PRIEST);
    assert_eq!(blast.chose(), Some((MindBlast, Some("enemy mage"))));
}

/// The filler never breaks the team's crowd control either.
#[test]
fn the_filler_never_breaks_the_teams_crowd_control() {
    let mut scene = pain_up(rotation(18.0))
        .cooldown(PRIEST, MindBlast, 5.0)
        .unit("mage", 1, Mage)
        .at(5.0, 5.0)
        .aura_full("mage", fortitude())
        .aura("mage", AuraType::WeakenedSoul, 15.0)
        .grant(PRIEST, FLAY);
    let mage = scene.entity("mage");
    scene = scene.aura_full(
        "enemy mage",
        Aura {
            effect_type: AuraType::Polymorph,
            duration: 2.0,
            break_on_damage_threshold: 0.0,
            caster: Some(mage),
            ability_name: "Polymorph".into(),
            ..Default::default()
        },
    );
    let view = scene.decide(PRIEST);
    assert!(
        matches!(
            view.rejected(FLAY),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "{}",
        describe(&view)
    );
}

/// A Shadowform Priest with Mind Flay or Silence stops its pursuit inside
/// their 20-yard reach; without either, with them locked, or out of the form,
/// it keeps its class's range.
#[test]
fn a_shadowform_priest_closes_to_mind_flays_reach() {
    let defs = AbilityDefinitions::shipped();
    let reach = defs.get_unchecked(&FLAY).range - MIND_FLAY_RANGE_BUFFER;
    assert_eq!(
        defs.get_unchecked(&SILENCE).range,
        defs.get_unchecked(&FLAY).range
    );
    let priest = |form: PriestForm, grants: &[AbilityType], locked: &[AbilityType]| {
        let mut book = Combatant::new(1, 0, Priest).book().clone();
        for a in grants {
            book = book.granting(defs, *a);
        }
        let book = book.locking(&locked.iter().copied().collect());
        let mut c = Combatant::new(1, 0, Priest).with_book(book);
        c.priest_form = form;
        shadow_pursuit_range(&c)
    };
    let form = PriestForm::Shadowform;
    assert_eq!(priest(form, &[FLAY], &[]), Some(reach));
    assert_eq!(priest(form, &[SILENCE], &[]), Some(reach));
    assert_eq!(priest(form, &[FLAY, SILENCE], &[]), Some(reach));
    assert_eq!(priest(form, &[], &[]), None);
    assert_eq!(priest(form, &[SILENCE], &[SILENCE]), None);
    assert_eq!(priest(PriestForm::NoForm, &[FLAY, SILENCE], &[]), None);
}

// ---- Vampiric Embrace -------------------------------------------------------

/// The Priest's own Vampiric Embrace on the Mage, from `caster`.
fn embraced(scene: Scene, caster: &'static str) -> Scene {
    let from = scene.entity(caster);
    let def = AbilityDefinitions::shipped()
        .get_unchecked(&EMBRACE)
        .clone();
    let pending = arenasim::states::play_match::components::AuraPending::from_ability(
        scene.entity("enemy mage"),
        from,
        &def,
    )
    .expect("Vampiric Embrace applies its leech");
    scene.aura_full("enemy mage", pending.aura)
}

/// In the Shadowform rotation, Vampiric Embrace goes on the kill target after
/// Shadow Word: Pain and ahead of Mind Blast; with its own already there (the
/// control), it is not recast and Mind Blast goes.
#[test]
fn vampiric_embrace_precedes_the_shadowform_damage() {
    let fresh = pain_up(rotation(20.0))
        .grant(PRIEST, EMBRACE)
        .decide(PRIEST);
    assert_eq!(
        fresh.chose(),
        Some((EMBRACE, Some("enemy mage"))),
        "{}",
        describe(&fresh)
    );
    let up = embraced(pain_up(rotation(20.0)).grant(PRIEST, EMBRACE), PRIEST).decide(PRIEST);
    assert!(matches!(
        up.rejected(EMBRACE),
        Some(RejectionReason::AlreadyApplied)
    ));
    assert_eq!(up.chose(), Some((MindBlast, Some("enemy mage"))));
}

/// Another Priest's Vampiric Embrace on the target is not this Priest's: its
/// party is healed only by its own, so it embraces anyway.
#[test]
fn another_priests_embrace_is_not_the_priests_own() {
    let scene = pain_up(rotation(20.0))
        .unit("other priest", 1, Priest)
        .at(-5.0, 0.0)
        .aura_full("other priest", fortitude())
        .aura("other priest", AuraType::WeakenedSoul, 15.0)
        .grant(PRIEST, EMBRACE);
    let view = embraced(scene, "other priest").decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((EMBRACE, Some("enemy mage"))),
        "{}",
        describe(&view)
    );
}

/// A target the team holds in breakable crowd control, or one immune to
/// damage, is not embraced (the cast gate's guards); the plain target is.
#[test]
fn vampiric_embrace_respects_the_cast_gates_guards() {
    let mut cc = pain_up(rotation(20.0))
        .unit("mage", 1, Mage)
        .at(5.0, 5.0)
        .aura_full("mage", fortitude())
        .aura("mage", AuraType::WeakenedSoul, 15.0)
        .grant(PRIEST, EMBRACE);
    let mage = cc.entity("mage");
    cc = cc.aura_full(
        "enemy mage",
        Aura {
            effect_type: AuraType::Polymorph,
            duration: 2.0,
            break_on_damage_threshold: 0.0,
            caster: Some(mage),
            ability_name: "Polymorph".into(),
            ..Default::default()
        },
    );
    let view = cc.decide(PRIEST);
    assert!(
        matches!(
            view.rejected(EMBRACE),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "{}",
        describe(&view)
    );
    let immune = pain_up(rotation(20.0))
        .aura("enemy mage", AuraType::DamageImmunity, 5.0)
        .grant(PRIEST, EMBRACE)
        .decide(PRIEST);
    assert!(
        matches!(
            immune.rejected(EMBRACE),
            Some(RejectionReason::TargetImmune)
        ),
        "{}",
        describe(&immune)
    );
}

/// A healing Priest with nothing owed embraces its target ahead of Mind
/// Blast; with a heal owed that it cannot cast yet (the control: its Warrior
/// below Flash Heal's trigger, beyond its reach), the embrace waits.
#[test]
fn a_healing_priest_embraces_once_nothing_is_owed() {
    let run = |hp: f32| {
        Scene::new()
            .unit(PRIEST, 1, Priest)
            .aura_full(PRIEST, fortitude())
            .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
            .targeting(ENEMY)
            .unit("warrior", 1, Warrior)
            .at(-45.0, 0.0)
            .health(hp)
            .aura_full("warrior", fortitude())
            .aura("warrior", AuraType::WeakenedSoul, 10.0)
            .unit(ENEMY, 2, Warrior)
            .at(25.0, 0.0)
            .grant(PRIEST, EMBRACE)
            .decide(PRIEST)
    };
    let idle = run(1.0);
    assert_eq!(
        idle.chose(),
        Some((EMBRACE, Some(ENEMY))),
        "{}",
        describe(&idle)
    );
    let owed = run(0.8);
    assert!(
        note(&owed, EMBRACE).is_some_and(|n| n.contains("a heal is owed")),
        "{}",
        describe(&owed)
    );
}

// ---- Silence -----------------------------------------------------------------

const HEALER: &str = "enemy priest";
const CASTER: &str = "enemy mage";

/// A Shadowform Priest with Silence, topped, its target an enemy Warrior 10
/// yards off; an enemy Mage 15 yards off and an enemy Priest 16 yards off,
/// each casting when named in `casting` (the Mage first in entity order).
fn silence_scene(casting: &[&'static str]) -> Scene {
    let mut scene = Scene::new()
        .unit(PRIEST, 1, Priest)
        .targeting(ENEMY)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
        .unit(CASTER, 2, Mage)
        .at(15.0, 0.0)
        .unit(HEALER, 2, Priest)
        .at(16.0, 3.0)
        .unit(ENEMY, 2, Warrior)
        .at(10.0, -3.0);
    for who in casting {
        let spell = if *who == HEALER {
            FlashHeal
        } else {
            AbilityType::Frostbolt
        };
        scene = scene.casting(who, spell);
    }
    scene.in_shadowform(PRIEST).grant(PRIEST, SILENCE)
}

/// An enemy healer mid-cast is silenced; with no one casting (the control)
/// Silence waits, with its reason.
#[test]
fn silence_takes_an_enemy_healer_mid_cast() {
    let view = silence_scene(&[HEALER]).decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((SILENCE, Some(HEALER))),
        "{}",
        describe(&view)
    );
    let idle = silence_scene(&[]).decide(PRIEST);
    assert!(
        note(&idle, SILENCE).is_some_and(|n| n.contains("mid-cast")),
        "{}",
        describe(&idle)
    );
}

/// A healer first: with the Mage (first in entity order) and the Priest both
/// casting, the healer is silenced; with the Mage alone casting (the
/// control), the Mage is.
#[test]
fn silence_takes_a_healer_first_then_a_caster() {
    let both = silence_scene(&[CASTER, HEALER]).decide(PRIEST);
    assert_eq!(
        both.chose(),
        Some((SILENCE, Some(HEALER))),
        "{}",
        describe(&both)
    );
    let mage = silence_scene(&[CASTER]).decide(PRIEST);
    assert_eq!(
        mage.chose(),
        Some((SILENCE, Some(CASTER))),
        "{}",
        describe(&mage)
    );
}

/// Passed over: a healer immune to silences (Aura Mastery), already
/// silenced, immune to the Silence category's diminishing returns, or immune
/// to damage. Each scene's Mage casts too, and is silenced instead.
#[test]
fn silence_passes_over_a_healer_it_cannot_hold() {
    let immune_dr = {
        let mut tracker = DRTracker::default();
        for _ in 0..3 {
            tracker.apply(DRCategory::Silence);
        }
        assert!(tracker.is_immune(DRCategory::Silence));
        tracker
    };
    let cases: Vec<(&str, Scene)> = vec![
        (
            "Aura Mastery",
            silence_scene(&[CASTER, HEALER]).aura(HEALER, AuraType::SilenceInterruptImmunity, 5.0),
        ),
        (
            "silenced",
            silence_scene(&[CASTER, HEALER]).aura(HEALER, AuraType::Silence, 3.0),
        ),
        (
            "DR immune",
            silence_scene(&[CASTER, HEALER]).dr_tracker(HEALER, immune_dr),
        ),
        (
            "Divine Shield",
            silence_scene(&[CASTER, HEALER]).aura(HEALER, AuraType::DamageImmunity, 5.0),
        ),
    ];
    for (why, scene) in cases {
        let view = scene.decide(PRIEST);
        assert_eq!(
            view.chose(),
            Some((SILENCE, Some(CASTER))),
            "{why}: {}",
            describe(&view)
        );
    }
}

/// A cast a silence does not stop is not silenced: a Hunter's Aimed Shot
/// (Pacify). A Warlock's Shadow Bolt (the control) is.
#[test]
fn silence_takes_only_a_cast_it_stops() {
    let run = |class: CharacterClass, spell: AbilityType| {
        Scene::new()
            .unit(PRIEST, 1, Priest)
            .targeting("enemy")
            .aura_full(PRIEST, fortitude())
            .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
            .unit("enemy", 2, class)
            .at(15.0, 0.0)
            .casting("enemy", spell)
            .in_shadowform(PRIEST)
            .grant(PRIEST, SILENCE)
            .decide(PRIEST)
    };
    let aimed = run(Hunter, AbilityType::AimedShot);
    assert!(note(&aimed, SILENCE).is_some(), "{}", describe(&aimed));
    let bolt = run(Warlock, Shadowbolt);
    assert_eq!(
        bolt.chose(),
        Some((SILENCE, Some("enemy"))),
        "{}",
        describe(&bolt)
    );
}

/// A caster beyond Silence's 20 yards is reported out of range.
#[test]
fn silence_reports_a_caster_out_of_reach() {
    let far = silence_scene(&[HEALER])
        .place(HEALER, 26.0, 0.0)
        .decide(PRIEST);
    assert!(
        matches!(
            far.rejected(SILENCE),
            Some(RejectionReason::OutOfRange { .. })
        ),
        "{}",
        describe(&far)
    );
}

/// Silence is the Shadowform rotation's; the healing chain never decides it,
/// granted or not.
#[test]
fn a_healing_priest_never_silences() {
    let view = Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
        .unit(HEALER, 2, Priest)
        .at(15.0, 0.0)
        .casting(HEALER, FlashHeal)
        .priest_form(PRIEST, PriestForm::NoForm)
        .grant(PRIEST, SILENCE)
        .decide(PRIEST);
    assert!(!considered(&view, SILENCE), "{}", describe(&view));
}

// ---- Aura Mastery answers Silence ------------------------------------------

/// A Holy Paladin with Aura Mastery, its Mage 10 yards off casting a
/// Frostbolt, and an enemy Shadowform Priest at `x` (Silence reaches 20
/// yards, and Aura Mastery reads 5 beyond): `silence` names whether the
/// Priest holds a Silence, and `ready` whether it is off cooldown.
fn mastery_scene(x: f32, silence: bool, form: PriestForm, ready: bool) -> Scene {
    let devotion = Aura {
        effect_type: AuraType::DamageTakenReduction,
        duration: 600.0,
        magnitude: 0.10,
        ability_name: "Devotion Aura".into(),
        ..Default::default()
    };
    let mut scene = Scene::new()
        .unit("paladin", 1, Paladin)
        .aura_full("paladin", devotion)
        .unit("mage", 1, Mage)
        .at(10.0, 0.0)
        .casting("mage", AbilityType::Frostbolt)
        .unit("enemy priest", 2, Priest)
        .at(x, 0.0)
        .targeting("mage")
        .priest_form("enemy priest", form)
        .grant("paladin", AbilityType::AuraMastery);
    if silence {
        scene = scene.grant("enemy priest", SILENCE);
    }
    if !ready {
        scene = scene.cooldown("enemy priest", SILENCE, 30.0);
    }
    scene
}

/// An enemy Shadowform Priest about to Silence the Paladin's casting Mage
/// raises Aura Mastery. The controls: no Silence; its Silence cooling down;
/// out of its reach; or a healing Priest's Silence, locked without the form.
#[test]
fn aura_mastery_answers_an_enemys_silence() {
    let mastery = AbilityType::AuraMastery;
    let threatened = mastery_scene(30.0, true, PriestForm::Shadowform, true).decide("paladin");
    assert_eq!(
        threatened.chose(),
        Some((mastery, Some("mage"))),
        "{}",
        describe(&threatened)
    );
    for (why, scene) in [
        (
            "no Silence",
            mastery_scene(30.0, false, PriestForm::Shadowform, true),
        ),
        (
            "cooling down",
            mastery_scene(30.0, true, PriestForm::Shadowform, false),
        ),
        (
            "out of reach",
            mastery_scene(40.0, true, PriestForm::Shadowform, true),
        ),
        (
            "locked",
            mastery_scene(30.0, true, PriestForm::NoForm, true),
        ),
    ] {
        let view = scene.decide("paladin");
        assert!(
            note(&view, mastery).is_some_and(|n| n.contains("about to interrupt")),
            "{why}: {}",
            describe(&view)
        );
    }
}

// ---------------------------------------------------------------------------
// Mind Flay's slow ends with its channel (`process_channeling`)
// ---------------------------------------------------------------------------

/// How a staged Mind Flay channel goes in [`channel_slow_after`].
#[derive(Clone, Copy, PartialEq, Debug)]
enum Channel {
    /// Still running, 2s left.
    Running,
    /// The Priest is stunned: crowd control breaks the channel.
    Stunned,
    /// The Priest is silenced.
    Silenced,
    /// An interrupt has marked the channel.
    Interrupted,
    /// Its last moment: it completes on the next frame.
    Ending,
    /// The channel is gone: a kill site stripped its dead Priest's
    /// `ChannelingState`, as every kill site does.
    Gone,
}

/// A Priest channelling Mind Flay on a Warrior that carries the channel's
/// slow and a second Priest's same-named slow, run through the real
/// `process_channeling` for a few frames as `how` says. Returns which of the
/// two slows are still on the Warrior: (its own, the other Priest's).
fn channel_slow_after(how: Channel) -> (bool, bool) {
    let defs = AbilityDefinitions::shipped();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(ArenaDampening::default())
        .add_systems(Update, process_channeling);
    let book = Combatant::new(1, 0, Priest)
        .book()
        .clone()
        .granting(defs, FLAY);
    let world = app.world_mut();
    let other = world.spawn_empty().id();
    let warrior = world
        .spawn((
            Transform::from_xyz(10.0, 0.0, 0.0),
            Combatant::new(2, 0, Warrior),
            ActiveAuras::default(),
        ))
        .id();
    let mut priest_auras = Vec::new();
    match how {
        Channel::Stunned => priest_auras.push(Aura {
            effect_type: AuraType::Stun,
            duration: 3.0,
            ability_name: "Hammer of Justice".into(),
            ..Default::default()
        }),
        Channel::Silenced => priest_auras.push(Aura {
            effect_type: AuraType::Silence,
            duration: 3.0,
            ability_name: "Silence".into(),
            ..Default::default()
        }),
        _ => {}
    }
    let channel = |ending: bool, interrupted: bool| ChannelingState {
        ability: FLAY,
        duration_remaining: if ending { 0.001 } else { 2.0 },
        time_until_next_tick: 0.5,
        tick_interval: 1.0,
        target: warrior,
        interrupted,
        interrupted_display_time: 0.5,
        ticks_applied: 1,
    };
    let mut channeler = Combatant::new(1, 0, Priest).with_book(book.clone());
    if how == Channel::Gone {
        channeler.current_health = 0.0;
    }
    let priest = world
        .spawn((
            Transform::default(),
            channeler,
            ActiveAuras {
                auras: priest_auras,
            },
        ))
        .id();
    if how != Channel::Gone {
        world
            .entity_mut(priest)
            .insert(channel(how == Channel::Ending, how == Channel::Interrupted));
    }
    // A second Priest flaying the same Warrior, its channel running
    // throughout: its slow is its own channel's.
    world.entity_mut(other).insert((
        Transform::from_xyz(0.0, 0.0, 5.0),
        Combatant::new(1, 1, Priest).with_book(book),
        ActiveAuras::default(),
        channel(false, false),
    ));
    let slow = |caster: Entity| Aura {
        effect_type: AuraType::MovementSpeedSlow,
        duration: 2.0,
        magnitude: 0.5,
        caster: Some(caster),
        ability_name: "Mind Flay".into(),
        ..Default::default()
    };
    world
        .get_mut::<ActiveAuras>(warrior)
        .unwrap()
        .auras
        .extend([slow(priest), slow(other)]);
    for _ in 0..3 {
        app.update();
    }
    let auras = &app.world().get::<ActiveAuras>(warrior).unwrap().auras;
    let has = |caster: Entity| auras.iter().any(|a| a.caster == Some(caster));
    (has(priest), has(other))
}

/// The channel's slow is the channel's: crowd control, a silence, an
/// interrupt, the channel's removal by a kill site or its own end takes it
/// off its target, and only that Priest's: a second Priest still flaying
/// keeps its slow. While the channel runs (the control) both stay. A kill
/// through a real site is played in
/// [`mind_flays_slow_ends_with_its_priests_death`].
#[test]
fn mind_flays_slow_ends_with_its_channel() {
    assert_eq!(channel_slow_after(Channel::Running), (true, true));
    for how in [
        Channel::Stunned,
        Channel::Silenced,
        Channel::Interrupted,
        Channel::Ending,
        Channel::Gone,
    ] {
        assert_eq!(channel_slow_after(how), (false, true), "{how:?}");
    }
}

// ---------------------------------------------------------------------------
// Played
// ---------------------------------------------------------------------------

fn mind_flay_build() -> Build {
    Build::of(&[
        (ImprovedMindBlast, 2),
        (Blackout, 1),
        (TalentId::MindFlay, 1),
    ])
}

/// A healing Priest whose Mage teammate has an enemy Warrior on it, the
/// Priest 12 yards off and its Mind Blast cooling down: it channels Mind
/// Flay on the Warrior for 15 mana, its ticks land 10 each, and the Warrior
/// is slowed while it channels. All three of its ticks land.
#[test]
fn mind_flay_channels_ticks_of_ten() {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Mage".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(4),
        max_duration_secs: 6.0,
        team1_builds: vec![mind_flay_build()],
        ..Default::default()
    };
    let stage = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T1B, 10.0, 0.0)
        .place(T2, 11.5, 0.0)
        .target(T2, T1B)
        .target(T1, T2)
        .cooldown(T1, MindBlast, 30.0)
        .aura(T1, weakened())
        .aura(T1B, weakened());
    let played = play(config, stage);
    let log = &played.log;
    let start = first_at(
        log,
        "Priest #1 begins channeling Mind Flay on Team 2 Warrior #1",
    )
    .unwrap_or_else(|| panic!("{log}"));
    let ticks: Vec<&str> = lines(log, "Priest #1's Mind Flay ticks on Team 2 Warrior #1 for")
        .into_iter()
        .filter(|l| stamp(l).is_some_and(|t| t > start && t <= start + 3.1))
        .collect();
    assert_eq!(ticks.len(), 3, "{log}");
    for t in &ticks {
        assert_eq!(amount(t, " for "), 10.0, "{t}");
    }
    assert!(
        log.contains("[CC] Mind Flay on Team 2 Warrior #1"),
        "the slow lands: {log}"
    );
    let paid = mana_paid_at(&played, start);
    assert!((paid - 15.0).abs() < 0.5, "paid {paid}\n{log}");
}

/// The mana the Priest (team 1, slot 0) paid at `at`: its largest one-frame
/// drop in the frames round it.
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

/// A Shadow build that takes Vampiric Embrace (and the form).
fn embrace_build() -> Build {
    Build::of(&[
        (ImprovedMindBlast, 2),
        (TalentId::Shadowform, 1),
        (TalentId::VampiricEmbrace, 1),
    ])
}

/// A Shadowform Priest, a Warrior and a Hunter (with its pet), each at half
/// health, against an enemy Mage 20 yards off that carries the Priest's
/// Vampiric Embrace when `embraced` (landed through `apply_pending_auras`).
/// `stage` adds to it. The log.
fn embrace_run(embraced: bool, stage: impl FnOnce(Stage) -> Stage) -> common::staged::Played {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into(), "Hunter".into()],
        team2: vec!["Mage".into()],
        random_seed: Some(6),
        // Past a staged clock (the dampening case's 135s): the Mage's death
        // ends it well before.
        max_duration_secs: 160.0,
        // The control's Priest lacks the talent, so casts no embrace of its
        // own.
        team1_builds: vec![if embraced {
            embrace_build()
        } else {
            Build::of(&[(ImprovedMindBlast, 2), (TalentId::Shadowform, 1)])
        }],
        team1_priest_forms: vec!["Shadowform".into()],
        ..Default::default()
    };
    let mut base = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T1B, -5.0, 0.0)
        .place(T1C, -5.0, 5.0)
        .place(T2, 20.0, 0.0)
        .target(T1, T2)
        .health(T1, 0.5)
        .health(T1B, 0.5)
        .health(T1C, 0.5)
        // No Ice Barrier to soak the Shadow damage that feeds the embrace.
        .clear_auras(T2);
    if embraced {
        base = base.land(T2, EMBRACE, T1);
    }
    play(config, stage(base))
}

/// Each heal the embrace landed at a Priest damage line's moment, by
/// recipient: (the Shadow damage the line landed, the heal to `whom`).
fn embrace_pairs(log: &str, whom: &str) -> Vec<(f32, f32)> {
    let damage: Vec<(f32, f32)> = log
        .lines()
        .filter(|l| l.contains("[DMG] Team 1 Priest #1's") && l.contains("Team 2 Mage #1"))
        .filter(|l| {
            ["Shadow Word: Pain", "Mind Blast"]
                .iter()
                .any(|n| l.contains(n))
        })
        .filter(|l| !l.contains("absorbed"))
        .filter_map(|l| Some((stamp(l)?, amount(l, " for "))))
        .collect();
    damage
        .iter()
        .filter_map(|(at, dealt)| {
            let heal = log
                .lines()
                .filter(|l| l.contains(whom) && stamp(l) == Some(*at))
                .map(|l| amount(l, " for "))
                .next()?;
            Some((*dealt, heal))
        })
        .collect()
}

/// The embrace heals the Priest's party — the Priest, the Warrior and the
/// Hunter — for 20% of each piece of Shadow damage the Priest lands on the
/// Mage, through `apply_healing`; the Hunter's pet is healed by no one.
/// Without the embrace (the control), nothing is.
#[test]
fn vampiric_embrace_heals_the_priests_party_for_a_fifth() {
    let played = embrace_run(true, |s| s);
    let log = &played.log;
    for whom in [
        "Priest #1's Vampiric Embrace heals for",
        "Vampiric Embrace heals Team 1 Warrior #2 for",
        "Vampiric Embrace heals Team 1 Hunter #3 for",
    ] {
        let pairs = embrace_pairs(log, whom);
        assert!(pairs.len() >= 2, "{whom}\n{log}");
        for (dealt, heal) in pairs {
            assert!(
                (heal - 0.2 * dealt).abs() <= 1.0,
                "{whom}: {heal} of {dealt}\n{log}"
            );
        }
    }
    assert!(
        !log.lines()
            .any(|l| l.contains("Vampiric Embrace heals") && l.contains("Spider")),
        "a pet is healed\n{log}"
    );
    let control = embrace_run(false, |s| s).log;
    assert!(
        !lines(&control, "[DMG] Team 1 Priest #1's").is_empty(),
        "{control}"
    );
    assert!(!control.contains("Vampiric Embrace heals"), "{control}");
}

/// Arena dampening reaches the embrace's heals: staged halfway up the ramp
/// (135s after the gates), each heals a tenth of the damage, not a fifth.
#[test]
fn vampiric_embraces_heals_are_dampened() {
    let log = embrace_run(true, |s| s.clock(135.0)).log;
    assert!(log.contains("Arena dampening reaches 50%"), "{log}");
    let pairs = embrace_pairs(&log, "Vampiric Embrace heals Team 1 Warrior #2 for");
    assert!(pairs.len() >= 2, "{log}");
    for (dealt, heal) in pairs {
        assert!(
            (heal - 0.1 * dealt).abs() <= 1.0 && heal < 0.2 * dealt - 1.0,
            "{heal} of {dealt}\n{log}"
        );
    }
}

/// Healing reduction reaches the embrace's heals: a Mortal Strike on the
/// Warrior (half its healing left) halves its share, with the Mortal Wounds
/// tell; the Priest's is not cut.
#[test]
fn vampiric_embraces_heals_meet_healing_reduction() {
    let wound = Aura {
        effect_type: AuraType::HealingReduction,
        duration: 30.0,
        magnitude: 0.5,
        ability_name: "Mortal Strike".into(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let played = embrace_run(true, move |s| s.aura(T1B, wound));
    let log = &played.log;
    let warrior = embrace_pairs(log, "Vampiric Embrace heals Team 1 Warrior #2 for");
    assert!(warrior.len() >= 2, "{log}");
    for (dealt, heal) in warrior {
        assert!(
            (heal - 0.1 * dealt).abs() <= 1.0,
            "{heal} of {dealt}\n{log}"
        );
    }
    let priest = embrace_pairs(log, "Priest #1's Vampiric Embrace heals for");
    assert!(priest.len() >= 2, "{log}");
    for (dealt, heal) in priest {
        assert!(
            (heal - 0.2 * dealt).abs() <= 1.0,
            "{heal} of {dealt}\n{log}"
        );
    }
    // Each cut heal raises the Mortal Wounds tell on the Warrior; with no
    // wound (the control) none rises.
    let tells = |played: &common::staged::Played| {
        let last = played.frames.last().expect("frames");
        last.combatants
            .iter()
            .find(|(_, c)| !c.is_pet && c.team == 1 && c.slot == 1)
            .and_then(|(e, _)| last.healing_refused.get(e).copied())
            .unwrap_or(0)
    };
    assert!(tells(&played) > 0, "{log}");
    assert_eq!(tells(&embrace_run(true, |s| s)), 0);
}

/// A Shadow build that reaches Silence, with Vampiric Embrace on the way.
fn silence_build() -> Build {
    Build::of(&[
        (ImprovedMindBlast, 2),
        (ImprovedPsychicScream, 1),
        (TalentId::Shadowform, 1),
        (TalentId::VampiricEmbrace, 1),
        (TalentId::Silence, 1),
    ])
}

/// `priests` Shadowform Priests on team 1 with `build`, 15 yards from an enemy
/// Priest whose Warrior teammate is at a third of its health: the healer
/// casts Flash Heal from the gates on.
fn silence_run(priests: usize, build: Build) -> String {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(); priests],
        team2: vec!["Priest".into(), "Warrior".into()],
        random_seed: Some(8),
        max_duration_secs: 6.0,
        team1_builds: vec![build; priests],
        team1_priest_forms: vec!["Shadowform".into(); priests],
        ..Default::default()
    };
    let mut stage = Stage::new()
        .place(T2, 15.0, 0.0)
        .place(T2B, 40.0, 0.0)
        .health(T2B, 0.35)
        .target(T2B, T1)
        .aura(T2, weakened())
        .aura(T2B, weakened());
    for slot in 0..priests as u8 {
        let me = Unit::new(1, slot);
        stage = stage
            .place(me, 0.0, f32::from(slot))
            .target(me, T2B)
            .aura(me, weakened());
    }
    play(config, stage).log
}

/// The healer's Flash Heal is silenced: the Silence lands on it and stops the
/// cast in progress. Without the talent (the control), no Silence.
#[test]
fn silence_stops_the_healers_cast() {
    let log = silence_run(1, silence_build());
    let cast = first_at(&log, "Priest #1 casts Silence on Team 2 Priest #1")
        .unwrap_or_else(|| panic!("{log}"));
    let stopped = first_at(&log, "Team 2 Priest #1's Flash Heal interrupted by Silence")
        .unwrap_or_else(|| panic!("{log}"));
    assert!(
        stopped >= cast && stopped - cast < 0.1,
        "{cast} -> {stopped}\n{log}"
    );
    let control = silence_run(
        1,
        Build::of(&[(ImprovedMindBlast, 2), (TalentId::Shadowform, 1)]),
    );
    assert!(control.contains("begins casting Flash Heal"), "{control}");
    assert!(!control.contains("Silence"), "{control}");
}

/// Two Shadowform Priests deciding in the frame the healer begins its cast:
/// the first silences it, and the second sees the silence it just landed (the
/// same-frame crowd-control queue), so it does not silence it again.
#[test]
fn a_second_silencer_in_the_frame_sees_the_first_silence() {
    let log = silence_run(2, silence_build());
    let first =
        first_at(&log, "casts Silence on Team 2 Priest #1").unwrap_or_else(|| panic!("{log}"));
    let same_frame = lines(&log, "casts Silence on Team 2 Priest #1")
        .into_iter()
        .filter(|l| stamp(l) == Some(first))
        .count();
    assert_eq!(same_frame, 1, "{log}");
}

/// A Shadowform Priest with Mind Flay, or with Silence and no Mind Flay,
/// closes to their 20-yard reach of its target; without either (the control),
/// it stands at its class's 28 yards. The target is a Warlock 40 yards off, which itself holds at 28.
#[test]
fn a_shadowform_priest_with_mind_flay_closes_in() {
    let closest = |build: Build| {
        let config = HeadlessMatchConfig {
            team1: vec!["Priest".into()],
            team2: vec!["Warlock".into()],
            random_seed: Some(9),
            max_duration_secs: 6.0,
            team1_builds: vec![build],
            team1_priest_forms: vec!["Shadowform".into()],
            ..Default::default()
        };
        let stage = Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T2, 40.0, 0.0)
            .target(T1, T2)
            .target(T2, T1);
        let played = play(config, stage);
        let distance = |f: &arenasim::headless::FrameObservation| {
            let at = |team: u8| {
                f.combatants
                    .values()
                    .find(|c| !c.is_pet && c.team == team)
                    .map(|c| Vec2::new(c.position.x, c.position.z))
                    .unwrap()
            };
            at(1).distance(at(2))
        };
        played
            .staged_frames()
            .map(distance)
            .fold(f32::MAX, f32::min)
    };
    let with = closest(Build::of(&[
        (ImprovedMindBlast, 2),
        (TalentId::Shadowform, 1),
        (TalentId::MindFlay, 1),
    ]));
    assert!(with <= 20.0, "{with}");
    // Silence alone (no Mind Flay) closes to its reach the same way.
    let silence = closest(silence_build());
    assert!(silence <= 20.0, "{silence}");
    let without = closest(Build::of(&[
        (ImprovedMindBlast, 2),
        (TalentId::Shadowform, 1),
    ]));
    assert!(without > 25.0, "{without}");
}

/// An enemy Hunter's pet on the healing Priest is a chaser too: the Priest
/// flays it.
#[test]
fn mind_flay_peels_a_pet() {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Hunter".into()],
        random_seed: Some(4),
        max_duration_secs: 8.0,
        team1_builds: vec![mind_flay_build()],
        ..Default::default()
    };
    let stage = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T1B, -30.0, 0.0)
        .place(T2, 30.0, 0.0)
        .target(T2, T1)
        .target(T1, T2)
        .cooldown(T1, MindBlast, 30.0)
        .cooldown(T1, AbilityType::PsychicScream, 30.0)
        .aura(T1, weakened())
        .aura(T1B, weakened());
    let log = play(config, stage).log;
    assert!(
        lines(&log, "Priest #1 begins channeling Mind Flay on Team 2")
            .iter()
            .any(|l| !l.contains("Hunter")),
        "{log}"
    );
}

/// Vampiric Embrace cools down its 10 seconds and Silence its 45 as each is
/// cast (TBC 15286 and 15487); Mind Flay has no cooldown.
#[test]
fn the_grants_cool_down_as_declared() {
    let cooldown = |c: &Combatant, a: AbilityType| c.ability_cooldowns.get(&a).copied();
    let (view, after) = pain_up(rotation(20.0))
        .grant(PRIEST, EMBRACE)
        .decide_keeping(PRIEST);
    assert_eq!(view.chose().map(|(a, _)| a), Some(EMBRACE));
    assert_eq!(cooldown(&after, EMBRACE), Some(10.0));
    let (view, after) = silence_scene(&[HEALER]).decide_keeping(PRIEST);
    assert_eq!(view.chose().map(|(a, _)| a), Some(SILENCE));
    assert_eq!(cooldown(&after, SILENCE), Some(45.0));
    let (view, after) = pain_up(rotation(18.0))
        .cooldown(PRIEST, MindBlast, 5.0)
        .grant(PRIEST, FLAY)
        .decide_keeping(PRIEST);
    assert_eq!(view.chose().map(|(a, _)| a), Some(FLAY));
    assert!(cooldown(&after, FLAY).is_none_or(|c| c <= 0.0));
}

/// A pet teammate wants no escape: an enemy Warrior on the Hunter's pet is
/// not flayed as the gates open, while one on the Hunter (the control) is.
#[test]
fn mind_flay_does_not_peel_for_a_pet() {
    const PET: Unit = Unit::pet_of(1, 1);
    let flayed_at_gates = |on_pet: bool| {
        let config = HeadlessMatchConfig {
            team1: vec!["Priest".into(), "Hunter".into()],
            team2: vec!["Warrior".into()],
            random_seed: Some(4),
            max_duration_secs: 2.0,
            team1_builds: vec![mind_flay_build()],
            ..Default::default()
        };
        let (victim, other) = if on_pet { (PET, T1B) } else { (T1B, PET) };
        let stage = Stage::new()
            .place(T1, 0.0, 0.0)
            .place(victim, 10.0, 0.0)
            .place(other, -20.0, 0.0)
            .place(T2, 11.5, 0.0)
            .target(T2, victim)
            .target(T1, T2)
            .cooldown(T1, MindBlast, 30.0)
            .aura(T1, weakened())
            .aura(T1B, weakened());
        let log = play(config, stage).log;
        let gates = first_at(&log, "Gates open!").expect("the gates open");
        first_at(&log, "Priest #1 begins channeling Mind Flay").is_some_and(|t| t - gates < 0.1)
    };
    assert!(!flayed_at_gates(true));
    assert!(flayed_at_gates(false));
}

/// A healing Priest flays the Warrior off its Mage (the Mage out of mana, so
/// it slows nothing itself), and a lethal
/// Physical damage over time the Warrior left on the Priest ticks 1.2s after
/// the gates: the Priest dies mid-channel
/// through a real kill site (`process_dot_ticks`), which strips its channel.
/// Mind Flay's slow comes off the Warrior by the frame after the death. With
/// no damage over time (the control) the Priest lives and the slow stays
/// through that same moment, until its channel ends.
#[test]
fn mind_flays_slow_ends_with_its_priests_death() {
    let run = |lethal: bool| {
        let config = HeadlessMatchConfig {
            team1: vec!["Priest".into(), "Mage".into()],
            team2: vec!["Warrior".into()],
            random_seed: Some(4),
            max_duration_secs: 6.0,
            team1_builds: vec![mind_flay_build()],
            ..Default::default()
        };
        let mut stage = Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T1B, 10.0, 0.0)
            .place(T2, 11.5, 0.0)
            .target(T2, T1B)
            .target(T1, T2)
            .mana(T1B, 0.0)
            .cooldown(T1, MindBlast, 30.0)
            .aura(T1, weakened())
            .aura(T1B, weakened());
        if lethal {
            stage = stage.world(|w| {
                let warrior = T2.entity(w);
                let priest = T1.entity(w);
                w.get_mut::<ActiveAuras>(priest).unwrap().auras.push(Aura {
                    effect_type: AuraType::DamageOverTime,
                    duration: 10.0,
                    magnitude: 100_000.0,
                    tick_interval: 1.2,
                    time_until_next_tick: 1.2,
                    caster: Some(warrior),
                    ability_name: "Lethal".into(),
                    // Physical, so the Priest cannot dispel it.
                    spell_school: Some(SpellSchool::Physical),
                    break_on_damage_threshold: -1.0,
                    ..Default::default()
                });
            });
        }
        play(config, stage)
    };
    let unit = |f: &arenasim::headless::FrameObservation, team: u8| {
        f.combatants
            .values()
            .find(|c| !c.is_pet && c.team == team && c.slot == 0)
            .cloned()
            .expect("observed")
    };
    let slowed = |f: &arenasim::headless::FrameObservation| {
        unit(f, 2).aura_types.contains(&AuraType::MovementSpeedSlow)
    };
    let killed = run(true);
    let log = &killed.log;
    let channel =
        first_at(log, "Priest #1 begins channeling Mind Flay").unwrap_or_else(|| panic!("{log}"));
    let frames = &killed.frames;
    let death = frames
        .iter()
        .position(|f| f.gates_open && !unit(f, 1).alive)
        .unwrap_or_else(|| panic!("the Priest never died\n{log}"));
    let died_at = frames[death].sim_time;
    assert!(
        died_at > channel + 0.5 && died_at < channel + 2.5,
        "{channel} -> {died_at}\n{log}"
    );
    // Slowed while the channel ran, before the death...
    assert!(slowed(&frames[death - 5]), "{log}");
    // ...and never again from the frame after it to where the channel would
    // have ended (after which the Mage's Frost Armor may chill it).
    let after: Vec<f32> = frames[death + 1..]
        .iter()
        .take_while(|f| f.sim_time < channel + 3.0)
        .filter(|f| slowed(f))
        .map(|f| f.sim_time)
        .collect();
    assert!(
        after.is_empty(),
        "slowed after the death at {died_at}: {after:?}\n{log}"
    );

    let control = run(false);
    let at = control
        .frames
        .iter()
        .find(|f| f.sim_time >= died_at + 0.1)
        .expect("a frame after the moment");
    assert!(unit(at, 1).alive, "{}", control.log);
    assert!(slowed(at), "{}", control.log);
}
