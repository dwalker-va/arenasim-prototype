//! Emoji art as ordinary textures.
//!
//! Banter needs a broad symbol vocabulary, and neither obvious route worked.
//! Fonts cannot carry it: egui's atlas is a single coverage channel
//! (`FontImage { pixels: Vec<f32> }`), so every emoji renders monochrome
//! whatever font is loaded, and the monochrome shapes were not recognisable at
//! bubble size. Drawing each mark as vector shapes DID look good, but cost a
//! code change, a render, and a human spot-check per symbol — a marginal cost
//! that made "use a symbol we haven't used yet" a small project.
//!
//! So emoji are loaded as images instead. Adding one is dropping a PNG in
//! `assets/icons/emoji/`; the loader keys on the FILENAME STEM, so `skull.png`
//! is reachable as `{emoji:skull}` with no code, no rebuild, and nothing to
//! tune. Colour comes free, which the font path could never provide.
//!
//! The directory is scanned at startup rather than enumerated in code, so the
//! set is data. See `assets/icons/emoji/ATTRIBUTION.md` for provenance and for
//! how to fetch more.

use bevy::prelude::*;
use std::collections::HashMap;

/// Where emoji art lives, relative to the assets root.
const EMOJI_DIR: &str = "icons/emoji";

/// Emoji textures registered with egui, keyed by filename stem.
#[derive(Resource, Default)]
pub struct EmojiIcons {
    pub textures: HashMap<String, egui::TextureId>,
    pub loaded: bool,
}

/// Bevy handles held so the images are not unloaded, mirroring
/// `SpellIconHandles`.
#[derive(Resource, Default)]
pub struct EmojiIconHandles {
    pub handles: Vec<(String, Handle<Image>)>,
}

use bevy_egui::egui;

/// Load every PNG in the emoji directory and register it with egui.
///
/// Mirrors `load_spell_icons`, including its failure posture: a handle that
/// fails to resolve is treated as resolved so one bad file cannot block
/// registration of the rest, and a missing emoji degrades to a placeholder in
/// one bubble rather than breaking the UI.
///
/// Discovery reads the directory directly rather than going through the asset
/// server, because Bevy's `AssetServer` has no portable directory listing —
/// and reading it here is what makes the set data instead of a hardcoded list.
pub fn load_emoji_icons(
    mut contexts: bevy_egui::EguiContexts,
    asset_server: Res<AssetServer>,
    mut icons: ResMut<EmojiIcons>,
    mut handles: ResMut<EmojiIconHandles>,
    images: Res<Assets<Image>>,
) {
    if icons.loaded {
        return;
    }

    if handles.handles.is_empty() {
        // Via the path seam, not a working-directory-relative literal: a
        // packaged build runs with a working directory of `/`, so a relative
        // scan finds nothing and every bubble falls back to a placeholder.
        let dir = crate::paths::asset_path(EMOJI_DIR);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            // No directory is a legitimate state (nobody has added art yet).
            // Mark loaded so this does not re-scan every frame forever.
            warn!("No emoji directory at {}; banter emoji will render as placeholders", dir.display());
            icons.loaded = true;
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("png") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // The asset server wants a path relative to `assets/`.
            let handle: Handle<Image> =
                asset_server.load(format!("{}/{}.png", EMOJI_DIR, stem));
            handles.handles.push((stem.to_string(), handle));
        }
        if handles.handles.is_empty() {
            icons.loaded = true;
        }
        return; // Resolve next frame.
    }

    use bevy::asset::LoadState;
    let still_loading = handles.handles.iter().any(|(_, h)| {
        matches!(
            asset_server.load_state(h.id()),
            LoadState::Loading | LoadState::NotLoaded
        )
    });
    if still_loading {
        return;
    }

    for (name, handle) in &handles.handles {
        if !images.contains(handle) {
            warn!("Emoji '{}' failed to load; rendering without it", name);
            continue;
        }
        let texture_id = contexts.add_image(handle.clone());
        icons.textures.insert(name.clone(), texture_id);
    }
    icons.loaded = true;
    info!("Loaded {} emoji textures from assets/{}", icons.textures.len(), EMOJI_DIR);
}
