//! Main menu screen.
//!
//! Split into a pure egui draw function (`draw_main_menu`) and a thin Bevy
//! wrapper system (`main_menu_ui`), mirroring the `draw_results_screen`
//! pattern in `results_ui.rs` so the menu can be iterated offscreen via the
//! egui_kittest snapshot test (`tests/main_menu_snapshot.rs`).

use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use super::chrome::ScreenNav;
use super::match_config::CharacterClass;
use super::play_match::{class_mesh_color, spawn_arena_environment, spawn_arena_sun};
use super::GameState;
use crate::ui::theme::{self, color, radius, size, space, stroke, text};
use crate::ui::widgets::{Button, Variant};

// ============================================================================
// Ambient 3D backdrop scene
// ============================================================================

/// Marker for every entity in the ambient menu scene (cleanup handle).
#[derive(Component)]
pub struct MenuSceneEntity;

/// Slow orbit around the arena. Parameters live on the camera entity so
/// tuning is one place.
#[derive(Component)]
pub struct MenuOrbitCamera {
    pub radius: f32,
    pub height: f32,
    pub speed_rad_per_sec: f32,
}

/// Where the orbit camera looks: slightly above the arena floor so the
/// walls sit in the lower third of the frame.
const ORBIT_LOOK_AT: Vec3 = Vec3::new(0.0, 2.0, 0.0);

/// Idle tableau: two teams of three facing off near the arena center.
/// Purely visual — these entities carry no gameplay components.
const IDLE_POSES: &[(CharacterClass, Vec3)] = &[
    (CharacterClass::Warrior, Vec3::new(-5.0, 1.0, -3.5)),
    (CharacterClass::Rogue, Vec3::new(-6.5, 1.0, 0.5)),
    (CharacterClass::Priest, Vec3::new(-9.0, 1.0, 3.0)),
    (CharacterClass::Mage, Vec3::new(5.5, 1.0, 3.0)),
    (CharacterClass::Warlock, Vec3::new(7.0, 1.0, -1.0)),
    (CharacterClass::Shaman, Vec3::new(9.0, 1.0, -4.0)),
];

/// Spawns the ambient arena backdrop when entering the main menu: the shared
/// arena environment (floor + walls), the shared sun, an orbiting HDR+bloom
/// camera, and a static tableau of class-colored idle combatants.
///
/// Everything is tagged `MenuSceneEntity` and torn down by
/// `cleanup_menu_scene`, so Options/Encyclopedia/Results round-trips rebuild the
/// scene symmetrically.
pub fn setup_menu_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Same render stack as the match camera (HDR + tonemapping + bloom so the
    // warm lighting glows), but WITHOUT the ArenaCamera/MainCamera markers —
    // PlayMatch camera systems must never pick this camera up.
    commands.spawn((
        Camera3d::default(),
        Camera {
            hdr: true,
            ..default()
        },
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        Transform::from_xyz(0.0, 28.0, 55.0).looking_at(ORBIT_LOOK_AT, Vec3::Y),
        MenuOrbitCamera {
            radius: 55.0,
            height: 28.0,
            speed_rad_per_sec: 0.05, // full orbit ≈ 2 minutes
        },
        MenuSceneEntity,
    ));

    // Backdrop uses the default arena, so no view scaling.
    let sun = spawn_arena_sun(&mut commands, 1.0);
    commands.entity(sun).insert(MenuSceneEntity);

    // Same ambient/clear-color as the match scene (resources — insert
    // overwrites, cleanup removes AmbientLight like cleanup_play_match does).
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.9, 0.85, 0.7),
        brightness: 250.0,
        affects_lightmapped_meshes: true,
    });
    commands.insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.09)));

    // The title backdrop is an open arena — no obstacle meshes.
    for entity in spawn_arena_environment(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        // Menu backdrop: the classic octagon, independent of map selection.
        &Default::default(),
        &[],
    ) {
        commands.entity(entity).insert(MenuSceneEntity);
    }

    // Idle combatants: visual-only capsules (no Combatant/AI/HUD components,
    // so gameplay systems structurally cannot touch them).
    for (class, position) in IDLE_POSES {
        commands.spawn((
            Mesh3d(meshes.add(Capsule3d::new(0.5, 1.5))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: class_mesh_color(*class),
                perceptual_roughness: 0.5,
                metallic: 0.2,
                ..default()
            })),
            Transform::from_translation(*position),
            MenuSceneEntity,
        ));
    }
}

/// Slowly orbits the menu camera around the arena. The angle is derived from
/// absolute elapsed time (not incremented per frame), so re-entering the menu
/// resumes the orbit exactly where it would have been — no drift, no jump.
pub fn orbit_menu_camera(time: Res<Time>, mut cameras: Query<(&mut Transform, &MenuOrbitCamera)>) {
    for (mut transform, orbit) in &mut cameras {
        let angle = time.elapsed_secs() * orbit.speed_rad_per_sec;
        transform.translation = Vec3::new(
            angle.sin() * orbit.radius,
            orbit.height,
            angle.cos() * orbit.radius,
        );
        transform.look_at(ORBIT_LOOK_AT, Vec3::Y);
    }
}

/// Tears the backdrop down when leaving the main menu.
pub fn cleanup_menu_scene(
    mut commands: Commands,
    scene_entities: Query<Entity, With<MenuSceneEntity>>,
) {
    for entity in scene_entities.iter() {
        commands.entity(entity).despawn();
    }
    // Mirrors cleanup_play_match: drop the ambient light; ClearColor is
    // harmless without a camera and the next scene overwrites it.
    commands.remove_resource::<AmbientLight>();
}

// ============================================================================
// Menu UI (egui)
// ============================================================================

/// What the user clicked on the main menu this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    StartMatch,
    Encyclopedia,
    AnimationSandbox,
    Options,
    Exit,
}

/// The menu's buttons, in display order: label, `--ui-script` id, action.
///
/// A UI script has to get INTO a screen before it can check anything on it,
/// so every menu button is opted in to the driver (`menu:MATCH`, …), and the
/// scripts under `tests/ui-scripts/` click them by these ids.
const MENU_ITEMS: [(&str, &str, MenuAction); 5] = [
    ("MATCH", "menu:MATCH", MenuAction::StartMatch),
    (
        "ENCYCLOPEDIA",
        "menu:ENCYCLOPEDIA",
        MenuAction::Encyclopedia,
    ),
    (
        "ANIMATIONS",
        "menu:ANIMATIONS",
        MenuAction::AnimationSandbox,
    ),
    ("OPTIONS", "menu:OPTIONS", MenuAction::Options),
    ("EXIT", "menu:EXIT", MenuAction::Exit),
];

/// The card's content width: the menu column with a [`space::XXXL`] gutter
/// either side, wide enough for the wordmark's rule at the top of its pulse.
const CARD_WIDTH: f32 = size::HERO_BUTTON_WIDTH + 2.0 * space::XXXL;

/// The wordmark's pulse, in radians per second (a cycle of about four
/// seconds). Only brightness and the rule beneath breathe; the glyphs never
/// lose legibility.
const PULSE_RATE: f32 = 1.6;
/// How far toward white the wordmark brightens at the top of its pulse.
const PULSE_LIGHTEN: f32 = 0.35;
/// The rule under the wordmark: its half-width at rest, and how much the
/// pulse adds to it.
const RULE_HALF_WIDTH: f32 = 130.0;
const RULE_PULSE_WIDTH: f32 = 45.0;
/// The rule's opacity at rest, and how much the pulse adds to it.
const RULE_OPACITY: f32 = 0.35;
const RULE_PULSE_OPACITY: f32 = 0.43;

/// Draws the main menu on the Hero template: the full-bleed scene (darkened
/// and vignetted), a centred card holding the wordmark and a vertical menu of
/// Hero buttons, and the version in the corner.
///
/// Pure egui — no Bevy ECS types — so the snapshot test can render it
/// offscreen. `time_secs` drives the wordmark pulse; the test passes a fixed
/// value for determinism.
pub fn draw_main_menu(ctx: &egui::Context, time_secs: f32) -> Option<MenuAction> {
    let mut action = None;

    // Transparent panel: the ambient 3D arena scene renders behind the menu
    // (bevy_egui composites egui over the camera output), so the frame has no
    // fill — readability comes from the scene scrims and the card.
    egui::CentralPanel::default()
        .frame(egui::Frame::new())
        .show(ctx, |ui| paint_scene_scrims(ui));

    egui::Area::new(egui::Id::new("main_menu_card"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(color::HERO_CARD)
                .corner_radius(radius::LG)
                .inner_margin(egui::Margin::symmetric(space::XXXL as i8, space::XL as i8))
                .show(ui, |ui| {
                    ui.set_width(CARD_WIDTH);
                    ui.vertical_centered(|ui| {
                        paint_wordmark(ui, time_secs);
                        ui.add_space(space::SM);
                        ui.label(
                            egui::RichText::new("Arena Combat Autobattler")
                                .size(text::H2)
                                .color(color::TEXT_MUTED),
                        );
                        ui.add_space(space::LG);
                        paint_class_accent_row(ui);
                        ui.add_space(space::XL);

                        for (i, (label, id, button_action)) in MENU_ITEMS.into_iter().enumerate() {
                            if i > 0 {
                                ui.add_space(space::SM);
                            }
                            if Button::new(label, Variant::Hero)
                                .driver_id(id)
                                .show(ui)
                                .clicked()
                            {
                                action = Some(button_action);
                            }
                        }
                    });
                });
        });

    egui::Area::new(egui::Id::new("main_menu_version"))
        .anchor(
            egui::Align2::RIGHT_BOTTOM,
            egui::vec2(-space::XL, -space::LG),
        )
        .interactable(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("v0.1.0 - Prototype")
                    .size(text::BODY)
                    .color(color::TEXT_DIM),
            );
        });

    action
}

/// The wordmark, crisp, with a pulse: the glyphs brighten toward white and a
/// gold rule under them widens and brightens in step. (Offset-text halos read
/// as blur, so there are none.)
fn paint_wordmark(ui: &mut egui::Ui, time_secs: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), text::WORDMARK + space::XL),
        egui::Sense::hover(),
    );
    // 0..1 over the pulse cycle.
    let pulse = 0.5 + 0.5 * (time_secs * PULSE_RATE).sin();
    let painter = ui.painter();
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "ARENASIM",
        egui::FontId::proportional(text::WORDMARK),
        theme::lighten(color::ACCENT, PULSE_LIGHTEN * pulse),
    );
    let half_width = RULE_HALF_WIDTH + RULE_PULSE_WIDTH * pulse;
    let y = rect.bottom() - space::SM;
    painter.line_segment(
        [
            egui::pos2(rect.center().x - half_width, y),
            egui::pos2(rect.center().x + half_width, y),
        ],
        egui::Stroke::new(
            stroke::MARK,
            color::ACCENT.gamma_multiply(RULE_OPACITY + RULE_PULSE_OPACITY * pulse),
        ),
    );
}

/// A centred row of small class-coloured diamonds under the subtitle — one
/// per class, in the UI class palette. Asset-free (no textures to plumb into
/// the snapshot harness).
fn paint_class_accent_row(ui: &mut egui::Ui) {
    const CLASSES: [CharacterClass; 9] = [
        CharacterClass::Warrior,
        CharacterClass::Rogue,
        CharacterClass::Priest,
        CharacterClass::Mage,
        CharacterClass::Warlock,
        CharacterClass::Paladin,
        CharacterClass::Hunter,
        CharacterClass::Shaman,
        CharacterClass::Druid,
    ];
    let pitch = space::XXL;
    let half = space::XS;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), space::MD),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    let start_x = rect.center().x - pitch * (CLASSES.len() as f32 - 1.0) / 2.0;
    for (i, class) in CLASSES.iter().enumerate() {
        let center = egui::pos2(start_x + i as f32 * pitch, rect.center().y);
        let points = vec![
            center + egui::Vec2::new(0.0, -half),
            center + egui::Vec2::new(half, 0.0),
            center + egui::Vec2::new(0.0, half),
            center + egui::Vec2::new(-half, 0.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            points,
            theme::class_color(*class),
            egui::Stroke::NONE,
        ));
    }
}

/// The Hero template's scene treatment, so the menu reads over the bright 3D
/// backdrop: a light tint over everything, and an edge vignette.
fn paint_scene_scrims(ui: &egui::Ui) {
    let screen = ui.max_rect();
    let painter = ui.painter();

    // Bloom highlights must not fight the text.
    painter.rect_filled(screen, egui::CornerRadius::ZERO, color::SCENE_TINT);

    // Four strips fading from dark at the screen edge to transparent
    // `size::VIGNETTE` inward, one mesh with per-vertex alpha.
    let edge = color::SCENE_VIGNETTE;
    let clear = egui::Color32::TRANSPARENT;
    let depth = size::VIGNETTE;
    let mut mesh = egui::Mesh::default();
    let mut quad = |corners: [egui::Pos2; 4]| {
        let base = mesh.vertices.len() as u32;
        for (pos, color) in corners.into_iter().zip([edge, edge, clear, clear]) {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    let (l, r, t, b) = (screen.left(), screen.right(), screen.top(), screen.bottom());
    // Top, bottom, left, right — outer pair dark, inner pair transparent.
    quad([
        egui::pos2(l, t),
        egui::pos2(r, t),
        egui::pos2(r, t + depth),
        egui::pos2(l, t + depth),
    ]);
    quad([
        egui::pos2(l, b),
        egui::pos2(r, b),
        egui::pos2(r, b - depth),
        egui::pos2(l, b - depth),
    ]);
    quad([
        egui::pos2(l, t),
        egui::pos2(l, b),
        egui::pos2(l + depth, b),
        egui::pos2(l + depth, t),
    ]);
    quad([
        egui::pos2(r, t),
        egui::pos2(r, b),
        egui::pos2(r - depth, b),
        egui::pos2(r - depth, t),
    ]);
    painter.add(egui::Shape::mesh(mesh));
}

/// Bevy wrapper: draws the menu and applies the chosen action.
pub fn main_menu_ui(
    mut contexts: EguiContexts,
    time: Res<Time>,
    mut nav: ScreenNav,
    mut commands: Commands,
    primary_window: Query<Entity, With<bevy::window::PrimaryWindow>>,
) {
    // Use try_ctx_mut to gracefully handle window close (the context
    // dies with the primary window; ctx_mut panics on the final frame)
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    match draw_main_menu(ctx, time.elapsed_secs()) {
        Some(MenuAction::StartMatch) => {
            info!("Match button pressed - transitioning to ConfigureMatch");
            nav.open(GameState::ConfigureMatch);
        }
        Some(MenuAction::Encyclopedia) => {
            info!("Encyclopedia button pressed - transitioning to Encyclopedia");
            nav.open(GameState::Encyclopedia);
        }
        Some(MenuAction::AnimationSandbox) => {
            info!("Animations button pressed - transitioning to AnimationSandbox");
            nav.open(GameState::AnimationSandbox);
        }
        Some(MenuAction::Options) => {
            info!("Options button pressed - transitioning to Options");
            nav.open(GameState::Options);
        }
        Some(MenuAction::Exit) => {
            info!("Exit button pressed - closing primary window");
            // Do NOT write `AppExit` from a system here: on macOS the
            // programmatic exit path can deadlock the winit event
            // loop and the app freezes instead of quitting
            // (bevyengine/bevy#23313 — observed here on Bevy 0.16
            // after the 0.16/winit-0.30 migration). Despawning the
            // primary window re-enters winit's native close path and
            // the default `ExitCondition::OnAllClosed` exits cleanly.
            for window in primary_window.iter() {
                commands.entity(window).despawn();
            }
        }
        None => {}
    }
}
