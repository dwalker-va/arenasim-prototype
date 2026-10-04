//! The overhead STATUS LABELS — the white text (`STUN 2.4s`, `SHEEPED 7.1s`,
//! `CYCLONE 5.0s`) stacked above a combatant's health bar.
//!
//! **The rule: full loss of control is always labelled.** An aura that takes
//! the unit out of the fight entirely — it cannot act, so its whole kit is
//! shut off for the duration — must be readable from the arena at a glance,
//! whatever its visual. "Full loss of control" is exactly
//! [`is_incapacitating`]: Stun, Fear, Polymorph, Incapacitate, Cyclone.
//! Partial control is not held to the rule. A root still lets its victim cast
//! and a silence still lets it move and swing; both keep the labels they have,
//! and a snare never gets one (it is a speed change, read off the movement).
//!
//! [`overhead_status_label`] answers the question with an EXHAUSTIVE match on
//! [`AuraType`], so a new aura type does not build until it says whether it
//! has a label, and `every_full_loss_of_control_aura_is_labelled` fails if a
//! new full-CC type answers "none". Where one type carries several spells that
//! read differently, the label is keyed by the aura's RON `name:` (its
//! `ability_name`), the way `DotStateVisual::for_dot` routes DoTs: Death
//! Coil's Fear-type horror reads `HORROR`, a Freezing Trap's incapacitate
//! reads `FROZEN`.

use crate::states::play_match::components::{Aura, AuraType};
#[cfg(doc)]
use crate::states::play_match::utils::is_incapacitating;

/// One overhead label: its text, and where it sits in the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverheadLabel {
    /// The label, without the duration countdown the HUD appends.
    pub text: &'static str,
    /// Stack order, nearest the health bar first. Labels of equal rank keep
    /// the order their auras sit in.
    pub rank: u8,
}

const fn label(text: &'static str, rank: u8) -> Option<OverheadLabel> {
    Some(OverheadLabel { text, rank })
}

/// The overhead label `aura` shows, or `None` for an aura that shows none.
///
/// **Exhaustive on purpose — do not add a `_ =>` arm.** A new `AuraType` must
/// be classified here; one that is full loss of control must answer with a
/// label (the module doc has the rule, and the test holds it).
pub fn overhead_status_label(aura: &Aura) -> Option<OverheadLabel> {
    match aura.effect_type {
        // Full loss of control — always labelled.
        AuraType::Stun => label("STUN", 0),
        AuraType::Fear => {
            // Death Coil applies a Fear-type aura (for the flee locomotion) but
            // is a separate horror with its own DR, so it reads as one.
            if aura.ability_name == "Death Coil" {
                label("HORROR", 2)
            } else {
                label("FEAR", 2)
            }
        }
        AuraType::Polymorph => label("SHEEPED", 3),
        AuraType::Incapacitate => {
            if aura.ability_name == "Freezing Trap" {
                label("FROZEN", 4)
            } else {
                label("INCAPACITATED", 4)
            }
        }
        AuraType::Cyclone => label("CYCLONE", 5),

        // Partial control — labelled, though the rule does not require it.
        AuraType::Root => label("ROOT", 1),
        AuraType::Silence => label("SILENCE", 6),

        // Berserker Rage's fear immunity: its label is the tell for why a fear
        // did not land.
        AuraType::FearImmunity => label("BERSERK", 7),

        // No overhead label: snares, damage, stat changes, buffs and markers
        // read off the team frames' aura icons and the effect visuals.
        AuraType::MovementSpeedSlow
        | AuraType::MaxHealthIncrease
        | AuraType::DamageOverTime
        | AuraType::SpellSchoolLockout
        | AuraType::HealingReduction
        | AuraType::MaxManaIncrease
        | AuraType::AttackPowerIncrease
        | AuraType::ShadowSight
        | AuraType::Absorb
        | AuraType::WeakenedSoul
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease
        | AuraType::DamageTakenReduction
        | AuraType::DamageImmunity
        | AuraType::SpellResistanceBuff
        | AuraType::AttackPowerReduction
        | AuraType::CritChanceIncrease
        | AuraType::ManaRegenIncrease
        | AuraType::AttackSpeedSlow
        | AuraType::LockoutDurationReduction
        | AuraType::FrostArmorBuff
        | AuraType::WeaponPoison
        | AuraType::SpellPowerIncrease
        | AuraType::HealingOverTime
        | AuraType::WindfuryBuff
        | AuraType::ArmorIncrease
        | AuraType::TravelForm => None,
    }
}

/// The labels to stack above one combatant, nearest the health bar first,
/// each with the aura it counts down. One label per aura TYPE — the first
/// aura of that type — so two stuns read as one `STUN`.
pub fn overhead_status_labels(auras: &[Aura]) -> Vec<(OverheadLabel, &Aura)> {
    let mut labels: Vec<(OverheadLabel, &Aura)> = Vec::new();
    for aura in auras {
        if labels
            .iter()
            .any(|(_, shown)| shown.effect_type == aura.effect_type)
        {
            continue;
        }
        if let Some(l) = overhead_status_label(aura) {
            labels.push((l, aura));
        }
    }
    // Stable: equal ranks keep aura order.
    labels.sort_by_key(|(l, _)| l.rank);
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::ability_config::load_ability_definitions;
    use crate::states::play_match::abilities::AbilityType;
    use crate::states::play_match::traps::freezing_trap_aura;
    use crate::states::play_match::utils::is_incapacitating;
    use bevy::prelude::Entity;

    /// A probe aura of `effect_type` under `name` — what a label is keyed on.
    /// Assigned field by field rather than as an `Aura { .. }` literal, which
    /// `tests/aura_catalog_audit.rs` would read as a named apply site.
    #[allow(clippy::field_reassign_with_default)]
    fn aura(effect_type: AuraType, name: &str) -> Aura {
        let mut probe = Aura::default();
        probe.effect_type = effect_type;
        probe.ability_name = name.to_string();
        probe
    }

    /// The rule, per TYPE: every full loss-of-control aura type has a label.
    #[test]
    fn every_full_loss_of_control_aura_is_labelled() {
        let full_cc: Vec<AuraType> = AuraType::ALL
            .into_iter()
            .filter(is_incapacitating)
            .collect();
        assert_eq!(
            full_cc,
            vec![
                AuraType::Stun,
                AuraType::Fear,
                AuraType::Polymorph,
                AuraType::Incapacitate,
                AuraType::Cyclone,
            ],
            "the full loss-of-control set changed; check each new member is labelled"
        );
        for t in full_cc {
            assert!(
                overhead_status_label(&aura(t, "")).is_some(),
                "{t:?} is full loss of control but shows no overhead label"
            );
        }
    }

    /// The rule, per NAMED INSTANCE: every ability that lands a full-CC aura
    /// shows a label under its own RON name, and the name-keyed ones read as
    /// themselves — a renamed spell falls back visibly here, not silently.
    #[test]
    fn every_full_loss_of_control_spell_is_labelled_under_its_own_name() {
        let defs = load_ability_definitions().expect("abilities.ron loads");
        let mut seen: Vec<(AbilityType, &'static str)> = Vec::new();
        for (ability, cfg) in defs.iter() {
            let Some(effect) = &cfg.applies_aura else {
                continue;
            };
            if !is_incapacitating(&effect.aura_type) {
                continue;
            }
            let shown = overhead_status_label(&aura(effect.aura_type, &cfg.name))
                .unwrap_or_else(|| panic!("{ability:?} lands full CC but shows no label"));
            seen.push((*ability, shown.text));
        }
        seen.sort_by_key(|(a, _)| format!("{a:?}"));
        assert_eq!(
            seen,
            vec![
                (AbilityType::BoarCharge, "STUN"),
                (AbilityType::CheapShot, "STUN"),
                (AbilityType::Cyclone, "CYCLONE"),
                (AbilityType::DeathCoil, "HORROR"),
                (AbilityType::Fear, "FEAR"),
                (AbilityType::FreezingTrap, "FROZEN"),
                (AbilityType::HammerOfJustice, "STUN"),
                (AbilityType::KidneyShot, "STUN"),
                (AbilityType::Polymorph, "SHEEPED"),
                (AbilityType::PsychicScream, "FEAR"),
            ],
        );
    }

    /// The trap's aura is built in code, not from its RON entry; it is the one
    /// that actually lands, so it is the one that must read `FROZEN`.
    #[test]
    fn a_sprung_freezing_trap_reads_frozen() {
        let shown = overhead_status_label(&freezing_trap_aura(Entity::PLACEHOLDER));
        assert_eq!(shown.map(|l| l.text), Some("FROZEN"));
    }

    /// Partial control keeps its labels; a snare never has one.
    #[test]
    fn partial_control_is_not_full_and_snares_are_unlabelled() {
        for t in [AuraType::Root, AuraType::Silence, AuraType::MovementSpeedSlow] {
            assert!(!is_incapacitating(&t), "{t:?} is not full loss of control");
        }
        assert!(overhead_status_label(&aura(AuraType::MovementSpeedSlow, "")).is_none());
    }

    /// The stack reads nearest-first in the established order, one per type.
    #[test]
    fn labels_stack_in_rank_order_one_per_type() {
        let auras = vec![
            aura(AuraType::Cyclone, "Cyclone"),
            aura(AuraType::Root, "Entangling Roots"),
            aura(AuraType::Stun, "Kidney Shot"),
            aura(AuraType::Stun, "Hammer of Justice"),
            aura(AuraType::DamageOverTime, "Corruption"),
        ];
        let stack: Vec<(&str, &str)> = overhead_status_labels(&auras)
            .into_iter()
            .map(|(l, a)| (l.text, a.ability_name.as_str()))
            .collect();
        assert_eq!(
            stack,
            vec![
                ("STUN", "Kidney Shot"),
                ("ROOT", "Entangling Roots"),
                ("CYCLONE", "Cyclone"),
            ]
        );
    }
}
