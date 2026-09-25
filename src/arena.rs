//! Engine-vs-engine games over real UCI: the machinery behind every strength claim.
//!
//! Each engine is a separate process spoken to exactly as a GUI would. The arena never
//! trusts an engine: every move is checked against the legal-move list, every reply is
//! awaited with a deadline, and games are ended by the rules rather than by the engines.
//! A move that isn't legal, a reply that never comes, or a process that dies all lose
//! the game, and the termination says why.

use crate::fen::parse_fen;
use crate::notation::to_san;
use crate::{insufficient_material, parse_move, Position, STARTPOS};
use chess::{Board, BoardStatus, Color, Piece};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// How to launch an engine and configure it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// UCI options sent as `setoption name <k> value <v>` after the handshake.
    pub options: Vec<(String, String)>,
}

/// How long each move may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeControl {
    /// Fixed node budget per move: reproducible across machines and loads.
    Nodes(u64),
    Depth(u8),
    MoveTime(Duration),
    /// Sudden death with increment: `base + inc` per player.
    Clock { base: Duration, increment: Duration },
}

impl FromStr for TimeControl {
    type Err = String;

    /// `nodes=N`, `depth=D`, `movetime=MS`, or `BASE+INC` in seconds (e.g. `10+0.1`).
    fn from_str(text: &str) -> Result<Self, String> {
        let number = |v: &str| v.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0);
        if let Some(v) = text.strip_prefix("nodes=") {
            return v.parse().map(TimeControl::Nodes).map_err(|_| format!("bad node count {v:?}"));
        }
        if let Some(v) = text.strip_prefix("depth=") {
            return v.parse().map(TimeControl::Depth).map_err(|_| format!("bad depth {v:?}"));
        }
        if let Some(v) = text.strip_prefix("movetime=") {
            return v
                .parse()
                .map(|ms| TimeControl::MoveTime(Duration::from_millis(ms)))
                .map_err(|_| format!("bad movetime {v:?}"));
        }
        if let Some((base, inc)) = text.split_once('+') {
            if let (Some(base), Some(inc)) = (number(base), number(inc)) {
                if base > 0.0 {
                    return Ok(TimeControl::Clock {
                        base: Duration::from_secs_f64(base),
                        increment: Duration::from_secs_f64(inc),
                    });
                }
            }
        }
        Err(format!("unrecognised time control {text:?} (nodes=N, depth=D, movetime=MS, BASE+INC)"))
    }
}

impl std::fmt::Display for TimeControl {
    /// PGN `TimeControl` tag where one exists, otherwise a readable description.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeControl::Nodes(n) => write!(f, "nodes={n}"),
            TimeControl::Depth(d) => write!(f, "depth={d}"),
            TimeControl::MoveTime(t) => write!(f, "movetime={}", t.as_millis()),
            TimeControl::Clock { base, increment } => {
                write!(f, "{}+{}", base.as_secs_f64(), increment.as_secs_f64())
            }
        }
    }
}

/// A starting point for a game: a position plus opening moves already played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    /// `None` = standard start position.
    pub fen: Option<String>,
    pub moves: Vec<String>,
}

impl Opening {
    /// Parse one line: `startpos [moves ...]`, `fen <FEN> [moves ...]`, or a bare list of
    /// UCI moves from the start position. Every move is checked for legality here, so a
    /// bad opening file fails before any game starts.
    pub fn parse(line: &str) -> Result<Self, String> {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let moves_at = tokens.iter().position(|&t| t == "moves");
        let (fen, moves): (Option<String>, Vec<String>) = match tokens.first() {
            Some(&"startpos") => (None, tail(&tokens, moves_at)),
            Some(&"fen") => {
                let end = moves_at.unwrap_or(tokens.len());
                (Some(tokens[1..end].join(" ")), tail(&tokens, moves_at))
            }
            Some(_) => (None, tokens.iter().map(|t| t.to_string()).collect()),
            None => return Err("empty opening".to_string()),
        };
        let opening = Opening { fen, moves };
        opening.position()?;
        Ok(opening)
    }

    /// The position after the opening, with history.
    pub fn position(&self) -> Result<Position, String> {
        let mut position = match &self.fen {
            None => Position::new(Board::from_str(STARTPOS).expect("start position")),
            Some(fen) => {
                let parsed = parse_fen(fen)?;
                Position::with_clock(parsed.board, parsed.halfmove_clock)
            }
        };
        for text in &self.moves {
            let m = parse_move(&position.board, text)?;
            position.play(m);
        }
        Ok(position)
    }

    fn uci_setup(&self) -> String {
        match &self.fen {
            None => "position startpos".to_string(),
            Some(fen) => format!("position fen {fen}"),
        }
    }
}

fn tail(tokens: &[&str], moves_at: Option<usize>) -> Vec<String> {
    moves_at.map_or(Vec::new(), |i| tokens[i + 1..].iter().map(|t| t.to_string()).collect())
}

/// Why an engine could not produce a move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineFailure {
    Timeout,
    Crashed,
    Protocol(String),
}

/// A running engine process.
pub struct UciProcess {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl UciProcess {
    /// Start the process and complete the UCI handshake, including options.
    pub fn start(config: &EngineConfig, timeout: Duration) -> Result<Self, EngineFailure> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| EngineFailure::Protocol(format!("cannot start {:?}: {e}", config.command)))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut engine = Self { child, stdin, lines };
        engine.send("uci")?;
        engine.wait_for(|l| l == "uciok", timeout)?;
        for (name, value) in &config.options {
            engine.send(&format!("setoption name {name} value {value}"))?;
        }
        engine.sync(timeout)?;
        Ok(engine)
    }

    fn send(&mut self, command: &str) -> Result<(), EngineFailure> {
        writeln!(self.stdin, "{command}")
            .and_then(|()| self.stdin.flush())
            .map_err(|_| EngineFailure::Crashed)
    }

    /// Read until a line satisfies `done`; return it and every line before it.
    fn wait_for(
        &mut self,
        done: impl Fn(&str) -> bool,
        timeout: Duration,
    ) -> Result<(String, Vec<String>), EngineFailure> {
        let deadline = Instant::now() + timeout;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) if done(&line) => return Ok((line, seen)),
                Ok(line) => seen.push(line),
                Err(RecvTimeoutError::Timeout) => return Err(EngineFailure::Timeout),
                Err(RecvTimeoutError::Disconnected) => return Err(EngineFailure::Crashed),
            }
        }
    }

    fn sync(&mut self, timeout: Duration) -> Result<(), EngineFailure> {
        self.send("isready")?;
        self.wait_for(|l| l == "readyok", timeout).map(|_| ())
    }

    pub fn new_game(&mut self, timeout: Duration) -> Result<(), EngineFailure> {
        self.send("ucinewgame")?;
        self.sync(timeout)
    }

    /// Send a position and `go`, and wait for `bestmove`. Returns the move text, the
    /// `info` lines of this search, and the time taken.
    pub fn think(
        &mut self,
        position: &str,
        go: &str,
        timeout: Duration,
    ) -> Result<(String, Vec<String>, Duration), EngineFailure> {
        self.send(position)?;
        self.send(go)?;
        let started = Instant::now();
        let (line, infos) = self.wait_for(|l| l.starts_with("bestmove"), timeout)?;
        let elapsed = started.elapsed();
        let mv = line
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| EngineFailure::Protocol(format!("malformed {line:?}")))?
            .to_string();
        Ok((mv, infos, elapsed))
    }
}

impl Drop for UciProcess {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "quit");
        let _ = self.stdin.flush();
        let deadline = Instant::now() + Duration::from_millis(200);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One move of a finished game.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveRecord {
    pub uci: String,
    pub san: String,
    /// The engine's last reported score in centipawns (mate scores as ±100000 ∓ N), from
    /// the mover's point of view, if it reported one.
    pub score_cp: Option<i32>,
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    pub millis: u128,
}

/// Game outcome from White's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    WhiteWins,
    BlackWins,
    Draw,
}

impl Outcome {
    pub fn pgn(&self) -> &'static str {
        match self {
            Outcome::WhiteWins => "1-0",
            Outcome::BlackWins => "0-1",
            Outcome::Draw => "1/2-1/2",
        }
    }

    /// Points scored by the given colour.
    pub fn points_for(&self, color: Color) -> f64 {
        match (self, color) {
            (Outcome::Draw, _) => 0.5,
            (Outcome::WhiteWins, Color::White) | (Outcome::BlackWins, Color::Black) => 1.0,
            _ => 0.0,
        }
    }

    fn win_for(color: Color) -> Self {
        match color {
            Color::White => Outcome::WhiteWins,
            Color::Black => Outcome::BlackWins,
        }
    }
}

/// A finished game.
#[derive(Debug, Clone, PartialEq)]
pub struct GameRecord {
    pub white: String,
    pub black: String,
    pub opening: Opening,
    pub outcome: Outcome,
    /// Human-readable reason, e.g. "checkmate", "threefold repetition",
    /// "Black lost on time".
    pub termination: String,
    pub moves: Vec<MoveRecord>,
}

/// Game-level settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameSettings {
    pub time_control: TimeControl,
    /// Adjudicate a draw after this many plies from the opening position.
    pub max_plies: u32,
    /// Extra time an engine may overrun a clock by before it loses on time.
    pub time_margin: Duration,
    /// How long to wait for `bestmove` under node or depth limits.
    pub stall_timeout: Duration,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            time_control: TimeControl::Nodes(10_000),
            max_plies: 400,
            time_margin: Duration::from_millis(100),
            stall_timeout: Duration::from_secs(60),
        }
    }
}

/// Whether `color` has enough material to checkmate an opponent that helps it. Used for
/// the FIDE rule that losing on time is only a loss if the opponent could still mate.
fn can_mate(board: &Board, color: Color) -> bool {
    let own = *board.color_combined(color);
    let heavy = *board.pieces(Piece::Pawn) | *board.pieces(Piece::Rook) | *board.pieces(Piece::Queen);
    if own & heavy != chess::EMPTY {
        return true;
    }
    let minors = own & (*board.pieces(Piece::Knight) | *board.pieces(Piece::Bishop));
    minors.popcnt() >= 2
}

/// Why the game is over according to the rules, if it is.
fn rules_verdict(position: &Position) -> Option<(Outcome, String)> {
    let board = &position.board;
    match board.status() {
        BoardStatus::Checkmate => {
            return Some((Outcome::win_for(!board.side_to_move()), "checkmate".to_string()))
        }
        BoardStatus::Stalemate => return Some((Outcome::Draw, "stalemate".to_string())),
        BoardStatus::Ongoing => {}
    }
    if position.halfmove_clock >= 100 {
        return Some((Outcome::Draw, "fifty-move rule".to_string()));
    }
    let current = board.get_hash();
    if position.prior.iter().filter(|&&h| h == current).count() >= 2 {
        return Some((Outcome::Draw, "threefold repetition".to_string()));
    }
    if insufficient_material(board) {
        return Some((Outcome::Draw, "insufficient material".to_string()));
    }
    None
}

fn parse_info(infos: &[String]) -> (Option<i32>, Option<u32>, Option<u64>) {
    let (mut score, mut depth, mut nodes) = (None, None, None);
    for line in infos.iter().filter(|l| l.starts_with("info ")) {
        let t: Vec<&str> = line.split_whitespace().collect();
        for i in 0..t.len().saturating_sub(1) {
            match t[i] {
                "depth" => depth = t[i + 1].parse().ok().or(depth),
                "nodes" => nodes = t[i + 1].parse().ok().or(nodes),
                "score" if i + 2 < t.len() => {
                    if let Ok(n) = t[i + 2].parse::<i32>() {
                        score = Some(match t[i + 1] {
                            "mate" if n > 0 => 100_000 - n,
                            "mate" => -100_000 - n,
                            _ => n,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    (score, depth, nodes)
}

/// Play one game. Engines are started fresh, so one game can't leak state into the next.
pub fn play_game(
    white: &EngineConfig,
    black: &EngineConfig,
    opening: &Opening,
    settings: &GameSettings,
) -> GameRecord {
    let mut record = GameRecord {
        white: white.name.clone(),
        black: black.name.clone(),
        opening: opening.clone(),
        outcome: Outcome::Draw,
        termination: String::new(),
        moves: Vec::new(),
    };
    let startup = Duration::from_secs(10);
    let mut engines = Vec::with_capacity(2);
    for (config, color) in [(white, Color::White), (black, Color::Black)] {
        match UciProcess::start(config, startup).and_then(|mut e| e.new_game(startup).map(|()| e)) {
            Ok(engine) => engines.push(engine),
            Err(failure) => {
                record.outcome = Outcome::win_for(!color);
                record.termination = format!("{} failed to start: {failure:?}", color_name(color));
                return record;
            }
        }
    }

    let mut position = opening.position().expect("openings are validated on load");
    let setup = opening.uci_setup();
    let mut played: Vec<String> = opening.moves.clone();
    let mut clock = match settings.time_control {
        TimeControl::Clock { base, .. } => Some([base, base]),
        _ => None,
    };

    for _ in 0..settings.max_plies {
        if let Some((outcome, why)) = rules_verdict(&position) {
            record.outcome = outcome;
            record.termination = why;
            return record;
        }
        let side = position.board.side_to_move();
        let index = if side == Color::White { 0 } else { 1 };
        let (go, timeout) = match (settings.time_control, clock) {
            (TimeControl::Nodes(n), _) => (format!("go nodes {n}"), settings.stall_timeout),
            (TimeControl::Depth(d), _) => (format!("go depth {d}"), settings.stall_timeout),
            (TimeControl::MoveTime(t), _) => {
                (format!("go movetime {}", t.as_millis()), t + settings.time_margin + Duration::from_secs(1))
            }
            (TimeControl::Clock { increment, .. }, Some([w, b])) => (
                format!(
                    "go wtime {} btime {} winc {} binc {}",
                    w.as_millis(),
                    b.as_millis(),
                    increment.as_millis(),
                    increment.as_millis()
                ),
                [w, b][index] + settings.time_margin,
            ),
            (TimeControl::Clock { .. }, None) => unreachable!("clock is set for clock games"),
        };
        let command = if played.is_empty() {
            setup.clone()
        } else {
            format!("{setup} moves {}", played.join(" "))
        };
        let reply = engines[index].think(&command, &go, timeout);
        let (text, infos, elapsed) = match reply {
            Ok(r) => r,
            Err(failure) => {
                let (outcome, what) = match failure {
                    EngineFailure::Timeout if clock.is_some() && !can_mate(&position.board, !side) => {
                        (Outcome::Draw, "lost on time, but the opponent cannot mate".to_string())
                    }
                    EngineFailure::Timeout if clock.is_some() => {
                        (Outcome::win_for(!side), "lost on time".to_string())
                    }
                    EngineFailure::Timeout => (Outcome::win_for(!side), "stalled (no bestmove)".to_string()),
                    EngineFailure::Crashed => (Outcome::win_for(!side), "crashed".to_string()),
                    EngineFailure::Protocol(p) => (Outcome::win_for(!side), format!("protocol error: {p}")),
                };
                record.outcome = outcome;
                record.termination = format!("{} {what}", color_name(side));
                return record;
            }
        };
        if let (Some(times), TimeControl::Clock { increment, .. }) = (clock.as_mut(), settings.time_control) {
            if elapsed > times[index] + settings.time_margin {
                record.outcome = if can_mate(&position.board, !side) {
                    Outcome::win_for(!side)
                } else {
                    Outcome::Draw
                };
                record.termination = format!("{} lost on time", color_name(side));
                return record;
            }
            times[index] = times[index].saturating_sub(elapsed) + increment;
        }
        let m = match parse_move(&position.board, &text) {
            Ok(m) => m,
            Err(_) => {
                record.outcome = Outcome::win_for(!side);
                record.termination = format!("{} played an illegal move: {text}", color_name(side));
                return record;
            }
        };
        let (score_cp, depth, nodes) = parse_info(&infos);
        record.moves.push(MoveRecord {
            uci: text.clone(),
            san: to_san(&position.board, m),
            score_cp,
            depth,
            nodes,
            millis: elapsed.as_millis(),
        });
        position.play(m);
        played.push(text);
    }
    record.outcome = rules_verdict(&position).map_or(Outcome::Draw, |(o, _)| o);
    record.termination = rules_verdict(&position)
        .map_or_else(|| format!("adjudicated draw after {} plies", settings.max_plies), |(_, why)| why);
    record
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "White",
        Color::Black => "Black",
    }
}

/// Render a game as PGN (export format).
///
/// The opening moves are included in the movetext, marked `{book}`, and the game starts
/// from the standard position unless the opening itself starts from a FEN. That keeps the
/// move numbers right for any reader. The first version emitted a `FEN` tag for the
/// position after the book, but the crate's `Board` drops the move counters, so the tag
/// said "move 1" while the movetext said "4.".
pub fn to_pgn(game: &GameRecord, event: &str, round: usize, date: &str, time_control: &TimeControl) -> String {
    let mut pgn = String::new();
    let tags = [
        ("Event", event.to_string()),
        ("Site", "chaoschess arena".to_string()),
        ("Date", date.to_string()),
        ("Round", round.to_string()),
        ("White", game.white.clone()),
        ("Black", game.black.clone()),
        ("Result", game.outcome.pgn().to_string()),
        ("TimeControl", time_control.to_string()),
        ("Termination", game.termination.clone()),
    ];
    for (k, v) in tags {
        let _ = writeln!(pgn, "[{k} \"{}\"]", v.replace('\\', "\\\\").replace('"', "\\\""));
    }
    let (mut board, mut fullmove) = match &game.opening.fen {
        None => (Board::from_str(STARTPOS).expect("start position"), 1usize),
        Some(fen) => {
            let parsed = parse_fen(fen).expect("validated opening");
            let fields: Vec<&str> = fen.split_whitespace().collect();
            let full = format!(
                "{} {} {} {} {} {}",
                fields[0], fields[1], fields[2], fields[3], parsed.halfmove_clock, parsed.fullmove_number
            );
            let _ = writeln!(pgn, "[SetUp \"1\"]");
            let _ = writeln!(pgn, "[FEN \"{full}\"]");
            (parsed.board, parsed.fullmove_number as usize)
        }
    };
    pgn.push('\n');

    let book = game.opening.moves.iter().map(|uci| (uci.as_str(), "book".to_string()));
    let played = game.moves.iter().map(|mv| {
        let mut comment = String::new();
        if let Some(cp) = mv.score_cp {
            let _ = write!(comment, "{:+.2}", f64::from(cp) / 100.0);
            if let Some(d) = mv.depth {
                let _ = write!(comment, "/{d}");
            }
            comment.push(' ');
        }
        let _ = write!(comment, "{:.3}s", mv.millis as f64 / 1000.0);
        (mv.uci.as_str(), comment)
    });
    let mut tokens: Vec<String> = Vec::new();
    for (i, (uci, comment)) in book.chain(played).enumerate() {
        let white = board.side_to_move() == Color::White;
        if white {
            tokens.push(format!("{fullmove}."));
        } else if i == 0 {
            tokens.push(format!("{fullmove}..."));
        }
        let m = parse_move(&board, uci).expect("recorded moves are legal");
        tokens.push(to_san(&board, m));
        tokens.push(format!("{{{comment}}}"));
        if !white {
            fullmove += 1;
        }
        board = board.make_move_new(m);
    }
    tokens.push(game.outcome.pgn().to_string());

    let mut line = String::new();
    for token in tokens {
        if line.len() + token.len() + 1 > 79 && !line.is_empty() {
            pgn.push_str(line.trim_end());
            pgn.push('\n');
            line.clear();
        }
        line.push_str(&token);
        line.push(' ');
    }
    pgn.push_str(line.trim_end());
    pgn.push_str("\n\n");
    pgn
}

/// SHA-256 of a byte string (FIPS 180-4). Used to record exactly which engine binaries
/// played a match, so a result can be tied to a build. No external crate needed.
pub fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (x, y) in h.iter_mut().zip(v) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_controls_parse() {
        assert_eq!("nodes=5000".parse(), Ok(TimeControl::Nodes(5000)));
        assert_eq!("depth=7".parse(), Ok(TimeControl::Depth(7)));
        assert_eq!("movetime=250".parse(), Ok(TimeControl::MoveTime(Duration::from_millis(250))));
        assert_eq!(
            "10+0.1".parse(),
            Ok(TimeControl::Clock { base: Duration::from_secs(10), increment: Duration::from_millis(100) })
        );
        for bad in ["", "10", "nodes=", "nodes=-1", "0+1", "x+y", "movetime=1.5", "depth=300"] {
            assert!(bad.parse::<TimeControl>().is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn openings_are_validated_on_parse() {
        assert!(Opening::parse("startpos moves e2e4 e7e5 g1f3").is_ok());
        assert!(Opening::parse("e2e4 c7c5").is_ok());
        assert!(Opening::parse("fen 4k3/8/8/8/8/8/4P3/4K3 w - - 0 1 moves e2e4").is_ok());
        assert!(Opening::parse("startpos moves e2e5").is_err(), "illegal move");
        assert!(Opening::parse("fen 8/8/8/8/8/8/8/8 w - - 0 1").is_err(), "invalid FEN");
        assert!(Opening::parse("").is_err());
    }

    #[test]
    fn rules_verdicts() {
        let pos = |fen: &str| {
            let f = parse_fen(fen).unwrap();
            Position::with_clock(f.board, f.halfmove_clock)
        };
        assert_eq!(rules_verdict(&pos("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1")).unwrap().0, Outcome::WhiteWins);
        assert_eq!(rules_verdict(&pos("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1")).unwrap().1, "stalemate");
        assert_eq!(rules_verdict(&pos("4k3/8/8/8/8/8/P7/R3K3 w - - 100 90")).unwrap().1, "fifty-move rule");
        assert_eq!(rules_verdict(&pos("8/8/8/4k3/8/8/8/2B1K3 w - - 0 1")).unwrap().1, "insufficient material");
        assert!(rules_verdict(&pos(STARTPOS)).is_none());
        let shuffle = Opening::parse("g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1 f6g8").unwrap();
        assert_eq!(rules_verdict(&shuffle.position().unwrap()).unwrap().1, "threefold repetition");
    }

    #[test]
    fn mating_material_for_time_forfeits() {
        let b = |fen: &str| parse_fen(fen).unwrap().board;
        assert!(!can_mate(&b("8/8/8/4k3/8/8/8/2B1K3 w - - 0 1"), Color::White));
        assert!(can_mate(&b("8/8/8/4k3/8/8/8/2B1KB2 w - - 0 1"), Color::White));
        assert!(can_mate(&b("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1"), Color::White));
        assert!(!can_mate(&b("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1"), Color::Black));
    }

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(sha256(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn info_lines_parse() {
        let infos = vec![
            "info depth 3 seldepth 5 score cp 12 nodes 100 pv e2e4".to_string(),
            "info depth 4 score mate 2 nodes 900 pv e2e4".to_string(),
            "info string hello".to_string(),
        ];
        assert_eq!(parse_info(&infos), (Some(99_998), Some(4), Some(900)));
    }
}
