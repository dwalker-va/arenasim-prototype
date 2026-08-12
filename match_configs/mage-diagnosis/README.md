# Mage diagnosis — why the priced model makes the Mage worse

`cc_policy: Priced` measures **-8 to -10pt (z~-2)** for `Mage+Priest`, while the
same policy is **+12pt (z=+2.87)** for the `Warlock+Priest` side of the identical
sweep. Four diagnosed-and-fixed mechanisms did not close it.

Comp: `Mage+Priest` (ours) vs `Warlock+Priest`. **No kill call** — free
targeting, unlike the Warlock set. Only `cc_policy` differs within each pair, and
matches are deterministic.

```bash
cargo run --release -- --replay match_configs/mage-diagnosis/mage-seed7-identity.json
cargo run --release -- --replay match_configs/mage-diagnosis/mage-seed7-priced.json
```

Check the banner reads `cc CcPolicies { team1: Priced, ... }` on the priced half.

## What the change does

The Mage's Polymorph gate had a hardcoded guard — *"cc_target equals kill target
— would break on damage"* — replaced by a priced choice over all enemies. It
sheeps **2.5x more often**: over 10 seeds, 6 Polymorphs (all on the enemy healer)
became 15 (12 healer, 3 Warlock), the last of which the old guard forbade.

## Four things found by reading a match, all real, none sufficient

Each is now modelled and none fixed the regression:

1. **Devour Magic.** The enemy Felhunter strips a Polymorph **0.03s** after it
   lands. `T_eff` counted only healers as dispellers, so it predicted a near-full
   10s sheep against a comp that removes it instantly. Now counted.
2. **Our own damage stops.** `check_friendly_cc` makes our team skip any target
   carrying a friendly break-on-any-damage aura — so sheeping the unit we are
   killing takes it out of the kill. Now subtracted as `forgone_damage`.
3. **Interrupt value is unpredictable here.** The devoured sheep still cancelled a
   Flash Heal — but at decision time the heal **had not started**. `I` cannot be
   read off a cast in flight when the cast begins after the decision.
4. **A landed CC costs a global** regardless of duration. Now floored at one GCD.

## What to watch for

The behavioural difference is small and specific. In seed 7 the runs are
**identical until 22.35s**, where `Identity` casts Polymorph and `Priced` casts
Frostbolt — then everything downstream diverges.

1. **Is the extra sheeping actually bad, or is it cascade?** A single substitution
   re-rolls the rest of the match. Watching the Warlock set, one apparent
   "improvement" turned out to be *our Priest* getting a different opportunity,
   nothing to do with the model. The same trap applies here.
2. **What does the Mage give up?** Every Polymorph is a cast that is not a
   Frostbolt, against a comp that devours the sheep almost immediately.
3. **Does sheeping the healer help at all in this matchup?** Identity's 6 casts
   might already be too many rather than too few.
4. **Is the identity heuristic winning for a reason, or by luck?** `cc_target`
   plus "never sheep the kill target" is crude, and it beats the priced model by
   8-10 points. Something in it is right that the value model does not capture.
