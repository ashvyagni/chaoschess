# E5 — Principal variation search in interior nodes

- **Date:** 2026-09-26
- **Change:** `c8576f3`; **base:** `74fcfb6`
- **Status:** **inconclusive, kept as infrastructure** (see Conclusion)

## Hypothesis

Scouting non-first moves with a null window reduces nodes without changing the minimax
value, so it should be neutral to slightly positive in strength.

## Result

Nodes at fixed depth: 0–6% fewer (startpos d8 1,310,156 → 1,287,052; open middlegame
d8 8,323,715 → 7,846,245). The full-width minimax exactness test (depth ≤ 2) still
passes.

Non-regression SPRT (H0 −10, H1 0, α = β = 0.05), 2+0.02, `tools/sprt.sh pvs 74fcfb6`:

| games | W / L / D | Elo (pentanomial 95%) | LLR | verdict |
|---:|---|---|---:|---|
| 1200 (cap) | 306 / 326 / 568 | **−5.8 ± 13.0** | −0.18 | inconclusive |

Pentanomial pairs `[36, 131, 284, 115, 34]`. Record: `matches/sprt-pvs.json`.

## A methodology error found by this test

16 of the 1,200 games were lost on time, 9 by the base and 7 by the new build. The
flag rate was the same for both, so the cause was not in the change. The cause was me: I
ran `cargo test` and release builds on the same 8-core machine while 7 games were running.
Scheduling stalls then exceeded the arena's 100 ms margin.

Control: the same two binaries, 300 games at 2+0.02, concurrency 6, **nothing else
running: 0 time forfeits** (142 mates, 154 repetitions, 3 fifty-move, 1 insufficient
material).

Rule adopted: **no CPU-heavy work while an SPRT is running.** Results from runs that
break it get a note. This one's forfeits were split evenly, so they add noise but no
bias.

## Conclusion

On its own, interior PVS has no measurable effect at this time control. The interval
[−18.8, +7.2] includes zero, and 1,200 games cannot resolve a difference this small.

It is kept, and this is stated plainly: the value of PVS in real engines comes with
reductions. Late-move reductions search reduced moves with exactly these null-window
scouts and re-search on a fail-high. LMR is the next change, and its SPRT against the
null-move commit measures the combination. If LMR is later rejected, E5 should be
revisited: removing interior PVS is a one-line revert.
