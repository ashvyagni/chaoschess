//! Match runner: two engines, paired games from an opening book, run in parallel, with
//! Elo, confidence intervals and an optional SPRT. Writes PGN and a machine-readable JSON
//! record that pins everything needed to reproduce the match.
//!
//! ```text
//! arena --engine name=new,cmd=target/release/crazy-chess \
//!       --engine name=base,cmd=/path/to/baseline,opt.Style=Chaos \
//!       --tc nodes=20000 --games 200 --concurrency 4 \
//!       --sprt elo0=0,elo1=10,alpha=0.05,beta=0.05
//! ```

use crazy_chess::arena::{play_game, sha256, to_pgn, EngineConfig, GameRecord, GameSettings, Opening, TimeControl};
use crazy_chess::stats::{EloEstimate, Pentanomial, Sprt, SprtVerdict, Trinomial};
use chess::Color;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

struct Args {
    engines: Vec<EngineConfig>,
    time_control: TimeControl,
    games: usize,
    openings: PathBuf,
    concurrency: usize,
    max_plies: u32,
    sprt: Option<Sprt>,
    pgn: Option<PathBuf>,
    json: Option<PathBuf>,
    event: String,
    offset: usize,
}

fn usage() -> ! {
    eprintln!(
        "usage: arena --engine name=N,cmd=PATH[,arg=A][,opt.NAME=VALUE] --engine ... \\\n\
         \t--tc nodes=N|depth=D|movetime=MS|BASE+INC [--games N] [--openings FILE]\n\
         \t[--concurrency K] [--max-plies P] [--sprt elo0=,elo1=,alpha=,beta=]\n\
         \t[--pgn FILE] [--json FILE] [--event NAME] [--offset K]"
    );
    std::process::exit(2);
}

fn parse_engine(spec: &str) -> Result<EngineConfig, String> {
    let mut config = EngineConfig { name: String::new(), command: PathBuf::new(), args: vec![], options: vec![] };
    for part in spec.split(',') {
        let (key, value) = part.split_once('=').ok_or_else(|| format!("expected key=value in {part:?}"))?;
        match key {
            "name" => config.name = value.to_string(),
            "cmd" => config.command = PathBuf::from(value),
            "arg" => config.args.push(value.to_string()),
            k if k.starts_with("opt.") => config.options.push((k[4..].to_string(), value.to_string())),
            other => return Err(format!("unknown engine key {other:?}")),
        }
    }
    if config.name.is_empty() || config.command.as_os_str().is_empty() {
        return Err(format!("engine needs name= and cmd=: {spec:?}"));
    }
    Ok(config)
}

fn parse_sprt(spec: &str) -> Result<Sprt, String> {
    let mut sprt = Sprt { elo0: 0.0, elo1: 10.0, alpha: 0.05, beta: 0.05 };
    for part in spec.split(',') {
        let (key, value) = part.split_once('=').ok_or_else(|| format!("expected key=value in {part:?}"))?;
        let v: f64 = value.parse().map_err(|_| format!("bad number {value:?}"))?;
        match key {
            "elo0" => sprt.elo0 = v,
            "elo1" => sprt.elo1 = v,
            "alpha" => sprt.alpha = v,
            "beta" => sprt.beta = v,
            other => return Err(format!("unknown sprt key {other:?}")),
        }
    }
    if sprt.elo1 <= sprt.elo0 || !(0.0..0.5).contains(&sprt.alpha) || !(0.0..0.5).contains(&sprt.beta) {
        return Err("need elo1 > elo0 and alpha, beta in (0, 0.5)".to_string());
    }
    Ok(sprt)
}

fn parse_args() -> Args {
    let mut args = Args {
        engines: vec![],
        time_control: TimeControl::Nodes(10_000),
        games: 100,
        openings: PathBuf::from("openings/standard40.txt"),
        concurrency: thread::available_parallelism().map_or(2, |n| (n.get() / 2).max(1)),
        max_plies: 400,
        sprt: None,
        pgn: None,
        json: None,
        event: "chaoschess match".to_string(),
        offset: 0,
    };
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let value = |i: &mut usize| -> String {
        *i += 1;
        raw.get(*i).cloned().unwrap_or_else(|| usage())
    };
    while i < raw.len() {
        let result: Result<(), String> = match raw[i].as_str() {
            "--engine" => parse_engine(&value(&mut i)).map(|e| args.engines.push(e)),
            "--tc" => value(&mut i).parse().map(|tc| args.time_control = tc),
            "--games" => value(&mut i).parse().map(|n| args.games = n).map_err(|e| format!("{e}")),
            "--openings" => Ok(args.openings = PathBuf::from(value(&mut i))),
            "--concurrency" => value(&mut i).parse().map(|n: usize| args.concurrency = n.max(1)).map_err(|e| format!("{e}")),
            "--max-plies" => value(&mut i).parse().map(|n| args.max_plies = n).map_err(|e| format!("{e}")),
            "--sprt" => parse_sprt(&value(&mut i)).map(|s| args.sprt = Some(s)),
            "--pgn" => Ok(args.pgn = Some(PathBuf::from(value(&mut i)))),
            "--json" => Ok(args.json = Some(PathBuf::from(value(&mut i)))),
            "--event" => Ok(args.event = value(&mut i)),
            "--offset" => value(&mut i).parse().map(|n| args.offset = n).map_err(|e| format!("{e}")),
            "-h" | "--help" => usage(),
            other => Err(format!("unknown argument {other:?}")),
        };
        if let Err(e) = result {
            eprintln!("arena: {e}");
            usage();
        }
        i += 1;
    }
    if args.engines.len() != 2 {
        eprintln!("arena: exactly two --engine arguments are needed");
        usage();
    }
    args
}

fn load_openings(path: &Path) -> Result<Vec<Opening>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let openings: Result<Vec<_>, _> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|(n, l)| Opening::parse(l).map_err(|e| format!("{}:{}: {e}", path.display(), n + 1)))
        .collect();
    let openings = openings?;
    if openings.is_empty() {
        return Err(format!("{}: no openings", path.display()));
    }
    Ok(openings)
}

fn file_sha256(path: &Path) -> String {
    std::fs::read(path).map_or_else(|e| format!("unreadable: {e}"), |bytes| sha256(&bytes))
}

/// `(days since 1970, seconds into day)` to a UTC date, without a date crate.
fn utc_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    // Civil-from-days (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}.{month:02}.{day:02}")
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_num(x: f64) -> String {
    if x.is_finite() { format!("{x:.3}") } else { "null".to_string() }
}

fn json_elo(e: Option<EloEstimate>) -> String {
    e.map_or("null".to_string(), |e| {
        format!("{{\"elo\": {}, \"lower\": {}, \"upper\": {}, \"score\": {}}}", json_num(e.elo), json_num(e.lower), json_num(e.upper), json_num(e.score))
    })
}

fn fmt_elo(e: Option<EloEstimate>, variance: f64) -> String {
    e.map_or("n/a".to_string(), |e| {
        if variance <= 0.0 {
            // Every unit scored the same (e.g. identical engines, each pair split 1-1). The
            // normal approximation then gives a zero-width interval, which would read as
            // certainty. It isn't; the interval is just not estimable yet.
            format!("{:+.1} (interval not estimable: zero sample variance)", e.elo)
        } else if e.elo.is_finite() && e.margin().is_finite() {
            format!("{:+.1} ± {:.1}", e.elo, e.margin())
        } else {
            format!("{:+.1} (interval unbounded)", e.elo)
        }
    })
}

fn main() {
    let args = parse_args();
    let openings = load_openings(&args.openings).unwrap_or_else(|e| {
        eprintln!("arena: {e}");
        std::process::exit(2);
    });
    for engine in &args.engines {
        if !engine.command.exists() {
            eprintln!("arena: engine binary not found: {}", engine.command.display());
            std::process::exit(2);
        }
    }
    let pairs = args.games.div_ceil(2).max(1);
    let settings = GameSettings { time_control: args.time_control, max_plies: args.max_plies, ..GameSettings::default() };
    let (a, b) = (args.engines[0].clone(), args.engines[1].clone());
    let started_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let date = utc_date(started_at);

    println!("{} vs {}: {} pairs ({} games), tc {}, {} openings from {}, concurrency {}",
        a.name, b.name, pairs, pairs * 2, args.time_control, openings.len(), args.openings.display(), args.concurrency);
    if let Some(s) = args.sprt {
        let (lo, hi) = s.bounds();
        println!("SPRT elo0={} elo1={} alpha={} beta={}  LLR bounds [{lo:.2}, {hi:.2}]", s.elo0, s.elo1, s.alpha, s.beta);
    }

    let next = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<(usize, GameRecord, GameRecord)>();
    let wall = Instant::now();
    let workers: Vec<_> = (0..args.concurrency.min(pairs))
        .map(|_| {
            let (next, stop, tx) = (Arc::clone(&next), Arc::clone(&stop), tx.clone());
            let (a, b, openings, settings, offset) = (a.clone(), b.clone(), openings.clone(), settings, args.offset);
            thread::spawn(move || loop {
                let pair = next.fetch_add(1, Ordering::SeqCst);
                if pair >= pairs || stop.load(Ordering::SeqCst) {
                    break;
                }
                let opening = &openings[(offset + pair) % openings.len()];
                let first = play_game(&a, &b, opening, &settings);
                let second = play_game(&b, &a, opening, &settings);
                if tx.send((pair, first, second)).is_err() {
                    break;
                }
            })
        })
        .collect();
    drop(tx);

    let mut tri = Trinomial::default();
    let mut penta = Pentanomial::default();
    let mut results: Vec<(usize, GameRecord, GameRecord)> = Vec::new();
    let mut verdict = SprtVerdict::Continue;
    let mut llr = 0.0;
    for (pair, first, second) in rx {
        // `a`'s score in each game: white in the first, black in the second.
        let s1 = first.outcome.points_for(Color::White);
        let s2 = second.outcome.points_for(Color::Black);
        for s in [s1, s2] {
            match s {
                x if x == 1.0 => tri.wins += 1,
                x if x == 0.5 => tri.draws += 1,
                _ => tri.losses += 1,
            }
        }
        penta.add_pair(s1, s2);
        results.push((pair, first, second));
        let mut line = format!(
            "[{:>4}/{pairs} pairs] {} vs {}: +{} -{} ={}  score {:.1}%  Elo {} (pentanomial)",
            results.len(), a.name, b.name, tri.wins, tri.losses, tri.draws, 100.0 * tri.score(), fmt_elo(penta.elo(), penta.variance())
        );
        if let Some(s) = args.sprt {
            llr = s.llr_pentanomial(&penta);
            let (lo, hi) = s.bounds();
            let _ = write!(line, "  LLR {llr:+.2} [{lo:.2}, {hi:.2}]");
            verdict = s.verdict(llr);
            if verdict != SprtVerdict::Continue && !stop.swap(true, Ordering::SeqCst) {
                let _ = write!(line, "  -> {verdict:?}, finishing games in progress");
            }
        }
        println!("{line}");
    }
    for w in workers {
        let _ = w.join();
    }
    results.sort_by_key(|(pair, _, _)| *pair);
    let elapsed = wall.elapsed();

    let tri_elo = tri.elo();
    let penta_elo = penta.elo();
    println!("\nFinal: {} vs {} after {} games in {:.0}s", a.name, b.name, tri.games(), elapsed.as_secs_f64());
    println!("  +{} -{} ={}  score {:.1}%   LOS {:.1}%", tri.wins, tri.losses, tri.draws, 100.0 * tri.score(), 100.0 * tri.los());
    println!("  Elo {} (pentanomial, 95%)", fmt_elo(penta_elo, penta.variance()));
    println!("  Elo {} (trinomial, 95%)", fmt_elo(tri_elo, tri.variance()));
    println!("  pairs by score [0, ½, 1, 1½, 2]: {:?}", penta.0);
    if args.sprt.is_some() {
        println!("  SPRT: LLR {llr:+.2}, verdict {verdict:?}");
    }

    let mut terminations: BTreeMap<String, usize> = BTreeMap::new();
    for (_, g1, g2) in &results {
        for g in [g1, g2] {
            *terminations.entry(g.termination.clone()).or_default() += 1;
        }
    }
    println!("  terminations: {terminations:?}");

    let stem = format!("matches/{started_at}-{}-vs-{}", a.name, b.name);
    let pgn_path = args.pgn.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}.pgn")));
    let json_path = args.json.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}.json")));
    for path in [&pgn_path, &json_path] {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
    }

    let mut pgn = String::new();
    for (pair, g1, g2) in &results {
        pgn.push_str(&to_pgn(g1, &args.event, 2 * pair + 1, &date, &args.time_control));
        pgn.push_str(&to_pgn(g2, &args.event, 2 * pair + 2, &date, &args.time_control));
    }
    std::fs::write(&pgn_path, pgn).expect("write PGN");

    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let mut json = String::from("{\n");
    let _ = writeln!(json, "  \"tool\": \"chaoschess arena\",\n  \"date_utc\": {},\n  \"unix_time\": {started_at},\n  \"event\": {},\n  \"repository_commit\": {},", json_str(&date), json_str(&args.event), json_str(&commit));
    json.push_str("  \"engines\": [\n");
    for (i, e) in [&a, &b].iter().enumerate() {
        let options: Vec<String> = e.options.iter().map(|(k, v)| format!("[{}, {}]", json_str(k), json_str(v))).collect();
        let arg_list: Vec<String> = e.args.iter().map(|x| json_str(x)).collect();
        let _ = writeln!(json, "    {{\"name\": {}, \"command\": {}, \"args\": [{}], \"options\": [{}], \"sha256\": {}}}{}",
            json_str(&e.name), json_str(&e.command.display().to_string()), arg_list.join(", "), options.join(", "),
            json_str(&file_sha256(&e.command)), if i == 0 { "," } else { "" });
    }
    json.push_str("  ],\n");
    let _ = writeln!(json, "  \"time_control\": {},\n  \"openings\": {{\"file\": {}, \"sha256\": {}, \"count\": {}, \"offset\": {}}},",
        json_str(&args.time_control.to_string()), json_str(&args.openings.display().to_string()), json_str(&file_sha256(&args.openings)), openings.len(), args.offset);
    let _ = writeln!(json, "  \"max_plies\": {},\n  \"concurrency\": {},\n  \"machine\": {{\"os\": {}, \"arch\": {}, \"logical_cpus\": {}}},",
        args.max_plies, args.concurrency, json_str(std::env::consts::OS), json_str(std::env::consts::ARCH),
        thread::available_parallelism().map_or(0, |n| n.get()));
    let _ = writeln!(json, "  \"wall_seconds\": {},\n  \"games\": {},\n  \"pairs_requested\": {pairs},\n  \"pairs_completed\": {},",
        json_num(elapsed.as_secs_f64()), tri.games(), results.len());
    let _ = writeln!(json, "  \"perspective\": {},\n  \"wins\": {}, \"draws\": {}, \"losses\": {},\n  \"pentanomial\": {:?},",
        json_str(&a.name), tri.wins, tri.draws, tri.losses, penta.0);
    let _ = writeln!(json, "  \"elo_pentanomial\": {},\n  \"pentanomial_variance\": {},\n  \"elo_trinomial\": {},\n  \"los\": {},",
        json_elo(penta_elo), json_num(penta.variance()), json_elo(tri_elo), json_num(tri.los()));
    match args.sprt {
        Some(s) => {
            let (lo, hi) = s.bounds();
            let _ = writeln!(json, "  \"sprt\": {{\"elo0\": {}, \"elo1\": {}, \"alpha\": {}, \"beta\": {}, \"llr\": {}, \"lower\": {}, \"upper\": {}, \"verdict\": {}}},",
                s.elo0, s.elo1, s.alpha, s.beta, json_num(llr), json_num(lo), json_num(hi), json_str(&format!("{verdict:?}")));
        }
        None => json.push_str("  \"sprt\": null,\n"),
    }
    let terms: Vec<String> = terminations.iter().map(|(k, v)| format!("{}: {v}", json_str(k))).collect();
    let _ = writeln!(json, "  \"terminations\": {{{}}},", terms.join(", "));
    json.push_str("  \"game_list\": [\n");
    let mut rows = Vec::new();
    for (pair, g1, g2) in &results {
        for (round, g) in [(2 * pair + 1, g1), (2 * pair + 2, g2)] {
            rows.push(format!("    {{\"round\": {round}, \"white\": {}, \"black\": {}, \"opening\": {}, \"result\": {}, \"termination\": {}, \"plies\": {}}}",
                json_str(&g.white), json_str(&g.black), json_str(&g.opening.moves.join(" ")), json_str(g.outcome.pgn()), json_str(&g.termination), g.moves.len()));
        }
    }
    json.push_str(&rows.join(",\n"));
    json.push_str("\n  ]\n}\n");
    std::fs::write(&json_path, json).expect("write JSON");
    println!("  wrote {} and {}", pgn_path.display(), json_path.display());
}
