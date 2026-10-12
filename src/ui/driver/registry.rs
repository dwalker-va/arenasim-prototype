//! The per-frame widget registry — the half of the driver that UI code touches.
//!
//! A screen opts one widget in with a single call, [`mark`], naming it and
//! handing over the screen-space rect it just allocated. A screen reports
//! something it can see and the driver cannot — a tooltip closure that
//! actually ran, a row's rendered colour — with [`note`]. That is the whole
//! surface area; nothing else in `src/states/` knows the driver exists.
//!
//! # Why the registry lives in egui's own temp data
//!
//! The call sites are PURE egui draw functions (`render_ability_row`,
//! `draw_encyclopedia`, …) that take a `&mut egui::Ui` and no Bevy world. A
//! Bevy resource is unreachable from there, and threading a registry parameter
//! through every draw signature would make the opt-in cost a refactor rather
//! than one line. `egui::Context::data_mut` is reachable from any `Ui`, and the
//! driver's Bevy system reaches the same store through `EguiContexts`.
//!
//! # Inertness
//!
//! The store holds two entries, both keyed by a fixed [`egui::Id`]:
//!
//! * `ARMED` — a `bool`, written ONLY by [`arm`], which only the driver's own
//!   Bevy system calls.
//! * `FRAME` — this frame's [`Frame`], written only by [`mark`] / [`note`].
//!
//! [`mark`] and [`note`] both read `ARMED` first and return without writing
//! anything when it is absent or false. With the driver disabled, nothing ever
//! calls [`arm`], so `ARMED` never exists, so `FRAME` is never created: a
//! normal client run does one `get_temp::<bool>` lookup per marked widget per
//! frame and records nothing. Taking the text as `fmt::Arguments` means the
//! FORMATTING is skipped too.
//!
//! # The `format_args!` rule for call sites
//!
//! `format_args!` defers formatting, **not evaluation of its arguments**. So
//! this costs nothing when disabled:
//!
//! ```ignore
//! ui_driver::note(ui, format_args!("equip-row {slot:?} item={id:?}"));
//! ```
//!
//! and this builds a `Vec<String>` and a `String` on every frame with the
//! driver OFF, because the argument expression runs before `note` is even
//! called:
//!
//! ```ignore
//! ui_driver::note(ui, format_args!("{}", xs.iter().map(f).collect::<Vec<_>>().join(",")));
//! ```
//!
//! If a note needs assembling, wrap the source in a `Display` and let the
//! formatter do the work — `view_combatant_ui::OverrideMap` is the worked
//! example, and it exists because this file's own claim was wrong at that one
//! call site. `note_does_not_format_its_arguments_when_disabled` pins the
//! primitive's half.
//!
//! `registry_records_nothing_until_it_is_armed` in `tests/ui_driver.rs` proves
//! that by MUTATION rather than by assertion: it runs the identical call
//! sequence twice against one `egui::Context`, unarmed and armed, and requires
//! the two to differ.

use bevy_egui::egui;

/// The `ARMED` flag's key.
fn armed_id() -> egui::Id {
    egui::Id::new("arenasim::ui_driver::armed")
}

/// The current frame's [`Frame`] key.
fn frame_id() -> egui::Id {
    egui::Id::new("arenasim::ui_driver::frame")
}

/// One widget a screen opted into, as it was drawn this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    /// The script-facing name. Unique per frame by convention; when two
    /// widgets share a name the driver targets the FIRST, and `dump` shows
    /// both, so a collision is visible rather than silent.
    pub id: String,
    /// Screen-space rect, in egui points — exactly what the driver needs to
    /// aim a synthetic cursor at.
    pub rect: egui::Rect,
    /// Whether the widget was drawn interactive. A disabled button still
    /// registers, so a script can assert that it IS disabled.
    pub enabled: bool,
    /// Whether the rect is inside its parent's clip rect — i.e. actually on
    /// screen. View Combatant is one long scroll, so a widget can be laid out
    /// and still be nowhere the cursor can reach it. The driver scrolls
    /// toward an invisible target rather than clicking a phantom coordinate.
    ///
    /// True as soon as ANY of the rect is inside the clip rect, so a widget
    /// straddling the bottom of the viewport is visible here while its centre
    /// is not — aim through [`Widget::aim_point`], never on this flag.
    pub visible: bool,
    /// The clip rect the widget was drawn under, in egui points:
    /// [`egui::Rect::NOTHING`] when its `Ui` was not visible at all.
    pub clip: egui::Rect,
}

impl Widget {
    /// Where a pointer can rest on this widget and be over it, or `None` when
    /// it has to be scrolled further into view first.
    ///
    /// That is the rect's centre once the whole rect is inside its clip rect.
    /// `visible` alone is not enough: a row of tiles straddling the bottom of a
    /// 1280x720 window is visible by a few points while its centre lies below
    /// the window, so a pointer sent there hovers nothing (AS-289). A widget
    /// too big for its clip rect can never be wholly shown, so for one of those
    /// a centre inside the clip rect is enough.
    pub fn aim_point(&self) -> Option<egui::Pos2> {
        let center = self.rect.center();
        let fits =
            self.rect.width() <= self.clip.width() && self.rect.height() <= self.clip.height();
        let shown = if fits {
            self.clip.contains_rect(self.rect)
        } else {
            self.clip.contains(center)
        };
        shown.then_some(center)
    }
}

/// Everything the last drawn frame reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub widgets: Vec<Widget>,
    pub notes: Vec<String>,
}

impl Frame {
    /// The first widget registered under `id` this frame, on screen or not.
    pub fn widget(&self, id: &str) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.id == id)
    }

    /// Whether `id` was drawn AND is inside the clip rect. This is what
    /// `assert-visible` means: laid out but scrolled off is not visible.
    pub fn is_visible(&self, id: &str) -> bool {
        self.widget(id).is_some_and(|w| w.visible)
    }

    /// Whether any note contains `needle`.
    pub fn has_note(&self, needle: &str) -> bool {
        self.notes.iter().any(|n| n.contains(needle))
    }
}

/// Whether the registry is collecting this frame.
fn is_armed(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(armed_id()).unwrap_or(false))
}

/// Start collecting: clear the previous frame's records and set the flag.
///
/// Called once per frame by the driver's Bevy system, before the UI draws.
/// Nothing else calls it, which is what makes the registry inert in a normal
/// build.
pub fn arm(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.insert_temp(armed_id(), true);
        d.insert_temp(frame_id(), Frame::default());
    });
}

/// Stop collecting and drop everything the registry holds.
pub fn disarm(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.remove_temp::<bool>(armed_id());
        d.remove_temp::<Frame>(frame_id());
    });
}

/// What the last armed frame recorded. `None` when the registry is unarmed.
pub fn snapshot(ctx: &egui::Context) -> Option<Frame> {
    if !is_armed(ctx) {
        return None;
    }
    raw_frame(ctx)
}

/// The stored frame WITHOUT the armed gate.
///
/// [`snapshot`] checks `ARMED` as a belt-and-braces guard, which makes it
/// useless for proving the opt-in calls wrote nothing: a leaking [`mark`]
/// would still read back as `None` through that gate. This is the unguarded
/// view, and it is what `registry_records_nothing_until_it_is_armed` asserts
/// on — the first draft asserted on `snapshot` and, as the mutation run
/// showed, could not see a `mark` that ignored the flag entirely.
pub fn raw_frame(ctx: &egui::Context) -> Option<Frame> {
    ctx.data(|d| d.get_temp::<Frame>(frame_id()))
}

/// Register the widget a screen just drew, so a script can name it.
///
/// `rect` is the screen-space rect the widget occupies — the one
/// `allocate_exact_size` returned, or `response.rect`. `enabled` is whether it
/// is interactive right now.
///
/// The id is taken as `fmt::Arguments` rather than `&str` so a call site can
/// interpolate freely (`format_args!("kit:{name}")`) without allocating on the
/// overwhelmingly common path where the driver is off.
///
/// ```ignore
/// ui_driver::mark(ui, rect, true, format_args!("kit:{ability:?}"));
/// ```
pub fn mark(ui: &egui::Ui, rect: egui::Rect, enabled: bool, id: std::fmt::Arguments<'_>) {
    // The armed check comes first so the disabled path does no work at all —
    // `is_rect_visible` is cheap, but it is still more than one hash lookup.
    if !is_armed(ui.ctx()) {
        return;
    }
    let clip = if ui.is_visible() {
        ui.clip_rect()
    } else {
        egui::Rect::NOTHING
    };
    mark_into(ui.ctx(), rect, enabled, clip, id);
}

/// [`mark`] with the clip rect supplied — the seam the registry's own tests
/// drive. The widget is visible when `rect` meets `clip`, which is egui's own
/// `Ui::is_rect_visible` rule.
pub fn mark_into(
    ctx: &egui::Context,
    rect: egui::Rect,
    enabled: bool,
    clip: egui::Rect,
    id: std::fmt::Arguments<'_>,
) {
    if !is_armed(ctx) {
        return;
    }
    let visible = rect.intersects(clip);
    let id = id.to_string();
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Frame>(frame_id())
            .widgets
            .push(Widget {
                id,
                rect,
                enabled,
                visible,
                clip,
            });
    });
}

/// Record something only the draw can see: which tooltip body ran, what a row
/// actually rendered. The driver logs these and `assert-note` matches on them.
pub fn note(ui: &egui::Ui, text: std::fmt::Arguments<'_>) {
    note_into(ui.ctx(), text);
}

/// [`note`] against a bare context.
pub fn note_into(ctx: &egui::Context, text: std::fmt::Arguments<'_>) {
    if !is_armed(ctx) {
        return;
    }
    let text = text.to_string();
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Frame>(frame_id())
            .notes
            .push(text);
    });
}
