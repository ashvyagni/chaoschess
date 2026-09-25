# MASTER ENGINE AUDIT

**Audit date:** 2026-09-26
**Auditor:** automated engineering pass (Claude Opus 5)
**Repository state audited:** commit `b091ae7` ("Baseline: import existing chess engine as-is")
**Method:** full file read of every source file, then empirical verification of every
claim by building, testing, benchmarking and probing the running binaries.

> **Ground rule applied throughout.** The repository is the source of truth. Nothing in
> this document is carried over from prior summaries or from `README.md`. Every
> performance number, bug and capability claim below was produced by a command that was
> actually run on this machine on the audit date, and the command is named so it can be
> re-run. Where I expected a defect and measurement disproved it, the measurement wins
> and that is recorded as such (see §G.5).

### Errata (added 2026-09-26, after the fixes in `7ac85ed`)

Three statements in the original audit were wrong. They are corrected in place, marked
**[corrected]**, and kept visible here because a record that silently changes is not
worth much.

1. **§G.1, "discards legal checking moves": the example was invalid.** The audit's
   position was `rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR w`. There the pawn on
   e2 blocks `Qd1-h5`, so `d1h5` is illegal whatever the generator does. The claim itself
   is true: `d1h5` in `rnbqkbnr/ppppp2p/5p2/6p1/4P3/8/PPPP1PPP/RNBQKBNR w` (a mating
   check) was not generated. It is now pinned by
   `tests_py/test_moves.py::LegalityFilter`.
2. **"kiwipete" is not kiwipete.** The FEN this repository called kiwipete
   (`…/2pP4/…/2N2N2/PPPQBPPP/…`) is a variant of the Chess Programming Wiki position
   (`…/3PN3/…/2N2Q1p/PPPBBPPP/…`). The §G.1 perft table compared it against the real
   kiwipete's counts, so its three "kiwipete" rows had wrong expected values. The
   generator was still broken on that position, just by different amounts. All other
   uses of "kiwipete" in this document (the §F.1 timings, the benchmarks) refer to the
   variant FEN. The measurements are unaffected; only the name was wrong.
3. **§G.1 found four defects; there were seven.** Knights generated 4 of 8 jumps, Black
   pawn captures used the wrong source square, and any pawn could double-push from any
   rank. The first two were found by re-reading while fixing; the third by
   `tools/diff_movegen.py`.

All seven are fixed in `7ac85ed`, where the Python generator matches every published
perft count to depth 4.

---

## Measurement environment

| Property | Value |
|---|---|
| Machine | Apple M3, 8 cores (4P+4E), **8 GB** unified memory |
| OS | macOS / Darwin 25.3.0 |
| Rust | rustc 1.97.1, cargo 1.97.1 |
| Build profile | `--release` (`lto = "thin"`, `codegen-units = 1`, `panic = "abort"`) |
| Python | 3.14.6 (in-tree `.venv`), numpy 2.5.1, PySide6 6.11.1 |
| Engine dependency | `chess = "3.2"` crate (third-party move generator) |

The 8 GB memory ceiling is a real constraint and is referenced in several findings
below. It also bounds what is credible for local NNUE training and large-scale
self-play; see §I and §J.

---

## A. Current architecture

The repository is **2,670 lines total** and contains **two disconnected engines** plus a
GUI that depends on both.

```
                          ┌──────────────────────────┐
                          │        gui.py (475)      │
                          │   PySide6 desktop UI     │
                          └────────┬────────┬────────┘
                                   │        │
              game state / legality│        │move selection
                                   ▼        ▼
              ┌────────────────────────┐  ┌──────────────────────┐
              │ LEGACY PYTHON ENGINE   │  │  uci_bridge.py (93)  │
              │ board.py  (278)        │  │  subprocess + UCI    │
              │ moves.py  (298)        │  └──────────┬───────────┘
              │ config.py (45)         │             │ stdin/stdout
              └────────────────────────┘             ▼
                     ▲                    ┌──────────────────────────┐
                     │ imports            │   RUST ENGINE            │
              ┌──────┴───────────┐        │   src/main.rs (141) UCI  │
              │ mcts.py     (128)│        │   src/lib.rs  (620) core │
              │ neural_net.py(199)│       │   dep: chess crate 3.2   │
              │ train.py    (141)│        └──────────┬───────────────┘
              │ main.py      (79)│                   │
              └──────────────────┘        ┌──────────┴───────────────┐
                 NumPy MLP + MCTS         │ tools/tournament.py (59) │
                 (no connection to Rust)  └──────────────────────────┘
```

### The critical structural defect

`gui.py` is the only component that touches both halves, and it wires them together
the wrong way round:

- **Authoritative game state lives in the legacy Python engine.** `gui.py:374` and
  `gui.py:405` advance the position with `apply_move()` from `moves.py`; `gui.py:327`
  computes the legal-move list the user is allowed to play with
  `generate_legal_moves()`; `gui.py:381` decides the game is over with `is_game_over()`.
- **The Rust engine is a stateless move oracle.** `uci_bridge.py:60` sends only
  `position fen <fen>` — never `moves ...`. The Rust engine therefore receives a
  position with **no history**.

So the tested, correct component (Rust, backed by the well-tested `chess` crate) is
subordinate to the untested, **provably incorrect** component (Python). §G.1 shows the
Python legality filter is inverted, which means the GUI's rules enforcement is wrong.

### Ownership of core chess rules

Move generation, legality, `perft`, Zobrist hashing and FEN parsing in the Rust engine
are **all provided by the third-party `chess` crate**, not by this repository. The
project's own Rust code is search + evaluation + UCI only. `README.md` is honest about
this being deliberate, and the choice is defensible — but it means "legal move
generation" and "perft validation" are *dependencies*, not assets of this codebase, and
they are not extensible (no SEE, no staged generation, no incremental accumulator hooks,
no `MoveList` reuse). §F.3 and §P explain why this becomes the binding constraint.

---

## B. Existing capabilities — verified

Each row was confirmed by running the thing, not by reading a claim about it.

| Capability | Status | Evidence |
|---|---|---|
| Rust core builds clean in release | ✅ | `cargo build --release` — no warnings |
| Rust test suite | ✅ 6/6 pass | `cargo test` |
| Perft correctness (Rust) | ✅ | startpos d1–d4 = 20/400/8902/197281; kiwipete d1–d2 = 42/1818 |
| Negamax + alpha-beta | ✅ | `src/lib.rs:370` |
| Quiescence search | ⚠️ present but pathological | `src/lib.rs:334`, see §F.1 |
| Transposition table | ⚠️ present but unsound | `src/lib.rs:279`, see §G.2 |
| Iterative deepening | ✅ | `src/lib.rs:519` |
| Aspiration window | ⚠️ present, ±40 fixed, no widening loop | `src/lib.rs:535` |
| PVS at root only | ⚠️ present but malformed | `src/lib.rs:447`, see §G.3 |
| History heuristic | ⚠️ present, reset every depth | `src/lib.rs:526` |
| MVV-LVA ordering | ✅ correct formula (misleading variable names) | `src/lib.rs:329` |
| Piece-square + pawn + passer + bishop-pair + king-safety eval | ✅ present | `src/lib.rs:99` |
| Classical / Chaos styles, deterministic | ✅ determinism verified | `tests::search_returns_legal_and_deterministic_move` |
| UCI `depth` / `nodes` / `movetime` | ✅ | probe A |
| UCI `Hash` / `Threads` / `Style` options | ⚠️ accepted; `Threads` value ignored | probe E, see §G.4 |
| Root parallel search | ❌ actively harmful | probe E, see §G.4 |
| `perft` / `bench` UCI extensions | ✅ | `src/main.rs:91`,`:98` |
| PySide6 GUI launches and plays | ⚠️ unusable in practice | see §F.2 |
| Python legacy move generator | ❌ **broken** | §G.1 |
| NumPy MLP + MCTS self-play | ⚠️ runs, but on the broken generator | §G.1, §I |
| Tournament harness | ⚠️ toy; cannot produce a result | §L |

### What does *not* exist at all

Nothing in the repository implements any of: null-move pruning, late move
reductions, late move pruning, futility/reverse-futility pruning, razoring, singular
extensions, SEE, killer moves, counter-move or continuation history, capture history,
multi-cut, verification search, mate-distance pruning, repetition detection, fifty-move
detection, insufficient-material detection, contempt, tapered evaluation, game-phase
interpolation, opening book, tablebases, NNUE or any neural evaluation reachable from
search, a personality configuration layer, structured search telemetry, an
explanation/analysis layer, a tactical test suite, SPRT, or Elo estimation.

`README.md` correctly lists opening books, NN evaluation, self-play training and
packaging as "planned". It does **not** disclose the defects in §F and §G.

---

## C. Existing weaknesses

Ranked by impact on playing strength.

1. **The engine cannot complete a one-ply search in a normal middlegame position.**
   Depth 4 from the start position takes **182.5 s / 95.17 M nodes**; kiwipete and a
   standard Italian Game position do not finish **depth 1** in **600 seconds** (§F.1).
   The shipped default is `depth: 6` (`src/lib.rs:40`), which is unreachable in any
   position that is not near-empty.
2. **The engine cannot play a timed game.** `go wtime/btime` is not parsed at all, so
   the engine never replies and forfeits (§G.5).
3. **The engine cannot be stopped.** `go infinite` + `stop` never returns (§G.6).
4. **Parallel search makes the engine strictly worse** — 51× the nodes for the same
   depth and no speedup, plus a 15× memory blowup (§G.4).
5. **The TT can return an entry belonging to a different position** (§G.2).
6. **No draw detection of any kind** — no repetition, no fifty-move, no insufficient
   material (§G.7).
7. **Evaluation is untapered** — one set of piece-square tables for all game phases, so
   the king table that pushes the king to the corner in the opening still applies in the
   endgame, where the king must centralise.
8. **Passed-pawn detection is wrong** — an enemy pawn *behind* the candidate blocks it
   (§G.8).
9. **Chaos style is ~3 full move generations more expensive per evaluated node** than
   Classical (§F.4), so the engine's "distinctive identity" is also its slowest mode.
10. **Mate scores are reported as centipawns** (`score cp 29999`), which is
    non-conformant UCI and displays as "+299.99" in any GUI (§G.9).

---

## D. Technical debt

| Item | Location | Cost |
|---|---|---|
| Two engines, one of them broken, with the broken one authoritative | `gui.py` | Blocks all correctness work; blocks GUI use |
| Zero tests for ~1,300 lines of Python | all `.py` | The §G.1 defects survived undetected |
| `main.py` wires `train`/`bench` to the broken Python generator | `main.py:14–29` | Any model trained is trained on illegal chess |
| `from x import *` in 6 files | `gui.py:8–12`, `train.py`, `mcts.py`, `moves.py` | `WHITE`/`BLACK`/`Move` provenance untraceable |
| Engine runs on the Qt main thread | `gui.py:397` | UI freezes for the whole search |
| `panic = "abort"` in release | `Cargo.toml:14` | Blocks `cargo test --release`; no unwinding |
| `id author OpenAI` | `src/main.rs:18` | Wrong attribution |
| No CI, no lint gate, no `rustfmt`/`clippy` in a checked-in config | — | Nothing prevents regression |
| `Searcher` + `Table` reallocated per iterative-deepening depth | `src/lib.rs:520` | TT and history discarded between depths |
| Repository was not under version control before this audit | — | No history for any prior work |
| `.venv/` committed-adjacent (gitignored) with an absolute path from a **different** directory name in `pyvenv.cfg` | `.venv/pyvenv.cfg` | venv is not relocatable; records `basic projects to make the github green/` |

---

## E. Missing subsystems

Against the target architecture, the following are absent (not "weak" — absent):
search-technique layer beyond plain alpha-beta; SEE; evaluation phase/tapering;
modular evaluation trait boundary; neural evaluation and any inference path; training
pipeline; reproducible self-play data generation; opening book; endgame tablebase
probing; endgame-specific evaluation; personality configuration layer; structured
telemetry; explanation layer; tactical suite; positional suite; benchmark suite with
stored history; real tournament driver (two processes, PGN, time control); SPRT; Elo
estimation with confidence intervals; documentation set (only `README.md` exists).

---

## F. Performance bottlenecks — measured

### F.1 The dominant bottleneck: unbounded quiet-check recursion in quiescence

`src/lib.rs:356` searches **every checking move** in quiescence, not just captures:

```rust
let is_capture = board.piece_on(m.get_dest()).is_some() || m.get_promotion().is_some();
if !in_check && !is_capture && board.make_move_new(m).checkers() == &chess::EMPTY {
    continue;   // only skips moves that are neither captures NOR checks
}
```

Quiet checks beget further quiet checks, bounded only by `MAX_QUIESCENCE_PLY = 32`.
The result is an exponential sub-tree hanging off every leaf of the main search.

**Baseline, `bench D` from startpos, Classical, 1 thread, Hash=16:**

| depth | nodes | wall time | effective branching factor |
|---:|---:|---:|---:|
| 1 | 50 | — | — |
| 2 | 1,066 | 0.043 s | 21 |
| 3 | 10,302 | 0.049 s | 9.7 |
| 4 | **95,173,917** | **182.523 s** | **9,238** |
| 5 | not reached | aborted at 600 s | — |

(Recorded machine-readably in `benchmarks/baseline-2026-09-26.json`.)

An effective branching factor of 9,238 is not a chess search.

**In real positions it is far worse, and the failure starts at depth 1.** Iterative
deepening begins at depth 1, and `negamax(depth=0)` immediately enters quiescence — so
the explosion is present in the very first iteration. Measured with a per-position
ceiling:

| position | depth | result |
|---|---:|---|
| CPW position 3 (`8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w`) | 2 | 118,876,593 nodes |
| CPW position 3 | 2 | **timeout at 45 s** on a repeat run |
| kiwipete | **1** | **did not finish in 600 s** |
| Italian Game (`r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w`) | **1** | **did not finish in 600 s** |

The start position is the engine's *best* case, because a closed initial position offers
almost no checks. Every position with open lines — i.e. every real game after move 5 —
is unsearchable at any depth. This reframes the defect: it is not "the engine is slow
beyond depth 3", it is **"the engine does not work"**.

**Experiment E1 — hypothesis test.** Restrict quiescence to captures and promotions
(one-line change), rebuild, re-measure:

| depth | baseline nodes | E1 nodes | node reduction | E1 time |
|---:|---:|---:|---:|---:|
| 3 | 10,302 | 9,424 | 1.1× | — |
| 4 | 95,173,917 | 30,464 | **3,124×** | 0.077 s |
| 5 | not reached | 101,624 | — | 0.181 s |
| 6 | not reached | 278,464 | — | 0.472 s |

Depth 4 goes from 182.5 s to 0.077 s — a **2,370× speedup** — and depth 6 (the shipped
default) becomes reachable in under half a second. This confirms the diagnosis.

E1 is a *diagnostic*, not the final fix: dropping checks from quiescence entirely may
cost tactical accuracy. The correct fix is to allow checks only for the first 1–2
quiescence plies and add SEE-based pruning of losing captures, then measure both
variants against a tactical suite. That suite does not exist yet, so E1 was reverted and
the baseline restored (`git checkout -- src/lib.rs`); the change is recorded in
`experiments/E1-quiescence-quiet-checks.md`.

### F.2 Consequence: the GUI is effectively frozen

`gui.py:57` constructs `UCIEngine(depth=4)` and `gui.py:397` calls `best_move()`
**synchronously on the Qt main thread**, with no `movetime` and no node cap — so the
search is bounded only by depth.

From the start position that is ~3 minutes of frozen window per engine move. Once the
opening is over it is unbounded: the depth-1 measurements above show a middlegame
position exceeding 600 s, and the GUI asks for depth 4. **The GUI hangs permanently on
the engine's first real middlegame move**, and because the call is on the Qt main thread
the window cannot even be closed. This is a hang, not a slowdown.

### F.3 Secondary bottlenecks (real, but dominated by F.1)

- **`ordered()` plays every move to score it.** `src/lib.rs:324` calls
  `board.make_move_new(*m)` for every move at every node purely to test for check, then
  `quiescence` calls `make_move_new` again on the moves it keeps. Two full board copies
  per move per node.
- **`ordered()` heap-allocates a `Vec<ChessMove>` per node** (`src/lib.rs:315`) and sorts
  all moves eagerly instead of using staged generation with a selection pass.
- **`evaluate_with_style` scans all 64 squares** (`src/lib.rs:103`) rather than iterating
  piece bitboards.
- **`king_safety` runs a full legal move generation per side, per evaluation.**
  `src/lib.rs:241–247` builds a null-move board and enumerates all its legal moves — so
  every `evaluate` call costs 2 extra move generations.
- **No incremental evaluation.** Every leaf evaluates from scratch; there is no
  accumulator, which is also the hook NNUE would need.
- **`Table` stores `Option<Entry>`**, paying a discriminant word and giving a
  cache-unfriendly entry size, with no bucketing and no aging.

### F.4 Chaos style is the most expensive mode

`evaluate_with_style` under `Style::Chaos` (`src/lib.rs:123–129`) additionally runs
`MoveGen::new_legal` twice for mobility, `checking_moves` twice (each of which plays
*every* legal move), and `center_control` (which runs `MoveGen::new_legal` **four more
times**). That is on the order of a dozen move generations per evaluated node, on top of
the two that `king_safety` already costs.

---

## G. Correctness risks

### G.1 The legacy Python move generator is comprehensively broken — **confirmed** (fixed in `7ac85ed`)

`moves.py:185` filters legality with the wrong king:

```python
def generate_legal_moves(board):
    for move in pseudo:
        new_board = apply_move(board, move)
        if not new_board.in_check():      # in_check() tests the SIDE TO MOVE
            legal.append(move)            # ...which is now the OPPONENT
```

`apply_move` flips the side to move, so `new_board.in_check()` asks "did the mover give
check?". The filter therefore **discards every legal checking move** and **admits every
move that leaves the mover's own king in check**.

Measured `perft` against known-correct counts:

| position | depth | expected | actual | |
|---|---:|---:|---:|---|
| startpos | 1 | 20 | 20 | pass |
| startpos | 2 | 400 | 400 | pass |
| startpos | 3 | 8,902 | 8,982 | **FAIL** |
| startpos | 4 | 197,281 | 200,296 | **FAIL** |
| kiwi-variant **[corrected]** | 1 | 42 | 39 | **FAIL** (3 legal moves missing) |
| kiwi-variant **[corrected]** | 2 | 1,818 | 1,554 | **FAIL** |
| kiwi-variant **[corrected]** | 3 | 75,804 | 56,295 | **FAIL** |
| CPW pos 3 | 1 | 14 | 15 | **FAIL** |
| CPW pos 3 | 4 | 43,238 | 62,055 | **FAIL** |
| CPW pos 4 | 1 | 6 | **36** | **FAIL** (30 illegal moves offered) |

Direct probes isolate four independent defects (three more were found later, see Errata):

1. **Inverted legality filter** (`moves.py:190`). In
   `4k3/8/8/8/8/8/4r3/4K3 w`, `generate_legal_moves` returns `e1d2` and `e1f2`, both of
   which leave the white king attacked by the rook on e2. **[corrected]** In
   `rnbqkbnr/ppppp2p/5p2/6p1/4P3/8/PPPP1PPP/RNBQKBNR w`, where `Qd1-h5` is a legal mating
   check, `d1h5` is **not** returned. (The position originally cited here had e2 blocked,
   so it proved nothing; see Errata.)
2. **All four castling occupancy masks are swapped between colours**
   (`moves.py:168–180`). White kingside tests `0x6000000000000000` (f8/g8) instead of
   `0x60` (f1/g1). Verified: from `R3KBNR w KQ` — f1 and g1 occupied by a bishop and a
   knight — `e1g1` **is** generated; from `rnbqkbnr/…/R3K2R w KQ` — f1/g1 empty — `e1g1`
   is **not** generated.
3. **En passant removes the wrong square and the wrong colour**
   (`moves.py:204–205`): `to_sq + 8` for White (should be `-8`), and it removes
   `W_PAWN` — the mover's own colour — instead of the opponent's pawn. Because
   `remove_piece` is a silent bitmask AND, nothing is removed at all. Verified: after
   `exd6` e.p. from `4k3/8/8/3pP3/8/8/8/4K3 w - d6`, the captured black pawn is **still
   on d5**. Material is created from nothing.
4. **Knight promotion produces a pawn on the 8th rank** (`moves.py:211`). `promo_map`
   is `[W_PAWN, W_BISHOP, W_ROOK, W_QUEEN]` but `PROMO_KNIGHT == 0`, so index 0 yields
   `W_PAWN`. Verified: `b7b8n` places a **white pawn on b8** — a position no legal FEN
   can represent.

**Blast radius (at audit time).** This is the GUI's rules engine (§A), so the GUI enforces wrong rules.
It is also the environment `mcts.py` searches and the generator `train.py` uses to
produce every training label (`main.py:14`, `main.py:24`), so **any model trained by this
repository is trained on a game that is not chess**.

### G.2 The transposition table never verifies the key — **confirmed by construction**

```rust
fn get(&self, key: u64) -> Option<Entry> {
    self.entries[(key as usize) % self.entries.len()]   // src/lib.rs:290
}
```

`Entry.key` is written by `put` (`:293`) and **never read back**. Any two positions whose
Zobrist hashes are congruent modulo the table length share a slot, and `negamax`
(`:386`) will take a cutoff or a bound from a completely unrelated position.

Honest scope: this is a certain defect by inspection, but I could **not** demonstrate it
changing a reported score in probing — scores were identical at Hash=1 MB and Hash=64 MB
across four test positions at depth 2–3. That is expected: at reachable depths too few
entries are stored to collide. The bug's impact grows with depth and with smaller hash,
i.e. it will start corrupting results precisely once §F.1 is fixed and real depths become
reachable. It needs a unit test on `Table` (which will fail today) rather than a
search-level test.

Two further TT defects in the same code: **mate scores are stored ply-relative and
returned unadjusted**, so a mate score read at a different ply is wrong by that offset;
and `put` replaces on `depth >= old.depth` with **no aging or generation counter**, so a
table that fills with deep entries from an earlier move never recovers.

### G.3 Root PVS searches the first move with a null window

```rust
let mut value = -searcher.negamax(&child, depth-1, -alpha - 1, -alpha, 1);   // :447
if index > 0 && value > alpha && value < beta && !searcher.stopped { ... }   // :448
```

The re-search is gated on `index > 0`, but the *scout* search is not. The first root move
— the one whose score defines the PV — is searched with the null window
`(-alpha-1, -alpha)` and **never re-searched**. On the first iteration `alpha = -INF`, so
that window is `(31999, 32000)`: the returned value is a meaningless bound, and it is
what seeds `alpha` for every sibling.

### G.4 Root parallelism is harmful, and `Threads` is ignored — **confirmed**

`parallel_root` (`src/lib.rs:463`) spawns **one thread per legal root move**, regardless
of the `Threads` setting, and gives each thread its **own full-size** `Table` and a full
`(-INF, INF)` window with no sharing.

`bench 3` from startpos, Hash=16:

| Threads | nodes | peak RSS | wall time |
|---:|---:|---:|---:|
| 1 | 10,302 | 18.6 MB | 0.766 s |
| 2 | 529,008 | 285.1 MB | 0.787 s |
| 4 | 529,008 | 307.0 MB | 0.752 s |
| 8 | 529,008 | 195.4 MB | 0.705 s |

Three findings: node count is **identical for 2, 4 and 8** threads (the setting is a
boolean, not a count); it is **51× the single-threaded count** for the same depth and the
same best move, because discarding the shared window and the shared TT discards all
alpha-beta pruning between root moves; and there is **no speedup at all**.

Memory scales with `Hash × (number of root moves)`, measured at Threads=2:

| Hash | peak RSS |
|---:|---:|
| 1 MB | 18.7 MB |
| 8 MB | 171.0 MB |
| 16 MB | 291.1 MB |
| 64 MB | 1,219.6 MB |

That is ~19 MB of RSS per MB of `Hash`. The advertised maximum `Hash=1024`
(`src/main.rs:20`) with `Threads=2` therefore attempts roughly **19.5 GB on an 8 GB
machine** — a hard OOM. I derived this from the measured scaling law rather than
triggering the crash.

### G.5 `go wtime/btime` is not implemented — the engine forfeits — **confirmed**

`src/main.rs:57–80` parses only `depth`, `nodes`, `movetime` and `infinite`. A standard
tournament `go wtime 5000 btime 5000 winc 0 binc 0` sets no limit, so the engine runs to
`limits.depth` (default **6**), which §F.1 shows takes hours. Probed: **no `bestmove`
within 12 s, no output at all.** Under any clock-based time control against any
opponent, this engine hangs and loses on time. This is the single hardest blocker to
"competing with established engines".

*Correction to my own prior expectation:* I predicted `movetime` would also be broken,
because `Searcher.start` is reset on every iterative-deepening depth (`src/lib.rs:523`)
and each depth therefore restarts the budget. Measurement disproved the practical impact
— requested vs actual was 100 ms→0.13 s, 300 ms→0.32 s, 1000 ms→1.02 s, 2000 ms→2.02 s
(≈1.0×). The reason is that the loop `break`s on the first depth that times out, so only
one depth ever consumes the budget. The per-depth clock reset is a **latent** bug worth
fixing, not a current time-loss. Recorded here because the measurement contradicted the
prediction.

### G.6 `stop` is a no-op and `go infinite` never returns — **confirmed**

`src/main.rs:113` is literally `"stop" => {}`. The UCI loop is a blocking read over
stdin and the search is synchronous, so no command can be received while a search runs.
Probed `go infinite` then `stop`: **no output in 12 s**. Any real GUI or analysis client
(Arena, Cute Chess, En Croissant) depends on `stop`; none of them can drive this engine.

### G.7 No draw detection at all

The Rust engine keeps a bare `Board` with no move history, `BoardStatus` offers only
`Checkmate`/`Stalemate`/`Ongoing`, and `uci_bridge.py:60` never sends the move list.
Consequently there is **no threefold-repetition detection, no fifty-move rule, and no
insufficient-material detection** anywhere in the Rust engine. It will repeat a position
in a won game, and it cannot claim a draw in a lost one. The Python side checks only
`halfmove >= 100` (`moves.py:263`) and never repetition.

### G.8 Passed-pawn detection is wrong

`passed_pawn_score` (`src/lib.rs:213`) treats an enemy pawn **anywhere** on the same or
an adjacent file as blocking, without comparing ranks. An enemy pawn *behind* the
candidate — which cannot stop it — suppresses the bonus, and there is no check that the
pawn's own path is clear.

### G.9 Mate scores are reported as centipawns

`src/main.rs:83` always prints `score cp`. Probed a mate-in-one
(`6k1/5ppp/8/8/8/8/5PPP/R5K1 w`): the engine finds `a1a8` but reports
`info depth 3 nodes 2187 score cp 29999`. UCI requires `score mate 1`. Every GUI will
render this as +299.99 pawns.

### G.10 Lesser correctness issues

- No `info ... pv ...` output at all — one `info` line is printed after the search ends
  (`src/main.rs:82`), so no GUI can show a principal variation and nothing can be
  verified mid-search.
- `setoption` name parsing takes a single token (`src/main.rs:28`), so any multi-word
  option name will break as soon as one is added.
- `position fen` consumes exactly 6 tokens (`src/main.rs:128`); a 4-field FEN fails.
- `ucinewgame` does not clear the TT (it is per-search today, which masks this).
- `quiescence` tests `BoardStatus::Checkmate` (`src/lib.rs:343`) which costs a full move
  generation, before the cheaper stand-pat test.
- `search` returns the first legal move as a fallback with `depth: 0`, which is
  indistinguishable from a real depth-0 result.
- `king_safety` uses `board.null_move().unwrap_or(*board)`; `null_move()` returns `None`
  when the side to move is in check, so in check the function silently measures the
  **wrong side's** moves.
- `gui.py:451` computes `result = get_result(...)` and never uses it; `end_game` reports
  "Draw!" for every non-checkmate termination including stalemate-by-rule situations.

---

## H. Neural architecture plan

Deliberately staged behind correctness, because on this repository neural work is
currently blocked twice over: the search cannot reach depths at which an evaluator
matters (§F.1), and the only data generator produces illegal chess (§G.1).

**Target: NNUE-style incrementally-updated evaluation.** Chosen over a policy/value
CNN because the search is CPU alpha-beta on an 8-core laptop, the same reason Stockfish
uses NNUE rather than a Leela-style net.

- **Features:** HalfKP-style (king square × piece × square), per-perspective, two
  accumulators.
- **Topology:** feature transformer 2×(N→256) → clipped-ReLU → 32 → 32 → 1, int16/int8
  quantised.
- **Inference:** incremental accumulator updates on make/unmake — which requires an
  incremental board representation the `chess` crate does not offer, hence the
  own-move-generator decision in §P.
- **Integration boundary:** an `Evaluator` trait so `search` calls `evaluate(&position)`
  without knowing whether the implementation is classical, NNUE or hybrid. This
  boundary is the prerequisite and should be introduced early, while the evaluation is
  still purely classical.
- **Model artefacts:** every net carries architecture metadata, dataset version,
  training config, engine commit, validation metrics and match result. No silent
  replacement.

**Honest constraint:** 8 GB of unified memory and no discrete GPU. Training a useful
NNUE from self-play on this machine is a matter of days-to-weeks of wall time, not
hours. The credible local path is supervised distillation from a strong engine's
evaluations or from a public position set, with self-play reinforcement as a later
stage. This is a hardware limit to be stated, not engineered around.

## I. Self-play architecture

Blocked on §G.1 until the generator is correct — generating data now would produce a
corpus of illegal positions. Planned: a driver that runs N engine processes over real
UCI, fixed seeds, fixed opening sets, recording per position FEN, move, score, depth,
nodes, PV, engine config, model version and game result, in a versioned on-disk format
with a manifest. Must be resumable and must record the exact binary hash.

## J. Training architecture

`DATASET → validate → clean → position extract → label → train → validate → integrate →
self-play → benchmark → match test → accept/reject`, with a gate at "match test": a net
is only promoted if it wins a match by a statistically meaningful margin (§L). The
existing `neural_net.py` is a hand-written NumPy MLP with hand-derived Adam; it is
useful as a reference but is not a training stack, and it currently learns from illegal
chess. It should be retained as research code and superseded rather than extended.

## K. Benchmark architecture

Needed, none exists. Four tracks: **engine performance** (nodes, NPS, depth, time to
depth, peak RSS, thread scaling); **search quality** (TT hit rate, cutoff rate,
effective branching factor, move-ordering efficiency — first-move cutoff share);
**chess strength** (tactical suite solve rate at fixed nodes, positional suites,
endgame suites); **neural quality** (validation loss, calibration, inference latency,
size, measured strength delta). Every run writes a timestamped JSON record with the
commit hash, so before/after is a diff and not a memory. Fixed **nodes**, not fixed
time, for anything that must be comparable across machines.

## L. Tournament architecture

The current `tools/tournament.py` is not a tournament harness: it drives **one process**,
swaps the `Style` option between moves, never detects game end, plays a fixed ply count,
and prints a move list rather than a result. It cannot produce a win/draw/loss, let
alone a rating.

Required: two independent engine processes over real UCI; real time control (needs
§G.5); legality validation of every move received; termination by mate, stalemate,
repetition, fifty-move and insufficient material (needs §G.7); PGN output; balanced
opening books played from both sides; machine-readable results; and **SPRT** for
accept/reject decisions on changes. Elo is only reported with opponent, hardware, time
control, sample size and a confidence interval — never as a bare number.

## M. GUI evolution plan

Order matters: the GUI's problems are mostly engine problems. (1) Make the **Rust engine
authoritative** for state and legality and delete the GUI's dependency on the Python
generator — this alone fixes the wrong-rules bug. (2) Move the search **off the Qt main
thread** so the window stays responsive. (3) Consume streamed `info` lines for live
depth/score/PV once §G.10 provides them. Only then: evaluation graph, candidate-move
list, evaluation breakdown, personality and model selectors, research-mode telemetry.

## N. Research roadmap

Experiments are tracked in `experiments/` one file each, with hypothesis, baseline,
implementation, result and conclusion (keep/modify/reject). Failed experiments are kept.
`experiments/E1-quiescence-quiet-checks.md` is the first entry. Near-term queue:
quiescence scope and SEE pruning; tapered evaluation; null-move and LMR with SPRT gates;
shared-TT Lazy SMP versus the current root split; Chaos as a *parameterised* evaluation
policy with measurable risk terms rather than an ad-hoc bonus block.

## O. Dependency graph

```
config.py ──> board.py ──> moves.py ──┬──> mcts.py ──> train.py ──> main.py
                                      │       ▲            │           │
                                      │       └── neural_net.py ───────┤
                                      └──> gui.py <── uci_bridge.py    │
                                              ▲            │           │
                                              └────────────┼───────────┘
                                                           ▼
                                              target/release/crazy-chess
                                                   (src/main.rs)
                                                        │
                                                   src/lib.rs
                                                        │
                                                   chess crate 3.2
```

Observations: `moves.py` is the transitive root of the entire Python half **and** it is
the broken component; `uci_bridge.py` is the only Rust↔Python edge and it is
history-less (§G.7); nothing depends on `mcts.py`/`neural_net.py` except `train.py` and
`main.py`, so the ML half can be quarantined and rebuilt without touching the engine;
`src/lib.rs` has no internal module structure at all — search, evaluation, TT and
parallelism are one 620-line file, which is why the `Evaluator` boundary in §H has
nowhere to attach yet.

---

## P. Prioritised implementation roadmap

Ordering is driven by the dependency analysis, not by the generic phase list. Three
things justify reordering:

- **§F.1 gates everything measurable.** No search, evaluation or neural work can be
  A/B tested while depth 4 costs 182 s, because no experiment can be run enough times.
  It is also a one-line-ish fix. It goes first.
- **§G.5/§G.6 gate all strength measurement.** Without `wtime/btime` and `stop`, no
  match can be played against any opponent, so no Elo, no SPRT, no accept/reject gate.
  These come before search features, because search features without a strength gate are
  guesses.
- **§G.1 gates all learning work**, and is also the GUI correctness bug. It comes before
  self-play and training, and it forces the question of whether to fix `moves.py` or
  delete it (recommendation: make Rust authoritative for the GUI, keep `moves.py` alive
  only behind a perft suite, since its four defects are all cheap to fix once tested).

| # | Work | Unblocks | Gate / exit criterion |
|---|---|---|---|
| 0 | Version control, baseline benchmark harness, this audit | measurement | committed ✅ |
| 1 | Fix quiescence scope (§F.1); TT key verification + mate-score adjustment (§G.2); root PVS window (§G.3) | depth ≥ 8 reachable | perft unchanged; depth-6 bench < 1 s; new unit tests fail before / pass after |
| 2 | `wtime/btime/movestogo/inc` + real time manager; threaded `stop`; `info … pv`; `score mate` (§G.5, §G.6, §G.9, §G.10) | all match play | plays a full timed game without forfeiting; `stop` answers < 50 ms |
| 3 | Perft + FEN + rules test suite in Rust; fix or quarantine `moves.py` (§G.1); repetition / fifty-move / insufficient material (§G.7) | correctness floor | all standard perft positions to d5; fuzz over random legal games finds no illegal move |
| 4 | Tactical suite + benchmark suite with stored JSON history (§K) | every later claim | solve-rate and NPS tracked per commit |
| 5 | Real two-process tournament driver + SPRT + Elo with CI (§L) | accept/reject on strength | can measure a known-good change as positive |
| 6 | Search strength: null-move, LMR, LMP, futility, RFP, killers, counter-move/continuation history, SEE, singular extensions — each behind an SPRT gate | strength | each technique kept only on measured gain |
| 7 | `Evaluator` trait + module split of `lib.rs`; tapered eval; fix passed pawns (§G.8); phase-aware endgame terms | neural integration | no strength regression from refactor |
| 8 | Own move generator with incremental make/unmake (magic bitboards) | NNUE accumulators, SEE, staged movegen | perft parity with `chess` crate, faster |
| 9 | Lazy SMP with a shared, lock-free TT replacing root split (§G.4) | scaling | measured scaling > 1.5× at 4 threads, no memory blowup |
| 10 | Personality layer: Chaos as parameterised, explainable policy | Chaos research | Chaos measurable vs Classical in matches |
| 11 | Opening book; Syzygy probing (optional, graceful without) | opening/endgame | — |
| 12 | Self-play data pipeline (§I) → NNUE training (§J) → hybrid eval | learning | net promoted only on SPRT-positive match |
| 13 | GUI: Rust-authoritative, off-thread, streamed telemetry, research mode (§M) | usability | responsive during search |
| 14 | Profiling-driven optimisation; packaging | performance | flamegraph-justified changes only |

### Explicit non-goals

Line count is not a target. The 600k-line figure is a possible consequence of items
6–14 and their tests, data pipelines and documentation — it is not steered toward, and
no file will be generated to inflate it. A smaller engine that measurably beats a larger
one is the better outcome.

### What cannot be done locally

Stated plainly rather than faked: no GPU, 8 GB RAM, 8 cores. Large-scale NNUE training,
million-game self-play corpora, and long SPRT runs at long time controls are all
compute-bound beyond this machine. Syzygy tablebases (~150 GB for 6-man) and large
opening books are data the repository does not have. Elo anchored to a public rating
list additionally requires reference engine binaries, which are not vendored here. All
of this will be prepared so it runs when the resources exist, and the documentation will
say which numbers are measured locally and which require external resources.

---

## Reproducing this audit

```bash
cargo build --release && cargo test
python3 tools/audit_baseline.py          # writes benchmarks/baseline-<date>.json
python3 tools/perft_python.py            # legacy Python generator vs known counts
python3 tools/uci_conformance.py         # time control, stop, mate reporting
```
