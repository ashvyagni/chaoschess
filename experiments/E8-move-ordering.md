# E8 — Standard move ordering (killers, SEE bands)

- **Date:** 2026-09-26
- **Change:** `e8b05bd`; **base:** `42eae59` (same engine as `63b3bce`, null move)
- **Status:** **accepted** (SPRT H1)

## Hypothesis

E7's analysis: LMR failed because the ordering doesn't put bad moves last. Replacing it
with the standard bands should gain Elo on its own, and should make LMR viable. The
bands are TT move > SEE-winning captures (MVV-LVA) > two killers > quiets by history >
SEE-losing captures.

## Result

Fixed depth: 1.8–5.0× fewer nodes in three positions, 0.86× (more nodes) in
kiwi-variant.

SPRT, H0 0 vs H1 +10, 2+0.02, idle machine:

| games | W / L / D | Elo (pentanomial 95%) | LOS | LLR | verdict |
|---:|---|---|---:|---:|---|
| 244 | 82 / 29 / 133 | **+76.7 ± 30.5** | 100.0% | +3.22 | **accept H1** |

Pentanomial `[3, 15, 45, 44, 15]`. Record: `matches/sprt-ordering.json`.

## Conclusion

**Keep.** The largest single gain so far. It came from ordering, which a node-count view
alone would have undervalued (one benchmark position got worse). Next: retry LMR on top
of this ordering (E9).
