# E24 — Null-move reduction scaled by the eval margin

- **Date:** 2026-09-28
- **Base:** `7924b63` (after E23 was reverted)
- **Status:** **rejected** (undecided at the game cap, no measurable effect), code reverted

## Hypothesis

The further the static score is above beta, the less a shallow refutation search can
change the null-move verdict. Reducing more in that case saves nodes without losing
cutoffs. This is a standard refinement of null-move pruning.

## Implementation

The null-move reduction became `3 + depth/6 + min((static_eval - beta) / 200, 3)`.
Before, it was `3 + depth/6`.

Fixed-depth bench (depth 10, fresh table), nodes before → after, with scores and moves
unchanged:
- start position: 126,952 → 126,952 (0%);
- Kiwipete: 726,621 → 712,253 (−2%);
- CPW position 3: 89,368 → 89,362 (0%).

## Result

SPRT, H0 0 vs H1 +10, 2+0.02:

| games | W / L / D | Elo (pentanomial 95%) | LOS | verdict |
|---:|---|---|---:|---|
| 4000 (cap) | 1413 / 1398 / 1189 | **+1.3 ± 8.7** | 61% | undecided (LLR −1.87) |

Record: `matches/sprt-nmp-eval.json`.

**Caveat — time losses.** 519 of the 4000 games were lost on time (13%), against about 1%
in earlier runs. They split evenly between the engines (base 263, new 256) and cluster
in the second half of the match, so they point to machine load or throttling, not to
either engine. Symmetric noise like this widens the interval but does not bias the
estimate.

## Conclusion

**Reject; reverted**, by the E19 rule: an undecided result with no lean is not worth
code. The engine already scales the reduction with depth, and null move rarely fires
near beta at this depth range, which the unchanged start-position count reflects.
