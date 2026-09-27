# E17 — Chaos vs Classical, and a sign error in the Chaos terms

- **Date:** 2026-09-27
- **Engine:** `ce58776` (all search and eval work through E16)
- **Status:** bug fixed, Chaos sped up 1.43× (identical tree); Chaos now −35 ± 30 vs Classical

## Why this was measured

Every SPRT so far used the default Classical style. Chaos is the project's identity, and
the directive requires it to be a *coherent alternative policy*, not a weaker one. It had
never been measured against Classical.

## First measurement

400 games, 2+0.02, same binary, `Style=Chaos` vs `Style=Classical`:

| | games | W / L / D | score | Elo (pentanomial 95%) |
|---|---:|---|---:|---|
| Chaos vs Classical, `ce58776` | 400 | 15 / 321 / 64 | 11.8% | **−350.3 ± 44.0** |

Speed, startpos `bench 11`: Classical 2.09 M nps, Chaos 0.88 M nps (0.42×). That's a real
cost, since the Chaos terms run several move generations per evaluation. But a 2.4× speed
deficit doesn't plausibly explain −350 Elo.

## The bug

`evaluate_with_style` builds `score` from White's point of view and negates it at the end
when Black is to move. The Chaos terms are side-to-move-relative:

- mobility = legal moves of the side to move minus the opponent's;
- checks, computed the same way;
- centre control, for the side to move only.

They were added *unconverted*. So whenever Black was to move, Chaos rewarded the
**opponent's** mobility and checks: half of every game was evaluated backwards.

A new colour-symmetry test (a position vs its colour mirror must score the same for the
side to move, in every style) failed for Chaos (45 vs 1) and passed for Classical. The fix
converts the terms into White's frame before adding them. The test now passes for both
styles.

## After the fix

| test | games | result |
|---|---:|---|
| SPRT fixed Chaos vs old Chaos (H0 0 / H1 +10) | 142 (decided at 128) | **accept H1**, +178.0 ± 55.9 (early stop, inflated) |
| fixed Chaos vs Classical, fixed length | 400 | **−133.9 ± 30.8** (87 / 234 / 79) |

Records: `matches/E17-*.json`, `matches/sprt-chaos-sign-fix.json`.

## Conclusion

The sign fix is **kept**. Chaos went from −350 to −134 against Classical. It is still far
weaker, and the next suspect is the measured 2.4× speed cost of its evaluation. Making the
Chaos terms cheap without changing their values (identical trees) is the next step, then
re-measure. If a gap remains, the Chaos terms themselves need rethinking: "initiative" as
a coherent, explainable policy (roadmap item 10) rather than raw mobility and check
counts.

## Follow-up: speed (`35903fa`)

The Chaos terms were rewritten to run one move generation per side, with a bitboard
`gives_check`. The tree is identical, and Chaos is 1.43× faster (see the commit message
for the verification). Re-measured on the same terms:

| engine | games | W / L / D | Elo vs Classical (pentanomial 95%) |
|---|---:|---|---|
| `ce58776` (sign bug) | 400 | 15 / 321 / 64 | −350.3 ± 44.0 |
| `0efd21d` (sign fixed) | 400 | 87 / 234 / 79 | −133.9 ± 30.8 |
| **`35903fa` (sign fixed, 1.43× faster)** | 400 | 121 / 161 / 118 | **−34.9 ± 30.2** |

The last step changed only speed, since the tree is identical, and was worth about +99 Elo.
That's more than a 1.43× speedup is usually worth (a rough prior would be +40–50 at this
time control). The uncertainty of the difference between two ±30 estimates is about
±43, so the result sits roughly 1.3σ above that prior. Plausible at 2+0.02, where depth
is scarce, but it is recorded as a measurement, not a rule.

**Where Chaos stands:** close to Classical, but still behind (Wilson interval
[−69, −1]). It still spends about 2.4× Classical's time per node. The directive asks for
Chaos as a coherent, explainable policy of initiative and king pressure; roadmap item 10
is to redesign its terms around that, not just around raw move counts.
