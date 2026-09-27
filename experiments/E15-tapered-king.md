# E15 — Tapered king evaluation

- **Date:** 2026-09-27
- **Base:** `1041462`
- **Status:** **accepted** (SPRT H1, confirmed by a fixed-length match)

## Hypothesis

The evaluation used one king table, the middlegame "stay sheltered in the corner"
table, at every phase of the game. In endings the king has to centralise. Blending a
middlegame and an endgame king table by game phase should gain, possibly a lot, because
the engine was playing every ending with its king in the wrong place.

## Change

`game_phase` = N + B + 2R + 4Q (24 with full material, clamped for promotions).
The king term is `(mg·phase + eg·(24 − phase)) / 24`, with the standard simplified-evaluation
endgame table. Only the king term changed.

Tests: a central king beats a cornered one in a pawn ending; a castled king beats an
exposed one with full material; phase is 24 and 0 at the ends; blending is exact at full
phase. The untapered mutant fails the test.

## Results

| test | games | W / L / D | Elo (pentanomial 95%) | note |
|---|---:|---|---|---|
| SPRT H0 0 / H1 +10 | 116 (decided at 102) | 62 / 16 / 38 | +145.8 ± 53.4 | accept H1; early stop, so inflated |
| **fixed-length confirmation** | **400** | **213 / 84 / 103** | **+116.2 ± 27.7** | openings offset 20; **the estimate to quote** |

Records: `matches/sprt-tapered-king.json`, `matches/E15-confirm.json`.

## Why two numbers

An SPRT stops as soon as the evidence crosses a bound. When the true effect is large, it
tends to cross on a lucky streak, so its point estimate is biased upward. The
fixed-length run has no stopping rule and used different openings. It came out lower
(+116) but still decisive (interval [+88, +144]).

## Conclusion

**Keep.** The largest single evaluation gain so far, and a correctness fix in
substance: the old evaluation actively misplaced the king in every ending. The
mechanism (endings lost or drawn with a cornered king) is the hypothesis this
experiment was built on. It is consistent with the data but not separately
verified game by game.

The other piece-square tables are still untapered, and pawn advancement is weighted
the same at every phase. These are the next candidates, each needing its own test.
