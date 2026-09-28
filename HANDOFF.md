# Handoff: read this first

This is the starting point for any agent or person taking over this repository. It covers:
- what the project is;
- the rules the work follows;
- what has been built and measured;
- where things stand;
- what to do next.

It is kept current as of the commit that last changed it. If `git log` shows commits after
that, check them first (`git log --oneline <that-commit>..HEAD`).

**Last updated:** 2026-09-28, after E24. HEAD at the time: `793524e` + this file.

---

## 1. What this project is

**Crazy Chess** (GitHub: `ashvyagni/chaoschess`) has these parts:
- a UCI chess engine in Rust, the library crate `crazy_chess` with the binary `crazy-chess`;
- a PySide6 GUI (`gui.py`, `main.py`, `uci_bridge.py`);
- a legacy Python move generator and neural-net code (`board.py`, `moves.py`, `mcts.py`, `neural_net.py`, `train.py`).

The engine has two **styles**:
- **Classical**, the default;
- **Chaos**, a deterministic alternative personality that rewards mobility, checks and centre control. It is not random.

The work is driven by a long "MASTER DIRECTIVE — GRANDMASTER SUPER ENGINE" from the owner. Its binding points:
- **Audit first, then evolve by roadmap.** The audit is `MASTER_ENGINE_AUDIT.md`, including its Errata section. The roadmap is its §P, tracked live in `docs/ROADMAP.md`.
- **Every change is measured.** It must pass unit tests, tactics tests and a benchmark, then be SPRT-gated in engine-vs-engine matches.
- **Failed experiments stay documented.** Never delete a rejected experiment.
- **Never fabricate Elo or claim grandmaster strength.** The engine is **unrated**, since nothing is anchored to a public rating list. All Elo numbers here are *relative* to earlier builds of this engine, at 2+0.02 s.
- **Distinguish implemented from planned** in every doc.
- **Line count is not a goal.**

## 2. Owner's working preferences (important)

- **Commit and push often, as `ashvyagni`,** to `https://github.com/ashvyagni/chaoschess.git`, branch `main`.
  - The git identity is already configured, and HTTPS push works.
  - `gh` is not installed. The SSH key is not authorised; use HTTPS.
  - Push right after each commit. If a push fails with "remote hung up", retrying works.
- The owner sometimes commits WIP themselves with short messages (e.g. "engine upgrade").
  - Always check `git status` / `git log` before committing.
  - **Never rewrite published history.** If a WIP snapshot swallowed your work, explain it in `CHANGELOG.md`.
- Commit messages end with a `Co-Authored-By:` line for the AI model used.

## 3. Repository map

| path | what |
|---|---|
| `src/lib.rs` | Wiring and re-exports; `parse_move`, `perft`, `mate_in_moves`; ~40 unit tests (exactness, colour symmetry, tapering, …) |
| `src/search.rs` | `SearchLimits`, `Searcher`, `Engine`, iterative deepening, `negamax`, quiescence, `search_root`, all search constants |
| `src/eval.rs` | `Evaluator` trait, `StyleEvaluator`, `evaluate_with_style`, PSTs, `tapered_piece_square`, pawn structure, passed pawns, king safety, Chaos terms |
| `src/see.rs`, `src/tt.rs` | Static exchange evaluation; transposition table (key verify, mate-score conversion, generations) |
| `src/position.rs`, `src/mate.rs`, `src/fen.rs` | Position with history and draw clocks; forced-mate prover; strict FEN validator |
| `src/uci.rs`, `src/time.rs` | Threaded UCI loop (stdin + worker); clock budgets |
| `src/arena.rs`, `src/stats.rs`, `src/notation.rs`, `src/suites.rs` | Match runner, SPRT/Elo statistics, SAN/PGN, test suites |
| `src/bin/arena.rs` | CLI for matches and SPRT (records the decision at the bound crossing) |
| `src/bin/{tactics,classify_mates,legal_moves,debug_pos}.rs` | Research tools |
| `tests/` | `tactics.rs`, `uci.rs`, `fuzz.rs`, `arena.rs`, `arena_cli.rs` |
| `tests_py/` | Python move-generator tests and an offscreen GUI smoke test |
| `tools/sprt.sh` | **The gate:** SPRT of the working tree vs a base commit |
| `tools/build_engine_at.sh` | Builds the engine at any commit, cached; prints the binary path |
| `tools/audit_baseline.py`, `tools/uci_conformance.py`, `tools/diff_movegen.py`, `tools/perft_python.py`, `tools/tournament.py` | Benchmarks and oracles |
| `experiments/E*.md` | One file per experiment: hypothesis, baseline, implementation, result, conclusion. Format rules in `experiments/README.md` |
| `matches/` | Every match's JSON (with both binaries' SHA-256) and gzipped PGN |
| `benchmarks/` | Fixed-depth benchmark JSONs |
| `openings/standard40.txt` | The 40-opening book used by every match (paired: each opening played with both colours) |
| `docs/ROADMAP.md` | **Live status of every roadmap item and search technique** |
| `docs/TOURNAMENTS.md` | Every match result in one table, plus SPRT methodology |
| `docs/ARCHITECTURE.md`, `docs/BENCHMARKS.md` | System map; benchmark history |
| `CHANGELOG.md` | Milestones, including descriptions of the owner's WIP commits |
| `MASTER_ENGINE_AUDIT.md` | The original audit, baseline measurements and the prioritised roadmap (§P) |

The engine depends on the `chess` 3.2 crate for move generation, Zobrist hashing and FEN.

## 4. How to build, test and measure

```bash
cargo build --release
cargo test --release            # 86 Rust tests; all must pass
python3 -m unittest discover -s tests_py
```

Quick fixed-depth benchmark (node counts, score, best move):

```bash
printf "position startpos\nbench 10\nquit\n" | target/release/crazy-chess
```

Other test positions: Kiwipete `r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1`
and CPW position 3 `8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1`. Current depth-10 counts
(HEAD `793524e`): startpos 126,952 / Kiwipete 726,621 / pos3 89,368.

**The SPRT gate**, for the working tree vs a base commit:

```bash
tools/sprt.sh <name> <base-commit> [tc=2+0.02] [elo0=0] [elo1=10] [max-games=4000]
```

- It writes `matches/sprt-<name>.json` and `.pgn.gz`.
- It takes 10–70 minutes on this laptop (8 cores, 8 GB, no GPU), using 7 concurrent games.
- For refactors and speed-only changes, use a non-regression test: `elo0=-10 elo1=0`.

## 5. Rules learned the hard way

1. **Run nothing CPU-heavy while an SPRT runs.** No builds, no test runs. It causes time forfeits and skews results.
   - Commit and edit docs only.
   - E24 still saw 13% time losses late in the run from machine load. They were symmetric, so the result was not biased, but watch for it.
2. **One variable per experiment.**
   - E21 changed three piece tables at once and lost.
   - E22, the same change without the pawn table, gained.
3. **Large SPRT gains are inflated by early stopping.** Re-measure with a fixed-length run of 400 games before quoting a number. E15's SPRT said +146; the confirmation said +116.
4. **Rejected experiments follow this commit pattern** (see E19, E21, E23, E24):
   1. commit the attempt, titled `...(E<n>; rejected, reverted in the next commit)`;
   2. `git revert --no-edit HEAD`. Don't pass `-q`: it's not a valid flag for `git revert`;
   3. commit the `experiments/E<n>-*.md` record plus the match JSON/PGN and the doc rows in `ROADMAP.md` and `TOURNAMENTS.md`.
5. **Accepted experiments:** commit the code, the experiment record, the match files and the doc rows together.
6. **Pure speed or refactor changes must give identical trees:** the same node counts, scores and best moves on the bench positions.
7. **`SearchLimits::selective = false` turns off the lossy techniques** (RFP, null move, futility, LMR, check extension). The exactness tests use it. Keep new pruning behind that switch.
8. **UCI probing pitfall:** piping `quit` right after `go` stops the search at depth 1. Use `bench <depth>`, or wait for `bestmove`.
9. **The GUI and Qt on this machine:** `~/Documents` is iCloud-synced, and it sets the hidden flag on `.venv`, so PySide6 can't load its plugins. The GUI shows a diagnostic, and the smoke test copies the plugins.
10. **Double-check your own test positions** with the legal-move oracle. Several hand-written "mate" and castling FENs were wrong.

## 6. What the engine has now

**Search** (`src/search.rs`):
- iterative deepening with aspiration windows;
- root and interior PVS;
- TT with move ordering: TT move > good captures (SEE) > killers > history > bad captures;
- null-move pruning, R = 3 + depth/6;
- reverse futility pruning (depth ≤ 6, 90 cp per ply);
- futility pruning (depth ≤ 2, 150 cp per ply);
- LMR (depth ≥ 3, move index ≥ 3);
- check extension, capped at 2× the iteration depth;
- quiescence with bounded checks and SEE pruning;
- repetition, fifty-move and insufficient-material draws.

Lazy SMP is not implemented. The `Threads` option is accepted, but search is single-threaded.

**Evaluation** (`src/eval.rs`):
- material and PSTs;
- **tapered** by game phase (`MAX_PHASE = 24`): king (E15), rook and queen (E22);
- pawns, knights and bishops use one table (a tapered pawn table double-counted, E21);
- doubled and isolated pawns;
- passed pawns judged by the pawns ahead of them, with a bonus doubling in the endgame (E14, E16);
- bishop pair;
- king safety from enemy attack maps (E13);
- for Chaos: mobility × 3 + checks × 8 + centre × 2, converted to White's frame (E17 fixed a sign bug here).

`Engine::set_evaluator` lets a different `Evaluator` (e.g. a future NNUE) plug in.

## 7. Experiment results (all at 2+0.02, relative Elo, 95% intervals)

| # | change | result | verdict |
|---|---|---|---|
| E1 | quiescence fix | depth-4 startpos 182 s → 0.02 s | kept |
| E2 | TT, root PVS, persistent searcher | — | kept |
| E3 | root-split threads | harmful | removed |
| E4 | current vs audited baseline | 140–0 | — |
| E5 | interior PVS | −5.8 ± 13.0 | inconclusive, kept as LMR infrastructure |
| E6 | null move | +25.5 ± 18.2 | accepted |
| E7 | LMR, first attempt | −6.2 ± 16.3 | rejected |
| E8 | move ordering | +76.7 ± 30.5 | accepted |
| E9 | LMR retry | +30.7 ± 17.4 | accepted |
| E10 | cumulative E5–E9 | +168.4 ± 27.9 | — |
| E11 | reverse futility | +14.8 ± 11.0 | accepted; Chaos finds 2 mates 2 plies later |
| E12 | check extension | +10.1 ± 10.8, then non-regression passed | kept |
| E13 | king-safety fix | +31.3 ± 23.0 | accepted |
| E14 | passed-pawn fix | +34.1 ± 22.3 | accepted |
| E15 | tapered king | +116.2 ± 27.7 | accepted |
| E16 | tapered passed pawns | +18.3 ± 13.1 | accepted |
| E17 | Chaos sign fix + 1.43× speedup | Chaos vs Classical −350 → **−35 ± 30** | kept |
| E18 | futility pruning | +73.2 ± 28.5 | accepted |
| E19 | late move pruning | +3.5 ± 12.3 | rejected, reverted |
| E20 | cumulative E11–E18 | **+304.8 ± 41.4** | — |
| E21 | tapered pawn/rook/queen PSTs | −9.4 ± 13.6 | rejected, reverted |
| E22 | tapered rook/queen PSTs | +13.9 ± 10.3 | **accepted** (current HEAD has it) |
| E23 | internal iterative reduction | −37.2 ± 22.8 | rejected, reverted |
| E24 | null-move R + eval margin | +1.3 ± 8.7 at the cap | rejected, reverted |

The full records are in `experiments/` and `matches/`, and in the table in `docs/TOURNAMENTS.md`.

## 8. Where we stand (roadmap, `docs/ROADMAP.md`)

| # | item | status |
|---|---|---|
| 0–3, 5 | audit, search correctness, UCI, rules/fuzzing, SPRT tooling | **done** |
| 4 | tactical and benchmark suites | partial: mate suites exist; no non-mate tactics or positional suites |
| 6 | search techniques, each SPRT-gated | in progress: see §6; open items below |
| 7 | module split, `Evaluator` trait, tapered eval | mostly done: pawn, knight and bishop tables are untapered by design (E21) |
| 8 | own move generator with incremental make/unmake | planned; prerequisite for NNUE |
| 9 | Lazy SMP over a shared lock-free TT | planned |
| 10 | Chaos as a parameterised, explainable policy | planned; Chaos is −35 ± 30 vs Classical and ~2.4× slower per node |
| 11 | opening book, Syzygy | planned; Syzygy needs ~150 GB of external data |
| 12 | self-play → NNUE → hybrid eval | planned; compute-bound (no GPU) |
| 13 | GUI off the Qt main thread, live telemetry | partial |
| 14 | profiling and packaging | ongoing |

## 9. Suggested next steps, in priority order

Each is one experiment: implement, test, bench, SPRT, then record and commit whichever way it goes.

1. **Chaos redesign (item 10).** Chaos should be an explainable initiative and king-pressure policy with named parameters, not ad-hoc bonuses.
   - Measure it against Classical, and against the current Chaos, with a fixed-length run of 400 games.
   - Also reduce its per-node cost: `move_features` generates moves for both sides at every eval.
2. **Parameter tuning of existing pruning.** Try RFP margin/depth, futility margin and the LMR formula, one at a time.
   - Alternatively, build a small SPSA harness around `arena`.
3. **Mate distance pruning.** Cheap; expect ≈ 0 Elo. Test it as a non-regression (`elo0=-10 elo1=0`).
4. **IIR and null-move variants** listed as untested in `experiments/E23`/`E24`, only with a clear hypothesis.
5. **Evaluation:** mobility for Classical, rook on open files, and a knight outpost term. Each is its own SPRT.
6. **Item 4:** a non-mate tactics suite, e.g. WAC-style positions verified by the engine at high depth.
7. **Bigger items:**
   - own move generator (item 8), verified with the perft oracle and `tools/diff_movegen.py`;
   - Lazy SMP (item 9);
   - GUI off-thread (item 13).
8. **Periodically re-run a cumulative fixed-length match** against an older anchor (as E10 and E20 did), so the sum of accepted gains is checked directly.

## 10. Honest-claims checklist (before writing any result)

- [ ] Opponent (commit), time control, games, W/L/D and the 95% interval are stated.
- [ ] Early-stopped large gains have a fixed-length confirmation.
- [ ] "Unrated" stays true until a match against engines with a known rating exists.
- [ ] Planned work is labelled *planned*.
