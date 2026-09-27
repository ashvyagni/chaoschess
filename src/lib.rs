use chess::{
    get_adjacent_files, get_bishop_moves, get_file, get_king_moves, get_knight_moves, get_pawn_attacks, get_rook_moves,
    ALL_PIECES,
    BitBoard, Board, BoardStatus, ChessMove, Color, File, MoveGen, Piece, Rank, Square,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub mod arena;
pub mod eval;
pub mod mate;
pub mod position;
pub mod search;
pub mod see;
pub mod tt;
pub mod fen;
pub mod notation;
pub mod stats;
pub mod suites;
pub mod time;
pub mod uci;

// Public API: everything that was public at the crate root before the module split stays
// reachable at the same path. Crate-internal items are imported for the other modules,
// which reach them through `use super::*`.
pub use eval::*;
pub use mate::*;
pub use position::*;
pub use search::*;
#[allow(unused_imports)]
use see::*;
#[allow(unused_imports)]
use tt::*;

pub const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

const INF: i32 = 32_000;

const MATE: i32 = 30_000;

/// Any score beyond this magnitude is a mate score ("mate in N plies"), not an evaluation.
const MATE_THRESHOLD: i32 = MATE - 1_000;

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
                // Selective techniques are lossy by design; this test is about the
                // score-preserving machinery only. (It passed with them on until the
                // E13 evaluation change happened to trigger reverse futility pruning at
                // a depth-1 node, which showed that the premise no longer held.)
                let limits = SearchLimits {
                    depth,
                    selective: false,
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

    /// Regression for §G.8 / E14: an enemy pawn *behind* a pawn cannot stop it.
    /// Positions are built so exactly one pawn is passed, making each expected value
    /// exact: bonus = 10 + 8 * (ranks advanced).
    #[test]
    fn passed_pawns_ignore_enemy_pawns_behind() {
        // White e5 is passed: black d4 is behind it. Black d4 is blocked by white d3, and
        // white d3 by black d4. Only e5 scores: 10 + 8*4 = 42. The audited code
        // scored 0 here (d4 on an adjacent file counted as blocking e5).
        let behind = Board::from_str("4k3/8/8/4P3/3p4/3P4/8/4K3 w - - 0 1").unwrap();
        assert_eq!(passed_pawn_score(&behind), 42);
        // Black d7 is *ahead* of e5 on an adjacent file: e5 is not passed, and e5 blocks
        // d7 in turn. Nothing scores.
        let ahead = Board::from_str("4k3/3p4/8/4P3/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(passed_pawn_score(&ahead), 0);
        // Colour symmetry: mirrored lone pawns score as exact negatives.
        let white = Board::from_str("4k3/8/8/8/3P4/8/8/4K3 w - - 0 1").unwrap();
        let black = Board::from_str("4k3/8/8/3p4/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(passed_pawn_score(&white), 10 + 8 * 3);
        assert_eq!(passed_pawn_score(&white), -passed_pawn_score(&black));
    }

    /// The evaluator seam: an engine given a different evaluator must search with it.
    /// Material-only evaluation doesn't prefer centralisation, so from the start position
    /// it scores 0 and needs no piece-square knowledge to pick a move.
    #[test]
    fn engine_searches_with_a_plugged_in_evaluator() {
        struct MaterialOnly;
        impl Evaluator for MaterialOnly {
            fn evaluate(&self, board: &Board) -> i32 {
                let mut score = 0;
                for piece in chess::ALL_PIECES {
                    let value = PIECE_VALUES[piece.to_index()];
                    let own = (*board.pieces(piece) & *board.color_combined(board.side_to_move())).popcnt() as i32;
                    let theirs = (*board.pieces(piece) & *board.color_combined(!board.side_to_move())).popcnt() as i32;
                    score += value * (own - theirs);
                }
                score
            }
            fn name(&self) -> &'static str {
                "material-only"
            }
        }
        let limits = SearchLimits { depth: 3, ..Default::default() };
        let no_stop = || Arc::new(AtomicBool::new(false));
        let mut engine = Engine::new(4);
        engine.set_evaluator(Arc::new(MaterialOnly));
        let result = engine.search(&Board::default(), limits, no_stop(), &mut |_| {}).unwrap();
        assert_eq!(result.score, 0, "material-only sees nothing to gain in the start position");
        // A free queen is found through the plugged-in evaluator too.
        let hanging = Board::from_str("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1").unwrap();
        let won = engine.search(&hanging, limits, no_stop(), &mut |_| {}).unwrap();
        assert_eq!(won.best_move.to_string(), "d1d5");
        // Clearing restores the default: identical to a fresh engine's result.
        engine.clear_evaluator();
        engine.clear();
        let default = engine.search(&Board::default(), limits, no_stop(), &mut |_| {}).unwrap();
        assert_eq!(default, search(&Board::default(), SearchLimits { hash_mb: 4, ..limits }).unwrap());
    }

    /// E15: in a pawn ending the king belongs in the centre; with full material it
    /// belongs behind its pawns. One evaluation must say both.
    #[test]
    fn king_placement_depends_on_game_phase() {
        let eval = |fen: &str| evaluate(&Board::from_str(fen).unwrap());
        // Pawn ending: a central white king beats a cornered one.
        assert!(eval("4k3/4p3/8/8/3K4/8/4P3/8 w - - 0 1") > eval("4k3/4p3/8/8/8/8/4P3/K7 w - - 0 1"));
        // Full material: the castled king (g1) beats one marched to e3.
        let castled = "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQ1RK1 w - - 0 1";
        let exposed = "r1bq1rk1/pppp1ppp/2n2n2/2b1p3/2B1P3/2N1KN2/PPPP1PPP/R1BQ3R w - - 0 1";
        assert!(eval(castled) > eval(exposed));
        assert_eq!(game_phase(&Board::default()), MAX_PHASE);
        assert_eq!(game_phase(&Board::from_str("4k3/4p3/8/8/3K4/8/4P3/8 w - - 0 1").unwrap()), 0);
        // Blending is exact at the ends.
        for index in 0..64 {
            assert_eq!(tapered_king(index, MAX_PHASE), piece_square(Piece::King, index / 8, index % 8));
        }
    }

    #[test]
    fn evaluation_rewards_bishop_pair() {
        let bishops = Board::from_str("4k3/8/8/8/8/8/2BB4/4K3 w - - 0 1").unwrap();
        let bishop = Board::from_str("4k3/8/8/8/8/8/2B5/4K3 w - - 0 1").unwrap();
        assert!(evaluate(&bishops) > evaluate(&bishop));
    }
}
