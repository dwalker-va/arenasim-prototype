# AS-179 — Hunter follow-ups from AS-125

**Tier:** per-card checks for milestone 0.7 — seeded repros, decision tests and
probes that pin each fix, byte-identity for comps with no Hunter. The win-rate
measurement is **deferred to the 0.7 milestone sweep**; the small runs below
show direction and mechanism, and are not a class's standing (n=10-50 per
cell).

**Base:** `main` at `8547f7b`. Inputs in `sweeps/2026-09-29-as179-*`: the
seeded repro (`repro.jsonl`), the no-healer set (`no-healer.jsonl`), the
no-Hunter control (`no-hunter.jsonl`), and the trap mechanism runner
(`trapmech.py` — AS-125's runner, plus a `breaker` column naming the damage
that broke a trap, and assets resolved next to the binary). Rows:
`2026-09-29-as179_{repro,no_healer,traps}_{base_8547f7b,after}.csv`.

## 1. The Hunter's own shot no longer breaks its own trap

**Mechanism.** A trap springs on whoever reaches it, and a Hunter already holds
fire on an enemy its trap holds (`pre_cast_ok`'s friendly-CC guard). It did not
look ahead: an Aimed Shot begun as an enemy ran into the Hunter's own unsprung
trap landed 1.6-2.5s after the trap sprang, and broke it. In the 27-comp trap
set (seeds 0-19, no kill target) at `8547f7b`, **8 of the 9 broken traps were
broken by the Hunter's own Aimed Shot** — six in `H+Pri vs Warlock+Rogue`,
where the lane trap goes on the Rogue once the Priest and the Warlock are
dead, one in `War+H vs Rogue+Priest` and one in `H+Pri vs Shaman+Rogue`.

**Fix.** The ability AI sees the Hunter's own Freezing Traps that have not
sprung — on the ground or still in flight, read each frame from the live
`Trap` / `TrapLaunchProjectile` entities, with the seconds until each arms.
Aimed Shot, Arcane Shot and Serpent Sting are not begun on a target that one
of them would catch between arming and the moment the shot's damage lands
(cast time, projectile flight, and for the sting its ticking duration). The
target's path is extrapolated exactly as the lane throw predicts its springer
(`first_inside`, factored out of `predicted_trap_springer` unchanged). A shot
that lands before the trap arms still goes. Traced as `own Freezing Trap would
catch the target before it lands`.

**Result** (same set, same seeds):

| | before | after |
|---|---|---|
| traps thrown | 331 | 338 |
| broken by damage | 9 (8 by the Hunter's own Aimed Shot) | 2 (1) |
| ran the full duration | 220 | 231 |

The one left (`H+Pri vs Shaman+Rogue` seed 2) is a healer trap the Rogue
walked into when its heading at the Aimed Shot's start did not carry it there:
the guard follows the springer prediction, and holding every shot near
any trap on the ground was not an option — traps never expire, so one lying
unsprung would suppress Aimed Shot for the rest of the match. The other break
is a partner Priest's Mind Blast (AS-125's partner-healer exception, as before).

**It does not recover `H+Pri vs Warlock+Rogue`.** AS-125's doc attributes that
comp's 20 → 10 to these breaks; with every one of its six breaks gone the
Hunter side wins **10 → 9 of 20**. Seeds 3, 8, 11, 14 and 18 were losses with
the trap broken and are losses with it running a full 8s: by the time the lane
trap goes on the Rogue, the fight is a lone Hunter against a Rogue, and a lone
Hunter holds fire for the whole freeze and resumes as it ends. Seed 19 goes the
other way — broken early, the Aimed Shot's damage had won it. The comp's loss
against `c79d8cc` is somewhere else in AS-125 (the held gates-open trap is the
candidate the AS-125 doc already names), and using the freeze window is a
separate question (follow-ups).

## 2. Hunter vs Priest: the opener's "has time" rule

**Mechanism.** AS-125 put Aimed Shot ahead of a due Serpent Sting in the
opener whenever the cast could finish (`aimed_shot_has_time`). A Priest has no
interrupt and is not a melee, so against a lone Priest there was always time
and the opener was always Aimed Shot. The sting-first order it replaced is
better there: the sting ticks a GCD longer, and the Priest spends its early
GCDs self-dispelling it — four Dispel Magics in the first 11s after the gates
of seed 7 — rather than Mind Blasting the Hunter. Disabling only the opener
rule restores 50/50 (the AS-125 tester's result, reproduced), and sting first
also kills faster: 35.3s mean over its 50 wins against 39.1s over the rule's
43.

**Fix.** The rule exists because the sting's GCD could cost the Aimed Shot its
window (Mage+Warrior: the Warrior arrived mid-cast after the sting). So that is
what it now asks (`aimed_shot_before_sting`): Aimed Shot goes first only when a
cast begun now would finish and one begun a GCD later would not. With time for
both, the sting goes first; with time for neither, the sting goes first too.
The Priest is not special-cased.

**Result.** Seeded 1v1, both slot orders, seeds 0-49:

| | before | after |
|---|---|---|
| Hunter vs Priest (Hunter slot 1) | 43/50 | 50/50 |
| Priest vs Hunter (Hunter slot 2) | 43/50 | 50/50 |

The after rows are identical, seed for seed, to the rule disabled outright.

**No-healer comps** (the comps the rule was added for): Hunter + each of 8
partners vs every no-healer pair of Warrior/Mage/Rogue/Warlock/Hunter, seeds
0-9, plus 1v1 vs every class both slot orders, seeds 0-19 — 1,520 matches.
Hunter-side win rate by enemy comp, pooled over partners (n=80 per 2v2 row,
40 per 1v1 row); unchanged rows omitted:

| enemy | before | after | flips |
|---|---|---|---|
| **Mage+Warrior** | 33.8% | 33.8% | none |
| Warrior+Hunter | 45.0% | 48.8% | +6/-3 |
| Mage+Mage | 20.0% | 28.8% | +13/-6 |
| Hunter+Hunter | 65.0% | 73.8% | +11/-4 |
| Mage+Hunter | 30.0% | 31.2% | +8/-7 |
| Warlock+Hunter | 60.0% | 61.2% | +4/-3 |
| Rogue+Hunter | 93.8% | 92.5% | +2/-3 |
| 1v1 Priest | 85.0% | 100.0% | +6/-0 |
| 1v1 Hunter (mirror, slot-1 wins) | 10.0% | 40.0% | +14/-2 |
| **all 1,520** | 49.1% | 51.4% | |

No enemy comp moves down beyond a single flip. Mage+Warrior does not move at
all: at `8547f7b` the opener rule already made no difference there (disabling
it entirely gives the same 33.8%). The one place the rule still fires and
matters in this set is **Warrior+Rogue**, where it keeps its Aimed Shot first
(90.0% → 90.0%) and dropping the rule altogether would have gone to 95.0%
(`H+Pri` 6 → 9 of 10) — n=10, and the same order as before this card, so it is
noted for the milestone sweep rather than acted on. The mirror's rise is draws
turning into wins: AS-125 measured both Hunters opening with the same Aimed
Shot and racing to simultaneous deaths.

## 3. The pressure trap while pinned: unchanged, measured

The case (AS-125 tester, `H+Pri vs Rogue+Priest`, `team1_kill_target: 1`,
seed 0): the Hunter throws the healer trap, the Rogue lands Crippling Poison,
Disengage is on cooldown, and the Hunter makes no shot for the freeze and dies
at 48.4s while the Priest thaws.

The arm (`sweeps/2026-09-29-as179-arm-no-pressure-trap-while-impaired.patch`)
does not throw the pressure trap from the dead zone while the Hunter is slowed
or rooted — at that point Disengage has already failed, so a slowed Hunter
cannot make room to shoot the melee. Same trap set, seeds 0-19:

| kill targets | healer traps | seconds healers held | Hunter-side wins | matches whose winner changed |
|---|---|---|---|---|
| none (540) | 161 → 157 | 1,102 → 1,060 | 260 → 260 | 0 |
| both at slot 0 (540) | 158 → 156 | 1,113 → 1,095 | 357 → 360 | 5 |
| team 1 on the healer (340, healer comps) | 116 → 116 | 921 → 899 | 115 → 116 | 3 |

**Not changed.** The pinned throws are working traps — the three `H+Pri vs
Rogue+Priest` throws the arm removes each held the Priest the full 8s — and
removing them moves nothing a 20-seed set can resolve (+0, +3, +1 wins). An
8s freeze on the enemy healer is worth something to the Hunter's team whether
or not the Hunter can shoot through it. And the tester's seed itself is not
this case: its throw comes before the Rogue lands Crippling Poison (36.3s
against 40.8s in the log), so the arm throws at the same moment and loses the
same way.
A rule that caught it would have to forecast the pin, not observe it — more
machinery for no measured gain.

## Byte-identity

The no-Hunter control — 140 1v1, 240 2v2 and 40 3v3 matches with no Hunter on
either side — is byte-identical before and after (420/420 rows; 406 ended by a
kill, 24 draws, 319 distinct durations). Every change is inside the Hunter's
ability AI; the live-trap view is built from trap entities, which only a
Hunter spawns.

## Follow-ups

- **Use the freeze window.** A lone Hunter holding fire through its own trap
  on its only enemy (`H+Pri vs Warlock+Rogue` endgames) could time an Aimed
  Shot to land as the trap ends, or open distance, instead of idling 8s.
- **Is the opener rule still earning its place?** At `8547f7b` it changes no
  Mage+Warrior outcome, and in Warrior+Rogue sting-first scored higher. For
  the milestone sweep.
- **`H+Pri vs Warlock+Rogue` 20 → 10 is not the trap breaks.** Re-attribute
  it against `c79d8cc`.
