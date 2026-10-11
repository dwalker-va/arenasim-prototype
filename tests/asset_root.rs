//! The game finds its assets however it was launched (AS-248).
//!
//! Two readers load from `assets/`: the RON config loaders, straight off the
//! filesystem, and Bevy's `AssetServer` (every icon, model and texture). They
//! must resolve the SAME root, and that root must not depend on cargo being
//! the launcher or on the working directory.
//!
//! They did not. Bevy's reader, left at `AssetPlugin::default()`, resolves
//! against `BEVY_ASSET_ROOT`, else `CARGO_MANIFEST_DIR`, else the
//! EXECUTABLE'S directory. `cargo run` sets the middle one, so every developer
//! launch worked; the review page's replay button runs `target/release/arenasim`
//! directly, so Bevy looked in `target/release/assets`, found nothing, and the
//! match played with no aura icons, no class portraits and no banter art — while
//! the configs, read relative to the working directory, loaded fine and hid it.
//!
//! The root is the build's own (`paths::assets_dir` states the order), so
//! nothing about a run may steer it. This file reproduces a launch that tries
//! — the working directory moved somewhere with no `assets/`, and an inherited
//! `CARGO_MANIFEST_DIR` naming a different, complete checkout — before
//! anything resolves a path, then asks Bevy's own reader where it would look.
//! It passes wherever the test binary itself was built, including an external
//! `CARGO_TARGET_DIR`. It is one test on purpose: the environment and the
//! working directory are process-wide, and `arenasim::paths` resolves once.

use std::path::{Path, PathBuf};

use bevy::asset::io::file::FileAssetReader;

/// Asset paths the in-match HUD loads through Bevy: every ability icon in
/// `abilities.ron`, the generic aura fallbacks, and a class portrait.
fn hud_asset_paths() -> Vec<String> {
    let abilities = arenasim::states::play_match::ability_config::load_ability_definitions()
        .expect("abilities.ron loads through arenasim::paths");
    let mut paths: Vec<String> = abilities
        .iter()
        .map(|(_, config)| config.icon.clone())
        .filter(|icon| !icon.is_empty())
        .collect();
    paths.extend(
        arenasim::states::play_match::rendering::GENERIC_AURA_ICONS
            .iter()
            .map(|(_, path)| path.to_string()),
    );
    paths.push("icons/classes/warrior.png".to_string());
    paths.sort();
    paths.dedup();
    paths
}

fn missing_under(root: &Path, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| !root.join(p).is_file())
        .cloned()
        .collect()
}

#[test]
fn bevy_and_the_config_loaders_share_one_asset_root_whatever_launched_the_binary() {
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // A launch nothing about the run can steer: a working directory that holds
    // no `assets/`, and a `CARGO_MANIFEST_DIR` INHERITED from some other shell
    // that names a DIFFERENT checkout — a complete one, so reading it would
    // succeed silently and swap this binary onto another tree's configs (the
    // A/B-voiding case). Must happen before ANY call into `arenasim::paths`,
    // which memoizes what it sees.
    let decoy = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(decoy.path().join("assets/config")).expect("decoy assets");
    std::fs::copy(
        checkout.join("assets/config/abilities.ron"),
        decoy.path().join("assets/config/abilities.ron"),
    )
    .expect("decoy abilities.ron");
    std::env::set_var("CARGO_MANIFEST_DIR", decoy.path());
    std::env::remove_var("BEVY_ASSET_ROOT");
    let elsewhere = tempfile::tempdir().expect("tempdir");
    std::env::set_current_dir(elsewhere.path()).expect("leave the checkout");

    // The config loaders: absolute, and the checkout's.
    let config = arenasim::paths::asset_path("config/abilities.ron");
    assert!(
        config.is_absolute() && config.is_file(),
        "the RON configs must resolve to an absolute path in the checkout, not \
         against the working directory: {config:?}"
    );
    assert_eq!(config, checkout.join("assets/config/abilities.ron"));

    let paths = hud_asset_paths();
    assert!(
        paths.len() > 50,
        "non-vacuity: expected every ability icon, got {} paths",
        paths.len()
    );

    // Non-vacuity for the reproduction itself: under this environment Bevy's
    // DEFAULT reader (which follows the runtime variable to the decoy) must
    // miss, or this test is not standing where a misdirected launch stands and
    // the assertion below proves nothing.
    let default_root = FileAssetReader::new("assets").root_path().clone();
    let missed = missing_under(&default_root, &paths);
    assert_eq!(
        missed.len(),
        paths.len(),
        "Bevy's default reader found the HUD assets at {default_root:?} — the \
         test no longer reproduces a misdirected launch, so it cannot catch the bug"
    );

    // The plugin the client is built with: Bevy's reader, rooted by it, finds
    // every HUD asset, in the same tree the configs came from.
    let plugin = arenasim::paths::asset_plugin();
    let root = FileAssetReader::new(&plugin.file_path).root_path().clone();
    assert_eq!(
        root,
        checkout.join("assets"),
        "Bevy's reader and the config loaders must share one root"
    );
    let missing = missing_under(&root, &paths);
    assert!(
        missing.is_empty(),
        "Bevy's reader, rooted by paths::asset_plugin(), cannot find {} of {} \
         HUD assets under {root:?}: {missing:?}",
        missing.len(),
        paths.len()
    );
}

/// `main.rs` builds the client's `DefaultPlugins` with `paths::asset_plugin()`.
///
/// The test above proves the plugin is right; this proves the client uses it.
/// `build_graphical_app` lives in the binary crate, so it cannot be called from
/// here — the source is the only thing a test can read. Leaving the plugin at
/// its default is exactly the bug: it compiles, runs, and works under
/// `cargo run`.
#[test]
fn the_client_roots_bevys_asset_reader_with_paths_asset_plugin() {
    let main = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))
        .expect("read src/main.rs");
    let code: String = main
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(
        code.matches("DefaultPlugins").count(),
        1,
        "expected the one DefaultPlugins build in main.rs; if there are now \
         several, each needs `.set(arenasim::paths::asset_plugin())`"
    );
    assert!(
        code.contains(".set(arenasim::paths::asset_plugin())"),
        "main.rs must build DefaultPlugins with \
         `.set(arenasim::paths::asset_plugin())`. Bevy's default asset root is \
         the executable's directory whenever cargo is not the launcher, so a \
         client launched as `target/release/arenasim` draws no icons."
    );
    assert!(
        !code.contains("AssetPlugin::default()") && !code.contains("AssetPlugin {"),
        "main.rs configures an AssetPlugin of its own; root it with \
         arenasim::paths::asset_plugin() instead"
    );
}
