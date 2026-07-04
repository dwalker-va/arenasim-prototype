//! Broadcast-style fixed team frames (spectator UI).
//!
//! Team 1 frames are pinned to the left screen edge, Team 2 to the right,
//! WoW-arena-tournament style: class icon, health/resource bars, cast bar,
//! and buff/debuff icon rows with timers. This is the stable home for
//! per-combatant information; the overhead nameplate keeps only what is
//! *spatially* meaningful (a thin HP sliver + CC status labels), leaving the
//! head-level space free for effects like the Berserker Rage mask.
//!
//! Split like the Results screen: [`draw_team_frames`] is a pure egui
//! function (no Bevy ECS) so `tests/team_frames_snapshot.rs` can render it
//! offscreen via `egui_kittest` for fast visual iteration; the thin Bevy
//! wrapper [`render_team_frames`] collects ECS state into plain data each
//! frame and calls it.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::states::configure_match_ui::ClassIcons;
use crate::states::match_config::CharacterClass;
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

/// A cast (or channel) in progress.
pub struct FrameCast {
    pub name: String,
    /// 0.0 → 1.0 fill fraction.
    pub progress: f32,
    pub interrupted: bool,
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
    pub cast: Option<FrameCast>,
    pub buffs: Vec<FrameAura>,
    pub debuffs: Vec<FrameAura>,
}

/// Both columns.
#[derive(Default)]
pub struct TeamFramesData {
    pub team1: Vec<CombatantFrame>,
    pub team2: Vec<CombatantFrame>,
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
const CAST_H: f32 = 14.0;
const AURA_ICON: f32 = 20.0;
const AURA_GAP: f32 = 2.0;
const ROW_GAP: f32 = 4.0;
/// Max icons per aura row before truncation ("+N" overflow text).
const MAX_AURA_ICONS: usize = 8;

const PET_HEADER_H: f32 = 16.0;
const PET_HP_H: f32 = 10.0;

fn frame_height(frame: &CombatantFrame) -> f32 {
    if frame.pet_label.is_some() {
        return PAD + PET_HEADER_H + ROW_GAP + PET_HP_H + PAD;
    }
    let mut h = PAD + HEADER_H + ROW_GAP + HP_H + 3.0 + RESOURCE_H;
    if frame.cast.is_some() {
        h += ROW_GAP + CAST_H;
    }
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

/// Draw both team-frame columns. Pure egui — no ECS access.
pub fn draw_team_frames(
    ctx: &egui::Context,
    data: &TeamFramesData,
    class_icons: &ClassIcons,
    spell_icons: &SpellIcons,
) {
    let screen = ctx.screen_rect();

    egui::Area::new(egui::Id::new("team_frames"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            let painter = ui.painter();
            draw_column(
                painter,
                &data.team1,
                EDGE_MARGIN,
                screen,
                "TEAM 1",
                class_icons,
                spell_icons,
            );
            draw_column(
                painter,
                &data.team2,
                screen.width() - EDGE_MARGIN - FRAME_W,
                screen,
                "TEAM 2",
                class_icons,
                spell_icons,
            );
        });
}

fn draw_column(
    painter: &egui::Painter,
    frames: &[CombatantFrame],
    x: f32,
    screen: egui::Rect,
    label: &str,
    class_icons: &ClassIcons,
    spell_icons: &SpellIcons,
) {
    if frames.is_empty() {
        return;
    }

    let label_h = 16.0;
    let total_h = label_h
        + frames.iter().map(frame_height).sum::<f32>()
        + (frames.len() as f32 - 1.0) * FRAME_SPACING;
    let mut y = (screen.center().y - total_h / 2.0).max(60.0);

    painter.text(
        egui::pos2(x + FRAME_W / 2.0, y + label_h / 2.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgb(160, 160, 175),
    );
    y += label_h + 2.0;

    for frame in frames {
        let h = frame_height(frame);
        draw_frame(
            painter,
            egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(FRAME_W, h)),
            frame,
            class_icons,
            spell_icons,
        );
        y += h + FRAME_SPACING;
    }
}

fn draw_frame(
    painter: &egui::Painter,
    rect: egui::Rect,
    frame: &CombatantFrame,
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
    painter.rect_filled(rect, 4.0, egui::Color32::from_rgba_unmultiplied(13, 13, 20, 235));
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 45, 60)),
        egui::StrokeKind::Outside,
    );
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
    let tag = if !frame.alive {
        Some(("DEAD", egui::Color32::from_rgb(220, 70, 70)))
    } else if frame.stealthed {
        Some(("STEALTH", egui::Color32::from_rgb(170, 170, 190)))
    } else {
        None
    };
    if let Some((text, color)) = tag {
        painter.text(
            egui::pos2(inner_x + inner_w, y + HEADER_H / 2.0 - 1.0),
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

    // Cast bar (only while casting/channeling).
    if let Some(cast) = &frame.cast {
        y += ROW_GAP;
        let cast_rect = egui::Rect::from_min_size(egui::pos2(inner_x, y), egui::vec2(inner_w, CAST_H));
        painter.rect_filled(cast_rect, 2.0, egui::Color32::from_rgb(15, 15, 20));
        if cast.interrupted {
            painter.rect_stroke(
                cast_rect,
                2.0,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(220, 50, 50)),
                egui::StrokeKind::Outside,
            );
            painter.text(
                cast_rect.center(),
                egui::Align2::CENTER_CENTER,
                "INTERRUPTED",
                egui::FontId::proportional(10.0),
                egui::Color32::WHITE,
            );
        } else {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    cast_rect.min,
                    egui::vec2(cast_rect.width() * cast.progress.clamp(0.0, 1.0), CAST_H),
                ),
                2.0,
                egui::Color32::from_rgb(255, 180, 50),
            );
            painter.text(
                cast_rect.center(),
                egui::Align2::CENTER_CENTER,
                &cast.name,
                egui::FontId::proportional(10.0),
                egui::Color32::WHITE,
            );
        }
        y += CAST_H;
    }

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

/// Collect per-combatant state into [`TeamFramesData`] and draw the frames.
pub fn render_team_frames(
    mut contexts: EguiContexts,
    abilities: Res<AbilityDefinitions>,
    class_icons: Res<ClassIcons>,
    spell_icons: Res<SpellIcons>,
    combatants: Query<(
        Entity,
        &Combatant,
        Option<&CastingState>,
        Option<&ChannelingState>,
        Option<&ActiveAuras>,
    )>,
    pet_query: Query<&Pet>,
    display_settings: Res<DisplaySettings>,
) {
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    // Sort by (team, slot) so frame order is stable across frames and matches
    // config slot order (pets have slot >= 100 and sink to the column bottom).
    let mut rows: Vec<_> = combatants.iter().collect();
    rows.sort_by_key(|(_, c, _, _, _)| (c.team, c.slot));

    let mut data = TeamFramesData::default();
    for (entity, combatant, casting, channeling, auras) in rows {
        let pet_label = pet_query
            .get(entity)
            .ok()
            .map(|pet| format!("{} (pet)", pet.pet_type.name()));

        let cast = if let Some(casting) = casting {
            let def = abilities.get(&casting.ability);
            let (name, cast_time) = def
                .map(|d| (d.name.clone(), d.cast_time))
                .unwrap_or_else(|| (format!("{:?}", casting.ability), 0.0));
            let progress = if cast_time > 0.0 {
                1.0 - (casting.time_remaining / cast_time)
            } else {
                0.0
            };
            Some(FrameCast { name, progress, interrupted: casting.interrupted })
        } else if let Some(channeling) = channeling {
            let def = abilities.get(&channeling.ability);
            let name = def
                .map(|d| d.name.clone())
                .unwrap_or_else(|| format!("{:?}", channeling.ability));
            // Channels drain rather than fill: bar shows time remaining.
            let channel_duration = def.and_then(|d| d.channel_duration).unwrap_or(5.0);
            let progress = (channeling.duration_remaining / channel_duration).clamp(0.0, 1.0);
            Some(FrameCast { name, progress, interrupted: channeling.interrupted })
        } else {
            None
        };

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
            cast,
            buffs,
            debuffs,
        };

        if combatant.team == 1 {
            data.team1.push(frame);
        } else {
            data.team2.push(frame);
        }
    }

    draw_team_frames(ctx, &data, &class_icons, &spell_icons);
}
