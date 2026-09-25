//! Verified test suites.
//!
//! Every entry here is **machine-verified**, not copied from memory or from an
//! unattributed list. The mate suite is checked by [`crate::prove_forced_mate`], a
//! brute-force prover that shares no heuristics with the search under test, so a position
//! cannot carry a wrong expected answer and a search bug cannot make a position look
//! solved.
//!
//! Positions were classified with `cargo run --release --bin classify_mates -- 3`.
//! Candidates the prover rejected were discarded rather than guessed at — including one
//! back-rank position I expected to be mate in 1, which is not, because a pawn on the
//! seventh rank promotes into the checking line and blocks it.

/// `(fen, mate_in_moves)` — the side to move has a forced mate in exactly this many moves.
///
/// "Moves" not plies: mate in 1 is one move by the side to move; mate in 2 is
/// move, reply, mate.
pub const MATE_SUITE: &[(&str, u8)] = &[
    // --- back rank ---
    ("6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1", 1),
    ("7k/6pp/8/8/8/8/6PP/5R1K w - - 0 1", 1),
    ("6k1/5ppp/8/8/8/8/5PPP/1Q4K1 w - - 0 1", 1),
    ("6k1/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1", 1),
    ("6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1", 1),
    ("r5k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1", 1),
    ("2r3k1/5ppp/8/8/8/8/5PPP/2R3K1 w - - 0 1", 1),
    // --- rook and king against king ---
    ("k7/8/1K6/8/8/8/8/7R w - - 0 1", 1),
    ("3k4/8/3K4/8/8/8/8/7R w - - 0 1", 1),
    ("4k3/8/4K3/8/8/8/8/7R w - - 0 1", 1),
    ("1k6/8/1K6/8/8/8/8/7R w - - 0 1", 1),
    ("k7/2K5/8/8/8/8/8/7R w - - 0 1", 1),
    ("2k5/8/1K6/8/8/8/8/7R w - - 0 1", 2),
    // --- two rooks ---
    ("7k/8/8/8/8/8/6R1/6RK w - - 0 1", 1),
    ("6k1/8/8/8/8/8/R7/1R5K w - - 0 1", 2),
    ("7k/8/8/8/8/8/R7/1R5K w - - 0 1", 2),
    // --- queen ---
    ("7k/5Q2/6K1/8/8/8/8/8 w - - 0 1", 1),
    ("4k3/8/4KQ2/8/8/8/8/8 w - - 0 1", 1),
    // --- knight, with the queen covering the escape ---
    ("6rk/6pp/8/6N1/8/8/8/K5Q1 w - - 0 1", 1),
    ("5rk1/5ppp/8/6N1/8/8/8/K6Q w - - 0 1", 1),
    // --- rook sacrifice to open the mating net ---
    ("r5rk/5p1p/5R2/4Q3/8/8/7P/7K w - - 0 1", 2),
    // --- black to move, so the suite is not one-sided ---
    ("r5k1/5ppp/8/8/8/8/5PPP/6K1 b - - 0 1", 1),
];
