//! The extra swing (AS-427): Hand of Justice's proc and Windfury Totem's bonus
//! swing share one path through `combat_auto_attack` — and only the totem's
//! draws the `WindfuryTornado`.
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

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::combat_core::{combat_auto_attack, ExtraSwingSource};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, AutoAttackSwing, Combatant, GameRng, MatchCountdown, WeaponHand,
    WindfuryTornado,
};
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

/// The source decides the tornado, and nothing else does.
#[test]
fn only_the_totem_source_spins_a_tornado() {
    assert!(ExtraSwingSource::WindfuryTotem.spawns_windfury_tornado());
    assert!(!ExtraSwingSource::Trinket(ItemId::HandOfJustice).spawns_windfury_tornado());
}
