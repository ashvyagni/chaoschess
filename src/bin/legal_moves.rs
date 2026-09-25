//! Reference oracle for differential testing: read one FEN per line on stdin, print its
//! legal moves (UCI notation, sorted, space-separated) on one line of stdout.
//!
//! Backed by the `chess` crate's generator, whose perft counts are verified by this
//! repository's tests. `tools/diff_movegen.py` uses it to find the exact positions where
//! another move generator disagrees, instead of only knowing that a perft total is off.
use chess::{Board, MoveGen};
use std::io::{self, BufRead, Write};
use std::str::FromStr;

fn main() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in io::stdin().lock().lines().map_while(Result::ok) {
        match Board::from_str(line.trim()) {
            Ok(board) => {
                let mut moves: Vec<String> = MoveGen::new_legal(&board).map(|m| m.to_string()).collect();
                moves.sort();
                writeln!(out, "{}", moves.join(" ")).unwrap();
            }
            Err(e) => writeln!(out, "ERROR {e:?}").unwrap(),
        }
        out.flush().unwrap();
    }
}
