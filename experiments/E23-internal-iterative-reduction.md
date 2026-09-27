# E23 — Internal iterative reduction

- **Date:** 2026-09-28
- **Base:** `2cafda6` (E22)
- **Status:** **rejected** (SPRT H0), code reverted

## Hypothesis

At a node with no TT move, move ordering is at its worst, so a full-depth search spends
most of its effort on the wrong moves. Searching one ply shallower is cheaper and leaves
a best move in the TT for the next visit. Many engines gain from it.

## Implementation

In `negamax`, after the TT probe: if `selective`, `depth >= 4` and the TT entry has no
best move (or there is no entry), `depth -= 1`. It applied at PV and non-PV nodes, before
RFP, null move and futility.

Fixed-depth bench (depth 10, fresh table), nodes before → after:
- start position: 126,952 → 100,110 (−21%), and the best move changed g1f3 → b1c3;
- Kiwipete: 726,621 → 651,113 (−10%);
- CPW position 3: 89,368 → 87,808 (−2%).

## Result

SPRT, H0 0 vs H1 +10, 2+0.02, idle machine:

| decided at | games | W / L / D | Elo (pentanomial 95%) | LOS | verdict |
|---:|---:|---|---|---:|---|
| 520 (LLR −2.97) | 534 | 145 / 202 / 187 | **−37.2 ± 22.8** | 0.1% | **accept H0** |

Record: `matches/sprt-iir.json`.

## Conclusion

**Reject; reverted.** Fewer nodes at fixed depth did not buy strength. The search was
saving nodes by being shallower, not by ordering better.

- **Ruled out:** repeated reductions. The TT stores a best move for every node that
  searched a move, so IIR fired at most once per position.
- **Untested explanation:** the reduced depth also feeds RFP (margin × depth), futility
  and null move. Each prunes more at a lower depth, so IIR compounds with them.

Variants worth a separate experiment:
- IIR after the pruning block;
- IIR only at PV nodes, or only at depth ≥ 6.

None of them is tried here, to avoid fitting variants to one match.
