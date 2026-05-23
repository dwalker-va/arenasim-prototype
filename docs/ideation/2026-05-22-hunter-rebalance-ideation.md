---
date: 2026-05-22
topic: hunter-rebalance
focus: "Hunter class is particularly lacking in matchups; understand why and ideate fixes"
mode: repo-grounded
---

# Ideation: Hunter Rebalance

Hunter sits at ~7% winrate across the 4,900-match baseline at `design-docs/balance/matrix_baseline_2026-05-16.csv`, losing 0% of matches in 6 of 7 matchups (fastest defeat 10.46s vs Mage; longest 48s vs Paladin; mirror 45/55 — order-dependent). This document is the **second pass** of this ideation — the first pass was based on a hallucinated codebase scan that asserted several Hunter systems were missing when they in fact exist. The user's "wait, can we look at the Warlock pet first?" question caught the error before any code was written. Reverified diagnosis follows.

## Methodological note (read first)

The first-pass grounding agent (Haiku model) claimed:
- "Pet is spawned but completely idle. No abilities. No attacks."
- "No Auto Shot baseline."
- Hunter HP 98 / AP 46 (these were base-class stats, not equipped stats).

All three were wrong. **The pet AI is a 635-line module** (`src/states/play_match/class_ai/pet_ai.rs`) with `felhunter_ai` / `spider_ai` / `boar_ai` / `bird_ai`. Pets auto-attack via the `combat_auto_attack` system (line 34 of `combat_core/auto_attack.rs` queries `&Pet`). Auto Shot exists in `abilities.ron` and lands for 22 damage / crits for 43 in real match logs. Hunter at full equipment has 363 HP, not 98.

Verifying with `--trace-mode on` and a real headless match (instead of relying on a summarized scan) is non-negotiable for balance ideation. This is exactly what survivor #7 in the first-pass doc proposed; this run is its first demonstration.

## Grounding Context — Corrected

### Hunter abilities (verified from `assets/config/abilities.ron`)

| Ability | Cast | Range | min_range | Mana | CD | Damage | Effect |
|---|---|---|---|---|---|---|---|
| Aimed Shot | 2.5s | 35yd | 8yd | 40 | 10s | 18-28 + 1.0× AP | 35% heal-reduction 10s |
| Arcane Shot | instant | 35yd | 8yd | 25 | 6s | 10-16 + 0.5× AP | — |
| Concussive Shot | instant | 35yd | 8yd | 15 | 12s | 0 | 50% slow 4s |
| Disengage | instant | self | — | 20 | 25s | 0 | Backward leap 15yd |
| Freezing Trap | instant place | 30yd | — | 50 | 25s | 0 | 10s Incapacitate, `break_on_damage_threshold: 0.0` |
| Frost Trap | instant place | 30yd | — | 30 | 20s | 0 | Slow zone |
| Auto Shot | passive | 35yd | — | 0 | — | weapon | Continuous swing — already exists |

### Hunter pet abilities (verified from `assets/config/abilities.ron`)

| Ability | Range | CD | Effect | Pet |
|---|---|---|---|---|
| Spider Web | 20yd | **45s** | 4s Root, break_on_damage 80 | Spider |
| Boar Charge | 25yd | **45s** | 1.5s Stun, is_charge | Boar |
| Master's Call | 40yd | **45s** | Dispel movement impairment (ally cast) | Bird |
| Spell Lock | 30yd | 30s | Interrupt + 3s lockout | Felhunter |
| Devour Magic | 30yd | **8s** | Magic dispel — strips one buff/debuff | Felhunter |

### Hunter base stats (verified from `combatant.rs:215`)

- Hunter: `max_health=265, max_mana=150, mana_regen=3.0, AP=18, crit=7%, mp5=5.0`
- With full Beaststalker loadout, in match: HP 363, Mana 150 starting.

### Pet AI behavior (verified from `class_ai/pet_ai.rs` + `combat_core/movement.rs`)

- Each pet has its own decide function. Spider has only `try_spider_web`; Boar has only `try_boar_charge`; Felhunter has SpellLock + DevourMagic priority.
- **Pets are never assigned a `combatant.target`.** With no target, the movement system routes pets to "follow owner at 3yd distance" (`movement.rs:311+`).
- Spider Web's target filter requires `dist_to_owner <= 15.0` AND `dist_to_spider <= 20.0`. Since the spider sits at owner's feet, both checks reduce to "enemy within 15yd of Hunter."
- All Hunter pets (Spider/Boar/Bird) are **melee** auto-attackers with 2.5yd range. They cannot deal damage at distance.

### Diagnostic trace data (from `--trace-mode on`, Hunter v Warrior, 27.77s match)

**Hunter rejection histogram (top entries):**
```
1092 SpiderWeb:OnCooldown      ← Wait, this is pet-attributed-to-Hunter; ignore for Hunter rotation
 945 FrostTrap:InsufficientMana
 388 ArcaneShot:InsufficientMana
 296 Disengage:OnCooldown
 261 Disengage:InsufficientMana
 217 ConcussiveShot:OnCooldown
 173 ConcussiveShot:InsufficientMana
 117 FreezingTrap:OnCooldown
 116 ConcussiveShot:OutOfRange
 115 ArcaneShot:OutOfRange
 115 AimedShot:OutOfRange
```

**Hunter chose only 6 ability casts in 27.77s** (FreezingTrap×1, ConcussiveShot×2, Disengage×1, ArcaneShot×1, AimedShot×1). Auto Shot fired throughout but is not in the trace (it's a swing-timer auto-attack, not a chosen ability).

**Spider Web outcome: 1607 NoValidTarget rejections in Hunter v Warlock; SpiderWeb cast exactly ONCE in Hunter v Warrior** (the only fire of any pet ability in the entire match besides one Felhunter DevourMagic).

### What the diagnostics actually reveal

1. **Mana economy is the binding constraint.** ~1,767 `InsufficientMana` rejections in a 27s match. The AI knows what it wants to cast but can't afford to. Auto Shot's continuous damage isn't enough to sustain the loss when 6 specials trickle out instead of 12.
2. **Pet ability cooldowns (45s) are longer than match durations (10-30s).** Spider Web, Boar Charge, and Master's Call effectively fire zero or one time per match.
3. **Pet target-acquisition is missing.** Pets follow Hunter, never close to attack range on enemies. In a Hunter v Warlock match where Hunter kited to -2.77 and Warlock stayed at +15.07, the Spider sat near Hunter and never closed on the Warlock — auto-attacks not even attempted because never in melee range.
4. **Spider Web's `dist_to_owner <= 15.0` filter is incompatible with Hunter's 35yd range identity.** The Spider can only Web targets that have closed within 15yd of Hunter — which is the dead zone Hunter is desperately trying to escape.
5. **Felhunter Devour Magic counters Concussive Shot 1:1.** Concussive Shot 12s CD vs Devour Magic 8s CD — Devour Magic always wins the race. Hunter v Warlock match log shows Concussive landing at 14.38s → Devoured at 14.40s; landing at 28.73s → Devoured at 28.75s. Hunter's kite identity collapses specifically vs Warlock.
6. **Hunter's overall AI band model is largely fine.** OutOfRange rejections sum to ~350 across all abilities — minor compared to ~1,767 mana rejections. The 3-band distance model isn't the binding constraint, despite the first-pass framing.

## Topic Axes (revised)

1. **Pet engagement** — pet target acquisition, pursuit movement, ability constraints
2. **Pet ability tuning** — cooldowns, ranges, dispellability
3. **Hunter resource economy** — mana pool, regen, ability costs
4. **CC & traps** — predictive trap placement, healer-targeting, dispel counters
5. **Defensives** — Disengage follow-through, anti-burst mitigation

(Note: "Kiting & range AI" was an axis in the first-pass doc but the trace data shows it's not the binding constraint. Demoted to the existing AI's adequate-for-now status.)

## Ranked Ideas

### 1. Pet engagement: target acquisition + pursuit movement

**Description:** Pets currently never get a `combatant.target` set, so `move_to_target` routes them to "follow owner at 3yd." Give pet_ai_system a step that assigns the pet's target to its owner's target (or to the owner's primary aggressor if the owner has no target) at the start of each decision tick. Then the existing target-pursuit movement carries the pet into melee range on its own, and auto-attacks start landing.

**Axis:** Pet engagement
**Basis:** `direct:` Hunter v Warlock match log shows zero Spider auto-attack entries despite a 16.78s fight; Spider position trails owner. `direct:` `combat_core/movement.rs:311+` shows pets without a target follow owner at 3yd distance; pets with a target use normal target-pursuit movement. `direct:` `class_ai/pet_ai.rs` never assigns to `combatant.target` in any of the four pet AIs.

**Rationale:** This is the single highest-impact pet fix. Today's pet contribution is "auto-attack when accidentally in melee range." With target pursuit, pets do their full auto-attack DPS (Spider/Boar/Bird ~7-12 damage per swing at 1.3s ≈ 6-9 DPS, comparable to Hunter Auto Shot). Felhunter already roughly closes — but it works mostly by accident because Devour Magic and Spell Lock are 30yd ranged abilities so it can fire from owner-position.

**Downsides:** A pet that auto-pursues can be peeled off and kited by anyone. Spider/Boar/Bird being melee is partly the point — they create a second-melee-threat for the enemy to deal with. Risk of Spider getting permanently rooted/CC'd if the enemy has crowd control. Tuning may need pet-specific "stay near owner" override when owner is below low HP.

**Confidence:** 95%
**Complexity:** Medium (~50-150 lines in pet_ai.rs + integration with target update systems)
**Status:** Unexplored

### 2. Bring pet ability cooldowns into match-duration parity

**Description:** Spider Web (45s), Boar Charge (45s), and Master's Call (45s) all have cooldowns longer than the average match length (~30s vs Hunter, 10-48s across matchups). The pet's signature ability fires once if at all. Tighten to ~12-18s cooldowns — same order of magnitude as Hunter's own abilities (Aimed Shot 10s, Concussive Shot 12s, Freezing Trap 25s). Also relax Spider Web's `dist_to_owner <= 15.0` constraint to 25-30yd (or remove entirely) and increase its range from 20yd → 30yd so the Spider can web from where it actually stands.

**Axis:** Pet ability tuning
**Basis:** `direct:` `abilities.ron` lines 808, 832, 850: all three pet abilities cooldown=45.0. `direct:` Decision trace shows 1607 `NoValidTarget` rejections for SpiderWeb across a 16.78s Hunter v Warlock match — the cooldown wasn't the issue here (Spider was never close enough to a valid target); but in Hunter v Warrior where the Warrior closed, the cooldown gated Spider Web after the first cast. `direct:` Spider Web range 20yd in `abilities.ron:807` vs Hunter rotation range 35yd in same file.

**Rationale:** Pure data tuning — single-digit lines in `abilities.ron`. No code changes. The 45s cooldowns appear to be defensive sandbagging from when pet AI was new; with target-pursuit fixed (idea #1), more frequent firing is appropriate. The `dist_to_owner <= 15.0` filter in `spider_ai.rs` should also be removed or widened — it's a kiting-incompatible constraint.

**Downsides:** Spider Web at 12s CD + 30yd range becomes a much stronger root than today. Could oppress melee classes (Warrior/Rogue) if Hunter chains it with Frost Trap + Concussive Shot — needs matrix sweep. Also: the 80-damage break threshold on Spider Web means Hunter must avoid AoE damage on rooted targets, which already feeds the friendly-CC guard (`pre_cast_ok`) and may need a re-test pass.

**Confidence:** 90%
**Complexity:** Low (RON tuning) to Medium (if spider_ai.rs filter also changes)
**Status:** Unexplored

### 3. Fix Hunter mana economy

**Description:** Trace data shows ~1,767 `InsufficientMana` rejections across abilities in a 27s Hunter v Warrior match. Hunter starts with 150 mana, regenerates 3/s, and a full rotation costs 180+ mana. The class is structurally OOM by ~15s into any extended fight. Three lever options: (a) raise `max_mana` from 150 → 200; (b) raise `mana_regen` from 3.0 → 5.0; (c) cut ability costs across the board by ~25%. Or combination. Single line(s) in `combatant.rs:215`.

**Axis:** Hunter resource economy
**Basis:** `direct:` `combatant.rs:215` defines Hunter's base mana stats. `direct:` Decision trace 945 FrostTrap + 388 ArcaneShot + 261 Disengage + 173 ConcussiveShot = 1,767 InsufficientMana rejections in 27.77s. `direct:` Hunter chose only 6 ability casts during that window — the AI wanted to cast more but couldn't afford to.

**Rationale:** This is the cheapest, highest-confidence fix in the list. The diagnostic data is unambiguous and the change is one or two stat tweaks. Hunter's damage isn't bad per cast — Aimed Shot for 95, Arcane for 52, Auto Shot crits for 43 — the problem is the cast count per fight. Doubling effective casts via mana doubles Hunter's match damage output.

**Downsides:** Pure tuning — easy to overshoot. Increases mana also makes Disengage spammable, which interacts with idea #6 (Disengage follow-through). Need matrix sweep before/after to confirm the move isn't accidentally an over-buff vs already-disadvantaged matchups.

**Confidence:** 95%
**Complexity:** Low (1-3 line change + matrix sweep validation)
**Status:** Explored — handed off to `/ce-brainstorm` on 2026-05-22

### 4. Counter Felhunter Devour Magic against Hunter CC

**Description:** In Hunter v Warlock, Felhunter Devour Magic (8s CD) strips Concussive Shot (12s CD) within 0.02s of landing — twice in one match log. Concussive Shot is `spell_school: Physical` in `abilities.ron` but DevourMagic dispels it anyway. Three options: (a) make Concussive Shot dispel-immune (its slow is from a physical projectile, arguable); (b) give Hunter Tranquilizing Shot (counter-dispel Warlock buffs back); (c) tune DevourMagic CD up or Concussive Shot CD down so they don't perma-counter.

**Axis:** CC & traps
**Basis:** `direct:` Match log `match_logs/match_1779438675.txt` shows the 0.02s Devour after Concussive — twice. `direct:` Concussive Shot `spell_school: Physical` in `abilities.ron:746` but is dispelled by `DevourMagic`. `direct:` Matrix baseline Hunter v Warlock 3% (the only non-zero non-mirror winrate) — Devour Magic is a primary contributor.

**Rationale:** This is the specific mechanical reason Hunter loses to Warlock specifically. Other matchups don't have this problem (Mage's Counterspell doesn't dispel; Paladin doesn't have an offensive dispel against Hunter abilities). Fixing this lifts one specific matchup from 3% toward parity without affecting the others.

**Downsides:** Option (a) (dispel-immune Concussive) is the cleanest but sets precedent for "Physical abilities never dispellable" which has interactions with other classes' Physical CC. Option (b) (Tranquilizing Shot) adds a new ability — scope creep. Option (c) (CD tuning) is the safest but may not be sufficient if a clever Warlock AI saves DevourMagic for after Concussive.

**Confidence:** 85%
**Complexity:** Low (RON tweak) to Medium (new ability)
**Status:** Unexplored

### 5. Predictive trap placement via `predicted_path()` primitive

**Description:** Today's trap placement uses a midpoint heuristic — Freezing Trap lands at the midpoint between Hunter and target (Hunter v Warlock log: "Freezing Trap lands at (0, -3)" at 11.72s; trigger at 15.78s — 4 seconds of trap idle). Add a deterministic `predicted_path(entity, t) -> Vec3` primitive that projects an enemy's position `t` seconds forward (using current velocity + intent) and place traps on the predicted intercept. The same primitive later powers Mage Blizzard / Flamestrike AoE placement and Warrior Charge intercept logic.

**Axis:** CC & traps
**Basis:** `direct:` Hunter v Warlock match log shows 4s lag between trap placement and trigger — placement was reactive, not predictive. `external:` Anti-submarine warfare doctrine (depth charges target the predicted sub position, accounting for sink time) and AAAI influence-map primitives. `direct:` `predicted_path` doesn't exist in the codebase yet — would be a new primitive in `class_ai/combat_snapshot.rs` or similar.

**Rationale:** Traps are Hunter's signature CC and the entry to offensive chains (Concussive Shot → kite into Frost Trap → Aimed Shot). Today they're cast hopefully and triggered (or not) randomly. Predictive placement converts placement-frequency into trigger-frequency. The primitive pays leverage downstream for every future ground-AoE ability.

**Downsides:** Requires deterministic velocity sampling — risk of replay divergence if prediction reads any non-BTreeMap collection. Tuning the projection time horizon `t` is non-obvious (too long = predicts past where enemy actually goes; too short = no different from current). Trap placement still doesn't fix the `break_on_damage_threshold: 0.0` issue on Freezing Trap, which is a separate concern.

**Confidence:** 80%
**Complexity:** Medium (~100-200 lines for primitive + integration)
**Status:** Unexplored

### 6. Disengage with follow-through (sprint + slow-cleanse, or Feign Death analogue)

**Description:** Disengage currently leaps backward 15yd once every 25s with no follow-up effect. Add either (a) a 2-3s sprint buff post-leap (+40% movement speed) so Hunter actually escapes rather than just relocating, (b) a slow-cleanse on activation so Hunter can leap out of Frost Nova / Hamstring states, or (c) a separate Feign Death-style ability that drops aggro/casts targeting the Hunter for ~5s on a longer CD.

**Axis:** Defensives
**Basis:** `direct:` `abilities.ron:756-764` — Disengage has only `cooldown: 25.0` and a self-cast, no aura or follow-up effect. `direct:` Hunter v Warrior decision trace shows 261 `Disengage:InsufficientMana` rejections — the ability is also locked behind mana when Hunter needs it most. `external:` Feign Death in WoW Classic resets trap CD, cancels enemy casts, and drops aggro — a documented mechanic that today's Disengage poorly approximates.

**Rationale:** The 0% winrates vs gap-closers (Warrior, Rogue) trace partly to "Hunter leaps but cannot escape." A 25s CD escape with no follow-up is *displacement*, not *defense*. Sprint + cleanse turns it into a real defensive cooldown. Pairs naturally with mana fix (#3) — once Hunter can afford Disengage, it should actually do something.

**Downsides:** Sprint + cleanse stacking on a 25s CD is potentially oppressive vs every slow ability in the game (Frost Nova, Hamstring, Mortal Strike's slow). Needs careful enumeration of slow interactions. Feign Death analogue is its own ability with its own design space — could be brainstorm content.

**Confidence:** 85%
**Complexity:** Low (RON aura addition) to Medium (new Feign Death ability)
**Status:** Unexplored

### 7. Codify the matrix-trace audit pattern (methodological)

**Description:** This entire ideation session demonstrated why "audit AI behavior via decision trace and live match logs before stat changes" is non-negotiable. The first-pass grounding by a summarized codebase scan asserted three Hunter systems were missing that actually exist (pet AI, Auto Shot, base stats). The fix: build a reusable "balance audit" workflow that runs a target class through (a) `--matrix 100` with traces, (b) `jq` analysis of rejection histograms per matchup, (c) one full match-log read per matchup, before any survivor or candidate stat change is proposed. Document the workflow in `docs/solutions/workflows/` and reference it from `CLAUDE.md`.

**Axis:** Methodological (cuts across axes)
**Basis:** `direct:` First-pass ideation doc (this same file's previous version) was wrong on idle pet, missing Auto Shot, and HP/AP numbers. `direct:` Trace tooling already shipped (PR #52 lineage); `jq` recipes are already documented in project `CLAUDE.md` for filtering Hunter rejections. `reasoned:` Shipping stat changes on top of a misdiagnosis would lock the misdiagnosis in semi-permanently as retuned numbers compensate for problems that don't exist.

**Rationale:** Without this workflow as a forcing function, the next balance ideation will likely repeat this error mode. Solving the problem once for Hunter is local; codifying the pattern is leverage on every future balance question. The decision-trace tooling already exists — what's missing is the workflow documentation that makes "always run a real trace first" a default rather than an oversight.

**Downsides:** Documentation drift — if the trace tooling changes shape, the workflow doc gets stale. Risk of analysis-paralysis if the audit becomes the project rather than a gate. Cost in agent time per balance question (one extra matchup run + trace analysis per class under review).

**Confidence:** 90%
**Complexity:** Low (one workflow doc + reference from `CLAUDE.md`)
**Status:** Unexplored

## Rejection Summary

| # | Idea | Reason rejected |
|---|------|----------------|
| 1 | "Activate the idle pet" (first-pass survivor #1) | **False premise — pet AI exists**, `class_ai/pet_ai.rs` is 635 lines. Replaced by ideas #1 and #2 (pet engagement + ability tuning) which address the real gaps |
| 2 | "Add Auto Shot continuous-damage baseline" (first-pass survivor #2) | **False premise — Auto Shot exists** in `abilities.ron` and lands for 22/crits for 43 in match logs |
| 3 | "Replace 3-band model with continuous `kiting_score`" (first-pass survivor #3) | Trace data shows OutOfRange rejections (~350 total) are minor vs InsufficientMana (~1767); the band model isn't the binding constraint |
| 4 | "Team-comp awareness — target healer with traps" (first-pass survivor #5) | Folded into #5 (Predictive trap placement); the same `predicted_path` primitive enables both target-prioritization and intercept placement |
| 5 | F2#2 Pet unkillable but 0 damage | Pet is doing damage when engaged; the gap is engagement (idea #1), not damage |
| 6 | F3#7 Two-body Hunter (controller + tank pet) | Radical retopology of class shape; defer to brainstorm |
| 7 | F6#1 / F6#2 5-pet swarm / zero-pet | Subject-adjacent; thought experiments not grounded |
| 8 | F3#3 Hunter as 2v2/3v3 specialist by design | Accepts the 1v1 problem rather than fixing it; matrix data shows the problem is universal not format-specific |
| 9 | F3#6 Distance as continuous damage multiplier | Bold reframe; not addressing a diagnosed failure |
| 10 | F2#7 / F6#5 Instant Aimed Shot / all-instant Hunter | Aimed Shot is the rare ability that actually lands (95 dmg in match log); cast time isn't the binding constraint |
| 11 | F5#5 Viper Sting mana drain | Hunter's own mana economy is broken — adding mana drain to enemies before fixing own is backward |
| 12 | F4#7 Combat duration ramp scaling | Speculative; matches end too fast for ramp to matter |
| 13 | F4#2 Aspect aura slot | High leverage but not addressing a diagnosed failure |
| 14 | F5#4 Steady Aim cast-time stacks | Aimed Shot cast time isn't the diagnosed constraint |
| 15 | F5#3 Fire-break Frost Trap placement | Folds into #5 (Predictive trap placement) |
| 16 | F5#7 Chess zugzwang trap+slow combo | Folds into #5 (Predictive trap placement) |
| 17 | F5#1 / F5#6 K-9 Send pet / octopus parallelism | Folds into #1 (Pet engagement) |
| 18 | F6#3 30yd dead zone sniper | Constraint flip; not addressing a diagnosed failure |
| 19 | F6#4 Zero-CD Concussive Shot | Concussive Shot CD isn't the binding constraint; mana is |
| 20 | F6#6 Throwable 35yd traps | Trap range (30yd) is already adequate; placement intelligence is the gap |
| 21 | F6#7 Disengage 5s CD spammable | Constraint flip; mana economy fix (#3) more directly addresses the same goal |
| 22 | F2#4 Inverted Disengage (enemy push) | Single-ability rework; brainstorm content |
| 23 | F2#5 Remove mana resource | Bold but coupled to broader resource model decision; mana tuning (#3) is the in-scope fix |
| 24 | F3#1 / F3#4 Hunter is CC class / Trap is signature | Identity reframes; not directly actionable from current diagnostics |
| 25 | F1#7 (original) Team-comp awareness as separate idea | Surfaced this run as the Hunter v Paladin 48s match; folds into #5 with the trap-prediction primitive |
| 26 | F4#6 Per-attack slow on ranged weapons | Strong candidate; deferred to second pass; mana fix more directly addresses survivability |

## Lessons captured (for future balance work)

1. **Never trust a summarized codebase scan for balance work.** Run a real headless match with `--trace-mode on` and read the trace before forming hypotheses. The decision-trace tooling exists for exactly this purpose.
2. **Match logs surface mechanical realities that grounding agents miss** (e.g., Felhunter dispelling Concussive Shot, Spider never closing on the Warlock, Hunter running OOM by 15s).
3. **`jq` rejection histograms are the most efficient first-pass diagnostic.** `select(.actor.class == "Hunter") | .candidates[]? | select(.status == "rejected") | .ability + ":" + (.reason | if type == "object" then keys[0] else . end)` directly reveals binding constraints.
4. **The user's instinct to verify a cross-class assumption ("does Warlock pet have the same problem?") caught the hallucination before any code was written.** This pattern — "before locking in a fix, check whether a parallel system has the same problem" — is the human counterpart to the audit workflow.
