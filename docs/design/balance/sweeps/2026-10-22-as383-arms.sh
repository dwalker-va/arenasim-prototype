#!/bin/bash
# AS-383 directional arms: one `--batch` run of the AS-292 reachable-cell
# generator (`2026-10-19-as292-gen.py 10 2`, 9,900 matches) per AS383_EXP
# knob, on ONE binary built from main cd0cdf3d with
# `2026-10-22-as383-instrument-and-knobs.patch` applied. The unset arm is the
# baseline; every arm is scored against it with `2026-10-19-as292-h2h.py`.
#
#   2026-10-22-as383-arms.sh <binary> <h2h.jsonl> <outdir> "" nodip dipgate_cc ...
#
# Knobs (TeamPlan Priest only, except fs): nodip (no scream dip), dipgate_tgt
# (no dip entry while the dip target targets the Priest within the intent
# radius), dipgate_cc / dipgate_path (no dip entry when the arrival spot / the
# straight walk is inside a ready hard-CC disc, the dip target's own excluded),
# dipcut_tgt / dipcut_cc (cut a live dip to PRESSURED on the same tests), fs
# (Frost Shock added to every TeamPlan healer's CcField). Comma-separate to
# combine.
bin=$1; jsonl=$2; out=$3; shift 3
for v in "$@"; do
  AS383_EXP="$v" "$bin" --batch "$jsonl" --out "$out/h2h_${v:-base}.csv" --jobs 4 --trace-mode off
done
