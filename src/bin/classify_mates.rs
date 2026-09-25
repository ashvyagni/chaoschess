//! Classify candidate tactical positions by their true shortest forced mate, using the
//! brute-force prover rather than the search under test. Output seeds tests/tactics.rs.
//!
//! Invalid FENs are reported rather than panicking, so a bad candidate is visible instead
//! of aborting the sweep.
use chess::Board;
use crazy_chess::shortest_forced_mate;
use std::str::FromStr;

fn main() {
    let limit: u8 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(3);
    let candidates = [
        // --- back rank ---
        "6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1",
        "7k/6pp/8/8/8/8/6PP/5R1K w - - 0 1",
        "6k1/5ppp/8/8/8/8/5PPP/1Q4K1 w - - 0 1",
        "6k1/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1",
        "6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1",
        "r5k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1",
        // --- rook + king vs king ---
        "k7/8/1K6/8/8/8/8/7R w - - 0 1",
        "3k4/8/3K4/8/8/8/8/7R w - - 0 1",
        "2k5/8/1K6/8/8/8/8/7R w - - 0 1",
        "4k3/8/4K3/8/8/8/8/7R w - - 0 1",
        "1k6/8/1K6/8/8/8/8/7R w - - 0 1",
        "k7/2K5/8/8/8/8/8/7R w - - 0 1",
        // --- two rooks (ladder) ---
        "7k/8/8/8/8/8/6R1/6RK w - - 0 1",
        "6k1/8/8/8/8/8/R7/1R5K w - - 0 1",
        "7k/8/8/8/8/8/R7/1R5K w - - 0 1",
        // --- queen + king vs king ---
        "7k/5Q2/6K1/8/8/8/8/8 w - - 0 1",
        "7k/8/6KQ/8/8/8/8/8 w - - 0 1",
        "k7/8/1KQ5/8/8/8/8/8 w - - 0 1",
        "8/8/8/8/8/2k5/8/K1Q5 w - - 0 1",
        "4k3/8/4KQ2/8/8/8/8/8 w - - 0 1",
        // --- knight / smothered ---
        "6rk/6pp/8/6N1/8/8/8/K5Q1 w - - 0 1",
        "5rk1/5ppp/8/6N1/8/8/8/K6Q w - - 0 1",
        // --- queen sacrifice / forcing ---
        "r5rk/5p1p/5R2/4Q3/8/8/7P/7K w - - 0 1",
        "1r3r1k/5Bpp/8/8/8/8/8/K6R w - - 0 1",
        "5rk1/5ppp/8/8/8/8/5PPP/3QR1K1 w - - 0 1",
        "2r3k1/5ppp/8/8/8/8/5PPP/2R3K1 w - - 0 1",
        // --- bishop pair / Boden-like ---
        "2kr4/8/8/8/8/8/8/K1B1B3 w - - 0 1",
        // --- black to move ---
        "r5k1/5ppp/8/8/8/8/5PPP/6K1 b - - 0 1",
        "6k1/5ppp/8/8/8/8/5PPP/4r1K1 b - - 0 1",
        "7K/6PP/8/8/8/8/6pp/5r1k b - - 0 1",
    ];

    println!("{:<62} {:>9}  {}", "fen", "mate in", "move");
    println!("{}", "-".repeat(86));
    let mut found = 0usize;
    for fen in candidates {
        match Board::from_str(fen) {
            Err(_) => println!("{fen:<62} {:>9}  -", "BAD FEN"),
            Ok(board) => match shortest_forced_mate(&board, limit) {
                Some((n, m)) => {
                    found += 1;
                    println!("{fen:<62} {n:>9}  {m}");
                }
                None => println!("{fen:<62} {:>9}  -", format!("none<={limit}")),
            },
        }
    }
    println!("\n{found}/{} candidates have a proven forced mate within {limit} moves",
             candidates.len());
}
