# Tournaments, Elo and SPRT

How strength is measured in this project, and the rules for what may be claimed from a
measurement.

## Rules for claims

1. **No Elo without an interval, a sample size, a time control and the opponents.** A bare
   number is not a result.
2. **Relative, not absolute.** Every Elo figure here is a *difference* between two named
   binaries. Nothing in this repository is anchored to a public rating list, because no
   reference engines are vendored (see `MASTER_ENGINE_AUDIT.md` §P). "+566 against the
   baseline" is not a rating.
3. **Changes are accepted by SPRT against the previous commit**, not by "it felt stronger"
   and not by node counts. Node counts measure efficiency. Games measure strength.
4. **Failed and inconclusive results are recorded** in `experiments/`, like successes.

## The arena

`src/bin/arena` plays two engines against each other over real UCI, each as a separate
process spoken to exactly as a GUI would.

```bash
cargo build --release
target/release/arena \
    --engine name=new,cmd=target/release/crazy-chess,opt.Hash=16 \
    --engine name=old,cmd=$(tools/build_engine_at.sh <commit>),opt.Hash=16 \
    --tc 2+0.02 --games 1000 --concurrency 7 \
    --sprt elo0=0,elo1=10,alpha=0.05,beta=0.05
```

| flag | meaning |
|---|---|
| `--engine name=,cmd=[,arg=][,opt.NAME=VALUE]` | exactly two; options are sent with `setoption` |
| `--tc` | `nodes=N` · `depth=D` · `movetime=MS` · `BASE+INC` (seconds) |
| `--games` | rounded up to whole pairs |
| `--openings` | default `openings/standard40.txt` |
| `--concurrency` | parallel games; default half the logical CPUs |
| `--sprt` | early stop when the test decides |
| `--max-plies` | draw adjudication after this many plies from the book |
| `--pgn`, `--json` | output paths; default `matches/<time>-<a>-vs-<b>.*` |

### What the arena guarantees

- **Nothing an engine says is trusted.** Every `bestmove` is checked against the legal
  move list. Every reply has a deadline.
- **Games end by the rules:** checkmate, stalemate, threefold repetition, fifty-move rule,
  insufficient material. An engine's claims about the result are ignored.
- **Failures lose, with the reason recorded:** illegal move, stall, crash, protocol error,
  time forfeit. A time forfeit against a side with no mating material is a draw
  (FIDE 6.9).
- **Each opening is played twice with colours swapped.** Workers take whole pairs, so
  the pentanomial statistics stay valid under concurrency.
- **Fresh engine processes per game.** Nothing leaks from one game into the next.

All of this is tested against deliberately broken engines in `tests/arena.rs`.

### The JSON record

Enough to reproduce and audit a result:

- the SHA-256 of both engine binaries, their commands, arguments and options;
- the opening file's path, SHA-256 and offset;
- the time control, concurrency and max plies;
- OS, architecture and logical CPU count;
- the repository commit and the UTC date;
- W/D/L, pentanomial counts and variance;
- Elo from three models with 95% intervals, and LOS;
- SPRT parameters, LLR and verdict;
- a termination histogram and a per-game list.

## Statistics (`src/stats.rs`)

- **Model:** logistic, `E(d) = 1 / (1 + 10^(-d/400))`.
- **Trinomial:** games are independent W/D/L outcomes.
- **Pentanomial (primary):** the unit is a pair of games on the same opening with colours
  swapped, scored 0, ½, 1, 1½ or 2. Paired games are correlated through the opening, so
  this is the honest model for paired matches. It is usually tighter than trinomial,
  because the opening's bias cancels within a pair.
- **Wilson score interval:** finite even when one side wins every game. The normal
  approximation then has zero variance and no interval.
- **GSPRT** (after Van den Bergh): `LLR ≈ n·(s1−s0)·(2s̄−s0−s1)/(2σ̂²)`, with bounds
  `ln(β/(1−α))` and `ln((1−β)/α)`.

The module checks itself against its own claims. In simulation, the SPRT must accept a
+40 engine and reject an equal one in at least 18 of 20 runs, and the 95% interval must
cover the true Elo in 180–198 of 200 simulated matches.

## The standard gate

```bash
tools/sprt.sh <name> <base-commit> [tc=2+0.02] [elo0=0] [elo1=10] [max-games=4000]
```

It builds the working tree and the base commit and copies both binaries before the first
game, so a rebuild mid-test can't change what is being tested. Results go to
`matches/sprt-<name>.json`.

| kind of change | hypotheses | reasoning |
|---|---|---|
| should gain strength | `elo0=0 elo1=10` | stop quickly on a real gain |
| should be neutral (refactor, speed-only) | `elo0=-10 elo1=0` | a non-regression test: "not worse" |
| large expected gain (null move, LMR) | `elo0=0 elo1=15..20` | fewer games to decide |

## Results so far

| experiment | engines | TC | games | result |
|---|---|---|---:|---|
| E4 | current `ea6b14a` vs audited baseline `b091ae7` | movetime 100 | 100 | +100 −0 =0, Elo ≥ +566 (Wilson 95%) |
| E4 | same | nodes 20,000 | 40 | +40 −0 =0, Elo ≥ +407 (Wilson 95%) |
| E5 | interior PVS `c8576f3` vs `74fcfb6` | 2+0.02 | 1200 | −5.8 ± 13.0, inconclusive (non-regression SPRT, LLR −0.18) |
| E6 | null-move pruning `63b3bce` vs `c8576f3` | 2+0.02 | 656 | **+25.5 ± 18.2, SPRT accepted H1** (LOS 99.6%) |
| E7 | late move reductions `9d4355f` vs `63b3bce` | 2+0.02 | 958 | −6.2 ± 16.3, **SPRT accepted H0 → reverted** |
| E8 | move ordering (killers, SEE bands) `e8b05bd` vs `42eae59` | 2+0.02 | 244 | **+76.7 ± 30.5, SPRT accepted H1** |
| E9 | LMR retry (same code as E7) `7030d5a` vs `8db5d50` | 2+0.02 | 818 | **+30.7 ± 17.4, SPRT accepted H1** |
| E10 | cumulative: `7030d5a` vs `c8576f3` | 2+0.02 | 400 | **+168.4 ± 27.9** (fixed length) |
| E11 | reverse futility pruning vs `6ecca19` | 2+0.02 | 1902 | **+14.8 ± 11.0, SPRT accepted H1**; Chaos finds 2 ladder mates 2 plies later |
| E12 | check extension vs `e326f65` | 2+0.02 | 2000 + 730 | +10.1 ± 10.8 (undecided at cap); non-regression SPRT then **accepted "not worse"** (+18.1 ± 17.6) |
| E13 | king-safety pressure from attack maps vs `507da0b` | 2+0.02 | 390 | non-regression **accepted** at 376 games; +31.3 ± 23.0 |
| E14 | passed pawns look only ahead vs `b794a98` | 2+0.02 | 440 | non-regression **accepted** at 426 games; +34.1 ± 22.3 |
| E15 | tapered king vs `1041462` | 2+0.02 | 116 SPRT + 400 fixed | SPRT **accepted** (+145.8, early-stop inflated); fixed-length **+116.2 ± 27.7** |
| E16 | tapered passed pawns vs `8a45f82` | 2+0.02 | 1614 | **+18.3 ± 13.1, SPRT accepted H1** |
| E17 | Chaos vs Classical at `ce58776` | 2+0.02 | 400 | −350.3 ± 44.0 → sign bug found |
| E17 | Chaos sign fix vs old Chaos | 2+0.02 | 142 | **SPRT accepted H1** (+178, early-stop inflated) |
| E17 | fixed Chaos vs Classical | 2+0.02 | 400 | −133.9 ± 30.8 |
| E17 | Chaos 1.43× faster (identical tree) vs Classical | 2+0.02 | 400 | **−34.9 ± 30.2** |
| E18 | futility pruning vs `84e8995` | 2+0.02 | 476 SPRT + 400 fixed | SPRT **accepted** (+50.7); fixed-length **+73.2 ± 28.5** |

More rows are added as SPRTs finish. The full records are in `matches/`.

## Early-stop bias

An SPRT that stops early on a large effect overestimates it. For large accepted gains, a
fixed-length confirmation on different openings gives the number to quote. In E15 that
was +146 by SPRT and +116 fixed-length.

## Operating rule

Run nothing CPU-heavy while a match is running. E5 found 16 time forfeits caused by
builds running alongside the match. The same binaries on an idle machine: 0 in 300 games.

## Limits of this setup

One 8-core laptop, so fast time controls. Results at 2+0.02 need not transfer to long
games, especially for time-management changes. No anchor engines. Opening lines are
common theory, not engine-balanced; balance comes from the colour-swapped pairing.
