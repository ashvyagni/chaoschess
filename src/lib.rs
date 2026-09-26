use chess::{
    get_bishop_moves, get_king_moves, get_knight_moves, get_pawn_attacks, get_rook_moves,
    BitBoard, Board, BoardStatus, ChessMove, Color, File, MoveGen, Piece, Rank, Square,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub mod arena;
pub mod fen;
pub mod notation;
pub mod stats;
pub mod suites;
pub mod time;
pub mod uci;

pub const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const INF: i32 = 32_000;
const MATE: i32 = 30_000;
/// Any score beyond this magnitude is a mate score ("mate in N plies"), not an evaluation.
const MATE_THRESHOLD: i32 = MATE - 1_000;
/// Deepest iterative-deepening depth the engine will attempt.
pub const MAX_DEPTH: u8 = 64;
/// Rows in the triangular principal-variation table; main-search ply never exceeds MAX_DEPTH.
const PV_ROWS: usize = MAX_DEPTH as usize + 2;
/// Null-move pruning is tried only with at least this much depth left.
const NULL_MOVE_MIN_DEPTH: u8 = 3;
/// Null-move search depth is `depth - 1 - (NULL_MOVE_BASE_REDUCTION + depth / 6)`.
const NULL_MOVE_BASE_REDUCTION: u8 = 3;
/// Late move reductions apply from this remaining depth...
const LMR_MIN_DEPTH: u8 = 3;
/// ...to moves at this index or later in the ordered list (0-based).
const LMR_MIN_INDEX: usize = 3;

/// How many plies to reduce a late quiet move: grows with both depth and move index
/// (the usual logarithmic shape), one ply less at PV nodes, and never so much that the
/// reduced search would skip straight to quiescence.
fn lmr_reduction(depth: u8, index: usize, pv_node: bool) -> u8 {
    let r = 0.75 + (f64::from(depth)).ln() * (index as f64).ln() / 2.25;
    let r = (r as u8).saturating_sub(u8::from(pv_node)).max(1);
    r.min(depth.saturating_sub(2))
}

/// Ceiling on history-heuristic values. History persists across iterative-deepening
/// iterations, so without a bound it grows until it outranks the TT move and captures.
const HISTORY_MAX: i32 = 16_384;
pub const MAX_QUIESCENCE_PLY: u8 = 32;
const PIECE_VALUES: [i32; 6] = [100, 320, 330, 500, 900, 20_000];

/// How many plies into the quiescence search non-capturing checks are still
/// searched. Searching quiet checks is valuable -- it finds short forced mates that a
/// captures-only quiescence walks straight past -- but it must be bounded, because quiet
/// checks generate further quiet checks. Leaving it unbounded is what made the audited
/// baseline unable to finish a one-ply search in a middlegame position; see
/// `experiments/E1-quiescence-quiet-checks.md`.
pub const QS_CHECK_PLIES: u8 = 2;

/// Piece values used by static exchange evaluation. The king is given a value larger
/// than any possible exchange so a king capture can never look profitable.
const SEE_VALUES: [i32; 6] = [100, 320, 330, 500, 900, 100_000];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Classical,
    Chaos,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchLimits {
    pub depth: u8,
    pub nodes: Option<u64>,
    /// Hard limit: the search is abandoned mid-iteration once this much time has passed.
    pub time: Option<Duration>,
    /// Soft limit: no new iteration is started once half of this has passed, because the
    /// next iteration would very likely overrun it. Used for clock-based time control;
    /// `None` means "use the hard limit only" (e.g. `go movetime`).
    pub soft_time: Option<Duration>,
    pub hash_mb: usize,
    pub style: Style,
    pub threads: usize,
    /// Plies into quiescence for which non-capturing checks are still searched.
    ///
    /// Exposed rather than hard-coded so the tradeoff can be *measured* instead of
    /// assumed: raising it buys tactical sight and costs nodes exponentially. Setting it
    /// to [`MAX_QUIESCENCE_PLY`] reproduces the unbounded behaviour of the audited
    /// baseline, which is how the two are compared.
    pub qs_check_plies: u8,
    /// Prune captures that static exchange evaluation scores as losing material.
    pub qs_see_pruning: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub best_move: ChessMove,
    /// Deepest *completed* iteration. An iteration interrupted by a limit is discarded,
    /// never reported as completed.
    pub depth: u8,
    pub seldepth: u8,
    pub score: i32,
    pub nodes: u64,
    /// Principal variation of the deepest completed iteration, starting with `best_move`.
    pub pv: Vec<ChessMove>,
}

/// Progress report emitted after every completed iterative-deepening iteration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchInfo {
    pub depth: u8,
    /// Deepest ply reached, including quiescence.
    pub seldepth: u8,
    /// Centipawns from the side to move's point of view, or a mate score; see
    /// [`mate_in_moves`].
    pub score: i32,
    pub nodes: u64,
    pub elapsed: Duration,
    pub pv: Vec<ChessMove>,
    /// Transposition-table occupancy by the current search, in permille.
    pub hashfull: u32,
}

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
fn resets_clock(board: &Board, m: ChessMove) -> bool {
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

/// Convert a search score into UCI "mate N" form: positive N means the side to move mates
/// in N moves, negative N means it is mated in N moves. `None` for ordinary scores.
pub fn mate_in_moves(score: i32) -> Option<i32> {
    if score > MATE_THRESHOLD {
        Some((MATE - score + 1) / 2)
    } else if score < -MATE_THRESHOLD {
        Some(-(MATE + score) / 2)
    } else {
        None
    }
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            depth: 6,
            nodes: None,
            time: None,
            soft_time: None,
            hash_mb: 16,
            style: Style::Classical,
            threads: 1,
            qs_check_plies: QS_CHECK_PLIES,
            qs_see_pruning: true,
        }
    }
}

pub fn parse_move(board: &Board, coordinate: &str) -> Result<ChessMove, String> {
    // Byte-indexed slicing below is only safe on ASCII. Without this, a 5-byte string
    // like "e€4" passed the length check and panicked mid-character. Via
    // `position startpos moves ...` that crashed the engine (found by tests/fuzz.rs).
    if !coordinate.is_ascii() || coordinate.len() < 4 || coordinate.len() > 5 {
        return Err(format!("invalid coordinate move: {coordinate}"));
    }
    let from = parse_square(&coordinate[0..2])?;
    let to = parse_square(&coordinate[2..4])?;
    let promotion = if coordinate.len() == 5 {
        Some(match &coordinate[4..] {
            "q" => Piece::Queen,
            "r" => Piece::Rook,
            "b" => Piece::Bishop,
            "n" => Piece::Knight,
            _ => return Err(format!("invalid promotion piece in {coordinate}")),
        })
    } else {
        None
    };
    let chess_move = ChessMove::new(from, to, promotion);
    if MoveGen::new_legal(board).any(|legal| legal == chess_move) {
        Ok(chess_move)
    } else {
        Err(format!("illegal move {coordinate} in position {board}"))
    }
}

fn parse_square(value: &str) -> Result<Square, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 2 || !(b'a'..=b'h').contains(&bytes[0]) || !(b'1'..=b'8').contains(&bytes[1])
    {
        return Err(format!("invalid square: {value}"));
    }
    Ok(Square::make_square(
        Rank::from_index(usize::from(bytes[1] - b'1')),
        File::from_index(usize::from(bytes[0] - b'a')),
    ))
}

pub fn perft(board: &Board, depth: u8) -> u64 {
    if depth == 0 {
        return 1;
    }
    MoveGen::new_legal(board)
        .map(|m| perft(&board.make_move_new(m), depth - 1))
        .sum()
}

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
fn is_mated_within(board: &Board, moves: u8) -> bool {
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

pub fn evaluate(board: &Board) -> i32 {
    evaluate_with_style(board, Style::Classical)
}

pub fn evaluate_with_style(board: &Board, style: Style) -> i32 {
    let mut score = 0;
    let mut files = [[0u8; 8]; 2];
    let mut bishops = [0u8; 2];
    for square in !chess::EMPTY {
        if let Some(piece) = board.piece_on(square) {
            let white = board.color_on(square) == Some(Color::White);
            let side = usize::from(!white);
            let rank = square.get_rank().to_index();
            let file = square.get_file().to_index();
            let relative_rank = if white { rank } else { 7 - rank };
            let value = PIECE_VALUES[piece.to_index()] + piece_square(piece, relative_rank, file);
            score += if white { value } else { -value };
            if piece == Piece::Pawn {
                files[side][file] += 1;
            }
            if piece == Piece::Bishop {
                bishops[side] += 1;
            }
        }
    }
    score += pawn_structure(&files) + passed_pawn_score(board, &files);
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

fn piece_square(piece: Piece, rank: usize, file: usize) -> i32 {
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

fn pawn_structure(files: &[[u8; 8]; 2]) -> i32 {
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

fn passed_pawn_score(board: &Board, files: &[[u8; 8]; 2]) -> i32 {
    let mut score = 0;
    for square in !chess::EMPTY {
        if board.piece_on(square) != Some(Piece::Pawn) {
            continue;
        }
        let white = board.color_on(square) == Some(Color::White);
        let side = usize::from(!white);
        let rank = square.get_rank().to_index();
        let file = square.get_file().to_index();
        let blocked = [file.saturating_sub(1), file, (file + 1).min(7)]
            .into_iter()
            .any(|f| files[1 - side][f] > 0);
        if !blocked {
            let advance = if white { rank } else { 7 - rank };
            let bonus = 10 + advance as i32 * 8;
            score += if white { bonus } else { -bonus };
        }
    }
    score
}

fn king_safety(board: &Board, color: Color) -> i32 {
    let king = board.king_square(color);
    let file = king.get_file().to_index();
    let rank = king.get_rank().to_index();
    let mut score = 0;
    for f in file.saturating_sub(1)..=(file + 1).min(7) {
        for r in rank.saturating_sub(1)..=(rank + 1).min(7) {
            if f == file && r == rank {
                continue;
            }
            let sq = Square::make_square(Rank::from_index(r), File::from_index(f));
            if board.color_on(sq) == Some(color) && board.piece_on(sq) == Some(Piece::Pawn) {
                score += 8;
            }
        }
    }
    let enemy = board.null_move().unwrap_or(*board);
    score
        - MoveGen::new_legal(&enemy)
            .filter(|m| m.get_dest() == king || m.get_dest().get_file() == king.get_file())
            .count() as i32
            * 3
}
fn checking_moves(board: &Board) -> i32 {
    MoveGen::new_legal(board)
        .filter(|m| board.make_move_new(*m).checkers() != &chess::EMPTY)
        .count() as i32
}
fn center_control(board: &Board) -> i32 {
    [Square::D4, Square::E4, Square::D5, Square::E5]
        .into_iter()
        .map(|sq| {
            MoveGen::new_legal(board)
                .filter(|m| m.get_dest() == sq)
                .count() as i32
        })
        .sum()
}

/// True when `m` is an en passant capture.
///
/// Note the `chess` crate stores the square of the *capturable pawn* in `en_passant()`,
/// not the square the capturing pawn moves to, so the destination has to be stepped back
/// one rank before comparing. Getting this backwards silently classifies every en passant
/// capture as a quiet move.
fn is_en_passant(board: &Board, m: ChessMove) -> bool {
    board.piece_on(m.get_source()) == Some(Piece::Pawn)
        && board.piece_on(m.get_dest()).is_none()
        && board.en_passant() == Some(m.get_dest().ubackward(board.side_to_move()))
}

/// True when `m` removes an enemy piece from the board, including en passant, where the
/// captured pawn is not on the destination square.
fn is_capture(board: &Board, m: ChessMove) -> bool {
    board.piece_on(m.get_dest()).is_some() || is_en_passant(board, m)
}

/// Every piece of either colour that attacks `square`, given an arbitrary occupancy.
///
/// Passing a modified `occupied` is what makes x-ray recomputation work in [`see`]: once
/// an attacker is removed, a slider behind it becomes an attacker in the next iteration.
fn attackers_to(board: &Board, square: Square, occupied: BitBoard) -> BitBoard {
    let pawns = *board.pieces(Piece::Pawn);
    let white = *board.color_combined(Color::White);
    let black = *board.color_combined(Color::Black);
    let diagonal = *board.pieces(Piece::Bishop) | *board.pieces(Piece::Queen);
    let straight = *board.pieces(Piece::Rook) | *board.pieces(Piece::Queen);

    // A white pawn on `p` attacks `square` exactly when `p` is one of the squares a black
    // pawn standing on `square` would attack, so the tables are probed with the colour
    // inverted.
    let mut attackers = get_pawn_attacks(square, Color::Black, pawns & white)
        | get_pawn_attacks(square, Color::White, pawns & black)
        | (get_knight_moves(square) & *board.pieces(Piece::Knight))
        | (get_king_moves(square) & *board.pieces(Piece::King));
    attackers |= get_bishop_moves(square, occupied) & diagonal;
    attackers |= get_rook_moves(square, occupied) & straight;
    attackers & occupied
}

/// The cheapest piece of `color` among `attackers`, as (piece, its square).
fn least_valuable(board: &Board, attackers: BitBoard, color: Color) -> Option<(Piece, Square)> {
    let mine = attackers & *board.color_combined(color);
    for piece in [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
        Piece::King,
    ] {
        let candidates = mine & *board.pieces(piece);
        if candidates != chess::EMPTY {
            return Some((piece, candidates.to_square()));
        }
    }
    None
}

/// Static exchange evaluation: the material the side to move nets from playing `m` if
/// both sides then recapture on that square with their cheapest piece until neither
/// wants to continue.
///
/// This is a static estimate, not a search -- it ignores pins, intermediate tactics and
/// the possibility that recapturing is simply bad. It exists to answer one cheap
/// question: "is this capture obviously losing material?" A negative result means yes.
fn see(board: &Board, m: ChessMove) -> i32 {
    let target = m.get_dest();
    let source = m.get_source();
    let Some(mut attacker) = board.piece_on(source) else {
        return 0;
    };

    let mut occupied = *board.combined();

    // Value of the piece being captured on this first move.
    let mut gain = [0i32; 32];
    gain[0] = if is_en_passant(board, m) {
        // The captured pawn sits behind the destination square; clear it from the
        // occupancy so sliders through that square are seen correctly.
        let captured = target.ubackward(board.side_to_move());
        occupied &= !BitBoard::from_square(captured);
        SEE_VALUES[Piece::Pawn.to_index()]
    } else {
        board
            .piece_on(target)
            .map_or(0, |p| SEE_VALUES[p.to_index()])
    };

    // A promotion arrives on the target square as the promoted piece, and the pawn's own
    // value is replaced.
    if let Some(promotion) = m.get_promotion() {
        gain[0] += SEE_VALUES[promotion.to_index()] - SEE_VALUES[Piece::Pawn.to_index()];
        attacker = promotion;
    }

    occupied &= !BitBoard::from_square(source);
    let mut side = !board.side_to_move();
    let mut depth = 0usize;

    loop {
        depth += 1;
        if depth >= gain.len() {
            break;
        }
        // If `side` recaptures, it wins the attacker standing on the target square but
        // exposes its own recapturing piece.
        gain[depth] = SEE_VALUES[attacker.to_index()] - gain[depth - 1];

        let attackers = attackers_to(board, target, occupied);
        let Some((next, from)) = least_valuable(board, attackers, side) else {
            break;
        };
        // Recapturing with the king is only legal if the opponent has no attackers left;
        // treating it as available anyway would over-value the exchange.
        if next == Piece::King
            && least_valuable(board, attackers_to(board, target, occupied), !side).is_some()
        {
            break;
        }
        occupied &= !BitBoard::from_square(from);
        attacker = next;
        side = !side;
    }

    // Walk back up the swap list: at each level the side to move can decline the
    // recapture, so it takes the better of "stop here" and "continue".
    while depth > 1 {
        depth -= 1;
        gain[depth - 1] = -(-gain[depth - 1]).max(gain[depth]);
    }
    gain[0]
}

#[derive(Clone, Copy)]
struct Entry {
    key: u64,
    depth: u8,
    /// Search generation this entry was written in, used to prefer replacing stale data.
    generation: u8,
    /// Stored root-relative for mates; see [`score_to_tt`].
    score: i32,
    flag: Bound,
    best: Option<ChessMove>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bound {
    Exact,
    Lower,
    Upper,
}

/// Mate scores are "mate in N plies *from here*". The same position reached at a
/// different ply has a different distance to mate from the root, so a mate score has to
/// be converted to "distance from this node" before storing and back on retrieval.
/// Storing it raw makes a mate found at ply 7 look like a mate at ply 3 when probed there.
fn score_to_tt(score: i32, ply: u8) -> i32 {
    if score > MATE_THRESHOLD {
        score + i32::from(ply)
    } else if score < -MATE_THRESHOLD {
        score - i32::from(ply)
    } else {
        score
    }
}

fn score_from_tt(score: i32, ply: u8) -> i32 {
    if score > MATE_THRESHOLD {
        score - i32::from(ply)
    } else if score < -MATE_THRESHOLD {
        score + i32::from(ply)
    } else {
        score
    }
}

struct Table {
    entries: Vec<Option<Entry>>,
    generation: u8,
}
impl Table {
    fn new(mb: usize) -> Self {
        let count = ((mb.max(1) * 1024 * 1024) / std::mem::size_of::<Option<Entry>>()).max(1);
        Self {
            entries: vec![None; count],
            generation: 0,
        }
    }

    fn slot(&self, key: u64) -> usize {
        (key as usize) % self.entries.len()
    }

    /// The entry for exactly this position, if one is stored.
    ///
    /// The key comparison is the whole point: many positions share a slot, and returning
    /// a neighbour's bound as if it were this position's is silent search corruption.
    /// The audited baseline omitted it (MASTER_ENGINE_AUDIT.md §G.2).
    fn get(&self, key: u64) -> Option<Entry> {
        self.entries[self.slot(key)].filter(|entry| entry.key == key)
    }

    /// Replacement policy: always replace an empty slot, a slot holding the same
    /// position, or a slot left over from an earlier search; otherwise keep whichever
    /// entry was searched deeper.
    fn put(&mut self, entry: Entry) {
        let slot = self.slot(entry.key);
        let replace = match self.entries[slot] {
            None => true,
            Some(old) => {
                old.key == entry.key || old.generation != self.generation || entry.depth >= old.depth
            }
        };
        if replace {
            self.entries[slot] = Some(Entry {
                generation: self.generation,
                ..entry
            });
        }
    }

    /// Start a new search. Entries from earlier searches stay usable but become the
    /// first candidates for replacement.
    fn new_search(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// A zero-capacity stand-in, used only while the real table is lent to a search.
    /// Never probed: `slot` would divide by zero.
    fn placeholder() -> Self {
        Self {
            entries: Vec::new(),
            generation: 0,
        }
    }

    fn clear(&mut self) {
        self.entries.iter_mut().for_each(|entry| *entry = None);
        self.generation = 0;
    }

    /// Permille of a fixed sample of slots written by the current search, the usual UCI
    /// `hashfull` estimate.
    fn hashfull(&self) -> u32 {
        let sample = self.entries.len().min(1_000);
        if sample == 0 {
            return 0;
        }
        let used = self.entries[..sample]
            .iter()
            .filter(|e| e.is_some_and(|e| e.generation == self.generation))
            .count();
        (used * 1_000 / sample) as u32
    }
}

struct Searcher {
    table: Table,
    limits: SearchLimits,
    start: Instant,
    nodes: u64,
    stopped: bool,
    history: [[i32; 64]; 64],
    /// Set by another thread (UCI `stop`, `quit`, a new `go`) to end the search.
    stop_flag: Arc<AtomicBool>,
    /// Time and external stops are honoured only once depth 1 is complete, so there is
    /// always a searched move to return rather than an arbitrary legal one.
    can_abort: bool,
    /// Triangular PV table: `pv[ply]` is the best line found from `ply` in the current node.
    pv: Vec<Vec<ChessMove>>,
    seldepth: u8,
    /// Hashes of every position from the oldest reversible game position to the current
    /// node, inclusive. Used for repetition detection.
    path: Vec<u64>,
    /// Halfmove clock of each node from the root to the current node.
    clocks: Vec<u32>,
    /// Whether each node from the root was reached by a null move, so two null moves are
    /// never made in a row (that would just hand the move back).
    null_moves: Vec<bool>,
}

impl Searcher {
    #[cfg(test)]
    fn new(limits: SearchLimits) -> Self {
        Self::with_table(limits, Table::new(limits.hash_mb), Arc::new(AtomicBool::new(false)))
    }

    fn with_table(limits: SearchLimits, table: Table, stop_flag: Arc<AtomicBool>) -> Self {
        Self {
            table,
            limits,
            start: Instant::now(),
            nodes: 0,
            stopped: false,
            history: [[0; 64]; 64],
            stop_flag,
            can_abort: false,
            pv: vec![Vec::new(); PV_ROWS],
            seldepth: 0,
            path: Vec::new(),
            clocks: vec![0],
            null_moves: vec![false],
        }
    }

    /// Reset the path to a root position and its game history.
    fn set_root(&mut self, root: &Position) {
        self.path.clear();
        self.path.extend_from_slice(&root.prior);
        self.path.push(root.board.get_hash());
        self.clocks.clear();
        self.clocks.push(root.halfmove_clock);
        self.null_moves.clear();
        self.null_moves.push(false);
    }

    /// Search `child` (the result of `m` played in `parent`) and return its score from
    /// the parent's point of view. This is the only way the main search descends a ply,
    /// so the repetition path and the halfmove clocks cannot drift out of step.
    #[allow(clippy::too_many_arguments)]
    fn search_child(
        &mut self,
        parent: &Board,
        m: ChessMove,
        child: &Board,
        depth: u8,
        alpha: i32,
        beta: i32,
        ply: u8,
    ) -> i32 {
        let clock = if resets_clock(parent, m) {
            0
        } else {
            self.clocks.last().copied().unwrap_or(0) + 1
        };
        self.path.push(child.get_hash());
        self.clocks.push(clock);
        self.null_moves.push(false);
        let value = -self.negamax(child, depth, -beta, -alpha, ply);
        self.path.pop();
        self.clocks.pop();
        self.null_moves.pop();
        value
    }

    /// Null-move pruning. If the side to move could skip its turn and a reduced search
    /// still fails high, a real move almost certainly fails high too, so the node is cut
    /// off. Returns the cutoff score, or `None` to search normally.
    ///
    /// The guards cover the method's known failure modes:
    /// - not at PV nodes, where an exact score is wanted;
    /// - not in check, where passing is illegal and the idea is meaningless;
    /// - not without non-pawn material: in pawn endings zugzwang is common, and there the
    ///   right to move is a disadvantage, so "passing is fine" proves nothing;
    /// - not twice in a row;
    /// - only when the static evaluation already reaches beta;
    /// - a mate score from the reduced search is not trusted: `beta` is returned instead.
    fn try_null_move(&mut self, board: &Board, depth: u8, beta: i32, ply: u8) -> Option<i32> {
        if depth < NULL_MOVE_MIN_DEPTH || self.null_moves.last() == Some(&true) {
            return None;
        }
        let side = *board.color_combined(board.side_to_move());
        let pawns_and_king = *board.pieces(Piece::Pawn) | *board.pieces(Piece::King);
        if side & !pawns_and_king == chess::EMPTY {
            return None;
        }
        if evaluate_with_style(board, self.limits.style) < beta {
            return None;
        }
        let passed = board.null_move()?;
        let reduction = NULL_MOVE_BASE_REDUCTION + depth / 6;
        // The null move resets the repetition window (clock 0): positions on either side
        // of a pass must never be counted as repetitions of each other.
        self.path.push(passed.get_hash());
        self.clocks.push(0);
        self.null_moves.push(true);
        let score = -self.negamax(&passed, depth.saturating_sub(1 + reduction), -beta, -beta + 1, ply + 1);
        self.path.pop();
        self.clocks.pop();
        self.null_moves.pop();
        if self.stopped || score < beta {
            return None;
        }
        Some(if score >= MATE_THRESHOLD { beta } else { score })
    }

    /// Whether the current node (the last entry of `path`) is a draw by repetition.
    ///
    /// A position that recurs *inside the search* counts as a draw on its first
    /// repetition: if a side can repeat once, it can repeat again, and treating it as a
    /// draw sooner saves search. A position whose earlier occurrences are all in the game
    /// history before the root needs two of them, since that is what makes a real
    /// threefold repetition.
    fn is_repetition(&self, ply: u8) -> bool {
        let n = self.path.len();
        let Some(&current) = self.path.last() else {
            return false;
        };
        let clock = self.clocks.last().copied().unwrap_or(0) as usize;
        let reach = clock.min(n - 1);
        let mut earlier = 0;
        // The same side is to move only every other ply, and a position cannot recur
        // after just two plies, so the scan starts four plies back.
        let mut back = 4;
        while back <= reach {
            if self.path[n - 1 - back] == current {
                if back <= usize::from(ply) {
                    return true;
                }
                earlier += 1;
                if earlier >= 2 {
                    return true;
                }
            }
            back += 2;
        }
        false
    }

    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        // A node budget is exact and always honoured, so fixed-node benchmarks stay
        // reproducible.
        if self.limits.nodes.is_some_and(|n| self.nodes >= n) {
            self.stopped = true;
            return;
        }
        // The clock and the external flag are polled every 1024 nodes: that is ~2 ms at
        // current speeds, and reading them at every node is overhead for nothing.
        if self.can_abort && self.nodes & 1023 == 0 {
            let external = self.stop_flag.load(Ordering::Relaxed);
            let timed_out = self.limits.time.is_some_and(|t| self.start.elapsed() >= t);
            self.stopped = external || timed_out;
        }
    }

    /// `pv[ply] = m` followed by the child's line.
    fn update_pv(&mut self, ply: u8, m: ChessMove) {
        let ply = usize::from(ply);
        if ply + 1 >= self.pv.len() {
            return;
        }
        let (head, tail) = self.pv.split_at_mut(ply + 1);
        let line = &mut head[ply];
        line.clear();
        line.push(m);
        line.extend_from_slice(&tail[0]);
    }
    /// Legal moves, best-first.
    ///
    /// `check_bonus` controls whether checking moves are promoted in the ordering. It
    /// costs a full board copy per move to find out, which is worth it in the main search
    /// (where it buys cutoffs over a large subtree) and not worth it in quiescence (where
    /// the subtree is shallow and the same information is recomputed immediately after).
    fn ordered(&self, board: &Board, tt: Option<ChessMove>, check_bonus: bool) -> Vec<ChessMove> {
        let mut moves: Vec<_> = MoveGen::new_legal(board).collect();
        // `sort_by_cached_key`, not `sort_by_key`: the key plays the move on a board copy
        // to test for check, and `sort_by_key` recomputes the key on every comparison,
        // about 2*log2(n) times per move. Profiling showed that closure taking ~42% of
        // search time. The cached variant computes each key once and is also stable, so
        // the ordering, and with it the whole search, is unchanged (verified: identical
        // node counts).
        moves.sort_by_cached_key(|m| {
            // MVV-LVA: prefer taking the most valuable victim with the least valuable
            // attacker.
            let victim = board
                .piece_on(m.get_dest())
                .map_or(0, |p| PIECE_VALUES[p.to_index()]);
            let attacker = board
                .piece_on(m.get_source())
                .map_or(1, |p| PIECE_VALUES[p.to_index()]);
            let tt_bonus = if Some(*m) == tt { 1_000_000 } else { 0 };
            let check = if check_bonus && board.make_move_new(*m).checkers() != &chess::EMPTY {
                50_000
            } else {
                0
            };
            -(tt_bonus + check + victim * 10 - attacker
                + self.history[m.get_source().to_index()][m.get_dest().to_index()])
        });
        moves
    }
    /// Quiescence search: resolve the position until nothing forcing is left, so the
    /// evaluation is not read in the middle of an exchange.
    ///
    /// `ply` is the absolute distance from the root and is only used for mate scoring.
    /// `qs_ply` counts plies inside quiescence and bounds it.
    fn quiescence(
        &mut self,
        board: &Board,
        mut alpha: i32,
        beta: i32,
        ply: u8,
        qs_ply: u8,
    ) -> i32 {
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);
        self.stop();
        if self.stopped {
            return 0;
        }

        let in_check = board.checkers() != &chess::EMPTY;

        // The move list doubles as terminal detection, so no separate status() call --
        // which would cost a second full move generation -- is needed.
        let moves = self.ordered(board, None, false);
        if moves.is_empty() {
            return if in_check { -MATE + i32::from(ply) } else { 0 };
        }

        // A capture sequence can strip the board down to a dead position; its static
        // evaluation would still show a material edge that can never become a win.
        if insufficient_material(board) {
            return 0;
        }

        if qs_ply >= MAX_QUIESCENCE_PLY {
            return evaluate_with_style(board, self.limits.style);
        }

        // Stand pat: the side to move is not obliged to capture, so the static score is a
        // lower bound -- except in check, where every move must address the check.
        let mut best = if in_check {
            -INF
        } else {
            let stand = evaluate_with_style(board, self.limits.style);
            if stand >= beta {
                return stand;
            }
            alpha = alpha.max(stand);
            stand
        };

        let allow_checks = qs_ply < self.limits.qs_check_plies;

        for m in moves {
            if !in_check {
                if is_capture(board, m) || m.get_promotion().is_some() {
                    // Skip captures that static exchange evaluation says lose material.
                    // These are the bulk of quiescence nodes and almost never change the
                    // score, because the opponent simply recaptures.
                    if self.limits.qs_see_pruning && see(board, m) < 0 {
                        continue;
                    }
                } else if allow_checks {
                    if board.make_move_new(m).checkers() == &chess::EMPTY {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            let score = -self.quiescence(&board.make_move_new(m), -beta, -alpha, ply + 1, qs_ply + 1);
            if self.stopped {
                return 0;
            }
            best = best.max(score);
            alpha = alpha.max(best);
            if alpha >= beta {
                break;
            }
        }

        // In check with every evasion pruned away cannot happen (evasions are never
        // pruned), so a -INF best here would be a bug rather than a mate.
        if best == -INF {
            return evaluate_with_style(board, self.limits.style);
        }
        best
    }

    fn negamax(&mut self, board: &Board, depth: u8, mut alpha: i32, beta: i32, ply: u8) -> i32 {
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);
        if let Some(line) = self.pv.get_mut(usize::from(ply)) {
            line.clear();
        }
        self.stop();
        if self.stopped {
            return 0;
        }
        match board.status() {
            BoardStatus::Checkmate => return -MATE + i32::from(ply),
            BoardStatus::Stalemate => return 0,
            BoardStatus::Ongoing => {}
        }
        // Draw rules come after the mate test: a checkmate delivered on the hundredth
        // halfmove is still checkmate. They also come before the transposition table,
        // because a draw depends on how the position was reached, not only on the position.
        if self.clocks.last().is_some_and(|&c| c >= 100)
            || self.is_repetition(ply)
            || insufficient_material(board)
        {
            return 0;
        }
        if depth == 0 {
            return self.quiescence(board, alpha, beta, ply, 0);
        }
        let key = board.get_hash();
        let tt = self.table.get(key);
        if let Some(entry) = tt.filter(|e| e.depth >= depth) {
            let tt_score = score_from_tt(entry.score, ply);
            match entry.flag {
                Bound::Exact => return tt_score,
                Bound::Lower if tt_score >= beta => return tt_score,
                Bound::Upper if tt_score <= alpha => return tt_score,
                _ => {}
            }
        }
        let in_check = board.checkers() != &chess::EMPTY;
        let pv_node = beta - alpha > 1;
        if !pv_node && !in_check {
            if let Some(cutoff) = self.try_null_move(board, depth, beta, ply) {
                return cutoff;
            }
        }

        let original_alpha = alpha;
        let mut best = None;
        let mut score = -INF;
        for (index, m) in self.ordered(board, tt.and_then(|e| e.best), true).into_iter().enumerate() {
            let child = board.make_move_new(m);
            // Principal variation search: with good ordering the first move is usually
            // best, so later moves only need to be *refuted*. A null-window scout at
            // (alpha, alpha + 1) is enough to show a move is no better, and only a move
            // that beats alpha is re-searched with the full window to get its true
            // value. Score-preserving: see `shallow_search_score_equals_plain_minimax`.
            let value = if index == 0 {
                self.search_child(board, m, &child, depth - 1, alpha, beta, ply + 1)
            } else {
                // Late move reductions: with good ordering, quiet moves far down the list
                // rarely matter, so they are first scouted at reduced depth. Only a move
                // that beats alpha there earns a full-depth scout. Captures, promotions,
                // checking moves, moves made while in check, and the first few moves are
                // never reduced.
                let reduction = if depth >= LMR_MIN_DEPTH
                    && index >= LMR_MIN_INDEX
                    && !in_check
                    && !is_capture(board, m)
                    && m.get_promotion().is_none()
                    && child.checkers() == &chess::EMPTY
                {
                    lmr_reduction(depth, index, pv_node)
                } else {
                    0
                };
                let mut scout = self.search_child(board, m, &child, depth - 1 - reduction, alpha, alpha + 1, ply + 1);
                if reduction > 0 && scout > alpha && !self.stopped {
                    scout = self.search_child(board, m, &child, depth - 1, alpha, alpha + 1, ply + 1);
                }
                if scout > alpha && scout < beta && !self.stopped {
                    self.search_child(board, m, &child, depth - 1, alpha, beta, ply + 1)
                } else {
                    scout
                }
            };
            if self.stopped {
                return 0;
            }
            if value > score {
                score = value;
                best = Some(m);
                if value > alpha {
                    self.update_pv(ply, m);
                }
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                if !is_capture(board, m) {
                    self.reward_history(m, depth);
                }
                break;
            }
        }
        let flag = if score <= original_alpha {
            Bound::Upper
        } else if score >= beta {
            Bound::Lower
        } else {
            Bound::Exact
        };
        self.table.put(Entry {
            key,
            depth,
            generation: 0, // stamped by Table::put
            score: score_to_tt(score, ply),
            flag,
            best,
        });
        score
    }

    /// Credit a quiet move that caused a beta cutoff.
    ///
    /// Uses the "gravity" update `h += b - h*b/MAX`, which saturates smoothly at
    /// [`HISTORY_MAX`] instead of growing without bound. Only quiet moves are credited:
    /// captures are already ordered by MVV-LVA, and crediting them too lets history
    /// swamp that ordering.
    fn reward_history(&mut self, m: ChessMove, depth: u8) {
        let bonus = (i32::from(depth) * i32::from(depth)).min(HISTORY_MAX);
        let entry = &mut self.history[m.get_source().to_index()][m.get_dest().to_index()];
        *entry += bonus - *entry * bonus / HISTORY_MAX;
    }
}

pub fn best_move(board: &Board, limits: SearchLimits) -> Option<ChessMove> {
    search(board, limits).map(|result| result.best_move)
}

/// Search the root position with principal variation search.
///
/// The first move -- the previous iteration's best move, when there is one -- is searched
/// with the full window, because its score defines the PV. Every later move gets a null
/// window scout and is only re-searched in full if it might beat the current best. The
/// audited baseline scouted the first move too and never re-searched it (§G.3), so on
/// iteration one the PV move's score was a bound against a window of (31999, 32000).
fn search_root(
    board: &Board,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    searcher: &mut Searcher,
    previous_best: Option<ChessMove>,
) -> Option<(ChessMove, i32)> {
    let moves = searcher.ordered(board, previous_best, true);
    let child_depth = depth.saturating_sub(1);
    let mut best = None;
    let mut best_score = -INF;
    for (index, m) in moves.into_iter().enumerate() {
        let child = board.make_move_new(m);
        let value = if index == 0 {
            searcher.search_child(board, m, &child, child_depth, alpha, beta, 1)
        } else {
            let scout = searcher.search_child(board, m, &child, child_depth, alpha, alpha + 1, 1);
            if scout > alpha && scout < beta && !searcher.stopped {
                searcher.search_child(board, m, &child, child_depth, alpha, beta, 1)
            } else {
                scout
            }
        };
        if searcher.stopped {
            break;
        }
        if value > best_score {
            best_score = value;
            best = Some(m);
            searcher.update_pv(0, m);
        }
        alpha = alpha.max(value);
        if alpha >= beta {
            // Fail high against an aspiration window: the caller re-searches with a
            // wider window, so finishing the remaining moves here is wasted work.
            break;
        }
    }
    best.map(|m| (m, best_score))
}

/// Search with a throwaway engine: fresh table, no external stop, no progress reports.
/// Deterministic for fixed depth/nodes, which is what tests and benchmarks need.
pub fn search(board: &Board, limits: SearchLimits) -> Option<SearchResult> {
    Engine::new(limits.hash_mb).search(board, limits, Arc::new(AtomicBool::new(false)), &mut |_| {})
}

/// A long-lived engine. It owns the transposition table across searches, so what it
/// learned thinking about one move is still there for the next. That is how it is used in
/// a real game, via UCI.
pub struct Engine {
    table: Table,
}

impl Engine {
    pub fn new(hash_mb: usize) -> Self {
        Self {
            table: Table::new(hash_mb),
        }
    }

    /// Reallocate the table at a new size. Its contents are lost.
    pub fn resize(&mut self, hash_mb: usize) {
        self.table = Table::new(hash_mb);
    }

    /// Forget everything, as for UCI `ucinewgame`.
    pub fn clear(&mut self) {
        self.table.clear();
    }

    /// Iterative-deepening search. `stop` ends it from another thread; `on_info` is called
    /// after every completed iteration. The table size is the engine's own, and
    /// `limits.hash_mb` is ignored here.
    pub fn search(
        &mut self,
        board: &Board,
        limits: SearchLimits,
        stop: Arc<AtomicBool>,
        on_info: &mut dyn FnMut(&SearchInfo),
    ) -> Option<SearchResult> {
        self.search_position(&Position::new(*board), limits, stop, on_info)
    }

    /// Search a position together with its game history, so repetitions and the
    /// fifty-move rule are seen. This is what UCI uses.
    pub fn search_position(
        &mut self,
        root: &Position,
        limits: SearchLimits,
        stop: Arc<AtomicBool>,
        on_info: &mut dyn FnMut(&SearchInfo),
    ) -> Option<SearchResult> {
        let board = &root.board;
        let table = std::mem::replace(&mut self.table, Table::placeholder());
        let mut searcher = Searcher::with_table(limits, table, stop);
        searcher.set_root(root);
        let result = iterate(&mut searcher, board, limits, on_info);
        self.table = std::mem::replace(&mut searcher.table, Table::placeholder());
        result
    }
}

fn iterate(
    searcher: &mut Searcher,
    board: &Board,
    limits: SearchLimits,
    on_info: &mut dyn FnMut(&SearchInfo),
) -> Option<SearchResult> {
    let fallback = MoveGen::new_legal(board).next()?;
    let mut result = SearchResult {
        best_move: fallback,
        depth: 0,
        seldepth: 0,
        score: 0,
        nodes: 0,
        pv: vec![fallback],
    };
    searcher.table.new_search();

    // `limits.threads` is accepted but not yet used: the root-splitting parallel search it
    // used to select was measured to be strictly harmful (experiments/E3) and was removed.
    // Lazy SMP over a shared table is the replacement (roadmap item 9).
    for depth in 1..=limits.depth.clamp(1, MAX_DEPTH) {
        let previous_best = (result.depth > 0).then_some(result.best_move);
        let (alpha, beta) = if result.depth > 0 && result.score.abs() < MATE_THRESHOLD {
            (result.score - 40, result.score + 40)
        } else {
            (-INF, INF)
        };
        let mut candidate = search_root(board, depth, alpha, beta, searcher, previous_best);
        if !searcher.stopped
            && (alpha, beta) != (-INF, INF)
            && candidate
                .as_ref()
                .is_some_and(|(_, s)| *s <= alpha || *s >= beta)
        {
            candidate = search_root(board, depth, -INF, INF, searcher, previous_best);
        }

        // An interrupted iteration is discarded: its score may be a bound and it has not
        // looked at every move. Depth 1 always runs to completion (see Searcher::can_abort)
        // unless a node budget cuts it, so a searched move is almost always available.
        if searcher.stopped && result.depth > 0 {
            break;
        }
        if let Some((m, score)) = candidate {
            let mut pv = searcher.pv[0].clone();
            if pv.first() != Some(&m) {
                pv = vec![m];
            }
            result = SearchResult {
                best_move: m,
                depth,
                seldepth: searcher.seldepth,
                score,
                nodes: searcher.nodes,
                pv,
            };
            on_info(&SearchInfo {
                depth,
                seldepth: searcher.seldepth,
                score,
                nodes: searcher.nodes,
                elapsed: searcher.start.elapsed(),
                pv: result.pv.clone(),
                hashfull: searcher.table.hashfull(),
            });
        }
        searcher.can_abort = true;
        if searcher.stopped || searcher.stop_flag.load(Ordering::Relaxed) {
            break;
        }
        if let Some(soft) = limits.soft_time {
            // Each iteration costs several times the one before, so once half the soft
            // budget is spent the next iteration would very likely overrun; starting it
            // only burns time that the hard limit then throws away.
            if searcher.start.elapsed() >= soft / 2 {
                break;
            }
        }
    }
    result.nodes = searcher.nodes;
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    #[test]
    fn start_position_perft_matches_known_counts() {
        let board = Board::default();
        assert_eq!(perft(&board, 1), 20);
        assert_eq!(perft(&board, 2), 400);
        assert_eq!(perft(&board, 3), 8_902);
        assert_eq!(perft(&board, 4), 197_281);
    }
    #[test]
    fn parses_and_rejects_moves() {
        let b = Board::default();
        assert!(parse_move(&b, "e2e4").is_ok());
        assert!(parse_move(&b, "e2e5").is_err());
    }
    #[test]
    fn search_returns_legal_and_deterministic_move() {
        let b = Board::default();
        let l = SearchLimits {
            depth: 2,
            ..Default::default()
        };
        let a = best_move(&b, l).unwrap();
        assert_eq!(Some(a), best_move(&b, l));
        assert!(MoveGen::new_legal(&b).any(|m| m == a));
    }
    #[test]
    fn chaos_is_selectable() {
        assert_ne!(
            evaluate_with_style(&Board::default(), Style::Chaos),
            i32::MIN
        );
    }
    /// The published "kiwipete" position (Chess Programming Wiki). Castling, en passant,
    /// promotions and pins all appear within three plies.
    #[test]
    fn kiwipete_perft_matches_published_counts() {
        let board =
            Board::from_str("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
                .unwrap();
        assert_eq!(perft(&board, 1), 48);
        assert_eq!(perft(&board, 2), 2_039);
        assert_eq!(perft(&board, 3), 97_862);
    }

    /// The original repository called this position "kiwipete", but it is a variant (c5/d5
    /// pawns, Qd2, Nf3). Its counts are not published; this pins the generator's current
    /// values so any change is noticed.
    #[test]
    fn kiwipete_variant_perft_regression() {
        let board =
            Board::from_str("r3k2r/p1ppqpb1/bn2pnp1/2pP4/1p2P3/2N2N2/PPPQBPPP/R3K2R w KQkq - 0 1")
                .unwrap();
        assert_eq!(perft(&board, 1), 42);
        assert_eq!(perft(&board, 2), 1818);
    }
    /// Static exchange evaluation against hand-computed exchanges. These are arithmetic
    /// facts about the given positions, so they pin the algorithm rather than the tuning.
    #[test]
    fn see_scores_known_exchanges() {
        let cases: [(&str, &str, i32, &str); 7] = [
            (
                "pawn takes undefended pawn wins a pawn",
                "4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1",
                100,
                "e4d5",
            ),
            (
                "pawn takes pawn defended by a pawn is an even trade",
                "4k3/8/2p5/3p4/4P3/8/8/4K3 w - - 0 1",
                0,
                "e4d5",
            ),
            (
                "rook takes pawn defended by a pawn loses a rook for a pawn",
                "4k3/8/2p5/3p4/8/8/8/3RK3 w - - 0 1",
                -400,
                "d1d5",
            ),
            (
                "queen takes pawn defended by a pawn loses a queen for a pawn",
                "4k3/8/2p5/3p4/8/8/8/3QK3 w - - 0 1",
                -800,
                "d1d5",
            ),
            (
                "a quiet move captures nothing",
                "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1",
                0,
                "e2e3",
            ),
            (
                // With the black king on e8 it defends d8, so this is an even trade, not
                // a free rook. Keeping both cases guards the king-as-defender path.
                "rook takes rook defended by the enemy king is an even trade",
                "3rk3/8/8/8/8/8/8/3RK3 w - - 0 1",
                0,
                "d1d8",
            ),
            (
                "rook takes genuinely undefended rook wins a rook",
                "3r3k/8/8/8/8/8/8/3RK3 w - - 0 1",
                500,
                "d1d8",
            ),
        ];
        for (description, fen, expected, uci) in cases {
            let board = Board::from_str(fen).unwrap();
            let m = parse_move(&board, uci).unwrap();
            assert_eq!(see(&board, m), expected, "{description} ({fen}, {uci})");
        }
    }

    /// En passant captures a pawn that is not on the destination square; SEE has to model
    /// that explicitly or it reads the exchange as winning nothing.
    #[test]
    fn see_handles_en_passant() {
        let board = Board::from_str("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let m = parse_move(&board, "e5d6").unwrap();
        assert_eq!(see(&board, m), 100, "en passant wins the passed pawn");
    }

    /// A queen promotion that also captures should be valued as the promotion gain plus
    /// the captured piece, not merely the captured piece.
    #[test]
    fn see_accounts_for_promotion() {
        let board = Board::from_str("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let m = parse_move(&board, "a7b8q").unwrap();
        // Wins a rook (500) and upgrades a pawn to a queen (+800), then Black has no
        // recapture available from the king on e8.
        assert_eq!(see(&board, m), 1300);
    }

    /// Regression for the audited defect (MASTER_ENGINE_AUDIT.md F.1): quiescence
    /// recursed on every quiet check for up to 32 plies, so these standard positions
    /// could not finish a ONE-ply search in 600 seconds. Quiescence checks are now
    /// bounded by QS_CHECK_PLIES.
    ///
    /// The node ceilings are deliberately loose -- they are a "did the exponential blowup
    /// come back" alarm, not a tuning target.
    #[test]
    fn quiescence_terminates_in_open_positions() {
        let positions = [
            "r3k2r/p1ppqpb1/bn2pnp1/2pP4/1p2P3/2N2N2/PPPQBPPP/R3K2R w KQkq - 0 1",
            "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 1",
        ];
        for fen in positions {
            let board = Board::from_str(fen).unwrap();
            // The node cap is what turns a regression into a fast failure: without it, a
            // returning blowup hangs the test run instead of failing it (found by
            // mutation testing -- reintroducing unbounded checks hung for >10 minutes).
            let result = search(
                &board,
                SearchLimits {
                    depth: 4,
                    nodes: Some(1_000_000),
                    ..Default::default()
                },
            )
            .expect("a legal move exists");
            assert!(
                MoveGen::new_legal(&board).any(|m| m == result.best_move),
                "search returned an illegal move in {fen}"
            );
            assert_eq!(result.depth, 4, "did not complete depth 4 in {fen}");
            assert!(
                result.nodes < 1_000_000,
                "{fen}: {} nodes at depth 4 -- quiescence blowup has returned",
                result.nodes
            );
        }
    }

    /// Regression for MASTER_ENGINE_AUDIT.md §G.2: two different positions that map to
    /// the same slot must not see each other's entries.
    #[test]
    fn transposition_table_rejects_slot_collisions() {
        let mut table = Table::new(1);
        let len = table.entries.len() as u64;
        let key = 12_345;
        let colliding = key + len; // same slot, different position
        assert_eq!(table.slot(key), table.slot(colliding));

        table.put(Entry {
            key,
            depth: 9,
            generation: 0,
            score: 777,
            flag: Bound::Exact,
            best: None,
        });
        assert!(table.get(key).is_some(), "the stored position must be found");
        assert!(
            table.get(colliding).is_none(),
            "a different position in the same slot must not be returned"
        );
    }

    /// A mate score converted for storage at one ply and read back at the same ply must
    /// be unchanged, and ordinary scores must never be touched.
    #[test]
    fn tt_mate_scores_round_trip() {
        for ply in [0u8, 1, 7, 40] {
            for score in [
                MATE - 3,
                -MATE + 5,
                MATE_THRESHOLD + 1,
                -MATE_THRESHOLD - 1,
                0,
                250,
                -1_234,
            ] {
                assert_eq!(score_from_tt(score_to_tt(score, ply), ply), score);
            }
            assert_eq!(score_to_tt(250, ply), 250, "non-mate scores are ply-independent");
        }
        // Mate in 3 plies found at ply 5 is "mate at ply 8" from the root. Probed at
        // ply 2 the same position is still 3 plies from mate, i.e. mate at ply 5.
        let found = MATE - 8;
        let stored = score_to_tt(found, 5);
        assert_eq!(score_from_tt(stored, 2), MATE - 5);
    }

    /// Full-width minimax over the same quiescence search: no pruning, no windows, no TT.
    /// Slow, but its answer does not depend on any of the search machinery under test.
    fn reference_minimax(reference: &mut Searcher, board: &Board, depth: u8, ply: u8) -> i32 {
        match board.status() {
            BoardStatus::Checkmate => return -MATE + i32::from(ply),
            BoardStatus::Stalemate => return 0,
            BoardStatus::Ongoing => {}
        }
        if depth == 0 {
            return reference.quiescence(board, -INF, INF, ply, 0);
        }
        MoveGen::new_legal(board)
            .map(|m| -reference_minimax(reference, &board.make_move_new(m), depth - 1, ply + 1))
            .max()
            .unwrap()
    }

    /// Regression for MASTER_ENGINE_AUDIT.md §G.3: PVS, aspiration windows and the TT are
    /// all supposed to be *score-preserving* -- they change how much is searched, never
    /// the answer. So at shallow depth the reported score must equal plain minimax.
    ///
    /// Up to depth 2 this is exact, not approximate: ply-1 positions cannot transpose into
    /// each other, and depth-0 nodes never probe the TT, so no entry from a deeper search
    /// can substitute for a shallower one.
    ///
    /// The baseline scouted the first root move with a (31999, 32000) window and never
    /// re-searched it. At depth 1 that only truncates exchanges longer than two plies,
    /// which is why an earlier depth-1-only version of this test did not catch it (verified
    /// by mutation testing). At depth 2 it collapses the first move's whole subtree to
    /// static evaluation.
    #[test]
    fn shallow_search_score_equals_plain_minimax() {
        let fens = [
            STARTPOS,
            "r3k2r/p1ppqpb1/bn2pnp1/2pP4/1p2P3/2N2N2/PPPQBPPP/R3K2R w KQkq - 0 1",
            "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1",
            "4k3/8/2p5/3p4/8/8/8/3QK3 w - - 0 1",
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 1",
            // Constructed to expose §G.3. Qxc6+ is the only capture and gives check, so it
            // is always ordered first; it forks the king and the a8 rook. The rook is only
            // won in the *grandchild's* quiescence, after Black's forced king move -- which
            // is exactly the part a (31999, 32000) window cuts off with an immediate stand
            // pat. The general positions above happen not to depend on that, which is why
            // a test built only from them passed against the buggy code.
            "r3k3/8/2p5/8/8/8/8/2Q1K3 w - - 0 1",
        ];
        for fen in fens {
            let board = Board::from_str(fen).unwrap();
            for depth in 1..=2 {
                let limits = SearchLimits {
                    depth,
                    ..Default::default()
                };
                let mut reference = Searcher::new(limits);
                let exact = reference_minimax(&mut reference, &board, depth, 0);
                let got = search(&board, limits).unwrap();
                assert_eq!(
                    got.score, exact,
                    "{fen} depth {depth}: search reported {}, plain minimax says {exact}",
                    got.score
                );
            }
        }
    }

    /// History must stay bounded however many cutoffs a move produces, otherwise it
    /// eventually outranks the transposition-table move in ordering.
    #[test]
    fn history_is_bounded() {
        let mut searcher = Searcher::new(SearchLimits::default());
        let m = ChessMove::new(Square::G1, Square::F3, None);
        for _ in 0..100_000 {
            searcher.reward_history(m, 20);
        }
        let value = searcher.history[Square::G1.to_index()][Square::F3.to_index()];
        assert!(value <= HISTORY_MAX, "history grew to {value}");
        assert!(value > HISTORY_MAX / 2, "history should saturate near the cap, got {value}");
    }

    #[test]
    fn mate_scores_convert_to_uci_moves() {
        assert_eq!(mate_in_moves(MATE - 1), Some(1)); // we mate next move
        assert_eq!(mate_in_moves(MATE - 3), Some(2));
        assert_eq!(mate_in_moves(-MATE + 2), Some(-1)); // we are mated after their move
        assert_eq!(mate_in_moves(-MATE + 4), Some(-2));
        assert_eq!(mate_in_moves(250), None);
        assert_eq!(mate_in_moves(-MATE_THRESHOLD), None);
    }

    /// The stop flag must end a search that has no other limit, and the answer must be a
    /// searched move from a completed iteration.
    #[test]
    fn stop_flag_ends_an_unbounded_search() {
        let board = Board::from_str(
            "r3k2r/p1ppqpb1/bn2pnp1/2pP4/1p2P3/2N2N2/PPPQBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            flag.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let result = Engine::new(16)
            .search(
                &board,
                SearchLimits {
                    depth: MAX_DEPTH,
                    ..Default::default()
                },
                stop,
                &mut |_| {},
            )
            .unwrap();
        stopper.join().unwrap();
        assert!(started.elapsed() < Duration::from_millis(1_500), "{:?}", started.elapsed());
        assert!(result.depth >= 1);
        assert!(MoveGen::new_legal(&board).any(|m| m == result.best_move));
    }

    /// Every PV reported per iteration must be playable from the root, and must start with
    /// the move the iteration chose.
    #[test]
    fn reported_pvs_are_legal_and_consistent() {
        for fen in [
            STARTPOS,
            "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        ] {
            let board = Board::from_str(fen).unwrap();
            let mut infos = Vec::new();
            let result = Engine::new(16)
                .search(
                    &board,
                    SearchLimits {
                        depth: 6,
                        ..Default::default()
                    },
                    Arc::new(AtomicBool::new(false)),
                    &mut |info| infos.push(info.clone()),
                )
                .unwrap();
            assert_eq!(infos.len(), 6, "{fen}: one report per completed depth");
            assert_eq!(result.pv.first(), Some(&result.best_move), "{fen}");
            for info in &infos {
                let mut position = board;
                for m in &info.pv {
                    assert!(
                        MoveGen::new_legal(&position).any(|legal| legal == *m),
                        "{fen} depth {}: illegal pv move {m}",
                        info.depth
                    );
                    position = position.make_move_new(*m);
                }
            }
        }
    }

    /// The engine keeps its table between searches (that is its purpose in a game), and
    /// `clear` must actually empty it.
    #[test]
    fn engine_table_persists_between_searches_until_cleared() {
        let board = Board::default();
        let limits = SearchLimits {
            depth: 6,
            ..Default::default()
        };
        let mut engine = Engine::new(16);
        let no_stop = || Arc::new(AtomicBool::new(false));
        let cold = engine.search(&board, limits, no_stop(), &mut |_| {}).unwrap();
        let warm = engine.search(&board, limits, no_stop(), &mut |_| {}).unwrap();
        assert!(
            warm.nodes < cold.nodes,
            "second search should reuse the table: cold {} warm {}",
            cold.nodes,
            warm.nodes
        );
        engine.clear();
        let cleared = engine.search(&board, limits, no_stop(), &mut |_| {}).unwrap();
        assert_eq!(cleared.nodes, cold.nodes, "clear() must restore cold behaviour");
    }

    #[test]
    fn insufficient_material_follows_the_rules() {
        let dead = [
            ("8/8/8/4k3/8/8/8/4K3 w - - 0 1", "bare kings"),
            ("8/8/8/4k3/8/8/8/2B1K3 w - - 0 1", "king and bishop"),
            ("8/8/8/4k3/8/8/8/1N2K3 w - - 0 1", "king and knight"),
            ("5b2/8/8/4k3/8/8/8/2B1K3 w - - 0 1", "bishops on the same colour"),
        ];
        let alive = [
            ("8/8/8/4k3/8/8/8/1N2KN2 w - - 0 1", "two knights: mate is possible"),
            ("2b5/8/8/4k3/8/8/8/2B1K3 w - - 0 1", "bishops on opposite colours"),
            ("8/8/8/4k3/8/8/8/R3K3 w - - 0 1", "rook"),
            ("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1", "pawn"),
            ("8/8/8/4k3/8/8/8/1NB1K3 w - - 0 1", "knight and bishop"),
        ];
        for (fen, why) in dead {
            assert!(insufficient_material(&Board::from_str(fen).unwrap()), "{why}");
        }
        for (fen, why) in alive {
            assert!(!insufficient_material(&Board::from_str(fen).unwrap()), "{why}");
        }
    }

    #[test]
    fn a_dead_position_searches_as_a_draw() {
        let board = Board::from_str("8/8/8/4k3/8/8/8/2B1K3 w - - 0 1").unwrap();
        let result = search(&board, SearchLimits { depth: 4, ..Default::default() }).unwrap();
        assert_eq!(result.score, 0, "an extra bishop cannot win");
    }

    /// The repetition rule, on hand-built paths so each branch is pinned exactly.
    #[test]
    fn repetition_rule_distinguishes_search_from_history() {
        let mut s = Searcher::new(SearchLimits::default());
        let (a, b, c, d) = (1u64, 2, 3, 4);

        // The current position A also stands 4 plies back.
        s.path = vec![a, b, c, d, a];
        s.clocks = vec![4];
        assert!(s.is_repetition(4), "earlier occurrence inside the search: a draw at once");
        assert!(!s.is_repetition(1), "one earlier occurrence in game history: only twofold");

        // Two earlier occurrences in the game history make threefold.
        s.path = vec![a, b, c, d, a, b, c, d, a];
        s.clocks = vec![8];
        assert!(s.is_repetition(1));

        // A capture or pawn move in between (small clock) hides the old occurrences.
        s.clocks = vec![3];
        assert!(!s.is_repetition(1), "positions before an irreversible move cannot recur");

        // Occurrences with the other side to move never count.
        s.path = vec![a, b, c, a, d];
        s.clocks = vec![4];
        assert!(!s.is_repetition(4));
    }

    /// Black is a queen down, but the game history lets Black claim threefold repetition
    /// by returning the knight. With the history, the engine finds the draw. Without it,
    /// the same move looks as lost as every other.
    #[test]
    fn engine_uses_game_history_to_find_threefold_repetition() {
        let start =
            Board::from_str("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let mut position = Position::new(start);
        for text in ["g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1"] {
            let m = parse_move(&position.board, text).unwrap();
            position.play(m);
        }
        let limits = SearchLimits { depth: 4, ..Default::default() };
        let no_stop = || Arc::new(AtomicBool::new(false));

        let with_history = Engine::new(16)
            .search_position(&position, limits, no_stop(), &mut |_| {})
            .unwrap();
        assert_eq!(with_history.best_move.to_string(), "f6g8");
        assert_eq!(with_history.score, 0, "threefold repetition is a draw");

        let bare = Engine::new(16)
            .search_position(&Position::new(position.board), limits, no_stop(), &mut |_| {})
            .unwrap();
        assert!(bare.score < -500, "without history Black is simply a queen down: {}", bare.score);
    }

    /// With the clock at 99, any move that is not a capture or pawn move ends the game in
    /// a draw. The side that is winning must push the pawn.
    #[test]
    fn fifty_move_rule_makes_the_winning_side_reset_the_clock() {
        let board = Board::from_str("4k3/8/8/8/8/8/P7/R3K3 w - - 99 80").unwrap();
        let result = Engine::new(16)
            .search_position(
                &Position::with_clock(board, 99),
                SearchLimits { depth: 3, ..Default::default() },
                Arc::new(AtomicBool::new(false)),
                &mut |_| {},
            )
            .unwrap();
        let pawn_move = board.piece_on(result.best_move.get_source()) == Some(Piece::Pawn);
        assert!(pawn_move, "played {} and drew by the fifty-move rule", result.best_move);
        assert!(result.score > 300, "{}", result.score);
    }

    /// Checkmate takes precedence over the fifty-move rule when both happen on the same
    /// move.
    #[test]
    fn mate_on_the_hundredth_halfmove_is_still_mate() {
        let board = Board::from_str("7k/5Q2/6K1/8/8/8/8/8 w - - 99 80").unwrap();
        let result = Engine::new(16)
            .search_position(
                &Position::with_clock(board, 99),
                SearchLimits { depth: 2, ..Default::default() },
                Arc::new(AtomicBool::new(false)),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(result.best_move.to_string(), "f7g7");
        assert_eq!(mate_in_moves(result.score), Some(1));
    }

    #[test]
    fn evaluation_rewards_bishop_pair() {
        let bishops = Board::from_str("4k3/8/8/8/8/8/2BB4/4K3 w - - 0 1").unwrap();
        let bishop = Board::from_str("4k3/8/8/8/8/8/2B5/4K3 w - - 0 1").unwrap();
        assert!(evaluate(&bishops) > evaluate(&bishop));
    }
}
