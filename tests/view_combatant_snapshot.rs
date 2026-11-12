//! Offscreen visual snapshots of the View Combatant screen, and the check that
//! every tooltip on it shows at the client's default window size.
//!
//! This is the fast visual-iteration loop for `src/states/view_combatant_ui.rs`:
//! it renders the real `draw_view_combatant` to a PNG via `egui_kittest`
//! (wgpu, no window, no Bevy app) in a fraction of a second, over the real
//! `items.ron` and `loadouts.ron`, and the real `abilities.ron` and
//! `talents.ron` restricted to the pinned catalogue
//! (`tests/common/pinned_catalogue.rs`). A loadout change, or a change to a
//! pinned ability's or talent node's own text, shows up here; an ability or a
//! talent node added since does not, so a card that adds one moves no pixel
//! of these sheets. (Items need no pin: the sheet draws only the equipped
//! ones, which the loadout names.)
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

mod common;

use bevy_egui::egui;
use egui_kittest::Harness;

use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::match_config::{CharacterClass, MatchConfig, PaladinAura};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::components::Combatant;
use arenasim::states::play_match::equipment::{
    load_default_loadouts, load_item_definitions, parse_item_definitions, resolve_equipped_loadout,
    Loadout,
};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId};
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

/// The same for a class with talent trees, whose sheet carries the talent
/// panel under its pre-match choice.
const TALENTED_SIZE: [f32; 2] = [1280.0, 2320.0];

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

/// The Warlock with a build: the talent panel's spec line and points, taken
/// nodes (Demonic Embrace 2/2, Fel Stamina 1/2), the tier-2 node open, and the
/// stats panel's health raised by the build.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn view_combatant_warlock_talented() {
    let mut config = mock_config();
    config.team2_builds[1] = Build::of(&[(TalentId::DemonicEmbrace, 2), (TalentId::FelStamina, 1)]);
    snapshot_with(
        "view_combatant_warlock_talented",
        ViewCombatantState {
            class: CharacterClass::Warlock,
            team: 2,
            slot: 1,
        },
        config,
        TALENTED_SIZE,
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

impl<'a> Sheet<'a> {
    /// The sheet for `class` in team 1's first slot of `config`, laid out at
    /// scroll 0 and settled.
    fn open(class: CharacterClass, config: MatchConfig, data: ViewCombatantData<'a>) -> Self {
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
            data,
        };
        // A few passes, so anything egui sizes from the previous frame settles.
        for _ in 0..4 {
            sheet.frame(Vec::new());
        }
        sheet
    }

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
        let mut sheet = Sheet::open(
            class,
            config,
            ViewCombatantData {
                abilities: &abilities,
                items: &items,
                default_loadouts: &defaults,
                class_icons: &class_icons,
                ability_icons: None,
                item_icons: None,
                pet_icons: None,
                talents: TalentDefinitions::shipped(),
            },
        );

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

/// Every class's pre-match choice is wholly on screen in a 1280x720 window
/// before the player scrolls: each of its tiles' full rect inside the window
/// and inside the scroll area's clip. The choice is the decision this screen
/// exists for (AS-294), so it may not sit below the fold whatever the length
/// of the kit; the kit rows and the equipment may still scroll.
///
/// The Warlock is checked against a full enemy team as well as the default
/// 1v1, since it draws one row of curse tiles per enemy.
#[test]
fn every_pre_match_choice_is_on_screen_at_the_default_window_size() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::from(DEFAULT_WINDOW));

    let mut cases: Vec<(CharacterClass, usize)> = CharacterClass::all()
        .iter()
        .map(|&class| (class, 1))
        .collect();
    cases.push((CharacterClass::Warlock, 3));
    for (class, enemies) in cases {
        let mut config = MatchConfig::default();
        config.set_team1_size(enemies);
        config.set_team2_size(enemies);
        config.team1[0] = Some(class);
        let mut sheet = Sheet::open(
            class,
            config,
            ViewCombatantData {
                abilities: &abilities,
                items: &items,
                default_loadouts: &defaults,
                class_icons: &class_icons,
                ability_icons: None,
                item_icons: None,
                pet_icons: None,
                talents: TalentDefinitions::shipped(),
            },
        );
        sheet.frame(Vec::new());

        let tiles: Vec<&registry::Widget> = sheet
            .last
            .widgets
            .iter()
            .filter(|w| w.id.starts_with("strategic:") || w.id.starts_with("choice:"))
            .collect();
        // Named per class, so a choice that stops drawing — or a class whose
        // choice is never registered — fails here instead of passing vacuously.
        let per_row = match class {
            // Two openers and four weapon poisons.
            CharacterClass::Rogue => 6,
            CharacterClass::Warrior
            | CharacterClass::Mage
            | CharacterClass::Paladin
            | CharacterClass::Hunter
            | CharacterClass::Warlock => 3,
            CharacterClass::Priest | CharacterClass::Shaman | CharacterClass::Druid => 0,
        };
        let rows = if class == CharacterClass::Warlock {
            enemies
        } else {
            1
        };
        assert_eq!(
            tiles.len(),
            per_row * rows,
            "{class:?} vs {enemies} enemies: expected {} choice tiles, found {:?}",
            per_row * rows,
            tiles.iter().map(|w| &w.id).collect::<Vec<_>>()
        );
        for tile in tiles {
            assert!(
                screen.contains_rect(tile.rect) && tile.clip.contains_rect(tile.rect),
                "{class:?} vs {enemies} enemies: {} at {:?} is not wholly on screen at \
                 scroll 0 in a {DEFAULT_WINDOW:?} window (scroll area {:?})",
                tile.id,
                tile.rect,
                tile.clip
            );
        }
    }
}

/// The mitigation rows say what their total buys: the Armor hover gives its
/// physical reduction (`armor / (armor + 5500)`), and a resistance's hover
/// its damage reduction (`value / (value * 5/3 + 300)`), both read off the
/// row's own total.
#[test]
fn mitigation_rows_say_what_their_total_buys() {
    use arenasim::states::play_match::equipment::{ItemId, ItemSlot};

    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Warlock);
    config.team1_equipment[0].insert(ItemSlot::Neck, ItemId::AmuletOfShadowWard);
    let sheet = Sheet::open(
        CharacterClass::Warlock,
        config,
        ViewCombatantData {
            abilities: &abilities,
            items: &items,
            default_loadouts: &defaults,
            class_icons: &class_icons,
            ability_icons: None,
            item_icons: None,
            pet_icons: None,
            talents: TalentDefinitions::shipped(),
        },
    );
    let row = |key: &str| -> (f32, String) {
        let prefix = format!("stat {key}=");
        let note = sheet
            .last
            .notes
            .iter()
            .find_map(|n| n.strip_prefix(prefix.as_str()))
            .unwrap_or_else(|| panic!("no {key} row: {:?}", sheet.last.notes));
        let (value, hover) = note.split_once(" hover=").expect("a hover");
        (value.parse().expect("a number"), hover.to_string())
    };

    let (armor, hover) = row("Armor");
    assert!(armor > 0.0, "the Warlock's kit carries armor");
    let want = format!(
        "({:.1}% physical reduction)",
        armor / (armor + 5500.0) * 100.0
    );
    assert!(hover.ends_with(&want), "{hover:?} should end {want:?}");

    let (shadow, hover) = row("Shadow Resist");
    assert_eq!(shadow, 20.0, "the amulet's 20 shadow resistance");
    assert!(hover.ends_with("(6.0% damage reduction)"), "{hover:?}");

    // A build that raises armor: the reduction is the TALENTED total's. A
    // fixture makes Demonic Embrace +50% armor a rank (no shipped talent
    // touches armor yet).
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads");
    let health = "per_rank: [StatPercent(MaxHealth, 0.03)],";
    assert!(text.contains(health));
    let fixture = TalentDefinitions::from_ron(
        &text.replacen(
            health,
            "budget: Some(11.0), per_rank: [StatPercent(Armor, 0.5)],",
            1,
        ),
        &abilities,
    )
    .expect("the fixture validates");
    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Warlock);
    config.team1_builds[0] = Build::of(&[(TalentId::DemonicEmbrace, 1)]);
    let talented = Sheet::open(
        CharacterClass::Warlock,
        config,
        ViewCombatantData {
            abilities: &abilities,
            items: &items,
            default_loadouts: &defaults,
            class_icons: &class_icons,
            ability_icons: None,
            item_icons: None,
            pet_icons: None,
            talents: &fixture,
        },
    );
    let note = talented
        .last
        .notes
        .iter()
        .find_map(|n| n.strip_prefix("stat Armor="))
        .expect("an Armor row");
    let (value, hover) = note.split_once(" hover=").expect("a hover");
    // Both totals are printed whole, so compare within a point of rounding.
    let boosted: f32 = value.parse().expect("a number");
    assert!(
        (boosted - armor * 1.5).abs() <= 1.0,
        "+50% of {armor} is not {boosted}"
    );
    let reduction: f32 = hover
        .rsplit_once('(')
        .and_then(|(_, tail)| tail.strip_suffix("% physical reduction)"))
        .expect("a physical reduction suffix")
        .parse()
        .expect("a percentage");
    let talented = boosted / (boosted + 5500.0) * 100.0;
    let geared = armor / (armor + 5500.0) * 100.0;
    assert!(
        (reduction - talented).abs() <= 0.1 && (reduction - geared).abs() > 0.1,
        "{hover:?}: the talented total buys {talented:.1}%, the gear alone {geared:.1}%"
    );
}

/// The curse panel offers Curse of Exhaustion only to a slot whose build
/// takes its talent: an untalented Warlock is offered the three base curses,
/// a talented one the fourth beside them.
#[test]
fn the_curse_panel_offers_exhaustion_only_with_its_talent() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let data = || ViewCombatantData {
        abilities: &abilities,
        items: &items,
        default_loadouts: &defaults,
        class_icons: &class_icons,
        ability_icons: None,
        item_icons: None,
        pet_icons: None,
        talents: TalentDefinitions::shipped(),
    };
    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Warlock);
    let untalented = Sheet::open(CharacterClass::Warlock, config.clone(), data());
    for curse in ["CurseOfAgony", "CurseOfWeakness", "CurseOfTongues"] {
        assert!(
            untalented
                .last
                .widget(&format!("strategic:{curse}:e0"))
                .is_some(),
            "{curse}"
        );
    }
    assert!(untalented
        .last
        .widget("strategic:CurseOfExhaustion:e0")
        .is_none());

    config.team1_builds[0] = Build::of(&[
        (TalentId::ImprovedDrainLife, 2),
        (TalentId::AmplifyCurse, 1),
        (TalentId::CurseOfExhaustion, 1),
    ]);
    let talented = Sheet::open(CharacterClass::Warlock, config, data());
    assert!(talented
        .last
        .widget("strategic:CurseOfExhaustion:e0")
        .is_some());
}

/// The sheet spends THIS combatant's build: a class with trees draws its
/// talent panel over the slot's build, which the stats read back (Demonic
/// Embrace 2/2 is +6% health); a class without trees draws no panel.
#[test]
fn the_sheet_draws_the_slots_build_and_its_stats_read_it() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let data = || ViewCombatantData {
        abilities: &abilities,
        items: &items,
        default_loadouts: &defaults,
        class_icons: &class_icons,
        ability_icons: None,
        item_icons: None,
        pet_icons: None,
        talents: TalentDefinitions::shipped(),
    };
    let health = |sheet: &Sheet| -> i32 {
        sheet
            .last
            .notes
            .iter()
            .find_map(|n| n.strip_prefix("stat Health="))
            .unwrap_or_else(|| panic!("no health note: {:?}", sheet.last.notes))
            .parse()
            .expect("a number")
    };

    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Warlock);
    let untalented = Sheet::open(CharacterClass::Warlock, config.clone(), data());
    assert!(untalented.last.widget("talent:DemonicEmbrace").is_some());
    assert!(untalented.last.has_note("talents:t1s0 points=0"));

    config.team1_builds[0] = Build::of(&[(TalentId::DemonicEmbrace, 2)]);
    let talented = Sheet::open(CharacterClass::Warlock, config, data());
    assert!(
        talented
            .last
            .has_note("talents:t1s0 points=2 spec=Demonology Warlock 0/2"),
        "{:?}",
        talented.last.notes
    );
    let (before, after) = (health(&untalented), health(&talented));
    assert_eq!(
        after,
        (before as f32 * 1.06).round() as i32,
        "{before} -> {after}"
    );

    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Warrior);
    let warrior = Sheet::open(CharacterClass::Warrior, config, data());
    assert!(
        !warrior
            .last
            .widgets
            .iter()
            .any(|w| w.id.starts_with("talent:")),
        "a class without trees draws no talent panel"
    );
}

/// The Move Speed row reads the speed the spawn moves at: the natural speed
/// raised by the one capped passive term, the strongest of the gear's and the
/// build's sources. A fixture makes Demonic Embrace +5% movement speed a rank,
/// and another gives the Warlock's boots +5% and its crown +3%. The Warlock
/// (4.5 yd/s) reads, bare: 4.5, then 4.7 (+5%), then 4.9 at two ranks (+10%
/// offered, capped at +8%; uncapped would read 5.0). Geared: 4.7 (the boots'
/// +5%, not +8%), and 4.7 with one rank (+5% beside +5%, not +10%). Each value
/// is the spawn's own, equipped, resolved and applied.
#[test]
fn move_speed_row_reads_the_spawns_capped_passive_speed() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let speedy = {
        let text = std::fs::read_to_string(arenasim::paths::asset_path("config/items.ron"))
            .expect("items.ron reads");
        let boots = "name: \"Magister's Boots\",";
        let crown = "name: \"Magister's Crown\",";
        assert!(text.contains(boots) && text.contains(crown));
        parse_item_definitions(
            &text
                .replacen(boots, &format!("{boots} movement_speed: 0.05,"), 1)
                .replacen(crown, &format!("{crown} movement_speed: 0.03,"), 1),
            "fixture",
        )
        .expect("the fixture loads")
    };
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads");
    let health = "per_rank: [StatPercent(MaxHealth, 0.03)],";
    assert!(text.contains(health));
    let fixture = TalentDefinitions::from_ron(
        &text.replacen(health, "per_rank: [StatAdd(MovementSpeed, 0.05)],", 1),
        &abilities,
    )
    .expect("the fixture validates");
    let mut rows = Vec::new();
    for (items, ranks) in [
        (&items, 0u8),
        (&items, 1),
        (&items, 2),
        (&speedy, 0),
        (&speedy, 1),
    ] {
        let build = if ranks == 0 {
            Build::default()
        } else {
            Build::of(&[(TalentId::DemonicEmbrace, ranks)])
        };
        let loadout =
            resolve_equipped_loadout(CharacterClass::Warlock, &defaults, &Loadout::new(), items);
        let mut spawn = Combatant::new(1, 0, CharacterClass::Warlock);
        spawn.apply_equipment(&loadout, items);
        fixture
            .resolve(&abilities, CharacterClass::Warlock, &build)
            .expect("the build resolves")
            .apply(&mut spawn);

        let mut config = MatchConfig::default();
        config.team1[0] = Some(CharacterClass::Warlock);
        config.team1_builds[0] = build;
        let sheet = Sheet::open(
            CharacterClass::Warlock,
            config,
            ViewCombatantData {
                abilities: &abilities,
                items,
                default_loadouts: &defaults,
                class_icons: &class_icons,
                ability_icons: None,
                item_icons: None,
                pet_icons: None,
                talents: &fixture,
            },
        );
        let row = note_value(&sheet.last.notes, "stat", "Move Speed")
            .unwrap_or_else(|| panic!("{ranks} ranks: no Move Speed row"));
        let (value, _) = row.split_once(" hover=").expect("a hover");
        assert_eq!(
            value,
            format!("{:.1}/s", spawn.base_movement_speed),
            "{ranks} ranks: the sheet against the spawn"
        );
        rows.push(value.to_string());
    }
    assert_eq!(rows, ["4.5/s", "4.7/s", "4.9/s", "4.7/s", "4.7/s"]);
}

/// A build's flat crit reads on the sheet as the spawn applies it: the Crit
/// Chance row is the geared total plus the points, and its hover names them.
/// A fixture makes Demonic Embrace +5% crit chance a rank, in points.
#[test]
fn a_stat_add_reads_on_the_sheet_in_points() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads");
    let health = "per_rank: [StatPercent(MaxHealth, 0.03)],";
    assert!(text.contains(health));
    let fixture = TalentDefinitions::from_ron(
        &text.replacen(health, "per_rank: [StatAdd(CritChance, 0.05)],", 1),
        &abilities,
    )
    .expect("the fixture validates");
    let crit = |build: Build| -> (f32, String) {
        let mut config = MatchConfig::default();
        config.team1[0] = Some(CharacterClass::Warlock);
        config.team1_builds[0] = build;
        let sheet = Sheet::open(
            CharacterClass::Warlock,
            config,
            ViewCombatantData {
                abilities: &abilities,
                items: &items,
                default_loadouts: &defaults,
                class_icons: &class_icons,
                ability_icons: None,
                item_icons: None,
                pet_icons: None,
                talents: &fixture,
            },
        );
        let note = sheet
            .last
            .notes
            .iter()
            .find_map(|n| n.strip_prefix("stat Crit Chance="))
            .unwrap_or_else(|| panic!("no crit row: {:?}", sheet.last.notes))
            .to_string();
        let (value, hover) = note.split_once(" hover=").expect("a hover");
        let value: f32 = value
            .strip_suffix('%')
            .expect("a percentage")
            .parse()
            .expect("a number");
        (value, hover.to_string())
    };
    let (geared, hover) = crit(Build::default());
    assert!(!hover.contains("from talents"), "{hover:?}");
    let (talented, hover) = crit(Build::of(&[(TalentId::DemonicEmbrace, 2)]));
    assert!(
        (talented - (geared + 10.0)).abs() < 0.051,
        "{geared}% + 2 x 5% is not {talented}%"
    );
    assert!(hover.ends_with("+ 10.0% from talents"), "{hover:?}");
}

/// The notes a Warlock's sheet leaves with `build`, over the shipped talents
/// with Demonic Embrace doing `per_rank` (and declaring a rank's price).
fn embrace_sheet_notes(per_rank: &str, build: Build) -> Vec<String> {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads");
    let health = "per_rank: [StatPercent(MaxHealth, 0.03)],";
    assert!(text.contains(health));
    let fixture = TalentDefinitions::from_ron(
        &text.replacen(
            health,
            &format!("budget: Some(11.0), per_rank: [{per_rank}],"),
            1,
        ),
        &abilities,
    )
    .expect("the fixture validates");
    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Warlock);
    config.team1_builds[0] = build;
    let sheet = Sheet::open(
        CharacterClass::Warlock,
        config,
        ViewCombatantData {
            abilities: &abilities,
            items: &items,
            default_loadouts: &defaults,
            class_icons: &class_icons,
            ability_icons: None,
            item_icons: None,
            pet_icons: None,
            talents: &fixture,
        },
    );
    sheet.last.notes.clone()
}

/// The note `prefix<key>=` left, its value (`None`: no such row).
fn note_value<'a>(notes: &'a [String], prefix: &str, key: &str) -> Option<&'a str> {
    let head = format!("{prefix} {key}=");
    notes.iter().find_map(|n| n.strip_prefix(head.as_str()))
}

/// A build's points draw every kind of stats row as the spawn applies them,
/// and in the bonus colour even where the gear added nothing: the Weapon
/// Damage row ("+3", and "+3 +10%" beside a percentage), a raised row
/// (Attack Power, which the Warlock's gear leaves alone) and a stat line
/// (Fire Resist, drawn from zero). Control: the empty build draws no Weapon
/// Damage or Fire Resist row and Attack Power in plain colour.
#[test]
fn stat_add_rows_print_their_points_in_the_bonus_colour() {
    let points = "StatAdd(WeaponDamage, 3.0), StatAdd(AttackPower, 10.0), \
                  StatAdd(FireResistance, 5.0)";
    let untalented = embrace_sheet_notes(points, Build::default());
    assert_eq!(note_value(&untalented, "stat", "Weapon Damage"), None);
    assert_eq!(note_value(&untalented, "stat", "Fire Resist"), None);
    assert_eq!(
        note_value(&untalented, "tone", "Attack Power"),
        Some("plain"),
        "the Warlock's gear adds no attack power"
    );
    let base_ap: i32 = note_value(&untalented, "stat", "Attack Power")
        .expect("an Attack Power row")
        .parse()
        .expect("a number");

    let talented = embrace_sheet_notes(points, Build::of(&[(TalentId::DemonicEmbrace, 1)]));
    assert_eq!(note_value(&talented, "stat", "Weapon Damage"), Some("+3"));
    assert_eq!(
        note_value(&talented, "tone", "Weapon Damage"),
        Some("bonus")
    );
    assert_eq!(
        note_value(&talented, "stat", "Attack Power"),
        Some((base_ap + 10).to_string().as_str())
    );
    assert_eq!(note_value(&talented, "tone", "Attack Power"), Some("bonus"));
    assert_eq!(
        note_value(&talented, "stat", "Fire Resist"),
        Some("5 hover=0 base + 0 from equipment + 5 from talents (1.6% damage reduction)")
    );
    assert_eq!(note_value(&talented, "tone", "Fire Resist"), Some("bonus"));

    let both = embrace_sheet_notes(
        "StatAdd(WeaponDamage, 3.0), StatPercent(WeaponDamage, 0.1)",
        Build::of(&[(TalentId::DemonicEmbrace, 1)]),
    );
    assert_eq!(note_value(&both, "stat", "Weapon Damage"), Some("+3 +10%"));
    assert_eq!(note_value(&both, "tone", "Weapon Damage"), Some("bonus"));
}

/// The Paladin's aura choice offers Retribution Aura only to a build that
/// takes its talent, and a choice of it the build no longer offers goes back
/// to Devotion Aura as the sheet draws, so the slot's spawn stays legal.
/// Control: with the talent, the tile shows and the choice stays.
#[test]
fn retribution_aura_is_offered_and_kept_only_with_its_talent() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    for talented in [false, true] {
        let data = ViewCombatantData {
            abilities: &abilities,
            items: &items,
            default_loadouts: &defaults,
            class_icons: &class_icons,
            ability_icons: None,
            item_icons: None,
            pet_icons: None,
            talents: TalentDefinitions::shipped(),
        };
        let mut config = MatchConfig::default();
        config.team1[0] = Some(CharacterClass::Paladin);
        config.team1_paladin_auras[0] = PaladinAura::RetributionAura;
        if talented {
            config.team1_builds[0] =
                Build::of(&[(TalentId::SacredDuty, 2), (TalentId::RetributionAura, 1)]);
        }
        let sheet = Sheet::open(CharacterClass::Paladin, config, data);
        let tile = sheet.last.widget("strategic:RetributionAura").is_some();
        assert_eq!(tile, talented, "talented: {talented}");
        assert!(sheet.last.widget("strategic:DevotionAura").is_some());
        let want = if talented {
            PaladinAura::RetributionAura
        } else {
            PaladinAura::DevotionAura
        };
        assert_eq!(
            sheet.config.team1_paladin_auras[0], want,
            "talented: {talented}"
        );
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
    let size = if common::pinned_catalogue::talents()
        .trees(view.class)
        .is_empty()
    {
        SIZE
    } else {
        TALENTED_SIZE
    };
    snapshot_with(name, view, mock_config(), size);
}

fn snapshot_with(name: &str, view: ViewCombatantState, config: MatchConfig, size: [f32; 2]) {
    let abilities = common::pinned_catalogue::abilities();
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let mut config = config;
    let mut picker = EquipmentPickerState::default();

    let mut harness = Harness::builder().with_size(size).build(move |ctx| {
        arenasim::ui::install(ctx);
        let data = ViewCombatantData {
            abilities: &abilities,
            items: &items,
            default_loadouts: &defaults,
            class_icons: &class_icons,
            ability_icons: None,
            item_icons: None,
            pet_icons: None,
            talents: common::pinned_catalogue::talents(),
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
