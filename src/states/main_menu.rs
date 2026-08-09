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

use super::match_config::CharacterClass;
use super::play_match::{class_mesh_color, spawn_arena_environment, spawn_arena_sun};
use super::GameState;

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
/// `cleanup_menu_scene`, so Options/Armory/Results round-trips rebuild the
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
    for entity in
        spawn_arena_environment(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            // Menu backdrop: the classic octagon, independent of map selection.
            &Default::default(),
            &[],
        )
    {
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
pub fn orbit_menu_camera(
    time: Res<Time>,
    mut cameras: Query<(&mut Transform, &MenuOrbitCamera)>,
) {
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
    Armory,
    AnimationSandbox,
    Options,
    Exit,
}

/// The menu's buttons, in display order.
///
/// Single source of truth: the button loop renders these and
/// `paint_backdrop_scrims` sizes the scrim from the count, so adding an entry
/// cannot leave the last button hanging outside the backdrop.
const MENU_ITEMS: [(&str, MenuAction); 5] = [
    ("MATCH", MenuAction::StartMatch),
    ("ARMORY", MenuAction::Armory),
    ("ANIMATIONS", MenuAction::AnimationSandbox),
    ("OPTIONS", MenuAction::Options),
    ("EXIT", MenuAction::Exit),
];

/// Vertical distance between successive button centres, in points. Matches the
/// button height plus the 10pt spacer the loop adds.
const BUTTON_PITCH: f32 = 73.0;

/// Draws the main menu. Pure egui — no Bevy ECS types — so the snapshot test
/// can render it offscreen. `time_secs` drives the title pulse; the test
/// passes a fixed value for determinism.
pub fn draw_main_menu(ctx: &egui::Context, time_secs: f32) -> Option<MenuAction> {
    let mut action = None;

    // Transparent panel: the ambient 3D arena scene renders behind the menu
    // (bevy_egui composites egui over the camera output), so the frame has no
    // fill — readability comes from the painted scrims below.
    egui::CentralPanel::default()
        .frame(egui::Frame::new())
        .show(ctx, |ui| {
            paint_backdrop_scrims(ui);

            ui.vertical_centered(|ui| {
                ui.add_space(150.0);

                // Title with a pulsing soft glow: layered offset halo copies
                // under a solid glyph. Only the halo alpha pulses, so the
                // title itself never loses legibility.
                let (title_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 96.0),
                    egui::Sense::hover(),
                );
                let title_font = egui::FontId::proportional(72.0);
                // 0..1 over a ~4s cycle
                let pulse = 0.5 + 0.5 * (time_secs * 1.6).sin();
                // Crisp pulse, no halo copies (offset-text halos read as
                // blur): the glyph brightness breathes, and a gold accent
                // rule under the title widens/brightens in sync.
                let brighten =
                    |c: u8| (c as f32 + (255 - c) as f32 * 0.35 * pulse).round() as u8;
                let gold = egui::Color32::from_rgb(brighten(230), brighten(204), brighten(153));
                let painter = ui.painter();
                painter.text(
                    title_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "ARENASIM",
                    title_font,
                    gold,
                );
                let rule_half_width = 130.0 + 45.0 * pulse;
                let rule_alpha = (90.0 + 110.0 * pulse) as u8;
                let rule_y = title_rect.bottom() - 8.0;
                painter.line_segment(
                    [
                        egui::pos2(title_rect.center().x - rule_half_width, rule_y),
                        egui::pos2(title_rect.center().x + rule_half_width, rule_y),
                    ],
                    egui::Stroke::new(
                        2.0,
                        egui::Color32::from_rgba_unmultiplied(230, 204, 153, rule_alpha),
                    ),
                );

                ui.add_space(10.0);

                // Subtitle
                ui.label(
                    egui::RichText::new("Arena Combat Autobattler")
                        .size(24.0)
                        .color(egui::Color32::from_rgb(178, 166, 150)),
                );

                ui.add_space(18.0);

                paint_class_accent_row(ui);

                ui.add_space(42.0);

                // Menu buttons
                for (label, button_action) in MENU_ITEMS {
                    if menu_button(ui, label).clicked() {
                        action = Some(button_action);
                    }
                    ui.add_space(10.0);
                }
            });

            // Version text in bottom right
            ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("v0.1.0 - Prototype")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(102, 102, 102)),
                    );
                });
            });
        });

    action
}

/// A main-menu button: dark translucent fill with a subtle stroke, and a
/// gold highlight ring + brightened label on hover. Hover styling is painted
/// manually (not via global `ctx` style mutation) so it can't leak into
/// other screens.
fn menu_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let button_size = egui::vec2(280.0, 60.0);
    let response = ui.add_sized(
        button_size,
        egui::Button::new(
            egui::RichText::new(label)
                .size(28.0)
                .color(egui::Color32::from_rgb(230, 217, 191)),
        )
        .fill(egui::Color32::from_rgba_unmultiplied(24, 24, 36, 200))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 70, 88))),
    );
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            6.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 204, 153)),
            egui::StrokeKind::Outside,
        );
        // Repaint the label brighter — cheaper than a second widget style.
        ui.painter().text(
            response.rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(28.0),
            egui::Color32::from_rgb(255, 244, 214),
        );
    }
    response
}

/// A centered row of small class-colored diamonds under the subtitle — one
/// per class, using the shared UI class palette. Asset-free (no textures to
/// plumb into the snapshot harness).
fn paint_class_accent_row(ui: &mut egui::Ui) {
    const CLASSES: [CharacterClass; 8] = [
        CharacterClass::Warrior,
        CharacterClass::Rogue,
        CharacterClass::Priest,
        CharacterClass::Mage,
        CharacterClass::Warlock,
        CharacterClass::Paladin,
        CharacterClass::Hunter,
        CharacterClass::Shaman,
    ];
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 14.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    let spacing = 30.0;
    let start_x = rect.center().x - spacing * (CLASSES.len() as f32 - 1.0) / 2.0;
    for (i, class) in CLASSES.iter().enumerate() {
        let center = egui::pos2(start_x + i as f32 * spacing, rect.center().y);
        let radius = 5.0;
        let points = vec![
            center + egui::vec2(0.0, -radius),
            center + egui::vec2(radius, 0.0),
            center + egui::vec2(0.0, radius),
            center + egui::vec2(-radius, 0.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            points,
            super::results_ui::class_color32(*class),
            egui::Stroke::NONE,
        ));
    }
}

/// Paints the darkening layers that keep the menu readable over the bright
/// 3D backdrop: a light full-screen tint, an edge vignette, and a stronger
/// scrim column behind the title + buttons.
fn paint_backdrop_scrims(ui: &egui::Ui) {
    let screen = ui.max_rect();
    let painter = ui.painter();

    // 1. Global darkening so bloom highlights don't fight the text.
    painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(70));

    // 2. Edge vignette: four strips fading from dark at the screen edge to
    //    transparent ~180px inward, built as one mesh with per-vertex alpha.
    let edge_color = egui::Color32::from_black_alpha(150);
    let clear = egui::Color32::TRANSPARENT;
    let depth = 180.0;
    let mut mesh = egui::Mesh::default();
    let mut quad = |corners: [egui::Pos2; 4], colors: [egui::Color32; 4]| {
        let base = mesh.vertices.len() as u32;
        for (pos, color) in corners.into_iter().zip(colors) {
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
    quad(
        [
            egui::pos2(l, t),
            egui::pos2(r, t),
            egui::pos2(r, t + depth),
            egui::pos2(l, t + depth),
        ],
        [edge_color, edge_color, clear, clear],
    );
    quad(
        [
            egui::pos2(l, b),
            egui::pos2(r, b),
            egui::pos2(r, b - depth),
            egui::pos2(l, b - depth),
        ],
        [edge_color, edge_color, clear, clear],
    );
    quad(
        [
            egui::pos2(l, t),
            egui::pos2(l, b),
            egui::pos2(l + depth, b),
            egui::pos2(l + depth, t),
        ],
        [edge_color, edge_color, clear, clear],
    );
    quad(
        [
            egui::pos2(r, t),
            egui::pos2(r, b),
            egui::pos2(r - depth, b),
            egui::pos2(r - depth, t),
        ],
        [edge_color, edge_color, clear, clear],
    );
    painter.add(egui::Shape::mesh(mesh));

    // 3. Center scrim column behind the title + buttons (the menu content is
    //    laid out from a 150px top spacer down to the last button).
    //
    //    The height is DERIVED from the button count, not hardcoded: adding a
    //    fifth entry to `MENU_ITEMS` previously left the last button hanging
    //    outside the scrim, because the 540px figure was tuned for four.
    let extra_buttons = MENU_ITEMS.len() as f32 - 4.0;
    let column_height = 540.0 + BUTTON_PITCH * extra_buttons;
    let column = egui::Rect::from_center_size(
        egui::pos2(
            screen.center().x,
            screen.top() + 390.0 + BUTTON_PITCH * extra_buttons / 2.0,
        ),
        egui::vec2(480.0, column_height),
    );
    painter.rect_filled(column, 16.0, egui::Color32::from_black_alpha(120));
}

/// Bevy wrapper: draws the menu and applies the chosen action.
pub fn main_menu_ui(
    mut contexts: EguiContexts,
    time: Res<Time>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
    primary_window: Query<Entity, With<bevy::window::PrimaryWindow>>,
) {
    // Use try_ctx_mut to gracefully handle window close (the context
    // dies with the primary window; ctx_mut panics on the final frame)
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    match draw_main_menu(ctx, time.elapsed_secs()) {
        Some(MenuAction::StartMatch) => {
            info!("Match button pressed - transitioning to ConfigureMatch");
            next_state.set(GameState::ConfigureMatch);
        }
        Some(MenuAction::Armory) => {
            info!("Armory button pressed - transitioning to Armory");
            next_state.set(GameState::Armory);
        }
        Some(MenuAction::AnimationSandbox) => {
            info!("Animations button pressed - transitioning to AnimationSandbox");
            next_state.set(GameState::AnimationSandbox);
        }
        Some(MenuAction::Options) => {
            info!("Options button pressed - transitioning to Options");
            next_state.set(GameState::Options);
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
