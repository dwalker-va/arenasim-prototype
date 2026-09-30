//! Heroic Strike's empowered swing (graphical-only).
//!
//! In the client Heroic Strike and Mortal Strike are drawn identically
//! (SpellVisual 39): a red weapon trail (procedural 356792, `0xF82A29`, 600 ms)
//! and `decisivestrike_impact_chest.m2` on the victim. By the user's decision
//! Mortal Strike keeps its own rising-diagonal stroke as the signature, and
//! Heroic Strike takes the trail and the landing on the ORDINARY auto-attack
//! arc — so the two share a colour and a landing and differ by stroke, and
//! Heroic Strike reads as the lesser blow. See
//! docs/design/2026-09-26-interrupts-and-one-offs-client-data.md.
//!
//! The empowered swing is the one whose `AutoAttackSwing` marker carries
//! [`HeroicStrikeSwing`], inserted by the sim where the queued bonus resolves.
//! Nothing here writes sim state or draws `game_rng`, and the system is
//! registered in `states/mod.rs` only, so headless stays byte-identical.

use super::mortal_strike::{spawn_weapon_trail, WeaponTrailStyle};
use super::weapon_swing::{hand_clock, swing_set, weapon_stroke_profile};
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::components::*;
use bevy::prelude::*;

/// Heroic Strike's trail colour — the client's `0xF82A29`, (248, 42, 41).
pub const HEROIC_STRIKE_TRAIL_COLOR: Color = Color::srgb(0.97, 0.16, 0.16);
/// How long the trail lingers behind the blade, seconds (the client's 600).
pub const HEROIC_STRIKE_TRAIL_LIFETIME: f32 = 0.60;
/// How wide the ribbon is along the blade, yards.
pub const HEROIC_STRIKE_TRAIL_WIDTH: f32 = 0.12;

/// Emissive gain of the trail over its colour.
const HEROIC_STRIKE_TRAIL_GLOW: f32 = 2.4;
/// Peak trail opacity, as the bench draws it.
const HEROIC_STRIKE_TRAIL_ALPHA: f32 = 0.85;

/// The trail's look, derived from the knobs above.
pub fn heroic_strike_trail() -> WeaponTrailStyle {
    let c = HEROIC_STRIKE_TRAIL_COLOR.to_srgba();
    let lin = HEROIC_STRIKE_TRAIL_COLOR.to_linear();
    WeaponTrailStyle {
        lifetime: HEROIC_STRIKE_TRAIL_LIFETIME,
        span: HEROIC_STRIKE_TRAIL_WIDTH,
        alpha: HEROIC_STRIKE_TRAIL_ALPHA,
        base_color: (c.red, c.green, c.blue),
        emissive: (
            lin.red * HEROIC_STRIKE_TRAIL_GLOW,
            lin.green * HEROIC_STRIKE_TRAIL_GLOW,
            lin.blue * HEROIC_STRIKE_TRAIL_GLOW,
        ),
    }
}

/// FixedUpdate (graphical-only): dress each empowered swing.
///
/// Lays the red trail along the attacker's ordinary release stroke, and lands
/// `decisivestrike_impact_chest` on the victim through the shared impact
/// (`client_landings.rs`). The landing is spawned with its age set back by the
/// stroke's `impact_at`, so it plays when the blade reaches the victim, not
/// while the weapon is still wound up — the rig emits nothing until its age
/// crosses zero.
///
/// Runs BEFORE `consume_swing_signals`, which despawns the marker. FixedUpdate
/// for the same reason that one is: several ticks can fall in one rendered
/// frame.
pub fn spawn_heroic_strike_flourish(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    swings: Query<&AutoAttackSwing, With<HeroicStrikeSwing>>,
    positions: Query<&Transform, With<Combatant>>,
    clocks: Query<(&Combatant, Option<&ActiveAuras>)>,
) {
    for swing in swings.iter() {
        // The empowered blow rides the ordinary auto stroke, which scales with
        // the weapon swinging it — read the same hand interval
        // `consume_swing_signals` freezes for that stroke, so the trail and
        // the landing stay in step with the blade.
        let interval = clocks
            .get(swing.attacker)
            .ok()
            .and_then(|(combatant, auras)| {
                hand_clock(combatant, auras, swing.hand, swing_set(swing.kind))
            })
            .map_or(0.0, |(_, interval)| interval);
        let stroke = weapon_stroke_profile(interval);
        spawn_weapon_trail(
            &mut commands,
            &mut meshes,
            &mut materials,
            swing.attacker,
            heroic_strike_trail(),
            stroke.total(),
        );
        let from = match (positions.get(swing.target), positions.get(swing.attacker)) {
            (Ok(v), Ok(a)) => (a.translation - v.translation).normalize_or_zero(),
            _ => Vec3::ZERO,
        };
        commands.spawn((
            SchoolImpact {
                target: swing.target,
                ability: AbilityType::HeroicStrike,
                school: SpellSchool::Physical,
                anchor: ImpactAnchor::Chest,
                from,
                magnitude: 0.0,
                is_crit: swing.is_crit,
                age: -stroke.impact_at(),
            },
            PlayMatchEntity,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trail_is_the_client_red() {
        let style = heroic_strike_trail();
        let (r, g, b) = style.base_color;
        assert!((r - 248.0 / 255.0).abs() < 0.01);
        assert!((g - 42.0 / 255.0).abs() < 0.02);
        assert!((b - 41.0 / 255.0).abs() < 0.02);
        assert_eq!(style.lifetime, 0.60);
    }
}
