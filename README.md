# Crazy Chess

A UCI chess engine in Rust, with a PySide6 GUI. Its distinctive idea is a **Chaos**
personality: a deterministic alternative decision policy, not a random one. The project is
run as an evidence-first research platform: every change is measured, and every strength
claim comes with its opponent, time control, sample size and confidence interval.

> **Strength, honestly stated.** The engine is unrated: nothing here is anchored to a
> public rating list. What *is* measured: the current engine beat the version this project
> started from **140–0** (Elo ≥ +566 at 95%, Wilson). The original code could not finish
> a one-ply search in a normal middlegame. See `experiments/E4` and `docs/TOURNAMENTS.md`.

## Quick start

```bash
cargo build --release
cargo test                      # Rust: unit, tactics, UCI, fuzz, arena
python3 -m unittest discover -s tests_py   # Python: rules + offscreen GUI smoke test
```

Run the engine in any UCI GUI, or by hand:

```text
$ target/release/crazy-chess
uci
setoption name Style value Chaos
position startpos moves e2e4 e7e5
go wtime 60000 btime 60000 winc 1000 binc 1000
```

Options: `Style` (Classical / Chaos), `Hash` (MB), `Threads` (accepted; search is
single-threaded until Lazy SMP), `Move Overhead`, `Clear Hash`. Non-standard helpers:
`bench <depth>`, `perft <depth>`, `d`.

Play a match between two engine builds:

```bash
target/release/arena --engine name=a,cmd=target/release/crazy-chess \
                     --engine name=b,cmd=target/release/crazy-chess,opt.Style=Chaos \
                     --tc 2+0.02 --games 200
```

Launch the GUI:

```bash
python3 main.py
```

If the project lives in an iCloud-synced folder, the GUI will explain why Qt can't load
its plugins from `.venv`: iCloud hides dot-folders. The fix is a virtual environment
outside iCloud.

## What works (tested)

- **Search:** iterative deepening, PVS (root and interior), aspiration windows, a
  transposition table (key-verified, mate-score-correct, generation-aged), guarded
  null-move pruning, quiescence with SEE pruning and bounded checks, history heuristic,
  and threefold / fifty-move / insufficient-material detection.
- **UCI:** clock time management, `stop` and `isready` mid-search (search on a worker
  thread), PV and `score mate` reporting, strict FEN validation.
- **Measurement:** `arena` match runner (paired openings, parallel, SPRT, PGN, JSON with
  binary SHA-256), Elo with pentanomial, trinomial and Wilson intervals, a
  machine-verified mate suite, fixed-depth benchmarks, and a UCI conformance probe.
- **Correctness:** perft against published counts, a differential move-generator oracle,
  and deterministic fuzzing of the search and the UCI input.

## Documentation

| | |
|---|---|
| `MASTER_ENGINE_AUDIT.md` | the audit this work started from, with errata |
| `CHANGELOG.md` | what each milestone changed, with measurements |
| `docs/ARCHITECTURE.md` | modules, design decisions, structural debt |
| `docs/TOURNAMENTS.md` | how strength is measured and what may be claimed |
| `docs/BENCHMARKS.md` | non-game measurements and their history |
| `docs/ROADMAP.md` | prioritised plan with live status |
| `experiments/` | one file per experiment: hypothesis, method, result, conclusion (failures kept) |

## Not implemented yet

Neural evaluation, self-play training, opening book, tablebases, multi-threaded search,
the personality framework beyond two styles, research telemetry. See
`docs/ROADMAP.md`, including what needs resources this repository doesn't have.

The Python files `mcts.py`, `neural_net.py` and `train.py` are an unconnected
NumPy/MCTS prototype, kept as research code.
