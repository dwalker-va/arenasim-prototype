//! A screen comes back where it was left, and a fresh visit starts at the
//! top: View Combatant, scrolled down to a kit row, opens that ability's
//! encyclopedia page, and on the way back the row is where it was; opened
//! again from Match Setup, the sheet is back at the top.
//!
//! egui keeps a scroll offset per widget id, and every Document screen's body
//! is a `ScrollArea` inside a `CentralPanel`. Left at egui's default id, those
//! bodies all shared ONE offset, so an encyclopedia page short enough to fit
//! the window clamped it to zero and the sheet came back at the top (AS-470).
//! `GameState::show_body` keys each body by its screen. Keyed, the offset
//! would also outlive a visit, so a screen opened forward
//! (`ScreenContext::open_from`, which `ScreenNav::open` writes) is handed to
//! egui by `chrome::open_fresh_bodies` and its body starts at the top; a
//! return (`ScreenNav::leave`) records nothing and restores.
//!
//! Both screens are the real pure draws, in one bare `egui::Context` as the
//! client's single egui context holds them, with no GPU, so the default
//! `cargo test` holds this.

use bevy_egui::egui;

use arenasim::keybindings::Keybindings;
use arenasim::settings::GameSettings;
use arenasim::states::chrome::{open_fresh_bodies, ScreenContext};
use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::encyclopedia::{
    draw_encyclopedia, CatalogSource, EncyclopediaData, EncyclopediaState, EngineFamily, Topic,
};
use arenasim::states::keybindings_ui::{draw_keybindings, KeybindingsUiState};
use arenasim::states::match_config::{CharacterClass, MatchConfig};
use arenasim::states::options_ui::{draw_options, OptionsData};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::equipment::{load_default_loadouts, load_item_definitions};
use arenasim::states::play_match::talents::TalentDefinitions;
use arenasim::states::play_match::AbilityType;
use arenasim::states::view_combatant_ui::{
    draw_view_combatant, EquipmentPickerState, ViewCombatantData, ViewCombatantState,
};
use arenasim::states::GameState;
use arenasim::ui::driver::registry;
use arenasim::ui::widgets::Nav;

/// The client's default window, in points.
const WINDOW: [f32; 2] = [1280.0, 720.0];

/// One simulated frame, sixty a second.
const DT: f64 = 1.0 / 60.0;

/// The row the round trip leaves from: below the fold of a 1280x720 window,
/// so reaching it takes a scroll.
const ROW: &str = "kit:Ambush";

/// A bare egui context with the client's style, stepped one frame at a time.
struct Client {
    ctx: egui::Context,
    time: f64,
    window: [f32; 2],
}

impl Client {
    fn new() -> Self {
        Self::sized(WINDOW)
    }

    fn sized(window: [f32; 2]) -> Self {
        let ctx = egui::Context::default();
        arenasim::ui::install(&ctx);
        Client {
            ctx,
            time: 0.0,
            window,
        }
    }

    /// One pass of `draw` with `events`; what it registered.
    fn frame(
        &mut self,
        events: Vec<egui::Event>,
        draw: impl FnMut(&egui::Context),
    ) -> registry::Frame {
        registry::arm(&self.ctx);
        self.time += DT;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::from(self.window),
            )),
            time: Some(self.time),
            predicted_dt: DT as f32,
            events,
            ..Default::default()
        };
        let _ = self.ctx.run(input, draw);
        registry::snapshot(&self.ctx).expect("armed")
    }

    /// Whether the last pass scrolled. egui plays a wheel's delta out over
    /// several passes, and a pass with none left to play has no scroll at
    /// all, so this is false only once the animation is over.
    /// (`smooth_scroll_delta` cannot say so: the scroll area that applies it
    /// zeroes it in the same pass.)
    fn scrolling(&self) -> bool {
        self.ctx.input(|i| i.time_since_last_scroll()) == 0.0
    }
}

#[test]
fn view_combatant_comes_back_where_it_was_left_and_opens_fresh_at_the_top() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let talents = TalentDefinitions::shipped();

    let view = ViewCombatantState {
        class: CharacterClass::Rogue,
        team: 1,
        slot: 0,
    };
    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Rogue);
    let mut picker = EquipmentPickerState::default();
    let sheet_data = ViewCombatantData {
        abilities: &abilities,
        items: &items,
        default_loadouts: &defaults,
        class_icons: &class_icons,
        ability_icons: None,
        item_icons: None,
        pet_icons: None,
        talents,
    };
    let mut sheet = |ctx: &egui::Context| {
        let _ = draw_view_combatant(
            ctx,
            Some(&view),
            &mut config,
            &mut picker,
            &sheet_data,
            Nav::Return(GameState::ConfigureMatch),
        );
    };

    let mut client = Client::new();
    let mut nav = ScreenContext::default();
    // Match Setup opens the sheet.
    nav.open_from(GameState::ViewCombatant, GameState::ConfigureMatch);
    open_fresh_bodies(&mut nav, &client.ctx);
    let mut frame = registry::Frame::default();
    for _ in 0..4 {
        frame = client.frame(Vec::new(), &mut sheet);
    }
    let unscrolled = frame.widget(ROW).expect("the kit row is drawn").rect;
    assert!(
        unscrolled.min.y > WINDOW[1],
        "{ROW} must start below the fold for this to test a scroll: {unscrolled:?}"
    );

    // Scroll the sheet until the row is wholly on screen, then let egui's
    // smooth scroll play out, as a player's wheel would.
    let centre = egui::pos2(WINDOW[0] / 2.0, WINDOW[1] / 2.0);
    frame = client.frame(vec![egui::Event::PointerMoved(centre)], &mut sheet);
    let mut wheeled = 0;
    while frame.widget(ROW).expect("the kit row is drawn").rect.max.y > WINDOW[1] {
        wheeled += 1;
        assert!(wheeled < 200, "{ROW} never scrolled into view");
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -1.0),
            modifiers: egui::Modifiers::default(),
        };
        frame = client.frame(vec![wheel], &mut sheet);
    }
    let mut settled = 0;
    while client.scrolling() {
        settled += 1;
        assert!(settled < 600, "the smooth scroll never ended");
        frame = client.frame(Vec::new(), &mut sheet);
    }
    let left_at = frame.widget(ROW).expect("the kit row is drawn").rect;
    assert!(
        left_at.max.y <= WINDOW[1] && left_at != unscrolled,
        "{ROW} should be scrolled on screen before leaving: {left_at:?}"
    );

    // Click through to its page: an ability page short enough to fit the
    // window, so a scroll offset it shared would be clamped to zero.
    let encyclopedia_data = EncyclopediaData {
        items: &items,
        abilities: &abilities,
        item_icons: None,
        class_icons: None,
        ability_icons: None,
        talents,
        engine_auras: EngineFamily::ALL,
    };
    let mut encyclopedia = EncyclopediaState::default();
    encyclopedia.rebuild_registry(&CatalogSource::new(&abilities, &items, talents));
    encyclopedia.open_at(Topic::Ability(AbilityType::Ambush));
    nav.open_from(GameState::Encyclopedia, GameState::ViewCombatant);
    open_fresh_bodies(&mut nav, &client.ctx);
    for _ in 0..4 {
        client.frame(Vec::new(), |ctx| {
            let _ = draw_encyclopedia(
                ctx,
                &mut encyclopedia,
                &encyclopedia_data,
                GameState::ViewCombatant,
            );
        });
    }

    // And back: a leave, which records nothing.
    open_fresh_bodies(&mut nav, &client.ctx);
    for _ in 0..4 {
        frame = client.frame(Vec::new(), &mut sheet);
    }
    let came_back_at = frame.widget(ROW).expect("the kit row is drawn").rect;
    assert_eq!(
        came_back_at, left_at,
        "View Combatant came back from the encyclopedia scrolled elsewhere: {ROW} \
         was left at {left_at:?} and came back at {came_back_at:?} (unscrolled: \
         {unscrolled:?})"
    );

    // Out to Match Setup, which has no body, and the sheet opened again: a
    // fresh visit, at the top.
    nav.open_from(GameState::ViewCombatant, GameState::ConfigureMatch);
    open_fresh_bodies(&mut nav, &client.ctx);
    for _ in 0..4 {
        frame = client.frame(Vec::new(), &mut sheet);
    }
    let reopened_at = frame.widget(ROW).expect("the kit row is drawn").rect;
    assert_eq!(
        reopened_at, unscrolled,
        "View Combatant opened fresh where the last visit left it: {ROW} at \
         {reopened_at:?}, at the top it is {unscrolled:?}"
    );
}

/// Every widget a pass registered, with where it was drawn.
fn layout(frame: &registry::Frame) -> Vec<(String, egui::Rect)> {
    frame
        .widgets
        .iter()
        .map(|w| (w.id.clone(), w.rect))
        .collect()
}

/// Each of the four Document screens, drawn by its REAL draw function in one
/// egui context, keeps its own scroll offset: each is scrolled by a different
/// amount, all four are visited, and each comes back laid out exactly as it
/// was left. Two screens whose bodies shared an egui id would share one
/// offset, and the earlier of the two would come back at the later one's.
/// The window is short, so every screen's body scrolls.
#[test]
fn every_document_screen_keeps_its_own_scroll_offset() {
    const SHORT: [f32; 2] = [1280.0, 400.0];
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let talents = TalentDefinitions::shipped();
    let settings = GameSettings::default();
    let keybindings = Keybindings::default();
    let mut keybindings_state = KeybindingsUiState::default();
    let view = ViewCombatantState {
        class: CharacterClass::Rogue,
        team: 1,
        slot: 0,
    };
    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Rogue);
    let mut picker = EquipmentPickerState::default();
    let sheet_data = ViewCombatantData {
        abilities: &abilities,
        items: &items,
        default_loadouts: &defaults,
        class_icons: &class_icons,
        ability_icons: None,
        item_icons: None,
        pet_icons: None,
        talents,
    };
    let encyclopedia_data = EncyclopediaData {
        items: &items,
        abilities: &abilities,
        item_icons: None,
        class_icons: None,
        ability_icons: None,
        talents,
        engine_auras: EngineFamily::ALL,
    };
    let mut encyclopedia = EncyclopediaState::default();
    encyclopedia.rebuild_registry(&CatalogSource::new(&abilities, &items, talents));

    let screens = [
        GameState::Options,
        GameState::Keybindings,
        GameState::ViewCombatant,
        GameState::Encyclopedia,
    ];
    let mut draw = |screen: GameState, ctx: &egui::Context| match screen {
        GameState::Options => {
            let data = OptionsData {
                settings: &settings,
                restart_required: false,
            };
            let _ = draw_options(ctx, &data, Nav::Return(GameState::MainMenu));
        }
        GameState::Keybindings => {
            let _ = draw_keybindings(
                ctx,
                &keybindings,
                &mut keybindings_state,
                Nav::Return(GameState::Options),
            );
        }
        GameState::ViewCombatant => {
            let _ = draw_view_combatant(
                ctx,
                Some(&view),
                &mut config,
                &mut picker,
                &sheet_data,
                Nav::Return(GameState::ConfigureMatch),
            );
        }
        GameState::Encyclopedia => {
            let _ = draw_encyclopedia(
                ctx,
                &mut encyclopedia,
                &encyclopedia_data,
                GameState::MainMenu,
            );
        }
        other => unreachable!("{other:?} is not a Document screen"),
    };

    let mut client = Client::sized(SHORT);
    let centre = egui::pos2(SHORT[0] / 2.0, SHORT[1] / 2.0);
    let mut left_at = Vec::new();
    for (i, &screen) in screens.iter().enumerate() {
        let mut frame = registry::Frame::default();
        for _ in 0..4 {
            frame = client.frame(Vec::new(), |ctx| draw(screen, ctx));
        }
        let unscrolled = layout(&frame);
        client.frame(vec![egui::Event::PointerMoved(centre)], |ctx| {
            draw(screen, ctx)
        });
        // A different depth for each screen: 2, 4, 6 and 8 wheel lines.
        for _ in 0..2 * (i + 1) {
            let wheel = egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -1.0),
                modifiers: egui::Modifiers::default(),
            };
            client.frame(vec![wheel], |ctx| draw(screen, ctx));
        }
        let mut passes = 0;
        while client.scrolling() {
            passes += 1;
            assert!(passes < 600, "{screen:?}: the smooth scroll never ended");
            client.frame(Vec::new(), |ctx| draw(screen, ctx));
        }
        // One pass more, laid out at the offset the scroll ended on.
        let frame = client.frame(Vec::new(), |ctx| draw(screen, ctx));
        let scrolled = layout(&frame);
        // The encyclopedia's body marks no widgets for the driver, so its
        // own offset is not visible here; it is still drawn, so a body it
        // shared with another screen would move that screen's offset.
        if screen != GameState::Encyclopedia {
            assert_ne!(
                scrolled, unscrolled,
                "{screen:?}: setup: the body should have scrolled in a {SHORT:?} window"
            );
        }
        left_at.push((screen, scrolled));
    }

    for (screen, scrolled) in left_at {
        let mut frame = registry::Frame::default();
        for _ in 0..4 {
            frame = client.frame(Vec::new(), |ctx| draw(screen, ctx));
        }
        assert_eq!(
            layout(&frame),
            scrolled,
            "{screen:?} did not come back where it was left after visiting the \
             other Document screens"
        );
    }
}
