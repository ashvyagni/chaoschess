# E1 — Quiescence search recurses on all quiet checking moves

- **Date:** 2026-09-26
- **Commit under test:** `b091ae7` (baseline)
- **Status:** diagnosis confirmed; real fix implemented in `022559d` (see Follow-up result)

## Hypothesis

`quiescence` (`src/lib.rs:334`) only skips a move when it is *neither* a capture *nor* a
check:

```rust
if !in_check && !is_capture && board.make_move_new(m).checkers() == &chess::EMPTY {
    continue;
}
```

So it searches **every quiet checking move**, recursively, bounded only by
`MAX_QUIESCENCE_PLY = 32`. Quiet checks generate further quiet checks, so each leaf of
the main search carries an exponential sub-tree.

**Prediction:** this, not evaluation cost, dominates the node count. Restricting
quiescence to captures and promotions should reduce nodes by orders of magnitude, and the
reduction should grow sharply with depth (more leaves) and with position openness (more
checks available).

## Baseline

`cargo build --release`, then `printf "position startpos\nbench D\nquit\n" | target/release/crazy-chess`,
Classical, 1 thread, Hash=16 MB:

| depth | nodes | wall time |
|---:|---:|---:|
| 1 | 50 | — |
| 2 | 1,066 | 0.043 s |
| 3 | 10,302 | 0.049 s |
| 4 | 95,173,917 | 182.523 s |
| 5 | not reached | aborted at 600 s |

Other positions, same build:

| position | depth | result |
|---|---:|---|
| CPW position 3 | 2 | 118,876,593 nodes |
| kiwipete | 1 | did not finish in 600 s |
| Italian Game | 1 | did not finish in 600 s |

## Implementation

Single change — drop the check clause so quiescence considers captures and promotions
only:

```rust
 let is_capture = board.piece_on(m.get_dest()).is_some() || m.get_promotion().is_some();
-if !in_check && !is_capture && board.make_move_new(m).checkers() == &chess::EMPTY {
+if !in_check && !is_capture {
     continue;
 }
```

Nothing else was touched. Evasions while `in_check` are still fully searched.

## Result

| depth | baseline nodes | E1 nodes | node reduction | baseline time | E1 time | speedup |
|---:|---:|---:|---:|---:|---:|---:|
| 3 | 10,302 | 9,424 | 1.1× | 0.049 s | — | — |
| 4 | 95,173,917 | 30,464 | **3,124×** | 182.523 s | 0.077 s | **2,370×** |
| 5 | not reached | 101,624 | — | >600 s | 0.181 s | — |
| 6 | not reached | 278,464 | — | >600 s | 0.472 s | — |

Best moves under E1 are sensible opening moves (d4 at depth 6, e4 at depth 5), and the
effective branching factor becomes ~2.7 instead of 9,238.

The prediction is confirmed, including its shape: the reduction is negligible at depth 3
(1.1×) and explosive at depth 4 (3,124×), exactly as expected from an exponential
sub-tree per leaf.

## Conclusion

**Diagnosis: confirmed and accepted. This specific patch: rejected as the final fix.**

The measurement proves the root cause, but captures-only quiescence is not obviously the
strongest choice. Searching checks in quiescence is legitimate and standard — it finds
short forced mates and tactics that a captures-only quiescence misses — the defect is
that it is **unbounded**, not that it exists.

The correct fix, to be implemented and measured separately:

1. Allow checking moves only in the **first 1–2 quiescence plies**, not all 32.
2. Add **SEE-based pruning** so losing captures are not searched either.
3. Keep full evasion search when in check.

That fix cannot be *chosen* on node counts alone, because the tradeoff is nodes against
tactical accuracy. It requires the tactical regression suite (audit §K, roadmap item 4),
which does not exist yet. Both variants will be run against it.

E1 was therefore reverted (`git checkout -- src/lib.rs`) so the committed baseline stays
the true, measured "before" state. The baseline is preserved in
`benchmarks/baseline-2026-09-26.json`.

**Follow-up:** roadmap item 1.

## Follow-up result (implemented)

The fix described above was implemented in `022559d`: quiet checks only for the first
`QS_CHECK_PLIES = 2` quiescence plies, SEE pruning of losing captures, full evasions in
check. Both knobs are exposed on `SearchLimits` so they can be swept.

**Tactical sweep** — 22 machine-proven mates (`cargo run --release --bin tactics -- --sweep`),
depth `2 × mate_in`, 10 s cap per position:

| configuration | solved | nodes | time | hit cap |
|---|---:|---:|---:|---:|
| baseline: unbounded checks, no SEE | **19/22** | 138,462,412 | 86.7 s | 8 |
| captures only, SEE on | 22/22 | 16,165 | 0.03 s | 0 |
| checks ≤ 1, SEE on | 22/22 | 25,547 | 0.04 s | 0 |
| checks ≤ 2, SEE on *(default)* | 22/22 | 27,679 | 0.05 s | 0 |
| checks ≤ 4, SEE on | 22/22 | 68,505 | 0.07 s | 0 |
| checks ≤ 2, SEE off | 22/22 | 27,949 | 0.04 s | 0 |

**What this shows.** The unbounded behaviour was not a speed-for-tactics trade: under a time
cap it *lost* three mates-in-2, because it could not finish depth 4. Bounding is a strict
improvement on this suite.

**What this does not show.** The suite cannot discriminate between 0, 1, 2 and 4 check
plies — all solve 22/22. The default of 2 is therefore a conventional choice that this data
neither supports nor contradicts; it costs 1.7× the nodes of captures-only here. Deciding it
needs positions where the mating move lies *beyond* the main-search horizon and must be found
by quiescence checks. Queued as an open question.

**Conclusion: keep** bounded checks + SEE. **Open:** the value of `QS_CHECK_PLIES`.
