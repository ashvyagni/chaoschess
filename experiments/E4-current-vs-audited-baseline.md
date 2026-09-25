# E4 — Current engine vs the audited baseline: first measured match result

- **Date:** 2026-09-26
- **Engines:**
  - `current-ea6b14a`: the repository at `ea6b14a`, with roadmap items 1–3 applied.
    SHA-256 `d7205eeec096a5b3…`.
  - `baseline-b091ae7`: the code as audited, built with `tools/build_engine_at.sh b091ae7`.
    SHA-256 `d47fdfb4c79b92ac…`.
- **Status:** measured. Records are in `matches/E4-*.json` and PGNs in `matches/E4-*.pgn`.

## Hypothesis

The audit (§F.1, §G.3) predicts the baseline can't play a middlegame under any time
limit. Its quiescence can't finish depth 1 in open positions, so it falls back to
the first legal move in generation order. Its root reports a window bound instead of a
score. The current engine should therefore win almost every game. This is not a
fine-grained strength test. It is the first end-to-end check that the arena works and
that the fixes change play, not just node counts.

## Method

`arena` (roadmap item 5), `openings/standard40.txt` (40 lines, each played with both
colours), Hash 16 MB for both, max 300 plies, concurrency 6, Apple M3, 8 GB.

| condition | games | why |
|---|---:|---|
| `movetime=100` | 100 | equal *time*: what a clock would give |
| `nodes=20000` | 40 | equal *work*: rules out "the new code is just faster" |

## Result

| condition | score | W / D / L | terminations | Elo (Wilson 95%) |
|---|---:|---|---|---|
| movetime 100 ms | **100%** | 100 / 0 / 0 | checkmate 100 | **≥ +566** |
| nodes 20,000 | **100%** | 40 / 0 / 0 | checkmate 40 | **≥ +407** |

Across both conditions, 140 of 140 games were won, every one by checkmate. Games lasted a
median of 31 plies after the book (min 6, max 73).

**Why Wilson.** With every game won, the sample variance is zero. The normal
approximation, trinomial or pentanomial, then gives an infinite point estimate and no
interval. The arena says so instead of printing "± 0", and reports the Wilson score
interval, which is finite at the extremes.

**The games match the audit's mechanism**, not just its prediction. From the PGN
comments (engine output recorded per move):

- The baseline reports **depth 1, score −319.99** on most moves. That's the
  (31999, 32000) window bound from §G.3, not an evaluation.
- On many moves it reports **depth 0**: it didn't finish one ply and played the first
  legal move (`8. a3`, `11. c3` in round 2).
- In round 2 it played `4. Bxf7+??` straight out of the Italian book, against an opponent
  searching depth 4–5 in the same 100 ms.

## Conclusion

The roadmap-1–3 engine is stronger than the audited baseline by at least ~400–570 Elo at
95% confidence, under both equal time and equal work. That is only the lower bound the
data supports. The real gap is probably larger, since the baseline in effect doesn't play
chess once the opening ends.

## What this does not show

- **Nothing about absolute strength.** Both engines are unrated. There's no anchor to any
  public rating list, because no reference engines are vendored here (audit §P, "what
  cannot be done locally"). "+566 against the baseline" is not a rating.
- **Not which fix mattered most.** Items 1–3 were measured separately by node counts
  (E1, E2), but not in games. A per-change SPRT against the previous commit is the tool
  for that from now on.
- **Independence.** The Wilson interval treats games as independent. Paired games
  share an opening, which matters for close results, not a 140–0 sweep.

## Reproduce

```bash
cargo build --release
BASE=$(tools/build_engine_at.sh b091ae7)
target/release/arena --engine name=current,cmd=target/release/crazy-chess,opt.Hash=16 \
    --engine name=baseline,cmd=$BASE,opt.Hash=16 \
    --tc movetime=100 --games 100 --concurrency 6 --max-plies 300
```
