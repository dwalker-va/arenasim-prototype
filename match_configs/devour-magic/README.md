# Watch: why the Mage cannot join the priced CC model

```bash
cargo run --release -- --replay match_configs/devour-magic/seed1.json
```

`Mage+Priest` vs `Warlock+Priest`, seed 1, BasicArena. **Default `Identity` AI —
no code change, no flag.** This is the shipped behaviour.

Seed chosen by MECHANISM (most Polymorph/Devour pairs across seeds 1-20), not by
who won.

## What to watch for

Timestamps are the log clock, which starts at match start and includes the 10s
pre-match countdown — so subtract ~10s for time-since-gates.

| time | what happens |
|---|---|
| **23.87s** | Mage begins casting Polymorph on the enemy Priest — a **1.5s** cast |
| **25.37s** | It lands. Full value: **10.0s, DR 100%** |
| 25.38s | It does its job — the Priest's Flash Heal is interrupted by the CC |
| 25.38s | Felhunter uses **Devour Magic** (instant, 0 mana, off the GCD) |
| **25.40s** | `[DEVOUR] Polymorph removed` |

**A 1.5-second cast bought 0.03 seconds of crowd control.**

Then watch the cost compound:

| time | what happens |
|---|---|
| 26.90s | Mage casts Polymorph on the same Priest again |
| **28.40s** | Lands at **5.0s, DR 50%** — the 0.03s sheep burned a diminishing-returns tier |
| 33.38s | Felhunter devours it again (this one held ~5s — its cooldown was up) |

## Why this parks the whole Mage extension

Polymorph's `break_on_damage` is `0.0`, so it also dies to any single point of
damage. Between that and Devour Magic, its realised duration is wildly variable —
and a decision takes the `argmax` of a predicted duration, which systematically
selects the cases where the prediction is most over-optimistic. The **optimizer's
curse**: improving average prediction accuracy by 32 points of skill changed the
Mage's results not at all, because what poisons an argmax is the upper tail of
the error, not its mean.

Measured over 20 matches of this exact cell, scoring denial rather than win rate:

| policy | casts | mean lifetime | dispelled | total denial |
|---|---|---|---|---|
| Identity | 18 | 3.29s of 7.78s | 67% | **59.2s** |
| Priced | 19 | 1.26s of 10.00s | 94% | **26.5s** |

The priced Mage casts *more* Polymorphs for *less than half* the denial. Hence
`MAGE_PRICED_POLYMORPH = false`.

Fear is unaffected by all of this — it carries a 100-damage budget and nothing
removes it instantly — which is why the Warlock is on the priced model at +9pt
(z=+2.43) and the Mage is not.

## The counter-play this suggests

Cross-CC the Felhunter first, or bait the dispel with a debuff that does not
invalidate the sheep. Neither exists yet; both are the same queued item.

## To watch the priced Mage instead

Flip `MAGE_PRICED_POLYMORPH` to `true` in
`src/states/play_match/class_ai/mage.rs`, rebuild, and add `"cc_policy":
"Priced"` to the config. With the flag off, `Priced` and `Identity` are
identical for the Mage by construction.
