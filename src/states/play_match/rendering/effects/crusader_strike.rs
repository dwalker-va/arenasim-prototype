//! Crusader Strike — Mortal Strike's stroke, the red trail, and TBC's strike
//! landing (graphical-only, AS-379).
//!
//! From the client data joined in
//! `docs/design/2026-10-18-paladin-talent-client-data.md` §7 (TBC 35395,
//! SpellVisual 8316):
//!
//! - **Cast** (kit 324): body anim 57, `Special1H` — the anim Mortal Strike
//!   plays — and a type-8 weapon trail, `0xF82929`, 600 ms: the same red
//!   trail as Mortal Strike's and Heroic Strike's (visual 39).
//! - **Impact** (kit 437): at TBC, `strike_impact_chest.m2` on the victim's
//!   chest, 0.67 s — gold-to-dark-red `flare` sparks and one red-grey-orange
//!   `fireplume64` puff. (Wrath points kit 437 at the decisivestrike model
//!   Heroic Strike's landing ships.)
//!
//! The user's ruling (2026-10-18, on the signed-off AS-337 Paladin Talent
//! Visuals Bench, version 1792379243-e3d6, its "Crusader Strike" tab;
//! committed as `docs/design/benches/2026-10-18-paladin-talent-bench.html`):
//! **Mortal Strike's stroke, Heroic Strike's shipped red trail, and TBC's
//! landing**, so the strike reads apart from Heroic Strike's ring. Every
//! layer at the client's values, every knob 1.0. So:
//!
//! - the stroke is `swing_style_for_ability(CrusaderStrike)` =
//!   [`SwingStyle::MortalStrike`] (`instant_ability.rs`);
//! - the trail is [`heroic_strike_trail`], laid along that stroke for as long
//!   as the blade moves (the bench's `drawStrike`, drawn `lighter`: additive);
//! - the landing is `client_landings.rs`'s `STRIKE_IMPACT_CHEST`, played
//!   through the shared impact (`SchoolImpact`) at the victim's chest.
//!
//! **A dodge or a parry** swings the stroke and lays the trail, and lands
//! nothing. The sim spawns no `InstantAbilityFired` for an avoided strike
//! (the landed-hit gate), so the avoidance is read off the sim's own
//! [`MeleeAvoided`] marker, between the phase that spawns it and the one that
//! retires it ([`note_avoided_strikes`]), and the stroke starts with the
//! landed strikes' ([`consume_avoided_strike_signals`]). Nothing on the sim
//! side changes.
//!
//! Departures from the bench, each named:
//!
//! - **The landing waits for OUR stroke.** It is spawned at the hit with its
//!   age set back by Mortal Strike's `impact_at` (0.336 s), so it plays when
//!   that stroke's blade reaches the victim; the bench's own sketched arm
//!   reaches it at 230 ms.
//! - **The trail is the shipped one.** [`heroic_strike_trail`] is
//!   `HEROIC_STRIKE_TRAIL_COLOR`, as the ruling names it, not a fresh copy of
//!   the client's `0xF82929`.
//! - **The sparks are a `Streak`, the plume a round `Glow`**, both on the
//!   client-particle kit's soft dot: the bench draws soft glows with a tail,
//!   and the plume as an upright flame ellipse (`STRIKE_IMPACT_CHEST`'s docs).
//! - **The landing rides its victim.** Its particles are children of the
//!   shared impact rig, which follows the victim, as every client landing's
//!   do; the bench's victim never moves.
//! - **The Wrath ring is not built.** The bench draws it only when its
//!   landing knob picks the decisivestrike model, which the ruling did not.
//!
//! Nothing here writes sim state or draws `game_rng`, and every system is
//! registered in `states/mod.rs` only, so headless stays byte-identical.

use bevy::prelude::*;

use super::heroic_strike::heroic_strike_trail;
use super::instant_ability::{start_signature_stroke, swing_style_for_ability};
use super::mortal_strike::spawn_weapon_trail;
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::combat_core::MeleeAvoided;
use crate::states::play_match::components::*;

/// The signature strikes that still swing when dodged or parried: the stroke
/// (and Crusader Strike's trail), never the landing. Crusader Strike and
/// Hemorrhage (AS-367: its stroke, no blood) only, each as its card says;
/// every other signature strike draws nothing for an avoided blow.
pub const STROKES_WHEN_AVOIDED: &[AbilityType] =
    &[AbilityType::CrusaderStrike, AbilityType::Hemorrhage];

/// A dodged or parried signature strike, noted off the sim's `MeleeAvoided`
/// for [`consume_avoided_strike_signals`]. Graphical-only: spawned and
/// despawned by this module's systems, never by the sim.
#[derive(Component, Clone, Copy, Debug)]
pub struct AvoidedStrikeSignal {
    pub caster: Entity,
    pub target: Entity,
    pub ability: AbilityType,
}

/// Crusader Strike's flourish, beside the stroke `style` already started on
/// `striker`'s main hand: the red trail for as long as the blade moves, and
/// — for a strike that LANDED, `landing` naming its victim and the unit
/// vector from the victim back toward the striker — `strike_impact_chest` on
/// the victim's chest when the blade arrives.
///
/// The landing is spawned now with its age set back by the stroke's
/// `impact_at`: the shared impact owes nothing until its age crosses zero,
/// so it does not go off while the weapon is still wound up.
pub fn spawn_crusader_strike_flourish(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    striker: Entity,
    landing: Option<(Entity, Vec3)>,
    is_crit: bool,
    style: SwingStyle,
) {
    spawn_weapon_trail(
        commands,
        meshes,
        materials,
        striker,
        heroic_strike_trail(),
        style.stroke_secs(),
    );
    if let Some((victim, from)) = landing {
        commands.spawn((
            SchoolImpact {
                target: victim,
                ability: AbilityType::CrusaderStrike,
                school: SpellSchool::Physical,
                anchor: ImpactAnchor::Chest,
                from,
                magnitude: 0.0,
                is_crit,
                age: -style.impact_at(),
            },
            PlayMatchEntity,
        ));
    }
}

/// FixedUpdate (graphical-only): note each dodged or parried strike in
/// [`STROKES_WHEN_AVOIDED`].
///
/// The class AI spawns the `MeleeAvoided` in the combat phase and
/// `resolve_melee_avoidances` retires it in the resolution phase, so this
/// runs between the two. The marker names the attack as the combat log does
/// — by its name in the attacker's own book — so the ability is the one in
/// that book under that name. An auto-attack's ("Auto Attack", "Heroic
/// Strike") never matches.
pub fn note_avoided_strikes(
    mut commands: Commands,
    avoided: Query<&MeleeAvoided>,
    attackers: Query<&Combatant>,
) {
    for avoid in avoided.iter() {
        let Ok(attacker) = attackers.get(avoid.attacker) else {
            continue;
        };
        let Some(ability) = STROKES_WHEN_AVOIDED.iter().copied().find(|a| {
            attacker
                .book()
                .get(a)
                .is_some_and(|def| def.name == avoid.attack)
        }) else {
            continue;
        };
        commands.spawn((
            AvoidedStrikeSignal {
                caster: avoid.attacker,
                target: avoid.defender,
                ability,
            },
            PlayMatchEntity,
        ));
    }
}

/// FixedUpdate (graphical-only): swing each avoided strike's stroke and lay
/// its trail, landing nothing, and despawn the signal.
///
/// Ordered after `consume_swing_signals`, as the landed strikes' router is, so
/// an auto-attack landing on the same tick cannot reset the socket to the
/// ordinary stroke after the signature started.
pub fn consume_avoided_strike_signals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    signals: Query<(Entity, &AvoidedStrikeSignal)>,
    mut sockets: Query<&mut WeaponSocket>,
    positions: Query<&Transform, With<Combatant>>,
) {
    for (entity, signal) in signals.iter() {
        if let Some(style) = swing_style_for_ability(signal.ability) {
            let aim = positions.get(signal.target).ok().map(|t| t.translation);
            start_signature_stroke(&mut sockets, signal.caster, aim, style);
            if signal.ability == AbilityType::CrusaderStrike {
                spawn_crusader_strike_flourish(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    signal.caster,
                    None,
                    false,
                    style,
                );
            }
        }
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every strike that strokes when avoided has a stroke to swing.
    #[test]
    fn every_avoided_stroke_has_a_style() {
        for ability in STROKES_WHEN_AVOIDED {
            assert!(swing_style_for_ability(*ability).is_some(), "{ability:?}");
        }
    }
}
