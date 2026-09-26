# E7 — Late move reductions (first attempt)

- **Date:** 2026-09-26
- **Change:** `9d4355f`; **base:** `63b3bce` (null move)
- **Status:** **rejected** (SPRT H0), reverted. To be retried after move-ordering work.

## Hypothesis

Reducing quiet moves late in the ordering should give a large depth gain for little
risk, typically one of the biggest single gains in an alpha-beta engine.

## Implementation

Quiet moves from index 3 at depth ≥ 3, r = 0.75 + ln(depth)·ln(index)/2.25 (−1 at PV
nodes). Reduced null-window scout first; full-depth scout on fail-high; full window
inside the window. Never reduced: captures, promotions, checks, moves while in check.

## Result

Nodes at fixed depth fell 2.9–31.9× (open middlegame d9: 20.6 M → 0.65 M nodes,
33.8 s → 0.96 s). Tests and the mate suite passed.

SPRT, H0 0 vs H1 +15, 2+0.02, idle machine:

| games | W / L / D | Elo (pentanomial 95%) | LLR | verdict |
|---:|---|---|---:|---|
| 958 | 254 / 271 / 433 | **−6.2 ± 16.3** | −2.96 | **accept H0** |

Record: `matches/sprt-lmr.json`.

## Analysis

A 30× cut in nodes that buys no strength means the reductions are cutting moves that
matter. LMR is only as good as the assumption "late move = bad move", and this engine's
ordering does not earn it:

- There are **no killer moves**, so quiet refutations found at sibling nodes are not
  tried early.
- **History (up to 16,384) can outrank captures** (MVV-LVA roughly 1,000–9,000), so
  capture ordering is noisy.
- **Losing captures are not separated from winning ones.** SEE exists, but ordering
  doesn't use it.
- A **check bonus** promotes every quiet check above most captures.

The same data is the case for doing ordering first. It's the cheaper change, and it
improves every node, not just reduced ones.

## Conclusion

**Reject and revert** (`git revert`, history kept). **Retry** LMR on top of the
move-ordering change, as a new experiment against that commit. Fixed-depth node counts
were strongly positive, yet the game result was negative: a concrete example of why
node counts are not accepted as evidence of strength (docs/BENCHMARKS.md).
