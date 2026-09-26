# E6 — Null-move pruning

- **Date:** 2026-09-26
- **Change:** `63b3bce`; **base:** `c8576f3` (interior PVS)
- **Status:** **accepted** (SPRT H1)

## Hypothesis

If the side to move can pass and still fail high in a reduced search, a real move almost
certainly fails high too. Cutting such nodes should reduce nodes sharply, and the extra
depth should gain Elo. The guards (listed in the commit) are there so the known failure
modes don't cost that gain back: zugzwang, check, double null moves, unproven mates.

## Result

Nodes at fixed depth: startpos d8 3.9× fewer, kiwi-variant 5.0×, open middlegame 5.9×,
rook endgame d9 2.0×. The 22-mate suite and all exactness tests pass.

SPRT, H0 0 vs H1 +15, α = β = 0.05, 2+0.02, idle machine (`tools/sprt.sh nmp c8576f3 2+0.02 0 15`):

| games | W / L / D | Elo (pentanomial 95%) | LOS | LLR | verdict |
|---:|---|---|---:|---:|---|
| 656 | 185 / 137 / 334 | **+25.5 ± 18.2** | 99.6% | +3.14 | **accept H1** |

Pentanomial pairs `[16, 64, 124, 104, 20]`. No time forfeits. Record:
`matches/sprt-nmp.json`.

## Conclusion

**Keep.** The first search technique in this project with a measured, statistically
decisive gain. The ±18 interval is wide; the point estimate is indicative only.

The parameters (min depth 3, reduction 3 + depth/6, eval ≥ beta precondition) are
conventional and untuned. Tuning them is a candidate for later SPRTs, one parameter at a
time.
