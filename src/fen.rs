//! Strict FEN validation, applied before anything reaches the `chess` crate.
//!
//! The crate's own FEN parser is not robust, which `tests/fuzz.rs` found. On malformed
//! input it can:
//! - hit **undefined behaviour**: an out-of-bounds `get_unchecked` in `magic.rs` when the
//!   side to move has no king. Debug builds abort; release builds read out of bounds;
//! - **panic**, e.g. two black kings and no white king;
//! - **silently accept garbage** and build a nonsense position: 7 or 9 files in a rank,
//!   7 ranks, pawns on the first or last rank, an en passant square on the wrong rank,
//!   junk in the castling field.
//!
//! Everything here is checked structurally first. The crate only sees FENs that are
//! well formed, and it then does the one check that needs a board (the side not to move
//! must not be in check).

use chess::{Board, Color, Piece, Square};
use std::str::FromStr;

/// A validated position plus the two counters the crate's `Board` does not keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fen {
    pub board: Board,
    pub halfmove_clock: u32,
    pub fullmove_number: u32,
}

/// Parse a FEN. The halfmove clock and fullmove number may be omitted (defaults 0 and 1),
/// because some tools leave them out.
pub fn parse_fen(text: &str) -> Result<Fen, String> {
    let fields: Vec<&str> = text.split_whitespace().collect();
    if !(4..=6).contains(&fields.len()) {
        return Err(format!("expected 4 to 6 fields, got {}", fields.len()));
    }
    let squares = parse_placement(fields[0])?;
    let side = match fields[1] {
        "w" => Color::White,
        "b" => Color::Black,
        other => return Err(format!("side to move must be 'w' or 'b', not {other:?}")),
    };
    check_castling(fields[2], &squares)?;
    check_en_passant(fields[3], side, &squares)?;
    let halfmove_clock = match fields.get(4) {
        None => 0,
        Some(v) => v
            .parse::<u32>()
            .ok()
            .filter(|&n| n <= 10_000)
            .ok_or_else(|| format!("halfmove clock must be 0..=10000, not {v:?}"))?,
    };
    let fullmove_number = match fields.get(5) {
        None => 1,
        Some(v) => v
            .parse::<u32>()
            .ok()
            .filter(|&n| n >= 1)
            .ok_or_else(|| format!("fullmove number must be a positive integer, not {v:?}"))?,
    };
    let canonical = format!(
        "{} {} {} {} {halfmove_clock} {fullmove_number}",
        fields[0], fields[1], fields[2], fields[3]
    );
    let board = Board::from_str(&canonical).map_err(|e| match e {
        chess::Error::InvalidBoard => {
            "illegal position (the side not to move is in check, or the kings touch)".to_string()
        }
        other => format!("{other:?}"),
    })?;
    Ok(Fen {
        board,
        halfmove_clock,
        fullmove_number,
    })
}

/// `squares[rank][file]`, rank 0 = rank 1. `None` = empty.
type Squares = [[Option<char>; 8]; 8];

fn parse_placement(placement: &str) -> Result<Squares, String> {
    let ranks: Vec<&str> = placement.split('/').collect();
    if ranks.len() != 8 {
        return Err(format!("board needs 8 ranks, got {}", ranks.len()));
    }
    let mut squares: Squares = [[None; 8]; 8];
    let mut counts = std::collections::HashMap::new();
    for (i, text) in ranks.iter().enumerate() {
        let rank = 7 - i;
        let mut file = 0usize;
        for c in text.chars() {
            match c {
                '1'..='8' => file += c as usize - '0' as usize,
                'p' | 'n' | 'b' | 'r' | 'q' | 'k' | 'P' | 'N' | 'B' | 'R' | 'Q' | 'K' => {
                    if file < 8 {
                        squares[rank][file] = Some(c);
                    }
                    *counts.entry(c).or_insert(0usize) += 1;
                    file += 1;
                }
                other => return Err(format!("invalid character {other:?} in rank {}", rank + 1)),
            }
            if file > 8 {
                return Err(format!("rank {} has more than 8 squares", rank + 1));
            }
        }
        if file != 8 {
            return Err(format!("rank {} has {file} squares, not 8", rank + 1));
        }
    }
    let count = |c: char| counts.get(&c).copied().unwrap_or(0);
    for (king, side) in [('K', "White"), ('k', "Black")] {
        if count(king) != 1 {
            return Err(format!("{side} must have exactly one king, found {}", count(king)));
        }
    }
    for (pieces, side) in [("PNBRQK", "White"), ("pnbrqk", "Black")] {
        let pawns = count(pieces.chars().next().unwrap());
        let total: usize = pieces.chars().map(count).sum();
        if pawns > 8 || total > 16 {
            return Err(format!("{side} has {pawns} pawns and {total} pieces (max 8 and 16)"));
        }
    }
    for (rank, name) in [(0, 1), (7, 8)] {
        if squares[rank].iter().any(|s| matches!(s, Some('P') | Some('p'))) {
            return Err(format!("pawn on rank {name}"));
        }
    }
    Ok(squares)
}

fn check_castling(field: &str, squares: &Squares) -> Result<(), String> {
    if field == "-" {
        return Ok(());
    }
    let mut seen = String::new();
    for c in field.chars() {
        let (king_sq, rook_sq, king, rook) = match c {
            'K' => ((0, 4), (0, 7), 'K', 'R'),
            'Q' => ((0, 4), (0, 0), 'K', 'R'),
            'k' => ((7, 4), (7, 7), 'k', 'r'),
            'q' => ((7, 4), (7, 0), 'k', 'r'),
            other => return Err(format!("invalid castling flag {other:?}")),
        };
        if seen.contains(c) {
            return Err(format!("castling flag {c:?} repeated"));
        }
        seen.push(c);
        if squares[king_sq.0][king_sq.1] != Some(king) || squares[rook_sq.0][rook_sq.1] != Some(rook) {
            return Err(format!("castling right {c:?} but king or rook is not on its home square"));
        }
    }
    Ok(())
}

fn check_en_passant(field: &str, side: Color, squares: &Squares) -> Result<(), String> {
    if field == "-" {
        return Ok(());
    }
    let square = Square::from_str(field).map_err(|_| format!("invalid en passant square {field:?}"))?;
    let (file, rank) = (square.get_file().to_index(), square.get_rank().to_index());
    // The square is the one the pawn skipped. White to move means Black just pushed, so
    // it is on rank 6 with the black pawn in front of it on rank 5; and mirrored for Black.
    let (expected_rank, pawn_rank, pawn, from_rank) = match side {
        Color::White => (5, 4, 'p', 6),
        Color::Black => (2, 3, 'P', 1),
    };
    if rank != expected_rank {
        return Err(format!("en passant square {field} is on the wrong rank for this side to move"));
    }
    if squares[pawn_rank][file] != Some(pawn) {
        return Err(format!("en passant square {field} has no pawn that just moved past it"));
    }
    if squares[rank][file].is_some() || squares[from_rank][file].is_some() {
        return Err(format!("en passant square {field} or the square behind it is occupied"));
    }
    let _ = Piece::Pawn; // keep the import meaningful if the checks above change
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_fens() {
        for fen in [
            crate::STARTPOS,
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2",
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
            "4k3/8/8/8/8/8/8/4K3 w - -",
            "4k3/8/8/8/8/8/8/4K3 w - - 37",
            "7k/5Q2/6K1/8/8/8/8/8 w - - 99 80",
        ] {
            assert!(parse_fen(fen).is_ok(), "{fen}: {:?}", parse_fen(fen));
        }
        assert_eq!(parse_fen("4k3/8/8/8/8/8/8/4K3 w - - 37 12").unwrap().halfmove_clock, 37);
    }

    /// Every FEN here either triggered undefined behaviour or a panic in the crate, or was
    /// silently accepted as a nonsense position (tests/fuzz.rs, 2026-09-26).
    #[test]
    fn rejects_malformed_fens_without_touching_the_crate() {
        for (fen, why) in [
            ("8/8/8/8/8/8/8/8 w - - 0 1", "no kings: crate UB"),
            ("4k3/8/8/8/8/8/8/8 w - - 0 1", "side to move has no king: crate UB"),
            ("4k2k/8/8/8/8/8/8/8 w - - 0 1", "two black kings: crate panic"),
            ("4k3/8/8/8/8/8/8/4KK2 w - - 0 1", "two white kings"),
            ("4k2P/8/8/8/8/8/8/4K3 w - - 0 1", "pawn on rank 8"),
            ("4k3/8/8/8/8/8/8/4K2p b - - 0 1", "pawn on rank 1"),
            ("4k4/8/8/8/8/8/8/4K3 w - - 0 1", "nine files"),
            ("4k2/8/8/8/8/8/8/4K3 w - - 0 1", "seven files"),
            ("4k3/8/8/8/8/8/4K3 w - - 0 1", "seven ranks"),
            ("4k3/8/8/8/8/8/8/8/4K3 w - - 0 1", "nine ranks"),
            ("4k3/08/8/8/8/8/8/4K3 w - - 0 1", "zero digit"),
            ("4k3/8/8/8/8/8/8/4K2X w - - 0 1", "bad piece letter"),
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d4 0 1", "en passant on the wrong rank"),
            ("4k3/8/8/8/8/8/8/4K3 w - e6 0 1", "en passant with no pawn"),
            ("4k3/8/8/8/8/8/8/4K3 w - z9 0 1", "garbage en passant"),
            ("4k3/8/8/8/8/8/8/4K3 w Z - 0 1", "garbage castling"),
            ("4k3/8/8/8/8/8/8/4K3 w KK - 0 1", "repeated castling flag"),
            ("4k3/8/8/8/8/8/8/4K3 w K - 0 1", "castling right without a rook"),
            ("4k3/8/8/8/8/8/8/4K3 x - - 0 1", "bad side to move"),
            ("4k3/8/8/8/8/8/8/4K3 w - - -1 1", "negative clock"),
            ("4k3/8/8/8/8/8/8/4K3 w - - 0 0", "fullmove zero"),
            ("4k3/8/8/8/8/8/4r3/4K3 b - - 0 1", "side not to move is in check"),
            ("PPPPPPPP/PPPPPPPP/8/8/8/8/8/4K2k w - - 0 1", "too many pawns"),
            ("4k3/8/8/8/8/8/8/4K3", "missing fields"),
            ("", "empty"),
        ] {
            assert!(parse_fen(fen).is_err(), "accepted {why}: {fen}");
        }
    }
}
