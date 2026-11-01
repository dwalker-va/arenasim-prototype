//! Configure Match UI - Team Setup and Map Selection
//!
//! This module handles the match configuration screen where players:
//! - Select team sizes (1-3 combatants per team)
//! - Choose character classes for each team slot
//! - Select the arena map
//! - Start the match when ready
//!
//! ## UI Structure
//! The Setup template (`docs/design/proposals/2026-10-11-ui-design-system.md`
//! §2.4), built from the widget kit:
//! - **Header**: the shared `screen_header`, whose exit names the opener
//! - **Three panel columns**: Team 1 | Arena/Map | Team 2, the team panels
//!   edged in the team tokens; slots and kill targets are `list_row`s
//! - **Character picker**: a kit `Modal` of class `list_row`s
//! - **Docked footer**: the screen's one Primary button, enabled only when
//!   every slot is filled (`--ui-script` id `configure:start`)
//!
//! ## Interaction Flow
//! 1. User adjusts team sizes with the − / + buttons
//! 2. Clicks empty character slots to open the picker
//! 3. Selects class from the picker, slot updates
//! 4. Cycles through maps with arrow buttons
//! 5. Clicks "START MATCH" when all slots filled

use super::chrome::ScreenNav;
use super::play_match::arena_bounds::{outline_half_extents, ArenaBounds, WALL_OFFSET};
use super::play_match::map_config::MapGeometryConfig;
use super::play_match::map_geometry::{prism_vertices_world, ObstacleVolume};
use super::play_match::{spawn_arena_environment, WALL_ARC_SEGMENTS};
use super::view_combatant_ui::ViewCombatantState;
use super::{
    match_config::{self, ArenaMap, MatchConfig},
    GameState,
};
use crate::states::match_config::CharacterClass;
use crate::ui::driver as ui_driver;
use crate::ui::theme::{class_color, color, data, radius, size, space, stroke, text};
use crate::ui::widgets::panel::margin_xy;
use crate::ui::widgets::{
    list_row, panel, panel_header, screen_header, section_label, Button, ListIcon, ListRow, Modal,
    Nav, NavAction, Variant,
};
use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::RenderLayers;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;

/// Resource storing loaded class icon textures for egui rendering.
/// Maps CharacterClass to egui TextureId for efficient icon display.
#[derive(Resource, Default)]
pub struct ClassIcons {
    /// Map of class to egui texture ID
    pub textures: HashMap<match_config::CharacterClass, egui::TextureId>,
    /// Whether icons have been loaded
    pub loaded: bool,
}

/// Resource storing the Bevy image handles for class icons.
/// These are kept alive to prevent the assets from being unloaded.
#[derive(Resource, Default)]
pub struct ClassIconHandles {
    pub handles: Vec<Handle<Image>>,
}

/// System to load class icons and register them with egui.
/// This runs during ConfigureMatch state update and only loads once.
pub fn load_class_icons(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    mut class_icons: ResMut<ClassIcons>,
    mut icon_handles: ResMut<ClassIconHandles>,
    images: Res<Assets<Image>>,
) {
    // Only load once
    if class_icons.loaded {
        return;
    }

    // Check if all images are loaded
    let class_paths = [
        (
            match_config::CharacterClass::Warrior,
            "icons/classes/warrior.png",
        ),
        (match_config::CharacterClass::Mage, "icons/classes/mage.png"),
        (
            match_config::CharacterClass::Rogue,
            "icons/classes/rogue.png",
        ),
        (
            match_config::CharacterClass::Priest,
            "icons/classes/priest.png",
        ),
        (
            match_config::CharacterClass::Warlock,
            "icons/classes/warlock.png",
        ),
        (
            match_config::CharacterClass::Paladin,
            "icons/classes/paladin.png",
        ),
        (
            match_config::CharacterClass::Hunter,
            "icons/classes/hunter.png",
        ),
        (
            match_config::CharacterClass::Shaman,
            "icons/classes/shaman.png",
        ),
        (
            match_config::CharacterClass::Druid,
            "icons/classes/druid.png",
        ),
    ];

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        for (_, path) in &class_paths {
            let handle: Handle<Image> = asset_server.load(*path);
            icon_handles.handles.push(handle);
        }
        return; // Wait for next frame to check if loaded
    }

    // Check if all images are loaded
    let all_loaded = icon_handles.handles.iter().all(|h| images.contains(h));
    if !all_loaded {
        return; // Wait for images to load
    }

    // Register textures with egui
    for (i, (class, _)) in class_paths.iter().enumerate() {
        let handle = icon_handles.handles[i].clone();
        let texture_id = contexts.add_image(handle);
        class_icons.textures.insert(*class, texture_id);
    }

    class_icons.loaded = true;
    info!("Class icons loaded and registered with egui");
}

/// State for the character picker modal.
/// Tracks which slot is being edited when the modal is open.
#[derive(Resource, Default)]
pub struct CharacterPickerState {
    /// Whether the modal is currently visible
    pub active: bool,
    /// Team being edited (1 or 2)
    pub team: u8,
    /// Slot index being edited (0-2)
    pub slot: usize,
}

/// A user action produced by the Configure Match screen that requires ECS
/// side effects (state transitions, resource inserts). Everything that only
/// mutates `MatchConfig` / `CharacterPickerState` is handled inside the pure
/// draw function; only these bubble out to the Bevy wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigureMatchAction {
    /// The header exit: leave for wherever opened the screen.
    Exit,
    /// Begin the match with the current config.
    StartMatch,
    /// Open the View Combatant screen for a filled slot.
    ViewCombatant {
        class: match_config::CharacterClass,
        team: u8,
        slot: usize,
    },
}

/// Main UI system for the Configure Match screen (Bevy wrapper).
///
/// Grabs the egui context and resources, drives the pure
/// [`draw_configure_match`] renderer, and applies the returned
/// [`ConfigureMatchAction`] via ECS.
pub fn configure_match_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<MatchConfig>,
    mut nav: ScreenNav,
    mut picker_state: Option<ResMut<CharacterPickerState>>,
    mut commands: Commands,
    class_icons: Res<ClassIcons>,
    map_geometry: Res<MapGeometryConfig>,
    map_preview: Option<Res<MapPreview>>,
) {
    // Initialize picker state if it doesn't exist
    if picker_state.is_none() {
        commands.insert_resource(CharacterPickerState::default());
    }

    // Esc closes the character picker first, then takes the header's exit.
    // It needs no egui context, so it answers on any frame.
    let screen_nav = nav.nav(false).expect("Configure Match has an exit");
    let escape = nav.escape(false, || match picker_state.as_deref_mut() {
        Some(picker) if picker.active => {
            picker.active = false;
            true
        }
        _ => false,
    });
    if let Some(NavAction::Exit(to)) = escape {
        nav.leave(to);
        return;
    }

    // Use try_ctx_mut to avoid panic when context isn't ready during state transitions
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    // The pure renderer needs a live picker state; if the resource was only
    // just queued this frame, fall back to a scratch one so the screen still
    // draws (the resource lands next frame).
    let mut scratch_picker = CharacterPickerState::default();
    let picker: &mut CharacterPickerState = match picker_state {
        Some(ref mut p) => p,
        None => &mut scratch_picker,
    };

    // Show the live 3D preview texture once it's registered and rendering the
    // currently-selected map; otherwise the pure renderer falls back to the
    // vector schematic (which is also what the offscreen egui harness sees).
    let preview_texture = map_preview
        .as_ref()
        .filter(|p| p.rendered_map == config.map)
        .map(|p| p.texture_id);

    match draw_configure_match(
        ctx,
        &mut config,
        picker,
        &class_icons,
        &map_geometry,
        preview_texture,
        screen_nav,
    ) {
        Some(ConfigureMatchAction::Exit) => {
            nav.leave(screen_nav.destination());
        }
        Some(ConfigureMatchAction::StartMatch) => {
            info!("Starting match with config: {:?}", *config);
            nav.open_match();
        }
        Some(ConfigureMatchAction::ViewCombatant { class, team, slot }) => {
            commands.insert_resource(ViewCombatantState { class, team, slot });
            nav.open(GameState::ViewCombatant);
        }
        None => {}
    }
}

/// The screen title, as the header draws it.
const TITLE: &str = "Configure Match";

/// The call to action's minimum width, so it keeps one size whichever label
/// it wears (START MATCH, or the prompt to fill the slots).
const CTA_MIN_WIDTH: f32 = 300.0;

/// A filled slot's Change button's minimum width; the slot row gives up this
/// much (and a gap) to it, so every row in a column ends at the same x.
const CHANGE_WIDTH: f32 = 72.0;

/// The widest the arena preview is drawn, whatever the column's width.
const PREVIEW_MAX_WIDTH: f32 = 230.0;

/// The tallest the character picker's list grows, as a fraction of the
/// screen, before it scrolls.
const PICKER_MAX_HEIGHT: f32 = 0.7;

/// The character picker's modal id: its egui id and its `--ui-script` name
/// (the close `×` registers as `configure:picker:close`).
const PICKER_ID: &str = "configure:picker";

/// Render the entire Configure Match screen into `ctx`, returning any action
/// that needs ECS side effects (see [`ConfigureMatchAction`]).
///
/// The Setup template: the shared [`screen_header`], three [`panel`] columns
/// (Team 1, the arena, Team 2), and a docked footer holding the screen's one
/// Primary button.
///
/// This is deliberately free of Bevy ECS system params (it takes plain
/// references) so it can be driven directly by an egui harness — see
/// `tests/configure_match_snapshot.rs`, which renders it offscreen with
/// `egui_kittest` for a fast, human-free visual-iteration loop. All state that
/// only mutates the config or the picker modal is applied in place here; only
/// state transitions bubble out through the return value.
pub fn draw_configure_match(
    ctx: &egui::Context,
    config: &mut MatchConfig,
    picker: &mut CharacterPickerState,
    class_icons: &ClassIcons,
    map_geometry: &MapGeometryConfig,
    preview_texture: Option<egui::TextureId>,
    nav: Nav,
) -> Option<ConfigureMatchAction> {
    let mut action: Option<ConfigureMatchAction> = None;

    // The docked footer: shown before the central panel so it claims the
    // bottom of the screen and the columns take what is left.
    egui::TopBottomPanel::bottom("configure:footer")
        .frame(
            egui::Frame::new()
                .fill(color::BG)
                .inner_margin(margin_xy(space::XL, space::LG)),
        )
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                let is_valid = config.is_valid();
                let label = if is_valid {
                    "Start match"
                } else {
                    "Select characters to continue"
                };
                if Button::new(label, Variant::Primary)
                    .enabled(is_valid)
                    .min_width(CTA_MIN_WIDTH)
                    .driver_id("configure:start")
                    .show(ui)
                    .clicked()
                {
                    action = Some(ConfigureMatchAction::StartMatch);
                }
            });
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(color::BG)
                .inner_margin(margin_xy(space::XL, space::LG)),
        )
        .show(ctx, |ui| {
            if screen_header(ui, TITLE, nav, |_| {}).is_some() {
                action = Some(ConfigureMatchAction::Exit);
            }
            ui.add_space(space::LG);

            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = space::MD;
                ui.columns(3, |columns| {
                    if let Some(a) =
                        render_team_panel(&mut columns[0], config, 1, picker, class_icons)
                    {
                        action = Some(a);
                    }
                    panel(None).show(&mut columns[1], |ui| {
                        ui.set_min_width(ui.available_width());
                        render_map_panel(ui, config, map_geometry, preview_texture);
                    });
                    if let Some(a) =
                        render_team_panel(&mut columns[2], config, 2, picker, class_icons)
                    {
                        action = Some(a);
                    }
                });
            });
        });

    if picker.active {
        render_character_picker_modal(ctx, config, picker, class_icons);
    }

    action
}

/// The class currently in `team`'s `slot`, if any.
fn slot_class(config: &MatchConfig, team: u8, slot: usize) -> Option<CharacterClass> {
    let slots = if team == 1 {
        &config.team1
    } else {
        &config.team2
    };
    slots.get(slot).copied().flatten()
}

/// Register a widget whose `--ui-script` id is built from data (a team, a
/// slot, a class). The kit's `driver_id` takes a `&str`, which would mean
/// formatting every id on every frame with the driver off; `format_args!`
/// here costs nothing until [`ui_driver::mark`] finds the driver armed.
fn mark(ui: &egui::Ui, response: &egui::Response, enabled: bool, id: std::fmt::Arguments<'_>) {
    ui_driver::mark(ui, response.rect, enabled, id);
}

/// A team's colour, from the team tokens.
fn team_color(team: u8) -> egui::Color32 {
    if team == 1 {
        data::TEAM_1
    } else {
        data::TEAM_2
    }
}

/// A class's icon for a [`ListRow`]: its art when loaded, else the shared
/// fallback tile edged in the class colour.
fn class_list_icon(class_icons: &ClassIcons, class: CharacterClass) -> ListIcon {
    ListIcon {
        texture: class_icons.textures.get(&class).copied(),
        edge: class_color(class),
        tier: None,
    }
}

/// The character picker: a kit [`Modal`] listing every class as a
/// [`list_row`], the slot's current class selected. Clicking a class assigns
/// it and closes the picker; the `×`, Esc and a click outside close it
/// without a change.
fn render_character_picker_modal(
    ctx: &egui::Context,
    config: &mut MatchConfig,
    picker: &mut CharacterPickerState,
    class_icons: &ClassIcons,
) {
    let title = format!(
        "Select character \u{2014} Team {}, slot {}",
        picker.team,
        picker.slot + 1
    );
    let current_class = slot_class(config, picker.team, picker.slot);
    let max_height = ctx.screen_rect().height() * PICKER_MAX_HEIGHT;

    let response = Modal::new(PICKER_ID, &title).show(ctx, |ui| {
        let mut chosen = None;
        // `min_scrolled_height` too: a modal is laid out from the screen's
        // centre, so the space egui offers below it would otherwise clip the
        // list to half the screen even when every class fits.
        egui::ScrollArea::vertical()
            .max_height(max_height)
            .min_scrolled_height(max_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for &class in CharacterClass::all() {
                    // Same reason as the slot rows: a script has to fill a
                    // slot before it can open View Combatant on it.
                    let row = ListRow::new(class.name())
                        .name_color(class_color(class))
                        .subtitle(class.description())
                        .icon(class_list_icon(class_icons, class))
                        .selected(current_class == Some(class));
                    let response = list_row(ui, row);
                    mark(ui, &response, true, format_args!("class:{class:?}"));
                    if response.clicked() {
                        chosen = Some(class);
                    }
                }
            });
        chosen
    });

    if let Some(class) = response.inner {
        // Through `set_class`, not a direct slot write: changing a slot's
        // class drops its equipment overrides, so the new class arrives
        // wearing its own defaults instead of the previous class's picks
        // silently stripped at resolve.
        config.set_class(picker.team, picker.slot, class);
        picker.active = false;
    } else if response.close {
        picker.active = false;
    }
}

/// Render a team panel (Team 1 or Team 2): a [`panel`] edged in the team's
/// colour, holding the header with its size controls, the three slots, and
/// the kill-target priority.
fn render_team_panel(
    ui: &mut egui::Ui,
    config: &mut MatchConfig,
    team: u8,
    picker: &mut CharacterPickerState,
    class_icons: &ClassIcons,
) -> Option<ConfigureMatchAction> {
    let tint = team_color(team);
    panel(Some(tint))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let mut action = None;

            let team_size = if team == 1 {
                config.team1_size
            } else {
                config.team2_size
            };

            // Header: the team's name, and its size as `−  N  +`.
            panel_header(ui, &format!("Team {team}"), tint, |ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                let mut new_size = team_size;
                let (can_grow, can_shrink) = (team_size < 3, team_size > 1);
                let plus = Button::new("+", Variant::Icon).enabled(can_grow).show(ui);
                mark(ui, &plus, can_grow, format_args!("size:t{team}:+"));
                if plus.clicked() {
                    new_size = team_size + 1;
                }
                ui.label(
                    egui::RichText::new(team_size.to_string())
                        .size(text::H3)
                        .color(color::TEXT),
                );
                let minus = Button::new("\u{2212}", Variant::Icon)
                    .enabled(can_shrink)
                    .show(ui);
                mark(ui, &minus, can_shrink, format_args!("size:t{team}:-"));
                if minus.clicked() {
                    new_size = team_size - 1;
                }
                if new_size != team_size {
                    if team == 1 {
                        config.set_team1_size(new_size);
                    } else {
                        config.set_team2_size(new_size);
                    }
                }
            });

            // Character slots (always three; those past the team size are
            // drawn disabled).
            for slot in 0..3 {
                let character = slot_class(config, team, slot);
                let is_active = slot < team_size;
                if let Some(a) =
                    render_character_slot(ui, team, slot, character, is_active, picker, class_icons)
                {
                    action = Some(a);
                }
            }

            ui.add_space(space::LG);
            render_kill_target(ui, config, team);

            action
        })
        .inner
}

/// The kill-target priority: one row per filled enemy slot, the chosen one
/// selected. Clicking a row selects it; clicking the selected row clears the
/// priority.
fn render_kill_target(ui: &mut egui::Ui, config: &mut MatchConfig, team: u8) {
    section_label(ui, "Kill target priority");

    let (enemy_team, enemy_size) = if team == 1 {
        (2, config.team2_size)
    } else {
        (1, config.team1_size)
    };
    let current = if team == 1 {
        config.team1_kill_target
    } else {
        config.team2_kill_target
    };

    let mut toggled = None;
    for slot in 0..enemy_size {
        let Some(enemy_class) = slot_class(config, enemy_team, slot) else {
            continue;
        };
        let is_selected = current == Some(slot);
        let label = format!("{}. {}", slot + 1, enemy_class.name());
        let response = list_row(ui, ListRow::new(&label).selected(is_selected));
        mark(ui, &response, true, format_args!("target:t{team}s{slot}"));
        if response.clicked() {
            toggled = Some(if is_selected { None } else { Some(slot) });
        }
    }
    if let Some(target) = toggled {
        if team == 1 {
            config.team1_kill_target = target;
        } else {
            config.team2_kill_target = target;
        }
    }

    if current.is_none() {
        ui.add_space(space::XS);
        ui.label(
            egui::RichText::new("No priority \u{2014} team targets freely")
                .size(text::SMALL)
                .color(color::TEXT_DIM),
        );
    }
}

/// Render a single character slot as a [`list_row`].
///
/// - **Active + filled**: the class's icon, name and description; clicking
///   the row opens View Combatant, and the Change button beside it reopens
///   the picker.
/// - **Active + empty**: an invitation to add a character; clicking opens the
///   picker.
/// - **Inactive**: a disabled row, past the team's size.
fn render_character_slot(
    ui: &mut egui::Ui,
    team: u8,
    slot: usize,
    character: Option<CharacterClass>,
    is_active: bool,
    picker: &mut CharacterPickerState,
    class_icons: &ClassIcons,
) -> Option<ConfigureMatchAction> {
    // The route into View Combatant, so a UI script can reach the screens the
    // driver actually exists to check. Inert unless `--ui-script` armed it.
    let mark_slot = |ui: &egui::Ui, response: &egui::Response| {
        mark(ui, response, is_active, format_args!("slot:t{team}s{slot}"));
    };
    let open_picker = |picker: &mut CharacterPickerState| {
        picker.active = true;
        picker.team = team;
        picker.slot = slot;
    };

    match character {
        Some(class) if is_active => {
            let mut action = None;
            ui.horizontal(|ui| {
                let gap = ui.spacing().item_spacing.x;
                let row = ListRow::new(class.name())
                    .name_color(class_color(class))
                    .subtitle(class.description())
                    .icon(class_list_icon(class_icons, class))
                    .meta("View \u{203a}")
                    .width(ui.available_width() - CHANGE_WIDTH - gap);
                let response = list_row(ui, row);
                mark_slot(ui, &response);
                if response.clicked() {
                    action = Some(ConfigureMatchAction::ViewCombatant { class, team, slot });
                }
                // The change-class affordance, so a script can walk several
                // classes through one slot rather than needing three.
                let change = Button::new("Change", Variant::Secondary)
                    .compact()
                    .min_width(CHANGE_WIDTH)
                    .show(ui);
                mark(
                    ui,
                    &change,
                    true,
                    format_args!("slot:t{team}s{slot}:change"),
                );
                if change.clicked() {
                    open_picker(picker);
                }
            });
            action
        }
        _ if is_active => {
            let row = ListRow::new("+  Add character")
                .name_color(color::TEXT_MUTED)
                .subtitle("Choose a class for this slot");
            let response = list_row(ui, row);
            mark_slot(ui, &response);
            if response.clicked() {
                open_picker(picker);
            }
            None
        }
        _ => {
            let row = ListRow::new("\u{2014}")
                .subtitle("Unused at this team size")
                .enabled(false);
            let response = list_row(ui, row);
            mark_slot(ui, &response);
            None
        }
    }
}

/// Render the map selection panel (center column).
///
/// Shows, top to bottom:
/// - A prominent VS badge (aligned with the two team headers — this is the
///   visual divider between the two sides)
/// - Arena caption
/// - A top-down preview of the selected map
/// - Map navigation controls (◀ name ▶)
/// - Map description
fn render_map_panel(
    ui: &mut egui::Ui,
    config: &mut MatchConfig,
    map_geometry: &MapGeometryConfig,
    preview_texture: Option<egui::TextureId>,
) {
    let max_width = ui.available_width();
    ui.vertical_centered(|ui| {
        // VS badge — the emotional beat, sitting between the team headers.
        ui.label(
            egui::RichText::new("VS")
                .size(text::DISPLAY)
                .color(color::ACCENT),
        );
        ui.label(
            egui::RichText::new("ARENA")
                .size(text::SMALL)
                .color(color::TEXT_MUTED),
        );
        ui.add_space(space::MD);

        // Map preview — real top-down geometry for the selected map.
        //
        // Aspect depends on WHICH preview is shown, and getting it wrong distorts
        // the arena:
        // - The live 3D texture has the render target's FIXED aspect, so the rect
        //   must match it exactly (the camera letterboxes the map inside that
        //   frame instead).
        // - The vector fallback draws the map's own outline, so it can use the
        //   map's aspect and fill the pane — this is what keeps Nagrand's
        //   near-square bowl out of the old 76x46 letterbox. Measured off
        //   `outline()`, the same extents `draw_map_preview` fits to, so the
        //   schematic fills its frame exactly.
        let preview_aspect = if preview_texture.is_some() {
            PREVIEW_TEX_ASPECT
        } else {
            let bounds = map_geometry.active_for(config.map).bounds;
            let half = outline_half_extents(&bounds.outline(WALL_ARC_SEGMENTS));
            half.x / half.y
        };
        let preview_width = max_width.min(PREVIEW_MAX_WIDTH);
        let preview_height = preview_width / preview_aspect;

        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(preview_width, preview_height),
            egui::Sense::hover(),
        );
        // Backing well behind the preview.
        ui.painter().rect(
            rect,
            radius::MD,
            color::BG,
            egui::Stroke::new(stroke::HAIRLINE, color::LINE),
            egui::StrokeKind::Inside,
        );
        if let Some(tex) = preview_texture {
            // Live 3D render of the arena (fixed-camera offscreen pass).
            ui.painter().image(
                tex,
                rect.shrink(space::XS),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                color::UNTINTED,
            );
        } else {
            // Fallback: vector top-down schematic (also what the egui harness
            // renders, since Bevy render-to-texture isn't available there).
            draw_map_preview(
                ui.painter(),
                rect.shrink(space::SM),
                config.map,
                map_geometry,
            );
        }

        ui.add_space(space::MD);

        // Map selection controls: a fixed-width row equal to the preview
        // width, with exact widget sizes and zero item spacing, so the row
        // centers under the preview exactly (the enclosing `vertical_centered`
        // centers the whole allocation).
        let label_width = preview_width - size::BUTTON_ICON * 2.0;
        let mut step: i32 = 0;
        ui.allocate_ui_with_layout(
            egui::vec2(preview_width, size::BUTTON_ICON),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if Button::new("\u{25c0}", Variant::Icon)
                    .driver_id("map:prev")
                    .show(ui)
                    .clicked()
                {
                    step = -1;
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(label_width, size::BUTTON_ICON),
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| {
                        ui.label(
                            egui::RichText::new(config.map.name())
                                .size(text::H3)
                                .color(color::TEXT),
                        );
                    },
                );
                if Button::new("\u{25b6}", Variant::Icon)
                    .driver_id("map:next")
                    .show(ui)
                    .clicked()
                {
                    step = 1;
                }
            },
        );

        if step != 0 {
            let maps = match_config::ArenaMap::all();
            let current_idx = maps.iter().position(|m| *m == config.map).unwrap_or(0);
            let new_idx = (current_idx as i32 + step).rem_euclid(maps.len() as i32) as usize;
            config.map = maps[new_idx];
        }

        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new(config.map.description())
                .size(text::SMALL)
                .color(color::TEXT_MUTED),
        );
    });
}

/// Draw a top-down schematic of `map` into `rect`, using the map's real
/// obstacle geometry (cylinders → circles, boxes → rectangles) scaled to fit.
///
/// World coordinates come from the map's own `ArenaBounds::outline`.
/// World x maps to the schematic's horizontal axis and world z to its vertical
/// axis, preserving aspect. Pure painter work so it renders identically in the
/// egui harness and the real client. The floor is a [`color::RAISED`] well
/// edged in [`color::LINE_STRONG`]; obstacles stand out of it in
/// [`color::LINE_STRONG`] edged in [`color::TEXT_MUTED`].
fn draw_map_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    map: match_config::ArenaMap,
    map_geometry: &MapGeometryConfig,
) {
    let active = map_geometry.active_for(map);

    // Fit the selected map's OWN outline — maps no longer share one shape, so a
    // fixed 76x46 frame would crop Nagrand and mis-scale its pillars.
    let outline = active.bounds.outline(WALL_ARC_SEGMENTS);
    let half = outline_half_extents(&outline);
    let scale = (rect.width() / (half.x * 2.0)).min(rect.height() / (half.y * 2.0));
    let center = rect.center();

    // World (x, z) -> screen point.
    let to_screen = |x: f32, z: f32| egui::pos2(center.x + x * scale, center.y + z * scale);

    // Arena floor: the same outline the 3D floor mesh is built from.
    //
    // Filled as a closed path rather than `convex_polygon` because the
    // bowl-with-alcoves outline is CONCAVE, and egui's tessellator emits spurious
    // triangles across a concavity when told the path is convex.
    let floor: Vec<egui::Pos2> = outline.iter().map(|p| to_screen(p.x, p.y)).collect();
    let mut floor_path = egui::epaint::PathShape::closed_line(
        floor,
        egui::Stroke::new(stroke::HAIRLINE, color::LINE_STRONG),
    );
    floor_path.fill = color::RAISED;
    painter.add(floor_path);

    // Obstacles for the selected map.
    let obstacle_fill = color::LINE_STRONG;
    let obstacle_stroke = egui::Stroke::new(stroke::HAIRLINE, color::TEXT_MUTED);
    for volume in &active.volumes {
        match volume {
            ObstacleVolume::Cylinder {
                center_xz, radius, ..
            } => {
                painter.circle(
                    to_screen(center_xz.x, center_xz.y),
                    radius * scale,
                    obstacle_fill,
                    obstacle_stroke,
                );
            }
            ObstacleVolume::Aabb { min, max } => {
                let r = egui::Rect::from_two_pos(to_screen(min.x, min.z), to_screen(max.x, max.z));
                painter.rect(
                    r,
                    radius::SM,
                    obstacle_fill,
                    obstacle_stroke,
                    egui::StrokeKind::Inside,
                );
            }
            ObstacleVolume::Prism {
                center_xz,
                circumradius,
                sides,
                rotation,
                ..
            } => {
                // Same outline helper the collision math and the 3D mesh use, so
                // the schematic shows the real footprint, not a circle standing in
                // for one.
                let pts: Vec<egui::Pos2> =
                    prism_vertices_world(*center_xz, *circumradius, *sides, *rotation)
                        .into_iter()
                        .map(|v| to_screen(v.x, v.y))
                        .collect();
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    obstacle_fill,
                    obstacle_stroke,
                ));
            }
        }
    }
}

// ============================================================================
// Live arena preview (render-to-texture)
//
// A dedicated offscreen camera renders the real arena meshes (floor + walls +
// the selected map's obstacles) to an image from a fixed 3/4 top-down angle;
// the image is registered with egui and drawn into the preview pane. The
// scene lives on its own `RenderLayers` so it never leaks into any other
// camera, and it is torn down when the state exits. This is graphical-only —
// the egui snapshot harness has no Bevy renderer, so it keeps the vector
// schematic fallback in `draw_map_preview`.
// ============================================================================

/// Isolated render layer for the preview scene (camera + light + meshes),
/// so it is invisible to every window-facing camera and vice versa.
const PREVIEW_LAYER: usize = 3;
/// Offscreen render-target size. Its aspect is FIXED (it was picked for the
/// historical 76 × 46 octagon), so `render_map_panel` must give the texture a rect
/// of the same aspect or the image is stretched, and `preview_camera_transform`
/// must frame each map inside this aspect rather than the map's own.
const PREVIEW_TEX_W: u32 = 640;
const PREVIEW_TEX_H: u32 = 388;

/// Aspect (width / height) of the offscreen preview render target.
const PREVIEW_TEX_ASPECT: f32 = PREVIEW_TEX_W as f32 / PREVIEW_TEX_H as f32;

/// Marks every entity in the offscreen preview scene (camera, light, meshes)
/// for teardown on state exit.
#[derive(Component)]
pub struct PreviewSceneEntity;

/// Marks just the arena-environment meshes, which are rebuilt when the
/// selected map changes (the camera and light persist across map switches).
#[derive(Component)]
pub struct PreviewEnvEntity;

/// Marks the offscreen preview camera so it can be REFRAMED when the selected map
/// changes. It persists across map switches, so without this it would keep the
/// framing of whichever map was selected first.
#[derive(Component)]
pub struct PreviewCamera;

/// Handle + egui texture id for the live arena preview render target, plus the
/// map currently rendered into it (so the UI only shows the texture once it
/// matches the selection, and the env is rebuilt when the selection changes).
#[derive(Resource)]
pub struct MapPreview {
    image: Handle<Image>,
    texture_id: egui::TextureId,
    rendered_map: ArenaMap,
}

/// 3/4 top-down camera framing the whole of `bounds`, walls included.
///
/// The framing was hand-tuned for the historical 76 × 46 octagon; arena shape is
/// per-map now, so the distance SCALES with the map's extents. Left fixed, the old
/// `(0, 52, 40)` showed roughly the middle 54yd of whatever was loaded — which
/// cropped everything outside Nagrand's centre, pillars included.
fn preview_camera_transform(bounds: &ArenaBounds) -> Transform {
    /// Half-extents of the arena the framing below was tuned against, walls
    /// included (`ArenaBounds::default().outline()` reaches 38 × 23).
    const REFERENCE_HALF: Vec2 = Vec2::new(38.0, 23.0);
    let half = bounds.half_extents() + Vec2::splat(WALL_OFFSET);
    // Scale by whichever axis is the binding constraint, so the arena fits the
    // render target's fixed aspect on both.
    let scale = (half.x / REFERENCE_HALF.x)
        .max(half.y / REFERENCE_HALF.y)
        .max(1.0);
    Transform::from_xyz(0.0, 52.0 * scale, 40.0 * scale).looking_at(Vec3::ZERO, Vec3::Y)
}

/// Spawn the arena environment for `map` onto the preview render layer, tagged
/// for rebuild. Reuses the same mesh builder the real match uses.
fn spawn_preview_environment(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    map: ArenaMap,
    map_geometry: &MapGeometryConfig,
) {
    let active = map_geometry.active_for(map);
    for entity in spawn_arena_environment(
        commands,
        meshes,
        materials,
        images,
        &active.bounds,
        &active.volumes,
    ) {
        commands.entity(entity).insert((
            RenderLayers::layer(PREVIEW_LAYER),
            PreviewEnvEntity,
            PreviewSceneEntity,
        ));
    }
}

/// `OnEnter(ConfigureMatch)`: build the offscreen render target, camera, light,
/// and arena environment, then register the texture with egui.
pub fn setup_map_preview(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<MatchConfig>,
    map_geometry: Res<MapGeometryConfig>,
) {
    // Offscreen render-target image.
    let size = Extent3d {
        width: PREVIEW_TEX_W,
        height: PREVIEW_TEX_H,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image_handle = images.add(image);
    let texture_id = contexts.add_image(image_handle.clone());

    // Preview camera: renders only PREVIEW_LAYER into the image. `order` is
    // negative so it resolves before any window camera; its target is the
    // image, so it never composites to the window.
    commands.spawn((
        Camera3d::default(),
        Camera {
            target: RenderTarget::Image(image_handle.clone().into()),
            hdr: true,
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.05, 0.06, 0.09)),
            ..default()
        },
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        preview_camera_transform(&map_geometry.active_for(config.map).bounds),
        RenderLayers::layer(PREVIEW_LAYER),
        PreviewCamera,
        PreviewSceneEntity,
    ));

    // Warm key light on the preview layer (the match sun is on layer 0).
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.95, 0.85),
            illuminance: 12000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(25.0, 45.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(PREVIEW_LAYER),
        PreviewSceneEntity,
    ));

    // Ambient fill so the shadowed sides aren't crushed (removed on exit).
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.9, 0.85, 0.7),
        brightness: 300.0,
        affects_lightmapped_meshes: true,
    });

    spawn_preview_environment(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        config.map,
        &map_geometry,
    );

    commands.insert_resource(MapPreview {
        image: image_handle,
        texture_id,
        rendered_map: config.map,
    });
}

/// `Update` while in ConfigureMatch: when the selected map changes, rebuild the
/// preview environment meshes for the new map.
pub fn update_map_preview(
    mut commands: Commands,
    config: Res<MatchConfig>,
    map_geometry: Res<MapGeometryConfig>,
    map_preview: Option<ResMut<MapPreview>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    env_entities: Query<Entity, With<PreviewEnvEntity>>,
    mut camera: Query<&mut Transform, With<PreviewCamera>>,
) {
    let Some(mut preview) = map_preview else {
        return;
    };
    if preview.rendered_map == config.map {
        return;
    }

    // Reframe: maps no longer share one shape, so the camera distance has to
    // follow the newly selected map's extents.
    let bounds = map_geometry.active_for(config.map).bounds;
    for mut transform in &mut camera {
        *transform = preview_camera_transform(&bounds);
    }

    for entity in &env_entities {
        commands.entity(entity).despawn();
    }
    spawn_preview_environment(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        config.map,
        &map_geometry,
    );
    preview.rendered_map = config.map;
}

/// `OnExit(ConfigureMatch)`: tear the preview scene down and drop its resources.
pub fn cleanup_map_preview(
    mut commands: Commands,
    mut contexts: EguiContexts,
    map_preview: Option<Res<MapPreview>>,
    scene_entities: Query<Entity, With<PreviewSceneEntity>>,
) {
    for entity in &scene_entities {
        commands.entity(entity).despawn();
    }
    if let Some(preview) = map_preview {
        contexts.remove_image(&preview.image);
    }
    commands.remove_resource::<MapPreview>();
    commands.remove_resource::<AmbientLight>();
}
