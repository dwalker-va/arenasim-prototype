# Committed sweep inputs

One JSONL per balance sweep whose findings are published in
`docs/design/balance/`. These are the **exact** files fed to
`arenasim --batch`, so a published result can be re-run rather than
re-derived — including the map, which the batch runner's CSV does not carry.

A findings doc that cites a sweep should name its input file here.

## `2026-09-13-as54-frost-armor-chill.jsonl`

Behind `2026-09-13-frost-armor-one-debuff-findings.md` (card AS-54). 5,150
configs: a 2v2 arm and a 3v3 arm concatenated, every line pinned to
`BasicArena`. Regenerate with:

```bash
scripts/gen_sweep.py --t1 'Mage+{p}' --t2-size 2 --n 20 --seed-base 0 \
  --exclude-double-healer --extra '{"map":"BasicArena"}'          # 3,500
scripts/gen_sweep.py --t1 'Mage+Priest+Warrior' --t2-size 3 --n 30 \
  --seed-base 0 --extra '{"map":"BasicArena"}'                    # 1,650
```

Run each arm's binary over the whole file, then compare them:

```bash
<binary> --batch docs/design/balance/sweeps/2026-09-13-as54-frost-armor-chill.jsonl \
  --out <before|after>.csv --jobs 6 --trace-mode off

sweeps/2026-09-13-as54-paired.py before.csv after.csv
```

`2026-09-13-as54-paired.py` is the analysis that produced the published tables:
McNemar on the flip counts, Wilson intervals for the level, and the slices the
change can physically reach. It lives beside its input rather than in
`scripts/` because its slicing knows what a Frost Armor chill is.
