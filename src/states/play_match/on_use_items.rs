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

use super::components::AuraType;
use super::equipment::ItemId;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnUseEffect {
    /// Break every loss of control and movement impairment on the wearer —
    /// stun, fear (horror included), polymorph, incapacitate, root and slow
    /// ([`cc_break_breaks`]). Grants no immunity: the next CC lands.
    CcBreak,
}

impl OnUseEffect {
    /// Whether an item carrying this effect must declare its price, because no
    /// stat weight can price it. Exhaustive: a new effect kind answers it.
    pub fn needs_declared_price(self) -> bool {
        match self {
            OnUseEffect::CcBreak => true,
        }
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
}

impl OnUseConfig {
    /// The item-budget cost of carrying this on-use: its per-use price times
    /// its uses in a mean arena life. See the module docs.
    pub fn budget_cost(&self) -> f32 {
        self.price.unwrap_or(0.0) * self.cooldown.uses_in_arena_life() as f32
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
    let uses = cooldown.uses_in_arena_life();
    let gap = cooldown.seconds().unwrap_or(f32::INFINITY);
    let up: f32 = (0..uses)
        .map(|k| duration.min(ARENA_LIFE_SECS - k as f32 * gap).max(0.0))
        .sum();
    weight * magnitude * up / ARENA_LIFE_SECS
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
/// too), polymorph, incapacitate (Freezing Trap), root and slow. A Cyclone
/// only if [`CC_BREAK_TAKES_CYCLONE`]. Not a silence: TBC's trinket left
/// silences alone, and a silenced unit still moves and swings.
pub const fn cc_break_breaks(aura: AuraType) -> bool {
    match aura {
        AuraType::Stun
        | AuraType::Fear
        | AuraType::Polymorph
        | AuraType::Incapacitate
        | AuraType::Root
        | AuraType::MovementSpeedSlow => true,
        AuraType::Cyclone => CC_BREAK_TAKES_CYCLONE,
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
        | AuraType::WeakenedSoul => false,
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
        | AuraType::TravelForm
        | AuraType::ShadowSight
        | AuraType::FrostArmorBuff
        | AuraType::WeaponPoison
        | AuraType::WindfuryBuff
        | AuraType::SpellResistanceBuff
        | AuraType::LockoutDurationReduction => false,
    }
}

/// Whether the crowd-control break frees its wearer from a Cyclone — and so
/// whether a cycloned wearer may use it at all (`combat_ai::dispatch_while_held`).
/// `false`, as in TBC: a Cyclone makes its target immune to everything, its
/// own trinket included. THE ONE LINE a ruling on Cyclone changes.
pub const CC_BREAK_TAKES_CYCLONE: bool = false;

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
    pub effect: OnUseEffect,
}

// ============================================================================
// PRESENTATION
// ============================================================================

/// The player-facing sentence for an on-use, shared by every surface that
/// shows the item, so they cannot drift.
pub fn on_use_description(on_use: &OnUseConfig) -> String {
    format!("Use: {}", on_use_sentence(on_use))
}

/// [`on_use_description`] without its "Use:", for a surface that heads the
/// sentence with its own "USE" (the encyclopedia's item page).
pub fn on_use_sentence(on_use: &OnUseConfig) -> String {
    let effect = match on_use.effect {
        OnUseEffect::CcBreak if CC_BREAK_TAKES_CYCLONE => {
            "Breaks every stun, fear, polymorph, incapacitate, cyclone, root and slow on you"
        }
        OnUseEffect::CcBreak => {
            "Breaks every stun, fear, polymorph, incapacitate, root and slow on you"
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
        }
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
                "Hamstring"
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
                "Cyclone",
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
            "Use: Breaks every stun, fear, polymorph, incapacitate, root and slow on you. \
             Once per match."
        );
        assert!(
            on_use_description(&cc_break(OnUseCooldown::Seconds(120.0), Some(1.0)))
                .ends_with("120s cooldown.")
        );
    }
}
