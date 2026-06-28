//! Totem Components (Shaman)
//!
//! A Shaman drops up to four element totems (one per element). Each totem is a
//! grounded, non-`Combatant` entity that pulses a beneficial aura onto allies
//! within its radius (see `totems::totem_pulse_system`). Recasting an element
//! replaces the early totem — the pulse system's per-`(owner, element)` dedup
//! keeps the freshest one.

use bevy::prelude::*;

use super::super::abilities::SpellSchool;
use super::auras::AuraType;

// ============================================================================
// Totem Element
// ============================================================================

/// The four Shaman totem elements. Index/order is stable — the per-element
/// spacing offset (so the four totems fan out around the Shaman's feet) and the
/// pulse-system dedup key both depend on `index()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TotemElement {
    Air,
    Water,
    Earth,
    Fire,
}

impl TotemElement {
    /// All elements in stable order (Air, Water, Earth, Fire).
    pub const ALL: [TotemElement; 4] = [
        TotemElement::Air,
        TotemElement::Water,
        TotemElement::Earth,
        TotemElement::Fire,
    ];

    /// Short element name (for logs / trace).
    pub fn name(&self) -> &'static str {
        match self {
            TotemElement::Air => "Air",
            TotemElement::Water => "Water",
            TotemElement::Earth => "Earth",
            TotemElement::Fire => "Fire",
        }
    }

    /// Stable element index 0..4 — drives the deterministic spacing offset and
    /// the AI's per-element totem-maintenance bookkeeping.
    pub fn index(&self) -> usize {
        match self {
            TotemElement::Air => 0,
            TotemElement::Water => 1,
            TotemElement::Earth => 2,
            TotemElement::Fire => 3,
        }
    }

    /// Display name of the totem this element drops. Doubles as the buff aura's
    /// `ability_name` — the STABLE refresh key the pulse system matches on, and
    /// the name shown in `[TOTEM]` / `[BUFF]` log lines. Must match the ability
    /// `name` in `abilities.ron`.
    pub fn buff_name(&self) -> &'static str {
        match self {
            TotemElement::Air => "Windfury Totem",
            TotemElement::Water => "Healing Stream Totem",
            TotemElement::Earth => "Strength of Earth Totem",
            TotemElement::Fire => "Flametongue Totem",
        }
    }

    /// Display color for the totem's ground visual (graphical mode only).
    pub fn color(&self) -> Color {
        match self {
            TotemElement::Air => Color::srgb(0.7, 0.9, 1.0), // pale sky
            TotemElement::Water => Color::srgb(0.2, 0.5, 0.9), // blue
            TotemElement::Earth => Color::srgb(0.55, 0.42, 0.25), // brown
            TotemElement::Fire => Color::srgb(1.0, 0.4, 0.1), // orange-red
        }
    }
}

// ============================================================================
// Totem Component
// ============================================================================

/// A grounded Shaman totem. Pulses `aura_type` (with `magnitude`) onto allied
/// combatants (`team == owner_team`) within `radius`. `duration_remaining`
/// ticks down each frame in `totem_pulse_system`; the totem despawns at 0.
#[derive(Component)]
pub struct Totem {
    /// Team of the Shaman who dropped this totem.
    pub owner_team: u8,
    /// Entity of the Shaman who dropped this totem (buff caster attribution).
    pub owner: Entity,
    /// Which element this totem is (display name, color, dedup key).
    pub element: TotemElement,
    /// Allies within this distance receive the totem's buff.
    pub radius: f32,
    /// Seconds remaining before the totem expires.
    pub duration_remaining: f32,
    /// The buff aura type this totem pulses.
    pub aura_type: AuraType,
    /// Buff magnitude (flat SP/AP, per-tick heal, or proc chance 0..1).
    pub magnitude: f32,
    /// Spell school carried on the pulsed aura.
    pub spell_school: SpellSchool,
}
