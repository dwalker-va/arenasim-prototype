//! The talent panel: a class's trees from `talents.ron`, drawn with the kit's
//! [`TalentTrees`] and spent against a [`Build`].
//!
//! Two surfaces use it. View Combatant (Custom Match) SPENDS a build:
//!
//! * a click learns a rank, when the build with it is legal;
//! * a shift-click takes one back, when the build without it is legal (a
//!   rank a higher tier's gate or a prerequisite leans on stays);
//! * a right-click opens the talent's encyclopedia page — the screen's
//!   reference affordance wherever the primary click is the edit, as on its
//!   equipment rows — and the panel says so in its own chrome;
//! * Reset clears the build to its floor.
//!
//! The encyclopedia READS: [`read_trees`] draws every node alike, each a link
//! to its page.
//!
//! **A floor locks spent ranks** (user ruling, 2026-10-12: builds are locked
//! once spent). A Gauntlet run's talent step hands [`spend_panel`] the ranks
//! already locked in as its floor: they cannot be taken back, Reset returns to
//! them, and the ranks above them draw as pending until the run locks them
//! too. Custom Match edits freely and passes no floor.
//!
//! Nothing here decides what is legal: [`TalentDefinitions::learn`] and
//! [`TalentDefinitions::unlearn`] do, and the panel offers only what they
//! allow, so a spawn never meets a build the panel made illegal.

use bevy_egui::egui;

use super::encyclopedia::{talents as encyclopedia_talents, EncyclopediaData, Topic};
use super::match_config::CharacterClass;
use super::play_match::talents::{
    Build, TalentDefinitions, TalentId, MAX_TALENT_POINTS, TIER_OPENS_AT,
};
use super::talent_text;
use crate::ui::driver as ui_driver;
use crate::ui::fonts::body_family;
use crate::ui::theme::{color, size, space, text};
use crate::ui::widgets::{
    hint_line, panel, panel_header, Button, TalentNode, TalentTreeColumn, TalentTrees, TreeMode,
    Variant,
};

/// What a click on a node asks of the build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeClick {
    Learn,
    Unlearn,
}

/// A class's trees as the kit draws them, and which talent each node is.
///
/// `build` gives the ranks and what is open; `floor`, when there is one, the
/// ranks locked in. In [`TreeMode::Read`] the build is ignored.
pub fn tree_columns<'a>(
    talents: &'a TalentDefinitions,
    class: CharacterClass,
    build: &Build,
    floor: Option<&Build>,
    data: &EncyclopediaData,
    driver_ids: bool,
) -> (Vec<TalentTreeColumn<'a>>, Vec<Vec<TalentId>>) {
    let points = talents.tree_points(class, build);
    // The tree (or, in a hybrid, the trees) with the most points names the
    // spec, and its head is drawn in accent.
    let most = points.iter().map(|(_, p)| *p).max().unwrap_or(0);
    let mut columns = Vec::new();
    let mut ids = Vec::new();
    for tree in talents.trees(class) {
        let mut nodes: Vec<(TalentId, &crate::states::play_match::talents::TalentConfig)> = talents
            .talents_for_class(class)
            .filter(|(_, t)| t.tree == tree.id)
            .collect();
        nodes.sort_by_key(|(_, t)| (t.tier, t.column));
        let index_of = |id: TalentId| nodes.iter().position(|(n, _)| *n == id);
        let column_nodes = nodes
            .iter()
            .map(|(id, t)| {
                let rank = build.rank(*id);
                let locked_in = floor.map_or(rank, |f| f.rank(*id).min(rank));
                TalentNode {
                    name: t.name.as_str(),
                    icon: Topic::Talent(*id).icon(data),
                    tier: t.tier,
                    column: t.column,
                    rank,
                    max_rank: t.max_rank,
                    pending: rank - locked_in,
                    open: talents.learn_blocker(class, build, *id).is_none(),
                    capstone: t.tier == crate::states::play_match::talents::CAPSTONE_TIER,
                    requires: t
                        .requires
                        .and_then(|(needed, r)| index_of(needed).map(|i| (i, r))),
                    driver_id: driver_ids.then(|| format!("talent:{}", id.as_str())),
                }
            })
            .collect();
        let tree_points = points
            .iter()
            .find(|(id, _)| *id == tree.id)
            .map_or(0, |(_, p)| *p);
        columns.push(TalentTreeColumn {
            name: tree.name.as_str(),
            icon: data.ability_icons.and_then(|icons| {
                icons
                    .textures
                    .get(&encyclopedia_talents::tree_icon_key(class, tree.id))
                    .copied()
            }),
            points: tree_points,
            leading: most > 0 && tree_points == most,
            gates: TIER_OPENS_AT,
            nodes: column_nodes,
        });
        ids.push(nodes.iter().map(|(id, _)| *id).collect());
    }
    (columns, ids)
}

/// Apply a click on `talent` to `build`, within `floor`: a learn when the
/// build with the rank is legal, an unlearn when the build without it is and
/// the rank is above the floor. Returns whether the build changed.
pub fn apply_click(
    talents: &TalentDefinitions,
    class: CharacterClass,
    build: &mut Build,
    floor: Option<&Build>,
    talent: TalentId,
    click: NodeClick,
) -> bool {
    let next = match click {
        NodeClick::Learn => talents.learn(class, build, talent).ok(),
        NodeClick::Unlearn => {
            let locked = floor.map_or(0, |f| f.rank(talent));
            if build.rank(talent) <= locked {
                None
            } else {
                talents.unlearn(class, build, talent)
            }
        }
    };
    match next {
        Some(next) => {
            *build = next;
            true
        }
        None => false,
    }
}

/// The talent panel that SPENDS `build`: its head (the spec, the points and
/// Reset), the trees, and the line saying what each click does. Returns the
/// talent whose encyclopedia page was asked for (a right-click).
///
/// `driver_key` names this panel's `--ui-script` note (`talents:<key>`).
pub fn spend_panel(
    ui: &mut egui::Ui,
    class: CharacterClass,
    build: &mut Build,
    floor: Option<&Build>,
    data: &EncyclopediaData,
    driver_key: &str,
) -> Option<Topic> {
    let talents = data.talents;
    let mut open = None;
    panel(None).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let spent = build.points();
        let mut reset = false;
        panel_header(ui, "Talents", color::ACCENT, |ui| {
            let floor_points = floor.map_or(0, Build::points);
            let can_reset = spent > floor_points;
            let button = Button::new("Reset", Variant::Ghost)
                .compact()
                .enabled(can_reset)
                .driver_id("talents:reset")
                .show(ui);
            if button.clicked() {
                reset = true;
            }
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new(format!("{spent} / {MAX_TALENT_POINTS} points"))
                    .size(text::BODY)
                    .color(color::TEXT_MUTED),
            );
        });
        if reset {
            *build = floor.cloned().unwrap_or_default();
        }

        let spent = build.points();
        let spec = talents.spec(class, build);
        let (spec_text, spec_color) = match &spec {
            Some(spec) => (spec.line(), color::ACCENT),
            None => ("No talents learned".to_string(), color::TEXT_DIM),
        };
        ui.label(
            egui::RichText::new(spec_text.as_str())
                .size(text::H3)
                .color(spec_color),
        );
        ui_driver::note(
            ui,
            format_args!("talents:{driver_key} points={spent} spec={spec_text}"),
        );
        ui.add_space(space::SM);

        let (columns, ids) = tree_columns(talents, class, build, floor, data, true);
        let responses = TalentTrees::new("spend", &columns, TreeMode::Spend).show(ui);
        let mut click = None;
        for r in responses {
            let id = ids[r.tree][r.node];
            let response = r.response.on_hover_ui(|ui| {
                ui_driver::note(ui, format_args!("tooltip:Talent({id:?})"));
                spend_tooltip(ui, class, id, build, floor, data);
            });
            if response.secondary_clicked() {
                open = Some(Topic::Talent(id));
            } else if response.clicked() {
                let unlearn = ui.input(|i| i.modifiers.shift);
                let kind = if unlearn {
                    NodeClick::Unlearn
                } else {
                    NodeClick::Learn
                };
                click = Some((id, kind));
            }
        }
        if let Some((id, kind)) = click {
            apply_click(talents, class, build, floor, id, kind);
        }

        ui.add_space(space::SM);
        hint_line(
            ui,
            "Click learns a rank \u{b7} shift-click takes one back \u{b7} right-click opens the encyclopedia",
        );
        hint_line(
            ui,
            &format!(
                "A tier opens at {} points in the tiers above it",
                gate_list()
            ),
        );
    });
    open
}

/// "2, 3 and 5": the points each tier past the first needs.
fn gate_list() -> String {
    let gates: Vec<String> = TIER_OPENS_AT
        .iter()
        .filter(|g| **g > 0)
        .map(u32::to_string)
        .collect();
    match gates.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

/// The tooltip a node shows while a build is being spent: what it does now
/// and at its next rank, and why it cannot take one if it cannot.
fn spend_tooltip(
    ui: &mut egui::Ui,
    class: CharacterClass,
    id: TalentId,
    build: &Build,
    floor: Option<&Build>,
    data: &EncyclopediaData,
) {
    ui.set_max_width(size::TOOLTIP_MAX_WIDTH);
    let talents = data.talents;
    let Some(talent) = talents.get(id) else {
        return;
    };
    let rank = build.rank(id);
    ui.label(
        egui::RichText::new(&talent.name)
            .size(text::BODY)
            .color(color::ACCENT)
            .strong(),
    );
    ui.label(
        egui::RichText::new(format!(
            "{} \u{b7} rank {rank}/{}",
            talent_text::placement(talents, talent),
            talent.max_rank
        ))
        .size(text::SMALL)
        .color(color::TEXT_MUTED),
    );
    ui.add_space(space::XXS);
    let prose = |ui: &mut egui::Ui, line: String, line_color: egui::Color32| {
        ui.label(
            egui::RichText::new(line)
                .family(body_family(ui.ctx()))
                .size(text::SMALL)
                .color(line_color),
        );
    };
    if rank > 0 {
        prose(
            ui,
            talent_text::rank_text(talent, rank, data.abilities),
            color::TEXT,
        );
    }
    if rank < talent.max_rank {
        let next = talent_text::rank_text(talent, rank + 1, data.abilities);
        let line = if rank == 0 {
            next
        } else {
            format!("Next rank: {next}")
        };
        prose(ui, line, color::TEXT_MUTED);
    }
    ui.add_space(space::XS);
    let action = match talents.learn_blocker(class, build, id) {
        None => "Click to learn".to_string(),
        Some(blocker) => talent_text::blocker_text(talents, class, blocker),
    };
    hint_line(ui, &action);
    let locked = floor.map_or(0, |f| f.rank(id));
    if rank > locked && talents.unlearn(class, build, id).is_some() {
        hint_line(ui, "Shift-click to take a rank back");
    }
}

/// A class's trees as a REFERENCE: every node alike, each a link to its page.
/// `highlight` draws one node as hovered — the page it is on. Returns the
/// talent clicked.
pub fn read_trees(
    ui: &mut egui::Ui,
    class: CharacterClass,
    data: &EncyclopediaData,
    id_salt: &str,
    highlight: Option<TalentId>,
) -> Option<Topic> {
    let (columns, ids) = tree_columns(data.talents, class, &Build::default(), None, data, false);
    if columns.is_empty() {
        return None;
    }
    let mut trees = TalentTrees::new(id_salt, &columns, TreeMode::Read);
    if let Some(highlight) = highlight {
        for (t, tree) in ids.iter().enumerate() {
            if let Some(n) = tree.iter().position(|id| *id == highlight) {
                trees = trees.preview_hover(t, n);
            }
        }
    }
    let mut clicked = None;
    for r in trees.show(ui) {
        let topic = Topic::Talent(ids[r.tree][r.node]);
        if let Some(topic) = crate::ui::widgets::link(r.response, topic, data) {
            clicked = Some(topic);
        }
    }
    clicked
}
