//! Melee avoidance: dodge and parry.
//!
//! A defender's [`Combatant::dodge_chance`] and [`Combatant::parry_chance`]
//! are rolled against EVERY incoming melee attack — an auto-attack swing of a
//! melee weapon (a pet's bite included) and every melee ability
//! ([`AbilityType::is_melee_attack`]). Spells and ranged attacks (Auto Shot, a
//! wand, Aimed Shot) are never avoided.
//!
//! An avoided attack deals nothing and applies nothing. What the attacker
//! spent on it stays spent: a swing's timer and its queued Heroic Strike, an
//! ability's cost, cooldown and global cooldown.
//!
//! **The zero guarantee.** [`roll_avoidance`] draws from `game_rng` only when
//! the defender has a dodge or parry chance above 0. Every class's base value
//! is 0, so a match without avoidance draws exactly the numbers it drew before
//! avoidance existed, and plays byte-identically.
//!
//! **One funnel.** Each roll site spawns a [`MeleeAvoided`] marker and nothing
//! else; [`resolve_melee_avoidances`] is the one place an avoidance is logged
//! (`[AVOID] X dodges Y's Mortal Strike`), shown as floating text over the
//! defender, and traced as a `melee_avoided` event.
//!
//! [`AbilityType::is_melee_attack`]: crate::states::play_match::abilities::AbilityType::is_melee_attack

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::super::components::{
    Combatant, FloatingCombatText, FloatingTextState, GameRng, Pet, PlayMatchEntity,
};
use super::super::decision_trace::{ActorView, DecisionTrace, TargetView};
use super::super::utils::{combat_log_id_for, get_next_fct_offset};
use super::super::FCT_HEIGHT;
use crate::combat::log::{CombatLog, CombatLogEventType};

/// How a melee attack was avoided.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Avoidance {
    Dodge,
    Parry,
}

impl Avoidance {
    /// The floating text, as Classic writes it (`GlobalStrings` DODGE / PARRY).
    pub fn label(self) -> &'static str {
        match self {
            Avoidance::Dodge => "Dodge",
            Avoidance::Parry => "Parry",
        }
    }

    /// The combat-log verb: `X dodges Y's Mortal Strike`.
    pub fn verb(self) -> &'static str {
        match self {
            Avoidance::Dodge => "dodges",
            Avoidance::Parry => "parries",
        }
    }
}

/// A defender's melee avoidance chances, as [`roll_avoidance`] reads them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeleeDefense {
    pub dodge: f32,
    pub parry: f32,
}

impl MeleeDefense {
    /// Whether there is anything to roll: a dodge or parry chance above 0.
    pub fn is_none(self) -> bool {
        self.dodge <= 0.0 && self.parry <= 0.0
    }
}

impl Combatant {
    /// This combatant's avoidance against an incoming melee attack.
    pub fn melee_defense(&self) -> MeleeDefense {
        MeleeDefense {
            dodge: self.dodge_chance,
            parry: self.parry_chance,
        }
    }
}

/// Roll one incoming melee attack against `defense`: dodged, parried, or
/// `None` (it connects).
///
/// One draw on Classic's single-roll table — dodge first, then parry — and
/// **no draw at all** while both chances are 0 ([`MeleeDefense::is_none`]):
/// that is what keeps every match without avoidance byte-identical.
pub fn roll_avoidance(defense: MeleeDefense, rng: &mut GameRng) -> Option<Avoidance> {
    if defense.is_none() {
        return None;
    }
    let roll = rng.random_f32();
    let dodge = defense.dodge.max(0.0);
    if roll < dodge {
        Some(Avoidance::Dodge)
    } else if roll < dodge + defense.parry.max(0.0) {
        Some(Avoidance::Parry)
    } else {
        None
    }
}

/// One avoided melee attack, spawned at the roll site and consumed the same
/// frame by [`resolve_melee_avoidances`]. Carries what the funnel needs to
/// log, show and trace it.
#[derive(Component, Clone, Debug)]
pub struct MeleeAvoided {
    pub attacker: Entity,
    pub defender: Entity,
    /// The attack's name, as the combat log names a landed one: "Auto
    /// Attack", "Heroic Strike", "Mortal Strike", ...
    pub attack: String,
    pub avoidance: Avoidance,
}

/// Roll `attacker`'s melee attack named `attack` against `defense`, and on an
/// avoidance spawn its [`MeleeAvoided`]. Returns the avoidance, so the caller
/// stops the attack there: no damage, no aura, no interrupt.
pub fn avoid_melee_attack(
    commands: &mut Commands,
    rng: &mut GameRng,
    defense: MeleeDefense,
    attacker: Entity,
    defender: Entity,
    attack: &str,
) -> Option<Avoidance> {
    let avoidance = roll_avoidance(defense, rng)?;
    commands.spawn(MeleeAvoided {
        attacker,
        defender,
        attack: attack.to_string(),
        avoidance,
    });
    Some(avoidance)
}

/// The combat-log line for an avoidance: `[AVOID] X dodges Y's Mortal Strike`.
pub fn avoidance_log_line(
    defender_id: &str,
    avoidance: Avoidance,
    attacker_id: &str,
    attack: &str,
) -> String {
    format!(
        "[AVOID] {} {} {}'s {}",
        defender_id,
        avoidance.verb(),
        attacker_id,
        attack
    )
}

/// The one funnel for an avoided melee attack: log it, float its word over
/// the defender, trace it, and despawn the marker. Runs in both modes (the
/// floating text is spawned headless too, like every other piece of it).
///
/// A no-op in any match without avoidance: no marker is ever spawned.
pub fn resolve_melee_avoidances(
    mut commands: Commands,
    avoided: Query<(Entity, &MeleeAvoided)>,
    combatants: Query<(&Combatant, &Transform, Option<&Pet>)>,
    mut fct_states: Query<&mut FloatingTextState>,
    mut combat_log: ResMut<CombatLog>,
    mut trace: Option<ResMut<DecisionTrace>>,
) {
    // In entity order, so the log and the floating text come out in the order
    // the sim spawned the avoidances.
    let mut markers: Vec<(Entity, &MeleeAvoided)> = avoided.iter().collect();
    markers.sort_by_key(|(e, _)| *e);
    for (marker, avoid) in markers {
        commands.entity(marker).despawn();
        let Ok((attacker, attacker_tf, attacker_pet)) = combatants.get(avoid.attacker) else {
            continue;
        };
        let Ok((defender, defender_tf, defender_pet)) = combatants.get(avoid.defender) else {
            continue;
        };
        let attacker_id = combat_log_id_for(attacker, attacker_pet);
        let defender_id = combat_log_id_for(defender, defender_pet);
        combat_log.log(
            CombatLogEventType::Damage,
            avoidance_log_line(&defender_id, avoid.avoidance, &attacker_id, &avoid.attack),
        );

        // Over the defender, where the avoided hit would have printed its
        // number: white, number-sized, the shipped lifetime and rise.
        let text_position = defender_tf.translation + Vec3::new(0.0, FCT_HEIGHT, 0.0);
        let (offset_x, offset_y) = match fct_states.get_mut(avoid.defender) {
            Ok(mut state) => get_next_fct_offset(&mut state),
            Err(_) => (0.0, 0.0),
        };
        commands.spawn((
            FloatingCombatText {
                world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                text: avoid.avoidance.label().to_string(),
                color: crate::ui::theme::fct::AVOID,
                lifetime: 1.5,
                vertical_offset: offset_y,
                is_crit: false,
            },
            PlayMatchEntity,
        ));

        if let Some(trace) = trace.as_deref_mut() {
            let pct = |cur: f32, max: f32| if max > 0.0 { cur / max } else { 0.0 };
            let actor = ActorView::from_raw(
                avoid.attacker,
                attacker.team,
                attacker.slot,
                attacker.class,
                pct(attacker.current_health, attacker.max_health),
                pct(attacker.current_mana, attacker.max_mana),
                attacker_tf.translation,
            );
            let target = TargetView {
                entity_id: avoid.defender.index(),
                class: defender.class,
                hp_pct: pct(defender.current_health, defender.max_health),
                distance: attacker_tf.translation.distance(defender_tf.translation),
            };
            trace.record_melee_avoided(actor, target, avoid.attack.clone(), avoid.avoidance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draws_left_alone(defense: MeleeDefense) -> bool {
        let mut rolled = GameRng::from_seed(7);
        let mut fresh = GameRng::from_seed(7);
        let _ = roll_avoidance(defense, &mut rolled);
        rolled.random_f32() == fresh.random_f32()
    }

    /// The zero guarantee: no draw while both chances are 0, and exactly one
    /// while either is not.
    #[test]
    fn zero_avoidance_draws_nothing() {
        let mut rng = GameRng::from_seed(7);
        assert_eq!(roll_avoidance(MeleeDefense::default(), &mut rng), None);
        assert_eq!(rng.draws(), 0, "a defender with no avoidance draws nothing");
        assert!(draws_left_alone(MeleeDefense::default()));

        let mut rng = GameRng::from_seed(7);
        let _ = roll_avoidance(
            MeleeDefense {
                dodge: 0.1,
                parry: 0.0,
            },
            &mut rng,
        );
        assert_eq!(rng.draws(), 1);
        let mut rng = GameRng::from_seed(7);
        let _ = roll_avoidance(
            MeleeDefense {
                dodge: 0.0,
                parry: 0.1,
            },
            &mut rng,
        );
        assert_eq!(rng.draws(), 1);
    }

    /// Dodge, then parry, then a hit, on the one roll.
    #[test]
    fn the_roll_reads_dodge_then_parry() {
        let mut rng = GameRng::from_seed(1);
        assert_eq!(
            roll_avoidance(
                MeleeDefense {
                    dodge: 1.0,
                    parry: 0.0
                },
                &mut rng
            ),
            Some(Avoidance::Dodge)
        );
        assert_eq!(
            roll_avoidance(
                MeleeDefense {
                    dodge: 0.0,
                    parry: 1.0
                },
                &mut rng
            ),
            Some(Avoidance::Parry)
        );
        let (mut dodges, mut parries, mut hits) = (0, 0, 0);
        let mut rng = GameRng::from_seed(3);
        for _ in 0..10_000 {
            match roll_avoidance(
                MeleeDefense {
                    dodge: 0.2,
                    parry: 0.3,
                },
                &mut rng,
            ) {
                Some(Avoidance::Dodge) => dodges += 1,
                Some(Avoidance::Parry) => parries += 1,
                None => hits += 1,
            }
        }
        // Within ~5 standard deviations of 2000 / 3000 / 5000.
        assert!((1800..2200).contains(&dodges), "dodges {dodges}");
        assert!((2770..3230).contains(&parries), "parries {parries}");
        assert!((4750..5250).contains(&hits), "hits {hits}");
    }

    #[test]
    fn the_log_line_names_defender_attacker_and_attack() {
        assert_eq!(
            avoidance_log_line(
                "Team 2 Rogue #1",
                Avoidance::Dodge,
                "Team 1 Warrior #1",
                "Mortal Strike"
            ),
            "[AVOID] Team 2 Rogue #1 dodges Team 1 Warrior #1's Mortal Strike"
        );
        assert_eq!(
            avoidance_log_line(
                "Team 2 Rogue #1",
                Avoidance::Parry,
                "Team 1 Spider #1",
                "Auto Attack"
            ),
            "[AVOID] Team 2 Rogue #1 parries Team 1 Spider #1's Auto Attack"
        );
    }
}
