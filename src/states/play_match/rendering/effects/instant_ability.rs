//! Instant-ability gesture router (graphical-only).
//!
//! A hard cast telegraphs itself through `CastingState` and the casting orb. An
//! instant does not — it is applied inline in its class AI or resolved through
//! `instant_landings.rs`'s `QueuedInstantAttack` drain, and never enters
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
//! `Some(target_pos)`, which silently excluded whole shapes of ability: one with
//! a flourish and no weapon stroke, and one that is caster-centred with no
//! target at all. Frost Nova is still exactly that second case — no stroke, no
//! target — and would spawn its marker, be consumed, and render nothing if the
//! two were ever renested. (Hammer of Justice used to be the first case too,
//! until it gained its uppercut; Frost Nova is now the only ability holding
//! that guard open, so do not "simplify" it away on the strength of one arm.)
//!
//! Registered only in `states/mod.rs`, in `FixedUpdate` after
//! `CombatSystemPhase::CombatResolution`: `FixedUpdate` can tick several times
//! per rendered frame, and a marker consumed a tick late desyncs from its hit.

use super::backhand_trail::{spawn_backhand_trail, BackhandTrailAssets};
use super::bleed_spray::spawn_bleed_spray_pending;
use super::crusader_strike::spawn_crusader_strike_flourish;
use super::expose_armor::spawn_expose_armor_pending;
use super::finisher_flash::{spawn_finisher_hand_flash, FinisherFlashAssets};
use super::frost_nova::spawn_frost_nova;
use super::holy_justice::spawn_holy_justice;
use super::life_tap::spawn_life_tap_glow;
use super::mortal_strike::spawn_mortal_strike_flourish;
use super::rogue_crescents::{spawn_crescent_fan, CHEAP_SHOT_CRESCENTS, KIDNEY_SHOT_CRESCENTS};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use bevy::prelude::*;

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
        AbilityType::CheapShot => Some(SwingStyle::CheapShot),
        AbilityType::KidneyShot => Some(SwingStyle::KidneyShot),
        // An uppercut. This ability HAD no stroke for a while, on the reasoning
        // that `HasMissile = 0` and a `SpecialUnarmed` animation name meant no
        // weapon motion — but those rule out a PROJECTILE, not a swing, and the
        // reference screenshot plainly shows the paladin in a raised-weapon pose
        // with a flash at the mace. The evidence was over-read.
        AbilityType::HammerOfJustice => Some(SwingStyle::HammerOfJustice),
        // The melee interrupts (roadmap B). Stroke-only — no flourish arm: the
        // victim side is already served by the casting orb's sputter, and the
        // source gives neither ability a caster-side effect model.
        AbilityType::Pummel => Some(SwingStyle::Pummel),
        AbilityType::Kick => Some(SwingStyle::Kick),
        // The rogue strikes (client data, 1.15.9.69547). Stroke-only, like the
        // interrupts: Ambush shares Backstab's visual wholesale (anim
        // `Attack1HPierce`, a 634ms pierce) and Sinister Strike is a plain
        // `Attack1H` with no cast model — neither has a bespoke effect model
        // to carry a flourish (Kidney Shot's crescents are what its extra
        // ModelAttach rows add; these kits have none).
        AbilityType::Ambush => Some(SwingStyle::Ambush),
        AbilityType::SinisterStrike => Some(SwingStyle::SinisterStrike),
        // Hemorrhage (AS-367) takes Sinister Strike's place in the rotation,
        // so it swings Sinister Strike's stroke. The client's Special1H and
        // its deep-red weapon trail are not drawn, as for Sinister Strike.
        AbilityType::Hemorrhage => Some(SwingStyle::SinisterStrike),
        // The apply stroke (AS-336): a finisher that puts its debuff ON the
        // victim sweeps a backhand across its chest.
        AbilityType::Rupture | AbilityType::ExposeArmor => Some(SwingStyle::Backhand),
        // Crusader Strike (TBC kit 324) plays body anim 57, Special1H — the
        // anim Mortal Strike plays — so it swings Mortal Strike's stroke
        // (AS-337 bench; `crusader_strike.rs` draws its trail and landing).
        AbilityType::CrusaderStrike => Some(SwingStyle::MortalStrike),
        _ => None,
    }
}

/// Whether a melee interrupt's stroke gives way to the stroke the hand is
/// already swinging: a strike's (Hemorrhage, Sinister Strike, Mortal Strike,
/// any signature but an interrupt's) that has not yet reached its contact.
/// The strike's contact is what its landing — a bleed spray, a crest, a
/// flinch — is timed to, so a Kick on the same frame, or one a frame after
/// it, must not take the hand away before the blade lands; the interrupt
/// still reads, on the victim, through its casting orb's sputter. A strike
/// always takes the hand, and an interrupt takes it from an auto-attack or a
/// stroke past its contact.
pub fn interrupt_yields(style: SwingStyle, swinging: SwingStyle, release_t: Option<f32>) -> bool {
    let interrupt = |s: SwingStyle| matches!(s, SwingStyle::Kick | SwingStyle::Pummel);
    interrupt(style)
        && swinging != SwingStyle::Auto
        && !interrupt(swinging)
        && release_t.is_some_and(|t| t < swinging.impact_at())
}

/// Start `style`'s stroke on `caster`'s main-hand socket, aimed at `aim` —
/// unless it is an interrupt the hand's strike keeps ([`interrupt_yields`]).
///
/// Main hand only. A styled ability stroke is a whole-body arc, so an
/// off-hand twin swinging along with it would read as two weapons doing the
/// same special. A caster with no sockets (Mage, Priest, Warlock, Shaman)
/// simply never matches — a silent no-op, never a panic.
pub(crate) fn start_signature_stroke(
    sockets: &mut Query<&mut WeaponSocket>,
    caster: Entity,
    aim: Option<Vec3>,
    style: SwingStyle,
) {
    for mut socket in sockets.iter_mut() {
        if socket.owner != caster || socket.hand != WeaponHand::Main {
            continue;
        }
        if interrupt_yields(style, socket.swing_style, socket.release_t) {
            continue;
        }
        if let Some(pos) = aim {
            socket.aim = pos;
        }
        socket.release_t = Some(0.0);
        socket.swing_style = style;
        // Start the stroke from a full windup rather than wherever the
        // auto-attack timer happened to sit. A signature arc that began from
        // a random partial depth would play at a different size every cast,
        // which reads as a broken animation rather than a varied one — and
        // this stroke REVERSES the auto's direction, so a partial start can
        // leave it beginning halfway up.
        socket.windup_s = -1.0;
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
    // Built once and reused, like the stun whirl's sparkle — the crescent's
    // opacity ladder is identical for every slash of every rogue stun.
    mut crescent_ladder: Local<Option<CrescentLadder>>,
    mut rune_tex: Local<Option<Handle<Image>>>,
    mut finisher_assets: Local<Option<FinisherFlashAssets>>,
    mut trail_assets: Local<Option<BackhandTrailAssets>>,
    signals: Query<(Entity, &InstantAbilityFired)>,
    mut sockets: Query<&mut WeaponSocket>,
    positions: Query<&Transform, With<Combatant>>,
    teams: Query<(Entity, &Transform, &Combatant)>,
    // Frost Nova's radius is GAMEPLAY data. Read it rather than restating it:
    // a hardcoded copy that drifts from `abilities.ron` denies `NovaFreezeDelay`
    // to a victim the sim did root, which freezes it instantly outside a wave
    // that never arrives — with nothing failing.
    ability_defs: Res<AbilityDefinitions>,
) {
    for (signal_entity, signal) in signals.iter() {
        let caster_pos = positions.get(signal.caster).map(|t| t.translation).ok();
        let target_pos = signal
            .target
            .and_then(|t| positions.get(t).map(|tf| tf.translation).ok());
        let style = swing_style_for_ability(signal.ability);
        let caster_team = teams
            .get(signal.caster)
            .map(|(_, _, c)| c.team)
            .unwrap_or(0);

        // ---- Weapon stroke (only for abilities that swing something) --------
        if let Some(style) = style {
            start_signature_stroke(&mut sockets, signal.caster, target_pos, style);
            // The backhand draws its sweep's trail along the hand's path,
            // whichever ability swings it.
            if style == SwingStyle::Backhand {
                spawn_backhand_trail(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut trail_assets,
                    signal.caster,
                );
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
            // The red trail along the stroke, and TBC's strike landing on
            // the victim's chest when the blade arrives (this marker is the
            // LANDED strike; a dodged or parried one strokes without it,
            // `crusader_strike.rs`).
            AbilityType::CrusaderStrike => {
                if let (Some(style), Some(victim), Some(target_pos)) =
                    (style, signal.target, target_pos)
                {
                    let from = caster_pos
                        .map(|c| (c - target_pos).normalize_or_zero())
                        .unwrap_or(Vec3::ZERO);
                    spawn_crusader_strike_flourish(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        signal.caster,
                        Some((victim, from)),
                        signal.is_crit,
                        style,
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
                        &mut crescent_ladder,
                        spec,
                        caster_pos,
                        target_pos,
                    );
                }
            }
            AbilityType::HammerOfJustice => {
                if let (Some(caster_pos), Some(target_pos)) = (caster_pos, target_pos) {
                    spawn_holy_justice(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut images,
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
                    let radius = ability_defs
                        .get(&AbilityType::FrostNova)
                        .map(|c| c.range)
                        .unwrap_or_default();
                    let victims: Vec<(Entity, Vec3)> = teams
                        .iter()
                        .filter(|(e, _, c)| *e != signal.caster && c.team != caster_team)
                        .filter(|(_, t, _)| {
                            t.translation.with_y(0.0).distance(caster_pos.with_y(0.0)) <= radius
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
            // Rupture and Expose Armor: the finisher hand flash rides the
            // backhand, and the landing waits for its contact — and for the
            // debuff to have landed, so a dodge, a parry or an immune victim
            // draws nothing on the victim. Rupture's landing is the bleed
            // spray; Expose Armor's the crest over the head (AS-366).
            AbilityType::Rupture | AbilityType::ExposeArmor => {
                if let Some(caster_pos) = caster_pos {
                    spawn_finisher_hand_flash(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut images,
                        &mut finisher_assets,
                        signal.caster,
                        caster_pos,
                        target_pos,
                    );
                }
                if let (Some(style), Some(victim)) = (style, signal.target) {
                    if let Some(aura) = ability_defs.get(&signal.ability) {
                        let landing = if signal.ability == AbilityType::Rupture {
                            spawn_bleed_spray_pending
                        } else {
                            spawn_expose_armor_pending
                        };
                        landing(
                            &mut commands,
                            victim,
                            signal.caster,
                            aura.name.clone(),
                            style.impact_at(),
                        );
                    }
                }
            }
            // Hemorrhage (AS-367): Rupture's bleed spray on the victim's chest
            // at the stroke's contact, every hit — a refresh of the stacking
            // debuff included. This marker is the LANDED strike (the drain
            // spawns it inside its landed-hit gate), so a dodge or a parry
            // strokes without it (`crusader_strike.rs`) and sprays nothing.
            AbilityType::Hemorrhage => {
                if let (Some(style), Some(victim)) = (style, signal.target) {
                    if let Some(aura) = ability_defs.get(&AbilityType::Hemorrhage) {
                        spawn_bleed_spray_pending(
                            &mut commands,
                            victim,
                            signal.caster,
                            aura.name.clone(),
                            style.impact_at(),
                        );
                    }
                }
            }
            // Caster-centred too: Life Tap lands on the Warlock's own chest.
            a if LifeTapGlow::plays_for(a) => {
                if let Some(caster_pos) = caster_pos {
                    spawn_life_tap_glow(&mut commands, signal.caster, caster_pos);
                }
            }
            _ => {}
        }

        commands.entity(signal_entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only an interrupt yields, only to a strike, and only before the
    /// strike's contact.
    #[test]
    fn an_interrupt_yields_only_to_a_strike_short_of_its_contact() {
        let contact = SwingStyle::SinisterStrike.impact_at();
        let before = Some(contact * 0.5);
        for interrupt in [SwingStyle::Kick, SwingStyle::Pummel] {
            assert!(interrupt_yields(
                interrupt,
                SwingStyle::SinisterStrike,
                before
            ));
            assert!(interrupt_yields(
                interrupt,
                SwingStyle::MortalStrike,
                Some(0.0)
            ));
            // Past contact, idle, an auto, or another interrupt: it takes the hand.
            assert!(!interrupt_yields(
                interrupt,
                SwingStyle::SinisterStrike,
                Some(contact)
            ));
            assert!(!interrupt_yields(
                interrupt,
                SwingStyle::SinisterStrike,
                None
            ));
            assert!(!interrupt_yields(interrupt, SwingStyle::Auto, before));
            assert!(!interrupt_yields(interrupt, SwingStyle::Kick, before));
            assert!(!interrupt_yields(interrupt, SwingStyle::Pummel, before));
        }
        // A strike never yields.
        assert!(!interrupt_yields(
            SwingStyle::SinisterStrike,
            SwingStyle::MortalStrike,
            before
        ));
    }
}
