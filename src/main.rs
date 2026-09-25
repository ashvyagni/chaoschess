use chess::Board;
use crazy_chess::{parse_move, perft, search, SearchLimits, Style};
use std::io::{self, BufRead, Write};
use std::str::FromStr;
use std::time::Duration;

fn main() {
    let stdin = io::stdin();
    let mut board = Board::default();
    let mut limits = SearchLimits::default();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let mut parts = line.split_whitespace();
        match parts.next().unwrap_or_default() {
            "uci" => {
                println!("id name Crazy Chess");
                println!("id author OpenAI");
                println!("option name Style type combo default Classical var Classical var Chaos");
                println!("option name Hash type spin default 16 min 1 max 1024");
                println!("option name Threads type spin default 1 min 1 max 32");
                println!("uciok");
            }
            "isready" => println!("readyok"),
            "ucinewgame" => board = Board::default(),
            "setoption" => {
                let values = parts.collect::<Vec<_>>();
                if let Some(name) = values.windows(2).find(|w| w[0] == "name").map(|w| w[1]) {
                    let value = values.windows(2).find(|w| w[0] == "value").map(|w| w[1]);
                    match name {
                        "Style" => {
                            limits.style = if value == Some("Chaos") {
                                Style::Chaos
                            } else {
                                Style::Classical
                            }
                        }
                        "Hash" => {
                            if let Some(hash) = value.and_then(|v| v.parse::<usize>().ok()) {
                                limits.hash_mb = hash.clamp(1, 1024);
                            }
                        }
                        "Threads" => {
                            if let Some(threads) = value.and_then(|v| v.parse::<usize>().ok()) {
                                limits.threads = threads.clamp(1, 32);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "position" => {
                if let Err(error) = set_position(&mut board, &mut parts) {
                    eprintln!("position error: {error}");
                }
            }
            "go" => {
                let args = parts.collect::<Vec<_>>();
                limits.nodes = None;
                limits.time = None;
                if let Some(depth) = args
                    .windows(2)
                    .find(|w| w[0] == "depth")
                    .and_then(|w| w[1].parse::<u8>().ok())
                {
                    limits.depth = depth.clamp(1, 64);
                }
                limits.nodes = args
                    .windows(2)
                    .find(|w| w[0] == "nodes")
                    .and_then(|w| w[1].parse().ok());
                limits.time = args
                    .windows(2)
                    .find(|w| w[0] == "movetime")
                    .and_then(|w| w[1].parse::<u64>().ok())
                    .map(Duration::from_millis);
                if args.contains(&"infinite") {
                    limits.time = None;
                    limits.depth = 64;
                }
                if let Some(result) = search(&board, limits) {
                    println!(
                        "info depth {} nodes {} score cp {}",
                        result.depth, result.nodes, result.score
                    );
                    println!("bestmove {}", result.best_move);
                } else {
                    println!("bestmove 0000");
                }
            }
            "perft" => {
                let depth = parts
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1);
                println!("nodes {}", perft(&board, depth));
            }
            "bench" => {
                let depth = parts
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(4);
                let mut bench_limits = limits;
                bench_limits.depth = depth;
                if let Some(result) = search(&board, bench_limits) {
                    println!(
                        "bench depth {} nodes {} score cp {} bestmove {}",
                        result.depth, result.nodes, result.score, result.best_move
                    );
                }
            }
            "d" => println!("{board}"),
            "stop" => {}
            "quit" => break,
            _ => {}
        }
        io::stdout().flush().expect("stdout should be writable");
    }
}

fn set_position<'a>(
    board: &mut Board,
    parts: &mut impl Iterator<Item = &'a str>,
) -> Result<(), String> {
    match parts.next() {
        Some("startpos") => *board = Board::default(),
        Some("fen") => {
            let fen = parts.by_ref().take(6).collect::<Vec<_>>().join(" ");
            *board = Board::from_str(&fen).map_err(|error| format!("invalid FEN: {error:?}"))?;
        }
        Some(value) => return Err(format!("unknown position source {value}")),
        None => return Err("missing position source".to_string()),
    }
    if parts.next() == Some("moves") {
        for coordinate in parts {
            let chess_move = parse_move(board, coordinate)?;
            *board = board.make_move_new(chess_move);
        }
    }
    Ok(())
}
