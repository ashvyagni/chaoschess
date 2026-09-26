# E9 — Late move reductions, retried on the new ordering

- **Date:** 2026-09-26
- **Change:** `7030d5a` (identical LMR code to `9d4355f`); **base:** `8db5d50` (E8 ordering)
- **Status:** **accepted** (SPRT H1)

## Hypothesis

E7 rejected LMR (−6.2 ± 16.3) and attributed the failure to move ordering, not to LMR.
If that diagnosis is right, *the same code* should gain once the ordering is fixed.

## Method

The rejected change was re-applied unchanged, by reverting its revert. The only
difference from E7 is the base: E8's ordering. Same time control (2+0.02), same SPRT
setup, idle machine.

## Result

Fixed depth vs `8db5d50`: startpos d10 3.5× fewer nodes, kiwi-variant d9 3.2×,
open middlegame d9 7.3×.

| experiment | base ordering | games | Elo (pentanomial 95%) | LLR | verdict |
|---|---|---:|---|---:|---|
| E7 | old | 958 | −6.2 ± 16.3 | −2.96 | accept H0 |
| **E9** | **E8** | **818** | **+30.7 ± 17.4** | **+3.29** | **accept H1** |

W/L/D 248 / 176 / 394, LOS 100%, pentanomial `[25, 71, 160, 113, 40]`. Record:
`matches/sprt-lmr-retry.json`.

## Conclusion

**Keep.** The diagnosis held: the same reductions went from about −6 to about +31 Elo when
the ordering stopped hiding good moves late in the list. The two intervals barely overlap
(E7 upper bound +10.1, E9 lower bound +13.3).

This is the pattern the experiment log exists for. A rejected idea was kept on record with
an explanation, the explanation made a prediction, and the prediction was then tested.
Had E7 been dropped silently, LMR might never have been tried again.
