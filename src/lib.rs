use chess::{
    get_bishop_moves, get_king_moves, get_knight_moves, get_pawn_attacks, get_rook_moves,
    BitBoard, Board, BoardStatus, ChessMove, Color, File, MoveGen, Piece, Rank, Square,
};
use std::thread;
use std::time::{Duration, Instant};

pub mod suites;

pub const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const INF: i32 = 32_000;
const MATE: i32 = 30_000;
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
    pub time: Option<Duration>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchResult {
    pub best_move: ChessMove,
    pub depth: u8,
    pub score: i32,
    pub nodes: u64,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            depth: 6,
            nodes: None,
            time: None,
            hash_mb: 16,
            style: Style::Classical,
            threads: 1,
            qs_check_plies: QS_CHECK_PLIES,
            qs_see_pruning: true,
        }
    }
}

pub fn parse_move(board: &Board, coordinate: &str) -> Result<ChessMove, String> {
    if coordinate.len() < 4 || coordinate.len() > 5 {
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

struct Table {
    entries: Vec<Option<Entry>>,
}
impl Table {
    fn new(mb: usize) -> Self {
        let count = ((mb.max(1) * 1024 * 1024) / std::mem::size_of::<Option<Entry>>()).max(1);
        Self {
            entries: vec![None; count],
        }
    }
    fn get(&self, key: u64) -> Option<Entry> {
        self.entries[(key as usize) % self.entries.len()]
    }
    fn put(&mut self, entry: Entry) {
        let slot = (entry.key as usize) % self.entries.len();
        if self.entries[slot].is_none_or(|old| entry.depth >= old.depth) {
            self.entries[slot] = Some(entry);
        }
    }
}

struct Searcher {
    table: Table,
    limits: SearchLimits,
    start: Instant,
    nodes: u64,
    stopped: bool,
    history: [[i32; 64]; 64],
}

impl Searcher {
    fn stop(&mut self) {
        self.stopped |= self.limits.nodes.is_some_and(|n| self.nodes >= n);
        self.stopped |= self.limits.time.is_some_and(|t| self.start.elapsed() >= t);
    }
    /// Legal moves, best-first.
    ///
    /// `check_bonus` controls whether checking moves are promoted in the ordering. It
    /// costs a full board copy per move to find out, which is worth it in the main search
    /// (where it buys cutoffs over a large subtree) and not worth it in quiescence (where
    /// the subtree is shallow and the same information is recomputed immediately after).
    fn ordered(&self, board: &Board, tt: Option<ChessMove>, check_bonus: bool) -> Vec<ChessMove> {
        let mut moves: Vec<_> = MoveGen::new_legal(board).collect();
        moves.sort_by_key(|m| {
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
        self.stop();
        if self.stopped {
            return 0;
        }
        match board.status() {
            BoardStatus::Checkmate => return -MATE + i32::from(ply),
            BoardStatus::Stalemate => return 0,
            BoardStatus::Ongoing => {}
        }
        if depth == 0 {
            return self.quiescence(board, alpha, beta, ply, 0);
        }
        let key = board.get_hash();
        let tt = self.table.get(key);
        if let Some(entry) = tt.filter(|e| e.depth >= depth) {
            match entry.flag {
                Bound::Exact => return entry.score,
                Bound::Lower if entry.score >= beta => return entry.score,
                Bound::Upper if entry.score <= alpha => return entry.score,
                _ => {}
            }
        }
        let original_alpha = alpha;
        let mut best = None;
        let mut score = -INF;
        for m in self.ordered(board, tt.and_then(|e| e.best), true) {
            let value = -self.negamax(&board.make_move_new(m), depth - 1, -beta, -alpha, ply + 1);
            if self.stopped {
                return 0;
            }
            if value > score {
                score = value;
                best = Some(m);
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                self.history[m.get_source().to_index()][m.get_dest().to_index()] +=
                    i32::from(depth) * i32::from(depth);
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
            score,
            flag,
            best,
        });
        score
    }
}

pub fn best_move(board: &Board, limits: SearchLimits) -> Option<ChessMove> {
    search(board, limits).map(|result| result.best_move)
}

fn search_root(
    board: &Board,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    searcher: &mut Searcher,
) -> Option<(ChessMove, i32)> {
    let moves = searcher.ordered(board, None, true);
    let mut best = None;
    let mut best_score = -INF;
    for (index, m) in moves.into_iter().enumerate() {
        let child = board.make_move_new(m);
        let mut value = -searcher.negamax(&child, depth.saturating_sub(1), -alpha - 1, -alpha, 1);
        if index > 0 && value > alpha && value < beta && !searcher.stopped {
            value = -searcher.negamax(&child, depth.saturating_sub(1), -beta, -alpha, 1);
        }
        if searcher.stopped {
            break;
        }
        if value > best_score {
            best_score = value;
            best = Some(m);
        }
        alpha = alpha.max(value);
    }
    best.map(|m| (m, best_score))
}

fn parallel_root(
    board: &Board,
    depth: u8,
    limits: SearchLimits,
) -> Option<(ChessMove, i32, u64, bool)> {
    let moves: Vec<_> = MoveGen::new_legal(board).collect();
    let started = Instant::now();
    let results = thread::scope(|scope| {
        moves
            .iter()
            .map(|m| {
                scope.spawn(|| {
                    let mut local_limits = limits;
                    local_limits.threads = 1;
                    local_limits.time = limits.time.map(|t| t.saturating_sub(started.elapsed()));
                    let mut searcher = Searcher {
                        table: Table::new(local_limits.hash_mb),
                        limits: local_limits,
                        start: Instant::now(),
                        nodes: 0,
                        stopped: false,
                        history: [[0; 64]; 64],
                    };
                    let score = -searcher.negamax(
                        &board.make_move_new(*m),
                        depth.saturating_sub(1),
                        -INF,
                        INF,
                        1,
                    );
                    (*m, score, searcher.nodes, searcher.stopped)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().expect("root search thread panicked"))
            .collect::<Vec<_>>()
    });
    let stopped = results.iter().any(|(_, _, _, stopped)| *stopped);
    let nodes = results.iter().map(|(_, _, nodes, _)| *nodes).sum();
    let mut best = results.first().copied()?;
    for candidate in results.into_iter().skip(1) {
        if candidate.1 > best.1 {
            best = candidate;
        }
    }
    Some((best.0, best.1, nodes, stopped))
}

pub fn search(board: &Board, limits: SearchLimits) -> Option<SearchResult> {
    let fallback = MoveGen::new_legal(board).next()?;
    let mut result = fallback;
    let mut completed_depth = 0;
    let mut result_score = 0;
    let mut total_nodes = 0;
    let mut previous = 0;
    for depth in 1..=limits.depth.max(1) {
        let mut searcher = Searcher {
            table: Table::new(limits.hash_mb),
            limits,
            start: Instant::now(),
            nodes: 0,
            stopped: false,
            history: [[0; 64]; 64],
        };
        let candidate = if limits.threads > 1 {
            parallel_root(board, depth, limits).map(|(m, score, nodes, stopped)| {
                total_nodes += nodes;
                searcher.stopped = stopped;
                (m, score)
            })
        } else {
            let (alpha, beta) = if completed_depth > 0 {
                (previous - 40, previous + 40)
            } else {
                (-INF, INF)
            };
            let mut candidate = search_root(board, depth, alpha, beta, &mut searcher);
            if !searcher.stopped
                && completed_depth > 0
                && candidate
                    .as_ref()
                    .is_some_and(|(_, s)| *s <= alpha || *s >= beta)
            {
                candidate = search_root(board, depth, -INF, INF, &mut searcher);
            }
            total_nodes += searcher.nodes;
            candidate
        };
        if let Some((m, score)) = candidate {
            result = m;
            result_score = score;
            previous = score;
            completed_depth = depth;
        }
        if searcher.stopped {
            break;
        }
    }
    Some(SearchResult {
        best_move: result,
        depth: completed_depth,
        score: result_score,
        nodes: total_nodes,
    })
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
    #[test]
    fn kiwipete_perft_regression() {
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
            let result = search(
                &board,
                SearchLimits {
                    depth: 4,
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
                result.nodes < 5_000_000,
                "{fen}: {} nodes at depth 4 -- quiescence blowup has returned",
                result.nodes
            );
        }
    }

    #[test]
    fn evaluation_rewards_bishop_pair() {
        let bishops = Board::from_str("4k3/8/8/8/8/8/2BB4/4K3 w - - 0 1").unwrap();
        let bishop = Board::from_str("4k3/8/8/8/8/8/2B5/4K3 w - - 0 1").unwrap();
        assert!(evaluate(&bishops) > evaluate(&bishop));
    }
}
