#!/bin/sh
# Build the UCI engine exactly as it was at a given commit, for version-vs-version matches.
#
#   tools/build_engine_at.sh <commit> [output-dir]
#
# Exports the commit with `git archive` (no worktree, no change to this checkout), builds it
# in release mode with its own Cargo.lock, and prints the path of the resulting binary.
set -eu
commit="${1:?usage: build_engine_at.sh <commit> [output-dir]}"
root="$(git rev-parse --show-toplevel)"
full="$(git -C "$root" rev-parse --verify "$commit^{commit}")"
short="$(git -C "$root" rev-parse --short "$full")"
out="${2:-${TMPDIR:-/tmp}/chaoschess-engines/$short}"
if [ ! -x "$out/target/release/crazy-chess" ]; then
    rm -rf "$out"
    mkdir -p "$out"
    git -C "$root" archive "$full" | tar -x -C "$out"
    (cd "$out" && cargo build --release --quiet --bin crazy-chess) >&2
fi
echo "$out/target/release/crazy-chess"
