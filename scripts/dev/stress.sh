#!/usr/bin/env bash
# Runs the tests a nextest filter selects many times on a loaded machine,
# and stops at the first failure, printing it: what a test that fails by
# chance on CI's four shared cores does here before it lands
# (docs/workflow.md, "A test that fails by chance").
#
#   scripts/dev/stress.sh [-n RUNS] [-c CPUS] [-l LOOPS] [-j THREADS] FILTER [-- NEXTEST_ARGS...]
#
#   scripts/dev/stress.sh 'test(=actions::an_impossible_action_is_inert_and_nothing_found_searches_the_launcher)'
#   scripts/dev/stress.sh -n 40 'package(hocus-compositor) & test(/^today::/)'
#
# RUNS (default 20): how many times each test runs, one run after the
# other. CPUS (default 0-1): the cores the tests run on, with LOOPS
# (default 2) busy loops pinned on the same cores beside them, so that a
# test and the programs it starts get a part of them only. THREADS
# (default 4, as CI): the tests run at a time.
#
# It builds first, unloaded: the tests' binaries and the programs the
# compositor's tests run (`cargo programs`). The run uses nextest's
# default profile (no retries, CI's quarantine not applied, so a
# quarantined test is run too) and fails fast. The busy loops are its own
# children, stopped by their PIDs when it ends, however it ends.

set -euo pipefail

usage() {
    sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
    printf 'stress.sh: %s\n' "$*" >&2
    exit 2
}

runs=20
cpus=0-1
loops=2
threads=4
while getopts ':n:c:l:j:h' opt; do
    case $opt in
    n) runs=$OPTARG ;;
    c) cpus=$OPTARG ;;
    l) loops=$OPTARG ;;
    j) threads=$OPTARG ;;
    h)
        usage
        exit 0
        ;;
    *)
        usage >&2
        exit 2
        ;;
    esac
done
shift $((OPTIND - 1))
[ $# -ge 1 ] || {
    usage >&2
    exit 2
}
filter=$1
shift
[ "${1:-}" != -- ] || shift
case $runs$loops$threads in
*[!0-9]*) fail "RUNS, LOOPS and THREADS are numbers" ;;
esac
[ "$runs" -ge 1 ] || fail "RUNS is at least 1"
command -v taskset >/dev/null || fail "taskset is missing (util-linux)"
cargo nextest --version >/dev/null 2>&1 || fail "cargo-nextest is missing"

cd "$(dirname "$0")/../.."

echo "stress.sh: building" >&2
cargo programs --quiet
cargo nextest run --workspace --no-run --cargo-quiet -E "$filter" "$@"

pids=()
stop() {
    local pid
    for pid in "${pids[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    for pid in "${pids[@]}"; do
        wait "$pid" 2>/dev/null || true
    done
}
trap stop EXIT
trap 'exit 130' INT TERM

for _ in $(seq "$loops"); do
    taskset -c "$cpus" sh -c 'while :; do :; done' &
    pids+=("$!")
done

echo "stress.sh: $runs runs on CPUs $cpus beside $loops busy loops, $threads at a time: $filter" >&2
status=0
taskset -c "$cpus" cargo nextest run --workspace --cargo-quiet \
    --stress-count "$runs" --test-threads "$threads" \
    --retries 0 --fail-fast --failure-output final --success-output never \
    -E "$filter" "$@" || status=$?
if [ "$status" -eq 0 ]; then
    echo "stress.sh: passed $runs times: $filter" >&2
else
    echo "stress.sh: FAILED (the first failure is above): $filter" >&2
fi
exit "$status"
