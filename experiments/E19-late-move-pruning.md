# E19 — Late move pruning

- **Date:** 2026-09-27
- **Change:** `67bfd91`; **base:** `b3a0ce0` (futility pruning)
- **Status:** **rejected**, reverted. No measurable effect.

## Change

At non-PV nodes not in check, with depth ≤ 3: once 3 + depth² moves have been tried,
the remaining quiet moves (not captures, promotions or checks) are skipped.

## Result

SPRT, H0 0 vs H1 +10, 2+0.02, idle machine, 2000-game cap:

| games | W / L / D | Elo (pentanomial 95%) | LOS | LLR | verdict |
|---:|---|---|---:|---:|---|
| 2000 | 662 / 642 / 696 | +3.5 ± 12.3 | 71.0% | −0.39 | **undecided at cap** |

Record: `matches/sprt-lmp.json`.

## Conclusion

**Reject.** The rule is to keep a change only on a positive or non-regression result.
E12 (check extension) was also undecided at the cap, but it leaned clearly positive
(LOS 96.8%) and then passed a pre-registered non-regression test. This one doesn't lean
either way (LOS 71%), so no second test was justified.

Likely reason: at these depths, futility pruning (E18) and LMR (E9) already remove or
reduce most of the same late quiet moves, so LMP has little left to add. It could be
revisited with a different threshold, or after tuning futility, as a new experiment.
