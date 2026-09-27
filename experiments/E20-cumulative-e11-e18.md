# E20 — Cumulative effect of E11–E18

- **Date:** 2026-09-27
- **Engines:** `e5d169f` (current) vs `7030d5a` (the E10 engine)
- **Status:** measured

The changes between them:
- RFP (E11), check extension (E12);
- king-safety fix (E13), passed-pawn fix (E14), tapered king (E15), tapered passers (E16);
- the Chaos changes (E17), which don't affect Classical;
- futility pruning (E18);
- LMP, rejected and reverted (E19);
- pure speedups with identical trees.

400 games, 2+0.02, fixed length, idle machine:

| games | W / L / D | score | Elo (pentanomial 95%) | Wilson 95% |
|---:|---|---:|---|---|
| 400 | 309 / 27 / 64 | 85.2% | **+304.8 ± 41.4** | [+256.9, +352.6] |

Pentanomial `[2, 4, 25, 48, 121]`. Record: `matches/E20-cumulative.json`.

For comparison, the individual estimates:

| experiment | estimate |
|---|---|
| E11 | +14.8 |
| E12 | +10.1 / +18.1 |
| E13 | +31.3 |
| E14 | +34.1 |
| E15 | +116.2 |
| E16 | +18.3 |
| E18 | +73.2 |
| plus two same-tree speedups | — |

Their sum is roughly +300, consistent with the direct measurement, although Elo from
separate tests doesn't simply add.

Chained with E10 (+168 over `c8576f3`), the engine is several hundred Elo stronger than
it was before roadmap item 6. That's relative only: nothing here is anchored to a public
rating list.
