//! Triggered absorbs: an absorb shield a condition on its bearer raises.
//!
//! The shape: carrying the source does nothing until its TRIGGER holds, and
//! then the bearer gains an `Absorb` aura of a declared SIZE, as often as its
//! LIMIT allows. Sacred Shield (the Paladin's Protection capstone, AS-421) is
//! the first user: for 30s, each time its bearer takes damage it gains a 6s
//! shield of about 40, at most once every 6s. Every judgement about a
//! variant is an exhaustive `match`, so a new source (an item's triggered
//! absorb, AS-429's Aegis of the Last Stand) is asked where it fires, how big
//! it is and how it rearms.
//!
//! ## Where a slot comes from
//!
//! A bearer carries its triggered absorbs as [`TriggeredAbsorbSlot`]s on
//! [`Combatant::triggered_absorbs`]. An aura-armed one ([`AbsorbSource::Aura`])
//! is pushed by `apply_pending_auras` when its arming aura
//! ([`AuraType::TriggeredAbsorb`], whose config rides on
//! `AuraDamageMods::triggered_absorb`) lands, and lives exactly as long as
//! that aura: the system drops it the frame the aura is gone, however it went
//! (expired, purged, its bearer cycloned out of it). A refresh of the arming
//! aura keeps the slot, so recasting never rearms it early.
//!
//! ## Where it fires
//!
//! One system, [`fire_triggered_absorbs`], at the END of the frame's combat
//! resolution: after every damage site of the frame (DoT ticks, casts,
//! projectiles, traps, the auto-attacks and the follow-ups, which are the
//! last). It reads the bearer's state, so it never touches the damage funnel
//! (`apply_damage_with_absorb`) and every damage source reaches it alike. A
//! shield raised there is up for all of the next frame's damage. The hit that
//! sets the trigger off is not absorbed: the shield is the answer to it, an
//! aura that holds until the next damage uses it (Wrath's form), not a cut
//! taken off that hit.
//!
//! ## Dampening
//!
//! An absorb counts as healing for arena dampening (CLAUDE.md, Combat Flow
//! 4), and the shield lands here, not through `apply_pending_auras`, so the
//! scale is applied here, at its own application site: the shield is
//! `dampening.apply(size)` and the combat log states that landed amount.
//!
//! ## Determinism
//!
//! The system draws no RNG and skips a bearer with no slot before touching
//! anything, so a match whose combatants carry no triggered absorb plays
//! exactly as it did before this module existed.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::components::{ActiveAuras, ArenaDampening, Aura, AuraType, Combatant, Pet};
use super::utils::combat_log_id_for;
use crate::combat::log::{CombatLog, CombatLogEventType};

// ============================================================================
// CONFIG
// ============================================================================

/// What raises the shield.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AbsorbTrigger {
    /// The bearer took damage to its health since the trigger last looked: a
    /// hit an absorb soaked whole does not count. Damage taken while the
    /// limit holds the slot is seen and spent, so it does not raise a shield
    /// the moment the slot rearms.
    DamageTaken,
}

/// How big the shield is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AbsorbSize {
    /// This many points, before dampening.
    Flat(f32),
}

impl AbsorbSize {
    /// The shield, before dampening.
    pub fn points(self) -> f32 {
        match self {
            AbsorbSize::Flat(points) => points,
        }
    }
}

/// How often it can fire.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TriggerLimit {
    /// At most once every this many seconds.
    Cooldown(f32),
}

/// A triggered absorb: what raises it, how big it is, how often, and how long
/// the shield lasts if nothing uses it up. On an ability it is written under
/// its arming aura's `triggered_absorb:`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TriggeredAbsorbConfig {
    pub trigger: AbsorbTrigger,
    pub shield: AbsorbSize,
    pub limit: TriggerLimit,
    /// How long the shield lasts if it is not used up, in seconds.
    pub duration: f32,
}

impl TriggeredAbsorbConfig {
    /// Why this config cannot fire or shields nothing, if it cannot.
    pub fn validate(&self) -> Result<(), String> {
        match self.trigger {
            AbsorbTrigger::DamageTaken => {}
        }
        match self.shield {
            AbsorbSize::Flat(points) if !(points.is_finite() && points > 0.0) => {
                return Err(format!("shield size {points} is not positive"));
            }
            AbsorbSize::Flat(_) => {}
        }
        match self.limit {
            TriggerLimit::Cooldown(secs) if !(secs.is_finite() && secs > 0.0) => {
                return Err(format!("cooldown {secs} is not positive"));
            }
            TriggerLimit::Cooldown(_) => {}
        }
        if !(self.duration.is_finite() && self.duration > 0.0) {
            return Err(format!("shield duration {} is not positive", self.duration));
        }
        Ok(())
    }
}

// ============================================================================
// PER-BEARER STATE
// ============================================================================

/// Where a slot comes from, which decides how long it lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbsorbSource {
    /// Armed by an [`AuraType::TriggeredAbsorb`] aura on the bearer named as
    /// the slot is (Sacred Shield): the slot lives while that aura does, and
    /// the shield takes the aura's caster, school and removal class.
    Aura,
}

/// One triggered absorb a combatant carries, and where its trigger and limit
/// stand.
#[derive(Clone, Debug, PartialEq)]
pub struct TriggeredAbsorbSlot {
    /// The shield's name: its aura's `ability_name`, what the buff bar, the
    /// combat log and the shield visual key on. An aura-armed slot's is its
    /// arming aura's.
    pub name: String,
    pub source: AbsorbSource,
    pub config: TriggeredAbsorbConfig,
    /// Seconds until the limit lets the slot fire again (0: ready).
    pub ready_in: f32,
    /// The bearer's `damage_taken` total when the trigger last looked.
    pub damage_seen: f32,
}

impl TriggeredAbsorbSlot {
    /// A fresh, ready slot on a bearer that has so far taken `damage_taken`.
    pub fn new(
        name: &str,
        source: AbsorbSource,
        config: TriggeredAbsorbConfig,
        damage_taken: f32,
    ) -> Self {
        Self {
            name: name.to_string(),
            source,
            config,
            ready_in: 0.0,
            damage_seen: damage_taken,
        }
    }

    /// Advance the slot `dt` seconds on `bearer` and say whether it fires
    /// now. The trigger looks every frame, ready or not, so what it saw while
    /// the limit held it is spent.
    pub fn observe(&mut self, bearer: &Combatant, dt: f32) -> bool {
        self.ready_in = (self.ready_in - dt).max(0.0);
        let triggered = match self.config.trigger {
            AbsorbTrigger::DamageTaken => bearer.damage_taken > self.damage_seen,
        };
        self.damage_seen = bearer.damage_taken;
        let ready = self.ready_in <= 0.0;
        if ready && triggered {
            match self.config.limit {
                TriggerLimit::Cooldown(secs) => self.ready_in = secs,
            }
            true
        } else {
            false
        }
    }

    /// The arming aura this slot lives by, among `auras`, if it is there.
    pub fn arming_aura<'a>(&self, auras: &'a [Aura]) -> Option<&'a Aura> {
        match self.source {
            AbsorbSource::Aura => auras.iter().find(|a| {
                a.effect_type == AuraType::TriggeredAbsorb && a.ability_name == self.name
            }),
        }
    }

    /// The shield this slot raises, with `points` already dampened, armed by
    /// `arming`: it takes that aura's caster, school and removal class (Sacred
    /// Shield's absorb is magic, as its buff is).
    pub fn shield_aura(&self, arming: &Aura, points: f32) -> Aura {
        Aura {
            effect_type: AuraType::Absorb,
            duration: self.config.duration,
            magnitude: points,
            break_on_damage_threshold: -1.0,
            caster: arming.caster,
            ability_name: self.name.clone(),
            spell_school: arming.spell_school,
            applied_this_frame: true,
            dispel_type: arming.dispel_type,
            ..Default::default()
        }
    }
}

// ============================================================================
// THE SYSTEM
// ============================================================================

/// Raise every triggered absorb whose trigger holds, on every living bearer,
/// and drop every aura-armed slot whose arming aura is gone.
///
/// Registered at the end of Phase 3 (`systems::add_core_combat_systems`),
/// after the last damage of the frame. Logs `<bearer> gains <name> (absorbs
/// N)` with the landed (dampened) amount. A shield replaces the one its slot
/// raised before, if that one is still up.
pub fn fire_triggered_absorbs(
    time: Res<Time>,
    dampening: Res<ArenaDampening>,
    mut combat_log: ResMut<CombatLog>,
    mut bearers: Query<(&mut Combatant, Option<&mut ActiveAuras>, Option<&Pet>)>,
) {
    let dt = time.delta_secs();
    for (mut combatant, auras, pet) in bearers.iter_mut() {
        if combatant.triggered_absorbs.is_empty() {
            continue;
        }
        let Some(mut auras) = auras else {
            // No aura at all: nothing arms an aura-armed slot.
            combatant.triggered_absorbs.clear();
            continue;
        };
        let mut slots = std::mem::take(&mut combatant.triggered_absorbs);
        slots.retain(|slot| slot.arming_aura(&auras.auras).is_some());
        if combatant.is_alive() {
            let mut raised = Vec::new();
            for slot in slots.iter_mut() {
                if !slot.observe(&combatant, dt) {
                    continue;
                }
                let arming = slot
                    .arming_aura(&auras.auras)
                    .expect("a kept slot's arming aura is on its bearer");
                let points = dampening.apply(slot.config.shield.points());
                raised.push(slot.shield_aura(arming, points));
            }
            let id = combat_log_id_for(&combatant, pet);
            for shield in raised {
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "{id} gains {} (absorbs {:.0})",
                        shield.ability_name, shield.magnitude
                    ),
                );
                auras.auras.retain(|a| {
                    !(a.effect_type == AuraType::Absorb && a.ability_name == shield.ability_name)
                });
                // Fully dampened: fired, but there is nothing to hold.
                if shield.magnitude > 0.0 {
                    auras.auras.push(shield);
                }
            }
        }
        combatant.triggered_absorbs = slots;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::match_config::CharacterClass;

    fn sacred_shield() -> TriggeredAbsorbConfig {
        TriggeredAbsorbConfig {
            trigger: AbsorbTrigger::DamageTaken,
            shield: AbsorbSize::Flat(40.0),
            limit: TriggerLimit::Cooldown(6.0),
            duration: 6.0,
        }
    }

    fn bearer(damage_taken: f32) -> Combatant {
        let mut c = Combatant::new(1, 0, CharacterClass::Warrior);
        c.damage_taken = damage_taken;
        c
    }

    #[test]
    fn it_fires_on_damage_taken_since_it_last_looked() {
        let mut slot = TriggeredAbsorbSlot::new("S", AbsorbSource::Aura, sacred_shield(), 10.0);
        assert!(!slot.observe(&bearer(10.0), 0.1), "no new damage");
        assert!(slot.observe(&bearer(12.0), 0.1), "new damage, ready");
    }

    #[test]
    fn the_cooldown_holds_it_and_spends_what_it_saw_meanwhile() {
        let mut slot = TriggeredAbsorbSlot::new("S", AbsorbSource::Aura, sacred_shield(), 0.0);
        assert!(slot.observe(&bearer(5.0), 0.0));
        // Damage while held: seen, not fired.
        assert!(!slot.observe(&bearer(9.0), 3.0));
        // Ready again, but nothing new since: no shield.
        assert!(!slot.observe(&bearer(9.0), 3.0));
        assert_eq!(slot.ready_in, 0.0);
        assert!(slot.observe(&bearer(9.5), 0.1));
    }

    #[test]
    fn validation_refuses_what_cannot_fire_or_shields_nothing() {
        assert!(sacred_shield().validate().is_ok());
        let mut c = sacred_shield();
        c.shield = AbsorbSize::Flat(0.0);
        assert!(c.validate().is_err());
        let mut c = sacred_shield();
        c.limit = TriggerLimit::Cooldown(0.0);
        assert!(c.validate().is_err());
        let mut c = sacred_shield();
        c.duration = 0.0;
        assert!(c.validate().is_err());
    }
}
