//! Time management: turn a UCI clock into a budget for one move.
//!
//! The audited baseline had none. `go wtime/btime` was not parsed, so under any clock
//! the engine never answered and lost on time (MASTER_ENGINE_AUDIT.md §G.5).
//!
//! The policy here is deliberately simple and conservative. Being clever about time
//! (panic time, stability-based extension, spending more on critical moves) is a
//! strength question that needs matches to settle. Never losing on time is a
//! correctness question, and that is what this module answers.

use std::time::Duration;

/// Assumed number of moves left when the GUI doesn't say (sudden death / increment).
pub const DEFAULT_MOVES_TO_GO: u32 = 30;

/// The side-to-move's clock, as given by `go wtime/btime winc/binc movestogo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub remaining: Duration,
    pub increment: Duration,
    pub moves_to_go: Option<u32>,
    /// Time lost outside the search: process scheduling, pipes, GUI latency.
    pub overhead: Duration,
}

/// How long to think about one move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Stop starting new iterations once half of this has elapsed.
    pub soft: Duration,
    /// Abandon the search outright at this point.
    pub hard: Duration,
}

/// Allocate a budget for this move.
///
/// Guarantees, each covered by a test:
/// - `soft <= hard`;
/// - `hard <= 60%` of the time left after overhead, so one move can never flag;
/// - more time left, more increment, or fewer moves to go never shrinks the budget.
pub fn allocate(clock: Clock) -> Budget {
    let usable = clock
        .remaining
        .saturating_sub(clock.overhead)
        .max(Duration::from_millis(1));
    let moves = clock.moves_to_go.unwrap_or(DEFAULT_MOVES_TO_GO).clamp(1, 50);

    // An equal share of what's left, plus most of the increment, which comes back after
    // this move anyway.
    let share = usable / moves + clock.increment * 3 / 4;
    let soft = share.min(usable * 2 / 5);
    let hard = (soft * 3).min(usable * 3 / 5).max(soft);
    Budget { soft, hard }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn clock(remaining: u64, increment: u64, moves_to_go: Option<u32>) -> Clock {
        Clock {
            remaining: ms(remaining),
            increment: ms(increment),
            moves_to_go,
            overhead: ms(30),
        }
    }

    /// Sweep a grid of clocks, including absurd ones: an increment bigger than the time
    /// left, 1 ms left, one move to go.
    fn grid() -> impl Iterator<Item = Clock> {
        let remaining = [1, 10, 31, 50, 100, 1_000, 10_000, 60_000, 3_600_000];
        let increment = [0, 10, 100, 1_000, 30_000];
        let mtg = [None, Some(1), Some(2), Some(10), Some(40), Some(500)];
        remaining.into_iter().flat_map(move |r| {
            increment
                .into_iter()
                .flat_map(move |i| mtg.into_iter().map(move |m| clock(r, i, m)))
        })
    }

    #[test]
    fn never_budgets_more_than_it_can_afford() {
        for c in grid() {
            let b = allocate(c);
            let usable = c.remaining.saturating_sub(c.overhead).max(ms(1));
            assert!(b.soft <= b.hard, "{c:?}: soft {:?} > hard {:?}", b.soft, b.hard);
            assert!(
                b.hard <= usable * 3 / 5,
                "{c:?}: hard {:?} exceeds 60% of usable {usable:?}",
                b.hard
            );
            assert!(b.soft > Duration::ZERO, "{c:?}: zero budget");
        }
    }

    #[test]
    fn more_resources_never_shrink_the_budget() {
        for c in grid() {
            let b = allocate(c);
            let more_time = allocate(Clock {
                remaining: c.remaining * 2,
                ..c
            });
            let more_inc = allocate(Clock {
                increment: c.increment + ms(500),
                ..c
            });
            assert!(more_time.soft >= b.soft, "{c:?}: doubling time shrank soft");
            assert!(more_inc.soft >= b.soft, "{c:?}: adding increment shrank soft");
            if let Some(m) = c.moves_to_go.filter(|&m| m > 1) {
                let fewer = allocate(Clock {
                    moves_to_go: Some(m - 1),
                    ..c
                });
                assert!(fewer.soft >= b.soft, "{c:?}: fewer moves to go shrank soft");
            }
        }
    }

    #[test]
    fn typical_blitz_budget_is_sensible() {
        // 3+2 blitz, one minute left: roughly 1/30 of the remainder plus most of the
        // increment. Pinned loosely: the point is "about 3.5 s", not an exact number.
        let b = allocate(clock(60_000, 2_000, None));
        assert!(b.soft >= ms(3_000) && b.soft <= ms(4_000), "{b:?}");
        assert!(b.hard >= b.soft && b.hard <= ms(12_000), "{b:?}");
    }
}
