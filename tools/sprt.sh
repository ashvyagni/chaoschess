#!/bin/sh
# SPRT gate: the current working-tree build against a base commit, on a real clock.
#
#   tools/sprt.sh <name> <base-commit> [tc] [elo0] [elo1] [max-games]
#
# Both binaries are copied to a private directory first, so a rebuild during the match
# can't change what is being tested. The result lands in matches/sprt-<name>.{json,pgn};
# the JSON records both binaries' SHA-256.
#
# Defaults: tc 2+0.02, H0 elo0=0, H1 elo1=10, alpha = beta = 0.05, at most 4000 games.
set -eu
name="${1:?usage: sprt.sh <name> <base-commit> [tc] [elo0] [elo1] [max-games]}"
base="${2:?base commit required}"
tc="${3:-2+0.02}"
elo0="${4:-0}"
elo1="${5:-10}"
games="${6:-4000}"
root="$(git rev-parse --show-toplevel)"
cd "$root"
cargo build --release --quiet --bin crazy-chess --bin arena
work="${TMPDIR:-/tmp}/chaoschess-sprt/$name"
mkdir -p "$work"
cp target/release/crazy-chess "$work/new"
cp target/release/arena "$work/arena"
base_bin="$(tools/build_engine_at.sh "$base")"
cp "$base_bin" "$work/base"
cores="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"
"$work/arena" \
    --engine "name=$name,cmd=$work/new,opt.Hash=16" \
    --engine "name=base-$base,cmd=$work/base,opt.Hash=16" \
    --tc "$tc" --games "$games" --concurrency "$((cores - 1))" --max-plies 400 \
    --sprt "elo0=$elo0,elo1=$elo1,alpha=0.05,beta=0.05" \
    --event "SPRT $name vs $base" \
    --pgn "matches/sprt-$name.pgn" --json "matches/sprt-$name.json"
