//! ArenaSim - Arena Combat Autobattler Prototype
//!
//! A prototype implementation of an autobattler where players configure teams
//! of combatants and watch them battle CPU vs CPU.

use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_egui::{EguiContexts, EguiPlugin};

use arenasim::camera::CameraPlugin;
use arenasim::cli;
use arenasim::combat::CombatPlugin;
use arenasim::headless;
use arenasim::settings::{GameSettings, SettingsPlugin};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    AbilityConfigPlugin, BanterConfigPlugin, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::ui::driver::{Outcome, UiDriverConfig, UiDriverPlugin};
use arenasim::ui::fonts::install_game_fonts;
use arenasim::ui::UiPlugin;

fn main() {
    let args = cli::parse_args();

    // `--ai-profile` is consumed only by `--matrix`; every other mode takes the
    // profile from its own JSON config (`ai_profile`). Say so rather than silently
    // running Legacy after the user asked for something else.
    if args.matrix.is_none() && args.ai_profile != cli::DEFAULT_AI_PROFILE {
        eprintln!(
            "warning: --ai-profile {} ignored — it applies to --matrix only. \
             Set \"ai_profile\" in the match config instead.",
            args.ai_profile
        );
    }

    // `--ui-script` only means anything to the client. Without this it would
    // be accepted and then quietly never run, because the headless arms are
    // dispatched first — the silent no-op this whole driver exists to stop
    // shipping.
    if let Some(mode) = args.ui_script_conflict() {
        eprintln!(
            "error: --ui-script drives the graphical client and cannot be combined with {mode}."
        );
        std::process::exit(2);
    }

    // Same shape, lower stakes, and the same precedent as the --ai-profile
    // warning above: say the flag did nothing rather than letting the user
    // wonder where their log went.
    if args.ui_script.is_none() && args.ui_script_log.is_some() {
        eprintln!("warning: --ui-script-log ignored — it applies to --ui-script only.");
    }

    if let Some(batch_path) = args.batch {
        // Parallel in-process batch runner for sweeps (2v2/3v3/strategy vars).
        let out = args.out.unwrap_or_else(|| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            arenasim::paths::match_log_dir().join(format!("batch_{}.csv", ts))
        });
        if let Err(e) = headless::run_batch(batch_path, out, args.jobs) {
            eprintln!("Batch run failed: {}", e);
            std::process::exit(1);
        }
    } else if let Some(n) = args.matrix {
        // 7×7 matchup matrix mode — defaults to trace `on` so every cell's
        // trace is on disk when an anomaly surfaces; explicit `off` opts out.
        let trace_mode = args.trace_mode.unwrap_or(cli::TraceMode::On);
        if let Err(e) = headless::run_matrix(
            n,
            args.seed_base,
            args.save_logs,
            trace_mode,
            args.matrix_map,
            args.ai_profile,
        ) {
            eprintln!("Matrix run failed: {}", e);
            std::process::exit(1);
        }
    } else if let Some(config_path) = args.headless {
        // Single headless match — defaults to trace `off`; opt in via
        // `--trace-mode on` (or `verbose`).
        let trace_mode = args.trace_mode.unwrap_or(cli::TraceMode::Off);
        run_headless_mode(config_path, args.output, args.max_duration, trace_mode);
    } else {
        // Graphical modes. `--ui-script` composes with both of them: it drives
        // whatever screen the client boots on, menu or replayed match.
        let driver = match args.ui_script {
            Some(ref path) => {
                let log = args
                    .ui_script_log
                    .clone()
                    .unwrap_or_else(default_ui_script_log);
                Some(UiDriverConfig::load(path, log).unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(2)
                }))
            }
            None => None,
        };
        let outcome = driver.as_ref().map(|c| c.outcome.clone());

        let replay = args.replay.map(|path| {
            headless::HeadlessMatchConfig::load_from_file(&path).unwrap_or_else(|e| {
                eprintln!("Error loading replay config: {e}");
                std::process::exit(1)
            })
        });
        build_graphical_app(replay, driver).run();

        // The verdict cannot ride out on `AppExit` — writing that from a system
        // deadlocks the macOS winit loop, so the driver closes the window and
        // leaves its answer here. See
        // docs/solutions/implementation-patterns/bevy-macos-exit-deadlock-egui-teardown.md
        if let Some(outcome) = outcome {
            let outcome = outcome.lock().expect("ui script outcome mutex").clone();
            match outcome {
                Outcome::Passed => println!("UI script PASSED"),
                Outcome::Failed(reason) => {
                    eprintln!("UI script FAILED — {reason}");
                    std::process::exit(1);
                }
                Outcome::Incomplete => {
                    eprintln!("UI script did not finish — the window closed before the last step");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Default `--ui-script-log` path: alongside the match logs, timestamped.
fn default_ui_script_log() -> std::path::PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    arenasim::paths::match_log_dir().join(format!("ui_script_{ts}.log"))
}

fn run_headless_mode(
    config_path: std::path::PathBuf,
    output: Option<std::path::PathBuf>,
    max_duration: Option<f32>,
    trace_mode: cli::TraceMode,
) {
    println!("Running in headless mode with config: {:?}", config_path);

    let mut config = match headless::HeadlessMatchConfig::load_from_file(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            std::process::exit(1);
        }
    };

    // Override from CLI args if provided
    if let Some(path) = output {
        config.output_path = Some(path.to_string_lossy().to_string());
    }
    if let Some(duration) = max_duration {
        config.max_duration_secs = duration;
    }

    // Build trace config when enabled. Single-match writes alongside the .txt
    // log with the same timestamp suffix.
    let trace_config = if trace_mode.is_enabled() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Some(headless::runner::TraceConfig {
            output_path: arenasim::paths::match_log_dir().join(format!("match_{}_trace.jsonl", ts)),
        })
    } else {
        None
    };

    match headless::run_headless_match_with(config, false, trace_config) {
        Ok(result) => {
            // Brief stdout summary; full details live in the saved log file.
            let winner = match result.winner {
                None => "DRAW".to_string(),
                Some(t) => format!("Team {}", t),
            };
            println!("Result: {} ({:.2}s)", winner, result.match_time);
        }
        Err(e) => {
            eprintln!("Error running match: {}", e);
            std::process::exit(1);
        }
    }
}

/// Build the client app.
///
/// `replay` pre-seeds the match and boots straight into it. `driver` is the
/// `--ui-script` run, if any; [`UiDriverPlugin::disabled`] is what every normal
/// launch gets, and it registers nothing at all.
fn build_graphical_app(
    replay: Option<headless::HeadlessMatchConfig>,
    driver: Option<UiDriverConfig>,
) -> App {
    // Load settings first to apply them to window configuration
    let settings = GameSettings::load();
    let (width, height) = settings.resolution.dimensions();
    let window_mode = settings.window_mode.to_bevy();
    let present_mode = if settings.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };

    let mut app = App::new();
    app
        // Bevy default plugins with settings-based window configuration
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ArenaSim".to_string(),
                resolution: (width, height).into(),
                mode: window_mode,
                present_mode,
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        // Our game plugins
        .add_plugins((
            EguiPlugin {
                enable_multipass_for_primary_context: false,
            },
            SettingsPlugin,
            AbilityConfigPlugin,
            MovementConfigPlugin,
            // Graphical-only by design — banter is visual, so the headless
            // runner deliberately does NOT register this (KTD5). See the
            // module doc in play_match/banter_config.rs.
            BanterConfigPlugin,
            MapConfigPlugin,
            EquipmentPlugin,
            StatesPlugin,
            CameraPlugin,
            CombatPlugin,
            UiPlugin,
            match driver {
                Some(config) => UiDriverPlugin::enabled(config),
                None => UiDriverPlugin::disabled(),
            },
        ))
        // Setup custom font
        .add_systems(Startup, setup_custom_font);

    match replay {
        // A replay boots directly into PlayMatch with the recorded seed, profile
        // and comps already inserted — `setup_play_match` honours all three
        // rather than overwriting them.
        Some(cfg) => {
            let match_config = cfg.to_match_config().unwrap_or_else(|e| {
                eprintln!("Invalid replay config: {e}");
                std::process::exit(1)
            });
            // Single-authority resolution (bare `ai_profile` = both teams,
            // per-team fields override) — same method the headless runner and
            // `validate()` use, so a replay can never disagree with them.
            let profile = cfg.ai_profiles().unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1)
            });
            let rng = match cfg.random_seed {
                Some(seed) => arenasim::states::play_match::GameRng::from_seed(seed),
                None => arenasim::states::play_match::GameRng::default(),
            };
            println!(
                "Replaying: {:?} vs {:?} on {} | profile {:?} | seed {:?}",
                cfg.team1, cfg.team2, cfg.map, profile, rng.seed,
            );
            app.insert_resource(match_config)
                .insert_resource(profile)
                .insert_resource(rng)
                .insert_state(GameState::PlayMatch);
        }
        None => {
            app.init_state::<GameState>();
        }
    }
    app
}

fn setup_custom_font(mut contexts: EguiContexts) {
    // Deliberately ctx_mut (not try_ctx_mut): this is a run-once Startup
    // system — silently skipping would permanently lose the custom font.
    // A missing context here should fail loudly.
    let ctx = contexts.ctx_mut();

    // The stack itself lives in `arenasim::ui::fonts` so the offscreen egui
    // snapshot harnesses install the SAME fonts the player sees.
    install_game_fonts(ctx);
}
