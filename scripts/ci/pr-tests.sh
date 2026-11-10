#!/usr/bin/env bash
# The tests a pull request runs: those of what it changes, not the whole
# suite, which runs on main after each merge (.github/workflows/ci.yml).
#
#   scripts/ci/pr-tests.sh BASE [full]
#   scripts/ci/pr-tests.sh --plan BASE [full]   # only says whether it is everything
#
# - a test file changed (crates/C/tests/NAME.rs): its tests, `NAME::…` in
#   the crate's one test binary (tests/main.rs, #765);
# - a crate's sources changed: its unit tests;
# - the pull request's description names more, on a line of its own:
#   `Tests: settings_sound, appearance` (test files' names or paths), from
#   $PR_BODY;
# - a test harness, the lock file, the workspace's Cargo.toml or nextest's
#   configuration changed, or `full` given: everything.
#
# The compositor's tests start the real shell, Settings, `hocus` and the
# secretary, which a test binary does not build: they are built first
# (`cargo programs`, .cargo/config.toml), and a test fails without them.
set -euo pipefail

plan=
if [ "${1:-}" = --plan ]; then
    plan=1
    shift
fi
base=$1
full=${2:-}

changed=$(git diff --name-only "$base" "${HEAD_REV:-HEAD}")
echo "Changed:"
printf '  %s\n' $changed

run() {
    echo "+ cargo nextest run $*"
    cargo nextest run --locked --profile ci --no-tests=warn "$@"
}

programs() {
    cargo programs --locked
}

if [ -n "$full" ] || grep -qE '^(Cargo\.lock|Cargo\.toml|rust-toolchain(\.toml)?|\.config/nextest\.toml)$|^crates/[^/]+/tests/(common|levels|fakes)/' <<< "$changed"; then
    echo "Everything: the workspace, its harness or full was asked."
    # CI then runs it as main does, built once and in four jobs.
    [ -n "$plan" ] && { echo "full=true" >> "${GITHUB_OUTPUT:-/dev/stdout}"; exit 0; }
    programs
    run --workspace
    exit 0
fi

[ -n "$plan" ] && { echo "full=false" >> "${GITHUB_OUTPUT:-/dev/stdout}"; exit 0; }

declare -A units=() tests=()

# A test named by its file, in whichever crate has it.
add_test() {
    local name=$1 file
    for file in crates/*/tests/"$name".rs; do
        [ -e "$file" ] || { echo "::error::no test file named $name (Tests: line)"; exit 1; }
        tests["$(cut -d/ -f2 <<< "$file")"]+="${name}|"
    done
}

while read -r path; do
    case $path in
        crates/*/tests/*/* | crates/*/tests/main.rs) ;;
        crates/*/tests/*.rs)
            [ -e "$path" ] && add_test "$(basename "$path" .rs)" ;;
        crates/*/src/* | crates/*/Cargo.toml)
            units["$(cut -d/ -f2 <<< "$path")"]=1 ;;
    esac
done <<< "$changed"

# What is in parentheses (test names, notes) is left out.
named=$(grep -iE '^[[:space:]]*Tests:' <<< "${PR_BODY:-}" | head -n1 | cut -d: -f2- | sed -E 's/\([^)]*\)//g' | tr ',' ' ' || true)
for name in $named; do
    # A name, or a test file's path: `crates/C/tests/NAME.rs`, `NAME.rs`, in backticks.
    name=${name//\`/}
    name=$(basename "$name" .rs)
    add_test "$name"
done

if [ ${#units[@]} -eq 0 ] && [ ${#tests[@]} -eq 0 ]; then
    echo "No Rust change to test."
    exit 0
fi

[ ${#tests[@]} -gt 0 ] && programs

if [ ${#units[@]} -gt 0 ]; then
    # A crate without a library (the shell) has only its binaries' tests:
    # `--lib` on it is an error.
    with_lib=() bins_only=()
    for crate in "${!units[@]}"; do
        if [ -e "crates/$crate/src/lib.rs" ]; then with_lib+=(-p "$crate"); else bins_only+=(-p "$crate"); fi
    done
    [ ${#with_lib[@]} -gt 0 ] && run "${with_lib[@]}" --lib --bins
    [ ${#bins_only[@]} -gt 0 ] && run "${bins_only[@]}" --bins
fi

for crate in "${!tests[@]}"; do
    # The files' modules: `test(/^(a|b)::/)`.
    run -p "$crate" --test main -E "test(/^(${tests[$crate]%|})::/)"
done
