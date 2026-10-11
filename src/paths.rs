//! Where the game's writable files belong.
//!
//! A development checkout and an installed build want different answers. In a
//! checkout, `settings.ron` and `match_logs/` sit next to the manifest — every
//! script, matrix sweep and probe in this repo assumes that. A double-clicked
//! macOS app, by contrast, runs with a working directory of `/`, so those same
//! relative writes fail silently: settings revert on every launch and no match
//! is ever recorded. Installed builds therefore write under the per-user data
//! directory instead.
//!
//! Every write site asks this module rather than building a path itself.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::log::warn;

/// Application name for the per-user data directory. On macOS this resolves to
/// `~/Library/Application Support/ArenaSim`, on Windows to
/// `%APPDATA%\ArenaSim\data`.
const APP_NAME: &str = "ArenaSim";

/// Settings file name — unchanged from the pre-distribution hardcoded literal.
const SETTINGS_FILE: &str = "settings.ron";

/// Match log directory name — unchanged from the pre-distribution literal.
const MATCH_LOG_DIR: &str = "match_logs";

/// Asset tree name — unchanged from the pre-distribution literals.
const ASSETS_DIR: &str = "assets";

/// Path to the settings file.
pub fn settings_path() -> PathBuf {
    settings_path_from(user_data_dir())
}

/// Directory that defaulted match logs are written to.
///
/// Only the DEFAULT location. An explicitly-passed output path (`--out`, `-o`,
/// `--output`) bypasses this entirely, which is what the balance scripts rely
/// on.
pub fn match_log_dir() -> PathBuf {
    match_log_dir_from(user_data_dir())
}

/// Whether the binary at `exe_path` is running from a development checkout,
/// determined by looking for a Cargo manifest in any ancestor directory.
///
/// **Why not `CARGO_MANIFEST_DIR` alone?** It is the tempting discriminator —
/// Bevy's own asset resolution keys on it — and on its own it is wrong here.
/// `cargo run` sets it, but `scripts/hunter_2v2_matrix.sh`,
/// `scripts/mage_2v2_matrix.sh`, `scripts/shaman_2v2_matrix.sh` and
/// `scripts/run_combat_tests.sh` all invoke `target/release/arenasim` directly,
/// where it is unset. Those are exactly the workflows that must keep writing
/// into the checkout, so keying on the env var ALONE would relocate their output
/// to the per-user directory.
///
/// Walking up from the executable's own directory classifies `cargo run` and a
/// direct `target/release` invocation alike as development, and classifies a
/// `.app` bundle or an unpacked zip as installed. It needs no build-time flag
/// and no marker file that packaging could forget to ship.
///
/// It is not sufficient ON ITS OWN, though — see [`detect_development`], which
/// covers the checkout whose build directory lives outside the repo.
pub fn is_development_build(exe_path: &Path) -> bool {
    checkout_containing(exe_path).is_some()
}

/// The nearest ancestor of `exe_path` holding a `Cargo.toml` — the checkout a
/// development binary was built in.
///
/// `ancestors()` yields the executable path itself first; that probe just
/// misses harmlessly. Starting there is what makes a manifest-adjacent binary
/// (not only the nested `target/release` case) resolve correctly.
fn checkout_containing(exe_path: &Path) -> Option<&Path> {
    exe_path
        .ancestors()
        .find(|dir| dir.join("Cargo.toml").is_file())
}

/// Whether this process is running from a development checkout, from the two
/// independent signals — neither of which is sufficient alone.
///
/// `CARGO_MANIFEST_DIR` is set by `cargo run`, `cargo test` and `cargo bench`
/// for the process they launch, and is never set for an end user's build. It
/// misses the balance scripts, which invoke `target/release/arenasim` directly
/// (that is what [`is_development_build`] is for). But it is the ONLY signal in
/// the case the manifest walk misses: a checkout whose `CARGO_TARGET_DIR` (or
/// `build.target-dir`) points outside the repo, where the executable has no
/// manifest above it at all. Without this arm such a checkout classifies as
/// installed and `assets_dir()` resolves to `<target-dir>/debug/assets`, so
/// startup panics reading `config/abilities.ron`.
///
/// `exe` is `None` when the executable path could not be resolved, which
/// degrades to development — checkout-relative paths — rather than guessing
/// "installed" and relocating a developer's files.
fn detect_development(manifest_dir_set: bool, exe: Option<&Path>) -> bool {
    manifest_dir_set || exe.map(is_development_build).unwrap_or(true)
}

/// [`detect_development`] for the running process, resolved once.
fn is_development() -> bool {
    static DEV: OnceLock<bool> = OnceLock::new();

    *DEV.get_or_init(|| {
        let exe = match std::env::current_exe() {
            Ok(exe) => Some(exe),
            Err(e) => {
                warn!("Could not resolve the executable path ({e}); using relative paths");
                None
            }
        };
        detect_development(
            std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
            exe.as_deref(),
        )
    })
}

/// Root of the game's read-only asset tree — an absolute path, resolved once.
///
/// **Every asset read goes through here — the RON configs read straight off
/// the filesystem, and Bevy's `AssetServer` (icons, models), which the
/// graphical client configures with [`asset_plugin`].** Two readers with two
/// rules was AS-248: the RON loaders resolved `assets` against the working
/// directory, while Bevy's reader, left to its default, resolves against
/// `CARGO_MANIFEST_DIR` or else the executable's own directory. Launch
/// `target/release/arenasim` directly (the review page's replay button does)
/// and the configs loaded while every icon was looked for in
/// `target/release/assets`.
///
/// The root is the first ASSET TREE (a directory holding
/// `config/abilities.ron`) found in this order — see [`resolve_asset_root`]:
///
/// 1. `assets/` beside the executable — a packaged build (`Contents/MacOS`
///    in the `.app`, beside the `.exe` in the zip);
/// 2. `assets/` in the checkout this binary was COMPILED in — a dev binary
///    reads its own build's assets wherever it runs from, including out of an
///    external `CARGO_TARGET_DIR` or copied out of `target/`;
/// 3. `assets/` in the nearest ancestor of the executable that has one.
///
/// Nothing at run time names the root: not the working directory, and never
/// the runtime `CARGO_MANIFEST_DIR`, which an inherited shell variable can
/// point at a different checkout and silently swap one arm of an A/B onto
/// the other's configs. Finding none of the three is fatal, naming every path
/// tried.
pub fn assets_dir() -> PathBuf {
    asset_root().to_path_buf()
}

/// Path to one asset, named relative to the asset root
/// (e.g. `"config/abilities.ron"`).
pub fn asset_path(relative: &str) -> PathBuf {
    assets_dir().join(relative)
}

/// [`asset_path`] as a string, for the config loaders that take `&str` and then
/// reuse the same value in their error and log messages.
pub fn asset_path_str(relative: &str) -> String {
    asset_path(relative).display().to_string()
}

/// Bevy's `AssetPlugin`, rooted at [`assets_dir`].
///
/// The graphical client MUST build its `DefaultPlugins` with this, never with
/// `AssetPlugin::default()`: an absolute `file_path` is the only way to make
/// Bevy's reader agree with the RON loaders whatever launched the binary.
/// `tests/asset_root.rs` holds `main.rs` to it.
pub fn asset_plugin() -> bevy::asset::AssetPlugin {
    bevy::asset::AssetPlugin {
        file_path: assets_dir().to_string_lossy().into_owned(),
        ..Default::default()
    }
}

/// The checkout this crate was compiled in. Compile-time on purpose: it names
/// the build, and nothing in the environment of a later run can change it.
const BUILD_CHECKOUT: &str = env!("CARGO_MANIFEST_DIR");

/// The file whose presence makes a directory an asset tree. Every build reads
/// it at startup, so a directory without it could not run the game anyway.
const ASSET_TREE_MARKER: &str = "config/abilities.ron";

/// [`assets_dir`]'s resolution, memoized for the process.
fn asset_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();

    ROOT.get_or_init(|| {
        let exe = std::env::current_exe().ok();
        resolve_asset_root(exe.as_deref(), Path::new(BUILD_CHECKOUT)).unwrap_or_else(|tried| {
            panic!(
                "could not find the game's assets — no `{ASSETS_DIR}/` directory holding \
                 `{ASSET_TREE_MARKER}`. Tried, in order:\n  {}",
                tried
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n  ")
            )
        })
    })
}

/// The asset root for an executable at `exe`, built in `build_checkout`, in
/// the order [`assets_dir`] documents; `Err` lists every candidate tried.
fn resolve_asset_root(exe: Option<&Path>, build_checkout: &Path) -> Result<PathBuf, Vec<PathBuf>> {
    let exe_dir = exe.and_then(Path::parent);
    let candidates = exe_dir
        .map(|dir| dir.join(ASSETS_DIR))
        .into_iter()
        .chain(std::iter::once(build_checkout.join(ASSETS_DIR)))
        .chain(
            exe_dir
                .into_iter()
                .flat_map(|dir| dir.ancestors().skip(1))
                .map(|dir| dir.join(ASSETS_DIR)),
        );

    let mut tried = Vec::new();
    for candidate in candidates {
        if candidate.join(ASSET_TREE_MARKER).is_file() {
            return Ok(candidate);
        }
        tried.push(candidate);
    }
    Err(tried)
}

/// Per-user data directory, or `None` when this is a development build or the
/// platform gave us nothing usable.
///
/// Memoized: both entry points are called on every settings save and every
/// match end, and neither the executable's location nor the user's home
/// directory changes mid-run.
fn user_data_dir() -> Option<&'static Path> {
    static DATA_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

    DATA_DIR
        .get_or_init(|| {
            if is_development() {
                return None;
            }

            let dirs = directories::ProjectDirs::from("", "", APP_NAME);
            if dirs.is_none() {
                // No home directory to hang it off. Relative paths may well
                // fail to write, but a failed save is survivable and refusing
                // to start is not.
                warn!("Could not resolve a per-user data directory; using relative paths");
            }
            dirs.map(|dirs| dirs.data_dir().to_path_buf())
        })
        .as_deref()
}

/// Settings path for a given data directory — `None` meaning a development
/// build (or an unresolvable data directory), which keeps today's exact
/// relative path.
fn settings_path_from(data_dir: Option<&Path>) -> PathBuf {
    match data_dir {
        Some(dir) => dir.join(SETTINGS_FILE),
        None => PathBuf::from(SETTINGS_FILE),
    }
}

/// Match log directory for a given data directory. See [`settings_path_from`].
fn match_log_dir_from(data_dir: Option<&Path>) -> PathBuf {
    match data_dir {
        Some(dir) => dir.join(MATCH_LOG_DIR),
        None => PathBuf::from(MATCH_LOG_DIR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// A binary under `target/release/` inside a checkout — the common case for
    /// both `cargo run` and a direct `target/release/arenasim` invocation.
    #[test]
    fn executable_nested_under_a_manifest_is_a_development_build() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("Cargo.toml"), "[package]\n").expect("write manifest");
        let exe = root.path().join("target/release/arenasim");
        std::fs::create_dir_all(exe.parent().unwrap()).expect("create target dir");

        assert!(is_development_build(&exe));
    }

    /// An unpacked zip or a `.app` bundle: no manifest anywhere above it.
    #[test]
    fn executable_with_no_manifest_above_it_is_an_installed_build() {
        let root = tempfile::tempdir().expect("tempdir");
        let exe = root.path().join("ArenaSim/arenasim");
        std::fs::create_dir_all(exe.parent().unwrap()).expect("create dir");

        assert!(!is_development_build(&exe));
    }

    /// The manifest-adjacent binary. `target/release` is nested, so a walk that
    /// only inspects grandparents would still pass the test above while getting
    /// this one wrong.
    #[test]
    fn executable_beside_a_manifest_is_a_development_build() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("Cargo.toml"), "[package]\n").expect("write manifest");
        let exe = root.path().join("arenasim");

        assert!(is_development_build(&exe));
    }

    /// The case the manifest walk alone gets wrong: `CARGO_TARGET_DIR` (or
    /// `build.target-dir`) pointing outside the checkout puts the executable
    /// where no `Cargo.toml` sits above it, so `cargo run` / `cargo test` would
    /// classify as INSTALLED and resolve assets to `<target-dir>/debug/assets`,
    /// panicking at startup. `CARGO_MANIFEST_DIR` is what rescues it.
    #[test]
    fn cargo_sets_the_manifest_dir_even_when_the_target_dir_is_outside_the_checkout() {
        let root = tempfile::tempdir().expect("tempdir");
        let exe = root.path().join("debug/deps/arenasim-abc123");

        assert!(
            !is_development_build(&exe),
            "no manifest above an external target dir — the walk cannot see it"
        );
        assert!(
            detect_development(true, Some(&exe)),
            "CARGO_MANIFEST_DIR being set must still classify this as development"
        );
    }

    /// The balance scripts' case: `target/release/arenasim` invoked directly,
    /// so `CARGO_MANIFEST_DIR` is unset and the manifest walk is the only
    /// signal. It must still be development.
    #[test]
    fn a_direct_invocation_inside_the_checkout_is_development_without_the_env_var() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("Cargo.toml"), "[package]\n").expect("write manifest");
        let exe = root.path().join("target/release/arenasim");
        std::fs::create_dir_all(exe.parent().unwrap()).expect("create target dir");

        assert!(detect_development(false, Some(&exe)));
    }

    /// An end user's build: no manifest above it and no cargo in the
    /// environment. Neither signal fires, so it is installed.
    #[test]
    fn an_installed_build_trips_neither_signal() {
        let root = tempfile::tempdir().expect("tempdir");
        let exe = root.path().join("ArenaSim.app/Contents/MacOS/arenasim");

        assert!(!detect_development(false, Some(&exe)));
    }

    /// An unresolvable executable path degrades to development — relative
    /// paths — rather than relocating a developer's files.
    #[test]
    fn an_unresolvable_executable_degrades_to_development() {
        assert!(detect_development(false, None));
    }

    /// A development build must produce byte-identical paths to the ones
    /// hardcoded today, so checkout workflows are unaffected.
    #[test]
    fn development_build_yields_todays_relative_paths() {
        assert_eq!(settings_path_from(None), PathBuf::from("settings.ron"));
        assert_eq!(match_log_dir_from(None), PathBuf::from("match_logs"));
    }

    #[test]
    fn installed_build_yields_distinct_paths_under_the_data_dir() {
        let data = tempfile::tempdir().expect("tempdir");
        let settings = settings_path_from(Some(data.path()));
        let logs = match_log_dir_from(Some(data.path()));

        assert!(
            settings.starts_with(data.path()),
            "settings under data dir: {settings:?}"
        );
        assert!(
            logs.starts_with(data.path()),
            "logs under data dir: {logs:?}"
        );
        assert_ne!(settings, logs);
    }

    /// The real resolver must classify this test binary (which lives under
    /// `target/debug/deps/` in the checkout) as development, so the seam's
    /// public entry points return today's paths while developing.
    #[test]
    fn the_running_test_binary_classifies_as_development() {
        assert_eq!(settings_path(), PathBuf::from("settings.ron"));
        assert_eq!(match_log_dir(), PathBuf::from("match_logs"));
    }

    /// A directory that counts as an asset tree.
    fn plant_assets(dir: &Path) -> PathBuf {
        let assets = dir.join("assets");
        std::fs::create_dir_all(assets.join("config")).expect("create assets/config");
        std::fs::write(assets.join(ASSET_TREE_MARKER), "()").expect("write marker");
        assets
    }

    /// A packaged build: `assets/` ships beside the executable, and wins even
    /// though the build checkout still exists on this machine.
    #[test]
    fn assets_beside_the_executable_win() {
        let install = tempfile::tempdir().expect("tempdir");
        let checkout = tempfile::tempdir().expect("tempdir");
        let beside = plant_assets(install.path());
        plant_assets(checkout.path());
        let exe = install.path().join("arenasim");

        assert_eq!(resolve_asset_root(Some(&exe), checkout.path()), Ok(beside));
    }

    /// `target/release/arenasim` invoked directly — the review page's replay
    /// button. Nothing beside it, so the build checkout's assets, not
    /// `target/release/assets` (AS-248).
    #[test]
    fn a_dev_binary_reads_its_build_checkout() {
        let checkout = tempfile::tempdir().expect("tempdir");
        let assets = plant_assets(checkout.path());
        let exe = checkout.path().join("target/release/arenasim");
        std::fs::create_dir_all(exe.parent().unwrap()).expect("create target dir");

        assert_eq!(resolve_asset_root(Some(&exe), checkout.path()), Ok(assets));
    }

    /// A binary built into an external `CARGO_TARGET_DIR`, or copied out of
    /// `target/` (a sweep arm), has no checkout above it. It still reads the
    /// checkout it was built in — not whatever the walk up would find.
    #[test]
    fn a_binary_outside_its_checkout_still_reads_its_build_checkout() {
        let checkout = tempfile::tempdir().expect("tempdir");
        let assets = plant_assets(checkout.path());
        let scratch = tempfile::tempdir().expect("tempdir");
        // A DIFFERENT asset tree above the copied binary, which the walk
        // would find: the build checkout must outrank it.
        plant_assets(scratch.path());
        let exe = scratch.path().join("arms/before/arenasim");
        std::fs::create_dir_all(exe.parent().unwrap()).expect("create arm dir");

        assert_eq!(resolve_asset_root(Some(&exe), checkout.path()), Ok(assets));
    }

    /// The build checkout is gone (or the binary moved to another machine):
    /// fall back to the nearest asset tree above the executable.
    #[test]
    fn a_missing_build_checkout_falls_back_to_the_walk_up() {
        let gone = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("tempdir");
        let assets = plant_assets(elsewhere.path());
        let exe = elsewhere.path().join("target/release/arenasim");
        std::fs::create_dir_all(exe.parent().unwrap()).expect("create target dir");

        assert_eq!(resolve_asset_root(Some(&exe), gone.path()), Ok(assets));
    }

    /// An empty `assets/` is not an asset tree; neither is a missing one.
    /// Finding none is an error that names every candidate, in order.
    #[test]
    fn finding_no_asset_tree_lists_every_path_tried() {
        let checkout = tempfile::tempdir().expect("tempdir");
        let install = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(install.path().join("assets")).expect("empty assets dir");
        let exe = install.path().join("arenasim");

        let tried = resolve_asset_root(Some(&exe), checkout.path()).expect_err("nothing to find");
        assert_eq!(
            tried[0],
            install.path().join("assets"),
            "beside the executable first"
        );
        assert_eq!(
            tried[1],
            checkout.path().join("assets"),
            "then the build checkout"
        );
        assert!(
            tried[2..]
                .iter()
                .any(|p| p == &install.path().parent().unwrap().join("assets")),
            "then the walk up: {tried:?}"
        );
    }

    /// The running test binary resolves its own checkout's config, by an
    /// absolute path.
    #[test]
    fn the_running_test_binary_reads_the_checkouts_assets() {
        let path = asset_path("config/abilities.ron");
        assert!(path.is_absolute(), "{path:?}");
        assert!(path.is_file(), "{path:?}");
        assert_eq!(
            path,
            Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/config/abilities.ron")
        );
    }

    /// `ProjectDirs` resolving to nothing must degrade to today's behavior
    /// rather than panicking — the game still has to start.
    #[test]
    fn unresolvable_data_dir_falls_back_to_relative_paths() {
        let none: Option<&Path> = None;
        assert_eq!(settings_path_from(none), PathBuf::from("settings.ron"));
        assert_eq!(match_log_dir_from(none), PathBuf::from("match_logs"));
    }
}
