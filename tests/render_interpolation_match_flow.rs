//! Where a moving unit is DRAWN, and where its HUD is painted, observed frame
//! by frame in a REAL match — a stealthed Rogue walking in on a Mage — booted
//! into `PlayMatch` through the real `StatesPlugin` schedule the way
//! `--replay` does, at 60Hz, 120Hz and a rate that is not a multiple of the
//! 60Hz sim.
//!
//! ## Why this exists
//!
//! The sim moves units once per 60Hz tick; the display renders at its own
//! rate. Three things used to disagree about where a walking unit was:
//!
//! - the MODEL was drawn at the raw sim `Transform`, so at 120Hz it stood still
//!   on every other frame and jumped a whole tick's travel on the rest;
//! - the CAMERA followed a smoothed version of those raw positions;
//! - the HUD (health bar, status labels) hung from its own interpolated anchor,
//!   projected in `Update` through the camera's `GlobalTransform` — which is
//!   only propagated in `PostUpdate`, so it was LAST frame's camera.
//!
//! Measured with this harness before the fix, at 120Hz: the model's world
//! position did not move on 379 of 758 walking frames, and the label's offset
//! from the model changed by up to 1.8 physical pixels frame to frame (2.4 at
//! 143Hz). Text that is crisp in any one frame and jitters in motion.
//!
//! Now one interpolated render position (`rendering::interpolation`) is what
//! the model, the camera and the HUD all read, and the HUD is placed after
//! transform propagation from this frame's final `GlobalTransform`s. The
//! tests pin all three parts:
//!
//! 1. the drawn unit advances every frame by its speed times the FRAME's
//!    duration — never zero, never a whole tick — while the sim, at every
//!    tick, still sees exactly the translation its previous tick left;
//! 2. the health bar is placed EXACTLY where the drawn unit projects through
//!    the camera the frame renders with, snapped to whole physical pixels; and
//!    the STEALTH label's nine copies (eight black outline copies and the
//!    white text) keep one pixel layout every frame. epaint rounds each shape
//!    to physical pixels on its own, so with a fractional origin the copies
//!    round different ways as the unit walks and the letters change shape.
//!
//! Runs with no GPU (same shape as `tests/hit_reaction_match_flow.rs`); the
//! second test adds a `Window` COMPONENT, never an OS window. Visual-only
//! systems never touch the sim, so the match is the seeded one.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy_egui::{egui, EguiContext};

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{ArenaCamera, Combatant};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    hud_screen_anchor, nameplate_origin, AbilityConfigPlugin, GameRng, MapConfigPlugin,
    MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::{CharacterClass, HeadlessMatchConfig};

/// A plain 60Hz display, a 120Hz one, and ~143Hz — never a multiple of the
/// 60Hz tick, so frames with no tick and frames with one interleave
/// irregularly, as they do on a real display.
const FRAME_RATES: [Duration; 3] = [
    Duration::from_micros(16_667),
    Duration::from_micros(8_333),
    Duration::from_millis(7),
];
/// Sim time to run: past the gates and through the stealthed approach.
const SIM_SECS: f32 = 17.0;
/// A tick's travel counts as a steady walk when it is at least this long...
const WALKING: f32 = 0.01;
/// ...and differs from the tick before by no more than this.
const STEADY: f32 = 1e-4;

const MATCH: &str = r#"{"team1":["Rogue"],"team2":["Mage"],"map":"BasicArena","random_seed":42}"#;

fn frames_for(frame: Duration) -> usize {
    (SIM_SECS / frame.as_secs_f32()) as usize
}

fn boot(frame: Duration) -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(MATCH).unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(BevyStatesPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .add_plugins(bevy::transform::TransformPlugin)
        .init_resource::<bevy_egui::EguiUserTextures>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        .init_asset::<bevy::scene::Scene>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<bevy::gltf::GltfNode>()
        .init_asset::<bevy::gltf::GltfMesh>()
        .init_asset::<bevy::gltf::GltfPrimitive>()
        .add_plugins((
            AbilityConfigPlugin,
            MovementConfigPlugin,
            MapConfigPlugin,
            EquipmentPlugin,
            CombatPlugin,
            arenasim::settings::SettingsPlugin,
            StatesPlugin,
        ));
    // The `--replay` entrance: straight into the match with the seed set.
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(frame));
    app
}

fn rogue<'a, T>(
    mut combatants: impl Iterator<Item = (&'a Combatant, T)>,
) -> Option<(&'a Combatant, T)> {
    combatants.find(|(c, _)| c.class == CharacterClass::Rogue)
}

#[derive(Resource, Default)]
struct Walk {
    /// The Rogue's sim translation after every tick.
    ticks: Vec<Vec3>,
    /// Ticks at which the sim saw a translation other than the one the
    /// previous tick left.
    sim_saw_drawn: Vec<(usize, Vec3, Vec3)>,
    /// Per rendered frame while the Rogue is stealthed: how many ticks had run,
    /// and where the Rogue was drawn.
    frames: Vec<(usize, Vec3)>,
}

fn observe_tick_start(mut walk: ResMut<Walk>, combatants: Query<(&Combatant, &Transform)>) {
    let Some((_, tf)) = rogue(combatants.iter()) else {
        return;
    };
    if let Some(&last) = walk.ticks.last() {
        if tf.translation != last {
            let tick = walk.ticks.len();
            walk.sim_saw_drawn.push((tick, last, tf.translation));
        }
    }
}

fn observe_tick_end(mut walk: ResMut<Walk>, combatants: Query<(&Combatant, &Transform)>) {
    if let Some((_, tf)) = rogue(combatants.iter()) {
        walk.ticks.push(tf.translation);
    }
}

/// After propagation: the position the model renders at this frame.
fn observe_drawn(mut walk: ResMut<Walk>, combatants: Query<(&Combatant, &GlobalTransform)>) {
    let Some((c, gt)) = rogue(combatants.iter()) else {
        return;
    };
    if !c.stealthed {
        return;
    }
    let ticks = walk.ticks.len();
    walk.frames.push((ticks, gt.translation()));
}

#[test]
fn drawn_unit_advances_every_frame_while_the_sim_keeps_its_own_positions() {
    for frame in FRAME_RATES {
        let mut app = boot(frame);
        app.init_resource::<Walk>()
            .add_systems(FixedPreUpdate, observe_tick_start)
            .add_systems(FixedPostUpdate, observe_tick_end)
            .add_systems(Last, observe_drawn);
        for _ in 0..frames_for(frame) {
            app.update();
        }
        let tick_secs = app
            .world()
            .resource::<Time<Fixed>>()
            .timestep()
            .as_secs_f32();
        let walk = std::mem::take(&mut *app.world_mut().resource_mut::<Walk>());
        let frame_secs = frame.as_secs_f32();
        let hz = 1.0 / frame_secs;

        // The sim is authoritative: every tick starts from exactly what the
        // tick before it left, however the frames in between drew the unit.
        assert!(
            walk.sim_saw_drawn.is_empty(),
            "{hz:.0}Hz: the sim started {} ticks from a position its previous tick \
             did not leave — the render interpolation leaked into the sim. First: \
             tick {} left {:?}, the next tick saw {:?}",
            walk.sim_saw_drawn.len(),
            walk.sim_saw_drawn[0].0,
            walk.sim_saw_drawn[0].1,
            walk.sim_saw_drawn[0].2,
        );

        let mut steady_frames = 0;
        for pair in walk.frames.windows(2) {
            let ((k0, a0), (k1, a1)) = (pair[0], pair[1]);
            // Both frames' drawn positions lie on the segments between ticks
            // k1-3 .. k1.
            if k1 < 3 || k1 - k0 > 1 {
                continue;
            }
            let p = &walk.ticks[k1 - 3..k1];
            let (d1, d2) = (p[1] - p[0], p[2] - p[1]);
            if d2.length() < WALKING || (d2 - d1).length() > STEADY {
                continue;
            }
            steady_frames += 1;
            let expected = d2 * (frame_secs / tick_secs);
            let actual = a1 - a0;
            assert!(
                (actual - expected).length() <= 0.05 * expected.length(),
                "{hz:.0}Hz, frame after tick {k1}: the unit was drawn {actual:?} further \
                 on, expected {expected:?} ({:.0}% of a tick's travel for {:.0}% of a \
                 tick's time) — the model is stepping with the sim instead of gliding",
                100.0 * actual.length() / d2.length(),
                100.0 * frame_secs / tick_secs,
            );
        }
        assert!(
            steady_frames >= 150,
            "{hz:.0}Hz: only {steady_frames} frames caught the stealthed Rogue walking \
             steadily — the seed no longer exercises the approach"
        );
    }
}

/// The display scale the user's Retina screen renders at.
const PIXELS_PER_POINT: f32 = 2.0;
/// Logical window size.
const WINDOW: (f32, f32) = (1280.0, 800.0);
/// Fraction of a physical pixel a coordinate may miss a whole one by.
const ON_PIXEL: f32 = 1e-3;

/// Per rendered frame while the Rogue is stealthed and alive.
#[derive(Resource, Default)]
struct Painted {
    frames: Vec<PaintedFrame>,
}

struct PaintedFrame {
    /// The health bar background rect, as painted.
    bar: egui::Rect,
    /// Where the Rogue as drawn projects through the camera as it renders,
    /// raised to the bar's height, in logical points.
    anchor: Vec2,
    /// Every STEALTH text shape's origin, in paint order (outline copies,
    /// then the white text on top).
    label: Vec<egui::Pos2>,
}

fn is_whole_pixel(points: f32) -> bool {
    let px = points * PIXELS_PER_POINT;
    (px - px.round()).abs() <= ON_PIXEL
}

fn to_px(v: egui::Vec2) -> (i32, i32) {
    (
        (v.x * PIXELS_PER_POINT).round() as i32,
        (v.y * PIXELS_PER_POINT).round() as i32,
    )
}

/// egui input for a 2x display; the pass is what gives the HUD a context to
/// paint into.
fn begin_egui_pass(mut contexts: Query<&mut EguiContext, With<PrimaryWindow>>) {
    let Ok(mut ctx) = contexts.single_mut() else {
        return;
    };
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(WINDOW.0, WINDOW.1),
        )),
        ..Default::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(PIXELS_PER_POINT);
    ctx.get_mut().begin_pass(input);
}

/// In `Last`, after everything has moved: the model and the camera exactly as
/// the frame renders them. The HUD must agree with THESE.
fn end_egui_pass(
    mut painted: ResMut<Painted>,
    mut contexts: Query<&mut EguiContext, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<ArenaCamera>>,
    combatants: Query<(&Combatant, &GlobalTransform)>,
) {
    let Ok(mut ctx) = contexts.single_mut() else {
        return;
    };
    let output = ctx.get_mut().end_pass();
    let (Ok((camera, camera_tf)), Some((c, drawn))) = (camera.single(), rogue(combatants.iter()))
    else {
        return;
    };
    if !c.stealthed || !c.is_alive() {
        return;
    }
    let Some(anchor) = hud_screen_anchor(camera, camera_tf, drawn, 3.5) else {
        return;
    };
    let mut bars = Vec::new();
    let mut label = Vec::new();
    for clipped in &output.shapes {
        match &clipped.shape {
            egui::Shape::Rect(r) if r.fill == egui::Color32::from_rgb(30, 30, 30) => {
                bars.push(r.rect);
            }
            egui::Shape::Text(t) if t.galley.text() == "STEALTH" => label.push(t.pos),
            _ => {}
        }
    }
    // Every living unit has a bar; the Rogue's is the one nearest its anchor.
    let target = egui::pos2(anchor.x, anchor.y);
    let bar = bars.into_iter().min_by(|a, b| {
        a.center()
            .distance(target)
            .total_cmp(&b.center().distance(target))
    });
    if let Some(bar) = bar {
        // The team frame shows STEALTH too; keep the copies over this bar.
        label.retain(|p| p.distance(bar.min) < 40.0);
        painted.frames.push(PaintedFrame { bar, anchor, label });
    }
}

fn paint_match(frame: Duration) -> Vec<PaintedFrame> {
    let mut app = boot(frame);
    let mut window = Window {
        resolution: WindowResolution::new(WINDOW.0, WINDOW.1)
            .with_scale_factor_override(PIXELS_PER_POINT),
        ..default()
    };
    window.resolution.set_scale_factor(PIXELS_PER_POINT);
    app.world_mut()
        .spawn((window, PrimaryWindow, EguiContext::default()));
    // Gives the match camera its viewport, as the render plugin would.
    app.init_resource::<bevy::render::camera::ManualTextureViews>()
        .add_systems(
            PostUpdate,
            bevy::render::camera::camera_system
                .after(bevy::transform::TransformSystem::TransformPropagate),
        )
        .init_resource::<Painted>()
        .add_systems(PreUpdate, begin_egui_pass)
        .add_systems(Last, end_egui_pass);
    for _ in 0..frames_for(frame) {
        app.update();
    }
    std::mem::take(&mut app.world_mut().resource_mut::<Painted>().frames)
}

#[test]
fn stealth_label_is_locked_to_the_drawn_unit_and_rasterises_identically() {
    let mut first_shape: Option<Vec<(i32, i32)>> = None;
    for frame in FRAME_RATES {
        let hz = 1.0 / frame.as_secs_f32();
        let painted = paint_match(frame);

        // Non-vacuity: the Rogue walked, so its anchor swept through sub-pixel
        // phases — the condition both defects need.
        let phases: std::collections::BTreeSet<(i32, i32)> = painted
            .iter()
            .map(|f| {
                let px = f.anchor * PIXELS_PER_POINT;
                (
                    ((px.x - px.x.floor()) * 8.0) as i32,
                    ((px.y - px.y.floor()) * 8.0) as i32,
                )
            })
            .collect();
        assert!(
            painted.len() >= 200 && phases.len() >= 32,
            "{hz:.0}Hz: {} painted frames over {} of 64 sub-pixel phase cells — the \
             seed no longer walks a stealthed Rogue across pixel phases",
            painted.len(),
            phases.len()
        );

        for (i, f) in painted.iter().enumerate() {
            // Placed from the unit as drawn and the camera as rendered, this
            // frame: EXACTLY that projection, snapped. A camera one frame
            // stale, or a HUD anchor that is not the drawn position, lands a
            // few tenths of a pixel off and rounds to a different pixel.
            let expected = nameplate_origin(f.anchor, f.bar.size(), PIXELS_PER_POINT);
            assert!(
                (f.bar.min - expected).length() <= ON_PIXEL / PIXELS_PER_POINT,
                "{hz:.0}Hz frame {i}: the health bar is at {:?}, but the unit as drawn \
                 this frame projects to {expected:?} — the HUD is not placed from this \
                 frame's final unit and camera transforms",
                f.bar.min
            );
            assert!(
                is_whole_pixel(f.bar.min.x) && is_whole_pixel(f.bar.min.y),
                "{hz:.0}Hz frame {i}: the health bar origin {:?} is not on a whole \
                 physical pixel",
                f.bar.min
            );
            assert_eq!(
                f.label.len(),
                9,
                "{hz:.0}Hz frame {i}: expected eight outline copies and the text"
            );
            let text = *f.label.last().unwrap();
            for p in &f.label {
                assert!(
                    is_whole_pixel(p.x) && is_whole_pixel(p.y),
                    "{hz:.0}Hz frame {i}: a STEALTH copy is drawn at {p:?}, not on a \
                     whole physical pixel — epaint will round each of the nine copies \
                     its own way and the letters change shape as the unit walks"
                );
            }
            // The label as one image, in physical pixels: the text relative to
            // the bar, and every copy relative to the text. Identical every
            // frame, at every frame rate.
            let mut shape = vec![to_px(text - f.bar.min)];
            shape.extend(f.label.iter().map(|p| to_px(*p - text)));
            match &first_shape {
                None => first_shape = Some(shape),
                Some(first) => assert_eq!(
                    &shape, first,
                    "{hz:.0}Hz frame {i}: the STEALTH label's layout in physical pixels \
                     changed — [text-vs-bar, copy-vs-text x9]"
                ),
            }
        }
    }
    // The outline is symmetric: equal copies either side on both axes.
    let shape = first_shape.unwrap();
    let k = shape[1..].iter().map(|(x, _)| x.abs()).max().unwrap();
    assert!(k >= 1, "the outline copies sit on the text");
    let mut expected = Vec::new();
    for dx in [-k, 0, k] {
        for dy in [-k, 0, k] {
            if dx != 0 || dy != 0 {
                expected.push((dx, dy));
            }
        }
    }
    expected.push((0, 0));
    assert_eq!(
        shape[1..].to_vec(),
        expected,
        "outline copies, in physical pixels"
    );
}
