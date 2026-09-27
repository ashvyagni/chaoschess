//! A position with the game history the draw rules need, and the draw rules that
//! depend only on the board.

use super::*;

/// A position to search from, with the game history the draw rules need.
///
/// The `chess` crate's `Board` has no halfmove clock and no memory of earlier positions,
/// so on its own it cannot see threefold repetition or the fifty-move rule. The audited
/// engine searched bare boards and had neither (MASTER_ENGINE_AUDIT.md §G.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub board: Board,
    /// Zobrist hashes of earlier positions since the last irreversible move, oldest first.
    /// Older positions cannot recur, so they are not kept.
    pub prior: Vec<u64>,
    /// Plies since the last capture or pawn move.
    pub halfmove_clock: u32,
}

impl Position {
    pub fn new(board: Board) -> Self {
        Self {
            board,
            prior: Vec::new(),
            halfmove_clock: 0,
        }
    }

    pub fn with_clock(board: Board, halfmove_clock: u32) -> Self {
        Self {
            board,
            prior: Vec::new(),
            halfmove_clock,
        }
    }

    /// Play a move that is already known to be legal.
    pub fn play(&mut self, m: ChessMove) {
        if resets_clock(&self.board, m) {
            self.prior.clear();
            self.halfmove_clock = 0;
        } else {
            self.prior.push(self.board.get_hash());
            self.halfmove_clock += 1;
        }
        self.board = self.board.make_move_new(m);
    }
}

/// Captures and pawn moves are irreversible: they reset the fifty-move clock, and no
/// position before them can ever recur.
pub(crate) fn resets_clock(board: &Board, m: ChessMove) -> bool {
    board.piece_on(m.get_source()) == Some(Piece::Pawn) || is_capture(board, m)
}

/// Neither side can possibly checkmate: bare kings, a single minor piece, or only bishops
/// that all stand on squares of one colour. (Two knights against a bare king cannot force
/// mate, but a mate is still *possible*, so by the rules it is not a dead position.)
pub fn insufficient_material(board: &Board) -> bool {
    let heavy = *board.pieces(Piece::Pawn) | *board.pieces(Piece::Rook) | *board.pieces(Piece::Queen);
    if heavy != chess::EMPTY {
        return false;
    }
    let knights = *board.pieces(Piece::Knight);
    let bishops = *board.pieces(Piece::Bishop);
    let minors = (knights | bishops).popcnt();
    if minors <= 1 {
        return true;
    }
    const LIGHT_SQUARES: u64 = 0x55AA_55AA_55AA_55AA;
    let on_light = (bishops & BitBoard::new(LIGHT_SQUARES)).popcnt();
    knights == chess::EMPTY && (on_light == 0 || on_light == bishops.popcnt())
}
