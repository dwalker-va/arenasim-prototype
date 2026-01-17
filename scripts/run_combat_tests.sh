#!/bin/bash
#
# Combat System Regression Test Runner
#
# Runs all test cases from the test suite concurrently and generates a summary report.
#
# Usage:
#   ./scripts/run_combat_tests.sh [options]
#
# Options:
#   -j, --jobs N       Number of concurrent jobs (default: 4)
#   -t, --timeout N    Timeout per test in seconds (default: 60)
#   -s, --suite FILE   Test suite JSON file (default: tests/combat/test_suite.json)
#   -o, --output DIR   Output directory for logs (default: match_logs/regression_<timestamp>)
#   -b, --baseline DIR Compare results against baseline directory
#   -q, --quiet        Only show summary, not individual test progress
#   -h, --help         Show this help message
#

set -e

# Default values
JOBS=4
TIMEOUT=180
SUITE_FILE="tests/combat/test_suite.json"
OUTPUT_DIR=""
BASELINE_DIR=""
QUIET=false

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Portable timeout function for macOS (no coreutils required)
# Returns 124 on timeout (same as GNU timeout)
run_with_timeout() {
    local timeout_secs=$1
    shift

    # Run command in background
    "$@" &
    local pid=$!

    # Start a timer in background
    (
        sleep "$timeout_secs"
        kill -TERM "$pid" 2>/dev/null
    ) &
    local timer_pid=$!

    # Wait for command to complete
    wait "$pid" 2>/dev/null
    local exit_code=$?

    # Kill timer if still running
    kill -TERM "$timer_pid" 2>/dev/null
    wait "$timer_pid" 2>/dev/null

    # If killed by SIGTERM (143), return 124 for timeout
    if [[ $exit_code -eq 143 ]]; then
        return 124
    fi
    return $exit_code
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -j|--jobs)
            JOBS="$2"
            shift 2
            ;;
        -t|--timeout)
            TIMEOUT="$2"
            shift 2
            ;;
        -s|--suite)
            SUITE_FILE="$2"
            shift 2
            ;;
        -o|--output)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        -b|--baseline)
            BASELINE_DIR="$2"
            shift 2
            ;;
        -q|--quiet)
            QUIET=true
            shift
            ;;
        -h|--help)
            head -25 "$0" | tail -20
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Check dependencies
if ! command -v jq &> /dev/null; then
    echo "Error: jq is required but not installed. Install with: brew install jq"
    exit 1
fi

# Check that suite file exists
if [[ ! -f "$SUITE_FILE" ]]; then
    echo "Error: Test suite file not found: $SUITE_FILE"
    exit 1
fi

# Check that binary is built
if [[ ! -f "target/release/arenasim" ]]; then
    echo "Building release binary..."
    cargo build --release
fi

# Get absolute path to binary for parallel jobs
BINARY_PATH="$(pwd)/target/release/arenasim"

# Create output directory (must be absolute for parallel jobs)
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
if [[ -z "$OUTPUT_DIR" ]]; then
    OUTPUT_DIR="$(pwd)/match_logs/regression_${TIMESTAMP}"
else
    # Convert to absolute path if relative
    case "$OUTPUT_DIR" in
        /*) ;;  # already absolute
        *) OUTPUT_DIR="$(pwd)/$OUTPUT_DIR" ;;
    esac
fi
mkdir -p "$OUTPUT_DIR"

# Extract test cases from suite
TEST_CASES=$(jq -r '.test_cases | length' "$SUITE_FILE")
DEFAULT_SEED=$(jq -r '.default_settings.random_seed // 42' "$SUITE_FILE")
DEFAULT_MAX_DURATION=$(jq -r '.default_settings.max_duration_secs // 120' "$SUITE_FILE")

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}Combat System Regression Test Suite${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo "Suite: $SUITE_FILE"
echo "Tests: $TEST_CASES"
echo "Jobs:  $JOBS"
echo "Output: $OUTPUT_DIR"
echo ""

# Create temp directory for test configs and results
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

# Generate individual test configs
echo "Generating test configurations..."
for i in $(seq 0 $((TEST_CASES - 1))); do
    TEST_NAME=$(jq -r ".test_cases[$i].name" "$SUITE_FILE")
    TEAM1=$(jq -c ".test_cases[$i].team1" "$SUITE_FILE")
    TEAM2=$(jq -c ".test_cases[$i].team2" "$SUITE_FILE")
    MAX_DUR=$(jq -r ".test_cases[$i].max_duration_secs // $DEFAULT_MAX_DURATION" "$SUITE_FILE")
    LOG_PATH="$OUTPUT_DIR/${TEST_NAME}.txt"

    # Create config file with output_path to avoid race conditions
    cat > "$TEMP_DIR/${TEST_NAME}.json" <<EOF
{
  "team1": $TEAM1,
  "team2": $TEAM2,
  "random_seed": $DEFAULT_SEED,
  "max_duration_secs": $MAX_DUR,
  "output_path": "$LOG_PATH"
}
EOF
done

# Function to run a single test
run_test() {
    local test_name=$1
    local config_file="$TEMP_DIR/${test_name}.json"
    local log_file="$OUTPUT_DIR/${test_name}.txt"
    local result_file="$TEMP_DIR/${test_name}.result"

    local start_time=$(date +%s.%N)

    # Run with timeout (log is written directly to output_path in config)
    if run_with_timeout "$TIMEOUT" "$BINARY_PATH" --headless "$config_file" > /dev/null 2>&1; then
        local status="PASS"
    else
        local exit_code=$?
        if [[ $exit_code -eq 124 ]]; then
            local status="TIMEOUT"
        else
            local status="FAIL"
        fi
    fi

    local end_time=$(date +%s.%N)
    local duration=$(echo "$end_time - $start_time" | bc)

    # Extract key metrics from log (written directly by simulation)
    if [[ -f "$log_file" ]]; then
        local winner=$(grep "^Winner:" "$log_file" 2>/dev/null | awk '{print $2}' || echo "None")
        local match_duration=$(grep "^Duration:" "$log_file" 2>/dev/null | awk '{print $2}' || echo "0s")
        local deaths=$(grep -c "\[DEATH\]" "$log_file" 2>/dev/null || echo "0")
    else
        local winner="N/A"
        local match_duration="N/A"
        local deaths="N/A"
    fi

    # Write result
    echo "$test_name|$status|$duration|$winner|$match_duration|$deaths" > "$result_file"

    if [[ "$QUIET" != "true" ]]; then
        case $status in
            PASS)
                echo -e "  ${GREEN}PASS${NC} $test_name (${duration}s) - Winner: $winner, Duration: $match_duration"
                ;;
            FAIL)
                echo -e "  ${RED}FAIL${NC} $test_name (${duration}s)"
                ;;
            TIMEOUT)
                echo -e "  ${YELLOW}TIMEOUT${NC} $test_name (exceeded ${TIMEOUT}s)"
                ;;
        esac
    fi
}

export -f run_with_timeout
export -f run_test
export TEMP_DIR OUTPUT_DIR TIMEOUT QUIET RED GREEN YELLOW NC BINARY_PATH

# Run tests with parallel jobs
echo ""
echo "Running tests..."
echo ""

# Get list of test names
TEST_NAMES=$(jq -r '.test_cases[].name' "$SUITE_FILE")

# Use xargs for parallel execution
echo "$TEST_NAMES" | xargs -P "$JOBS" -I {} bash -c 'run_test "{}"'

# Collect results
echo ""
echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}Test Results Summary${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

PASSED=0
FAILED=0
TIMEOUTS=0

# Table header
printf "%-30s %-10s %-10s %-12s %-12s %-8s\n" "Test Name" "Status" "Run Time" "Winner" "Match Time" "Deaths"
printf "%-30s %-10s %-10s %-12s %-12s %-8s\n" "------------------------------" "----------" "----------" "------------" "------------" "--------"

for result_file in "$TEMP_DIR"/*.result; do
    if [[ -f "$result_file" ]]; then
        IFS='|' read -r test_name status duration winner match_duration deaths < "$result_file"

        case $status in
            PASS)
                ((PASSED++))
                status_colored="${GREEN}PASS${NC}"
                ;;
            FAIL)
                ((FAILED++))
                status_colored="${RED}FAIL${NC}"
                ;;
            TIMEOUT)
                ((TIMEOUTS++))
                status_colored="${YELLOW}TIMEOUT${NC}"
                ;;
        esac

        printf "%-30s %-10b %-10s %-12s %-12s %-8s\n" "$test_name" "$status_colored" "${duration}s" "$winner" "$match_duration" "$deaths"
    fi
done

echo ""
echo -e "${BLUE}========================================${NC}"
TOTAL=$((PASSED + FAILED + TIMEOUTS))
echo -e "Total: $TOTAL tests"
echo -e "  ${GREEN}Passed:${NC}   $PASSED"
echo -e "  ${RED}Failed:${NC}   $FAILED"
echo -e "  ${YELLOW}Timeouts:${NC} $TIMEOUTS"
echo ""

# Compare with baseline if provided
if [[ -n "$BASELINE_DIR" && -d "$BASELINE_DIR" ]]; then
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}Baseline Comparison${NC}"
    echo -e "${BLUE}========================================${NC}"
    echo ""

    DIFFS_FOUND=false
    for log_file in "$OUTPUT_DIR"/*.txt; do
        test_name=$(basename "$log_file")
        baseline_file="$BASELINE_DIR/$test_name"

        if [[ -f "$baseline_file" ]]; then
            # Compare winner and duration
            new_winner=$(grep "^Winner:" "$log_file" 2>/dev/null | awk '{print $2}')
            old_winner=$(grep "^Winner:" "$baseline_file" 2>/dev/null | awk '{print $2}')

            if [[ "$new_winner" != "$old_winner" ]]; then
                echo -e "${YELLOW}DIFF${NC} $test_name: Winner changed from $old_winner to $new_winner"
                DIFFS_FOUND=true
            fi
        else
            echo -e "${BLUE}NEW${NC}  $test_name (no baseline)"
        fi
    done

    if [[ "$DIFFS_FOUND" == "false" ]]; then
        echo -e "${GREEN}No differences found from baseline${NC}"
    fi
    echo ""
fi

# Generate summary JSON
SUMMARY_FILE="$OUTPUT_DIR/summary.json"
cat > "$SUMMARY_FILE" <<EOF
{
  "timestamp": "$TIMESTAMP",
  "suite": "$SUITE_FILE",
  "total": $TOTAL,
  "passed": $PASSED,
  "failed": $FAILED,
  "timeouts": $TIMEOUTS,
  "output_dir": "$OUTPUT_DIR"
}
EOF

echo "Logs saved to: $OUTPUT_DIR"
echo "Summary saved to: $SUMMARY_FILE"
echo ""

# Exit with error if any tests failed
if [[ $FAILED -gt 0 || $TIMEOUTS -gt 0 ]]; then
    exit 1
fi

exit 0
