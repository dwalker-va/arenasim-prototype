//! Broadcast-style fixed team frames (spectator UI).
//!
//! Team 1 frames are pinned to the left screen edge, Team 2 to the right,
//! WoW-arena-tournament style: class icon, health/resource bars, and
//! buff/debuff icon rows with timers. This is the stable home for
//! per-combatant information; the overhead nameplate keeps what is
//! *spatially* meaningful — a thin HP sliver, CC status labels, and the
//! cast/channel bars (with no casting animations on the character models,
//! the overhead bar is the tell for who is casting at whom) — leaving the
//! head-level space free for effects like the Berserker Rage mask.
//!
//! Split like the Results screen: [`draw_team_frames`] is a pure egui
//! function (no Bevy ECS) so `tests/team_frames_snapshot.rs` can render it
//! offscreen via `egui_kittest` for fast visual iteration; the thin Bevy
//! wrapper [`render_team_frames`] collects ECS state into plain data each
//! frame and calls it.
//!
//! The frames also host the in-match kill-target call. Clicking a combatant's
//! frame sets the *opposing* team's call on that combatant — Team 1's call
//! points at an enemy, so it marks a frame in the Team 2 column. Following the
//! Results-screen convention, the pure draw function only reports which frame
//! was clicked; [`render_team_frames`] applies it to [`MatchConfig`], and
//! `acquire_targets` re-reads that config every tick, so no second
//! target-forcing path exists.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::states::configure_match_ui::ClassIcons;
use crate::states::match_config::{CharacterClass, MatchConfig};
use crate::states::results_ui::class_color32;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;

use super::{get_aura_icon_key, is_buff_aura};

// ==============================================================================
// Plain data (what the pure draw function consumes)
// ==============================================================================

/// One buff/debuff icon in a frame's aura row.
pub struct FrameAura {
    /// Key into `SpellIcons.textures` (ability name or generic `aura_*` key).
    pub icon_key: String,
    /// Seconds remaining.
    pub remaining: f32,
    /// Gold border (buff) vs red border (debuff).
    pub is_buff: bool,
    /// Hard CC gets the bright border treatment.
    pub is_hard_cc: bool,
}

/// Everything the frame shows for one combatant.
pub struct CombatantFrame {
    pub class: CharacterClass,
    /// Pets render as compact sub-frames with this label instead of the class name.
    pub pet_label: Option<String>,
    pub alive: bool,
    pub stealthed: bool,
    pub current_health: f32,
    pub max_health: f32,
    /// Remaining absorb shielding (drawn as a white segment on the HP bar).
    pub absorb: f32,
    pub current_resource: f32,
    pub max_resource: f32,
    pub resource_type: ResourceType,
    pub buffs: Vec<FrameAura>,
    pub debuffs: Vec<FrameAura>,
}

/// Both columns.
///
/// The two `*_called_slot` fields are already flipped from the config's
/// point of view: they name the primary slot marked *in that column*, which
/// is the call belonging to the other team. The wrapper does the flip once so
/// the draw function never has to reason about whose call it is showing.
#[derive(Default)]
pub struct TeamFramesData {
    pub team1: Vec<CombatantFrame>,
    pub team2: Vec<CombatantFrame>,
    /// Primary slot marked in the Team 1 column — i.e. Team 2's kill-target call.
    pub team1_called_slot: Option<usize>,
    /// Primary slot marked in the Team 2 column — i.e. Team 1's kill-target call.
    pub team2_called_slot: Option<usize>,
    /// Whether the call markers and their click affordance are shown at all.
    pub show_calls: bool,
}

/// A frame the operator clicked, reported back by [`draw_team_frames`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallClick {
    /// The column that was clicked (1 or 2). The call belongs to the *other* team.
    pub clicked_team: u8,
    /// Index into that column's primary combatants, pets excluded — the same
    /// indexing `MatchConfig::teamN_kill_target` uses.
    pub slot: usize,
}

/// The `(team1_called_slot, team2_called_slot)` a config implies.
///
/// A team's call points at an enemy, so the flip is total: Team 1's call is
/// what marks the Team 2 column. Paired with [`apply_call_click`], which
/// flips the same way, so a click and the mark it produces land in the same
/// column.
pub fn called_slots(config: &MatchConfig) -> (Option<usize>, Option<usize>) {
    (config.team2_kill_target, config.team1_kill_target)
}

/// Apply a frame click to the kill-target config.
///
/// A team's call points at an enemy, so a click in the Team 2 column sets
/// `team1_kill_target`. Clicking the already-called combatant clears the call,
/// matching the pre-match Kill Target Priority control's toggle-off behavior.
pub fn apply_call_click(config: &mut MatchConfig, click: CallClick) {
    let slot = Some(click.slot);
    let call = if click.clicked_team == 1 {
        &mut config.team2_kill_target
    } else {
        &mut config.team1_kill_target
    };
    *call = if *call == slot { None } else { slot };
}

// ==============================================================================
// Layout constants
// ==============================================================================

const FRAME_W: f32 = 210.0;
const PAD: f32 = 8.0;
const EDGE_MARGIN: f32 = 12.0;
const FRAME_SPACING: f32 = 10.0;
const HEADER_H: f32 = 26.0;
const HP_H: f32 = 16.0;
const RESOURCE_H: f32 = 8.0;
const AURA_ICON: f32 = 20.0;
const AURA_GAP: f32 = 2.0;
const ROW_GAP: f32 = 4.0;
/// Max icons per aura row before truncation ("+N" overflow text).
const MAX_AURA_ICONS: usize = 8;

const PET_HEADER_H: f32 = 16.0;
const PET_HP_H: f32 = 10.0;
/// Height of the "TEAM N" caption above a column.
const COLUMN_LABEL_H: f32 = 16.0;

/// The call marker is deliberately hueless. Every hue in this UI is already
/// spoken for by gameplay signal — class colors, gold buff borders, red
/// debuff/CC borders, the green/yellow/red HP thresholds — so the call reads
/// by *shape* (a reticle) and *value* (a brighter border) instead of claiming
/// another color.
const CALL_MARK: egui::Color32 = egui::Color32::from_rgb(236, 238, 248);
/// Border shown while the pointer is over a clickable frame.
const CALL_HOVER: egui::Color32 = egui::Color32::from_rgb(150, 154, 172);
/// Border shown on every clickable frame while the affordance is on.
///
/// This is what makes the toggle legible. Brighter than the inert border so
/// turning calls on visibly arms the column, dimmer than [`CALL_HOVER`] so the
/// pointer still reads, and far below [`CALL_MARK`] so the actual call stays
/// the loudest thing on screen. Still hueless, per the note above.
const CALL_CALLABLE: egui::Color32 = egui::Color32::from_rgb(96, 100, 118);
const RETICLE_R: f32 = 5.0;
/// How far the reticle's cross ticks reach past its ring.
const RETICLE_TICK: f32 = 2.5;

fn frame_height(frame: &CombatantFrame) -> f32 {
    if frame.pet_label.is_some() {
        return PAD + PET_HEADER_H + ROW_GAP + PET_HP_H + PAD;
    }
    let mut h = PAD + HEADER_H + ROW_GAP + HP_H + 3.0 + RESOURCE_H;
    if !frame.buffs.is_empty() {
        h += ROW_GAP + AURA_ICON;
    }
    if !frame.debuffs.is_empty() {
        h += ROW_GAP + AURA_ICON;
    }
    h + PAD
}

// ==============================================================================
// Pure draw function (kittest-renderable)
// ==============================================================================

/// Call slot for each frame in a column, top to bottom.
///
/// `None` for pets AND for the dead, because both are absent from the list the
/// simulation indexes. `acquire_targets` builds `enemy_primary` by skipping
/// `!c.is_alive()` first and pets second, then resolves `teamN_kill_target`
/// as a position in what remains — so that list COMPACTS as combatants die.
///
/// Numbering the dead here would desynchronise the two index spaces the moment
/// anyone fell: in a 3v3 whose slot 0 has died, a click on the frame this
/// function called 2 would write a call the simulation resolves against a
/// two-element list, silently falling back to nearest-enemy while the marker
/// rendered on a different frame. Skipping the dead keeps one shared meaning
/// for a slot number, which is the whole contract between the click and the AI.
pub fn call_slots(frames: &[CombatantFrame]) -> Vec<Option<usize>> {
    let mut next = 0;
    frames
        .iter()
        .map(|frame| {
            if frame.pet_label.is_some() || !frame.alive {
                return None;
            }
            let slot = next;
            next += 1;
            Some(slot)
        })
        .collect()
}

/// Screen rects of one column's frames, top to bottom — the exact geometry
/// [`draw_team_frames`] paints and hit-tests against. Public so tests can
/// click a frame without restating the layout math.
pub fn column_frame_rects(
    frames: &[CombatantFrame],
    team: u8,
    screen: egui::Rect,
) -> Vec<egui::Rect> {
    if frames.is_empty() {
        return Vec::new();
    }
    let x = if team == 1 {
        screen.left() + EDGE_MARGIN
    } else {
        screen.right() - EDGE_MARGIN - FRAME_W
    };
    let total_h = COLUMN_LABEL_H
        + frames.iter().map(frame_height).sum::<f32>()
        + (frames.len() as f32 - 1.0) * FRAME_SPACING;
    let mut y = (screen.center().y - total_h / 2.0).max(60.0) + COLUMN_LABEL_H + 2.0;

    frames
        .iter()
        .map(|frame| {
            let h = frame_height(frame);
            let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(FRAME_W, h));
            y += h + FRAME_SPACING;
            rect
        })
        .collect()
}

/// Draw both team-frame columns. Pure egui — no ECS access.
///
/// Returns the frame the operator clicked this frame, if any. Clicks are only
/// sensed when `data.show_calls` is set: with the affordance off the frames
/// allocate no interactable rects at all, so nothing can be called by accident
/// and the pointer passes through to the camera as before.
pub fn draw_team_frames(
    ctx: &egui::Context,
    data: &TeamFramesData,
    class_icons: &ClassIcons,
    spell_icons: &SpellIcons,
) -> Option<CallClick> {
    // Anchor to available_rect (not screen_rect): egui panels shown earlier
    // this frame (e.g. the combat log SidePanel) shrink the available rect,
    // so open diagnostics push the frames aside instead of being painted
    // over. Requires render_combat_panel to run before this system.
    let avail = ctx.available_rect();

    let mut clicked = None;
    egui::Area::new(egui::Id::new("team_frames"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            // Both columns always draw; `or` picks whichever reported a click.
            let c1 = draw_column(
                ui,
                &data.team1,
                1,
                avail,
                CallAffordance::new(data.show_calls, data.team1_called_slot),
                class_icons,
                spell_icons,
            );
            let c2 = draw_column(
                ui,
                &data.team2,
                2,
                avail,
                CallAffordance::new(data.show_calls, data.team2_called_slot),
                class_icons,
                spell_icons,
            );
            clicked = c1.or(c2);
        });
    clicked
}

/// Whether a column shows and accepts kill-call interaction, and which slot
/// the opposing team currently calls.
///
/// The two facts are not independent — with the affordance off there is no
/// marker to draw and no rect to sense — so folding them into one value makes
/// "hidden, but with a called slot" unrepresentable instead of an invalid
/// combination every caller has to remember not to construct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallAffordance {
    /// Toggled off: no marker drawn, no click sensed.
    Hidden,
    /// Toggled on, carrying the slot the opposing team calls, if any.
    Active(Option<usize>),
}

impl CallAffordance {
    fn new(show_calls: bool, called_slot: Option<usize>) -> Self {
        if show_calls {
            CallAffordance::Active(called_slot)
        } else {
            CallAffordance::Hidden
        }
    }

    /// The called slot, or `None` when hidden — so a marker test never has to
    /// check visibility separately.
    fn called(self) -> Option<usize> {
        match self {
            CallAffordance::Active(slot) => slot,
            CallAffordance::Hidden => None,
        }
    }

    fn interactive(self) -> bool {
        matches!(self, CallAffordance::Active(_))
    }
}

fn draw_column(
    ui: &egui::Ui,
    frames: &[CombatantFrame],
    team: u8,
    screen: egui::Rect,
    affordance: CallAffordance,
    class_icons: &ClassIcons,
    spell_icons: &SpellIcons,
) -> Option<CallClick> {
    let rects = column_frame_rects(frames, team, screen);
    if rects.is_empty() {
        return None;
    }

    let painter = ui.painter();
    painter.text(
        egui::pos2(
            rects[0].center().x,
            rects[0].min.y - 2.0 - COLUMN_LABEL_H / 2.0,
        ),
        egui::Align2::CENTER_CENTER,
        if team == 1 { "TEAM 1" } else { "TEAM 2" },
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgb(160, 160, 175),
    );

    let mut clicked = None;
    for ((frame, rect), slot) in frames.iter().zip(&rects).zip(call_slots(frames)) {
        // Only primary combatants are clickable; a pet sub-frame and a corpse
        // both come back with no slot and stay inert.
        let mut call_state = FrameCallState::Inert;
        if let (true, Some(slot)) = (affordance.interactive(), slot) {
            let response = ui.interact(
                *rect,
                egui::Id::new(("team_frame_call", team, slot)),
                egui::Sense::click(),
            );
            call_state = if response.hovered() {
                FrameCallState::Hovered
            } else {
                FrameCallState::Callable
            };
            if response.clicked() {
                clicked = Some(CallClick {
                    clicked_team: team,
                    slot,
                });
            }
        }
        // Being called outranks hover and callable. `called()` is `None` when
        // the affordance is hidden, so this covers visibility too.
        if slot.is_some() && slot == affordance.called() {
            call_state = FrameCallState::Called;
        }
        draw_frame(painter, *rect, frame, call_state, class_icons, spell_icons);
    }
    clicked
}

/// How a frame participates in the call affordance, as one value rather than a
/// pile of booleans.
///
/// The states are ordered by visual weight and are mutually exclusive, which is
/// what an enum says and three bools do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameCallState {
    /// Affordance off, or a frame that can never be called (a pet, a corpse).
    Inert,
    /// Callable right now, pointer elsewhere.
    Callable,
    /// Callable and under the pointer.
    Hovered,
    /// The opposing team's current call.
    Called,
}

fn draw_frame(
    painter: &egui::Painter,
    rect: egui::Rect,
    frame: &CombatantFrame,
    call_state: FrameCallState,
    class_icons: &ClassIcons,
    spell_icons: &SpellIcons,
) {
    let dim = if frame.alive { 1.0 } else { 0.45 };
    let dimmed = |c: egui::Color32| {
        egui::Color32::from_rgb(
            (c.r() as f32 * dim) as u8,
            (c.g() as f32 * dim) as u8,
            (c.b() as f32 * dim) as u8,
        )
    };

    // Frame background + class-colored accent stripe on the arena-facing edge.
    // The border doubles as the call marker and as the affordance's own tell.
    //
    // `Callable` carries a visible border on purpose. Without it, switching the
    // affordance on changed nothing you could see until you happened to hover a
    // frame — the toggle looked broken, because a control mode that renders no
    // evidence of being on is indistinguishable from one that did not fire.
    painter.rect_filled(rect, 4.0, egui::Color32::from_rgba_unmultiplied(13, 13, 20, 235));
    let border = match call_state {
        FrameCallState::Called => egui::Stroke::new(2.0, CALL_MARK),
        FrameCallState::Hovered => egui::Stroke::new(1.0, CALL_HOVER),
        FrameCallState::Callable => egui::Stroke::new(1.0, CALL_CALLABLE),
        FrameCallState::Inert => egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 45, 60)),
    };
    painter.rect_stroke(rect, 4.0, border, egui::StrokeKind::Outside);
    let stripe = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
    painter.rect_filled(stripe, 2.0, dimmed(class_color32(frame.class)));

    let inner_x = rect.min.x + PAD + 3.0;
    let inner_w = FRAME_W - 2.0 * PAD - 3.0;
    let mut y = rect.min.y + PAD;

    // Compact pet frame: small label + HP sliver only.
    if let Some(pet_label) = &frame.pet_label {
        painter.text(
            egui::pos2(inner_x, y + PET_HEADER_H / 2.0),
            egui::Align2::LEFT_CENTER,
            pet_label,
            egui::FontId::proportional(11.0),
            dimmed(egui::Color32::from_rgb(200, 200, 210)),
        );
        y += PET_HEADER_H + ROW_GAP;
        draw_hp_bar(
            painter,
            egui::Rect::from_min_size(egui::pos2(inner_x, y), egui::vec2(inner_w, PET_HP_H)),
            frame,
            false,
            dim,
        );
        return;
    }

    // Header: class icon + name + DEAD/STEALTH tag.
    let icon_rect = egui::Rect::from_min_size(egui::pos2(inner_x, y), egui::vec2(HEADER_H - 2.0, HEADER_H - 2.0));
    if let Some(texture_id) = class_icons.textures.get(&frame.class) {
        painter.image(
            *texture_id,
            icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::from_gray((255.0 * dim) as u8),
        );
    } else {
        // Fallback: class-color square with the class initial.
        painter.rect_filled(icon_rect, 3.0, dimmed(class_color32(frame.class)));
        painter.text(
            icon_rect.center(),
            egui::Align2::CENTER_CENTER,
            &frame.class.name()[0..1],
            egui::FontId::proportional(14.0),
            egui::Color32::BLACK,
        );
    }
    painter.text(
        egui::pos2(icon_rect.max.x + 6.0, y + HEADER_H / 2.0 - 1.0),
        egui::Align2::LEFT_CENTER,
        frame.class.name(),
        egui::FontId::proportional(14.0),
        dimmed(egui::Color32::WHITE),
    );
    // Call reticle at the header's trailing edge; the DEAD/STEALTH tag steps
    // aside for it, since the call outlives the combatant it points at.
    let reticle_w = 2.0 * (RETICLE_R + RETICLE_TICK);
    let mut tag_right = inner_x + inner_w;
    if call_state == FrameCallState::Called {
        draw_call_reticle(
            painter,
            egui::pos2(inner_x + inner_w - reticle_w / 2.0, y + HEADER_H / 2.0 - 1.0),
        );
        tag_right -= reticle_w + 4.0;
    }
    let tag = if !frame.alive {
        Some(("DEAD", egui::Color32::from_rgb(220, 70, 70)))
    } else if frame.stealthed {
        Some(("STEALTH", egui::Color32::from_rgb(170, 170, 190)))
    } else {
        None
    };
    if let Some((text, color)) = tag {
        painter.text(
            egui::pos2(tag_right, y + HEADER_H / 2.0 - 1.0),
            egui::Align2::RIGHT_CENTER,
            text,
            egui::FontId::proportional(11.0),
            color,
        );
    }
    y += HEADER_H + ROW_GAP;

    // HP bar (with hp/max text + absorb segment).
    draw_hp_bar(
        painter,
        egui::Rect::from_min_size(egui::pos2(inner_x, y), egui::vec2(inner_w, HP_H)),
        frame,
        true,
        dim,
    );
    y += HP_H + 3.0;

    // Resource bar.
    let res_rect = egui::Rect::from_min_size(egui::pos2(inner_x, y), egui::vec2(inner_w, RESOURCE_H));
    let (res_color, _) = resource_colors(frame.resource_type);
    let res_pct = if frame.max_resource > 0.0 {
        (frame.current_resource / frame.max_resource).clamp(0.0, 1.0)
    } else {
        0.0
    };
    painter.rect_filled(res_rect, 2.0, egui::Color32::from_rgb(20, 20, 30));
    painter.rect_filled(
        egui::Rect::from_min_size(res_rect.min, egui::vec2(res_rect.width() * res_pct, RESOURCE_H)),
        2.0,
        dimmed(res_color),
    );
    y += RESOURCE_H;

    // Buff row (gold borders), then debuff row (red borders).
    if !frame.buffs.is_empty() {
        y += ROW_GAP;
        draw_aura_row(painter, inner_x, y, &frame.buffs, spell_icons);
        y += AURA_ICON;
    }
    if !frame.debuffs.is_empty() {
        y += ROW_GAP;
        draw_aura_row(painter, inner_x, y, &frame.debuffs, spell_icons);
    }
}

/// A crosshair reticle: ring plus four outward ticks, drawn geometrically so
/// it needs no glyph the bundled fonts might not carry.
fn draw_call_reticle(painter: &egui::Painter, center: egui::Pos2) {
    let stroke = egui::Stroke::new(1.5, CALL_MARK);
    painter.circle_stroke(center, RETICLE_R, stroke);
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        painter.line_segment(
            [
                center + egui::vec2(dx, dy) * (RETICLE_R * 0.5),
                center + egui::vec2(dx, dy) * (RETICLE_R + RETICLE_TICK),
            ],
            stroke,
        );
    }
}

fn draw_hp_bar(
    painter: &egui::Painter,
    rect: egui::Rect,
    frame: &CombatantFrame,
    with_text: bool,
    dim: f32,
) {
    let hp_pct = if frame.max_health > 0.0 {
        (frame.current_health / frame.max_health).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Same green/yellow/red thresholds as the overhead sliver.
    let fill = if !frame.alive {
        egui::Color32::from_rgb(60, 60, 60)
    } else if hp_pct > 0.5 {
        egui::Color32::from_rgb(0, 200, 0)
    } else if hp_pct > 0.25 {
        egui::Color32::from_rgb(255, 200, 0)
    } else {
        egui::Color32::from_rgb(200, 0, 0)
    };

    painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(28, 28, 34));
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * hp_pct, rect.height())),
        2.0,
        fill,
    );

    // Absorb: translucent white segment appended after the health fill.
    if frame.absorb > 0.0 && frame.max_health > 0.0 && frame.alive {
        let absorb_w = (frame.absorb / frame.max_health).min(1.0 - hp_pct) * rect.width();
        if absorb_w > 0.5 {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(rect.min.x + rect.width() * hp_pct, rect.min.y),
                    egui::vec2(absorb_w, rect.height()),
                ),
                0.0,
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, (110.0 * dim) as u8),
            );
        }
    }

    if with_text {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{:.0} / {:.0}", frame.current_health.max(0.0), frame.max_health),
            egui::FontId::proportional(10.0),
            egui::Color32::WHITE,
        );
    }
}

fn draw_aura_row(
    painter: &egui::Painter,
    x: f32,
    y: f32,
    auras: &[FrameAura],
    spell_icons: &SpellIcons,
) {
    let shown = auras.len().min(MAX_AURA_ICONS);
    for (i, aura) in auras.iter().take(shown).enumerate() {
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(x + i as f32 * (AURA_ICON + AURA_GAP), y),
            egui::vec2(AURA_ICON, AURA_ICON),
        );

        let border = if aura.is_hard_cc {
            egui::Color32::from_rgb(255, 60, 60)
        } else if aura.is_buff {
            egui::Color32::from_rgb(255, 215, 0)
        } else {
            egui::Color32::from_rgb(200, 50, 50)
        };

        painter.rect_filled(icon_rect, 2.0, egui::Color32::from_rgb(20, 20, 20));
        if let Some(texture_id) = spell_icons.textures.get(&aura.icon_key) {
            painter.image(
                *texture_id,
                icon_rect.shrink(1.5),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            // Fallback square tinted by buff/debuff so kittest renders read.
            let fallback = if aura.is_buff {
                egui::Color32::from_rgb(140, 120, 30)
            } else {
                egui::Color32::from_rgb(120, 40, 40)
            };
            painter.rect_filled(icon_rect.shrink(1.5), 1.0, fallback);
        }
        painter.rect_stroke(icon_rect, 2.0, egui::Stroke::new(1.5, border), egui::StrokeKind::Outside);

        // Remaining-time text, centered on the icon (WoW cooldown-text style)
        // with a shadow for legibility over icon art.
        let text = if aura.remaining >= 10.0 {
            format!("{:.0}", aura.remaining)
        } else {
            format!("{:.1}", aura.remaining)
        };
        let text_pos = icon_rect.center();
        painter.text(
            text_pos + egui::vec2(1.0, 1.0),
            egui::Align2::CENTER_CENTER,
            &text,
            egui::FontId::proportional(9.0),
            egui::Color32::BLACK,
        );
        painter.text(
            text_pos,
            egui::Align2::CENTER_CENTER,
            &text,
            egui::FontId::proportional(9.0),
            egui::Color32::WHITE,
        );
    }

    if auras.len() > shown {
        painter.text(
            egui::pos2(x + shown as f32 * (AURA_ICON + AURA_GAP) + 2.0, y + AURA_ICON / 2.0),
            egui::Align2::LEFT_CENTER,
            format!("+{}", auras.len() - shown),
            egui::FontId::proportional(10.0),
            egui::Color32::from_rgb(200, 200, 210),
        );
    }
}

fn resource_colors(resource_type: ResourceType) -> (egui::Color32, egui::Color32) {
    match resource_type {
        ResourceType::Mana => (
            egui::Color32::from_rgb(80, 150, 255),
            egui::Color32::from_rgb(150, 150, 200),
        ),
        ResourceType::Energy => (
            egui::Color32::from_rgb(255, 255, 100),
            egui::Color32::from_rgb(200, 200, 150),
        ),
        ResourceType::Rage => (
            egui::Color32::from_rgb(255, 80, 80),
            egui::Color32::from_rgb(200, 150, 150),
        ),
    }
}

// ==============================================================================
// Bevy wrapper (collects ECS state into plain data, then draws)
// ==============================================================================

/// Collect per-combatant state into [`TeamFramesData`], draw the frames, and
/// apply any call the operator clicked.
pub fn render_team_frames(
    mut contexts: EguiContexts,
    abilities: Res<AbilityDefinitions>,
    class_icons: Res<ClassIcons>,
    spell_icons: Res<SpellIcons>,
    combatants: Query<(Entity, &Combatant, Option<&ActiveAuras>)>,
    pet_query: Query<&Pet>,
    display_settings: Res<DisplaySettings>,
    mut config: ResMut<MatchConfig>,
) {
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    // Sort by (team, slot) so frame order is stable across frames and matches
    // config slot order (pets have slot >= 100 and sink to the column bottom).
    let mut rows: Vec<_> = combatants.iter().collect();
    rows.sort_by_key(|(_, c, _)| (c.team, c.slot));

    // `call_slots` derives each frame's slot index from the same pet-filtered
    // ordering `acquire_targets` uses, so the marks line up with the config.
    let (team1_called_slot, team2_called_slot) = called_slots(&config);
    let mut data = TeamFramesData {
        team1_called_slot,
        team2_called_slot,
        show_calls: display_settings.show_call_display,
        ..default()
    };
    for (entity, combatant, auras) in rows {
        let pet_label = pet_query
            .get(entity)
            .ok()
            .map(|pet| format!("{} (pet)", pet.pet_type.name()));

        let mut buffs = Vec::new();
        let mut debuffs = Vec::new();
        let mut absorb = 0.0;
        if let Some(auras) = auras {
            for aura in &auras.auras {
                if aura.effect_type == AuraType::Absorb {
                    absorb += aura.magnitude;
                }
                // The aura-icon display toggle (hotkey + checkbox) now gates the
                // frame rows, since the overhead rows it used to control moved here.
                if !display_settings.show_aura_icons {
                    continue;
                }
                let frame_aura = FrameAura {
                    icon_key: get_aura_icon_key(aura, &abilities),
                    remaining: aura.duration,
                    is_buff: is_buff_aura(&aura.effect_type),
                    is_hard_cc: super::hud::is_hard_cc_aura(&aura.effect_type),
                };
                if frame_aura.is_buff {
                    buffs.push(frame_aura);
                } else {
                    debuffs.push(frame_aura);
                }
            }
        }

        let frame = CombatantFrame {
            class: combatant.class,
            pet_label,
            alive: combatant.is_alive(),
            stealthed: combatant.stealthed,
            current_health: combatant.current_health,
            max_health: combatant.max_health,
            absorb,
            current_resource: combatant.current_mana,
            max_resource: combatant.max_mana,
            resource_type: combatant.resource_type,
            buffs,
            debuffs,
        };

        if combatant.team == 1 {
            data.team1.push(frame);
        } else {
            data.team2.push(frame);
        }
    }

    if let Some(click) = draw_team_frames(ctx, &data, &class_icons, &spell_icons) {
        // KTD1: writing the config is the whole mechanism — `acquire_targets`
        // re-reads it next tick and re-forces the team onto the new target.
        apply_call_click(&mut config, click);
    }
}

// ==============================================================================
// Tests
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal primary frame; only the fields the call logic reads matter.
    fn primary(class: CharacterClass) -> CombatantFrame {
        CombatantFrame {
            class,
            pet_label: None,
            alive: true,
            stealthed: false,
            current_health: 100.0,
            max_health: 100.0,
            absorb: 0.0,
            current_resource: 0.0,
            max_resource: 100.0,
            resource_type: ResourceType::Mana,
            buffs: Vec::new(),
            debuffs: Vec::new(),
        }
    }

    fn pet(class: CharacterClass) -> CombatantFrame {
        CombatantFrame {
            pet_label: Some("Spider (pet)".to_string()),
            ..primary(class)
        }
    }

    #[test]
    fn call_slots_number_primaries_in_column_order() {
        let frames = vec![
            primary(CharacterClass::Hunter),
            primary(CharacterClass::Priest),
        ];
        assert_eq!(call_slots(&frames), vec![Some(0), Some(1)]);
    }

    #[test]
    fn call_slots_skip_pets() {
        // Pets are not addressable by a call, so they get no slot and no click.
        let frames = vec![primary(CharacterClass::Hunter), pet(CharacterClass::Hunter)];
        assert_eq!(call_slots(&frames), vec![Some(0), None]);
    }

    #[test]
    fn pets_do_not_shift_primary_slot_indices() {
        // Even interleaved (the wrapper sorts pets last, but the mapping must
        // not depend on that), a pet consumes no index.
        let frames = vec![
            primary(CharacterClass::Hunter),
            pet(CharacterClass::Hunter),
            primary(CharacterClass::Priest),
        ];
        assert_eq!(call_slots(&frames), vec![Some(0), None, Some(1)]);
    }

    #[test]
    fn clicking_a_column_sets_the_opposing_teams_call() {
        let mut config = MatchConfig::default();
        config.team1_kill_target = None;
        config.team2_kill_target = None;

        // Clicking a Team 2 frame is Team 1 calling that target.
        apply_call_click(&mut config, CallClick { clicked_team: 2, slot: 1 });
        assert_eq!(config.team1_kill_target, Some(1));
        assert_eq!(config.team2_kill_target, None, "the other call is untouched");

        // ...and the mirror.
        apply_call_click(&mut config, CallClick { clicked_team: 1, slot: 0 });
        assert_eq!(config.team2_kill_target, Some(0));
        assert_eq!(config.team1_kill_target, Some(1));
    }

    #[test]
    fn a_click_marks_the_column_it_was_made_in() {
        // The click flip and the marker flip must agree, or a call would light
        // up the wrong column.
        let mut config = MatchConfig::default();
        config.team1_kill_target = None;
        config.team2_kill_target = None;

        apply_call_click(&mut config, CallClick { clicked_team: 2, slot: 1 });
        assert_eq!(called_slots(&config), (None, Some(1)));

        apply_call_click(&mut config, CallClick { clicked_team: 1, slot: 0 });
        assert_eq!(called_slots(&config), (Some(0), Some(1)));
    }

    #[test]
    fn clicking_the_called_frame_clears_the_call() {
        let mut config = MatchConfig::default();
        config.team1_kill_target = Some(2);

        apply_call_click(&mut config, CallClick { clicked_team: 2, slot: 2 });
        assert_eq!(config.team1_kill_target, None);

        // Clicking it again re-selects, matching the pre-match toggle.
        apply_call_click(&mut config, CallClick { clicked_team: 2, slot: 2 });
        assert_eq!(config.team1_kill_target, Some(2));
    }

    #[test]
    fn clicking_a_different_frame_replaces_the_call_rather_than_clearing() {
        let mut config = MatchConfig::default();
        config.team1_kill_target = Some(0);

        apply_call_click(&mut config, CallClick { clicked_team: 2, slot: 1 });
        assert_eq!(config.team1_kill_target, Some(1));
    }

    #[test]
    fn column_frame_rects_stack_down_each_screen_edge() {
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1500.0, 820.0));
        let frames = vec![primary(CharacterClass::Hunter), pet(CharacterClass::Hunter)];

        let left = column_frame_rects(&frames, 1, screen);
        let right = column_frame_rects(&frames, 2, screen);
        assert_eq!(left.len(), 2);
        assert_eq!(left[0].min.x, EDGE_MARGIN);
        assert_eq!(right[0].min.x, 1500.0 - EDGE_MARGIN - FRAME_W);
        // Frames stack downward without overlapping.
        assert!(left[1].min.y >= left[0].max.y);
        assert!(!left[0].intersects(right[0]));
    }

    #[test]
    fn column_frame_rects_are_empty_for_an_empty_column() {
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1500.0, 820.0));
        assert!(column_frame_rects(&[], 1, screen).is_empty());
    }
}
