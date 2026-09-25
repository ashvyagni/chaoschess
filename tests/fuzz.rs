//! Deterministic fuzzing: random games through the search, and malformed input through
//! the UCI front end.
//!
//! Seeded, so a failure reproduces exactly. No external crates: a small xorshift
//! generator is enough to reach positions no one would hand-pick. Those include
//! underpromotions, en passant after long shuffles, and castling rights lost in odd
//! orders.

use chess::{Board, BoardStatus, MoveGen};
use crazy_chess::{parse_move, search, Engine, Position, SearchLimits};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Play random games, alternating random moves with short engine searches, and check
/// every engine answer against the legal move list. Also checks that each reported PV is
/// playable and that the position bookkeeping (halfmove clock, history) stays consistent.
#[test]
fn random_games_never_yield_an_illegal_move() {
    let mut searched = 0;
    for seed in 1..=24u64 {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
        let mut position = Position::new(Board::default());
        let mut engine = Engine::new(1);
        for ply in 0..160 {
            if position.board.status() != BoardStatus::Ongoing || position.halfmove_clock >= 100 {
                break;
            }
            let legal: Vec<_> = MoveGen::new_legal(&position.board).collect();
            let m = if rng.below(3) == 0 {
                let result = engine
                    .search_position(
                        &position,
                        SearchLimits {
                            depth: 64,
                            nodes: Some(400),
                            ..Default::default()
                        },
                        Arc::new(AtomicBool::new(false)),
                        &mut |_| {},
                    )
                    .expect("a legal move exists");
                searched += 1;
                assert!(
                    legal.contains(&result.best_move),
                    "seed {seed} ply {ply}: illegal {} in {}",
                    result.best_move,
                    position.board
                );
                let mut b = position.board;
                for pv_move in &result.pv {
                    assert!(
                        MoveGen::new_legal(&b).any(|x| x == *pv_move),
                        "seed {seed} ply {ply}: illegal pv move {pv_move} in {b}"
                    );
                    b = b.make_move_new(*pv_move);
                }
                result.best_move
            } else {
                legal[rng.below(legal.len())]
            };
            let clock_before = position.halfmove_clock;
            let history_before = position.prior.len();
            position.play(m);
            assert!(
                position.halfmove_clock == 0 || position.halfmove_clock == clock_before + 1,
                "seed {seed} ply {ply}: clock jumped"
            );
            assert!(
                position.prior.is_empty() || position.prior.len() == history_before + 1,
                "seed {seed} ply {ply}: history bookkeeping broke"
            );
        }
    }
    assert!(searched > 300, "fuzzer barely exercised the search ({searched} searches)");
}

/// The free `search` on positions reached by random play, with fresh tables: the same
/// check without any state carried between searches.
#[test]
fn fresh_searches_on_random_positions_are_legal() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    for _ in 0..60 {
        let mut board = Board::default();
        for _ in 0..rng.below(90) {
            let legal: Vec<_> = MoveGen::new_legal(&board).collect();
            if legal.is_empty() {
                break;
            }
            board = board.make_move_new(legal[rng.below(legal.len())]);
        }
        if board.status() != BoardStatus::Ongoing {
            continue;
        }
        let result = search(
            &board,
            SearchLimits {
                depth: 3,
                hash_mb: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(MoveGen::new_legal(&board).any(|m| m == result.best_move), "{board}");
    }
}

/// parse_move must reject garbage cleanly, never panic, including multi-byte text
/// that a byte-indexed slice would cut through the middle of a character.
#[test]
fn parse_move_never_panics() {
    let board = Board::default();
    let inputs = [
        "", "e", "e2", "e2e", "e2e4q", "e2e4qq", "e7e8k", "i2i4", "e0e4", "e2e9", "a1a1",
        "é2e4", "e2é4", "ée4", "€€", "e2e4\u{0}", "♔e2e4", "e2\u{301}e4", "0000", "O-O",
        "e2-e4", "E2E4", "e2e4 ", " e2e4",
        // Five bytes, so they pass a byte-length check, but byte 2 falls inside a
        // multi-byte character: slicing [0..2] would panic.
        "e€4", "€e2", "e2e€",
    ];
    for text in inputs {
        let outcome = std::panic::catch_unwind(|| parse_move(&board, text));
        assert!(outcome.is_ok(), "parse_move panicked on {text:?}");
    }
    assert!(parse_move(&board, "e2e4").is_ok());
}

/// Throw malformed and hostile commands at the real binary. It must stay alive, keep
/// answering `isready`, and still play a legal move afterwards.
#[test]
fn uci_survives_malformed_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_crazy-chess"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut hostile: Vec<String> = [
        "position", "position fen", "position fen 8/8/8/8/8/8/8/8 w - - 0 1",
        "position fen rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR x KQkq - 0 1",
        "position fen rnbqkbnr/pppppppp/9/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "position fen rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - x y",
        "position fen 4k3/8/8/8/8/8/8/4K3 w - - 0 1 moves",
        "position startpos moves e2e5", "position startpos moves é2e4",
        "position startpos moves e2e4 e7e5 g1f3 b8c6 f1b5 zzzz",
        "position startpos moves e7e8q", "position banana",
        "go depth 0", "go depth -3", "go depth 99999999999999999999", "go nodes 0",
        "go movetime -1", "go wtime 0 btime 0", "go wtime abc", "go movestogo 0 wtime 100",
        "setoption", "setoption name", "setoption name Hash value -5",
        "setoption name Hash value 999999999", "setoption name Threads value zero",
        "setoption value 3", "setoption name Style value ", "setoption name  value",
        "stop", "stop", "ucinewgame", "\u{7f}\u{1b}[31m", "é€♔", "", "   ", "\t\t",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    // Plus random lines assembled from protocol words and junk.
    let vocab = [
        "position", "startpos", "fen", "moves", "go", "depth", "nodes", "movetime", "wtime",
        "btime", "winc", "binc", "movestogo", "infinite", "stop", "setoption", "name", "value",
        "Hash", "Style", "Chaos", "e2e4", "e7e5", "8/8/8/8/8/8/8/8", "w", "b", "-", "KQkq", "0",
        "1", "-1", "18446744073709551616", "é", "€", "isready", "ucinewgame", "d",
    ];
    let mut rng = Rng(0x0BAD_5EED);
    for _ in 0..400 {
        let words: Vec<&str> = (0..=rng.below(8)).map(|_| vocab[rng.below(vocab.len())]).collect();
        hostile.push(words.join(" "));
    }

    for line in &hostile {
        writeln!(stdin, "{line}").unwrap();
        // Keep searches from `go` lines short: stop right away.
        writeln!(stdin, "stop").unwrap();
    }
    stdin.flush().unwrap();

    // Synchronise on a sentinel only the engine can echo, and only after it has processed
    // every earlier line. `isready` won't do: the random input contains `isready` lines,
    // so an early `readyok` arrives while hostile `go` commands are still queued. The
    // first version of this test did that and read one of their stale bestmoves.
    writeln!(stdin, "setoption name fuzz-sentinel-7f3a").unwrap();
    stdin.flush().unwrap();
    let deadline = Duration::from_secs(30);
    let alive = loop {
        match rx.recv_timeout(deadline) {
            Ok(line) if line.contains("fuzz-sentinel-7f3a") => break true,
            Ok(_) => continue,
            Err(_) => break false,
        }
    };
    assert!(alive, "engine stopped answering after malformed input (crashed or hung)");

    writeln!(stdin, "position startpos moves e2e4\ngo depth 2").unwrap();
    stdin.flush().unwrap();
    let after_e4 = Board::from_str("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1").unwrap();
    let best = loop {
        let line = rx.recv_timeout(deadline).expect("bestmove after recovery");
        if let Some(mv) = line.strip_prefix("bestmove ") {
            break mv.to_string();
        }
    };
    assert!(parse_move(&after_e4, &best).is_ok(), "illegal {best} after recovery");
    let _ = child.kill();
}
