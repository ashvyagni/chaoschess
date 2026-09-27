# E21 — Tapered pawn, rook and queen piece-square tables

- **Date:** 2026-09-27
- **Base:** `c04fcf0`
- **Status:** **rejected** (SPRT H0), code reverted (`d38c483`, reverted by `0c9c7c3`)

## Hypothesis

E15 showed that a single king table used at every phase was badly wrong in endgames
(+116 Elo once tapered). The other tables have smaller but similar problems:
- the middlegame pawn table penalises central pawns on the seventh rank;
- the rook table rewards d1/e1, a bonus meant for after castling;
- the queen table barely centralises.

Blending each toward an endgame table should gain.

## Implementation

`tapered_piece_square(piece, index, phase)` generalised `tapered_king`. The endgame tables were:
- **Pawns:** by relative rank only, `[0, 0, 5, 10, 20, 35, 55, 0]`.
- **Rooks:** flat, except +10 on the seventh rank.
- **Queens:** centralised, from −20 in the corners to +20 in the centre.

Knights and bishops kept one table. A unit test checked that the blend is exact at both ends.

## Result

SPRT, H0 0 vs H1 +10, 2+0.02, idle machine:

| decided at | games | W / L / D | Elo (pentanomial 95%) | LOS | verdict |
|---:|---:|---|---|---:|---|
| 1722 (LLR −2.96) | 1736 | 589 / 636 / 511 | **−9.4 ± 13.6** | 9.0% | **accept H0** |

Record: `matches/sprt-tapered-pst.json`.

## Conclusion

**Reject; reverted.** The likeliest cause is double counting. `tapered_passed_pawns`
already pays for pawn advancement, and it doubles in the endgame (E16). A passed pawn on
the seventh therefore gained a further +55 on top. Changing three pieces' tables at once
also breaks the "one variable at a time" rule in spirit.

Two follow-ups, each its own experiment:
- rook and queen endgame tables without the pawn table (done: E22, +13.9 ± 10.3, accepted);
- tables fitted to game outcomes instead of written by hand (untested).
