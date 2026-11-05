#!/usr/bin/env bash
# The tests a pull request runs: those of what it changes, not the whole
# suite, which runs on main after each merge (.github/workflows/ci.yml).
#
#   scripts/ci/pr-tests.sh BASE [full]
#
# - a test file changed (crates/C/tests/NAME.rs): that test binary;
# - a crate's sources changed: its unit tests;
# - the pull request's description names more, on a line of its own:
#   `Tests: settings_sound, appearance` (test files' names), from $PR_BODY;
# - a test harness, the lock file, the workspace's Cargo.toml or nextest's
#   configuration changed, or `full` given: everything.
#
# The compositor's tests start the real shell, Settings and `hocus`, which
# a test binary does not build: they are built first.
set -euo pipefail

base=$1
full=${2:-}

changed=$(git diff --name-only "$base" "${HEAD_REV:-HEAD}")
echo "Changed:"
printf '  %s\n' $changed

run() {
    echo "+ cargo nextest run $*"
    cargo nextest run --no-tests=warn "$@"
}

programs() {
    cargo build -p hocus-shell -p hocus-cli -p hocus-settings
}

if [ -n "$full" ] || grep -qE '^(Cargo\.lock|Cargo\.toml|rust-toolchain(\.toml)?|\.config/nextest\.toml)$|^crates/[^/]+/tests/(common|levels|fakes)/' <<< "$changed"; then
    echo "Everything: the workspace, its harness or full was asked."
    programs
    run --workspace
    exit 0
fi

declare -A units=() tests=()

# A test named by its file, in whichever crate has it.
add_test() {
    local name=$1 file
    for file in crates/*/tests/"$name".rs; do
        [ -e "$file" ] || { echo "::error::no test file named $name (Tests: line)"; exit 1; }
        tests["$(cut -d/ -f2 <<< "$file")"]+=" --test $name"
    done
}

while read -r path; do
    case $path in
        crates/*/tests/*/*) ;;
        crates/*/tests/*.rs)
            [ -e "$path" ] && add_test "$(basename "$path" .rs)" ;;
        crates/*/src/* | crates/*/Cargo.toml)
            units["$(cut -d/ -f2 <<< "$path")"]=1 ;;
    esac
done <<< "$changed"

named=$(grep -iE '^[[:space:]]*Tests:' <<< "${PR_BODY:-}" | head -n1 | cut -d: -f2- | tr ',' ' ' || true)
for name in $named; do
    add_test "$name"
done

if [ ${#units[@]} -eq 0 ] && [ ${#tests[@]} -eq 0 ]; then
    echo "No Rust change to test."
    exit 0
fi

[ ${#tests[@]} -gt 0 ] && programs

if [ ${#units[@]} -gt 0 ]; then
    args=()
    for crate in "${!units[@]}"; do args+=(-p "$crate"); done
    run "${args[@]}" --lib --bins
fi

for crate in "${!tests[@]}"; do
    # shellcheck disable=SC2086 # the --test flags, split on purpose
    run -p "$crate" ${tests[$crate]}
done
