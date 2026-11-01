//! Triggered absorbs — an absorb shield a condition on its bearer raises.
//!
//! The shape: wearing (or holding) the source does nothing until its TRIGGER
//! holds, and then the bearer gains an `Absorb` aura of a declared SIZE, as
//! often as its LIMIT allows. Aegis of the Last Stand is the first user: below
//! 35% health, a shield of 20% of maximum health, once a match. Sacred Shield
//! (the Paladin's Protection talent, AS-421) has the same shape — on damage
//! taken, a flat shield, at most once every 6s — and is meant to arrive as a
//! new [`AbsorbTrigger`], [`AbsorbSize`] and [`TriggerLimit`] variant on this
//! mechanism, with its slot pushed by the aura that grants it instead of by
//! `apply_equipment`. Every judgement about a variant is an exhaustive `match`,
//! so a new one is asked where it fires, how big it is and how it rearms.
//!
//! ## Where it fires
//!
//! One system, [`fire_triggered_absorbs`], at the END of the frame's combat
//! resolution: after every damage site of the frame (DoT ticks, casts,
//! projectiles, traps and the auto-attacks, which are the last). It reads the
//! bearer's state, so it never touches the damage funnel
//! (`apply_damage_with_absorb`) and every damage source reaches it alike. A
//! shield raised there is up for all of the next frame's damage. The hit that
//! crosses the threshold is not absorbed — the shield is the answer to it, as
//! a trinket's is. A blow that kills from above the threshold raises nothing:
//! the bearer is dead before the system runs.
//!
//! ## Dampening
//!
//! An absorb counts as healing for arena dampening (CLAUDE.md, Combat Flow 4),
//! and the shield lands here, not through `apply_pending_auras`, so the scale
//! is applied here, at its own application site: the shield is
//! `dampening.apply(size)` and the combat log states that landed amount.
//!
//! ## Determinism
//!
//! The system draws no RNG and returns before touching anything for a bearer
//! with no slot, so a match whose combatants carry no triggered absorb plays
//! exactly as it did before this module existed.
//!
//! ## What it costs
//!
//! A triggered effect is priced at VALUE x TRIGGER RATE (talents-and-Gauntlet
//! proposal §6.5): the shield's size on a reference combatant
//! ([`TRIGGERED_ABSORB_REFERENCE_HEALTH`]), at the health weight, times the
//! fraction of combatant-lives in which it fires. The rate is DECLARED in the
//! item's data until the Gauntlet bot measures it (G4/G5), and is provisional
//! until then: see [`TriggeredAbsorbConfig::budget_cost`].

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::components::{ActiveAuras, ArenaDampening, Aura, AuraType, Combatant, DispelType, Pet};
use super::constants::{TRIGGERED_ABSORB_REFERENCE_HEALTH, WEIGHT_MAX_HEALTH};
use super::equipment::ItemId;
use super::utils::combat_log_id_for;
use crate::combat::log::{CombatLog, CombatLogEventType};

// ============================================================================
// CONFIG
// ============================================================================

/// What raises the shield.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AbsorbTrigger {
    /// The bearer is alive with its health below this fraction of its maximum
    /// health, in `(0.0, 1.0)`. Read as a LEVEL, not an edge: a bearer starts a
    /// match at full health, so the first frame it is below is the frame it
    /// dropped below.
    HealthBelow(f32),
}

/// How big the shield is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AbsorbSize {
    /// This fraction of the bearer's maximum health when it fires, in
    /// `(0.0, 1.0]`.
    MaxHealthFraction(f32),
}

impl AbsorbSize {
    /// The shield, before dampening, on a bearer of `max_health`.
    pub fn points(self, max_health: f32) -> f32 {
        match self {
            AbsorbSize::MaxHealthFraction(f) => f * max_health,
        }
    }
}

/// How often it can fire.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TriggerLimit {
    /// Once, for the rest of the match — whether or not the shield is used.
    OncePerMatch,
}

/// A triggered absorb, as `items.ron` declares it under `triggered_absorb:`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TriggeredAbsorbConfig {
    pub trigger: AbsorbTrigger,
    pub shield: AbsorbSize,
    pub limit: TriggerLimit,
    /// How long the shield lasts if it is not used up, in seconds.
    pub duration: f32,
    /// The fraction of combatant-lives in which it fires, as its PRICE assumes.
    /// Declared, not measured: provisional until the Gauntlet bot measures the
    /// rate (G4/G5), when the item's shield or threshold is sized to it.
    pub trigger_rate: f32,
}

impl TriggeredAbsorbConfig {
    /// The item-budget cost of carrying this absorb: the shield on a
    /// [`TRIGGERED_ABSORB_REFERENCE_HEALTH`] combatant, at the health weight
    /// (an absorbed point is a health point that was not lost), times
    /// [`TriggeredAbsorbConfig::trigger_rate`]. Its duration does not enter: an
    /// arena shield raised below a health threshold is under fire, and is
    /// assumed spent. Provisional with the rate; tuning waits on the milestone
    /// sweep and the bot.
    pub fn budget_cost(&self) -> f32 {
        WEIGHT_MAX_HEALTH
            * self.shield.points(TRIGGERED_ABSORB_REFERENCE_HEALTH)
            * self.trigger_rate
    }

    /// The shield aura, with `points` already dampened.
    ///
    /// `source_item` makes it wear its trinket's icon in the buff bar. An
    /// absorb is source-keyed already, so it sits beside a Power Word: Shield.
    /// `DispelType::Physical`: an item's effect is not a spell, so no purge
    /// takes it (as for a proc trinket's buff).
    pub fn aura(&self, name: &str, source_item: Option<ItemId>, points: f32) -> Aura {
        Aura {
            effect_type: AuraType::Absorb,
            duration: self.duration,
            magnitude: points,
            break_on_damage_threshold: -1.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: None,
            ability_name: name.to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None,
            applied_this_frame: true,
            backlash_damage: None,
            dr_category_override: None,
            dispel_type: DispelType::Physical,
            compound: None,
            source_item,
            stacks: None,
            bloom: None,
        }
    }
}

/// Reject a triggered absorb that cannot fire, cannot be priced, or shields
/// nothing.
pub fn validate_triggered_absorb(name: &str, config: &TriggeredAbsorbConfig) -> Result<(), String> {
    match config.trigger {
        AbsorbTrigger::HealthBelow(f) if !(f > 0.0 && f < 1.0) => {
            return Err(format!(
                "{name}: triggered absorb threshold {f} is outside (0.0, 1.0)"
            ));
        }
        AbsorbTrigger::HealthBelow(_) => {}
    }
    match config.shield {
        AbsorbSize::MaxHealthFraction(f) if !(f > 0.0 && f <= 1.0) => {
            return Err(format!(
                "{name}: triggered absorb size {f} is outside (0.0, 1.0]"
            ));
        }
        AbsorbSize::MaxHealthFraction(_) => {}
    }
    if config.duration <= 0.0 {
        return Err(format!(
            "{name}: triggered absorb duration {} must be positive",
            config.duration
        ));
    }
    if !(config.trigger_rate > 0.0 && config.trigger_rate <= 1.0) {
        return Err(format!(
            "{name}: triggered absorb trigger_rate {} is outside (0.0, 1.0]",
            config.trigger_rate
        ));
    }
    Ok(())
}

// ============================================================================
// PER-BEARER STATE
// ============================================================================

/// One triggered absorb a combatant carries, and whether it is spent.
///
/// An item's is built by `Combatant::apply_equipment`, the seam both spawn
/// paths go through.
#[derive(Clone, Debug)]
pub struct TriggeredAbsorbSlot {
    /// The shield's name: its aura's `ability_name`, what the buff bar, the
    /// combat log and the shield visual key on. An item's is the item's name.
    pub name: String,
    /// The item it comes from, if it comes from one (its icon).
    pub source_item: Option<ItemId>,
    pub config: TriggeredAbsorbConfig,
    /// Fired, under a limit that does not rearm.
    pub spent: bool,
}

impl TriggeredAbsorbSlot {
    /// Whether this slot fires for a living bearer at `health` of `max_health`.
    pub fn fires(&self, health: f32, max_health: f32) -> bool {
        let ready = match self.config.limit {
            TriggerLimit::OncePerMatch => !self.spent,
        };
        let triggered = match self.config.trigger {
            AbsorbTrigger::HealthBelow(f) => health > 0.0 && health < f * max_health,
        };
        ready && triggered
    }

    /// Mark the slot as having fired.
    fn fire(&mut self) {
        match self.config.limit {
            TriggerLimit::OncePerMatch => self.spent = true,
        }
    }
}

// ============================================================================
// THE SYSTEM
// ============================================================================

/// Raise every triggered absorb whose trigger holds, on every living bearer.
///
/// Registered at the end of Phase 3 (`systems::add_core_combat_systems`), after
/// the last damage of the frame. Logs `<bearer> gains <name> (absorbs N)` with
/// the landed (dampened) amount.
pub fn fire_triggered_absorbs(
    mut commands: Commands,
    dampening: Res<ArenaDampening>,
    mut combat_log: ResMut<CombatLog>,
    mut bearers: Query<(
        Entity,
        &mut Combatant,
        Option<&mut ActiveAuras>,
        Option<&Pet>,
    )>,
) {
    for (entity, mut combatant, mut auras, pet) in bearers.iter_mut() {
        if combatant.triggered_absorbs.is_empty() || !combatant.is_alive() {
            continue;
        }
        let (health, max_health) = (combatant.current_health, combatant.max_health);
        let mut raised = Vec::new();
        for slot in combatant.triggered_absorbs.iter_mut() {
            if !slot.fires(health, max_health) {
                continue;
            }
            slot.fire();
            let points = dampening.apply(slot.config.shield.points(max_health));
            raised.push((
                slot.name.clone(),
                slot.config.aura(&slot.name, slot.source_item, points),
            ));
        }
        if raised.is_empty() {
            continue;
        }
        let id = combat_log_id_for(&combatant, pet);
        for (name, aura) in raised {
            combat_log.log(
                CombatLogEventType::Buff,
                format!("{id} gains {name} (absorbs {:.0})", aura.magnitude),
            );
            // Fully dampened: fired and spent, but there is nothing to hold.
            if aura.magnitude <= 0.0 {
                continue;
            }
            match auras.as_mut() {
                Some(auras) => auras.auras.push(aura),
                None => {
                    commands
                        .entity(entity)
                        .insert(ActiveAuras { auras: vec![aura] });
                }
            }
        }
    }
}

// ============================================================================
// PRESENTATION
// ============================================================================

/// The player-facing sentence, shared by the item page, the item tooltip and
/// the equipment picker, and the shield's own encyclopedia entry.
pub fn triggered_absorb_description(config: &TriggeredAbsorbConfig) -> String {
    let when = match config.trigger {
        AbsorbTrigger::HealthBelow(f) => {
            format!("When your health drops below {:.0}%", f * 100.0)
        }
    };
    let size = match config.shield {
        AbsorbSize::MaxHealthFraction(f) => {
            format!("{:.0}% of your maximum health", f * 100.0)
        }
    };
    let limit = match config.limit {
        TriggerLimit::OncePerMatch => "Once a match.",
    };
    format!(
        "{when}, gain a shield absorbing {size} for {:.0}s. {limit}",
        config.duration
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aegis() -> TriggeredAbsorbConfig {
        TriggeredAbsorbConfig {
            trigger: AbsorbTrigger::HealthBelow(0.35),
            shield: AbsorbSize::MaxHealthFraction(0.20),
            limit: TriggerLimit::OncePerMatch,
            duration: 30.0,
            trigger_rate: 0.575,
        }
    }

    fn slot() -> TriggeredAbsorbSlot {
        TriggeredAbsorbSlot {
            name: "Aegis".into(),
            source_item: None,
            config: aegis(),
            spent: false,
        }
    }

    #[test]
    fn it_fires_strictly_below_the_threshold_and_never_on_a_corpse() {
        let s = slot();
        assert!(s.fires(34.9, 100.0));
        assert!(!s.fires(35.0, 100.0));
        assert!(!s.fires(90.0, 100.0));
        assert!(!s.fires(0.0, 100.0));
    }

    #[test]
    fn once_a_match_is_spent_by_firing() {
        let mut s = slot();
        s.fire();
        assert!(!s.fires(10.0, 100.0));
    }

    #[test]
    fn the_price_is_value_times_rate_on_the_reference_combatant() {
        // 20% of 420 is 84 points of shield, at 57.5%: a Champion trinket's 48.3.
        assert!(
            (aegis().budget_cost() - 48.3).abs() < 1e-3,
            "{}",
            aegis().budget_cost()
        );
    }

    #[test]
    fn validation_refuses_what_cannot_fire_or_be_priced() {
        assert!(validate_triggered_absorb("ok", &aegis()).is_ok());
        let mut c = aegis();
        c.trigger = AbsorbTrigger::HealthBelow(1.0);
        assert!(validate_triggered_absorb("x", &c).is_err());
        let mut c = aegis();
        c.shield = AbsorbSize::MaxHealthFraction(0.0);
        assert!(validate_triggered_absorb("x", &c).is_err());
        let mut c = aegis();
        c.duration = 0.0;
        assert!(validate_triggered_absorb("x", &c).is_err());
        let mut c = aegis();
        c.trigger_rate = 0.0;
        assert!(validate_triggered_absorb("x", &c).is_err());
    }

    #[test]
    fn the_sentence_states_the_numbers() {
        assert_eq!(
            triggered_absorb_description(&aegis()),
            "When your health drops below 35%, gain a shield absorbing 20% of your maximum \
             health for 30s. Once a match."
        );
    }
}
