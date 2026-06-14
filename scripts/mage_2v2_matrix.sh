#!/bin/bash
#
# Mage 2v2 Matrix Sweep
#
# Runs N matches per matchup for Mage+Priest vs each-class+Priest (6 matchups)
# and aggregates winrates into a CSV byte-compatible with the 1v1 matrix output
# from src/headless/matrix.rs:217.
#
# Usage:
#   ./scripts/mage_2v2_matrix.sh [N] [--seed-base SEED] [--out OUT_CSV] [--swap-sides]
#
# Arguments:
#   N             Number of seeds per matchup (default: 100)
#   --seed-base   Base RNG seed; each match gets seed = base + run_idx (default: 0)
#   --out         Output CSV path (default: match_logs/mage_2v2_<timestamp>.csv)
#   --swap-sides  Run the transposed ordering (<class>+Priest as team1, Hunter+Priest
#                 as team2). Pair a normal and a --swap-sides run at the same seeds to
#                 compute side-symmetrized cells (see docs/solutions/implementation-
#                 patterns/mirror-asymmetry-side-symmetrized-measurement.md).
#
# Output:
#   CSV columns: team1,team2,runs,team1_wins,team2_wins,draws,team1_winrate,draw_rate,avg_duration_secs
#
# Mirrors scripts/run_combat_tests.sh patterns (output_path per config, log-file grep).

set -e

N=100
SEED_BASE=0
OUT_CSV=""
SWAP_SIDES=0

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --seed-base)
            SEED_BASE="$2"
            shift 2
            ;;
        --out)
            OUT_CSV="$2"
            shift 2
            ;;
        --swap-sides)
            SWAP_SIDES=1
            shift
            ;;
        --help|-h)
            cat <<'USAGE'
Mage 2v2 Matrix Sweep

Runs N matches per matchup for Mage+Priest vs each-class+Priest (6 matchups)
and aggregates winrates into a CSV byte-compatible with the 1v1 matrix output.

Usage:
  ./scripts/mage_2v2_matrix.sh [N] [--seed-base SEED] [--out OUT_CSV] [--swap-sides]

Arguments:
  N             Number of seeds per matchup (default: 100)
  --seed-base   Base RNG seed (default: 0)
  --out         Output CSV path (default: match_logs/mage_2v2_<timestamp>.csv)
  --swap-sides  Run the transposed ordering (<class>+Priest as team1) for
                side-symmetrized measurement

CSV columns: team1,team2,runs,team1_wins,team2_wins,draws,team1_winrate,draw_rate,avg_duration_secs
USAGE
            exit 0
            ;;
        *)
            # First positional arg is N
            if [[ "$1" =~ ^[0-9]+$ ]]; then
                N="$1"
                shift
            else
                echo "Unknown argument: $1" >&2
                exit 1
            fi
            ;;
    esac
done

# Default output path with timestamp
TIMESTAMP=$(date +%s)
if [[ -z "$OUT_CSV" ]]; then
    mkdir -p match_logs
    OUT_CSV="match_logs/mage_2v2_${TIMESTAMP}.csv"
fi

# Verify release binary exists
BINARY_PATH="target/release/arenasim"
if [[ ! -x "$BINARY_PATH" ]]; then
    echo "Building release binary..."
    cargo build --release >/dev/null 2>&1
fi

# Temp dir for configs and logs
TEMP_DIR=$(mktemp -d -t mage_2v2_XXXXXX)
trap 'rm -rf "$TEMP_DIR"' EXIT

# Mage+Priest vs each opposing class + Priest (mirror healer partner)
HEALER="Priest"
OPPONENTS=("Warrior" "Rogue" "Priest" "Warlock" "Paladin" "Hunter")

# Write CSV header
echo "team1,team2,runs,team1_wins,team2_wins,draws,team1_winrate,draw_rate,avg_duration_secs" > "$OUT_CSV"

if [[ "$SWAP_SIDES" -eq 1 ]]; then
    echo "Running <class>+${HEALER} vs Mage+${HEALER} 2v2 sweep (swapped sides): N=${N} per matchup, seed_base=${SEED_BASE}"
else
    echo "Running Mage+${HEALER} vs <class>+${HEALER} 2v2 sweep: N=${N} per matchup, seed_base=${SEED_BASE}"
fi
echo "Output: $OUT_CSV"
echo ""

for opp in "${OPPONENTS[@]}"; do
    T1_WINS=0
    T2_WINS=0
    DRAWS=0
    TOTAL_DURATION="0.0"

    SIDE_A="Mage"; SIDE_B="${opp}"
    if [[ "$SWAP_SIDES" -eq 1 ]]; then SIDE_A="${opp}"; SIDE_B="Mage"; fi
    matchup_label="${SIDE_A}+${HEALER}_vs_${SIDE_B}+${HEALER}"
    echo -n "  ${matchup_label}: "

    for run_idx in $(seq 0 $((N - 1))); do
        SEED=$((SEED_BASE + run_idx))
        LOG_PATH="$TEMP_DIR/${matchup_label}_seed${SEED}.txt"
        CFG_PATH="$TEMP_DIR/${matchup_label}_seed${SEED}.json"

        cat > "$CFG_PATH" <<EOF
{
  "team1": ["${SIDE_A}", "${HEALER}"],
  "team2": ["${SIDE_B}", "${HEALER}"],
  "random_seed": ${SEED},
  "max_duration_secs": 120,
  "output_path": "${LOG_PATH}"
}
EOF

        # Run match; log writes via output_path
        "$BINARY_PATH" --headless "$CFG_PATH" >/dev/null 2>&1 || true

        if [[ -f "$LOG_PATH" ]]; then
            WINNER=$(grep "^Winner:" "$LOG_PATH" | head -1 | sed 's/^Winner: //')
            DURATION=$(grep "^Duration:" "$LOG_PATH" | head -1 | awk '{print $2}' | sed 's/s$//')

            case "$WINNER" in
                "Team 1") T1_WINS=$((T1_WINS + 1)) ;;
                "Team 2") T2_WINS=$((T2_WINS + 1)) ;;
                "DRAW") DRAWS=$((DRAWS + 1)) ;;
                *) DRAWS=$((DRAWS + 1)) ;;  # missing/invalid → count as draw to keep totals consistent
            esac

            if [[ -n "$DURATION" ]]; then
                TOTAL_DURATION=$(awk -v a="$TOTAL_DURATION" -v b="$DURATION" 'BEGIN {printf "%.4f", a + b}')
            fi
        else
            DRAWS=$((DRAWS + 1))
        fi
    done

    # Aggregate this matchup
    T1_WINRATE=$(awk -v w="$T1_WINS" -v n="$N" 'BEGIN {printf "%.4f", w / n}')
    DRAW_RATE=$(awk -v d="$DRAWS" -v n="$N" 'BEGIN {printf "%.4f", d / n}')
    AVG_DURATION=$(awk -v t="$TOTAL_DURATION" -v n="$N" 'BEGIN {printf "%.2f", t / n}')

    # CSV row uses paired-team identifiers in the team1/team2 columns to mirror
    # the 1v1 matrix CSV shape (single value per side). The 2v2 nature is
    # encoded in the pair label.
    echo "${SIDE_A}+${HEALER},${SIDE_B}+${HEALER},${N},${T1_WINS},${T2_WINS},${DRAWS},${T1_WINRATE},${DRAW_RATE},${AVG_DURATION}" >> "$OUT_CSV"

    echo "T1 ${T1_WINS}/${N}, T2 ${T2_WINS}/${N}, draws ${DRAWS} (avg ${AVG_DURATION}s)"
done

echo ""
echo "Done. CSV written to: $OUT_CSV"
