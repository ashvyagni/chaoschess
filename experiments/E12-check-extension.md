# E12 — Check extension

- **Date:** 2026-09-27
- **Base:** `e326f65` (RFP)
- **Status:** **kept**. The first SPRT was inconclusive; a pre-registered non-regression
  SPRT accepted "not worse".

## Change

A node whose side to move is in check is searched one ply deeper. The cap is ply <
2 × iteration depth (and < `MAX_DEPTH`), so long checking sequences can't extend
without bound. `in_check` is now computed once, before the depth-0 test.

## Results

**Test 1**: gain test, H0 0 vs H1 +10, 2000-game cap, 2+0.02:

| games | W / L / D | Elo | LOS | LLR | verdict |
|---:|---|---|---:|---:|---|
| 2000 | 519 / 461 / 1020 | +10.1 ± 10.8 | 96.8% | +1.67 | **undecided at cap** |

**Test 2**: decided and stated *before* running: a fresh non-regression SPRT, H0 −10 vs
H1 0. Keep if it accepts "not worse", revert otherwise.

| games | W / L / D | Elo | LLR at decision | verdict |
|---:|---|---|---:|---|
| 716 (730 incl. overshoot) | 199 / 161 / 370 | +18.1 ± 17.6 | +2.95 | **accept H1: not worse** |

Records: `matches/sprt-check-ext.json`, `matches/sprt-check-ext-nonreg.json`.

The arena build that ran test 2 recorded its verdict as "Continue", because it
recomputed the verdict after overshoot games had pulled the LLR back to +2.88. That was a
reporting bug, fixed in `b4dfec0`. The JSON was corrected from the run log, with a note
saying so.

## Conclusion

**Keep.** Both tests point the same way: +10.1 and +18.1 point estimates, and no sign of
a regression. The gain is small and not established at the +10 level. A tactical side
effect was checked: it does **not** restore the two ladder mates RFP delays (E11), because
those turn on a quiet move, not on a check.

On method: the second test was chosen after seeing the first. That is why its
hypotheses and the decision rule are written down above, and it is weaker evidence than
a single decisive test.
