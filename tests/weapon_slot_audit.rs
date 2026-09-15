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

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::components::Combatant;
use arenasim::states::play_match::equipment::{
    load_default_loadouts, load_item_definitions, ItemSlot,
};

/// For every shipped loadout: the socket the predicate names DOES hold a
/// weapon.
///
/// Off-hand is excluded on purpose rather than by accident. `apply_equipment`
/// documents and implements that an off-hand weapon never replaces
/// attack_damage / attack_speed, so an off-hand weapon is not a candidate for
/// "the socket that is live" and must not make this audit ambiguous.
///
/// A class may legitimately hold a weapon in a replacement-ineligible socket
/// as well (AS-87): a Mage, Warlock or Priest carries a caster one-hander in
/// MainHand for its spell power while swinging from Ranged. That item's damage
/// fields are inert, and inert BY CONSTRUCTION rather than by ambiguity —
/// `apply_equipment` replaces from exactly one socket, the one `weapon_slot()`
/// names, so there is never a question of which weapon "wins". What the audit
/// has to prove is the thing that actually broke: that the named socket is not
/// EMPTY of a weapon, because then the class silently keeps its class base.
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
