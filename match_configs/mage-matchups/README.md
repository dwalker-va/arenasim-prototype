# The priced Mage across matchups

> **`MAGE_PRICED_POLYMORPH` is currently `true`** in
> `src/states/play_match/class_ai/mage.rs` so these are watchable. It only takes
> effect under `cc_policy: "Priced"`, and `Identity` is the default — so default
> behaviour everywhere else is unchanged. One line to revert.

## Why these exist

Every previous measurement of the priced Mage — all eight attempts — used
`Mage+Priest vs Warlock+Priest`. That is the Warlock's **designed** counter to
the Mage (see the `warlock-soft-counters-mage` note): the Felhunter exists to
shred exactly this. Judging the feature there alone was a methodology error.

Measured across seven matchups at n=300, BasicArena:

| opponent | Δ when the Mage gets `Priced` | z |
|---|---|---|
| **Paladin+Warrior** | **+41pt** | +10.64 |
| Mage+Priest (mirror) | +1 | +0.25 |
| Paladin+Shaman | −1 | −1.74 |
| Warrior+Priest | −3 | −1.64 |
| Hunter+Priest | −8 | −4.35 |
| Warlock+Priest (the counter) | −19 | −4.74 |
| **Rogue+Priest** | **−27** | −7.84 |

Not "it doesn't work". It is strongly matchup-dependent, with a large win, a
large loss, and a cluster of near-neutral cells.

---

## 1. The win — `+41pt`

```bash
cargo run --release -- --replay match_configs/mage-matchups/win-vs-paladin-warrior-identity.json
cargo run --release -- --replay match_configs/mage-matchups/win-vs-paladin-warrior-priced.json
```

Same seed, same teams; only the policy differs.

| policy | casts | target | mean lifetime | total denial |
|---|---|---|---|---|
| Identity | 39 | **all Warrior** | **0.21s** (51% broke) | **8.3s** |
| Priced | 26 | 15 Paladin / 11 Warrior | **9.36s** (93% expired) | **146.1s** |

**Watch the Identity run first.** The Mage sheeps the Warrior over and over —
the unit its own team is engaged with — and it never sticks. Thirty-nine casts
buy eight seconds.

Two mechanisms remove it, roughly evenly (51% broke on damage, 49% dispelled),
and which one you see depends on the seed. **Seed 3 is all dispel**: the sheep
lands at 26.6s and the Paladin's Cleanse strips it 0.03s later, then again at
29.6s. Watch the Paladin, not just the damage.

The priced run sheeps the **Paladin** instead: nobody is hitting it, so 93% of
those run the full 10 seconds. This is the value model deriving *"damage the
kill target, CC the off target"* from expected duration rather than being told
it — the original goal of the whole exercise.

## 2. The regression — `−13pt`, seed 6

```bash
cargo run --release -- --replay match_configs/mage-matchups/regression-vs-rogue-priest-identity.json
cargo run --release -- --replay match_configs/mage-matchups/regression-vs-rogue-priest-priced.json
```

Seed 6 chosen because Identity **wins** and Priced **loses** on it, and the
mechanism is legible rather than incidental. Three other seeds in 1-24 flip the
same way (14, 17, 22) with the same shape.

| | Identity | Priced |
|---|---|---|
| Polymorphs | 0 | 2 (Priest at 35.2s, Rogue at 38.0s) |
| enemy Priest dies | **41.1s** | **64.2s** |
| enemy Rogue dies | 53.5s | never |
| result | **Team 1 wins** | Team 1 wiped, 67.2s / 76.5s |

**Watch the enemy Priest's health bar.** Under Identity the Mage simply kills
it — dead at 41s, Rogue mopped up at 53s. Under Priced the Mage sheeps that same
Priest at 35.2s, *while it is in the middle of killing it*. Polymorph breaks on
any damage, so `pre_cast_ok`'s friendly-CC guard makes the whole team stop
attacking it — and the kill slips from 41 seconds to **64 seconds**, by which
point the Rogue has killed the Mage.

## Why the `forgone_damage` fix does not catch this

It is not the same error as the Warrior case above. The model does charge for
lost damage — but sheeping a HEALER also scores a large `healing_capped` denial,
which is deliberately *not* discounted (see `DAMAGE_DENIAL_DISCOUNT`), and that
outweighs the forgone damage.

The arithmetic is locally correct and strategically wrong, because it is missing
one idea: **a kill is permanent and crowd control is temporary.** Denying a
healer 10 seconds of casting is worth something; killing that healer denies it
*the rest of the match*. Crowd-controlling a target you are about to kill trades
the permanent removal for the temporary one — and pays a 1.5s cast for the
privilege.

The missing term is a kill-proximity penalty: value CC on a target inversely to
how close it is to dying. That is the next fix, and it is not yet implemented.

## The common cause

Both cells are ranking defects with clean repros — a much better problem to have
than the "T_eff is unpredictable" verdict the single-cell measurement produced.
They are however DIFFERENT defects:

- **Paladin+Warrior** was "we are damaging this target and the penalty could not
  see it" — fixed by charging `forgone_damage` at commitment rather than at the
  victim's trailing incoming rate. +41pt -> +70pt.
- **Rogue+Priest** is "we are about to KILL this target" — not fixed. Needs a
  kill-proximity term, because a kill is permanent and crowd control is not.
