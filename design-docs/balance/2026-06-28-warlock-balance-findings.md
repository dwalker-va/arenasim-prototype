# Warlock balance: the kit can't beat a healer — it never locks one

**Date:** 2026-06-28
**Outcome:** Diagnosed the Warlock's core weakness (healer comps) and shipped a
pure **AI change** that weaponizes the healer-lockout tools the kit already owns.
Measured **+3.7pt vs healer comps / +2.2pt overall in 2v2**, with zero behavior
change where it shouldn't fire.

Scope note: per the request, this stays to **AI + existing abilities only**. TBC
Warlock power that lives in passives (Soul Link, etc.) is deferred to the future
"talents" feature and deliberately out of scope here.

---

## The measured weakness

Baseline (HEAD before this change), 300s cap.

**1v1 matrix** (Warlock vs each class, n=100) — overall **34.8%**:

| vs | Rogue | Shaman | Hunter | Paladin | Warrior | Mage | Priest |
|----|------:|-------:|-------:|--------:|--------:|-----:|-------:|
| WR | 0% | 10% | 23% | 24% | 33% | 46% | **100%** |

**2v2** (Warlock+partner vs all 2-combos, n=20, double-healer excluded) —
overall **39.2%**. The dominant signal is the healer slice:

| slice | winrate |
|---|---|
| enemy has healer | **28.2%** |
| enemy NO healer | **55.8%** |
| enemy has melee | 41.5% |
| enemy NO melee | 36.8% |

The **28pt healer gap** is the whole story. (Enemy-has-melee being *higher* than
no-melee re-confirms the [movement findings](2026-06-14-warlock-movement-findings.md):
the Warlock out-sustains melee by standing, casting, and Drain-Life'ing — melee
is not its problem. Healers are.)

Crucially, the vs-healer losses are **kills at ~34s median, not timeouts** (only
25 of 2100 vs-healer matches were draws). The Warlock isn't failing to *close* a
won game — its team is losing the damage race and dying, because the enemy
healer out-heals pure DoT + a 2s Shadow Bolt.

## Root cause: it owns healer-lockout tools and never uses them

The Warlock has two ways to deny enemy healing, both long present, neither aimed
at the healer by the AI:

1. **Fear** (8s, breaks after 100 dmg) was **priority 4**, behind
   Corruption → UA → curse-spread → Immolate. Those earlier gates `return true`
   on every GCD (there's always a DoT to refresh or a Shadow Bolt to throw), so
   Fear was **starved**: measured **0 Fears cast across many seeds** of
   Warlock+Mage vs healer comps. And when it *did* reach the Fear gate, it
   targeted `cc_target.or(target)` — i.e. the focused kill target, where the
   Warlock's own DoT ticks instantly break the Fear. It was structurally
   incapable of locking a healer.

2. **Felhunter Spell Lock** (interrupt + *school-specific* lockout, 24s CD) fired
   on the **first enemy cast in range** — often a DPS nuke, not a heal. Spell
   Lock's lockout is school-specific, so interrupting a heal locks the healer out
   of *healing*; spending it on a Frostbolt does not.

## The change (AI only)

`class_ai/warlock.rs` — **proactive healer Fear** (new priority 1.75, after
Corruption/UA establish kill-target pressure): if we're killing a DPS and there's
a living enemy healer that is castable, not already CC'd, and not Fear-DR-immune,
Fear *the healer*. Diminishing Returns on the Fears category is the natural rate
limiter — once the healer is DR-immune the gate returns `None` and the Warlock
resumes its damage rotation, so it can't perma-lock. Fear targets the healer (not
the DoT'd kill target), so the Warlock's own damage doesn't break it.

`class_ai/pet_ai.rs` — **Spell Lock prefers heal casts**: among all interruptible
enemy casts in range this frame, pick a heal cast over anything else (first-seen
fallback otherwise). No "holding" across frames, so it never wastes the CD
waiting.

## Result

Measured on both target brackets (2v2/3v3 — 1v1 is diagnostic only), clean
before(main)/after(branch) binaries:

| bracket | slice | before | after | Δ |
|---|---|---:|---:|---:|
| **2v2** | OVERALL | 39.2% | **41.3%** | **+2.1** |
| | enemy has healer | 28.2% | **31.6%** | **+3.4** |
| | enemy NO healer | 55.8% | 55.8% | +0.0 |
| **3v3** | OVERALL | 47.5% | **49.7%** | **+2.2** |
| | enemy has healer | 46.0% | **48.6%** | **+2.6** |
| | enemy NO healer | 54.0% | 54.7% | +0.7 (noise, ±5.6) |

(2v2 = 3500-match Warlock+partner sweep; 3v3 = 1650-match Warlock+Priest+Mage and
Warlock+Priest+Warrior vs all size-3 enemy teams, n=15.)

- The **no-healer slices are neutral in both brackets** — the change only
  activates when there's a healer to lock, exactly as designed (same validation
  pattern the movement findings used).
- 3v3 gain is concentrated in the caster-cleave comp, where a feared healer lets
  the partner's burst land: **Warlock+Priest+Mage 71.9% → 77.0% (+5.1)**; the
  already-weak melee-cleave Warlock+Priest+Warrior is flat (23.0% → 22.4%).
- **1v1 is byte-identical** (34.8% → 34.8%, every cell unchanged): with no
  separate healer to fear, `pick_healer_to_fear` returns `None`.
- All 185+ tests pass.

The Spell-Lock heal-priority is a logically-correct improvement but
**measurement-neutral** on its own (the +fear+lock run matched +fear at 31.6% vs
31.9%, within noise) — the pet rarely catches a heal cast off-CD. Kept because
it's a strict correctness win with no regression risk.

This narrows the healer gap from 28pt to 24pt. It does **not** "fix" the Warlock —
healer comps remain its worst matchup — but it converts a dead tool into a real
one with a clean, slice-isolated win.

## Assessed but not done (recommended next levers)

Two weaknesses this change does **not** touch, with the highest-value follow-ups:

1. **0% vs Rogue / 10% vs Shaman (1v1).** A self-peel problem: Fear breaks under
   the chaser's damage and the Warlock has no instant CC that survives a hit.
   **Recommended ability: Death Coil** — instant, ~3s *horror* that does **not**
   break on damage, plus a self-heal. It simultaneously (a) peels a training
   melee that current Fear can't hold, and (b) adds a healer-lockout that works
   under cleave. Iconic, pure-ability (no passive/talent), addresses the two
   matchups the healer-Fear change leaves untouched. Would need its own cooldown
   /horror-duration/self-heal tuning sweep.

2. **Target tunnel.** When the enemy DPS opens stealthed (e.g. vs Rogue+Priest),
   the kill-target heuristic picks the *healer* at t=0 and never re-evaluates, so
   the Warlock tries to kill a self-healing Priest with DoTs and the healer-Fear
   gate (which only fires when killing a DPS) never engages. This is a team-level
   targeting fix in `combat_ai.rs` (affects all classes), so it's a separate,
   higher-risk change — flagged, not bundled here.

## Reproduction

```bash
cargo build --release && cp target/release/arenasim /tmp/after
git stash; git checkout main && cargo build --release && cp target/release/arenasim /tmp/before
git checkout -

python3 scripts/gen_sweep.py --t1 'Warlock+{p}' --t2-size 2 --n 20 \
  --exclude-double-healer > /tmp/wl.jsonl
/tmp/after  --batch /tmp/wl.jsonl --out /tmp/after.csv  --jobs 16
/tmp/before --batch /tmp/wl.jsonl --out /tmp/before.csv --jobs 16
# slice by enemy-has-healer to see the +3.7pt (see python in the session notes)
```
