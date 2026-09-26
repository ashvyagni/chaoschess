# Roadmap

The prioritised plan from `MASTER_ENGINE_AUDIT.md` §P, with live status. The order follows
the dependency analysis: each item unblocks the next measurement. Status says what is
*done and tested*, not what was started.

| # | item | status | evidence |
|---|---|---|---|
| 0 | version control, baseline, audit | **done** | `MASTER_ENGINE_AUDIT.md`, `benchmarks/baseline-*.json` |
| 1 | search correctness: quiescence, TT, root PVS | **done** | E1, E2; depth-4 startpos 182 s → 0.02 s |
| 2 | UCI: clock, `stop`, PV, mate scores | **done** | UCI conformance 4/8 → 8/8; `tests/uci.rs` |
| 3 | correctness floor: rules, draw detection, fuzzing, GUI rules | **done** | 7 Python generator bugs fixed; crate UB guarded; `tests/fuzz.rs` |
| 4 | tactical + benchmark suites | **partial** | 22 proven mates; fixed-depth benchmark JSON. *Missing:* non-mate tactics and positional suites |
| 5 | tournament driver, SPRT, Elo | **done** | `arena`, `stats`, `tools/sprt.sh`; E4 (140/140 vs baseline) |
| 6 | search strength, each SPRT-gated | **in progress** | null-move +25.5 Elo (E6); PVS inconclusive (E5); LMR next |
| 7 | split `lib.rs`; `Evaluator` trait; tapered eval; passed-pawn fix | planned | — |
| 8 | own move generator with incremental make/unmake | planned | prerequisite for NNUE accumulators |
| 9 | Lazy SMP over a shared lock-free TT | planned | root split removed as harmful (E3) |
| 10 | personality layer: Chaos as a parameterised, explainable policy | planned | — |
| 11 | opening book; optional Syzygy probing | planned | needs external data (tablebases ~150 GB for 6-man) |
| 12 | self-play data → NNUE training → hybrid eval | planned | compute-bound on this machine (8 GB, no GPU) |
| 13 | GUI: Rust-authoritative, off-thread, live telemetry | partial | GUI sends history and ends games by rule; still on the Qt main thread |
| 14 | profiling-driven optimisation; packaging | ongoing | `74fcfb6`: ordering keys cached, 1.2–1.9× |

## Item 6: search techniques

Each technique is implemented alone, checked for correctness (unit, tactics and exactness
tests), measured by nodes at fixed depth, then SPRT'd against the previous commit
(`tools/sprt.sh`). It is kept only on a positive or non-regression result.

| technique | status |
|---|---|
| interior PVS | inconclusive (−5.8 ± 13.0); kept as LMR infrastructure (E5) |
| null-move pruning | **accepted**: +25.5 ± 18.2 Elo (E6) |
| late move reductions | next; needs PVS |
| move ordering: good captures > killers > history > bad captures | planned |
| check extension | planned |
| reverse futility / futility / late move pruning | planned |
| mate distance pruning, IIR | planned |
| singular extensions | planned; needs TT move + reliable depth |

## Blocked on external resources

Stated plainly rather than faked:

- **Absolute Elo** needs reference engines at known ratings. None are vendored.
- **Syzygy** needs the tablebase files (~150 GB for up to 6 pieces).
- **NNUE at useful strength** needs a large labelled dataset and GPU time. Locally,
  distillation from our own search is possible but will be small-scale.
- **Long time controls** make SPRTs slow on one laptop; fast-TC results may not transfer.
