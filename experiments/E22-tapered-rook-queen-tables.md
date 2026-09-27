# E22 — Tapered rook and queen tables (pawns untapered)

- **Date:** 2026-09-28
- **Base:** `6166315` (E21 rejected and reverted)
- **Status:** **accepted** (SPRT H1)

## Hypothesis

E21 (pawn, rook and queen endgame tables together) lost 9.4 ± 13.6 Elo. The suspected
cause was the pawn table double-counting advancement already paid by
`tapered_passed_pawns`. If so, the same change without the pawn table should gain.

## Implementation

E21's code with the pawn arm removed. `tapered_piece_square` blends kings (E15), rooks
(no d1/e1 bonus, +10 on the seventh) and queens (centralised) toward endgame tables.
Pawns, knights and bishops keep one table. `tapered_king` is now a test-only wrapper.
A unit test checks the blend is exact at both ends and what each endgame table says.

## Result

SPRT, H0 0 vs H1 +10, 2+0.02, idle machine:

| decided at | games | W / L / D | Elo (pentanomial 95%) | LOS | verdict |
|---:|---:|---|---|---:|---|
| 2686 (LLR +2.99) | 2700 | 915 / 807 / 978 | **+13.9 ± 10.3** | 99.5% | **accept H1** |

Record: `matches/sprt-tapered-rook-queen.json`. The decision came late, so early-stop
inflation is small and no fixed-length confirmation was run (the E16 rule).

## Conclusion

**Keep.** The gain supports the double-counting explanation for E21, although this
match alone doesn't prove it. The difference between the two runs is about 23 Elo, and
the only change was the pawn table. The tables are hand-written and untuned.
