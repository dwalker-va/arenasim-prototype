# In-match kill-target control + pre-match target banter

Status: **IDEA — seeded for a future session.** Not started. Captured while the
step-5 findings were fresh, because the feature's design constraints fall
directly out of them.

## The idea (as seeded)

1. **An in-match kill-target control.** Each team gets a control usable DURING
   the match to set its kill target — absolute semantics, identical to the
   existing pre-match "Kill Target Priority" control on the configure screen
   (the `teamN_kill_target` config fields). Today that order can only be given
   before the gates; this makes it live.
2. **Pre-match target banter.** If a team has a kill target set when the match
   starts, its members hold a brief HUMOROUS exchange about the choice in the
   starting area during the countdown — visualized with the existing speech
   bubbles and class icons. Pure character/flavor for the visual sim.
3. **Starting-room bubble cleanup (prerequisite).** Abilities cast during the
   countdown currently announce themselves in speech bubbles — the Mage yells
   "Frost Armor!" (and "Ice Barrier!") at nobody in the starting room. With
   banter added, the starting area becomes an actual scene, and rotation-buff
   announcements read as noise stepping on the dialogue. Clean up before or
   with the banter.

## Why this is worth doing (beyond flavor)

**The in-match control is the manual version of mid-match kill-target
switching** — which step 5 just promoted from "deferred enhancement" to
"prerequisite for the AI's team call" (see the step-5 amendment in
`team-level-positioning-ai.md`). Building the manual control first:

- exercises the exact consumer machinery the AI switch will need (a call that
  CHANGES mid-match, re-forcing the team onto the new target), on a code path
  where a human decides WHEN — so the hard, unsolved part (the switch-decision
  rule with commitment/hysteresis) is deferred while everything downstream of
  it gets built and debugged;
- turns the step-5 measurement table into a playable toy: the user can watch
  what focus-fire does to a comp live, including switching off a bad call —
  the thing the measured static calls could not do.

## Design constraints established this session (do not re-derive)

- **Semantics: absolute, same as the pre-match control.** The user order beats
  any AI opinion. When the AI switch eventually lands, the decision made on
  2026-08-06 stands: the manual order stays absolute — it is a
  debugging/experimentation lever and predictability is its value.
- **The step-5 table is the user-facing hazard.** A held call is worth +25 to
  +65pt set well and -48 to -69pt set badly (n=100 per cell; table in the
  design doc's step-5 amendment). The control has no guardrails and should not
  grow any — but the session doc for whoever builds the UI should consider a
  subtle affordance (e.g., showing the current call clearly enough that a bad
  one is noticeable).
- **Changing the call mid-match must re-force through the existing path**: the
  config-call re-force in `acquire_targets` (combat_ai.rs) already handles
  visibility/immunity fallback and the melee sticky-swap gate (bucket A). The
  in-match control should mutate `MatchConfig::teamN_kill_target` (or a
  successor resource) and let that existing machinery do the rest — do NOT
  build a second forcing path.
- **Banter must not touch `GameRng`.** Line selection / timing jitter must use
  a visual-only source (the established pattern: `drip_jitter`-style hashing in
  `rendering/effects.rs`, which deliberately never draws from `GameRng`).
  A seeded draw from the sim RNG would shift the draw order and break replay
  byte-identity. Same rule as every visual system.
- **Graphical-only, registered in `states/mod.rs`**, never in
  `add_core_combat_systems` — headless must not know banter exists. No
  baseline can move.

## Implementation notes (grounded in current code)

- **Speech bubbles**: `utils::spawn_speech_bubble(commands, owner, text)` —
  spawns a `SpeechBubble { owner, text, lifetime: 2.0 }`. Today every call site
  formats `"{ability}!"`. Banter wants arbitrary text and probably a longer /
  configurable lifetime, plus SEQUENCING (A speaks, then B replies) — a small
  countdown-scene system that schedules 2-3 bubbles with staggered start
  times. Class icons already render per-combatant; the conversation reads as
  "the Warrior said, the Priest replied" for free.
- **Countdown window**: the 10s pre-gate phase (`MatchCountdown`,
  `gates_opened == false`) is the banter stage. Combatants are parked in the
  starting rooms; camera now frames them correctly on every map
  (`arena_view_scale`).
- **The cleanup**: prep-phase ability bubbles come from the class-AI rotation
  running during countdown (buff priorities: Frost/Mage Armor, Ice Barrier,
  Battle Shout, PW:F, PW:S...). Options, pick during the session:
  (a) suppress ability speech bubbles while `!gates_opened` (one gate in
  `spawn_speech_bubble` or at its call sites — check whether any pre-gate
  bubble is genuinely wanted);
  (b) keep first-cast announcements but only post-gate re-casts get bubbles.
  Option (a) is simpler and probably right: the combat log still records the
  buffs, and the timeline UI shows them.
- **The in-match control surface**: the client already has click-selection
  (`selection.rs`, `Selection` resource) — "select an enemy, press a key /
  click a button to call it" is the natural gesture. Alternatively a small
  per-team panel. UI design is the session's main open question.
- **Who banter-speaks what**: a tiny line pool keyed on (speaker class,
  called-target class) with generic fallbacks. Keep it small and dry; the
  color-budget principle applies to text too — this is seasoning, not a
  dialogue system.

## Open questions for the session

- Does the in-match order persist across the called target's death (auto-clear
  vs. hold-until-changed)? Pre-match semantics auto-fall-back when invalid;
  probably keep that.
- Should changing the call mid-match trigger a (short, non-blocking) in-fight
  bubble ("Switch to the Priest!") to make the order legible? Cheap and
  probably worth it — it makes the mechanic visible in replays.
- Headless: should the in-match control have a scripted-timeline equivalent
  (e.g., "at t=30, team 1 calls slot 1") so switching effects are measurable
  without a human? Likely yes — it is also the harness the future AI-switching
  work will want for A/Bs.
