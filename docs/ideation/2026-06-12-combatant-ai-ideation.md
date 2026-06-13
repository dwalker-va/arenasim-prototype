---
date: 2026-06-12
topic: combatant-ai
focus: movement system next steps; whether a similar data-driven system fits ability usage
mode: repo-grounded
---

# Ideation: Combatant AI — Movement Next Steps & Ability-Decision Architecture

Six ideation frames (pain/friction, inversion, assumption-breaking, leverage, cross-domain analogy, constraint-flipping) generated ~48 raw ideas, deduped to 21, filtered to 7 survivors. Notable convergence: all six frames independently proposed the intent/commit two-phase redesign (#5); five independently proposed the gate+utility hybrid (#1).

## Grounding Context

**Codebase context:** Rust/Bevy autobattler. Ability *mechanics* are data-driven (`abilities.ron`); healer movement is fully data-driven (`movement.ron`: posture FSM FREE/PRESSURED/ESCAPE/DIP + weighted 16-direction scorer); ability *decisions* are hand-coded per-class priority lists (~8,758 LOC across `class_ai/`), builder-instrumented with typed rejection reasons. Centralized `pre_cast_ok` cast guard. Pain point: priority knowledge scattered across imperative code; no tuning surface for ability ordering.

**Past learnings:** decision-trace builder + closed-enum audit is load-bearing for new AI surfaces; snapshot blind spots masquerade as balance noise (±50pt casting-visibility incident); `pre_cast_ok` proved data-keyed predicates beat ability-keyed code; refactors stage through byte-identical matrix checkpoints; measurement uses side-symmetrized cells (~18% deterministic mirror side bias from same-frame ECS resolution order — open gap); pets build a separate snapshot.

**External context:** Utility AI/IAUS (considerations through response curves) vs APL priority lists (= current pattern; documented sequencing brittleness) vs GOAP/HTN (judged overkill for 2-3 step combos). Industry hybrid: hard gates filter, small utility scorer re-sorts. Context steering (Fray, Game AI Pro 2): additive interest map + masking danger map. CMA-ES weight tuning with sim as fitness (coevolutionary EA precedent). big-brain Bevy crate: IAUS-shaped but archived on GitHub, no determinism guarantees — build in-house. Autobattlers (TFT/Underlords) ship simple threshold APLs; AI sophistication is a design choice.

## Topic Axes

1. movement-evolution — danger masking, DPS/non-healer movement, kiting, LoS
2. ability-architecture — utility scoring vs priority lists, data-driven config, hybrid gate+scorer
3. team-coordination — focus fire, CC chaining, peel, win-condition awareness
4. pets-and-parallel-surfaces — pet snapshot/guard parity, same-frame order-independence
5. tuning-infrastructure — weight optimization, trace analytics, probes, balance workflow

## Ranked Ideas

### 1. Gate-and-rank: data-driven utility re-sort for ability selection
**Description:** Keep typed precondition gates (`try_*` predicates, `pre_cast_ok`, cooldown/range/mana) as a hard filter pass; replace call-order priority with a small utility scorer — 2-4 considerations per ability through response curves, weights in `ability_ai.ron` mirroring movement.ron (struct defaults + `validate()` + `scorer_terms` on `ability_decision` trace events). Staged: (1) extract mechanical gates (derivable from abilities.ron) into a generic evaluator, byte-identical matrix checkpoint; (2) pilot the scorer on one class with probe contracts as acceptance suite.
**Axis:** ability-architecture
**Basis:** `external:` industry hybrid (gates filter + utility re-sort) is documented best practice; SimC APLs have known sequencing brittleness; full utility rewrites/GOAP overkill. `direct:` `cast_guard.rs::pre_cast_ok` proved the gate-consolidation move; trace builders already reify the eligible set — only ordering is hand-coded.
**Rationale:** Direct answer to the focus question. Converts ability tuning from Rust edits to RON edits; makes the matrix usable as an optimizer for ability behavior; second consumer of the movement.ron template.
**Downsides:** Large migration; needs a weights-reproduce-old-ordering equivalence stage before tuning.
**Confidence:** 85% **Complexity:** High (staged) **Status:** Unexplored

### 2. Context-steering masks, then universal movement
**Description:** Part A: split `score_direction` into additive interest terms + boolean danger masks (boundary, ally-anchor), deleting the 1000.0-must-dominate invariant `validate()` polices. Part B: extend the scorer to DPS classes as weight profiles (Mage kiting, Hunter dead-zone, melee pursuit), retiring `find_best_kiting_direction` and the `kiting_timer` coupling (ability AI writes movement state directly at mage.rs:475, hunter.rs:152).

**Deep-dive findings (2026-06-12):** Three movement brains exist today: the posture scorer (healers), `find_best_kiting_direction` (Mage/Hunter — already half context-steering: masks out-of-bounds via `continue` skip at movement.rs:69, penalty-hacks range-keeping via -1000.0 at movement.rs:89, hardcoded weights, no config, no trace), and simple pursuit (melee/pets). The refactor unifies siblings sharing a skeleton, not importing a foreign pattern. Mask-argmax ≡ penalty-argmax wherever ≥1 unmasked candidate exists (dominance already enforced), so Part A is provably near-byte-identical with divergence only in all-masked frames — which the masks make explicit via a fallback ladder (the one real design decision; currently invisible/untestable). Add `masked_directions: u16` bitmask to movement_decision trace events. Part B translation: kiting → threat_repulsion (exists) + new `range_band` ring-attraction term that generalizes the kiting orbit (0.85×keep_range), wand_pull (min=0), Hunter dead-zone (min=8), and melee pursuit (max=MELEE_RANGE). Rollout: A (near-identity, 1 session) → B-Mage (pilot + sweep) → B-Hunter → melee; never bundle A and B in one PR (U4 lesson). A-before-B because range_band as a soft term would otherwise fight the dominance invariant; A also creates the danger-mask slot LoS (#3) plugs into. Open questions: all-masked fallback ladder ordering (posture-dependent?); DPS postures minimal ENGAGE/KITE pair vs full set; kiting_timer survives as KITE entry signal vs postures reading aura state directly; pets ride the scorer now or wait for #5.
**Axis:** movement-evolution
**Basis:** `external:` context steering (Fray, GAP2) separates interest from masking danger. `direct:` movement.ron documents `ally_anchor: 1000.0 — HARD constraint… must dominate all soft terms (enforced by validate())`; movement.rs R14 comment marks the kiting carve-out seam; kiting branch already masks bounds.
**Rationale:** Deletes a standing tuning tax (dominance invariant), prerequisite for safe automated weight search (#6), unifies the movement architecture so every improvement lands for all classes, makes DPS movement traceable/probeable (currently a trace blind spot).
**Downsides:** Scorer-semantics refactor under pinned probe behavior; Part B intentionally changes balance-load-bearing kiting behavior (validated by sweep, per-class).
**Confidence:** 80% **Complexity:** Medium **Status:** Explored

### 3. LoS and pillar play
**Description:** The sim has zero line-of-sight logic anywhere — PillaredArena's defining feature is invisible to AI and casting. Add raycast LoS to the snapshot; `cover_pull` danger/interest terms (break enemy caster LoS while keeping anchor LoS) during PRESSURED/ESCAPE; `los_maintain` for casters. Plugs into #2's danger-mask slot.
**Axis:** movement-evolution
**Basis:** `direct:` grep confirms no LoS references in class_ai/ or combat_core/. `external:` Killzone tactical position scoring (LoS/cover predicates per threat, per-archetype weights). `reasoned:` pillar play is the defining arena healer survival skill; ESCAPE today can only run, and open-field running loses to casters by construction.
**Rationale:** Unlocks the dormant half of the map pool; gives ESCAPE an outcome class beyond fleeing; LoS query becomes shared infrastructure for ability decisions.
**Downsides:** Forces the sim-fidelity question of whether casts themselves check LoS (balance implications).
**Confidence:** 75% **Complexity:** Medium-High **Status:** Unexplored

### 4. Team claims ledger
**Description:** Per-team deterministic (BTree) resource of claims: focus-target selection (replacing static `kill_target` config), CC reservations with DR awareness, interrupt duty assignment, peel requests from the healer's PRESSURED transition. Class AIs stay autonomous, reading/writing claims as extra gates. Auction-bid allocation and a lethal/kill-window solver are second-generation consumers.
**Axis:** team-coordination
**Basis:** `direct:` the shipped `"HoJ reserved for enemy-healer dip"` rejection note proves cross-system reservation works; DoT-design memory documents reservation deadlocks (ad-hoc reservations don't scale); kill_target is static config. `reasoned:` 2v2/3v3 is the stated balance frame; CC overlap and doubled interrupts are what per-combatant AI cannot express.
**Rationale:** The step-function from "two solo AIs" to "a team," built as a generalization of an existing shipped mechanism.
**Downsides:** New trace vocabulary; claim/deadlock rules need explicit design.
**Confidence:** 80% **Complexity:** Medium **Status:** Unexplored

### 5. Two-phase intent/commit + unified snapshot
**Description:** All actors — combatants AND pets — read one frozen frame-start snapshot and emit intents; a deterministic, entity-order-independent commit phase resolves them. Kills the ~18% mirror side bias at the source (retiring the 2× symmetrization tax) and folds the pet snapshot into combat_snapshot.rs (making the ±50pt blind-spot bug class structurally impossible). Proposed independently by all six ideation frames.
**Axis:** pets-and-parallel-surfaces
**Basis:** `direct:` side bias and separate pet snapshot are documented open gaps; mid-cast exclusion incident is the documented cost. `external:` Diplomacy simultaneous adjudication / lockstep RTS engines exist precisely because sequential resolution creates first-mover artifacts.
**Rationale:** Substrate fix multiplying the value of every measurement and every other idea; cleaner signal for all future tuning.
**Downsides:** Touches combat-core resolution ordering with byte-level consequences for every pinned seed; needs matrix re-baseline at the final intentional change. Highest risk, highest leverage.
**Confidence:** 70% **Complexity:** High **Status:** Unexplored

### 6. Closed-loop weight optimizer with competence fitness
**Description:** CMA-ES/derivative-free search over RON weight surfaces using the planned in-process parallel matrix as evaluator. Fitness = behavioral competence KPIs vs frozen opponents (separation gained during ESCAPE, heal uptime, deaths-while-casting) — NOT raw winrate (symmetric changes wash out in team formats; optimizers exploit omitted objectives). Balance remains a human decision.
**Axis:** tuning-infrastructure
**Basis:** `external:` CMA-ES standard for this regime; coevolutionary EA precedent converged comparable system to 0.5 winrates fast. `direct:` parallel runner already planned (memory); movement_kpis.sh + probe helpers define the fitness vocabulary.
**Rationale:** Every data-driven surface (#1, #2) becomes machine-tunable; doubles as sensitivity analysis (dead knobs, slammed bounds reveal modeling gaps).
**Downsides:** Objective design is the hard part; wrong objective actively harms; compute cost. Gated on the parallel runner.
**Confidence:** 70% **Complexity:** High **Status:** Unexplored

### 7. Exceedance detector library (FOQA for the arena)
**Description:** Standard post-sweep mining of all matrix traces for defined bad events (healer outside anchor range >2s, CC into existing CC/DR, idle GCDs with mana, overheal fraction, heals started on doomed targets), emitting per-class exceedance-rate CSVs beside winrate CSVs. Rates are dense and attributable — they move before winrate does (the ±50pt U4 event was trace-visible long before winrate exposed it) — and serve as #6's cheap inner-loop fitness proxy.
**Axis:** tuning-infrastructure
**Basis:** `external:` FAA FOQA/FDM programs (AC 120-82) — fleet-wide rates of defined bad events converge faster than outcome metrics. `direct:` systematizes the ~12 manual jq recipes in CLAUDE.md and movement_kpis.sh.
**Rationale:** Cheapest survivor; underwrites every other idea as the regression net; converts the trace investment from interactive forensics to automated coverage.
**Downsides:** Threshold calibration to avoid alert fatigue; some "bad events" are correct play in context.
**Confidence:** 85% **Complexity:** Low-Medium **Status:** Unexplored

## Rejection Summary

| # | Idea | Reason Rejected |
|---|------|-----------------|
| 1 | Generic gate pass from abilities.ron | Folded into survivor 1 as its stage 1 |
| 2 | Threat-forecast / cast-completion eval (quiescence) | Lands naturally as a consideration input to survivor 1's scorer — brainstorm variant, not standalone |
| 3 | Auction task allocation (MURDOCH/TraderBots) | Duplicates survivor 4 with more machinery; revisit if the ledger proves insufficient |
| 4 | Lethal solver / kill-window GO (Hearthstone analogy) | Second-generation consumer of survivor 4's ledger; sequence after |
| 5 | Emergent kill-target selection | Folded into survivor 4 as its first consumer |
| 6 | Trace-diff first-divergence tool | Narrower than survivor 7; byte-identical checkpoints cover most value; revisit during survivor-1 migration |
| 7 | Ability-decision probe contracts | Folded into survivor 1 as its acceptance suite |
| 8 | Counterfactual forking / regret ranking (rr analogy) | Expensive microscope; build only if survivor 7's cheap layer proves insufficient |
| 9 | HTN set plays with abort conditions | External research judges multi-step planning overkill for 2-3 step combos; commitment insight folds into survivor 1 |
| 10 | Strategic posture layer gating ability lists | Integration step presupposing survivors 1+2; revisit after both land |
| 11 | Observed-cooldown ledger (opponent memory) | Gated on survivor 1's gate vocabulary existing; second-generation data source |
| 12 | Kill-window pipeline (cross-cut) | Composition of rejected/sequenced ideas; premature |
| 13 | Exceedance-as-fitness (cross-cut) | Folded into survivors 6+7 coupling |
| 14 | Masks-as-optimizer-prereq (cross-cut) | Folded into survivor 2/6 sequencing note |

## Sequencing Notes

#7 is cheapest and underwrites everything. #1 and #2 are the two architecture moves sharing the movement.ron lineage. #2 Part A precedes Part B and creates the slot #3 plugs into. #5 is the substrate fix making all measurement cleaner. #6 pays off only after #1/#2 grow the weight surface.
