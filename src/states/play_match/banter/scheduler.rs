//! Beat pacing and bubble emission: the Bevy half of the banter pipeline.
//!
//! Drains the queue `watcher` fills, hands each change to `resolver`, holds the
//! resulting beats per team on its own clock, and spawns a speech bubble as
//! each falls due. The roster helpers here are the bridge between the two: they
//! turn the world's `Combatant` query into the plain-data lineup and target
//! classes the resolver takes.

use std::collections::HashMap;

use bevy::prelude::*;

use super::super::banter_config::BanterConfig;
use super::super::components::{Combatant, GameRng, Pet, SpeechBubble};
use super::super::match_config::CharacterClass;
use super::super::utils::spawn_speech_line;
use super::resolver::{resolve_exchange, BanterCall, BanterCombatant, BanterLineup, ResolvedExchange};
use super::watcher::CallWatcher;

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

    /// Record every currently-live bubble as speaker occupancy.
    ///
    /// Takes the max against whatever is already recorded so a banter bubble's
    /// own booking is never shortened by this, and so a speaker holding two
    /// bubbles is busy until the later one clears.
    ///
    /// Split from [`take_due`](Self::take_due) rather than folded into it
    /// because the queue mechanics stay pure and testable over plain values;
    /// this is the one step that needs the World.
    fn observe_live_bubbles(&mut self, bubbles: &Query<&SpeechBubble>) {
        for bubble in bubbles.iter() {
            let free_at = self.clock + bubble.lifetime;
            self.speaking_until
                .entry(bubble.owner)
                .and_modify(|until| *until = until.max(free_at))
                .or_insert(free_at);
        }
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
/// `resolve_exchange` and the [`BanterScheduler`] methods — is pure over
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
    bubbles: Query<&SpeechBubble>,
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

    // Fold EVERY live bubble into the occupancy map before deciding what is
    // due, not just the banter ones the scheduler emitted itself.
    //
    // Bubbles carry no per-owner offset, so two live on one speaker draw on top
    // of each other. Post-gate that is reachable: ability bubbles render again
    // once the gates open, so a mid-fight shout landing while its speaker is
    // mid-"Mortal Strike!" would overlap it. The scheduler only knew about its
    // own emissions, which made the one-bubble-per-speaker rule true within
    // banter and false against the rest of the UI.
    scheduler.observe_live_bubbles(&bubbles);

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
/// Same lifecycle as `reset_call_watcher_on_exit`: the resource is
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

    // The queue mechanics are pure — `advance` a clock, `take_due` against a
    // liveness predicate — so almost everything below runs without a `World`.
    // Only the plumbing tests at the end build one.
    use super::super::resolver::ResolvedBeat;
    use super::super::test_fixtures::{beat, speaker, two_speaker};
    use super::super::watcher::{banter_context_for, CallChange, LastSeenCall};
    use crate::states::play_match::banter_config::{
        BanterContext, BanterExchange, BanterTiming, ClassConstraint,
    };

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

    use crate::states::play_match::components::PlayMatchEntity;

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
