//! The UCI protocol front end.
//!
//! The search runs on a worker thread, and this thread keeps reading commands. That's
//! what lets `stop`, `isready` and `quit` work during a search. In the audited baseline
//! the search ran inline on the stdin loop, so `stop` was a no-op and `go infinite`
//! never returned (MASTER_ENGINE_AUDIT.md §G.6).
//!
//! Ownership keeps it simple: the [`Engine`] (and its transposition table) moves into the
//! worker for the search and comes back through the join handle. No locks.

use crate::time::{allocate, Clock};
use crate::{mate_in_moves, parse_move, perft, search, Engine, SearchInfo, SearchLimits, Style};
use crate::{MAX_DEPTH, STARTPOS};
use chess::{Board, Color};
use std::io::{self, BufRead, Write};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const ENGINE_NAME: &str = "Crazy Chess";
const ENGINE_AUTHOR: &str = "ashvyagni";
const DEFAULT_HASH_MB: usize = 16;
const MAX_HASH_MB: usize = 1024;
const DEFAULT_OVERHEAD_MS: u64 = 30;

/// Run the UCI loop on stdin/stdout until `quit` or end of input.
pub fn run() {
    let mut session = Session::new();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if !session.handle(&line) {
            break;
        }
        let _ = io::stdout().flush();
    }
    session.stop_search();
}

struct Session {
    board: Board,
    /// Options that persist between `go` commands: style, threads, quiescence knobs.
    template: SearchLimits,
    hash_mb: usize,
    overhead: Duration,
    /// `None` exactly while a worker thread owns it.
    engine: Option<Engine>,
    worker: Option<JoinHandle<Engine>>,
    stop: Arc<AtomicBool>,
}

impl Session {
    fn new() -> Self {
        Self {
            board: Board::default(),
            template: SearchLimits {
                hash_mb: DEFAULT_HASH_MB,
                ..SearchLimits::default()
            },
            hash_mb: DEFAULT_HASH_MB,
            overhead: Duration::from_millis(DEFAULT_OVERHEAD_MS),
            engine: Some(Engine::new(DEFAULT_HASH_MB)),
            worker: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Handle one command line. Returns `false` when the engine should exit.
    fn handle(&mut self, line: &str) -> bool {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some((&command, args)) = tokens.split_first() else {
            return true;
        };
        match command {
            "uci" => {
                println!("id name {ENGINE_NAME}");
                println!("id author {ENGINE_AUTHOR}");
                println!("option name Style type combo default Classical var Classical var Chaos");
                println!(
                    "option name Hash type spin default {DEFAULT_HASH_MB} min 1 max {MAX_HASH_MB}"
                );
                println!("option name Threads type spin default 1 min 1 max 32");
                println!(
                    "option name Move Overhead type spin default {DEFAULT_OVERHEAD_MS} min 0 max 5000"
                );
                println!("option name Clear Hash type button");
                println!("uciok");
            }
            // Must be answered promptly even mid-search, which is why the search is on
            // another thread.
            "isready" => println!("readyok"),
            "ucinewgame" => {
                self.idle_engine().clear();
                self.board = Board::default();
            }
            "setoption" => self.set_option(args),
            "position" => {
                self.stop_search();
                match parse_position(args) {
                    Ok(board) => self.board = board,
                    Err(error) => println!("info string position rejected: {error}"),
                }
            }
            "go" => self.go(args),
            "stop" => self.stop_search(),
            "quit" => return false,
            // Non-standard conveniences, kept from the baseline because tools depend on them.
            "perft" => {
                self.stop_search();
                let depth = args.first().and_then(|v| v.parse().ok()).unwrap_or(1);
                println!("nodes {}", perft(&self.board, depth));
            }
            "bench" => {
                self.stop_search();
                // A fresh table, so a bench result doesn't depend on what was searched
                // before it. The output format is parsed by tools/audit_baseline.py.
                let depth = args.first().and_then(|v| v.parse().ok()).unwrap_or(4);
                let limits = SearchLimits {
                    depth,
                    hash_mb: self.hash_mb,
                    ..self.template
                };
                if let Some(result) = search(&self.board, limits) {
                    println!(
                        "bench depth {} nodes {} score cp {} bestmove {}",
                        result.depth, result.nodes, result.score, result.best_move
                    );
                }
            }
            "d" => println!("{}", self.board),
            _ => println!("info string unknown command: {command}"),
        }
        true
    }

    fn set_option(&mut self, args: &[&str]) {
        // Option names may contain spaces ("Move Overhead"), so take everything between
        // `name` and `value` rather than a single token. The baseline took one token.
        let name_at = args.iter().position(|&t| t == "name");
        let value_at = args.iter().position(|&t| t == "value");
        let Some(name_at) = name_at else { return };
        let name_end = value_at.unwrap_or(args.len());
        if name_end <= name_at {
            return;
        }
        let name = args[name_at + 1..name_end].join(" ");
        let value = value_at.map(|v| args[v + 1..].join(" "));
        let value = value.as_deref();

        let is = |expected: &str| name.eq_ignore_ascii_case(expected);
        if is("Style") {
            self.template.style = match value {
                Some(v) if v.eq_ignore_ascii_case("Chaos") => Style::Chaos,
                _ => Style::Classical,
            };
        } else if is("Hash") {
            if let Some(mb) = value.and_then(|v| v.parse::<usize>().ok()) {
                self.hash_mb = mb.clamp(1, MAX_HASH_MB);
                let hash_mb = self.hash_mb;
                self.idle_engine().resize(hash_mb);
            }
        } else if is("Threads") {
            if let Some(threads) = value.and_then(|v| v.parse::<usize>().ok()) {
                self.template.threads = threads.clamp(1, 32);
                if self.template.threads > 1 {
                    println!(
                        "info string Threads={} accepted; search is single-threaded until Lazy SMP lands",
                        self.template.threads
                    );
                }
            }
        } else if is("Move Overhead") {
            if let Some(ms) = value.and_then(|v| v.parse::<u64>().ok()) {
                self.overhead = Duration::from_millis(ms.min(5_000));
            }
        } else if is("Clear Hash") {
            self.idle_engine().clear();
        } else {
            println!("info string unknown option: {name}");
        }
    }

    fn go(&mut self, args: &[&str]) {
        self.stop_search();
        let params = GoParams::parse(args);
        let limits = params.limits(self.template, self.board.side_to_move(), self.overhead);
        let infinite = params.infinite;

        let mut engine = self.engine.take().expect("engine is idle after stop_search");
        let board = self.board;
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Arc::clone(&stop);

        self.worker = Some(thread::spawn(move || {
            let result = engine.search(&board, limits, Arc::clone(&stop), &mut print_info);
            if infinite {
                // UCI: under `go infinite`, bestmove must not be sent before `stop`, even
                // if the search has run out of depth.
                while !stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(1));
                }
            }
            match result {
                Some(r) => println!("bestmove {}", r.best_move),
                None => println!("bestmove 0000"),
            }
            let _ = io::stdout().flush();
            engine
        }));
    }

    /// End any running search and take the engine back. Safe to call when idle.
    fn stop_search(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.stop.store(true, Ordering::Relaxed);
            let engine = worker.join().expect("search thread panicked");
            self.engine = Some(engine);
        }
    }

    fn idle_engine(&mut self) -> &mut Engine {
        self.stop_search();
        self.engine.as_mut().expect("engine is idle after stop_search")
    }
}

fn print_info(info: &SearchInfo) {
    let ms = info.elapsed.as_millis().max(1);
    let score = match mate_in_moves(info.score) {
        Some(n) => format!("mate {n}"),
        None => format!("cp {}", info.score),
    };
    let pv: Vec<String> = info.pv.iter().map(ToString::to_string).collect();
    println!(
        "info depth {} seldepth {} multipv 1 score {} nodes {} nps {} hashfull {} time {} pv {}",
        info.depth,
        info.seldepth,
        score,
        info.nodes,
        u128::from(info.nodes) * 1000 / ms,
        info.hashfull,
        info.elapsed.as_millis(),
        pv.join(" ")
    );
    let _ = io::stdout().flush();
}

/// Parse `position startpos|fen <fields> [moves ...]` into a board.
///
/// Builds a fresh board and returns it only on full success, so a bad command leaves the
/// current position untouched. The baseline applied moves in place and could stop
/// halfway on an illegal move.
fn parse_position(args: &[&str]) -> Result<Board, String> {
    let moves_at = args.iter().position(|&t| t == "moves").unwrap_or(args.len());
    let (setup, moves) = args.split_at(moves_at);
    let mut board = match setup.split_first() {
        Some((&"startpos", _)) => Board::from_str(STARTPOS).expect("start position is valid"),
        Some((&"fen", fields)) => {
            if fields.len() < 4 {
                return Err(format!("FEN needs at least 4 fields, got {}", fields.len()));
            }
            // Accept FENs without the halfmove and fullmove counters, which some tools omit.
            let mut fen: Vec<&str> = fields.iter().take(6).copied().collect();
            if fen.len() == 4 {
                fen.push("0");
            }
            if fen.len() == 5 {
                fen.push("1");
            }
            Board::from_str(&fen.join(" ")).map_err(|e| format!("invalid FEN: {e:?}"))?
        }
        Some((other, _)) => return Err(format!("unknown position source {other}")),
        None => return Err("missing position source".to_string()),
    };
    for text in moves.iter().skip(1) {
        let m = parse_move(&board, text)?;
        board = board.make_move_new(m);
    }
    Ok(board)
}

/// The arguments of one `go` command.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct GoParams {
    wtime: Option<u64>,
    btime: Option<u64>,
    winc: Option<u64>,
    binc: Option<u64>,
    movestogo: Option<u32>,
    depth: Option<u8>,
    nodes: Option<u64>,
    movetime: Option<u64>,
    infinite: bool,
}

impl GoParams {
    fn parse(args: &[&str]) -> Self {
        let mut params = Self::default();
        let mut tokens = args.iter().peekable();
        while let Some(&token) = tokens.next() {
            let mut number = || tokens.next().and_then(|v| v.parse::<u64>().ok());
            match token {
                "wtime" => params.wtime = number(),
                "btime" => params.btime = number(),
                "winc" => params.winc = number(),
                "binc" => params.binc = number(),
                "movestogo" => params.movestogo = number().map(|n| n.min(u64::from(u32::MAX)) as u32),
                "depth" => params.depth = number().map(|n| n.clamp(1, u64::from(MAX_DEPTH)) as u8),
                "nodes" => params.nodes = number(),
                "movetime" => params.movetime = number(),
                "infinite" => params.infinite = true,
                _ => {} // ponder, mate, searchmoves: not supported yet, ignored safely
            }
        }
        params
    }

    fn limits(&self, template: SearchLimits, side: Color, overhead: Duration) -> SearchLimits {
        let mut limits = SearchLimits {
            depth: self.depth.unwrap_or(MAX_DEPTH),
            nodes: self.nodes,
            time: None,
            soft_time: None,
            ..template
        };
        if self.infinite {
            return limits;
        }
        let (remaining, increment) = match side {
            Color::White => (self.wtime, self.winc),
            Color::Black => (self.btime, self.binc),
        };
        if let Some(movetime) = self.movetime {
            limits.time = Some(Duration::from_millis(movetime).saturating_sub(overhead).max(Duration::from_millis(1)));
        } else if let Some(remaining) = remaining {
            let budget = allocate(Clock {
                remaining: Duration::from_millis(remaining),
                increment: Duration::from_millis(increment.unwrap_or(0)),
                moves_to_go: self.movestogo,
                overhead,
            });
            limits.time = Some(budget.hard);
            limits.soft_time = Some(budget.soft);
        }
        limits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_parses_every_supported_field() {
        let p = GoParams::parse(&[
            "wtime", "1000", "btime", "2000", "winc", "10", "binc", "20", "movestogo", "5",
            "depth", "7", "nodes", "999", "movetime", "300",
        ]);
        assert_eq!(p.wtime, Some(1000));
        assert_eq!(p.btime, Some(2000));
        assert_eq!(p.winc, Some(10));
        assert_eq!(p.binc, Some(20));
        assert_eq!(p.movestogo, Some(5));
        assert_eq!(p.depth, Some(7));
        assert_eq!(p.nodes, Some(999));
        assert_eq!(p.movetime, Some(300));
        assert!(!p.infinite);
        assert!(GoParams::parse(&["infinite"]).infinite);
    }

    #[test]
    fn clock_uses_the_side_to_move() {
        let template = SearchLimits::default();
        let p = GoParams::parse(&["wtime", "600000", "btime", "1000"]);
        let white = p.limits(template, Color::White, Duration::ZERO);
        let black = p.limits(template, Color::Black, Duration::ZERO);
        assert!(white.time.unwrap() > black.time.unwrap());
        assert!(black.time.unwrap() <= Duration::from_millis(600));
    }

    #[test]
    fn infinite_and_bare_go_have_no_time_limit() {
        let template = SearchLimits::default();
        for args in [&["infinite"][..], &[][..]] {
            let l = GoParams::parse(args).limits(template, Color::White, Duration::from_millis(30));
            assert_eq!(l.time, None);
            assert_eq!(l.depth, MAX_DEPTH);
        }
    }

    #[test]
    fn position_accepts_short_fens_and_rejects_bad_moves_atomically() {
        let b = parse_position(&["fen", "4k3/8/8/8/8/8/4P3/4K3", "w", "-", "-"]).unwrap();
        assert_eq!(b.side_to_move(), Color::White);
        let moved = parse_position(&["startpos", "moves", "e2e4", "e7e5"]).unwrap();
        assert_ne!(moved, Board::default());
        assert!(parse_position(&["startpos", "moves", "e2e4", "e2e4"]).is_err());
        assert!(parse_position(&["fen", "8/8/8"]).is_err());
    }
}
