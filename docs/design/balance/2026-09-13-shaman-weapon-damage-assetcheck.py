#!/usr/bin/env python3
"""Positive check that each arm's binary reads ITS OWN assets.

`paths::assets_dir` resolves to the RELATIVE path "assets" for a development
build, so the asset tree a run reads is decided by the process CWD. This runs
one seeded Shaman match per arm with an absolute binary path and a hard-pinned
CWD, and reads the Shaman's wand-shot damage straight out of the log.

before: class base 7.0 dmg @ 0.8 speed  -> 5 per hit post-armor, ~1.27s cadence
after : mace      12.5 dmg @ 1.0 speed  -> 9 per hit post-armor, ~1.02s cadence

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

    dmgs, times = [], []
    for line in open(log, errors="replace"):
        if SHAMAN_WAND.search(line):
            t = re.match(r"\[\s*([0-9.]+)s\]", line)
            d = re.search(r"for (\d+) damage", line)
            if t:
                times.append(float(t.group(1)))
            if d and int(d.group(1)) > 0:
                dmgs.append(int(d.group(1)))

    gaps = [round(b - a, 2) for a, b in zip(times, times[1:]) if b - a < 2.0]
    modal = max(set(gaps), key=gaps.count) if gaps else None
    common = max(set(dmgs), key=dmgs.count) if dmgs else None
    print(
        "%-7s wand_shots=%3d  modal_damage=%s  modal_interval=%ss  assets=%s"
        % (tag, len(times), common, modal, os.path.join(wt, "assets/config/loadouts.ron"))
    )
