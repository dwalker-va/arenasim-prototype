//! One bad icon must not blank every icon.
//!
//! Both icon loaders (`load_spell_icons` in-match, `load_ability_icons` for the
//! encyclopedia and View Combatant) open a whole batch of handles and register
//! textures only once the batch has SETTLED. They share
//! `icon_load_settled` to decide that. If it waited for every handle to be
//! LOADED, a single missing or undecodable file would never get there and the
//! batch would never register — every icon on the screen drawn blank, forever.
//!
//! This drives a real `AssetServer`: a file that does not exist settles as
//! FAILED, and that counts as settled. No window, no GPU.

use bevy::asset::{AssetPlugin, LoadState};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::texture::ImagePlugin;

use arenasim::states::play_match::rendering::icon_load_settled;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        ImagePlugin::default(),
    ));
    app
}

/// Update until `handle` stops resolving, or give up. Bounded so a regression
/// fails rather than hangs; asset IO is off-thread, so this polls real time.
fn settle(app: &mut App, handle: &Handle<Image>) -> LoadState {
    for _ in 0..500 {
        app.update();
        let state = app
            .world()
            .resource::<AssetServer>()
            .load_state(handle.id());
        if !matches!(state, LoadState::Loading | LoadState::NotLoaded) {
            return state;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the asset never finished loading or failing");
}

#[test]
fn a_missing_icon_counts_as_settled_so_it_cannot_hold_the_batch() {
    let mut app = app();
    let handle: Handle<Image> = app
        .world()
        .resource::<AssetServer>()
        .load("icons/items/this_icon_does_not_exist.jpg");

    let state = settle(&mut app, &handle);

    assert!(
        matches!(state, LoadState::Failed(_)),
        "the fixture must actually fail to load, or this proves nothing: {state:?}"
    );
    assert!(
        icon_load_settled(&state),
        "a FAILED icon is treated as still pending — one bad file would blank every icon"
    );
    // ...and the loader skips it, because there is no image to register.
    assert!(!app.world().resource::<Assets<Image>>().contains(&handle));
}

/// The rest of the partition: a loaded icon is settled, and a handle still
/// resolving is not — the loaders must keep waiting for it.
#[test]
fn loaded_is_settled_and_resolving_is_not() {
    assert!(icon_load_settled(&LoadState::Loaded));
    assert!(!icon_load_settled(&LoadState::NotLoaded));
    assert!(!icon_load_settled(&LoadState::Loading));
}
