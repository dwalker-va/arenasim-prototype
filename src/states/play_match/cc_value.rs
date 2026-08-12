//! CC value model — the `T_eff` predictor.
//!
//! See `design-docs/cc-value-model.md`. Pure functions only: no ECS, no RNG, no
//! clock, so this is unit-testable and callable from both the step-0 probe
//! harness and the class AI.
//!
//! ## Mechanics this encodes (verified, not assumed)
//!
//! - **DR is already baked into the applied duration.** `apply_pending_auras`
//!   does `aura_to_add.duration *= multiplier` (`auras.rs:167` and `:668`), so
//!   the duration an aura is applied with already equals `nominal × DR`. The
//!   predictor must NOT scale by DR again.
//! - **Absorbed damage does not break CC.** `DamageTakenThisFrame.amount` is
//!   set from `actual_damage`, the post-absorb figure returned by
//!   `apply_damage_with_absorb`, and the break accumulator only ever adds that
//!   (`auras.rs:761`). So an absorb shield *protects* crowd control.
//!
//!   Consequence worth stating where someone will find it: **purging a shield
//!   lowers the `T_eff` of your own CC on that target.** Purge and CC on the
//!   same unit are anti-synergistic, and "strip their defensives, then CC" is
//!   backwards.
//!
//! ## Why the break term is attacker-shaped, not history-shaped
//!
//! v1 predicted breaks from the target's own trailing damage rate. Step 0
//! measured that at **58% precision / 72% recall**, and showed why: the trailing
//! rate barely separates the two outcomes.
//!
//! | at cast | realized during the CC |
//! |---|---|
//! | predicted break, broke (n=21): 25.6 dmg/s | **93.3 dmg/s** |
//! | predicted break, did not (n=15): 18.8 dmg/s | **3.0 dmg/s** |
//!
//! Nearly identical inputs, a 30× difference in outcome. What actually decides
//! it is **who is attacking, and whether this CC takes the target away from
//! them** — a feared target runs out of melee, a stunned one does not:
//!
//! | CC kind | at cast | realized |
//! |---|---|---|
//! | displacing (Fear/horror), n=50 | 25.0 dmg/s | 17.0 dmg/s |
//! | stationary (Stun/Poly), n=64 | 9.0 dmg/s | 36.6 dmg/s |
//!
//! So the caller supplies incoming damage **split by delivery mode**, and the
//! model discounts the melee share when the CC displaces its target.

use crate::states::play_match::components::AuraType;

/// Fraction of melee damage still expected to land on a target that the CC
/// sends fleeing. Derived from step 0: the displacing slice realized 17.0 dmg/s
/// against 25.0 trailing (68% overall), and attributing the whole shortfall to
/// the melee share puts retention near a third.
///
/// A first estimate from 150 applications, not a tuned constant — it is a
/// candidate for re-measurement once the break classifier is re-scored.
pub const MELEE_RETENTION_WHEN_DISPLACED: f32 = 0.35;

/// The expected dispel, given who can actually remove the aura and whether the
/// crowd control carries its victim out of their reach.
#[derive(Debug, Clone, Copy)]
pub struct DispelExpectation {
    /// Probability the aura is removed before it would otherwise end.
    pub probability: f32,
    /// Expected seconds it survives, GIVEN it is removed.
    pub latency: f32,
}

/// Calibrated on 286 dispellable applications across 224 matches covering a
/// Mage beside every partner against four opponent shapes.
///
/// This replaces a flat 18% applied whenever any "healer" was free, which was
/// wrong twice over:
///
/// - **Wrong population.** It counted the Shaman, whose Purge is OFFENSIVE
///   (`try_purge_enemy` skips its own team), so a Shaman cannot take anything
///   off an ally. Only the Priest (Dispel Magic), the Paladin (Cleanse) and the
///   Warlock's Felhunter (Devour Magic) can. Counting correctly sharpens the
///   split from 46%-vs-24% to **61%-vs-11%**.
/// - **Wrong magnitude.** With one real dispeller free, 61% of dispellable CC
///   was removed — more than three times the shipped figure.
///
/// Displacement is the other axis, and it is mechanical rather than fitted: a
/// FEARED ally runs away from the teammate who would cleanse them, so a single
/// dispeller catches only 21% of them (n=14) against 68% of stationary CC
/// (n=87). A second dispeller covers the gap (57%, n=14).
pub fn dispel_expectation(free_dispellers: u32, displaces_target: bool) -> DispelExpectation {
    match (free_dispellers, displaces_target) {
        // Nobody able. The residual is Divine Shield clearing its owner's own
        // crowd control, plus dispellers that free up mid-window.
        (0, _) => DispelExpectation { probability: 0.11, latency: 1.18 }, // n=171
        // Stationary and answerable: the common, and most punishing, case.
        (_, false) => DispelExpectation { probability: 0.65, latency: 2.32 }, // n=87
        // Feared away from their only dispeller.
        (1, true) => DispelExpectation { probability: 0.21, latency: 3.94 }, // n=14
        // Feared, but they have cover.
        (_, true) => DispelExpectation { probability: 0.57, latency: 0.64 }, // n=14
    }
}

// A pet-dispel term was built here and REMOVED after measurement. Devour Magic
// is real and it is the mechanism behind threshold-0 CC vanishing — step 0 found
// 9 of 12 Polymorphs landed on a Warlock were eaten by its Felhunter, against
// 0 of 6 removed by a free healer. But pricing it did not pay for itself:
//
//   - predictor accuracy: 1.13s -> 1.12s absolute error (+1% skill). The whole
//     gain in this area came from gating the dispel term on DISPELLABILITY, not
//     from adding a second dispeller.
//   - live decisions: it made the Warlock mirror cell worse, -4pt -> -7pt
//     (n=300, not significant on its own, but the wrong direction on both
//     assignments).
//
// The calibration was measured on POLYMORPH and the live term applied it to
// FEAR, which is an unsupported extrapolation — and the mirror cell is exactly
// where that extrapolation bites. Re-derive it from Fear-specific data if the
// Mage is ever admitted to the priced model, since Polymorph-into-Felhunter is
// the case it was built for.

/// Mean damage carried by a single landed attack.
///
/// Used as the event size that converts a damage *rate* into an arrival *rate*
/// for break-on-any-damage crowd control: `lambda = rate / TYPICAL_DAMAGE_EVENT`
/// attacks per second. See the threshold-0 branch of `predict_t_eff`.
///
/// Recovered from step 0 as (trailing rate x observed time-to-first-damage) over
/// the 10 threshold-0 applications that took any damage: 19.4 with one attacker
/// (n=4), 11.1 with two or more (n=6), pooling to 14.4.
///
/// **Honest status: neither this constant nor the arrival form it feeds improves
/// threshold-0 accuracy**, and the value model still loses to a constant
/// predictor on that slice (-39% skill). It is kept because the alternative —
/// erosion of a zero budget — predicts exactly 0.0s for any nonzero trailing
/// damage, which is a degeneracy rather than a calibration error. Note this
/// is NO LONGER live-inert: the Mage's priced Polymorph is enabled, so this
/// constant now shapes real decisions and is worth re-measuring.
pub const TYPICAL_DAMAGE_EVENT: f32 = 14.0;

/// Damage-equivalent value of denying one unit one second of being able to act.
///
/// Used to price an incoming CC cast: a Fear about to land on us is worth
/// interrupting even though it deals no damage, and without this the model
/// values it at exactly zero. Calibrated from step 0's realized single-attacker
/// median (melee 14.7 dmg/s) — a unit that cannot act is a unit not dealing
/// roughly that much.
///
/// A first estimate, and deliberately on the low side: over-valuing incoming CC
/// would have interrupts chase every utility cast.
pub const CC_DENIAL_PER_SECOND: f32 = 15.0;

/// Expected seconds to a dispel, given one happens. Step 0 median (mean 1.95s,
/// range 0.02-6.07s); the median is used because the distribution has a long
/// right tail that a mean would let drag predictions upward.
pub const DISPEL_LATENCY_SECS: f32 = 1.40;

/// Which bound determined a `T_eff` prediction, before any dispel adjustment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TEffCap {
    /// The aura's own applied duration was the binding limit.
    Duration,
    /// Incoming damage is expected to exhaust the break budget first.
    BreakBudget,
}

/// Incoming damage on the CC target, split by how it is delivered — because a
/// CC that displaces its target denies melee and does nothing about ranged.
#[derive(Debug, Clone, Copy, Default)]
pub struct IncomingDamage {
    /// Damage per second from attackers who must stay in melee reach.
    pub melee_rate: f32,
    /// Damage per second from attackers who do not — casters, shots, and DoTs
    /// already ticking on the target.
    pub ranged_rate: f32,
}

impl IncomingDamage {
    /// Rate expected to keep arriving once this CC is applied.
    pub fn effective_rate(&self, displaces_target: bool) -> f32 {
        let melee = if displaces_target {
            self.melee_rate * MELEE_RETENTION_WHEN_DISPLACED
        } else {
            self.melee_rate
        };
        (melee + self.ranged_rate).max(0.0)
    }
}

/// How many living enemies are currently pointed at the CC target, by delivery
/// mode. This is the *forward-looking* half of the damage estimate; a trailing
/// rate alone is the backward-looking half and step 0 showed it does not
/// discriminate on its own.
#[derive(Debug, Clone, Copy, Default)]
pub struct AttackerMix {
    pub melee: u32,
    pub ranged: u32,
}

impl AttackerMix {
    pub fn total(&self) -> u32 {
        self.melee + self.ranged
    }
}

/// Turn a trailing damage rate plus current attacker composition into the damage
/// expected to keep arriving.
///
/// **Nobody pointed at the target means the damage is about to stop.** This is
/// the single strongest signal step 0 found: of 32 CC applications landed on a
/// target with zero attackers on it, **31 did not break**. A trailing rate is a
/// record of the past — it stays high for seconds after the attackers have
/// switched away, and predicting a break from it is where the false alarms came
/// from.
///
/// The residual in that case is DoTs already ticking, which do continue and can
/// break a CC. Predicting zero costs the one such case in thirty-two, and buys
/// the thirty-one. If DoT-driven breaks ever matter more, the honest fix is to
/// pass residual DoT damage explicitly, not to soften this to a fudge factor.
// NOTE: a composition-only estimator (per-class DPS constants, no damage
// history) was implemented and scored here on 2026-08-08, and is deliberately
// NOT kept. It was worse on every measure: precision 60% -> 45%, recall
// 72% -> 34%, relative error 37% -> 46%, and Fear-on-healers went from +0.01s
// to -3.82s. Calibration medians were melee 14.7 dmg/s (n=38) and ranged
// 4.0 dmg/s (n=14); both are far too low to predict the breaks that actually
// happen. The lesson: the trailing rate is a weak CLASSIFIER but a much better
// DURATION estimator than class constants, so callers must supply real damage
// history. See the design doc's *Predictor v2*.

/// Estimate incoming damage from a trailing rate plus attacker composition.
/// The trailing rate supplies the magnitude; the attacker mix supplies the
/// forward-looking split and the zero-attacker cutoff.
pub fn expected_incoming(trailing_rate: f32, mix: AttackerMix) -> IncomingDamage {
    if mix.total() == 0 {
        return IncomingDamage::default();
    }
    let share = trailing_rate.max(0.0) / mix.total() as f32;
    IncomingDamage {
        melee_rate: share * mix.melee as f32,
        ranged_rate: share * mix.ranged as f32,
    }
}

/// Everything `predict_t_eff` needs, as plain values.
#[derive(Debug, Clone, Copy)]
pub struct TEffInputs {
    /// Duration the aura will be applied with. **DR is already included** — see
    /// the module docs. Do not multiply by a DR multiplier here.
    pub applied_duration: f32,
    /// The aura's `break_on_damage_threshold`. `0.0` breaks on any damage
    /// (Polymorph), a positive value is a damage budget (Fear = 100), and a
    /// negative value never breaks (Death Coil's horror = -1.0).
    pub break_threshold: f32,
    /// Damage already counted against the threshold. Normally 0.0 at application.
    pub accumulated_damage: f32,
    /// Gross (pre-absorb) incoming damage, split by delivery mode.
    pub incoming: IncomingDamage,
    /// Does this CC send the target fleeing? Fear and Death Coil's horror do;
    /// stuns and roots hold it in place. Gates the melee discount.
    pub displaces_target: bool,
    /// Remaining absorb pool on the target. Damage is eaten by this before any
    /// of it counts toward the break budget.
    pub absorb_remaining: f32,
    /// How many units on the target's side can actually take this aura off an
    /// ally right now — Priest, Paladin or Felhunter, alive, not themselves
    /// crowd controlled, and NOT the target.
    ///
    /// **`None` means the aura is not dispellable at all** — a stun, a horror —
    /// and is emphatically NOT the same as `Some(0)`, which means "dispellable,
    /// but nobody is free". `Some(0)` still carries an 11% removal expectation
    /// (Divine Shield clearing its owner's own crowd control, and dispellers
    /// that free up mid-window); `None` carries none, because no amount of
    /// dispelling can remove something undispellable. Collapsing the two is the
    /// bug that once gave every stun in the survey a discount it could never
    /// earn.
    ///
    /// **The caller owns the dispellability half of that**, and getting it
    /// wrong is expensive: the step-0 probe checked only for a free dispeller
    /// and never for `AuraType::is_magic_dispellable`, so every stun in the
    /// survey — Cheap Shot, Kidney Shot, Hammer of Justice, none of them
    /// removable by any dispel — carried a discount it could never earn.
    /// Correcting it took the model from **worse than a constant predictor**
    /// (-2% skill) to +42%, and the never-breaks slice from -17% to +82%.
    pub free_dispellers: Option<u32>,
}

/// A `T_eff` prediction and how it was arrived at.
#[derive(Debug, Clone, Copy)]
pub struct CcPrediction {
    /// Expected effective duration in seconds.
    pub t_eff: f32,
    /// Which hard term bound the result before the dispel adjustment.
    pub cap: TEffCap,
    /// Whether the dispel expectation shortened it.
    pub dispel_adjusted: bool,
    /// True when the model expects damage to break this CC before it expires.
    /// This is the **classification** step 0 showed is the weak part, so it is
    /// surfaced separately from the duration for scoring precision and recall.
    pub expects_break: bool,
}

/// Predict the effective duration of a CC application.
///
/// Hard caps first (`applied_duration`, then the break budget), then a
/// probabilistic dispel adjustment. Dispel is blended rather than treated as a
/// cap because only ~18% of CC cast into a free dispeller is actually removed —
/// capping every such prediction at the dispel latency would be a far larger
/// error than ignoring dispels entirely.
pub fn predict_t_eff(i: &TEffInputs) -> CcPrediction {
    let mut t_eff = i.applied_duration.max(0.0);
    let mut cap = TEffCap::Duration;
    let mut expects_break = false;

    // Break budget. A negative threshold never breaks (horror), so it is not a
    // bound at all. A zero threshold breaks on the first point of HEALTH damage,
    // which is still gated behind the absorb pool.
    let rate = i.incoming.effective_rate(i.displaces_target);
    if i.break_threshold > 0.0 && rate > 0.0 {
        // EROSION. A real budget (Fear's 100) genuinely depletes at a rate, so
        // dividing budget by rate is the right shape.
        let remaining_budget = (i.break_threshold - i.accumulated_damage).max(0.0);
        // Damage must chew through the shield before any of it counts.
        let time_to_break = (i.absorb_remaining.max(0.0) + remaining_budget) / rate;
        if time_to_break < t_eff {
            t_eff = time_to_break;
            cap = TEffCap::BreakBudget;
            expects_break = true;
        }
    } else if i.break_threshold == 0.0 && rate > 0.0 {
        // ARRIVAL, not erosion. A zero budget does not deplete — there is
        // nothing to deplete. The aura ends on the FIRST landed attack, so the
        // governing quantity is the *waiting time* for that attack.
        //
        // Treating this as erosion with a tiny budget (which is what flooring
        // the budget at one hit amounted to) is the wrong functional form, and
        // it showed: threshold-0 was the only slice in the step-0 survey where
        // the model scored WORSE than a constant predictor, and its strongest
        // observable is a *probability* (of 22 applications, 4 of 16 with one
        // attacker took damage, against 6 of 6 with two or more) which an
        // erosion formula has nowhere to put.
        //
        // Model damage events as arrivals at `rate / TYPICAL_DAMAGE_EVENT` per
        // second. Then `T_eff` is the expected time to the first arrival capped
        // by the aura's own duration, and for an exponential waiting time that
        // has a closed form:
        //
        //     E[min(X, D)] = (1 - e^(-λD)) / λ,   X ~ Exp(λ)
        //
        // which IS the mixture "P(nothing lands) x duration + P(something
        // lands) x E[when]", without needing the two branches separately. It
        // degrades correctly at both ends: as λ -> 0 it returns the full
        // duration, and as λ grows it returns ~1/λ.
        //
        // Absorb still sits in front: absorbed damage never reaches the break
        // accumulator, so the clock on health damage does not start until the
        // shield is gone.
        let lambda = rate / TYPICAL_DAMAGE_EVENT;
        let shielded_for = (i.absorb_remaining.max(0.0) / rate).min(t_eff);
        let exposed_window = (t_eff - shielded_for).max(0.0);
        let landed = 1.0 - (-lambda * exposed_window).exp();
        let expected_wait = if lambda > 0.0 { landed / lambda } else { exposed_window };
        let arrival = shielded_for + expected_wait.min(exposed_window);
        if arrival < t_eff {
            t_eff = arrival;
            cap = TEffCap::BreakBudget;
        }
        // Unlike erosion, this is a probability rather than a certainty, so the
        // break is only "expected" when it is more likely than not.
        expects_break = landed > 0.5;
    }

    // Dispel: an expectation over "removed early" vs "ran its course", not a cap.
    let mut dispel_adjusted = false;
    if let Some(free) = i.free_dispellers {
        let d = dispel_expectation(free, i.displaces_target);
        let early = d.latency.min(t_eff);
        let blended = (1.0 - d.probability) * t_eff + d.probability * early;
        if blended < t_eff {
            t_eff = blended;
            dispel_adjusted = true;
        }
    }

    CcPrediction { t_eff, cap, dispel_adjusted, expects_break }
}

// ---------------------------------------------------------------------------
// `I` — interrupt value
// ---------------------------------------------------------------------------

/// What a cast currently in flight is worth, in damage-equivalent units.
///
/// Healing and damage are commensurable here on purpose: a heal of 100 denied is
/// worth roughly a hit of 100 landed, which is what lets a CC or an interrupt be
/// compared against the rotation it displaces.
#[derive(Debug, Clone, Copy, Default)]
pub struct CastValue {
    /// Healing the cast would deliver, already discounted for overheal (healing
    /// past a full target buys the enemy nothing, so denying it buys us nothing)
    /// and for arena dampening, which scales all healing down over a long match.
    pub healing_denied: f32,
    /// Damage the cast would land on our team, including damage-over-time it
    /// would apply. A DoT is still damage; valuing only the direct component
    /// makes the model blind to Unstable Affliction and friends.
    pub damage_denied: f32,
    /// Damage-equivalent value of the crowd control the cast would land on us,
    /// via [`CC_DENIAL_PER_SECOND`]. Without this term an incoming Fear or
    /// Polymorph prices at zero and an interrupt will walk straight past it.
    pub control_denied: f32,
}

impl CastValue {
    pub fn total(&self) -> f32 {
        self.healing_denied + self.damage_denied + self.control_denied
    }

    /// Damage-equivalent value of an incoming CC of `duration` seconds landing
    /// on one of our units.
    pub fn control_value(duration: f32) -> f32 {
        duration.max(0.0) * CC_DENIAL_PER_SECOND
    }
}

/// Value of cancelling a cast that is in flight — the `I` term.
///
/// A one-shot payoff, independent of any duration, which is what makes an
/// offensive Fear on a fully-DoTed target correct *at that instant and not
/// otherwise*: the pseudo-interrupt case a blanket "never CC the kill target"
/// filter cannot express.
///
/// **`I` does not survive your own cast time.** Interrupt value attaches to a
/// cast already in flight, so it is claimable only when the interrupting action
/// lands before that cast completes. This is the same law as "`I` does not
/// survive a walk", applied to the caster's own bar rather than to travel:
///
/// - an instant interrupt (Kick, Pummel, Wind Shear, Spell Lock) has
///   `time_to_land ≈ 0` and can catch anything still casting;
/// - the Warlock's Fear is a **1.5s hardcast**, so it can only interrupt a cast
///   with more than 1.5s left — which in practice means it almost never claims
///   `I`, and its value has to come from `D × T_eff` instead.
///
/// Returns 0.0 rather than a small number when the timing fails, because a cast
/// we cannot beat is worth exactly nothing to aim at.
pub fn interrupt_value(cast: CastValue, cast_remaining: f32, time_to_land: f32) -> f32 {
    interrupt_value_with_lockout(cast, cast_remaining, time_to_land, 0.0, 0.0)
}

/// `interrupt_value` including the value of the **school lockout** a real
/// interrupt applies on top of cancelling the cast.
///
/// This term is why "interrupt the healer" was a good heuristic and a
/// magnitude-only model is not. Locking Holy for 3s denies *every* heal in that
/// window, not just the one cancelled; locking Frost on a Mage that also casts
/// Arcane denies much less. Measured: without this term the priced interrupt
/// policy scored -5pt (z=-0.81) against the identity heuristic it replaced,
/// because it happily traded a Flash Heal for a bigger Shadow Bolt.
///
/// The lockout is priced at the interrupted cast's own **value per second** —
/// a cheap, self-calibrating proxy for "how much throughput does this unit push
/// through this school". A 2s Flash Heal worth 100 implies 50/s, so 3s of
/// lockout is worth another 150; a slow one-off nuke implies far less.
pub fn interrupt_value_with_lockout(
    cast: CastValue,
    cast_remaining: f32,
    time_to_land: f32,
    lockout_duration: f32,
    cast_time: f32,
) -> f32 {
    if cast_remaining <= 0.0 || time_to_land >= cast_remaining {
        return 0.0;
    }
    let immediate = cast.total().max(0.0);
    let per_second = if cast_time > 0.0 { immediate / cast_time } else { 0.0 };
    immediate + per_second * lockout_duration.max(0.0)
}

// ---------------------------------------------------------------------------
// The kill-window term: denial is capped by exploitation
// ---------------------------------------------------------------------------

/// Damage-equivalent value of the kill window a CC opens.
///
/// **Denying healing is worth exactly as much as the damage it fails to erase.**
/// A healer locked out while your team lands nothing has denied you nothing —
/// they would have been overhealing. The same lockout while your team is landing
/// 30 dmg/s is worth thirty damage a second.
///
/// This is the term the model was missing, and its absence is measurable rather
/// than theoretical. Step 1's CC change measured **+10pt on BasicArena and +2pt
/// on PillaredArena** with the *same* AI decisions on both maps (Fears on the
/// enemy healer went 4 -> 16 and 6 -> 16 respectively). Neither cover nor
/// displacement explained the gap:
///
/// | | BasicArena | PillaredArena |
/// |---|---|---|
/// | mean distance to the kill target | 38.5yd | **54.3yd** |
/// | match length | 39.9s | **57.3s** |
/// | damage delivered onto the kill target | 14.24/s | **12.52/s** |
///
/// The bigger map keeps a team half again as far from its target for forty
/// percent longer, so the same denial buys less. Pricing `D x T_eff` with no
/// reference to whether anyone can exploit the window makes every CC score
/// map-dependent by construction — which is precisely what the two sweeps found.
///
/// `delivery_rate` is gross damage per second our team is landing on the kill
/// target. `RecentDamage` already measures it: enemies do not damage each other,
/// so a unit's incoming damage *is* our delivery onto it.
pub fn kill_window_value(t_eff: f32, delivery_rate: f32) -> f32 {
    t_eff.max(0.0) * delivery_rate.max(0.0)
}

// ---------------------------------------------------------------------------
// `D` — denial rate
// ---------------------------------------------------------------------------

/// What one enemy is worth removing, per second, in damage-equivalent units.
///
/// This is the term that replaces `class.is_healer()` as the answer to "who is
/// worth CCing". A role constant cannot see that a healer with nobody hurt
/// denies nothing, that an out-of-mana caster denies nothing, or that the melee
/// currently killing our healer denies a great deal — and the last of those is
/// a measured defect, not a hypothetical:
///
/// In an audited match the enemy Rogue hit our **Priest 14 times and our Warlock
/// 3**, and the Warlock's Death Coil — 30s cooldown, 3s horror that never breaks
/// on damage, the best peel in the kit — fired **once**, because
/// `pick_death_coil_peel` gated on `info.target == Some(me)`. The Warlock peeled
/// only for itself while its healer was killed. Nothing in any class AI had a
/// trigger for "my healer is being killed".
///
/// **"Us" means the team, not the caster.** That is the whole content of this
/// term.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenialInputs {
    /// Damage per second this unit is landing on OUR TEAM — any member, not just
    /// the evaluating unit.
    pub damage_to_us: f32,
    /// Healing per second this unit delivers, **already capped by our delivery**
    /// onto their team. Denying healing is worth what it fails to erase, so a
    /// healer whose team is taking nothing denies nothing. See
    /// [`kill_window_value`] for the same idea applied to a window.
    pub healing_capped: f32,
}

/// How much a second of DAMAGE denial is worth against a second of HEALING
/// denial, which the model previously priced at par.
///
/// They are not the same kind of thing:
///
/// - **Denying healing compounds.** During the window our damage lands
///   unhealed, and health not restored cannot be restored retroactively. That is
///   what converts damage into a kill.
/// - **Denying damage is deferred, not erased.** The unit resumes the moment the
///   crowd control ends; all we bought is that our own healer had less to repair
///   — and when we are already ahead on the damage race, that is slack.
///
/// Pricing them at par is what made the priced Mage sheep the enemy Rogue 19
/// times in 20 matches (132.7s of denial) and LOSE by 27 points, in a matchup
/// the identity heuristic wins 90% of by never sheeping at all. The arithmetic
/// was not wrong — ~6s of Rogue denial does exceed one Frostbolt — it was
/// denominated in the wrong currency.
///
/// A first estimate, deliberately not tuned to a single cell. This is exactly
/// the sort of weight that belongs in `cc.ron` once that exists, and it is
/// per-class by nature: a peel-oriented kit should value damage denial higher
/// than a burst one.
pub const DAMAGE_DENIAL_DISCOUNT: f32 = 0.5;

/// Damage-equivalent value per second of taking this unit out of the game.
///
/// Additive because the two components are genuinely separate throughput: a unit
/// that both damages us and heals its team denies both when locked down — but
/// they are weighted differently, see [`DAMAGE_DENIAL_DISCOUNT`].
pub fn denial_rate(i: &DenialInputs) -> f32 {
    (i.damage_to_us.max(0.0) * DAMAGE_DENIAL_DISCOUNT + i.healing_capped.max(0.0)).max(0.0)
}

// ---------------------------------------------------------------------------
// `C` — cost
// ---------------------------------------------------------------------------

/// What an action costs, in the same damage-equivalent currency as its value.
///
/// **Priced in mana, not GCDs**, and that is a measured correction to the
/// original design. The plan assumed the contended resource was the global
/// cooldown, so cost meant "the damage this GCD would otherwise have done".
/// Reading actual matches says otherwise: a Warlock casts roughly **eleven
/// spells in a 50-70 second match**, every combatant finishes on **~10 of 296
/// mana**, and the tail of each match is fought with wands. The GCD is not
/// scarce; mana is.
///
/// Scarcity is expressed as **the fraction of REMAINING mana** an action
/// consumes, which needs no forecast of how long the match will last. A 30-mana
/// Fear costs a tenth of a full pool and half of a 60-mana one — so CC is cheap
/// while resourced and correctly becomes prohibitive when nearly dry, which is
/// exactly when the last casts should go to damage instead.
#[derive(Debug, Clone, Copy, Default)]
pub struct CostInputs {
    /// Mana the action will consume.
    pub mana_cost: f32,
    /// Mana the caster has right now.
    pub current_mana: f32,
    /// Damage-equivalent value of the action this one displaces — the rotation
    /// cast that would have gone out on this global instead.
    pub displaced_value: f32,
}

/// Weight converting "fraction of remaining mana consumed" into
/// damage-equivalent units.
///
/// **Calibrated by sweep, and the result is a negative one worth keeping.**
/// Measured at n=300 (BasicArena, healer called), varying only this constant:
///
/// | value | effect |
/// |---|---|
/// | 0 (gate off) | +10pt, z=2.46 |
/// | 150 | +10pt, z=2.38 |
/// | **300** | **+10pt, z=2.38** |
/// | 600 | +6pt, z=1.39 |
/// | 1000 | +5pt, z=1.31 |
///
/// There is no setting where the gate HELPS: at or below 300 it is
/// indistinguishable from having no gate, and above that it starts declining
/// Fears that were worth casting. So **mana scarcity on its own does not earn a
/// veto.** 300 is kept as the largest measurably-neutral value, which leaves the
/// comparison structure in place for when `displaced_value` is real — pricing
/// the rotation cast a CC displaces is the half of `C` that has not been built,
/// and is the part most likely to make this gate mean something.
///
/// The original anchor (1000, from "a full pool is worth a match's ~1000 damage")
/// was wrong: that figure includes wand chip and DoT ticks, not just
/// mana-derived damage.
pub const MANA_POOL_DAMAGE_EQUIVALENT: f32 = 300.0;

/// Damage-equivalent cost of taking this action now.
pub fn action_cost(i: &CostInputs) -> f32 {
    let mana = i.mana_cost.max(0.0);
    // Guard the dry case: with no mana left the action is not merely expensive,
    // it is unaffordable, and the caller's own resource check will reject it.
    let remaining = i.current_mana.max(1.0);
    let fraction = (mana / remaining).clamp(0.0, 1.0);
    i.displaced_value.max(0.0) + fraction * MANA_POOL_DAMAGE_EQUIVALENT
}

// ---------------------------------------------------------------------------
// `E` — enabling value (depth 1)
// ---------------------------------------------------------------------------

/// Fraction of a teammate's uplifted value credited to the CC that enables it.
///
/// Depth-1 uplift is worth less than one's own denial: the teammate may not
/// follow up, may be interrupted, or may pick a different action. A discount
/// under 1 also guarantees the chain cannot inflate without bound.
pub const ENABLING_DISCOUNT: f32 = 0.6;

/// What a candidate CC is worth *to teammates*, beyond its own denial.
///
/// The chain's FIRST crowd-control is a pure externality: it creates the whole
/// uplift and captures none of it, so a greedy per-unit argmax systematically
/// under-values the action that starts a chain. `E` prices that.
///
/// **Depth 1, hard cut.** `teammate_uplift` must be computed against teammates'
/// *un-uplifted* values. If A's enabling value could read B's already-uplifted
/// value, two units can talk each other into CC on the strength of a follow-up
/// neither will make, and the fixed point may oscillate.
///
/// Callers must discount by deliverability — a teammate that is out of range,
/// on cooldown, out of resources or itself CC'd cannot follow up, and its uplift
/// is worth nothing. That check is the caller's because only it knows the
/// ability in question.
pub fn enabling_value(teammate_uplift: f32) -> f32 {
    teammate_uplift.max(0.0) * ENABLING_DISCOUNT
}

/// Probability a freshly applied DoT is dispelled before it expires, when the
/// target's team has a dispeller. Step-0 measurement: **46% of 9.5 DoT
/// applications per match** were removed.
pub const DOT_DISPEL_PROBABILITY: f32 = 0.46;

/// Mean seconds a DoT survives when it IS dispelled. Step-0 measurement: 2.1s.
/// Curse of Agony applied at 19.25s was gone at 19.28s.
pub const DOT_LIFETIME_WHEN_DISPELLED: f32 = 2.1;

/// Expected damage a damage-over-time application actually delivers.
///
/// **A DoT's value has the same shape as a CC's**: magnitude times expected
/// surviving duration, where survival is threatened by dispel. Pricing it at
/// full duration badly overstates it against a dispeller — measured, 46% of
/// applications are removed after a mean 2.1 seconds.
///
/// This is what a CC displaces when the Warlock Fears instead of re-applying a
/// DoT, and therefore the `displaced_value` half of [`action_cost`] that has
/// been 0 until now. Note how much smaller it is than the Shadow Bolt figure an
/// earlier version of the gate compared against: a Corruption expected to be
/// dispelled half the time is worth a fraction of a nuke, which is precisely why
/// that comparison rejected every Fear.
pub fn dot_expected_damage(
    damage_per_tick: f32,
    tick_interval: f32,
    duration: f32,
    dispeller_present: bool,
) -> f32 {
    if damage_per_tick <= 0.0 || tick_interval <= 0.0 || duration <= 0.0 {
        return 0.0;
    }
    let full = (duration / tick_interval).floor().max(0.0) * damage_per_tick;
    if !dispeller_present {
        return full;
    }
    let cut = (DOT_LIFETIME_WHEN_DISPELLED / tick_interval).floor().max(0.0) * damage_per_tick;
    (1.0 - DOT_DISPEL_PROBABILITY) * full + DOT_DISPEL_PROBABILITY * cut
}

/// Damage **we** give up by crowd-controlling a unit our own team is attacking.
///
/// The third member of a family this model kept meeting and could not express:
/// CC whose value is negative *through one of our own mechanics*.
///
/// - purging a shield shortens the `T_eff` of our own CC on that target;
/// - CCing a dispeller suppresses our own Unstable Affliction's punish;
/// - **CCing with a break-on-any-damage effect stops our whole team damaging
///   that unit**, because `pre_cast_ok`'s `check_friendly_cc` guard skips any
///   target carrying a friendly `break_on_damage_threshold == 0.0` aura.
///
/// That last one is what the Mage's hardcoded guard was really protecting:
///
/// > `cc_target equals kill target — would break on damage`
///
/// The stated reason (it would break) is the lesser harm. The real cost is that
/// sheeping the unit we are killing **removes it from our damage for the
/// duration**. Measured: replacing that guard with `D x T_eff` alone made the
/// Mage sheep 2.5x more often (6 -> 15 over 10 seeds, including 3 on the kill
/// target) and cost **-8pt (z=-1.89)** at n=300.
///
/// Only applies to CC that trips the guard — a Fear (budget 100) or a stun does
/// not suppress friendly damage, so `suppresses_our_damage` is false for them.
pub fn forgone_damage(delivery_rate: f32, t_eff: f32, suppresses_our_damage: bool) -> f32 {
    if !suppresses_our_damage {
        return 0.0;
    }
    delivery_rate.max(0.0) * t_eff.max(0.0)
}

/// Whether an aura type denies its target the ability to *act*.
///
/// Delegates to the simulation's own `is_incapacitating` so the accounting can
/// never drift from the definition the combat code enforces: Stun, Fear,
/// Polymorph and Incapacitate. Root is excluded — it denies movement only.
pub fn denies_actions(effect: AuraType) -> bool {
    crate::states::play_match::utils::is_incapacitating(&effect)
}

/// Whether an aura type makes its target flee, taking it away from melee.
///
/// `AuraType::Fear` covers both Fear proper and Death Coil's horror (which
/// reuses the type for its flee locomotion and diminishes on its own bucket).
pub fn displaces_target(effect: AuraType) -> bool {
    matches!(effect, AuraType::Fear)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> TEffInputs {
        TEffInputs {
            applied_duration: 8.0,
            break_threshold: 100.0,
            accumulated_damage: 0.0,
            incoming: IncomingDamage::default(),
            displaces_target: false,
            absorb_remaining: 0.0,
            free_dispellers: None,
        }
    }

    fn ranged(rate: f32) -> IncomingDamage {
        IncomingDamage { melee_rate: 0.0, ranged_rate: rate }
    }

    fn melee(rate: f32) -> IncomingDamage {
        IncomingDamage { melee_rate: rate, ranged_rate: 0.0 }
    }

    #[test]
    fn undisturbed_cc_runs_its_applied_duration() {
        let p = predict_t_eff(&base());
        assert_eq!(p.t_eff, 8.0);
        assert_eq!(p.cap, TEffCap::Duration);
        assert!(!p.expects_break);
    }

    #[test]
    fn applied_duration_is_taken_as_is_because_dr_is_already_baked_in() {
        // A 50%-DR fear arrives as a 4s aura; the predictor must not halve it again.
        let p = predict_t_eff(&TEffInputs { applied_duration: 4.0, ..base() });
        assert_eq!(p.t_eff, 4.0);
    }

    #[test]
    fn incoming_damage_binds_via_the_break_budget() {
        let p = predict_t_eff(&TEffInputs { incoming: ranged(40.0), ..base() });
        assert_eq!(p.t_eff, 2.5);
        assert_eq!(p.cap, TEffCap::BreakBudget);
        assert!(p.expects_break);
    }

    #[test]
    fn displacing_cc_discounts_melee_but_not_ranged() {
        // The step-0 finding: a feared target runs out of melee reach.
        let stationary = predict_t_eff(&TEffInputs {
            incoming: melee(40.0),
            displaces_target: false,
            ..base()
        });
        let displacing = predict_t_eff(&TEffInputs {
            incoming: melee(40.0),
            displaces_target: true,
            ..base()
        });
        assert!(
            displacing.t_eff > stationary.t_eff,
            "fearing a melee's target should extend the CC, got {} vs {}",
            displacing.t_eff,
            stationary.t_eff
        );

        // Ranged damage is unaffected by displacement.
        let a = predict_t_eff(&TEffInputs { incoming: ranged(40.0), displaces_target: false, ..base() });
        let b = predict_t_eff(&TEffInputs { incoming: ranged(40.0), displaces_target: true, ..base() });
        assert_eq!(a.t_eff, b.t_eff);
    }

    #[test]
    fn an_absorb_shield_protects_the_cc() {
        // A 70-point shield eats the first 1.75s of 40 dps, so the break arrives
        // at (70 + 100) / 40 = 4.25s instead of 2.5s.
        let p = predict_t_eff(&TEffInputs {
            incoming: ranged(40.0),
            absorb_remaining: 70.0,
            ..base()
        });
        assert_eq!(p.t_eff, 4.25);

        // A large enough shield removes the break as a bound entirely.
        let p = predict_t_eff(&TEffInputs {
            incoming: ranged(40.0),
            absorb_remaining: 500.0,
            ..base()
        });
        assert_eq!(p.cap, TEffCap::Duration);
        assert!(!p.expects_break);
    }

    #[test]
    fn a_never_breaking_horror_ignores_incoming_damage() {
        // Death Coil: 3s, break_on_damage -1.0. Full duration under heavy fire.
        let p = predict_t_eff(&TEffInputs {
            applied_duration: 3.0,
            break_threshold: -1.0,
            incoming: ranged(200.0),
            ..base()
        });
        assert_eq!(p.t_eff, 3.0);
        assert_eq!(p.cap, TEffCap::Duration);
        assert!(!p.expects_break);
    }

    #[test]
    fn polymorph_breaks_on_the_first_damage_event_not_instantly() {
        // A zero break budget does NOT mean zero duration: the aura survives
        // until an attack actually lands, and attacks are discrete. At 40 dmg/s
        // the next ~14-damage event is 0.35s away.
        //
        // The old model answered 0.0s here — `budget / rate` with a budget of
        // zero — which made every threshold-0 CC look worthless to price. Step 0
        // measured these lasting 2.21s on average, not 0.
        let p = predict_t_eff(&TEffInputs {
            break_threshold: 0.0,
            incoming: ranged(40.0),
            ..base()
        });
        assert!((p.t_eff - TYPICAL_DAMAGE_EVENT / 40.0).abs() < 1e-6, "got {}", p.t_eff);
        assert!(p.t_eff > 0.0, "a break needs a landed attack, so it cannot be instant");
        assert!(p.expects_break);
    }

    #[test]
    fn the_three_break_regimes_are_shaped_differently() {
        // The point of the split. Same trickle of incoming damage, three
        // thresholds, three qualitatively different answers:
        //
        //   stun   (-1) — damage is irrelevant, full duration
        //   poly    (0) — ends on the FIRST landed attack: an arrival process
        //   fear  (100) — a budget that erodes: duration scales with the budget
        let trickle = ranged(4.0);
        let stun = predict_t_eff(&TEffInputs {
            break_threshold: -1.0,
            incoming: trickle,
            ..base()
        });
        let poly = predict_t_eff(&TEffInputs {
            break_threshold: 0.0,
            incoming: trickle,
            ..base()
        });
        let fear = predict_t_eff(&TEffInputs {
            break_threshold: 100.0,
            incoming: trickle,
            ..base()
        });
        assert_eq!(stun.t_eff, 8.0, "a stun ignores damage entirely");
        assert!(poly.t_eff < fear.t_eff, "poly {} fear {}", poly.t_eff, fear.t_eff);
        assert!(poly.t_eff > 0.0, "one trickle must not zero it out");
    }

    #[test]
    fn a_trickle_does_not_collapse_a_break_on_any_cc() {
        // Where the arrival form differs from erosion, and why it is the right
        // shape. At 2 dmg/s a ~14-damage attack lands roughly every 7s, so a
        // Polymorph should expect to survive a meaningful part of its 8s — NOT
        // to be treated as though a budget of zero were being eaten instantly.
        let p = predict_t_eff(&TEffInputs {
            break_threshold: 0.0,
            incoming: ranged(2.0),
            ..base()
        });
        assert!(p.t_eff > 3.0, "a slow trickle should leave real duration, got {}", p.t_eff);
        assert!(p.t_eff < 8.0, "but not the full duration, got {}", p.t_eff);
        // Erosion with a one-hit floor would have said 14/2 = 7.0s flat; the
        // arrival form discounts it for the chance a hit lands sooner.
        assert!(p.t_eff < 7.0, "must discount below the naive inter-arrival time");
    }

    #[test]
    fn the_event_floor_never_binds_on_a_real_damage_budget() {
        // Fear's 100-damage budget is far above one attack, so the floor added
        // for threshold-0 must leave every positive-budget prediction untouched.
        let p = predict_t_eff(&TEffInputs { incoming: ranged(40.0), ..base() });
        assert_eq!(p.t_eff, 2.5, "100 damage at 40 dmg/s, unchanged by the floor");
    }

    #[test]
    fn polymorph_on_a_shielded_target_survives_until_the_shield_is_gone() {
        // The mechanic that makes purge-then-CC backwards. The shield must be
        // chewed through BEFORE the breaking event can land, so both terms add.
        let p = predict_t_eff(&TEffInputs {
            break_threshold: 0.0,
            incoming: ranged(40.0),
            absorb_remaining: 70.0,
            ..base()
        });
        assert!(
            (p.t_eff - (70.0 + TYPICAL_DAMAGE_EVENT) / 40.0).abs() < 1e-6,
            "got {}",
            p.t_eff
        );
    }

    #[test]
    fn accumulated_damage_shortens_the_remaining_budget() {
        let p = predict_t_eff(&TEffInputs {
            incoming: ranged(40.0),
            accumulated_damage: 60.0,
            ..base()
        });
        assert_eq!(p.t_eff, 1.0);
    }

    #[test]
    fn dispel_exposure_shortens_but_does_not_cap() {
        // A dispel is an expectation, not a cap: even at the measured 65%
        // removal rate for answerable stationary CC, an 8s aura must NOT
        // collapse to the latency — 35% of the time it runs its course.
        let p = predict_t_eff(&TEffInputs { free_dispellers: Some(1), ..base() });
        assert!(p.dispel_adjusted);
        let d = dispel_expectation(1, false);
        let expected = (1.0 - d.probability) * 8.0 + d.probability * d.latency;
        assert!((p.t_eff - expected).abs() < 1e-4, "got {} want {}", p.t_eff, expected);
        assert!(
            p.t_eff > d.latency && p.t_eff < 8.0,
            "expected a partial shortening, got {}",
            p.t_eff
        );
    }

    #[test]
    fn undispellable_is_not_the_same_as_unanswered() {
        // `None` (a stun) must take NO dispel discount at all, while `Some(0)`
        // (dispellable, nobody free) still carries the measured 11% residual.
        // Collapsing these gave every stun a discount it could never earn.
        let stun = predict_t_eff(&TEffInputs { free_dispellers: None, ..base() });
        let unanswered = predict_t_eff(&TEffInputs { free_dispellers: Some(0), ..base() });
        assert!(!stun.dispel_adjusted, "an undispellable aura cannot be dispelled");
        assert_eq!(stun.t_eff, 8.0);
        assert!(unanswered.t_eff < stun.t_eff, "Some(0) still carries a residual");
    }

    #[test]
    fn fleeing_the_dispeller_is_worth_something() {
        // Mechanical, not fitted: a FEARED ally runs away from the teammate who
        // would cleanse them, so one dispeller catches far fewer of them (21%)
        // than of a stationary target (68%).
        let displacing = dispel_expectation(1, true);
        let stationary = dispel_expectation(1, false);
        assert!(
            displacing.probability < stationary.probability,
            "displacing {} stationary {}",
            displacing.probability,
            stationary.probability
        );
        // ...but a second dispeller covers the gap.
        assert!(dispel_expectation(2, true).probability > displacing.probability);
    }

    #[test]
    fn dispel_adjustment_never_lengthens_a_short_prediction() {
        // A CC already predicted to break in under the dispel latency must not
        // be pushed upward by the blend.
        let short = predict_t_eff(&TEffInputs { incoming: ranged(200.0), ..base() });
        let with_dispel =
            predict_t_eff(&TEffInputs { incoming: ranged(200.0), free_dispellers: Some(1), ..base() });
        assert!(with_dispel.t_eff <= short.t_eff + 1e-6);
    }

    #[test]
    fn a_kill_window_is_worth_what_the_team_can_deliver_into_it() {
        // Same CC duration, different team damage: the window is worth what you
        // can put through it.
        assert_eq!(kill_window_value(8.0, 30.0), 240.0);
        assert_eq!(kill_window_value(8.0, 12.0), 96.0);
    }

    #[test]
    fn denying_a_healer_while_delivering_nothing_is_worth_nothing() {
        // The whole point of the term: a locked-out healer whose team is taking
        // no damage has denied us nothing, however long the lock lasts.
        assert_eq!(kill_window_value(8.0, 0.0), 0.0);
        // ...and a CC that lands for no time is worth nothing however hard we hit.
        assert_eq!(kill_window_value(0.0, 100.0), 0.0);
    }

    #[test]
    fn kill_window_value_is_monotonic_in_both_inputs() {
        assert!(kill_window_value(4.0, 20.0) > kill_window_value(2.0, 20.0));
        assert!(kill_window_value(4.0, 20.0) > kill_window_value(4.0, 10.0));
        // Negative inputs cannot produce negative value.
        assert_eq!(kill_window_value(-1.0, 20.0), 0.0);
        assert_eq!(kill_window_value(4.0, -5.0), 0.0);
    }

    #[test]
    fn denial_counts_damage_aimed_at_any_teammate_not_just_the_caster() {
        // The measured defect: a Rogue killing our healer must price high even
        // though it is not touching the evaluating unit at all.
        let killing_our_healer = DenialInputs { damage_to_us: 25.0, ..Default::default() };
        assert_eq!(denial_rate(&killing_our_healer), 25.0 * DAMAGE_DENIAL_DISCOUNT);
        assert!(denial_rate(&killing_our_healer) > 0.0, "it must still register");
    }

    #[test]
    fn a_healer_with_nobody_to_save_denies_nothing() {
        // Healing capped by our delivery: if we are landing nothing, a locked
        // healer costs the enemy nothing. This is what a role constant cannot see.
        let idle_healer = DenialInputs { damage_to_us: 0.0, healing_capped: 0.0 };
        assert_eq!(denial_rate(&idle_healer), 0.0);
    }

    #[test]
    fn damage_and_healing_denial_add_but_are_not_priced_at_par() {
        let hybrid = DenialInputs { damage_to_us: 10.0, healing_capped: 15.0 };
        assert_eq!(denial_rate(&hybrid), 10.0 * DAMAGE_DENIAL_DISCOUNT + 15.0);
    }

    #[test]
    fn denying_healing_outranks_denying_the_same_damage() {
        // The asymmetry the model now encodes: healing denied is ERASED — our
        // damage lands unhealed and converts into a kill — while damage denied
        // is merely DEFERRED until the crowd control ends.
        //
        // Pricing these at par is what made the priced Mage sheep an enemy Rogue
        // 19 times in 20 matches and lose by 27 points, in a matchup the plain
        // heuristic wins 90% of by never sheeping at all.
        let denies_damage = DenialInputs { damage_to_us: 20.0, healing_capped: 0.0 };
        let denies_healing = DenialInputs { damage_to_us: 0.0, healing_capped: 20.0 };
        assert!(
            denial_rate(&denies_healing) > denial_rate(&denies_damage),
            "healing {} should outrank damage {}",
            denial_rate(&denies_healing),
            denial_rate(&denies_damage)
        );
    }

    #[test]
    fn denial_is_never_negative() {
        let odd = DenialInputs { damage_to_us: -5.0, healing_capped: -5.0 };
        assert_eq!(denial_rate(&odd), 0.0);
    }

    #[test]
    fn the_same_cc_costs_more_when_nearly_dry() {
        let full = action_cost(&CostInputs { mana_cost: 30.0, current_mana: 296.0, displaced_value: 0.0 });
        let low = action_cost(&CostInputs { mana_cost: 30.0, current_mana: 60.0, displaced_value: 0.0 });
        assert!(low > full * 4.0, "expected scarcity to dominate, got {full} vs {low}");
    }

    #[test]
    fn cost_includes_the_action_it_displaces() {
        let bare = action_cost(&CostInputs { mana_cost: 30.0, current_mana: 296.0, displaced_value: 0.0 });
        let with_alt = action_cost(&CostInputs { mana_cost: 30.0, current_mana: 296.0, displaced_value: 120.0 });
        assert!((with_alt - bare - 120.0).abs() < 1e-3);
    }

    #[test]
    fn a_free_action_still_costs_what_it_displaces() {
        let c = action_cost(&CostInputs { mana_cost: 0.0, current_mana: 296.0, displaced_value: 90.0 });
        assert_eq!(c, 90.0);
    }

    #[test]
    fn cost_is_finite_when_out_of_mana() {
        let c = action_cost(&CostInputs { mana_cost: 30.0, current_mana: 0.0, displaced_value: 0.0 });
        assert!(c.is_finite() && c > 0.0);
    }

    #[test]
    fn enabling_pays_the_chain_opener_a_discounted_share() {
        // The first CC creates the uplift and captures none of it without this.
        let v = enabling_value(100.0);
        assert!(v > 0.0 && v < 100.0, "expected a discounted share, got {v}");
        assert!((v - 60.0).abs() < 1e-3);
    }

    #[test]
    fn enabling_is_never_negative_and_scales_with_the_uplift() {
        assert_eq!(enabling_value(0.0), 0.0);
        assert_eq!(enabling_value(-50.0), 0.0);
        assert!(enabling_value(200.0) > enabling_value(100.0));
    }

    #[test]
    fn the_discount_is_strictly_below_one_so_chains_cannot_inflate() {
        // A depth-1 cut plus a sub-unit discount is what bounds the recursion.
        assert!(ENABLING_DISCOUNT < 1.0);
        assert!(enabling_value(100.0) < 100.0);
    }

    #[test]
    fn a_dot_is_worth_its_full_run_with_no_dispeller() {
        // Corruption: 10 per 3s tick over 18s = 6 ticks.
        assert_eq!(dot_expected_damage(10.0, 3.0, 18.0, false), 60.0);
    }

    #[test]
    fn a_dispeller_cuts_a_dots_expected_value_sharply() {
        let safe = dot_expected_damage(10.0, 3.0, 18.0, false);
        let contested = dot_expected_damage(10.0, 3.0, 18.0, true);
        assert!(contested < safe * 0.7, "expected a sharp cut, got {contested} vs {safe}");
        assert!(contested > 0.0);
    }

    #[test]
    fn a_dots_expected_value_is_far_below_a_nuke() {
        // The reason gating CC against a Shadow Bolt rejected every Fear: the
        // action a Fear actually displaces is a DoT worth a fraction of one.
        let dot = dot_expected_damage(10.0, 3.0, 18.0, true);
        assert!(dot < 100.0, "a contested Corruption should be worth well under a nuke, got {dot}");
    }

    #[test]
    fn a_degenerate_dot_is_worth_nothing() {
        assert_eq!(dot_expected_damage(0.0, 3.0, 18.0, true), 0.0);
        assert_eq!(dot_expected_damage(10.0, 0.0, 18.0, true), 0.0);
        assert_eq!(dot_expected_damage(10.0, 3.0, 0.0, true), 0.0);
    }

    #[test]
    fn ccing_the_unit_we_are_killing_costs_us_its_damage() {
        // A break-on-any-damage CC stops our team hitting that target.
        assert_eq!(forgone_damage(20.0, 5.0, true), 100.0);
    }

    #[test]
    fn cc_that_does_not_trip_the_friendly_guard_forgoes_nothing() {
        // Fear has a 100-damage budget, so friendly damage continues.
        assert_eq!(forgone_damage(20.0, 5.0, false), 0.0);
    }

    #[test]
    fn forgone_damage_is_zero_when_nobody_is_hitting_the_target() {
        // The case the hardcoded guard got wrong: sheeping a unit our team is
        // NOT attacking costs us nothing, and should be allowed.
        assert_eq!(forgone_damage(0.0, 8.0, true), 0.0);
    }

    #[test]
    fn denies_actions_matches_the_sims_own_definition() {
        for ty in [AuraType::Stun, AuraType::Fear, AuraType::Polymorph, AuraType::Incapacitate] {
            assert!(denies_actions(ty), "{ty:?} should deny actions");
        }
        // Root denies movement only — excluded, per `is_incapacitated`.
        assert!(!denies_actions(AuraType::Root));
        assert!(!denies_actions(AuraType::MovementSpeedSlow));
    }

    #[test]
    fn an_instant_interrupt_can_catch_any_live_cast() {
        let v = CastValue { healing_denied: 120.0, damage_denied: 0.0, ..Default::default() };
        assert_eq!(interrupt_value(v, 0.3, 0.0), 120.0);
        assert_eq!(interrupt_value(v, 2.0, 0.0), 120.0);
    }

    #[test]
    fn a_hardcast_cannot_claim_interrupt_value_it_will_not_beat() {
        // The Warlock's Fear is a 1.5s cast: it cannot interrupt a Frostbolt
        // with 1.0s left, so that Frostbolt is worth exactly zero to aim at.
        let v = CastValue { healing_denied: 0.0, damage_denied: 85.0, ..Default::default() };
        assert_eq!(interrupt_value(v, 1.0, 1.5), 0.0);
        // ...but a 3s Pyroblast-shaped cast it CAN beat.
        assert_eq!(interrupt_value(v, 3.0, 1.5), 85.0);
    }

    #[test]
    fn a_finished_or_zero_value_cast_is_worth_nothing() {
        let v = CastValue { healing_denied: 100.0, damage_denied: 0.0, ..Default::default() };
        assert_eq!(interrupt_value(v, 0.0, 0.0), 0.0);
        assert_eq!(interrupt_value(CastValue::default(), 2.0, 0.0), 0.0);
    }

    #[test]
    fn the_school_lockout_is_what_makes_a_heal_worth_more_than_a_bigger_nuke() {
        // A 1.5s Flash Heal worth 100 vs a 3s nuke worth 130. On magnitude
        // alone the nuke wins. With a 3s lockout priced at each cast's own
        // rate, the FAST REPEATABLE heal wins (100 + 66.7*3 = 300) over the slow
        // one-off (130 + 43.3*3 = 260) — which is the whole point of the term.
        let heal = CastValue { healing_denied: 100.0, ..Default::default() };
        let nuke = CastValue { damage_denied: 130.0, ..Default::default() };
        assert!(interrupt_value(nuke, 1.0, 0.0) > interrupt_value(heal, 1.0, 0.0));
        assert!(
            interrupt_value_with_lockout(heal, 1.0, 0.0, 3.0, 1.5)
                > interrupt_value_with_lockout(nuke, 1.0, 0.0, 3.0, 3.0)
        );
    }

    #[test]
    fn a_zero_lockout_interrupt_is_worth_exactly_the_cast() {
        let v = CastValue { healing_denied: 100.0, ..Default::default() };
        assert_eq!(
            interrupt_value_with_lockout(v, 1.0, 0.0, 0.0, 2.0),
            interrupt_value(v, 1.0, 0.0)
        );
    }

    #[test]
    fn an_incoming_cc_cast_is_worth_denying_even_with_no_damage() {
        // A Fear deals nothing and is still worth an interrupt. Before the
        // control term existed this priced at exactly zero and was walked past.
        let fear = CastValue {
            control_denied: CastValue::control_value(8.0),
            ..Default::default()
        };
        assert!(interrupt_value(fear, 1.5, 0.0) > 0.0);
        // Longer CC is worth more.
        let short = CastValue { control_denied: CastValue::control_value(3.0), ..Default::default() };
        assert!(interrupt_value(fear, 1.5, 0.0) > interrupt_value(short, 1.5, 0.0));
    }

    #[test]
    fn healing_and_damage_denial_are_one_currency() {
        let heal = CastValue { healing_denied: 60.0, damage_denied: 0.0, ..Default::default() };
        let dmg = CastValue { healing_denied: 0.0, damage_denied: 60.0, ..Default::default() };
        assert_eq!(interrupt_value(heal, 2.0, 0.0), interrupt_value(dmg, 2.0, 0.0));
    }

    #[test]
    fn only_fear_displaces() {
        assert!(displaces_target(AuraType::Fear));
        assert!(!displaces_target(AuraType::Stun));
        assert!(!displaces_target(AuraType::Polymorph));
        assert!(!displaces_target(AuraType::Root));
    }
}
