#!/bin/bash
#
# Movement KPI extraction from AI decision-trace JSONL files.
#
# Computes position-derivable KPIs per match per entity from the positions
# carried on trace events. Today only `ability_decision` events carry an
# actor position; `movement_decision` events (U3+) will too — both kinds are
# handled, and missing kinds are tolerated (a trace with no movement events
# still produces KPIs from ability events).
#
# KPIs per (match, team, slot, class):
#   - post_gate_path_len   straight-line path length estimate between
#                          consecutive post-gate position samples (an
#                          UNDERESTIMATE: events are sparse, corners are cut)
#   - avg_nearest_enemy    mean distance to the nearest enemy with a sample
#                          in the same frame
#   - min_nearest_enemy    minimum of the same
#   - pct_within_4yd       % of paired samples with nearest enemy <= 4yd
#   - pct_within_10yd      % of paired samples with nearest enemy <= 10yd
#
# Distances are computed on the x/z plane (y is height; pets float at a
# different y than their owners).
#
# Truncated traces (SIGKILL / OOM mid-match) leave a partial last line;
# `fromjson?` skips unparseable lines instead of aborting (see CLAUDE.md
# trace recipes).
#
# Usage:
#   scripts/movement_kpis.sh [--gate-time T] <trace.jsonl> [more traces...]
#
# Options:
#   --gate-time T   sim_time at which gates open (default: 10.0 — the fixed
#                   10s countdown). Samples before T are excluded from path
#                   length; distance KPIs use all samples.
#
# Output: CSV on stdout —
#   match,team,slot,class,samples,post_gate_path_len,avg_nearest_enemy,min_nearest_enemy,pct_within_4yd,pct_within_10yd

set -euo pipefail

GATE_TIME=10.0
TRACES=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --gate-time)
            GATE_TIME="$2"
            shift 2
            ;;
        --help|-h)
            grep '^#' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            TRACES+=("$1")
            shift
            ;;
    esac
done

if [[ ${#TRACES[@]} -eq 0 ]]; then
    echo "usage: $0 [--gate-time T] <trace.jsonl> [more traces...]" >&2
    exit 1
fi

echo "match,team,slot,class,samples,post_gate_path_len,avg_nearest_enemy,min_nearest_enemy,pct_within_4yd,pct_within_10yd"

for trace in "${TRACES[@]}"; do
    if [[ ! -f "$trace" ]]; then
        echo "warning: $trace not found, skipping" >&2
        continue
    fi
    match_name="$(basename "$trace")"

    # Extract: frame, sim_time, team, slot, class, x, z — one TSV row per
    # position-carrying event. `fromjson?` tolerates the truncated last line.
    # The writer canonicalizes order by (frame, entity, kind), so rows arrive
    # frame-grouped; sort -n anyway in case of concatenated/partial traces.
    jq -R -r '
        fromjson?
        | select(.kind == "ability_decision" or .kind == "movement_decision")
        | select(.actor != null and .actor.position != null)
        | [ .frame, .sim_time,
            .actor.team, .actor.slot, (.actor.class | tostring),
            .actor.position[0], .actor.position[2] ]
        | @tsv
    ' "$trace" 2>/dev/null \
    | sort -t$'\t' -k1,1n -k3,3n -k4,4n \
    | awk -F'\t' -v match_name="$match_name" -v gate_time="$GATE_TIME" '
        function flush_frame(    i, j, d, dx, dz, best) {
            # For every actor sampled in the buffered frame, find the nearest
            # same-frame enemy sample. Frames with no enemy sample contribute
            # nothing to the distance KPIs (tolerated, not an error).
            for (i = 1; i <= fn; i++) {
                best = -1
                for (j = 1; j <= fn; j++) {
                    if (fteam[i] == fteam[j]) continue
                    dx = fx[i] - fx[j]; dz = fz[i] - fz[j]
                    d = sqrt(dx*dx + dz*dz)
                    if (best < 0 || d < best) best = d
                }
                if (best >= 0) {
                    k = fkey[i]
                    pair_n[k]++
                    dist_sum[k] += best
                    if (!(k in dist_min) || best < dist_min[k]) dist_min[k] = best
                    if (best <= 4.0)  within4[k]++
                    if (best <= 10.0) within10[k]++
                }
            }
            fn = 0
        }
        {
            frame = $1; t = $2; team = $3; slot = $4; cls = $5; x = $6; z = $7
            key = team SUBSEP slot SUBSEP cls

            if (frame != cur_frame) { flush_frame(); cur_frame = frame }
            fn++
            fkey[fn] = key; fteam[fn] = team; fx[fn] = x; fz[fn] = z

            samples[key]++
            # Post-gate path length: distance between consecutive post-gate
            # samples of the same entity.
            if (t + 0.0 >= gate_time + 0.0) {
                if (key in last_x) {
                    dx = x - last_x[key]; dz = z - last_z[key]
                    path[key] += sqrt(dx*dx + dz*dz)
                }
                last_x[key] = x; last_z[key] = z
            }
        }
        END {
            flush_frame()
            for (k in samples) {
                split(k, parts, SUBSEP)
                n = pair_n[k] + 0
                avg = (n > 0) ? dist_sum[k] / n : ""
                mind = (k in dist_min) ? dist_min[k] : ""
                p4  = (n > 0) ? 100.0 * (within4[k]  + 0) / n : ""
                p10 = (n > 0) ? 100.0 * (within10[k] + 0) / n : ""
                printf "%s,%s,%s,%s,%d,%.2f,%s,%s,%s,%s\n",
                    match_name, parts[1], parts[2], parts[3],
                    samples[k], path[k] + 0,
                    (avg  == "" ? "" : sprintf("%.2f", avg)),
                    (mind == "" ? "" : sprintf("%.2f", mind)),
                    (p4   == "" ? "" : sprintf("%.1f", p4)),
                    (p10  == "" ? "" : sprintf("%.1f", p10))
            }
        }
    ' | sort -t, -k2,2n -k3,3n
done
