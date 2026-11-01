//! The talent tree: a class's trees side by side, each four tier rows of up
//! to three nodes, with rank pips, prerequisite arrows, a locked state and a
//! spend affordance.
//!
//! The layout follows the talents prototype
//! (`docs/design/benches/2026-10-11-gauntlet-prototype.html`, "Talent
//! screen"): a box per tree, its name and points at the head, the tier rows
//! under it with each row's gate ("2+") in a gutter on its left, and a node
//! tile per talent — its art, its name and a pip per rank. The capstone wears
//! a double edge.
//!
//! A node speaks in the chrome palette and adds no hue (the colour budget):
//!
//! * **taken** — a rank or more learned: a [`color::SELECTED`] fill, an
//!   [`color::ACCENT`] edge, its pips filled in accent;
//! * **open** — can take a rank now: a [`color::LINE_STRONG`] edge over no
//!   fill, its art at full colour;
//! * **locked** — its tier or prerequisite is closed, or the points are
//!   spent: a [`color::LINE`] edge, dim text, its art tinted
//!   [`color::ICON_LOCKED`];
//! * **pending** — in a build whose spent ranks are locked in (a Gauntlet
//!   run's), a rank taken this visit and not yet locked: a pip ringed in
//!   accent rather than filled. A Custom Match build has no floor, so every
//!   rank it takes is drawn filled.
//!
//! A prerequisite arrow runs from the node it needs to the node that needs
//! it, in accent once met and [`color::LINE_STRONG`] until then.
//!
//! The component knows nothing about talents' rules or data: the caller
//! hands it plain [`TalentTreeColumn`]s (whether a node is open, its rank,
//! its prerequisite's index) and reads each node's [`egui::Response`] back to
//! learn a rank, show a tooltip or open a page. That is what lets the gallery
//! draw the full three-tree shape from fixture trees, whatever the shipped
//! `talents.ron` holds.

use bevy_egui::egui;

use super::bars::icon_tile;
use super::{fitted, mark};
use crate::ui::theme::{color, icon, radius, size, space, stroke, text};

/// Every tree has four tiers; the fourth is the capstone.
pub const TIERS: usize = 4;

/// Nodes a tier row holds side by side.
pub const COLUMNS: usize = 3;

/// The most lines a node's name runs to before it ends in an ellipsis.
const NAME_ROWS: usize = 2;

/// The whole of a texture, as UV coordinates.
const FULL_UV: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

/// One talent node, as the tree draws it.
#[derive(Debug, Clone)]
pub struct TalentNode<'a> {
    pub name: &'a str,
    pub icon: Option<egui::TextureId>,
    /// 1 to [`TIERS`].
    pub tier: u8,
    /// 0 to `COLUMNS - 1`.
    pub column: u8,
    pub rank: u8,
    pub max_rank: u8,
    /// How many of `rank` are pending — taken this visit in a build whose
    /// spent ranks are locked in. 0 for a build with no floor.
    pub pending: u8,
    /// It can take a rank now.
    pub open: bool,
    /// The tree's capstone: drawn with a double edge.
    pub capstone: bool,
    /// The node of the SAME tree it needs (its index in the column's
    /// `nodes`), and the rank it needs there.
    pub requires: Option<(usize, u8)>,
    /// The `--ui-script` id it registers under.
    pub driver_id: Option<String>,
}

/// One tree: its head and its nodes.
#[derive(Debug, Clone)]
pub struct TalentTreeColumn<'a> {
    pub name: &'a str,
    pub icon: Option<egui::TextureId>,
    /// The points the build spends in this tree.
    pub points: u32,
    /// The tree names the spec: its head is drawn in accent.
    pub leading: bool,
    /// The points each tier needs in the tiers below it before it opens.
    pub gates: [u32; TIERS],
    pub nodes: Vec<TalentNode<'a>>,
}

impl TalentTreeColumn<'_> {
    /// The points the column's nodes hold in the tiers below `tier`.
    fn points_below(&self, tier: u8) -> u32 {
        self.nodes
            .iter()
            .filter(|n| n.tier < tier)
            .map(|n| u32::from(n.rank))
            .sum()
    }
}

/// What a tree is drawn for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeMode {
    /// A build being spent: nodes show taken, open and locked, and an open
    /// node wears the pointing hand.
    Spend,
    /// A reference (the encyclopedia): every node drawn alike, each a link.
    Read,
}

/// The response for one node, addressed by tree and node index.
pub struct TalentNodeResponse {
    pub tree: usize,
    pub node: usize,
    pub response: egui::Response,
}

/// A set of trees, configured. Draw it with [`TalentTrees::show`].
#[must_use = "call `.show(ui)` to draw the trees"]
pub struct TalentTrees<'a> {
    trees: &'a [TalentTreeColumn<'a>],
    mode: TreeMode,
    id_salt: &'a str,
    preview_hover: Option<(usize, usize)>,
}

impl<'a> TalentTrees<'a> {
    /// `id_salt` keeps two sets of trees on one screen apart.
    pub fn new(id_salt: &'a str, trees: &'a [TalentTreeColumn<'a>], mode: TreeMode) -> Self {
        Self {
            trees,
            mode,
            id_salt,
            preview_hover: None,
        }
    }

    /// Draw node `node` of tree `tree` as hovered, whatever the pointer
    /// does: the gallery's seam.
    pub fn preview_hover(mut self, tree: usize, node: usize) -> Self {
        self.preview_hover = Some((tree, node));
        self
    }

    /// One node's height: its art, two name lines and its pips.
    pub fn node_height(ui: &egui::Ui) -> f32 {
        let name_h = ui.fonts(|f| f.row_height(&name_font())) * NAME_ROWS as f32;
        space::XS + icon::LG + space::XS + name_h + space::XS + size::PIP + space::SM
    }

    /// A tree's height, head to foot.
    pub fn tree_height(ui: &egui::Ui) -> f32 {
        let rows = TIERS as f32;
        space::SM
            + head_height(ui)
            + space::SM
            + rows * Self::node_height(ui)
            + (rows - 1.0) * space::SM
            + space::SM
    }

    pub fn show(self, ui: &mut egui::Ui) -> Vec<TalentNodeResponse> {
        let count = self.trees.len().max(1) as f32;
        let width = ui.available_width();
        // A tree grows no wider than its nodes at their widest need, so a
        // class with fewer than three trees keeps the lattice's proportions;
        // the set starts at the left edge, as the page's text does.
        let widest = 2.0 * space::SM
            + gutter_width(ui)
            + COLUMNS as f32 * size::TALENT_NODE_MAX
            + (COLUMNS as f32 - 1.0) * space::SM;
        let tree_w = ((width - (count - 1.0) * space::MD) / count).min(widest);
        let tree_h = Self::tree_height(ui);
        let (area, _) = ui.allocate_exact_size(egui::vec2(width, tree_h), egui::Sense::hover());

        let mut out = Vec::new();
        for (t, tree) in self.trees.iter().enumerate() {
            let left = area.left() + t as f32 * (tree_w + space::MD);
            let rect =
                egui::Rect::from_min_size(egui::pos2(left, area.top()), egui::vec2(tree_w, tree_h));
            self.tree(ui, t, tree, rect, &mut out);
        }
        out
    }

    fn tree(
        &self,
        ui: &mut egui::Ui,
        t: usize,
        tree: &TalentTreeColumn,
        rect: egui::Rect,
        out: &mut Vec<TalentNodeResponse>,
    ) {
        let painter = ui.painter_at(rect.expand(stroke::MARK));
        painter.rect(
            rect,
            radius::MD,
            color::BG,
            egui::Stroke::new(stroke::HAIRLINE, color::LINE),
            egui::StrokeKind::Inside,
        );

        // The head: the tree's art and name, its points at the right.
        let inner = rect.shrink(space::SM);
        let head_h = head_height(ui);
        let head = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), head_h));
        let art = egui::Rect::from_min_size(
            egui::pos2(head.left(), head.center().y - icon::SM / 2.0),
            egui::Vec2::splat(icon::SM),
        );
        icon_tile(&painter, art, tree.icon, color::LINE);
        // A reference has no build, so it shows no points.
        let points_text = match self.mode {
            TreeMode::Spend => format!("{} pts", tree.points),
            TreeMode::Read => String::new(),
        };
        let points = super::galley(ui, points_text, text::SMALL, color::TEXT_MUTED);
        let points_w = points.size().x;
        painter.galley(
            egui::pos2(
                head.right() - points_w,
                head.center().y - points.size().y / 2.0,
            ),
            points,
            color::TEXT_MUTED,
        );
        let name_color = if tree.leading {
            color::ACCENT
        } else {
            color::TEXT
        };
        let name_left = art.right() + space::SM;
        let name = fitted(
            ui,
            tree.name,
            egui::FontId::proportional(text::H3),
            name_color,
            (head.right() - points_w - space::SM - name_left).max(space::XL),
        );
        painter.galley(
            egui::pos2(name_left, head.center().y - name.size().y / 2.0),
            name,
            name_color,
        );

        // The grid: a gate gutter, then three node columns, centred in what
        // is left when a node reaches its widest.
        let gutter = gutter_width(ui);
        let grid_left = inner.left() + gutter;
        let grid_w = inner.right() - grid_left;
        let columns = COLUMNS as f32;
        let node_w = ((grid_w - (columns - 1.0) * space::SM) / columns).min(size::TALENT_NODE_MAX);
        let used = columns * node_w + (columns - 1.0) * space::SM;
        let grid_left = grid_left + (grid_w - used) / 2.0;
        let node_h = Self::node_height(ui);
        let rows_top = head.bottom() + space::SM;

        let row_top = |tier: u8| rows_top + f32::from(tier - 1) * (node_h + space::SM);
        let node_rect = |node: &TalentNode| {
            egui::Rect::from_min_size(
                egui::pos2(
                    grid_left + f32::from(node.column) * (node_w + space::SM),
                    row_top(node.tier.clamp(1, TIERS as u8)),
                ),
                egui::vec2(node_w, node_h),
            )
        };

        // Each tier's gate, in the gutter: lit once the tier is open.
        for (i, gate) in tree.gates.iter().enumerate() {
            let tier = i as u8 + 1;
            if *gate == 0 {
                continue;
            }
            let open = self.mode == TreeMode::Read || tree.points_below(tier) >= *gate;
            let gate_color = if open {
                color::TEXT_MUTED
            } else {
                color::TEXT_DIM
            };
            painter.text(
                egui::pos2(inner.left(), row_top(tier) + space::XS),
                egui::Align2::LEFT_TOP,
                format!("{gate}+"),
                egui::FontId::proportional(text::MICRO),
                gate_color,
            );
        }

        // Arrows under the nodes, so a node's tile covers the line's ends.
        for node in &tree.nodes {
            let Some((needed, rank)) = node.requires else {
                continue;
            };
            let Some(from) = tree.nodes.get(needed) else {
                continue;
            };
            let met = self.mode == TreeMode::Read || from.rank >= rank;
            let arrow_color = if met {
                color::ACCENT
            } else {
                color::LINE_STRONG
            };
            arrow(&painter, node_rect(from), node_rect(node), arrow_color);
        }

        for (n, node) in tree.nodes.iter().enumerate() {
            let rect = node_rect(node);
            let id = ui.id().with((self.id_salt, "talent", t, n));
            let response = ui.interact(rect, id, egui::Sense::click());
            let enabled = match self.mode {
                TreeMode::Spend => node.open,
                TreeMode::Read => true,
            };
            let response = if enabled {
                response.on_hover_cursor(egui::CursorIcon::PointingHand)
            } else {
                response
            };
            mark(ui, rect, enabled, node.driver_id.as_deref());
            let hovered = response.hovered() || self.preview_hover == Some((t, n));
            self.node(ui, &painter, rect, node, hovered);
            out.push(TalentNodeResponse {
                tree: t,
                node: n,
                response,
            });
        }
    }

    fn node(
        &self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        node: &TalentNode,
        hovered: bool,
    ) {
        #[derive(PartialEq)]
        enum State {
            Taken,
            Open,
            Locked,
        }
        let state = match self.mode {
            TreeMode::Read => State::Open,
            TreeMode::Spend if node.rank > 0 => State::Taken,
            TreeMode::Spend if node.open => State::Open,
            TreeMode::Spend => State::Locked,
        };
        let none = egui::Color32::TRANSPARENT;
        let (fill, edge, name_color, tint) = match state {
            State::Taken => (
                color::SELECTED,
                if hovered {
                    color::ACCENT_RAISED
                } else {
                    color::ACCENT
                },
                color::TEXT,
                color::UNTINTED,
            ),
            State::Open => (
                if hovered { color::RAISED } else { none },
                color::LINE_STRONG,
                color::TEXT,
                color::UNTINTED,
            ),
            State::Locked => (
                if hovered { color::RAISED } else { none },
                color::LINE,
                color::TEXT_DIM,
                color::ICON_LOCKED,
            ),
        };
        painter.rect(
            rect,
            radius::MD,
            fill,
            egui::Stroke::new(stroke::HAIRLINE, edge),
            egui::StrokeKind::Inside,
        );
        if node.capstone {
            painter.rect_stroke(
                rect.shrink(space::XXS + stroke::HAIRLINE),
                radius::SM,
                egui::Stroke::new(stroke::HAIRLINE, edge),
                egui::StrokeKind::Inside,
            );
        }

        let art = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + space::XS + icon::LG / 2.0),
            egui::Vec2::splat(icon::LG),
        );
        match node.icon {
            Some(texture) => {
                painter.image(texture, art, FULL_UV, tint);
            }
            None => icon_tile(painter, art, None, edge),
        }

        let name_w = rect.width() - 2.0 * space::XS;
        let mut job =
            egui::text::LayoutJob::simple(node.name.to_owned(), name_font(), name_color, name_w);
        job.wrap.max_rows = NAME_ROWS;
        job.wrap.overflow_character = Some('\u{2026}');
        job.halign = egui::Align::Center;
        let name = ui.fonts(|f| f.layout_job(job));
        painter.galley(
            egui::pos2(rect.center().x, art.bottom() + space::XS),
            name,
            name_color,
        );

        // One pip per rank, centred on the node's foot.
        let ranks = f32::from(node.max_rank);
        let pips_w = ranks * size::PIP + (ranks - 1.0).max(0.0) * space::XS;
        let pip_y = rect.bottom() - space::SM - size::PIP / 2.0;
        let committed = node.rank.saturating_sub(node.pending);
        for i in 0..node.max_rank {
            let center = egui::pos2(
                rect.center().x - pips_w / 2.0
                    + f32::from(i) * (size::PIP + space::XS)
                    + size::PIP / 2.0,
                pip_y,
            );
            let r = size::PIP / 2.0;
            if self.mode == TreeMode::Spend && i < committed {
                painter.circle_filled(center, r, color::ACCENT);
            } else if self.mode == TreeMode::Spend && i < node.rank {
                painter.circle_stroke(
                    center,
                    r,
                    egui::Stroke::new(stroke::HAIRLINE, color::ACCENT),
                );
            } else {
                let ring = if state == State::Locked {
                    color::LINE
                } else {
                    color::LINE_STRONG
                };
                painter.circle_stroke(center, r, egui::Stroke::new(stroke::HAIRLINE, ring));
            }
        }
    }
}

/// A node's name font.
fn name_font() -> egui::FontId {
    egui::FontId::proportional(text::SMALL)
}

/// The gutter left of the tier rows, wide enough for the widest gate ("5+").
fn gutter_width(ui: &egui::Ui) -> f32 {
    super::galley(ui, "5+", text::MICRO, color::TEXT_DIM)
        .size()
        .x
        + space::XS
}

/// A tree head's height: its name at [`text::H3`].
fn head_height(ui: &egui::Ui) -> f32 {
    ui.fonts(|f| f.row_height(&egui::FontId::proportional(text::H3)))
        .max(icon::SM)
}

/// An arrow from `from`'s edge to `to`'s, pointing at `to`.
fn arrow(painter: &egui::Painter, from: egui::Rect, to: egui::Rect, arrow_color: egui::Color32) {
    let start = edge_point(from, to.center());
    let end = edge_point(to, from.center());
    let along = end - start;
    if along.length() <= f32::EPSILON {
        return;
    }
    let dir = along.normalized();
    let line = egui::Stroke::new(stroke::MARK, arrow_color);
    let tip = space::SM;
    let base = end - dir * tip;
    painter.line_segment([start, base], line);
    let side = egui::vec2(-dir.y, dir.x) * (tip / 2.0);
    painter.add(egui::Shape::convex_polygon(
        vec![end, base + side, base - side],
        arrow_color,
        egui::Stroke::NONE,
    ));
}

/// Where the line from `rect`'s centre toward `toward` leaves `rect`.
fn edge_point(rect: egui::Rect, toward: egui::Pos2) -> egui::Pos2 {
    let c = rect.center();
    let d = toward - c;
    let half = rect.size() / 2.0;
    let sx = if d.x.abs() > f32::EPSILON {
        half.x / d.x.abs()
    } else {
        f32::INFINITY
    };
    let sy = if d.y.abs() > f32::EPSILON {
        half.y / d.y.abs()
    } else {
        f32::INFINITY
    };
    let s = sx.min(sy);
    if s.is_finite() {
        c + d * s
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arrow_leaves_and_enters_through_the_facing_edges() {
        let top = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 80.0));
        let below = top.translate(egui::vec2(0.0, 100.0));
        assert_eq!(edge_point(top, below.center()), egui::pos2(40.0, 80.0));
        assert_eq!(edge_point(below, top.center()), egui::pos2(40.0, 100.0));
    }
}
