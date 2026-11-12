//! Per-match choices and the talents that shape them (AS-408): `WidenChoice`,
//! a rank that patches what it added (`FromRank`), a second slot
//! (`AddChoiceSlot`), exclusive choices (`choice_rules`), and the `Mechanic`
//! hook's resolution onto the combatant.
//!
//! The shipped table names few of these yet (the tree cards do; AS-413's
//! Shadowform is the first), so each is pinned on a fixture table over the
//! shipped talent ids, every case paired with its control.

use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::match_config::{
    CharacterClass, PaladinAura, PriestForm, RoguePoison, WarlockCurse, WarlockDemon,
};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::cast_guard::{
    own_state_ok, PreCastOpts, CHOICE_LOCKED_NOTE,
};
use arenasim::states::play_match::class_ai::scene::Scene;
use arenasim::states::play_match::components::{AuraType, Combatant};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::talent_choices::{
    ChoiceMenu, ChoiceOption, ChoiceSlot, SlotChoices,
};
use arenasim::states::play_match::talent_mechanics::TalentMechanic;
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId};

use TalentId::{DemonicEmbrace, FelStamina, UnholyPower};

fn abilities() -> &'static AbilityDefinitions {
    AbilityDefinitions::shipped()
}

/// The fixture's hand-written nodes: Demonic Embrace widens Curse of Tongues
/// at rank 1 and patches it at rank 2 (Retribution Aura's shape); Fel Stamina
/// is a ROGUE node here, opening the off-hand poison slot and carrying Cheat
/// Death; Unholy Power carries Soul Link. Tongues switches Unholy Power off
/// and forbids Drain Life; Death Coil requires Weakness. Every other talent
/// id is a generated filler ([`fixture_text`]), so a talent node added to the
/// game needs no edit here.
const NODES: &str = r#"
        DemonicEmbrace: (
            name: "Demonic Embrace", class: Warlock, tree: Demonology, tier: 1, column: 0,
            icon: "x.jpg", max_rank: 2, budget: Some(11.0),
            per_rank: [
                WidenChoice(WarlockCurse(Tongues)),
                FromRank(2, Patch(on: Ability(CurseOfTongues), field: AuraMagnitude, op: Add(0.25))),
            ],
        ),
        FelStamina: (
            name: "Fel Stamina", class: Rogue, tree: Demonology, tier: 1, column: 0,
            icon: "x.jpg", max_rank: 1, budget: Some(11.0),
            per_rank: [
                AddChoiceSlot(OffHandPoison),
                Mechanic(CheatDeath(health: 0.1, reduction: 0.9, duration: 3.0)),
            ],
        ),
        UnholyPower: (
            name: "Unholy Power", class: Warlock, tree: Demonology, tier: 1, column: 2,
            icon: "x.jpg", max_rank: 2, budget: Some(11.0),
            per_rank: [Mechanic(SoulLink(share: 0.06))],
        ),
        // The shipped Paladin nodes, inert here: every table names every
        // talent, and grants each talent ability once.
        SacredDuty: (
            name: "Sacred Duty", class: Paladin, tree: Protection, tier: 1, column: 0,
            icon: "x.jpg", max_rank: 2, budget: Some(11.0),
            per_rank: [Patch(on: Ability(HammerOfJustice), field: Cooldown, op: Add(-1.0))],
        ),
        ImprovedHammerOfJustice: (
            name: "Improved Hammer of Justice", class: Paladin, tree: Protection, tier: 1,
            column: 2, icon: "x.jpg", max_rank: 2, budget: Some(11.0),
            per_rank: [Patch(on: Ability(HammerOfJustice), field: Cooldown, op: Add(-10.0))],
        ),
        DivineProtection: (
            name: "Divine Protection", class: Paladin, tree: Protection, tier: 2, column: 0,
            icon: "x.jpg", max_rank: 1, budget: Some(11.0),
            per_rank: [Grant(DivineProtection)],
        ),
        RetributionAura: (
            name: "Retribution Aura", class: Paladin, tree: Protection, tier: 2, column: 1,
            icon: "x.jpg", max_rank: 2, budget: Some(11.0),
            per_rank: [WidenChoice(PaladinAura(RetributionAura))],
        ),
        Toughness: (
            name: "Toughness", class: Paladin, tree: Protection, tier: 2, column: 2,
            icon: "x.jpg", max_rank: 2, budget: Some(11.0),
            per_rank: [StatPercent(Armor, 0.10)],
        ),
        BlessingOfSacrifice: (
            name: "Blessing of Sacrifice", class: Paladin, tree: Protection, tier: 3, column: 1,
            icon: "x.jpg", max_rank: 1, budget: Some(11.0),
            per_rank: [Grant(BlessingOfSacrifice)],
        ),
        SacredShield: (
            name: "Sacred Shield", class: Paladin, tree: Protection, tier: 4, column: 1,
            icon: "x.jpg", max_rank: 1, budget: Some(22.0),
            per_rank: [Grant(SacredShield)],
        ),
        // The shipped Priest nodes, which every table must name; inert here.
        ImprovedMindBlast: (
            name: "Improved Mind Blast", class: Priest, tree: Shadow, tier: 1, column: 0,
            icon: "x.jpg", max_rank: 2, budget: Some(11.0),
            per_rank: [Patch(on: Ability(MindBlast), field: Cooldown, op: Add(-1.0))],
        ),
        ImprovedPsychicScream: (
            name: "Improved Psychic Scream", class: Priest, tree: Shadow, tier: 1, column: 2,
            icon: "x.jpg", max_rank: 2, budget: Some(11.0),
            per_rank: [Patch(on: Ability(PsychicScream), field: Cooldown, op: Add(-2.0))],
        ),
        Shadowform: (
            name: "Shadowform", class: Priest, tree: Shadow, tier: 2, column: 1,
            icon: "x.jpg", max_rank: 1, budget: Some(11.0),
            per_rank: [WidenChoice(PriestForm(Shadowform)), Grant(Shadowform)],
        ),
"#;

/// The trees the hand-written nodes sit in: (class, tree, name).
const TREES: &[(&str, &str, &str)] = &[
    ("Warlock", "Demonology", "Demonology"),
    ("Rogue", "Demonology", "Fixture"),
    ("Paladin", "Protection", "Protection"),
    ("Priest", "Shadow", "Shadow"),
];

const RULES: &str = r#"[
        SwitchesOff(option: WarlockCurse(Tongues), talent: UnholyPower),
        Forbids(option: WarlockCurse(Tongues), ability: DrainLife),
        Requires(ability: DeathCoil, option: WarlockCurse(Weakness)),
    ]"#;

/// The whole fixture table: the hand-written [`NODES`], and an inert filler
/// for every talent id they do not name — a table must name every id, and
/// these cases only use a few. A filler sits where its shipped node does and
/// carries only that node's `Grant`s (each talent ability is granted by
/// exactly one node); its other effects are left out, so no filler widens a
/// choice, opens a slot or carries a mechanic the cases do not expect. Read
/// off the shipped table, so a talent node added to the game is a filler here
/// with no edit.
fn fixture_text() -> &'static str {
    static TEXT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TEXT.get_or_init(|| {
        use arenasim::states::play_match::talents::TalentEffect;
        let named = |id: &TalentId| NODES.contains(&format!("\n        {id:?}: (\n"));
        let shipped = TalentDefinitions::shipped();
        let mut trees: Vec<(String, String, String)> = TREES
            .iter()
            .map(|(c, t, n)| (c.to_string(), t.to_string(), n.to_string()))
            .collect();
        let mut fillers = String::new();
        for id in TalentId::all().iter().filter(|id| !named(id)) {
            let node = shipped.get(*id).expect("every id has a shipped node");
            let (class, tree) = (format!("{:?}", node.class), format!("{:?}", node.tree));
            if !trees.iter().any(|(c, t, _)| *c == class && *t == tree) {
                trees.push((class.clone(), tree.clone(), tree.clone()));
            }
            let grants: Vec<String> = node
                .per_rank
                .iter()
                .filter_map(|e| match e {
                    TalentEffect::Grant(ability) => Some(format!("Grant({ability:?})")),
                    _ => None,
                })
                .collect();
            let (max_rank, per_rank) = if grants.is_empty() {
                (node.max_rank, "StatPercent(WeaponDamage, 0.01)".to_string())
            } else {
                (1, grants.join(", "))
            };
            fillers.push_str(&format!(
                "        {id:?}: (name: \"Filler {id:?}\", class: {class}, tree: {tree}, \
                 tier: {}, column: {}, icon: \"x.jpg\", max_rank: {max_rank}, \
                 budget: Some(11.0), per_rank: [{per_rank}]),\n",
                node.tier, node.column
            ));
        }
        let mut by_class: Vec<String> = Vec::new();
        for (class, _, _) in &trees {
            if by_class.iter().any(|c| c == class) {
                continue;
            }
            by_class.push(class.clone());
        }
        let tree_block: String = by_class
            .iter()
            .map(|class| {
                let list: Vec<String> = trees
                    .iter()
                    .filter(|(c, _, _)| c == class)
                    .map(|(_, t, n)| format!("(id: {t}, name: \"{n}\", icon: \"x.jpg\")"))
                    .collect();
                format!("        {class}: [{}],\n", list.join(", "))
            })
            .collect();
        format!(
            "(\n    trees: {{\n{tree_block}    }},\n    talents: {{{NODES}{fillers}    }},\n    \
             choice_rules: {RULES},\n)"
        )
    })
}

fn fixture() -> TalentDefinitions {
    TalentDefinitions::from_ron(fixture_text(), abilities()).unwrap_or_else(|e| panic!("{e}"))
}

fn load_err(from: &str, to: &str) -> String {
    assert!(fixture_text().contains(from), "the fixture has {from:?}");
    match TalentDefinitions::from_ron(&fixture_text().replacen(from, to, 1), abilities()) {
        Ok(_) => panic!("the edit {from:?} -> {to:?} loaded"),
        Err(e) => e,
    }
}

const TONGUES: ChoiceOption = ChoiceOption::WarlockCurse(WarlockCurse::Tongues);
const WEAKNESS: ChoiceOption = ChoiceOption::WarlockCurse(WarlockCurse::Weakness);
const AGONY: ChoiceOption = ChoiceOption::WarlockCurse(WarlockCurse::Agony);

fn warlock(options: &[ChoiceOption]) -> SlotChoices {
    SlotChoices::of(options, &[])
}

// ---------------------------------------------------------------------------
// WidenChoice
// ---------------------------------------------------------------------------

#[test]
fn a_widened_option_is_offered_only_to_a_build_that_takes_its_node() {
    let t = fixture();
    let empty = Build::default();
    let embrace = Build::of(&[(DemonicEmbrace, 1)]);
    assert!(!t.offers(&empty, TONGUES), "hidden without the talent");
    assert!(t.offers(&embrace, TONGUES));
    assert!(
        t.offers(&empty, AGONY) && t.offers(&empty, WEAKNESS),
        "the rest are not hidden"
    );
    assert_eq!(
        t.offered(&empty, ChoiceMenu::WarlockCurse),
        vec![AGONY, WEAKNESS]
    );
    assert_eq!(
        t.offered(&embrace, ChoiceMenu::WarlockCurse),
        vec![AGONY, WEAKNESS, TONGUES]
    );
    // A slot that picks the hidden option without the talent is refused.
    assert!(t
        .validate_choices(CharacterClass::Warlock, &empty, &warlock(&[TONGUES]))
        .is_err());
    assert!(t
        .validate_choices(CharacterClass::Warlock, &embrace, &warlock(&[TONGUES]))
        .is_ok());
    // The shipped table widens three options: the Warlock's Sacrifice
    // (AS-410), the Priest's Shadowform (AS-413) and the Paladin's
    // Retribution Aura (AS-421). Every other option is offered to every build.
    let shipped = TalentDefinitions::shipped();
    let widened = [
        ChoiceOption::WarlockDemon(WarlockDemon::Sacrifice),
        ChoiceOption::PriestForm(PriestForm::Shadowform),
        ChoiceOption::PaladinAura(PaladinAura::RetributionAura),
    ];
    assert_eq!(
        shipped.widened_options(),
        std::collections::BTreeSet::from(widened)
    );
    for menu in ChoiceMenu::ALL {
        let all_but: Vec<ChoiceOption> = menu
            .options()
            .into_iter()
            .filter(|o| !widened.contains(o))
            .collect();
        assert_eq!(shipped.offered(&empty, menu), all_but, "{menu:?}");
    }
}

/// Retribution Aura's shape: rank 1 adds the option, rank 2 patches it.
#[test]
fn a_rank_can_patch_what_the_first_rank_added() {
    let t = fixture();
    let magnitude = |rank: u8| {
        let build = Build::of(&[(DemonicEmbrace, rank)]);
        t.resolve(abilities(), CharacterClass::Warlock, &build)
            .unwrap()
            .book()
            .get_unchecked(&AbilityType::CurseOfTongues)
            .applies_aura
            .as_ref()
            .unwrap()
            .magnitude
    };
    let base = abilities()
        .get_unchecked(&AbilityType::CurseOfTongues)
        .applies_aura
        .as_ref()
        .unwrap()
        .magnitude;
    assert_eq!(magnitude(1), base, "rank 1 only widens");
    assert_eq!(magnitude(2), base + 0.25, "rank 2 patches");
    // Both ranks widen.
    assert!(t.offers(&Build::of(&[(DemonicEmbrace, 2)]), TONGUES));
}

// ---------------------------------------------------------------------------
// A second slot
// ---------------------------------------------------------------------------

#[test]
fn a_second_slot_opens_only_with_its_talent_and_holds_a_different_option() {
    let t = fixture();
    let rogue = CharacterClass::Rogue;
    let poisoner = Build::of(&[(FelStamina, 1)]);
    let crippling = ChoiceOption::RoguePoison(RoguePoison::Crippling);
    let deadly = ChoiceOption::RoguePoison(RoguePoison::Deadly);
    let off = |o| SlotChoices::of(&[crippling], &[(ChoiceSlot::OffHandPoison, o)]);
    assert!(t.opens(&poisoner, ChoiceSlot::OffHandPoison));
    assert!(!t.opens(&Build::default(), ChoiceSlot::OffHandPoison));
    assert!(t.validate_choices(rogue, &poisoner, &off(deadly)).is_ok());
    assert!(
        t.validate_choices(rogue, &Build::default(), &off(deadly))
            .is_err(),
        "no talent, no second slot"
    );
    assert!(
        t.validate_choices(rogue, &poisoner, &off(crippling))
            .is_err(),
        "the off hand's poison differs from the main hand's"
    );
    assert!(
        t.validate_choices(
            rogue,
            &poisoner,
            &SlotChoices::of(&[crippling], &[(ChoiceSlot::OffHandPoison, TONGUES)])
        )
        .is_err(),
        "the slot holds an option of its own menu"
    );
    // Resolved, the spawn writes the off-hand poison onto the Rogue.
    let mut combatant = Combatant::new(1, 0, rogue);
    t.resolve_with_choices(abilities(), rogue, &poisoner, &off(deadly))
        .unwrap()
        .apply(&mut combatant);
    assert_eq!(combatant.rogue_offhand_poison, Some(RoguePoison::Deadly));
    // Control: the slot left empty coats both weapons with the one poison.
    let mut combatant = Combatant::new(1, 0, rogue);
    t.resolve_with_choices(
        abilities(),
        rogue,
        &poisoner,
        &SlotChoices::of(&[crippling], &[]),
    )
    .unwrap()
    .apply(&mut combatant);
    assert_eq!(combatant.rogue_offhand_poison, None);
}

/// The headless entrance checks a slot's choices against its build: the
/// shipped table opens no second slot, so naming an off-hand poison is
/// refused, and leaving it empty is not.
#[test]
fn the_headless_config_checks_choices_against_builds() {
    let config = |off: Vec<Option<String>>| HeadlessMatchConfig {
        team1: vec!["Rogue".into()],
        team2: vec!["Warrior".into()],
        team1_rogue_offhand_poisons: off,
        ..Default::default()
    };
    let err = config(vec![Some("Deadly".into())])
        .to_match_config()
        .expect_err("no build opens the off-hand slot");
    assert!(err.contains("OffHandPoison"), "{err}");
    // An off-hand poison on a slot that is not a Rogue is refused, not
    // ignored.
    let warrior = HeadlessMatchConfig {
        team1: vec!["Warrior".into()],
        team2: vec!["Warrior".into()],
        team1_rogue_offhand_poisons: vec![Some("Deadly".into())],
        ..Default::default()
    };
    let err = warrior
        .to_match_config()
        .expect_err("a Warrior has no poison");
    assert!(err.contains("Rogue's choice"), "{err}");
    let ok = config(vec![None])
        .to_match_config()
        .expect("an empty slot is fine");
    assert_eq!(ok.team1_rogue_offhand_poisons, vec![None]);
    assert_eq!(ok.team2_rogue_offhand_poisons, vec![None]);
}

// ---------------------------------------------------------------------------
// The Mechanic hook
// ---------------------------------------------------------------------------

#[test]
fn a_mechanic_resolves_onto_the_combatant_and_its_ranks_add() {
    let t = fixture();
    let resolved = t
        .resolve(
            abilities(),
            CharacterClass::Warlock,
            &Build::of(&[(UnholyPower, 2)]),
        )
        .unwrap();
    assert!(
        (resolved.mechanics().soul_link().unwrap() - 0.12).abs() < 1e-6,
        "0.06 a rank"
    );
    let mut combatant = Combatant::new(1, 0, CharacterClass::Warlock);
    resolved.apply(&mut combatant);
    assert_eq!(combatant.talent_mechanics, *resolved.mechanics());
    assert_eq!(
        combatant
            .talent_mechanics
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .len(),
        1
    );
    // A Rogue's Cheat Death.
    let rogue = t
        .resolve(
            abilities(),
            CharacterClass::Rogue,
            &Build::of(&[(FelStamina, 1)]),
        )
        .unwrap();
    assert_eq!(rogue.mechanics().cheat_death(), Some((0.1, 0.9, 3.0)));
    // Control: an empty build carries none.
    let empty = t
        .resolve(abilities(), CharacterClass::Warlock, &Build::default())
        .unwrap();
    assert!(empty.mechanics().is_empty());
    let mut combatant = Combatant::new(1, 0, CharacterClass::Warlock);
    empty.apply(&mut combatant);
    assert!(combatant.talent_mechanics.is_empty());
}

// ---------------------------------------------------------------------------
// Exclusive choices
// ---------------------------------------------------------------------------

/// Demonic Sacrifice's shape: an option chosen switches a talent off.
#[test]
fn an_option_chosen_switches_a_talent_off_for_the_match() {
    let t = fixture();
    let build = Build::of(&[(DemonicEmbrace, 1), (UnholyPower, 2)]);
    let with = t
        .resolve_with_choices(
            abilities(),
            CharacterClass::Warlock,
            &build,
            &warlock(&[TONGUES]),
        )
        .unwrap();
    assert!(
        with.mechanics().is_empty(),
        "Tongues switches Unholy Power off"
    );
    let without = t
        .resolve_with_choices(
            abilities(),
            CharacterClass::Warlock,
            &build,
            &warlock(&[AGONY]),
        )
        .unwrap();
    assert!(without.mechanics().soul_link().is_some());
}

/// Shadowform's shape: an option forbids an ability, and an ability requires
/// an option.
#[test]
fn options_lock_the_abilities_they_forbid_or_do_not_allow() {
    let t = fixture();
    let build = Build::of(&[(DemonicEmbrace, 1)]);
    let book = |choices: &[ChoiceOption]| {
        t.resolve_with_choices(
            abilities(),
            CharacterClass::Warlock,
            &build,
            &warlock(choices),
        )
        .unwrap()
        .book()
        .clone()
    };
    let tongues = book(&[TONGUES]);
    assert!(
        tongues.is_locked(AbilityType::DrainLife),
        "Tongues forbids it"
    );
    assert!(
        tongues.is_locked(AbilityType::DeathCoil),
        "it requires Weakness"
    );
    assert!(!tongues.is_locked(AbilityType::Shadowbolt));
    let weakness = book(&[WEAKNESS]);
    assert!(!weakness.is_locked(AbilityType::DrainLife));
    assert!(!weakness.is_locked(AbilityType::DeathCoil));
    // A locked ability stays in the book: the AI reads it freely.
    assert!(tongues.get(&AbilityType::DrainLife).is_some());
    // The shipped table writes no Warlock rule: nothing of its is locked.
    let shipped = TalentDefinitions::shipped()
        .resolve(abilities(), CharacterClass::Warlock, &Build::default())
        .unwrap();
    assert!(shipped
        .book()
        .iter()
        .all(|(a, _)| !shipped.book().is_locked(*a)));

    // The cast gate refuses a locked ability; the control passes it.
    let def = abilities().get_unchecked(&AbilityType::DrainLife);
    let locked = Combatant::new(1, 0, CharacterClass::Warlock).with_book(tongues);
    let free = Combatant::new(1, 0, CharacterClass::Warlock).with_book(weakness);
    assert!(!own_state_ok(
        AbilityType::DrainLife,
        def,
        &locked,
        None,
        PreCastOpts::default()
    ));
    assert!(own_state_ok(
        AbilityType::DrainLife,
        def,
        &free,
        None,
        PreCastOpts::default()
    ));
}

/// The cast gate's refusal, played through the class dispatch: a Priest whose
/// choices lock Flash Heal does not heal a dying ally, and the trace says why.
#[test]
fn a_locked_heal_is_refused_by_the_gate_in_a_scene() {
    let scene = |locked: bool| {
        let s = Scene::new()
            .unit("priest", 1, CharacterClass::Priest)
            .aura("priest", AuraType::MaxHealthIncrease, 600.0)
            .aura("priest", AuraType::WeakenedSoul, 15.0)
            .unit("ally", 1, CharacterClass::Warrior)
            .at(5.0, 0.0)
            .health(0.3)
            .aura("ally", AuraType::MaxHealthIncrease, 600.0)
            .aura("ally", AuraType::WeakenedSoul, 15.0)
            .unit("enemy", 2, CharacterClass::Warrior)
            .at(0.0, 30.0);
        if locked {
            s.lock("priest", AbilityType::FlashHeal)
        } else {
            s
        }
    };
    let free = scene(false).decide("priest");
    assert_eq!(
        free.chose().map(|(a, _)| a),
        Some(AbilityType::FlashHeal),
        "control"
    );
    let locked = scene(true).decide("priest");
    assert_ne!(locked.chose().map(|(a, _)| a), Some(AbilityType::FlashHeal));
    assert!(
        matches!(
            locked.rejected(AbilityType::FlashHeal),
            Some(RejectionReason::PreconditionUnmet { note }) if note == CHOICE_LOCKED_NOTE
        ),
        "{:?}",
        locked.rejected(AbilityType::FlashHeal)
    );
}

// ---------------------------------------------------------------------------
// The loader
// ---------------------------------------------------------------------------

#[test]
fn the_loader_rejects_each_broken_choice_rule() {
    // A default is never hidden.
    assert!(load_err(
        "WidenChoice(WarlockCurse(Tongues))",
        "WidenChoice(WarlockCurse(Agony))"
    )
    .contains("default"));
    // Another class's menu.
    assert!(load_err(
        "WidenChoice(WarlockCurse(Tongues))",
        "WidenChoice(PaladinAura(ConcentrationAura))"
    )
    .contains("not a Warlock choice"));
    // One option, one node.
    assert!(load_err(
        "Mechanic(SoulLink(share: 0.06))",
        "WidenChoice(WarlockCurse(Tongues))"
    )
    .contains("already taken"));
    // FromRank starts at 2, at most max_rank, and wraps no grant or FromRank.
    assert!(load_err("FromRank(2,", "FromRank(1,").contains("FromRank"));
    assert!(load_err("FromRank(2,", "FromRank(3,").contains("FromRank"));
    // A Rogue slot on a Warlock node.
    assert!(load_err(
        "Mechanic(SoulLink(share: 0.06))",
        "AddChoiceSlot(OffHandPoison)"
    )
    .contains("not a Warlock slot"));
    // Soul Link needs a pet; Cheat Death has one rank.
    assert!(load_err(
        "AddChoiceSlot(OffHandPoison),\n                Mechanic(CheatDeath",
        "AddChoiceSlot(OffHandPoison),\n                Mechanic(SoulLink(share: 0.1)), Mechanic(CheatDeath"
    )
    .contains("pet"));
    assert!(load_err(
        "max_rank: 1, budget: Some(11.0),\n            per_rank: [\n                AddChoiceSlot",
        "max_rank: 2, budget: Some(11.0),\n            per_rank: [\n                AddChoiceSlot"
    )
    .contains("one rank"));
    // Rules name a picked option, a talent of the option's class, a reachable
    // ability.
    assert!(load_err(
        "SwitchesOff(option: WarlockCurse(Tongues)",
        "SwitchesOff(option: WarlockCurse(Agony)"
    )
    .contains("default"));
    assert!(load_err(
        "SwitchesOff(option: WarlockCurse(Tongues), talent: UnholyPower)",
        "SwitchesOff(option: WarlockCurse(Tongues), talent: FelStamina)"
    )
    .contains("switches off a Rogue talent"));
    assert!(load_err("ability: DrainLife", "ability: FlashHeal").contains("no Warlock can have"));
    // A Forbids and a Requires on one ability and option lock it for good.
    assert!(load_err(
        "Requires(ability: DeathCoil, option: WarlockCurse(Weakness)),",
        "Requires(ability: DrainLife, option: WarlockCurse(Tongues)),"
    )
    .contains("no choice could ever cast it"));
    // The fixture itself loads (the control).
    fixture();
}

/// The mechanic variants a tree card names, parsed as `talents.ron` writes
/// them.
#[test]
fn every_mechanic_parses_from_its_ron_shape() {
    let parse = |s: &str| ron::from_str::<TalentMechanic>(s).unwrap();
    assert_eq!(
        parse("SoulLink(share: 0.12)"),
        TalentMechanic::SoulLink { share: 0.12 }
    );
    assert_eq!(
        parse("ReflectiveShield(share: 0.3)"),
        TalentMechanic::ReflectiveShield { share: 0.3 }
    );
    assert_eq!(
        parse("CheatDeath(health: 0.1, reduction: 0.9, duration: 3.0)"),
        TalentMechanic::CheatDeath {
            health: 0.1,
            reduction: 0.9,
            duration: 3.0
        }
    );
}
