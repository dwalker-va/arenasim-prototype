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
//! tests pin all four parts:
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
//! 3. the drawn FACING turns every frame too, by the tick's turn times the
//!    frame's share of a tick, and a facing snap is drawn at the new facing
//!    rather than swung through — while the sim, at every tick, still sees
//!    exactly the rotation its previous tick left. Measured before the fix, at
//!    120Hz over the first minute of the Warrior-Hunter match below: a turning
//!    unit drew the same facing as the frame before on 184 of 486 frames and
//!    a whole tick's turn on the rest, the weapons in its hands with it;
//!
//! 4. drawing the HUD after propagation never outruns egui's font atlas: on
//!    every frame egui re-uploads its managed texture (new glyphs), the new
//!    image's `AssetEvent::Added` is flushed in that same frame, before the
//!    render world extracts it. When it was not, that frame rendered with no
//!    font texture, and every piece of egui text and every filled shape
//!    blinked out while icon images stayed — 24 of 24 atlas updates in a
//!    probed run of the client.
//!
//! Runs with no GPU (same shape as `tests/hit_reaction_match_flow.rs`); the
//! later tests add a `Window` COMPONENT, never an OS window. Visual-only
//! systems never touch the sim, so the match is the seeded one.

use std::collections::BTreeMap;
use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy_egui::{egui, EguiContext};

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{ArenaCamera, Combatant, Pet};
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
    boot_match(MATCH, frame)
}

fn boot_match(cfg: &str, frame: Duration) -> App {
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

/// A match whose units turn all the time: the Warrior chasing, the Hunter
/// kiting, the pet running at its target, with every kind of facing snap
/// (reversals, compass-direction commits) in between.
const TURNING_MATCH: &str =
    r#"{"team1":["Warrior"],"team2":["Hunter"],"map":"BasicArena","random_seed":11}"#;
/// Sim time to run: through the opening chase and the first kites.
const TURNING_SECS: f32 = 40.0;
/// A tick's turn counts as a steady turn when it is at least this long
/// (radians)...
const TURNING: f32 = 5e-4;
/// ...and differs from the tick before by no more than this fraction of it.
const STEADY_TURN: f32 = 0.02;
/// Larger than any continuous turn in one tick: the interpolation's own
/// facing-snap threshold (`MAX_TICK_TURN`, 45 degrees).
const SNAP: f32 = std::f32::consts::FRAC_PI_4;

/// A unit, named by what the sim calls it rather than by `Entity`.
type Unit = (u8, u8, bool);

/// Heading about the vertical axis. Every facing the sim sets is a pure yaw.
fn yaw(q: Quat) -> f32 {
    q.to_euler(EulerRot::YXZ).0
}

/// `b - a` as the shorter signed angle.
fn turn(a: f32, b: f32) -> f32 {
    let d = (b - a).rem_euclid(std::f32::consts::TAU);
    if d > std::f32::consts::PI {
        d - std::f32::consts::TAU
    } else {
        d
    }
}

#[derive(Resource, Default)]
struct Turns {
    /// Every unit's sim facing after every tick.
    ticks: Vec<BTreeMap<Unit, Quat>>,
    /// Ticks at which the sim saw a facing other than the one its previous
    /// tick left: (tick, unit, left, saw).
    sim_saw_drawn: Vec<(usize, Unit, Quat, Quat)>,
    /// Per rendered frame: how many ticks had run, and every unit's facing as
    /// drawn.
    frames: Vec<(usize, BTreeMap<Unit, Quat>)>,
}

fn facings<'a>(
    units: impl Iterator<Item = (&'a Combatant, Option<&'a Pet>, Quat)>,
) -> BTreeMap<Unit, Quat> {
    units
        .filter(|(c, ..)| c.is_alive())
        .map(|(c, pet, q)| ((c.team, c.slot, pet.is_some()), q))
        .collect()
}

fn observe_turn_start(
    mut turns: ResMut<Turns>,
    units: Query<(&Combatant, Option<&Pet>, &Transform)>,
) {
    let now = facings(units.iter().map(|(c, p, t)| (c, p, t.rotation)));
    let tick = turns.ticks.len();
    let Some(last) = turns.ticks.last() else {
        return;
    };
    let leaks: Vec<_> = now
        .iter()
        .filter_map(|(u, q)| {
            let left = *last.get(u)?;
            (*q != left).then_some((tick, *u, left, *q))
        })
        .collect();
    turns.sim_saw_drawn.extend(leaks);
}

fn observe_turn_end(
    mut turns: ResMut<Turns>,
    units: Query<(&Combatant, Option<&Pet>, &Transform)>,
) {
    let now = facings(units.iter().map(|(c, p, t)| (c, p, t.rotation)));
    turns.ticks.push(now);
}

/// After propagation: the facing every model renders at this frame.
fn observe_drawn_facing(
    mut turns: ResMut<Turns>,
    units: Query<(&Combatant, Option<&Pet>, &GlobalTransform)>,
) {
    let now = facings(
        units
            .iter()
            .map(|(c, p, gt)| (c, p, gt.compute_transform().rotation)),
    );
    let ticks = turns.ticks.len();
    turns.frames.push((ticks, now));
}

#[test]
fn drawn_facing_turns_every_frame_while_the_sim_keeps_its_own_rotations() {
    for frame in FRAME_RATES {
        let mut app = boot_match(TURNING_MATCH, frame);
        app.init_resource::<Turns>()
            .add_systems(FixedPreUpdate, observe_turn_start)
            .add_systems(FixedPostUpdate, observe_turn_end)
            .add_systems(Last, observe_drawn_facing);
        for _ in 0..(TURNING_SECS / frame.as_secs_f32()) as usize {
            app.update();
        }
        let tick_secs = app
            .world()
            .resource::<Time<Fixed>>()
            .timestep()
            .as_secs_f32();
        let turns = std::mem::take(&mut *app.world_mut().resource_mut::<Turns>());
        let frame_secs = frame.as_secs_f32();
        let hz = 1.0 / frame_secs;

        // The sim is authoritative: every tick starts from exactly the facing
        // the tick before it left, however the frames in between drew it.
        if let Some((tick, unit, left, saw)) = turns.sim_saw_drawn.first() {
            panic!(
                "{hz:.0}Hz: the sim started {} unit-ticks from a facing its previous tick did \
                 not leave — the render interpolation leaked into the sim. First: tick {tick}, \
                 {unit:?} was left at {left:?} and the next tick saw {saw:?}",
                turns.sim_saw_drawn.len()
            );
        }

        let (mut steady, mut snaps) = (0, 0);
        for pair in turns.frames.windows(2) {
            let ((k0, a0), (k1, a1)) = (&pair[0], &pair[1]);
            let (k0, k1) = (*k0, *k1);
            if k1 < 3 || k1 - k0 > 1 {
                continue;
            }
            let p = &turns.ticks[k1 - 3..k1];
            for (unit, drawn) in a1 {
                let (Some(q0), Some(q1), Some(q2), Some(before)) =
                    (p[0].get(unit), p[1].get(unit), p[2].get(unit), a0.get(unit))
                else {
                    continue;
                };
                let (d1, d2) = (turn(yaw(*q0), yaw(*q1)), turn(yaw(*q1), yaw(*q2)));

                // A facing snap is drawn at the new facing, not swung
                // through the angles in between.
                if d2.abs() > SNAP {
                    snaps += 1;
                    // (Compared as headings: `angle_between` is an `acos`,
                    // which turns the last-ULP noise of decomposing a
                    // `GlobalTransform` into a tenth of a degree.)
                    let short = turn(yaw(*drawn), yaw(*q2));
                    assert!(
                        short.abs() < 1e-4,
                        "{hz:.0}Hz, frame after tick {k1}: {unit:?} snapped {:.0} degrees \
                         in one tick and was drawn {:.1} degrees short of its new facing — \
                         the snap is being swung through",
                        d2.to_degrees(),
                        short.to_degrees()
                    );
                    continue;
                }

                if d2.abs() < TURNING || (d2 - d1).abs() > STEADY_TURN * d2.abs() {
                    continue;
                }
                steady += 1;
                let expected = d2 * (frame_secs / tick_secs);
                let actual = turn(yaw(*before), yaw(*drawn));
                assert!(
                    (actual - expected).abs() <= 0.05 * expected.abs(),
                    "{hz:.0}Hz, frame after tick {k1}: {unit:?} was drawn turning {:.4} \
                     degrees, expected {:.4} ({:.0}% of a tick's turn for {:.0}% of a \
                     tick's time) — the facing is stepping with the sim instead of turning \
                     smoothly",
                    actual.to_degrees(),
                    expected.to_degrees(),
                    100.0 * actual / d2,
                    100.0 * frame_secs / tick_secs,
                );
            }
        }
        // At 60Hz: 1103 steady-turn frames and 6 snaps; proportionally more
        // at the faster rates.
        assert!(
            steady >= 500 && snaps >= 3,
            "{hz:.0}Hz: {steady} frames caught a unit turning steadily and {snaps} caught a \
             facing snap — the seed no longer exercises turning"
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

/// Per frame: every managed egui texture whose `Image` handle changed, and
/// whether that handle's `Added` event was flushed in the same frame.
#[derive(Resource, Default)]
struct Uploads {
    handles: std::collections::HashMap<(Entity, u64), AssetId<Image>>,
    frame: usize,
    /// Every re-upload seen, as (frame, flushed that frame).
    seen: Vec<(usize, bool)>,
}

fn observe_uploads(
    mut uploads: ResMut<Uploads>,
    managed: Res<bevy_egui::EguiManagedTextures>,
    mut events: EventReader<AssetEvent<Image>>,
) {
    uploads.frame += 1;
    let added: std::collections::HashSet<AssetId<Image>> = events
        .read()
        .filter_map(|e| match e {
            AssetEvent::Added { id } => Some(*id),
            _ => None,
        })
        .collect();
    let frame = uploads.frame;
    for (key, texture) in managed.iter() {
        let id = texture.handle.id();
        if uploads.handles.insert(*key, id) != Some(id) {
            uploads.seen.push((frame, added.contains(&id)));
        }
    }
}

#[test]
fn a_font_atlas_upload_is_ready_in_the_frame_that_draws_with_it() {
    let frame = Duration::from_micros(8_333);
    let mut app = boot(frame);
    app.add_plugins(bevy_egui::EguiPlugin {
        enable_multipass_for_primary_context: false,
    });
    let mut window = Window {
        resolution: WindowResolution::new(WINDOW.0, WINDOW.1)
            .with_scale_factor_override(PIXELS_PER_POINT),
        ..default()
    };
    window.resolution.set_scale_factor(PIXELS_PER_POINT);
    app.world_mut().spawn((window, PrimaryWindow));
    app.init_resource::<bevy::render::camera::ManualTextureViews>()
        .add_systems(
            PostUpdate,
            bevy::render::camera::camera_system
                .after(bevy::transform::TransformSystem::TransformPropagate),
        )
        .init_resource::<Uploads>()
        .add_systems(Last, observe_uploads);
    for _ in 0..frames_for(frame) {
        app.update();
    }
    let uploads = std::mem::take(&mut *app.world_mut().resource_mut::<Uploads>());

    assert!(
        uploads.seen.len() >= 3,
        "only {} font atlas uploads in the match — nothing exercised the upload path",
        uploads.seen.len()
    );
    let late: Vec<usize> = uploads
        .seen
        .iter()
        .filter(|(_, flushed)| !flushed)
        .map(|(f, _)| *f)
        .collect();
    assert!(
        late.is_empty(),
        "{} of {} font atlas uploads were flushed a frame late (frames {late:?}): \
         the render world has no font texture on those frames, so all egui text \
         and filled shapes blink out — egui's texture upload must run before \
         `AssetEvents` in `PostUpdate`",
        late.len(),
        uploads.seen.len()
    );
}
