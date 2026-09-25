from config import *

def sq_to_bit(sq):
    return 1 << sq

def sq_file(sq):
    return sq & 7

def sq_rank(sq):
    return sq >> 3

def make_sq(file, rank):
    return rank * 8 + file

def bit_scan(bb):
    if bb == 0:
        return -1
    return (bb & -bb).bit_length() - 1

def pop_lsb(bb):
    sq = bit_scan(bb)
    bb &= bb - 1
    return sq, bb

def count_bits(bb):
    return bb.bit_count() if hasattr(int, 'bit_count') else bin(bb).count('1')

def iter_bits(bb):
    while bb:
        sq, bb = pop_lsb(bb)
        yield sq

def more_than_one(bb):
    return bb & (bb - 1) != 0

FILE_A = 0x0101010101010101
FILE_B = 0x0202020202020202
FILE_G = 0x4040404040404040
FILE_H = 0x8080808080808080
RANK_1 = 0x00000000000000FF
RANK_2 = 0x000000000000FF00
RANK_4 = 0x00000000FF000000
RANK_5 = 0x000000FF00000000
RANK_7 = 0x00FF000000000000
RANK_8 = 0xFF00000000000000

NOT_FILE_A = ~FILE_A & 0xFFFFFFFFFFFFFFFF
NOT_FILE_H = ~FILE_H & 0xFFFFFFFFFFFFFFFF
NOT_FILE_AB = ~(FILE_A | FILE_B) & 0xFFFFFFFFFFFFFFFF
NOT_FILE_GH = ~(FILE_G | FILE_H) & 0xFFFFFFFFFFFFFFFF

def shift_n(bb):
    return (bb << 8) & 0xFFFFFFFFFFFFFFFF

def shift_s(bb):
    return bb >> 8

def shift_ne(bb):
    return (bb << 9) & 0xFFFFFFFFFFFFFFFF & NOT_FILE_A

def shift_nw(bb):
    return (bb << 7) & 0xFFFFFFFFFFFFFFFF & NOT_FILE_H

def shift_se(bb):
    return (bb >> 7) & NOT_FILE_A

def shift_sw(bb):
    return (bb >> 9) & NOT_FILE_H


def _knight_table():
    table = []
    for sq in range(64):
        f, r = sq_file(sq), sq_rank(sq)
        bb = 0
        for df, dr in ((1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)):
            nf, nr = f + df, r + dr
            if 0 <= nf < 8 and 0 <= nr < 8:
                bb |= 1 << make_sq(nf, nr)
        table.append(bb)
    return table


KNIGHT_ATTACKS = _knight_table()


class Board:
    def __init__(self):
        self.pieces = [0] * 12
        self.color = WHITE
        self.castling = [False, False, False, False]
        self.ep_sq = -1
        self.halfmove = 0
        self.fullmove = 1
        self.hash = 0
        self.history = []

    def copy(self):
        b = Board()
        b.pieces = self.pieces[:]
        b.color = self.color
        b.castling = self.castling[:]
        b.ep_sq = self.ep_sq
        b.halfmove = self.halfmove
        b.fullmove = self.fullmove
        return b

    def all_pieces(self):
        bb = 0
        for p in self.pieces:
            bb |= p
        return bb

    def white_pieces(self):
        return self.pieces[W_PAWN] | self.pieces[W_KNIGHT] | self.pieces[W_BISHOP] | \
               self.pieces[W_ROOK] | self.pieces[W_QUEEN] | self.pieces[W_KING]

    def black_pieces(self):
        return self.pieces[B_PAWN] | self.pieces[B_KNIGHT] | self.pieces[B_BISHOP] | \
               self.pieces[B_ROOK] | self.pieces[B_QUEEN] | self.pieces[B_KING]

    def friendly(self):
        return self.white_pieces() if self.color == WHITE else self.black_pieces()

    def enemy(self):
        return self.black_pieces() if self.color == WHITE else self.white_pieces()

    def piece_at(self, sq):
        mask = sq_to_bit(sq)
        for i in range(12):
            if self.pieces[i] & mask:
                return i
        return -1

    def set_piece(self, sq, piece):
        mask = sq_to_bit(sq)
        self.pieces[piece] |= mask

    def remove_piece(self, sq, piece):
        mask = ~(sq_to_bit(sq))
        self.pieces[piece] &= mask

    def move_piece(self, sq_from, sq_to, piece):
        self.remove_piece(sq_from, piece)
        self.set_piece(sq_to, piece)

    def is_attacked(self, sq, by_color):
        if sq < 0:
            return True
        occ = self.all_pieces()
        if by_color == WHITE:
            if shift_sw(sq_to_bit(sq)) & self.pieces[W_PAWN]:
                return True
            if shift_se(sq_to_bit(sq)) & self.pieces[W_PAWN]:
                return True
            if self.knight_attacks(sq) & self.pieces[W_KNIGHT]:
                return True
            if self.sliding_attacks(sq, occ, True) & (self.pieces[W_BISHOP] | self.pieces[W_QUEEN]):
                return True
            if self.sliding_attacks(sq, occ, False) & (self.pieces[W_ROOK] | self.pieces[W_QUEEN]):
                return True
            if self.king_attacks(sq) & self.pieces[W_KING]:
                return True
        else:
            if shift_ne(sq_to_bit(sq)) & self.pieces[B_PAWN]:
                return True
            if shift_nw(sq_to_bit(sq)) & self.pieces[B_PAWN]:
                return True
            if self.knight_attacks(sq) & self.pieces[B_KNIGHT]:
                return True
            if self.sliding_attacks(sq, occ, True) & (self.pieces[B_BISHOP] | self.pieces[B_QUEEN]):
                return True
            if self.sliding_attacks(sq, occ, False) & (self.pieces[B_ROOK] | self.pieces[B_QUEEN]):
                return True
            if self.king_attacks(sq) & self.pieces[B_KING]:
                return True
        return False

    def sliding_attacks(self, sq, occ, diagonal):
        attacks = 0
        directions = [(1, 1), (1, -1), (-1, 1), (-1, -1)] if diagonal else [(1, 0), (-1, 0), (0, 1), (0, -1)]
        for df, dr in directions:
            f, r = sq_file(sq), sq_rank(sq)
            while True:
                f += df
                r += dr
                if f < 0 or f > 7 or r < 0 or r > 7:
                    break
                s = make_sq(f, r)
                attacks |= sq_to_bit(s)
                if occ & sq_to_bit(s):
                    break
        return attacks

    def king_attacks(self, sq):
        k = sq_to_bit(sq)
        attacks = shift_n(k) | shift_s(k)
        attacks |= shift_ne(k) | shift_nw(k) | shift_se(k) | shift_sw(k)
        attacks |= (k << 1) & NOT_FILE_A
        attacks |= (k >> 1) & NOT_FILE_H
        return attacks

    def knight_attacks(self, sq):
        # Precomputed from explicit (file, rank) offsets. The previous shift-composition
        # version applied the same two-up-one-over shift twice and never generated the
        # two-over-one-up jumps, so a centralised knight had 4 moves instead of 8.
        return KNIGHT_ATTACKS[sq]

    def in_check(self):
        if self.color == WHITE:
            king_sq = bit_scan(self.pieces[W_KING])
            return self.is_attacked(king_sq, BLACK)
        else:
            king_sq = bit_scan(self.pieces[B_KING])
            return self.is_attacked(king_sq, WHITE)

    def find_king(self, color):
        return bit_scan(self.pieces[W_KING if color == WHITE else B_KING])

    def set_fen(self, fen):
        self.pieces = [0] * 12
        parts = fen.split()
        rows = parts[0].split('/')
        for r in range(8):
            f = 0
            for ch in rows[r]:
                if ch.isdigit():
                    f += int(ch)
                else:
                    sq = make_sq(f, 7 - r)
                    piece_map = {
                        'P': W_PAWN, 'N': W_KNIGHT, 'B': W_BISHOP,
                        'R': W_ROOK, 'Q': W_QUEEN, 'K': W_KING,
                        'p': B_PAWN, 'n': B_KNIGHT, 'b': B_BISHOP,
                        'r': B_ROOK, 'q': B_QUEEN, 'k': B_KING,
                    }
                    self.set_piece(sq, piece_map[ch])
                    f += 1
        self.color = WHITE if parts[1] == 'w' else BLACK
        self.castling = [c in parts[2] for c in 'KQkq']
        self.ep_sq = -1 if parts[3] == '-' else make_sq(ord(parts[3][0]) - ord('a'), int(parts[3][1]) - 1)
        self.halfmove = int(parts[4]) if len(parts) > 4 else 0
        self.fullmove = int(parts[5]) if len(parts) > 5 else 1

    def to_fen(self):
        rows = []
        for r in range(7, -1, -1):
            row = ''
            empty = 0
            for f in range(8):
                sq = make_sq(f, r)
                piece = self.piece_at(sq)
                if piece == -1:
                    empty += 1
                else:
                    if empty:
                        row += str(empty)
                        empty = 0
                    chars = 'PNBRQKpnbrqk'
                    row += chars[piece]
            if empty:
                row += str(empty)
            rows.append(row)
        fen = '/'.join(rows)
        fen += ' w' if self.color == WHITE else ' b'
        cast = ''
        if self.castling[0]: cast += 'K'
        if self.castling[1]: cast += 'Q'
        if self.castling[2]: cast += 'k'
        if self.castling[3]: cast += 'q'
        fen += ' ' + (cast if cast else '-')
        fen += ' ' + (chr(ord('a') + sq_file(self.ep_sq)) + str(sq_rank(self.ep_sq) + 1) if self.ep_sq >= 0 else '-')
        fen += ' ' + str(self.halfmove)
        fen += ' ' + str(self.fullmove)
        return fen

    def __str__(self):
        piece_chars = {
            W_PAWN: 'P', W_KNIGHT: 'N', W_BISHOP: 'B', W_ROOK: 'R',
            W_QUEEN: 'Q', W_KING: 'K', B_PAWN: 'p', B_KNIGHT: 'n',
            B_BISHOP: 'b', B_ROOK: 'r', B_QUEEN: 'q', B_KING: 'k'
        }
        lines = []
        for r in range(7, -1, -1):
            row = str(r + 1) + ' '
            for f in range(8):
                sq = make_sq(f, r)
                piece = self.piece_at(sq)
                row += (piece_chars[piece] if piece != -1 else '.') + ' '
            lines.append(row)
        lines.append('  a b c d e f g h')
        return '\n'.join(lines)
