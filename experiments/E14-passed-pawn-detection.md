# E14 — Passed-pawn detection looks only ahead

- **Date:** 2026-09-27
- **Base:** `b794a98`
- **Status:** **accepted** (pre-registered non-regression SPRT; the effect size suggests a gain)

## Motivation: audit §G.8

A pawn counted as passed only if no enemy pawn stood anywhere on its own or an adjacent
file, including behind it, where it can never stop the pawn. The fix uses forward spans:
enemy pawns on those files and *ahead* of the pawn. It also iterates the pawn bitboard,
and no longer takes the file counts as input.

## Tests

`passed_pawns_ignore_enemy_pawns_behind` uses positions where exactly one pawn is passed,
so the expected values are exact (42, 0, ±34), plus a colour-symmetry check. The first
version of this test used a position in which two pawns were passed and the bonuses
cancelled; I had written vague assertions around that instead of fixing the position. It
was replaced before commit. Mutation check: restoring the audited "any pawn on the files"
logic fails the test.

## SPRT (stated before running)

Non-regression, H0 −10 vs H1 0, 2+0.02, idle machine:

| decided at | games incl. overshoot | W / L / D | Elo (pentanomial 95%) | LOS | verdict |
|---:|---:|---|---|---:|---|
| game 426 (LLR +2.98) | 440 | 144 / 101 / 195 | **+34.1 ± 22.3** | 99.7% | **accept "not worse"** |

Record: `matches/sprt-passed-pawns.json`.

## Conclusion

**Keep.** As with E13, only non-regression was tested. The interval [+11.8, +56.4]
excludes zero, which is consistent with fixing a term that misjudged the most important
endgame asset.
