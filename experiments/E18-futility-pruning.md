# E18 — Futility pruning

- **Date:** 2026-09-27
- **Base:** `84e8995`
- **Status:** **accepted** (SPRT H1, confirmed by a fixed-length match)

## Change

At non-PV nodes not in check, with depth ≤ 2, a quiet move (not the first, not a capture,
promotion or check) is skipped when static eval + 150·depth ≤ alpha. Mate-range alphas are
excluded. A pruned move still contributes the margin as a fail-soft upper bound, so a node
where everything is pruned returns that bound instead of −INF. The static eval is the one
already computed for RFP and null move.

## Results

| test | games | W / L / D | Elo (pentanomial 95%) |
|---|---:|---|---|
| SPRT H0 0 / H1 +10 (decided at 462) | 476 | 191 / 122 / 163 | +50.7 ± 24.6, accept H1 |
| **fixed-length confirmation**, openings offset 20 | **400** | **180 / 97 / 123** | **+73.2 ± 28.5** |

Records: `matches/sprt-futility.json`, `matches/E18-confirm.json`.

Unlike E15, the confirmation came out *higher* than the SPRT. Early-stop inflation is a
tendency, not a rule. Both runs agree that the gain is large and real. The fixed-length
figure is quoted, per the practice in docs/TOURNAMENTS.md.

## Conclusion

**Keep.** The margin (150 cp/ply) and depth limit (2) are conventional and untuned.
