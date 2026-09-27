# E13 — King-safety pressure from enemy attack maps

- **Date:** 2026-09-27
- **Base:** `507da0b`
- **Status:** **accepted** (pre-registered non-regression SPRT; the effect size suggests a gain)

## Motivation: a correctness bug, not a tuning idea

The pressure half of `king_safety` counted legal moves landing on the king's file in a
null-moved position. That is only meaningful for the side to move:

- For the **side not to move**, the null move handed the move to that side itself, so the
  term counted *its own* moves onto its own king file. That's nonsense, and it made the
  term asymmetric.
- **In check**, `null_move()` is unavailable and the unmodified board was used, so the
  term measured the wrong side again (audit §G.10).

The replacement counts enemy attacks on each square of the king's file, from attack maps.
It is well-defined for both sides and in check, and it is cheaper. The weight (3 cp per
attack) is unchanged. A colour-mirrored Italian Game position scores identically after
the change (75 vs 75 at depth 1).

## Test (stated before running)

Non-regression SPRT, H0 −10 vs H1 0, 2+0.02, idle machine. Keep if "not worse".

| decided at | games incl. overshoot | W / L / D | Elo (pentanomial 95%) | LOS | verdict |
|---:|---:|---|---|---:|---|
| game 376 (LLR +2.95) | 390 | 123 / 88 / 179 | **+31.3 ± 23.0** | 99.2% | **accept "not worse"** |

Record: `matches/sprt-king-safety-fix.json`.

## Conclusion

**Keep.** The test that was run only establishes "not worse". The interval, [+8.3, +54.3],
excludes zero, so a real gain is likely, but no gain test was pre-registered, and it is
not claimed as one.

## Side finding

Making this change broke the minimax-exactness test. The test had been passing by luck
since E11 and E12 added lossy selective techniques, and the new evaluation numbers
happened to trigger reverse futility pruning at a depth-1 node. The fix was to the test's
premise, not to this change: `SearchLimits::selective` lets the exactness test check only
the score-preserving machinery (`507da0b`, re-verified by mutation).
