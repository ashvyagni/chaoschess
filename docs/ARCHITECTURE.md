# Architecture

What exists in the repository **today**, how the parts fit, and what is still only
planned. Everything described here is backed by code and tests. Planned subsystems are
labelled *planned* and appear only in the last section.

## System map

```
                    ┌──────────────────────────────────────────────────────────┐
                    │  crazy_chess (Rust library, src/)                        │
                    │                                                          │
  UCI stdin/out ───►│  uci.rs ──► Engine ──► search (lib.rs)                   │
                    │   │ threaded: stdin loop + worker thread                 │
                    │   │                    ├─ iterative deepening            │
                    │   ├─ time.rs           ├─ PVS / aspiration / TT / NMP    │
                    │   │  clock budgets     ├─ quiescence + SEE               │
                    │   └─ fen.rs            ├─ draw rules (Position)          │
                    │      strict FEN guard  └─ evaluate_with_style            │
                    │                                                          │
                    │  arena.rs ─ stats.rs ─ notation.rs   (measurement)       │
                    │  suites.rs + prove_forced_mate       (tactical truth)    │
                    │                                                          │
                    │  dependency: chess 3.2 (move generation, Zobrist, FEN)   │
                    └───────────────▲──────────────────────────────────────────┘
                                    │ UCI over pipes
      ┌─────────────────────────────┼──────────────────────────────┐
      │                             │                              │
  src/bin/arena  (matches,     uci_bridge.py ◄── gui.py      tools/*.py, *.sh
  SPRT, PGN, JSON)             (Python, PySide6)             (benchmarks, gates)
                                    │
                               moves.py / board.py
                               (GUI move entry; perft-verified)
```

## Modules

| module | lines | responsibility | key tests |
|---|---:|---|---|
| `src/lib.rs` | ~1,900 | search, evaluation, TT, SEE, draw rules, `Engine`, `Position`, mate prover | 30+ unit tests in-file |
| `src/uci.rs` | ~420 | UCI protocol; the search runs on a worker thread so `stop`/`isready`/`quit` work | `tests/uci.rs` (real binary) |
| `src/time.rs` | ~130 | clock → soft/hard budget; property-tested safety bounds | grid property tests |
| `src/fen.rs` | ~230 | strict FEN validation before the `chess` crate sees input | 25 malformed FENs |
| `src/arena.rs` | ~740 | engine-vs-engine games over UCI; adjudication; PGN; SHA-256 | `tests/arena.rs` (incl. fake broken engines) |
| `src/stats.rs` | ~480 | Elo, trinomial/pentanomial intervals, Wilson, LOS, GSPRT | formula + simulation tests |
| `src/notation.rs` | ~140 | SAN for PGN | rule-by-rule tests |
| `src/suites.rs` | ~50 | machine-verified tactical positions | `tests/tactics.rs` |
| `src/bin/arena.rs` | ~390 | match runner CLI: parallel paired games, SPRT early stop, JSON record | `tests/arena_cli.rs` |
| `src/bin/tactics.rs` | ~155 | tactical solve rates; quiescence parameter sweeps | — |
| `src/bin/legal_moves.rs` | ~25 | legal-move oracle for differential testing | used by `tools/diff_movegen.py` |
| `gui.py` + `uci_bridge.py` | ~660 | PySide6 GUI; sends the full move list to the engine | `tests_py/test_gui_smoke.py` (offscreen) |
| `moves.py` / `board.py` | ~670 | Python rules for GUI move entry and game end | `tests_py/test_moves.py`, perft to depth 4 |
| `mcts.py`, `neural_net.py`, `train.py` | ~470 | legacy NumPy MLP + MCTS prototype | none; research code, not on any engine path |

## Key design decisions

**The Rust engine is authoritative for search; the `chess` crate for move generation.**
The crate is perft-verified by this repository's tests, which is why it was kept rather
than replaced. Its FEN parser is **not** trusted: `src/fen.rs` validates structure first,
because malformed input triggered undefined behaviour, a panic, and silent acceptance of
nonsense (see `CHANGELOG.md`, `cf4ab83`). Replacing the generator with an in-tree one is
roadmap item 8, needed for NNUE-style incremental updates and staged generation.

**UCI protocol state lives in `uci.rs`; search state in `Engine`.** The `Engine` owns the
transposition table and moves *into* the worker thread for each search, coming back
through the join handle, so there are no locks. `stop` sets an `AtomicBool` that the
search polls every 1024 nodes.

**A search always has a position with history.** `Position` = board + reversible-move
hashes + halfmove clock. The bare `chess::Board` can see neither repetition nor the
fifty-move rule. UCI builds a `Position` from `position ... moves ...`, and the GUI sends
the full move list.

**Every descent goes through one function.** `Searcher::search_child` is the only place
the repetition path, the halfmove-clock stack and the null-move stack are pushed and
popped, so the three can't drift apart.

**Measurement is part of the architecture, not an afterthought.** `arena` + `stats`
turn any two binaries into an Elo estimate with an interval. `tools/sprt.sh` gates changes
against a base commit, and `tools/build_engine_at.sh` builds any historical commit.
See `docs/TOURNAMENTS.md`.

## Dependency graph (current)

```
chess (crate) ◄── lib.rs  (search, eval, Position, Engine)
                    ▲
        ┌───────────┼──────────────┬──────────────┐
      fen.rs    notation.rs     arena.rs        uci.rs ──► time.rs
                    ▲              ▲               ▲
                    └──── arena.rs ┘               └── main.rs
                                   ▲
                          src/bin/arena.rs ──► stats.rs
```

`stats.rs` depends on nothing, and `time.rs` depends on nothing. Both are pure and fully
unit-tested for that reason.

## Known structural debt

- **`lib.rs` is a monolith** (~1,900 lines): search, evaluation, TT, SEE and the draw
  rules share one file. Roadmap item 7 splits it (`search/`, `eval/`, `tt.rs`, `see.rs`)
  behind an `Evaluator` trait. That split is also the prerequisite for neural evaluation.
- **Evaluation is untapered** and its king-safety term runs a full move generation per
  call (~17% of search time in the latest profile).
- **Move generation is the crate's.** It has no staged generation, so every node
  generates and sorts all moves.
- **The legacy Python ML prototype is not connected to anything** and trains on its own
  generator. It is kept as research code only.

## Planned subsystems (not implemented)

Neural evaluation (NNUE), self-play data generation, training pipeline, opening book
beyond the match openings, Syzygy tablebases, Lazy SMP, personality framework beyond the
two `Style` values, research telemetry, explanation layer. Order and rationale:
`docs/ROADMAP.md` and `MASTER_ENGINE_AUDIT.md` §P.
