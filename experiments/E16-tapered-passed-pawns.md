# E16 — Tapered passed-pawn bonus

- **Date:** 2026-09-27
- **Base:** `8a45f82` (tapered king)
- **Status:** **accepted** (SPRT H1)

## Change

The passed-pawn bonus is blended by game phase: the existing bonus in the middlegame,
twice that with no pieces left (`tapered_passed_pawns`). Tested at both ends and in the
middle.

## Result

SPRT, H0 0 vs H1 +10, 2+0.02, idle machine:

| decided at | games | W / L / D | Elo (pentanomial 95%) | LOS | verdict |
|---:|---:|---|---|---:|---|
| 1600 (LLR +3.01) | 1614 | 565 / 480 / 569 | **+18.3 ± 13.1** | 99.6% | **accept H1** |

Record: `matches/sprt-tapered-passers.json`. The decision came late, at game 1600, so
early-stop inflation is small here, unlike E15. No separate confirmation run was needed.

## Conclusion

**Keep.** A smaller gain than the tapered king, as expected: passed pawns were already
scored, just with a phase-independent weight. The 2× endgame factor is a conventional
first guess and untuned.
