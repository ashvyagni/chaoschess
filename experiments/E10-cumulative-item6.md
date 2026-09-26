# E10 — Cumulative effect of the accepted search changes

- **Date:** 2026-09-26
- **Engines:** `7030d5a` (null move + ordering + LMR) vs `c8576f3` (before any of them)
- **Status:** measured

SPRT gains are measured one step at a time and don't simply add. This is the direct
measurement: 400 games, 2+0.02, fixed length (no early stop), idle machine.

| games | W / L / D | score | Elo (pentanomial 95%) | Wilson 95% |
|---:|---|---:|---|---|
| 400 | 215 / 35 / 150 | 72.5% | **+168.4 ± 27.9** | [+130.4, +206.5] |

Pentanomial `[3, 10, 48, 82, 57]`. Record: `matches/E10-cumulative-item6.json`.

The sum of the individual SPRT estimates (E6 +25.5, E8 +76.7, E9 +30.7) is +132.9.
The direct measurement is larger. Given the widths of the intervals, that difference
could be noise. It is also what the known interaction predicts: LMR only paid off once
the ordering was fixed, so the combination is worth more than LMR measured alone
against a base that already had the ordering.

Still relative strength only. These engines aren't anchored to any rating list.
