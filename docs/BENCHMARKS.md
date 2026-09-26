# Benchmarks

Tools that measure the engine without playing games. Each writes machine-readable output,
or exits non-zero on failure, so it can serve as a gate.

| tool | measures | output |
|---|---|---|
| `tools/audit_baseline.py` | nodes, time, NPS, score and best move at fixed depth on 5 positions | `benchmarks/<label>-<date>.json` (records `+dirty` when sources differ from HEAD) |
| `target/release/tactics` | solve rate on the 22 machine-proven mates; `--sweep` compares quiescence settings | stdout |
| `tools/uci_conformance.py` | clock handling, `stop`, mate scores, PV | exit code = failures (baseline 4/8, now 8/8) |
| `tools/perft_python.py` | Python move generator vs published perft counts | exit 1 on any mismatch |
| `tools/diff_movegen.py` | Python vs Rust legal moves at every node of a tree | first disagreeing position |
| `bench <depth>` (UCI) | fixed-depth search from the current position, fresh table | one line; parsed by the tools |
| `sample <pid> <secs>` (macOS) | where search time goes | call tree |

## Principles

- **Fixed depth or fixed nodes** for anything compared across runs. Wall-clock numbers are
  reported, but never compared across machines.
- **A pure speed change must leave the tree unchanged.** The check is identical node
  counts at fixed depth. The move-ordering fix in `74fcfb6` was accepted on exactly that:
  same nodes, 1.2–1.9× faster.
- **A search change is judged by games** (`docs/TOURNAMENTS.md`). Fewer nodes to a given
  depth is necessary, not sufficient: pruning can reach depth cheaply by searching the
  wrong things.

## History (fixed depth 7 unless noted)

| milestone | startpos | kiwi-variant | open middlegame | source |
|---|---:|---:|---:|---|
| audited baseline `b091ae7` | depth 4: 95,173,917 nodes / 182.5 s | depth 1: >600 s | depth 1: >600 s | `benchmarks/baseline-2026-09-26.json` |
| quiescence fix `022559d` | 994,067 | 1,633,383 | 7,255,660 | `benchmarks/qs-fix-2026-09-26.json` |
| TT / root PVS / persistence `01be376` | 438,340 | 980,673 | 2,107,757 | `benchmarks/search-fixes-2026-09-26.json` |

Depth 8, recent commits:

| commit | startpos | kiwi-variant | open-mid |
|---|---:|---:|---:|
| `74fcfb6` (ordering keys cached) | 1,310,156 | 3,132,816 | 8,323,715 |
| `c8576f3` (interior PVS) | 1,287,052 | 3,126,901 | 7,846,245 |
| `63b3bce` (null-move pruning) | 332,839 | 628,924 | 1,333,388 |

## Latest profile (`74fcfb6`, before the ordering fix)

The search thread's time went to: the move-ordering sort key ~42% (fixed in `74fcfb6`),
evaluation ~17% (the king-safety term runs a full move generation), move generation
~12%, `make_move` ~10%, sorting ~5%, allocation ~4%. Re-profile before the next
performance change; don't optimize from this table.
