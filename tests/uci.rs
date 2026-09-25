//! UCI protocol behaviour, tested against the real engine binary over pipes. This is the
//! same interface a GUI or tournament manager uses.
//!
//! Every read has a timeout. The engine under test has hung on these exact commands
//! before (MASTER_ENGINE_AUDIT.md §G.5, §G.6), and a hung test is worse than a failed one.

use chess::{Board, MoveGen};
use crazy_chess::parse_move;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

struct Engine {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Engine {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_crazy-chess"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("engine binary starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut engine = Self {
            child,
            stdin,
            lines,
            seen: Vec::new(),
        };
        engine.send("uci");
        engine.expect("uciok", Duration::from_secs(5));
        engine
    }

    fn send(&mut self, command: &str) {
        writeln!(self.stdin, "{command}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Wait for a line starting with `prefix`; return it and the time waited.
    fn expect(&mut self, prefix: &str, timeout: Duration) -> (String, Duration) {
        let start = Instant::now();
        loop {
            let left = timeout.saturating_sub(start.elapsed());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    self.seen.push(line.clone());
                    if line.starts_with(prefix) {
                        return (line, start.elapsed());
                    }
                }
                Err(_) => panic!(
                    "no line starting with {prefix:?} within {timeout:?}; saw:\n{}",
                    self.seen.join("\n")
                ),
            }
        }
    }

    fn bestmove(&mut self, board: &Board, timeout: Duration) -> (String, Duration) {
        let (line, waited) = self.expect("bestmove", timeout);
        let mv = line.split_whitespace().nth(1).unwrap().to_string();
        assert!(
            parse_move(board, &mv).is_ok(),
            "engine played illegal move {mv} in {board}"
        );
        (mv, waited)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn clock_time_control_is_respected() {
    // The baseline never answered this at all.
    let mut e = Engine::start();
    e.send("position startpos moves e2e4 e7e5");
    e.send("go wtime 3000 btime 3000 winc 0 binc 0");
    let board = Board::from_str("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2").unwrap();
    let (_, waited) = e.bestmove(&board, Duration::from_secs(3));
    // allocate() never lets a single move use more than 60% of the remaining time.
    assert!(waited < Duration::from_millis(1_900), "used {waited:?} of a 3 s clock");
}

#[test]
fn stop_ends_an_infinite_search_promptly() {
    let mut e = Engine::start();
    e.send("position startpos");
    e.send("go infinite");
    thread::sleep(Duration::from_millis(300));
    e.send("stop");
    let (_, waited) = e.bestmove(&Board::default(), Duration::from_secs(2));
    assert!(waited < Duration::from_millis(250), "stop took {waited:?}");
}

#[test]
fn infinite_search_withholds_bestmove_until_stop() {
    // UCI: even if the search finishes (here: a mate in 1 is found instantly and depth
    // runs out fast), `go infinite` must not send bestmove before `stop`.
    let mut e = Engine::start();
    e.send("position fen 6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1");
    e.send("go infinite depth 2");
    thread::sleep(Duration::from_millis(300));
    assert!(
        e.lines.try_iter().inspect(|l| e.seen.push(l.clone())).all(|l| !l.starts_with("bestmove")),
        "bestmove sent before stop"
    );
    e.send("stop");
    e.expect("bestmove", Duration::from_secs(2));
}

#[test]
fn isready_is_answered_during_a_search() {
    let mut e = Engine::start();
    e.send("position startpos");
    e.send("go infinite");
    thread::sleep(Duration::from_millis(100));
    e.send("isready");
    let (_, waited) = e.expect("readyok", Duration::from_secs(1));
    assert!(waited < Duration::from_millis(200), "readyok took {waited:?} mid-search");
    e.send("stop");
    e.expect("bestmove", Duration::from_secs(2));
}

#[test]
fn mate_is_reported_as_mate_with_a_pv() {
    let mut e = Engine::start();
    e.send("position fen 6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1");
    e.send("go depth 3");
    let board = Board::from_str("6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1").unwrap();
    let (mv, _) = e.bestmove(&board, Duration::from_secs(10));
    assert_eq!(mv, "a1a8");
    let last_info = e
        .seen
        .iter()
        .rev()
        .find(|l| l.starts_with("info depth"))
        .expect("at least one info line");
    assert!(last_info.contains("score mate 1"), "{last_info}");
    assert!(last_info.contains(" pv a1a8"), "{last_info}");
}

#[test]
fn every_reported_pv_is_a_legal_line() {
    let fen = "r3k2r/p1ppqpb1/bn2pnp1/2pP4/1p2P3/2N2N2/PPPQBPPP/R3K2R w KQkq - 0 1";
    let mut e = Engine::start();
    e.send(&format!("position fen {fen}"));
    e.send("go depth 6");
    e.expect("bestmove", Duration::from_secs(60));
    let infos: Vec<&String> = e.seen.iter().filter(|l| l.starts_with("info depth")).collect();
    assert!(infos.len() >= 6, "expected an info line per depth, got {}", infos.len());
    for info in infos {
        let pv = info.split(" pv ").nth(1).expect("pv field");
        let mut board = Board::from_str(fen).unwrap();
        for mv in pv.split_whitespace() {
            let m = parse_move(&board, mv).unwrap_or_else(|e| panic!("{info}\n  {e}"));
            board = board.make_move_new(m);
        }
    }
}

#[test]
fn quit_during_a_search_exits() {
    let mut e = Engine::start();
    e.send("position startpos");
    e.send("go infinite");
    thread::sleep(Duration::from_millis(100));
    e.send("quit");
    let start = Instant::now();
    loop {
        if e.child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2), "engine did not exit after quit");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_bad_position_command_keeps_the_previous_position() {
    let mut e = Engine::start();
    e.send("position startpos moves e2e4");
    e.send("position startpos moves e2e4 e2e4"); // second e2e4 is illegal
    e.expect("info string position rejected", Duration::from_secs(2));
    e.send("go depth 1");
    // Black to move after 1.e4 -- a white move here would mean the board was reset.
    let after_e4 =
        Board::from_str("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1").unwrap();
    e.bestmove(&after_e4, Duration::from_secs(5));
    assert!(MoveGen::new_legal(&after_e4).count() == 20);
}
