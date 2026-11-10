#!/usr/bin/env bash
# Builds the shell (the bar) or the secretary alone and reloads it in the
# running Hocuspanda session, without a package nor sudo: the windows stay,
# the bar's space stays kept (docs/workflow.md, "Development loop").
#
#   scripts/dev/live.sh shell|secretary [--no-build]
#   scripts/dev/live.sh shell|secretary --installed
#
# The program built runs for this session only: the compositor holds its
# path and writes nothing to the configuration, so the next login runs the
# installed one, and so does `--installed` at once. A build that does not
# start or keeps ending gives way to the installed program by itself, and
# a notification says so.
#
# The build is cargo's dev profile in a target directory of its own
# ($HOCUS_LIVE_TARGET, default ~/.cache/hocuspanda-dev/target), incremental,
# without sccache's wrapper (it has nothing to cache of an incremental
# build), linked as .cargo/config.toml says (clang and mold). The first
# build there compiles every dependency; the next ones only what changed.
#
# This touches the live session: only when the maintainer asks to see a
# change live (CLAUDE.md, "Hard rules"). A compositor change is not seen
# this way: it needs the package and a new login.

set -euo pipefail

usage() {
    sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
    printf 'live.sh: %s\n' "$*" >&2
    exit 1
}

what=
build=yes
installed=no
for arg in "$@"; do
    case $arg in
        shell | secretary) what=$arg ;;
        --no-build) build=no ;;
        --installed) installed=yes ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            exit 2
            ;;
    esac
done
if [ -z "$what" ]; then
    usage >&2
    exit 2
fi

case $what in
    shell)
        package=hocus-shell
        program=hocuspanda-shell
        status=(shell status)
        back='the bar drawn again'
        ;;
    secretary)
        package=hocus-secretary
        program=hocus-secretary
        status=(secretary process)
        back='her process running'
        ;;
esac

root=$(cd "$(dirname "$0")/../.." && pwd)
target=${HOCUS_LIVE_TARGET:-$HOME/.cache/hocuspanda-dev/target}
built=$target/debug/$program

# The session's control socket, as `hocus msg` finds it: $HOCUS_SOCKET,
# else beside $WAYLAND_DISPLAY's socket; else (a terminal that is not the
# session's, as over ssh) the only one in $XDG_RUNTIME_DIR.
find_socket() {
    local runtime=${XDG_RUNTIME_DIR:-} display=${WAYLAND_DISPLAY:-} found socket
    local sockets=()
    if [ -n "${HOCUS_SOCKET:-}" ]; then
        printf '%s\n' "$HOCUS_SOCKET"
        return
    fi
    if [ -n "$display" ]; then
        case $display in
            /*) found=$display.control ;;
            *) found=$runtime/$display.control ;;
        esac
        if [ -S "$found" ]; then
            printf '%s\n' "$found"
            return
        fi
    fi
    [ -n "$runtime" ] || return 1
    for socket in "$runtime"/hocus-*.control; do
        [ -S "$socket" ] && sockets+=("$socket")
    done
    case ${#sockets[@]} in
        1) printf '%s\n' "${sockets[0]}" ;;
        0) return 1 ;;
        *)
            printf 'live.sh: several desktops in %s: name one with HOCUS_SOCKET\n' "$runtime" >&2
            return 1
            ;;
    esac
}

command -v hocus >/dev/null || fail 'no hocus command in PATH: is Hocuspanda installed?'
socket=$(find_socket) || fail 'no Hocuspanda session found: this reloads a program in a running session.'
[ -S "$socket" ] || fail "no Hocuspanda session found: $socket is not a socket."

ask() {
    hocus --socket "$socket" msg "$@"
}

# What the status says after a word.
said() {
    sed -n "s/^$1 //p" <<<"$2"
}

# Now, in microseconds; a time between two of them, in seconds.
now() {
    printf '%s\n' "${EPOCHREALTIME//[!0-9]/}"
}
seconds() {
    local us=$(($2 - $1))
    printf '%d.%d s' $((us / 1000000)) $((us % 1000000 / 100000))
}

before=$(ask "${status[@]}") ||
    fail "the session at $socket does not answer \`hocus msg ${status[*]}\`: its compositor is older than this, or gone. Install a package that has it and log in again (packaging/README.md)."
case $(said state "$before") in
    off)
        case $what in
            shell) fail 'this session runs no shell.' ;;
            secretary) fail 'the secretary is off: [secretary] mode is not "assistant" (Settings, Secretary).' ;;
        esac
        ;;
esac

if [ "$installed" = yes ]; then
    ask "$what" restart --installed
    exit
fi

build_time=
if [ "$build" = yes ]; then
    started=$(now)
    (
        cd "$root"
        unset RUSTC_WRAPPER CARGO_BUILD_RUSTC_WRAPPER
        CARGO_INCREMENTAL=1 CARGO_TARGET_DIR=$target cargo build -p "$package"
    )
    build_time=$(seconds "$started" "$(now)")
fi
[ -x "$built" ] || fail "$built is not built: run without --no-build."

starts=$(said starts "$before")
asked=$(now)
ask "$what" restart --program "$built" >/dev/null

# Until the status says the program built runs: a run started since, and
# for the shell every bar drawn.
deadline=$((asked + 30000000))
while :; do
    now_status=$(ask "${status[@]}") || fail 'the session no longer answers.'
    fell_back=$(said fell-back "$now_status")
    if [ -n "$fell_back" ]; then
        fail "the build was let go of, the installed $what runs again: $fell_back. The session's log says why: journalctl --user -t hocuspanda -b"
    fi
    if [ "$(said state "$now_status")" = running ] &&
        [ "$(said program "$now_status")" = "$built" ] &&
        [ "$(said starts "$now_status")" -gt "$starts" ]; then
        bars=$(said bars "$now_status")
        # "N of M": the bars drawn of those the screens have.
        if [ -z "$bars" ] || [ "${bars%% of *}" = "${bars##* of }" ]; then
            break
        fi
    fi
    [ "$(now)" -lt "$deadline" ] || fail "the $what did not come back in 30 s: $(tr '\n' ' ' <<<"$now_status")"
    sleep 0.05
done
reload_time=$(seconds "$asked" "$(now)")

printf '%s reloaded from %s\n' "$what" "$built"
[ -z "$build_time" ] || printf '  build   %s\n' "$build_time"
printf '  reload  %s  (restart asked, to %s)\n' "$reload_time" "$back"
printf 'The installed one again: scripts/dev/live.sh %s --installed (or the next login).\n' "$what"
