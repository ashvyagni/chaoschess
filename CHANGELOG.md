# Changelog

One entry per milestone, newest first. Numbers are measured and reproducible with the
tools named. The per-experiment detail lives in `experiments/`, the baseline in
`MASTER_ENGINE_AUDIT.md`.

Some milestones landed in commits made by the project owner with short messages. Their
detailed descriptions are kept here, so the history stays explainable without rewriting
published commits.

## Roadmap item 3 — correctness floor (`7ac85ed` … `cf4ab83`)

**GUI rules (`7ac85ed`).** `moves.py`, which decides what a human may play in the GUI,
had **seven** bugs. The audit found four; re-reading found two more, and the new
differential tester found the seventh:

1. the legality filter tested the wrong king;
2. castling masks were swapped between colours;
3. en passant left the captured pawn on the board;
4. knight promotion made a pawn;
5. knights had only 4 of their 8 jumps;
6. Black captures used the wrong source square;
7. pawns could double-push from any rank.

`tools/diff_movegen.py` compares Python's legal moves with a Rust oracle at every node.
All published perft counts now match to depth 4.

**Draw rules (`486d94b`, `77a86ee`).** The engine now sees threefold repetition, the
fifty-move rule and insufficient material through `Position` (board + reversible history +
halfmove clock). Checkmate still takes precedence on the hundredth halfmove. The GUI sends
the full move list and ends games with a named reason. Cost: none measurable.

**Fuzzing (`cf4ab83`).** Deterministic random games and hostile UCI input found:

- undefined behaviour in the `chess` crate (a FEN whose side to move has no king);
- a crate panic on bad king counts;
- silent acceptance of nonsense FENs;
- a crash in `parse_move` on multi-byte input.

`src/fen.rs` now validates every FEN before the crate sees it.

**GUI environment.** iCloud-synced `~/Documents` marks `.venv` contents hidden, and Qt
then cannot load its plugins. The GUI now detects this and explains the fix instead of
aborting with Qt's opaque error.

**Corrections to earlier work.**

- The audit gained an Errata section: an invalid example in §G.1, a FEN mislabelled as
  kiwipete, and the three missed defects.
- Several of my own test expectations were wrong and were caught before commit: two
  castling positions, one "mate" that was not mate, and a fuzz-test synchronisation bug.

Tests: 50 Rust, 20 Python.

## Roadmap item 2 — UCI: clock, stop, PV, mate scores (`930e2b2`)

**Why:** the engine could not play a timed game, so matches, SPRT and Elo were all
impossible.

`tools/uci_conformance.py`: **4/8 → 8/8**.

| check | before | after |
|---|---|---|
| `go wtime/btime` | no reply in 10 s (forfeit) | bestmove in 0.17 s |
| `go infinite` + `stop` | never returned | bestmove in < 1 ms |
| forced mate | `score cp 29999` | `score mate 1` |
| principal variation | never emitted | every iteration |

- **`src/uci.rs`**: the protocol loop is now library code. Search runs on a worker
  thread while stdin keeps being read, so `stop`, `isready` and `quit` work during a
  search. The `Engine` moves into the worker and comes back through the join handle,
  with no locks.
- **`src/time.rs`**: `allocate()` turns the clock into soft/hard budgets. Property tests
  over a 270-point grid check that soft ≤ hard, hard ≤ 60% of usable time, and that more
  time or increment never shrinks the budget.
- **`Engine`** keeps the transposition table across moves; `ucinewgame` and `Clear Hash`
  reset it.
- The stop flag and clock are polled every 1024 nodes. Depth 1 always completes, and
  interrupted iterations are discarded rather than reported as completed.
- Protocol: multi-word option names, 4-field FENs, atomic `position`, `go infinite`
  withholds `bestmove` until `stop`, new `Move Overhead` and `Clear Hash` options, and
  corrected `id author`.
- **`uci_bridge.py`** parses `info` by keyword, not by index. The old parser would have
  crashed the GUI on the new format. The GUI's depth-4 search in an Italian Game
  position now takes 0.09 s; on the baseline it did not finish in 600 s.
- Tests: 36 total (25 unit, 3 tactics, 8 UCI integration driving the real binary with a
  timeout on every read).

## Threads: root-split parallelism removed (`440e626`)

It was measured to be strictly harmful: 51× the nodes, no speedup, and memory scaling of
~19 MB per MB of Hash. See `experiments/E3`. `Threads` is still accepted; Lazy SMP is
roadmap item 9.

## Roadmap item 1 — search correctness (`022559d`, `01be376`, `ae170b2`)

- Quiescence checks are bounded to 2 plies, and losing captures are pruned with SEE
  (E1). Start position depth 4: **95.2 M nodes / 182.5 s → 8.3 K nodes / 0.018 s**. Kiwipete
  depth 1 went from not finishing in 600 s to 0.008 s.
- The TT verifies keys, stores mate scores node-relative, and ages entries by
  generation. Root PVS gives the first move a full window. One searcher persists across
  iterative-deepening depths (E2). At depth 7 this means a further 1.7–3.4× fewer nodes.
- Tactical suite: 22 mates proven by brute force. The baseline's unbounded quiescence
  solved 19/22 under a 10 s cap; the current engine solves 22/22 in 0.05 s.

## Audit (`c91f28c`)

`MASTER_ENGINE_AUDIT.md`, plus the three tools that produce its numbers.
