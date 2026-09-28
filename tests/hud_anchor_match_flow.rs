//! The health bar block's anchor observed in a REAL match — a stealthed Rogue
//! walking in on a Mage — booted into `PlayMatch` through the real
//! `StatesPlugin` schedule the way `--replay` does, at a render rate that is
//! not a multiple of the 60Hz sim.
//!
//! ## Why this exists
//!
//! The sim moves units once per 60Hz tick; the display renders faster. The
//! health bar, cast bar and status labels (STEALTH, STUN, ...) used to hang
//! from the raw `Transform`, so a walking unit's label stood still on frames
//! with no tick and jumped a whole tick's travel on frames with one, while the
//! smoothly-following camera slid it backwards in between. Measured in the
//! running client at ~120fps on this exact approach: +2.19px, -0.14px,
//! +2.19px, -0.14px per frame — text that is crisp in any one frame and fuzzy
//! in motion.
//!
//! The anchor is now interpolated between the last two ticks
//! (`hud_anchor_translation`). This pins the property that fixes the shimmer:
//! while the unit walks steadily, the anchor advances by the unit's speed times
//! the FRAME's duration on every frame — never zero, never a whole tick.
//!
//! The second test drives the real `render_health_bars` into a real egui
//! context at a 2x display scale and reads back the shapes it painted. The
//! interpolated anchor lands on a new sub-pixel phase every frame; epaint
//! rounds each shape to physical pixels on its own, so unless the block's
//! origin is snapped first, the STEALTH label's nine copies (eight black
//! outline copies and the white text) round different ways from frame to
//! frame and the letters change shape. That is what a screen recording of the
//! running client showed after the interpolation alone had shipped.
//!
//! Runs with no GPU (same shape as `tests/hit_reaction_match_flow.rs`); the
//! second test adds a `Window` COMPONENT, never an OS window. Visual-only
//! systems never touch the sim, so the match is the seeded one.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{Combatant, PreviousSimTranslation};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    hud_anchor_translation, render_health_bars, AbilityConfigPlugin, GameRng, MapConfigPlugin,
    MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::{CharacterClass, HeadlessMatchConfig};
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy_egui::{egui, EguiContext};

/// ~143fps: never a multiple of the 60Hz tick, so frames with no tick and
/// frames with one interleave irregularly, as they do on a real display.
const FRAME: Duration = Duration::from_millis(7);
/// Past the gates and through the stealthed approach.
const MAX_FRAMES: usize = 3000;
/// A tick's travel counts as a steady walk when it is at least this long...
const WALKING: f32 = 0.01;
/// ...and differs from the tick before by no more than this.
const STEADY: f32 = 1e-4;

#[derive(Resource, Default)]
struct Seen {
    /// The Rogue's translation after every sim tick.
    ticks: Vec<Vec3>,
    /// Per rendered frame while the Rogue is stealthed: how many ticks had run,
    /// and the anchor the health bar block hangs from.
    frames: Vec<(usize, Vec3)>,
}

fn boot(cfg: &str) -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(cfg).unwrap();
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
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>()
        .add_systems(FixedLast, observe_tick)
        // After the fixed loop, as `render_health_bars` sees the world.
        .add_systems(Update, observe_frame);
    app
}

fn rogue<'a>(
    combatants: impl Iterator<
        Item = (
            &'a Combatant,
            &'a Transform,
            Option<&'a PreviousSimTranslation>,
        ),
    >,
) -> Option<(
    &'a Combatant,
    &'a Transform,
    Option<&'a PreviousSimTranslation>,
)> {
    combatants
        .into_iter()
        .find(|(c, _, _)| c.class == CharacterClass::Rogue)
}

fn observe_tick(
    mut seen: ResMut<Seen>,
    combatants: Query<(&Combatant, &Transform, Option<&PreviousSimTranslation>)>,
) {
    if let Some((_, tf, _)) = rogue(combatants.iter()) {
        seen.ticks.push(tf.translation);
    }
}

fn observe_frame(
    mut seen: ResMut<Seen>,
    fixed: Res<Time<Fixed>>,
    combatants: Query<(&Combatant, &Transform, Option<&PreviousSimTranslation>)>,
) {
    let Some((c, tf, previous)) = rogue(combatants.iter()) else {
        return;
    };
    if !c.stealthed {
        return;
    }
    let anchor = hud_anchor_translation(tf.translation, previous, fixed.overstep_fraction());
    let ticks = seen.ticks.len();
    seen.frames.push((ticks, anchor));
}

#[test]
fn stealth_label_anchor_advances_every_frame_at_the_units_speed() {
    let mut app =
        boot(r#"{"team1":["Rogue"],"team2":["Mage"],"map":"BasicArena","random_seed":42}"#);
    for _ in 0..MAX_FRAMES {
        app.update();
    }
    let tick_secs = app
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f32();
    let seen = std::mem::take(&mut *app.world_mut().resource_mut::<Seen>());
    let frame_secs = FRAME.as_secs_f32();

    let mut steady_frames = 0;
    for pair in seen.frames.windows(2) {
        let ((k0, a0), (k1, a1)) = (pair[0], pair[1]);
        // Both frames' anchors lie on the segments between ticks k1-3 .. k1.
        if k1 < 3 || k1 - k0 > 1 {
            continue;
        }
        let p = &seen.ticks[k1 - 3..k1];
        let (d1, d2) = (p[1] - p[0], p[2] - p[1]);
        if d2.length() < WALKING || (d2 - d1).length() > STEADY {
            continue;
        }
        steady_frames += 1;
        let expected = d2 * (frame_secs / tick_secs);
        let actual = a1 - a0;
        assert!(
            (actual - expected).length() <= 0.05 * expected.length(),
            "frame after tick {k1}: the anchor moved {actual:?}, expected {expected:?} \
             ({:.0}% of a tick's travel for {:.0}% of a tick's time) — the HUD is \
             stepping with the sim instead of gliding",
            100.0 * actual.length() / d2.length(),
            100.0 * frame_secs / tick_secs,
        );
    }
    assert!(
        steady_frames >= 200,
        "only {steady_frames} frames caught the stealthed Rogue walking steadily — \
         the seed no longer exercises the approach"
    );
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
    /// Where `render_health_bars` should have hung the block this frame: the
    /// interpolated anchor, projected by the same camera, in logical points.
    expected_anchor: Option<Vec2>,
    frames: Vec<PaintedFrame>,
}

struct PaintedFrame {
    /// The health bar background rect, as painted.
    bar: egui::Rect,
    /// The projection of the interpolated anchor this frame.
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

/// egui input for a 2x display; the pass is what gives `render_health_bars`
/// a context to paint into.
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

/// The anchor the draw site is meant to use, computed the way it computes it,
/// immediately after it ran — same camera, same overstep.
fn expect_anchor(
    mut painted: ResMut<Painted>,
    fixed: Res<Time<Fixed>>,
    camera: Query<(&Camera, &GlobalTransform)>,
    combatants: Query<(&Combatant, &Transform, Option<&PreviousSimTranslation>)>,
) {
    painted.expected_anchor = None;
    let (Ok((camera, camera_tf)), Some((c, tf, previous))) =
        (camera.single(), rogue(combatants.iter()))
    else {
        return;
    };
    if !c.stealthed || !c.is_alive() {
        return;
    }
    let world = hud_anchor_translation(tf.translation, previous, fixed.overstep_fraction())
        + Vec3::new(0.0, 3.5, 0.0);
    painted.expected_anchor = camera.world_to_viewport(camera_tf, world).ok();
}

fn end_egui_pass(
    mut painted: ResMut<Painted>,
    mut contexts: Query<&mut EguiContext, With<PrimaryWindow>>,
) {
    let Ok(mut ctx) = contexts.single_mut() else {
        return;
    };
    let output = ctx.get_mut().end_pass();
    let Some(anchor) = painted.expected_anchor.take() else {
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

#[test]
fn stealth_label_rasterises_identically_every_frame() {
    let mut app =
        boot(r#"{"team1":["Rogue"],"team2":["Mage"],"map":"BasicArena","random_seed":42}"#);
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
        .add_systems(Update, expect_anchor.after(render_health_bars))
        .add_systems(Last, end_egui_pass);

    for _ in 0..MAX_FRAMES {
        app.update();
    }
    let painted = std::mem::take(&mut *app.world_mut().resource_mut::<Painted>());

    // Non-vacuity: the Rogue walked, so the unsnapped anchor swept through
    // sub-pixel phases — the condition the defect needs.
    let phases: std::collections::BTreeSet<(i32, i32)> = painted
        .frames
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
        painted.frames.len() >= 200 && phases.len() >= 32,
        "{} painted frames over {} of 64 sub-pixel phase cells — the seed no \
         longer walks a stealthed Rogue across pixel phases",
        painted.frames.len(),
        phases.len()
    );

    let mut first_shape: Option<Vec<(i32, i32)>> = None;
    for (i, f) in painted.frames.iter().enumerate() {
        // The draw site hangs the block from the INTERPOLATED anchor: the bar
        // is centred on it to within the half physical pixel of the snap.
        let off = f.bar.center() - egui::pos2(f.anchor.x, f.anchor.y);
        assert!(
            off.x.abs() <= 0.5 / PIXELS_PER_POINT + ON_PIXEL
                && off.y.abs() <= 0.5 / PIXELS_PER_POINT + ON_PIXEL,
            "frame {i}: the bar is centred {off:?} points off the interpolated anchor \
             — render_health_bars is not hanging the block from hud_anchor_translation"
        );
        assert!(
            is_whole_pixel(f.bar.min.x) && is_whole_pixel(f.bar.min.y),
            "frame {i}: the health bar origin {:?} is not on a whole physical pixel",
            f.bar.min
        );
        assert_eq!(
            f.label.len(),
            9,
            "frame {i}: expected eight outline copies and the text"
        );
        let text = *f.label.last().unwrap();
        for p in &f.label {
            assert!(
                is_whole_pixel(p.x) && is_whole_pixel(p.y),
                "frame {i}: a STEALTH copy is drawn at {p:?}, not on a whole physical \
                 pixel — epaint will round each of the nine copies its own way and \
                 the letters change shape as the anchor's phase changes"
            );
        }
        // The label as one image, in physical pixels: the text relative to the
        // bar, and every copy relative to the text. Identical every frame.
        let mut shape = vec![to_px(text - f.bar.min)];
        shape.extend(f.label.iter().map(|p| to_px(*p - text)));
        match &first_shape {
            None => first_shape = Some(shape),
            Some(first) => assert_eq!(
                &shape, first,
                "frame {i}: the STEALTH label's layout in physical pixels changed \
                 from frame 0 — [text-vs-bar, copy-vs-text x9]"
            ),
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
