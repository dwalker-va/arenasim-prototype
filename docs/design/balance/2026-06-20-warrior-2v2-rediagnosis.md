# Warrior 2v2 re-diagnosis — Hunter+Priest vs Warrior+Priest

**Date:** 2026-06-20
**Build:** Psychic Scream meta (PR #73, `origin/main` 9130933)
**Comp:** `Hunter+Priest` (team1) vs `Warrior+Priest` (team2)

## Summary

The roadmap previously listed this as a ~0% healer **target-selection** bug —
"the Priest heals *itself* instead of peeling the focused Hunter when it is
trained through Mortal Wounds." Reproducing on the merged Psychic Scream build
shows that framing is wrong. The comp sits at **~19%**, and the loss is driven
by **zero Hunter offensive uptime**, not healer targeting.

## Reproduction

```bash
# 16 seeds, single-match random_seed
for s in $(seq 0 15); do
  printf '{"team1":["Hunter","Priest"],"team2":["Warrior","Priest"],"random_seed":%d}\n' $s > /tmp/w.json
  cargo run --release -- --headless /tmp/w.json
done
```

| Build | Result (N=16) |
|---|---|
| Pre-scream (`4fb595a`) | 0/16 |
| Psychic Scream (`e211ad4`/`9130933`) | **3/16 (~19%)** |

## Findings

1. **The Warrior trains the *enemy Priest*, not the Hunter.** Across all 16
   seeds the first Mortal Strike lands on the team1 Priest, which dies **first**
   (~47s); the Hunter then loses 1v2 (~65s). There is no Hunter→Priest→Hunter
   swap-back on this build. Self-healing is therefore **correct** targeting — the
   Priest *is* the focused target — so the original "heals itself instead of the
   Hunter" framing does not apply.

2. **The focused Priest can't out-sustain Mortal Wounds + interrupt-lock.** Flash
   Heals on itself decay under healing reduction (66 → 43), and the Warrior
   repeatedly interrupt-locks the Holy school (4s ×3 in seed 0 at 15.7/27.8/39.8s).

3. **Near-zero Hunter kill pressure.** Hunter+pet deal only ~609 dmg/match to
   team2. The cast list is *all peels* — Freezing Trap, Frost Trap, Disengage,
   Concussive Shot — which get DR'd (Concussive 4.0s → 2.0s, DR 50%) as the
   Warrior re-closes. **Aimed Shot never lands.** No kill race exists, so the
   match is pure attrition the Warrior wins.

4. **Psychic Scream is a real but capped lever, not a fix.** The defensive scream
   fires reliably (~17s, once PRESSURED, fully instrumented in the decision
   trace), fearing the Warrior for 8s. But the enemy double-healer comp **dispels
   the Fear within ~1.7s every time** (16/16 casts dispelled), and the 30s
   cooldown makes it once-per-fight. It buys the 0% → ~19% lift, nothing more —
   the Priest AI can't prevent the dispel.

## Conclusion / leverage

The win condition is **Hunter offensive uptime** (a kill race), not healer
target-selection. See roadmap bucket B — the "plant when safe" strategic layer
and Hunter burst-during-CC — as the real levers. The "zero Hunter damage uptime"
half of the original diagnosis is strongly confirmed; the "healer self-peel"
half is build-dependent and no longer the bottleneck.
