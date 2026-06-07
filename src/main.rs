//! ArenaSim - Arena Combat Autobattler Prototype
//!
//! A prototype implementation of an autobattler where players configure teams
//! of combatants and watch them battle CPU vs CPU.

use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

use arenasim::camera::CameraPlugin;
use arenasim::cli;
use arenasim::combat::CombatPlugin;
use arenasim::headless;
use arenasim::settings::{GameSettings, SettingsPlugin};
use arenasim::states::play_match::{AbilityConfigPlugin, MovementConfigPlugin};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::{GameState, StatesPlugin};
use arenasim::ui::UiPlugin;

fn main() {
    let args = cli::parse_args();

    if let Some(batch_path) = args.batch {
        // Parallel in-process batch runner for sweeps (2v2/3v3/strategy vars).
        let out = args.out.unwrap_or_else(|| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("match_logs/batch_{}.csv", ts).into()
        });
        if let Err(e) = headless::run_batch(batch_path, out, args.jobs) {
            eprintln!("Batch run failed: {}", e);
            std::process::exit(1);
        }
    } else if let Some(n) = args.matrix {
        // 7×7 matchup matrix mode — defaults to trace `on` so every cell's
        // trace is on disk when an anomaly surfaces; explicit `off` opts out.
        let trace_mode = args.trace_mode.unwrap_or(cli::TraceMode::On);
        if let Err(e) = headless::run_matrix(n, args.seed_base, args.save_logs, trace_mode) {
            eprintln!("Matrix run failed: {}", e);
            std::process::exit(1);
        }
    } else if let Some(config_path) = args.headless {
        // Single headless match — defaults to trace `off`; opt in via
        // `--trace-mode on` (or `verbose`).
        let trace_mode = args.trace_mode.unwrap_or(cli::TraceMode::Off);
        run_headless_mode(config_path, args.output, args.max_duration, trace_mode);
    } else {
        // Normal graphical mode
        run_graphical_mode();
    }
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
            output_path: format!("match_logs/match_{}_trace.jsonl", ts).into(),
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

fn run_graphical_mode() {
    // Load settings first to apply them to window configuration
    let settings = GameSettings::load();
    let (width, height) = settings.resolution.dimensions();
    let window_mode = settings.window_mode.to_bevy();
    let present_mode = if settings.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };

    App::new()
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
            EguiPlugin { enable_multipass_for_primary_context: false },
            SettingsPlugin,
            AbilityConfigPlugin,
            MovementConfigPlugin,
            EquipmentPlugin,
            StatesPlugin,
            CameraPlugin,
            CombatPlugin,
            UiPlugin,
        ))
        // Start in the main menu state
        .init_state::<GameState>()
        // Setup custom font
        .add_systems(Startup, setup_custom_font)
        .run();
}

fn setup_custom_font(
    mut contexts: EguiContexts,
) {
    // Deliberately ctx_mut (not try_ctx_mut): this is a run-once Startup
    // system — silently skipping would permanently lose the custom font.
    // A missing context here should fail loudly.
    let ctx = contexts.ctx_mut();
    
    // Load font data
    let mut fonts = egui::FontDefinitions::default();
    
    // Load Rajdhani Bold
    fonts.font_data.insert(
        "rajdhani_bold".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Rajdhani-Bold.ttf")).into(),
    );

    // Load Rajdhani Regular
    fonts.font_data.insert(
        "rajdhani_regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Rajdhani-Regular.ttf")).into(),
    );
    
    // Set Rajdhani Bold as the primary proportional font for headings
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "rajdhani_bold".to_owned());
    
    // Set Rajdhani Regular as secondary
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(1, "rajdhani_regular".to_owned());
    
    ctx.set_fonts(fonts);
}

