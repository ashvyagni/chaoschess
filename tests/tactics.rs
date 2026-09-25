//! Tactical regression suite.
//!
//! Two independent assertions per position:
//!
//! 1. The **expected answer is verified** by the brute-force prover, so the suite's data
//!    cannot silently rot and cannot be wrong in a way the search shares.
//! 2. The **search must find it** within a depth that a correct engine should need.
//!
//! Assertion 1 is what makes assertion 2 meaningful. Without it, "the engine solves 22/22"
//! would only mean "the engine agrees with whatever was typed into the table".

use chess::Board;
use crazy_chess::suites::MATE_SUITE;
use crazy_chess::{move_forces_mate, search, shortest_forced_mate, SearchLimits, Style};
use std::str::FromStr;

/// Mate in `n` moves is `2n - 1` plies. One extra ply of slack keeps the test about
/// "can the engine see it" rather than about exact horizon arithmetic.
fn depth_for(mate_in: u8) -> u8 {
    2 * mate_in
}

#[test]
fn suite_expected_answers_are_provably_correct() {
    for &(fen, mate_in) in MATE_SUITE {
        let board = Board::from_str(fen).unwrap_or_else(|e| panic!("{fen}: bad FEN: {e:?}"));
        let proven = shortest_forced_mate(&board, mate_in)
            .unwrap_or_else(|| panic!("{fen}: no forced mate within {mate_in} move(s) exists"));
        assert_eq!(
            proven.0, mate_in,
            "{fen}: shortest forced mate is {} move(s), suite claims {mate_in}",
            proven.0
        );
    }
}

#[test]
fn search_solves_the_mate_suite() {
    let mut failures = Vec::new();
    for &(fen, mate_in) in MATE_SUITE {
        let board = Board::from_str(fen).unwrap();
        let limits = SearchLimits {
            depth: depth_for(mate_in),
            ..Default::default()
        };
        let result = search(&board, limits).expect("a legal move exists");
        if !move_forces_mate(&board, result.best_move, mate_in) {
            failures.push(format!(
                "  {fen}\n    mate in {mate_in}, engine played {} (depth {}, {} nodes)",
                result.best_move, result.depth, result.nodes
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}/{} mate positions unsolved:\n{}",
        failures.len(),
        MATE_SUITE.len(),
        failures.join("\n")
    );
}

/// The Chaos personality must be a different *policy*, not a broken one: it is still
/// required to find forced mates. A personality that misses mate in 1 is a bug, not a
/// style.
#[test]
fn chaos_style_also_solves_the_mate_suite() {
    let mut failures = Vec::new();
    for &(fen, mate_in) in MATE_SUITE {
        let board = Board::from_str(fen).unwrap();
        let limits = SearchLimits {
            depth: depth_for(mate_in),
            style: Style::Chaos,
            ..Default::default()
        };
        let result = search(&board, limits).expect("a legal move exists");
        if !move_forces_mate(&board, result.best_move, mate_in) {
            failures.push(format!("  {fen} (mate in {mate_in}): played {}", result.best_move));
        }
    }
    assert!(
        failures.is_empty(),
        "Chaos failed {}/{}:\n{}",
        failures.len(),
        MATE_SUITE.len(),
        failures.join("\n")
    );
}
