//! Player-facing ability and aura text, generated from the game's own data.
//!
//! ONE source of truth for the prose that describes what an ability does. The
//! View Combatant screen and the encyclopedia both render it, so an ability's
//! tooltip says the same thing wherever the player meets it. Extracting it here
//! is what stops the encyclopedia becoming a second, drifting generator — the
//! failure mode the hand-written `get_class_abilities()` list already showed.
//!
//! Nothing here is hand-authored per ability. The text is derived from
//! `abilities.ron` plus the owning class's [`ClassBaseStats`], with two
//! documented overrides, in priority order:
//!
//! 1. **Totems** win over everything — their text is generated from the
//!    gameplay buff spec (`class_ai::shaman::totem_buff_spec`), so the numbers
//!    shown can never drift from the ones the sim applies.
//! 2. A **hand-written `description`** in `abilities.ron` wins over the
//!    generated text, for effects the numeric fields cannot express (Purge,
//!    Mana Burn, Heroic Strike, Disengage, Frost Trap). Only 5 of the 70
//!    abilities use it; `every_ability_and_aura_generates_text` is what
//!    forces an ability with no expressible effect to carry one.
//!
//! Pure functions of plain data — no egui, no Bevy — so any surface can call
//! them and tests can assert on the strings directly.

use crate::states::play_match::abilities::ScalingStat;
use crate::states::play_match::ability_config::{AbilityConfig, AuraEffect};
use crate::states::play_match::components::{class_base_stats, AuraType, ClassBaseStats};
use crate::states::play_match::AbilityType;

/// What [`build_ability_description`] says when none of an ability's numeric
/// or flag fields yields a sentence AND the ability applies no aura — the one
/// case where saying nothing would leave nothing to show on any surface. An
/// ability whose whole effect is its aura gets `""` instead, so the aura
/// sentence leads. A test that wants to know whether the effect half was
/// actually generated therefore compares against this AND against `""`.
pub const UTILITY_FALLBACK: &str = "Utility ability.";

/// Totem tooltip text generated from the gameplay buff spec (magnitude +
/// `TOTEM_DURATION`), so the displayed numbers always match the simulation.
/// `None` for non-totem abilities.
pub fn totem_description(ability: AbilityType) -> Option<String> {
    use crate::states::play_match::class_ai::shaman::totem_buff_spec;
    use crate::states::play_match::constants::TOTEM_DURATION;
    let (aura, mag) = totem_buff_spec(ability)?;
    let effect = match aura {
        AuraType::SpellPowerIncrease => {
            format!("increases the spell power of nearby allies by {:.0}", mag)
        }
        AuraType::AttackPowerIncrease => {
            format!("increases the attack power of nearby allies by {:.0}", mag)
        }
        AuraType::HealingOverTime => {
            format!("heals nearby allies for {:.0} health every second", mag)
        }
        AuraType::WindfuryBuff => format!(
            "gives nearby melee allies a {:.0}% chance for an extra attack",
            mag * 100.0
        ),
        _ => return None,
    };
    Some(format!("Summons a totem that {}. Lasts {:.0} sec.", effect, TOTEM_DURATION))
}

/// The ability's PRECONDITIONS, as a trailing sentence — what must be true to
/// press it, as opposed to what pressing it does.
///
/// Kept out of [`build_ability_description`] so it can be appended LAST, after
/// the aura sentence: Cheap Shot's whole effect is its stun, and a description
/// that led with "Must be stealthed." made its one-line summary say only that.
pub fn requirement_sentence(config: &AbilityConfig) -> Option<String> {
    config.requires_stealth.then(|| "Must be stealthed.".to_string())
}

/// An ability's EFFECT half, before the aura sentence and the precondition are
/// composed on top.
///
/// Split out of [`build_ability_description`] so the joined string is not the
/// only thing that knows how many effects an ability has. A summary that wants
/// to ask "is this ability's effect nothing but its damage line?"
/// ([`effect_is_damage_only`]) reads the SAME parts the sentence is built from,
/// so a new effect branch below cannot silently drift the two apart.
enum Effect {
    /// A documented override supplies the whole description — the totem spec or
    /// hand-written prose. There are no separable effect sentences.
    Authored(String),
    /// Generated: one sentence per numeric effect the ability has. Empty when
    /// the numeric fields express nothing.
    Generated(Vec<String>),
}

/// The ability's generated effect sentences, in the order they are printed.
fn effect_parts(ability: AbilityType, config: &AbilityConfig, stats: &ClassBaseStats) -> Effect {
    // Totems: generate the description straight from the gameplay buff spec so
    // the tooltip can never drift from the actual magnitude (single source of
    // truth: `class_ai::shaman::totem_spec`). Wins over everything else.
    if let Some(desc) = totem_description(ability) {
        return Effect::Authored(desc);
    }

    // Otherwise a hand-written description (abilities.ron) wins over the
    // auto-generated text — for effects the numeric config can't express (Purge).
    if !config.description.is_empty() {
        return Effect::Authored(config.description.clone());
    }

    let mut parts = Vec::new();

    // Calculate stat contribution for damage
    let damage_stat_value = match config.damage_scales_with {
        ScalingStat::AttackPower => stats.attack_power,
        ScalingStat::SpellPower => stats.spell_power,
        ScalingStat::None => 0.0,
    };
    let damage_bonus = damage_stat_value * config.damage_coefficient;

    // Calculate stat contribution for healing (uses spell power)
    let healing_bonus = stats.spell_power * config.healing_coefficient;

    // Damage
    if config.damage_base_max > 0.0 {
        let min_damage = config.damage_base_min + damage_bonus;
        let max_damage = config.damage_base_max + damage_bonus;
        if config.channel_duration.is_some() {
            // Channeled damage - show per tick
            parts.push(format!("Deals {:.0}-{:.0} damage per tick.", min_damage, max_damage));
        } else {
            parts.push(format!("Deals {:.0}-{:.0} damage.", min_damage, max_damage));
        }
    }

    // Healing
    if config.healing_base_max > 0.0 {
        let min_heal = config.healing_base_min + healing_bonus;
        let max_heal = config.healing_base_max + healing_bonus;
        parts.push(format!("Heals for {:.0}-{:.0}.", min_heal, max_heal));
    }

    // Channel healing (Drain Life style)
    if config.channel_healing_per_tick > 0.0 {
        parts.push(format!("Restores {:.0} health to the caster per tick.", config.channel_healing_per_tick));
    }

    // Interrupt
    if config.is_interrupt {
        if config.lockout_duration > 0.0 {
            parts.push(format!("Interrupts spellcasting and locks out the school for {:.1} sec.", config.lockout_duration));
        } else {
            parts.push("Interrupts spellcasting.".to_string());
        }
    }

    // Charge
    if config.is_charge {
        parts.push("Charges to the target.".to_string());
    }

    // NOTE: the stealth requirement is NOT an effect and is deliberately not
    // here — see `requirement_sentence`.

    // Dispel
    if config.is_dispel {
        parts.push("Removes one magic debuff from an ally.".to_string());
    }

    Effect::Generated(parts)
}

/// Whether the ability's effect half is NOTHING BUT its damage line.
///
/// The question a one-line summary has to ask. For Mortal Strike, Frostbolt or
/// Frost Nova the damage sentence is the boring half — the healing debuff, the
/// slow, the root is what you pick the ability for — so a summary that stops
/// after sentence one stops one sentence early. Aura-only abilities are NOT
/// this case: their aura sentence already leads, and they stay one line.
///
/// Reads [`effect_parts`] rather than re-deriving the branches, so adding an
/// effect below automatically takes an ability out of this set. Damage is
/// pushed first, so a single generated part on a damaging ability IS the
/// damage part.
pub fn effect_is_damage_only(ability: AbilityType, config: &AbilityConfig) -> bool {
    let stats = class_base_stats(config.class);
    matches!(
        effect_parts(ability, config, &stats),
        Effect::Generated(parts) if parts.len() == 1 && config.damage_base_max > 0.0
    )
}

/// Build a description string for an ability based on its config and the
/// casting class's base stats (damage and healing ranges are shown already
/// scaled, so the numbers read as what the player will see land).
///
/// EFFECTS only. The aura sentence and the precondition
/// ([`requirement_sentence`]) are composed on top by the caller, in that order.
pub fn build_ability_description(
    ability: AbilityType,
    config: &AbilityConfig,
    stats: &ClassBaseStats,
) -> String {
    let parts = match effect_parts(ability, config, stats) {
        Effect::Authored(text) => return text,
        Effect::Generated(parts) => parts,
    };

    if !parts.is_empty() {
        return parts.join(" ");
    }

    // Nothing the numeric fields can express. When the ability's whole effect
    // IS the aura it applies — half the corpus: every shout, curse, armor,
    // aura and hard CC — say NOTHING here and let the aura sentence carry the
    // meaning. Leading each of those with a filler line buried the only real
    // sentence they have, and it made the one-line summary that View Combatant
    // shows read "Utility ability." and nothing else.
    if config.applies_aura.is_some() {
        return String::new();
    }
    UTILITY_FALLBACK.to_string()
}

/// Build a description string for an aura effect
pub fn build_aura_description(aura: &AuraEffect) -> String {
    match aura.aura_type {
        AuraType::MovementSpeedSlow => {
            let slow_pct = ((1.0 - aura.magnitude) * 100.0) as i32;
            format!("Slows movement speed by {}% for {:.0} sec.", slow_pct, aura.duration)
        }
        AuraType::Root => {
            if aura.break_on_damage > 0.0 {
                format!("Roots the target for {:.0} sec. Breaks after {:.0} damage.", aura.duration, aura.break_on_damage)
            } else {
                format!("Roots the target for {:.0} sec.", aura.duration)
            }
        }
        AuraType::Stun => {
            format!("Stuns the target for {:.0} sec.", aura.duration)
        }
        AuraType::Fear => {
            format!("Causes the target to flee in fear for {:.0} sec. Breaks on damage.", aura.duration)
        }
        AuraType::Polymorph => {
            format!("Transforms the target into a sheep for {:.0} sec. Breaks on any damage.", aura.duration)
        }
        AuraType::DamageOverTime => {
            let total_ticks = (aura.duration / aura.tick_interval).ceil() as i32;
            let total_damage = aura.magnitude * total_ticks as f32;
            format!("Deals {:.0} damage over {:.0} sec.", total_damage, aura.duration)
        }
        AuraType::HealingReduction => {
            let reduction_pct = ((1.0 - aura.magnitude) * 100.0) as i32;
            format!("Reduces healing received by {}% for {:.0} sec.", reduction_pct, aura.duration)
        }
        AuraType::Absorb => {
            format!("Absorbs {:.0} damage for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::MaxHealthIncrease => {
            format!("Increases maximum health by {:.0} for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::MaxManaIncrease => {
            format!("Increases maximum mana by {:.0} for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::AttackPowerIncrease => {
            format!("Increases attack power by {:.0} for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::SpellSchoolLockout => {
            format!("Locks out a spell school for {:.0} sec.", aura.duration)
        }
        AuraType::ShadowSight => {
            format!("Reveals stealthed enemies for {:.0} sec.", aura.duration)
        }
        AuraType::WeakenedSoul => {
            format!("Cannot receive Power Word: Shield for {:.0} sec.", aura.duration)
        }
        AuraType::DamageReduction => {
            let reduction_pct = (aura.magnitude * 100.0) as i32;
            format!("Reduces physical damage dealt by {}% for {:.0} sec.", reduction_pct, aura.duration)
        }
        AuraType::CastTimeIncrease => {
            let increase_pct = (aura.magnitude * 100.0) as i32;
            format!("Increases cast time by {}% for {:.0} sec.", increase_pct, aura.duration)
        }
        AuraType::DamageTakenReduction => {
            let reduction_pct = (aura.magnitude * 100.0) as i32;
            format!("Reduces damage taken by {}% for {:.0} sec.", reduction_pct, aura.duration)
        }
        AuraType::DamageImmunity => {
            format!("Immune to all damage for {:.0} sec. Reduces damage dealt by 50%.", aura.duration)
        }
        AuraType::Incapacitate => {
            if aura.break_on_damage > 0.0 {
                format!("Incapacitates the target for {:.0} sec. Breaks on any damage.", aura.duration)
            } else {
                format!("Incapacitates the target for {:.0} sec.", aura.duration)
            }
        }
        AuraType::SpellResistanceBuff => {
            format!("Increases spell resistance by {:.0} for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::AttackPowerReduction => {
            format!("Reduces attack power by {:.0} for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::CritChanceIncrease => {
            let crit_pct = (aura.magnitude * 100.0) as i32;
            format!("Increases critical strike chance by {}% for {:.0} sec.", crit_pct, aura.duration)
        }
        AuraType::ManaRegenIncrease => {
            format!("Increases mana regeneration by {:.0}/sec for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::AttackSpeedSlow => {
            let slow_pct = (aura.magnitude * 100.0) as i32;
            format!("Reduces attack speed by {}% for {:.0} sec.", slow_pct, aura.duration)
        }
        AuraType::LockoutDurationReduction => {
            let reduction_pct = (aura.magnitude * 100.0) as i32;
            format!("Reduces interrupt lockout duration by {}% for {:.0} sec.", reduction_pct, aura.duration)
        }
        AuraType::FrostArmorBuff => {
            format!("Frost Armor active for {:.0} sec. Slows melee attackers.", aura.duration)
        }
        AuraType::Silence => {
            format!("Silenced for {:.0} sec. Cannot cast mana-cost abilities.", aura.duration)
        }
        AuraType::WeaponPoison => {
            "Weapon coated with poison. Attacks may apply a poison debuff.".to_string()
        }
        AuraType::SpellPowerIncrease => {
            format!("Increases spell power by {:.0} for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::HealingOverTime => {
            format!("Heals {:.0} every {:.0} sec for {:.0} sec.", aura.magnitude, aura.tick_interval, aura.duration)
        }
        AuraType::WindfuryBuff => {
            format!("Empowers melee auto-attacks for {:.0} sec.", aura.duration)
        }
        AuraType::FearImmunity => {
            format!("Breaks Fear and grants immunity to Fear effects for {:.0} sec. Does not affect Horror (Death Coil).", aura.duration)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::ability_config::load_ability_definitions;
    use crate::states::play_match::components::class_base_stats;

    /// Every ability in `abilities.ron` produces player-facing text, and every
    /// applied aura produces its own sentence — the property the encyclopedia's
    /// zero-marginal-cost rule depends on. Ability N+1 gets prose for free, or
    /// this fails.
    ///
    /// An ability whose whole effect is its aura returns nothing HERE by
    /// design; the aura sentence is its text. So the invariant is that one of
    /// the two speaks — never neither.
    ///
    /// "Produces text" is otherwise judged against the generator's own filler,
    /// not against `""`: an ability with no aura falls back to
    /// [`UTILITY_FALLBACK`], so a bare non-empty check would pass for it no
    /// matter what broke. The assertions that carry weight are the two around
    /// the filler:
    ///
    /// - an ability whose config declares an effect (damage, healing, an
    ///   interrupt, …) must NOT fall back — or say nothing. That is the
    ///   regression that would silently empty the EFFECT half of a
    ///   two-sentence ability — Frostbolt, Mortal Strike — while its aura
    ///   sentence kept the tooltip looking populated;
    /// - an ability that DOES fall back must carry an aura, so the filler is
    ///   never the whole story on any surface ("never neither").
    #[test]
    fn every_ability_and_aura_generates_text() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        for (ability, config) in abilities.iter() {
            let stats = class_base_stats(config.class);
            let text = build_ability_description(*ability, config, &stats);
            // The fields the generator turns into effect sentences. The stealth
            // requirement is NOT one — it is a precondition, rendered by
            // `requirement_sentence`, not an effect.
            let declares_an_effect = config.damage_base_max > 0.0
                || config.healing_base_max > 0.0
                || config.channel_healing_per_tick > 0.0
                || config.is_interrupt
                || config.is_charge
                || config.is_dispel;
            if declares_an_effect {
                assert!(
                    !text.trim().is_empty(),
                    "{:?} declares an effect but generated no effect text",
                    ability
                );
                assert_ne!(
                    text, UTILITY_FALLBACK,
                    "{:?} declares an effect but its effect text fell back to the filler",
                    ability
                );
            }

            match &config.applies_aura {
                Some(aura) => assert!(
                    !build_aura_description(aura).trim().is_empty(),
                    "{:?}'s aura generated no description",
                    ability
                ),
                None => {
                    assert!(
                        !text.trim().is_empty(),
                        "{:?} generated no description and has no aura to speak for it",
                        ability
                    );
                    assert_ne!(
                        text, UTILITY_FALLBACK,
                        "{:?} applies no aura and generated no effect text — nothing to show",
                        ability
                    );
                }
            }
        }
    }

    /// The totem text must embed the EXACT gameplay magnitude (sourced from
    /// `totem_spec` via `totem_buff_spec`), so a balance retune of a totem can
    /// never leave a tooltip stale.
    #[test]
    fn totem_text_reflects_gameplay_magnitude() {
        use crate::states::play_match::class_ai::shaman::totem_buff_spec;

        for ability in [
            AbilityType::FireTotem,
            AbilityType::EarthTotem,
            AbilityType::WaterTotem,
            AbilityType::AirTotem,
        ] {
            let desc = totem_description(ability).expect("totem has a generated description");
            let (aura, mag) = totem_buff_spec(ability).unwrap();
            // Windfury is a proc chance shown as a percent; the rest are flat.
            let shown = match aura {
                AuraType::WindfuryBuff => format!("{:.0}", mag * 100.0),
                _ => format!("{:.0}", mag),
            };
            assert!(
                desc.contains(&shown),
                "{:?} text {:?} must contain the gameplay magnitude {}",
                ability,
                desc,
                shown
            );
        }
        // Non-totem abilities get no totem description.
        assert!(totem_description(AbilityType::LightningBolt).is_none());
    }

    /// The two documented overrides, in priority order.
    #[test]
    fn totems_beat_hand_written_text_which_beats_the_generator() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");

        let totem = abilities.get_unchecked(&AbilityType::WaterTotem);
        let stats = class_base_stats(totem.class);
        assert!(
            build_ability_description(AbilityType::WaterTotem, totem, &stats).starts_with("Summons a totem"),
            "the totem spec must win"
        );

        // Mana Burn's effect is not expressible from the numeric fields, so
        // `abilities.ron` carries prose for it.
        let burn = abilities.get_unchecked(&AbilityType::ManaBurn);
        let burn_stats = class_base_stats(burn.class);
        assert!(!burn.description.is_empty(), "Mana Burn carries hand-written text");
        assert_eq!(
            build_ability_description(AbilityType::ManaBurn, burn, &burn_stats),
            burn.description
        );
    }
}
