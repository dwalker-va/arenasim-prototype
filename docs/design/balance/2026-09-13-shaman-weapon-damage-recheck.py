#!/usr/bin/env python3
"""Contamination re-check for the AS-97 sweep.

Re-runs a stratified subset of the committed sweep input (seeds 0-9 of all 14
cells) through both arms with a HARD-PINNED working directory and an ABSOLUTE
binary path, then compares every row against the committed CSVs.

If the original run had read another card's mid-edit assets -- or the wrong
tree's binary, which a relative `./target/release/arenasim` could have resolved
to under a flapped CWD -- these rows would not reproduce.
"""
import csv
import json
import os
import subprocess
import sys

S = "/private/tmp/claude-501/-Users-dwalker-Projects-arenasim-prototype--claude-worktrees-animations/9e56438a-688e-4700-ae79-5f012de98eea/scratchpad/proof"
REPO = "/Users/dwalker/Projects/arenasim-prototype/.claude/worktrees"
ARMS = {
    "before": os.path.join(REPO, "card-as97-before"),
    "after": os.path.join(REPO, "card-as97-shaman-weapon-damage"),
}
COMMITTED = {
    "before": os.path.join(
        ARMS["after"],
        "docs/design/balance/2026-09-13-shaman-weapon-damage-before.csv",
    ),
    "after": os.path.join(
        ARMS["after"],
        "docs/design/balance/2026-09-13-shaman-weapon-damage-after.csv",
    ),
}
SEEDS = set(range(10))

# Build the subset from the COMMITTED input, so we re-run exactly what shipped.
src = os.path.join(ARMS["after"], "docs/design/balance/2026-09-13-shaman-weapon-damage-sweep.jsonl")
subset = os.path.join(S, "recheck_subset.jsonl")
n = 0
with open(src) as fi, open(subset, "w") as fo:
    for line in fi:
        cfg = json.loads(line)
        if cfg["random_seed"] in SEEDS:
            fo.write(line)
            n += 1
print("subset: %d matches per arm (seeds 0-9 of every cell)" % n)

FIELDS = ("winner", "end_reason", "duration_secs")
overall_ok = True

for tag, wt in ARMS.items():
    binary = os.path.join(wt, "target/release/arenasim")
    assert os.path.isfile(binary), "missing binary: " + binary
    out = os.path.join(S, "recheck_%s.csv" % tag)

    # Hard pin: absolute binary, cwd forced to this arm's worktree.
    r = subprocess.run(
        [binary, "--batch", subset, "--out", out],
        cwd=wt,
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        print("%s: RUN FAILED rc=%d\n%s" % (tag, r.returncode, r.stderr[-2000:]))
        overall_ok = False
        continue

    def load(p):
        d = {}
        with open(p) as f:
            for row in csv.DictReader(f):
                d[(row["label"], row["seed"])] = row
        return d

    fresh, orig = load(out), load(COMMITTED[tag])
    missing = [k for k in fresh if k not in orig]
    mismatch = [
        k for k in fresh if k in orig and any(fresh[k][f] != orig[k][f] for f in FIELDS)
    ]
    print(
        "%-7s rows=%d  matched=%d  mismatched=%d  missing_from_committed=%d"
        % (tag, len(fresh), len(fresh) - len(mismatch) - len(missing), len(mismatch), len(missing))
    )
    for k in mismatch[:10]:
        print("   MISMATCH %s seed=%s: committed=%s fresh=%s" % (
            k[0], k[1],
            {f: orig[k][f] for f in FIELDS},
            {f: fresh[k][f] for f in FIELDS},
        ))
    if mismatch or missing:
        overall_ok = False

print()
print("VERDICT: %s" % ("CLEAN - committed sweep reproduces exactly" if overall_ok
                       else "CONTAMINATED - committed sweep does NOT reproduce"))
sys.exit(0 if overall_ok else 1)
