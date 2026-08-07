//! Pre-match and in-fight team banter (graphical only).
//!
//! The team speaks whenever its kill-target call changes — an exchange during
//! the countdown, a corrective beat if the call changes before the gates, and
//! a single-beat shout when it changes mid-fight. Match start counts as a
//! change from nothing, so the opening exchange needs no separate path.
//!
//! Everything here is GRAPHICAL ONLY and registered in `src/states/mod.rs`,
//! never in `add_core_combat_systems`. Line selection and beat timing use a
//! `drip_jitter`-style hash seeded from `GameRng::seed` (read, never drawn
//! from), so no headless baseline can move. See the plan's KTD4-KTD10 in
//! `docs/plans/2026-08-06-001-feat-in-match-kill-call-and-banter-plan.md`.

use std::collections::HashMap;

use bevy::prelude::*;

use super::banter_config::{BanterConfig, BanterContext, BanterExchange};
use super::components::{Combatant, GameRng, MatchCountdown, Pet};
use super::match_config::{CharacterClass, MatchConfig};
use super::utils::spawn_speech_line;

// =============================================================================
// Change detection (KTD4)
// =============================================================================

/// What the watcher last saw for one team's call.
///
/// The `NeverObserved` sentinel is the whole reason this is an enum rather than
/// a bare `Option<usize>`: it distinguishes "this team has no call" (`Seen(None)`)
/// from "we have not looked yet" (`NeverObserved`). Match start is then just a
/// change from the sentinel, so the opening exchange needs no separate
/// match-start code path — the first frame of a match reports a change for both
/// teams and the context derivation turns it into `Opening`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LastSeenCall {
    /// No observation has been made for this team yet this match.
    #[default]
    NeverObserved,
    /// The call as of the last observation (`None` = the call was cleared).
    Seen(Option<usize>),
}

impl LastSeenCall {
    /// Whether the live `current` call differs from what was last seen.
    ///
    /// `NeverObserved` differs from everything, including `None` — that is the
    /// sentinel doing its job, not an accident.
    fn differs_from(&self, current: Option<usize>) -> bool {
        match self {
            LastSeenCall::NeverObserved => true,
            LastSeenCall::Seen(previous) => *previous != current,
        }
    }

    /// The previously-called slot, if there was an observation carrying one.
    ///
    /// Collapses both "never looked" and "explicitly cleared" to `None`, which
    /// is what the `{prev_target}` substitution wants: in either case there is
    /// no previous target to name.
    fn slot(&self) -> Option<usize> {
        match self {
            LastSeenCall::NeverObserved => None,
            LastSeenCall::Seen(previous) => *previous,
        }
    }
}

/// One detected call change, carrying everything the banter layer needs to
/// pick and schedule an exchange without re-reading the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CallChange {
    /// Which team's call moved — `1` or `2`, matching `Combatant::team`.
    pub team: u8,
    /// The call as it now stands (`None` = cleared).
    pub new_call: Option<usize>,
    /// What the watcher had last seen, sentinel included. Kept as the full
    /// `LastSeenCall` rather than flattened to `Option<usize>` so a consumer
    /// can still tell "first call of the match" from "call cleared to nothing"
    /// after the fact.
    pub previous: LastSeenCall,
    /// Gate state at the instant of observation. Retained alongside `context`
    /// because the scheduler's beat pacing keys off whether the countdown is
    /// still running, not only off which exchange pool was selected.
    pub gates_opened: bool,
    /// Which banter pool this change should draw from, derived here so the
    /// rule lives in one place and is unit-testable.
    pub context: BanterContext,
}

/// Maps a change to the exchange pool it should draw from.
///
/// The three contexts are one mechanism seen in three situations, so the rule
/// is deliberately tiny and total:
/// - gates open   -> `Switch`     (a single-beat shout mid-fight)
/// - gates closed, nothing seen before -> `Opening`    (the countdown exchange)
/// - gates closed, a call was replaced -> `Correction` (`{prev_target}` is live)
fn banter_context_for(previous: LastSeenCall, gates_opened: bool) -> BanterContext {
    if gates_opened {
        BanterContext::Switch
    } else if previous == LastSeenCall::NeverObserved {
        BanterContext::Opening
    } else {
        BanterContext::Correction
    }
}

/// Diffs both teams' calls against what was last seen and returns one change
/// per team that moved.
///
/// Pure over plain values so the interesting logic is testable without a Bevy
/// `World` — [`watch_kill_target_calls`] is only the resource plumbing around
/// it. Index 0 of each array is team 1, index 1 is team 2.
///
/// The two teams are diffed independently, so both changing on the same frame
/// yields two changes in team order. An empty return is the overwhelmingly
/// common case and allocates nothing (`Vec::new` defers its allocation to the
/// first push).
fn detect_call_changes(
    last_seen: [LastSeenCall; 2],
    current: [Option<usize>; 2],
    gates_opened: bool,
) -> Vec<CallChange> {
    let mut changes = Vec::new();
    for index in 0..2 {
        let previous = last_seen[index];
        let new_call = current[index];
        if !previous.differs_from(new_call) {
            continue;
        }
        changes.push(CallChange {
            team: index as u8 + 1,
            new_call,
            previous,
            gates_opened,
            context: banter_context_for(previous, gates_opened),
        });
    }
    changes
}

// =============================================================================
// Watcher resource
// =============================================================================

/// The graphical-only watcher: the last-seen call per team plus the changes
/// waiting for the beat scheduler.
///
/// DELIVERY: a drained queue on this resource rather than a Bevy `Event`.
/// Three reasons, in order of weight:
///  1. This codebase has no custom Bevy events at all — cross-system signalling
///     is done with marker components (`CastEnding`, the swing signals) and
///     resource state. A queue matches that idiom; an `add_event` would be the
///     first of its kind for a single producer/single consumer pair.
///  2. `Events<T>` double-buffers with a two-frame lifetime and its own cleanup
///     system that is NOT state-gated, so a change emitted on the last frame of
///     a match could still be readable after the state transition. A queue that
///     `reset_call_watcher_on_exit` clears has no such tail.
///  3. The watcher already owns per-team state that must reset per match, so
///     the queue rides along on a resource that exists anyway.
///
/// [`play_banter_beats`] drains the queue via [`CallWatcher::take_pending`]
/// every frame. The queue is bounded in practice regardless: two entries at
/// match start plus one per operator call change, all discarded on match exit.
#[derive(Resource, Debug, Default)]
pub struct CallWatcher {
    /// Last-seen call, index 0 = team 1, index 1 = team 2.
    last_seen: [LastSeenCall; 2],
    /// Changes detected but not yet consumed, oldest first.
    pending: Vec<CallChange>,
}

impl CallWatcher {
    /// Removes and returns every queued change, leaving the queue empty.
    fn take_pending(&mut self) -> Vec<CallChange> {
        std::mem::take(&mut self.pending)
    }
}

// =============================================================================
// Systems (graphical only — registered in `StatesPlugin::build()`)
// =============================================================================

/// Diffs the live [`MatchConfig`] calls against the watcher each frame and
/// queues any that moved.
///
/// Deliberately NOT `Res<MatchConfig>::is_changed()` (KTD4). A `ResMut` deref
/// marks the resource changed whether or not a field actually moved — the
/// in-match call control takes a `ResMut` to write one team's call and would
/// therefore flag the other team too — and `is_changed()` is true on the first
/// run after insert, which would need its own match-start special case. The
/// explicit diff has neither problem.
///
/// `MatchCountdown` is optional so the system is inert on any frame where the
/// match resources are not yet inserted; skipping the observation entirely
/// (rather than assuming a gate state) means the change is simply reported on
/// the next frame with the real gate state attached.
pub fn watch_kill_target_calls(
    config: Res<MatchConfig>,
    countdown: Option<Res<MatchCountdown>>,
    mut watcher: ResMut<CallWatcher>,
) {
    let Some(countdown) = countdown else {
        return;
    };

    let current = [config.team1_kill_target, config.team2_kill_target];
    let changes = detect_call_changes(watcher.last_seen, current, countdown.gates_opened);

    if changes.is_empty() {
        return;
    }

    watcher.pending.extend(changes);
    watcher.last_seen = [
        LastSeenCall::Seen(current[0]),
        LastSeenCall::Seen(current[1]),
    ];
}

/// Clears the watcher on leaving a match so the next one starts from the
/// sentinel and reports its own opening change.
///
/// This is why the watcher needs no per-match setup in `play_match/mod.rs`:
/// the resource is `init_resource`d once for the app and reset at the state
/// boundary, the same lifecycle `Selection` / `reset_selection_on_exit` uses.
/// Clearing `pending` as well as `last_seen` means an unconsumed change cannot
/// leak into the next match's queue.
pub fn reset_call_watcher_on_exit(mut watcher: ResMut<CallWatcher>) {
    *watcher = CallWatcher::default();
}

// =============================================================================
// Exchange resolution (KTD7, KTD8, KTD10)
// =============================================================================
//
// Everything from here to the scheduler boundary is a PURE FUNCTION over plain
// data: no `World`, no `Commands`, no `Res`. [`resolve_exchange`] takes a
// lineup, the called target, a context and a seed, and returns beats already
// bound to combatants with their text substituted and their start times
// derived. The scheduler below owns the Bevy plumbing that gathers those
// inputs and emits the bubbles; none of the interesting logic needs an app to
// test.
//
// The pipeline is filter -> weight -> pick -> bind -> substitute:
//
//  1. FILTER   drop exchanges whose context differs, whose target constraint
//              the call does not satisfy, or whose roles cannot all bind to
//              distinct living combatants. Unsatisfiable never reaches the
//              weighting step — which is why a 1v1 team is silent for any
//              two-speaker exchange with no special case (AE1).
//  2. WEIGHT   `specificity_weight ^ (non-`Any` constraint count)`. A WEIGHT,
//              not a filter (KTD8) — see [`exchange_weight`].
//  3. PICK     a weighted draw off a hash of (seed, team, context, occurrence),
//              never `GameRng` (KTD7, R15).
//  4. BIND     constrained roles first, then unconstrained in slot order.
//  5. SUBSTITUTE `{target}` everywhere, `{prev_target}` in `Correction` only.

/// Selection seed used when `GameRng::seed` is `None`.
///
/// The graphical client always records a seed (`GameRng::default()` routes
/// through `from_os_rng()`, which draws one and stores it), so this is purely
/// defensive: with no seed the banter is simply the same every run rather than
/// the resolver panicking or silently going quiet.
const BANTER_FALLBACK_SEED: u64 = 0x4B41_4C4C_4341_4C4C;

/// Initial hash state. Deliberately DIFFERENT from [`BANTER_FALLBACK_SEED`] —
/// [`mix`] starts with an xor, so folding a value into an identical state would
/// zero the accumulator and collapse the seedless case to a fixed roll of 0.
const BANTER_HASH_INIT: u64 = 0xA076_1D64_78BD_642F;

/// What `{target}` / `{prev_target}` render as when the value does not exist.
///
/// Three situations reach this: the call was cleared to `None`, the previous
/// call was nothing (match start), or `{prev_target}` appears outside
/// `Correction` where it is deliberately unavailable. The alternative —
/// leaving the raw placeholder in the string — would put a literal
/// `"{target} dies first."` in a speech bubble, so a neutral pronoun is the
/// sane failure mode. Authors are still expected to keep `{prev_target}` to
/// `Correction` entries; this only stops a mistake looking like a crash.
const UNRESOLVED_TARGET: &str = "them";

/// One combatant as the resolver sees them. Plain data, owned, no `World`.
///
/// Pets are NOT part of a lineup — they have no voice and no slot in the
/// pre-match roster — so the caller filters them out when building one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BanterCombatant {
    /// Who speaks. Carried through to the resolved beat so the scheduler can
    /// hang a bubble on them without re-deriving the binding.
    pub entity: Entity,
    /// Drives both the class constraints and the `{target}` substitution — the
    /// repo has no per-combatant names (`assets/config/characters.ron` carries
    /// class-level names only), so class is the only handle banter has.
    pub class: CharacterClass,
    /// Dead combatants stay in the lineup so slot order is stable, but are
    /// never bound as speakers.
    pub alive: bool,
}

/// The speaking team, in slot order.
///
/// Slot order is the roster order, and it is load-bearing: unconstrained roles
/// fill from it, so a given lineup and exchange always bind the same way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct BanterLineup {
    /// `1` or `2`, matching `Combatant::team`. Hashed into selection so the two
    /// teams do not tell the same joke in the same match.
    pub team: u8,
    /// The team's primary combatants in slot order, pets excluded.
    pub allies: Vec<BanterCombatant>,
}

/// The called target and the one it replaced, reduced to what banter can use.
///
/// Slot indices are resolved to classes by the caller, because that is all the
/// resolver needs: the target class drives the `target` constraint and both
/// substitutions. A `None` target means the call was cleared (or pointed at a
/// slot that no longer exists), which satisfies `Any` and nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BanterCall {
    /// Class of the newly-called combatant.
    pub target: Option<CharacterClass>,
    /// Class of the combatant the call replaced. Only ever substituted in
    /// `Correction`.
    pub prev_target: Option<CharacterClass>,
}

/// One beat, ready to spawn: who says it, what it says, and when.
#[derive(Clone, Debug, PartialEq)]
struct ResolvedBeat {
    /// The combatant bound to this beat's role.
    pub speaker: Entity,
    /// Final text — every placeholder already substituted.
    pub text: String,
    /// Seconds after the call change at which this beat speaks, derived from
    /// the config's pacing block (`BanterTiming::beat_start`). Never authored.
    pub start: f32,
}

/// A picked, bound, substituted exchange — everything the scheduler needs.
#[derive(Clone, Debug, PartialEq)]
struct ResolvedExchange {
    /// Which pool this came from. Retained so the scheduler can log and reason
    /// about a queued exchange without carrying the change alongside it.
    pub context: BanterContext,
    /// Beats in play order, start times ascending.
    pub beats: Vec<ResolvedBeat>,
    /// Bubble lifetime from the timing block, copied here so the scheduler can
    /// call `spawn_speech_line` from the resolved value alone.
    pub lifetime: f32,
}

/// Filter, weight, pick, bind and substitute — the whole resolver.
///
/// Returns `None` when the pool has nothing this lineup can play: an empty
/// context pool, or every entry unsatisfiable. That is a normal outcome (a
/// solo team can play no two-speaker exchange), not an error, so the caller
/// simply schedules nothing.
///
/// `occurrence` is a per-team counter the caller increments on each call
/// change, so a team that has its call corrected three times does not tell the
/// same joke three times. `seed` is `GameRng::seed` READ, never drawn from
/// (KTD7) — reading a public field cannot advance the generator, so replay
/// byte-identity is safe by construction.
fn resolve_exchange(
    config: &BanterConfig,
    lineup: &BanterLineup,
    call: BanterCall,
    context: BanterContext,
    seed: Option<u64>,
    occurrence: u32,
) -> Option<ResolvedExchange> {
    // --- 1. Filter: keep only what this lineup can actually play -----------
    // Binding happens here rather than after the pick because satisfiability
    // IS whether the roles bind. Doing it once and keeping the result avoids
    // binding twice for the winner.
    let candidates: Vec<(&BanterExchange, Vec<Entity>)> = config
        .exchanges_for(context)
        // An exchange with no beats says nothing; treating it as unsatisfiable
        // keeps "resolved" and "audible" the same thing for the scheduler.
        .filter(|exchange| !exchange.beats.is_empty())
        // Validation already rejects a beat naming an undeclared role, so this
        // only ever bites a hand-built `BanterConfig`. Dropping the exchange
        // here rather than trusting the loader is what lets the binding lookup
        // below index without a bounds check.
        .filter(|exchange| exchange.beats.iter().all(|beat| declares(exchange, &beat.role)))
        .filter(|exchange| target_satisfies(exchange, call))
        .filter_map(|exchange| Some((exchange, bind_roles(exchange, lineup)?)))
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // --- 2. Weight, then 3. pick -------------------------------------------
    let weights: Vec<f32> = candidates
        .iter()
        .map(|(exchange, _)| exchange_weight(exchange, config.timing.specificity_weight))
        .collect();
    let roll = banter_roll(seed, lineup.team, context, occurrence);
    let (exchange, bound) = &candidates[weighted_pick(&weights, roll)];

    // --- 4. Bind (already done) + 5. substitute ----------------------------
    let beats = exchange
        .beats
        .iter()
        .enumerate()
        .map(|(index, beat)| ResolvedBeat {
            // Cannot miss: the filter above dropped any exchange with a beat on
            // an undeclared role, and `bind_roles` binds every declared one.
            speaker: bound[role_index(exchange, &beat.role)],
            text: render_line(&beat.text, call, context),
            start: config.timing.beat_start(context, index),
        })
        .collect();

    Some(ResolvedExchange {
        context,
        beats,
        lifetime: config.timing.line_lifetime,
    })
}

/// Whether the call satisfies the exchange's target constraint.
///
/// A specific constraint needs a target to constrain, so a cleared call
/// (`None`) satisfies `Any` and nothing else.
fn target_satisfies(exchange: &BanterExchange, call: BanterCall) -> bool {
    match call.target {
        Some(class) => exchange.target.is_satisfied_by(class),
        None => !exchange.target.is_specific(),
    }
}

/// Bind every declared role to a distinct living combatant, or `None` if that
/// is impossible. Returned in `exchange.speakers` order.
///
/// CONSTRAINED ROLES BIND FIRST. An exchange declaring `responder: Priest` in
/// a Warrior+Priest lineup must put the Priest on `responder` even though slot
/// order would hand it the Warrior — so the class filter runs before the
/// slot-order fill, not alongside it.
///
/// Greedy first-fit is optimal here, which is not true of bipartite matching in
/// general: every constraint is a class EQUALITY, so two constrained roles have
/// either identical or disjoint candidate sets. Same-class roles only ever
/// compete with each other and greedy hands them distinct combatants until it
/// runs out; disjoint roles cannot steal from each other at all. Unconstrained
/// roles accept anyone, so they can only be made harder by binding early —
/// hence last.
///
/// Roles with no beat still bind: declaring a role IS the way to say "only
/// offer this exchange when the lineup has one of these", whether or not they
/// speak. Distinctness applies to them too, which is what makes an exchange
/// needing two Priests unsatisfiable for a team with one.
fn bind_roles(exchange: &BanterExchange, lineup: &BanterLineup) -> Option<Vec<Entity>> {
    let mut bound: Vec<Option<Entity>> = vec![None; exchange.speakers.len()];
    let mut taken: Vec<Entity> = Vec::with_capacity(exchange.speakers.len());

    // Pass 1: class-constrained roles. Pass 2: the rest, in slot order.
    for constrained in [true, false] {
        for (index, speaker) in exchange.speakers.iter().enumerate() {
            if speaker.class.is_specific() != constrained {
                continue;
            }
            let pick = lineup
                .allies
                .iter()
                .find(|ally| {
                    ally.alive
                        && speaker.class.is_satisfied_by(ally.class)
                        && !taken.contains(&ally.entity)
                })?
                .entity;
            taken.push(pick);
            bound[index] = Some(pick);
        }
    }

    // Every slot is filled or we returned early via `?` above.
    bound.into_iter().collect()
}

/// Whether `role` is declared in the exchange's `speakers` list.
fn declares(exchange: &BanterExchange, role: &str) -> bool {
    exchange.speakers.iter().any(|speaker| speaker.role == role)
}

/// Index of `role` within the exchange's declared speakers.
///
/// Only called on exchanges the candidate filter already proved declare every
/// role their beats name, so the `unwrap_or` is unreachable — it is there so a
/// future caller that skips the filter degrades into a wrong speaker rather
/// than taking the client down over a cosmetic line.
fn role_index(exchange: &BanterExchange, role: &str) -> usize {
    exchange
        .speakers
        .iter()
        .position(|speaker| speaker.role == role)
        .unwrap_or(0)
}

/// Selection weight: `specificity_weight` once per non-`Any` constraint (KTD8).
///
/// This is the whole reason specificity is a weight rather than a filter.
/// Most-specific-wins would let one bespoke Priest-and-Warrior joke crowd out
/// every generic for that comp, forever — the exact repetition the pool is
/// meant to avoid. Weighting favours the bespoke line without ever removing
/// the generics from play.
fn exchange_weight(exchange: &BanterExchange, specificity_weight: f32) -> f32 {
    specificity_weight.powi(exchange.specificity() as i32)
}

/// Pick an index by walking the cumulative weights against `roll` in `[0, 1)`.
///
/// The trailing `weights.len() - 1` is the float-rounding guard: with `roll`
/// arbitrarily close to `1.0` the cumulative sum can land a hair above
/// `roll * total` only on the last step, or miss it entirely. A non-positive
/// total (only reachable from a degenerate `specificity_weight`) takes index 0.
/// Never called with an empty slice — the caller returns early on an empty
/// candidate list.
fn weighted_pick(weights: &[f32], roll: f32) -> usize {
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return 0;
    }
    let mut cursor = roll * total;
    for (index, weight) in weights.iter().enumerate() {
        cursor -= weight;
        if cursor < 0.0 {
            return index;
        }
    }
    weights.len() - 1
}

/// Fold one value into a running hash.
///
/// The same job `drip_jitter` in `rendering/effects.rs` does — a cheap
/// deterministic mix that deliberately never touches `GameRng` — widened to 64
/// bits so a full `GameRng::seed` mixes in without being truncated first, and
/// given a proper avalanche because this drives a weighted pool draw rather
/// than a particle offset where a weak low bit would not show.
fn mix(state: u64, value: u64) -> u64 {
    let mut s = state ^ value;
    s = s.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    s ^= s >> 29;
    s = s.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    s ^ (s >> 32)
}

/// A stable per-context salt. Written out rather than derived from the enum's
/// discriminant so reordering `BanterContext` cannot silently reshuffle every
/// match's banter.
fn context_salt(context: BanterContext) -> u64 {
    match context {
        BanterContext::Opening => 0x0B1E,
        BanterContext::Correction => 0x0C02,
        BanterContext::Switch => 0x0503,
    }
}

/// The selection roll in `[0, 1)` for one (seed, team, context, occurrence).
///
/// Hashing the SEED is what makes banter vary per match while a replay stays
/// identical (KTD7) — hashing the lineup alone would have the same comp tell
/// the same joke forever. Reading `GameRng::seed` is a public-field read, so it
/// cannot advance draw order and no headless baseline can move (R15, R18).
fn banter_roll(
    seed: Option<u64>,
    team: u8,
    context: BanterContext,
    occurrence: u32,
) -> f32 {
    let mut hash = mix(BANTER_HASH_INIT, seed.unwrap_or(BANTER_FALLBACK_SEED));
    hash = mix(hash, u64::from(team));
    hash = mix(hash, context_salt(context));
    hash = mix(hash, u64::from(occurrence));
    // 24 bits is far more resolution than a pool of a few dozen needs, and
    // matches `drip_jitter`'s "take a slice off the top, divide by its range".
    ((hash >> 16) & 0xFF_FFFF) as f32 / 16_777_216.0
}

/// Substitute `{target}` and `{prev_target}` into one beat's text.
///
/// `{prev_target}` resolves in `Correction` ONLY — it is the one context where
/// a previous call exists as a thing worth naming. Elsewhere it falls back with
/// everything else rather than leaking a literal brace into a bubble; see
/// [`UNRESOLVED_TARGET`].
fn render_line(text: &str, call: BanterCall, context: BanterContext) -> String {
    let prev = match context {
        BanterContext::Correction => call.prev_target,
        BanterContext::Opening | BanterContext::Switch => None,
    };
    text.replace("{prev_target}", class_word(prev))
        .replace("{target}", class_word(call.target))
}

/// A class's display name, or the fallback when there is nothing to name.
fn class_word(class: Option<CharacterClass>) -> &'static str {
    class.map_or(UNRESOLVED_TARGET, |class| class.name())
}

// =============================================================================
// Beat scheduling and emission (KTD9)
// =============================================================================
//
// A resolved exchange is a list of beats with start times RELATIVE to the call
// change. The scheduler turns those into absolute times on its own clock,
// holds them per team, and spawns a bubble as each falls due.
//
// Three rules shape the design, all of them consequences of how bubbles draw:
//
//  1. ONE LIVE BUBBLE PER SPEAKER. `render_speech_bubbles` projects every
//     bubble to a fixed offset above its owner with no per-owner stacking or
//     dedup, so two concurrent bubbles on one combatant draw on top of each
//     other and neither is readable. `BanterConfig::validate()` enforces the gap WITHIN
//     an exchange; the scheduler enforces it ACROSS exchanges, where the
//     config cannot see the collision coming (see [`BanterScheduler::take_due`]).
//  2. A CORRECTION CANCELS THE UNPLAYED BEATS (KTD9). Letting the opening
//     exchange finish would talk over the correction with lines about a target
//     that is no longer called — the exact confusion the correction exists to
//     resolve.
//  3. LIVENESS IS CHECKED AT EMISSION, NOT RESOLUTION. Resolution binds
//     speakers who were alive when the call changed; a beat five seconds later
//     can belong to a corpse. A dead speaker's beat is dropped, and the rest of
//     the exchange plays on.
//
// The clock is `Res<Time>`, which in `Update` follows `Time<Virtual>` — the
// same clock the sim-speed control scales and the pause button stops. Banter
// therefore paces with the match rather than with wall time.

/// One beat waiting for its moment, in scheduler-clock terms.
#[derive(Clone, Debug, PartialEq)]
struct PendingBeat {
    /// Who says it. Bound at resolution; re-checked for liveness at emission.
    pub speaker: Entity,
    /// Final text — placeholders already substituted by the resolver.
    pub text: String,
    /// Absolute time on [`BanterScheduler::clock`] at which this beat speaks.
    /// Deferral (see [`BanterScheduler::take_due`]) is the only thing that
    /// moves it after queueing, and only ever forward.
    pub at: f32,
    /// Bubble lifetime, copied off the resolved exchange so emission needs
    /// nothing but the beat.
    pub lifetime: f32,
}

/// The graphical-only beat queues: one per team, plus the bookkeeping that
/// keeps two bubbles off one combatant.
///
/// Reset wholesale on leaving a match by [`reset_banter_scheduler_on_exit`],
/// the same lifecycle `CallWatcher` uses — so the clock restarts at zero and
/// no beat can survive into the next match.
#[derive(Resource, Debug, Default)]
pub struct BanterScheduler {
    /// Seconds since this match's scheduler started. Absolute beat times are
    /// on this clock, so it must never run backwards within a match.
    clock: f32,
    /// Unplayed beats, index 0 = team 1, index 1 = team 2, each ascending in
    /// `at`. The two teams schedule independently: a correction on one side
    /// never touches the other's queue.
    queues: [Vec<PendingBeat>; 2],
    /// When each speaker's live bubble expires, on the same clock. The
    /// cross-exchange half of the one-bubble-per-speaker rule. Entries are
    /// never pruned: at most one per combatant, and the whole resource is
    /// dropped on match exit.
    speaking_until: HashMap<Entity, f32>,
    /// Per-team count of resolutions so far this match, fed to
    /// `resolve_exchange` so a team corrected three times does not tell the
    /// same joke three times.
    occurrence: [u32; 2],
}

/// Queue index for a team number (`1` or `2`).
///
/// Anything that is not team 2 maps to team 1's queue rather than panicking —
/// a malformed team number is a cosmetic misfile, not a reason to take the
/// client down mid-match.
fn team_index(team: u8) -> usize {
    usize::from(team == 2)
}

impl BanterScheduler {
    /// Advance the clock by one frame's (virtual) delta.
    fn advance(&mut self, delta: f32) {
        self.clock += delta;
    }

    /// Consume and return this team's occurrence counter, incrementing it.
    ///
    /// Incremented per CALL CHANGE, not per successful resolution: a change
    /// whose pool came up empty still moves the counter, so the next change
    /// does not land on the roll the silent one would have used.
    fn next_occurrence(&mut self, team: u8) -> u32 {
        let slot = &mut self.occurrence[team_index(team)];
        let occurrence = *slot;
        *slot = slot.saturating_add(1);
        occurrence
    }

    /// Drop this team's unplayed beats (KTD9).
    ///
    /// Called on EVERY change, including one that resolves to nothing. What is
    /// queued is dialogue about a target that is no longer called, so it is
    /// stale whether or not there is anything to replace it with. Already-
    /// emitted bubbles are untouched — they are live entities on their own
    /// lifetime timer, and yanking them would blink text off mid-read.
    fn cancel_team(&mut self, team: u8) {
        self.queues[team_index(team)].clear();
    }

    /// Queue a resolved exchange's beats at `clock + beat.start`.
    ///
    /// Does NOT cancel on its own — [`cancel_team`](Self::cancel_team) is a
    /// separate call because a change that resolves to nothing must still
    /// cancel.
    fn queue_exchange(&mut self, team: u8, exchange: &ResolvedExchange) {
        let queue = &mut self.queues[team_index(team)];
        let now = self.clock;
        queue.extend(exchange.beats.iter().map(|beat| PendingBeat {
            speaker: beat.speaker,
            text: beat.text.clone(),
            at: now + beat.start,
            lifetime: exchange.lifetime,
        }));
    }

    /// Remove and return every beat due at the current clock, in play order.
    ///
    /// `is_alive` is asked per beat at EMISSION time — a speaker bound five
    /// seconds ago may since have died, and a corpse must not talk. Its beat is
    /// dropped and the exchange carries on with the next one; the alternative
    /// (dropping the rest of the exchange too) would silence a survivor's reply
    /// because their partner fell.
    ///
    /// Each team's queue is walked in order and STOPS at the first beat that
    /// cannot speak yet. That is what keeps an exchange in sequence: if beat 0
    /// is deferred behind a live bubble, beat 1 waits behind it rather than
    /// jumping the queue and delivering the punchline first.
    ///
    /// DEFERRAL, not dropping, is how a cross-exchange collision is resolved: a
    /// beat whose speaker still has a bubble up has its `at` pushed to the
    /// moment that bubble expires. The alternative — dropping it — would eat
    /// the first line of a correction precisely when the operator most wants to
    /// be told what changed. The push is bounded by one `line_lifetime` (the
    /// blocking bubble was spawned at most that long ago), and it cannot
    /// cascade past one step, because `BanterConfig::validate()` already keeps same-role
    /// beats within an exchange at least `line_lifetime` apart.
    fn take_due(&mut self, is_alive: impl Fn(Entity) -> bool) -> Vec<PendingBeat> {
        let mut due: Vec<PendingBeat> = Vec::new();

        for index in 0..self.queues.len() {
            loop {
                // Queues hold at most a handful of beats, so the front-removal
                // cost of a `Vec` is not worth a `VecDeque`'s extra type noise.
                let Some(beat) = self.queues[index].first() else {
                    break;
                };
                if beat.at > self.clock {
                    break;
                }
                if !is_alive(beat.speaker) {
                    self.queues[index].remove(0);
                    continue;
                }
                let free_at = self
                    .speaking_until
                    .get(&beat.speaker)
                    .copied()
                    .unwrap_or(f32::NEG_INFINITY);
                if self.clock < free_at {
                    self.queues[index][0].at = free_at;
                    break;
                }
                let beat = self.queues[index].remove(0);
                self.speaking_until
                    .insert(beat.speaker, self.clock + beat.lifetime);
                due.push(beat);
            }
        }

        due
    }
}

/// Both teams' primary combatants in slot order, index 0 = team 1.
///
/// Pets are excluded by the query filter, not by a slot-number test: a call
/// index addresses PRIMARY combatants only (`acquire_targets` builds the same
/// pet-filtered list before indexing `teamN_kill_target`), and a pet in the
/// list would shift every index past it. Dead combatants are kept so positions
/// stay stable — `resolve_exchange` refuses to bind them as speakers, and
/// `class_at` still needs a dead target's class to substitute.
///
/// Private, and it stays private: a `pub fn` taking a `Query` is exactly what
/// `tests/registration_audit.rs` flags as an unregistered system.
fn team_rosters(combatants: &Query<(Entity, &Combatant), Without<Pet>>) -> [Vec<BanterCombatant>; 2] {
    let mut by_slot: [Vec<(u8, BanterCombatant)>; 2] = Default::default();
    for (entity, combatant) in combatants.iter() {
        // The DEAD are excluded, not merely flagged, because this list is
        // indexed by a call slot. `acquire_targets` builds `enemy_primary` by
        // skipping the dead first and pets second, so that list compacts as
        // combatants fall; a roster that kept the dead would name a different
        // class in `{target}` than the AI is actually attacking. Speaker
        // binding is unaffected — it only ever bound the living anyway.
        if !combatant.is_alive() {
            continue;
        }
        by_slot[team_index(combatant.team)].push((
            combatant.slot,
            BanterCombatant {
                entity,
                class: combatant.class,
                alive: true,
            },
        ));
    }
    by_slot.map(|mut roster| {
        roster.sort_by_key(|(slot, _)| *slot);
        roster.into_iter().map(|(_, combatant)| combatant).collect()
    })
}

/// Class of the combatant at call index `slot` of `roster`.
///
/// `None` for a cleared call or an index past the end of the roster (a call at
/// a slot the comp does not have). Either way the resolver treats it as "no
/// target to name", which satisfies `Any` and nothing else.
fn class_at(roster: &[BanterCombatant], slot: Option<usize>) -> Option<CharacterClass> {
    roster.get(slot?).map(|combatant| combatant.class)
}

/// Drains the watcher, resolves an exchange per change, and spawns each beat's
/// bubble as it falls due.
///
/// The whole banter pipeline's Bevy half. Everything interesting it calls —
/// [`resolve_exchange`] and the [`BanterScheduler`] methods — is pure over
/// plain data, so this function is only the plumbing that gathers inputs.
///
/// GRAPHICAL ONLY: registered in `StatesPlugin::build()` behind
/// `in_state(GameState::PlayMatch)` and never in `add_core_combat_systems`. It
/// writes nothing but `SpeechBubble` entities, which no sim system reads.
///
/// LATE CORRECTIONS MAY LAND AFTER THE GATES, and that is accepted. `latest_beat`
/// (9.0s) bounds a beat's offset within an exchange, but a correction made at
/// t=9.5 in the 10s countdown schedules its first beat at ~11.5s — a second or
/// so into the fight. Suppressing it would silently swallow a call the operator
/// just made; a beat landing just after the gates open reads fine, and banter
/// bubbles render in both gate states by design (`bubble_visible`).
pub fn play_banter_beats(
    mut commands: Commands,
    time: Res<Time>,
    banter_config: Option<Res<BanterConfig>>,
    rng: Option<Res<GameRng>>,
    mut watcher: ResMut<CallWatcher>,
    mut scheduler: ResMut<BanterScheduler>,
    combatants: Query<(Entity, &Combatant), Without<Pet>>,
) {
    let Some(banter_config) = banter_config else {
        // `BanterConfigPlugin` registers in `src/main.rs` only (KTD5), so this
        // is unreachable in the client and reachable in any app that skips it.
        // Drain rather than return so the watcher's queue cannot grow across a
        // whole match with nobody consuming it.
        watcher.take_pending();
        return;
    };

    scheduler.advance(time.delta_secs());

    let changes = watcher.take_pending();
    if !changes.is_empty() {
        // Built once per frame that has changes, not once per change: both
        // teams' rosters come out of the same query pass.
        let rosters = team_rosters(&combatants);
        // Read, never drawn from (KTD7) — a public-field read cannot advance
        // the generator, which is what keeps replays byte-identical.
        let seed = rng.as_ref().and_then(|rng| rng.seed);

        for change in changes {
            let speaking = team_index(change.team);
            // A team's call names a slot on the OPPOSING side.
            let enemies = &rosters[1 - speaking];
            let call = BanterCall {
                target: class_at(enemies, change.new_call),
                prev_target: class_at(enemies, change.previous.slot()),
            };
            let lineup = BanterLineup {
                team: change.team,
                allies: rosters[speaking].clone(),
            };
            let occurrence = scheduler.next_occurrence(change.team);

            // KTD9: cancel first, unconditionally — see `cancel_team`.
            scheduler.cancel_team(change.team);

            // A call cleared to nothing cancels the stale dialogue and stops
            // there. There is no subject to speak about, and every line in the
            // pool names one: resolving anyway would substitute the
            // `UNRESOLVED_TARGET` fallback into text written around a class
            // name and put "the them dies first" in a bubble.
            if call.target.is_none() {
                continue;
            }

            if let Some(resolved) = resolve_exchange(
                &banter_config,
                &lineup,
                call,
                change.context,
                seed,
                occurrence,
            ) {
                scheduler.queue_exchange(change.team, &resolved);
            }
        }
    }

    // A despawned entity fails the `get` and reads as dead, which is the right
    // answer for a beat whose speaker is gone.
    let due = scheduler.take_due(|entity| {
        combatants
            .get(entity)
            .is_ok_and(|(_, combatant)| combatant.is_alive())
    });
    for beat in due {
        spawn_speech_line(&mut commands, beat.speaker, beat.text, beat.lifetime);
    }
}

/// Clears the scheduler on leaving a match so no beat, clock offset, or
/// occurrence count survives into the next one.
///
/// Same lifecycle as [`reset_call_watcher_on_exit`]: the resource is
/// `init_resource`d once for the app and reset at the state boundary, so
/// `play_match/mod.rs` needs no per-match insert/remove pair. Resetting the
/// CLOCK matters as much as the queues — absolute beat times are relative to
/// it, so a carried-over clock would make the next match's first exchange
/// arrive instantly.
pub fn reset_banter_scheduler_on_exit(mut scheduler: ResMut<BanterScheduler>) {
    *scheduler = BanterScheduler::default();
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Both teams unobserved — the state a freshly-reset watcher is in.
    const FRESH: [LastSeenCall; 2] = [LastSeenCall::NeverObserved; 2];

    #[test]
    fn first_observation_reports_a_change_from_nothing_for_each_team() {
        // The shipped defaults: both teams call enemy slot 0.
        let changes = detect_call_changes(FRESH, [Some(0), Some(0)], false);

        assert_eq!(changes.len(), 2, "both teams should report their first call");
        assert_eq!(changes[0].team, 1);
        assert_eq!(changes[1].team, 2);
        for change in &changes {
            assert_eq!(change.previous, LastSeenCall::NeverObserved);
            assert_eq!(change.new_call, Some(0));
            assert_eq!(change.context, BanterContext::Opening);
        }
    }

    #[test]
    fn unchanged_calls_report_nothing() {
        let seen = [
            LastSeenCall::Seen(Some(0)),
            LastSeenCall::Seen(Some(1)),
        ];
        assert!(detect_call_changes(seen, [Some(0), Some(1)], false).is_empty());
        // Gate state alone must not manufacture a change — the gates opening
        // mid-match is not itself a call change.
        assert!(detect_call_changes(seen, [Some(0), Some(1)], true).is_empty());
    }

    #[test]
    fn a_changed_call_reports_both_new_and_previous() {
        let seen = [LastSeenCall::Seen(Some(0)), LastSeenCall::Seen(Some(0))];
        let changes = detect_call_changes(seen, [Some(2), Some(0)], false);

        assert_eq!(changes.len(), 1, "only team 1 moved");
        let change = changes[0];
        assert_eq!(change.team, 1);
        assert_eq!(change.new_call, Some(2));
        assert_eq!(change.previous, LastSeenCall::Seen(Some(0)));
        assert_eq!(change.previous.slot(), Some(0));
    }

    #[test]
    fn both_teams_changing_on_one_frame_report_two_independent_changes() {
        let seen = [LastSeenCall::Seen(Some(0)), LastSeenCall::Seen(Some(0))];
        let changes = detect_call_changes(seen, [Some(1), Some(2)], true);

        assert_eq!(changes.len(), 2);
        assert_eq!((changes[0].team, changes[0].new_call), (1, Some(1)));
        assert_eq!((changes[1].team, changes[1].new_call), (2, Some(2)));
        // Independent means each carries its own previous value, not a shared one.
        assert_eq!(changes[0].previous, LastSeenCall::Seen(Some(0)));
        assert_eq!(changes[1].previous, LastSeenCall::Seen(Some(0)));
    }

    #[test]
    fn reported_gate_state_matches_the_countdown_at_observation_time() {
        let seen = [LastSeenCall::Seen(Some(0)), LastSeenCall::Seen(Some(0))];

        let pre_gate = detect_call_changes(seen, [Some(1), Some(0)], false);
        assert!(!pre_gate[0].gates_opened);

        let post_gate = detect_call_changes(seen, [Some(1), Some(0)], true);
        assert!(post_gate[0].gates_opened);
    }

    #[test]
    fn a_call_cleared_to_none_is_reported_as_a_change() {
        let seen = [LastSeenCall::Seen(Some(1)), LastSeenCall::Seen(Some(0))];
        let changes = detect_call_changes(seen, [None, Some(0)], false);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].new_call, None);
        assert_eq!(changes[0].previous, LastSeenCall::Seen(Some(1)));

        // ...and clearing an already-cleared call is not a change.
        let cleared = [LastSeenCall::Seen(None), LastSeenCall::Seen(Some(0))];
        assert!(detect_call_changes(cleared, [None, Some(0)], false).is_empty());
    }

    #[test]
    fn context_is_opening_correction_or_switch_in_the_three_situations() {
        // Gates closed, nothing seen yet: the countdown exchange.
        assert_eq!(
            banter_context_for(LastSeenCall::NeverObserved, false),
            BanterContext::Opening
        );
        // Gates closed, a call already existed: the operator corrected it.
        assert_eq!(
            banter_context_for(LastSeenCall::Seen(Some(0)), false),
            BanterContext::Correction
        );
        // Gates open: a mid-fight shout, regardless of what came before.
        assert_eq!(
            banter_context_for(LastSeenCall::Seen(Some(0)), true),
            BanterContext::Switch
        );
        assert_eq!(
            banter_context_for(LastSeenCall::NeverObserved, true),
            BanterContext::Switch
        );
    }

    #[test]
    fn never_observed_is_distinct_from_an_explicitly_cleared_call() {
        // The sentinel's whole purpose: `Seen(None)` and `NeverObserved` are
        // both "no slot", but only one of them counts as a change against None.
        assert!(LastSeenCall::NeverObserved.differs_from(None));
        assert!(!LastSeenCall::Seen(None).differs_from(None));
        assert_eq!(LastSeenCall::NeverObserved.slot(), None);
        assert_eq!(LastSeenCall::Seen(None).slot(), None);
    }

    #[test]
    fn take_pending_drains_the_queue() {
        let mut watcher = CallWatcher::default();
        watcher.pending = detect_call_changes(FRESH, [Some(0), Some(0)], false);

        let drained = watcher.take_pending();
        assert_eq!(drained.len(), 2);
        assert!(watcher.pending.is_empty(), "a second consumer sees nothing");
        assert!(watcher.take_pending().is_empty());
    }

    // -------------------------------------------------------------------
    // Resource plumbing — the system around the pure function. These build a
    // minimal `World` rather than a full app: the system takes three resources
    // and no queries, so nothing else is needed to exercise it.
    // -------------------------------------------------------------------

    fn run_watcher(world: &mut World) {
        let mut system = IntoSystem::into_system(watch_kill_target_calls);
        system.initialize(world);
        system.run((), world);
    }

    fn world_with(kill_targets: (Option<usize>, Option<usize>), gates_opened: bool) -> World {
        let mut world = World::new();
        let mut config = MatchConfig::default();
        config.team1_kill_target = kill_targets.0;
        config.team2_kill_target = kill_targets.1;
        world.insert_resource(config);
        world.insert_resource(MatchCountdown {
            time_remaining: if gates_opened { 0.0 } else { 10.0 },
            gates_opened,
        });
        world.insert_resource(CallWatcher::default());
        world
    }

    #[test]
    fn the_system_queues_the_opening_change_then_goes_quiet() {
        let mut world = world_with((Some(0), Some(0)), false);

        run_watcher(&mut world);
        assert_eq!(world.resource::<CallWatcher>().pending.len(), 2);

        // Second frame with nothing moved must add nothing.
        run_watcher(&mut world);
        assert_eq!(world.resource::<CallWatcher>().pending.len(), 2);
        assert_eq!(
            world.resource::<CallWatcher>().last_seen,
            [LastSeenCall::Seen(Some(0)), LastSeenCall::Seen(Some(0))]
        );
    }

    #[test]
    fn the_system_picks_up_a_mid_fight_write_to_match_config() {
        let mut world = world_with((Some(0), Some(0)), true);
        run_watcher(&mut world);
        world.resource_mut::<CallWatcher>().take_pending();

        // The in-match control's write: one team only.
        world.resource_mut::<MatchConfig>().team1_kill_target = Some(1);
        run_watcher(&mut world);

        let pending = world.resource_mut::<CallWatcher>().take_pending();
        assert_eq!(pending.len(), 1, "a ResMut deref must not flag team 2");
        assert_eq!(pending[0].team, 1);
        assert_eq!(pending[0].context, BanterContext::Switch);
        assert!(pending[0].gates_opened);
    }

    #[test]
    fn the_system_is_inert_without_a_countdown() {
        // No `MatchCountdown` yet: observing now would burn the first-observation
        // change against an unknown gate state, so the system must skip entirely.
        let mut world = World::new();
        world.insert_resource(MatchConfig::default());
        world.insert_resource(CallWatcher::default());

        run_watcher(&mut world);

        let watcher = world.resource::<CallWatcher>();
        assert!(watcher.pending.is_empty());
        assert_eq!(watcher.last_seen, FRESH, "nothing observed, nothing recorded");
    }

    #[test]
    fn reset_clears_last_seen_and_pending_so_state_cannot_leak_between_matches() {
        let mut world = world_with((Some(0), Some(0)), false);
        run_watcher(&mut world);
        assert!(!world.resource::<CallWatcher>().pending.is_empty());

        let mut system = IntoSystem::into_system(reset_call_watcher_on_exit);
        system.initialize(&mut world);
        system.run((), &mut world);

        let watcher = world.resource::<CallWatcher>();
        assert_eq!(watcher.last_seen, FRESH);
        assert!(watcher.pending.is_empty());

        // The next match therefore reports its own Opening change.
        run_watcher(&mut world);
        let pending = &world.resource::<CallWatcher>().pending;
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|c| c.context == BanterContext::Opening));
    }

    // -------------------------------------------------------------------
    // Exchange resolution
    //
    // All of this is pure over plain data, so the fixtures below are hand-built
    // `BanterConfig`s rather than the shipped `banter.ron` — a content edit must
    // never be able to fail a resolver test.
    // -------------------------------------------------------------------

    use super::super::banter_config::{
        BanterBeat, BanterExchange, BanterSpeaker, BanterTiming, ClassConstraint,
    };

    /// Seed used wherever a test needs *a* seed and does not care which.
    const A_SEED: Option<u64> = Some(0xC0FF_EE12);

    fn speaker(role: &str, class: ClassConstraint) -> BanterSpeaker {
        BanterSpeaker { role: role.to_string(), class }
    }

    fn beat(role: &str, text: &str) -> BanterBeat {
        BanterBeat { role: role.to_string(), text: text.to_string() }
    }

    /// A two-speaker exchange whose beats are tagged with `label`, so a test can
    /// tell which pool entry the resolver picked by reading the rendered text.
    fn two_speaker(
        context: BanterContext,
        label: &str,
        responder: ClassConstraint,
        target: ClassConstraint,
    ) -> BanterExchange {
        BanterExchange {
            context,
            speakers: vec![
                speaker("caller", ClassConstraint::Any),
                speaker("responder", responder),
            ],
            target,
            beats: vec![
                beat("caller", &format!("{}: kill the {{target}}.", label)),
                beat("responder", &format!("{}: on it.", label)),
            ],
        }
    }

    fn config_with(exchanges: Vec<BanterExchange>) -> BanterConfig {
        BanterConfig { timing: BanterTiming::default(), exchanges }
    }

    /// Living combatants of the given classes, in slot order, on team 1.
    fn lineup(classes: &[CharacterClass]) -> BanterLineup {
        BanterLineup {
            team: 1,
            allies: classes
                .iter()
                .enumerate()
                .map(|(index, class)| BanterCombatant {
                    entity: Entity::from_raw(index as u32 + 1),
                    class: *class,
                    alive: true,
                })
                .collect(),
        }
    }

    fn called(target: CharacterClass) -> BanterCall {
        BanterCall { target: Some(target), prev_target: None }
    }

    /// The label prefix a resolved exchange's first beat carries, for
    /// identifying which pool entry won without depending on pool order.
    fn label_of(resolved: &ResolvedExchange) -> String {
        resolved.beats[0]
            .text
            .split(':')
            .next()
            .expect("split always yields one element")
            .to_string()
    }

    /// AE1. Nobody to answer, so nothing is said — and this falls out of role
    /// binding rather than a solo-team special case.
    #[test]
    fn a_solo_lineup_satisfies_no_two_speaker_exchange() {
        let config = config_with(vec![two_speaker(
            BanterContext::Opening,
            "generic",
            ClassConstraint::Any,
            ClassConstraint::Any,
        )]);
        let solo = lineup(&[CharacterClass::Mage]);

        assert!(resolve_exchange(
            &config,
            &solo,
            called(CharacterClass::Warrior),
            BanterContext::Opening,
            A_SEED,
            0,
        )
        .is_none());

        // The same pool with one more body resolves, so the fixture is testing
        // the lineup and not a broken exchange.
        assert!(resolve_exchange(
            &config,
            &lineup(&[CharacterClass::Mage, CharacterClass::Priest]),
            called(CharacterClass::Warrior),
            BanterContext::Opening,
            A_SEED,
            0,
        )
        .is_some());
    }

    /// AE2 / KTD8. Specificity is a WEIGHT: the bespoke Priest-responder entry
    /// is favoured, but the generics never stop appearing. Strict
    /// most-specific-wins would make the generic count here zero.
    #[test]
    fn specificity_favours_the_bespoke_exchange_without_excluding_generics() {
        let config = config_with(vec![
            two_speaker(
                BanterContext::Opening,
                "priest",
                ClassConstraint::Class(CharacterClass::Priest),
                ClassConstraint::Any,
            ),
            two_speaker(
                BanterContext::Opening,
                "generic_a",
                ClassConstraint::Any,
                ClassConstraint::Any,
            ),
            two_speaker(
                BanterContext::Opening,
                "generic_b",
                ClassConstraint::Any,
                ClassConstraint::Any,
            ),
            two_speaker(
                BanterContext::Opening,
                "generic_c",
                ClassConstraint::Any,
                ClassConstraint::Any,
            ),
        ]);
        // A lineup that satisfies the bespoke entry AND all the generics.
        let team = lineup(&[CharacterClass::Warrior, CharacterClass::Priest]);

        let mut specific = 0;
        let mut generic = 0;
        for seed in 0..200u64 {
            let resolved = resolve_exchange(
                &config,
                &team,
                called(CharacterClass::Mage),
                BanterContext::Opening,
                Some(seed),
                0,
            )
            .expect("a satisfiable pool always resolves");
            if label_of(&resolved) == "priest" {
                specific += 1;
            } else {
                generic += 1;
            }
        }

        assert!(specific > 0, "the bespoke exchange must be reachable");
        assert!(
            generic > 0,
            "generics must stay in the pool — specificity is a weight, not a filter"
        );
        // weight 3 vs 3 x weight 1 => the bespoke entry should take roughly half
        // the draws. Bounds are loose enough to be about the weighting, not the
        // hash's exact distribution.
        assert!(
            (60..=140).contains(&specific),
            "expected the weighting to favour but not monopolise the bespoke entry, \
             got {}/200 specific",
            specific
        );
    }

    /// AE3. Same seed, team, context and occurrence — same exchange, every time.
    #[test]
    fn identical_inputs_resolve_identically() {
        let config = config_with(vec![
            two_speaker(
                BanterContext::Opening,
                "a",
                ClassConstraint::Any,
                ClassConstraint::Any,
            ),
            two_speaker(
                BanterContext::Opening,
                "b",
                ClassConstraint::Any,
                ClassConstraint::Any,
            ),
            two_speaker(
                BanterContext::Opening,
                "c",
                ClassConstraint::Any,
                ClassConstraint::Any,
            ),
        ]);
        let team = lineup(&[CharacterClass::Warrior, CharacterClass::Priest]);
        let resolve = || {
            resolve_exchange(
                &config,
                &team,
                called(CharacterClass::Mage),
                BanterContext::Opening,
                Some(777),
                2,
            )
        };

        let first = resolve().expect("resolves");
        for _ in 0..5 {
            assert_eq!(resolve().expect("resolves"), first);
        }

        // ...and the varying inputs actually vary it, or the assertion above
        // would pass on a resolver that ignored them entirely.
        let mut seen = std::collections::HashSet::new();
        for occurrence in 0..12 {
            let resolved = resolve_exchange(
                &config,
                &team,
                called(CharacterClass::Mage),
                BanterContext::Opening,
                Some(777),
                occurrence,
            )
            .expect("resolves");
            seen.insert(label_of(&resolved));
        }
        assert!(
            seen.len() > 1,
            "the occurrence counter must move selection, got only {:?}",
            seen
        );
    }

    /// A constrained role takes the combatant of its class even when slot order
    /// would hand it someone else — constrained roles bind FIRST.
    #[test]
    fn a_class_constrained_role_binds_to_that_class_not_slot_order() {
        let config = config_with(vec![two_speaker(
            BanterContext::Opening,
            "priest",
            ClassConstraint::Class(CharacterClass::Priest),
            ClassConstraint::Any,
        )]);
        // Priest is in the LAST slot, so a naive slot-order fill would put the
        // Warrior on `responder` and the Priest on `caller`.
        let team = lineup(&[
            CharacterClass::Warrior,
            CharacterClass::Rogue,
            CharacterClass::Priest,
        ]);
        let priest = team.allies[2].entity;
        let warrior = team.allies[0].entity;

        let resolved = resolve_exchange(
            &config,
            &team,
            called(CharacterClass::Mage),
            BanterContext::Opening,
            A_SEED,
            0,
        )
        .expect("the lineup has a Priest");

        // Beat 0 is `caller` (unconstrained), beat 1 is `responder` (Priest).
        assert_eq!(resolved.beats[1].speaker, priest);
        assert_eq!(
            resolved.beats[0].speaker, warrior,
            "the unconstrained role fills from slot order out of what is left"
        );
    }

    /// Roles must bind to DISTINCT combatants, so two same-class roles need two
    /// of that class. This is the same mechanism that silences a 1v1.
    #[test]
    fn two_roles_that_could_only_bind_to_one_combatant_are_unsatisfiable() {
        let two_priests = BanterExchange {
            context: BanterContext::Opening,
            speakers: vec![
                speaker("a", ClassConstraint::Class(CharacterClass::Priest)),
                speaker("b", ClassConstraint::Class(CharacterClass::Priest)),
            ],
            target: ClassConstraint::Any,
            beats: vec![beat("a", "Heals up."), beat("b", "Heals up.")],
        };
        let config = config_with(vec![two_priests]);

        assert!(
            resolve_exchange(
                &config,
                &lineup(&[CharacterClass::Priest, CharacterClass::Warrior]),
                called(CharacterClass::Mage),
                BanterContext::Opening,
                A_SEED,
                0,
            )
            .is_none(),
            "one Priest cannot fill two Priest roles"
        );

        assert!(
            resolve_exchange(
                &config,
                &lineup(&[CharacterClass::Priest, CharacterClass::Priest]),
                called(CharacterClass::Mage),
                BanterContext::Opening,
                A_SEED,
                0,
            )
            .is_some(),
            "two Priests fill two Priest roles"
        );
    }

    /// A specific target constraint filters on the CALLED combatant's class,
    /// and a cleared call satisfies `Any` only.
    #[test]
    fn a_target_constraint_filters_on_the_called_class() {
        let config = config_with(vec![two_speaker(
            BanterContext::Opening,
            "vs_warrior",
            ClassConstraint::Any,
            ClassConstraint::Class(CharacterClass::Warrior),
        )]);
        let team = lineup(&[CharacterClass::Mage, CharacterClass::Priest]);
        let resolve = |call: BanterCall| {
            resolve_exchange(&config, &team, call, BanterContext::Opening, A_SEED, 0)
        };

        assert!(resolve(called(CharacterClass::Warrior)).is_some());
        assert!(resolve(called(CharacterClass::Mage)).is_none());
        assert!(
            resolve(BanterCall::default()).is_none(),
            "a cleared call has no class, so it cannot satisfy a specific target"
        );
    }

    /// R13. `{target}` renders the called combatant's class — the only handle
    /// banter has, since combatants carry no per-combatant names.
    #[test]
    fn target_substitution_renders_the_called_class() {
        let config = config_with(vec![two_speaker(
            BanterContext::Opening,
            "generic",
            ClassConstraint::Any,
            ClassConstraint::Any,
        )]);
        let resolved = resolve_exchange(
            &config,
            &lineup(&[CharacterClass::Mage, CharacterClass::Priest]),
            called(CharacterClass::Warlock),
            BanterContext::Opening,
            A_SEED,
            0,
        )
        .expect("resolves");

        assert_eq!(resolved.beats[0].text, "generic: kill the Warlock.");
        assert!(
            !resolved.beats[0].text.contains('{'),
            "no placeholder may survive into a bubble"
        );
    }

    /// R13. `{prev_target}` is a `Correction`-only substitution. Everywhere
    /// else it falls back rather than leaking a literal brace.
    #[test]
    fn prev_target_substitutes_in_correction_only() {
        let with_prev = |context| BanterExchange {
            context,
            speakers: vec![
                speaker("caller", ClassConstraint::Any),
                speaker("responder", ClassConstraint::Any),
            ],
            target: ClassConstraint::Any,
            beats: vec![
                beat("caller", "Forget the {prev_target} — {target} now."),
                beat("responder", "Understood."),
            ],
        };
        let config = config_with(vec![
            with_prev(BanterContext::Correction),
            with_prev(BanterContext::Switch),
        ]);
        let team = lineup(&[CharacterClass::Mage, CharacterClass::Priest]);
        let call = BanterCall {
            target: Some(CharacterClass::Rogue),
            prev_target: Some(CharacterClass::Paladin),
        };
        let resolve = |context| {
            resolve_exchange(&config, &team, call, context, A_SEED, 0).expect("resolves")
        };

        assert_eq!(
            resolve(BanterContext::Correction).beats[0].text,
            "Forget the Paladin — Rogue now."
        );

        // Outside Correction the previous call is not a thing worth naming, so
        // it renders as the neutral fallback — never the class, never a brace.
        let switch = resolve(BanterContext::Switch);
        assert_eq!(switch.beats[0].text, "Forget the them — Rogue now.");
        assert!(!switch.beats[0].text.contains("Paladin"));
        assert!(!switch.beats[0].text.contains('{'));
    }

    /// An absent target renders the fallback rather than a raw placeholder — a
    /// cleared call must never put `"{target}"` in a speech bubble.
    #[test]
    fn an_unresolvable_target_falls_back_instead_of_leaking_a_placeholder() {
        let config = config_with(vec![two_speaker(
            BanterContext::Switch,
            "generic",
            ClassConstraint::Any,
            ClassConstraint::Any,
        )]);
        let resolved = resolve_exchange(
            &config,
            &lineup(&[CharacterClass::Mage, CharacterClass::Priest]),
            BanterCall::default(),
            BanterContext::Switch,
            A_SEED,
            0,
        )
        .expect("a fully-generic exchange survives a cleared call");

        assert_eq!(resolved.beats[0].text, "generic: kill the them.");
    }

    /// KTD7's defensive branch. Headless never runs this and the client always
    /// records a seed, but a `None` must resolve — same answer every time —
    /// rather than panicking or going quiet.
    #[test]
    fn a_none_seed_still_resolves_deterministically() {
        let config = config_with(vec![
            two_speaker(
                BanterContext::Opening,
                "a",
                ClassConstraint::Any,
                ClassConstraint::Any,
            ),
            two_speaker(
                BanterContext::Opening,
                "b",
                ClassConstraint::Any,
                ClassConstraint::Any,
            ),
        ]);
        let team = lineup(&[CharacterClass::Mage, CharacterClass::Priest]);
        let resolve = || {
            resolve_exchange(
                &config,
                &team,
                called(CharacterClass::Warrior),
                BanterContext::Opening,
                None,
                0,
            )
            .expect("a seedless resolve still picks")
        };

        let first = resolve();
        assert_eq!(resolve(), first);
        // The fallback seed must not collapse the hash to a degenerate roll —
        // `mix` starts with an xor, so an init constant equal to the fallback
        // would zero the accumulator and pin every seedless draw to index 0.
        assert!(banter_roll(None, 1, BanterContext::Opening, 0) > 0.0);
    }

    /// Dead combatants keep their slot (so ordering is stable) but never speak.
    #[test]
    fn dead_combatants_are_never_bound_as_speakers() {
        let config = config_with(vec![two_speaker(
            BanterContext::Opening,
            "generic",
            ClassConstraint::Any,
            ClassConstraint::Any,
        )]);
        let mut team = lineup(&[
            CharacterClass::Warrior,
            CharacterClass::Rogue,
            CharacterClass::Priest,
        ]);
        team.allies[0].alive = false;
        let rogue = team.allies[1].entity;
        let priest = team.allies[2].entity;

        let resolved = resolve_exchange(
            &config,
            &team,
            called(CharacterClass::Mage),
            BanterContext::Opening,
            A_SEED,
            0,
        )
        .expect("two survivors can still hold a two-speaker exchange");
        assert_eq!(resolved.beats[0].speaker, rogue);
        assert_eq!(resolved.beats[1].speaker, priest);

        // Kill one more and the exchange loses its second voice entirely.
        team.allies[1].alive = false;
        assert!(resolve_exchange(
            &config,
            &team,
            called(CharacterClass::Mage),
            BanterContext::Opening,
            A_SEED,
            0,
        )
        .is_none());
    }

    /// Beat times are derived from the pacing block, not authored, and the
    /// bubble lifetime rides along so the scheduler needs nothing else.
    #[test]
    fn resolved_beats_carry_derived_start_times_and_the_bubble_lifetime() {
        let config = config_with(vec![two_speaker(
            BanterContext::Opening,
            "generic",
            ClassConstraint::Any,
            ClassConstraint::Any,
        )]);
        let resolved = resolve_exchange(
            &config,
            &lineup(&[CharacterClass::Mage, CharacterClass::Priest]),
            called(CharacterClass::Warrior),
            BanterContext::Opening,
            A_SEED,
            0,
        )
        .expect("resolves");

        assert_eq!(resolved.context, BanterContext::Opening);
        assert_eq!(resolved.lifetime, config.timing.line_lifetime);
        assert_eq!(
            resolved.beats.iter().map(|b| b.start).collect::<Vec<_>>(),
            vec![
                config.timing.beat_start(BanterContext::Opening, 0),
                config.timing.beat_start(BanterContext::Opening, 1),
            ]
        );
    }

    /// The pool is filtered by context, so a change never plays an exchange
    /// written for a different situation — and an empty context pool is a quiet
    /// `None`, not an error.
    #[test]
    fn the_pool_is_filtered_by_context() {
        let config = config_with(vec![two_speaker(
            BanterContext::Opening,
            "generic",
            ClassConstraint::Any,
            ClassConstraint::Any,
        )]);
        let team = lineup(&[CharacterClass::Mage, CharacterClass::Priest]);
        let resolve = |context| {
            resolve_exchange(&config, &team, called(CharacterClass::Warrior), context, A_SEED, 0)
        };

        assert!(resolve(BanterContext::Opening).is_some());
        assert!(resolve(BanterContext::Correction).is_none());
        assert!(resolve(BanterContext::Switch).is_none());
    }

    /// A beatless entry is unsatisfiable: "resolved" and "audible" have to mean
    /// the same thing, or the scheduler queues silence.
    #[test]
    fn a_beatless_exchange_is_never_picked() {
        let silent = BanterExchange {
            context: BanterContext::Switch,
            speakers: vec![speaker("caller", ClassConstraint::Any)],
            target: ClassConstraint::Any,
            beats: Vec::new(),
        };
        let config = config_with(vec![silent]);

        assert!(resolve_exchange(
            &config,
            &lineup(&[CharacterClass::Mage, CharacterClass::Priest]),
            called(CharacterClass::Warrior),
            BanterContext::Switch,
            A_SEED,
            0,
        )
        .is_none());
    }

    /// A beat naming a role the exchange never declared is dropped, not bound
    /// to whoever happens to be first. `validate()` rejects this at load, so
    /// only a hand-built config can reach it — which is exactly what a test
    /// harness is.
    #[test]
    fn an_exchange_with_an_undeclared_role_is_dropped() {
        let malformed = BanterExchange {
            context: BanterContext::Switch,
            speakers: vec![speaker("caller", ClassConstraint::Any)],
            target: ClassConstraint::Any,
            beats: vec![beat("heckler", "Who even am I?")],
        };
        let config = config_with(vec![malformed]);

        assert!(resolve_exchange(
            &config,
            &lineup(&[CharacterClass::Mage, CharacterClass::Priest]),
            called(CharacterClass::Warrior),
            BanterContext::Switch,
            A_SEED,
            0,
        )
        .is_none());
    }

    /// The two teams hash differently, so a mirror comp does not have both
    /// sides say the same line in the same match.
    #[test]
    fn the_two_teams_draw_independently() {
        let config = config_with(
            (0..6)
                .map(|i| {
                    two_speaker(
                        BanterContext::Opening,
                        &format!("e{}", i),
                        ClassConstraint::Any,
                        ClassConstraint::Any,
                    )
                })
                .collect(),
        );
        let resolve = |team| {
            let mut lineup = lineup(&[CharacterClass::Mage, CharacterClass::Priest]);
            lineup.team = team;
            label_of(
                &resolve_exchange(
                    &config,
                    &lineup,
                    called(CharacterClass::Warrior),
                    BanterContext::Opening,
                    Some(4242),
                    0,
                )
                .expect("resolves"),
            )
        };

        // Not a guarantee for every seed — teams draw independently, they are
        // not forced apart — so this asserts over a run of seeds instead of
        // pinning one. Any divergence at all proves the team salt is live.
        let differs = (0..64u64).any(|seed| {
            banter_roll(Some(seed), 1, BanterContext::Opening, 0)
                != banter_roll(Some(seed), 2, BanterContext::Opening, 0)
        });
        assert!(differs, "team must be part of the selection hash");
        // ...and the resolve path actually threads the team through.
        assert!(!resolve(1).is_empty() && !resolve(2).is_empty());
    }

    // -------------------------------------------------------------------
    // Beat scheduling
    //
    // The queue mechanics are pure — `advance` a clock, `take_due` against a
    // liveness predicate — so almost everything below runs without a `World`.
    // Only the four plumbing tests at the end build one.
    // -------------------------------------------------------------------

    /// Speakers used by the scheduler fixtures. Raw entities are fine here:
    /// the scheduler only ever compares and hashes them.
    const ALEX: Entity = Entity::from_raw(11);
    const BEA: Entity = Entity::from_raw(12);

    /// A hand-built resolved exchange, so a scheduler test never depends on
    /// which entry the resolver happens to pick.
    fn resolved(
        context: BanterContext,
        lifetime: f32,
        beats: &[(Entity, &str, f32)],
    ) -> ResolvedExchange {
        ResolvedExchange {
            context,
            lifetime,
            beats: beats
                .iter()
                .map(|(speaker, text, start)| ResolvedBeat {
                    speaker: *speaker,
                    text: (*text).to_string(),
                    start: *start,
                })
                .collect(),
        }
    }

    /// Everyone lives — the default for scheduler tests that are not about
    /// death.
    fn all_alive(_: Entity) -> bool {
        true
    }

    /// Advance the clock and collect whatever that emits, as `(speaker, text)`.
    fn step(scheduler: &mut BanterScheduler, delta: f32) -> Vec<(Entity, String)> {
        scheduler.advance(delta);
        scheduler
            .take_due(all_alive)
            .into_iter()
            .map(|beat| (beat.speaker, beat.text))
            .collect()
    }

    #[test]
    fn beats_emit_in_order_at_their_configured_offsets() {
        let mut scheduler = BanterScheduler::default();
        scheduler.queue_exchange(
            1,
            &resolved(
                BanterContext::Opening,
                2.6,
                &[(ALEX, "Kill the Mage.", 2.0), (BEA, "On it.", 4.2)],
            ),
        );

        // Nothing before the first offset — the exchange does not start on the
        // frame the call changed.
        assert!(step(&mut scheduler, 1.9).is_empty());
        assert_eq!(
            step(&mut scheduler, 0.2),
            vec![(ALEX, "Kill the Mage.".to_string())],
            "beat 0 speaks once the clock passes its start"
        );
        assert!(step(&mut scheduler, 1.0).is_empty(), "beat 1 is not due yet");
        assert_eq!(
            step(&mut scheduler, 1.5),
            vec![(BEA, "On it.".to_string())]
        );
        // ...and the queue is now empty rather than replaying.
        assert!(step(&mut scheduler, 10.0).is_empty());
    }

    /// KTD9. The opening exchange is about a target that is no longer called,
    /// so its unplayed beats go rather than talk over the correction.
    #[test]
    fn a_correction_mid_exchange_drops_the_unplayed_beats() {
        let mut scheduler = BanterScheduler::default();
        scheduler.queue_exchange(
            1,
            &resolved(
                BanterContext::Opening,
                2.6,
                &[(ALEX, "opening 0", 2.0), (BEA, "opening 1", 4.2)],
            ),
        );

        assert_eq!(step(&mut scheduler, 2.0).len(), 1, "opening beat 0 played");

        // The operator changes the call: cancel, then queue the replacement.
        scheduler.cancel_team(1);
        scheduler.queue_exchange(
            1,
            &resolved(
                BanterContext::Correction,
                2.6,
                &[(BEA, "correction 0", 2.0), (ALEX, "correction 1", 3.6)],
            ),
        );

        let mut spoken: Vec<String> = Vec::new();
        for _ in 0..12 {
            spoken.extend(step(&mut scheduler, 0.5).into_iter().map(|(_, text)| text));
        }
        assert_eq!(
            spoken,
            vec!["correction 0".to_string(), "correction 1".to_string()],
            "the opening's unplayed beat must not survive the correction"
        );
    }

    /// A correction arriving after the opening finished has nothing to cancel,
    /// so it simply queues — the cancel is unconditional but harmless.
    #[test]
    fn a_correction_after_the_last_beat_played_queues_normally() {
        let mut scheduler = BanterScheduler::default();
        scheduler.queue_exchange(
            1,
            &resolved(BanterContext::Opening, 1.0, &[(ALEX, "opening", 1.0)]),
        );
        assert_eq!(step(&mut scheduler, 1.0).len(), 1);
        assert!(scheduler.queues[0].is_empty(), "the opening is fully played");

        scheduler.cancel_team(1);
        scheduler.queue_exchange(
            1,
            &resolved(BanterContext::Correction, 1.0, &[(BEA, "correction", 1.0)]),
        );
        assert_eq!(
            step(&mut scheduler, 1.0),
            vec![(BEA, "correction".to_string())]
        );
    }

    /// Liveness is checked at EMISSION, not resolution: a speaker bound while
    /// alive can be a corpse by the time their beat comes round.
    #[test]
    fn a_speaker_who_dies_before_their_beat_emits_nothing() {
        let mut scheduler = BanterScheduler::default();
        scheduler.queue_exchange(
            1,
            &resolved(
                BanterContext::Opening,
                1.0,
                &[(ALEX, "the dead one", 1.0), (BEA, "the survivor", 3.0)],
            ),
        );

        scheduler.advance(1.0);
        let due = scheduler.take_due(|entity| entity != ALEX);
        assert!(due.is_empty(), "a corpse must not talk");

        // ...and the rest of the exchange still plays: one speaker falling
        // must not silence their partner's reply.
        scheduler.advance(2.0);
        let due = scheduler.take_due(|entity| entity != ALEX);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].speaker, BEA);
    }

    /// One live bubble per speaker, ACROSS exchanges. `BanterConfig::validate()` covers
    /// the within-exchange case; only the scheduler can see a correction beat
    /// landing while an opening beat from the same speaker is still up.
    #[test]
    fn a_second_bubble_on_one_speaker_is_deferred_until_the_first_expires() {
        let mut scheduler = BanterScheduler::default();
        scheduler.queue_exchange(
            1,
            &resolved(BanterContext::Opening, 2.6, &[(ALEX, "opening", 0.0)]),
        );
        assert_eq!(step(&mut scheduler, 0.0).len(), 1, "bubble up at t=0");

        // A correction two seconds later puts ALEX back on the mic while the
        // first bubble (2.6s) is still drawn.
        scheduler.advance(2.0);
        scheduler.cancel_team(1);
        scheduler.queue_exchange(
            1,
            &resolved(BanterContext::Correction, 2.6, &[(ALEX, "correction", 0.0)]),
        );

        assert!(
            scheduler.take_due(all_alive).is_empty(),
            "the correction must wait rather than draw over the live bubble"
        );
        assert_eq!(
            scheduler.queues[0][0].at, 2.6,
            "deferred to exactly when the first bubble expires"
        );

        // The push is bounded by one lifetime, so the line is delayed, never lost.
        assert_eq!(
            step(&mut scheduler, 0.7),
            vec![(ALEX, "correction".to_string())]
        );
    }

    /// A deferred beat holds the ones behind it, so an exchange cannot deliver
    /// its punchline before its setup.
    #[test]
    fn later_beats_wait_behind_a_deferred_beat() {
        let mut scheduler = BanterScheduler::default();
        // ALEX is already mid-bubble when the exchange is queued...
        scheduler.queue_exchange(
            1,
            &resolved(BanterContext::Opening, 4.0, &[(ALEX, "earlier", 0.0)]),
        );
        assert_eq!(step(&mut scheduler, 0.0).len(), 1);

        scheduler.cancel_team(1);
        scheduler.queue_exchange(
            1,
            &resolved(
                BanterContext::Switch,
                1.0,
                &[(ALEX, "setup", 0.0), (BEA, "punchline", 0.5)],
            ),
        );

        // ...so at t=0.5 BEA's beat is due, but it must not jump ALEX's.
        assert!(step(&mut scheduler, 0.5).is_empty());
        let spoken: Vec<String> = (0..10)
            .flat_map(|_| step(&mut scheduler, 0.5))
            .map(|(_, text)| text)
            .collect();
        assert_eq!(spoken, vec!["setup".to_string(), "punchline".to_string()]);
    }

    /// The occurrence counter is per team and advances on every change — it is
    /// what stops a thrice-corrected team telling one joke three times.
    #[test]
    fn the_occurrence_counter_advances_per_team() {
        let mut scheduler = BanterScheduler::default();
        assert_eq!(scheduler.next_occurrence(1), 0);
        assert_eq!(scheduler.next_occurrence(1), 1);
        assert_eq!(
            scheduler.next_occurrence(2),
            0,
            "team 2 counts independently"
        );
        assert_eq!(scheduler.next_occurrence(1), 2);
    }

    /// The two teams schedule independently: a correction on one side never
    /// touches the other's queue.
    #[test]
    fn cancelling_one_team_leaves_the_other_queue_intact() {
        let mut scheduler = BanterScheduler::default();
        let exchange = resolved(BanterContext::Opening, 1.0, &[(ALEX, "hello", 1.0)]);
        scheduler.queue_exchange(1, &exchange);
        scheduler.queue_exchange(2, &exchange);

        scheduler.cancel_team(1);
        assert!(scheduler.queues[0].is_empty());
        assert_eq!(scheduler.queues[1].len(), 1);
    }

    // -------------------------------------------------------------------
    // Scheduler plumbing — a minimal `World` with the resources and
    // combatants `play_banter_beats` reads, as U4's watcher tests do.
    // -------------------------------------------------------------------

    use super::super::components::{PlayMatchEntity, SpeechBubble};

    /// A pool with a two-beat `Opening` and a one-beat `Switch`, on fast
    /// timings so a test can walk a whole exchange in a few frames.
    fn scheduler_config() -> BanterConfig {
        let timing = BanterTiming {
            opening_start: 1.0,
            switch_start: 0.1,
            beat_gap: 1.0,
            line_lifetime: 0.5,
            correction_beat_gap: 1.0,
            latest_beat: 9.0,
            specificity_weight: 3.0,
        };
        let one_beat_switch = BanterExchange {
            context: BanterContext::Switch,
            speakers: vec![
                speaker("caller", ClassConstraint::Any),
                speaker("responder", ClassConstraint::Any),
            ],
            target: ClassConstraint::Any,
            // Deliberately ONE beat with a second role declared: the shout is
            // single-beat, but the exchange still needs a team to shout in.
            beats: vec![beat("caller", "Switch to the {target}!")],
        };
        BanterConfig {
            timing,
            exchanges: vec![
                two_speaker(
                    BanterContext::Opening,
                    "opening",
                    ClassConstraint::Any,
                    ClassConstraint::Any,
                ),
                one_beat_switch,
            ],
        }
    }

    /// A world with `classes` on each team, a banter pool, and the two banter
    /// resources. `slots` are assigned in list order, which is what call
    /// indices address.
    fn banter_world(team1: &[CharacterClass], team2: &[CharacterClass]) -> World {
        let mut world = World::new();
        for (team, classes) in [(1u8, team1), (2u8, team2)] {
            for (slot, class) in classes.iter().enumerate() {
                world.spawn(Combatant::new(team, slot as u8, *class));
            }
        }
        world.insert_resource(scheduler_config());
        world.insert_resource(GameRng::from_seed(4242));
        world.insert_resource(CallWatcher::default());
        world.insert_resource(BanterScheduler::default());
        world.insert_resource(Time::<()>::default());
        world
    }

    /// Run `play_banter_beats` for one frame of `delta` seconds, applying its
    /// deferred `Commands` so spawned bubbles are visible to the assertions.
    fn run_scheduler(world: &mut World, delta: f32) {
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(delta));
        world.insert_resource(time);

        let mut system = IntoSystem::into_system(play_banter_beats);
        system.initialize(world);
        system.run((), world);
        system.apply_deferred(world);
    }

    /// Every speech bubble currently in the world, as `(owner, text)`.
    fn bubbles(world: &mut World) -> Vec<(Entity, String)> {
        world
            .query::<&SpeechBubble>()
            .iter(world)
            .map(|bubble| (bubble.owner, bubble.text.clone()))
            .collect()
    }

    /// Queue a change by hand, the way the watcher would.
    fn push_change(world: &mut World, change: CallChange) {
        world.resource_mut::<CallWatcher>().pending.push(change);
    }

    fn change(team: u8, new_call: Option<usize>, previous: LastSeenCall, gates: bool) -> CallChange {
        CallChange {
            team,
            new_call,
            previous,
            gates_opened: gates,
            context: banter_context_for(previous, gates),
        }
    }

    #[test]
    fn the_system_plays_an_opening_exchange_from_a_call_change() {
        let mut world = banter_world(
            &[CharacterClass::Warrior, CharacterClass::Priest],
            &[CharacterClass::Mage, CharacterClass::Warlock],
        );
        push_change(
            &mut world,
            change(1, Some(0), LastSeenCall::NeverObserved, false),
        );

        // opening_start is 1.0, so the first frame queues but says nothing.
        run_scheduler(&mut world, 0.1);
        assert!(bubbles(&mut world).is_empty());
        assert_eq!(world.resource::<BanterScheduler>().queues[0].len(), 2);

        run_scheduler(&mut world, 1.0);
        let spoken = bubbles(&mut world);
        assert_eq!(spoken.len(), 1, "beat 0 only");
        assert!(
            spoken[0].1.contains("Mage"),
            "{{target}} must render team 2 slot 0's class, got {:?}",
            spoken[0].1
        );

        run_scheduler(&mut world, 1.0);
        assert_eq!(bubbles(&mut world).len(), 2, "beat 1 followed");
        // Two distinct speakers, which is the one-bubble-per-speaker invariant
        // holding across the exchange.
        let owners: std::collections::HashSet<Entity> =
            bubbles(&mut world).into_iter().map(|(o, _)| o).collect();
        assert_eq!(owners.len(), 2);
    }

    #[test]
    fn a_post_gate_change_emits_a_single_beat_shout() {
        let mut world = banter_world(
            &[CharacterClass::Warrior, CharacterClass::Priest],
            &[CharacterClass::Mage, CharacterClass::Warlock],
        );
        push_change(
            &mut world,
            change(1, Some(1), LastSeenCall::Seen(Some(0)), true),
        );

        run_scheduler(&mut world, 0.1);
        run_scheduler(&mut world, 1.0);
        let spoken = bubbles(&mut world);
        assert_eq!(spoken.len(), 1, "Switch is a single-beat shout");
        assert_eq!(spoken[0].1, "Switch to the Warlock!", "slot 1 of team 2");

        // ...and nothing follows it.
        for _ in 0..6 {
            run_scheduler(&mut world, 1.0);
        }
        assert_eq!(bubbles(&mut world).len(), 1);
    }

    #[test]
    fn a_team_whose_resolver_returns_nothing_queues_no_beats() {
        // A 1v1: no two-speaker exchange in the pool can bind, so the team is
        // silent (AE1) — and the system must not error on the empty resolve.
        let mut world = banter_world(&[CharacterClass::Mage], &[CharacterClass::Warrior]);
        push_change(
            &mut world,
            change(1, Some(0), LastSeenCall::NeverObserved, false),
        );

        for _ in 0..8 {
            run_scheduler(&mut world, 1.0);
        }
        assert!(bubbles(&mut world).is_empty());
        assert!(world.resource::<BanterScheduler>().queues[0].is_empty());
        // The occurrence counter still moved, so the next change does not land
        // on the roll this silent one would have used.
        assert_eq!(world.resource::<BanterScheduler>().occurrence[0], 1);
    }

    #[test]
    fn the_system_drains_the_watcher_so_a_change_is_handled_once() {
        let mut world = banter_world(
            &[CharacterClass::Warrior, CharacterClass::Priest],
            &[CharacterClass::Mage, CharacterClass::Warlock],
        );
        push_change(
            &mut world,
            change(1, Some(0), LastSeenCall::NeverObserved, false),
        );

        run_scheduler(&mut world, 0.1);
        assert!(world.resource::<CallWatcher>().pending.is_empty());
        assert_eq!(world.resource::<BanterScheduler>().queues[0].len(), 2);

        // A second frame must not re-queue the same exchange.
        run_scheduler(&mut world, 0.1);
        assert_eq!(world.resource::<BanterScheduler>().queues[0].len(), 2);
    }

    #[test]
    fn queued_beats_do_not_survive_into_a_new_match() {
        let mut world = banter_world(
            &[CharacterClass::Warrior, CharacterClass::Priest],
            &[CharacterClass::Mage, CharacterClass::Warlock],
        );
        push_change(
            &mut world,
            change(1, Some(0), LastSeenCall::NeverObserved, false),
        );
        run_scheduler(&mut world, 0.1);
        assert!(!world.resource::<BanterScheduler>().queues[0].is_empty());

        let mut reset = IntoSystem::into_system(reset_banter_scheduler_on_exit);
        reset.initialize(&mut world);
        reset.run((), &mut world);

        let scheduler = world.resource::<BanterScheduler>();
        assert!(scheduler.queues.iter().all(|queue| queue.is_empty()));
        assert!(scheduler.speaking_until.is_empty());
        assert_eq!(scheduler.occurrence, [0, 0]);
        assert_eq!(
            scheduler.clock, 0.0,
            "a carried-over clock would fire the next match's exchange instantly"
        );

        // Walking well past the old beat times emits nothing.
        for _ in 0..8 {
            run_scheduler(&mut world, 1.0);
        }
        assert!(bubbles(&mut world).is_empty());
    }

    #[test]
    fn a_bubble_spawned_by_a_beat_is_a_play_match_entity_with_the_configured_lifetime() {
        // Tagging matters: `cleanup_play_match` despawns by `PlayMatchEntity`,
        // so an untagged bubble would outlive its match.
        let mut world = banter_world(
            &[CharacterClass::Warrior, CharacterClass::Priest],
            &[CharacterClass::Mage, CharacterClass::Warlock],
        );
        push_change(
            &mut world,
            change(1, Some(0), LastSeenCall::NeverObserved, false),
        );
        run_scheduler(&mut world, 0.1);
        run_scheduler(&mut world, 1.0);

        let lifetime = world.resource::<BanterConfig>().timing.line_lifetime;
        let tagged = world
            .query::<(&SpeechBubble, &PlayMatchEntity)>()
            .iter(&world)
            .map(|(bubble, _)| bubble.lifetime)
            .collect::<Vec<_>>();
        assert_eq!(tagged, vec![lifetime]);
    }
}
