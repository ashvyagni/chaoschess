"""Regression tests for the legacy Python move generator (moves.py / board.py).

Each test pins one of the seven defects found in the audit and the follow-up
differential testing. They run with nothing but the standard library:

    .venv/bin/python -m unittest discover -s tests_py -v

This generator decides which moves the GUI lets a human play, so these defects were
user-visible rules violations, not only engine internals.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from board import Board, sq_to_bit  # noqa: E402
from config import B_PAWN, BLACK, W_KNIGHT, W_PAWN, WHITE  # noqa: E402
from moves import apply_move, generate_legal_moves  # noqa: E402


def board(fen: str) -> Board:
    b = Board()
    b.set_fen(fen)
    return b


def legal(fen: str) -> set[str]:
    return {str(m) for m in generate_legal_moves(board(fen))}


def play(fen: str, uci: str) -> Board:
    b = board(fen)
    move = next(m for m in generate_legal_moves(b) if str(m) == uci)
    return apply_move(b, move)


def perft(b: Board, depth: int) -> int:
    if depth == 0:
        return 1
    moves = generate_legal_moves(b)
    if depth == 1:
        return len(moves)
    return sum(perft(apply_move(b, m), depth - 1) for m in moves)


class LegalityFilter(unittest.TestCase):
    """Defect 1: the filter tested the opponent's king instead of the mover's."""

    def test_moves_that_leave_own_king_attacked_are_rejected(self):
        moves = legal("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1")
        self.assertNotIn("e1d2", moves)  # still attacked along rank 2
        self.assertNotIn("e1f2", moves)
        self.assertEqual(moves, {"e1d1", "e1e2", "e1f1"})

    def test_checking_moves_are_allowed(self):
        # Qh5 is mate here (reversed fool's mate). The old filter discarded every move that
        # gave check, so it was not generated. (The audit's original example for this used
        # a position where e2 still blocks the queen -- d1h5 is illegal there regardless.)
        self.assertIn("d1h5", legal("rnbqkbnr/ppppp2p/5p2/6p1/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 3"))


class Castling(unittest.TestCase):
    """Defect 2: the occupancy masks for White and Black were swapped."""

    def test_cannot_castle_through_own_pieces(self):
        self.assertNotIn("e1g1", legal("4k3/8/8/8/8/8/8/R3KBNR w KQ - 0 1"))

    def test_can_castle_when_path_is_clear_even_if_opponent_back_rank_is_full(self):
        # Black pieces on the squares White's masks used to test by mistake, placed so that
        # none of them attacks White's castling path.
        self.assertIn("e1g1", legal("4kbnr/8/8/8/8/8/8/R3K2R w KQ - 0 1"))
        self.assertIn("e1c1", legal("1nbnk3/8/8/8/8/8/8/R3K2R w KQ - 0 1"))

    def test_cannot_castle_through_an_attacked_square(self):
        # The queen on d8 attacks d1, which the king crosses when castling queenside.
        self.assertNotIn("e1c1", legal("3qk3/8/8/8/8/8/8/R3K2R w KQ - 0 1"))

    def test_black_castling_uses_rank_eight(self):
        self.assertIn("e8g8", legal("r3k2r/8/8/8/8/8/8/RNBQKBNR b kq - 0 1"))
        self.assertNotIn("e8g8", legal("r3kbnr/8/8/8/8/8/8/4K3 b kq - 0 1"))

    def test_rights_without_a_rook_do_not_castle(self):
        self.assertNotIn("e1g1", legal("4k3/8/8/8/8/8/8/R3K3 w KQ - 0 1"))


class EnPassant(unittest.TestCase):
    """Defect 3: the capture removed the wrong square, and the mover's own colour."""

    def test_white_en_passant_removes_the_black_pawn(self):
        after = play("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6")
        self.assertEqual(after.pieces[B_PAWN], 0)
        self.assertTrue(after.pieces[W_PAWN] & sq_to_bit(43))  # d6

    def test_black_en_passant_removes_the_white_pawn(self):
        after = play("4k3/8/8/8/3Pp3/8/8/4K3 b - d3 0 1", "e4d3")
        self.assertEqual(after.pieces[W_PAWN], 0)
        self.assertTrue(after.pieces[B_PAWN] & sq_to_bit(19))  # d3


class Promotion(unittest.TestCase):
    """Defect 4: promoting to a knight placed a pawn on the last rank."""

    def test_every_promotion_piece(self):
        for uci, symbol in (("b7b8n", "N"), ("b7b8b", "B"), ("b7b8r", "R"), ("b7b8q", "Q")):
            after = play("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1", uci)
            self.assertEqual("PNBRQKpnbrqk"[after.piece_at(57)], symbol, uci)
        self.assertTrue(play("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1", "b7b8n").pieces[W_KNIGHT])


class KnightMoves(unittest.TestCase):
    """Defect 5: only the two-up-one-over jumps were generated (4 of 8)."""

    def test_centralised_knight_has_eight_moves(self):
        moves = {m for m in legal("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1") if m.startswith("d4")}
        self.assertEqual(moves, {"d4b3", "d4b5", "d4c2", "d4c6", "d4e2", "d4e6", "d4f3", "d4f5"})

    def test_corner_knight_does_not_wrap_around_the_board(self):
        moves = {m for m in legal("4k3/8/8/8/8/8/8/N3K3 w - - 0 1") if m.startswith("a1")}
        self.assertEqual(moves, {"a1b3", "a1c2"})


class BlackPawnCaptures(unittest.TestCase):
    """Defect 6: Black capture source squares used the other diagonal's offset."""

    def test_black_pawn_captures_from_its_own_square(self):
        self.assertIn("d5e4", legal("4k3/8/8/3p4/4P3/8/8/4K3 b - - 0 1"))
        self.assertIn("d5c4", legal("4k3/8/8/3p4/2P5/8/8/4K3 b - - 0 1"))
        self.assertNotIn("f5e4", legal("4k3/8/8/3p4/4P3/8/8/4K3 b - - 0 1"))


class DoublePush(unittest.TestCase):
    """Defect 7: any pawn could advance two squares, not only from its start rank.

    Found by tools/diff_movegen.py after the first six fixes, not by the audit.
    """

    def test_only_start_rank_pawns_double_push(self):
        moves = legal("4k3/8/8/8/8/P7/1P6/4K3 w - - 0 1")
        self.assertIn("b2b4", moves)
        self.assertNotIn("a3a5", moves)
        self.assertNotIn("d6d4", legal("4k3/8/3p4/8/8/8/8/4K3 b - - 0 1"))


class Perft(unittest.TestCase):
    """Known-correct perft counts (Chess Programming Wiki), kept shallow for speed.

    Deeper checks: tools/perft_python.py and tools/diff_movegen.py.
    """

    CASES = [
        ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 3, 8_902),
        ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", 2, 2_039),
        ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 3, 2_812),
        ("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1", 3, 9_467),
        ("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 0 1", 2, 1_486),
    ]

    def test_perft(self):
        for fen, depth, expected in self.CASES:
            with self.subTest(fen=fen, depth=depth):
                self.assertEqual(perft(board(fen), depth), expected)


if __name__ == "__main__":
    unittest.main()


class GameEnd(unittest.TestCase):
    """Rules the GUI uses to end a game (moves.game_over_reason)."""

    def keys_after(self, fen, moves):
        from moves import position_key
        b = board(fen)
        keys = [position_key(b)]
        for uci in moves:
            b = apply_move(b, next(m for m in generate_legal_moves(b) if str(m) == uci))
            keys.append(position_key(b))
        return b, keys

    def test_threefold_repetition(self):
        from moves import game_over_reason
        start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        shuffle = ["g1f3", "g8f6", "f3g1", "f6g8"]
        b, keys = self.keys_after(start, shuffle)
        self.assertIsNone(game_over_reason(b, keys), "twofold is not a draw")
        b, keys = self.keys_after(start, shuffle * 2)
        self.assertEqual(game_over_reason(b, keys), "threefold repetition")

    def test_unusable_en_passant_square_does_not_hide_a_repetition(self):
        from moves import position_key
        # After 1.e4 nothing can capture en passant, so the position equals the same
        # placement reached without a double push.
        after_e4 = board("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1")
        same = board("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
        self.assertEqual(position_key(after_e4), position_key(same))

    def test_insufficient_material_and_other_endings(self):
        from moves import game_over_reason
        self.assertEqual(game_over_reason(board("8/8/8/4k3/8/8/8/2B1K3 w - - 0 1"), []),
                         "insufficient material")
        self.assertIsNone(game_over_reason(board("8/8/8/4k3/8/8/8/R3K3 w - - 0 1"), []))
        self.assertEqual(game_over_reason(board("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1"), []), "stalemate")
        self.assertEqual(game_over_reason(board("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1"), []), "checkmate")
        self.assertEqual(game_over_reason(board("4k3/8/8/8/8/8/P7/R3K3 w - - 100 90"), []),
                         "fifty-move rule")
