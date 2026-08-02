# Nagrand pillar-camp: findings and handoff

**Date:** 2026-08-01
**Branch:** `feat/team-plan-camp` (8 commits ahead of `main`, unpushed)
**Status:** TeamPlan step 3 (pillar-camp opener) — **regression fixed.** The camp
now buys 28.3 occlusion-seconds per match against `Legacy`'s 0.0, at win parity
(9/12 both). A smaller residual remains and is step 4's to fix.

This is a handoff document. It records what is *established*, what was
*retracted*, and what to do next, so the next session does not re-derive any of
it or repeat the measurement mistakes.

---

## 0. Resolution (added after §1–§8 were written)

**§3.1 named the wrong fix.** "The healer cannot commit to a side when rounding
its pillar" is the symptom. The cause is one line of scope:

> **The camp was an opener that never ended.** Its release predicate
> (`should_hold`) asked *"is an enemy within 15yd of ME"*. For a healer facing a
> ranged comp that is never true — the enemy stops at 30–40yd to cast — so the
> camp never released. The Priest stayed welded to a ring 8.5yd around its pillar
> for the entire match with its posture AI suppressed (the camp branch removes
> `MovementDirective`), chasing a hold spot that moved as the fight moved.

Measured on seeds 7/11/12 before the fix: the teams met 18.9s after the gates,
and the healer was **still camping for 71–79% of every frame after that**. When
it was actively trying to poke for a heal it could not see its Warrior on
**60–94%** of those frames — while standing a mean 3.6–5.8yd behind its own
orders.

Three hypotheses were killed by measurement on the way, and are recorded so they
are not re-run: the healer was never out of heal range (0% of frames beyond
40yd, mean separation 12.8yd); the commanded hold spot was *stable* (it jumped
>4yd on 1–4 frames of a whole match, so the ring tie-break is not thrashing);
and whenever a poke was requested the commanded spot did have line of sight to
the ally (0 failures) — the healer simply never got there.

### The fix

`teams_in_contact` (`team_plan.rs`): contact is a **team** event — any enemy
within `CAMP_ENGAGE_RADIUS` of any living member. `update_team_plans` checks it
every frame, above the roster gate, and latches it per team. On the transition
the stance drops `Hold` → `Press`, which the movement consumer already reads.

The latch has to outlive a replan. A death is the commonest replan trigger and
it lands mid-fight; without the latch the recomputed plan hands the comp
`Stance::Hold` again and marches its healer back to the pillar at the worst
possible moment. That case is pinned by
`a_replan_after_contact_does_not_re_arm_the_camp`.

This is the scope the design doc already specifies: step 3 is the opener ("the
team takes the pillar *before contact*"), and in-fight positioning around cover
is step 4's focal-rooted team solve.

### Result — 12 paired seeds, `Warrior+Priest` vs `Warlock+Priest`

|  | Legacy | TeamPlan before | TeamPlan after |
|---|---|---|---|
| team-1 wins | 9/12 | 5/12 | **9/12** |
| Warlock denied sight of Priest | 0.0s | — | **28.3s** |
| heal delivered to Warrior | 481 | 190 | 295 |
| heal line occluded | 0% | 55% | 25% |
| mean duration | 58.1s | 86.2s | 76.5s |

**`Legacy` measures 0.0 occlusion-seconds.** That is the number step 3 exists to
move, and it is the first time this AI has used the Nagrand geometry at all.

### What remains (step 4, not step 3)

Post-release the healer starts the fight standing next to a pillar, and
`cover_pull` then pins it there — 11.8yd from the camp pillar on average against
`Legacy`'s 32.6yd. It still loses the heal line on 39% of post-contact frames
and heals 295 against `Legacy`'s 481. Both pathology probes therefore still
assert a live pathology, at reduced magnitude; their doc comments carry the
revised attribution. This is the reactive layer's limit, and step 4 retires
`cover_pull` for the team solve.

Two smaller things worth knowing, neither fixed:

- **The camp stacks the team on one coordinate.** Separation between Warrior and
  Priest at contact is *exactly* 0.0yd: with `keep_sighted: None` both units feed
  `hold_position` identical inputs and get the identical point. A melee/healer
  opener presumably wants the melee screening, not co-located.
- **A geometric probe cannot decide whether the camp released.** Position alone
  does not separate "camp active" from "`cover_pull` picking a similar spot"
  (on-ring occupancy was 31% early vs 22% late — no separation). One was drafted
  and deleted rather than fitted to a threshold. The release seam is decided in
  `team_plan.rs`'s unit tests, where it is unambiguous.

### Reproduce

```bash
cargo test --release --test camp_sweep -- --ignored --nocapture       # the table above
cargo test --release --test movement_probes pillar_self_block -- --nocapture
```

`tests/camp_sweep.rs` replaces the scratch sweep script §6 records as lost. It
reads positions and health straight off the observer, so the pet-aliasing bug of
§4.2 cannot recur — entities are keyed by `Entity` and `is_pet` is a field.

**Everything below this section is the original handoff, unedited.** §3.1 is
superseded by the above; §3.3, §3.4, §4 and §5 all still stand.

---

## 1. The headline finding

A `TeamPlan` healer camping a Nagrand pillar **puts that same pillar between
itself and the ally it is supposed to heal**, and never commits to a side.

Measured per-frame via the observer harness (`tests/movement_probes.rs`, module
`pillar_self_block`):

| seed | paired samples | span | heal line blocked | caused by own camp pillar | longest blackout |
|---|---|---|---|---|---|
| 11 | 2408 | 40.1s | 963 (40.0%) | 963 (**100%**) | 8.13s |
| 7  | 2853 | 47.5s | 1468 (51.5%) | 1468 (**100%**) | 20.18s |
| 12 | 2763 | 46.0s | 1523 (55.1%) | 1523 (**100%**) | 20.42s |

**3954 of 3954 blocked frames are the healer's own cover.** Not a distant
pillar, not unavoidable geometry. Blackouts run *continuous* for 20s against a
1.5s Flash Heal, and the healer reverses which side of the blocking axis it
stands on roughly once per second throughout — it is oscillating, not
committing.

This was found by watching a replay, not by any metric: the healer visibly gets
confused about which way to round the pillar, lines itself out of the heal, then
gets spell-locked and feared, and the Warrior dies inside that fear.

### The consequence, from the 12-seed paired sweep

| profile | T1 wins | avg duration | heal delivered to Warrior | Warrior died |
|---|---|---|---|---|
| Legacy | 9/12 | 68.1s | 483 | 3/12 |
| TeamPlan | 5/12 | 86.2s | 190 | 7/12 |

**Every TeamPlan loss delivered EXACTLY ZERO healing to the Warrior. Every win
delivered 306–616.** Perfect separation, no overlap. The same rule holds in
Legacy (its one zero-heal seed is also its loss).

Win rate alone is *not* statistically conclusive — 6 discordant pairs, 5 in one
direction, McNemar two-sided p ≈ 0.22. The zero-heal separation and the
self-blocking measurement are what carry the conclusion, not the win column.

Comp under test: `Warrior+Priest` (team 1, the camping side) vs
`Warlock+Priest` (team 2), map `PillaredArena`, seeds 1–12.

---

## 2. Causal chain

```
camp a pillar
  -> own pillar lands on the line to the ally
  -> 20s continuous blackouts, no heal possible
  -> healer eventually exposes to heal
  -> spell-locked (Felhunter), Holy school locked 3.0s
  -> free Fear cast lands during the lockout (8s)
  -> ally dies inside the fear, never healed
```

Seed 11, from the graphical replay log:

```
40.30s  Warlock BEGINS casting Fear on Team 1 Priest
40.75s  Priest begins Flash Heal on Warrior
40.75s  Felhunter interrupts it — Holy school locked 3.0s
41.80s  Fear LANDS on the Priest (8.0s)
47.83s  Warrior dies
```

The Priest had **1.05s** between being locked out and the Fear landing, during
which casting was impossible and movement was therefore free. It did not move.
Roughly 7yd of travel at ~7yd/s against a 6yd-circumradius pillar with a 2yd
camp standoff — feasible but tight; **not confirmed**.

The lockout-into-Fear is a **secondary** problem. The primary problem is the
healer denying itself the heal through bad pillar movement. Fixing the
positioning is what matters; a "use school-lockout windows to break LoS from
enemy cast bars" behaviour is a separate, generally-useful follow-up.

---

## 3. What to fix (recommended order)

### 3.1 Side commitment when rounding a pillar — PRIMARY

The healer reverses sides ~1×/sec. `CLAUDE.md` states side commitment is
"emergent, not stored" and "cannot flip-flop" — that reasoning holds for a mover
with a *single* destination, but a camping healer is handed two opposing goals
(hide from threats, see the ally) and thrashes between them. The poke side must
be chosen against the constraint that actually matters and then *held*.

Relevant code:
- `src/states/play_match/team_plan.rs` — `hold_position`, `should_break_cover`,
  `CAMP_STANDOFF`, `CAMP_ENGAGE_RADIUS`
- `src/states/play_match/combat_core/movement.rs` — the camp block (above the
  `MovementDirective` branch)
- `src/states/play_match/map_geometry.rs` — `steer_toward_goal` tangent steering

**The probes will tell you if it works.** Both are written to pin *current*
behaviour (the U2 inverted-statue idiom), so a fix flips them loudly rather than
leaving a red test:
- `pillar_self_block::priest_blocks_its_own_line_to_the_warrior`
- `pillar_self_block::priest_thrashes_across_the_pillar_axis`

Re-run: `cargo test --release --test movement_probes pillar_self_block -- --nocapture`

### 3.2 Do NOT start with the comp→plan table

An earlier plan was to flip melee+healer from `Hold` to `Press`. **That is
probably the wrong fix** — the stance is not what is broken, the pillar-rounding
is. Revisit only after 3.1 is measured.

### 3.3 Graphical/headless divergence — diagnosed, not fixed

Seed 11 runs **93.44s headless** and **79.37s in the client** (Warrior dies
50.12s vs 47.83s). Same seed, same profile, same map. A genuinely different
trajectory, not end-detection granularity.

**Root cause:** headless builds from `MinimalPlugins` + `TransformPlugin`; the
client uses `DefaultPlugins` and its combatants carry `Mesh3d`/`MeshMaterial3d`.
Graphical-only systems *add and remove* components on those same entities
mid-match (`update_shield_bubbles`, `update_polymorph_visuals`,
`update_stealth_visuals`). Each add/remove moves the entity between archetypes;
Bevy query iteration follows archetype order; and the combat systems draw from
`GameRng` **in query iteration order** (`combat_core/auto_attack.rs`,
`casting.rs`, `damage.rs`). Different iteration order → different draw sequence →
different match.

Ruled out: visual effects drawing RNG (`rendering/effects.rs` deliberately uses
its own `drip_jitter` hash and never touches `GameRng`); `update_play_match`
(just an ESC keybinding handler); time scale (a persisted 2× speed explains the
10s countdown finishing in 4.9s wall clock with no sim implication).

Three ways to close it:
1. **Make sim iteration order component-independent** — collect and sort by a
   stable key (slot, or entity index) before any RNG-drawing loop. Correct and
   permanent; touches every draw site; **will move the headless baseline**, so it
   needs its own dated capture.
2. **Keep visual components off sim entities** — attach to child entities.
   Less invasive, but nothing enforces it afterwards.
3. **Declare headless the source of truth** and stop treating the client as
   reproducible.

Recommended: (1). The probe work depends on headless and the client agreeing
when you want to *watch* a seed you measured — which is exactly what failed.

### 3.4 Separate schedule bugs found along the way (fix regardless)

Not the divergence cause, but real, and the same class as the match-clock bug
fixed in `3a16a46`:
- `check_match_end` runs in `Update` in graphical mode, while headless runs
  `headless_check_match_end` in `FixedUpdate` (`src/states/mod.rs` ~line 165).
- `spawn_projectile_visuals` is registered in `Update` but declares
  `.in_set(CombatSystemPhase::CombatAndMovement)` — a set that now lives in
  `FixedUpdate`, so the constraint is silently void (`src/states/mod.rs` ~line 155).
- The graphical `Update` block at ~line 135 declares
  `.before(CombatSystemPhase::ResourcesAndAuras)`, also now cross-schedule.

---

## 4. Retractions — do not build on these

Three things stated earlier in the session were wrong. They are corrected here
so they are not inherited.

1. **"After the fixed-timestep change a seed reproduces in both modes."**
   False — see 3.3. The client cannot currently reproduce a headless seed.

2. **`seen_by_wl` and `near8` in the 12-seed sweep are contaminated.** The
   decision trace reports the Felhunter's class as `"Warlock"`, and the
   extractor keyed on `(team, class)`, so pet and owner overwrote each other
   (9235 rows under one key vs 4113 for the Priest). `seen_by_wl` measured a
   mixture of owner and pet; `near8` pooled pet positions as camping.
   **The claim that heavy campers are *more* visible to the Warlock is
   withdrawn.** `heal_to_warrior` and the win rate are parsed from match logs,
   not traces, and are unaffected.
   *Fix:* key on `actor.entity_id` and exclude pets via `actor.slot >= 10`
   (pet slots start at 10 — `PET_SLOT_BASE`).

3. **"Mana Burn is the mechanism."** Withdrawn. Jumped to from one console tail;
   the sweep shows TeamPlan wins average 0.8 Mana Burn hits and losses 0.9, and
   five of the seven zero-heal losses have *zero*. Not causal.

---

## 5. Measurement lessons (these cost real time)

- **The decision trace cannot measure movement under CC.** Trace events fire on
  *decisions*, and a unit under sustained CC makes none. On seed 11 the Warrior
  stopped emitting at 39.0s but lived to 50.12s — the trace read 12.3% blocked
  against the observer's 40.0%, understating the pathology **threefold by going
  blind exactly where it mattered**. Use `run_headless_match_observed` for
  anything position-dense.
- **Pets alias to their owner's class in the trace actor view.** Always key on
  `entity_id`, never `(team, class)`.
- **n=3 on a binary outcome is noise.** An earlier n=1 reading said camping took
  the team 2/3 → 0/3; paired at n=3 it was 2/3 → 1/3; at n=12 it is 9/12 → 5/12.
  Four commits were tuned against the n=1 reading. Do not tune against win rate
  below ~12 paired seeds.

---

## 6. Reproduction

```bash
# The probes (fast, ~10s)
cargo test --release --test movement_probes pillar_self_block -- --nocapture

# Watch a seed — NOTE: diverges from headless, see 3.3
cargo run --release -- --replay examples/replays/camp_fails_s11.json
cargo run --release -- --replay examples/replays/camp_fails_s11_legacy.json   # same seed, Legacy wins
cargo run --release -- --replay examples/replays/camp_works_s3.json           # win with the WORST ally sight

# Behaviour baseline (determinism reference — must stay byte-identical)
scripts/behaviour_baseline.sh | diff tests/baselines/legacy_behaviour_2026-08-01_fixed_timestep.txt -
```

Replay configs in `examples/replays/`: `camp_fails_s11`, `camp_fails_s11_legacy`,
`camp_fails_s9`, `camp_works_s5`, `camp_works_s3`.

The 24-cell sweep script was written to scratch and is not committed; it runs
`Legacy`/`TeamPlan` × seeds 1–12 of `Warrior+Priest` vs `Warlock+Priest` on
`PillaredArena`, parsing winner/duration/heals from match logs and geometry from
traces. Rebuild it with the `entity_id` keying fix from §4.2.

---

## 7. Branch state

```
145a757  test(probes): measure the healer's self-inflicted pillar blocking
3a16a46  fix(sim): run combat on a fixed timestep, not per rendered frame
fbe4250  line-of-sight cycling — poke to heal, duck to hide
0cf22c1  hide from every enemy, not just the nearest
bb0c405  camped healer must keep its partner in sight
9a83ca5  camp must outrank the posture directive
7d42a12  pillar-camp consumer — the "hold ground" primitive
```

Full suite green: **591 passed, 0 failed**. Working tree clean.

Note that commits `bb0c405`, `0cf22c1` and `fbe4250` were each tuned against the
n=1 seed-7 reading and each improved a metric without flipping an outcome. They
are not known to be wrong, but they are **not known to help either** — worth
re-measuring against the probes once 3.1 lands.

---

## 8. Open items, ranked

1. **Side commitment when rounding a pillar** (§3.1) — the primary fix; probes
   are in place to verify it.
2. **Graphical/headless divergence** (§3.3) — undermines the tool that found
   this bug in the first place.
3. **Schedule bugs** (§3.4) — small, independent, fix regardless.
4. **Re-derive `seen_by_wl` / `near8`** with the corrected extractor (§4.2).
5. **Was the Fear dodgeable?** (§2) — 1.05s of free movement during the school
   lockout; feasible but unconfirmed. Leads to a general "use lockout windows to
   break LoS" behaviour.
6. **Push the branch / open a PR** — 7 commits, still local.
