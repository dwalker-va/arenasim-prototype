use super::warlock_dots::{curse_spec, CORRUPTION_AURA, UA_AURA};
use crate::states::play_match::components::*;

// ==============================================================================
// DoT state routing — the ONE place a damage-over-time aura meets its visual
// ==============================================================================
//
// A DoT aura carries only its ability's RON `name:` string, so routing it to a
// state visual is exact-string matching. That used to be spread across three
// independent tables (the drip map in `affliction.rs`, the two name constants
// the Warlock detector tested, and the curse table), each with a silent
// fallthrough — which is how Immolate's 15-second burn rendered nothing
// while every test stayed green.
//
// Every renderer that draws a DoT state now asks `DotStateVisual::for_dot`, and
// `tests/lands_silently_audit.rs` sweeps every DoT in `abilities.ron` through
// the same function, so a DoT that routes nowhere fails the build instead of
// landing in silence.

/// The scene treatment a damage-over-time aura's STATE gets on its victim.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DotStateVisual {
    /// Generic affliction drips (`affliction.rs`): green poison, red bleed.
    Drip(DripKind),
    /// Corruption's darkening shroud rig (`warlock_dots.rs`).
    CorruptionShroud,
    /// Unstable Affliction's authored violet glow + crackle (`warlock_dots.rs`).
    UnstableAfflictionState,
    /// Immolate's flames licking up the victim from the feet (`immolate.rs`).
    ImmolateBurn,
    /// A curse whose client identity is its APPLY apparition alone — the era
    /// client draws nothing for the rest of the curse (`CURSE_SUSTAIN_WHISPER`
    /// in `warlock_dots.rs` records why that is deliberate). A named decision,
    /// not a fallthrough: the apparition is the visual.
    ApplyApparitionOnly(CurseKind),
}

/// Immolate's aura name (the RON `name:` string).
pub const IMMOLATE_AURA: &str = "Immolate";

impl DotStateVisual {
    /// The state visual for a `DamageOverTime` aura named `name`, or `None`
    /// when nothing draws it. `None` is what the lands-silently audit fails
    /// on, so it never means "handled elsewhere".
    pub fn for_dot(name: &str) -> Option<DotStateVisual> {
        match name {
            "Serpent Sting" => Some(DotStateVisual::Drip(DripKind::Poison)),
            "Rend" => Some(DotStateVisual::Drip(DripKind::Bleed)),
            CORRUPTION_AURA => Some(DotStateVisual::CorruptionShroud),
            UA_AURA => Some(DotStateVisual::UnstableAfflictionState),
            IMMOLATE_AURA => Some(DotStateVisual::ImmolateBurn),
            // A curse that is itself a DoT (Curse of Agony) is drawn by the
            // curse table, so derive it from that table rather than naming it
            // twice.
            _ => CurseKind::ALL
                .into_iter()
                .find(|&curse| {
                    let spec = curse_spec(curse);
                    spec.aura_type == AuraType::DamageOverTime && spec.aura_name == name
                })
                .map(DotStateVisual::ApplyApparitionOnly),
        }
    }

    /// The state visual a live aura routes to — `None` for anything that is
    /// not a DoT, whatever its name (UA's dispel-backlash silence shares its
    /// ability name with the DoT and must never count).
    pub fn for_aura(aura: &Aura) -> Option<DotStateVisual> {
        if aura.effect_type != AuraType::DamageOverTime {
            return None;
        }
        DotStateVisual::for_dot(&aura.ability_name)
    }
}

/// Does this unit carry a DoT whose state routes to `kind`?
pub fn has_dot_state(auras: Option<&ActiveAuras>, kind: DotStateVisual) -> bool {
    auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|au| DotStateVisual::for_aura(au) == Some(kind))
    })
}
