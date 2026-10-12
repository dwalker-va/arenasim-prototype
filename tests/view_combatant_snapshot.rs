//! Offscreen visual snapshots of the View Combatant screen, and the check that
//! every tooltip on it shows at the client's default window size.
//!
//! This is the fast visual-iteration loop for `src/states/view_combatant_ui.rs`:
//! it renders the real `draw_view_combatant` to a PNG via `egui_kittest`
//! (wgpu, no window, no Bevy app) in a fraction of a second, over the real
//! `abilities.ron`, `items.ron` and `loadouts.ron`, so a kit or loadout change
//! shows up here.
//!
//! ## Loop
//! ```bash
//! # Render the screens; writes tests/snapshots/view_combatant_*.new.png
//! cargo test --release --test view_combatant_snapshot -- --ignored
//! # ...then open / read those PNGs, tweak view_combatant_ui.rs, repeat.
//!
//! # Once it looks right, bless the baselines:
//! UPDATE_SNAPSHOTS=1 cargo test --release --test view_combatant_snapshot -- --ignored
//! ```
//!
//! `#[ignore]` keeps these out of the default `cargo test` run because they
//! need a GPU adapter (wgpu), which CI runners may lack.
//!
//! Fidelity caveat, shared with the other egui snapshot loops: kittest has no
//! Bevy textures, so the class icon is absent and every ability, item and pet
//! icon renders as the kit's fallback tile. Fonts are the client's own stack
//! (`arenasim::ui::install`), so layout, spacing, colour, type and copy
//! iterate faithfully; icon art still needs the client.

use bevy_egui::egui;
use egui_kittest::Harness;

use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::match_config::{CharacterClass, MatchConfig};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::equipment::{load_default_loadouts, load_item_definitions};
use arenasim::states::view_combatant_ui::{
    draw_view_combatant, EquipmentPickerState, ViewCombatantData, ViewCombatantState,
};
use arenasim::states::GameState;
use arenasim::ui::driver::registry;
use arenasim::ui::driver::runner::{self, Inject, Micro, PointerStep, Progress};
use arenasim::ui::driver::script::Step;
use arenasim::ui::widgets::Nav;

/// Tall enough that the whole sheet — header, stats and kit, the class's
/// strategic panel and every equipment row — fits without scrolling.
const SIZE: [f32; 2] = [1280.0, 1820.0];

/// A 2v2: Warrior + Priest against Mage + Warlock, so both views below have a
/// filled enemy team (the Warlock's curse panel draws one row per enemy).
fn mock_config() -> MatchConfig {
    let mut config = MatchConfig::default();
    config.set_team1_size(2);
    config.set_team2_size(2);
    config.team1[0] = Some(CharacterClass::Warrior);
    config.team1[1] = Some(CharacterClass::Priest);
    config.team2[0] = Some(CharacterClass::Mage);
    config.team2[1] = Some(CharacterClass::Warlock);
    config
}

/// The Warrior: the generic strategic-option panel (Battle Shout) and a
/// two-hander's equipment set.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn view_combatant_warrior() {
    snapshot(
        "view_combatant_warrior",
        ViewCombatantState {
            class: CharacterClass::Warrior,
            team: 1,
            slot: 0,
        },
    );
}

/// The Warlock: the curse-preferences panel, one row per enemy, and the
/// Felhunter's pet subsection in the kit.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn view_combatant_warlock() {
    snapshot(
        "view_combatant_warlock",
        ViewCombatantState {
            class: CharacterClass::Warlock,
            team: 2,
            slot: 1,
        },
    );
}

// ===========================================================================
// EVERY TOOLTIP AT THE DEFAULT WINDOW SIZE
//
// Not `#[ignore]`d: this lays the screen out in a bare `egui::Context` with
// the `--ui-script` registry armed — no GPU — so the default `cargo test`
// holds it.
// ===========================================================================

/// The client's default window, in points (`GameSettings::default()`:
/// Windowed, `HD720`).
const DEFAULT_WINDOW: [f32; 2] = [1280.0, 720.0];

/// One simulated frame. Sixty a second, like the client under vsync; every
/// wait the driver makes is in egui's SECONDS, so the rate only sets their
/// resolution.
const DT: f32 = 1.0 / 60.0;

/// How long any one pointer action may wait before the test calls it stuck,
/// in sim seconds. A ceiling, not a settle: each wait ends on its condition.
const WAIT_LIMIT_SECS: f64 = 10.0;

/// The driver's per-step settle, in frames — `UiDriverConfig`'s default.
const SETTLE_FRAMES: u32 = 3;

/// The View Combatant screen in a bare egui context, driven by the
/// `--ui-script` driver's OWN code: `runner::expand` turns a `hover` into its
/// actions and `runner::pointer_step` decides each frame of them — the aim,
/// the scroll, the waits — exactly as `run_ui_script` does in the client.
/// All this harness adds is the translation of what it injects into egui
/// events, which bevy_egui does in the client.
struct Sheet<'a> {
    ctx: egui::Context,
    time: f64,
    /// What the last pass registered: the driver decides from the frame
    /// before, as it does in the client.
    last: registry::Frame,
    cursor: egui::Pos2,
    last_seen: Option<(String, egui::Rect)>,
    view: ViewCombatantState,
    config: MatchConfig,
    picker: EquipmentPickerState,
    data: ViewCombatantData<'a>,
}

impl Sheet<'_> {
    /// Run one frame with `events`, and keep what it registered.
    fn frame(&mut self, events: Vec<egui::Event>) {
        registry::arm(&self.ctx);
        self.time += f64::from(DT);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::from(DEFAULT_WINDOW),
            )),
            time: Some(self.time),
            predicted_dt: DT,
            events,
            ..Default::default()
        };
        let (view, config, picker, data) =
            (&self.view, &mut self.config, &mut self.picker, &self.data);
        let _ = self.ctx.run(input, |ctx| {
            let _ = draw_view_combatant(
                ctx,
                Some(view),
                config,
                picker,
                data,
                Nav::Return(GameState::ConfigureMatch),
            );
        });
        self.last = registry::snapshot(&self.ctx).expect("armed");
    }

    /// Run a script `hover` of `id` through the driver, then answer what
    /// `assert-note` would: whether the last drawn frame carries `note`.
    fn hover_shows(&mut self, id: &str, note: &str) -> bool {
        let hover = Step::Hover { id: id.to_string() };
        for action in runner::expand(&hover, SETTLE_FRAMES) {
            if let Micro::Idle(n) = action {
                for _ in 0..n {
                    self.frame(Vec::new());
                }
                continue;
            }
            let start = self.time;
            loop {
                let PointerStep { progress, inject } = runner::pointer_step(
                    &action,
                    &self.last,
                    &self.ctx,
                    &mut self.cursor,
                    &mut self.last_seen,
                )
                .unwrap_or_else(|| panic!("a hover expanded into {action:?}"));
                let events = inject
                    .into_iter()
                    .map(|event| match event {
                        Inject::Pointer { to, .. } => egui::Event::PointerMoved(to),
                        Inject::Wheel { lines } => egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Line,
                            delta: egui::vec2(0.0, lines),
                            modifiers: egui::Modifiers::default(),
                        },
                    })
                    .collect();
                self.frame(events);
                match progress {
                    Progress::Done => break,
                    Progress::Again { stuck } => assert!(
                        self.time - start < WAIT_LIMIT_SECS,
                        "hovering {id} in a {DEFAULT_WINDOW:?} window: {stuck}"
                    ),
                }
            }
        }
        self.last.has_note(note)
    }
}

/// Every View Combatant tooltip shows in a 1280x720 window: each kit row,
/// each pre-match choice tile that is an ability, and each worn item's row,
/// for every class. Each is hovered by the `--ui-script` driver's own code,
/// so a target the driver cannot reach — or reaches and points off the window,
/// as it did the Warrior's shout tiles straddling the bottom edge (AS-289) —
/// fails here rather than only in a client run.
#[test]
fn every_tooltip_shows_at_the_default_window_size() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();

    for &class in CharacterClass::all() {
        // Match Setup's default 1v1 with this class in team 1's first slot —
        // what `view-combatant-tooltips.script` opens.
        let mut config = MatchConfig::default();
        config.team1[0] = Some(class);
        let ctx = egui::Context::default();
        arenasim::ui::install(&ctx);
        let mut sheet = Sheet {
            ctx,
            time: 0.0,
            last: registry::Frame::default(),
            cursor: egui::Pos2::ZERO,
            last_seen: None,
            view: ViewCombatantState {
                class,
                team: 1,
                slot: 0,
            },
            config,
            picker: EquipmentPickerState::default(),
            data: ViewCombatantData {
                abilities: &abilities,
                items: &items,
                default_loadouts: &defaults,
                class_icons: &class_icons,
                ability_icons: None,
                item_icons: None,
                pet_icons: None,
            },
        };

        // A few passes, so anything egui sizes from the previous frame settles.
        for _ in 0..4 {
            sheet.frame(Vec::new());
        }

        let targets = hover_targets(&sheet.last);
        assert!(
            targets.iter().any(|(id, _)| id.starts_with("kit:"))
                && targets.iter().any(|(id, _)| id.starts_with("equip:")),
            "{class:?}: found no kit or equipment rows to hover: {targets:?}"
        );
        for (id, note) in targets {
            assert!(
                sheet.hover_shows(&id, &note),
                "{class:?}: hovering {id} in a {DEFAULT_WINDOW:?} window showed no \
                 tooltip ({note}). Pointer at {:?}; notes: {:?}",
                sheet.cursor,
                sheet.last.notes
            );
        }
    }
}

/// Every widget on the sheet with a tooltip, and the note its tooltip body
/// leaves: `kit:X` and `strategic:X[:eN]` show `Ability(X)`, and a worn item's
/// `equip:<Slot>` row shows `Item(<id>)`. An empty socket has no tooltip.
fn hover_targets(frame: &registry::Frame) -> Vec<(String, String)> {
    let mut targets = Vec::new();
    for widget in &frame.widgets {
        let id = widget.id.as_str();
        if let Some(ability) = id.strip_prefix("kit:") {
            targets.push((id.to_string(), format!("tooltip:Ability({ability})")));
        } else if let Some(rest) = id.strip_prefix("strategic:") {
            let ability = rest.split(':').next().expect("an ability name");
            targets.push((id.to_string(), format!("tooltip:Ability({ability})")));
        } else if let Some(slot) = id.strip_prefix("equip:") {
            let worn = format!("equip-row {slot} item=");
            let item = frame
                .notes
                .iter()
                .find_map(|n| n.strip_prefix(worn.as_str()))
                .and_then(|rest| rest.split(' ').next());
            if let Some(item) = item.filter(|item| *item != "empty") {
                targets.push((id.to_string(), format!("tooltip:Item({item})")));
            }
        }
    }
    targets
}

fn snapshot(name: &str, view: ViewCombatantState) {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let mut config = mock_config();
    let mut picker = EquipmentPickerState::default();

    let mut harness = Harness::builder().with_size(SIZE).build(move |ctx| {
        arenasim::ui::install(ctx);
        let data = ViewCombatantData {
            abilities: &abilities,
            items: &items,
            default_loadouts: &defaults,
            class_icons: &class_icons,
            ability_icons: None,
            item_icons: None,
            pet_icons: None,
        };
        let _ = draw_view_combatant(
            ctx,
            Some(&view),
            &mut config,
            &mut picker,
            &data,
            Nav::Return(GameState::ConfigureMatch),
        );
    });

    harness.run();
    harness.snapshot(name);
}
