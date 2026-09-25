# Changelog

One entry per milestone, newest first. Numbers are measured and reproducible with the
tools named. The per-experiment detail lives in `experiments/`, the baseline in
`MASTER_ENGINE_AUDIT.md`.

Some milestones landed in commits made by the project owner with short messages. Their
detailed descriptions are kept here, so the history stays explainable without rewriting
published commits.

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
