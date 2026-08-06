//! Durable guard: `check_match_end` clears in-flight cast/channel state on
//! match end. Without this, `process_channeling` early-returns during
//! `VictoryCelebration` (see match_flow.rs), so a Drain Life channel (or a
//! hard cast) active on the exact frame one team wipes freezes its beam /
//! cast bar / casting orb through the celebration instead of ending cleanly.
//!
//! Headless mode's match-end path is a DIFFERENT function
//! (`headless::runner`), so a headless/graphical seed-compare cannot catch a
//! regression in this graphical-only cleanup — this test drives
//! `check_match_end` directly in a minimal Bevy App, same idiom as
//! `casting_mana_charge.rs`.

use bevy::prelude::*;
use bevy::MinimalPlugins;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::MatchConfig;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::components::{
    CastingState, ChannelingState, Combatant, VictoryCelebration,
};
use arenasim::states::play_match::match_flow::check_match_end;
use arenasim::CharacterClass;

/// Minimal App running only `check_match_end`, with the resources it reads
/// (`MatchConfig`, `CombatLog`) present and everything else — `GameRng`,
/// `VictoryCelebration`, the marker-entity queries (projectiles, traps,
/// etc.) — left absent/empty, since the system treats them as optional or
/// tolerates zero matches.
fn harness_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(MatchConfig::default())
        .insert_resource(CombatLog::default())
        .add_systems(Update, check_match_end);
    app
}

fn spawn_combatant(app: &mut App, team: u8, class: CharacterClass, pos: Vec3) -> Entity {
    app.world_mut()
        .spawn((Transform::from_translation(pos), Combatant::new(team, 0, class)))
        .id()
}

/// Team 1 wipes (its lone combatant is dead) while Team 2's lone survivor is
/// mid-channel (Drain Life) AND mid-cast (Corruption) on the same frame —
/// `check_match_end` must strip both `ChannelingState` and `CastingState`
/// from the winner so the celebration doesn't freeze a live channel beam or
/// cast bar.
#[test]
fn match_end_clears_channel_and_cast_state_on_winner() {
    let mut app = harness_app();

    let loser = spawn_combatant(&mut app, 1, CharacterClass::Warrior, Vec3::new(-5.0, 1.0, 0.0));
    app.world_mut().get_mut::<Combatant>(loser).unwrap().current_health = 0.0;

    let winner = spawn_combatant(&mut app, 2, CharacterClass::Warlock, Vec3::new(5.0, 1.0, 0.0));
    app.world_mut().entity_mut(winner).insert(ChannelingState {
        ability: AbilityType::DrainLife,
        duration_remaining: 3.0,
        time_until_next_tick: 1.0,
        tick_interval: 1.0,
        target: loser,
        interrupted: false,
        interrupted_display_time: 0.0,
        ticks_applied: 1,
    });
    app.world_mut().entity_mut(winner).insert(CastingState {
        ability: AbilityType::Corruption,
        time_remaining: 1.0,
        target: Some(loser),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    app.update();

    assert!(
        app.world().get::<ChannelingState>(winner).is_none(),
        "check_match_end must remove ChannelingState from a surviving combatant"
    );
    assert!(
        app.world().get::<CastingState>(winner).is_none(),
        "check_match_end must remove CastingState from a surviving combatant"
    );
    assert!(
        app.world().get_resource::<VictoryCelebration>().is_some(),
        "sanity check: match end must actually have fired (VictoryCelebration inserted) \
         for the ChannelingState/CastingState removal above to be meaningful"
    );
}
