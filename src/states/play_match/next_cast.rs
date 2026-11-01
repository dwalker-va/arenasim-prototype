//! Next-cast modifiers: "your next X is instant", "your next heal is a sure
//! crit", "your next ability costs 5 less energy".
//!
//! One generalised effect (AS-331), not a variant per field. A modifier is an
//! [`AuraType::NextCast`] aura on its holder carrying a [`NextCastModifier`]:
//! the abilities it covers ([`NextCastScope`]) and what it changes about the
//! next cast of one of them — instant, free, a crit chance (1.0 is a sure
//! crit), a shorter cast, a smaller cost, a stronger effect. Granted abilities
//! apply one through their `applies_aura` block (Divine Favor, Amplify Curse,
//! Inner Focus); a talent's proc applies one as its effect (Shadow Trance,
//! Infusion of Light, Light's Grace, The Art of War, Initiative).
//!
//! **Read.** While a modifier is up, its holder decides from an EFFECTIVE book
//! ([`effective_book`]): its own book with every covered ability's config
//! changed. So the cast gate, the cast time a cast starts with and the cost an
//! instant pays all read the changed numbers, with no edit at any class AI
//! site. A holder with no modifier decides from its own book unchanged, which
//! is what keeps a match without talents byte-identical.
//!
//! **Spent.** The decision that USES a covered ability spends every modifier
//! covering it ([`consume`]), in `decide_abilities` right after the unit's
//! turn. A cast with a cast bar resolves later, in `process_casting` (and a
//! projectile later still), so the spent modifier rides on the combatant as
//! [`SpentNextCast`] until that cast completes: its cost, its crit chance and
//! its power are read from it there. A cast that is interrupted keeps nothing:
//! the modifier was spent when the cast began.
//!
//! **Stacking.** Two modifiers covering one cast do not compound: each field
//! takes the strongest of them ([`NextCastModifier::strongest`]), the stacking
//! simplification every same-kind effect follows. Both are spent.

use serde::{Deserialize, Serialize};

use super::abilities::AbilityType;
use super::ability_book::AbilityBook;
use super::ability_config::AbilityConfig;
use super::components::{ActiveAuras, AuraType};
use super::talents::AbilityField;

/// The abilities a [`NextCastModifier`] changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NextCastScope {
    /// Any ability its holder uses next (Initiative, Inner Focus).
    Any,
    /// Only these (Shadow Trance: Shadow Bolt; Divine Favor: the heals).
    Abilities(Vec<AbilityType>),
}

impl NextCastScope {
    /// Whether this scope covers `ability`.
    pub fn covers(&self, ability: AbilityType) -> bool {
        match self {
            NextCastScope::Any => true,
            NextCastScope::Abilities(list) => list.contains(&ability),
        }
    }
}

/// What a next-cast modifier changes about the next cast it covers. Every
/// change defaults to none, so a RON entry names only what it does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NextCastModifier {
    pub applies_to: NextCastScope,
    /// The cast has no cast time.
    #[serde(default)]
    pub instant: bool,
    /// The cast costs no resource.
    #[serde(default)]
    pub free: bool,
    /// Added to the cast's crit chance. `1.0` is a sure crit.
    #[serde(default)]
    pub crit_chance: f32,
    /// Seconds taken off the cast time (never below zero).
    #[serde(default)]
    pub cast_time_reduction: f32,
    /// Resource taken off the cost — mana, energy or rage (never below zero).
    #[serde(default)]
    pub cost_reduction: f32,
    /// A fraction added to the cast's effect: its damage, its healing and its
    /// aura's strength ([`AbilityField::AuraStrength`]). `0.5` is +50%
    /// (Amplify Curse).
    #[serde(default)]
    pub power: f32,
}

impl NextCastModifier {
    /// Whether this modifier changes `ability`.
    pub fn covers(&self, ability: AbilityType) -> bool {
        self.applies_to.covers(ability)
    }

    /// `config` as the next cast changed by this modifier sees it.
    pub fn apply(&self, config: &mut AbilityConfig) {
        if self.instant {
            config.cast_time = 0.0;
        } else if self.cast_time_reduction != 0.0 {
            config.cast_time = (config.cast_time - self.cast_time_reduction).max(0.0);
        }
        if self.free {
            config.mana_cost = 0.0;
        } else if self.cost_reduction != 0.0 {
            config.mana_cost = (config.mana_cost - self.cost_reduction).max(0.0);
        }
        config.crit_chance += self.crit_chance;
        if self.power != 0.0 {
            let scale = 1.0 + self.power;
            for field in [
                AbilityField::DamageBaseMin,
                AbilityField::DamageBaseMax,
                AbilityField::DamageCoefficient,
                AbilityField::HealingBaseMin,
                AbilityField::HealingBaseMax,
                AbilityField::HealingCoefficient,
                AbilityField::AuraStrength,
            ] {
                if let Some(value) = field.get(config) {
                    field.set(config, value * scale);
                }
            }
        }
    }

    /// The strongest of each change among `modifiers` (which must all cover
    /// the cast they are combined for): a flag is set when any sets it, a
    /// number is the largest. `None` for no modifiers. The scope of the result
    /// is the first one's — it is only ever applied to a cast every one
    /// covers.
    pub fn strongest<'a>(modifiers: impl IntoIterator<Item = &'a NextCastModifier>) -> Option<Self> {
        let mut out: Option<NextCastModifier> = None;
        for m in modifiers {
            match out.as_mut() {
                None => out = Some(m.clone()),
                Some(o) => {
                    o.instant |= m.instant;
                    o.free |= m.free;
                    o.crit_chance = o.crit_chance.max(m.crit_chance);
                    o.cast_time_reduction = o.cast_time_reduction.max(m.cast_time_reduction);
                    o.cost_reduction = o.cost_reduction.max(m.cost_reduction);
                    o.power = o.power.max(m.power);
                }
            }
        }
        out
    }

    /// This modifier with every number scaled by `ranks` — what `ranks` ranks
    /// of a talent whose ranks add to its effect give (ranks are additive).
    pub fn times(&self, ranks: f32) -> Self {
        Self {
            crit_chance: self.crit_chance * ranks,
            cast_time_reduction: self.cast_time_reduction * ranks,
            cost_reduction: self.cost_reduction * ranks,
            power: self.power * ranks,
            ..self.clone()
        }
    }

    /// Whether this modifier has any number for a rank to scale.
    pub fn has_magnitude(&self) -> bool {
        self.crit_chance != 0.0
            || self.cast_time_reduction != 0.0
            || self.cost_reduction != 0.0
            || self.power != 0.0
    }

    /// Reject a modifier that changes nothing, names no ability, or carries a
    /// number that is negative or not finite.
    pub fn validate(&self) -> Result<(), String> {
        if let NextCastScope::Abilities(list) = &self.applies_to {
            if list.is_empty() {
                return Err("a next-cast modifier names no ability".to_string());
            }
        }
        for (name, value) in [
            ("crit_chance", self.crit_chance),
            ("cast_time_reduction", self.cast_time_reduction),
            ("cost_reduction", self.cost_reduction),
            ("power", self.power),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("a next-cast modifier's {name} of {value} is negative"));
            }
        }
        if !(self.instant || self.free || self.has_magnitude()) {
            return Err("a next-cast modifier changes nothing".to_string());
        }
        Ok(())
    }
}

/// A modifier spent by a cast that resolves after its decision: the cast it
/// was spent on, and the change. Carried on the combatant until
/// `process_casting` completes that cast.
#[derive(Clone, Debug, PartialEq)]
pub struct SpentNextCast {
    pub ability: AbilityType,
    pub modifier: NextCastModifier,
}

/// Every live next-cast modifier on a holder, in aura order.
fn live(auras: Option<&ActiveAuras>) -> impl Iterator<Item = &NextCastModifier> {
    auras
        .into_iter()
        .flat_map(|a| a.auras.iter())
        .filter(|a| a.effect_type == AuraType::NextCast)
        .filter_map(|a| a.next_cast.as_ref())
}

/// The book its holder decides from: `book` with every ability a live
/// modifier covers changed by the strongest of them. `book` itself (a shared
/// handle, so no copy) when no modifier is up.
pub fn effective_book(book: &AbilityBook, auras: Option<&ActiveAuras>) -> AbilityBook {
    if live(auras).next().is_none() {
        return book.clone();
    }
    let mut out = book.clone();
    let covered: Vec<(AbilityType, NextCastModifier)> = book
        .iter()
        .filter_map(|(ability, _)| {
            NextCastModifier::strongest(live(auras).filter(|m| m.covers(*ability)))
                .map(|m| (*ability, m))
        })
        .collect();
    for (ability, modifier) in covered {
        out = out.patching(ability, |config| modifier.apply(config));
    }
    out
}

/// Spend every live modifier covering `ability`: remove their auras from the
/// holder and return the strongest of them, or `None` when none covers it.
/// Returns the names of the spent auras too, for the log.
pub fn consume(auras: &mut ActiveAuras, ability: AbilityType) -> Option<(NextCastModifier, Vec<String>)> {
    let covering: Vec<usize> = auras
        .auras
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            a.effect_type == AuraType::NextCast
                && a.next_cast.as_ref().is_some_and(|m| m.covers(ability))
        })
        .map(|(i, _)| i)
        .collect();
    if covering.is_empty() {
        return None;
    }
    let combined =
        NextCastModifier::strongest(covering.iter().filter_map(|i| auras.auras[*i].next_cast.as_ref()))?;
    let mut names = Vec::new();
    for index in covering.into_iter().rev() {
        names.push(auras.remove_debuff_at(index).ability_name);
    }
    names.reverse();
    Some((combined, names))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::components::Aura;

    fn modifier(scope: NextCastScope) -> NextCastModifier {
        NextCastModifier {
            applies_to: scope,
            instant: false,
            free: false,
            crit_chance: 0.0,
            cast_time_reduction: 0.0,
            cost_reduction: 0.0,
            power: 0.0,
        }
    }

    fn primed(name: &str, m: NextCastModifier) -> Aura {
        Aura {
            effect_type: AuraType::NextCast,
            duration: 10.0,
            magnitude: 1.0,
            ability_name: name.to_string(),
            next_cast: Some(m),
            ..Default::default()
        }
    }

    fn book() -> AbilityBook {
        AbilityBook::shipped(crate::states::match_config::CharacterClass::Warlock)
    }

    #[test]
    fn with_no_modifier_the_book_is_unchanged() {
        let book = book();
        let eff = effective_book(&book, Some(&ActiveAuras::default()));
        for (ability, config) in book.iter() {
            assert_eq!(
                format!("{config:?}"),
                format!("{:?}", eff.get_unchecked(ability))
            );
        }
    }

    /// Shadow Trance: Shadow Bolt instant, nothing else touched.
    #[test]
    fn an_instant_modifier_changes_only_what_it_covers() {
        let book = book();
        let mut m = modifier(NextCastScope::Abilities(vec![AbilityType::Shadowbolt]));
        m.instant = true;
        let auras = ActiveAuras {
            auras: vec![primed("Shadow Trance", m)],
        };
        let eff = effective_book(&book, Some(&auras));
        assert_eq!(eff.get_unchecked(&AbilityType::Shadowbolt).cast_time, 0.0);
        assert!(book.get_unchecked(&AbilityType::Shadowbolt).cast_time > 0.0);
        assert_eq!(
            eff.get_unchecked(&AbilityType::Immolate).cast_time,
            book.get_unchecked(&AbilityType::Immolate).cast_time
        );
    }

    /// Free, a cost cut, a cast-time cut, a crit chance and power, each on its
    /// own field.
    #[test]
    fn every_change_lands_on_its_own_field() {
        let base = book().get_unchecked(&AbilityType::Shadowbolt).clone();
        let mut free = modifier(NextCastScope::Any);
        free.free = true;
        let mut c = base.clone();
        free.apply(&mut c);
        assert_eq!(c.mana_cost, 0.0);

        let mut cut = modifier(NextCastScope::Any);
        cut.cost_reduction = 5.0;
        cut.cast_time_reduction = 0.5;
        cut.crit_chance = 0.25;
        cut.power = 0.5;
        let mut c = base.clone();
        cut.apply(&mut c);
        assert_eq!(c.mana_cost, base.mana_cost - 5.0);
        assert_eq!(c.cast_time, base.cast_time - 0.5);
        assert_eq!(c.crit_chance, base.crit_chance + 0.25);
        assert_eq!(c.damage_base_max, base.damage_base_max * 1.5);
        assert_eq!(c.damage_coefficient, base.damage_coefficient * 1.5);

        // Never below zero.
        let mut big = modifier(NextCastScope::Any);
        big.cost_reduction = 1.0e6;
        big.cast_time_reduction = 1.0e6;
        let mut c = base.clone();
        big.apply(&mut c);
        assert_eq!((c.mana_cost, c.cast_time), (0.0, 0.0));
    }

    /// Amplify Curse: +50% to a curse's aura, read as its STRENGTH — a
    /// remainder-fraction aura (a slow, a healing cut) gets stronger, not
    /// weaker.
    #[test]
    fn power_strengthens_an_aura() {
        let defs = crate::states::play_match::ability_config::AbilityDefinitions::shipped();
        let mut power = modifier(NextCastScope::Any);
        power.power = 0.5;
        let mut agony = defs.get_unchecked(&AbilityType::CurseOfAgony).clone();
        let before = agony.applies_aura.as_ref().unwrap().magnitude;
        power.apply(&mut agony);
        assert_eq!(agony.applies_aura.as_ref().unwrap().magnitude, before * 1.5);
        // Mortal Strike's Mortal Wound: 0.65 left (35% cut) -> 52.5% cut.
        let mut ms = defs.get_unchecked(&AbilityType::MortalStrike).clone();
        power.apply(&mut ms);
        let left = ms.applies_aura.as_ref().unwrap().magnitude;
        assert!((left - (1.0 - 0.35 * 1.5)).abs() < 1e-6, "{left}");
    }

    #[test]
    fn two_modifiers_take_the_strongest_of_each_not_the_sum() {
        let mut a = modifier(NextCastScope::Any);
        a.crit_chance = 0.25;
        a.instant = true;
        let mut b = modifier(NextCastScope::Any);
        b.crit_chance = 1.0;
        b.cost_reduction = 5.0;
        let s = NextCastModifier::strongest([&a, &b]).unwrap();
        assert!(s.instant);
        assert_eq!(s.crit_chance, 1.0);
        assert_eq!(s.cost_reduction, 5.0);
        let s = NextCastModifier::strongest([&a, &a]).unwrap();
        assert_eq!(s.crit_chance, 0.25, "two of one kind do not add");
    }

    /// The cast spends every modifier covering it, and only those.
    #[test]
    fn consume_spends_only_what_covers_the_cast() {
        let mut trance = modifier(NextCastScope::Abilities(vec![AbilityType::Shadowbolt]));
        trance.instant = true;
        let mut other = modifier(NextCastScope::Abilities(vec![AbilityType::Immolate]));
        other.free = true;
        let mut auras = ActiveAuras {
            auras: vec![primed("Shadow Trance", trance), primed("Other", other)],
        };
        assert!(consume(&mut auras, AbilityType::Corruption).is_none());
        assert_eq!(auras.auras.len(), 2);
        let (spent, names) = consume(&mut auras, AbilityType::Shadowbolt).unwrap();
        assert!(spent.instant);
        assert_eq!(names, vec!["Shadow Trance".to_string()]);
        assert_eq!(auras.auras.len(), 1);
        assert_eq!(auras.auras[0].ability_name, "Other");
    }

    #[test]
    fn validate_rejects_a_modifier_that_does_nothing_or_names_nothing() {
        assert!(modifier(NextCastScope::Any).validate().is_err());
        let mut m = modifier(NextCastScope::Abilities(vec![]));
        m.free = true;
        assert!(m.validate().is_err());
        let mut m = modifier(NextCastScope::Any);
        m.cost_reduction = -1.0;
        assert!(m.validate().is_err());
        let mut m = modifier(NextCastScope::Any);
        m.free = true;
        assert!(m.validate().is_ok());
    }

    #[test]
    fn ranks_scale_every_number_and_no_flag() {
        let mut m = modifier(NextCastScope::Any);
        m.cost_reduction = 5.0;
        m.instant = true;
        let two = m.times(2.0);
        assert_eq!(two.cost_reduction, 10.0);
        assert!(two.instant);
    }
}
