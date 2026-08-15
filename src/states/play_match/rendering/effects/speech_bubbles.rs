use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::states::play_match::banter::vocab;
use crate::states::play_match::components::*;

// ==============================================================================
// Speech Bubble Systems
// ==============================================================================

/// Whether a speech bubble should be drawn at all, given the gate state.
///
/// Ability bubbles are suppressed while the gates are closed: the countdown is
/// the buff rotation, and a Mage yelling "Frost Armor!" at an empty starting
/// room steps on the banter that plays there. Banter always renders. The filter
/// lives here rather than at the ~12 `spawn_speech_bubble` call sites because
/// this renderer is graphical-only — no sim file is touched, so headless stays
/// byte-identical by construction.
pub fn bubble_visible(kind: BubbleKind, gates_opened: bool) -> bool {
    match kind {
        BubbleKind::Ability => gates_opened,
        BubbleKind::Banter => true,
    }
}

/// Render speech bubbles above combatants' heads
pub fn render_speech_bubbles(
    mut contexts: EguiContexts,
    speech_bubbles: Query<&SpeechBubble>,
    combatants: Query<&Transform, With<Combatant>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    countdown: Res<MatchCountdown>,
    class_icons: Res<crate::states::configure_match_ui::ClassIcons>,
    spell_icons: Res<SpellIcons>,
    emoji_icons: Res<crate::states::play_match::rendering::emoji::EmojiIcons>,
) {
    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    for bubble in speech_bubbles.iter() {
        // Ability bubbles stay silent until the gates open (banter always shows)
        if !bubble_visible(bubble.kind, countdown.gates_opened) {
            continue;
        }

        // Get owner's position
        let Ok(owner_transform) = combatants.get(bubble.owner) else {
            continue;
        };

        // Position above the combatant's head
        let bubble_world_pos = owner_transform.translation + Vec3::new(0.0, 4.0, 0.0);

        // Project to screen space
        let Ok(screen_pos) = camera.world_to_viewport(camera_transform, bubble_world_pos) else {
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
                    vocab::Span::Text(text) => ctx
                        .fonts(|f| {
                            f.layout_no_wrap(text.clone(), font_id.clone(), egui::Color32::BLACK)
                        })
                        .size()
                        .x,
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
        let padding = egui::vec2(12.0, 6.0);
        let bubble_size = egui::vec2(content_w, content_h) + padding * 2.0;
        let bubble_pos = egui::pos2(
            screen_pos.x - bubble_size.x / 2.0,
            screen_pos.y - bubble_size.y / 2.0,
        );

        let rect = egui::Rect::from_min_size(bubble_pos, bubble_size);

        // Paint speech bubble background
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(format!("speech_bubble_{:?}", bubble.owner)),
        ));

        // White rounded rectangle background
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(6),
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 240),
        );

        // Black border
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(6),
            egui::Stroke::new(2.0, egui::Color32::BLACK),
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
                        egui::Color32::BLACK,
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
                            painter.image(*texture, icon_rect, UV_FULL, egui::Color32::WHITE);
                        }
                        // No texture: a dark plate carrying the class initial,
                        // so the line still reads if art is missing.
                        None => {
                            painter.rect_filled(icon_rect, 3.0, egui::Color32::from_gray(40));
                            painter.text(
                                icon_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &class.name()[0..1],
                                egui::FontId::proportional(13.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                    // Inside, so the frame overlays the art's edge rather than
                    // bleeding into the glyph beside it.
                    painter.rect_stroke(
                        icon_rect,
                        3.0,
                        egui::Stroke::new(2.0, team_tint(*team)),
                        egui::StrokeKind::Inside,
                    );
                }
                vocab::Span::Ability(name) => {
                    let icon_rect = icon_rect_at(x, mid_y);
                    match spell_icons.textures.get(name) {
                        Some(texture) => {
                            painter.image(*texture, icon_rect, UV_FULL, egui::Color32::WHITE);
                        }
                        // A named ability with no loaded icon is a content
                        // mistake; show a placeholder rather than a gap so it
                        // is noticed.
                        None => {
                            painter.rect_stroke(
                                icon_rect,
                                3.0,
                                egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                }
                vocab::Span::Emoji(name) => {
                    let icon_rect = icon_rect_at(x, mid_y);
                    match emoji_icons.textures.get(name) {
                        Some(texture) => {
                            painter.image(*texture, icon_rect, UV_FULL, egui::Color32::WHITE);
                        }
                        // Named art that is not on disk. A dashed-looking
                        // outline rather than nothing, so a typo'd name is
                        // visible in the bubble instead of a silent gap.
                        None => {
                            painter.rect_stroke(
                                icon_rect,
                                3.0,
                                egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                }
                vocab::Span::Unknown => {
                    painter.rect_stroke(
                        icon_rect_at(x, mid_y),
                        3.0,
                        egui::Stroke::new(1.0, egui::Color32::RED),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            x += width;
        }
    }
}

/// Glyph size inside a speech bubble.
const BUBBLE_TEXT_SIZE: f32 = 18.0;
/// Icon edge length inside a speech bubble, matched to the glyph size so
/// portraits and symbols sit on one visual line.
const BUBBLE_ICON: f32 = 20.0;
const UV_FULL: egui::Rect =
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

/// A square icon slot at `x`, vertically centred on `mid_y`.
fn icon_rect_at(x: f32, mid_y: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(x, mid_y - BUBBLE_ICON / 2.0),
        egui::vec2(BUBBLE_ICON, BUBBLE_ICON),
    )
}

/// The colour of the frame around a team's class portraits.
///
/// Same blue/red the combat-log timeline already uses for team headers, so a
/// portrait in a bubble reads as the same team a reader has seen elsewhere.
fn team_tint(team: u8) -> egui::Color32 {
    if team == 1 {
        egui::Color32::from_rgb(100, 150, 255)
    } else {
        egui::Color32::from_rgb(255, 100, 100)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ability_bubbles_are_hidden_until_the_gates_open() {
        // The starting-room cleanup: no buff shouting during the countdown.
        assert!(!bubble_visible(BubbleKind::Ability, false));
        assert!(bubble_visible(BubbleKind::Ability, true));
    }

    #[test]
    fn banter_bubbles_render_in_both_gate_states() {
        // Banter's whole point is the pre-gate scene, and the mid-fight shout
        // has to survive the gates opening too.
        assert!(bubble_visible(BubbleKind::Banter, false));
        assert!(bubble_visible(BubbleKind::Banter, true));
    }
}

