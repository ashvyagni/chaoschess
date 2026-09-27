//! Static evaluation: material, piece-square tables, pawn structure, passed pawns,
//! bishop pair, king safety, and the Chaos style's extra terms.

use super::*;

/// The boundary between search and evaluation. The search calls `evaluate` and never
/// needs to know what implements it: the handcrafted styles today, and a neural (NNUE) or
/// hybrid evaluator later (roadmap items 8 and 12). Scores are centipawns from the side
/// to move's point of view.
///
/// `Send + Sync` because an `Engine` moves into the UCI worker thread with its evaluator.
pub trait Evaluator: Send + Sync {
    fn evaluate(&self, board: &Board) -> i32;
    /// Short identifier, reported in telemetry and match records.
    fn name(&self) -> &'static str;
}

/// The handcrafted evaluation in one of its styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleEvaluator(pub Style);

impl Evaluator for StyleEvaluator {
    fn evaluate(&self, board: &Board) -> i32 {
        evaluate_with_style(board, self.0)
    }

    fn name(&self) -> &'static str {
        match self.0 {
            Style::Classical => "classical",
            Style::Chaos => "chaos",
        }
    }
}

pub(crate) const PIECE_VALUES: [i32; 6] = [100, 320, 330, 500, 900, 20_000];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Classical,
    Chaos,
}

pub fn evaluate(board: &Board) -> i32 {
    evaluate_with_style(board, Style::Classical)
}

pub fn evaluate_with_style(board: &Board, style: Style) -> i32 {
    let mut score = 0;
    let mut files = [[0u8; 8]; 2];
    let mut bishops = [0u8; 2];
    let phase = game_phase(board);
    // Iterate occupied squares per piece and colour, not all 64 squares with a piece_on /
    // color_on lookup each (profiled: evaluation was ~32% of search time).
    for (side, color) in [Color::White, Color::Black].into_iter().enumerate() {
        let white = side == 0;
        let own = *board.color_combined(color);
        for piece in ALL_PIECES {
            for square in *board.pieces(piece) & own {
                let rank = square.get_rank().to_index();
                let file = square.get_file().to_index();
                let relative_rank = if white { rank } else { 7 - rank };
                let positional = if piece == Piece::King {
                    tapered_king(relative_rank * 8 + file, phase)
                } else {
                    piece_square(piece, relative_rank, file)
                };
                let value = PIECE_VALUES[piece.to_index()] + positional;
                score += if white { value } else { -value };
                if piece == Piece::Pawn {
                    files[side][file] += 1;
                }
                if piece == Piece::Bishop {
                    bishops[side] += 1;
                }
            }
        }
    }
    score += pawn_structure(&files) + passed_pawn_score(board);
    score += if bishops[0] >= 2 { 28 } else { 0 } - if bishops[1] >= 2 { 28 } else { 0 };
    score += king_safety(board, Color::White) - king_safety(board, Color::Black);
    if style == Style::Chaos {
        let black_board = board.null_move().unwrap_or(*board);
        let mobility =
            MoveGen::new_legal(board).len() as i32 - MoveGen::new_legal(&black_board).len() as i32;
        let checks = checking_moves(board) - checking_moves(&black_board);
        score += mobility * 3 + checks * 8 + center_control(board) * 2;
    }
    if board.side_to_move() == Color::White {
        score
    } else {
        -score
    }
}

/// Full-material phase: 4 knights and 4 bishops (1 each), 4 rooks (2), 2 queens (4).
pub(crate) const MAX_PHASE: i32 = 24;

/// How much middlegame is left: `MAX_PHASE` with all pieces on the board, 0 with only
/// kings and pawns. Promotions can push the raw sum above the maximum, so it is clamped.
pub(crate) fn game_phase(board: &Board) -> i32 {
    let count = |p: Piece| board.pieces(p).popcnt() as i32;
    (count(Piece::Knight) + count(Piece::Bishop) + 2 * count(Piece::Rook) + 4 * count(Piece::Queen))
        .min(MAX_PHASE)
}

/// King placement, blended by phase: shelter in the middlegame, centralisation in the
/// endgame. The audited evaluation used the middlegame table at every phase, so in a pawn
/// ending it still told the king to stay in the corner (experiments/E15).
pub(crate) fn tapered_king(index: usize, phase: i32) -> i32 {
    // Index = relative rank * 8 + file, own back rank first (the same layout as
    // `piece_square`). Values follow the widely used simplified-evaluation endgame table.
    const KING_ENDGAME: [i32; 64] = [
        -50, -30, -30, -30, -30, -30, -30, -50, //
        -30, -30, 0, 0, 0, 0, -30, -30, //
        -30, -10, 20, 30, 30, 20, -10, -30, //
        -30, -10, 30, 40, 40, 30, -10, -30, //
        -30, -10, 30, 40, 40, 30, -10, -30, //
        -30, -10, 20, 30, 30, 20, -10, -30, //
        -30, -20, -10, 0, 0, -10, -20, -30, //
        -50, -40, -30, -20, -20, -30, -40, -50,
    ];
    let middlegame = piece_square(Piece::King, index / 8, index % 8);
    (middlegame * phase + KING_ENDGAME[index] * (MAX_PHASE - phase)) / MAX_PHASE
}

pub(crate) fn piece_square(piece: Piece, rank: usize, file: usize) -> i32 {
    const PAWN: [i32; 64] = [
        0, 0, 0, 0, 0, 0, 0, 0, 5, 8, 8, -2, -2, 8, 8, 5, 3, 3, 5, 10, 12, 5, 3, 3, 2, 2, 4, 12,
        16, 12, 4, 2, 0, 0, 0, 8, 10, 0, 0, 0, 2, -2, -4, 0, 0, -4, -2, 2, 0, 0, 0, -8, -8, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const KNIGHT: [i32; 64] = [
        -50, -30, -20, -20, -20, -20, -30, -50, -30, -10, 0, 5, 5, 0, -10, -30, -20, 5, 15, 15, 15,
        15, 5, -20, -15, 0, 15, 20, 20, 15, 0, -15, -15, 5, 15, 20, 20, 15, 5, -15, -20, 0, 10, 15,
        15, 10, 0, -20, -30, -10, 0, 0, 0, 0, -10, -30, -50, -30, -20, -20, -20, -20, -30, -50,
    ];
    const BISHOP: [i32; 64] = [
        -20, -10, -10, -10, -10, -10, -10, -20, -10, 5, 0, 0, 0, 0, 5, -10, -10, 10, 10, 10, 10,
        10, 10, -10, -10, 0, 10, 10, 10, 10, 0, -10, -10, 5, 5, 10, 10, 5, 5, -10, -10, 0, 5, 10,
        10, 5, 0, -10, -10, 0, 0, 0, 0, 0, 0, -10, -20, -10, -10, -10, -10, -10, -10, -20,
    ];
    const ROOK: [i32; 64] = [
        0, 0, 5, 10, 10, 5, 0, 0, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0,
        0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, 5, 10, 10, 10, 10, 10, 10, 5, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const QUEEN: [i32; 64] = [
        -20, -10, -10, -5, -5, -10, -10, -20, -10, 0, 5, 0, 0, 0, 0, -10, -10, 5, 5, 5, 5, 5, 0,
        -10, 0, 0, 5, 5, 5, 5, 0, -5, -5, 0, 5, 5, 5, 5, 0, -5, -10, 0, 5, 5, 0, 0, 0, -10, -20,
        -10, -10, -5, -5, -10, -10, -20, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const KING: [i32; 64] = [
        20, 30, 10, 0, 0, 10, 30, 20, 20, 20, 0, 0, 0, 0, 20, 20, -10, -20, -20, -20, -20, -20,
        -20, -10, -20, -30, -30, -40, -40, -30, -30, -20, -30, -40, -40, -50, -50, -40, -40, -30,
        -30, -40, -40, -50, -50, -40, -40, -30, -30, -30, -30, -40, -40, -30, -30, -30, -20, -20,
        -20, -20, -20, -20, -20, -30,
    ];
    let table = match piece {
        Piece::Pawn => &PAWN,
        Piece::Knight => &KNIGHT,
        Piece::Bishop => &BISHOP,
        Piece::Rook => &ROOK,
        Piece::Queen => &QUEEN,
        Piece::King => &KING,
    };
    table[rank * 8 + file]
}

pub(crate) fn pawn_structure(files: &[[u8; 8]; 2]) -> i32 {
    let mut score = 0;
    for (side, side_files) in files.iter().enumerate() {
        for file in 0..8 {
            let n = side_files[file];
            if n > 1 {
                score += if side == 0 {
                    -12 * i32::from(n - 1)
                } else {
                    12 * i32::from(n - 1)
                };
            }
            if n > 0
                && (file == 0 || side_files[file - 1] == 0)
                && (file == 7 || side_files[file + 1] == 0)
            {
                score += if side == 0 { -10 } else { 10 };
            }
        }
    }
    score
}

/// Bonus for passed pawns: no enemy pawn *ahead* of the pawn on its own or an adjacent
/// file. The audited version (§G.8) counted any enemy pawn on those files, including
/// ones behind the pawn that can never stop it (experiments/E14).
pub(crate) fn passed_pawn_score(board: &Board) -> i32 {
    let pawns = *board.pieces(Piece::Pawn);
    let white_pawns = pawns & *board.color_combined(Color::White);
    let black_pawns = pawns & *board.color_combined(Color::Black);
    let mut score = 0;
    for square in pawns {
        let white = white_pawns & BitBoard::from_square(square) != chess::EMPTY;
        let rank = square.get_rank().to_index();
        let file = square.get_file();
        let span = get_adjacent_files(file) | get_file(file);
        let ahead = if white {
            span & !BitBoard::new((1u64 << (8 * (rank + 1))) - 1)
        } else {
            span & BitBoard::new((1u64 << (8 * rank)) - 1)
        };
        let blockers = if white { black_pawns } else { white_pawns };
        if ahead & blockers == chess::EMPTY {
            let advance = if white { rank } else { 7 - rank };
            let bonus = 10 + advance as i32 * 8;
            score += if white { bonus } else { -bonus };
        }
    }
    score
}

pub(crate) fn king_safety(board: &Board, color: Color) -> i32 {
    let king = board.king_square(color);
    // Pawn shelter: own pawns on the squares around the king.
    let shelter = get_king_moves(king) & *board.pieces(Piece::Pawn) & *board.color_combined(color);
    let score = 8 * shelter.popcnt() as i32;
    // Pressure: enemy attacks on the squares of the king's file, from attack maps. The
    // previous version counted legal moves in a null-moved position. For the side not to
    // move, that counted its *own* moves onto its king file, and in check it silently used
    // the wrong position (see experiments/E13).
    let enemy = *board.color_combined(!color);
    let occupied = *board.combined();
    let pressure: u32 = get_file(king.get_file())
        .map(|square| (attackers_to(board, square, occupied) & enemy).popcnt())
        .sum();
    score - pressure as i32 * 3
}

pub(crate) fn checking_moves(board: &Board) -> i32 {
    MoveGen::new_legal(board)
        .filter(|m| board.make_move_new(*m).checkers() != &chess::EMPTY)
        .count() as i32
}

pub(crate) fn center_control(board: &Board) -> i32 {
    [Square::D4, Square::E4, Square::D5, Square::E5]
        .into_iter()
        .map(|sq| {
            MoveGen::new_legal(board)
                .filter(|m| m.get_dest() == sq)
                .count() as i32
        })
        .sum()
}
