# E3 — Remove root-split parallel search

- **Date:** 2026-09-26
- **Baseline commit:** `b091ae7` (measurements from the audit, §G.4)
- **Status:** **rejected** — code removed; replacement is Lazy SMP (roadmap item 9)

## Hypothesis under test

The baseline's `parallel_root` claimed to parallelise search by giving each root move its
own thread. If that design were sound, more threads should mean fewer wall-clock seconds
to a given depth, at a bounded memory cost.

## Baseline measurement (from the audit)

`bench 3` from the start position, Hash = 16 MB:

| Threads | nodes | peak RSS | wall time |
|---:|---:|---:|---:|
| 1 | 10,302 | 18.6 MB | 0.766 s |
| 2 | 529,008 | 285.1 MB | 0.787 s |
| 4 | 529,008 | 307.0 MB | 0.752 s |
| 8 | 529,008 | 195.4 MB | 0.705 s |

And peak RSS against Hash at Threads = 2: 1 MB → 18.7 MB, 8 → 171.0, 16 → 291.1,
64 → 1,219.6 MB — about 19 MB of RSS per MB of Hash.

## Why it fails, mechanically

- It spawned **one thread per legal root move**, ignoring the `Threads` value. That is why
  2, 4 and 8 threads give identical node counts.
- Every thread searched with a full `(-INF, INF)` window and **its own private TT**. Nothing
  learned in one root move's subtree could prune another's, so the design discards the
  alpha-beta pruning *between* root moves. That produced 51× the nodes of one thread for the
  same depth and the same answer.
- Each thread allocated a full `Hash`-sized table. With ~20 root moves, the advertised
  `Hash = 1024` at `Threads = 2` would attempt ~19.5 GB on this 8 GB machine.
- It could not be stopped: threads only checked their own wall-clock budget. That blocks the
  threaded `stop` in roadmap item 2.

## Change

`parallel_root` was deleted. `Threads` is still accepted, so GUIs that send it keep working.
Above 1, the engine prints an `info string` saying search is single-threaded for now. Git
history keeps the removed code.

## Result

`bench 5` from the start position, Hash = 16 MB, after the change:

| Threads | nodes | peak RSS |
|---:|---:|---:|
| 1 | 49,449 | 18.7 MB |
| 2 | 49,449 | 18.7 MB |
| 8 | 49,449 | 18.7 MB |

No speedup was lost, because there never was one. The crash risk is gone.

## Conclusion

**Reject.** This deletes code, but the measurements back it: the feature was slower at
every thread count, used unbounded memory, and blocked `stop`. A real parallel search needs
a shared, lock-free table, and each worker has to learn from the others. That's Lazy SMP,
roadmap item 9, and it will be measured against this single-threaded baseline.
