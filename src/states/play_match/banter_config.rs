//! Data-Driven Banter Configuration (team dialogue on a kill-call change)
//!
//! Mirrors the `movement_config.rs` loading pattern: serde structs with
//! container-level `#[serde(default)]`, direct `std::fs::read_to_string` +
//! `ron::from_str` (no asset server — keeps loading testable), `validate()`
//! accumulating every violation, a `Resource`, and a plugin that panics on
//! failure.
//!
//! ## Registered in `src/main.rs` ONLY — NOT in the headless runner
//!
//! This is the one deliberate deviation from `movement_config.rs`, whose
//! plugin registers in BOTH `src/headless/runner.rs` and `src/main.rs`. Read
//! side by side the asymmetry looks like the dual-registration bug this repo
//! has been burned by — it is not. Banter is visual-only and graphical-only:
//!
//! - Headless must never read `assets/config/banter.ron`, so a malformed or
//!   missing pool can never stop a balance sweep.
//! - Keeping the config out of the headless app is what stops it becoming a
//!   sim input by accident; nothing sim-side can read a resource that was
//!   never inserted.
//!
//! (KTD5 of `docs/plans/2026-08-06-001-feat-in-match-kill-call-and-banter-plan.md`.)
//!
//! ## Shape
//!
//! The authored unit is a whole *exchange* — an ordered list of beats, each
//! naming a speaker role — not a loose line, so a setup can never be stapled
//! to a punchline it was not written for. An exchange declares the
//! constraints a lineup must satisfy (`speakers[].class`, `target`); the
//! resolver (U5) filters the pool to the satisfiable ones, weights them by
//! specificity, and picks deterministically.
//!
//! Beat start times are DERIVED, never authored: beat `i` starts at
//! `opening_start + i * gap`, where `gap` is `correction_beat_gap` for the
//! `Correction` context and `beat_gap` otherwise. Validation works off those
//! derived times.
//!
//! ## Usage
//! ```ignore
//! fn my_system(banter: Res<BanterConfig>) {
//!     let lifetime = banter.timing.line_lifetime;
//!     let pool = banter.exchanges_for(BanterContext::Opening);
//! }
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::states::match_config::CharacterClass;

/// Which "the call changed" situation an exchange is written for.
///
/// One mechanism, three contexts — they differ only in beat count, pacing,
/// and which substitutions are available.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BanterContext {
    /// The countdown exchange: the first call of the match.
    Opening,
    /// The call was replaced before the gates opened. `{prev_target}` is
    /// available for substitution here and nowhere else.
    Correction,
    /// The call changed after the gates opened — a single-beat shout.
    Switch,
}

impl BanterContext {
    /// Every context, in the order validation reports coverage gaps.
    pub fn all() -> &'static [BanterContext] {
        &[
            BanterContext::Opening,
            BanterContext::Correction,
            BanterContext::Switch,
        ]
    }
}

/// A class requirement on a speaker role or on the called target.
///
/// `Any` is the unconstrained value and is what makes an exchange count
/// toward the per-context generic-coverage floor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClassConstraint {
    /// No requirement — any class satisfies this.
    #[default]
    Any,
    /// Satisfied only by this class.
    Class(CharacterClass),
}

impl ClassConstraint {
    /// Whether a combatant of `class` satisfies this constraint.
    pub fn is_satisfied_by(&self, class: CharacterClass) -> bool {
        match self {
            ClassConstraint::Any => true,
            ClassConstraint::Class(required) => *required == class,
        }
    }

    /// Whether this constraint narrows the pool (used for specificity
    /// weighting in the resolver and for the generic-coverage floor here).
    pub fn is_specific(&self) -> bool {
        matches!(self, ClassConstraint::Class(_))
    }
}

/// A speaker role declared by an exchange, plus the class it requires.
///
/// Roles are exchange-local labels (`"caller"`, `"responder"`) that the beats
/// reference; the resolver binds each to a distinct living combatant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BanterSpeaker {
    /// Exchange-local role label, referenced by `BanterBeat::role`.
    pub role: String,
    /// Class requirement on whoever gets bound to this role.
    #[serde(default)]
    pub class: ClassConstraint,
}

/// One spoken line: which role says it, and what they say.
///
/// `text` supports `{target}` in every context and `{prev_target}` in
/// `Correction` only. Substitution happens at resolve time (U5); this layer
/// only stores the strings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BanterBeat {
    /// Role saying this line — must be declared in the exchange's `speakers`.
    pub role: String,
    /// The line, with `{target}` / `{prev_target}` placeholders unresolved.
    pub text: String,
}

/// An authored exchange: the requirements it needs plus the beats it plays.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BanterExchange {
    /// Which call-change situation this exchange is written for.
    pub context: BanterContext,
    /// Roles this exchange needs bound, with their class requirements.
    #[serde(default)]
    pub speakers: Vec<BanterSpeaker>,
    /// Class requirement on the called target.
    #[serde(default)]
    pub target: ClassConstraint,
    /// The lines, in play order.
    #[serde(default)]
    pub beats: Vec<BanterBeat>,
}

impl BanterExchange {
    /// Count of non-`Any` constraints — the specificity the resolver weights
    /// by (KTD8), and the generic-coverage predicate here (`0` = fully
    /// generic).
    pub fn specificity(&self) -> u32 {
        let speaker_constraints = self
            .speakers
            .iter()
            .filter(|s| s.class.is_specific())
            .count() as u32;
        speaker_constraints + u32::from(self.target.is_specific())
    }

    /// Whether this exchange constrains nothing at all, so any lineup with
    /// enough combatants can play it. Every context needs at least one.
    pub fn is_fully_generic(&self) -> bool {
        self.specificity() == 0
    }
}

/// Beat pacing and bubble lifetime. All values in seconds except
/// `specificity_weight`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BanterTiming {
    /// Seconds after the call change before beat 0 speaks.
    pub opening_start: f32,
    /// Delay before a mid-fight `Switch` shout speaks. Near-immediate by
    /// design — the shout's job is to make the operator's order legible as it
    /// is given, so it does not wait out the countdown-oriented
    /// `opening_start`.
    pub switch_start: f32,
    /// Seconds between consecutive beats (`Opening` and `Switch`).
    pub beat_gap: f32,
    /// How long a bubble stays up. Bubbles carry no per-owner offset, so two
    /// beats on the SAME role closer together than this would draw on top of
    /// each other — validation rejects that.
    pub line_lifetime: f32,
    /// Beat gap for the `Correction` context — tighter, because a correction
    /// races the gates.
    pub correction_beat_gap: f32,
    /// No beat may start after this. The countdown is 10s, so a beat past it
    /// would be cut off by the gates opening.
    pub latest_beat: f32,
    /// Selection-weight multiplier applied once per non-`Any` constraint
    /// (KTD8). `1.0` makes specificity irrelevant; higher favours bespoke
    /// exchanges without ever excluding the generics.
    pub specificity_weight: f32,
}

impl Default for BanterTiming {
    fn default() -> Self {
        Self {
            opening_start: 1.5,
            // Near-immediate, but not near-zero: the deferral check below
            // requires `line_lifetime - switch_start < beat_gap` for any
            // context that could hold a multi-beat exchange, and 0.8 keeps the
            // defaults internally consistent for ANY pool rather than only the
            // shipped single-beat one.
            switch_start: 0.8,
            beat_gap: 3.0,
            line_lifetime: 3.6,
            correction_beat_gap: 2.4,
            latest_beat: 9.0,
            specificity_weight: 3.0,
        }
    }
}

impl BanterTiming {
    /// Gap between consecutive beats in `context`.
    fn beat_gap_for(&self, context: BanterContext) -> f32 {
        match context {
            BanterContext::Correction => self.correction_beat_gap,
            BanterContext::Opening | BanterContext::Switch => self.beat_gap,
        }
    }

    /// Delay before a context's FIRST beat speaks.
    ///
    /// `Opening` and `Correction` wait out `opening_start` because they play
    /// during the countdown, where combatants are still settling into the
    /// starting room and an instant line would land before the scene reads. A
    /// mid-fight `Switch` is the opposite case: it exists to make the
    /// operator's order legible the moment it is given, and inheriting a
    /// two-second delay would put the shout well after the team has already
    /// visibly re-targeted.
    fn start_delay_for(&self, context: BanterContext) -> f32 {
        match context {
            BanterContext::Switch => self.switch_start,
            BanterContext::Opening | BanterContext::Correction => self.opening_start,
        }
    }

    /// Derived start time of beat `index` in `context`. Beat times are never
    /// authored — they fall out of the pacing block.
    pub fn beat_start(&self, context: BanterContext, index: usize) -> f32 {
        self.start_delay_for(context) + index as f32 * self.beat_gap_for(context)
    }
}

/// Resource containing banter timing and the exchange pool.
///
/// Loaded from `assets/config/banter.ron` at GRAPHICAL startup only (see the
/// module doc). Access via `Res<BanterConfig>` in graphical systems.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BanterConfig {
    pub timing: BanterTiming,
    pub exchanges: Vec<BanterExchange>,
}

impl BanterConfig {
    /// Every exchange authored for `context`, in file order.
    pub fn exchanges_for(
        &self,
        context: BanterContext,
    ) -> impl Iterator<Item = &BanterExchange> {
        self.exchanges.iter().filter(move |e| e.context == context)
    }

    /// Characters in a line that egui cannot draw, ignoring `{...}` tokens.
    ///
    /// Token bodies are skipped because they hold class and ability NAMES —
    /// ordinary letters, resolved into icons long before anything is drawn.
    /// Only the literal text between tokens has to be speakable.
    fn unspeakable_chars(text: &str) -> Vec<char> {
        let mut bad = Vec::new();
        let mut in_token = false;
        for c in text.chars() {
            match c {
                '{' => in_token = true,
                '}' => in_token = false,
                _ if in_token => {}
                _ if crate::states::play_match::banter::vocab::is_speakable(c) => {}
                _ => {
                    if !bad.contains(&c) {
                        bad.push(c);
                    }
                }
            }
        }
        bad
    }

    /// Emoji names a line references that have no art on disk.
    ///
    /// The renderer degrades a missing emoji to a grey outline, which is fine
    /// as a runtime posture and useless as feedback — a typo'd `{emoji:skul}`
    /// would just be a faint box mid-match. Checking at load turns that into a
    /// startup failure naming the token, which matters more now that adding a
    /// symbol is "drop a file and reference it" and the reference is the only
    /// place a mistake can hide.
    ///
    /// Returns nothing when the directory is absent — a checkout without art
    /// should not fail validation, since the renderer already copes.
    fn missing_emoji(&self) -> Vec<String> {
        let dir = std::path::Path::new("assets/icons/emoji");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let available: std::collections::HashSet<String> = entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("png"))
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .collect();

        let mut missing = Vec::new();
        for exchange in &self.exchanges {
            for beat in &exchange.beats {
                for name in emoji_names(&beat.text) {
                    if !available.contains(&name) && !missing.contains(&name) {
                        missing.push(name);
                    }
                }
            }
        }
        missing
    }

    /// Check value and pool sanity. Returns the list of violations on
    /// failure — every offender is named, so one load reports every problem.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut issues: Vec<String> = Vec::new();
        let t = &self.timing;

        // --- Timing sanity -------------------------------------------------
        let positives = [
            ("timing.opening_start", t.opening_start),
            ("timing.beat_gap", t.beat_gap),
            ("timing.line_lifetime", t.line_lifetime),
            ("timing.correction_beat_gap", t.correction_beat_gap),
            ("timing.latest_beat", t.latest_beat),
            ("timing.switch_start", t.switch_start),
        ];
        for (name, value) in positives {
            if !(value > 0.0) || !value.is_finite() {
                issues.push(format!(
                    "{} must be a positive finite number, got {}",
                    name, value
                ));
            }
        }

        // The scheduler defers a beat whose speaker still has a live bubble to
        // that bubble's expiry, but it does NOT push the beats queued behind
        // it. The largest such push is `line_lifetime - start_delay`; if that
        // exceeds the gap between beats, a deferred setup and its untouched
        // punchline can land on the same frame and draw over each other.
        //
        // Checked only for contexts whose pool actually holds a multi-beat
        // exchange, because a single-beat context has nothing queued behind to
        // collide with — that is what lets `Switch` run its near-immediate
        // `switch_start` against the shared `line_lifetime`. Authoring a
        // two-beat `Switch` later turns the check on for it automatically.
        //
        // The shipped values satisfy this, which is exactly why it needs
        // checking: the safety is arithmetic, not structural, so a plausible
        // retune would reintroduce the collision silently.
        for (context, start, gap) in [
            (BanterContext::Opening, t.opening_start, t.beat_gap),
            (
                BanterContext::Correction,
                t.opening_start,
                t.correction_beat_gap,
            ),
            (BanterContext::Switch, t.switch_start, t.beat_gap),
        ] {
            let has_multi_beat = self
                .exchanges
                .iter()
                .any(|e| e.context == context && e.beats.len() > 1);
            if !has_multi_beat {
                continue;
            }
            let max_push = t.line_lifetime - start;
            if max_push >= gap {
                issues.push(format!(
                    "{:?}: timing.line_lifetime ({}) minus its start delay ({}) is {}, which is \
                     not below its beat gap ({}) — a deferred beat could collide with the one \
                     queued behind it",
                    context, t.line_lifetime, start, max_push, gap
                ));
            }
        }
        if !(t.specificity_weight >= 1.0) || !t.specificity_weight.is_finite() {
            issues.push(format!(
                "timing.specificity_weight must be a finite number >= 1.0 (below 1.0 would \
                 PENALISE specific exchanges), got {}",
                t.specificity_weight
            ));
        }

        // --- Per-exchange structure ---------------------------------------
        for (index, exchange) in self.exchanges.iter().enumerate() {
            let label = format!("exchanges[{}] ({:?})", index, exchange.context);

            // Roles are exchange-local labels; duplicates make role binding
            // ambiguous (which combatant is "caller"?).
            for (i, speaker) in exchange.speakers.iter().enumerate() {
                if exchange.speakers[..i].iter().any(|s| s.role == speaker.role) {
                    issues.push(format!(
                        "{}: duplicate speaker role '{}' — roles must be unique within an exchange",
                        label, speaker.role
                    ));
                }
            }

            // Every character outside a token must be one egui can draw.
            //
            // This is the guard that makes the pictographic vocabulary safe to
            // author. egui carries only a subset of emoji, so `→` and `✓`
            // render as tofu boxes while `➡` and `✔` render fine — a
            // difference invisible in the RON and glaring in the client.
            // Rejecting at load turns "shipped a box" into "the game refuses to
            // start and names the character".
            for (i, beat) in exchange.beats.iter().enumerate() {
                for bad in Self::unspeakable_chars(&beat.text) {
                    issues.push(format!(
                        "{}: beat {} uses '{}' (U+{:04X}), which is not in the approved glyph set \
                         — it would render as an empty box. See banter::vocab::GLYPHS.",
                        label, i, bad, bad as u32
                    ));
                }
            }

            // Every beat must name a declared role, or it can never bind.
            for (i, beat) in exchange.beats.iter().enumerate() {
                if !exchange.speakers.iter().any(|s| s.role == beat.role) {
                    issues.push(format!(
                        "{}: beat {} references role '{}', which is not declared in `speakers`",
                        label, i, beat.role
                    ));
                }
            }

            // Derived beat times must fit inside the countdown window.
            for (i, _beat) in exchange.beats.iter().enumerate() {
                let start = t.beat_start(exchange.context, i);
                if start > t.latest_beat {
                    issues.push(format!(
                        "{}: beat {} starts at {}s, past timing.latest_beat ({}s)",
                        label, i, start, t.latest_beat
                    ));
                }
            }

            // Two beats on ONE role closer than the bubble lifetime would
            // draw the second bubble on top of the first — bubble rendering
            // applies no per-owner offset (AE4).
            for (i, beat) in exchange.beats.iter().enumerate() {
                for (j, other) in exchange.beats.iter().enumerate().skip(i + 1) {
                    if beat.role != other.role {
                        continue;
                    }
                    let start_i = t.beat_start(exchange.context, i);
                    let start_j = t.beat_start(exchange.context, j);
                    if (start_j - start_i).abs() < t.line_lifetime {
                        issues.push(format!(
                            "{}: role '{}' speaks at {}s and {}s, closer than \
                             timing.line_lifetime ({}s) — the second bubble would draw on top \
                             of the first",
                            label, beat.role, start_i, start_j, t.line_lifetime
                        ));
                    }
                }
            }
        }

        // --- Generic-coverage floor (AE6) ----------------------------------
        // Every context needs at least one exchange with NO constraints, so
        // the resolver can never come up empty for a satisfiable lineup.
        for context in BanterContext::all() {
            if !self
                .exchanges_for(*context)
                .any(BanterExchange::is_fully_generic)
            {
                issues.push(format!(
                    "context {:?} has no fully-generic exchange (all speaker classes Any AND \
                     target Any) — every context needs one so resolution can never fail",
                    context
                ));
            }
        }

        for name in self.missing_emoji() {
            issues.push(format!(
                "no emoji art for '{}' — expected assets/icons/emoji/{}.png (see that \
                 directory's ATTRIBUTION.md for how to add one)",
                name, name
            ));
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }
}

/// Parse a banter config from RON text. `source` names the origin for error
/// messages (a path, or "inline" in tests).
pub fn parse_banter_config(contents: &str, source: &str) -> Result<BanterConfig, String> {
    let config: BanterConfig =
        ron::from_str(contents).map_err(|e| format!("Failed to parse {}: {}", source, e))?;

    config
        .validate()
        .map_err(|issues| format!("Invalid banter config in {}:\n  {}", source, issues.join("\n  ")))?;

    Ok(config)
}

/// Load and validate a banter config from a RON file path.
pub fn load_banter_config_from(path: &str) -> Result<BanterConfig, String> {
    let contents =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
    parse_banter_config(&contents, path)
}

/// Load banter configuration from assets/config/banter.ron
pub fn load_banter_config() -> Result<BanterConfig, String> {
    let config_path = "assets/config/banter.ron";
    let config = load_banter_config_from(config_path)?;
    info!(
        "Loaded banter configuration from {} ({} exchanges)",
        config_path,
        config.exchanges.len()
    );
    Ok(config)
}

/// Bevy plugin for banter configuration loading.
///
/// Registered in `src/main.rs` ONLY — deliberately NOT in
/// `src/headless/runner.rs`. See the module doc: banter is graphical-only, so
/// headless never reads `banter.ron` and the pool can never become a sim
/// input.
pub struct BanterConfigPlugin;

impl Plugin for BanterConfigPlugin {
    fn build(&self, app: &mut App) {
        match load_banter_config() {
            Ok(config) => {
                app.insert_resource(config);
            }
            Err(e) => {
                // Panic to ensure the config is always valid at startup —
                // same policy as AbilityConfigPlugin / MovementConfigPlugin.
                panic!("Failed to load banter configuration: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-beat generic exchange in `context`, used to build inline pools.
    fn generic_exchange(context: BanterContext) -> BanterExchange {
        BanterExchange {
            context,
            speakers: vec![
                BanterSpeaker { role: "caller".to_string(), class: ClassConstraint::Any },
                BanterSpeaker { role: "responder".to_string(), class: ClassConstraint::Any },
            ],
            target: ClassConstraint::Any,
            beats: vec![
                BanterBeat {
                    role: "caller".to_string(),
                    text: "{ability:Mortal Strike} {emoji:arrow} {target}".to_string(),
                },
                BanterBeat { role: "responder".to_string(), text: "{emoji:yes}".to_string() },
            ],
        }
    }

    /// A pool that satisfies the coverage floor for all three contexts, so a
    /// test can inject one broken exchange and see only that violation.
    fn covered_config() -> BanterConfig {
        BanterConfig {
            timing: BanterTiming::default(),
            exchanges: BanterContext::all().iter().copied().map(generic_exchange).collect(),
        }
    }

    /// The shipped banter.ron loads, parses, and validates.
    ///
    /// Pacing values are deliberately NOT pinned to literals — they are a feel
    /// knob and expected to move. What is pinned is the bound that keeps the
    /// countdown coherent, and the property that makes lines readable at all.
    #[test]
    fn shipped_banter_ron_loads_and_validates() {
        let config = load_banter_config().expect("assets/config/banter.ron must load");
        assert!(
            config.timing.latest_beat < 10.0,
            "latest_beat must sit inside the 10s countdown, got {}",
            config.timing.latest_beat
        );
        assert!(
            config.timing.line_lifetime > config.timing.opening_start,
            "a bubble must outlive the delay before the first one speaks, or an \
             exchange's opening line is gone before the reply arrives: lifetime {} vs start {}",
            config.timing.line_lifetime,
            config.timing.opening_start
        );
        for context in BanterContext::all() {
            assert!(
                config.exchanges_for(*context).next().is_some(),
                "shipped pool must carry at least one {:?} exchange",
                context
            );
        }
    }

    /// Every shipped exchange needs two distinct speakers, which is what keeps
    /// a 1v1 team silent.
    ///
    /// The decision is that a solo combatant never speaks — not in the
    /// countdown, and not on a mid-fight switch either. A lone fighter
    /// announcing a target to an empty arena is the same noise the pre-gate
    /// ability-bubble cleanup exists to remove.
    ///
    /// Nothing in the schema enforces that: a one-speaker exchange is
    /// perfectly valid, and the `Switch` entries in particular *look* like
    /// they only need one (they have a single beat). They carry a silent
    /// "witness" role precisely so a solo lineup cannot bind them. Without
    /// this test, appending one well-meaning single-speaker entry would make
    /// 1v1 combatants start talking to themselves with nothing failing.
    #[test]
    fn shipped_exchanges_require_two_speakers_so_1v1_stays_silent() {
        let config = load_banter_config().expect("assets/config/banter.ron must load");
        for (index, exchange) in config.exchanges.iter().enumerate() {
            assert!(
                exchange.speakers.len() >= 2,
                "exchange {} ({:?}) declares {} speaker role(s); it needs at least 2 or a solo \
                 combatant will satisfy it and break 1v1 silence",
                index,
                exchange.context,
                exchange.speakers.len()
            );
        }
    }

    /// Every `{ability:...}` the shipped pool names must be a real ability.
    ///
    /// `validate()` catches a typo'd `{emoji:...}` because the art is a
    /// directory it can read; ability names have no such check at load, and
    /// the renderer degrades an unknown one to a faint grey outline — a
    /// content mistake that is invisible in the RON and nearly invisible in
    /// the client. This test is that missing guard, run from `cargo test`.
    #[test]
    fn shipped_banter_only_names_real_abilities() {
        use crate::states::play_match::ability_config::load_ability_definitions;
        use crate::states::play_match::banter::vocab;

        let config = load_banter_config().expect("assets/config/banter.ron must load");
        let definitions =
            load_ability_definitions().expect("assets/config/abilities.ron must load");
        let known: std::collections::HashSet<&str> =
            definitions.iter().map(|(_, c)| c.name.as_str()).collect();

        for (index, exchange) in config.exchanges.iter().enumerate() {
            for (i, beat) in exchange.beats.iter().enumerate() {
                for span in vocab::parse(&beat.text) {
                    if let vocab::Span::Ability(name) = span {
                        assert!(
                            known.contains(name.as_str()),
                            "exchanges[{}] beat {} names ability '{}', which is not in \
                             abilities.ron — it would render as an empty outline",
                            index,
                            i,
                            name
                        );
                    }
                }
            }
        }
    }

    /// Missing file → loader error with a clear message. The plugin panics
    /// with this exact string, so testing the loader covers the panic path
    /// without aborting the test binary.
    #[test]
    fn missing_file_yields_clear_error() {
        let err = load_banter_config_from("assets/config/does_not_exist.ron")
            .expect_err("missing file must fail");
        assert!(
            err.contains("Failed to read assets/config/does_not_exist.ron"),
            "error should name the missing path: {}",
            err
        );
    }

    #[test]
    fn malformed_ron_yields_parse_error() {
        let err = parse_banter_config("(timing: (beat_gap: \"not a number\"))", "inline")
            .expect_err("malformed RON must fail");
        assert!(err.contains("Failed to parse inline"), "got: {}", err);
    }

    /// AE6: a context whose entries all carry class constraints has no
    /// guaranteed resolution, so the load fails.
    #[test]
    fn validate_rejects_missing_generic_coverage() {
        let mut config = covered_config();
        // Constrain every Opening entry — Correction and Switch stay generic.
        for exchange in config
            .exchanges
            .iter_mut()
            .filter(|e| e.context == BanterContext::Opening)
        {
            exchange.target = ClassConstraint::Class(CharacterClass::Warrior);
        }
        let issues = config
            .validate()
            .expect_err("a context with no generic exchange must fail");
        assert!(
            issues
                .iter()
                .any(|i| i.contains("Opening") && i.contains("fully-generic")),
            "issues should name the uncovered context: {:?}",
            issues
        );
        assert!(
            !issues.iter().any(|i| i.contains("Switch")),
            "still-covered contexts must not be reported: {:?}",
            issues
        );
    }

    /// AE4: consecutive beats on one role sit `beat_gap` apart, which is
    /// below `line_lifetime` — the second bubble would draw over the first.
    #[test]
    fn validate_rejects_same_role_beats_within_line_lifetime() {
        let mut config = covered_config();
        let exchange = config
            .exchanges
            .iter_mut()
            .find(|e| e.context == BanterContext::Opening)
            .expect("fixture has an Opening exchange");
        exchange.beats[1].role = "caller".to_string();
        assert!(
            config.timing.beat_gap < config.timing.line_lifetime,
            "fixture assumes adjacent beats collide"
        );
        let issues = config
            .validate()
            .expect_err("same-role beats inside the bubble lifetime must fail");
        assert!(
            issues
                .iter()
                .any(|i| i.contains("'caller'") && i.contains("line_lifetime")),
            "issues should name the colliding role: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_beat_referencing_undeclared_role() {
        let mut config = covered_config();
        config.exchanges[0].beats[1].role = "heckler".to_string();
        let issues = config
            .validate()
            .expect_err("a beat on an undeclared role must fail");
        assert!(
            issues
                .iter()
                .any(|i| i.contains("'heckler'") && i.contains("speakers")),
            "issues should name the undeclared role: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_duplicate_speaker_roles() {
        let mut config = covered_config();
        config.exchanges[0].speakers[1].role = "caller".to_string();
        config.exchanges[0].beats[1].role = "caller".to_string();
        let issues = config
            .validate()
            .expect_err("duplicate roles must fail");
        assert!(
            issues.iter().any(|i| i.contains("duplicate speaker role")),
            "issues should report the duplicate: {:?}",
            issues
        );
    }

    /// Beat times are derived, so "too late" is a function of the pacing —
    /// enough beats at the shipped gap runs past the gates.
    #[test]
    fn validate_rejects_beat_past_latest_beat() {
        let mut config = covered_config();
        let exchange = config
            .exchanges
            .iter_mut()
            .find(|e| e.context == BanterContext::Opening)
            .expect("fixture has an Opening exchange");
        // opening_start 2.0 + 4 * beat_gap 2.2 = 10.8 > latest_beat 9.0.
        for i in 2..5 {
            let role = if i % 2 == 0 { "caller" } else { "responder" };
            exchange.beats.push(BanterBeat {
                role: role.to_string(),
                text: format!("Beat {}.", i),
            });
        }
        let issues = config
            .validate()
            .expect_err("a beat past latest_beat must fail");
        assert!(
            issues.iter().any(|i| i.contains("latest_beat")),
            "issues should name latest_beat: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_specificity_weight_below_one() {
        let mut config = covered_config();
        config.timing.specificity_weight = 0.5;
        let issues = config
            .validate()
            .expect_err("specificity_weight below 1.0 must fail");
        assert!(
            issues.iter().any(|i| i.contains("timing.specificity_weight")),
            "issues should name specificity_weight: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_nonpositive_line_lifetime() {
        let mut config = covered_config();
        config.timing.line_lifetime = 0.0;
        let issues = config
            .validate()
            .expect_err("line_lifetime 0 must fail validation");
        assert!(
            issues.iter().any(|i| i.contains("timing.line_lifetime")),
            "issues should name line_lifetime: {:?}",
            issues
        );
    }

    /// Partial RON files fill missing fields from the struct defaults
    /// (serde(default) at container level) — a timing tweak can override one
    /// value without restating the rest. Validation is skipped here because
    /// an exchange-less pool cannot meet the coverage floor.
    #[test]
    fn partial_ron_uses_defaults() {
        let config: BanterConfig = ron::from_str("(timing: (beat_gap: 9.5))")
            .expect("partial config must parse");
        assert_eq!(config.timing.beat_gap, 9.5, "the stated field wins");
        // Compared against the default rather than a literal: this test is
        // about serde filling the gaps, not about the current pacing, and
        // hardcoding the number makes every retune fail an unrelated test.
        assert_eq!(
            config.timing.line_lifetime,
            BanterTiming::default().line_lifetime,
            "unspecified fields use defaults"
        );
        assert_eq!(config.timing.specificity_weight, 3.0);
        assert!(config.exchanges.is_empty(), "an omitted pool defaults to empty");
    }

    /// A partially-specified exchange also fills from defaults — an entry
    /// that states no `target` is unconstrained, not malformed.
    #[test]
    fn partial_exchange_uses_default_constraints() {
        let config: BanterConfig = ron::from_str(
            r#"(exchanges: [(
                context: Switch,
                speakers: [(role: "caller")],
                beats: [(role: "caller", text: "New call: {target}.")],
            )])"#,
        )
        .expect("partial exchange must parse");
        let exchange = &config.exchanges[0];
        assert_eq!(exchange.target, ClassConstraint::Any);
        assert_eq!(exchange.speakers[0].class, ClassConstraint::Any);
        assert!(exchange.is_fully_generic());
    }

    #[test]
    fn defaults_pass_validation() {
        // The built-in timing defaults are internally consistent; the pool is
        // content, so the coverage floor is checked against a covered pool.
        covered_config()
            .validate()
            .expect("built-in defaults must be internally consistent");
    }

    /// Specificity counts every non-`Any` constraint — the weight the
    /// resolver multiplies by (KTD8) and the coverage predicate here.
    #[test]
    fn specificity_counts_non_any_constraints() {
        let mut exchange = generic_exchange(BanterContext::Opening);
        assert_eq!(exchange.specificity(), 0);
        assert!(exchange.is_fully_generic());

        exchange.target = ClassConstraint::Class(CharacterClass::Mage);
        assert_eq!(exchange.specificity(), 1);
        assert!(!exchange.is_fully_generic());

        exchange.speakers[0].class = ClassConstraint::Class(CharacterClass::Priest);
        assert_eq!(exchange.specificity(), 2);
    }

    /// Beat times are derived from the pacing block, not authored.
    ///
    /// Asserted against the timing fields rather than literals so retuning the
    /// pacing (which is expected — it is a feel knob) does not fail a test
    /// about the derivation.
    #[test]
    fn beat_start_derives_from_context_gap() {
        let t = BanterTiming::default();
        assert_eq!(t.beat_start(BanterContext::Opening, 0), t.opening_start);
        assert_eq!(
            t.beat_start(BanterContext::Opening, 1),
            t.opening_start + t.beat_gap
        );
        assert_eq!(t.beat_start(BanterContext::Correction, 0), t.opening_start);
        assert_eq!(
            t.beat_start(BanterContext::Correction, 1),
            t.opening_start + t.correction_beat_gap,
            "Correction paces tighter between beats than Opening"
        );
        assert!(
            t.correction_beat_gap < t.beat_gap,
            "a correction races the gates, so it must be the tighter of the two"
        );
    }

    /// A mid-fight shout does NOT wait out the countdown-oriented
    /// `opening_start`.
    ///
    /// The switch shout exists to make the operator's order legible the moment
    /// it is given; inheriting the two-second opening delay put it well after
    /// the team had already visibly re-targeted, which is the tell that the
    /// bubble is decoration rather than feedback.
    #[test]
    fn switch_shout_starts_almost_immediately() {
        let t = BanterTiming::default();
        assert_eq!(t.beat_start(BanterContext::Switch, 0), t.switch_start);
        assert!(
            t.beat_start(BanterContext::Switch, 0) < t.beat_start(BanterContext::Opening, 0),
            "a mid-fight shout must land sooner than a countdown opener: switch {} vs opening {}",
            t.beat_start(BanterContext::Switch, 0),
            t.beat_start(BanterContext::Opening, 0),
        );
    }
}

/// Every `{emoji:<name>}` referenced by a line.
///
/// A tiny scanner rather than a call into `banter::vocab::parse`, so config
/// validation does not depend on the renderer's span model — the two answer
/// different questions and should be able to change independently.
fn emoji_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{emoji:") {
        let after = &rest[start + "{emoji:".len()..];
        let Some(end) = after.find('}') else { break };
        let name = &after[..end];
        if !name.is_empty() {
            names.push(name.to_string());
        }
        rest = &after[end + 1..];
    }
    names
}
