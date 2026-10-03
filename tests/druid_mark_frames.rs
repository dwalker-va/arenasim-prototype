//! Mark of the Wild through the real `decide_abilities` system, frame by frame.
//!
//! Two Druids marking in ONE frame must mark different allies — the second
//! Mark would only refresh the first, which lands next frame
//! (`marked_this_frame`). And that guard must be a THIS-frame guard: an ally
//! whose Mark is purged later is marked again. The system is registered once
//! and run for both frames, so any state it kept across runs (a `Local`, a
//! resource) would carry the first frame's set into the second.

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, AuraPending, Combatant, CompoundDebuff, GameRng, MatchCountdown,
};
use arenasim::states::play_match::decision_trace::DecisionTrace;
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::{decide_abilities, AbilityDefinitions, MovementConfig};
use arenasim::CharacterClass;

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(GameRng::from_seed(7));
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(AbilityDefinitions::default());
    world.insert_resource(MovementConfig::default());
    world.insert_resource(DecisionTrace::default());
    // Before the gates: Mark of the Wild is the only thing a Druid casts.
    world.insert_resource(MatchCountdown {
        time_remaining: 5.0,
        gates_opened: false,
    });
    world.insert_resource(ActiveMapGeometry {
        bounds: Default::default(),
        volumes: Vec::new(),
        cover_anchors: Vec::new(),
    });
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
    world.insert_resource(time);
    world
}

fn spawn(world: &mut World, slot: u8, class: CharacterClass, x: f32) -> Entity {
    world
        .spawn((
            Combatant::new(1, slot, class),
            Transform::from_xyz(x, 1.0, 0.0),
        ))
        .id()
}

/// The (caster, target) of every Mark sent this frame, then land them.
fn marks_sent(world: &mut World) -> Vec<(Entity, Entity)> {
    let mut q = world.query::<&AuraPending>();
    let mut sent: Vec<(Entity, Entity)> = q
        .iter(world)
        .filter(|p| p.aura.compound == Some(CompoundDebuff::MarkOfTheWild))
        .map(|p| (p.aura.caster.unwrap(), p.target))
        .collect();
    sent.sort();
    world.run_system_once(apply_pending_auras).unwrap();
    sent
}

fn ready(world: &mut World, druids: &[Entity]) {
    for &d in druids {
        world.get_mut::<Combatant>(d).unwrap().global_cooldown = 0.0;
    }
}

#[test]
fn a_purged_mark_is_marked_again_in_a_later_frame() {
    let mut world = world();
    let druids = [
        spawn(&mut world, 0, CharacterClass::Druid, 0.0),
        spawn(&mut world, 1, CharacterClass::Druid, 2.0),
    ];
    let warrior = spawn(&mut world, 2, CharacterClass::Warrior, 4.0);
    let decide = world.register_system(decide_abilities);

    // Frame 1: two Druids, three unmarked allies — two different ones marked.
    world.run_system(decide).unwrap();
    let frame1 = marks_sent(&mut world);
    assert_eq!(frame1.len(), 2, "both Druids mark: {frame1:?}");
    assert_ne!(
        frame1[0].1, frame1[1].1,
        "the same frame never sends two Marks to one ally"
    );

    // Frame 2: the last unmarked ally gets one.
    ready(&mut world, &druids);
    world.run_system(decide).unwrap();
    let frame2 = marks_sent(&mut world);
    assert_eq!(frame2.len(), 1, "one ally left to mark: {frame2:?}");
    let all: Vec<Entity> = frame1.iter().chain(&frame2).map(|&(_, t)| t).collect();
    for unit in druids.iter().chain([&warrior]) {
        assert!(all.contains(unit), "everyone is marked once");
    }

    // Purge the Warrior's Mark. A later frame marks it again — the same-frame
    // set from frame 1 or 2 must not still be holding it.
    world
        .get_mut::<ActiveAuras>(warrior)
        .unwrap()
        .auras
        .retain(|a| a.compound != Some(CompoundDebuff::MarkOfTheWild));
    ready(&mut world, &druids);
    world.run_system(decide).unwrap();
    let frame3 = marks_sent(&mut world);
    assert_eq!(
        frame3.iter().map(|&(_, t)| t).collect::<Vec<_>>(),
        vec![warrior],
        "the purged Warrior is marked again, once"
    );
}
