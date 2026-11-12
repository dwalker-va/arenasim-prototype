//! No tooltip shows while the content under the pointer is scrolling.
//!
//! egui hides tooltips during a scroll only while the scroll is younger than
//! `tooltip_delay`, so the client's old zero delay switched that guard off. A
//! sheet scrolling under a resting pointer then opened the tooltip of whatever
//! tile passed under it, and a tooltip under the pointer takes the wheel: in
//! View Combatant the sheet stopped scrolling for good with a choice tile's
//! tooltip under the pointer (AS-470, `view-combatant-tooltips.script:35`).
//! This drives the client's own style (`arenasim::ui::install`).

use bevy_egui::egui;

const DT: f64 = 1.0 / 60.0;
const WIDGET: egui::Rect = egui::Rect {
    min: egui::pos2(100.0, 100.0),
    max: egui::pos2(200.0, 140.0),
};

struct Screen {
    ctx: egui::Context,
    time: f64,
}

impl Screen {
    /// One pass of a hoverable widget with a text tooltip, over a scroll area
    /// that fills the window; whether the tooltip showed.
    fn pass(&mut self, events: Vec<egui::Event>) -> bool {
        self.time += DT;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            time: Some(self.time),
            predicted_dt: DT as f32,
            events,
            ..Default::default()
        };
        let mut shown = false;
        let _ = self.ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.allocate_space(egui::vec2(100.0, 5000.0));
                        // Fixed on screen, so the pointer stays on it however
                        // far the content scrolls.
                        let response =
                            ui.interact(WIDGET, egui::Id::new("tile"), egui::Sense::hover());
                        response.on_hover_ui(|ui| {
                            shown = true;
                            ui.label("Cheap Shot");
                        });
                    });
            });
        });
        shown
    }

    fn wheel(&mut self) -> bool {
        self.pass(vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -1.0),
            modifiers: egui::Modifiers::default(),
        }])
    }
}

#[test]
fn no_tooltip_shows_while_the_content_scrolls() {
    let ctx = egui::Context::default();
    arenasim::ui::install(&ctx);
    let mut screen = Screen { ctx, time: 0.0 };

    // The control: resting on the widget, its tooltip shows.
    screen.pass(vec![egui::Event::PointerMoved(WIDGET.center())]);
    let mut passes = 0;
    while !screen.pass(Vec::new()) {
        passes += 1;
        assert!(
            passes < 120,
            "the tooltip never showed on a resting pointer"
        );
    }

    // A scroll: no pass shows the tooltip while the content moves, the open
    // one included.
    let mut shown_while_scrolling = 0;
    for _ in 0..10 {
        if screen.wheel() {
            shown_while_scrolling += 1;
        }
    }
    assert!(
        screen.ctx.input(|i| i.time_since_last_scroll()) == 0.0,
        "setup: the last pass scrolled"
    );
    assert_eq!(
        shown_while_scrolling, 0,
        "the tooltip showed on {shown_while_scrolling} of 10 scrolling passes"
    );

    // And once the scroll has played out, the resting pointer gets it back.
    let mut passes = 0;
    while !screen.pass(Vec::new()) {
        passes += 1;
        assert!(passes < 600, "the tooltip never came back after the scroll");
    }
}
