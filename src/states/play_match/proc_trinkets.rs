//! Proc trinkets — a TRIGGER plus an EFFECT, on a per-trinket internal cooldown.
//!
//! The Dragonspine Trophy shape: wearing the item does nothing on its own, but
//! a qualifying combat event has a chance to grant the wearer a short stat buff.
//! Each trinket carries its OWN internal cooldown (ICD), so one proc cannot
//! re-trigger while its own buff is up — and two DIFFERENT trinkets can be up at
//! the same time, which is what makes the second trinket socket worth filling.
//! There is deliberately NO global proc lock.
//!
//! ## What a proc costs
//!
//! A trigger-and-effect is not a stat field, so nothing in the item budget would
//! see it unless it is priced explicitly. It is priced at its EXPECTED VALUE:
//!
//! ```text
//! cost = stat_weight(effect) * magnitude * uptime_ceiling
//! uptime_ceiling = duration / (duration + internal_cooldown)
//! ```
//!
//! The uptime term is the ICD-bounded CEILING, not a realised rate. That is
//! deliberate and it is the only defensible choice for a budget check: realised
//! uptime depends on who wears the item, which enemy they face and how the match
//! goes, none of which are properties of the ITEM. The ceiling is a property of
//! the item, so the ceiling is what the budget can check. The realised rate is a
//! thing to MEASURE, and is reported per sweep rather than assumed here.
//!
//! A corollary worth stating plainly: **`chance` is a feel knob, not a power
//! knob.** Two trinkets with the same effect, duration and ICD cost the same
//! whether they proc at 5% or 50%, because the ICD — not the trigger rate — is
//! what bounds sustained uptime. A low `chance` only means the wearer reaches
//! that ceiling less often, which the pricing treats as the wearer's loss.
//!
//! ## Determinism
//!
//! Every roll is drawn from the seeded `GameRng`, and the whole mechanism is
//! gated on the wearer actually having a proc trinket equipped (`Combatant`'s
//! `proc_trinkets` empty → the hook returns before touching the RNG). A loadout
//! with no proc trinket therefore draws NOTHING new, so matches between
//! combatants carrying none are bit-identical to the same match before this
//! module existed. That is the same "prove the no-op case is a no-op" shape as
//! `steer_toward_goal`'s `if obstacles.is_empty()`.
//!
//! ## What triggers exist, and what they do NOT cover
//!
//! [`ProcTrigger`] covers the minimum that serves melee, casters and healers:
//! a landed melee swing, a completed cast, and a completed heal. Two Classic
//! shapes are deliberately absent — on-crit and on-damage-taken — see the enum's
//! own documentation.
//!
//! **`SpellCast` and `Heal` fire on a cast that has a CAST TIME, and only
//! that.** Two neighbours are outside them, both because they resolve
//! somewhere other than `process_casting`'s completion pass:
//!
//! - an INSTANT ability never creates a `CastingState` at all, so Holy Shock
//!   and Sinister Strike proc nothing;
//! - a CHANNEL (Drain Life) is ticked by `process_channeling`, so neither its
//!   start nor its per-tick healing procs anything.
//!
//! Every class is still served — melee kits proc off `MeleeHit`, and the
//! caster and healer kits are cast-time abilities — so this bounds which
//! abilities a trinket can key off, not which classes can wear one.

use serde::{Deserialize, Serialize};

use super::components::GameRng;
use super::components::{Aura, AuraType, DispelType};
use super::constants::{
    WEIGHT_ATTACK_POWER, WEIGHT_CRIT_CHANCE, WEIGHT_MANA_REGEN, WEIGHT_MAX_HEALTH, WEIGHT_MAX_MANA,
    WEIGHT_SPELL_POWER,
};
use super::equipment::ItemId;

// ============================================================================
// TRIGGERS
// ============================================================================

/// The variant list is written ONCE and every derived form expands from the
/// same tokens, so a trigger cannot exist without also being enumerable and
/// nameable. Same reasoning as `item_ids!` in `equipment.rs`: the failure this
/// closes is a hand-maintained `ALL` slice that quietly stops covering the
/// newest variant while still reading like a whole-enum sweep.
///
/// It generates a LIST, not a DECISION. Every judgement about a trigger — where
/// it fires from, what it is worth — stays an exhaustive `match` elsewhere, so
/// the compiler asks rather than defaulting.
macro_rules! proc_triggers {
    ($( $(#[$meta:meta])* $variant:ident ),* $(,)?) => {
        /// What combat event gives a proc trinket its chance to fire.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum ProcTrigger {
            $( $(#[$meta])* $variant, )*
        }

        impl ProcTrigger {
            /// Every trigger, in declaration order. Generated from the enum's
            /// own token list, so it cannot fall behind it.
            pub fn all() -> &'static [ProcTrigger] {
                &[ $( ProcTrigger::$variant, )* ]
            }

            /// The trigger's canonical string form — its variant name, which is
            /// also its serde form and so its spelling in `items.ron`.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( ProcTrigger::$variant => stringify!($variant), )*
                }
            }
        }
    };
}

proc_triggers! {
    /// A melee auto-attack that LANDED on a living enemy. Rolled in the apply
    /// pass of `combat_auto_attack`, alongside the Crippling Poison roll, so
    /// a swing dropped by the friendly-CC guard or by a same-frame death never
    /// procs. Ranged auto-attacks do not fire it.
    MeleeHit,
    /// A cast with a cast time that COMPLETED and landed — the same resolution
    /// point at which mana is charged. A cast interrupted, or fizzled at
    /// completion by line of sight or a dead target, does not fire it. Instant
    /// abilities have no cast phase and do not fire it (see the module docs).
    SpellCast,
    /// A completed HEALING cast that landed. A strict subset of the events that
    /// fire [`ProcTrigger::SpellCast`], so a trinket keyed to `Heal` procs only
    /// while its wearer is actually healing. Heal-over-time ticks and instant
    /// heals do not fire it.
    Heal,
    // DELIBERATELY ABSENT, and why:
    //
    // `Crit` — the same two seams as MeleeHit/SpellCast with one extra
    // predicate. It changes only the RATE at which a proc gets its chance, and
    // the rate is already a declared knob (`chance`), so it would prove nothing
    // about the machinery that MeleeHit does not.
    //
    // `DamageTaken` — the one trigger that fires from inside the damage
    // application path, which is where a proc could re-enter itself (a proc
    // that deals damage, procced by damage). Leaving it out is what lets this
    // module state flatly that no proc effect deals damage or healing, and so
    // that a proc can never trigger another proc. Adding it is a card of its
    // own, and that analysis is its cost of entry.
}

// ============================================================================
// CONFIG
// ============================================================================

/// A trinket's proc, as declared in `items.ron` under `proc:`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcConfig {
    /// What combat event gives this trinket its chance to fire.
    pub trigger: ProcTrigger,
    /// Probability per qualifying event, in `(0.0, 1.0]`.
    pub chance: f32,
    /// The buff the proc grants its wearer. Restricted to the stat auras
    /// [`proc_effect_budget_weight`] can price; anything else is rejected at
    /// load.
    pub effect: AuraType,
    /// The buff's magnitude, in the effect aura's own units (flat points for
    /// attack/spell power, a fraction for crit chance, mana per second for
    /// mana regen).
    pub magnitude: f32,
    /// How long the buff lasts, in seconds.
    pub duration: f32,
    /// How long this trinket cannot proc again, in seconds, measured from the
    /// moment it fires. Must be at least `duration` — that is what makes "a
    /// proc cannot stack on top of itself" structural rather than hoped for.
    pub internal_cooldown: f32,
}

impl ProcConfig {
    /// The largest fraction of a match this proc's buff can be up for, given
    /// its own ICD: it lasts `duration` and then cannot return for
    /// `internal_cooldown - duration`. Reached only by a wearer who procs on
    /// the first eligible event after every cooldown, so it is a ceiling and
    /// not a prediction.
    pub fn uptime_ceiling(&self) -> f32 {
        self.duration / (self.duration + self.internal_cooldown)
    }

    /// The item-budget cost of carrying this proc — see the module docs.
    /// `0.0` for an effect with no price, which cannot ship: [`validate_proc`]
    /// rejects it before it can reach an item.
    pub fn budget_cost(&self) -> f32 {
        proc_effect_budget_weight(self.effect).unwrap_or(0.0)
            * self.magnitude
            * self.uptime_ceiling()
    }

    /// The buff this proc grants, ready to hand to `AuraPending`.
    ///
    /// `distinct_by_source` is set, so this buff coexists with a same-stat buff
    /// from another source — a Warrior's Battle Shout does not swallow an
    /// attack-power proc, and two different trinkets granting the same stat are
    /// both live. That is the behaviour the second trinket socket depends on.
    pub fn aura(&self, source_name: &str) -> Aura {
        Aura {
            effect_type: self.effect,
            duration: self.duration,
            magnitude: self.magnitude,
            break_on_damage_threshold: -1.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: None,
            ability_name: source_name.to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None,
            applied_this_frame: false,
            backlash_damage: None,
            dr_category_override: None,
            dispel_type: DispelType::Auto,
            compound: None,
            distinct_by_source: true,
        }
    }
}

/// What one point of an effect aura's magnitude costs in item budget, or `None`
/// when the aura is not something a proc may grant.
///
/// **Exhaustive on purpose — do not add a `_ =>` arm.** Whether a new
/// `AuraType` is a priceable proc effect is a JUDGEMENT, and a wildcard would
/// answer it silently: either by letting an unpriced effect ship free, or by
/// locking out an effect nobody decided to lock out. A new variant should stop
/// the build here and make its author choose.
///
/// The priced set is the stat auras whose magnitude is denominated in the same
/// unit as an item stat field, at the same `WEIGHT_*` the item budget already
/// charges for that stat. Nothing here invents a second pricing table.
pub fn proc_effect_budget_weight(effect: AuraType) -> Option<f32> {
    match effect {
        // --- priced: a flat or fractional stat, in an item stat's own units ---
        AuraType::AttackPowerIncrease => Some(WEIGHT_ATTACK_POWER),
        AuraType::SpellPowerIncrease => Some(WEIGHT_SPELL_POWER),
        AuraType::CritChanceIncrease => Some(WEIGHT_CRIT_CHANCE),
        AuraType::ManaRegenIncrease => Some(WEIGHT_MANA_REGEN),
        AuraType::MaxHealthIncrease => Some(WEIGHT_MAX_HEALTH),
        AuraType::MaxManaIncrease => Some(WEIGHT_MAX_MANA),

        // --- not priceable as a proc effect ---
        // Crowd control. A trinket that randomly stuns the enemy it hit is a
        // different feature with a different cost model (counterplay-free
        // seconds, not stat points), and it would need a target other than the
        // wearer, which a proc effect does not have.
        AuraType::Root
        | AuraType::Stun
        | AuraType::Fear
        | AuraType::Polymorph
        | AuraType::Incapacitate
        | AuraType::Silence
        | AuraType::SpellSchoolLockout => None,
        // Debuffs and enemy-facing effects: same objection — a proc buffs its
        // WEARER, and these only mean anything aimed at somebody else.
        AuraType::MovementSpeedSlow
        | AuraType::AttackSpeedSlow
        | AuraType::HealingReduction
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease
        | AuraType::AttackPowerReduction
        | AuraType::DamageOverTime => None,
        // Throughput effects denominated in health rather than in a stat.
        // Pricing an absorb or a heal-over-time against the stat table would be
        // inventing a second table; they are their own card when wanted.
        AuraType::Absorb | AuraType::HealingOverTime => None,
        // Percentage mitigation and outright immunity. Not linear in a stat
        // weight at all — `DamageTakenReduction` at magnitude 1.0 is immunity,
        // which the budget would happily sell for one point.
        AuraType::DamageTakenReduction | AuraType::DamageImmunity | AuraType::FearImmunity => None,
        // Reveals stealthed enemies and reveals the holder. A visibility swap,
        // not a quantity — there is no magnitude to multiply.
        AuraType::ShadowSight => None,
        // Markers and bookkeeping auras carry no magnitude to price.
        AuraType::WeakenedSoul
        | AuraType::FrostArmorBuff
        | AuraType::WeaponPoison
        | AuraType::WindfuryBuff
        | AuraType::SpellResistanceBuff
        | AuraType::LockoutDurationReduction => None,
    }
}

/// Reject a proc that cannot be priced, cannot fire, or could stack on itself.
///
/// `internal_cooldown >= duration` is the rule that makes the user-facing
/// promise — "trinket procs cannot stack on top of each other" — a property of
/// the DATA rather than of the roll: a trinket whose buff is still up is still
/// on its own cooldown, so there is no ordering in which it doubles up.
pub fn validate_proc(item_name: &str, proc: &ProcConfig) -> Result<(), String> {
    if proc_effect_budget_weight(proc.effect).is_none() {
        return Err(format!(
            "{}: proc effect {:?} has no budget price — a proc may only grant an effect \
             `proc_effect_budget_weight` prices",
            item_name, proc.effect
        ));
    }
    if !(proc.chance > 0.0 && proc.chance <= 1.0) {
        return Err(format!(
            "{}: proc chance {} is outside (0.0, 1.0]",
            item_name, proc.chance
        ));
    }
    if proc.duration <= 0.0 {
        return Err(format!(
            "{}: proc duration {} must be positive",
            item_name, proc.duration
        ));
    }
    if proc.magnitude <= 0.0 {
        return Err(format!(
            "{}: proc magnitude {} must be positive",
            item_name, proc.magnitude
        ));
    }
    if proc.internal_cooldown < proc.duration {
        return Err(format!(
            "{}: proc internal_cooldown {} is shorter than its duration {} — the buff could \
             stack on itself",
            item_name, proc.internal_cooldown, proc.duration
        ));
    }
    Ok(())
}

// ============================================================================
// PER-WEARER STATE
// ============================================================================

/// One equipped proc trinket and its live internal cooldown.
///
/// Built by `Combatant::apply_equipment`, which is the single seam BOTH spawn
/// paths (graphical `spawn_combatant` and `headless::runner`) already go
/// through — so there is no second place to remember.
#[derive(Clone, Debug)]
pub struct ProcSlot {
    /// Which item this proc came from. Two sockets can never hold the same
    /// trinket (unique-equipped), so this is unique within a wearer.
    pub item: ItemId,
    /// The item's display name, captured at equip time so the hook sites need
    /// no `ItemDefinitions`. It is what the buff bar and combat log show.
    pub name: String,
    /// The proc's declaration.
    pub config: ProcConfig,
    /// Seconds until this trinket may proc again. `0.0` means ready.
    pub remaining_icd: f32,
}

/// Tick every equipped proc's internal cooldown down by `dt`.
///
/// Called from `regenerate_resources`, next to the ability-cooldown tick it
/// mirrors, rather than from a system of its own — same loop, same concept, one
/// fewer registration to forget.
pub fn tick_proc_cooldowns(slots: &mut [ProcSlot], dt: f32) {
    for slot in slots.iter_mut() {
        if slot.remaining_icd > 0.0 {
            slot.remaining_icd = (slot.remaining_icd - dt).max(0.0);
        }
    }
}

/// Roll every ready proc whose trigger is in `fired`, and return the buffs that
/// landed, in slot order.
///
/// **Draws no RNG when `slots` is empty**, which is what makes a loadout with no
/// proc trinket bit-identical to the same match before this module existed.
/// Callers still guard on emptiness themselves so the call is free; this is the
/// belt to that braces.
///
/// One roll per eligible slot per event, in the wearer's socket order (a
/// `Loadout` is a `BTreeMap`, so `apply_equipment` builds the slots in
/// `ItemSlot` order). A firing slot goes straight onto its own cooldown; nothing
/// here consults any other slot, which is the no-global-lock rule.
pub fn roll_procs(
    slots: &mut [ProcSlot],
    fired: &[ProcTrigger],
    game_rng: &mut GameRng,
) -> Vec<Aura> {
    if slots.is_empty() {
        return Vec::new();
    }
    let mut granted = Vec::new();
    for slot in slots.iter_mut() {
        if slot.remaining_icd > 0.0 || !fired.contains(&slot.config.trigger) {
            continue;
        }
        if game_rng.random_f32() < slot.config.chance {
            slot.remaining_icd = slot.config.internal_cooldown;
            granted.push(slot.config.aura(&slot.name));
        }
    }
    granted
}

// ============================================================================
// PRESENTATION
// ============================================================================

/// The player-facing sentence for a proc, shared by the encyclopedia detail
/// page and the loadout editor's tooltip so the two cannot drift.
pub fn proc_description(proc: &ProcConfig) -> String {
    let event = match proc.trigger {
        ProcTrigger::MeleeHit => "on a melee hit",
        ProcTrigger::SpellCast => "on a completed spell cast",
        ProcTrigger::Heal => "on a completed heal",
    };
    format!(
        "Chance {} ({:.0}%) to gain {} for {:.0}s. Cannot occur more than once every {:.0}s.",
        event,
        proc.chance * 100.0,
        proc_effect_phrase(proc),
        proc.duration,
        proc.internal_cooldown,
    )
}

/// The effect half of [`proc_description`] — "+55 attack power", "+10% crit
/// chance". Exhaustive over the PRICED effects and falls back to the aura's own
/// mechanic name for the rest, which cannot reach an item anyway.
fn proc_effect_phrase(proc: &ProcConfig) -> String {
    match proc.effect {
        AuraType::AttackPowerIncrease => format!("+{:.0} attack power", proc.magnitude),
        AuraType::SpellPowerIncrease => format!("+{:.0} spell power", proc.magnitude),
        AuraType::CritChanceIncrease => format!("+{:.0}% crit chance", proc.magnitude * 100.0),
        AuraType::ManaRegenIncrease => format!("+{:.0} mana per second", proc.magnitude),
        AuraType::MaxHealthIncrease => format!("+{:.0} maximum health", proc.magnitude),
        AuraType::MaxManaIncrease => format!("+{:.0} maximum mana", proc.magnitude),
        other => format!("{} ({:.0})", other.display_name(), proc.magnitude),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(effect: AuraType, magnitude: f32, duration: f32, icd: f32) -> ProcConfig {
        ProcConfig {
            trigger: ProcTrigger::MeleeHit,
            chance: 0.15,
            effect,
            magnitude,
            duration,
            internal_cooldown: icd,
        }
    }

    // ---- the pricing rule ----

    #[test]
    fn uptime_ceiling_is_duration_over_duration_plus_icd() {
        let c = cfg(AuraType::AttackPowerIncrease, 55.0, 10.0, 40.0);
        assert!((c.uptime_ceiling() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn budget_cost_is_weight_times_magnitude_times_uptime() {
        // The card's own worked example: +100 attack power at 20% uptime is
        // 1.5 * 100 * 0.20 = 30 points, which overspends an ilvl-58 trinket
        // (24.5 points) on the proc alone.
        let c = cfg(AuraType::AttackPowerIncrease, 100.0, 10.0, 40.0);
        assert!((c.budget_cost() - 30.0).abs() < 1e-4, "{}", c.budget_cost());
        assert!(c.budget_cost() > 58.0 * 0.75 * 0.5625);
    }

    #[test]
    fn budget_cost_scales_with_every_one_of_its_three_terms() {
        let base = cfg(AuraType::AttackPowerIncrease, 40.0, 10.0, 40.0).budget_cost();
        // magnitude
        assert!(cfg(AuraType::AttackPowerIncrease, 80.0, 10.0, 40.0).budget_cost() > base);
        // uptime, via a shorter ICD
        assert!(cfg(AuraType::AttackPowerIncrease, 40.0, 10.0, 20.0).budget_cost() > base);
        // weight, via a dearer stat at the same magnitude
        assert!(cfg(AuraType::CritChanceIncrease, 40.0, 10.0, 40.0).budget_cost() > base);
    }

    #[test]
    fn chance_does_not_change_the_price() {
        // Stated in the module docs: the ICD bounds sustained uptime, so the
        // trigger rate is a feel knob. If this ever stops holding, the doc is
        // wrong and so is every trinket priced under it.
        let mut cheap = cfg(AuraType::SpellPowerIncrease, 50.0, 12.0, 50.0);
        cheap.chance = 0.01;
        let mut certain = cheap.clone();
        certain.chance = 1.0;
        assert_eq!(cheap.budget_cost(), certain.budget_cost());
    }

    #[test]
    fn every_priced_effect_is_priced_at_the_item_budget_weight_for_that_stat() {
        // The rule the module claims: nothing here invents a second pricing
        // table. Named pairs, not a count — a weight that silently changed to
        // some other positive number would still pass a `> 0.0` check.
        let expected = [
            (AuraType::AttackPowerIncrease, WEIGHT_ATTACK_POWER),
            (AuraType::SpellPowerIncrease, WEIGHT_SPELL_POWER),
            (AuraType::CritChanceIncrease, WEIGHT_CRIT_CHANCE),
            (AuraType::ManaRegenIncrease, WEIGHT_MANA_REGEN),
            (AuraType::MaxHealthIncrease, WEIGHT_MAX_HEALTH),
            (AuraType::MaxManaIncrease, WEIGHT_MAX_MANA),
        ];
        for (effect, weight) in expected {
            assert_eq!(
                proc_effect_budget_weight(effect),
                Some(weight),
                "{:?} is not priced at its item-budget weight",
                effect
            );
        }
        // ...and the priced SET is exactly those six. A seventh variant
        // becoming priceable must be a deliberate edit here, not a surprise.
        let priced: Vec<AuraType> = AuraType::ALL
            .iter()
            .copied()
            .filter(|a| proc_effect_budget_weight(*a).is_some())
            .collect();
        let mut named: Vec<AuraType> = expected.iter().map(|(a, _)| *a).collect();
        named.sort_by_key(|a| AuraType::ALL.iter().position(|x| x == a).unwrap());
        assert_eq!(priced, named, "the priced effect set changed");
    }

    // ---- validation ----

    #[test]
    fn validate_accepts_a_well_formed_proc() {
        assert!(validate_proc("ok", &cfg(AuraType::AttackPowerIncrease, 55.0, 10.0, 45.0)).is_ok());
    }

    #[test]
    fn validate_rejects_an_unpriceable_effect() {
        let err = validate_proc("cc", &cfg(AuraType::Stun, 1.0, 4.0, 30.0)).unwrap_err();
        assert!(err.contains("no budget price"), "{}", err);
    }

    #[test]
    fn validate_rejects_an_icd_shorter_than_the_duration() {
        // The stacking rule, as data: a buff that outlives its own cooldown
        // could be applied on top of itself.
        let err = validate_proc(
            "stacky",
            &cfg(AuraType::AttackPowerIncrease, 20.0, 20.0, 10.0),
        )
        .unwrap_err();
        assert!(err.contains("stack on itself"), "{}", err);
        // ...and equal is allowed: back-to-back, never overlapping.
        assert!(validate_proc(
            "touching",
            &cfg(AuraType::AttackPowerIncrease, 20.0, 10.0, 10.0)
        )
        .is_ok());
    }

    #[test]
    fn validate_rejects_out_of_range_chance_and_non_positive_numbers() {
        let mut zero_chance = cfg(AuraType::AttackPowerIncrease, 20.0, 10.0, 30.0);
        zero_chance.chance = 0.0;
        assert!(validate_proc("x", &zero_chance).is_err());

        let mut over_chance = cfg(AuraType::AttackPowerIncrease, 20.0, 10.0, 30.0);
        over_chance.chance = 1.5;
        assert!(validate_proc("x", &over_chance).is_err());

        assert!(validate_proc("x", &cfg(AuraType::AttackPowerIncrease, 0.0, 10.0, 30.0)).is_err());
        assert!(validate_proc("x", &cfg(AuraType::AttackPowerIncrease, 20.0, 0.0, 30.0)).is_err());
    }

    // ---- the roll ----

    /// A proc set up to fire on the very next eligible event.
    fn certain(trigger: ProcTrigger, item: ItemId) -> ProcSlot {
        ProcSlot {
            item,
            name: format!("{:?}", item),
            config: ProcConfig {
                trigger,
                chance: 1.0,
                effect: AuraType::AttackPowerIncrease,
                magnitude: 30.0,
                duration: 10.0,
                internal_cooldown: 40.0,
            },
            remaining_icd: 0.0,
        }
    }

    #[test]
    fn an_empty_slot_list_draws_no_rng() {
        // The split-control claim, at unit scale: a wearer with no proc
        // trinket must leave the shared RNG stream untouched.
        let before = GameRng::from_seed(7).random_f32();
        let mut rng = GameRng::from_seed(7);
        let granted = roll_procs(&mut [], &[ProcTrigger::MeleeHit], &mut rng);
        assert!(granted.is_empty());
        assert_eq!(
            rng.random_f32(),
            before,
            "roll_procs consumed a draw for a wearer with no proc trinket"
        );
    }

    #[test]
    fn a_ready_proc_fires_and_goes_on_its_own_cooldown() {
        let mut rng = GameRng::from_seed(1);
        let mut slots = vec![certain(ProcTrigger::MeleeHit, ItemId::MarkOfTheChampion)];
        let granted = roll_procs(&mut slots, &[ProcTrigger::MeleeHit], &mut rng);
        assert_eq!(granted.len(), 1);
        assert_eq!(granted[0].effect_type, AuraType::AttackPowerIncrease);
        assert_eq!(slots[0].remaining_icd, 40.0);
    }

    #[test]
    fn a_proc_on_cooldown_does_not_fire_and_the_cooldown_ticks_to_ready() {
        let mut rng = GameRng::from_seed(1);
        let mut slots = vec![certain(ProcTrigger::MeleeHit, ItemId::MarkOfTheChampion)];
        roll_procs(&mut slots, &[ProcTrigger::MeleeHit], &mut rng);
        assert!(roll_procs(&mut slots, &[ProcTrigger::MeleeHit], &mut rng).is_empty());

        tick_proc_cooldowns(&mut slots, 39.9);
        assert!(roll_procs(&mut slots, &[ProcTrigger::MeleeHit], &mut rng).is_empty());
        tick_proc_cooldowns(&mut slots, 0.2);
        assert_eq!(slots[0].remaining_icd, 0.0, "cooldown clamps at zero");
        assert_eq!(
            roll_procs(&mut slots, &[ProcTrigger::MeleeHit], &mut rng).len(),
            1
        );
    }

    #[test]
    fn a_non_matching_trigger_neither_fires_nor_draws() {
        let expected = GameRng::from_seed(3).random_f32();
        let mut rng = GameRng::from_seed(3);
        let mut slots = vec![certain(ProcTrigger::Heal, ItemId::MarkOfTheChampion)];
        assert!(roll_procs(&mut slots, &[ProcTrigger::MeleeHit], &mut rng).is_empty());
        assert_eq!(
            rng.random_f32(),
            expected,
            "a slot whose trigger did not fire still consumed a draw"
        );
    }

    #[test]
    fn two_different_trinkets_proc_independently_with_no_global_lock() {
        // The user's stacking rule: per-trinket ICD, and two DIFFERENT trinkets
        // may be active at once.
        let mut rng = GameRng::from_seed(5);
        let mut slots = vec![
            certain(ProcTrigger::MeleeHit, ItemId::MarkOfTheChampion),
            certain(ProcTrigger::MeleeHit, ItemId::EssenceOfEternalLife),
        ];
        let granted = roll_procs(&mut slots, &[ProcTrigger::MeleeHit], &mut rng);
        assert_eq!(granted.len(), 2, "one proc suppressed the other");
        assert!(slots.iter().all(|s| s.remaining_icd == 40.0));
    }

    #[test]
    fn a_heal_fires_both_the_heal_and_the_spell_cast_trigger() {
        let mut rng = GameRng::from_seed(9);
        let mut slots = vec![
            certain(ProcTrigger::SpellCast, ItemId::MarkOfTheChampion),
            certain(ProcTrigger::Heal, ItemId::EssenceOfEternalLife),
        ];
        let granted = roll_procs(
            &mut slots,
            &[ProcTrigger::SpellCast, ProcTrigger::Heal],
            &mut rng,
        );
        assert_eq!(granted.len(), 2);
    }

    #[test]
    fn a_proc_buff_is_source_keyed_so_it_coexists_with_a_same_stat_buff() {
        // Without this, a Warrior's Battle Shout (AttackPowerIncrease) would
        // swallow an attack-power proc for the whole match — the buff dedup in
        // `apply_pending_auras` keys on effect_type unless told otherwise.
        let aura = cfg(AuraType::AttackPowerIncrease, 55.0, 10.0, 45.0).aura("Dragonspine Trophy");
        assert!(aura.distinct_by_source);
        assert_eq!(aura.ability_name, "Dragonspine Trophy");
        assert_eq!(
            aura.break_on_damage_threshold, -1.0,
            "a proc buff is not broken by damage"
        );
    }

    // ---- the trigger enum ----

    #[test]
    fn all_lists_every_trigger() {
        // Generated from the enum's own tokens, so this pins the CONTENTS
        // rather than the count: a new trigger must be named here.
        assert_eq!(
            ProcTrigger::all(),
            &[
                ProcTrigger::MeleeHit,
                ProcTrigger::SpellCast,
                ProcTrigger::Heal
            ]
        );
    }

    #[test]
    fn as_str_is_the_serde_form() {
        for trigger in ProcTrigger::all() {
            let serialised = ron::to_string(trigger).expect("trigger serialises");
            assert_eq!(serialised, trigger.as_str());
        }
    }
}
