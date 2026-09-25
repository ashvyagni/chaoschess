# Crazy Chess

Crazy Chess is a chess-engine project with a fast, testable Rust core and a
Python prototype retained for experimentation and model training. Its first
distinctive identity is **Chaos style**: a deterministic search personality
that values initiative, mobility, checks, and king pressure instead of only
material.

## Why the stack changed

The original implementation is a useful prototype, but Python is a poor
runtime for the engine's hottest loops: legal move generation, board copying,
tree search, and self-play. The new boundary is:

- **Rust**: authoritative board state, legal moves, style-aware evaluation,
  iterative deepening alpha-beta search, quiescence search, transposition
  tables, perft, and UCI protocol.
- **Python**: research/training playground until a model-backed evaluator is
  stable enough to expose through a Rust FFI or service boundary.
- **PySide6**: existing desktop UI, to be migrated to UCI in a later slice
  instead of coupling UI work to engine correctness.

The dependency on the well-tested `chess` crate is deliberate: chess rules are
an invariant-heavy boundary, so the first migration prioritizes correctness
and known perft counts over duplicating an untested move generator.

## Run the Rust engine

```bash
cargo test
cargo run --release
```

The binary speaks a minimal UCI-compatible protocol:

```text
uci
isready
setoption name Style value Chaos
position startpos moves e2e4 e7e5
go depth 6
quit
```

The engine supports `Classical` and `Chaos` styles, `Hash` sizing, node
budgets, and `movetime` limits through UCI. `Chaos` is not random: identical
positions and limits produce identical moves.

Search emits UCI telemetry such as completed depth, node count, and
centipawn score. The evaluator combines material, piece-square tables, pawn
structure and passed pawns, bishop pair, king shelter/pressure, and
style-specific mobility and initiative terms. Search uses deterministic
iterative deepening with aspiration windows/PVS; `Threads` parallelizes root
moves with stable tie-breaking.

To compare the built-in personalities over a deterministic
game:

```bash
cargo build --release
python3 tools/tournament.py --depth 1 --plies 40
```

For rule validation, the engine also exposes a `perft` command:

```text
position startpos
perft 4
```

Expected result: `nodes 197281`.

The UCI surface also supports `setoption name Threads value N`, `go nodes N`,
`go movetime N`, `go infinite` (bounded by the configured depth), and a
`bench [depth]` command for reproducible local measurements. `stop` is
accepted as a protocol command; searches are bounded by node/time limits and
the current synchronous driver cannot interrupt an already-running `go
infinite` call.

## Existing Python prototype

The Python files (`board.py`, `moves.py`, `mcts.py`, `neural_net.py`, and
`train.py`) remain available as research code. They are not the authoritative
engine yet: they currently lack regression tests, a stable model artifact, and
an interoperability boundary. The next migration slice should make the GUI
launch the Rust UCI process and then replace the NumPy MLP with a real training
framework such as PyTorch.

## Current scope and honest limitations

The Rust core is a reliable, testable engine platform, not a claimed
grandmaster-strength engine. It now uses iterative deepening, quiescence,
bounded transposition tables, piece-square and pawn-structure evaluation,
king-safety terms, deterministic PVS-style ordering, and deterministic root
parallelism. Opening books, NN evaluation, self-play training, and release
packaging remain planned because they require data, compute, and external
rating validation rather than just source code. The UI launches the Rust UCI
engine when the optimized binary is available, with `cargo run --release` as
a development fallback.
