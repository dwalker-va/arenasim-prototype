#!/usr/bin/env python3
"""Positive check that each arm's binary reads ITS OWN assets.

`paths::assets_dir` resolves to the RELATIVE path "assets" for a development
build, so the asset tree a run reads is decided by the process CWD. This runs
one seeded Shaman match per arm with an absolute binary path and a hard-pinned
CWD, and reads the Shaman's wand-shot damage straight out of the log.

before: class base 7.0 dmg @ 0.8 speed  -> 5 vs the Warrior, ~1.27s cadence
after : mace      12.5 dmg @ 1.0 speed  -> 9 vs the Warrior, ~1.02s cadence

Damage is reported PER TARGET because it is post-armor. The Warrior row is the
like-for-like one: the after arm also shoots the cloth Priest (for 12), which
the before arm never reaches, so a mode taken across both targets reads 12 and
overstates the change.

A run that had picked up another card's mid-edit loadouts.ron/items.ron -- AS-87
is restatting this very mace -- would not land on these numbers.
"""
import json
import os
import re
import subprocess

S = "/private/tmp/claude-501/-Users-dwalker-Projects-arenasim-prototype--claude-worktrees-animations/9e56438a-688e-4700-ae79-5f012de98eea/scratchpad/proof"
REPO = "/Users/dwalker/Projects/arenasim-prototype/.claude/worktrees"
ARMS = {
    "before": os.path.join(REPO, "card-as97-before"),
    "after": os.path.join(REPO, "card-as97-shaman-weapon-damage"),
}

SHAMAN_WAND = re.compile(r"Team \d Shaman #\d's Wand Shot")

for tag, wt in ARMS.items():
    log = os.path.join(S, "assetcheck_%s.txt" % tag)
    cfg = os.path.join(S, "assetcheck_%s.json" % tag)
    with open(cfg, "w") as f:
        json.dump(
            {
                "team1": ["Shaman", "Priest"],
                "team2": ["Warrior", "Priest"],
                "map": "BasicArena",
                "random_seed": 7,
                "max_duration_secs": 300,
                "output_path": log,
            },
            f,
        )
    binary = os.path.join(wt, "target/release/arenasim")
    r = subprocess.run(
        [binary, "--headless", cfg], cwd=wt, capture_output=True, text=True
    )
    assert r.returncode == 0, "%s run failed: %s" % (tag, r.stderr[-1500:])

    # Damage is post-armor, so it is only comparable AGAINST THE SAME TARGET.
    # Moding across every target mixes in the cloth Priest, whom the after arm
    # shoots and the before arm never reaches, and reports 12 rather than the
    # like-for-like 9. So bucket by target and report each separately.
    times = []
    by_target = {}
    for line in open(log, errors="replace"):
        m = SHAMAN_WAND.search(line)
        if not m:
            continue
        t = re.match(r"\[\s*([0-9.]+)s\]", line)
        if t:
            times.append(float(t.group(1)))
        d = re.search(r"Wand Shot (?:hits|CRITS) (Team \d \w+ #\d) for (\d+) damage", line)
        if d and "CRITS" not in line and int(d.group(2)) > 0:
            by_target.setdefault(d.group(1), []).append(int(d.group(2)))

    gaps = [round(b - a, 2) for a, b in zip(times, times[1:]) if b - a < 2.0]
    modal = max(set(gaps), key=gaps.count) if gaps else None
    print(
        "%-7s wand_shots=%3d  modal_interval=%ss  assets=%s"
        % (tag, len(times), modal, os.path.join(wt, "assets/config/loadouts.ron"))
    )
    for tgt in sorted(by_target):
        v = by_target[tgt]
        print(
            "          vs %-18s normal_hits=%2d modal_damage=%d"
            % (tgt, len(v), max(set(v), key=v.count))
        )
