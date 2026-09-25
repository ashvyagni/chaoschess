//! The `arena` binary end to end: it runs a tiny match and writes a JSON record and a
//! PGN a standard reader can number correctly.

use std::process::Command;

#[test]
fn arena_cli_writes_a_reproducible_record() {
    let dir = std::env::temp_dir().join(format!("arena-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (pgn, json) = (dir.join("m.pgn"), dir.join("m.json"));
    let engine = env!("CARGO_BIN_EXE_crazy-chess");
    let out = Command::new(env!("CARGO_BIN_EXE_arena"))
        .args([
            "--engine", &format!("name=A,cmd={engine},opt.Hash=4"),
            "--engine", &format!("name=B,cmd={engine},opt.Hash=4,opt.Style=Chaos"),
            "--tc", "nodes=400", "--games", "4", "--concurrency", "2", "--max-plies", "80",
            "--sprt", "elo0=0,elo1=20,alpha=0.05,beta=0.05",
            "--openings", concat!(env!("CARGO_MANIFEST_DIR"), "/openings/standard40.txt"),
            "--pgn", pgn.to_str().unwrap(), "--json", json.to_str().unwrap(),
        ])
        .output()
        .expect("arena runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let record = std::fs::read_to_string(&json).unwrap();
    for key in [
        "\"games\": 4", "\"pairs_completed\": 2", "\"sha256\": \"", "\"pentanomial\": [",
        "\"elo_pentanomial\"", "\"sprt\": {", "\"verdict\"", "\"time_control\": \"nodes=400\"",
        "\"date_utc\"", "opt.Hash", // not a key, so check options were recorded:
    ]
    .iter()
    .filter(|k| **k != "opt.Hash")
    {
        assert!(record.contains(key), "JSON lacks {key}:\n{record}");
    }
    assert!(record.contains("[\"Style\", \"Chaos\"]"), "engine options recorded");
    // The engine binary's SHA-256 is 64 hex characters.
    let digest = record.split("\"sha256\": \"").nth(1).unwrap();
    assert!(digest[..64].chars().all(|c| c.is_ascii_hexdigit()));

    let games = std::fs::read_to_string(&pgn).unwrap();
    assert_eq!(games.matches("[Event ").count(), 4);
    // First opening line is 1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5, from the standard start position,
    // so no FEN tag and correct numbering from move 1.
    assert!(games.contains("1. e4 {book} e5 {book} 2. Nf3 {book}"), "{games}");
    assert!(!games.contains("[FEN "), "startpos openings need no FEN tag");
    let _ = std::fs::remove_dir_all(&dir);
}
