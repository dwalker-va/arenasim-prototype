//! Per-match choices, and the talents that shape them.
//!
//! A combatant makes its per-match choices before the gates (the Rogue's
//! opener and poison, the Warlock's curses, the Paladin's aura, ...). They are
//! not part of a [`Build`], but a build can shape them, three ways:
//!
//! - **`WidenChoice(option)`** makes an option selectable that is hidden
//!   without it. Which options are hidden is DATA: an option some node of
//!   `talents.ron` widens is offered only to a build that takes that node
//!   ([`TalentDefinitions::offers`]). A menu's default is never hidden.
//! - **`AddChoiceSlot(slot)`** opens a second slot of a menu (Master
//!   Poisoner's off-hand poison). Without it the slot is empty and its menu's
//!   one choice covers both (one poison coats both weapons).
//! - **Choice rules** (`choice_rules` in `talents.ron`, [`ChoiceRule`]) make
//!   choices exclusive: an option can switch a talent's effects off for the
//!   match, forbid an ability, or be the one option an ability requires. A
//!   forbidden ability stays in the combatant's book, locked
//!   ([`AbilityBook::is_locked`](super::ability_book::AbilityBook::is_locked)),
//!   and the cast gate refuses it.
//!
//! Every entrance checks a slot's choices against its build
//! ([`TalentDefinitions::validate_choices`]); a spawn resolves the build with
//! them ([`TalentDefinitions::resolve_with_choices`]). With nothing widened,
//! no slot opened and no rule written, every option is offered, every choice
//! is legal and nothing is locked: the default choices change nothing.
//!
//! [`Build`]: super::talents::Build

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::abilities::AbilityType;
use super::talents::{Build, TalentDefinitions, TalentEffect, TalentId};
use crate::states::match_config::{
    CharacterClass, HunterPetType, MageArmor, MatchConfig, PaladinAura, RogueOpener, RoguePoison,
    WarlockCurse, WarriorShout,
};

/// One per-match menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChoiceMenu {
    RogueOpener,
    RoguePoison,
    WarlockCurse,
    HunterPet,
    WarriorShout,
    MageArmor,
    PaladinAura,
}

impl ChoiceMenu {
    /// The class whose menu this is.
    pub fn class(self) -> CharacterClass {
        match self {
            ChoiceMenu::RogueOpener | ChoiceMenu::RoguePoison => CharacterClass::Rogue,
            ChoiceMenu::WarlockCurse => CharacterClass::Warlock,
            ChoiceMenu::HunterPet => CharacterClass::Hunter,
            ChoiceMenu::WarriorShout => CharacterClass::Warrior,
            ChoiceMenu::MageArmor => CharacterClass::Mage,
            ChoiceMenu::PaladinAura => CharacterClass::Paladin,
        }
    }

    /// The option a slot that chooses nothing gets. Never hidden.
    pub fn default_option(self) -> ChoiceOption {
        match self {
            ChoiceMenu::RogueOpener => ChoiceOption::RogueOpener(RogueOpener::default()),
            ChoiceMenu::RoguePoison => ChoiceOption::RoguePoison(RoguePoison::default()),
            ChoiceMenu::WarlockCurse => ChoiceOption::WarlockCurse(WarlockCurse::default()),
            ChoiceMenu::HunterPet => ChoiceOption::HunterPet(HunterPetType::default()),
            ChoiceMenu::WarriorShout => ChoiceOption::WarriorShout(WarriorShout::default()),
            ChoiceMenu::MageArmor => ChoiceOption::MageArmor(MageArmor::default()),
            ChoiceMenu::PaladinAura => ChoiceOption::PaladinAura(PaladinAura::default()),
        }
    }

    /// Every option of the menu, in display order, hidden ones included.
    pub fn options(self) -> Vec<ChoiceOption> {
        match self {
            ChoiceMenu::RogueOpener => RogueOpener::ALL
                .map(ChoiceOption::RogueOpener)
                .to_vec(),
            ChoiceMenu::RoguePoison => RoguePoison::ALL
                .map(ChoiceOption::RoguePoison)
                .to_vec(),
            ChoiceMenu::WarlockCurse => WarlockCurse::ALL
                .map(ChoiceOption::WarlockCurse)
                .to_vec(),
            ChoiceMenu::HunterPet => HunterPetType::ALL.map(ChoiceOption::HunterPet).to_vec(),
            ChoiceMenu::WarriorShout => WarriorShout::ALL
                .map(ChoiceOption::WarriorShout)
                .to_vec(),
            ChoiceMenu::MageArmor => MageArmor::ALL.map(ChoiceOption::MageArmor).to_vec(),
            ChoiceMenu::PaladinAura => PaladinAura::ALL
                .map(ChoiceOption::PaladinAura)
                .to_vec(),
        }
    }

    /// Every menu.
    pub const ALL: [ChoiceMenu; 7] = [
        ChoiceMenu::RogueOpener,
        ChoiceMenu::RoguePoison,
        ChoiceMenu::WarlockCurse,
        ChoiceMenu::HunterPet,
        ChoiceMenu::WarriorShout,
        ChoiceMenu::MageArmor,
        ChoiceMenu::PaladinAura,
    ];
}

/// One option of one per-match menu. The variant names the menu, so
/// `talents.ron` writes `WidenChoice(WarlockCurse(Tongues))`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChoiceOption {
    RogueOpener(RogueOpener),
    RoguePoison(RoguePoison),
    WarlockCurse(WarlockCurse),
    HunterPet(HunterPetType),
    WarriorShout(WarriorShout),
    MageArmor(MageArmor),
    PaladinAura(PaladinAura),
}

impl ChoiceOption {
    /// The menu this option is on.
    pub fn menu(self) -> ChoiceMenu {
        match self {
            ChoiceOption::RogueOpener(_) => ChoiceMenu::RogueOpener,
            ChoiceOption::RoguePoison(_) => ChoiceMenu::RoguePoison,
            ChoiceOption::WarlockCurse(_) => ChoiceMenu::WarlockCurse,
            ChoiceOption::HunterPet(_) => ChoiceMenu::HunterPet,
            ChoiceOption::WarriorShout(_) => ChoiceMenu::WarriorShout,
            ChoiceOption::MageArmor(_) => ChoiceMenu::MageArmor,
            ChoiceOption::PaladinAura(_) => ChoiceMenu::PaladinAura,
        }
    }

    /// Whether this is its menu's default.
    pub fn is_default(self) -> bool {
        self.menu().default_option() == self
    }
}

/// A second slot of a menu, which only a talent opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChoiceSlot {
    /// The Rogue's off-hand poison (Master Poisoner). Empty, the main-hand
    /// poison coats both weapons; filled, the off hand carries its own, which
    /// must differ from the main hand's.
    OffHandPoison,
}

impl ChoiceSlot {
    /// The menu the slot chooses from.
    pub fn menu(self) -> ChoiceMenu {
        match self {
            ChoiceSlot::OffHandPoison => ChoiceMenu::RoguePoison,
        }
    }
}

/// A rule that makes a per-match choice exclusive, written in `talents.ron`'s
/// `choice_rules`. Each names an option a player PICKS: a rule on a menu's
/// default would bind every slot that chose nothing, so the loader rejects
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ChoiceRule {
    /// While `option` is chosen, `talent` does nothing for the match (Soul
    /// Link, with the Felhunter sacrificed).
    SwitchesOff {
        option: ChoiceOption,
        talent: TalentId,
    },
    /// While `option` is chosen, `ability` cannot be cast (a heal, in
    /// Shadowform).
    Forbids {
        option: ChoiceOption,
        ability: AbilityType,
    },
    /// `ability` can be cast only while `option` is chosen (Silence, only in
    /// Shadowform).
    Requires {
        ability: AbilityType,
        option: ChoiceOption,
    },
}

impl ChoiceRule {
    /// The option the rule turns on.
    pub fn option(self) -> ChoiceOption {
        match self {
            ChoiceRule::SwitchesOff { option, .. }
            | ChoiceRule::Forbids { option, .. }
            | ChoiceRule::Requires { option, .. } => option,
        }
    }
}

/// What one slot chose for the match: every option on its class's menus
/// (each of a Warlock's per-target curses), and the option in each second
/// slot its build opened and it filled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotChoices {
    chosen: BTreeSet<ChoiceOption>,
    second: BTreeMap<ChoiceSlot, ChoiceOption>,
}

impl SlotChoices {
    /// Choices from `options` (menu choices) and `second` (filled second
    /// slots).
    pub fn of(options: &[ChoiceOption], second: &[(ChoiceSlot, ChoiceOption)]) -> Self {
        Self {
            chosen: options.iter().copied().collect(),
            second: second.iter().copied().collect(),
        }
    }

    /// The choices of `team`'s slot `slot`, a `class`, as `config` holds them:
    /// only its own class's menus (another class's preference is inert).
    pub fn from_config(config: &MatchConfig, team: u8, slot: usize, class: CharacterClass) -> Self {
        fn pick_for<T: Copy>(team: u8, slot: usize, t1: &[T], t2: &[T]) -> Option<T> {
            if team == 1 { t1 } else { t2 }.get(slot).copied()
        }
        let mut chosen = BTreeSet::new();
        let mut second = BTreeMap::new();
        match class {
            CharacterClass::Rogue => {
                let opener = pick_for(team, slot, &config.team1_rogue_openers, &config.team2_rogue_openers);
                chosen.insert(ChoiceOption::RogueOpener(opener.unwrap_or_default()));
                let poison = pick_for(team, slot, &config.team1_rogue_poisons, &config.team2_rogue_poisons);
                chosen.insert(ChoiceOption::RoguePoison(poison.unwrap_or_default()));
                let off = pick_for(team, slot, &config.team1_rogue_offhand_poisons,
                    &config.team2_rogue_offhand_poisons,
                );
                if let Some(Some(off)) = off {
                    second.insert(ChoiceSlot::OffHandPoison, ChoiceOption::RoguePoison(off));
                }
            }
            CharacterClass::Warlock => {
                let prefs = if team == 1 {
                    &config.team1_warlock_curse_prefs
                } else {
                    &config.team2_warlock_curse_prefs
                };
                let curses = prefs.get(slot).cloned().unwrap_or_default();
                if curses.is_empty() {
                    chosen.insert(ChoiceOption::WarlockCurse(WarlockCurse::default()));
                }
                chosen.extend(curses.into_iter().map(ChoiceOption::WarlockCurse));
            }
            CharacterClass::Hunter => {
                let pet = pick_for(team, slot, &config.team1_hunter_pet_types, &config.team2_hunter_pet_types);
                chosen.insert(ChoiceOption::HunterPet(pet.unwrap_or_default()));
            }
            CharacterClass::Warrior => {
                let shout = pick_for(team, slot, &config.team1_warrior_shouts, &config.team2_warrior_shouts);
                chosen.insert(ChoiceOption::WarriorShout(shout.unwrap_or_default()));
            }
            CharacterClass::Mage => {
                let armor = pick_for(team, slot, &config.team1_mage_armors, &config.team2_mage_armors);
                chosen.insert(ChoiceOption::MageArmor(armor.unwrap_or_default()));
            }
            CharacterClass::Paladin => {
                let aura = pick_for(team, slot, &config.team1_paladin_auras, &config.team2_paladin_auras);
                chosen.insert(ChoiceOption::PaladinAura(aura.unwrap_or_default()));
            }
            CharacterClass::Priest | CharacterClass::Shaman | CharacterClass::Druid => {}
        }
        Self { chosen, second }
    }

    /// Whether the slot chose `option`, in a menu or in a second slot.
    pub fn chose(&self, option: ChoiceOption) -> bool {
        self.chosen.contains(&option) || self.second.values().any(|o| *o == option)
    }

    /// Every option the slot chose on its menus.
    pub fn options(&self) -> impl Iterator<Item = ChoiceOption> + '_ {
        self.chosen.iter().copied()
    }

    /// What the slot put in `slot`, when it filled it.
    pub fn second(&self, slot: ChoiceSlot) -> Option<ChoiceOption> {
        self.second.get(&slot).copied()
    }

    /// Every filled second slot.
    pub fn seconds(&self) -> impl Iterator<Item = (ChoiceSlot, ChoiceOption)> + '_ {
        self.second.iter().map(|(s, o)| (*s, *o))
    }
}

/// The effects a node's rank applies, each with the times it applies: a
/// plain effect once per rank, a `FromRank(r, e)` once per rank from `r` on.
pub(super) fn ranked_effects(
    per_rank: &[TalentEffect],
    rank: u8,
) -> impl Iterator<Item = (&TalentEffect, u8)> {
    per_rank.iter().filter_map(move |effect| match effect {
        TalentEffect::FromRank(from, inner) => {
            let times = rank.saturating_sub(from.saturating_sub(1));
            (times > 0).then_some((inner.as_ref(), times))
        }
        other => Some((other, rank)),
    })
}

impl TalentDefinitions {
    /// The table-wide rules for choices and mechanics, run by the loader:
    /// each option is widened, each slot opened and each mechanic carried by
    /// at most one node; each choice rule names an option a player picks,
    /// on a menu of the class whose talent or ability it names.
    pub(super) fn validate_choice_shapes(
        &self,
        abilities: &super::ability_config::AbilityDefinitions,
    ) -> Result<(), String> {
        let mut widened: BTreeMap<ChoiceOption, TalentId> = BTreeMap::new();
        let mut opened: BTreeMap<ChoiceSlot, TalentId> = BTreeMap::new();
        let mut mechanics: BTreeMap<super::talent_mechanics::MechanicKind, TalentId> =
            BTreeMap::new();
        for (id, talent) in self.iter() {
            let effects = talent.per_rank.iter().map(|e| match e {
                TalentEffect::FromRank(_, inner) => inner.as_ref(),
                other => other,
            });
            for effect in effects {
                let clash = match effect {
                    TalentEffect::WidenChoice(o) => widened.insert(*o, id).map(|t| format!("{o:?} by {t:?}")),
                    TalentEffect::AddChoiceSlot(slot) => {
                        opened.insert(*slot, id).map(|t| format!("{slot:?} by {t:?}"))
                    }
                    TalentEffect::Mechanic(m) => mechanics
                        .insert(m.kind(), id)
                        .map(|t| format!("{:?} by {t:?}", m.kind())),
                    _ => None,
                };
                if let Some(clash) = clash {
                    return Err(format!("{id:?}: already taken: {clash}"));
                }
            }
        }
        for rule in self.rules() {
            let option = rule.option();
            if option.is_default() {
                return Err(format!(
                    "the choice rule {rule:?} names {option:?}, its menu's default, which every \
                     slot that chooses nothing has"
                ));
            }
            let class = option.menu().class();
            match *rule {
                ChoiceRule::SwitchesOff { talent, .. } => {
                    let Some(t) = self.get(talent) else {
                        return Err(format!("the choice rule {rule:?} names a talent with no entry"));
                    };
                    if t.class != class {
                        return Err(format!(
                            "the choice rule {rule:?} switches off a {:?} talent with a {class:?} \
                             choice",
                            t.class
                        ));
                    }
                }
                ChoiceRule::Forbids { ability, .. } | ChoiceRule::Requires { ability, .. } => {
                    if !self.reachable_abilities(class, abilities).contains(&ability) {
                        return Err(format!(
                            "the choice rule {rule:?} names {ability:?}, which no {class:?} can have"
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Every option some node widens: hidden from a build without that node.
    pub fn widened_options(&self) -> BTreeSet<ChoiceOption> {
        self.iter()
            .flat_map(|(_, t)| t.per_rank.iter())
            .filter_map(|effect| match effect {
                TalentEffect::WidenChoice(option) => Some(*option),
                TalentEffect::FromRank(_, inner) => match inner.as_ref() {
                    TalentEffect::WidenChoice(option) => Some(*option),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The options and second slots `build` opens, read from the nodes it
    /// takes at the ranks it takes them.
    fn opened_by(&self, build: &Build) -> (BTreeSet<ChoiceOption>, BTreeSet<ChoiceSlot>) {
        let mut options = BTreeSet::new();
        let mut slots = BTreeSet::new();
        for (id, rank) in &build.ranks {
            let Some(talent) = self.get(*id) else {
                continue;
            };
            for (effect, _) in ranked_effects(&talent.per_rank, *rank) {
                match effect {
                    TalentEffect::WidenChoice(option) => {
                        options.insert(*option);
                    }
                    TalentEffect::AddChoiceSlot(slot) => {
                        slots.insert(*slot);
                    }
                    _ => {}
                }
            }
        }
        (options, slots)
    }

    /// Whether a slot with `build` may choose `option`: always, unless a node
    /// widens it, and then only when `build` takes that node (at a rank that
    /// widens it).
    pub fn offers(&self, build: &Build, option: ChoiceOption) -> bool {
        !self.widened_options().contains(&option) || self.opened_by(build).0.contains(&option)
    }

    /// The options of `menu` a slot with `build` may choose, in display order.
    pub fn offered(&self, build: &Build, menu: ChoiceMenu) -> Vec<ChoiceOption> {
        menu.options()
            .into_iter()
            .filter(|o| self.offers(build, *o))
            .collect()
    }

    /// Whether `build` opens the second slot `slot`.
    pub fn opens(&self, build: &Build, slot: ChoiceSlot) -> bool {
        self.opened_by(build).1.contains(&slot)
    }

    /// The choice rules, as `talents.ron` writes them.
    pub fn choice_rules(&self) -> &[ChoiceRule] {
        self.rules()
    }

    /// Whether `choices` are legal for a `class` slot with `build`: every
    /// option chosen is offered to the build; every filled second slot is one
    /// the build opens, its option on that slot's menu, offered, and not the
    /// option its first slot already holds.
    pub fn validate_choices(
        &self,
        class: CharacterClass,
        build: &Build,
        choices: &SlotChoices,
    ) -> Result<(), String> {
        for option in choices.options() {
            if option.menu().class() != class {
                return Err(format!("{option:?} is not a {class:?} choice"));
            }
            if !self.offers(build, option) {
                return Err(format!(
                    "{option:?} is chosen, but only a talent that widens the {:?} menu offers it",
                    option.menu()
                ));
            }
        }
        for (slot, option) in choices.seconds() {
            if slot.menu().class() != class {
                return Err(format!("{slot:?} is not a {class:?} slot"));
            }
            if !self.opens(build, slot) {
                return Err(format!(
                    "{slot:?} is filled, but the build takes no talent that opens it"
                ));
            }
            if option.menu() != slot.menu() {
                return Err(format!("{slot:?} holds {option:?}, from another menu"));
            }
            if !self.offers(build, option) {
                return Err(format!("{slot:?} holds {option:?}, which the build is not offered"));
            }
            if choices.chosen.contains(&option) {
                return Err(format!(
                    "{slot:?} holds {option:?}, the option its first slot already has"
                ));
            }
        }
        Ok(())
    }

    /// The talents `choices` switch off for the match.
    pub fn switched_off(&self, choices: &SlotChoices) -> BTreeSet<TalentId> {
        self.rules()
            .iter()
            .filter_map(|rule| match rule {
                ChoiceRule::SwitchesOff { option, talent } if choices.chose(*option) => {
                    Some(*talent)
                }
                _ => None,
            })
            .collect()
    }

    /// The abilities `choices` lock for the match: every one an option chosen
    /// forbids, and every one that requires an option not chosen.
    pub fn locked_by(&self, choices: &SlotChoices) -> BTreeSet<AbilityType> {
        self.rules()
            .iter()
            .filter_map(|rule| match rule {
                ChoiceRule::Forbids { option, ability } if choices.chose(*option) => Some(*ability),
                ChoiceRule::Requires { ability, option } if !choices.chose(*option) => {
                    Some(*ability)
                }
                _ => None,
            })
            .collect()
    }

    /// Check every slot of `config` chose what its build allows.
    pub fn validate_config_choices(&self, config: &MatchConfig) -> Result<(), String> {
        for (team, classes, builds) in [
            (1u8, &config.team1, &config.team1_builds),
            (2u8, &config.team2, &config.team2_builds),
        ] {
            for (slot, class) in classes.iter().enumerate() {
                let Some(class) = class else {
                    continue;
                };
                let build = builds.get(slot).cloned().unwrap_or_default();
                let choices = SlotChoices::from_config(config, team, slot, *class);
                self.validate_choices(*class, &build, &choices)
                    .map_err(|e| format!("team{team} slot {slot} ({class:?}): {e}"))?;
            }
        }
        Ok(())
    }
}
