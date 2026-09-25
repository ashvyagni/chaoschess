# E2 — Sound TT, exact root PVS, persistent iterative deepening

- **Date:** 2026-09-26
- **Baseline commit:** `022559d` (quiescence fixed, TT/root defects still present)
- **Change commit:** `01be376`
- **Status:** **kept**

## Hypothesis

Three audited defects waste work or corrupt scores, and fixing them should reduce nodes
at equal depth without changing correct results:

1. **TT without key verification** (§G.2) returns a neighbour's bound on slot collisions,
   and stores mate scores ply-relative so they are wrong when probed at another ply.
2. **Root scouts the first move with a null window and never re-searches it** (§G.3), so
   the PV move's score is a bound, which then seeds alpha for every sibling.
3. **A fresh TT and history per iterative-deepening depth** throws away exactly the
   information iterative deepening exists to produce.

Predictions: (1) and (2) are *correctness* fixes — detectable by exact-score tests, not
necessarily by node counts. (3) is a *performance* fix — node counts should fall, most in
positions where move ordering matters (open middlegames).

## Implementation

- `Table::get` compares keys; `score_to_tt` / `score_from_tt` convert mate distances;
  entries carry a generation and stale entries are replaced first.
- Root: full window for the first move, null-window scouts with re-search for the rest,
  stop on fail-high, previous best move ordered first.
- One `Searcher` for the whole ID run; history bounded by a gravity update.

## Result

Correctness, verified by mutation testing (bug reintroduced → test fails):

| defect | test | catches the mutant |
|---|---|---|
| TT key check removed | `transposition_table_rejects_slot_collisions` | yes |
| mate scores stored raw | `tt_mate_scores_round_trip` | yes |
| root scouts first move | `shallow_search_score_equals_plain_minimax` | yes — **after** adding a constructed position |

The third row is the instructive one. The first version of the exactness test used five
general positions and **passed against the buggy root**. The degenerate window only loses
information when the first move's value depends on a capture two plies below it, which
those positions happen not to have. A constructed fork (`r3k3/8/2p5/8/8/8/8/2Q1K3 w`,
Qxc6+ forking king and rook) exposes it: the buggy root reports **+392**, the true value is
**+883**. The bug was real and material; the general-position test was simply blind to it.

Performance, depth 7, same five benchmark positions (`tools/audit_baseline.py`):

| position | before (`022559d`) | after (`01be376`) | reduction | time before → after |
|---|---:|---:|---:|---|
| startpos | 994,067 | 438,340 | 2.3× | 1.70 s → 0.66 s |
| kiwipete | 1,633,383 | 980,673 | 1.7× | 4.45 s → 2.48 s |
| open middlegame | 7,255,660 | 2,107,757 | 3.4× | 17.08 s → 4.48 s |
| rook endgame | 299,350 | 298,809 | 1.0× | 0.32 s → 0.32 s |
| pawn endgame | 13,455 | 11,057 | 1.2× | 0.014 s → 0.013 s |

As predicted, the gain concentrates where ordering matters (open middlegame 3.4×) and
vanishes in the sparse rook endgame. Depth 8 now completes in all five positions
(worst: open middlegame, 21.6 s).

Scores at depth ≥ 3 differ from the previous build in some positions. That is expected and
not a regression: with a persistent TT, entries searched deeper than required can satisfy
shallower probes, so values legitimately depend on search history. Exactness is asserted
only where it is provable (depth ≤ 2).

## Conclusion

**Keep.** All three changes are strictly better on this evidence.

**Caveat on the evidence.** Node counts at fixed depth measure *efficiency*, not *strength*.
Whether the engine plays better needs games, which needs the clock handling in roadmap
item 2. The benchmark set is five positions; the percentages are indicative, not general.
