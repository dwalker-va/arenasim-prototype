//! Talent procs: a TRIGGER, a CHANCE and an EFFECT, carried by a combatant
//! whose build takes a `Proc` node (`talents.rs`, AS-331).
//!
//! The proc-trinket shape (`proc_trinkets.rs`), widened for the talent trees:
//! more triggers ([`TalentTrigger`]), qualifiers on them (the abilities that
//! fire it, crits only), a chance that may be normalised for weapon speed
//! ([`ProcChance::PerMinute`]), and effects aimed at the unit on the other end
//! of the event as well as at the owner ([`ProcEffect`]). A talent's proc is
//! priced by its node's declared budget, never by the trinket formula.
//!
//! ## How a proc fires
//!
//! Each site where a trigger happens pushes a [`ProcEvent`] for the unit it
//! happened to ([`push`]), and only when that unit carries a talent proc at
//! all — so a match without talents pushes nothing, draws nothing and is
//! byte-identical to the match before this module existed. One system,
//! [`resolve_talent_procs`], drains the queue at the end of the frame: it
//! rolls each of the unit's ready procs the event qualifies for, in slot
//! order, and queues the effect as an [`AuraPending`]. One roll site, so the
//! draw order is the event order and nothing else.
//!
//! The sites:
//! - [`TalentTrigger::MeleeHit`] — a landed melee auto-attack
//!   (`combat_auto_attack`, beside the trinkets' roll);
//! - [`TalentTrigger::SpellHit`] — an ability's direct damage landing: a cast
//!   completing (`process_casting`), a projectile arriving
//!   (`process_projectile_hits`), a queued instant attack (`decide_abilities`);
//! - [`TalentTrigger::DotTick`] — a damage tick of the unit's DoT
//!   (`process_dot_ticks`) or of its channel (`process_channeling`);
//! - [`TalentTrigger::HealCrit`] — a cast heal that crit (`process_casting`);
//! - [`TalentTrigger::CritTaken`] — a melee or ranged crit landing ON the unit:
//!   an auto-attack or a queued instant attack;
//! - [`TalentTrigger::StealthBreak`] — the unit left stealth, by any means,
//!   noticed by the resolver itself from the unit's `stealthed` flag;
//! - [`TalentTrigger::AbilityUsed`] — the unit's decision used an ability
//!   (`decide_abilities`, after its turn).
//!
//! An instant decided outside `decide_abilities` (an interrupt) is not an
//! `AbilityUsed`, and an instant's damage landed by its own class AI is not a
//! `SpellHit` until its site pushes one; a tree card granting such an ability
//! adds the push at its site.
//!
//! ## Ranks
//!
//! Ranks are additive (user ruling, 2026-10-31). A proc declares what a rank
//! adds to ([`ProcRankScale`]): its chance (Nightfall: 5% a rank) or its
//! effect (Initiative: 5 energy a rank). Never both.
//!
//! ## Stacking
//!
//! A proc's aura is refresh-only (`max_stacks: 1`): a proc that fires again
//! while its aura is up renews it, never doubles it. Keyed by its own name, so
//! it coexists with a same-type aura from anywhere else.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::abilities::AbilityType;
use super::components::{
    Aura, AuraPending, AuraStacks, AuraType, Combatant, DispelType, GameRng, StackScope,
};
use super::next_cast::NextCastModifier;
use super::talents::TalentId;
use super::utils::combatant_id;
use super::PlayMatchEntity;
use crate::combat::log::{CombatLog, CombatLogEventType};

// ============================================================================
// CONFIG
// ============================================================================

/// What combat event gives a talent proc its chance. See the module docs for
/// where each fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TalentTrigger {
    /// A melee auto-attack that landed. Carries the swing's interval, so a
    /// [`ProcChance::PerMinute`] rate is normalised for weapon speed.
    MeleeHit,
    /// An ability's direct damage landed on an enemy.
    SpellHit,
    /// A periodic damage tick of the unit's DoT or channel.
    DotTick,
    /// A heal the unit cast landed as a critical heal.
    HealCrit,
    /// A melee or ranged critical strike landed on the unit.
    CritTaken,
    /// The unit left stealth.
    StealthBreak,
    /// The unit used an ability.
    AbilityUsed,
}

impl TalentTrigger {
    /// Whether the event names the ability behind it, so `abilities` can
    /// narrow it.
    pub fn carries_ability(self) -> bool {
        match self {
            TalentTrigger::SpellHit
            | TalentTrigger::DotTick
            | TalentTrigger::HealCrit
            | TalentTrigger::AbilityUsed => true,
            TalentTrigger::MeleeHit | TalentTrigger::CritTaken | TalentTrigger::StealthBreak => {
                false
            }
        }
    }

    /// Whether the event has a unit on its other end (the struck enemy, the
    /// healed ally, the attacker), for a [`ProcTarget::Other`] effect.
    pub fn has_other(self) -> bool {
        match self {
            TalentTrigger::MeleeHit
            | TalentTrigger::SpellHit
            | TalentTrigger::DotTick
            | TalentTrigger::HealCrit
            | TalentTrigger::CritTaken => true,
            TalentTrigger::StealthBreak | TalentTrigger::AbilityUsed => false,
        }
    }

    /// Whether the event carries an amount (damage dealt or taken, healing),
    /// for a [`ProcEffect::HealBack`].
    pub fn has_amount(self) -> bool {
        self.has_other()
    }

    /// Whether the event can be a crit, so `crit_only` means something.
    pub fn can_crit(self) -> bool {
        match self {
            TalentTrigger::MeleeHit | TalentTrigger::SpellHit => true,
            // A heal crit and a crit taken are crits already; a tick, a
            // stealth break and a use never are.
            TalentTrigger::HealCrit
            | TalentTrigger::CritTaken
            | TalentTrigger::DotTick
            | TalentTrigger::StealthBreak
            | TalentTrigger::AbilityUsed => false,
        }
    }
}

/// How likely a proc is to fire on a qualifying event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ProcChance {
    /// This chance on every qualifying event, in `(0, 1]`.
    Chance(f32),
    /// This many procs per minute of melee, whatever the weapon: a hit's
    /// chance is `rate × swing_time / 60`, so a slow weapon's fewer swings
    /// each carry more of it (The Art of War: 4.8 a minute, 20% at a 2.5s
    /// swing). [`TalentTrigger::MeleeHit`] only.
    PerMinute(f32),
}

impl ProcChance {
    /// The chance one event carries: a swing of `swing_time` seconds for a
    /// per-minute rate.
    pub fn per_event(self, swing_time: Option<f32>) -> f32 {
        match self {
            ProcChance::Chance(c) => c,
            ProcChance::PerMinute(rate) => rate * swing_time.unwrap_or(0.0) / 60.0,
        }
    }

    fn times(self, ranks: f32) -> Self {
        match self {
            ProcChance::Chance(c) => ProcChance::Chance(c * ranks),
            ProcChance::PerMinute(r) => ProcChance::PerMinute(r * ranks),
        }
    }
}

/// What a talent's ranks add to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcRankScale {
    /// Each rank adds its chance (Nightfall, Improved Shadow Bolt, Blackout).
    Chance,
    /// Each rank adds its effect's numbers (Initiative, Inspiration,
    /// Vengeance).
    Effect,
}

/// Who a proc's aura lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcTarget {
    /// The proc's owner.
    Owner,
    /// The unit on the event's other end: the enemy struck, the ally healed,
    /// the attacker that crit.
    Other,
}

/// What a talent proc does when it fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProcEffect {
    /// An aura on its owner or on the event's other unit: Blackout's stun,
    /// Aftermath's slow, Vengeance's damage buff.
    Aura {
        on: ProcTarget,
        aura_type: AuraType,
        magnitude: f32,
        duration: f32,
        #[serde(default)]
        tick_interval: f32,
        /// Its removal class: declared, as a proc has no school to derive one
        /// from.
        dispel_type: DispelType,
        /// The aura's name; the talent's name when omitted.
        #[serde(default)]
        name: Option<String>,
    },
    /// A next-cast modifier on its owner (Shadow Trance, Infusion of Light).
    NextCast {
        modifier: NextCastModifier,
        duration: f32,
        dispel_type: DispelType,
        #[serde(default)]
        name: Option<String>,
    },
    /// A heal over time on its owner of `fraction` of the event's amount,
    /// spread over `duration` in ticks of `tick_interval` (Blessed Recovery:
    /// 25% of a crit taken over 6s). Lands through `apply_healing`, as every
    /// heal over time does.
    HealBack {
        fraction: f32,
        duration: f32,
        tick_interval: f32,
        #[serde(default)]
        name: Option<String>,
    },
}

impl ProcEffect {
    fn times(&self, ranks: f32) -> Self {
        match self {
            ProcEffect::Aura {
                on,
                aura_type,
                magnitude,
                duration,
                tick_interval,
                dispel_type,
                name,
            } => ProcEffect::Aura {
                on: *on,
                aura_type: *aura_type,
                magnitude: magnitude * ranks,
                duration: *duration,
                tick_interval: *tick_interval,
                dispel_type: *dispel_type,
                name: name.clone(),
            },
            ProcEffect::NextCast {
                modifier,
                duration,
                dispel_type,
                name,
            } => ProcEffect::NextCast {
                modifier: modifier.times(ranks),
                duration: *duration,
                dispel_type: *dispel_type,
                name: name.clone(),
            },
            ProcEffect::HealBack {
                fraction,
                duration,
                tick_interval,
                name,
            } => ProcEffect::HealBack {
                fraction: fraction * ranks,
                duration: *duration,
                tick_interval: *tick_interval,
                name: name.clone(),
            },
        }
    }

    fn has_magnitude(&self) -> bool {
        match self {
            ProcEffect::Aura { .. } | ProcEffect::HealBack { .. } => true,
            ProcEffect::NextCast { modifier, .. } => modifier.has_magnitude(),
        }
    }

    fn name(&self) -> Option<&str> {
        match self {
            ProcEffect::Aura { name, .. }
            | ProcEffect::NextCast { name, .. }
            | ProcEffect::HealBack { name, .. } => name.as_deref(),
        }
    }
}

/// One talent proc, as a `Proc` effect in `talents.rs` writes it: one rank's
/// worth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TalentProc {
    pub trigger: TalentTrigger,
    /// The abilities whose events qualify (Nightfall: Corruption and Drain
    /// Life). Empty for every one. Only for a trigger that names its ability.
    #[serde(default)]
    pub abilities: Vec<AbilityType>,
    /// Only a critical event qualifies (Vengeance: a melee crit).
    #[serde(default)]
    pub crit_only: bool,
    pub chance: ProcChance,
    pub ranks_add: ProcRankScale,
    pub effect: ProcEffect,
    /// Seconds the proc cannot fire again after it does. `0.0` (the default)
    /// for none.
    #[serde(default)]
    pub internal_cooldown: f32,
}

impl TalentProc {
    /// This proc at `rank` ranks: its chance or its effect (as it declares)
    /// times the rank.
    pub fn at_rank(&self, rank: u8) -> Self {
        let ranks = f32::from(rank);
        match self.ranks_add {
            ProcRankScale::Chance => Self {
                chance: self.chance.times(ranks),
                ..self.clone()
            },
            ProcRankScale::Effect => Self {
                effect: self.effect.times(ranks),
                ..self.clone()
            },
        }
    }

    /// Whether `event` qualifies for this proc (its trigger, its ability and
    /// its crit), cooldown aside.
    pub fn qualifies(&self, event: &ProcEvent) -> bool {
        self.trigger == event.trigger
            && (self.abilities.is_empty()
                || event.ability.is_some_and(|a| self.abilities.contains(&a)))
            && (!self.crit_only || event.crit)
    }

    /// Reject a proc that cannot fire, fires for certain past one, or aims an
    /// effect at something its trigger does not have. `max_rank` is its
    /// node's; `reachable` every ability its class can have.
    pub fn validate(&self, max_rank: u8, reachable: &BTreeSet<AbilityType>) -> Result<(), String> {
        let ranks = f32::from(max_rank);
        match self.chance {
            ProcChance::Chance(c) => {
                if !(c > 0.0 && c <= 1.0) {
                    return Err(format!("a proc chance of {c} is outside (0, 1]"));
                }
                if self.ranks_add == ProcRankScale::Chance && c * ranks > 1.0 + 1e-6 {
                    return Err(format!(
                        "a proc chance of {c} a rank passes 1 at {max_rank} ranks"
                    ));
                }
            }
            ProcChance::PerMinute(rate) => {
                if self.trigger != TalentTrigger::MeleeHit {
                    return Err("a per-minute proc rate is for a MeleeHit proc only".to_string());
                }
                if !(rate.is_finite() && rate > 0.0) {
                    return Err(format!("a proc rate of {rate} a minute is not positive"));
                }
            }
        }
        if !self.abilities.is_empty() && !self.trigger.carries_ability() {
            return Err(format!(
                "a {:?} proc names abilities, but its event names none",
                self.trigger
            ));
        }
        if let Some(a) = self.abilities.iter().find(|a| !reachable.contains(a)) {
            return Err(format!("a proc names {a:?}, which its class cannot have"));
        }
        if self.crit_only && !self.trigger.can_crit() {
            return Err(format!("a {:?} proc cannot be crit-only", self.trigger));
        }
        if !(self.internal_cooldown.is_finite() && self.internal_cooldown >= 0.0) {
            return Err(format!(
                "a proc internal cooldown of {} is negative",
                self.internal_cooldown
            ));
        }
        if self.ranks_add == ProcRankScale::Effect && !self.effect.has_magnitude() {
            return Err("a proc's ranks add to its effect, which has no number".to_string());
        }
        match &self.effect {
            ProcEffect::Aura {
                on,
                aura_type,
                magnitude,
                duration,
                tick_interval,
                ..
            } => {
                if *aura_type == AuraType::NextCast {
                    return Err("a proc primes a next cast through its NextCast effect".to_string());
                }
                if *on == ProcTarget::Other && !self.trigger.has_other() {
                    return Err(format!(
                        "a {:?} proc has no other unit to land its aura on",
                        self.trigger
                    ));
                }
                if !magnitude.is_finite() || !(*duration > 0.0) || *tick_interval < 0.0 {
                    return Err("a proc aura needs a finite magnitude and a duration".to_string());
                }
            }
            ProcEffect::NextCast {
                modifier, duration, ..
            } => {
                modifier.validate()?;
                if !(*duration > 0.0) {
                    return Err("a proc's next-cast modifier needs a duration".to_string());
                }
                if let super::next_cast::NextCastScope::Abilities(list) = &modifier.applies_to {
                    if let Some(a) = list.iter().find(|a| !reachable.contains(a)) {
                        return Err(format!("a proc primes {a:?}, which its class cannot have"));
                    }
                }
            }
            ProcEffect::HealBack {
                fraction,
                duration,
                tick_interval,
                ..
            } => {
                if !self.trigger.has_amount() {
                    return Err(format!("a {:?} event has no amount to heal back", self.trigger));
                }
                if !(*fraction > 0.0) || !(*duration > 0.0) || !(*tick_interval > 0.0) {
                    return Err("a heal-back needs a fraction, a duration and a tick".to_string());
                }
            }
        }
        Ok(())
    }
}

// ============================================================================
// PER-COMBATANT STATE
// ============================================================================

/// One proc a combatant's build gave it, and its live internal cooldown.
#[derive(Debug, Clone, PartialEq)]
pub struct TalentProcSlot {
    pub talent: TalentId,
    /// The talent's name: the log line, and the aura's name unless its effect
    /// names one.
    pub name: String,
    pub proc: TalentProc,
    /// Seconds until it may fire again. `0.0` is ready.
    pub remaining_icd: f32,
}

impl TalentProcSlot {
    pub fn new(talent: TalentId, name: &str, proc: TalentProc) -> Self {
        Self {
            talent,
            name: name.to_string(),
            proc,
            remaining_icd: 0.0,
        }
    }
}

/// A combatant's talent procs ([`super::components::Combatant::talent_procs`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TalentProcs {
    pub slots: Vec<TalentProcSlot>,
    /// Whether the combatant was stealthed when the resolver last looked: a
    /// `true` turned `false` is a [`TalentTrigger::StealthBreak`].
    pub was_stealthed: bool,
}

impl TalentProcs {
    /// Whether the combatant carries no talent proc: the common case, in
    /// which no site pushes an event for it.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Whether it carries a proc on `trigger`.
    pub fn listens_for(&self, trigger: TalentTrigger) -> bool {
        self.slots.iter().any(|s| s.proc.trigger == trigger)
    }

    /// Tick every internal cooldown down by `dt`.
    pub fn tick(&mut self, dt: f32) {
        for slot in &mut self.slots {
            if slot.remaining_icd > 0.0 {
                slot.remaining_icd = (slot.remaining_icd - dt).max(0.0);
            }
        }
    }
}

// ============================================================================
// EVENTS
// ============================================================================

/// One trigger that happened to `unit`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcEvent {
    pub unit: Entity,
    pub trigger: TalentTrigger,
    /// The ability behind it, for a trigger that names one.
    pub ability: Option<AbilityType>,
    pub crit: bool,
    /// The damage dealt or taken, or the healing done.
    pub amount: f32,
    /// The unit on its other end.
    pub other: Option<Entity>,
    /// The melee swing's interval, for a per-minute rate.
    pub swing_time: Option<f32>,
}

impl ProcEvent {
    /// An event with only its unit and trigger.
    pub fn new(unit: Entity, trigger: TalentTrigger) -> Self {
        Self {
            unit,
            trigger,
            ability: None,
            crit: false,
            amount: 0.0,
            other: None,
            swing_time: None,
        }
    }

    pub fn ability(mut self, ability: AbilityType) -> Self {
        self.ability = Some(ability);
        self
    }

    pub fn crit(mut self, crit: bool) -> Self {
        self.crit = crit;
        self
    }

    pub fn amount(mut self, amount: f32) -> Self {
        self.amount = amount;
        self
    }

    pub fn other(mut self, other: Entity) -> Self {
        self.other = Some(other);
        self
    }

    pub fn swing(mut self, swing_time: f32) -> Self {
        self.swing_time = Some(swing_time);
        self
    }
}

/// The frame's proc events, drained by [`resolve_talent_procs`].
#[derive(Resource, Debug, Default)]
pub struct TalentProcEvents(pub Vec<ProcEvent>);

/// Queue `event` for the resolver, through `commands` so a site needs no new
/// system parameter. Call it only for a unit whose `talent_procs` listens for
/// the trigger: that guard is what makes a match without talents push nothing.
pub fn push(commands: &mut Commands, event: ProcEvent) {
    commands.queue(move |world: &mut World| {
        world
            .get_resource_or_insert_with(TalentProcEvents::default)
            .0
            .push(event);
    });
}

/// [`push`] when `unit`'s procs listen for the event's trigger.
pub fn push_for(commands: &mut Commands, procs: &TalentProcs, event: ProcEvent) {
    if procs.listens_for(event.trigger) {
        push(commands, event);
    }
}

// ============================================================================
// RESOLUTION
// ============================================================================

/// The aura `slot`'s effect lands, for an event whose owner is `owner`, or
/// `None` when it has nowhere to land.
pub fn effect_aura(slot: &TalentProcSlot, owner: Entity, event: &ProcEvent) -> Option<AuraPending> {
    let name = slot
        .proc
        .effect
        .name()
        .map_or_else(|| slot.name.clone(), str::to_string);
    let base = |effect_type: AuraType, magnitude: f32, duration: f32, tick: f32, dispel| Aura {
        effect_type,
        duration,
        magnitude,
        break_on_damage_threshold: -1.0,
        tick_interval: tick,
        time_until_next_tick: tick,
        caster: Some(owner),
        ability_name: name.clone(),
        dispel_type: dispel,
        source_talent: Some(slot.talent),
        // Refresh-only: a proc firing again while its aura is up renews it.
        stacks: Some(AuraStacks::new(1, StackScope::for_aura(effect_type))),
        ..Default::default()
    };
    match &slot.proc.effect {
        ProcEffect::Aura {
            on,
            aura_type,
            magnitude,
            duration,
            tick_interval,
            dispel_type,
            ..
        } => {
            let target = match on {
                ProcTarget::Owner => owner,
                ProcTarget::Other => event.other?,
            };
            Some(AuraPending {
                target,
                aura: base(*aura_type, *magnitude, *duration, *tick_interval, *dispel_type),
            })
        }
        ProcEffect::NextCast {
            modifier,
            duration,
            dispel_type,
            ..
        } => Some(AuraPending {
            target: owner,
            aura: Aura {
                next_cast: Some(modifier.clone()),
                ..base(AuraType::NextCast, 1.0, *duration, 0.0, *dispel_type)
            },
        }),
        ProcEffect::HealBack {
            fraction,
            duration,
            tick_interval,
            ..
        } => {
            let ticks = (duration / tick_interval).round().max(1.0);
            let per_tick = fraction * event.amount / ticks;
            (per_tick > 0.0).then(|| AuraPending {
                target: owner,
                aura: base(
                    AuraType::HealingOverTime,
                    per_tick,
                    *duration,
                    *tick_interval,
                    DispelType::Auto,
                ),
            })
        }
    }
}

/// Roll `procs`' ready slots that `event` qualifies for, in slot order, and
/// return the auras that fired with the names of the talents that fired them.
/// A certain chance (1.0 or more) fires without a draw; nothing is drawn for
/// a slot the event does not qualify for, or one cooling down.
pub fn roll(
    procs: &mut TalentProcs,
    owner: Entity,
    event: &ProcEvent,
    game_rng: &mut GameRng,
) -> Vec<(String, AuraPending)> {
    let mut fired = Vec::new();
    for slot in procs.slots.iter_mut() {
        if slot.remaining_icd > 0.0 || !slot.proc.qualifies(event) {
            continue;
        }
        let chance = slot.proc.chance.per_event(event.swing_time);
        let hit = chance >= 1.0 || game_rng.random_f32() < chance;
        if !hit {
            continue;
        }
        slot.remaining_icd = slot.proc.internal_cooldown;
        if let Some(pending) = effect_aura(slot, owner, event) {
            fired.push((slot.name.clone(), pending));
        }
    }
    fired
}

/// The talent effects of one unit's turn, run by `decide_abilities` right
/// after it with the abilities the turn USED
/// ([`super::decision_trace::DecisionTrace::take_casts_of`]):
/// - each next-cast modifier a used ability spends comes off `auras`
///   ([`super::next_cast::consume`]), and the last one spent rides on the
///   combatant ([`Combatant::next_cast_spent`]) for a cast that resolves
///   later. A turn that used something but spent nothing clears it. Pass
///   `auras` only when a `NextCast` aura is on the unit;
/// - each use is an [`TalentTrigger::AbilityUsed`] event, for a unit that
///   listens for one.
pub fn after_turn(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    entity: Entity,
    combatant: &mut Combatant,
    mut auras: Option<&mut super::components::ActiveAuras>,
    used: &[AbilityType],
) {
    if used.is_empty() {
        return;
    }
    if let Some(auras) = auras.as_deref_mut() {
        let mut spent = None;
        for ability in used {
            if let Some((modifier, names)) = super::next_cast::consume(auras, *ability) {
                let cast = combatant
                    .book()
                    .get(ability)
                    .map_or_else(|| format!("{ability:?}"), |d| d.name.clone());
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "{}'s {} is spent on {}",
                        combatant_id(combatant.team, combatant.slot, combatant.class),
                        names.join(" and "),
                        cast
                    ),
                );
                spent = Some(super::next_cast::SpentNextCast {
                    ability: *ability,
                    modifier,
                });
            }
        }
        combatant.next_cast_spent = spent;
    } else if combatant.next_cast_spent.is_some() {
        combatant.next_cast_spent = None;
    }
    if combatant.talent_procs.listens_for(TalentTrigger::AbilityUsed) {
        for ability in used {
            push(
                commands,
                ProcEvent::new(entity, TalentTrigger::AbilityUsed).ability(*ability),
            );
        }
    }
}

/// Resolve the frame's proc events (see the module docs). Registered at the
/// end of `CombatResolution` in both modes.
pub fn resolve_talent_procs(
    mut commands: Commands,
    mut events: ResMut<TalentProcEvents>,
    mut game_rng: ResMut<GameRng>,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(Entity, &mut Combatant)>,
) {
    let mut queue: Vec<ProcEvent> = std::mem::take(&mut events.0);
    // Stealth left this frame, by any means: noticed here, once, rather than
    // at every site that clears the flag.
    for (entity, mut combatant) in combatants.iter_mut() {
        if combatant.talent_procs.is_empty() {
            continue;
        }
        let stealthed = combatant.stealthed;
        if combatant.talent_procs.was_stealthed && !stealthed {
            queue.push(ProcEvent::new(entity, TalentTrigger::StealthBreak));
        }
        if combatant.talent_procs.was_stealthed != stealthed {
            combatant.talent_procs.was_stealthed = stealthed;
        }
    }
    for event in queue {
        let Ok((_, mut combatant)) = combatants.get_mut(event.unit) else {
            continue;
        };
        if !combatant.is_alive() || combatant.talent_procs.is_empty() {
            continue;
        }
        let id = combatant_id(combatant.team, combatant.slot, combatant.class);
        let fired = roll(
            &mut combatant.talent_procs,
            event.unit,
            &event,
            &mut game_rng,
        );
        for (talent, pending) in fired {
            combat_log.log(
                CombatLogEventType::Buff,
                format!("[PROC] {}'s {} procs {}", id, talent, pending.aura.ability_name),
            );
            commands.spawn((pending, PlayMatchEntity));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::next_cast::NextCastScope;

    fn trance() -> TalentProc {
        TalentProc {
            trigger: TalentTrigger::DotTick,
            abilities: vec![AbilityType::Corruption, AbilityType::DrainLife],
            crit_only: false,
            chance: ProcChance::Chance(0.05),
            ranks_add: ProcRankScale::Chance,
            effect: ProcEffect::NextCast {
                modifier: NextCastModifier {
                    applies_to: NextCastScope::Abilities(vec![AbilityType::Shadowbolt]),
                    instant: true,
                    free: false,
                    crit_chance: 0.0,
                    cast_time_reduction: 0.0,
                    cost_reduction: 0.0,
                    power: 0.0,
                },
                duration: 10.0,
                dispel_type: DispelType::Auto,
                name: Some("Shadow Trance".to_string()),
            },
            internal_cooldown: 0.0,
        }
    }

    fn slot(proc: TalentProc) -> TalentProcSlot {
        TalentProcSlot::new(TalentId::DemonicEmbrace, "Nightfall", proc)
    }

    fn unit() -> Entity {
        Entity::from_raw(7)
    }

    fn reachable() -> BTreeSet<AbilityType> {
        [
            AbilityType::Corruption,
            AbilityType::DrainLife,
            AbilityType::Shadowbolt,
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn a_chance_rank_adds_its_chance_and_an_effect_rank_its_effect() {
        let two = trance().at_rank(2);
        assert_eq!(two.chance, ProcChance::Chance(0.1));
        assert_eq!(two.effect, trance().effect);

        let mut initiative = trance();
        initiative.ranks_add = ProcRankScale::Effect;
        initiative.chance = ProcChance::Chance(1.0);
        initiative.effect = ProcEffect::NextCast {
            modifier: NextCastModifier {
                applies_to: NextCastScope::Any,
                instant: false,
                free: false,
                crit_chance: 0.0,
                cast_time_reduction: 0.0,
                cost_reduction: 5.0,
                power: 0.0,
            },
            duration: 30.0,
            dispel_type: DispelType::Untyped,
            name: None,
        };
        let two = initiative.at_rank(2);
        assert_eq!(two.chance, ProcChance::Chance(1.0));
        let ProcEffect::NextCast { modifier, .. } = two.effect else {
            unreachable!()
        };
        assert_eq!(modifier.cost_reduction, 10.0);
    }

    /// The Art of War: 4.8 procs a minute is 20% on a 2.5s swing, 32% on a
    /// 4.0s one and 12% on a 1.5s one.
    #[test]
    fn a_per_minute_rate_is_normalised_for_weapon_speed() {
        let ppm = ProcChance::PerMinute(4.8);
        assert!((ppm.per_event(Some(2.5)) - 0.20).abs() < 1e-6);
        assert!((ppm.per_event(Some(4.0)) - 0.32).abs() < 1e-6);
        assert!((ppm.per_event(Some(1.5)) - 0.12).abs() < 1e-6);
        // ...and a per-hit chance is not.
        assert_eq!(ProcChance::Chance(0.2).per_event(Some(4.0)), 0.2);
    }

    #[test]
    fn an_event_qualifies_on_its_trigger_its_ability_and_its_crit() {
        let p = trance();
        let tick = ProcEvent::new(unit(), TalentTrigger::DotTick).ability(AbilityType::Corruption);
        assert!(p.qualifies(&tick));
        assert!(!p.qualifies(
            &ProcEvent::new(unit(), TalentTrigger::DotTick).ability(AbilityType::Immolate)
        ));
        assert!(!p.qualifies(
            &ProcEvent::new(unit(), TalentTrigger::SpellHit).ability(AbilityType::Corruption)
        ));
        let mut vengeance = trance();
        vengeance.trigger = TalentTrigger::MeleeHit;
        vengeance.abilities.clear();
        vengeance.crit_only = true;
        assert!(!vengeance.qualifies(&ProcEvent::new(unit(), TalentTrigger::MeleeHit)));
        assert!(vengeance.qualifies(&ProcEvent::new(unit(), TalentTrigger::MeleeHit).crit(true)));
    }

    #[test]
    fn a_certain_proc_fires_without_a_draw_and_goes_on_its_cooldown() {
        let mut p = trance();
        p.chance = ProcChance::Chance(1.0);
        p.internal_cooldown = 6.0;
        let mut procs = TalentProcs {
            slots: vec![slot(p)],
            was_stealthed: false,
        };
        let expected = GameRng::from_seed(3).random_f32();
        let mut rng = GameRng::from_seed(3);
        let tick = ProcEvent::new(unit(), TalentTrigger::DotTick).ability(AbilityType::Corruption);
        let fired = roll(&mut procs, unit(), &tick, &mut rng);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].1.aura.effect_type, AuraType::NextCast);
        assert_eq!(fired[0].1.aura.ability_name, "Shadow Trance");
        assert_eq!(fired[0].1.aura.source_talent, Some(TalentId::DemonicEmbrace));
        assert_eq!(rng.random_f32(), expected, "a certain proc drew");
        assert_eq!(procs.slots[0].remaining_icd, 6.0);
        assert!(roll(&mut procs, unit(), &tick, &mut rng).is_empty());
        procs.tick(6.0);
        assert_eq!(roll(&mut procs, unit(), &tick, &mut rng).len(), 1);
    }

    #[test]
    fn an_unqualified_event_draws_nothing() {
        let mut procs = TalentProcs {
            slots: vec![slot(trance())],
            was_stealthed: false,
        };
        let expected = GameRng::from_seed(5).random_f32();
        let mut rng = GameRng::from_seed(5);
        let hit = ProcEvent::new(unit(), TalentTrigger::SpellHit).ability(AbilityType::Corruption);
        assert!(roll(&mut procs, unit(), &hit, &mut rng).is_empty());
        assert_eq!(rng.random_f32(), expected);
    }

    #[test]
    fn an_aura_lands_on_the_owner_or_the_other_unit() {
        let mut blackout = trance();
        blackout.trigger = TalentTrigger::SpellHit;
        blackout.effect = ProcEffect::Aura {
            on: ProcTarget::Other,
            aura_type: AuraType::Stun,
            magnitude: 1.0,
            duration: 3.0,
            tick_interval: 0.0,
            dispel_type: DispelType::Auto,
            name: Some("Blackout".to_string()),
        };
        let victim = Entity::from_raw(9);
        let event = ProcEvent::new(unit(), TalentTrigger::SpellHit).other(victim);
        let landed = effect_aura(&slot(blackout.clone()), unit(), &event).unwrap();
        assert_eq!(landed.target, victim);
        assert_eq!(landed.aura.caster, Some(unit()));
        // No other unit: nothing lands.
        assert!(
            effect_aura(&slot(blackout), unit(), &ProcEvent::new(unit(), TalentTrigger::SpellHit))
                .is_none()
        );
    }

    /// Blessed Recovery: 25% of a 40-damage crit over 6s in 2s ticks is
    /// three ticks of 10/3.
    #[test]
    fn a_heal_back_spreads_its_share_of_the_event_over_its_ticks() {
        let mut br = trance();
        br.trigger = TalentTrigger::CritTaken;
        br.abilities.clear();
        br.effect = ProcEffect::HealBack {
            fraction: 0.25,
            duration: 6.0,
            tick_interval: 2.0,
            name: None,
        };
        let event = ProcEvent::new(unit(), TalentTrigger::CritTaken).amount(40.0);
        let landed = effect_aura(&slot(br), unit(), &event).unwrap();
        assert_eq!(landed.target, unit());
        assert_eq!(landed.aura.effect_type, AuraType::HealingOverTime);
        assert!((landed.aura.magnitude - 10.0 / 3.0).abs() < 1e-5);
        assert_eq!(landed.aura.tick_interval, 2.0);
        assert_eq!(landed.aura.ability_name, "Nightfall");
    }

    #[test]
    fn validate_holds_a_proc_to_its_trigger() {
        let r = reachable();
        assert!(trance().validate(2, &r).is_ok());
        let mut over = trance();
        over.chance = ProcChance::Chance(0.6);
        assert!(over.validate(2, &r).is_err(), "1.2 at two ranks");
        let mut ppm = trance();
        ppm.chance = ProcChance::PerMinute(4.8);
        assert!(ppm.validate(1, &r).is_err(), "per minute on a DotTick");
        ppm.trigger = TalentTrigger::MeleeHit;
        ppm.abilities.clear();
        assert!(ppm.validate(1, &r).is_ok());
        let mut named = trance();
        named.trigger = TalentTrigger::StealthBreak;
        assert!(named.validate(1, &r).is_err(), "names abilities");
        let mut foreign = trance();
        foreign.abilities = vec![AbilityType::Frostbolt];
        assert!(foreign.validate(1, &r).is_err(), "another class's ability");
        let mut crit = trance();
        crit.crit_only = true;
        assert!(crit.validate(1, &r).is_err(), "a tick never crits");
        let mut other = trance();
        other.trigger = TalentTrigger::AbilityUsed;
        other.abilities.clear();
        other.effect = ProcEffect::Aura {
            on: ProcTarget::Other,
            aura_type: AuraType::Stun,
            magnitude: 1.0,
            duration: 3.0,
            tick_interval: 0.0,
            dispel_type: DispelType::Auto,
            name: None,
        };
        assert!(other.validate(1, &r).is_err(), "a use has no other unit");
    }
}
