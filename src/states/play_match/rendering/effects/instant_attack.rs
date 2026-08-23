//! Landed instant-melee router (graphical-only).
//!
//! Mortal Strike, Ambush and Sinister Strike are true instants: they are
//! applied inline in their class AI, queued as `QueuedInstantAttack`,
//! and resolved in `combat_ai.rs`. None of them ever enters `CastingState`, so
//! the cast-completion hook the earlier signatures used (Polymorph, Fear,
//! Lightning Bolt) does not exist for them.
//!
//! Core spawns one ability-AGNOSTIC [`InstantAttackLanded`] marker per landed
//! instant hit. This module is the single place that decides which of those
//! abilities has a signature and what it looks like. Adding the next one costs
//! one arm in [`swing_style_for_ability`] (if it wants a bespoke stroke) and
//! one arm in the flourish match below — no combat-code change at all.
//!
//! Registered only in `states/mod.rs`, in `FixedUpdate` after
//! `CombatSystemPhase::CombatResolution`: `FixedUpdate` can tick several times
//! per rendered frame, and a marker consumed a tick late desyncs from its hit.

use bevy::prelude::*;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;
use super::mortal_strike::spawn_mortal_strike_flourish;

/// Height above the target's origin at which a melee hit registers.
const IMPACT_HEIGHT: f32 = 1.45;

/// The bespoke stroke an instant ability swings, if it has one.
///
/// `None` means the ability keeps whatever the auto-attack machinery was doing
/// — the correct answer for instants with no signature yet, which is most of
/// them.
pub fn swing_style_for_ability(ability: AbilityType) -> Option<SwingStyle> {
    match ability {
        AbilityType::MortalStrike => Some(SwingStyle::MortalStrike),
        _ => None,
    }
}

/// FixedUpdate (graphical-only): consume landed instant-attack markers, start
/// the styled stroke on the attacker's weapon, and fire the ability's flourish.
///
/// Every marker is despawned as it is read, signature or not, so an ability
/// with no visual never leaks marker entities.
pub fn consume_instant_attack_signals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    signals: Query<(Entity, &InstantAttackLanded)>,
    mut sockets: Query<&mut WeaponSocket>,
    positions: Query<&Transform, With<Combatant>>,
) {
    for (signal_entity, signal) in signals.iter() {
        if let Some(style) = swing_style_for_ability(signal.ability) {
            let target_pos = positions.get(signal.target).map(|t| t.translation).ok();

            // Main hand only. A styled ability stroke is a whole-body arc, so
            // an off-hand twin swinging along with it would read as two
            // weapons doing the same special.
            for mut socket in sockets.iter_mut() {
                if socket.owner != signal.attacker || socket.hand != WeaponHand::Main {
                    continue;
                }
                if let Some(pos) = target_pos {
                    socket.aim = pos;
                }
                socket.release_t = Some(0.0);
                socket.swing_style = style;
                // Start the stroke from a full windup rather than wherever the
                // auto-attack timer happened to sit. A signature arc that began
                // from a random partial depth would play at a different size
                // every cast, which reads as a broken animation rather than a
                // varied one — and this stroke REVERSES the auto's direction,
                // so a partial start can leave it beginning halfway up.
                socket.windup_s = -1.0;
            }

            // One arm per signature ability. The next melee-instant signature
            // adds its spawner here and nowhere else.
            if let Some(target_pos) = target_pos {
                let impact = target_pos + Vec3::Y * IMPACT_HEIGHT;
                match signal.ability {
                    AbilityType::MortalStrike => spawn_mortal_strike_flourish(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        signal.attacker,
                        impact,
                        signal.is_crit,
                        style.stroke_secs(),
                    ),
                    _ => {}
                }
            }
        }
        commands.entity(signal_entity).despawn();
    }
}
