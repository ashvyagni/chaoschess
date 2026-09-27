//! Brute-force forced-mate prover. Deliberately shares no heuristics with the search, so
//! it can validate the tactical suite's expected answers.

use super::*;

/// Exhaustively prove that the side to move can force mate within `moves` moves, and
/// return a move that does so.
///
/// This is deliberately a brute-force prover with **no pruning, no evaluation and no
/// transposition table**, so its answer does not depend on any of the heuristics under
/// test. That is the point: it is used to validate the expected answers in the tactical
/// suite, so the suite cannot be "passed" by an engine bug that the prover shares.
///
/// Cost is exponential in `moves`; it is practical to about `moves == 3`.
pub fn prove_forced_mate(board: &Board, moves: u8) -> Option<ChessMove> {
    if moves == 0 {
        return None;
    }
    MoveGen::new_legal(board).find(|m| is_mated_within(&board.make_move_new(*m), moves))
}

/// True when the side to move is mated within `moves` moves, the opponent having just
/// moved. Every defence must lose, hence `all`.
pub(crate) fn is_mated_within(board: &Board, moves: u8) -> bool {
    match board.status() {
        BoardStatus::Checkmate => true,
        BoardStatus::Stalemate => false,
        BoardStatus::Ongoing => {
            if moves <= 1 {
                return false;
            }
            MoveGen::new_legal(board)
                .all(|d| prove_forced_mate(&board.make_move_new(d), moves - 1).is_some())
        }
    }
}

/// True when playing `m` forces mate within `moves` moves.
pub fn move_forces_mate(board: &Board, m: ChessMove, moves: u8) -> bool {
    MoveGen::new_legal(board).any(|legal| legal == m)
        && is_mated_within(&board.make_move_new(m), moves)
}

/// The shortest forced mate for the side to move, up to `limit` moves, or `None`.
pub fn shortest_forced_mate(board: &Board, limit: u8) -> Option<(u8, ChessMove)> {
    (1..=limit).find_map(|n| prove_forced_mate(board, n).map(|m| (n, m)))
}
