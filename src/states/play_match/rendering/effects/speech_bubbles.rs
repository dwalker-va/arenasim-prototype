use crate::states::play_match::banter::vocab;
use crate::states::play_match::components::*;
use crate::ui::driver as ui_driver;
use crate::ui::theme::{self, data, hud};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use egui::emath::GuiRounding;

// ==============================================================================
// Speech Bubble Systems
// ==============================================================================

/// Render speech bubbles above combatants' heads
pub fn render_speech_bubbles(
    mut contexts: EguiContexts,
    speech_bubbles: Query<&SpeechBubble>,
    combatants: Query<&GlobalTransform, With<Combatant>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    class_icons: Res<crate::states::configure_match_ui::ClassIcons>,
    spell_icons: Res<SpellIcons>,
    emoji_icons: Res<crate::states::play_match::rendering::emoji::EmojiIcons>,
) {
    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    for bubble in speech_bubbles.iter() {
        // Above the owner's head, where the owner is drawn this frame.
        let Ok(owner_transform) = combatants.get(bubble.owner) else {
            continue;
        };
        let Some(screen_pos) = crate::states::play_match::rendering::hud_screen_anchor(
            camera,
            camera_transform,
            owner_transform,
            4.0,
        ) else {
            continue;
        };

        // A line is a sequence of glyph runs and icons, so measure the pieces
        // and lay them out on one row rather than measuring a single string.
        let font_id = egui::FontId::proportional(BUBBLE_TEXT_SIZE);
        let spans = vocab::parse(&bubble.text);
        let measured: Vec<(vocab::Span, f32)> = spans
            .into_iter()
            .map(|span| {
                let width = match &span {
                    vocab::Span::Text(text) => {
                        ctx.fonts(|f| {
                            f.layout_no_wrap(text.clone(), font_id.clone(), hud::BUBBLE_TEXT)
                        })
                        .size()
                        .x
                    }
                    // Icons are square and sized to the line's cap height so
                    // they sit on the same visual baseline as the glyphs.
                    _ => BUBBLE_ICON,
                };
                (span, width)
            })
            .collect();
        let content_w: f32 = measured.iter().map(|(_, w)| *w).sum();
        let content_h = BUBBLE_ICON.max(BUBBLE_TEXT_SIZE);

        // Tight padding around content
        let padding = hud::size::BUBBLE_PADDING;
        let bubble_size = egui::vec2(content_w, content_h) + padding * 2.0;
        // On a whole physical pixel, so the bubble and its text move as one
        // image instead of each shape rounding a new sub-pixel phase its own
        // way every frame (`nameplate_origin` has the full story).
        let bubble_pos = egui::pos2(
            screen_pos.x - bubble_size.x / 2.0,
            screen_pos.y - bubble_size.y / 2.0,
        )
        .round_to_pixels(ctx.pixels_per_point());

        let rect = egui::Rect::from_min_size(bubble_pos, bubble_size);

        // Paint speech bubble background
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(format!("speech_bubble_{:?}", bubble.owner)),
        ));

        // White rounded rectangle background
        painter.rect_filled(rect, theme::radius::MD, hud::BUBBLE_FILL);

        // Black border
        painter.rect_stroke(
            rect,
            theme::radius::MD,
            egui::Stroke::new(2.0_f32, hud::BUBBLE_EDGE),
            egui::StrokeKind::Outside,
        );

        // Lay the spans out left to right, vertically centred.
        let mut x = rect.min.x + padding.x;
        let mid_y = rect.center().y;
        for (span, width) in &measured {
            match span {
                vocab::Span::Text(text) => {
                    painter.text(
                        egui::pos2(x, mid_y),
                        egui::Align2::LEFT_CENTER,
                        text,
                        font_id.clone(),
                        hud::BUBBLE_TEXT,
                    );
                }
                vocab::Span::Class(class, team) => {
                    let icon_rect = icon_rect_at(x, mid_y);
                    // Team ownership is carried by a BORDER, not a tint.
                    //
                    // Tinting was tried first and multiplies the portrait's own
                    // colours, which muddies the silhouette exactly when the
                    // reader needs to identify a class at a glance in a bubble
                    // that is on screen for a few seconds. A frame around the
                    // art says the same thing and costs the art nothing.
                    match class_icons.textures.get(class) {
                        Some(texture) => {
                            painter.image(*texture, icon_rect, UV_FULL, theme::color::UNTINTED);
                            // For a `--ui-script`, which sees no pixels: the
                            // portrait art drew, not the initial plate below.
                            ui_driver::note_into(
                                ctx,
                                format_args!("banter-portrait-drawn:{class:?}"),
                            );
                        }
                        // No texture: a dark plate carrying the class initial,
                        // so the line still reads if art is missing.
                        None => {
                            ui_driver::note_into(
                                ctx,
                                format_args!("banter-portrait-missing:{class:?}"),
                            );
                            painter.rect_filled(icon_rect, theme::radius::SM, hud::BUBBLE_PLATE);
                            painter.text(
                                icon_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &class.name()[0..1],
                                egui::FontId::proportional(hud::text::BUBBLE_INITIAL),
                                hud::BUBBLE_PLATE_INITIAL,
                            );
                        }
                    }
                    // Inside, so the frame overlays the art's edge rather than
                    // bleeding into the glyph beside it.
                    painter.rect_stroke(
                        icon_rect,
                        theme::radius::SM,
                        egui::Stroke::new(2.0_f32, team_tint(*team)),
                        egui::StrokeKind::Inside,
                    );
                }
                vocab::Span::Ability(name) => {
                    let icon_rect = icon_rect_at(x, mid_y);
                    match spell_icons.textures.get(name) {
                        Some(texture) => {
                            painter.image(*texture, icon_rect, UV_FULL, theme::color::UNTINTED);
                        }
                        // A named ability with no loaded icon is a content
                        // mistake; show a placeholder rather than a gap so it
                        // is noticed.
                        None => {
                            painter.rect_stroke(
                                icon_rect,
                                theme::radius::SM,
                                egui::Stroke::new(1.0_f32, hud::BUBBLE_MISSING_ABILITY),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                }
                vocab::Span::Emoji(name) => {
                    let icon_rect = icon_rect_at(x, mid_y);
                    match emoji_icons.textures.get(name) {
                        Some(texture) => {
                            painter.image(*texture, icon_rect, UV_FULL, theme::color::UNTINTED);
                        }
                        // Named art that is not on disk. A dashed-looking
                        // outline rather than nothing, so a typo'd name is
                        // visible in the bubble instead of a silent gap.
                        None => {
                            painter.rect_stroke(
                                icon_rect,
                                theme::radius::SM,
                                egui::Stroke::new(1.0_f32, hud::BUBBLE_MISSING_EMOJI),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                }
                vocab::Span::Unknown => {
                    painter.rect_stroke(
                        icon_rect_at(x, mid_y),
                        theme::radius::SM,
                        egui::Stroke::new(1.0_f32, hud::BUBBLE_UNKNOWN),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            x += width;
        }
    }
}

/// Glyph size inside a speech bubble.
const BUBBLE_TEXT_SIZE: f32 = hud::text::BUBBLE;
/// Icon edge length inside a speech bubble, matched to the glyph size so
/// portraits and symbols sit on one visual line.
const BUBBLE_ICON: f32 = 20.0;
const UV_FULL: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

/// A square icon slot at `x`, vertically centred on `mid_y`.
fn icon_rect_at(x: f32, mid_y: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(x, mid_y - BUBBLE_ICON / 2.0),
        egui::vec2(BUBBLE_ICON, BUBBLE_ICON),
    )
}

/// The colour of the frame around a team's class portraits.
///
/// The team pair the combat-log timeline also uses for team headers, so a
/// portrait in a bubble reads as the same team a reader has seen elsewhere.
fn team_tint(team: u8) -> egui::Color32 {
    if team == 1 {
        data::TEAM_1
    } else {
        data::TEAM_2
    }
}

/// Update speech bubble lifetimes and remove expired ones
pub fn update_speech_bubbles(
    time: Res<Time>,
    mut commands: Commands,
    mut bubbles: Query<(Entity, &mut SpeechBubble)>,
) {
    let dt = time.delta_secs();

    for (entity, mut bubble) in bubbles.iter_mut() {
        bubble.lifetime -= dt;

        if bubble.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
