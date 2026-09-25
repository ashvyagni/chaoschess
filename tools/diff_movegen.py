#!/usr/bin/env python3
"""Differential test of the legacy Python move generator against the Rust reference.

Walks the game tree from each test position. At every node it compares the set of
legal moves Python generates with the set the Rust `legal_moves` oracle generates.
The first disagreement is printed with the FEN and the exact extra/missing moves.

A perft mismatch only tells you a count is wrong somewhere below the root. This tells
you which position and which move.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from board import Board  # noqa: E402
from moves import apply_move, generate_legal_moves  # noqa: E402
from perft_python import POSITIONS  # type: ignore  # noqa: E402


class Oracle:
    def __init__(self, binary: Path):
        self.proc = subprocess.Popen([str(binary)], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, text=True, bufsize=1)

    def legal(self, fen: str) -> set[str] | str:
        assert self.proc.stdin and self.proc.stdout
        self.proc.stdin.write(fen + "\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline().strip()
        return line if line.startswith("ERROR") else set(line.split())


def walk(oracle: Oracle, board: Board, depth: int, path: list[str], found: list) -> None:
    if found and len(found) >= 5:
        return
    fen = board.to_fen()
    python_moves = {str(m): m for m in generate_legal_moves(board)}
    reference = oracle.legal(fen)
    if isinstance(reference, str):
        found.append((path, fen, f"oracle rejected FEN produced by Python: {reference}", set(), set()))
        return
    extra = set(python_moves) - reference
    missing = reference - set(python_moves)
    if extra or missing:
        found.append((path, fen, "move sets differ", extra, missing))
        return
    if depth == 0:
        return
    for text in sorted(python_moves):
        walk(oracle, apply_move(board, python_moves[text]), depth - 1, path + [text], found)
        if found:
            return


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--depth", type=int, default=3)
    parser.add_argument("--oracle", type=Path, default=ROOT / "target/release/legal_moves")
    args = parser.parse_args()
    if not args.oracle.exists():
        raise SystemExit("build the oracle first: cargo build --release --bin legal_moves")

    oracle = Oracle(args.oracle)
    failures = 0
    for name, (fen, _) in POSITIONS.items():
        board = Board()
        board.set_fen(fen)
        found: list = []
        walk(oracle, board, args.depth, [], found)
        if not found:
            print(f"{name:10} agree to depth {args.depth}")
            continue
        failures += 1
        path, bad_fen, why, extra, missing = found[0]
        print(f"{name:10} DISAGREE after {' '.join(path) or '(root)'}")
        print(f"           fen:     {bad_fen}")
        print(f"           {why}")
        if extra:
            print(f"           python generates but must not: {sorted(extra)}")
        if missing:
            print(f"           python misses:                 {sorted(missing)}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
