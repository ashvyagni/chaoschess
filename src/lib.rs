use chess::{Board, BoardStatus, ChessMove, Color, File, MoveGen, Piece, Rank, Square};
use std::thread;
use std::time::{Duration, Instant};

pub const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const INF: i32 = 32_000;
const MATE: i32 = 30_000;
const MAX_QUIESCENCE_PLY: u8 = 32;
const PIECE_VALUES: [i32; 6] = [100, 320, 330, 500, 900, 20_000];

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
    fn ordered(&self, board: &Board, tt: Option<ChessMove>) -> Vec<ChessMove> {
        let mut moves: Vec<_> = MoveGen::new_legal(board).collect();
        moves.sort_by_key(|m| {
            let capture = board
                .piece_on(m.get_dest())
                .map_or(0, |p| PIECE_VALUES[p.to_index()]);
            let victim = board
                .piece_on(m.get_source())
                .map_or(1, |p| PIECE_VALUES[p.to_index()]);
            let tt_bonus = if Some(*m) == tt { 1_000_000 } else { 0 };
            let check_bonus = if board.make_move_new(*m).checkers() == &chess::EMPTY {
                0
            } else {
                50_000
            };
            -(tt_bonus + check_bonus + capture * 10 - victim
                + self.history[m.get_source().to_index()][m.get_dest().to_index()])
        });
        moves
    }
    fn quiescence(&mut self, board: &Board, mut alpha: i32, beta: i32, ply: u8) -> i32 {
        self.nodes += 1;
        self.stop();
        if self.stopped {
            return 0;
        }
        if ply >= MAX_QUIESCENCE_PLY {
            return evaluate_with_style(board, self.limits.style);
        }
        if board.status() == BoardStatus::Checkmate {
            return -MATE + i32::from(ply);
        }
        let in_check = board.checkers() != &chess::EMPTY;
        let stand = evaluate_with_style(board, self.limits.style);
        if !in_check && stand >= beta {
            return stand;
        }
        if !in_check {
            alpha = alpha.max(stand);
        }
        for m in self.ordered(board, None) {
            let is_capture = board.piece_on(m.get_dest()).is_some() || m.get_promotion().is_some();
            if !in_check && !is_capture && board.make_move_new(m).checkers() == &chess::EMPTY {
                continue;
            }
            let score = -self.quiescence(&board.make_move_new(m), -beta, -alpha, ply + 1);
            if self.stopped {
                return 0;
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        alpha
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
            return self.quiescence(board, alpha, beta, ply);
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
        for m in self.ordered(board, tt.and_then(|e| e.best)) {
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
    let moves = searcher.ordered(board, None);
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
    #[test]
    fn evaluation_rewards_bishop_pair() {
        let bishops = Board::from_str("4k3/8/8/8/8/8/2BB4/4K3 w - - 0 1").unwrap();
        let bishop = Board::from_str("4k3/8/8/8/8/8/2B5/4K3 w - - 0 1").unwrap();
        assert!(evaluate(&bishops) > evaluate(&bishop));
    }
}
