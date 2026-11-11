#!/usr/bin/env bash
# Fails when CLAUDE.md or docs/ name a crate (`crates/hocus-…`, `-p
# hocus-…`) or a `hocus msg` subcommand that does not exist (#779): the
# names the docs drifted on most as the code moved.
set -euo pipefail
cd "$(dirname "$0")/../.."

docs=(CLAUDE.md $(git ls-files 'docs/*.md'))
failed=0

for crate in $(grep -ohE '(crates/|-p )hocus-[a-z0-9-]+' "${docs[@]}" |
    sed -E 's/^(crates\/|-p )//' | sort -u); do
    if [ ! -f "crates/$crate/Cargo.toml" ]; then
        echo "no crate $crate:" >&2
        grep -nE "(crates/|-p )$crate([^a-z0-9-]|$)" "${docs[@]}" >&2
        failed=1
    fi
done

# The subcommands are MsgCommand's variants, kebab-cased as clap names them.
known=$(awk '/^enum MsgCommand/ { on = 1; next } on && /^}/ { exit }
    on && /^    [A-Z][A-Za-z0-9]*[ ,{(]/ { print $1 }' crates/hocus-cli/src/main.rs |
    tr -d ',{(' | sed -E 's/([a-z0-9])([A-Z])/\1-\2/g' | tr '[:upper:]' '[:lower:]')
for sub in $(grep -ohE 'hocus( --socket [^ `]+)? msg [a-z][a-z0-9-]*' "${docs[@]}" |
    awk '{ print $NF }' | sort -u); do
    if ! grep -qx -- "$sub" <<<"$known"; then
        echo "no hocus msg $sub (known: $(echo $known)):" >&2
        grep -nE "msg $sub([^a-z0-9-]|$)" "${docs[@]}" >&2
        failed=1
    fi
done

exit $failed
