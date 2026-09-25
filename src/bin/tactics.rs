//! Tactical suite reporter, and a sweep over the quiescence knobs.
//!
//! Reports solve rate, nodes and time for each configuration so the quiescence tradeoff
//! is decided by measurement. `--sweep` compares the audited baseline behaviour
//! (unbounded quiet checks, no SEE pruning) against the current defaults.

use chess::Board;
use crazy_chess::suites::MATE_SUITE;
use crazy_chess::{move_forces_mate, search, SearchLimits, Style, MAX_QUIESCENCE_PLY};
use std::str::FromStr;
use std::time::{Duration, Instant};

const PER_POSITION_CAP_S: u64 = 10;

struct Outcome {
    solved: usize,
    total: usize,
    nodes: u64,
    seconds: f64,
    unsolved: Vec<String>,
    capped: usize,
}

fn run(label: &str, limits_for: impl Fn(u8) -> SearchLimits, budget_s: f64) -> Outcome {
    let mut out = Outcome {
        solved: 0,
        total: 0,
        nodes: 0,
        seconds: 0.0,
        unsolved: Vec::new(),
        capped: 0,
    };
    for &(fen, mate_in) in MATE_SUITE {
        let board = Board::from_str(fen).expect("verified fen");
        out.total += 1;
        let start = Instant::now();
        let result = search(&board, limits_for(mate_in));
        let elapsed = start.elapsed().as_secs_f64();
        out.seconds += elapsed;
        if elapsed >= PER_POSITION_CAP_S as f64 {
            out.capped += 1;
        }
        match result {
            Some(r) => {
                out.nodes += r.nodes;
                if move_forces_mate(&board, r.best_move, mate_in) {
                    out.solved += 1;
                } else {
                    out.unsolved.push(format!(
                        "    {fen}  (mate in {mate_in}) played {} in {elapsed:.2}s",
                        r.best_move
                    ));
                }
            }
            None => out.unsolved.push(format!("    {fen}  no move returned")),
        }
        if out.seconds > budget_s {
            out.unsolved
                .push(format!("    ... aborted: exceeded {budget_s:.0}s total budget"));
            break;
        }
    }
    println!(
        "{label:38} {:>2}/{} solved  {:>13} nodes  {:>8.2}s  {} hit the {PER_POSITION_CAP_S}s cap",
        out.solved, out.total, fmt(out.nodes), out.seconds, out.capped
    );
    out
}

fn fmt(n: u64) -> String {
    let s = n.to_string();
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            o.push(',');
        }
        o.push(c);
    }
    o
}

fn main() {
    let sweep = std::env::args().any(|a| a == "--sweep");
    let budget = 400.0;

    println!("Tactical suite: {} machine-verified mate positions\n", MATE_SUITE.len());

    let configs: Vec<(String, SearchLimits)> = if sweep {
        vec![
            (
                "baseline: unbounded checks, no SEE".to_string(),
                SearchLimits {
                    qs_check_plies: MAX_QUIESCENCE_PLY,
                    qs_see_pruning: false,
                    ..Default::default()
                },
            ),
            (
                "checks<=0 (captures only), SEE on".to_string(),
                SearchLimits { qs_check_plies: 0, ..Default::default() },
            ),
            (
                "checks<=1, SEE on".to_string(),
                SearchLimits { qs_check_plies: 1, ..Default::default() },
            ),
            (
                "checks<=2, SEE on  (current default)".to_string(),
                SearchLimits::default(),
            ),
            (
                "checks<=4, SEE on".to_string(),
                SearchLimits { qs_check_plies: 4, ..Default::default() },
            ),
            (
                "checks<=2, SEE off".to_string(),
                SearchLimits { qs_see_pruning: false, ..Default::default() },
            ),
        ]
    } else {
        vec![
            ("Classical (default)".to_string(), SearchLimits::default()),
            (
                "Chaos (default)".to_string(),
                SearchLimits { style: Style::Chaos, ..Default::default() },
            ),
        ]
    };

    let mut all = Vec::new();
    for (label, base) in configs {
        // Per-position wall-clock cap. A capped search still returns its best move so far,
        // which is then checked like any other answer -- a timeout counts as unsolved
        // only if that move does not actually force mate.
        let outcome = run(
            &label,
            |mate_in| SearchLimits {
                depth: 2 * mate_in,
                time: Some(Duration::from_secs(PER_POSITION_CAP_S)),
                ..base
            },
            budget,
        );
        all.push((label, outcome));
    }

    println!();
    for (label, o) in &all {
        if !o.unsolved.is_empty() {
            println!("{label} — unsolved:");
            for u in &o.unsolved {
                println!("{u}");
            }
        }
    }
}
