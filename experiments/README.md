# Experiments

One file per experiment, named `E<n>-<slug>.md`. Every file records, in this order:

1. **Hypothesis** — what should improve, and why, stated before the measurement.
2. **Baseline** — the measured "before", with the exact command and commit.
3. **Implementation** — what changed.
4. **Result** — the measured "after", same command, same conditions.
5. **Conclusion** — keep / modify / reject, and what happens to the code.

Rules:

- **Failed experiments stay.** A negative result is a result; deleting it means the next
  person repeats the work. Reject an experiment by writing the conclusion, not by
  removing the file.
- **One variable at a time.** If two things change, the measurement attributes nothing.
- **Fixed nodes or fixed depth for comparability**, not wall-clock budgets, unless the
  experiment is specifically about time management.
- **State the confidence.** A single-position node count is evidence about that position.
  A strength claim needs a match with a sample size and an interval (see
  `MASTER_ENGINE_AUDIT.md` §L).
