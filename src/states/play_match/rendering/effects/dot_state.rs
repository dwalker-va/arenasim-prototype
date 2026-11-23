use bevy::prelude::*;

use super::siphon_life::SIPHON_LIFE_AURA;
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
    /// Siphon Life's violet chest glow, TBC kit 234 (`siphon_life.rs`).
    SiphonLifeGlow,
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
            // Rupture has no client state; it bleeds as Rend does (AS-336).
            "Rend" | "Rupture" => Some(DotStateVisual::Drip(DripKind::Bleed)),
            CORRUPTION_AURA => Some(DotStateVisual::CorruptionShroud),
            UA_AURA => Some(DotStateVisual::UnstableAfflictionState),
            IMMOLATE_AURA => Some(DotStateVisual::ImmolateBurn),
            SIPHON_LIFE_AURA => Some(DotStateVisual::SiphonLifeGlow),
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

// ==============================================================================
// The sheep rule — one for every DoT state
// ==============================================================================
//
// A polymorphed victim wears no DoT state: every piece a state draws on it is
// hidden while it is a sheep, and its clock runs on, so the state comes back
// where it would have been when the sheep ends. The body itself is swapped for
// the sheep's (`polymorph.rs`), and the rest of the house hides what rides
// the body there — Shadow Dance, the Discipline states, Vengeance, Silence's
// breath, Expose Armor's crest — as stealth fades it. A DoT's damage breaks
// the Polymorph at its next tick, so this is a short window, but a state the
// sheep wears reads as a bug.
//
// Every entity a DoT state draws carries [`DotStateOn`]: the state's rig, or
// each world-space drop a drip emits. [`hide_dot_states_on_sheep`] is the one
// system that applies the rule; nothing else sets a state's visibility.

/// Marks an entity a DoT state draws on `victim`: the state's rig, or a drop
/// it lets fall. Its [`Visibility`] belongs to [`hide_dot_states_on_sheep`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(Visibility)]
pub struct DotStateOn {
    pub victim: Entity,
    pub kind: DotStateVisual,
}

impl DotStateOn {
    pub fn new(victim: Entity, kind: DotStateVisual) -> Self {
        DotStateOn { victim, kind }
    }
}

/// The sheep rule: a state is drawn on its victim unless the victim is a
/// sheep.
pub fn dot_state_visibility(sheep: bool) -> Visibility {
    if sheep {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    }
}

/// PostUpdate (graphical-only), before visibility propagates: hide every DoT
/// state on a polymorphed victim and show it again when the sheep ends. A
/// victim that is gone leaves the state as it was (its own cleanup takes it).
pub fn hide_dot_states_on_sheep(
    mut states: Query<(&DotStateOn, &mut Visibility)>,
    victims: Query<Has<PolymorphedVisual>, With<Combatant>>,
) {
    for (state, mut visibility) in states.iter_mut() {
        if let Ok(sheep) = victims.get(state.victim) {
            visibility.set_if_neq(dot_state_visibility(sheep));
        }
    }
}
