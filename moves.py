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

    double_push = dbl_push(pawns) & empty & push(empty)
    while double_push:
        to_sq, double_push = pop_lsb(double_push)
        from_sq = to_sq - (16 if board.color == WHITE else -16)
        moves.append(Move(from_sq, to_sq))

    left_captures = left_cap(pawns) & enemy
    while left_captures:
        to_sq, left_captures = pop_lsb(left_captures)
        from_sq = to_sq - (7 if board.color == WHITE else -9)
        if sq_to_bit(to_sq) & promo_rank:
            for pr in range(4):
                moves.append(Move(from_sq, to_sq, promo=pr))
        else:
            moves.append(Move(from_sq, to_sq))

    right_captures = right_cap(pawns) & enemy
    while right_captures:
        to_sq, right_captures = pop_lsb(right_captures)
        from_sq = to_sq - (9 if board.color == WHITE else -7)
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
            from_sq = to_sq - (7 if board.color == WHITE else -9)
            moves.append(Move(from_sq, to_sq, ep=True))
        while ep_pawns_right:
            to_sq, ep_pawns_right = pop_lsb(ep_pawns_right)
            from_sq = to_sq - (9 if board.color == WHITE else -7)
            moves.append(Move(from_sq, to_sq, ep=True))

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

    if board.color == WHITE:
        if board.castling[0] and not (occ & 0x6000000000000000):
            if not board.is_attacked(4, BLACK) and not board.is_attacked(5, BLACK) and not board.is_attacked(6, BLACK):
                moves.append(Move(4, 6, castle=True))
        if board.castling[1] and not (occ & 0x0E00000000000000):
            if not board.is_attacked(4, BLACK) and not board.is_attacked(3, BLACK) and not board.is_attacked(2, BLACK):
                moves.append(Move(4, 2, castle=True))
    else:
        if board.castling[2] and not (occ & 0x0000000000000060):
            if not board.is_attacked(60, WHITE) and not board.is_attacked(61, WHITE) and not board.is_attacked(62, WHITE):
                moves.append(Move(60, 62, castle=True))
        if board.castling[3] and not (occ & 0x000000000000000E):
            if not board.is_attacked(60, WHITE) and not board.is_attacked(59, WHITE) and not board.is_attacked(58, WHITE):
                moves.append(Move(60, 58, castle=True))

    return moves


def generate_legal_moves(board):
    pseudo = generate_pseudo_moves(board)
    legal = []
    for move in pseudo:
        new_board = apply_move(board, move)
        if not new_board.in_check():
            legal.append(move)
    return legal


def apply_move(board, move):
    new = board.copy()
    piece = new.piece_at(move.from_sq)
    captured = new.piece_at(move.to_sq)

    if captured != -1:
        new.remove_piece(move.to_sq, captured)

    if move.ep:
        ep_pawn_sq = move.to_sq + (8 if new.color == WHITE else -8)
        new.remove_piece(ep_pawn_sq, W_PAWN if new.color == WHITE else B_PAWN)

    new.move_piece(move.from_sq, move.to_sq, piece)

    if move.promo != -1:
        new.remove_piece(move.to_sq, piece)
        promo_map = [W_PAWN, W_BISHOP, W_ROOK, W_QUEEN] if new.color == WHITE else [B_PAWN, B_BISHOP, B_ROOK, B_QUEEN]
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
