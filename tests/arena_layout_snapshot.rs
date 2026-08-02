//! Offscreen annotated top-down snapshot of each arena's real geometry.
//!
//! This is the fast tuning loop for map dimensions (`assets/config/maps.ron`):
//! it renders `draw_arena_layout` to a PNG via `egui_kittest` (wgpu, no window,
//! no Bevy ECS) in a fraction of a second, with every dimension *measured off
//! the loaded geometry* and labelled.
//!
//! ## Loop
//! ```bash
//! # Render; writes tests/snapshots/arena_layout_<map>.new.png on any diff
//! cargo test --release --test arena_layout_snapshot -- --ignored
//! # ...open that PNG, edit assets/config/maps.ron, repeat.
//!
//! # Bless the baselines once the layout is right:
//! UPDATE_SNAPSHOTS=1 cargo test --release --test arena_layout_snapshot -- --ignored
//! ```
//!
//! `#[ignore]`d because it needs a GPU adapter (wgpu), which CI runners may lack
//! — same rationale as the other snapshot harnesses in this directory.
//!
//! These render the **shipped `maps.ron`**, not the Rust defaults, so the picture
//! reflects what a match would actually load. `nagrand_dimensions_are_as_specified`
//! is a plain assertion test (no GPU) that pins the intended numbers, so a RON
//! edit that breaks the spec fails `cargo test` even without rendering.

use egui_kittest::Harness;

use arenasim::states::arena_layout_debug::draw_arena_layout_screen;
use arenasim::states::match_config::ArenaMap;
use arenasim::states::play_match::arena_bounds::ArenaBounds;
use arenasim::states::play_match::map_config::{load_map_geometry_config, MapGeometryConfig};
use arenasim::states::play_match::map_geometry::ObstacleVolume;

/// Load the shipped `assets/config/maps.ron`.
///
/// Deliberately `expect`, NOT `unwrap_or_default()`: these tests exist to pin the
/// shipped asset, and the built-in defaults satisfy every assertion below. With a
/// fallback, the exact edits this file is meant to catch — a typo'd field, a
/// pillar pushed outside the bounds, a degenerate `corner_sum` — make the loader
/// return `Err`, the tests silently assert against `MapGeometryConfig::default()`
/// and pass green, while the real client panics at startup on that same file.
fn shipped_geometry() -> MapGeometryConfig {
    load_map_geometry_config()
        .expect("assets/config/maps.ron must load and validate — these tests pin the SHIPPED asset")
}

fn render(map: ArenaMap, name: &str) {
    let geometry = shipped_geometry();
    let mut harness = Harness::builder()
        .with_size([1100.0, 900.0])
        .build(move |ctx| draw_arena_layout_screen(ctx, map, &geometry));
    harness.run();
    harness.snapshot(name);
}

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn arena_layout_pillared() {
    render(ArenaMap::PillaredArena, "arena_layout_pillared");
}

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn arena_layout_basic() {
    render(ArenaMap::BasicArena, "arena_layout_basic");
}

/// TwinPillars gets a baseline too: it is a shipped, selectable map, and the one
/// the line-of-sight probe suite measures, so a `maps.ron` edit that moves its
/// pillars should be visible in the tuning loop and not only in an assertion.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn arena_layout_twin_pillars() {
    render(ArenaMap::TwinPillars, "arena_layout_twin_pillars");
}

/// Pins the Nagrand spec numerically so the layout cannot silently drift, and so
/// the spec is checked in the default `cargo test` run (no GPU required).
///
/// Reads the shipped RON, so this is a check on the real asset, not on the Rust
/// fallback constants.
#[test]
fn nagrand_dimensions_are_as_specified() {
    let geometry = shipped_geometry();
    let active = geometry.active_for(ArenaMap::PillaredArena);

    // Four octagonal pillars.
    assert_eq!(active.volumes.len(), 4, "Nagrand has four pillars");
    let mut centers: Vec<(f32, f32)> = Vec::new();
    for v in &active.volumes {
        match *v {
            ObstacleVolume::Prism {
                center_xz, sides, ..
            } => {
                assert_eq!(sides, 8, "pillars are octagonal, not round");
                centers.push((center_xz.x, center_xz.y));
            }
            other => panic!("expected an octagonal prism, got {other:?}"),
        }
    }
    centers.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // 40yd between the pair on one side; 80yd between the two pairs.
    let lateral = (centers[1].1 - centers[0].1).abs();
    let gate_to_gate = (centers[2].0 - centers[0].0).abs();
    assert_eq!(lateral, 40.0, "same-side pillar pair should be 40yd apart");
    assert_eq!(gate_to_gate, 80.0, "the two pairs should be 80yd apart");

    // Symmetric about both axes — both teams must see the same arena.
    assert_eq!(centers[0].0, -centers[3].0, "pillars mirror about x=0");
    assert_eq!(centers[0].1, -centers[3].1, "pillars mirror about z=0");

    // A circular bowl clearing the pillars by ~15yd, with 10yd starting rooms.
    let ArenaBounds::Bowl {
        semi_x,
        semi_z,
        alcove_depth,
        alcove_half_width,
    } = active.bounds
    else {
        panic!("Nagrand must use Bowl bounds, got {:?}", active.bounds);
    };
    assert_eq!(semi_x, semi_z, "the bowl should be circular");
    let pillar_dist = (40.0_f32.powi(2) + 20.0_f32.powi(2)).sqrt();
    assert!(
        ((semi_x - pillar_dist) - 15.0).abs() < 0.05,
        "wall should clear the pillars by ~15yd, got {:.2}",
        semi_x - pillar_dist
    );
    assert_eq!(alcove_depth, 10.0, "starting rooms are 10yd deep");

    // A 3v3 line abreast uses 3yd slot spacing, so the room must fit ~6yd.
    assert!(
        alcove_half_width * 2.0 >= 6.0,
        "starting room ({}yd wide) must fit a 3v3 line abreast",
        alcove_half_width * 2.0
    );

    // Every pillar must actually be inside the bowl, and clear of the rooms.
    for (x, z) in &centers {
        assert!(
            active
                .bounds
                .contains(bevy::prelude::Vec3::new(*x, 1.0, *z)),
            "pillar ({x}, {z}) is outside the arena"
        );
    }
}

/// The other maps must NOT have been dragged onto Nagrand's bounds — their
/// balance baselines and movement probes depend on the original octagon.
/// TwinPillars is the load-bearing one: the whole line-of-sight probe suite and
/// the 2026-07-23 balance doc measure exactly that geometry.
#[test]
fn other_maps_keep_the_historical_octagon() {
    let geometry = shipped_geometry();
    for map in [
        ArenaMap::BasicArena,
        ArenaMap::TwinPillars,
        ArenaMap::TestVerticality,
    ] {
        assert_eq!(
            geometry.active_for(map).bounds,
            ArenaBounds::default(),
            "{map:?} must keep the historical octagon bounds"
        );
    }
}
