//! Instant-ability gesture router (graphical-only).
//!
//! A hard cast telegraphs itself through `CastingState` and the casting orb. An
//! instant does not — it is applied inline in its class AI or resolved through
//! `combat_ai.rs`'s `QueuedInstantAttack` drain, and never enters
//! `CastingState`/`process_casting` at all. So an instant that wants an
//! actor-side animation states it by spawning an ability-AGNOSTIC
//! [`InstantAbilityFired`] marker, and this module is the single place that
//! decides which abilities have a signature and what each looks like.
//!
//! Adding the next one costs one arm in [`swing_style_for_ability`] (if it
//! wants a bespoke weapon stroke) and one arm in the flourish match (if it
//! wants particles or geometry) — no combat-code change at all.
//!
//! **The two dispatches are INDEPENDENT and must stay that way.** They were
//! nested once, with the flourish reachable only through a `Some(style)` and a
//! `Some(target_pos)`, which silently excluded two whole shapes of ability:
//! Hammer of Justice has a flourish but deliberately NO stroke (the source has
//! no hammer — see A2 in the roadmap), and Frost Nova is caster-centred with no
//! target at all. Either would have spawned its marker, been consumed, and
//! rendered nothing.
//!
//! Registered only in `states/mod.rs`, in `FixedUpdate` after
//! `CombatSystemPhase::CombatResolution`: `FixedUpdate` can tick several times
//! per rendered frame, and a marker consumed a tick late desyncs from its hit.

use bevy::prelude::*;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;
use super::mortal_strike::spawn_mortal_strike_flourish;
use super::frost_nova::spawn_frost_nova;
use super::holy_justice::spawn_holy_justice;
use super::rogue_crescents::{spawn_crescent_fan, CHEAP_SHOT_CRESCENTS, KIDNEY_SHOT_CRESCENTS};

/// Height above the target's origin at which a melee hit registers.
const IMPACT_HEIGHT: f32 = 1.45;

/// Frost Nova's point-blank radius, matching `abilities.ron`. Only used to pick
/// which enemies the wavefront tells to freeze — the sim has already decided who
/// is actually rooted.
const FROST_NOVA_RADIUS: f32 = 10.0;

/// The bespoke stroke an instant ability swings, if it has one.
///
/// `None` means the ability keeps whatever the auto-attack machinery was doing
/// — the correct answer for instants with no signature yet, which is most of
/// them.
pub fn swing_style_for_ability(ability: AbilityType) -> Option<SwingStyle> {
    match ability {
        AbilityType::MortalStrike => Some(SwingStyle::MortalStrike),
        AbilityType::CheapShot => Some(SwingStyle::CheapShot),
        AbilityType::KidneyShot => Some(SwingStyle::KidneyShot),
        // Hammer of Justice deliberately has NO stroke: the source spawns no
        // hammer and no projectile, only a ground decal and a `SpecialUnarmed`
        // gesture. Swinging the Paladin's mace would be inventing a weapon
        // attack the ability does not have.
        _ => None,
    }
}

/// FixedUpdate (graphical-only): consume instant-ability markers, start the
/// styled stroke on the caster's weapon, and fire the ability's flourish.
///
/// Every marker is despawned as it is read, signature or not, so an ability
/// with no visual never leaks marker entities.
pub fn consume_instant_ability_signals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    // Built once and reused, like the stun whirl's sparkle — the crescent is
    // identical for every slash of every rogue stun.
    mut crescent_tex: Local<Option<Handle<Image>>>,
    mut streak_tex: Local<Option<Handle<Image>>>,
    mut rune_tex: Local<Option<Handle<Image>>>,
    signals: Query<(Entity, &InstantAbilityFired)>,
    mut sockets: Query<&mut WeaponSocket>,
    positions: Query<&Transform, With<Combatant>>,
    teams: Query<(Entity, &Transform, &Combatant)>,
) {
    for (signal_entity, signal) in signals.iter() {
        let caster_pos = positions.get(signal.caster).map(|t| t.translation).ok();
        let target_pos = signal
            .target
            .and_then(|t| positions.get(t).map(|tf| tf.translation).ok());
        let style = swing_style_for_ability(signal.ability);
        let caster_team = teams.get(signal.caster).map(|(_, _, c)| c.team).unwrap_or(0);

        // ---- Weapon stroke (only for abilities that swing something) --------
        //
        // Main hand only. A styled ability stroke is a whole-body arc, so an
        // off-hand twin swinging along with it would read as two weapons doing
        // the same special. A caster with no sockets (Mage, Priest, Warlock,
        // Shaman) simply never matches — a silent no-op, never a panic.
        if let Some(style) = style {
            for mut socket in sockets.iter_mut() {
                if socket.owner != signal.caster || socket.hand != WeaponHand::Main {
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
        }

        // ---- Flourish (independent of the stroke) ---------------------------
        //
        // NOT nested inside the stroke: Hammer of Justice has a flourish and no
        // stroke, and Frost Nova has neither a stroke nor a target. Each arm
        // states which of `caster_pos` / `target_pos` it needs.
        match signal.ability {
            AbilityType::MortalStrike => {
                if let (Some(style), Some(target_pos)) = (style, target_pos) {
                    spawn_mortal_strike_flourish(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        signal.caster,
                        target_pos + Vec3::Y * IMPACT_HEIGHT,
                        signal.is_crit,
                        style.stroke_secs(),
                        style.impact_at(),
                    );
                }
            }
            AbilityType::CheapShot | AbilityType::KidneyShot => {
                if let Some(caster_pos) = caster_pos {
                    let spec = if signal.ability == AbilityType::CheapShot {
                        CHEAP_SHOT_CRESCENTS
                    } else {
                        KIDNEY_SHOT_CRESCENTS
                    };
                    spawn_crescent_fan(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut images,
                        &mut crescent_tex,
                        spec,
                        caster_pos,
                        target_pos,
                    );
                }
            }
            // No stroke — see `swing_style_for_ability`. This arm exists ONLY
            // because the flourish dispatch is independent of it.
            AbilityType::HammerOfJustice => {
                if let (Some(caster_pos), Some(target_pos)) = (caster_pos, target_pos) {
                    spawn_holy_justice(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut images,
                        &mut streak_tex,
                        &mut rune_tex,
                        caster_pos,
                        target_pos,
                    );
                }
            }
            // Caster-centred: `target` is `None` by construction, so this arm
            // is unreachable under the router's old nesting.
            AbilityType::FrostNova => {
                if let Some(caster_pos) = caster_pos {
                    // Everyone the wave will reach, so each can be told when to
                    // freeze. Read from live positions rather than passed through
                    // the marker: the AoE's victim list is a sim concern, and the
                    // marker stays ability-agnostic.
                    let victims: Vec<(Entity, Vec3)> = teams
                        .iter()
                        .filter(|(e, _, c)| *e != signal.caster && c.team != caster_team)
                        .filter(|(_, t, _)| {
                            t.translation.with_y(0.0).distance(caster_pos.with_y(0.0))
                                <= FROST_NOVA_RADIUS
                        })
                        .map(|(e, t, _)| (e, t.translation))
                        .collect();
                    spawn_frost_nova(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        signal.caster,
                        caster_pos,
                        &victims,
                    );
                }
            }
            _ => {}
        }

        commands.entity(signal_entity).despawn();
    }
}
