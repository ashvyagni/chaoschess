//! The arena against real engine processes, including deliberately broken ones: each
//! failure (illegal move, stall, crash, flag) must lose the game with the right reason,
//! and never hang the arena.

use crazy_chess::arena::{play_game, to_pgn, EngineConfig, GameSettings, Opening, Outcome, TimeControl};
use crazy_chess::parse_move;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn ours(name: &str) -> EngineConfig {
    EngineConfig {
        name: name.to_string(),
        command: PathBuf::from(env!("CARGO_BIN_EXE_crazy-chess")),
        args: vec![],
        options: vec![("Hash".to_string(), "4".to_string())],
    }
}

/// A fake engine written in sh. `on_go` is the shell code run for every `go`.
fn fake(name: &str, on_go: &str) -> EngineConfig {
    let script = format!(
        r#"while IFS= read -r line; do
  case "$line" in
    uci) echo "id name {name}"; echo uciok;;
    isready) echo readyok;;
    go*) {on_go};;
    quit) exit 0;;
  esac
done"#
    );
    EngineConfig {
        name: name.to_string(),
        command: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_string(), script],
        options: vec![],
    }
}

fn nodes(n: u64) -> GameSettings {
    GameSettings {
        time_control: TimeControl::Nodes(n),
        max_plies: 60,
        stall_timeout: Duration::from_secs(2),
        ..GameSettings::default()
    }
}

#[test]
fn real_games_are_legal_and_consistent() {
    for line in ["e2e4 e7e5 g1f3 b8c6", "d2d4 d7d5 c2c4", "startpos"] {
        let opening = Opening::parse(line).unwrap();
        let game = play_game(&ours("A"), &ours("B"), &opening, &nodes(300));
        assert!(!game.termination.is_empty());
        // Replay every move from the opening position: all must be legal.
        let mut board = opening.position().unwrap().board;
        for mv in &game.moves {
            let m = parse_move(&board, &mv.uci).unwrap_or_else(|e| panic!("{line}: {e}"));
            board = board.make_move_new(m);
        }
        assert!(
            game.moves.len() == 60 || !game.termination.starts_with("adjudicated"),
            "{line}: {} plies, termination {:?}",
            game.moves.len(),
            game.termination
        );
        let pgn = to_pgn(&game, "test", 1, "2026.09.26", &TimeControl::Nodes(300));
        assert!(pgn.starts_with("[Event \"test\"]"));
        assert!(pgn.contains(&format!("[Result \"{}\"]", game.outcome.pgn())));
        assert!(pgn.trim_end().ends_with(game.outcome.pgn()));
    }
}

#[test]
fn checkmate_ends_the_game_for_the_right_side() {
    let opening = Opening::parse("fen 6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1").unwrap();
    let game = play_game(&ours("Mater"), &ours("Victim"), &opening, &nodes(2_000));
    assert_eq!(game.outcome, Outcome::WhiteWins);
    assert_eq!(game.termination, "checkmate");
    assert_eq!(game.moves.len(), 1);
    assert_eq!(game.moves[0].san, "Ra8#");
    let pgn = to_pgn(&game, "mate", 1, "2026.09.26", &TimeControl::Nodes(2_000));
    assert!(pgn.contains("[SetUp \"1\"]") && pgn.contains("Ra8#"), "{pgn}");
}

#[test]
fn an_illegal_move_loses() {
    let game = play_game(&fake("Cheater", "echo 'bestmove a1a1'"), &ours("B"), &Opening::parse("startpos").unwrap(), &nodes(100));
    assert_eq!(game.outcome, Outcome::BlackWins);
    assert!(game.termination.contains("illegal move"), "{}", game.termination);
}

#[test]
fn a_stalled_engine_loses_without_hanging_the_arena() {
    let started = Instant::now();
    let game = play_game(&ours("A"), &fake("Sleeper", ":"), &Opening::parse("startpos").unwrap(), &nodes(100));
    assert_eq!(game.outcome, Outcome::WhiteWins);
    assert!(game.termination.contains("stalled"), "{}", game.termination);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_crashing_engine_loses() {
    let game = play_game(&fake("Crasher", "exit 3"), &ours("B"), &Opening::parse("startpos").unwrap(), &nodes(100));
    assert_eq!(game.outcome, Outcome::BlackWins);
    assert!(game.termination.contains("crashed"), "{}", game.termination);
}

#[test]
fn flagging_loses_on_time() {
    let settings = GameSettings {
        time_control: TimeControl::Clock { base: Duration::from_millis(300), increment: Duration::ZERO },
        max_plies: 20,
        time_margin: Duration::from_millis(50),
        ..GameSettings::default()
    };
    let slow = fake("Slowpoke", "sleep 1; echo 'bestmove e2e4'");
    let game = play_game(&slow, &ours("B"), &Opening::parse("startpos").unwrap(), &settings);
    assert_eq!(game.outcome, Outcome::BlackWins);
    assert!(game.termination.contains("on time"), "{}", game.termination);
}

#[test]
fn flagging_against_a_bare_king_is_a_draw() {
    // White flags, but Black has only a king and cannot mate by any sequence (FIDE 6.9).
    let settings = GameSettings {
        time_control: TimeControl::Clock { base: Duration::from_millis(300), increment: Duration::ZERO },
        max_plies: 20,
        time_margin: Duration::from_millis(50),
        ..GameSettings::default()
    };
    let slow = fake("Slowpoke", "sleep 1; echo 'bestmove a1a2'");
    let opening = Opening::parse("fen 4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
    let game = play_game(&slow, &ours("B"), &opening, &settings);
    assert_eq!(game.outcome, Outcome::Draw, "{}", game.termination);
}

#[test]
fn a_real_clock_game_is_played_within_time() {
    let settings = GameSettings {
        time_control: TimeControl::Clock { base: Duration::from_secs(2), increment: Duration::from_millis(20) },
        max_plies: 40,
        ..GameSettings::default()
    };
    let started = Instant::now();
    let game = play_game(&ours("A"), &ours("B"), &Opening::parse("e2e4 c7c5").unwrap(), &settings);
    assert!(!game.termination.contains("on time"), "engine flagged: {}", game.termination);
    // Two 2 s clocks plus increments bound the game's length.
    assert!(started.elapsed() < Duration::from_secs(8), "{:?}", started.elapsed());
}
