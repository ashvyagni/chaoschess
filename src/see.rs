//! Capture classification and static exchange evaluation.

use super::*;

/// Piece values used by static exchange evaluation. The king is given a value larger
/// than any possible exchange so a king capture can never look profitable.
pub(crate) const SEE_VALUES: [i32; 6] = [100, 320, 330, 500, 900, 100_000];

/// True when `m` is an en passant capture.
///
/// Note the `chess` crate stores the square of the *capturable pawn* in `en_passant()`,
/// not the square the capturing pawn moves to, so the destination has to be stepped back
/// one rank before comparing. Getting this backwards silently classifies every en passant
/// capture as a quiet move.
pub(crate) fn is_en_passant(board: &Board, m: ChessMove) -> bool {
    board.piece_on(m.get_source()) == Some(Piece::Pawn)
        && board.piece_on(m.get_dest()).is_none()
        && board.en_passant() == Some(m.get_dest().ubackward(board.side_to_move()))
}

/// True when `m` removes an enemy piece from the board, including en passant, where the
/// captured pawn is not on the destination square.
pub(crate) fn is_capture(board: &Board, m: ChessMove) -> bool {
    board.piece_on(m.get_dest()).is_some() || is_en_passant(board, m)
}

/// Every piece of either colour that attacks `square`, given an arbitrary occupancy.
///
/// Passing a modified `occupied` is what makes x-ray recomputation work in [`see`]: once
/// an attacker is removed, a slider behind it becomes an attacker in the next iteration.
pub(crate) fn attackers_to(board: &Board, square: Square, occupied: BitBoard) -> BitBoard {
    let pawns = *board.pieces(Piece::Pawn);
    let white = *board.color_combined(Color::White);
    let black = *board.color_combined(Color::Black);
    let diagonal = *board.pieces(Piece::Bishop) | *board.pieces(Piece::Queen);
    let straight = *board.pieces(Piece::Rook) | *board.pieces(Piece::Queen);

    // A white pawn on `p` attacks `square` exactly when `p` is one of the squares a black
    // pawn standing on `square` would attack, so the tables are probed with the colour
    // inverted.
    let mut attackers = get_pawn_attacks(square, Color::Black, pawns & white)
        | get_pawn_attacks(square, Color::White, pawns & black)
        | (get_knight_moves(square) & *board.pieces(Piece::Knight))
        | (get_king_moves(square) & *board.pieces(Piece::King));
    attackers |= get_bishop_moves(square, occupied) & diagonal;
    attackers |= get_rook_moves(square, occupied) & straight;
    attackers & occupied
}

/// The cheapest piece of `color` among `attackers`, as (piece, its square).
pub(crate) fn least_valuable(board: &Board, attackers: BitBoard, color: Color) -> Option<(Piece, Square)> {
    let mine = attackers & *board.color_combined(color);
    for piece in [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
        Piece::King,
    ] {
        let candidates = mine & *board.pieces(piece);
        if candidates != chess::EMPTY {
            return Some((piece, candidates.to_square()));
        }
    }
    None
}

/// Static exchange evaluation: the material the side to move nets from playing `m` if
/// both sides then recapture on that square with their cheapest piece until neither
/// wants to continue.
///
/// This is a static estimate, not a search -- it ignores pins, intermediate tactics and
/// the possibility that recapturing is simply bad. It exists to answer one cheap
/// question: "is this capture obviously losing material?" A negative result means yes.
pub(crate) fn see(board: &Board, m: ChessMove) -> i32 {
    let target = m.get_dest();
    let source = m.get_source();
    let Some(mut attacker) = board.piece_on(source) else {
        return 0;
    };

    let mut occupied = *board.combined();

    // Value of the piece being captured on this first move.
    let mut gain = [0i32; 32];
    gain[0] = if is_en_passant(board, m) {
        // The captured pawn sits behind the destination square; clear it from the
        // occupancy so sliders through that square are seen correctly.
        let captured = target.ubackward(board.side_to_move());
        occupied &= !BitBoard::from_square(captured);
        SEE_VALUES[Piece::Pawn.to_index()]
    } else {
        board
            .piece_on(target)
            .map_or(0, |p| SEE_VALUES[p.to_index()])
    };

    // A promotion arrives on the target square as the promoted piece, and the pawn's own
    // value is replaced.
    if let Some(promotion) = m.get_promotion() {
        gain[0] += SEE_VALUES[promotion.to_index()] - SEE_VALUES[Piece::Pawn.to_index()];
        attacker = promotion;
    }

    occupied &= !BitBoard::from_square(source);
    let mut side = !board.side_to_move();
    let mut depth = 0usize;

    loop {
        depth += 1;
        if depth >= gain.len() {
            break;
        }
        // If `side` recaptures, it wins the attacker standing on the target square but
        // exposes its own recapturing piece.
        gain[depth] = SEE_VALUES[attacker.to_index()] - gain[depth - 1];

        let attackers = attackers_to(board, target, occupied);
        let Some((next, from)) = least_valuable(board, attackers, side) else {
            break;
        };
        // Recapturing with the king is only legal if the opponent has no attackers left;
        // treating it as available anyway would over-value the exchange.
        if next == Piece::King
            && least_valuable(board, attackers_to(board, target, occupied), !side).is_some()
        {
            break;
        }
        occupied &= !BitBoard::from_square(from);
        attacker = next;
        side = !side;
    }

    // Walk back up the swap list: at each level the side to move can decline the
    // recapture, so it takes the better of "stop here" and "continue".
    while depth > 1 {
        depth -= 1;
        gain[depth - 1] = -(-gain[depth - 1]).max(gain[depth]);
    }
    gain[0]
}
