//! The `Mechanic(TalentMechanic)` hook: bespoke talent rules, each with one
//! code site.
//!
//! The last resort among a talent's effect kinds (design §3): a rule no
//! `StatPercent`, `Patch`, `Grant`, proc or aura can express. Each variant
//! names its talent and carries its numbers, which `talents.ron` writes; its
//! behaviour lives at ONE site in the sim, named on the variant. A build
//! resolves its mechanics into [`TalentMechanics`], which the spawn writes
//! onto the combatant ([`ResolvedBuild::apply`]); the site reads them from
//! there. An empty build carries none, so no site does anything.
//!
//! **Ranks add**, as for every effect kind: a mechanic with a share takes
//! `share × rank`; one without ([`TalentMechanic::CheatDeath`]) has one rank.
//!
//! [`ResolvedBuild::apply`]: super::talents::ResolvedBuild::apply

use serde::Deserialize;

use crate::states::match_config::CharacterClass;

/// One bespoke rule. Exhaustive: a new rule is a new variant, and every match
/// on it asks where its site is.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum TalentMechanic {
    /// Soul Link: `share` of the damage the holder takes is dealt to its pet
    /// instead, while the pet lives — the damage-transfer step of
    /// `combat_core::apply_damage_with_absorb`, with the pet as receiver.
    SoulLink { share: f32 },
    /// Reflective Shield: `share` of the damage the holder's Absorb shields
    /// soak is dealt back to the attacker, credited to the holder. Its site:
    /// `apply_pending_auras` marks each shield the holder casts as it lands
    /// (`AuraDamageMods::reflect_absorbed`).
    ReflectiveShield { share: f32 },
    /// Cheat Death: the first blow of the match that would kill the holder
    /// leaves it at `health` of its maximum (where it stood, if lower), and it
    /// takes `reduction` less damage for `duration` seconds. Deterministic
    /// (TBC rolled a chance). Its site: the killing-blow step of
    /// `combat_core::apply_damage_with_absorb`.
    CheatDeath {
        health: f32,
        reduction: f32,
        duration: f32,
    },
}

/// Which mechanic a variant is, ignoring its numbers: a node names each at
/// most once, and so does a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MechanicKind {
    SoulLink,
    ReflectiveShield,
    CheatDeath,
}

impl TalentMechanic {
    /// Which mechanic this is.
    pub fn kind(&self) -> MechanicKind {
        match self {
            TalentMechanic::SoulLink { .. } => MechanicKind::SoulLink,
            TalentMechanic::ReflectiveShield { .. } => MechanicKind::ReflectiveShield,
            TalentMechanic::CheatDeath { .. } => MechanicKind::CheatDeath,
        }
    }

    /// Whether a node of `class` may carry this mechanic at `max_rank`, and
    /// whether its numbers make sense.
    pub fn validate(&self, class: CharacterClass, max_rank: u8) -> Result<(), String> {
        let fraction = |name: &str, v: f32| -> Result<(), String> {
            if v.is_finite() && v > 0.0 && v <= 1.0 {
                Ok(())
            } else {
                Err(format!("{self:?}: {name} {v} is not in (0, 1]"))
            }
        };
        match *self {
            TalentMechanic::SoulLink { share } => {
                fraction("share", share * f32::from(max_rank))?;
                if !matches!(class, CharacterClass::Warlock | CharacterClass::Hunter) {
                    return Err(format!("{self:?} needs a class with a pet, not {class:?}"));
                }
            }
            TalentMechanic::ReflectiveShield { share } => {
                fraction("share", share * f32::from(max_rank))?;
            }
            TalentMechanic::CheatDeath {
                health,
                reduction,
                duration,
            } => {
                fraction("health", health)?;
                fraction("reduction", reduction)?;
                if !duration.is_finite() || duration < 0.0 {
                    return Err(format!("{self:?}: duration {duration} is negative"));
                }
                if max_rank != 1 {
                    return Err(format!("{self:?} has one rank"));
                }
            }
        }
        Ok(())
    }

    /// The mechanic at `rank` ranks: shares scale by the rank.
    pub fn at_rank(self, rank: u8) -> TalentMechanic {
        let r = f32::from(rank);
        match self {
            TalentMechanic::SoulLink { share } => TalentMechanic::SoulLink { share: share * r },
            TalentMechanic::ReflectiveShield { share } => {
                TalentMechanic::ReflectiveShield { share: share * r }
            }
            cheat_death @ TalentMechanic::CheatDeath { .. } => cheat_death,
        }
    }
}

/// The mechanics a combatant's build gives it, read by each mechanic's site.
/// Empty for an untalented combatant.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TalentMechanics {
    list: Vec<TalentMechanic>,
}

impl TalentMechanics {
    /// Mechanics from a list, one per kind (a later one of a kind replaces
    /// an earlier one; the loader allows only one).
    pub fn of(list: &[TalentMechanic]) -> Self {
        let mut out = Self::default();
        for m in list {
            out.insert(*m);
        }
        out
    }

    pub(crate) fn insert(&mut self, mechanic: TalentMechanic) {
        self.list.retain(|m| m.kind() != mechanic.kind());
        self.list.push(mechanic);
        self.list.sort_by_key(TalentMechanic::kind);
    }

    /// Whether the combatant has none.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Every mechanic, in kind order.
    pub fn iter(&self) -> impl Iterator<Item = &TalentMechanic> {
        self.list.iter()
    }

    /// Soul Link's share, when the combatant has it.
    pub fn soul_link(&self) -> Option<f32> {
        self.list.iter().find_map(|m| match m {
            TalentMechanic::SoulLink { share } => Some(*share),
            _ => None,
        })
    }

    /// Reflective Shield's share, when the combatant has it.
    pub fn reflective_shield(&self) -> Option<f32> {
        self.list.iter().find_map(|m| match m {
            TalentMechanic::ReflectiveShield { share } => Some(*share),
            _ => None,
        })
    }

    /// Cheat Death's numbers `(health, reduction, duration)`, when the
    /// combatant has it.
    pub fn cheat_death(&self) -> Option<(f32, f32, f32)> {
        self.list.iter().find_map(|m| match m {
            TalentMechanic::CheatDeath {
                health,
                reduction,
                duration,
            } => Some((*health, *reduction, *duration)),
            _ => None,
        })
    }
}
