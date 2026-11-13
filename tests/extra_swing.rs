//! The extra swing (AS-427): Hand of Justice's proc and Windfury Totem's bonus
//! swing share one path through `combat_auto_attack` — and only the totem's
//! draws the `WindfuryTornado`. Since AS-448 it follows TBC's two rules: an
//! extra swing that lands grants rage like any main-hand white hit, and one
//! swing is granted at most one extra attack, the trinket's ahead of the
//! totem's.
//!
//! These drive the real auto-attack system on `MinimalPlugins`, so they prove
//! the CALL SITES, which the unit tests in `proc_trinkets.rs` cannot: that a
//! landed swing rolls the trinket, that an off-hand hit rolls it too and
//! swings the MAIN hand, and that the tornado follows the source. Every proc
//! here is certain (chance 1.0) and each claim is paired with a control that
//! removes its one condition, so none can pass on zero events.
//!
//! The rate in a real match, and that a Warrior's ordinary loadout draws
//! nothing new, are `tests/proc_trinket_probes.rs`'s.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::{CombatLog, StructuredEventData};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::combat_core::{
    combat_auto_attack, ExtraSwingSource, MeleeAvoided,
};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, AutoAttackSwing, Combatant, GameRng, MatchCountdown, WeaponHand,
    WindfuryTornado,
};
use arenasim::states::play_match::constants::{OFFHAND_DAMAGE_MULTIPLIER, RAGE_PER_WEAPON_SECOND};
use arenasim::states::play_match::equipment::ItemId;
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::proc_trinkets::{ProcConfig, ProcEffect, ProcSlot, ProcTrigger};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const FRAMES: u32 = 150;

fn app(seed: u64) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(TICK))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(GameRng::from_seed(seed))
        .insert_resource(AbilityDefinitions::default())
        .insert_resource(ActiveMapGeometry {
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        })
        .add_systems(Update, combat_auto_attack);
    app
}

/// A certain Hand of Justice: an extra swing on every melee hit.
fn certain_hand_of_justice() -> ProcSlot {
    ProcSlot {
        item: ItemId::HandOfJustice,
        name: "Hand of Justice".to_string(),
        config: ProcConfig {
            trigger: ProcTrigger::MeleeHit,
            chance: 1.0,
            effect: ProcEffect::ExtraSwing,
            magnitude: 0.0,
            duration: 0.0,
            internal_cooldown: 0.0,
        },
        remaining_icd: 0.0,
    }
}

/// A certain Windfury Totem buff, as the totem pulses it.
fn certain_windfury() -> ActiveAuras {
    ActiveAuras {
        auras: vec![Aura {
            effect_type: AuraType::WindfuryBuff,
            magnitude: 1.0,
            duration: 1000.0,
            ..Default::default()
        }],
    }
}

/// What one run produced from the attacker: the hand of every swing that
/// landed, the tornadoes spawned, and the `procs` lines the trinket logged.
struct Run {
    main: usize,
    off: usize,
    tornadoes: usize,
    procs: usize,
}

/// A Warrior (or, `dual`, a Warrior given an off hand whose main hand never
/// comes round) swinging at a Mage at melee range.
fn run(hand_of_justice: bool, windfury: bool, dual: bool) -> Run {
    let mut app = app(7);
    // A target that outlives every run, so no swing is lost to its death.
    let mut mage = Combatant::new(2, 0, CharacterClass::Mage);
    mage.max_health = 1.0e6;
    mage.current_health = 1.0e6;
    let target = app
        .world_mut()
        .spawn((mage, Transform::from_translation(Vec3::new(1.5, 0.0, 0.0))))
        .id();
    let mut attacker = Combatant::new(1, 0, CharacterClass::Warrior);
    attacker.target = Some(target);
    if dual {
        // The main hand never swings inside the run, so every main-hand
        // swing seen is an extra swing an OFF-hand hit procced.
        attacker.weapon_speed = 1000.0;
        attacker.offhand_damage = 20.0;
        attacker.offhand_weapon_speed = 0.2;
    } else {
        attacker.weapon_speed = 0.2;
    }
    if hand_of_justice {
        attacker.proc_trinkets.push(certain_hand_of_justice());
    }
    let mut entity = app
        .world_mut()
        .spawn((attacker, Transform::from_translation(Vec3::ZERO)));
    if windfury {
        entity.insert(certain_windfury());
    }
    let attacker = entity.id();

    let (mut main, mut off, mut tornadoes) = (0, 0, 0);
    for _ in 0..FRAMES {
        app.update();
        let mut swings = app.world_mut().query::<(Entity, &AutoAttackSwing)>();
        let seen: Vec<_> = swings
            .iter(app.world())
            .filter(|(_, s)| s.attacker == attacker)
            .map(|(e, s)| (e, s.hand))
            .collect();
        for (e, hand) in seen {
            match hand {
                WeaponHand::Main => main += 1,
                WeaponHand::Off => off += 1,
            }
            app.world_mut().despawn(e);
        }
        let mut funnels = app.world_mut().query::<(Entity, &WindfuryTornado)>();
        let spun: Vec<Entity> = funnels
            .iter(app.world())
            .filter(|(_, t)| t.target == attacker)
            .map(|(e, _)| e)
            .collect();
        for e in spun {
            tornadoes += 1;
            app.world_mut().despawn(e);
        }
    }
    let procs = app
        .world()
        .resource::<CombatLog>()
        .entries
        .iter()
        .filter(|e| e.message.contains("procs Hand of Justice (extra attack)"))
        .count();
    Run {
        main,
        off,
        tornadoes,
        procs,
    }
}

/// A certain Hand of Justice swings an extra main-hand swing off every hit —
/// more swings than the same Warrior lands bare — and spins no tornado.
#[test]
fn hand_of_justice_swings_again_and_draws_no_tornado() {
    let bare = run(false, false, false);
    let worn = run(true, false, false);
    assert!(bare.main >= 5, "too few swings to judge: {}", bare.main);
    assert_eq!(bare.procs, 0);
    assert!(worn.procs > 0, "the trinket never procced");
    assert!(
        worn.main > bare.main,
        "the extra swing never landed: {} swings worn, {} bare",
        worn.main,
        bare.main
    );
    assert_eq!(
        worn.tornadoes, 0,
        "Hand of Justice's swing spun a Windfury tornado"
    );
}

/// The control for the tornado half: the totem's own extra swing DOES spin
/// one, so the count above is not zero for want of a counter.
#[test]
fn windfury_totem_swings_again_and_draws_its_tornado() {
    let totem = run(false, true, false);
    assert!(
        totem.tornadoes > 0,
        "the totem's extra swing spun no tornado"
    );
    assert_eq!(totem.procs, 0, "the totem logged a trinket proc");
}

/// An OFF-hand hit rolls the trinket too, and the swing it buys is the MAIN
/// hand's. The main hand never comes round in this run, so every main-hand
/// swing is one an off-hand hit procced — and without the trinket there are
/// none.
#[test]
fn an_off_hand_hit_procs_a_main_hand_swing() {
    let bare = run(false, false, true);
    let worn = run(true, false, true);
    assert!(
        bare.off >= 5,
        "too few off-hand swings to judge: {}",
        bare.off
    );
    assert_eq!(
        bare.main, 0,
        "the main hand swung in a run built so it cannot"
    );
    assert!(worn.procs > 0, "no off-hand hit procced the trinket");
    assert!(worn.main > 0, "an off-hand proc swung no main hand");
}

/// Main- and off-hand damage far enough apart that a hit's size names its
/// weapon even through a crit: an off-hand hit is 20 or 40, a main-hand one
/// 100 or 200.
const MAIN_DAMAGE: f32 = 100.0;
const OFF_DAMAGE: f32 = 20.0;
/// Between an off-hand crit (40) and a main-hand hit (100).
const MAIN_FLOOR: f32 = 60.0;

/// Run `attacker` — wearing a certain Hand of Justice, with a main hand that
/// never comes round and an off hand that swings fast — at an armourless
/// target, and return how many main-hand swings landed, the weapon damage of
/// every hit (a crit's halved back), read off the combat log, and the
/// attacker's resource at the end.
fn off_hand_procced_swings(mut attacker: Combatant) -> (usize, Vec<f32>, f32) {
    let mut app = app(11);
    let mut mage = Combatant::new(2, 0, CharacterClass::Mage);
    mage.max_health = 1.0e6;
    mage.current_health = 1.0e6;
    mage.armor = 0.0;
    let target = app
        .world_mut()
        .spawn((mage, Transform::from_translation(Vec3::new(1.5, 0.0, 0.0))))
        .id();
    attacker.target = Some(target);
    attacker.offhand_damage = OFF_DAMAGE;
    attacker.offhand_weapon_speed = 0.2;
    attacker.proc_trinkets.push(certain_hand_of_justice());
    let attacker = app
        .world_mut()
        .spawn((attacker, Transform::from_translation(Vec3::ZERO)))
        .id();
    let start_resource = app.world().get::<Combatant>(attacker).unwrap().current_mana;

    let mut main = 0;
    for _ in 0..FRAMES {
        app.update();
        let mut swings = app.world_mut().query::<(Entity, &AutoAttackSwing)>();
        let seen: Vec<_> = swings
            .iter(app.world())
            .filter(|(_, s)| s.attacker == attacker)
            .map(|(e, s)| (e, s.hand))
            .collect();
        for (e, hand) in seen {
            if hand == WeaponHand::Main {
                main += 1;
            }
            app.world_mut().despawn(e);
        }
    }
    let hits = app
        .world()
        .resource::<CombatLog>()
        .entries
        .iter()
        .filter_map(|e| match &e.structured_data {
            Some(StructuredEventData::Damage {
                amount, is_crit, ..
            }) => Some(if *is_crit { amount / 2.0 } else { *amount }),
            _ => None,
        })
        .collect();
    let resource = app.world().get::<Combatant>(attacker).unwrap().current_mana;
    (main, hits, resource - start_resource)
}

/// Every hit the size of a main-hand swing is one of the main-hand swings
/// seen — so each extra swing an off-hand hit procced dealt main-hand damage —
/// and every other hit is the size of an off-hand one.
fn assert_extra_swings_deal_main_hand_damage(who: &str, main: usize, hits: &[f32]) {
    assert!(main > 0, "{who}: no off-hand hit procced an extra swing");
    let big: Vec<f32> = hits.iter().copied().filter(|d| *d > MAIN_FLOOR).collect();
    assert_eq!(
        big.len(),
        main,
        "{who}: {main} main-hand swings landed, but {} hits were main-hand sized: {hits:?}",
        big.len()
    );
    assert!(
        hits.iter().all(|d| *d > MAIN_FLOOR || *d <= OFF_DAMAGE),
        "{who}: a hit is neither off-hand nor main-hand sized: {hits:?}"
    );
}

/// A dual-wielder's off-hand hit procs a swing of its MAIN-hand weapon.
#[test]
fn an_off_hand_proc_swings_the_main_hand_weapon() {
    let mut warrior = Combatant::new(1, 0, CharacterClass::Warrior);
    warrior.attack_damage = MAIN_DAMAGE;
    warrior.weapon_speed = 1000.0;
    let (main, hits, _) = off_hand_procced_swings(warrior);
    assert_extra_swings_deal_main_hand_damage("dual-wielding Warrior", main, &hits);
}

/// The extra swing an OFF-hand hit procs pays the rage of the MAIN-hand
/// weapon it swings, and each off-hand hit its own half share: the rage is
/// exactly that sum.
#[test]
fn an_off_hand_procced_swing_pays_the_main_hand_weapon_s_rage() {
    let main_speed = 3.0;
    let off_speed = 0.2;
    let mut warrior = Combatant::new(1, 0, CharacterClass::Warrior);
    warrior.attack_damage = MAIN_DAMAGE;
    // The main hand's own timer never comes round inside the run (its speed
    // is that of the swing the extra one copies, but its clock starts far
    // back), so every main-hand swing is one an off-hand hit procced.
    warrior.weapon_speed = main_speed;
    warrior.attack_timer = -1000.0;
    warrior.offhand_weapon_speed = off_speed;
    warrior.current_mana = 0.0;
    warrior.max_mana = 1.0e9;
    let (main, hits, rage) = off_hand_procced_swings(warrior);
    assert_extra_swings_deal_main_hand_damage("dual-wielding Warrior", main, &hits);
    let off = hits.iter().filter(|d| **d <= OFF_DAMAGE).count();
    assert!(off > 0, "no off-hand hit landed");
    let expected = RAGE_PER_WEAPON_SECOND * main_speed * main as f32
        + RAGE_PER_WEAPON_SECOND * off_speed * OFFHAND_DAMAGE_MULTIPLIER * off as f32;
    assert!(
        (rage - expected).abs() < 1e-3,
        "rage {rage}: {main} main-hand extra swings and {off} off-hand hits pay {expected}"
    );
}

/// A Hunter's off hand accompanies its MELEE main hand, so the swing its
/// off-hand hit procs is that weapon's, never the bow's.
#[test]
fn a_hunter_s_off_hand_proc_swings_its_melee_weapon_not_its_bow() {
    let mut hunter = Combatant::new(1, 0, CharacterClass::Hunter);
    hunter.attack_damage = 7.0; // the bow: neither hand's size
    hunter.weapon_speed = 1000.0;
    hunter.melee_damage = MAIN_DAMAGE;
    hunter.melee_weapon_speed = 1000.0;
    let (main, hits, _) = off_hand_procced_swings(hunter);
    assert_extra_swings_deal_main_hand_damage("Hunter", main, &hits);
}

/// The source decides the tornado, and nothing else does.
#[test]
fn only_the_totem_source_spins_a_tornado() {
    assert!(ExtraSwingSource::WindfuryTotem.spawns_windfury_tornado());
    assert!(!ExtraSwingSource::Trinket(ItemId::HandOfJustice).spawns_windfury_tornado());
}

// ============================================================================
// AS-448: TBC's rules — rage, and one extra attack per swing
// ============================================================================

/// The weapon speed of the one-swing rig below: slow enough that the main
/// hand swings once in `ONE_SWING_FRAMES`, and its rage a round number.
const ONE_SWING_SPEED: f32 = 2.0;
/// Frames enough for the main hand's first swing (ready on the first frame)
/// and nothing after it.
const ONE_SWING_FRAMES: u32 = 10;

/// What ONE main-hand swing of a Warrior produced: the main-hand swings that
/// landed (the swing and any extra one), the tornadoes, the trinket's `procs`
/// lines, the damage events that critted (the attacker is the only one
/// dealing damage), and the rage it ended with.
struct OneSwing {
    main: usize,
    tornadoes: usize,
    procs: usize,
    crits: usize,
    rage: f32,
}

/// [`one_swing_at`] at the Warrior's own crit chance.
fn one_swing(hand_of_justice: bool, windfury: bool) -> OneSwing {
    one_swing_at(hand_of_justice, windfury, None)
}

/// A Warrior with an empty rage bar whose main hand is ready, swinging once
/// at a Mage that cannot dodge or parry, with or without a certain Hand of
/// Justice and a certain Windfury Totem buff, at `crit_chance` when one is
/// given.
fn one_swing_at(hand_of_justice: bool, windfury: bool, crit_chance: Option<f32>) -> OneSwing {
    let mut app = app(7);
    let mut mage = Combatant::new(2, 0, CharacterClass::Mage);
    mage.max_health = 1.0e6;
    mage.current_health = 1.0e6;
    mage.dodge_chance = 0.0;
    mage.parry_chance = 0.0;
    let target = app
        .world_mut()
        .spawn((mage, Transform::from_translation(Vec3::new(1.5, 0.0, 0.0))))
        .id();
    let mut attacker = Combatant::new(1, 0, CharacterClass::Warrior);
    attacker.target = Some(target);
    attacker.weapon_speed = ONE_SWING_SPEED;
    attacker.attack_timer = ONE_SWING_SPEED;
    attacker.current_mana = 0.0;
    attacker.max_mana = 100.0;
    if let Some(crit_chance) = crit_chance {
        attacker.crit_chance = crit_chance;
    }
    if hand_of_justice {
        attacker.proc_trinkets.push(certain_hand_of_justice());
    }
    let mut entity = app
        .world_mut()
        .spawn((attacker, Transform::from_translation(Vec3::ZERO)));
    if windfury {
        entity.insert(certain_windfury());
    }
    let attacker = entity.id();

    for _ in 0..ONE_SWING_FRAMES {
        app.update();
    }
    let mut swings = app.world_mut().query::<&AutoAttackSwing>();
    let main = swings
        .iter(app.world())
        .filter(|s| s.attacker == attacker && s.hand == WeaponHand::Main)
        .count();
    let mut funnels = app.world_mut().query::<&WindfuryTornado>();
    let tornadoes = funnels
        .iter(app.world())
        .filter(|t| t.target == attacker)
        .count();
    let procs = app
        .world()
        .resource::<CombatLog>()
        .entries
        .iter()
        .filter(|e| e.message.contains("procs Hand of Justice (extra attack)"))
        .count();
    let crits = app
        .world()
        .resource::<CombatLog>()
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.structured_data,
                Some(StructuredEventData::Damage { is_crit: true, .. })
            )
        })
        .count();
    let rage = app.world().get::<Combatant>(attacker).unwrap().current_mana;
    OneSwing {
        main,
        tornadoes,
        procs,
        crits,
        rage,
    }
}

/// The rage one landed main-hand white hit of the rig's weapon grants.
fn swing_rage() -> f32 {
    RAGE_PER_WEAPON_SECOND * ONE_SWING_SPEED
}

/// An extra swing that lands grants rage like the swing that procced it —
/// from the trinket and from the totem alike — and the control, the same
/// swing with no extra one, grants one swing's worth.
#[test]
fn an_extra_swing_grants_rage_like_a_white_hit() {
    let bare = one_swing(false, false);
    assert_eq!(bare.main, 1, "the rig swung {} times, not once", bare.main);
    assert_eq!(bare.rage, swing_rage(), "one white hit's rage");

    for (who, run) in [
        ("Hand of Justice", one_swing(true, false)),
        ("Windfury Totem", one_swing(false, true)),
    ] {
        assert_eq!(run.main, 2, "{who}: the swing and its extra one");
        assert_eq!(
            run.rage,
            2.0 * swing_rage(),
            "{who}: the extra swing granted no rage, or not a white hit's"
        );
    }
}

/// An extra swing that CRITS pays the same rage as one that does not: a white
/// hit's rage follows its weapon's speed, never its damage. At a certain crit
/// every swing crits, so the extra swing — from the trinket and from the
/// totem alike — is seen to crit and still pays exactly one white hit's
/// rage; the control, the critting swing with no extra one, pays one.
#[test]
fn a_critting_extra_swing_pays_a_white_hit_s_rage() {
    let bare = one_swing_at(false, false, Some(1.0));
    assert_eq!(
        (bare.main, bare.crits),
        (1, 1),
        "control: one swing, and it crits"
    );
    assert_eq!(bare.rage, swing_rage(), "control: one white hit's rage");

    for (who, run) in [
        ("Hand of Justice", one_swing_at(true, false, Some(1.0))),
        ("Windfury Totem", one_swing_at(false, true, Some(1.0))),
    ] {
        assert_eq!(
            (run.main, run.crits),
            (2, 2),
            "{who}: the swing and its extra one, both critting"
        );
        assert_eq!(
            run.rage - bare.rage,
            swing_rage(),
            "{who}: the critting extra swing paid other than one white hit's rage"
        );
    }
}

/// One swing that procs BOTH the trinket and the totem is granted ONE extra
/// attack, and it is the trinket's (TBC resolves the equip proc first and
/// does not roll the weapon enchant while an extra attack is pending): one
/// `procs` line, no tornado, two swings' rage. The controls — each source
/// alone — show each one would have fired on this swing.
#[test]
fn one_swing_that_procs_both_gets_one_extra_attack_the_trinket_s() {
    let trinket = one_swing(true, false);
    assert_eq!(
        (trinket.main, trinket.procs, trinket.tornadoes),
        (2, 1, 0),
        "control: the trinket alone"
    );
    let totem = one_swing(false, true);
    assert_eq!(
        (totem.main, totem.procs, totem.tornadoes),
        (2, 0, 1),
        "control: the totem alone"
    );

    let both = one_swing(true, true);
    assert_eq!(
        both.main, 2,
        "one swing and ONE extra attack, not one from each proc"
    );
    assert_eq!(
        (both.procs, both.tornadoes),
        (1, 0),
        "the one extra attack is the trinket's, and the totem never fired"
    );
    assert_eq!(both.rage, 2.0 * swing_rage());
}

/// An extra swing the target dodges or parries grants no rage, as an avoided
/// white hit grants none: over a run against a target that avoids half of
/// everything, the rage is exactly one white hit's per main-hand swing that
/// LANDED — and some extra swings were avoided, so the claim is not vacuous.
#[test]
fn an_avoided_extra_swing_grants_no_rage() {
    let mut app = app(7);
    let mut mage = Combatant::new(2, 0, CharacterClass::Mage);
    mage.max_health = 1.0e6;
    mage.current_health = 1.0e6;
    mage.dodge_chance = 0.5;
    mage.parry_chance = 0.0;
    let target = app
        .world_mut()
        .spawn((mage, Transform::from_translation(Vec3::new(1.5, 0.0, 0.0))))
        .id();
    let speed = 0.2;
    let mut attacker = Combatant::new(1, 0, CharacterClass::Warrior);
    attacker.target = Some(target);
    attacker.weapon_speed = speed;
    attacker.current_mana = 0.0;
    attacker.max_mana = 1.0e6;
    attacker.proc_trinkets.push(certain_hand_of_justice());
    let attacker = app
        .world_mut()
        .spawn((attacker, Transform::from_translation(Vec3::ZERO)))
        .id();
    for _ in 0..FRAMES {
        app.update();
    }

    let mut swings = app.world_mut().query::<&AutoAttackSwing>();
    let landed = swings
        .iter(app.world())
        .filter(|s| s.attacker == attacker && s.hand == WeaponHand::Main)
        .count();
    let mut avoided = app.world_mut().query::<&MeleeAvoided>();
    let avoided = avoided
        .iter(app.world())
        .filter(|a| a.attacker == attacker)
        .count();
    let procs = app
        .world()
        .resource::<CombatLog>()
        .entries
        .iter()
        .filter(|e| e.message.contains("procs Hand of Justice (extra attack)"))
        .count();
    // Every swing that lands procs the certain trinket, so `procs` swings of
    // the timer's landed and `landed - procs` extra swings did; the rest of
    // the procs were avoided.
    let extra_avoided = 2 * procs - landed;
    assert!(
        extra_avoided > 0,
        "no extra swing was avoided ({procs} procs, {landed} landed, {avoided} avoided)"
    );
    assert!(avoided >= extra_avoided);
    let rage = app.world().get::<Combatant>(attacker).unwrap().current_mana;
    let expected = RAGE_PER_WEAPON_SECOND * speed * landed as f32;
    assert!(
        (rage - expected).abs() < 1e-3,
        "rage {rage}, but {landed} landed white hits pay {expected}"
    );
}

/// A trinket swing the target AVOIDS still spends the swing's one extra
/// attack — the proc fired — so the totem is not rolled after it. A certain
/// trinket beside a certain totem, against a target that dodges half of
/// everything, never lets the totem fire, even though some of the trinket's
/// swings are dodged; the control, the totem alone against the same target,
/// fires it.
#[test]
fn an_avoided_trinket_swing_still_spends_the_extra_attack() {
    let run = |hand_of_justice: bool| {
        let mut app = app(7);
        let mut mage = Combatant::new(2, 0, CharacterClass::Mage);
        mage.max_health = 1.0e6;
        mage.current_health = 1.0e6;
        mage.dodge_chance = 0.5;
        mage.parry_chance = 0.0;
        let target = app
            .world_mut()
            .spawn((mage, Transform::from_translation(Vec3::new(1.5, 0.0, 0.0))))
            .id();
        let mut attacker = Combatant::new(1, 0, CharacterClass::Warrior);
        attacker.target = Some(target);
        attacker.weapon_speed = 0.2;
        if hand_of_justice {
            attacker.proc_trinkets.push(certain_hand_of_justice());
        }
        let attacker = app
            .world_mut()
            .spawn((
                attacker,
                Transform::from_translation(Vec3::ZERO),
                certain_windfury(),
            ))
            .id();
        for _ in 0..4 * FRAMES {
            app.update();
        }
        let mut swings = app.world_mut().query::<&AutoAttackSwing>();
        let landed = swings
            .iter(app.world())
            .filter(|s| s.attacker == attacker && s.hand == WeaponHand::Main)
            .count();
        let mut funnels = app.world_mut().query::<&WindfuryTornado>();
        let tornadoes = funnels
            .iter(app.world())
            .filter(|t| t.target == attacker)
            .count();
        let procs = app
            .world()
            .resource::<CombatLog>()
            .entries
            .iter()
            .filter(|e| e.message.contains("procs Hand of Justice (extra attack)"))
            .count();
        (landed, tornadoes, procs)
    };

    let (_, totem_tornadoes, _) = run(false);
    assert!(totem_tornadoes > 0, "control: the totem alone never fired");

    let (landed, tornadoes, procs) = run(true);
    // Every swing of the timer's that lands procs the certain trinket and
    // nothing else does, so `procs` of the landed swings were the timer's and
    // `landed - procs` the trinket's: the rest of the procs were dodged.
    let trinket_dodged = 2 * procs - landed;
    assert!(
        trinket_dodged > 0,
        "no trinket swing was dodged ({procs} procs, {landed} landed)"
    );
    assert_eq!(
        tornadoes, 0,
        "the totem fired after a dodged trinket swing had spent the extra attack"
    );
}
