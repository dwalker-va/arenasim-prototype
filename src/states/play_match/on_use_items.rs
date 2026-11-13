//! On-use items — an EFFECT its wearer chooses when to fire, on the item's own
//! cooldown.
//!
//! The Medallion shape: wearing the item does nothing on its own, but its
//! wearer's AI may USE it. Unlike a proc ([`super::proc_trinkets`]), nothing
//! rolls: the AI decides, so each effect KIND needs one AI rule (never one per
//! item), and the item-side declaration says only what the use does and how
//! often it may happen.
//!
//! ## What an on-use costs
//!
//! An on-use starts READY, and arena lives are short, so its price is its
//! value over the USES IT GETS IN AN ARENA LIFE ([`ARENA_LIFE_SECS`], 37.8s —
//! measured by `realised_uptime_against_the_long_run_bound` in
//! `tests/proc_trinket_probes.rs`), not the long-run uptime bound a proc is
//! charged at. In a 37.8s life a 45s cooldown is ONE use: 15s of +40 spell
//! power is `1.5 x 40 x 15/37.8`, about 24 points, where the long-run bound
//! (`15/60`) would charge 15 and underprice it ([`on_use_buff_cost`], the
//! talents proposal §6.5 item 3, which wrote it against the 43s life measured
//! before the 0.9 changes).
//!
//! A STAT BUFF on use ([`OnUseEffect::Buff`]: the Talisman, Earthstrike,
//! Kiss of the Spider's haste) is priced that way, at the same weight the item
//! budget charges for its stat ([`proc_effect_budget_weight`], the one table a
//! proc's buff is priced by too). A HEAL OVER TIME riding the buff (the Diamond
//! Flask's, [`OnUseHot`]) is priced at the health it heals inside the life, at
//! the weight of a point of health ([`on_use_heal_cost`]) — the unit Aegis of
//! the Last Stand's absorb is priced in.
//!
//! An effect no stat weight can price — the crowd-control break — carries a
//! DECLARED price per use (`price:` in `items.ron`), sweep-checked like a
//! talent mechanic (§6.5 item 4): the item is played against an item of the
//! same budget spent on stats, and the declared price stands while the two
//! measure level. [`validate_on_use`] requires the declaration exactly where
//! no formula exists.
//!
//! ## Determinism
//!
//! Nothing here draws from `GameRng`, and every hook is gated on the wearer
//! carrying an on-use item (`Combatant::on_use_items` empty → nothing runs and
//! nothing is traced), so a match between combatants carrying none is
//! bit-identical to the same match before this module existed.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::components::{Aura, AuraType, DispelType};
use super::constants::WEIGHT_MAX_HEALTH;
use super::equipment::ItemId;
use super::proc_trinkets::proc_effect_budget_weight;

/// The mean arena life after the gates, in seconds: how long a wearer is
/// alive to use an on-use item, on average. Measured over 60 Warrior+Priest
/// mirrors by `realised_uptime_against_the_long_run_bound`
/// (`tests/proc_trinket_probes.rs`) on 2026-10-31 (main at 2c63c1ff); the proc
/// module cites the same figure. Re-measure it when lives move.
pub const ARENA_LIFE_SECS: f32 = 37.8;

// ============================================================================
// CONFIG
// ============================================================================

/// How often an on-use item may be used.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum OnUseCooldown {
    /// Once, then never again this match (the Medallion).
    OncePerMatch,
    /// Again once this many seconds have passed since the last use.
    Seconds(f32),
}

/// The seconds of a mean arena life an effect lasting `duration` is up, when
/// it is used at once and again every time `cooldown` allows: every use an
/// item that starts ready gets in that life, each cut off where the life ends.
fn seconds_up_in_arena_life(duration: f32, cooldown: OnUseCooldown) -> f32 {
    // A once-a-match item has one use, at 0s; it has no gap to multiply (an
    // infinite one would make the first use's start `0 x inf`, NaN).
    let gap = cooldown.seconds().unwrap_or(0.0);
    (0..cooldown.uses_in_arena_life())
        .map(|k| duration.min(ARENA_LIFE_SECS - k as f32 * gap).max(0.0))
        .sum()
}

impl OnUseCooldown {
    /// How many times an item that starts ready can be used in an arena
    /// life: every `k` with `k x cooldown < ARENA_LIFE_SECS`.
    pub fn uses_in_arena_life(self) -> u32 {
        match self {
            OnUseCooldown::OncePerMatch => 1,
            OnUseCooldown::Seconds(cooldown) => (ARENA_LIFE_SECS / cooldown).ceil().max(1.0) as u32,
        }
    }

    /// The seconds the item waits after a use, or `None` when it never
    /// returns.
    pub fn seconds(self) -> Option<f32> {
        match self {
            OnUseCooldown::OncePerMatch => None,
            OnUseCooldown::Seconds(s) => Some(s),
        }
    }
}

/// What using the item does. Each kind has ONE AI rule that decides when to
/// fire it, and one resolution site (`effects::on_use`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum OnUseEffect {
    /// Break every loss of control and movement impairment on the wearer —
    /// stun, fear (horror included), polymorph, incapacitate, Cyclone, root and
    /// slow ([`cc_break_breaks`]). Grants no immunity: the next CC lands.
    CcBreak,
    /// Grant the wearer a buff of this aura type, for the on-use's `duration`,
    /// at its `magnitude` — a stat burst (+spell power, +attack power) or haste
    /// (`AttackSpeedIncrease`, which speeds melee and ranged alike). Restricted
    /// to the stat auras [`proc_effect_budget_weight`] prices. It may carry a
    /// heal over time beside it ([`OnUseConfig::heal_over_time`], the Diamond
    /// Flask).
    Buff(AuraType),
}

impl OnUseEffect {
    /// Whether an item carrying this effect must declare its price, because no
    /// stat weight can price it. Exhaustive: a new effect kind answers it.
    pub fn needs_declared_price(self) -> bool {
        match self {
            OnUseEffect::CcBreak => true,
            OnUseEffect::Buff(_) => false,
        }
    }
}

/// A heal over time an on-use buff carries beside it (the Diamond Flask):
/// `heal_per_tick` every `tick_interval` seconds for the buff's `duration`,
/// landed on the wearer as a `HealingOverTime` aura, so every tick heals
/// through `combat_core::apply_healing` (healing reduction and dampening).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OnUseHot {
    pub heal_per_tick: f32,
    pub tick_interval: f32,
}

impl OnUseHot {
    /// Health healed per second.
    pub fn rate(&self) -> f32 {
        self.heal_per_tick / self.tick_interval
    }
}

/// An item's on-use, as declared in `items.ron` under `on_use:`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OnUseConfig {
    /// What a use does.
    pub effect: OnUseEffect,
    /// How often it may be used.
    pub cooldown: OnUseCooldown,
    /// The DECLARED item-budget price of ONE use, for an effect no stat weight
    /// prices ([`OnUseEffect::needs_declared_price`]); absent otherwise.
    #[serde(default)]
    pub price: Option<f32>,
    /// A buff's magnitude, in its aura's own units (flat points for attack or
    /// spell power, a fraction for haste). `0.0` for a crowd-control break,
    /// which leaves it out of `items.ron`.
    #[serde(default)]
    pub magnitude: f32,
    /// How long a buff, and the heal over time beside it, lasts, in seconds.
    /// `0.0` for a crowd-control break.
    #[serde(default)]
    pub duration: f32,
    /// A heal over time landed with the buff (the Diamond Flask). A buff only.
    #[serde(default)]
    pub heal_over_time: Option<OnUseHot>,
}

impl OnUseConfig {
    /// The item-budget cost of carrying this on-use. A declared price is
    /// charged per use, times its uses in a mean arena life; a buff at
    /// [`on_use_buff_cost`], and a heal over time beside it at
    /// [`on_use_heal_cost`]. See the module docs.
    pub fn budget_cost(&self) -> f32 {
        match self.effect {
            OnUseEffect::CcBreak => {
                self.price.unwrap_or(0.0) * self.cooldown.uses_in_arena_life() as f32
            }
            OnUseEffect::Buff(aura) => {
                on_use_buff_cost(
                    proc_effect_budget_weight(aura).unwrap_or(0.0),
                    self.magnitude,
                    self.duration,
                    self.cooldown,
                ) + self.heal_over_time.map_or(0.0, |hot| {
                    on_use_heal_cost(hot.rate(), self.duration, self.cooldown)
                })
            }
        }
    }

    /// The aura type this on-use's buff carries, or `None` when it is not a
    /// buff. Every consumer that lists, draws or describes on-use BUFFS asks
    /// this, so a crowd-control break is never mistaken for one.
    pub fn buff(&self) -> Option<AuraType> {
        match self.effect {
            OnUseEffect::Buff(aura) => Some(aura),
            OnUseEffect::CcBreak => None,
        }
    }

    /// The buff this on-use grants, ready to hand to `AuraPending` — the
    /// shape a proc trinket's buff has (`ProcConfig::aura`): named after the
    /// item, wearing its icon (`source_item`, so it coexists with a same-stat
    /// buff from an ability or another trinket), never broken by damage, and
    /// `DispelType::Physical` — an item's effect is not a spell, so no purge
    /// or dispel takes it.
    ///
    /// Panics on an on-use that is not a buff; the resolution site asks only
    /// a buff for it.
    pub fn aura(&self, item: ItemId, source_name: &str) -> Aura {
        let effect_type = self
            .buff()
            .unwrap_or_else(|| panic!("{source_name}: a crowd-control break grants no aura"));
        Aura {
            effect_type,
            duration: self.duration,
            magnitude: self.magnitude,
            break_on_damage_threshold: -1.0,
            ability_name: source_name.to_string(),
            dispel_type: DispelType::Physical,
            source_item: Some(item),
            ..Default::default()
        }
    }

    /// The heal over time this on-use lands beside its buff, cast by its
    /// `wearer` (whose healing done it credits), or `None` when it carries
    /// none. Ticks `heal_per_tick` every `tick_interval`, the last on the
    /// frame it runs out, through `process_hot_ticks` and so through
    /// `combat_core::apply_healing`. Named after the item, like the buff, and
    /// as undispellable.
    pub fn heal_aura(&self, item: ItemId, source_name: &str, wearer: Entity) -> Option<Aura> {
        let hot = self.heal_over_time?;
        Some(Aura {
            effect_type: AuraType::HealingOverTime,
            duration: self.duration,
            magnitude: hot.heal_per_tick,
            break_on_damage_threshold: -1.0,
            tick_interval: hot.tick_interval,
            time_until_next_tick: hot.tick_interval,
            caster: Some(wearer),
            ability_name: source_name.to_string(),
            dispel_type: DispelType::Physical,
            source_item: Some(item),
            ..Default::default()
        })
    }
}

/// The budget cost of an on-use STAT BUFF (+`magnitude` of a stat whose item
/// weight is `weight`, for `duration` seconds, on `cooldown`): its weight times
/// its magnitude times the fraction of a mean arena life it is up, counting
/// every use an item that starts ready gets in that life, each cut off where
/// the life ends.
///
/// The pricing rule the stat-burst on-uses (AS-430) are charged by: 15s of +40
/// spell power on a 45s cooldown is about 24.
pub fn on_use_buff_cost(
    weight: f32,
    magnitude: f32,
    duration: f32,
    cooldown: OnUseCooldown,
) -> f32 {
    weight * magnitude * seconds_up_in_arena_life(duration, cooldown) / ARENA_LIFE_SECS
}

/// The budget cost of an on-use HEAL OVER TIME healing `rate` health a second
/// for `duration` seconds, on `cooldown`: the health it heals inside a mean
/// arena life, counted as [`on_use_buff_cost`] counts a buff's time, at the
/// weight of a point of maximum health — the unit an absorb is priced in
/// (Aegis of the Last Stand). A point healed and a point of health more are
/// the same point of damage survived.
pub fn on_use_heal_cost(rate: f32, duration: f32, cooldown: OnUseCooldown) -> f32 {
    WEIGHT_MAX_HEALTH * rate * seconds_up_in_arena_life(duration, cooldown)
}

/// Reject an on-use that cannot be priced or cannot be used.
pub fn validate_on_use(item_name: &str, on_use: &OnUseConfig) -> Result<(), String> {
    if let OnUseCooldown::Seconds(s) = on_use.cooldown {
        if s <= 0.0 {
            return Err(format!(
                "{item_name}: on_use cooldown {s} must be positive — use OncePerMatch for a \
                 single use"
            ));
        }
    }
    if let OnUseEffect::Buff(aura) = on_use.effect {
        if proc_effect_budget_weight(aura).is_none() {
            return Err(format!(
                "{item_name}: on_use buff {aura:?} has no budget price — an on-use may only \
                 grant an effect `proc_effect_budget_weight` prices"
            ));
        }
        if on_use.magnitude <= 0.0 || on_use.duration <= 0.0 {
            return Err(format!(
                "{item_name}: on_use buff needs a positive magnitude and duration (got {} for \
                 {}s)",
                on_use.magnitude, on_use.duration
            ));
        }
        if let OnUseCooldown::Seconds(s) = on_use.cooldown {
            if s < on_use.duration {
                return Err(format!(
                    "{item_name}: on_use cooldown {s} is shorter than its buff's duration {} — \
                     a use while the buff is up would be wasted",
                    on_use.duration
                ));
            }
        }
        if let Some(hot) = on_use.heal_over_time {
            if hot.heal_per_tick <= 0.0
                || hot.tick_interval <= 0.0
                || hot.tick_interval > on_use.duration
            {
                return Err(format!(
                    "{item_name}: on_use heal over time needs a positive heal and a tick \
                     interval within the buff's {}s (got {} every {}s)",
                    on_use.duration, hot.heal_per_tick, hot.tick_interval
                ));
            }
        }
    } else if on_use.magnitude != 0.0 || on_use.duration != 0.0 || on_use.heal_over_time.is_some() {
        return Err(format!(
            "{item_name}: on_use effect {:?} has no magnitude, duration or heal over time — \
             leave them out",
            on_use.effect
        ));
    }
    match (on_use.effect.needs_declared_price(), on_use.price) {
        (true, None) => Err(format!(
            "{item_name}: on_use effect {:?} has no stat weight, so it must declare its \
             price (`price: Some(..)`)",
            on_use.effect
        )),
        (true, Some(p)) if p <= 0.0 => {
            Err(format!("{item_name}: on_use price {p} must be positive"))
        }
        (false, Some(_)) => Err(format!(
            "{item_name}: on_use effect {:?} is priced by formula and may not declare a price",
            on_use.effect
        )),
        _ => Ok(()),
    }
}

/// Whether the crowd-control break removes an aura of this type.
///
/// **Exhaustive on purpose — do not add a `_ =>` arm.** Whether a new
/// `AuraType` is something the break frees its wearer from is a judgement.
///
/// The set is every crowd control on the wearer (user ruling, 2026-10-31) —
/// every movement impairment and every loss of control: stun, fear (a horror
/// too), polymorph, incapacitate (Freezing Trap), Cyclone, root and slow. A
/// Cyclone's immunity does not reach its target's own trinket, which breaks
/// it as in TBC (user ruling, 2026-11-07). Not a silence: TBC's trinket left
/// silences alone, and a silenced unit still moves and swings.
pub const fn cc_break_breaks(aura: AuraType) -> bool {
    match aura {
        AuraType::Stun
        | AuraType::Fear
        | AuraType::Polymorph
        | AuraType::Incapacitate
        | AuraType::Root
        | AuraType::MovementSpeedSlow
        | AuraType::Cyclone => true,
        // Not loss of control: a silenced unit still moves and swings, and a
        // school lockout is an interrupt's.
        AuraType::Silence | AuraType::SpellSchoolLockout => false,
        // Debuffs that are not CC.
        AuraType::AttackSpeedSlow
        | AuraType::CastTimeIncrease
        | AuraType::HealingReduction
        | AuraType::DamageReduction
        | AuraType::AttackPowerReduction
        | AuraType::DamageOverTime
        | AuraType::WeakenedSoul
        | AuraType::DamageTakenIncrease
        | AuraType::ArmorPercentReduction
        | AuraType::LeechMark => false,
        // Buffs, states and markers.
        AuraType::Absorb
        | AuraType::HealingOverTime
        | AuraType::MaxHealthIncrease
        | AuraType::MaxManaIncrease
        | AuraType::ArmorIncrease
        | AuraType::AttackPowerIncrease
        | AuraType::AttackSpeedIncrease
        | AuraType::SpellPowerIncrease
        | AuraType::CritChanceIncrease
        | AuraType::ManaRegenIncrease
        | AuraType::DamageTakenReduction
        | AuraType::DamageImmunity
        | AuraType::FearImmunity
        | AuraType::SilenceInterruptImmunity
        | AuraType::TravelForm
        | AuraType::ShadowSight
        | AuraType::FrostArmorBuff
        | AuraType::WeaponPoison
        | AuraType::WindfuryBuff
        | AuraType::SpellResistanceBuff
        | AuraType::LockoutDurationReduction
        | AuraType::NextCast
        | AuraType::DamageDealtIncrease
        | AuraType::ArmorPercentIncrease
        | AuraType::DamageTransfer
        | AuraType::TriggeredAbsorb
        | AuraType::KillingBlowSave
        | AuraType::HealingReceivedIncrease
        | AuraType::DamageReflect => false,
    }
}

// ============================================================================
// PER-WEARER STATE
// ============================================================================

/// One equipped on-use item and its live cooldown. Built by
/// `Combatant::apply_equipment`, the seam both spawn paths go through.
#[derive(Clone, Debug)]
pub struct OnUseSlot {
    /// Which item this is. Unique within a wearer (unique-equipped).
    pub item: ItemId,
    /// The item's display name, captured at equip time for the log and trace.
    pub name: String,
    /// The on-use's declaration.
    pub config: OnUseConfig,
    /// Seconds until it may be used again; `0.0` is ready.
    pub remaining: f32,
    /// A once-a-match item that has been used.
    pub spent: bool,
    /// While its wearer holds the item for a teammate's dispel
    /// (`class_ai::on_use`): when the wait began, and when it was last asked,
    /// on the trace's clock. A wait not asked for longer than
    /// [`WAIT_GAP_SECS`] belongs to an earlier hold and starts over.
    pub waiting: Option<(f32, f32)>,
}

/// The longest gap between two asks of a held wearer that still counts as one
/// hold: the dispatch asks every frame of a hold, so a longer gap means the
/// hold ended and this is a new one.
pub const WAIT_GAP_SECS: f32 = 0.25;

impl OnUseSlot {
    /// Whether the item may be used now.
    pub fn ready(&self) -> bool {
        !self.spent && self.remaining <= 0.0
    }

    /// Put the item on its cooldown: spent for good, or waiting its seconds.
    pub fn start_cooldown(&mut self) {
        match self.config.cooldown {
            OnUseCooldown::OncePerMatch => self.spent = true,
            OnUseCooldown::Seconds(s) => self.remaining = s,
        }
    }
}

/// Whether `item` is one of `slots` whose use grants a BUFF: an aura the
/// item lands because its wearer USED it, not because it procced.
/// `apply_pending_auras` asks it, so an on-use buff does not log the `procs`
/// line a proc-rate count reads.
pub fn grants_buff(slots: &[OnUseSlot], item: ItemId) -> bool {
    slots
        .iter()
        .any(|slot| slot.item == item && slot.config.buff().is_some())
}

/// Tick every equipped on-use's cooldown down by `dt`. Called from
/// `regenerate_resources`, beside the proc cooldown tick it mirrors.
pub fn tick_on_use_cooldowns(slots: &mut [OnUseSlot], dt: f32) {
    for slot in slots.iter_mut() {
        if slot.remaining > 0.0 {
            slot.remaining = (slot.remaining - dt).max(0.0);
        }
    }
}

/// A use the AI has decided on, resolved by `effects::on_use::process_on_use_items`
/// (the deferred pattern Divine Shield uses: the AI holds no mutable auras).
#[derive(Component, Clone, Debug)]
pub struct OnUsePending {
    pub wearer: Entity,
    pub item: ItemId,
    pub item_name: String,
    /// The on-use's declaration: its effect, and a buff's numbers.
    pub config: OnUseConfig,
}

// ============================================================================
// PRESENTATION
// ============================================================================

/// "+40 spell power", "+20% attack speed": a buff's effect, in its aura's own
/// units. Exhaustive over the priced effects; falls back to the aura's
/// mechanic name for the rest, which validation keeps off every item.
fn buff_phrase(aura: AuraType, magnitude: f32) -> String {
    match aura {
        AuraType::AttackPowerIncrease => format!("+{magnitude:.0} attack power"),
        AuraType::SpellPowerIncrease => format!("+{magnitude:.0} spell power"),
        AuraType::CritChanceIncrease => format!("+{:.0}% crit chance", magnitude * 100.0),
        AuraType::ManaRegenIncrease => format!("+{magnitude:.0} mana per second"),
        AuraType::MaxHealthIncrease => format!("+{magnitude:.0} maximum health"),
        AuraType::MaxManaIncrease => format!("+{magnitude:.0} maximum mana"),
        AuraType::AttackSpeedIncrease => format!("+{:.0}% attack speed", magnitude * 100.0),
        other => format!("{} ({magnitude:.0})", other.display_name()),
    }
}

/// The player-facing sentence for an on-use, shared by every surface that
/// shows the item, so they cannot drift.
pub fn on_use_description(on_use: &OnUseConfig) -> String {
    format!("Use: {}", on_use_sentence(on_use))
}

/// [`on_use_description`] without its "Use:", for a surface that heads the
/// sentence with its own "USE" (the encyclopedia's item page).
pub fn on_use_sentence(on_use: &OnUseConfig) -> String {
    let effect = match on_use.effect {
        OnUseEffect::CcBreak => {
            "Breaks every stun, fear, polymorph, incapacitate, cyclone, root and slow on you"
                .to_string()
        }
        OnUseEffect::Buff(aura) => {
            let buff = format!(
                "Grants {} for {:.0}s",
                buff_phrase(aura, on_use.magnitude),
                on_use.duration
            );
            match on_use.heal_over_time {
                Some(hot) => format!(
                    "{buff}, and heals you for {:.0} every {:.0}s while it lasts",
                    hot.heal_per_tick, hot.tick_interval
                ),
                None => buff,
            }
        }
    };
    let cooldown = match on_use.cooldown {
        OnUseCooldown::OncePerMatch => "Once per match.".to_string(),
        OnUseCooldown::Seconds(s) => format!("{s:.0}s cooldown."),
    };
    format!("{effect}. {cooldown}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::constants::WEIGHT_SPELL_POWER;

    fn cc_break(cooldown: OnUseCooldown, price: Option<f32>) -> OnUseConfig {
        OnUseConfig {
            effect: OnUseEffect::CcBreak,
            cooldown,
            price,
            magnitude: 0.0,
            duration: 0.0,
            heal_over_time: None,
        }
    }

    fn buff(aura: AuraType, magnitude: f32, duration: f32, cooldown: OnUseCooldown) -> OnUseConfig {
        OnUseConfig {
            effect: OnUseEffect::Buff(aura),
            cooldown,
            price: None,
            magnitude,
            duration,
            heal_over_time: None,
        }
    }

    #[test]
    fn a_buff_is_priced_at_its_stat_weight_over_its_uses_in_a_life() {
        use crate::states::play_match::constants::{WEIGHT_ATTACK_POWER, WEIGHT_ATTACK_SPEED};
        let talisman = buff(
            AuraType::SpellPowerIncrease,
            40.0,
            15.0,
            OnUseCooldown::Seconds(45.0),
        );
        let expected =
            on_use_buff_cost(WEIGHT_SPELL_POWER, 40.0, 15.0, OnUseCooldown::Seconds(45.0));
        assert_eq!(talisman.budget_cost(), expected);
        assert!((expected - 23.81).abs() < 0.01, "{expected}");
        // Each term moves the price: the stat's weight, the magnitude, the
        // duration and the cooldown's uses.
        let ap = buff(
            AuraType::AttackPowerIncrease,
            40.0,
            15.0,
            OnUseCooldown::Seconds(45.0),
        );
        assert_eq!(WEIGHT_ATTACK_POWER, WEIGHT_SPELL_POWER);
        assert_eq!(ap.budget_cost(), talisman.budget_cost());
        let haste = buff(
            AuraType::AttackSpeedIncrease,
            0.2,
            15.0,
            OnUseCooldown::Seconds(60.0),
        );
        assert!((haste.budget_cost() - WEIGHT_ATTACK_SPEED * 0.2 * 15.0 / 37.8).abs() < 1e-4);
        assert!(
            buff(
                AuraType::SpellPowerIncrease,
                80.0,
                15.0,
                OnUseCooldown::Seconds(45.0)
            )
            .budget_cost()
                > talisman.budget_cost()
        );
        assert!(
            buff(
                AuraType::SpellPowerIncrease,
                40.0,
                20.0,
                OnUseCooldown::Seconds(45.0)
            )
            .budget_cost()
                > talisman.budget_cost()
        );
        assert!(
            buff(
                AuraType::SpellPowerIncrease,
                40.0,
                15.0,
                OnUseCooldown::Seconds(30.0)
            )
            .budget_cost()
                > talisman.budget_cost()
        );
    }

    #[test]
    fn a_heal_over_time_is_priced_at_the_health_it_heals_in_a_life() {
        // 4 every 5s for 30s, once: 24 health, at 1.0 a point.
        let mut flask = buff(
            AuraType::AttackPowerIncrease,
            20.0,
            30.0,
            OnUseCooldown::OncePerMatch,
        );
        let bare = flask.budget_cost();
        flask.heal_over_time = Some(OnUseHot {
            heal_per_tick: 4.0,
            tick_interval: 5.0,
        });
        assert!((on_use_heal_cost(0.8, 30.0, OnUseCooldown::OncePerMatch) - 24.0).abs() < 1e-4);
        assert!(
            (flask.budget_cost() - bare - 24.0).abs() < 1e-4,
            "{}",
            flask.budget_cost()
        );
        // A heal outlasting the life is charged only what lands inside it.
        assert!((on_use_heal_cost(1.0, 60.0, OnUseCooldown::OncePerMatch) - 37.8).abs() < 1e-3);
    }

    #[test]
    fn a_buff_validates_its_shape() {
        let ok = buff(
            AuraType::SpellPowerIncrease,
            40.0,
            15.0,
            OnUseCooldown::Seconds(45.0),
        );
        assert!(validate_on_use("ok", &ok).is_ok());
        // An unpriceable buff.
        let mut bad = ok.clone();
        bad.effect = OnUseEffect::Buff(AuraType::Stun);
        assert!(validate_on_use("stun", &bad)
            .unwrap_err()
            .contains("no budget price"));
        // No magnitude, no duration.
        let mut bad = ok.clone();
        bad.magnitude = 0.0;
        assert!(validate_on_use("flat", &bad).is_err());
        let mut bad = ok.clone();
        bad.duration = 0.0;
        assert!(validate_on_use("instant", &bad).is_err());
        // A cooldown shorter than the buff.
        let mut bad = ok.clone();
        bad.cooldown = OnUseCooldown::Seconds(10.0);
        assert!(validate_on_use("overlap", &bad)
            .unwrap_err()
            .contains("shorter"));
        // A buff may not declare a price.
        let mut bad = ok.clone();
        bad.price = Some(5.0);
        assert!(validate_on_use("priced", &bad).is_err());
        // A heal over time needs a heal and a tick inside the buff.
        let mut hot = ok.clone();
        hot.heal_over_time = Some(OnUseHot {
            heal_per_tick: 4.0,
            tick_interval: 5.0,
        });
        assert!(validate_on_use("hot", &hot).is_ok());
        hot.heal_over_time = Some(OnUseHot {
            heal_per_tick: 4.0,
            tick_interval: 20.0,
        });
        assert!(validate_on_use("slow hot", &hot).is_err());
        hot.heal_over_time = Some(OnUseHot {
            heal_per_tick: 0.0,
            tick_interval: 5.0,
        });
        assert!(validate_on_use("empty hot", &hot).is_err());
        // A break carries none of a buff's numbers.
        let mut bad = cc_break(OnUseCooldown::OncePerMatch, Some(30.0));
        bad.duration = 5.0;
        assert!(validate_on_use("break", &bad).is_err());
    }

    #[test]
    fn a_buff_lands_as_an_undispellable_item_aura_and_its_heal_beside_it() {
        let wearer = Entity::from_raw(3);
        let mut flask = buff(
            AuraType::AttackPowerIncrease,
            20.0,
            30.0,
            OnUseCooldown::OncePerMatch,
        );
        assert!(flask
            .heal_aura(ItemId::DiamondFlask, "Diamond Flask", wearer)
            .is_none());
        flask.heal_over_time = Some(OnUseHot {
            heal_per_tick: 4.0,
            tick_interval: 5.0,
        });
        let aura = flask.aura(ItemId::DiamondFlask, "Diamond Flask");
        assert_eq!(aura.effect_type, AuraType::AttackPowerIncrease);
        assert_eq!((aura.magnitude, aura.duration), (20.0, 30.0));
        assert_eq!(aura.dispel_type, DispelType::Physical);
        assert_eq!(aura.source_item, Some(ItemId::DiamondFlask));
        assert_eq!(aura.break_on_damage_threshold, -1.0);
        assert!(!aura.can_be_dispelled());
        let heal = flask
            .heal_aura(ItemId::DiamondFlask, "Diamond Flask", wearer)
            .expect("the Flask heals");
        assert_eq!(heal.effect_type, AuraType::HealingOverTime);
        assert_eq!((heal.magnitude, heal.duration), (4.0, 30.0));
        assert_eq!((heal.tick_interval, heal.time_until_next_tick), (5.0, 5.0));
        assert_eq!(heal.caster, Some(wearer));
        assert_eq!(heal.dispel_type, DispelType::Physical);
        assert_eq!(heal.source_item, Some(ItemId::DiamondFlask));
        // The two coexist: same item, different effects.
        assert!(!heal.same_source_as(&aura));
    }

    #[test]
    fn a_buff_describes_its_stat_its_heal_and_its_cooldown() {
        assert_eq!(
            on_use_description(&buff(
                AuraType::SpellPowerIncrease,
                40.0,
                15.0,
                OnUseCooldown::Seconds(45.0)
            )),
            "Use: Grants +40 spell power for 15s. 45s cooldown."
        );
        assert_eq!(
            on_use_description(&buff(
                AuraType::AttackSpeedIncrease,
                0.2,
                15.0,
                OnUseCooldown::Seconds(60.0)
            )),
            "Use: Grants +20% attack speed for 15s. 60s cooldown."
        );
        let mut flask = buff(
            AuraType::AttackPowerIncrease,
            20.0,
            30.0,
            OnUseCooldown::OncePerMatch,
        );
        flask.heal_over_time = Some(OnUseHot {
            heal_per_tick: 4.0,
            tick_interval: 5.0,
        });
        assert_eq!(
            on_use_description(&flask),
            "Use: Grants +20 attack power for 30s, and heals you for 4 every 5s while it \
             lasts. Once per match."
        );
    }

    #[test]
    fn uses_in_an_arena_life_count_every_use_that_starts_inside_it() {
        assert_eq!(ARENA_LIFE_SECS, 37.8);
        assert_eq!(OnUseCooldown::OncePerMatch.uses_in_arena_life(), 1);
        // A 45s cooldown outlives the 37.8s life: one use.
        assert_eq!(OnUseCooldown::Seconds(45.0).uses_in_arena_life(), 1);
        // Exactly the life: the second use would land at 37.8s, which is not
        // inside it.
        assert_eq!(OnUseCooldown::Seconds(37.8).uses_in_arena_life(), 1);
        // 0, 15 and 30s.
        assert_eq!(OnUseCooldown::Seconds(15.0).uses_in_arena_life(), 3);
    }

    #[test]
    fn the_card_example_prices_at_its_uses_not_the_long_run_bound() {
        // §6.5 item 3: 15s of +40 SP on a 45s cooldown is 1.5 x 40 x 15/37.8.
        let cost = on_use_buff_cost(WEIGHT_SPELL_POWER, 40.0, 15.0, OnUseCooldown::Seconds(45.0));
        assert!((cost - 1.5 * 40.0 * 15.0 / 37.8).abs() < 1e-4, "{cost}");
        assert!((cost - 23.81).abs() < 0.01, "{cost}");
        // ...where the long-run bound would charge 15 and underprice it.
        let long_run = WEIGHT_SPELL_POWER * 40.0 * 15.0 / 60.0;
        assert!(cost > long_run);
    }

    #[test]
    fn a_later_use_is_cut_off_where_the_life_ends() {
        // 0s and 30s: the first buff is whole, the second runs 7.8s of its 15.
        let cost = on_use_buff_cost(1.0, 37.8, 15.0, OnUseCooldown::Seconds(30.0));
        assert!((cost - 22.8).abs() < 1e-4, "{cost}");
    }

    #[test]
    fn a_declared_price_is_charged_per_use() {
        assert_eq!(
            cc_break(OnUseCooldown::OncePerMatch, Some(30.0)).budget_cost(),
            30.0
        );
        assert_eq!(
            cc_break(OnUseCooldown::Seconds(15.0), Some(10.0)).budget_cost(),
            30.0
        );
    }

    #[test]
    fn validation_requires_a_declared_price_exactly_where_no_formula_exists() {
        assert!(validate_on_use("ok", &cc_break(OnUseCooldown::OncePerMatch, Some(30.0))).is_ok());
        let err =
            validate_on_use("free", &cc_break(OnUseCooldown::OncePerMatch, None)).unwrap_err();
        assert!(err.contains("must declare its price"), "{err}");
        assert!(
            validate_on_use("zero", &cc_break(OnUseCooldown::OncePerMatch, Some(0.0))).is_err()
        );
        assert!(
            validate_on_use("instant", &cc_break(OnUseCooldown::Seconds(0.0), Some(5.0))).is_err()
        );
    }

    #[test]
    fn the_break_takes_exactly_the_medallion_scope() {
        let broken: Vec<AuraType> = AuraType::ALL
            .iter()
            .copied()
            .filter(|a| cc_break_breaks(*a))
            .collect();
        let mut expected = vec![
            AuraType::Stun,
            AuraType::Fear,
            AuraType::Polymorph,
            AuraType::Incapacitate,
            AuraType::Cyclone,
            AuraType::Root,
            AuraType::MovementSpeedSlow,
        ];
        expected.sort_by_key(|a| AuraType::ALL.iter().position(|x| x == a).unwrap());
        assert_eq!(broken, expected);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn the_break_frees_every_hold_and_impairment_whole_and_nothing_else() {
        use crate::states::play_match::components::{ActiveAuras, Aura, CompoundDebuff};
        // Built field by field: a test fixture, not an apply site, so it
        // stays out of `aura_catalog_audit`'s literal scan.
        let aura = |effect_type: AuraType, name: &str, compound: Option<CompoundDebuff>| {
            let mut aura = Aura::default();
            aura.effect_type = effect_type;
            aura.ability_name = name.to_string();
            aura.duration = 5.0;
            aura.compound = compound;
            aura
        };
        let mut auras = ActiveAuras {
            auras: vec![
                aura(AuraType::Stun, "Hammer of Justice", None),
                aura(AuraType::Fear, "Death Coil", None),
                aura(AuraType::Polymorph, "Polymorph", None),
                aura(AuraType::Incapacitate, "Freezing Trap", None),
                aura(
                    AuraType::Root,
                    "Entangling Roots",
                    Some(CompoundDebuff::EntanglingRoots),
                ),
                aura(
                    AuraType::DamageOverTime,
                    "Entangling Roots",
                    Some(CompoundDebuff::EntanglingRoots),
                ),
                aura(AuraType::MovementSpeedSlow, "Hamstring", None),
                aura(AuraType::Silence, "Silence", None),
                aura(AuraType::DamageOverTime, "Corruption", None),
                aura(AuraType::HealingReduction, "Mortal Strike", None),
                aura(AuraType::Cyclone, "Cyclone", None),
                aura(AuraType::SpellPowerIncrease, "Arcane Brilliance", None),
            ],
        };
        let broken = auras.break_debuffs_where(cc_break_breaks);
        assert_eq!(
            broken,
            [
                "Hammer of Justice",
                "Death Coil",
                "Polymorph",
                "Freezing Trap",
                "Entangling Roots",
                "Hamstring",
                "Cyclone"
            ]
        );
        let left: Vec<&str> = auras
            .auras
            .iter()
            .map(|a| a.ability_name.as_str())
            .collect();
        // The roots' damage rider went with its root.
        assert_eq!(
            left,
            [
                "Silence",
                "Corruption",
                "Mortal Strike",
                "Arcane Brilliance"
            ]
        );
    }

    #[test]
    fn a_slot_is_ready_until_used_and_a_once_a_match_one_never_returns() {
        let mut once = OnUseSlot {
            item: ItemId::MedallionOfTheAlliance,
            name: "Medallion".into(),
            config: cc_break(OnUseCooldown::OncePerMatch, Some(30.0)),
            remaining: 0.0,
            spent: false,
            waiting: None,
        };
        assert!(once.ready());
        once.start_cooldown();
        assert!(!once.ready());
        tick_on_use_cooldowns(std::slice::from_mut(&mut once), 1000.0);
        assert!(!once.ready(), "a once-a-match item came back");

        let mut timed = OnUseSlot {
            config: cc_break(OnUseCooldown::Seconds(60.0), Some(10.0)),
            ..once.clone()
        };
        timed.spent = false;
        timed.start_cooldown();
        assert!(!timed.ready());
        tick_on_use_cooldowns(std::slice::from_mut(&mut timed), 59.9);
        assert!(!timed.ready());
        tick_on_use_cooldowns(std::slice::from_mut(&mut timed), 0.2);
        assert!(timed.ready());
    }

    #[test]
    fn regenerate_resources_ticks_an_on_use_cooldown() {
        use crate::states::match_config::CharacterClass;
        use crate::states::play_match::combat_core::regenerate_resources;
        use crate::states::play_match::components::Combatant;
        use bevy::ecs::system::RunSystemOnce;
        use std::time::Duration;

        let mut combatant = Combatant::new(1, 0, CharacterClass::Warrior);
        let mut slot = OnUseSlot {
            item: ItemId::MedallionOfTheAlliance,
            name: "Medallion".into(),
            config: cc_break(OnUseCooldown::Seconds(60.0), Some(10.0)),
            remaining: 0.0,
            spent: false,
            waiting: None,
        };
        slot.start_cooldown();
        combatant.on_use_items.push(slot);
        let mut world = World::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(5));
        world.insert_resource(time);
        let wearer = world.spawn(combatant).id();
        world
            .run_system_once(regenerate_resources)
            .expect("regenerate_resources ran");
        let remaining = world.get::<Combatant>(wearer).unwrap().on_use_items[0].remaining;
        assert!((remaining - 55.0).abs() < 1e-4, "{remaining}");
    }

    #[test]
    fn the_description_names_the_effect_and_the_cooldown() {
        assert_eq!(
            on_use_description(&cc_break(OnUseCooldown::OncePerMatch, Some(30.0))),
            "Use: Breaks every stun, fear, polymorph, incapacitate, cyclone, root and slow on you. \
             Once per match."
        );
        assert!(
            on_use_description(&cc_break(OnUseCooldown::Seconds(120.0), Some(1.0)))
                .ends_with("120s cooldown.")
        );
    }
}
