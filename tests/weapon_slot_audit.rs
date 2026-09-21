//! Weapon-socket audit (AS-97)
//!
//! `Combatant::apply_equipment` replaces a combatant's `attack_damage` and
//! `attack_speed` from ONE socket: the one [`CharacterClass::weapon_slot`]
//! names. Every other equipped item only adds. So if that predicate and the
//! socket a class's loadout actually fills disagree, the class silently keeps
//! its class base weapon stats — no panic, no warning, nothing in a match log
//! saying the mace it is holding is inert.
//!
//! That is not hypothetical. The socket used to be picked by
//! `CharacterClass::is_melee()`, which answers a DIFFERENT question (how far
//! away the class swings, at the auto-attack range gate). The two answers
//! coincide for seven of the eight classes, so the conflation looked correct
//! for as long as those seven were the only classes. The Shaman — a MainHand
//! mace wielder that attacks at wand range — broke the coincidence, and was
//! sent to its Ranged socket, which holds a relic. Its mace did nothing.
//!
//! A unit test of the predicate alone could not have caught that: the predicate
//! was self-consistent. What was wrong was the predicate RELATIVE TO the
//! shipped loadouts. This audit pins exactly that relation, so the two cannot
//! drift apart again — whether the next change moves a weapon between sockets
//! in `loadouts.ron` or adds a class to the match arm.
//!
//! # The defect class this guard exists for
//!
//! **`weapon_slot()` disagreeing with `loadouts.ron` about which weapon is
//! live.** Both are self-consistent when that happens; what is wrong is one
//! relative to the other, and the symptom is silence — a class swings numbers
//! nobody chose for it.
//!
//! Nothing else in this file can catch that, and the reason is worth stating
//! exactly, because it is not obvious and it is easy to believe otherwise:
//!
//! > `every_class_swings_the_weapon_in_its_named_socket` derives its
//! > expectation FROM `weapon_slot()`, so it is definitionally satisfied
//! > whenever `apply_equipment` reads that same socket. It can only catch those
//! > two disagreeing with each other, never the predicate disagreeing with
//! > INTENT.
//!
//! A test whose expected value is computed from the thing under test cannot
//! fail on that thing being wrong. So the burden sits here, on the two
//! assertions below, and it sits on BOTH of them:
//!
//! - the named socket must HOLD a weapon — the form the Shaman hit, where the
//!   predicate points at a socket carrying a relic, or nothing;
//! - every OTHER weapon must be DECLARED — the form available only once a class
//!   carries two, where the predicate points at a real weapon that is the WRONG
//!   real weapon. Name MainHand for a Mage and it swings its caster dagger
//!   instead of its wand; both sockets hold weapons, so "the named socket holds
//!   a weapon" is satisfied and says nothing.
//!
//! Neither assertion is redundant and neither covers the other. Relaxing either
//! one reopens a defect that ships silently.
//!
//! # A stated limit
//!
//! `DECLARED_STAT_STICK_WEAPONS` can be silenced by writing a FALSE
//! justification into it: declare `(Mage, Ranged)` and the Mage may then point
//! at MainHand unchallenged. That is the accepted cost of a named-exception
//! list, the same cost `JUSTIFIED_EMPTY_SOCKETS` and `JUSTIFIED_USELESS_SOCKETS`
//! carry, and it is deliberate rather than an oversight. Silencing this guard
//! requires writing a sentence that is not true, in source, where a reviewer
//! reads it — which is the point of asking for the sentence.

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::components::{AutoAttackKind, Combatant};
use arenasim::states::play_match::constants::OFFHAND_DAMAGE_MULTIPLIER;
use arenasim::states::play_match::equipment::{
    can_equip_in_socket, load_default_loadouts, load_item_definitions, ItemSlot,
};

/// The classes that deliberately carry a weapon in a socket they do NOT swing
/// from, with the reason. A stat stick: its `attack_damage_*` and
/// `attack_speed` never reach the combatant.
///
/// Checked in BOTH directions, like the other justification lists in this
/// repo — an undeclared second weapon fails, and so does a declared pair that
/// no longer holds one, so a stale entry cannot sit here unnoticed.
const DECLARED_STAT_STICK_WEAPONS: &[(CharacterClass, ItemSlot, &str)] = &[
    (
        CharacterClass::Mage,
        ItemSlot::MainHand,
        "AS-87 caster one-hander, held for its spell power; the Mage swings \
         from Ranged",
    ),
    (
        CharacterClass::Warlock,
        ItemSlot::MainHand,
        "AS-87 caster one-hander, held for its spell power; the Warlock swings \
         from Ranged",
    ),
    (
        CharacterClass::Priest,
        ItemSlot::MainHand,
        "AS-87 caster one-hander, held for its spell power; the Priest swings \
         from Ranged",
    ),
];

/// For every shipped loadout: the socket the predicate names holds a weapon,
/// and every other weapon the class carries is a declared stat stick.
///
/// Off-hand is excluded on purpose rather than by accident. `apply_equipment`
/// documents and implements that an off-hand weapon never replaces
/// attack_damage / attack_speed, so an off-hand weapon is not a candidate for
/// "the socket that is live" and must not make this audit ambiguous.
///
/// A class may hold a weapon in a replacement-INELIGIBLE socket: a Mage,
/// Warlock or Priest carries a caster one-hander in MainHand for its spell
/// power while swinging from Ranged. Its damage fields are inert BY
/// CONSTRUCTION rather than by ambiguity — `apply_equipment` replaces from
/// exactly one socket, so there is never a question of which weapon "wins".
/// Such a weapon belongs in `DECLARED_STAT_STICK_WEAPONS` with its reason, so
/// that carrying two is a decision somebody wrote down rather than drift.
#[test]
fn weapon_slot_matches_the_socket_each_loadout_fills() {
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");

    // Non-vacuity counters. An audit that ran over zero weapons, or that only
    // ever saw one socket kind, would pass while proving nothing.
    let mut audited = 0usize;
    let mut main_hand_classes = 0usize;
    let mut ranged_classes = 0usize;

    for class in CharacterClass::all() {
        let loadout = defaults
            .get(*class)
            .unwrap_or_else(|| panic!("{} has no default loadout", class.name()));

        let live: Vec<ItemSlot> = loadout
            .iter()
            .filter(|(slot, _)| **slot != ItemSlot::OffHand)
            .filter(|(_, id)| items.get(id).is_some_and(|item| item.is_weapon))
            .map(|(slot, _)| *slot)
            .collect();

        let expected = class.weapon_slot();
        assert!(
            live.contains(&expected),
            "{} carries no weapon in {:?}, the socket \
             CharacterClass::weapon_slot() names — it fills {:?} instead. \
             apply_equipment replaces attack_damage / attack_speed ONLY from \
             the named socket, so as written this class fights with its class \
             base weapon stats. Fix whichever of the two is wrong: the match \
             arm in src/states/match_config.rs, or the socket in \
             assets/config/loadouts.ron.",
            class.name(),
            expected,
            live
        );

        // Every OTHER weapon must be a declared stat stick.
        for slot in live.iter().filter(|s| **s != expected) {
            let declared = DECLARED_STAT_STICK_WEAPONS
                .iter()
                .find(|(c, s2, _)| c == class && s2 == slot);
            assert!(
                declared.is_some(),
                "{} carries a weapon in {:?} as well as in its live socket \
                 {:?}, and that second weapon is not declared. Its damage and \
                 speed are inert — apply_equipment reads only {:?} — so either \
                 this is a deliberate stat stick and belongs in \
                 DECLARED_STAT_STICK_WEAPONS with its reason, or \
                 CharacterClass::weapon_slot() names the wrong socket and this \
                 class is swinging the wrong weapon.",
                class.name(),
                slot,
                expected,
                expected,
            );
        }

        audited += 1;
        match expected {
            ItemSlot::MainHand => main_hand_classes += 1,
            ItemSlot::Ranged => ranged_classes += 1,
            other => panic!(
                "{} maps to {:?}, which is not a primary weapon socket",
                class.name(),
                other
            ),
        }
    }

    // A declared pair that no longer holds a weapon is a stale exemption.
    for (class, slot, why) in DECLARED_STAT_STICK_WEAPONS {
        let loadout = defaults.get(*class).expect("default loadout");
        let holds_weapon = loadout
            .get(slot)
            .and_then(|id| items.get(id))
            .is_some_and(|item| item.is_weapon);
        assert!(
            holds_weapon,
            "({}, {:?}) is declared in DECLARED_STAT_STICK_WEAPONS as {:?}, but \
             that socket no longer holds a weapon — delete the stale entry.",
            class.name(),
            slot,
            why,
        );
    }

    assert_eq!(
        audited,
        CharacterClass::all().len(),
        "audit skipped a class"
    );
    assert!(
        main_hand_classes > 0 && ranged_classes > 0,
        "both socket kinds must be exercised, saw {main_hand_classes} MainHand \
         and {ranged_classes} Ranged — a predicate returning one constant would \
         otherwise pass this audit"
    );
}

/// The Shaman's mace is live, with the numbers it is live AT.
///
/// The audit above proves the predicate and the loadout agree. This proves the
/// agreement reaches the combatant: `Hammer of the Righteous` is 10–15 damage
/// at speed 1.0, so a Shaman that has applied its loadout swings for the 12.5
/// average at 1.0 — not the 7.0 / 0.8 class base it shipped with while the
/// socket pick was wrong.
#[test]
fn shaman_equips_its_main_hand_mace() {
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");

    let mut shaman = Combatant::new(1, 0, CharacterClass::Shaman);
    assert_eq!(
        (shaman.attack_damage, shaman.attack_speed),
        (7.0, 0.8),
        "class base stats moved; this test's before/after framing needs updating"
    );

    let loadout = defaults
        .get(CharacterClass::Shaman)
        .expect("Shaman has a default loadout");
    shaman.apply_equipment(loadout, &items);

    assert_eq!(
        shaman.attack_damage, 12.5,
        "Shaman attack_damage: expected the mace's 10-15 average"
    );
    assert_eq!(
        shaman.attack_speed, 1.0,
        "Shaman attack_speed: expected the mace's speed"
    );
}

/// Every class's applied weapon stats come from the item in its named socket.
///
/// Generalises the Shaman pin, so the seven classes this card does not intend
/// to move are pinned too — the change is a no-op for them because the match
/// arm reproduces what `is_melee()` returned, and this is what makes that
/// claim checkable rather than asserted.
#[test]
fn every_class_swings_the_weapon_in_its_named_socket() {
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");

    for class in CharacterClass::all() {
        let loadout = defaults.get(*class).expect("default loadout");
        let slot = class.weapon_slot();
        let weapon = loadout
            .get(&slot)
            .and_then(|id| items.get(id))
            .unwrap_or_else(|| panic!("{} has no item in {:?}", class.name(), slot));

        let mut c = Combatant::new(1, 0, *class);
        c.apply_equipment(loadout, &items);

        let expected_damage = (weapon.attack_damage_min + weapon.attack_damage_max) / 2.0;
        assert_eq!(
            c.attack_damage,
            expected_damage,
            "{} attack_damage should come from {:?}",
            class.name(),
            slot
        );
        assert_eq!(
            c.attack_speed,
            weapon.attack_speed,
            "{} attack_speed should come from {:?}",
            class.name(),
            slot
        );
    }
}

/// The Rogue dual wields by default, and is the only class that does (AS-122).
///
/// The off hand is a DATA claim — one line in `loadouts.ron` — so nothing in
/// the type system holds it up. Delete that line and the Rogue quietly loses
/// the ~10 points of win rate the card measured for it, with every test in the
/// repo still green. This is the test that goes red instead.
///
/// It asserts the whole chain rather than the line's presence, because the
/// line's presence is not the property that matters: the item must be a
/// weapon, the Rogue must be allowed to hold it there, and `apply_equipment`
/// must turn it into a real second swing.
///
/// The `false` half is what keeps the `true` half honest. Asserting only that
/// the Rogue dual wields would pass just as well against an
/// `is_dual_wielding()` stuck at `true`, or against an `apply_equipment` that
/// armed the off hand for everybody; requiring the other seven classes to come
/// out single-wielding rules both out.
#[test]
fn the_rogue_is_the_only_class_that_dual_wields_by_default() {
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");

    let loadout = defaults
        .get(CharacterClass::Rogue)
        .expect("Rogue has a default loadout");

    let off_id = loadout.get(&ItemSlot::OffHand).unwrap_or_else(|| {
        panic!(
            "the Rogue's default loadout has an empty OffHand socket. AS-122 \
             armed it with a second Serpent Fang Dagger and measured what that \
             is worth (docs/design/balance/2026-09-18-as122-rogue-offhand-findings.md); \
             restoring the empty socket is a balance change, not a cleanup."
        )
    });
    let off = items
        .get(off_id)
        .expect("the off-hand item exists in items.ron");

    assert!(
        off.is_weapon,
        "the Rogue's off hand holds {}, which is not a weapon — a shield or a \
         held frill adds stats but arms no second swing",
        off.name
    );
    assert!(
        can_equip_in_socket(CharacterClass::Rogue, ItemSlot::OffHand, off),
        "the Rogue may not legally hold {} in its off hand, so the shipped \
         loadout is one the equipment picker would refuse",
        off.name
    );

    // The swing itself, derived from the item rather than restated, so a
    // re-tuned dagger moves the expectation with it.
    let mut rogue = Combatant::new(1, 0, CharacterClass::Rogue);
    rogue.apply_equipment(loadout, &items);
    let avg = (off.attack_damage_min + off.attack_damage_max) / 2.0;
    assert!(
        rogue.is_dual_wielding(),
        "the Rogue applied its own default loadout and came out single-wielding"
    );
    assert_eq!(
        rogue.offhand_damage,
        avg * OFFHAND_DAMAGE_MULTIPLIER,
        "off-hand damage should be {} of {}'s {avg} average",
        OFFHAND_DAMAGE_MULTIPLIER,
        off.name
    );
    assert_eq!(
        rogue.offhand_speed, off.attack_speed,
        "off-hand speed should be {}'s own speed — the two hands keep separate \
         timers",
        off.name
    );

    // ...and nobody else. Pins the doc claim in wow-mechanics.md, and stops
    // this test passing for a reason that has nothing to do with the Rogue.
    let mut single_wielders = 0usize;
    for class in CharacterClass::all() {
        if *class == CharacterClass::Rogue {
            continue;
        }
        let other = defaults.get(*class).expect("default loadout");
        let mut c = Combatant::new(1, 0, *class);
        c.apply_equipment(other, &items);
        assert!(
            !c.is_dual_wielding(),
            "{} dual wields from its default loadout; only the Rogue is meant \
             to. If that is intended, it is a balance change that needs its own \
             measurement, and wow-mechanics.md needs updating.",
            class.name()
        );
        single_wielders += 1;
    }
    assert_eq!(
        single_wielders, 7,
        "expected the other seven classes to be checked"
    );
}

/// What every shipped loadout's auto-attack IS (AS-138).
///
/// `Combatant::auto_attack_kind` is derived in `apply_equipment` from the item
/// in `class.weapon_slot()`, and `combat_core::auto_attack` reads it for range,
/// the Hunter dead zone, the line-of-sight gate, the Windfury and Frost Armor
/// proc gates, the swing visual and the log name. So this one value decides
/// how the whole roster auto-attacks, and it is a DATA claim — change a weapon
/// in `loadouts.ron` and it changes, with nothing else to notice.
///
/// The table below is the user's 2026-09-20 ruling written as an assertion:
/// *"derive the auto attack from the equipped weapon. Shamans can not equip
/// wands, but Priests/Mages/Warlocks can."* The Shaman is the row that moved —
/// it fired a 30yd `Wand Shot` while holding a relic, because the old ladder
/// asked `CharacterClass::is_melee()` instead of asking the socket.
///
/// Asserted as SET EQUALITY over `CharacterClass::all()`, not as a lookup with
/// a floor: an expectation is required for every class, an unknown class fails
/// rather than being skipped, and a stale row for a class that no longer
/// exists fails too. A count floor here would let a class silently drop out.
const EXPECTED_AUTO_ATTACK: &[(CharacterClass, AutoAttackKind, &str)] = &[
    (
        CharacterClass::Warrior,
        AutoAttackKind::Melee,
        "ArcaniteReaper, a two-hander in MainHand",
    ),
    (
        CharacterClass::Rogue,
        AutoAttackKind::Melee,
        "SerpentFangDagger in MainHand",
    ),
    (
        CharacterClass::Paladin,
        AutoAttackKind::Melee,
        "HammerOfTheRighteous in MainHand; its Ranged socket holds a Libram, \
         which is not what it swings",
    ),
    (
        CharacterClass::Shaman,
        AutoAttackKind::Melee,
        "HammerOfTheRighteous in MainHand. Its Ranged socket is a TOTEM socket \
         holding TotemOfLife, and weapon_proficiency(Shaman) declares Bow, Gun, \
         Crossbow, Thrown and Wand all Untrained — so it has no ranged \
         auto-attack to fire",
    ),
    (
        CharacterClass::Hunter,
        AutoAttackKind::Shot,
        "AshwoodBow in Ranged — the only Shot in the roster",
    ),
    (
        CharacterClass::Mage,
        AutoAttackKind::Wand,
        "WandOfShadows in Ranged; the MainHand caster one-hander is a stat stick",
    ),
    (
        CharacterClass::Priest,
        AutoAttackKind::Wand,
        "StaffOfDominance in Ranged; the MainHand caster one-hander is a stat stick",
    ),
    (
        CharacterClass::Warlock,
        AutoAttackKind::Wand,
        "WandOfShadows in Ranged; the MainHand caster one-hander is a stat stick",
    ),
];

#[test]
fn every_shipped_loadout_derives_the_expected_auto_attack() {
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");

    // Every class must have an expectation, and every expectation must name a
    // class that exists — set equality in both directions.
    for class in CharacterClass::all() {
        assert!(
            EXPECTED_AUTO_ATTACK.iter().any(|(c, _, _)| c == class),
            "{} has no row in EXPECTED_AUTO_ATTACK. Adding a class means \
             deciding what it auto-attacks WITH; this list is the place that \
             decision gets written down.",
            class.name()
        );
    }
    for (class, _, _) in EXPECTED_AUTO_ATTACK {
        assert!(
            CharacterClass::all().contains(class),
            "EXPECTED_AUTO_ATTACK names {}, which is not a CharacterClass",
            class.name()
        );
    }
    assert_eq!(
        EXPECTED_AUTO_ATTACK.len(),
        CharacterClass::all().len(),
        "EXPECTED_AUTO_ATTACK must have exactly one row per class (duplicates \
         would satisfy the two membership checks above)"
    );

    // The derivation itself, through the production path.
    for (class, expected, why) in EXPECTED_AUTO_ATTACK {
        let loadout = defaults
            .get(*class)
            .unwrap_or_else(|| panic!("{} has no default loadout", class.name()));
        let mut combatant = Combatant::new(1, 0, *class);
        combatant.apply_equipment(loadout, &items);
        assert_eq!(
            combatant.auto_attack_kind,
            *expected,
            "{} derives {:?} from its shipped loadout, expected {:?} ({}). \
             This decides its auto-attack RANGE, its log name, its \
             line-of-sight gate and both proc gates at once, so a surprise \
             here is a gameplay change nothing else reports.",
            class.name(),
            combatant.auto_attack_kind,
            expected,
            why
        );
    }

    // Non-vacuity: all three live kinds must actually be exercised. Were the
    // roster ever to collapse onto one kind, every assertion above would still
    // pass while the interesting cases went untested.
    for kind in [
        AutoAttackKind::Melee,
        AutoAttackKind::Shot,
        AutoAttackKind::Wand,
    ] {
        assert!(
            EXPECTED_AUTO_ATTACK.iter().any(|(_, k, _)| *k == kind),
            "no class in the roster auto-attacks as {kind:?} — this audit is \
             no longer covering that arm of the derivation"
        );
    }
}

/// `AutoAttackKind::None` — a live weapon socket holding no weapon — is the arm
/// the old class ladder could not express, and NO shipped loadout reaches it.
/// So it has no coverage from the audit above, and a regression in it would be
/// invisible until a loadout change made it reachable. Pin it directly.
#[test]
fn a_live_socket_holding_no_weapon_yields_no_auto_attack() {
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");

    // The Shaman's own relic, in the Shaman's own live socket. Its Ranged
    // socket really does hold TotemOfLife; what makes this case hypothetical
    // is only that weapon_slot(Shaman) is MainHand, so the relic is never the
    // live one.
    let shaman = defaults
        .get(CharacterClass::Shaman)
        .expect("Shaman loadout");
    let relic = *shaman
        .get(&ItemSlot::Ranged)
        .expect("the Shaman's Ranged socket holds its relic");
    assert!(
        !items.get(&relic).expect("relic is a known item").is_weapon,
        "the Shaman's Ranged item is a weapon — this test no longer builds the \
         empty-live-socket case it exists to cover"
    );

    // A Hunter swings from Ranged, so putting the relic there makes that
    // socket live and weaponless.
    assert_eq!(
        CharacterClass::Hunter.weapon_slot(),
        ItemSlot::Ranged,
        "this case is built on the Hunter swinging from Ranged"
    );
    let mut loadout = arenasim::states::play_match::equipment::Loadout::new();
    loadout.insert(ItemSlot::Ranged, relic);
    let mut combatant = Combatant::new(1, 0, CharacterClass::Hunter);
    combatant.apply_equipment(&loadout, &items);
    assert_eq!(
        combatant.auto_attack_kind,
        AutoAttackKind::None,
        "a live socket holding a relic must yield no auto-attack at all"
    );

    // An EMPTY live socket is the same claim by a different route.
    let empty = arenasim::states::play_match::equipment::Loadout::new();
    let mut bare = Combatant::new(1, 0, CharacterClass::Hunter);
    bare.apply_equipment(&empty, &items);
    assert_eq!(
        bare.auto_attack_kind,
        AutoAttackKind::None,
        "an empty live socket must yield no auto-attack at all"
    );
}
