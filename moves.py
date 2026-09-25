from board import *

class Move:
    __slots__ = ('from_sq', 'to_sq', 'promo', 'castle', 'ep')

    def __init__(self, frm, to, promo=-1, castle=False, ep=False):
        self.from_sq = frm
        self.to_sq = to
        self.promo = promo
        self.castle = castle
        self.ep = ep

    def __eq__(self, other):
        return self.from_sq == other.from_sq and self.to_sq == other.to_sq and self.promo == other.promo

    def __hash__(self):
        return hash((self.from_sq, self.to_sq, self.promo))

    def __str__(self):
        f = chr(ord('a') + sq_file(self.from_sq)) + str(sq_rank(self.from_sq) + 1)
        t = chr(ord('a') + sq_file(self.to_sq)) + str(sq_rank(self.to_sq) + 1)
        p = ''
        if self.promo == PROMO_KNIGHT: p = 'n'
        elif self.promo == PROMO_BISHOP: p = 'b'
        elif self.promo == PROMO_ROOK: p = 'r'
        elif self.promo == PROMO_QUEEN: p = 'q'
        return f + t + p

    def to_index(self):
        return self.from_sq * 64 + self.to_sq

    def is_capture(self, board):
        mask = sq_to_bit(self.to_sq)
        if self.ep:
            return True
        if board.color == WHITE:
            return bool(mask & board.black_pieces())
        else:
            return bool(mask & board.white_pieces())


def generate_pseudo_moves(board):
    moves = []
    occ = board.all_pieces()
    friendly = board.friendly()
    enemy = board.enemy()

    if board.color == WHITE:
        pawns = board.pieces[W_PAWN]
        knight_sq = W_KNIGHT
        bishop_sq = W_BISHOP
        rook_sq = W_ROOK
        queen_sq = W_QUEEN
        king_sq = W_KING
        promo_rank = RANK_8
        push = shift_n
        dbl_push = lambda bb: shift_n(shift_n(bb))
        left_cap = shift_nw
        right_cap = shift_ne
        start_rank = RANK_2
        ep_row = 5
    else:
        pawns = board.pieces[B_PAWN]
        knight_sq = B_KNIGHT
        bishop_sq = B_BISHOP
        rook_sq = B_ROOK
        queen_sq = B_QUEEN
        king_sq = B_KING
        promo_rank = RANK_1
        push = shift_s
        dbl_push = lambda bb: shift_s(shift_s(bb))
        left_cap = shift_se
        right_cap = shift_sw
        start_rank = RANK_7
        ep_row = 2

    empty = ~occ & 0xFFFFFFFFFFFFFFFF
    single_push = push(pawns) & empty
    while single_push:
        to_sq, single_push = pop_lsb(single_push)
        from_sq = to_sq - (8 if board.color == WHITE else -8)
        if sq_to_bit(to_sq) & promo_rank:
            for pr in range(4):
                moves.append(Move(from_sq, to_sq, promo=pr))
        else:
            moves.append(Move(from_sq, to_sq))

    # Only pawns still on their starting rank may advance two squares. `start_rank` was
    # defined above but never applied, so any pawn could double-push from anywhere.
    double_push = dbl_push(pawns & start_rank) & empty & push(empty)
    while double_push:
        to_sq, double_push = pop_lsb(double_push)
        from_sq = to_sq - (16 if board.color == WHITE else -16)
        moves.append(Move(from_sq, to_sq))

    # White: left_cap = shift_nw (+7), right_cap = shift_ne (+9).
    # Black: left_cap = shift_se (-7), right_cap = shift_sw (-9).
    # The source square is the destination minus that same shift. The original code used
    # -9/-7 for Black, i.e. the other diagonal, so Black captures named an empty square.
    left_shift = 7 if board.color == WHITE else -7
    right_shift = 9 if board.color == WHITE else -9

    left_captures = left_cap(pawns) & enemy
    while left_captures:
        to_sq, left_captures = pop_lsb(left_captures)
        from_sq = to_sq - left_shift
        if sq_to_bit(to_sq) & promo_rank:
            for pr in range(4):
                moves.append(Move(from_sq, to_sq, promo=pr))
        else:
            moves.append(Move(from_sq, to_sq))

    right_captures = right_cap(pawns) & enemy
    while right_captures:
        to_sq, right_captures = pop_lsb(right_captures)
        from_sq = to_sq - right_shift
        if sq_to_bit(to_sq) & promo_rank:
            for pr in range(4):
                moves.append(Move(from_sq, to_sq, promo=pr))
        else:
            moves.append(Move(from_sq, to_sq))

    if board.ep_sq >= 0 and sq_rank(board.ep_sq) == ep_row:
        ep_mask = sq_to_bit(board.ep_sq)
        ep_pawns_left = left_cap(pawns) & ep_mask
        ep_pawns_right = right_cap(pawns) & ep_mask
        while ep_pawns_left:
            to_sq, ep_pawns_left = pop_lsb(ep_pawns_left)
            moves.append(Move(to_sq - left_shift, to_sq, ep=True))
        while ep_pawns_right:
            to_sq, ep_pawns_right = pop_lsb(ep_pawns_right)
            moves.append(Move(to_sq - right_shift, to_sq, ep=True))

    knights = board.pieces[knight_sq]
    while knights:
        from_sq, knights = pop_lsb(knights)
        targets = board.knight_attacks(from_sq) & ~friendly
        while targets:
            to_sq, targets = pop_lsb(targets)
            moves.append(Move(from_sq, to_sq))

    bishops = board.pieces[bishop_sq]
    while bishops:
        from_sq, bishops = pop_lsb(bishops)
        targets = board.sliding_attacks(from_sq, occ, True) & ~friendly
        while targets:
            to_sq, targets = pop_lsb(targets)
            moves.append(Move(from_sq, to_sq))

    rooks = board.pieces[rook_sq]
    while rooks:
        from_sq, rooks = pop_lsb(rooks)
        targets = board.sliding_attacks(from_sq, occ, False) & ~friendly
        while targets:
            to_sq, targets = pop_lsb(targets)
            moves.append(Move(from_sq, to_sq))

    queens = board.pieces[queen_sq]
    while queens:
        from_sq, queens = pop_lsb(queens)
        targets = board.sliding_attacks(from_sq, occ, False) & ~friendly
        targets |= board.sliding_attacks(from_sq, occ, True) & ~friendly
        while targets:
            to_sq, targets = pop_lsb(targets)
            moves.append(Move(from_sq, to_sq))

    king = board.pieces[king_sq]
    ksq = bit_scan(king)
    targets = board.king_attacks(ksq) & ~friendly
    while targets:
        to_sq, targets = pop_lsb(targets)
        moves.append(Move(ksq, to_sq))

    # Squares that must be empty: f1 g1 = 0x60, b1 c1 d1 = 0x0E, and the same shifted to
    # rank 8. The original code had the rank-1 and rank-8 masks swapped between the two
    # colours, so White castled through its own pieces whenever Black's were clear.
    # The rook must actually be on its corner too, in case castling rights arrived from
    # an inconsistent FEN.
    if board.color == WHITE:
        if (board.castling[0] and not (occ & 0x0000000000000060)
                and board.pieces[W_ROOK] & sq_to_bit(7) and board.pieces[W_KING] & sq_to_bit(4)):
            if not board.is_attacked(4, BLACK) and not board.is_attacked(5, BLACK) and not board.is_attacked(6, BLACK):
                moves.append(Move(4, 6, castle=True))
        if (board.castling[1] and not (occ & 0x000000000000000E)
                and board.pieces[W_ROOK] & sq_to_bit(0) and board.pieces[W_KING] & sq_to_bit(4)):
            if not board.is_attacked(4, BLACK) and not board.is_attacked(3, BLACK) and not board.is_attacked(2, BLACK):
                moves.append(Move(4, 2, castle=True))
    else:
        if (board.castling[2] and not (occ & 0x6000000000000000)
                and board.pieces[B_ROOK] & sq_to_bit(63) and board.pieces[B_KING] & sq_to_bit(60)):
            if not board.is_attacked(60, WHITE) and not board.is_attacked(61, WHITE) and not board.is_attacked(62, WHITE):
                moves.append(Move(60, 62, castle=True))
        if (board.castling[3] and not (occ & 0x0E00000000000000)
                and board.pieces[B_ROOK] & sq_to_bit(56) and board.pieces[B_KING] & sq_to_bit(60)):
            if not board.is_attacked(60, WHITE) and not board.is_attacked(59, WHITE) and not board.is_attacked(58, WHITE):
                moves.append(Move(60, 58, castle=True))

    return moves


def generate_legal_moves(board):
    """Pseudo-legal moves that do not leave the mover's own king attacked.

    apply_move() hands the turn to the opponent, so the king to test is the one belonging
    to the side that just moved. The original code called in_check() on the new board,
    which tests the *opponent's* king: it discarded every legal checking move and kept
    every move that left the mover in check.
    """
    mover = board.color
    opponent = 1 - mover
    legal = []
    for move in generate_pseudo_moves(board):
        new_board = apply_move(board, move)
        if not new_board.is_attacked(new_board.find_king(mover), opponent):
            legal.append(move)
    return legal


def apply_move(board, move):
    new = board.copy()
    piece = new.piece_at(move.from_sq)
    captured = new.piece_at(move.to_sq)

    if captured != -1:
        new.remove_piece(move.to_sq, captured)

    if move.ep:
        # The captured pawn is behind the destination from the mover's point of view, and
        # it belongs to the opponent. The original code looked the wrong way and removed
        # the mover's own colour, so nothing was removed and the captured pawn survived.
        ep_pawn_sq = move.to_sq - 8 if new.color == WHITE else move.to_sq + 8
        new.remove_piece(ep_pawn_sq, B_PAWN if new.color == WHITE else W_PAWN)

    new.move_piece(move.from_sq, move.to_sq, piece)

    if move.promo != -1:
        new.remove_piece(move.to_sq, piece)
        # Indexed by PROMO_KNIGHT=0, PROMO_BISHOP=1, PROMO_ROOK=2, PROMO_QUEEN=3. The first
        # entry used to be the pawn, so "promote to knight" left a pawn on the last rank.
        promo_map = ([W_KNIGHT, W_BISHOP, W_ROOK, W_QUEEN] if new.color == WHITE
                     else [B_KNIGHT, B_BISHOP, B_ROOK, B_QUEEN])
        new.set_piece(move.to_sq, promo_map[move.promo])

    if move.castle:
        if move.to_sq == 6:
            new.move_piece(7, 5, W_ROOK)
        elif move.to_sq == 2:
            new.move_piece(0, 3, W_ROOK)
        elif move.to_sq == 62:
            new.move_piece(63, 61, B_ROOK)
        elif move.to_sq == 58:
            new.move_piece(56, 59, B_ROOK)

    new.castling[0] = new.castling[0] and move.from_sq != 4 and move.to_sq != 4 and move.from_sq != 7
    new.castling[1] = new.castling[1] and move.from_sq != 4 and move.to_sq != 4 and move.from_sq != 0
    new.castling[2] = new.castling[2] and move.from_sq != 60 and move.to_sq != 60 and move.from_sq != 63
    new.castling[3] = new.castling[3] and move.from_sq != 60 and move.to_sq != 60 and move.from_sq != 56
    if move.from_sq == 7 or move.to_sq == 7: new.castling[0] = False
    if move.from_sq == 0 or move.to_sq == 0: new.castling[1] = False
    if move.from_sq == 63 or move.to_sq == 63: new.castling[2] = False
    if move.from_sq == 56 or move.to_sq == 56: new.castling[3] = False

    new.ep_sq = -1
    if piece != -1:
        is_pawn = piece in (W_PAWN, B_PAWN)
        dist = abs(sq_rank(move.to_sq) - sq_rank(move.from_sq))
        if is_pawn and dist == 2:
            ep_rank = (sq_rank(move.from_sq) + sq_rank(move.to_sq)) // 2
            new.ep_sq = make_sq(sq_file(move.from_sq), ep_rank)

    if piece not in (W_PAWN, B_PAWN) and captured == -1 and not move.ep:
        new.halfmove += 1
    else:
        new.halfmove = 0

    if new.color == BLACK:
        new.fullmove += 1
    new.color = 1 - new.color
    return new


def is_checkmate(board):
    return board.in_check() and len(generate_legal_moves(board)) == 0


def is_stalemate(board):
    return not board.in_check() and len(generate_legal_moves(board)) == 0


def is_game_over(board):
    if len(generate_legal_moves(board)) == 0:
        return True
    if board.halfmove >= 100:
        return True
    return False


def position_key(board):
    """Identity of a position for repetition (FIDE 9.2.2): piece placement, side to move,
    castling rights, and the en passant square *only if an en passant capture is actually
    legal*. board.ep_sq is set after every double push, whether or not anything can capture,
    so including it unconditionally would hide real repetitions.
    """
    placement, side, castling, ep = board.to_fen().split()[:4]
    if ep != '-' and not any(m.ep for m in generate_legal_moves(board)):
        ep = '-'
    return f"{placement} {side} {castling} {ep}"


def insufficient_material(board):
    """Neither side can mate: bare kings, one minor piece, or only same-coloured bishops.
    Same rule as the Rust engine's insufficient_material()."""
    heavy = (board.pieces[W_PAWN] | board.pieces[B_PAWN] | board.pieces[W_ROOK]
             | board.pieces[B_ROOK] | board.pieces[W_QUEEN] | board.pieces[B_QUEEN])
    if heavy:
        return False
    knights = board.pieces[W_KNIGHT] | board.pieces[B_KNIGHT]
    bishops = board.pieces[W_BISHOP] | board.pieces[B_BISHOP]
    if count_bits(knights | bishops) <= 1:
        return True
    light = 0x55AA55AA55AA55AA
    on_light = count_bits(bishops & light)
    return knights == 0 and on_light in (0, count_bits(bishops))


def game_over_reason(board, keys):
    """Why the game is over, or None. `keys` holds position_key() of every position in the
    game so far, the current one last.

    Checkmate is tested first: a mate on the hundredth halfmove is still mate.
    """
    legal = generate_legal_moves(board)
    if not legal:
        return "checkmate" if board.in_check() else "stalemate"
    if board.halfmove >= 100:
        return "fifty-move rule"
    if keys and keys.count(keys[-1]) >= 3:
        return "threefold repetition"
    if insufficient_material(board):
        return "insufficient material"
    return None


def get_result(board):
    if is_checkmate(board):
        return -1 if board.color == WHITE else 1
    return 0


def encode_board(board):
    planes = []
    piece_order = [W_PAWN, W_KNIGHT, W_BISHOP, W_ROOK, W_QUEEN, W_KING,
                   B_PAWN, B_KNIGHT, B_BISHOP, B_ROOK, B_QUEEN, B_KING]
    for p in piece_order:
        plane = [0.0] * 64
        bb = board.pieces[p]
        while bb:
            sq, bb = pop_lsb(bb)
            plane[sq] = 1.0
        planes.append(plane)

    cast_plane = [float(c) for c in board.castling]
    ep_plane = [0.0] * 64
    if board.ep_sq >= 0:
        ep_plane[board.ep_sq] = 1.0
    side_plane = [float(board.color)] * 64

    flat = []
    for p in planes:
        flat.extend(p)
    flat.extend(cast_plane)
    flat.extend(ep_plane[:4])
    flat.extend(side_plane[:1])
    return flat
