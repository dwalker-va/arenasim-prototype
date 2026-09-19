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
use super::resolver::{
    cc_target_class, resolve_exchange, BanterCall, BanterCombatant, BanterLineup, ResolvedExchange,
};
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
//     other and neither is readable. Enforced at EMISSION, in
//     [`play_banter_beats`]: a speaker who starts a new line while their last
//     is still up has that bubble despawned, so the new line REPLACES it.
//     Neither the config nor the queue prevents the overlap — a pool may put
//     consecutive beats on ONE role, and that self-interruption is an authored
//     device (one party overcommunicating), not an error to be scheduled away.
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
    /// Written once, by [`BanterScheduler::queue_exchange`], and only read
    /// afterwards — nothing moves a beat once it is queued. That is precisely
    /// what holds an exchange to its AUTHORED pacing: a speaker who is already
    /// talking does not push their next beat back, they replace their own
    /// bubble.
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
    /// Each team's queue is walked in ascending `at` and STOPS at the first
    /// beat that is not due yet, so an exchange always plays in sequence and
    /// can never deliver its punchline before its setup.
    ///
    /// A beat is NEVER held back because its speaker is already talking. A
    /// speaker who starts a new line while their last is still up REPLACES it
    /// (see `play_banter_beats`) — self-interruption, like chat. That is the
    /// point: consecutive beats on one role are an authored device, and the
    /// rapid-fire replacement is what reads as one party overcommunicating.
    /// Holding the beat instead would stretch such a run to one `line_lifetime`
    /// per beat and spill it past the gates.
    fn take_due(&mut self, is_alive: impl Fn(Entity) -> bool) -> Vec<PendingBeat> {
        let mut due: Vec<PendingBeat> = Vec::new();

        for index in 0..self.queues.len() {
            // Queues hold at most a handful of beats, so the front-removal
            // cost of a `Vec` is not worth a `VecDeque`'s extra type noise.
            while let Some(beat) = self.queues[index].first() {
                if beat.at > self.clock {
                    break;
                }
                if !is_alive(beat.speaker) {
                    self.queues[index].remove(0);
                    continue;
                }
                let beat = self.queues[index].remove(0);
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
fn team_rosters(
    combatants: &Query<(Entity, &Combatant), Without<Pet>>,
) -> [Vec<BanterCombatant>; 2] {
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
/// LATE BEATS MAY LAND AFTER THE GATES, and that is accepted — by ruling
/// (2026-09-14), a whole exchange may spill into the opening approach:
/// `latest_beat` bounds a beat's offset within an exchange but sits past the
/// 10s countdown, because the walk across the arena's dead space leaves ample
/// time to finish a conversation before contact. Likewise a correction made
/// at t=9.5 schedules beats into the fight; suppressing either would silently
/// swallow a call the operator just made.
pub fn play_banter_beats(
    mut commands: Commands,
    time: Res<Time>,
    banter_config: Option<Res<BanterConfig>>,
    rng: Option<Res<GameRng>>,
    mut watcher: ResMut<CallWatcher>,
    mut scheduler: ResMut<BanterScheduler>,
    combatants: Query<(Entity, &Combatant), Without<Pet>>,
    bubbles: Query<(Entity, &SpeechBubble)>,
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
                // Computed from the same roster `{target}` indexes, so "not
                // the kill target" is the call slot and not a class match.
                cc_target: cc_target_class(enemies, change.new_call),
                // Teams are 1 and 2, so the opposition is whichever this is not.
                enemy_team: if change.team == 1 { 2 } else { 1 },
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

    // One bubble per speaker: a new line REPLACES that speaker's live one.
    //
    // Bubbles carry no per-owner offset, so two live on one speaker would draw
    // on top of each other. Replacement — not a layout offset, and not holding
    // the new line back — is what keeps that impossible, and self-interruption
    // is the intended read for consecutive beats on one role.
    //
    // Seeded from the World rather than from the scheduler's own bookkeeping,
    // so the rule holds against any bubble, not only the ones banter emitted.
    // The map is kept current as beats spawn, which also covers two beats for
    // one speaker falling due on the SAME frame: `Commands` are deferred, so a
    // bubble spawned here is not yet visible to `bubbles`.
    let mut live: HashMap<Entity, Entity> = bubbles
        .iter()
        .map(|(entity, bubble)| (bubble.owner, entity))
        .collect();
    for beat in due {
        let speaker = beat.speaker;
        let bubble = spawn_speech_line(&mut commands, speaker, beat.text, beat.lifetime);
        if let Some(previous) = live.insert(speaker, bubble) {
            commands.entity(previous).try_despawn();
        }
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
        assert!(
            step(&mut scheduler, 1.0).is_empty(),
            "beat 1 is not due yet"
        );
        assert_eq!(step(&mut scheduler, 1.5), vec![(BEA, "On it.".to_string())]);
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
        assert!(
            scheduler.queues[0].is_empty(),
            "the opening is fully played"
        );

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

    /// One bubble per speaker, ACROSS exchanges — by REPLACEMENT, not by
    /// waiting. A correction landing while an opening beat from the same
    /// speaker is still up speaks at once and takes the bubble over; holding it
    /// back would delay the operator's own correction to tell them nothing new.
    #[test]
    fn a_second_line_on_one_speaker_speaks_at_once() {
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

        assert_eq!(
            scheduler.take_due(all_alive).len(),
            1,
            "the correction speaks now — the live bubble is replaced, not waited out"
        );
        assert!(
            scheduler.queues[0].is_empty(),
            "and nothing is left queued behind it"
        );
    }

    /// Beats keep their authored times and their order. A speaker already
    /// mid-bubble no longer holds anything back, so the setup speaks on time
    /// (replacing the line before it) and the punchline still waits for its own
    /// beat rather than arriving alongside it.
    #[test]
    fn beats_keep_their_order_and_their_pacing() {
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

        // ...and the setup speaks straight over it, on its authored beat.
        assert_eq!(step(&mut scheduler, 0.0), vec![(ALEX, "setup".to_string())]);
        assert!(
            step(&mut scheduler, 0.2).is_empty(),
            "the punchline must not jump forward to fill the gap"
        );
        assert_eq!(
            step(&mut scheduler, 0.3),
            vec![(BEA, "punchline".to_string())],
            "it lands on its own beat time"
        );
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

    fn change(
        team: u8,
        new_call: Option<usize>,
        previous: LastSeenCall,
        gates: bool,
    ) -> CallChange {
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
        assert_eq!(
            spoken[0].1, "Switch to the {class:Warlock:2}!",
            "slot 1 of team 2, resolved to a portrait token the renderer can tint"
        );

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

    /// The one-bubble-per-speaker rule is seeded from the WORLD, not from the
    /// scheduler's own emissions, so a bubble it never queued is replaced too.
    ///
    /// The pure-value tests above cover the queue mechanics; this is the half
    /// that needs a World, and the only thing that would notice if the
    /// replacement map stopped reading live bubbles.
    #[test]
    fn a_live_bubble_the_scheduler_did_not_emit_is_replaced() {
        let mut world = banter_world(
            &[CharacterClass::Warrior, CharacterClass::Priest],
            &[CharacterClass::Mage, CharacterClass::Warlock],
        );
        // The caller binds in slot order, so team 1 slot 0 speaks beat 0.
        let caller = world
            .query::<(Entity, &Combatant)>()
            .iter(&world)
            .find(|(_, c)| c.team == 1 && c.slot == 0)
            .map(|(e, _)| e)
            .expect("team 1 slot 0 exists");
        let blocker = world
            .spawn(SpeechBubble {
                owner: caller,
                text: "already talking".to_string(),
                lifetime: 2.0,
            })
            .id();
        push_change(
            &mut world,
            change(1, Some(0), LastSeenCall::NeverObserved, false),
        );

        // opening_start is 1.0, so beat 0 is due here — and it TAKES OVER the
        // bubble already on the caller's head rather than stacking on it.
        run_scheduler(&mut world, 0.1);
        run_scheduler(&mut world, 1.0);

        let on_caller: Vec<String> = bubbles(&mut world)
            .into_iter()
            .filter(|(owner, _)| *owner == caller)
            .map(|(_, text)| text)
            .collect();
        assert_eq!(
            on_caller.len(),
            1,
            "exactly one bubble may be live on a speaker, got {:?}",
            on_caller
        );
        assert!(
            on_caller[0].contains("{class:"),
            "and it is the new beat, not the line it replaced: {:?}",
            on_caller
        );
        assert!(
            world.get_entity(blocker).is_err(),
            "the replaced bubble is despawned, not merely hidden"
        );
    }

    /// A same-role RUN plays as clean sequential replacement: never two
    /// bubbles on one head, and the lines arrive in authored order.
    ///
    /// This is the property that had to hold before the validator's same-role
    /// rule could go. Three beats on ONE role, each landing while the previous
    /// bubble is still live, is the shape that rule used to reject — and the
    /// shape an authored "this speaker is overcommunicating" run needs.
    #[test]
    fn a_same_role_run_replaces_rather_than_stacking() {
        let timing = BanterTiming {
            opening_start: 1.0,
            switch_start: 0.1,
            beat_gap: 1.0,
            // ABOVE the beat gap on purpose: every beat lands while the
            // previous bubble is still up, so every one of them replaces.
            line_lifetime: 3.0,
            correction_beat_gap: 1.0,
            latest_beat: 9.0,
            specificity_weight: 3.0,
        };
        let run = BanterExchange {
            context: BanterContext::Opening,
            speakers: vec![
                speaker("caller", ClassConstraint::Any),
                speaker("responder", ClassConstraint::Any),
            ],
            target: ClassConstraint::Any,
            beats: vec![
                beat("caller", "one"),
                beat("caller", "two"),
                beat("caller", "three"),
            ],
        };
        let mut world = banter_world(
            &[CharacterClass::Warrior, CharacterClass::Priest],
            &[CharacterClass::Mage, CharacterClass::Warlock],
        );
        world.insert_resource(BanterConfig {
            timing,
            exchanges: vec![run],
        });
        push_change(
            &mut world,
            change(1, Some(0), LastSeenCall::NeverObserved, false),
        );

        // Bubbles never expire here — `update_speech_bubbles` is not in this
        // harness — so anything still live is something replacement failed to
        // clear, which is exactly what the per-frame check wants to catch.
        let mut spoken: Vec<String> = Vec::new();
        for _ in 0..40 {
            run_scheduler(&mut world, 0.1);
            let live = bubbles(&mut world);

            let mut owners: Vec<Entity> = live.iter().map(|(owner, _)| *owner).collect();
            let before = owners.len();
            owners.sort();
            owners.dedup();
            assert_eq!(
                owners.len(),
                before,
                "two bubbles live on one speaker — replacement failed: {:?}",
                live
            );

            if let Some((_, text)) = live.first() {
                if spoken.last() != Some(text) {
                    spoken.push(text.clone());
                }
            }
        }

        assert_eq!(
            spoken,
            vec!["one".to_string(), "two".to_string(), "three".to_string()],
            "the run must play in authored order, each line taking over from the last"
        );
    }

    /// The scheduler is the only place that knows the enemy roster, so this is
    /// the half of `{cctarget}` the resolver's own suite cannot reach: that the
    /// off-target enemy handed to the resolver is the one actually standing on
    /// the other side of the arena.
    #[test]
    fn the_cc_target_names_the_enemy_the_call_did_not() {
        let mut world = banter_world(
            &[CharacterClass::Warrior, CharacterClass::Hunter],
            &[CharacterClass::Mage, CharacterClass::Priest],
        );
        let mut config = scheduler_config();
        config.exchanges = vec![BanterExchange {
            context: BanterContext::Opening,
            speakers: vec![
                speaker("caller", ClassConstraint::Any),
                speaker("responder", ClassConstraint::Any),
            ],
            target: ClassConstraint::Any,
            beats: vec![beat(
                "caller",
                "{ability:Freezing Trap} {cctarget} , {target}",
            )],
        }];
        world.insert_resource(config);
        // Call slot 0 of team 2 — the Mage. The Priest is the off-target
        // healer, which is what the token must find.
        push_change(
            &mut world,
            change(1, Some(0), LastSeenCall::NeverObserved, false),
        );

        run_scheduler(&mut world, 0.1);
        run_scheduler(&mut world, 1.0);
        let spoken = bubbles(&mut world);
        assert_eq!(spoken.len(), 1);
        assert_eq!(
            spoken[0].1,
            "{ability:Freezing Trap} {class:Priest:2} , {class:Mage:2}"
        );
    }

    /// ...and in a lineup where the call is the whole enemy side, that same
    /// pool is silent rather than speaking a line about nobody.
    #[test]
    fn a_cc_target_exchange_is_silent_when_the_call_is_the_only_enemy() {
        let mut world = banter_world(
            &[CharacterClass::Warrior, CharacterClass::Hunter],
            &[CharacterClass::Mage],
        );
        let mut config = scheduler_config();
        config.exchanges = vec![BanterExchange {
            context: BanterContext::Opening,
            speakers: vec![
                speaker("caller", ClassConstraint::Any),
                speaker("responder", ClassConstraint::Any),
            ],
            target: ClassConstraint::Any,
            beats: vec![beat("caller", "{ability:Freezing Trap} {cctarget}")],
        }];
        world.insert_resource(config);
        push_change(
            &mut world,
            change(1, Some(0), LastSeenCall::NeverObserved, false),
        );

        for _ in 0..8 {
            run_scheduler(&mut world, 1.0);
        }
        assert!(
            bubbles(&mut world).is_empty(),
            "a 2v1 has nobody to trap, so the exchange never plays"
        );
        assert!(world.resource::<BanterScheduler>().queues[0].is_empty());
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
