//! Offscreen snapshot of the component gallery: every `ui::widgets`
//! component, in every state it can be drawn in, on one page.
//!
//! A token or component change shows up here in ONE image, rather than
//! scattered across every screen baseline that happens to use it. A new
//! component joins this page in the same commit that adds it
//! (`src/ui/widgets/mod.rs`). Hovered and pressed states are pinned through
//! each component's `preview` builder, so the page needs no pointer.
//!
//! The approved visual reference is the bench,
//! `docs/design/benches/2026-10-11-ui-design-system-bench.html`; read a new
//! render against it before blessing.
//!
//! ## Loop
//! ```bash
//! # Render; writes tests/snapshots/ui_gallery.new.png on any difference
//! cargo test --release --test ui_gallery_snapshot -- --ignored
//! # Bless once every visible difference is one you intended:
//! UPDATE_SNAPSHOTS=1 cargo test --release --test ui_gallery_snapshot -- --ignored
//! ```
//!
//! `#[ignore]` keeps it out of the default `cargo test` run because it needs a
//! GPU adapter (wgpu), which CI runners may lack. Icons are not loaded (no Bevy
//! textures), so every icon draws its fallback tile — which is itself a
//! component state this page has to show.

use bevy_egui::egui;
use egui_kittest::Harness;

use arenasim::states::encyclopedia::{EncyclopediaData, Topic};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::equipment::load_item_definitions;
use arenasim::states::play_match::AbilityType;
use arenasim::states::GameState;
use arenasim::ui::fonts::body_family;
use arenasim::ui::theme::{class_color, color, data, icon, school_color, size, space, text};
use arenasim::ui::widgets::{
    class_icon, empty_state, link, list_row, paint_icon, panel, panel_header, resource_bar,
    section_label, stat_row, stat_rows, tooltip_frame, Button, Checkbox, Chip, ChoiceTile, Header,
    ListIcon, ListRow, Look, Modal, Nav, Tab, TalentNode, TalentTreeColumn, TalentTrees, Tooltip,
    TreeMode, Variant,
};

const WIDTH: f32 = 1400.0;
const HEIGHT: f32 = 2340.0;
/// Each of the page's two columns.
const COLUMN: f32 = 660.0;

const STATES: [Look; 4] = [Look::Idle, Look::Hovered, Look::Pressed, Look::Disabled];

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn ui_gallery() {
    let items = load_item_definitions().expect("items.ron must load");
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let data = EncyclopediaData {
        items: &items,
        abilities: &abilities,
        item_icons: None,
        class_icons: None,
        ability_icons: None,
        talents: arenasim::states::play_match::talents::TalentDefinitions::shipped(),
    };

    let mut harness = Harness::builder().with_size([WIDTH, HEIGHT]).build(|ctx| {
        arenasim::ui::install(ctx);
        gallery(ctx, &data);
    });
    harness.run();
    harness.snapshot("ui_gallery");
}

fn gallery(ctx: &egui::Context, data: &EncyclopediaData) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(color::BG)
                .inner_margin(egui::Margin::symmetric(space::XL as i8, space::LG as i8)),
        )
        .show(ctx, |ui| {
            let _ =
                Header::new("Component gallery", Nav::Return(GameState::MainMenu)).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("ui_gallery_snapshot")
                            .size(text::SMALL)
                            .color(color::TEXT_DIM),
                    );
                });
            ui.add_space(space::LG);
            talent_trees(ui);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = space::XL;
                column(ui, left_column);
                column(ui, |ui| right_column(ui, data));
            });
        });
}

/// The talent tree, full width, over fixture trees in the full shape the
/// design gives a class (three trees, four tiers, a capstone each), whatever
/// `talents.ron` ships today. One set of trees shows every node state:
///
/// * Affliction, untouched: tier 1 open, the gated tiers locked, an unmet
///   arrow into its capstone;
/// * Demonology, the spec (accent head): taken nodes, a met arrow into a
///   taken capstone, a node hovered, and one rank PENDING above a locked-in
///   floor — the Gauntlet's locked mode;
/// * Destruction, two points in: tier 2 open beside a maxed tier-1 node.
fn talent_trees(ui: &mut egui::Ui) {
    let node =
        |name: &'static str, tier: u8, column: u8, rank: u8, max_rank: u8, open: bool| TalentNode {
            name,
            icon: None,
            tier,
            column,
            rank,
            max_rank,
            pending: 0,
            open,
            capstone: tier == 4,
            requires: None,
            driver_id: None,
        };
    let gates = [0, 2, 3, 5];
    let affliction = TalentTreeColumn {
        name: "Affliction",
        icon: None,
        points: 0,
        leading: false,
        gates,
        nodes: vec![
            node("Improved Drain Life", 1, 0, 0, 2, true),
            node("Improved Curse of Agony", 1, 2, 0, 2, true),
            node("Amplify Curse", 2, 0, 0, 1, false),
            node("Nightfall", 2, 2, 0, 2, false),
            node("Siphon Life", 3, 0, 0, 1, false),
            node("Curse of Exhaustion", 3, 2, 0, 1, false),
            TalentNode {
                requires: Some((4, 1)),
                ..node("Shadow Mastery", 4, 0, 0, 1, false)
            },
        ],
    };
    let demonology = TalentTreeColumn {
        name: "Demonology",
        icon: None,
        points: 6,
        leading: true,
        gates,
        nodes: vec![
            node("Demonic Embrace", 1, 0, 1, 2, true),
            node("Fel Stamina", 1, 2, 2, 2, false),
            TalentNode {
                pending: 1,
                ..node("Unholy Power", 2, 0, 1, 2, true)
            },
            node("Master Demonologist", 2, 2, 0, 1, true),
            node("Demonic Sacrifice", 3, 1, 1, 1, false),
            TalentNode {
                requires: Some((1, 2)),
                ..node("Soul Link", 4, 2, 1, 1, false)
            },
        ],
    };
    let destruction = TalentTreeColumn {
        name: "Destruction",
        icon: None,
        points: 2,
        leading: false,
        gates,
        nodes: vec![
            node("Improved Shadow Bolt", 1, 0, 0, 2, true),
            node("Bane", 1, 2, 2, 2, false),
            node("Aftermath", 2, 0, 0, 2, true),
            node("Shadowburn", 2, 2, 0, 1, true),
            node("Improved Immolate", 3, 1, 0, 1, false),
            TalentNode {
                requires: Some((4, 1)),
                ..node("Conflagrate", 4, 1, 0, 1, false)
            },
        ],
    };
    let trees = [affliction, demonology, destruction];
    section(
        ui,
        "Talent trees (fixture): untouched / the spec, one rank pending, a node hovered / two points in",
        |ui| {
            let _ = TalentTrees::new("gallery", &trees, TreeMode::Spend)
                .preview_hover(1, 3)
                .show(ui);
        },
    );
}

fn column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(COLUMN, ui.available_height()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(COLUMN);
            add(ui);
        },
    );
}

fn section(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    section_label(ui, label);
    add(ui);
    ui.add_space(space::XL);
}

/// A caption naming the state the thing beside it is drawn in.
fn caption(ui: &mut egui::Ui, text_: &str) {
    ui.label(
        egui::RichText::new(text_.to_uppercase())
            .size(text::MICRO)
            .color(color::TEXT_DIM),
    );
}

fn left_column(ui: &mut egui::Ui) {
    section(
        ui,
        "Screen header: exit, stacked root, stacked deep (hovered)",
        |ui| {
            let _ = Header::new("Match setup", Nav::Exit(GameState::MainMenu)).show(ui, |_| {});
            ui.add_space(space::MD);
            let _ = Header::new(
                "Encyclopedia",
                Nav::Stacked {
                    can_back: false,
                    return_to: GameState::MainMenu,
                },
            )
            .show(ui, |_| {});
            ui.add_space(space::MD);
            let _ = Header::new(
                "Encyclopedia",
                Nav::Stacked {
                    can_back: true,
                    return_to: GameState::Results,
                },
            )
            .preview(Look::Hovered)
            .show(ui, |ui| caption(ui, "trailing slot"));
        },
    );

    section(ui, "Buttons: idle / hovered / pressed / disabled", |ui| {
        let variants = [
            (Variant::Primary, "Start match"),
            (Variant::Secondary, "Secondary"),
            (Variant::Ghost, "Ghost"),
            (Variant::Danger, "Remove"),
            (Variant::Icon, "\u{d7}"),
        ];
        egui::Grid::new("buttons")
            .spacing(egui::vec2(space::SM, space::SM))
            .show(ui, |ui| {
                for (variant, label) in variants {
                    for look in STATES {
                        let _ = Button::new(label, variant)
                            .enabled(look != Look::Disabled)
                            .preview(look)
                            .show(ui);
                    }
                    ui.end_row();
                }
            });
    });

    // The Hero template's menu button: pressed draws as hovered, and the
    // ring is the hover state, so idle and hovered are the whole story.
    section(ui, "Hero buttons (main menu): idle / hovered", |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::LG;
            for look in [Look::Idle, Look::Hovered] {
                let _ = Button::new("Match", Variant::Hero).preview(look).show(ui);
            }
        });
    });

    section(
        ui,
        "Compact buttons: primary / secondary / disabled \u{b7} checkboxes",
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                let _ = Button::new("\u{25b6}  Play", Variant::Primary)
                    .compact()
                    .show(ui);
                let _ = Button::new("Pause", Variant::Secondary).compact().show(ui);
                let _ = Button::new("Pause", Variant::Secondary)
                    .compact()
                    .preview(Look::Hovered)
                    .show(ui);
                let _ = Button::new("Step", Variant::Secondary)
                    .compact()
                    .enabled(false)
                    .show(ui);
            });
            ui.add_space(space::SM);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::LG;
                for (on, look, label) in [
                    (true, Look::Idle, "On"),
                    (false, Look::Idle, "Off"),
                    (false, Look::Hovered, "Hovered"),
                    (true, Look::Disabled, "Disabled on"),
                    (false, Look::Disabled, "Disabled off"),
                ] {
                    let mut on = on;
                    let _ = Checkbox::new(&mut on, label)
                        .enabled(look != Look::Disabled)
                        .preview(look)
                        .show(ui);
                }
            });
        },
    );

    section(ui, "Tabs: active / idle / hovered", |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            let _ = Tab::new("Classes").active(true).show(ui);
            let _ = Tab::new("Abilities").show(ui);
            let _ = Tab::new("Buffs & Debuffs").preview(Look::Hovered).show(ui);
            let _ = Tab::new("Items").show(ui);
        });
    });

    section(
        ui,
        "Chips: on / idle / hovered / disabled / school-coloured",
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                let _ = Chip::new("All").on(true).show(ui);
                let _ = Chip::new("Warrior").show(ui);
                let _ = Chip::new("Mage").preview(Look::Hovered).show(ui);
                let _ = Chip::new("Druid").enabled(false).show(ui);
                for school in [SpellSchool::Fire, SpellSchool::Frost, SpellSchool::Shadow] {
                    let _ = Chip::new(&format!("{school:?}"))
                        .label_color(school_color(school))
                        .show(ui);
                }
            });
        },
    );

    section(
        ui,
        "List rows: idle / hovered / selected / disabled / one line",
        |ui| {
            panel(None).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let class_row = |class: CharacterClass, sub: &'static str, meta: &'static str| {
                    ListRow::new(class.name())
                        .name_color(class_color(class))
                        .subtitle(sub)
                        .meta(meta)
                        .icon(ListIcon {
                            texture: None,
                            edge: class_color(class),
                        })
                };
                let _ = list_row(
                    ui,
                    class_row(CharacterClass::Warrior, "Sturdy melee fighter", "Rage"),
                );
                let _ = list_row(
                    ui,
                    class_row(CharacterClass::Mage, "Powerful spellcaster", "Mana")
                        .preview(Look::Hovered),
                );
                let _ = list_row(
                    ui,
                    class_row(CharacterClass::Paladin, "Holy warrior and healer", "Mana")
                        .selected(true),
                );
                let _ = list_row(
                    ui,
                    class_row(CharacterClass::Druid, "Already on the other team", "Mana")
                        .enabled(false),
                );
                let _ = list_row(
                    ui,
                    ListRow::new("Frostbolt").meta("1.5s").icon(ListIcon {
                        texture: None,
                        edge: color::LINE,
                    }),
                );
            });
        },
    );

    section(
        ui,
        "Choice tiles: selected / idle / hovered / disabled",
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                let _ = ChoiceTile::new("Frost Armor").selected(true).show(ui);
                let _ = ChoiceTile::new("Mage Armor").show(ui);
                let _ = ChoiceTile::new("Shadow Resistance Aura")
                    .preview(Look::Hovered)
                    .show(ui);
                let _ = ChoiceTile::new("Molten Armor").enabled(false).show(ui);
            });
        },
    );

    section(ui, "Panels: plain + stat rows, team accent", |ui| {
        panel(None).show(ui, |ui| {
            ui.set_width(ui.available_width());
            panel_header(ui, "Frostbolt", school_color(SpellSchool::Frost), |ui| {
                let _ = Chip::new("Mage").show(ui);
            });
            stat_rows(
                ui,
                [
                    ("Cast time", "1.5 sec"),
                    ("Range", "40 yd"),
                    ("Cost", "20 Mana"),
                    ("Damage", "10 \u{2013} 15 + 60% of spell power"),
                ],
            );
            // A value that carries meaning: a stat the worn gear raised.
            let _ = stat_row(ui, "Attack Power", "312", data::ITEM_BONUS);
        });
        ui.add_space(space::MD);
        panel(Some(data::TEAM_1)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            panel_header(ui, "Team 1", data::TEAM_1, |ui| {
                let _ = Button::new("+", Variant::Icon).show(ui);
                ui.label(egui::RichText::new("2").size(text::BODY).color(color::TEXT));
                let _ = Button::new("\u{2212}", Variant::Icon).show(ui);
            });
            let _ = list_row(
                ui,
                ListRow::new(CharacterClass::Warlock.name())
                    .name_color(class_color(CharacterClass::Warlock))
                    .meta("Mana")
                    .icon(ListIcon {
                        texture: None,
                        edge: class_color(CharacterClass::Warlock),
                    }),
            );
        });
    });
}

fn right_column(ui: &mut egui::Ui, data: &EncyclopediaData) {
    section(ui, "Tooltip", |ui| {
        tooltip_frame().show(ui, |ui| {
            Tooltip::new("Rogue")
                .title_color(class_color(CharacterClass::Rogue))
                .meta("Energy \u{b7} 275 health")
                .body("Swift shadow striker. Opens from stealth with Ambush or Cheap Shot.")
                .hint("Click to open in the encyclopedia")
                .ui(ui);
        });
    });

    section(
        ui,
        "Linked icons (fallback tiles: no textures here)",
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::LG;
                for topic in [
                    Topic::Ability(AbilityType::Shadowbolt),
                    Topic::Ability(AbilityType::Corruption),
                    Topic::Class(CharacterClass::Druid),
                ] {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::XS;
                        let (rect, response) = ui
                            .allocate_exact_size(egui::Vec2::splat(icon::SM), egui::Sense::click());
                        paint_icon(ui.painter(), rect, topic, data);
                        let _ = link(response, topic, data);
                        ui.label(
                            egui::RichText::new(topic.name(data))
                                .size(text::BODY)
                                .color(color::LINK),
                        );
                    });
                }
            });
        },
    );

    section(ui, "Empty state: with an action / without", |ui| {
        let _ = empty_state(
            ui,
            "gallery_search_empty",
            "No matches",
            "Nothing matches \u{201c}frostbolt rank 9\u{201d}. Clear the search or a filter.",
            Some("Clear filters"),
        );
        ui.add_space(space::MD);
        let _ = empty_state(
            ui,
            "gallery_slot_empty",
            "No item for this slot",
            "Nothing in the armory fits this socket for this class.",
            None,
        );
    });

    section(ui, "Modal (card over the scrim)", |ui| {
        egui::Frame::new()
            .fill(color::SCRIM)
            .corner_radius(arenasim::ui::theme::radius::MD)
            .inner_margin(egui::Margin::same(space::XL as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    let _ = Modal::new("gallery_modal", "Select: Trinket 2")
                        .width(size::MODAL_MAX_WIDTH)
                        .footer(|ui| {
                            let _ = Button::new("Equip", Variant::Secondary).show(ui);
                            let _ = Button::new("Cancel", Variant::Ghost).show(ui);
                        })
                        .show_card(ui, |ui| {
                            let _ = list_row(
                                ui,
                                ListRow::new("Dragonspine Trophy \u{2014} +4 AP")
                                    .subtitle(
                                        "Chance on a melee hit (15%) to gain +20% attack speed \
                                         for 10s. Cannot occur more than once every 45s.",
                                    )
                                    .icon(ListIcon {
                                        texture: None,
                                        edge: color::LINE,
                                    })
                                    .wrap(),
                            );
                            let _ = list_row(
                                ui,
                                ListRow::new("Mark of the Champion \u{2014} +6 AP, +4 SP")
                                    .name_color(color::ACCENT)
                                    .icon(ListIcon {
                                        texture: None,
                                        edge: color::LINE,
                                    })
                                    .selected(true)
                                    .wrap(),
                            );
                            let _ = list_row(
                                ui,
                                ListRow::new("Whetstone of Fury \u{2014} +12 HP")
                                    .icon(ListIcon {
                                        texture: None,
                                        edge: color::LINE,
                                    })
                                    .wrap(),
                            );
                        });
                });
            });
    });

    section(ui, "Bars and class icons (fallback)", |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            for class in CharacterClass::all() {
                let _ = class_icon(ui, *class, None, icon::MD);
            }
        });
        ui.add_space(space::SM);
        // On a panel, so the empty track has a ground to read against.
        panel(None).show(ui, |ui| {
            for (fraction, fill) in [
                (1.0, data::HEALING),
                (0.62, data::TEAM_1),
                (0.3, data::DAMAGE),
                (0.0, data::TAKEN),
            ] {
                let _ = resource_bar(ui, fraction, fill, COLUMN / 2.0);
            }
        });
    });

    section(ui, "Type scale", |ui| {
        for (name, size_) in [
            ("Display 42", text::DISPLAY),
            ("Title 30", text::TITLE),
            ("H2 20", text::H2),
            ("H3 16", text::H3),
            ("Body 14", text::BODY),
            ("Small 12", text::SMALL),
            ("Micro 10", text::MICRO),
        ] {
            ui.label(egui::RichText::new(name.to_uppercase()).size(size_).color(
                if size_ >= text::TITLE {
                    color::ACCENT
                } else {
                    color::TEXT
                },
            ));
        }
    });

    section(
        ui,
        "Prose: Rajdhani Bold vs the Body family (Regular)",
        |ui| {
            panel(None).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let prose = "Deals 40-45 Frost damage and slows the target's movement speed by 30% for 5 sec.";
            ui.label(egui::RichText::new(prose).size(text::BODY).color(color::TEXT));
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new(prose)
                    .size(text::BODY)
                    .family(body_family(ui.ctx()))
                    .color(color::TEXT),
            );
        });
        },
    );
}
