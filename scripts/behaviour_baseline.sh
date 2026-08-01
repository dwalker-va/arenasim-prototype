#!/usr/bin/env bash
#
# Behaviour baseline — a content digest of a fixed set of matches.
#
# WHY: some changes are supposed to alter nothing. The `TeamPlan` migration
# (design-docs/team-level-positioning-ai.md) opens with a step that must be a
# *provable* no-op, and "the tests still pass" is far too coarse a check for
# that — the calibrated probes assert bounded properties, not exact behaviour.
# This records the exact outcome of a fixed match set so a later run can be
# diffed against it line by line.
#
# A seeded match log is fully deterministic — no wall-clock content — so the
# SHA of the log is a valid identity for the whole simulation, not just the
# winner. Two runs agreeing here means every damage roll, movement decision and
# ability choice matched.
#
# USAGE
#   scripts/behaviour_baseline.sh                        # print the digest
#   scripts/behaviour_baseline.sh > baseline.txt         # record it
#   scripts/behaviour_baseline.sh | diff baseline.txt -  # verify no drift
#
# A non-empty diff means behaviour changed. That is not automatically wrong —
# but it must be INTENDED, and on a no-op step it is a failure.
#
# Coverage is deliberately spread across the axes that can move independently:
#   - maps: obstacle-free, small-with-cover, and the large bowl
#   - comps: healer/melee, double-ranged, mirror, and a pet owner
#   - seeds: three per cell, since a single seed can miss a regression
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

MAPS=("BasicArena" "TwinPillars" "PillaredArena")
# name:team1:team2 — chosen to exercise different subsystems, not for balance.
COMPS=(
  "healer_v_healer:Warrior,Priest:Warlock,Priest"
  "ranged_v_melee:Mage,Priest:Warrior,Paladin"
  "pet_comp:Hunter,Shaman:Rogue,Priest"
)
SEEDS=(1 4 7)

AI_PROFILE="${AI_PROFILE:-Legacy}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "# behaviour baseline  ai_profile=${AI_PROFILE}"
echo "# map comp seed winner duration log_sha256"

for map in "${MAPS[@]}"; do
  for entry in "${COMPS[@]}"; do
    name="${entry%%:*}"; rest="${entry#*:}"
    t1="${rest%%:*}"; t2="${rest#*:}"
    for seed in "${SEEDS[@]}"; do
      python3 - "$TMP/cfg.json" "$t1" "$t2" "$map" "$seed" "$AI_PROFILE" <<'PY'
import json, sys
out, t1, t2, mp, seed, prof = sys.argv[1:7]
json.dump({
    "team1": t1.split(","), "team2": t2.split(","),
    "map": mp, "max_duration_secs": 300,
    "random_seed": int(seed), "ai_profile": prof,
}, open(out, "w"))
PY
      # stderr is kept: with `set -e` a build or match failure would otherwise
      # abort the whole sweep with no diagnostic at all.
      cargo run --release --quiet -- --headless "$TMP/cfg.json" >/dev/null
      # `sed -n 1p`, not `head -1`: head exits after the first line, and under
      # `set -o pipefail` the resulting SIGPIPE on `ls` fails the assignment once
      # match_logs/ grows past a pipe buffer. sed drains the stream.
      log=$(ls -t match_logs/match_*.txt | sed -n 1p)
      # Whole value, not $2 — "Winner: Team 1" would otherwise record as "Team".
      winner=$(grep -m1 '^Winner:'   "$log" | sed 's/^Winner: *//' | tr ' ' '_')
      dur=$(grep    -m1 '^Duration:' "$log" | awk '{print $2}')
      # Full-log digest: catches any divergence, not just the outcome.
      sha=$(shasum -a 256 "$log" | awk '{print substr($1,1,16)}')
      printf '%-14s %-16s %-4s %-8s %-10s %s\n' "$map" "$name" "$seed" "$winner" "$dur" "$sha"
    done
  done
done
