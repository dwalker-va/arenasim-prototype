//! CC lifecycle probe — step 0 of the CC value model.
//!
//! See `design-docs/cc-value-model.md`. This is the read-only harness that must
//! run BEFORE any of the behavioural steps, because the entire value model rests
//! on one assumption: that expected CC duration is *predictable* in this sim. If
//! prediction error is large, expected-value pricing is unsound here and steps
//! 1-6 would be built on sand. Better to learn that from a probe than from a
//! seven-class rewrite.
//!
//! It does two things, neither of which changes behaviour:
//!
//! 1. **Prediction error.** Every CC application is fed to
//!    `cc_value::predict_t_eff` using only state observable at that instant,
//!    then followed to its end. Predicted vs actual duration, attributed to the
//!    binding term and to the reason it actually ended.
//! 2. **CC accounting.** The per-frame denial metrics from the doc's measurement
//!    plan: simultaneous-control seconds, counterplay-free seconds, and CC
//!    seconds lost to friendly damage.
//!
//! Observation is via `run_headless_match_observed`, whose non-perturbation
//! guarantee is load-bearing here and is itself covered by
//! `observed_run_does_not_perturb_outcomes` in `tests/movement_probes.rs`.
//!
//! ## Reading the output
//!
//! The reporting tests are `#[ignore]`d — they print a table rather than
//! asserting a threshold, because step 0's job is to *tell us the numbers*, not
//! to pin them before we know what good looks like.
//!
//! ```bash
//! cargo test --release --test cc_lifecycle_probe -- --ignored --nocapture
//! ```
//!
//! The non-ignored tests assert only structural invariants that must hold
//! regardless of tuning (the tracker sees CC at all; predictions are finite;
//! actual duration never exceeds the applied duration).

use std::collections::BTreeMap;

use arenasim::headless::{run_headless_match_observed, FrameObservation, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::{load_ability_definitions, AbilityDefinitions};
use arenasim::states::play_match::cc_value::{
    denies_actions, displaces_target, expected_incoming, predict_t_eff, AttackerMix,
    IncomingDamage, TEffCap, TEffInputs,
};
use arenasim::states::play_match::components::{AuraType, PetType};
use bevy::prelude::Entity;

/// Fixed timestep the headless runner advances by. Used to convert frame counts
/// to seconds and to recognise "this aura had <= one frame left".
const FRAME_DT: f32 = 1.0 / 60.0;

/// Trailing window over which observed damage is averaged into the
/// `gross_damage_rate` fed to the predictor. The doc proposes a trailing
/// measurement of ACTUAL recent damage rather than a predictive model; this is
/// that, and its length is one of the things step 0 exists to calibrate.
const DAMAGE_WINDOW_SECS: f32 = 2.0;

/// How far back to look for damage when attributing an early ending to a break.
/// Must exceed the deferred-`Commands` lag between a damage site recording
/// `DamageTakenThisFrame` and `auras.rs` acting on it; a few frames is ample,
/// and staying well under a GCD keeps it from swallowing genuine dispels.
const BREAK_ATTRIBUTION_WINDOW_SECS: f32 = 0.15;

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// How a tracked CC aura stopped being active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndReason {
    /// Ran its full applied duration.
    Expired,
    /// Damage exhausted the break budget.
    Broke,
    /// Removed while it still had duration left and the target was undamaged
    /// that frame — a dispel, a purge, or Divine Shield clearing its owner's CC.
    Removed,
    /// Superseded by a fresh application of the same CC on the same target.
    ///
    /// Split out from `Removed` because it is a MEASUREMENT artifact, not an
    /// outcome: the instance did not "end early", the tracker simply stopped
    /// following it. Counting these as short durations biases the observed mean
    /// downward and would calibrate the model against its own bookkeeping.
    Refreshed,
    /// The target died under it.
    TargetDied,
    /// The match ended with it still active. Excluded from error statistics —
    /// its true duration is unknown (right-censored), and counting the truncated
    /// value would bias every mean downward.
    Censored,
}

#[derive(Debug, Clone)]
struct CcRecord {
    ability: String,
    effect: AuraType,
    target_class: CharacterClass,
    target_is_healer: bool,
    applied_at: f32,
    /// Duration the aura carried when first observed. DR is already baked in.
    applied_duration: f32,
    predicted: f32,
    predicted_cap: TEffCap,
    actual: f32,
    end: EndReason,
    /// Cast the target lost the frame this CC landed, if any.
    cancelled_cast: Option<String>,
    /// The aura's break budget. `0.0` = breaks on any damage (so the friendly-CC
    /// guard applies to it), positive = a budget, negative = never breaks.
    break_threshold: f32,
    /// Gross damage that actually arrived on the target between application and
    /// end. Quantifies step 0's correction 1: for a `0.0`-threshold CC the
    /// friendly-CC guard should drive this to ~0, which is precisely what a
    /// trailing-rate predictor fails to anticipate.
    damage_during: f32,
    /// The damage rate the predictor extrapolated from at application. Paired
    /// with `damage_during` it shows the extrapolation error directly.
    predicted_from_rate: f32,
    /// Whether the target's team had a living, non-incapacitated dispeller when
    /// the CC landed. The gating input for a real `expected_dispel_delay`.
    dispeller_available: bool,
    /// Absorb the target's shields GAINED during the CC window. The predictor
    /// only knows the pool present at application, so a shield refreshed mid-CC
    /// is buffer it never accounted for — and since absorbed damage never
    /// advances a break budget, that buffer extends the CC.
    absorb_gained_during: f32,
    /// Absorb pool present at application, for comparison against the above.
    absorb_at_application: f32,
    /// How many living enemies were pointed at this target when the CC landed.
    /// The candidate discriminator for "will damage keep arriving".
    attackers_at_cast: usize,
    melee_at_cast: usize,
    ranged_at_cast: usize,
    /// Seconds from application to the first frame that landed gross damage on
    /// the target, if any did before the CC ended.
    ///
    /// For a `0.0`-threshold CC this **is** the effective duration: the aura
    /// breaks on the first point of health damage, so what matters is the
    /// *arrival time of the next damage event*, not a rate. A rate-based model
    /// asks "how long to accumulate a budget of zero" and necessarily answers
    /// "instantly", which is why threshold-0 is the predictor's worst case.
    time_to_first_damage: Option<f32>,
    /// DoT auras ticking on the target when the CC landed. These are the damage
    /// that keeps arriving regardless of the friendly-CC guard, so they are the
    /// prime suspect for what actually breaks a Polymorph.
    dots_at_cast: usize,
    /// Living, free, ALLY-dispelling units on the target's team, excluding the
    /// target itself. Priest (Dispel Magic), Paladin (Cleanse) and the
    /// Felhunter (Devour Magic) only — the Shaman's Purge is OFFENSIVE
    /// (`try_purge_enemy` skips its own team), so a Shaman cannot remove
    /// anything from an ally, and counting it as a dispeller is a live
    /// mis-specification in `dispel_exposed`.
    free_dispellers: usize,
    /// Target's health fraction when the CC landed. The direct test of
    /// "is the model crowd-controlling something it is about to kill" — a kill
    /// is permanent and crowd control is not, so CC on a dying target trades the
    /// former for the latter.
    target_hp_frac: f32,
    /// Target's mana fraction at application. `enemy_healing_capped` only zeroes
    /// a healer's denial value below 5%, which is well under the cost of a heal
    /// — so a healer that cannot afford to cast still prices as a full CC target.
    target_mana_frac: f32,
    /// Team the CC LANDED on. Lets a report attribute an application to the
    /// side that cast it when both sides own the same crowd control.
    target_team: u8,
    /// Whether a living Felhunter — the only unit in this sim with Devour Magic
    /// — stood on the target's side at application. Reported, not fed to the
    /// model: pricing it was measured and did not pay for itself (see
    /// `cc_value`), but it remains the mechanism behind threshold-0 CC vanishing.
    pet_dispel_available: bool,
}

impl CcRecord {
    fn error(&self) -> f32 {
        self.actual - self.predicted
    }
}

// ---------------------------------------------------------------------------
// Tracker
// ---------------------------------------------------------------------------

/// Identity of one aura instance across frames. Auras live in a `Vec` whose
/// indices shift on removal, so index is not identity; this tuple is. The second
/// element is `"{effect:?}|{ability_name}"` — a `String` rather than the
/// `AuraType` itself because `AuraType` is not `Ord`, and a `BTreeMap` (stable
/// iteration order) is worth more here than avoiding a format call.
type AuraKey = (Entity, String);

/// Total remaining absorb pool across a combatant's shields.
fn absorb_pool(o: &arenasim::headless::ObservedCombatant) -> f32 {
    o.auras
        .iter()
        .filter(|a| a.effect_type == AuraType::Absorb)
        .map(|a| a.magnitude)
        .sum()
}

fn aura_key(entity: Entity, effect: AuraType, ability_name: &str) -> AuraKey {
    (entity, format!("{effect:?}|{ability_name}"))
}

/// Estimate expected incoming damage from the target's trailing rate and the
/// composition of enemies currently pointed at it.
///
/// Per-attacker attribution is not observable, so the trailing total is divided
/// evenly across current attackers and summed by delivery mode. The zero-attacker
/// case is handled by `expected_incoming` itself — see its docs.
fn split_incoming(frame: &FrameObservation, target: Entity, total_rate: f32) -> IncomingDamage {
    let Some(obs) = frame.combatants.get(&target) else {
        return IncomingDamage::default();
    };
    let attackers: Vec<&arenasim::headless::ObservedCombatant> = frame
        .combatants
        .values()
        .filter(|c| c.team != obs.team && c.alive && c.target == Some(target))
        .collect();
    let melee = attackers.iter().filter(|c| c.class.is_melee()).count() as u32;
    expected_incoming(
        total_rate,
        AttackerMix { melee, ranged: attackers.len() as u32 - melee },
    )
}

#[derive(Debug, Clone)]
struct Live {
    applied_at: f32,
    applied_duration: f32,
    predicted: f32,
    predicted_cap: TEffCap,
    last_duration_remaining: f32,
    target_class: CharacterClass,
    target_is_healer: bool,
    cancelled_cast: Option<String>,
    /// Carried on the record so `close` need not re-parse the key.
    ability: String,
    effect: AuraType,
    break_threshold: f32,
    predicted_from_rate: f32,
    dispeller_available: bool,
    absorb_at_application: f32,
    /// Running total of absorb added to this target since the CC landed.
    absorb_gained: f32,
    attackers_at_cast: usize,
    melee_at_cast: usize,
    ranged_at_cast: usize,
    dots_at_cast: usize,
    pet_dispel_available: bool,
    target_team: u8,
    free_dispellers: usize,
    target_hp_frac: f32,
    target_mana_frac: f32,
}

struct CcTracker {
    defs: AbilityDefinitions,
    live: BTreeMap<AuraKey, Live>,
    done: Vec<CcRecord>,
    /// Per-target rolling history of (sim_time, gross damage that frame).
    damage_history: BTreeMap<Entity, Vec<(f32, f32)>>,
    prev: Option<FrameObservation>,

    // Accounting, accumulated per frame.
    denied_seconds: f32,
    overlap_seconds: f32,
    counterplay_free_seconds: f32,
    /// Seconds of cast bar spent on abilities that apply an action-denying aura.
    /// This is the honest, *measurable* half of the doc's "damage forgone to CC"
    /// pairing: it counts the time CC consumed without yet pricing what that
    /// time would have produced. Pricing arrives with step 4.
    cc_cast_seconds: f32,
    /// Same, for everything else that was hard-cast — the denominator that makes
    /// the CC figure meaningful.
    other_cast_seconds: f32,
}

/// Does this ability apply an action-denying aura? Read from `abilities.ron`
/// rather than hardcoded, so a newly added CC cannot silently escape the
/// accounting.
fn is_cc_ability(defs: &AbilityDefinitions, ability: AbilityType) -> bool {
    defs.get(&ability)
        .and_then(|d| d.applies_aura.as_ref())
        .is_some_and(|e| denies_actions(e.aura_type))
}

impl CcTracker {
    /// Gross (pre-absorb) damage a target took between two frames: health lost
    /// plus absorb pool consumed. Health alone would undercount, because an
    /// absorbed hit moves no health — and it is exactly that damage which does
    /// NOT feed the CC break budget.
    fn gross_damage_between(prev: &FrameObservation, cur: &FrameObservation, e: Entity) -> f32 {
        let (Some(p), Some(c)) = (prev.combatants.get(&e), cur.combatants.get(&e)) else {
            return 0.0;
        };
        let health_lost = (p.current_health - c.current_health).max(0.0);
        let absorb_consumed = (absorb_pool(p) - absorb_pool(c)).max(0.0);
        health_lost + absorb_consumed
    }

    /// Mean gross damage per second on `target` over the trailing window.
    fn recent_damage_rate(&self, target: Entity, now: f32) -> f32 {
        let Some(hist) = self.damage_history.get(&target) else {
            return 0.0;
        };
        let cutoff = now - DAMAGE_WINDOW_SECS;
        let total: f32 = hist.iter().filter(|(t, _)| *t >= cutoff).map(|(_, d)| *d).sum();
        // Divide by the window actually covered, so an early-match application
        // is not flattered by a short history.
        let covered = DAMAGE_WINDOW_SECS.min(now.max(FRAME_DT));
        total / covered
    }

    /// Did this target take any gross damage in the last `window` seconds?
    fn took_damage_within(&self, target: Entity, now: f32, window: f32) -> bool {
        self.damage_history
            .get(&target)
            .is_some_and(|h| h.iter().any(|(t, d)| *t >= now - window && *d > 0.0))
    }

    fn observe(&mut self, frame: &FrameObservation) {
        // Damage bookkeeping first, so a CC applied this frame is predicted
        // against history that does not yet include its own frame.
        if let Some(prev) = &self.prev {
            for e in frame.combatants.keys() {
                let dmg = Self::gross_damage_between(prev, frame, *e);
                if dmg > 0.0 {
                    self.damage_history.entry(*e).or_default().push((frame.sim_time, dmg));
                }
            }
        }

        // Absorb REPLENISHMENT during a live CC. The predictor sees only the
        // pool at application, so a shield cast mid-CC is buffer it never
        // counted — and because absorbed damage never advances a break budget,
        // that buffer silently extends the CC beyond the prediction.
        if let Some(prev) = &self.prev {
            for (entity, obs) in &frame.combatants {
                let Some(pobs) = prev.combatants.get(entity) else {
                    continue;
                };
                let gained = (absorb_pool(obs) - absorb_pool(pobs)).max(0.0);
                if gained <= 0.0 {
                    continue;
                }
                for (key, live) in self.live.iter_mut() {
                    if key.0 == *entity {
                        live.absorb_gained += gained;
                    }
                }
            }
        }

        let mut seen: BTreeMap<AuraKey, f32> = BTreeMap::new();

        for (entity, obs) in &frame.combatants {
            for aura in &obs.auras {
                if !denies_actions(aura.effect_type) {
                    continue;
                }
                let key = aura_key(*entity, aura.effect_type, &aura.ability_name);
                seen.insert(key.clone(), aura.duration_remaining);

                // A duration that jumped UP is a re-application, not the same
                // instance: close the old record and open a new one.
                let refreshed = self
                    .live
                    .get(&key)
                    .is_some_and(|l| aura.duration_remaining > l.last_duration_remaining + FRAME_DT);
                if refreshed {
                    self.close(&key, frame, EndReason::Refreshed);
                }

                if let Some(live) = self.live.get_mut(&key) {
                    live.last_duration_remaining = aura.duration_remaining;
                    continue;
                }

                // --- new application: predict from observable state only ---
                let absorb_remaining: f32 = obs
                    .auras
                    .iter()
                    .filter(|a| a.effect_type == AuraType::Absorb)
                    .map(|a| a.magnitude)
                    .sum();

                // A dispel can only shorten an aura a dispel can REMOVE. Stuns
                // (Cheap Shot, Kidney Shot, Hammer of Justice) are not magic, so
                // no dispeller shortens them however free it is.
                //
                // The probe checked only for a dispeller, never for
                // dispellability, so every stun prediction carried a dispel
                // discount it should never have had — silently contaminating the
                // headline error figure for the ~54 stun applications in this
                // survey.
                let dispellable = aura.effect_type.is_magic_dispellable();

                // Could the target's team answer this with a dispel? Living,
                // non-pet healer on that team who is not itself locked down.
                // A living Felhunter on the target's side, free to act. Devour
                // Magic is instant, free and off the global cooldown, and step 0
                // found it — not healer dispels — behind almost every early
                // removal of a threshold-0 CC.
                let pet_dispel_available = dispellable
                    && frame.combatants.iter().any(|(e, c)| {
                        c.team == obs.team
                            && c.alive
                            && e != entity
                            && c.pet_type == Some(PetType::Felhunter)
                            && !c.auras.iter().any(|a| denies_actions(a.effect_type))
                    });

                // Units that can actually take this off an ally.
                let free_dispellers = frame
                    .combatants
                    .iter()
                    .filter(|(e, c)| {
                        c.team == obs.team
                            && c.alive
                            && *e != entity
                            && (c.pet_type == Some(PetType::Felhunter)
                                || (!c.is_pet
                                    && matches!(
                                        c.class,
                                        CharacterClass::Priest | CharacterClass::Paladin
                                    )))
                            && !c.auras.iter().any(|a| denies_actions(a.effect_type))
                    })
                    .count();

                let dispel_exposed = dispellable && free_dispellers > 0;

                let atk: Vec<&arenasim::headless::ObservedCombatant> = frame
                    .combatants
                    .values()
                    .filter(|c| c.team != obs.team && c.alive && c.target == Some(*entity))
                    .collect();
                let rate = self.recent_damage_rate(*entity, frame.sim_time);
                let inputs = TEffInputs {
                    applied_duration: aura.duration_remaining,
                    break_threshold: aura.break_on_damage_threshold,
                    accumulated_damage: aura.accumulated_damage,
                    incoming: split_incoming(frame, *entity, rate),
                    displaces_target: displaces_target(aura.effect_type),
                    absorb_remaining,
                    free_dispellers: dispellable.then_some(free_dispellers as u32),
                };
                let p = predict_t_eff(&inputs);

                let dispeller_available = dispel_exposed;

                // Did this CC cancel a cast? The target was hard-casting on the
                // previous frame and is not on this one.
                let cancelled_cast = self
                    .prev
                    .as_ref()
                    .and_then(|pf| pf.combatants.get(entity))
                    .and_then(|pc| pc.casting.as_ref())
                    .filter(|_| obs.casting.is_none())
                    .map(|(ability, _)| format!("{ability:?}"));

                self.live.insert(
                    key,
                    Live {
                        applied_at: frame.sim_time,
                        applied_duration: aura.duration_remaining,
                        predicted: p.t_eff,
                        predicted_cap: p.cap,
                        last_duration_remaining: aura.duration_remaining,
                        target_class: obs.class,
                        target_is_healer: obs.class.is_healer(),
                        cancelled_cast,
                        ability: aura.ability_name.clone(),
                        effect: aura.effect_type,
                        break_threshold: aura.break_on_damage_threshold,
                        predicted_from_rate: inputs.incoming.effective_rate(inputs.displaces_target),
                        dispeller_available,
                        absorb_at_application: absorb_remaining,
                        absorb_gained: 0.0,
                        attackers_at_cast: atk.iter().count(),
                        melee_at_cast: atk.iter().filter(|c| c.class.is_melee()).count(),
                        ranged_at_cast: atk.iter().filter(|c| !c.class.is_melee()).count(),
                        pet_dispel_available,
                        free_dispellers,
                        target_hp_frac: if obs.max_health > 0.0 {
                            obs.current_health / obs.max_health
                        } else {
                            0.0
                        },
                        target_mana_frac: if obs.max_mana > 0.0 {
                            obs.current_mana / obs.max_mana
                        } else {
                            1.0
                        },
                        target_team: obs.team,
                        dots_at_cast: obs
                            .auras
                            .iter()
                            .filter(|a| a.effect_type == AuraType::DamageOverTime)
                            .count(),
                    },
                );
            }
        }

        // Anything tracked but no longer present ended this frame.
        let gone: Vec<AuraKey> = self.live.keys().filter(|k| !seen.contains_key(*k)).cloned().collect();
        for key in gone {
            let reason = self.classify_end(&key, frame);
            self.close(&key, frame, reason);
        }

        self.accumulate_accounting(frame);
        self.prev = Some(frame.clone());
    }

    /// Attribute an ending from last-seen aura state plus this frame's target
    /// state. The aura is already gone from the frame, so the evidence is
    /// necessarily one frame stale — which is why "expired" is a `<= one frame
    /// left` test rather than an equality.
    fn classify_end(&self, key: &AuraKey, frame: &FrameObservation) -> EndReason {
        let Some(live) = self.live.get(key) else {
            return EndReason::Removed;
        };
        let target = frame.combatants.get(&key.0);
        if target.is_some_and(|t| !t.alive) {
            return EndReason::TargetDied;
        }
        if live.last_duration_remaining <= FRAME_DT * 1.5 {
            return EndReason::Expired;
        }
        // Still had duration left, so something ended it early. Damage is the
        // candidate — but the evidence lags: damage sites record the break
        // accumulator via `commands.entity(..).insert(DamageTakenThisFrame..)`,
        // a DEFERRED insert, and `auras.rs`'s break check reads it on a later
        // frame than the one the health actually dropped on. A single-frame
        // damage test therefore misses almost every real break (it found 2 where
        // the combat log shows many), so look back over a short window instead.
        if self.took_damage_within(key.0, frame.sim_time, BREAK_ATTRIBUTION_WINDOW_SECS) {
            EndReason::Broke
        } else {
            EndReason::Removed
        }
    }

    fn close(&mut self, key: &AuraKey, frame: &FrameObservation, end: EndReason) {
        let Some(live) = self.live.remove(key) else {
            return;
        };
        // Gross damage that actually landed on the target across the CC window.
        let damage_during: f32 = self
            .damage_history
            .get(&key.0)
            .map(|h| {
                h.iter()
                    .filter(|(t, _)| *t > live.applied_at && *t <= frame.sim_time)
                    .map(|(_, d)| *d)
                    .sum()
            })
            .unwrap_or(0.0);
        self.done.push(CcRecord {
            ability: live.ability.clone(),
            effect: live.effect,
            target_class: live.target_class,
            target_is_healer: live.target_is_healer,
            applied_at: live.applied_at,
            applied_duration: live.applied_duration,
            predicted: live.predicted,
            predicted_cap: live.predicted_cap,
            actual: (frame.sim_time - live.applied_at).max(0.0),
            end,
            cancelled_cast: live.cancelled_cast,
            break_threshold: live.break_threshold,
            damage_during,
            predicted_from_rate: live.predicted_from_rate,
            dispeller_available: live.dispeller_available,
            absorb_gained_during: live.absorb_gained,
            absorb_at_application: live.absorb_at_application,
            attackers_at_cast: live.attackers_at_cast,
            melee_at_cast: live.melee_at_cast,
            ranged_at_cast: live.ranged_at_cast,
            dots_at_cast: live.dots_at_cast,
            pet_dispel_available: live.pet_dispel_available,
            target_team: live.target_team,
            free_dispellers: live.free_dispellers,
            target_hp_frac: live.target_hp_frac,
            target_mana_frac: live.target_mana_frac,
            // First damage strictly AFTER application, up to and including the
            // frame the CC ended on (the breaking hit lands on that frame).
            time_to_first_damage: self
                .damage_history
                .get(&key.0)
                .and_then(|h| {
                    h.iter()
                        .find(|(t, d)| *t > live.applied_at && *t <= frame.sim_time && *d > 0.0)
                        .map(|(t, _)| *t - live.applied_at)
                }),
        });
    }

    /// Per-frame denial accounting. Frames are a fixed 1/60s, so summing frames
    /// is summing seconds.
    fn accumulate_accounting(&mut self, frame: &FrameObservation) {
        for team in [1u8, 2u8] {
            let denied: Vec<&arenasim::headless::ObservedCombatant> = frame
                .combatants
                .values()
                .filter(|c| c.team == team && c.alive && !c.is_pet)
                .filter(|c| c.auras.iter().any(|a| denies_actions(a.effect_type)))
                .collect();

            if !denied.is_empty() {
                self.denied_seconds += FRAME_DT * denied.len() as f32;
            }
            // Cast-time spent on CC vs everything else, over the whole frame's
            // casters. Counts a frame of cast bar, so summing frames is seconds.
            for c in frame.combatants.values().filter(|c| c.team == team && c.alive) {
                if let Some((ability, _)) = c.casting {
                    if is_cc_ability(&self.defs, ability) {
                        self.cc_cast_seconds += FRAME_DT;
                    } else {
                        self.other_cast_seconds += FRAME_DT;
                    }
                }
            }

            if denied.len() >= 2 {
                self.overlap_seconds += FRAME_DT;
                // "No counterplay": the healer is locked AND at least one other
                // member is too, so nothing on that team can answer.
                if denied.iter().any(|c| c.class.is_healer()) {
                    self.counterplay_free_seconds += FRAME_DT;
                }
            }
        }
    }

    /// Close out anything still live when the match ended, marked censored.
    fn finish(&mut self, last: &FrameObservation) {
        let keys: Vec<AuraKey> = self.live.keys().cloned().collect();
        for key in keys {
            self.close(&key, last, EndReason::Censored);
        }
    }
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

fn config(team1: &[&str], team2: &[&str], seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        random_seed: Some(seed),
        ..Default::default()
    }
}

impl CcTracker {
    fn new() -> Self {
        Self {
            defs: load_ability_definitions()
                .expect("abilities.ron must load — tests run from the crate root"),
            live: BTreeMap::new(),
            done: Vec::new(),
            damage_history: BTreeMap::new(),
            prev: None,
            denied_seconds: 0.0,
            overlap_seconds: 0.0,
            counterplay_free_seconds: 0.0,
            cc_cast_seconds: 0.0,
            other_cast_seconds: 0.0,
        }
    }
}

fn run(cfg: HeadlessMatchConfig) -> CcTracker {
    let mut tracker = CcTracker::new();
    let mut last: Option<FrameObservation> = None;
    run_headless_match_observed(cfg, true, None, |frame| {
        tracker.observe(frame);
        last = Some(frame.clone());
    })
    .expect("match should run");
    if let Some(last) = last {
        tracker.finish(&last);
    }
    tracker
}

/// The comps the CC investigation was run against, plus a double-healer case so
/// dispel pressure is represented, plus **3v3** — without which the
/// simultaneous-control and counterplay-free metrics are identical by
/// construction (two denied members of a two-person team necessarily include its
/// healer) and the whole cross-CC question is unobservable.
fn survey() -> Vec<(Vec<&'static str>, Vec<&'static str>, u64)> {
    let mut out = Vec::new();
    for seed in 1..=6u64 {
        // 2v2 — the brackets the investigation used.
        out.push((vec!["Warlock", "Warrior"], vec!["Priest", "Mage"], seed));
        out.push((vec!["Warlock", "Mage"], vec!["Mage", "Priest"], seed));
        out.push((vec!["Rogue", "Priest"], vec!["Warlock", "Priest"], seed));
        out.push((vec!["Mage", "Warlock"], vec!["Paladin", "Warrior"], seed));
        // 3v3 — a spare off-target exists, so chains and overlap are possible.
        out.push((
            vec!["Warlock", "Priest", "Mage"],
            vec!["Warrior", "Priest", "Rogue"],
            seed,
        ));
        out.push((
            vec!["Rogue", "Priest", "Mage"],
            vec!["Warlock", "Paladin", "Warrior"],
            seed,
        ));
    }
    out
}

/// A deliberately WIDE survey, used only by the composition report.
///
/// The main `survey()` is the six comps the CC investigation used, and it is
/// too narrow to generalise from — Shaman and Hunter never appear in it at all,
/// and its whole threshold-0 sample is 22 Polymorphs against three target
/// classes. This one puts a Mage (the only source of threshold-0 CC) beside
/// every possible partner, against opponents chosen to vary the three things
/// that plausibly drive a break: how much damage WE field, whether they hold a
/// pet dispeller, and whether the sheep target is a melee stuck to our team.
fn wide_survey() -> Vec<(Vec<&'static str>, Vec<&'static str>, u64)> {
    const PARTNERS: [&str; 7] =
        ["Warrior", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman"];
    // (label implied by contents) — melee-heavy, caster-heavy, pet-dispeller,
    // double-healer.
    const OPPONENTS: [[&str; 2]; 4] = [
        ["Warrior", "Rogue"],    // two melee, no dispel
        ["Mage", "Priest"],      // caster + healer dispel
        ["Warlock", "Priest"],   // Felhunter + healer dispel
        ["Paladin", "Shaman"],   // two dispellers, one melee
    ];
    let mut out = Vec::new();
    for seed in 1..=8u64 {
        for p in PARTNERS {
            for opp in OPPONENTS {
                out.push((vec!["Mage", p], vec![opp[0], opp[1]], seed));
            }
        }
    }
    out
}

/// Totals accumulated across the survey. Named rather than a tuple because six
/// bare `f32`s at a call site is how the wrong one gets printed.
#[derive(Default)]
struct SurveyTotals {
    denied: f32,
    overlap: f32,
    counterplay_free: f32,
    cc_cast: f32,
    other_cast: f32,
}

/// As `run_survey`, under an explicit CC policy. Chains are a per-frame
/// property, so they are scored on simultaneous-control seconds rather than on
/// win rate — one bit per match cannot see a chain at all.
fn run_survey_with(policy: &str) -> (Vec<CcRecord>, SurveyTotals) {
    let mut records = Vec::new();
    let mut t = SurveyTotals::default();
    for (t1, t2, seed) in survey() {
        let mut cfg = config(&t1, &t2, seed);
        cfg.cc_policy = Some(policy.to_string());
        let r = run(cfg);
        records.extend(r.done.iter().cloned());
        t.denied += r.denied_seconds;
        t.overlap += r.overlap_seconds;
        t.counterplay_free += r.counterplay_free_seconds;
        t.cc_cast += r.cc_cast_seconds;
        t.other_cast += r.other_cast_seconds;
    }
    (records, t)
}

fn run_survey() -> (Vec<CcRecord>, SurveyTotals) {
    let mut records = Vec::new();
    let mut t = SurveyTotals::default();
    for (t1, t2, seed) in survey() {
        let r = run(config(&t1, &t2, seed));
        records.extend(r.done.iter().cloned());
        t.denied += r.denied_seconds;
        t.overlap += r.overlap_seconds;
        t.counterplay_free += r.counterplay_free_seconds;
        t.cc_cast += r.cc_cast_seconds;
        t.other_cast += r.other_cast_seconds;
    }
    (records, t)
}

fn mean(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f32>() / v.len() as f32
}

// ---------------------------------------------------------------------------
// Structural invariants (always run)
// ---------------------------------------------------------------------------

#[test]
fn tracker_observes_cc_applications() {
    let t = run(config(&["Warlock", "Warrior"], &["Priest", "Mage"], 1));
    assert!(
        !t.done.is_empty(),
        "the survey comps must produce CC applications; an empty tracker means \
         the lifecycle matcher is broken, not that the match was peaceful"
    );
}

#[test]
fn predictions_are_finite_and_non_negative() {
    let (records, _) = run_survey();
    for r in &records {
        assert!(
            r.predicted.is_finite() && r.predicted >= 0.0,
            "{} on {:?}: non-finite prediction {}",
            r.ability,
            r.target_class,
            r.predicted
        );
    }
}

#[test]
fn actual_duration_never_exceeds_the_applied_duration() {
    let (records, _) = run_survey();
    for r in &records {
        // One frame of slack: the aura is first observed after it has already
        // ticked once, so `applied_duration` is up to 1/60s short of the real one.
        assert!(
            r.actual <= r.applied_duration + FRAME_DT * 2.0,
            "{} on {:?} lasted {:.3}s against an applied duration of {:.3}s",
            r.ability,
            r.target_class,
            r.actual,
            r.applied_duration
        );
    }
}

#[test]
fn a_never_breaking_horror_is_predicted_at_full_duration() {
    // Death Coil's horror carries break_on_damage -1.0, so no amount of observed
    // incoming damage may shorten its prediction. Guards the sign convention.
    let (records, _) = run_survey();
    for r in records.iter().filter(|r| r.ability.contains("Death Coil")) {
        assert_eq!(
            r.predicted_cap,
            TEffCap::Duration,
            "Death Coil predicted against {:?} by {:?}, but horror never breaks",
            r.target_class,
            r.predicted_cap
        );
    }
}

// ---------------------------------------------------------------------------
// Reports (opt-in — these print, they do not judge)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "step-0 report: prints a table, asserts nothing. Run with --nocapture"]
fn report_t_eff_prediction_error() {
    let (records, _) = run_survey();
    // Refreshed is excluded alongside Censored: both are cases where the true
    // duration is unknown, so scoring them would measure the tracker.
    let scored: Vec<&CcRecord> = records
        .iter()
        .filter(|r| r.end != EndReason::Censored && r.end != EndReason::Refreshed)
        .collect();
    // The headline excludes deaths as well as censored records: when the target
    // dies under a CC, the duration was cut by something none of the three terms
    // models, so scoring it would measure the kill, not the predictor.
    let judged: Vec<&&CcRecord> = scored.iter().filter(|r| r.end != EndReason::TargetDied).collect();

    println!("\n=== T_eff prediction error ===");
    println!(
        "{} CC applications over {} matches ({} scored, {} judged)\n",
        records.len(),
        survey().len(),
        scored.len(),
        judged.len()
    );

    let errs: Vec<f32> = judged.iter().map(|r| r.error()).collect();
    let abs: Vec<f32> = errs.iter().map(|e| e.abs()).collect();
    let actual: Vec<f32> = judged.iter().map(|r| r.actual).collect();
    println!("mean signed error   {:+.2}s  (positive = CC outlived the prediction)", mean(&errs));
    println!("mean absolute error  {:.2}s", mean(&abs));
    println!(
        "  ...against a mean observed duration of {:.2}s  =>  {:.0}% relative error",
        mean(&actual),
        100.0 * mean(&abs) / mean(&actual).max(0.001)
    );

    // The case the whole design came from.
    let healer_fears: Vec<&&CcRecord> = judged
        .iter()
        .filter(|r| r.target_is_healer && r.ability == "Fear")
        .copied()
        .collect();
    if !healer_fears.is_empty() {
        println!(
            "\nFear on healers (the motivating case): n={} predicted {:.2}s  actual {:.2}s  err {:+.2}s",
            healer_fears.len(),
            mean(&healer_fears.iter().map(|r| r.predicted).collect::<Vec<_>>()),
            mean(&healer_fears.iter().map(|r| r.actual).collect::<Vec<_>>()),
            mean(&healer_fears.iter().map(|r| r.error()).collect::<Vec<_>>()),
        );
    }

    // Skill score: does the model beat "always predict the average"?
    //
    // A low absolute error is not by itself evidence the model works — if the
    // durations barely vary, a constant scores well too. The comparison against
    // the in-sample mean (which FLATTERS the baseline, since the model never
    // sees it) is the honest test of whether the inputs carry information.
    println!("\n-- skill vs a constant predictor, by break threshold --");
    for (label, pick) in [
        ("break-on-any (0.0)", 0),
        ("damage budget (>0)", 1),
        ("never breaks (<0)", 2),
        ("ALL", 3),
    ] {
        let rs: Vec<&&CcRecord> = judged
            .iter()
            .filter(|r| match pick {
                0 => r.break_threshold == 0.0,
                1 => r.break_threshold > 0.0,
                2 => r.break_threshold < 0.0,
                _ => true,
            })
            .copied()
            .collect();
        if rs.len() < 2 {
            continue;
        }
        let actual: Vec<f32> = rs.iter().map(|r| r.actual).collect();
        let base = mean(&actual);
        let model_err = mean(&rs.iter().map(|r| r.error().abs()).collect::<Vec<_>>());
        let const_err = mean(&actual.iter().map(|a| (a - base).abs()).collect::<Vec<_>>());
        let skill = 1.0 - model_err / const_err.max(0.001);
        println!(
            "{:<20} n={:<4} model {:>5.2}s   constant({:.2}s) {:>5.2}s   skill {:+.0}%{}",
            label,
            rs.len(),
            model_err,
            base,
            const_err,
            100.0 * skill,
            if skill < 0.0 { "   <-- WORSE THAN A CONSTANT" } else { "" },
        );
    }

    println!("\n-- by end reason --");
    for reason in [
        EndReason::Expired,
        EndReason::Broke,
        EndReason::Removed,
        EndReason::TargetDied,
    ] {
        let rs: Vec<&&CcRecord> = scored.iter().filter(|r| r.end == reason).collect();
        if rs.is_empty() {
            continue;
        }
        let e: Vec<f32> = rs.iter().map(|r| r.error()).collect();
        println!(
            "{:>10?}  n={:<4} predicted {:>5.2}s  actual {:>5.2}s  err {:+.2}s",
            reason,
            rs.len(),
            mean(&rs.iter().map(|r| r.predicted).collect::<Vec<_>>()),
            mean(&rs.iter().map(|r| r.actual).collect::<Vec<_>>()),
            mean(&e)
        );
    }

    println!("\n-- by binding term --");
    for cap in [TEffCap::Duration, TEffCap::BreakBudget] {
        let rs: Vec<&&CcRecord> = scored.iter().filter(|r| r.predicted_cap == cap).collect();
        if rs.is_empty() {
            continue;
        }
        let e: Vec<f32> = rs.iter().map(|r| r.error()).collect();
        println!("{:>12?}  n={:<4} mean err {:+.2}s", cap, rs.len(), mean(&e));
    }

    println!("\n-- by ability --");
    let mut by_ability: BTreeMap<&str, Vec<&&CcRecord>> = BTreeMap::new();
    for r in &scored {
        by_ability.entry(r.ability.as_str()).or_default().push(r);
    }
    for (ability, rs) in by_ability {
        let e: Vec<f32> = rs.iter().map(|r| r.error()).collect();
        println!(
            "{:<22} n={:<4} applied {:>5.2}s  actual {:>5.2}s  err {:+.2}s",
            ability,
            rs.len(),
            mean(&rs.iter().map(|r| r.applied_duration).collect::<Vec<_>>()),
            mean(&rs.iter().map(|r| r.actual).collect::<Vec<_>>()),
            mean(&e)
        );
    }

    // --- Correction 1: what the friendly-CC guard does to incoming damage ---
    println!("\n-- damage arriving DURING the CC, vs the rate extrapolated at cast --");
    println!("   (correction 1: applying CC changes our OWN team's behaviour)");
    for (label, filter) in [
        ("break-on-any (guard applies)", 0),
        ("damage budget", 1),
        ("never breaks", 2),
    ] {
        let rs: Vec<&&CcRecord> = judged
            .iter()
            .filter(|r| match filter {
                0 => r.break_threshold == 0.0,
                1 => r.break_threshold > 0.0,
                _ => r.break_threshold < 0.0,
            })
            .copied()
            .collect();
        if rs.is_empty() {
            continue;
        }
        let extrapolated: f32 = mean(&rs
            .iter()
            .map(|r| r.predicted_from_rate * r.actual)
            .collect::<Vec<_>>());
        println!(
            "{:<30} n={:<4} extrapolated {:>6.1} dmg  actually arrived {:>6.1} dmg",
            label,
            rs.len(),
            extrapolated,
            mean(&rs.iter().map(|r| r.damage_during).collect::<Vec<_>>()),
        );
    }

    // --- Does the break term predict WHETHER a break happens, or just when? ---
    // A mean error hides this: predicting a break that never comes and predicting
    // one late are opposite errors that partly cancel in the aggregate.
    println!("\n-- break term: discrimination, not just timing --");
    let pred_break: Vec<&&CcRecord> =
        judged.iter().filter(|r| r.predicted_cap == TEffCap::BreakBudget).copied().collect();
    let pred_survive: Vec<&&CcRecord> =
        judged.iter().filter(|r| r.predicted_cap != TEffCap::BreakBudget).copied().collect();
    let hit = pred_break.iter().filter(|r| r.end == EndReason::Broke).count();
    let miss = pred_survive.iter().filter(|r| r.end == EndReason::Broke).count();
    if !pred_break.is_empty() {
        println!(
            "predicted a break: {:<4} of which {} actually broke  => precision {:.0}%",
            pred_break.len(),
            hit,
            100.0 * hit as f32 / pred_break.len() as f32
        );
        let broke_total = hit + miss;
        if broke_total > 0 {
            println!(
                "actual breaks:     {:<4} of which {} were predicted   => recall    {:.0}%",
                broke_total,
                hit,
                100.0 * hit as f32 / broke_total as f32
            );
        }
        // Timing error on the cases it got right — the honest "when" figure.
        let correct: Vec<&&CcRecord> =
            pred_break.iter().filter(|r| r.end == EndReason::Broke).copied().collect();
        if !correct.is_empty() {
            println!(
                "timing error when it correctly predicted a break: n={} err {:+.2}s",
                correct.len(),
                mean(&correct.iter().map(|r| r.error()).collect::<Vec<_>>())
            );
        }
        let false_alarm: Vec<&&CcRecord> =
            pred_break.iter().filter(|r| r.end != EndReason::Broke).copied().collect();
        if !false_alarm.is_empty() {
            println!(
                "error on false alarms (predicted break, none came):  n={} err {:+.2}s",
                false_alarm.len(),
                mean(&false_alarm.iter().map(|r| r.error()).collect::<Vec<_>>())
            );
        }
    }

    // --- Where do false alarms come from? Did the damage actually continue? ---
    println!("\n-- realized damage rate DURING the CC vs the trailing rate at cast --");
    for (label, rs) in [
        (
            "predicted break, broke",
            pred_break.iter().filter(|r| r.end == EndReason::Broke).copied().collect::<Vec<_>>(),
        ),
        (
            "predicted break, did NOT",
            pred_break.iter().filter(|r| r.end != EndReason::Broke).copied().collect::<Vec<_>>(),
        ),
    ] {
        if rs.is_empty() {
            continue;
        }
        let realized: Vec<f32> = rs
            .iter()
            .filter(|r| r.actual > 0.05)
            .map(|r| r.damage_during / r.actual)
            .collect();
        println!(
            "{:<26} n={:<4} trailing {:>6.1} dmg/s  ->  realized {:>6.1} dmg/s",
            label,
            rs.len(),
            mean(&rs.iter().map(|r| r.predicted_from_rate).collect::<Vec<_>>()),
            mean(&realized),
        );
    }
    // Calibration for a mix-based estimator: what does ONE attacker of each
    // delivery mode actually deliver, measured as realized rate during the CC?
    println!("\n-- realized rate by attacker mix at cast (estimator calibration) --");
    for (label, m, r) in [("1 melee, 0 ranged", 1usize, 0usize), ("0 melee, 1 ranged", 0, 1)] {
        let rs: Vec<&&CcRecord> = judged
            .iter()
            .filter(|x| x.actual > 0.2 && x.melee_at_cast == m && x.ranged_at_cast == r)
            .copied()
            .collect();
        if rs.is_empty() {
            println!("{label:<20} n=0");
            continue;
        }
        let mut rates: Vec<f32> = rs.iter().map(|x| x.damage_during / x.actual).collect();
        rates.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "{:<20} n={:<4} realized mean {:>6.1} dmg/s  median {:>6.1}",
            label,
            rs.len(),
            mean(&rates),
            rates[rates.len() / 2]
        );
    }

    println!("\n-- attackers pointed at the target when the CC landed --");
    for (label, rs) in [
        ("broke", judged.iter().filter(|r| r.end == EndReason::Broke).copied().collect::<Vec<_>>()),
        ("did not break", judged.iter().filter(|r| r.end != EndReason::Broke).copied().collect::<Vec<_>>()),
    ] {
        if rs.is_empty() { continue; }
        let zero = rs.iter().filter(|r| r.attackers_at_cast == 0).count();
        println!(
            "{:<16} n={:<4} mean attackers {:.2}   ({} of {} had ZERO attackers on it)",
            label,
            rs.len(),
            mean(&rs.iter().map(|r| r.attackers_at_cast as f32).collect::<Vec<_>>()),
            zero,
            rs.len(),
        );
    }

    // Displacement check: fear and horror make the target flee, which should cut
    // incoming melee damage during the CC specifically for those effects.
    println!("\n-- realized rate by whether the CC displaces the target --");
    for (label, displacing) in [("displacing (Fear/horror)", true), ("stationary (Stun/Poly)", false)] {
        let rs: Vec<&&CcRecord> = judged
            .iter()
            .filter(|r| (r.effect == AuraType::Fear) == displacing)
            .filter(|r| r.actual > 0.05)
            .copied()
            .collect();
        if rs.is_empty() {
            continue;
        }
        println!(
            "{:<26} n={:<4} trailing {:>6.1} dmg/s  ->  realized {:>6.1} dmg/s",
            label,
            rs.len(),
            mean(&rs.iter().map(|r| r.predicted_from_rate).collect::<Vec<_>>()),
            mean(&rs.iter().map(|r| r.damage_during / r.actual).collect::<Vec<_>>()),
        );
    }

    // --- Absorb replenishment: buffer the predictor never counted ---
    println!("\n-- absorb pool at application vs absorb GAINED during the CC --");
    let breakable: Vec<&&CcRecord> = judged.iter().filter(|r| r.break_threshold >= 0.0).copied().collect();
    if !breakable.is_empty() {
        let refreshed = breakable.iter().filter(|r| r.absorb_gained_during > 0.0).count();
        println!(
            "{:<30} n={:<4} at cast {:>6.1}  gained during {:>6.1}   ({} of {} were re-shielded mid-CC)",
            "breakable CC",
            breakable.len(),
            mean(&breakable.iter().map(|r| r.absorb_at_application).collect::<Vec<_>>()),
            mean(&breakable.iter().map(|r| r.absorb_gained_during).collect::<Vec<_>>()),
            refreshed,
            breakable.len(),
        );
        let with = |b: bool| -> Vec<&&CcRecord> {
            breakable
                .iter()
                .filter(|r| (r.absorb_gained_during > 0.0) == b)
                .copied()
                .collect()
        };
        for (label, rs) in [("re-shielded mid-CC", with(true)), ("not re-shielded", with(false))] {
            if rs.is_empty() {
                continue;
            }
            println!(
                "   {:<22} n={:<4} err {:+.2}s",
                label,
                rs.len(),
                mean(&rs.iter().map(|r| r.error()).collect::<Vec<_>>())
            );
        }
    }

    // --- Correction 2: what a dispel term would need to know ---
    println!("\n-- dispel latency (the `expected_dispel_delay` the model lacks) --");
    let removed: Vec<&&CcRecord> = judged.iter().filter(|r| r.end == EndReason::Removed).copied().collect();
    let with_dispeller: Vec<&&CcRecord> = removed.iter().filter(|r| r.dispeller_available).copied().collect();
    println!(
        "{} of {} judged applications ended Removed; {} of those had a free dispeller at cast",
        removed.len(),
        judged.len(),
        with_dispeller.len()
    );
    if !with_dispeller.is_empty() {
        let mut lat: Vec<f32> = with_dispeller.iter().map(|r| r.actual).collect();
        lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "   latency: mean {:.2}s  median {:.2}s  min {:.2}s  max {:.2}s",
            mean(&lat),
            lat[lat.len() / 2],
            lat[0],
            lat[lat.len() - 1]
        );
    }
    // The base rate that decides whether the term is worth modelling at all.
    let exposed: Vec<&&CcRecord> = judged.iter().filter(|r| r.dispeller_available).copied().collect();
    if !exposed.is_empty() {
        let dispelled = exposed.iter().filter(|r| r.end == EndReason::Removed).count();
        println!(
            "   of {} applications cast INTO a free dispeller, {} were removed ({:.0}%)",
            exposed.len(),
            dispelled,
            100.0 * dispelled as f32 / exposed.len() as f32
        );
    }

    let censored = records.len() - scored.len();
    println!("\n({censored} censored by match end, excluded from every figure above)");
}

/// Per-application dump for one match, so a classification can be audited line
/// by line against that match's combat log. The classifier reasons from stale
/// observation (an aura is gone before its cause is visible), so it needs to
/// stay checkable rather than trusted — the first version of it called only 2
/// breaks where the log showed many, because damage attribution lags a frame.
/// Why threshold-0 CC is the predictor's worst case, in one table.
///
/// A `0.0` break threshold means the aura ends on the first point of health
/// damage. The rate model asks "how long to accumulate a budget of zero" and
/// answers "instantly" whenever any damage is trailing — but the quantity that
/// actually decides the duration is the ARRIVAL TIME of the next damage event,
/// which is a waiting time, not a rate.
#[test]
#[ignore = "step-0 report: prints a table, asserts nothing. Run with --nocapture"]
fn report_break_on_any_damage() {
    let (records, _) = run_survey();
    let judged: Vec<&CcRecord> = records
        .iter()
        .filter(|r| {
            r.end != EndReason::Censored
                && r.end != EndReason::TargetDied
                && r.end != EndReason::Refreshed
        })
        .collect();
    let zero: Vec<&&CcRecord> = judged.iter().filter(|r| r.break_threshold == 0.0).collect();

    println!("\n=== break-on-any-damage (threshold 0.0) ===");
    println!("n={} judged applications\n", zero.len());
    if zero.is_empty() {
        return;
    }

    println!(
        "predicted {:.2}s   actual {:.2}s   err {:+.2}s   (abs {:.2}s)",
        mean(&zero.iter().map(|r| r.predicted).collect::<Vec<_>>()),
        mean(&zero.iter().map(|r| r.actual).collect::<Vec<_>>()),
        mean(&zero.iter().map(|r| r.error()).collect::<Vec<_>>()),
        mean(&zero.iter().map(|r| r.error().abs()).collect::<Vec<_>>()),
    );

    // The claim under test: for these, actual duration IS time-to-first-damage.
    let with_dmg: Vec<&&&CcRecord> =
        zero.iter().filter(|r| r.time_to_first_damage.is_some()).collect();
    println!(
        "\n{} of {} took damage before ending; for those:",
        with_dmg.len(),
        zero.len()
    );
    if !with_dmg.is_empty() {
        let ttfd: Vec<f32> = with_dmg.iter().map(|r| r.time_to_first_damage.unwrap()).collect();
        let act: Vec<f32> = with_dmg.iter().map(|r| r.actual).collect();
        println!(
            "   time to first damage {:.2}s   vs actual duration {:.2}s   (gap {:+.2}s)",
            mean(&ttfd),
            mean(&act),
            mean(&act) - mean(&ttfd)
        );
        let mut sorted = ttfd.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "   time-to-first-damage quartiles: min {:.2}  p25 {:.2}  median {:.2}  p75 {:.2}  max {:.2}",
            sorted[0],
            sorted[sorted.len() / 4],
            sorted[sorted.len() / 2],
            sorted[sorted.len() * 3 / 4],
            sorted[sorted.len() - 1],
        );
    }

    // Split by what is actually able to deliver that first point of damage.
    // Our own team suppresses attacks on a break-on-any CC target via the
    // friendly-CC guard, so DoTs already ticking are the prime suspect.
    println!("\n-- by DoTs ticking on the target at application --");
    for (label, has_dots) in [("no DoTs", false), ("1+ DoTs", true)] {
        let rs: Vec<&&&CcRecord> =
            zero.iter().filter(|r| (r.dots_at_cast > 0) == has_dots).collect();
        if rs.is_empty() {
            continue;
        }
        let ttfd: Vec<f32> = rs.iter().filter_map(|r| r.time_to_first_damage).collect();
        println!(
            "{:<10} n={:<4} actual {:>5.2}s   broke {:>2}/{:<3}   first damage at {}",
            label,
            rs.len(),
            mean(&rs.iter().map(|r| r.actual).collect::<Vec<_>>()),
            rs.iter().filter(|r| r.end == EndReason::Broke).count(),
            rs.len(),
            if ttfd.is_empty() { "never".to_string() } else { format!("{:.2}s", mean(&ttfd)) },
        );
    }

    println!("\n-- by attackers pointed at the target at application --");
    for (label, lo, hi) in [("0 attackers", 0, 0), ("1 attacker", 1, 1), ("2+ attackers", 2, 99)] {
        let rs: Vec<&&&CcRecord> = zero
            .iter()
            .filter(|r| r.attackers_at_cast >= lo && r.attackers_at_cast <= hi)
            .collect();
        if rs.is_empty() {
            continue;
        }
        let ttfd: Vec<f32> = rs.iter().filter_map(|r| r.time_to_first_damage).collect();
        println!(
            "{:<14} n={:<4} actual {:>5.2}s   broke {:>2}/{:<3}   first damage at {}",
            label,
            rs.len(),
            mean(&rs.iter().map(|r| r.actual).collect::<Vec<_>>()),
            rs.iter().filter(|r| r.end == EndReason::Broke).count(),
            rs.len(),
            if ttfd.is_empty() { "never".to_string() } else { format!("{:.2}s", mean(&ttfd)) },
        );
    }

    println!("\n-- how these actually ended --");
    for reason in [EndReason::Expired, EndReason::Broke, EndReason::Removed] {
        let rs: Vec<&&&CcRecord> = zero.iter().filter(|r| r.end == reason).collect();
        if rs.is_empty() {
            continue;
        }
        println!(
            "{:>10?}  n={:<4} actual {:>5.2}s  predicted {:>5.2}s",
            reason,
            rs.len(),
            mean(&rs.iter().map(|r| r.actual).collect::<Vec<_>>()),
            mean(&rs.iter().map(|r| r.predicted).collect::<Vec<_>>()),
        );
    }

    // Decompose the error by PROCESS. Threshold-0 auras end by one of two
    // completely different mechanisms — a landed attack (what the break term
    // models) or an instant pet dispel (what it cannot see). Scoring them
    // together hides which one the model is actually bad at.
    println!("\n-- error by ENDING PROCESS (is the break model itself sound?) --");
    for (label, keep) in [
        ("break process only (Expired+Broke)", true),
        ("dispel process (Removed)", false),
    ] {
        let rs: Vec<&&&CcRecord> = zero
            .iter()
            .filter(|r| (r.end != EndReason::Removed) == keep)
            .collect();
        if rs.is_empty() {
            continue;
        }
        let act: Vec<f32> = rs.iter().map(|r| r.actual).collect();
        let base = mean(&act);
        let model = mean(&rs.iter().map(|r| r.error().abs()).collect::<Vec<_>>());
        let konst = mean(&act.iter().map(|a| (a - base).abs()).collect::<Vec<_>>());
        println!(
            "{:<36} n={:<3} model {:>5.2}s   constant({:.2}s) {:>5.2}s   skill {:+.0}%",
            label,
            rs.len(),
            model,
            base,
            konst,
            100.0 * (1.0 - model / konst.max(0.001)),
        );
    }

    // Calibration for a hit-size floor: if damage arrives at rate `r` in discrete
    // chunks of mean size `h`, events are spaced `h/r` apart. Recovering `h` from
    // observed (rate, waiting time) pairs tells us what to put in place of the
    // degenerate zero budget.
    println!("\n-- implied hit size (trailing rate x observed time-to-first-damage) --");
    for (label, lo, hi) in [("1 attacker", 1, 1), ("2+ attackers", 2, 99)] {
        let rs: Vec<&&&CcRecord> = zero
            .iter()
            .filter(|r| {
                r.attackers_at_cast >= lo
                    && r.attackers_at_cast <= hi
                    && r.time_to_first_damage.is_some()
                    && r.predicted_from_rate > 0.0
            })
            .collect();
        if rs.is_empty() {
            continue;
        }
        let implied: Vec<f32> = rs
            .iter()
            .map(|r| r.predicted_from_rate * r.time_to_first_damage.unwrap())
            .collect();
        println!(
            "{:<14} n={:<3} trailing rate {:>5.1} dmg/s   first damage {:>5.2}s   => implied hit {:>5.1}",
            label,
            rs.len(),
            mean(&rs.iter().map(|r| r.predicted_from_rate).collect::<Vec<_>>()),
            mean(&rs.iter().map(|r| r.time_to_first_damage.unwrap()).collect::<Vec<_>>()),
            mean(&implied),
        );
    }

    // Removal rate: is the shared 18% dispel probability right for these?
    let exposed: Vec<&&&CcRecord> = zero.iter().filter(|r| r.dispeller_available).collect();
    let unexposed: Vec<&&&CcRecord> = zero.iter().filter(|r| !r.dispeller_available).collect();
    println!("\n-- removal (dispel) rate, vs the model's shared 18% --");
    println!(
        "with a HEALER dispeller visible at cast:    {:>2}/{:<3} removed",
        exposed.iter().filter(|r| r.end == EndReason::Removed).count(),
        exposed.len(),
    );
    println!(
        "with NO healer dispeller visible at cast:   {:>2}/{:<3} removed",
        unexposed.iter().filter(|r| r.end == EndReason::Removed).count(),
        unexposed.len(),
    );

    // The association above runs BACKWARDS, and this is why: the removals are
    // not healer dispels at all.
    let pet: Vec<&&&CcRecord> = zero.iter().filter(|r| r.pet_dispel_available).collect();
    let nopet: Vec<&&&CcRecord> = zero.iter().filter(|r| !r.pet_dispel_available).collect();
    println!("\n-- removal rate by PET dispeller (Felhunter / Devour Magic) --");
    for (label, rs) in [("Felhunter present", &pet), ("no Felhunter", &nopet)] {
        if rs.is_empty() {
            continue;
        }
        println!(
            "{:<20} {:>2}/{:<3} removed   actual {:>5.2}s",
            label,
            rs.iter().filter(|r| r.end == EndReason::Removed).count(),
            rs.len(),
            mean(&rs.iter().map(|r| r.actual).collect::<Vec<_>>()),
        );
    }
    println!(
        "   Devour Magic is instant, free and off the GCD. Pricing it was measured\n   \
         and REMOVED (+1% predictor skill, -3pt in the Warlock mirror) — see cc_value."
    );

    // Which targets these land on, and how it goes for each. This sim has no pet
    // dispel, so an early removal with no damage has to come from somewhere else
    // — Divine Shield purging its owner's CC is the candidate.
    println!("\n-- by target class --");
    let mut by_class: BTreeMap<String, Vec<&&&CcRecord>> = BTreeMap::new();
    for r in &zero {
        by_class.entry(format!("{:?}", r.target_class)).or_default().push(r);
    }
    for (class, rs) in by_class {
        println!(
            "{:<10} n={:<3} actual {:>5.2}s   expired {:>2}  broke {:>2}  removed {:>2}",
            class,
            rs.len(),
            mean(&rs.iter().map(|r| r.actual).collect::<Vec<_>>()),
            rs.iter().filter(|r| r.end == EndReason::Expired).count(),
            rs.iter().filter(|r| r.end == EndReason::Broke).count(),
            rs.iter().filter(|r| r.end == EndReason::Removed).count(),
        );
    }
}

/// Is the break-on-any-damage problem specific to the Warlock/Felhunter matchup,
/// or general?
///
/// The narrow survey's threshold-0 sample is 22 Polymorphs against three target
/// classes, which is not enough to tell "our own damage breaks our own crowd
/// control" from "the Felhunter eats it". This runs a Mage beside every partner
/// against four opponent shapes and splits the outcome by what should drive it.
#[test]
#[ignore = "step-0 report: prints a table, asserts nothing. Run with --nocapture"]
fn report_break_on_any_by_composition() {
    let mut rows: Vec<(String, String, CcRecord)> = Vec::new();
    for (t1, t2, seed) in wide_survey() {
        let ours = t1.join("+");
        let theirs = t2.join("+");
        for r in run(config(&t1, &t2, seed)).done {
            // Only OUR Mage's crowd control: team 2 is the side we cast onto.
            if r.break_threshold == 0.0
                && r.target_team == 2
                && r.end != EndReason::Censored
                && r.end != EndReason::Refreshed
                && r.end != EndReason::TargetDied
            {
                rows.push((ours.clone(), theirs.clone(), r));
            }
        }
    }

    println!("\n=== break-on-any (Polymorph) by composition ===");
    println!("{} applications over {} matches\n", rows.len(), wide_survey().len());
    if rows.is_empty() {
        return;
    }

    let summarise = |label: &str, sel: &dyn Fn(&(String, String, CcRecord)) -> bool| {
        let rs: Vec<&(String, String, CcRecord)> = rows.iter().filter(|r| sel(r)).collect();
        if rs.is_empty() {
            return;
        }
        let n = rs.len();
        let broke = rs.iter().filter(|r| r.2.end == EndReason::Broke).count();
        let removed = rs.iter().filter(|r| r.2.end == EndReason::Removed).count();
        let expired = rs.iter().filter(|r| r.2.end == EndReason::Expired).count();
        let act = mean(&rs.iter().map(|r| r.2.actual).collect::<Vec<_>>());
        let full = mean(&rs.iter().map(|r| r.2.applied_duration).collect::<Vec<_>>());
        println!(
            "  {:<26} n={:<4} broke {:>3.0}%  dispelled {:>3.0}%  expired {:>3.0}%   \
             lasted {:>4.2}s of {:>4.2}s ({:>3.0}%)",
            label,
            n,
            100.0 * broke as f32 / n as f32,
            100.0 * removed as f32 / n as f32,
            100.0 * expired as f32 / n as f32,
            act,
            full,
            100.0 * act / full.max(0.001),
        );
    };

    // THE question: does our own comp's damage output drive the break rate?
    println!("-- by OUR partner (who else of ours is dealing damage) --");
    for p in ["Priest", "Paladin", "Shaman", "Hunter", "Warlock", "Rogue", "Warrior"] {
        let want = format!("Mage+{p}");
        summarise(&want.clone(), &move |r| r.0 == want);
    }

    println!("\n-- by THEIR comp (what we are sheeping into) --");
    let mut opps: Vec<String> = rows.iter().map(|r| r.1.clone()).collect();
    opps.sort();
    opps.dedup();
    for o in opps {
        let want = o.clone();
        summarise(&want.clone(), &move |r| r.1 == want);
    }

    println!("\n-- by the sheep target's own class --");
    let mut cls: Vec<String> = rows.iter().map(|r| format!("{:?}", r.2.target_class)).collect();
    cls.sort();
    cls.dedup();
    for c in cls {
        let want = c.clone();
        summarise(&want.clone(), &move |r| format!("{:?}", r.2.target_class) == want);
    }

    println!("\n-- pet dispeller present at cast? --");
    summarise("Felhunter present", &|r| r.2.pet_dispel_available);
    summarise("no Felhunter", &|r| !r.2.pet_dispel_available);
}

/// Calibrate the dispel term against the count of units that can ACTUALLY take
/// the aura off an ally.
///
/// The shipped term is a flat 18% whenever any "healer" is free. Two things are
/// suspect: the probability (composition data suggests far higher), and the
/// population (it counts Shamans, whose Purge is offensive only).
#[test]
#[ignore = "step-0 report: prints a table, asserts nothing. Run with --nocapture"]
fn report_dispel_calibration() {
    let mut recs: Vec<CcRecord> = Vec::new();
    for (t1, t2, seed) in wide_survey() {
        recs.extend(run(config(&t1, &t2, seed)).done.into_iter().filter(|r| {
            r.end != EndReason::Censored
                && r.end != EndReason::Refreshed
                && r.end != EndReason::TargetDied
        }));
    }
    // Only dispellable auras can be dispelled; stuns are noise here.
    let dispellable: Vec<&CcRecord> = recs
        .iter()
        .filter(|r| r.effect.is_magic_dispellable())
        .collect();

    println!("\n=== dispel calibration ({} dispellable applications) ===", dispellable.len());

    let row = |label: &str, rs: Vec<&&CcRecord>| {
        if rs.is_empty() {
            return;
        }
        let n = rs.len();
        let removed = rs.iter().filter(|r| r.end == EndReason::Removed).count();
        let lat: Vec<f32> = rs
            .iter()
            .filter(|r| r.end == EndReason::Removed)
            .map(|r| r.actual)
            .collect();
        println!(
            "  {:<34} n={:<4} removed {:>3.0}%   latency {}",
            label,
            n,
            100.0 * removed as f32 / n as f32,
            if lat.is_empty() { "-".to_string() } else { format!("{:.2}s", mean(&lat)) },
        );
    };

    println!("\n-- by FREE ALLY-DISPELLERS at cast (Priest/Paladin/Felhunter, target excluded) --");
    for k in 0..=3usize {
        row(
            &format!("{k} free dispeller(s)"),
            dispellable.iter().filter(|r| r.free_dispellers == k).collect(),
        );
    }

    println!("\n-- does the CC DISPLACE the target? (a feared ally may flee dispel range) --");
    for (label, want) in [("displacing (Fear)", true), ("stationary (Poly/Root)", false)] {
        row(
            label,
            dispellable
                .iter()
                .filter(|r| displaces_target(r.effect) == want && r.free_dispellers > 0)
                .collect(),
        );
    }

    println!("\n-- cross: displacement x dispeller count --");
    for (label, want) in [("displacing", true), ("stationary", false)] {
        for k in [1usize, 2] {
            row(
                &format!("{label}, {k} dispeller(s)"),
                dispellable
                    .iter()
                    .filter(|r| displaces_target(r.effect) == want && r.free_dispellers == k)
                    .collect(),
            );
        }
    }

    println!("\n-- what the SHIPPED term counts (is_healer, incl. the Shaman) --");
    for (label, want) in [("healer flagged free", true), ("no healer flagged", false)] {
        row(label, dispellable.iter().filter(|r| r.dispeller_available == want).collect());
    }
    println!("\n  shipped DISPEL_PROBABILITY = 0.18");
}

/// Does the priced Mage pick BETTER Polymorph targets, independent of win rate?
///
/// Win rate is the wrong instrument for "does this look like competent play".
/// This compares, for the one cell the Mage extension was ever measured on,
/// what each policy actually sheeps and how long it survives.
#[test]
#[ignore = "step-0 report: prints a table, asserts nothing. Run with --nocapture"]
fn report_mage_polymorph_choice_by_policy() {
    for (opp, policy) in [
        (["Paladin", "Warrior"], "Identity"),
        (["Paladin", "Warrior"], "Priced"),
        (["Rogue", "Priest"], "Identity"),
        (["Rogue", "Priest"], "Priced"),
        (["Warlock", "Priest"], "Identity"),
        (["Warlock", "Priest"], "Priced"),
    ] {
        let mut recs: Vec<CcRecord> = Vec::new();
        for seed in 1..=20u64 {
            let mut cfg = config(&["Mage", "Priest"], &opp, seed);
            cfg.cc_policy = Some(policy.to_string());
            recs.extend(run(cfg).done.into_iter().filter(|r| {
                r.break_threshold == 0.0
                    && r.target_team == 2
                    && r.end != EndReason::Censored
                    && r.end != EndReason::Refreshed
            }));
        }
        println!(
            "\n=== vs {} — {policy}: {} Polymorphs over 20 matches ===",
            opp.join("+"),
            recs.len()
        );
        if recs.is_empty() {
            continue;
        }
        let mut by: BTreeMap<String, Vec<&CcRecord>> = BTreeMap::new();
        for r in &recs {
            by.entry(format!("{:?}", r.target_class)).or_default().push(r);
        }
        for (cls, rs) in by {
            let n = rs.len();
            println!(
                "  on {:<9} n={:<3} ({:>3.0}% of casts)   lasted {:>4.2}s of {:>4.2}s   \
                 broke {:>2.0}%  dispelled {:>2.0}%  expired {:>2.0}%",
                cls,
                n,
                100.0 * n as f32 / recs.len() as f32,
                mean(&rs.iter().map(|r| r.actual).collect::<Vec<_>>()),
                mean(&rs.iter().map(|r| r.applied_duration).collect::<Vec<_>>()),
                100.0 * rs.iter().filter(|r| r.end == EndReason::Broke).count() as f32 / n as f32,
                100.0 * rs.iter().filter(|r| r.end == EndReason::Removed).count() as f32 / n as f32,
                100.0 * rs.iter().filter(|r| r.end == EndReason::Expired).count() as f32 / n as f32,
            );
        }
        let denied: f32 = recs.iter().map(|r| r.actual).sum();
        println!("  total seconds of enemy action denied: {denied:.1}s");
        // WHEN are these cast? "Opening tempo" requires them to be early.
        let mut times: Vec<f32> = recs.iter().map(|r| r.applied_at).collect();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  cast at (sim seconds, gates open at 10): median {:.1}  min {:.1}  max {:.1}   \
             first 20s of combat: {}/{}",
            times[times.len() / 2],
            times[0],
            times[times.len() - 1],
            recs.iter().filter(|r| r.applied_at < 30.0).count(),
            recs.len(),
        );

        // Are WE attacking the thing we just crowd-controlled? `attackers_at_cast`
        // counts units hostile to the target — i.e. ours — pointed at it.
        let engaged = recs.iter().filter(|r| r.attackers_at_cast > 0).count();
        println!(
            "  cast on a target OUR team was attacking: {}/{} ({:.0}%)   mean attackers {:.2}",
            engaged,
            recs.len(),
            100.0 * engaged as f32 / recs.len() as f32,
            mean(&recs.iter().map(|r| r.attackers_at_cast as f32).collect::<Vec<_>>()),
        );

        // Hypothesis 1: crowd control landed on a target we are about to kill.
        let dying: Vec<&CcRecord> = recs.iter().filter(|r| r.target_hp_frac < 0.35).collect();
        println!(
            "  cast on a target below 35% HP: {}/{} ({:.0}%)   mean HP at cast {:.0}%",
            dying.len(),
            recs.len(),
            100.0 * dying.len() as f32 / recs.len() as f32,
            100.0 * mean(&recs.iter().map(|r| r.target_hp_frac).collect::<Vec<_>>()),
        );
        // Hypothesis 2: crowd control spent on a healer that cannot afford a heal.
        let healers: Vec<&CcRecord> = recs.iter().filter(|r| r.target_class.is_healer()).collect();
        if !healers.is_empty() {
            let dry = healers.iter().filter(|r| r.target_mana_frac < 0.20).count();
            let _ = &healers;
            println!(
                "  on healers: {}   of which under 20% mana: {} ({:.0}%)   mean mana {:.0}%",
                healers.len(),
                dry,
                100.0 * dry as f32 / healers.len() as f32,
                100.0 * mean(&healers.iter().map(|r| r.target_mana_frac).collect::<Vec<_>>()),
            );
        }
    }
}


/// Score the priced model on DENIAL rather than win rate.
///
/// Every behavioural step in this work was judged on win rate, which is exactly
/// the instrument that cannot see compounding: fifteen continuous seconds of
/// control and three separate five-second windows produce the same `D x T_eff`
/// and can produce the same win rate, while being worth very different amounts.
///
/// Attribution is PER SIDE — the shared accounting sums both teams, which cannot
/// answer "did OUR policy deny more". Each cell is run in both assignments so
/// the two policies meet across the same table on the same seeds.
///
/// Three metrics, weakest to strongest:
///
/// - **denied seconds** — enemy member-seconds spent unable to act. Volume.
/// - **counterplay-free seconds** — two or more enemies denied at once, one of
///   them a healer, so nothing on that side can answer. This is the "cross CC"
///   shape, and it requires coordination between two casters.
/// - **longest unbroken lockout** on any single enemy, per match. The direct
///   measure of chaining: it only grows if a new application lands before the
///   previous one lapses.
#[test]
#[ignore = "step-0 report: prints a table, asserts nothing. Run with --nocapture"]
fn report_denial_scoring_by_policy() {
    struct Tally {
        denied: f32,
        counterplay_free: f32,
        longest: Vec<f32>,
    }

    let cells: [(&str, Vec<&str>, Vec<&str>); 6] = [
        ("2v2 vs Paladin+Warrior", vec!["Mage", "Priest"], vec!["Paladin", "Warrior"]),
        ("2v2 vs Rogue+Priest", vec!["Mage", "Priest"], vec!["Rogue", "Priest"]),
        ("2v2 vs Warlock+Priest", vec!["Mage", "Priest"], vec!["Warlock", "Priest"]),
        ("2v2 Warlock: vs Priest+Rogue", vec!["Warlock", "Priest"], vec!["Priest", "Rogue"]),
        // 3v3: counterplay-free requires TWO enemies locked at once including a
        // healer, which in a 2v2 means the whole enemy team. More bodies and
        // more crowd control should give the metric room to register.
        ("3v3 Mage+Warlock+Priest", vec!["Mage", "Warlock", "Priest"],
            vec!["Rogue", "Paladin", "Warrior"]),
        ("3v3 vs double healer", vec!["Mage", "Warlock", "Priest"],
            vec!["Priest", "Paladin", "Rogue"]),
    ];

    println!("\n=== denial inflicted, by the policy that inflicted it ===");
    println!("(both assignments per cell, 20 seeds each, so the policies meet on the same seeds)\n");

    for (label, t1, t2) in cells {
        // policy name -> what that side inflicted on the other
        let mut by_policy: BTreeMap<&str, Tally> = BTreeMap::new();
        for p in ["Identity", "Priced"] {
            by_policy.insert(p, Tally { denied: 0.0, counterplay_free: 0.0, longest: Vec::new() });
        }

        for (p1, p2) in [("Priced", "Identity"), ("Identity", "Priced")] {
            for seed in 1..=20u64 {
                let mut cfg = config(&t1, &t2, seed);
                cfg.team1_cc_policy = Some(p1.to_string());
                cfg.team2_cc_policy = Some(p2.to_string());

                // Per-team-being-denied accumulators, plus per-entity lockout runs.
                let mut denied = [0.0f32; 3];
                let mut cpf = [0.0f32; 3];
                let mut run: BTreeMap<Entity, f32> = BTreeMap::new();
                let mut best = [0.0f32; 3];

                run_headless_match_observed(cfg, true, None, |frame| {
                    for team in [1u8, 2u8] {
                        let locked: Vec<&arenasim::headless::ObservedCombatant> = frame
                            .combatants
                            .values()
                            .filter(|c| c.team == team && c.alive && !c.is_pet)
                            .filter(|c| c.auras.iter().any(|a| denies_actions(a.effect_type)))
                            .collect();
                        denied[team as usize] += FRAME_DT * locked.len() as f32;
                        if locked.len() >= 2 && locked.iter().any(|c| c.class.is_healer()) {
                            cpf[team as usize] += FRAME_DT;
                        }
                    }
                    // Unbroken lockout per entity: extend while denied, reset when not.
                    for (e, c) in &frame.combatants {
                        if c.is_pet || !c.alive {
                            continue;
                        }
                        let held = c.auras.iter().any(|a| denies_actions(a.effect_type));
                        let cur = run.entry(*e).or_insert(0.0);
                        if held {
                            *cur += FRAME_DT;
                            let t = c.team as usize;
                            if *cur > best[t] {
                                best[t] = *cur;
                            }
                        } else {
                            *cur = 0.0;
                        }
                    }
                })
                .expect("match should run");

                // Team 1 ran p1 and inflicted on team 2, and vice versa.
                for (policy, victim) in [(p1, 2usize), (p2, 1usize)] {
                    let t = by_policy.get_mut(policy).unwrap();
                    t.denied += denied[victim];
                    t.counterplay_free += cpf[victim];
                    t.longest.push(best[victim]);
                }
            }
        }

        println!("-- {label} --");
        for p in ["Identity", "Priced"] {
            let t = &by_policy[p];
            let n = t.longest.len().max(1) as f32;
            println!(
                "  {:<9} denied {:>7.1}s   counterplay-free {:>6.1}s   longest lockout {:.2}s (max {:.2}s)",
                p,
                t.denied / n,
                t.counterplay_free / n,
                mean(&t.longest),
                t.longest.iter().copied().fold(0.0f32, f32::max),
            );
        }
        println!();
    }
}

#[test]
#[ignore = "step-0 report: prints a table, asserts nothing. Run with --nocapture"]
fn report_single_match_detail() {
    let t = run(config(&["Warlock", "Mage"], &["Mage", "Priest"], 2));
    println!("\n=== Warlock+Mage vs Mage+Priest, seed 2 ===");
    println!(
        "{:<18} {:<9} {:>7} {:>8} {:>8} {:>8}  {:<11} {}",
        "ability", "target", "at", "applied", "predict", "actual", "ended", "cancelled"
    );
    for r in &t.done {
        println!(
            "{:<18} {:<9} {:>6.2}s {:>7.2}s {:>7.2}s {:>7.2}s  {:<11} {}",
            r.ability,
            format!("{:?}", r.target_class),
            r.applied_at,
            r.applied_duration,
            r.predicted,
            r.actual,
            format!("{:?}", r.end),
            r.cancelled_cast.as_deref().unwrap_or("-"),
        );
    }
}

#[test]
#[ignore = "step-0 report: prints a table, asserts nothing. Run with --nocapture"]
fn report_cc_accounting() {
    let (records, totals) = run_survey();
    let matches = survey().len() as f32;

    println!("\n=== CC accounting (per match, {} matches) ===\n", matches);
    println!("control-seconds denied      {:>7.2}s", totals.denied / matches);
    println!("simultaneous-control        {:>7.2}s   (>=2 enemies denied at once)", totals.overlap / matches);
    println!("counterplay-free            {:>7.2}s   (healer + >=1 other, together)", totals.counterplay_free / matches);
    if (totals.overlap - totals.counterplay_free).abs() < f32::EPSILON {
        println!(
            "  (equal. FORCED at 2v2 — two denied members of a two-person team\n   \
             necessarily include its healer. The survey also contains 3v3, where\n   \
             it is NOT forced, so the equality there is a finding: every\n   \
             simultaneous-control window in this survey included a healer.)"
        );
    }

    let broke: Vec<&CcRecord> = records.iter().filter(|r| r.end == EndReason::Broke).collect();
    let wasted: f32 = broke.iter().map(|r| r.applied_duration - r.actual).sum();
    println!(
        "\nCC seconds lost to friendly damage  {:>7.2}s   ({} of {} applications broke early)",
        wasted / matches,
        broke.len(),
        records.len()
    );

    let on_healers: Vec<&CcRecord> = records.iter().filter(|r| r.target_is_healer).collect();
    let healer_broke = on_healers.iter().filter(|r| r.end == EndReason::Broke).count();
    if !on_healers.is_empty() {
        println!(
            "  on healers: {}/{} broke early ({:.0}%), mean duration {:.2}s of {:.2}s applied",
            healer_broke,
            on_healers.len(),
            100.0 * healer_broke as f32 / on_healers.len() as f32,
            mean(&on_healers.iter().map(|r| r.actual).collect::<Vec<_>>()),
            mean(&on_healers.iter().map(|r| r.applied_duration).collect::<Vec<_>>()),
        );
    }

    let cancelled: Vec<&CcRecord> = records.iter().filter(|r| r.cancelled_cast.is_some()).collect();
    println!("\ncasts cancelled by CC       {:>7} of {} applications", cancelled.len(), records.len());
    let mut by_cast: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &cancelled {
        *by_cast.entry(r.cancelled_cast.as_deref().unwrap()).or_default() += 1;
    }
    for (cast, n) in by_cast {
        println!("  {cast:<24} {n}");
    }

    println!("\n-- what the CC cost, in the units available before step 4 --");
    println!(
        "cast-seconds on CC          {:>7.2}s   of {:.2}s total hard-casting ({:.0}%)",
        totals.cc_cast / matches,
        (totals.cc_cast + totals.other_cast) / matches,
        100.0 * totals.cc_cast / (totals.cc_cast + totals.other_cast).max(0.001)
    );
    println!(
        "\nNOTE: this is TIME, not value. Pricing those seconds at the damage they\n\
         displaced needs step 4's cost model — until then the denial figures above\n\
         and this cost figure cannot be traded off against each other, only\n\
         reported side by side."
    );
}


/// Step 5 (`E`, enabling) scored on the metric the measurement plan specifies.
///
/// Win rate is one bit per match and cannot see a chain; simultaneous-control
/// seconds can. The baseline to move is **0.21s per match of two-or-more enemies
/// controlled at once, against 13.93s of total denial** — under 2% of denial
/// time overlapping, i.e. chains essentially do not happen.
#[test]
#[ignore = "measurement: prints a table, asserts nothing. Run with --nocapture"]
fn report_chain_metrics_by_policy() {
    let matches = survey().len() as f32;
    println!("\n=== Chain metrics by CC policy ({} matches each) ===\n", matches);
    println!(
        "{:<10} {:>14} {:>20} {:>20} {:>14}",
        "policy", "denied_s", "simultaneous_s", "counterplay_free_s", "overlap_%"
    );
    for policy in ["Identity", "Priced"] {
        let (_records, t) = run_survey_with(policy);
        let overlap_pct = 100.0 * t.overlap / t.denied.max(0.001);
        println!(
            "{:<10} {:>14.2} {:>20.2} {:>20.2} {:>13.1}%",
            policy,
            t.denied / matches,
            t.overlap / matches,
            t.counterplay_free / matches,
            overlap_pct
        );
    }
    println!("\nsimultaneous_s is the number step 5 exists to move: seconds per match");
    println!("with >=2 enemies denied at once. If `Priced` does not raise it, the");
    println!("enabling term is not producing chains whatever the win rate says.");
}
