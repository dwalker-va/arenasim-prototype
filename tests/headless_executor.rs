//! Every schedule the headless runner ticks runs single-threaded.
//!
//! A headless match is one of many running side by side in the batch runner,
//! each on its own worker thread. A schedule left on Bevy's multi-threaded
//! executor dispatches its systems to the one shared `ComputeTaskPool` and
//! parks the worker on a condvar every tick, so a sweep spends most of its CPU
//! in the kernel and runs about 30x slower — with byte-identical output, which
//! is why nothing else notices. The combat systems live in `FixedUpdate`, so a
//! hand-written list of schedules that predates that move misses the one that
//! matters.
//!
//! The set checked here is read from the app's own `Schedules` resource, never
//! listed, so a schedule a new plugin adds is covered by default.

use arenasim::headless::runner::build_headless_app;
use arenasim::headless::HeadlessMatchConfig;
use bevy::app::FixedUpdate;
use bevy::ecs::schedule::{ExecutorKind, Schedules};
use bevy::prelude::{App, Time};

fn multi_threaded_schedules(app: &App) -> Vec<String> {
    let schedules = app.world().resource::<Schedules>();
    let mut offenders: Vec<String> = schedules
        .iter()
        .filter(|(_, s)| !matches!(s.get_executor_kind(), ExecutorKind::SingleThreaded))
        .map(|(label, s)| format!("{label:?} ({:?})", s.get_executor_kind()))
        .collect();
    offenders.sort();
    offenders
}

#[test]
fn every_headless_schedule_is_single_threaded() {
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Priest".into()],
        team2: vec!["Mage".into(), "Rogue".into()],
        map: "PillaredArena".into(),
        random_seed: Some(7),
        ..Default::default()
    };
    let mut app = build_headless_app(&config, true, None);

    // Non-vacuity: the schedule the combat systems run in is present and
    // populated, so an empty or renamed `Schedules` cannot pass this.
    {
        let schedules = app.world().resource::<Schedules>();
        let fixed = schedules
            .get(FixedUpdate)
            .expect("the headless app has a FixedUpdate schedule");
        let systems = fixed.systems_len();
        assert!(
            systems >= 30,
            "FixedUpdate holds {systems} systems; the core combat systems should be there"
        );
        assert!(
            schedules.iter().count() >= 10,
            "only {} schedules in the headless app",
            schedules.iter().count()
        );
    }
    let at_build = multi_threaded_schedules(&app);
    assert!(
        at_build.is_empty(),
        "headless schedules not single-threaded after build: {at_build:?}"
    );

    // Tick past the gates into combat, so any schedule created on first run
    // (state transitions, fixed-step sub-schedules) is in `Schedules` too.
    for _ in 0..(60 * 15) {
        app.update();
    }
    let elapsed = app.world().resource::<Time>().elapsed_secs();
    assert!(elapsed > 14.0, "only {elapsed}s of match time elapsed");
    let after_run = multi_threaded_schedules(&app);
    assert!(
        after_run.is_empty(),
        "headless schedules not single-threaded after 15s of play: {after_run:?}"
    );
}
