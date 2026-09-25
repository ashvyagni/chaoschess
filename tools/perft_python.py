#!/usr/bin/env python3
"""Perft the legacy pure-Python move generator against known-correct node counts.

The counts below are the standard Chess Programming Wiki perft results. They are
facts about chess, not about this repository, so any mismatch is a defect in
``moves.py``. Run with --max-depth to trade coverage for runtime; the Python
generator is slow enough that depth 4 on a busy position takes minutes.

Exit status is non-zero if any position fails, so this doubles as a regression gate.
"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from board import Board  # noqa: E402
from moves import apply_move, generate_legal_moves  # noqa: E402

# name -> (fen, [perft(1), perft(2), ...])
POSITIONS: dict[str, tuple[str, list[int]]] = {
    "startpos": (
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        [20, 400, 8_902, 197_281, 4_865_609],
    ),
    "kiwipete": (
        "r3k2r/p1ppqpb1/bn2pnp1/2pP4/1p2P3/2N2N2/PPPQBPPP/R3K2R w KQkq - 0 1",
        [48, 2_039, 97_862, 4_085_603],
    ),
    "cpw-pos3": (
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        [14, 191, 2_812, 43_238, 674_624],
    ),
    "cpw-pos4": (
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        [6, 264, 9_467, 422_333],
    ),
    "cpw-pos5": (
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 0 1",
        [44, 1_486, 62_379],
    ),
    "cpw-pos6": (
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 1",
        [46, 2_079, 89_890],
    ),
}


def perft(board: Board, depth: int) -> int:
    if depth == 0:
        return 1
    if depth == 1:
        return len(generate_legal_moves(board))
    return sum(perft(apply_move(board, m), depth - 1) for m in generate_legal_moves(board))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--max-depth", type=int, default=3,
                        help="cap the depth searched per position (default 3)")
    parser.add_argument("--only", help="run a single named position")
    args = parser.parse_args()

    failures = 0
    print(f"{'position':12} {'depth':>5} {'expected':>12} {'actual':>12} {'delta':>10} {'time':>8}  verdict")
    print("-" * 82)
    for name, (fen, expected) in POSITIONS.items():
        if args.only and name != args.only:
            continue
        for depth, want in enumerate(expected[: args.max_depth], start=1):
            board = Board()
            board.set_fen(fen)
            start = time.perf_counter()
            try:
                got: int | str = perft(board, depth)
            except Exception as exc:  # a crash is also a failure
                got = f"{type(exc).__name__}"
            elapsed = time.perf_counter() - start
            ok = got == want
            failures += not ok
            delta = f"{got - want:+d}" if isinstance(got, int) else "-"
            print(f"{name:12} {depth:>5} {want:>12,} {got if isinstance(got,str) else format(got,',d'):>12} "
                  f"{delta:>10} {elapsed:>7.2f}s  {'pass' if ok else 'FAIL'}")

    print("-" * 82)
    if failures:
        print(f"{failures} position/depth combination(s) FAILED -- moves.py generates illegal chess.")
        print("See MASTER_ENGINE_AUDIT.md section G.1 for the four isolated root causes.")
    else:
        print("all checked positions match known-correct perft counts.")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
