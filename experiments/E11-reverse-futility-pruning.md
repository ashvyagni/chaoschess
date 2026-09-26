# E11 — Reverse futility pruning

- **Date:** 2026-09-27
- **Base:** `6ecca19` (null move + ordering + LMR)
- **Status:** **accepted** (SPRT H1), **with a known tactical cost**

## Change

At non-PV nodes not in check, with depth ≤ 6: if static eval − 90·depth ≥ beta, return
the static eval without searching. Mate-range betas are excluded. The static eval is now
computed once per node and shared with null-move pruning; that sharing doesn't change
null-move behaviour.

## Result

SPRT, H0 0 vs H1 +10, 2+0.02, idle machine:

| games | W / L / D | Elo (pentanomial 95%) | LOS | LLR | verdict |
|---:|---|---|---:|---:|---|
| 1902 | 549 / 468 / 885 | **+14.8 ± 11.0** | 99.4% | +3.15 | **accept H1** |

Record: `matches/sprt-rfp.json`. Fixed-depth nodes were mixed (kiwi-variant 2.6× fewer,
startpos 0.85×, i.e. more).

## Known cost: mates found later

Chaos no longer finds the two-rook ladder mates-in-2 (`6k1/8/8/8/8/8/R7/1R5K w`,
`7k/…`) at depth 4, the minimal depth. It finds them at depth 6. Classical still finds
them at depth 4.

Mechanism: after the quiet mating preparation `Rb7`, the defender's static eval at depth 1
looks better than in the alternatives. With Chaos's extra evaluation terms it clears the
90 cp margin, so RFP returns that score before quiescence can reach `Ra8#`.

The Chaos mate-suite test now allows two extra plies (`CHAOS_SELECTIVITY_ALLOWANCE`).
The Classical test stays strict. The trade was accepted on the SPRT result. It shows up
in the test file, not hidden in it.

**Candidate follow-ups**, each needing its own SPRT: a smaller or eval-scaled margin;
skipping RFP at depth 1; RFP that respects mate threats.

## A probe error along the way

While diagnosing this I first reported that the engine "never sees the mate even at
depth 8". That was wrong. My shell probe piped `quit` directly after `go`, and UCI `quit`
ends a running search, so every "depth 8" result was really depth 1. The corrected probe
(waiting for `bestmove`) gave the numbers above.
