//! Standard Algebraic Notation (SAN), as PGN requires.
//!
//! Rules implemented (PGN standard §8.2.3):
//! - piece letter (none for pawns), then disambiguation, then `x` for captures, then the
//!   destination, then `=Q` for promotion;
//! - disambiguation by origin file if that is unique among the pieces that can reach the
//!   destination, else by rank, else both;
//! - pawn captures name the origin file (`exd5`), en passant included;
//! - castling is `O-O` / `O-O-O`;
//! - `+` for check, `#` for checkmate.

use chess::{Board, BoardStatus, ChessMove, MoveGen, Piece};

fn letter(piece: Piece) -> &'static str {
    match piece {
        Piece::Pawn => "",
        Piece::Knight => "N",
        Piece::Bishop => "B",
        Piece::Rook => "R",
        Piece::Queen => "Q",
        Piece::King => "K",
    }
}

/// SAN for a legal move `m` in `board`. The caller guarantees legality.
pub fn to_san(board: &Board, m: ChessMove) -> String {
    let from = m.get_source();
    let to = m.get_dest();
    let piece = board.piece_on(from).expect("a legal move starts on a piece");
    let mut san = String::new();

    let file_distance = (from.get_file().to_index() as i32 - to.get_file().to_index() as i32).abs();
    if piece == Piece::King && file_distance == 2 {
        san.push_str(if to.get_file().to_index() > from.get_file().to_index() { "O-O" } else { "O-O-O" });
    } else {
        let capture = board.piece_on(to).is_some()
            || (piece == Piece::Pawn && from.get_file() != to.get_file());
        if piece == Piece::Pawn {
            if capture {
                san.push((b'a' + from.get_file().to_index() as u8) as char);
            }
        } else {
            san.push_str(letter(piece));
            let rivals: Vec<_> = MoveGen::new_legal(board)
                .filter(|other| {
                    other.get_dest() == to
                        && other.get_source() != from
                        && board.piece_on(other.get_source()) == Some(piece)
                })
                .collect();
            if !rivals.is_empty() {
                let same_file = rivals.iter().any(|r| r.get_source().get_file() == from.get_file());
                let same_rank = rivals.iter().any(|r| r.get_source().get_rank() == from.get_rank());
                let file = (b'a' + from.get_file().to_index() as u8) as char;
                let rank = (b'1' + from.get_rank().to_index() as u8) as char;
                if !same_file {
                    san.push(file);
                } else if !same_rank {
                    san.push(rank);
                } else {
                    san.push(file);
                    san.push(rank);
                }
            }
        }
        if capture {
            san.push('x');
        }
        san.push_str(&to.to_string());
        if let Some(promotion) = m.get_promotion() {
            san.push('=');
            san.push_str(letter(promotion));
        }
    }

    let after = board.make_move_new(m);
    if after.status() == BoardStatus::Checkmate {
        san.push('#');
    } else if after.checkers() != &chess::EMPTY {
        san.push('+');
    }
    san
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_move;
    use std::str::FromStr;

    fn san(fen: &str, uci: &str) -> String {
        let board = Board::from_str(fen).unwrap();
        to_san(&board, parse_move(&board, uci).unwrap())
    }

    #[test]
    fn basic_moves() {
        let start = crate::STARTPOS;
        assert_eq!(san(start, "e2e4"), "e4");
        assert_eq!(san(start, "g1f3"), "Nf3");
        assert_eq!(san(start, "b1c3"), "Nc3");
    }

    #[test]
    fn castling() {
        let kiwipete = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
        assert_eq!(san(kiwipete, "e1g1"), "O-O");
        assert_eq!(san(kiwipete, "e1c1"), "O-O-O");
    }

    #[test]
    fn captures_en_passant_and_promotion() {
        assert_eq!(san("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1", "e4d5"), "exd5");
        assert_eq!(san("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"), "exd6");
        assert_eq!(san("3r2k1/4P3/8/8/8/8/8/4K3 w - - 0 1", "e7d8q"), "exd8=Q+");
        assert_eq!(san("3r2k1/4P3/8/8/8/8/8/4K3 w - - 0 1", "e7e8n"), "e8=N");
        assert_eq!(san("4k3/8/8/8/8/8/8/R3K3 w - - 0 1", "a1a8"), "Ra8+");
    }

    #[test]
    fn disambiguation() {
        // Knights on a1 and c1 both reach b3: disambiguate by file.
        assert_eq!(san("4k3/8/8/8/8/8/8/N1N1K3 w - - 0 1", "a1b3"), "Nab3");
        assert_eq!(san("4k3/8/8/8/8/8/8/N1N1K3 w - - 0 1", "c1b3"), "Ncb3");
        // Rooks on a1 and a8 both reach a4: same file, so disambiguate by rank.
        assert_eq!(san("R7/8/7k/8/8/8/8/R3K3 w - - 0 1", "a1a4"), "R1a4");
        assert_eq!(san("R7/8/7k/8/8/8/8/R3K3 w - - 0 1", "a8a4"), "R8a4");
        // Queens on a1, a3 and c1 all reach b2: a1 shares a file with a3 and a rank with
        // c1, so it needs both.
        assert_eq!(san("4k3/8/8/8/8/Q7/8/Q1Q1K3 w - - 0 1", "a1b2"), "Qa1b2");
        // A pinned rival does not count: only legal moves force disambiguation. Both
        // knights attack d5, but the c3 knight is pinned by the c8 rook.
        assert_eq!(san("2r1k3/8/8/8/8/2N1N3/8/2K5 w - - 0 1", "e3d5"), "Nd5");
    }

    #[test]
    fn checkmate_suffix() {
        assert_eq!(san("6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1", "a1a8"), "Ra8#");
    }
}
