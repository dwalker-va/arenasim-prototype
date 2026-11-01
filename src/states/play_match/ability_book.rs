//! The per-combatant ability book.
//!
//! [`AbilityDefinitions`] is the GLOBAL table, loaded from `abilities.ron`: every
//! ability in the game, as written. An [`AbilityBook`] is ONE combatant's
//! abilities: the entries its class owns (a pet's: the entries its pet type
//! casts), resolved once at spawn and never edited during a match. Anything
//! that changes mid-match is an aura, not a book edit.
//!
//! Today a book is a copy of its class's slice of the global table, so reading
//! a config from the book or from the table gives the same answer. Talents
//! (AS-297) resolve a build into the book at spawn — granted abilities added,
//! patches applied — which is why every sim path that has a combatant to hand
//! reads its configs from that combatant's book, and `tests/talent_book_audit.rs`
//! keeps it that way.
//!
//! A book holds only its owner's abilities. Asking it for another class's
//! ability is a bug at the call site — it should be asking the OTHER unit's
//! book — so [`AbilityBook::get_unchecked`] names the book in its panic, and
//! [`AbilityBook::get`] answers `None` ("this unit has no such ability").

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::abilities::AbilityType;
use super::ability_config::{AbilityConfig, AbilityDefinitions, AuraEffect};
use super::components::PetType;
use super::match_config::CharacterClass;

/// Whose abilities a book holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookOwner {
    /// A class's own abilities, pet abilities excluded.
    Class(CharacterClass),
    /// The abilities one pet type casts.
    Pet(PetType),
}

/// One combatant's resolved abilities. Cheap to clone: the entries are shared,
/// because a book never changes after spawn.
#[derive(Clone)]
pub struct AbilityBook {
    owner: BookOwner,
    entries: Arc<BTreeMap<AbilityType, AbilityConfig>>,
    /// The live next-cast modifiers' view of this book, for one decision
    /// (`next_cast::effective_book`). `None` for every book a combatant
    /// carries.
    overlay: Option<Arc<NextCastOverlay>>,
}

/// One turn's next-cast modifiers over a book: the configs of the abilities
/// they cover, changed, until the turn CHOOSES one of them
/// ([`NextCastOverlay::spend`], called by the decision trace's `choose`). From
/// then on the chosen ability keeps its changed config and every other covered
/// ability reads its own again — a modifier is spent by the first cast that
/// uses it, never by two in one turn.
pub struct NextCastOverlay {
    changed: BTreeMap<AbilityType, AbilityConfig>,
    modifiers: BTreeMap<AbilityType, crate::states::play_match::next_cast::NextCastModifier>,
    spent: Mutex<Option<AbilityType>>,
}

impl NextCastOverlay {
    /// An overlay changing `changed`'s abilities, each by its modifier.
    pub fn new(
        changed: BTreeMap<AbilityType, AbilityConfig>,
        modifiers: BTreeMap<AbilityType, crate::states::play_match::next_cast::NextCastModifier>,
    ) -> Self {
        Self {
            changed,
            modifiers,
            spent: Mutex::new(None),
        }
    }

    /// Record `ability` as the cast that spends the modifiers, when it is
    /// covered and nothing has spent them yet.
    pub fn spend(&self, ability: AbilityType) {
        if self.changed.contains_key(&ability) {
            let mut spent = self.spent.lock().expect("an unpoisoned lock");
            if spent.is_none() {
                *spent = Some(ability);
            }
        }
    }

    /// The covered ability that spent the modifiers, if one did.
    pub fn spent(&self) -> Option<AbilityType> {
        *self.spent.lock().expect("an unpoisoned lock")
    }

    fn view(&self, ability: &AbilityType) -> Option<&AbilityConfig> {
        let config = self.changed.get(ability)?;
        match self.spent() {
            None => Some(config),
            Some(spent) if spent == *ability => Some(config),
            Some(_) => None,
        }
    }
}

impl std::fmt::Debug for AbilityBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AbilityBook({:?})", self.owner)
    }
}

impl AbilityBook {
    /// The book of an untalented combatant of `class`: the class's base
    /// abilities ([`AbilityDefinitions::base_abilities_for_class`]) — pet
    /// abilities excluded, a pet's abilities are in the pet's book, and talent
    /// abilities excluded, which only a build's `Grant` adds
    /// (`talents::TalentDefinitions::resolve`).
    pub fn for_class(defs: &AbilityDefinitions, class: CharacterClass) -> Self {
        Self::from_entries(
            BookOwner::Class(class),
            defs,
            defs.base_abilities_for_class(class),
        )
    }

    /// The book of a pet of `pet`: the abilities that pet casts
    /// ([`AbilityDefinitions::abilities_for_pet`]).
    pub fn for_pet(defs: &AbilityDefinitions, pet: PetType) -> Self {
        Self::from_entries(BookOwner::Pet(pet), defs, defs.abilities_for_pet(pet))
    }

    /// [`Self::for_class`] over the shipped table
    /// ([`AbilityDefinitions::shipped`]): the book of a combatant built
    /// outside a match's spawn — `Combatant::new`, a test's unit.
    pub fn shipped(class: CharacterClass) -> Self {
        Self::for_class(AbilityDefinitions::shipped(), class)
    }

    /// [`Self::for_pet`] over the shipped table.
    pub fn shipped_pet(pet: PetType) -> Self {
        Self::for_pet(AbilityDefinitions::shipped(), pet)
    }

    /// The shipped book of a unit of `class`, or of the pet `pet` when it is
    /// one: what a test's hand-built `CombatantInfo` carries.
    pub fn shipped_for(class: CharacterClass, pet: Option<PetType>) -> Self {
        match pet {
            Some(pet) => Self::shipped_pet(pet),
            None => Self::shipped(class),
        }
    }

    /// This book with `ability` added from `defs`, as it is written there. A
    /// BUILD-time addition, made before the book's combatant enters the world:
    /// a scene staging a unit mid-cast of an ability outside its class
    /// (`Scene::casting`), and the shape a talent's granted ability (AS-297)
    /// takes. Nothing edits a book during a match.
    pub fn granting(mut self, defs: &AbilityDefinitions, ability: AbilityType) -> Self {
        Arc::make_mut(&mut self.entries).insert(ability, defs.get_unchecked(&ability).clone());
        self
    }

    /// This book seen through `overlay` (`next_cast::effective_book`).
    pub fn with_overlay(mut self, overlay: NextCastOverlay) -> Self {
        self.overlay = Some(Arc::new(overlay));
        self
    }

    /// The overlay this book is seen through, if any: what the decision
    /// trace's `choose` spends.
    pub fn overlay(&self) -> Option<Arc<NextCastOverlay>> {
        self.overlay.clone()
    }

    /// The next-cast modifier `ability` spent in this book's turn, if it did:
    /// what a cast that lands later (an instant projectile, a pending effect)
    /// carries to its landing.
    pub fn spent_modifier(
        &self,
        ability: AbilityType,
    ) -> Option<crate::states::play_match::next_cast::NextCastModifier> {
        let overlay = self.overlay.as_ref()?;
        (overlay.spent() == Some(ability))
            .then(|| overlay.modifiers.get(&ability).cloned())
            .flatten()
    }

    /// This book with `edit` applied to its config for `ability`: a talent's
    /// `Patch`, made while a build is resolved, before the book's combatant
    /// enters the world. Panics when the book has no such ability.
    pub fn patching(mut self, ability: AbilityType, edit: impl FnOnce(&mut AbilityConfig)) -> Self {
        let owner = self.owner;
        let config = Arc::make_mut(&mut self.entries)
            .get_mut(&ability)
            .unwrap_or_else(|| panic!("{ability:?} is not in the ability book of {owner:?}"));
        edit(config);
        self
    }

    fn from_entries(
        owner: BookOwner,
        defs: &AbilityDefinitions,
        abilities: Vec<AbilityType>,
    ) -> Self {
        let entries = abilities
            .into_iter()
            .map(|ability| (ability, defs.get_unchecked(&ability).clone()))
            .collect();
        Self {
            owner,
            entries: Arc::new(entries),
            overlay: None,
        }
    }

    /// Whose abilities this book holds.
    pub fn owner(&self) -> BookOwner {
        self.owner
    }

    /// This combatant's config for `ability`, or `None` when it has no such
    /// ability.
    pub fn get(&self, ability: &AbilityType) -> Option<&AbilityConfig> {
        if let Some(changed) = self.overlay.as_ref().and_then(|o| o.view(ability)) {
            return Some(changed);
        }
        self.entries.get(ability)
    }

    /// This combatant's config for an ability it is known to have. Panics,
    /// naming the book, when it does not: a site asking one unit's book for
    /// another unit's ability should be asking that unit's book.
    #[track_caller]
    pub fn get_unchecked(&self, ability: &AbilityType) -> &AbilityConfig {
        self.get(ability).unwrap_or_else(|| {
            panic!(
                "{:?} is not in the ability book of {:?}: ask the book of the unit that casts it",
                ability, self.owner
            )
        })
    }

    /// Whether this combatant has `ability`.
    pub fn contains(&self, ability: &AbilityType) -> bool {
        self.entries.contains_key(ability)
    }

    /// The aura effect an ability in this book applied `aura` from — the
    /// ability whose RON `name` the aura carries, when its `applies_aura` is
    /// of the aura's own type. Ask the book of the aura's CASTER. `None` for
    /// an aura no ability of this book applied directly (a totem pulse, a
    /// proc, a trap) and for a compound's riders, which share their face's
    /// name but not its type.
    pub fn source_effect(&self, aura: &super::components::Aura) -> Option<&AuraEffect> {
        self.entries
            .values()
            .filter(|def| def.name == aura.ability_name)
            .filter_map(|def| def.applies_aura.as_ref())
            .find(|effect| effect.aura_type == aura.effect_type)
    }

    /// Every ability in the book with its OWN config, in `AbilityType` order
    /// (a next-cast overlay is not read: `get` reads it).
    pub fn iter(&self) -> impl Iterator<Item = (&AbilityType, &AbilityConfig)> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no talents, a book is its class's slice of the global table,
    /// entry for entry — the property that makes the migration byte-identical.
    #[test]
    fn a_book_is_its_owners_slice_of_the_table() {
        let defs = AbilityDefinitions::default();
        for class in CharacterClass::all() {
            let book = AbilityBook::for_class(&defs, *class);
            let own = defs.base_abilities_for_class(*class);
            assert_eq!(
                book.iter().map(|(a, _)| *a).collect::<Vec<_>>(),
                own,
                "{class:?}"
            );
            for (ability, config) in book.iter() {
                assert_eq!(
                    format!("{config:?}"),
                    format!("{:?}", defs.get_unchecked(ability)),
                    "{ability:?}"
                );
            }
        }
        for pet in [
            PetType::Felhunter,
            PetType::Spider,
            PetType::Boar,
            PetType::Bird,
        ] {
            let book = AbilityBook::for_pet(&defs, pet);
            assert_eq!(
                book.iter().map(|(a, _)| *a).collect::<Vec<_>>(),
                defs.abilities_for_pet(pet),
                "{pet:?}"
            );
        }
    }

    /// Every ability in the table is in exactly one base book — its class's,
    /// or its pet's — except a talent ability, which is in none: only the
    /// talent that grants it adds it (`talents.rs` holds each to exactly one
    /// granting talent). Nothing is unreachable, and nothing is held twice.
    #[test]
    fn every_ability_is_in_exactly_one_book() {
        let defs = AbilityDefinitions::default();
        let mut books: Vec<AbilityBook> = CharacterClass::all()
            .iter()
            .map(|c| AbilityBook::for_class(&defs, *c))
            .collect();
        books.extend(
            [
                PetType::Felhunter,
                PetType::Spider,
                PetType::Boar,
                PetType::Bird,
            ]
            .map(|p| AbilityBook::for_pet(&defs, p)),
        );
        for ability in defs.ability_types() {
            let holders: Vec<BookOwner> = books
                .iter()
                .filter(|b| b.contains(ability))
                .map(|b| b.owner())
                .collect();
            let expected = if defs.get_unchecked(ability).talent {
                0
            } else {
                1
            };
            assert_eq!(holders.len(), expected, "{ability:?} is in {holders:?}");
        }
    }

    #[test]
    #[should_panic(expected = "is not in the ability book of Class(Warrior)")]
    fn asking_for_another_units_ability_names_the_book() {
        let defs = AbilityDefinitions::default();
        AbilityBook::for_class(&defs, CharacterClass::Warrior)
            .get_unchecked(&AbilityType::Frostbolt);
    }
}
