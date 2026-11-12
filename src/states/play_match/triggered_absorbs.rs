//! Triggered absorbs — an absorb shield a condition on its bearer raises.
//!
//! The shape: carrying the source does nothing until its TRIGGER holds, and
//! then the bearer gains an `Absorb` aura of a declared SIZE, as often as its
//! LIMIT allows. Two sources share the one mechanism:
//!
//! - **An item** (Aegis of the Last Stand, AS-429): below 35% health, a shield
//!   of 20% of maximum health, once a match. Its slot is pushed by
//!   `Combatant::apply_equipment` and lives all match.
//! - **An aura** (Sacred Shield, the Paladin's Protection capstone, AS-421):
//!   for 30s, each time damage reaches its bearer's health, a shield of 40, at
//!   most once every 6s. Its slot is pushed by `apply_pending_auras` when the
//!   arming aura ([`AuraType::TriggeredAbsorb`], whose config rides on
//!   `AuraDamageMods::triggered_absorb`) lands, and lives exactly as long as
//!   that aura: the system drops it the frame the aura is gone, however it
//!   went. A refresh of the arming aura keeps the slot, so recasting never
//!   re-arms it early.
//!
//! Every judgement about a variant — [`AbsorbTrigger`], [`AbsorbSize`],
//! [`TriggerLimit`], [`AbsorbSource`] — is an exhaustive `match`, so a new one
//! is asked where it fires, how big it is, how it rearms and how long it lives.
//!
//! ## Where it fires
//!
//! One system, [`fire_triggered_absorbs`], at the END of the frame's combat
//! resolution: after every damage site of the frame (DoT ticks, casts,
//! projectiles, traps, the auto-attacks) and after the follow-ups those hits
//! set off (`process_damage_follow_ups`: transfers, reflects, a save's heal).
//! It reads the bearer's state, so it never touches the damage funnel
//! (`apply_damage_with_absorb`) and every damage source reaches it alike. A
//! shield raised there is up for all of the next frame's damage. The hit that
//! sets the trigger off is not absorbed — the shield is the answer to it. A
//! blow that kills raises nothing: the bearer is dead before the system runs.
//! A shield raised again replaces its slot's earlier one if it is still up.
//!
//! ## How it meets the damage pipeline
//!
//! The shield is an ordinary [`AuraType::Absorb`] aura, so the funnel soaks
//! with it where it soaks every absorb (its step 5): after the attacker's
//! damage-dealt increase, armor or resistance, the damage-taken modifiers and
//! a transfer's share, and before the killing-blow save and health. So it
//! soaks the hit as the pipeline has made it, not as its source rolled it; a
//! transferred hit (Blessing of Sacrifice, Soul Link) skips it, as it skips
//! every absorb; and a blow it soaks below the bearer's health spends no save.
//! Because the system runs after the follow-ups, damage a transfer or a
//! reflect lands raises the shield on the same frame as a direct hit would,
//! and a killing-blow save is judged before it: Cheat Death, which leaves its
//! Rogue alive below the line, raises an Aegis on the frame of the blow; a
//! save that leaves its holder where it stood raises nothing unless it stood
//! below the line already.
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
//! The system draws no RNG and skips a bearer with no slot before touching
//! anything, so a match whose combatants carry no triggered absorb plays
//! exactly as it did before this module existed.
//!
//! ## What an item's costs
//!
//! An item's triggered absorb ([`ItemTriggeredAbsorb`]) is priced at VALUE x
//! TRIGGER RATE (talents-and-Gauntlet proposal §6.5): the shield's size on a
//! reference combatant ([`TRIGGERED_ABSORB_REFERENCE_HEALTH`]), at the health
//! weight, times the fraction of combatant-lives in which it fires. The rate is
//! DECLARED in the item's data until the Gauntlet bot measures it (G4/G5), and
//! is provisional until then: see [`ItemTriggeredAbsorb::budget_cost`]. A
//! talent's (Sacred Shield) is priced by its node.

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
    /// dropped below. (Aegis of the Last Stand.)
    HealthBelow(f32),
    /// The bearer took damage to its health since the trigger last looked: a
    /// hit an absorb soaked whole does not count. Damage taken while the
    /// limit holds the slot is seen and spent, so it does not raise a shield
    /// the moment the slot rearms. (Sacred Shield.)
    DamageTaken,
}

/// How big the shield is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AbsorbSize {
    /// This fraction of the bearer's maximum health when it fires, in
    /// `(0.0, 1.0]`. (Aegis of the Last Stand.)
    MaxHealthFraction(f32),
    /// This many points. (Sacred Shield.)
    Flat(f32),
}

impl AbsorbSize {
    /// The shield, before dampening, on a bearer of `max_health`.
    pub fn points(self, max_health: f32) -> f32 {
        match self {
            AbsorbSize::MaxHealthFraction(f) => f * max_health,
            AbsorbSize::Flat(points) => points,
        }
    }
}

/// How often it can fire.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TriggerLimit {
    /// Once, for the rest of the match — whether or not the shield is used.
    OncePerMatch,
    /// At most once every this many seconds.
    Cooldown(f32),
}

impl TriggerLimit {
    /// Seconds until a slot that just fired may fire again: forever, or the
    /// cooldown.
    fn rearm(self) -> f32 {
        match self {
            TriggerLimit::OncePerMatch => f32::INFINITY,
            TriggerLimit::Cooldown(secs) => secs,
        }
    }
}

/// A triggered absorb: what raises it, how big it is, how often, and how long
/// the shield lasts if nothing uses it up. Shared by every source: an item
/// declares it inside its [`ItemTriggeredAbsorb`], an ability under its arming
/// aura's `triggered_absorb:`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TriggeredAbsorbConfig {
    pub trigger: AbsorbTrigger,
    pub shield: AbsorbSize,
    pub limit: TriggerLimit,
    /// How long the shield lasts if it is not used up, in seconds.
    pub duration: f32,
}

/// Where a shield's aura takes its source from.
#[derive(Clone, Copy)]
pub enum ShieldFrom<'a> {
    /// An item's: no caster, wearing the item's icon (`source_item`), and
    /// `DispelType::Physical` — an item's effect is not a spell, so no purge
    /// takes it (as for a proc trinket's buff).
    Item(ItemId),
    /// An aura's: its caster, school and removal class are the arming aura's
    /// (Sacred Shield's absorb is magic, as its buff is).
    Aura(&'a Aura),
}

impl TriggeredAbsorbConfig {
    /// The shield aura `name` raises from `from`, with `points` already
    /// dampened. An absorb is source-keyed already, so it sits beside a Power
    /// Word: Shield.
    pub fn shield_aura(&self, name: &str, from: ShieldFrom, points: f32) -> Aura {
        let mut shield = Aura {
            effect_type: AuraType::Absorb,
            duration: self.duration,
            magnitude: points,
            break_on_damage_threshold: -1.0,
            ability_name: name.to_string(),
            applied_this_frame: true,
            ..Default::default()
        };
        match from {
            ShieldFrom::Item(item) => {
                shield.dispel_type = DispelType::Physical;
                shield.source_item = Some(item);
            }
            ShieldFrom::Aura(arming) => {
                shield.caster = arming.caster;
                shield.spell_school = arming.spell_school;
                shield.dispel_type = arming.dispel_type;
            }
        }
        shield
    }
}

/// Reject a triggered absorb that cannot fire or shields nothing, naming it
/// `name`.
pub fn validate_triggered_absorb(name: &str, config: &TriggeredAbsorbConfig) -> Result<(), String> {
    match config.trigger {
        AbsorbTrigger::HealthBelow(f) if !(f > 0.0 && f < 1.0) => {
            return Err(format!(
                "{name}: triggered absorb threshold {f} is outside (0.0, 1.0)"
            ));
        }
        AbsorbTrigger::HealthBelow(_) | AbsorbTrigger::DamageTaken => {}
    }
    match config.shield {
        AbsorbSize::MaxHealthFraction(f) if !(f > 0.0 && f <= 1.0) => {
            return Err(format!(
                "{name}: triggered absorb size {f} is outside (0.0, 1.0]"
            ));
        }
        AbsorbSize::Flat(points) if !(points.is_finite() && points > 0.0) => {
            return Err(format!(
                "{name}: triggered absorb size {points} is not positive"
            ));
        }
        AbsorbSize::MaxHealthFraction(_) | AbsorbSize::Flat(_) => {}
    }
    match config.limit {
        TriggerLimit::Cooldown(secs) if !(secs.is_finite() && secs > 0.0) => {
            return Err(format!(
                "{name}: triggered absorb cooldown {secs} is not positive"
            ));
        }
        TriggerLimit::OncePerMatch | TriggerLimit::Cooldown(_) => {}
    }
    if !(config.duration.is_finite() && config.duration > 0.0) {
        return Err(format!(
            "{name}: triggered absorb duration {} must be positive",
            config.duration
        ));
    }
    Ok(())
}

/// An item's triggered absorb, as `items.ron` declares it under
/// `triggered_absorb:`: the shared config and the rate its price assumes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemTriggeredAbsorb {
    pub absorb: TriggeredAbsorbConfig,
    /// The fraction of combatant-lives in which it fires, as its PRICE assumes.
    /// Declared, not measured: provisional until the Gauntlet bot measures the
    /// rate (G4/G5), when the item's shield or threshold is sized to it.
    pub trigger_rate: f32,
}

impl ItemTriggeredAbsorb {
    /// The item-budget cost of carrying this absorb: the shield on a
    /// [`TRIGGERED_ABSORB_REFERENCE_HEALTH`] combatant, at the health weight
    /// (an absorbed point is a health point that was not lost), times
    /// [`Self::trigger_rate`]. Its duration does not enter: an arena shield
    /// raised below a health threshold is under fire, and is assumed spent.
    /// Provisional with the rate; tuning waits on the milestone sweep and the
    /// bot.
    pub fn budget_cost(&self) -> f32 {
        WEIGHT_MAX_HEALTH
            * self.absorb.shield.points(TRIGGERED_ABSORB_REFERENCE_HEALTH)
            * self.trigger_rate
    }

    /// [`validate_triggered_absorb`], and a rate that can price it.
    pub fn validate(&self, name: &str) -> Result<(), String> {
        validate_triggered_absorb(name, &self.absorb)?;
        if !(self.trigger_rate > 0.0 && self.trigger_rate <= 1.0) {
            return Err(format!(
                "{name}: triggered absorb trigger_rate {} is outside (0.0, 1.0]",
                self.trigger_rate
            ));
        }
        Ok(())
    }
}

// ============================================================================
// PER-BEARER STATE
// ============================================================================

/// Where a slot comes from, which decides how long it lives and what its
/// shield is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbsorbSource {
    /// An item the bearer wears: the slot lives all match.
    Item(ItemId),
    /// Armed by an [`AuraType::TriggeredAbsorb`] aura on the bearer named as
    /// the slot is (Sacred Shield): the slot lives while that aura does.
    Aura,
}

/// One triggered absorb a combatant carries, and where its trigger and limit
/// stand.
#[derive(Clone, Debug, PartialEq)]
pub struct TriggeredAbsorbSlot {
    /// The shield's name: its aura's `ability_name`, what the buff bar, the
    /// combat log and the shield visual key on. An item's is the item's name;
    /// an aura-armed one's is its arming aura's.
    pub name: String,
    pub source: AbsorbSource,
    pub config: TriggeredAbsorbConfig,
    /// Seconds until the limit lets the slot fire again (0: ready; infinite:
    /// spent for the match).
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

    /// Whether the slot has fired under a limit that never rearms.
    pub fn spent(&self) -> bool {
        self.ready_in.is_infinite()
    }

    /// Advance the slot `dt` seconds on `bearer` and say whether it fires
    /// now; a slot that fires starts its limit. The trigger looks every
    /// frame, ready or not, so what it saw while the limit held it is spent.
    pub fn observe(&mut self, bearer: &Combatant, dt: f32) -> bool {
        self.ready_in = (self.ready_in - dt).max(0.0);
        let triggered = match self.config.trigger {
            AbsorbTrigger::HealthBelow(f) => {
                bearer.current_health > 0.0 && bearer.current_health < f * bearer.max_health
            }
            AbsorbTrigger::DamageTaken => bearer.damage_taken > self.damage_seen,
        };
        self.damage_seen = bearer.damage_taken;
        if self.ready_in <= 0.0 && triggered {
            self.ready_in = self.config.limit.rearm();
            true
        } else {
            false
        }
    }

    /// The arming aura an aura-armed slot lives by, among `auras`, if it is
    /// there.
    pub fn arming_aura<'a>(&self, auras: &'a [Aura]) -> Option<&'a Aura> {
        auras
            .iter()
            .find(|a| a.effect_type == AuraType::TriggeredAbsorb && a.ability_name == self.name)
    }

    /// Whether the slot still lives on a bearer holding `auras` (`None`: no
    /// aura list): an item's always, an aura-armed one while its arming aura
    /// is there.
    fn lives(&self, auras: Option<&[Aura]>) -> bool {
        match self.source {
            AbsorbSource::Item(_) => true,
            AbsorbSource::Aura => auras.is_some_and(|a| self.arming_aura(a).is_some()),
        }
    }

    /// The shield this slot raises, `points` strong (already dampened), on a
    /// bearer holding `auras`.
    fn shield(&self, auras: Option<&[Aura]>, points: f32) -> Option<Aura> {
        let from = match self.source {
            AbsorbSource::Item(item) => ShieldFrom::Item(item),
            AbsorbSource::Aura => ShieldFrom::Aura(self.arming_aura(auras?)?),
        };
        Some(self.config.shield_aura(&self.name, from, points))
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
/// N)` with the landed (dampened) amount. A slot's shield replaces the one it
/// raised before, if that one is still up. Without a `Time` resource (a test
/// world) no time passes.
pub fn fire_triggered_absorbs(
    mut commands: Commands,
    time: Option<Res<Time>>,
    dampening: Res<ArenaDampening>,
    mut combat_log: ResMut<CombatLog>,
    mut bearers: Query<(
        Entity,
        &mut Combatant,
        Option<&mut ActiveAuras>,
        Option<&Pet>,
    )>,
) {
    let dt = time.map_or(0.0, |t| t.delta_secs());
    for (entity, mut combatant, mut auras, pet) in bearers.iter_mut() {
        if combatant.triggered_absorbs.is_empty() {
            continue;
        }
        let mut slots = std::mem::take(&mut combatant.triggered_absorbs);
        slots.retain(|slot| slot.lives(auras.as_ref().map(|a| a.auras.as_slice())));
        if combatant.is_alive() {
            let max_health = combatant.max_health;
            let mut raised = Vec::new();
            for slot in slots.iter_mut() {
                if !slot.observe(&combatant, dt) {
                    continue;
                }
                let points = dampening.apply(slot.config.shield.points(max_health));
                if let Some(shield) =
                    slot.shield(auras.as_ref().map(|a| a.auras.as_slice()), points)
                {
                    raised.push(shield);
                }
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
                land_shield(&mut commands, entity, auras.as_deref_mut(), shield);
            }
        }
        combatant.triggered_absorbs = slots;
    }
}

/// Land `shield` on `bearer` in place of its slot's earlier one. A fully
/// dampened shield (fired and spent) lands nothing. A bearer with no aura list
/// yet may be given one by another system's command this frame too, so the
/// shield merges into whatever is there when the command lands rather than
/// inserting over it.
fn land_shield(
    commands: &mut Commands,
    bearer: Entity,
    auras: Option<&mut ActiveAuras>,
    shield: Aura,
) {
    let same =
        |a: &Aura| a.effect_type == AuraType::Absorb && a.ability_name == shield.ability_name;
    match auras {
        Some(auras) => {
            auras.auras.retain(|a| !same(a));
            if shield.magnitude > 0.0 {
                auras.auras.push(shield);
            }
        }
        None if shield.magnitude > 0.0 => {
            commands.queue(
                move |world: &mut World| match world.get_mut::<ActiveAuras>(bearer) {
                    Some(mut auras) => auras.auras.push(shield),
                    None => {
                        if let Ok(mut entity) = world.get_entity_mut(bearer) {
                            entity.insert(ActiveAuras {
                                auras: vec![shield],
                            });
                        }
                    }
                },
            );
        }
        None => {}
    }
}

// ============================================================================
// PRESENTATION
// ============================================================================

/// The player-facing sentence for an item's triggered absorb, shared by the
/// item page, the item tooltip and the equipment picker, and the shield's own
/// encyclopedia entry.
pub fn triggered_absorb_description(config: &TriggeredAbsorbConfig) -> String {
    let when = match config.trigger {
        AbsorbTrigger::HealthBelow(f) => {
            format!("When your health drops below {:.0}%", f * 100.0)
        }
        AbsorbTrigger::DamageTaken => "Each time you take damage".to_string(),
    };
    let size = match config.shield {
        AbsorbSize::MaxHealthFraction(f) => {
            format!("{:.0}% of your maximum health", f * 100.0)
        }
        AbsorbSize::Flat(points) => format!("{points:.0} damage"),
    };
    let limit = match config.limit {
        TriggerLimit::OncePerMatch => "Once a match.".to_string(),
        TriggerLimit::Cooldown(secs) => format!("At most once every {secs:.0}s."),
    };
    format!(
        "{when}, gain a shield absorbing {size} for {:.0}s. {limit}",
        config.duration
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::match_config::CharacterClass;

    fn aegis() -> ItemTriggeredAbsorb {
        ItemTriggeredAbsorb {
            absorb: TriggeredAbsorbConfig {
                trigger: AbsorbTrigger::HealthBelow(0.35),
                shield: AbsorbSize::MaxHealthFraction(0.20),
                limit: TriggerLimit::OncePerMatch,
                duration: 30.0,
            },
            trigger_rate: 0.575,
        }
    }

    fn sacred_shield() -> TriggeredAbsorbConfig {
        TriggeredAbsorbConfig {
            trigger: AbsorbTrigger::DamageTaken,
            shield: AbsorbSize::Flat(40.0),
            limit: TriggerLimit::Cooldown(6.0),
            duration: 6.0,
        }
    }

    fn bearer(health: f32, damage_taken: f32) -> Combatant {
        let mut c = Combatant::new(1, 0, CharacterClass::Warrior);
        c.max_health = 100.0;
        c.current_health = health;
        c.damage_taken = damage_taken;
        c
    }

    fn aegis_slot() -> TriggeredAbsorbSlot {
        TriggeredAbsorbSlot::new(
            "Aegis",
            AbsorbSource::Item(ItemId::AegisOfTheLastStand),
            aegis().absorb,
            0.0,
        )
    }

    #[test]
    fn health_below_fires_strictly_below_the_threshold_and_never_on_a_corpse() {
        for (health, fires) in [(34.9, true), (35.0, false), (90.0, false), (0.0, false)] {
            let mut s = aegis_slot();
            assert_eq!(s.observe(&bearer(health, 0.0), 0.0), fires, "{health}");
        }
    }

    #[test]
    fn once_a_match_is_spent_by_firing() {
        let mut s = aegis_slot();
        assert!(s.observe(&bearer(10.0, 0.0), 0.0));
        assert!(s.spent());
        assert!(!s.observe(&bearer(10.0, 0.0), 1000.0));
    }

    #[test]
    fn damage_taken_fires_on_damage_since_it_last_looked() {
        let mut slot = TriggeredAbsorbSlot::new("S", AbsorbSource::Aura, sacred_shield(), 10.0);
        assert!(!slot.observe(&bearer(90.0, 10.0), 0.1), "no new damage");
        assert!(slot.observe(&bearer(88.0, 12.0), 0.1), "new damage, ready");
    }

    #[test]
    fn the_cooldown_holds_it_and_spends_what_it_saw_meanwhile() {
        let mut slot = TriggeredAbsorbSlot::new("S", AbsorbSource::Aura, sacred_shield(), 0.0);
        assert!(slot.observe(&bearer(95.0, 5.0), 0.0));
        assert!(!slot.spent());
        // Damage while held: seen, not fired.
        assert!(!slot.observe(&bearer(91.0, 9.0), 3.0));
        // Ready again, but nothing new since: no shield.
        assert!(!slot.observe(&bearer(91.0, 9.0), 3.0));
        assert_eq!(slot.ready_in, 0.0);
        assert!(slot.observe(&bearer(90.5, 9.5), 0.1));
    }

    #[test]
    fn the_shield_takes_its_source() {
        let item = aegis().absorb.shield_aura(
            "Aegis",
            ShieldFrom::Item(ItemId::AegisOfTheLastStand),
            80.0,
        );
        assert_eq!(item.caster, None);
        assert_eq!(item.dispel_type, DispelType::Physical);
        assert_eq!(item.source_item, Some(ItemId::AegisOfTheLastStand));
        let arming = Aura {
            effect_type: AuraType::TriggeredAbsorb,
            caster: Some(Entity::from_raw(7)),
            spell_school: Some(super::super::abilities::SpellSchool::Holy),
            ability_name: "Sacred Shield".into(),
            // Not the default, so a shield that drops it is seen to.
            dispel_type: DispelType::Untyped,
            ..Default::default()
        };
        let aura = sacred_shield().shield_aura("Sacred Shield", ShieldFrom::Aura(&arming), 40.0);
        assert_eq!(aura.caster, arming.caster);
        assert_eq!(aura.spell_school, arming.spell_school);
        assert_eq!(aura.dispel_type, DispelType::Untyped);
        assert_eq!(aura.source_item, None);
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
    fn validation_refuses_what_cannot_fire_be_priced_or_shield_anything() {
        assert!(aegis().validate("ok").is_ok());
        assert!(validate_triggered_absorb("ok", &sacred_shield()).is_ok());
        let mut c = aegis();
        c.absorb.trigger = AbsorbTrigger::HealthBelow(1.0);
        assert!(c.validate("x").is_err());
        let mut c = aegis();
        c.absorb.shield = AbsorbSize::MaxHealthFraction(0.0);
        assert!(c.validate("x").is_err());
        let mut c = aegis();
        c.absorb.duration = 0.0;
        assert!(c.validate("x").is_err());
        let mut c = aegis();
        c.trigger_rate = 0.0;
        assert!(c.validate("x").is_err());
        let mut c = sacred_shield();
        c.shield = AbsorbSize::Flat(0.0);
        assert!(validate_triggered_absorb("x", &c).is_err());
        let mut c = sacred_shield();
        c.limit = TriggerLimit::Cooldown(0.0);
        assert!(validate_triggered_absorb("x", &c).is_err());
    }

    #[test]
    fn the_sentence_states_the_numbers() {
        assert_eq!(
            triggered_absorb_description(&aegis().absorb),
            "When your health drops below 35%, gain a shield absorbing 20% of your maximum \
             health for 30s. Once a match."
        );
        assert_eq!(
            triggered_absorb_description(&sacred_shield()),
            "Each time you take damage, gain a shield absorbing 40 damage for 6s. At most \
             once every 6s."
        );
    }
}
