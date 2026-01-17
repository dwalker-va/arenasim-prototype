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
use arenasim::states::play_match::AbilityConfigPlugin;
use arenasim::states::{GameState, StatesPlugin};
use arenasim::ui::UiPlugin;

fn main() {
    let args = cli::parse_args();

    if let Some(config_path) = args.headless {
        // Headless mode
        run_headless_mode(config_path, args.output, args.max_duration);
    } else {
        // Normal graphical mode
        run_graphical_mode();
    }
}

fn run_headless_mode(
    config_path: std::path::PathBuf,
    output: Option<std::path::PathBuf>,
    max_duration: Option<f32>,
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

    if let Err(e) = headless::run_headless_match(config) {
        eprintln!("Error running match: {}", e);
        std::process::exit(1);
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
            EguiPlugin,
            SettingsPlugin,
            AbilityConfigPlugin,
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
    let ctx = contexts.ctx_mut();
    
    // Load font data
    let mut fonts = egui::FontDefinitions::default();
    
    // Load Rajdhani Bold
    fonts.font_data.insert(
        "rajdhani_bold".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Rajdhani-Bold.ttf")),
    );
    
    // Load Rajdhani Regular
    fonts.font_data.insert(
        "rajdhani_regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Rajdhani-Regular.ttf")),
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

