//! The pure resolution pipeline: a call change in, a bound and substituted
//! exchange out.
//!
//! `watcher` reports THAT a call moved; this file decides WHAT gets said.
//! Everything here is a pure function over plain data — no `World`, no
//! `Commands`, no `Res` — so none of the interesting logic needs an app to
//! test. `scheduler` owns the Bevy plumbing that gathers these inputs and turns
//! a [`ResolvedExchange`] into speech bubbles.

use bevy::prelude::*;

use super::super::banter_config::{BanterConfig, BanterContext, BanterExchange};
use super::super::match_config::CharacterClass;

// =============================================================================
// Exchange resolution (KTD7, KTD8, KTD10)
// =============================================================================
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
pub(super) struct BanterCombatant {
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
pub(super) struct BanterLineup {
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
pub(super) struct BanterCall {
    /// Class of the newly-called combatant.
    pub target: Option<CharacterClass>,
    /// Class of the combatant the call replaced. Only ever substituted in
    /// `Correction`.
    pub prev_target: Option<CharacterClass>,
}

/// One beat, ready to spawn: who says it, what it says, and when.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ResolvedBeat {
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
pub(super) struct ResolvedExchange {
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
pub(super) fn resolve_exchange(
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
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // All of this is pure over plain data, so the fixtures below are hand-built
    // `BanterConfig`s rather than the shipped `banter.ron` — a content edit must
    // never be able to fail a resolver test. `speaker` / `beat` / `two_speaker`
    // are shared with the scheduler's suite, so they live in the parent's
    // `test_fixtures`.
    use super::super::test_fixtures::{beat, speaker, two_speaker};
    use crate::states::play_match::banter_config::{
        BanterExchange, BanterTiming, ClassConstraint,
    };

    /// Seed used wherever a test needs *a* seed and does not care which.
    const A_SEED: Option<u64> = Some(0xC0FF_EE12);

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
}
