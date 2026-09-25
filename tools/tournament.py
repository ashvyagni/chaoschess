"""Deterministic local tournament harness for comparing engine personalities."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


def play_game(
    binary: Path, white_style: str, black_style: str, depth: int, movetime: int, max_plies: int
) -> str:
    process = subprocess.Popen(
        [str(binary)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    assert process.stdin and process.stdout
    process.stdin.write("uci\nisready\n")
    process.stdin.write(f"setoption name Style value {white_style}\n")
    process.stdin.write("ucinewgame\n")
    process.stdin.write("position startpos\n")
    process.stdin.flush()
    for line in process.stdout:
        if line.strip() == "readyok":
            break

    moves: list[str] = []
    for _ in range(max_plies):
        style = white_style if len(moves) % 2 == 0 else black_style
        process.stdin.write(f"setoption name Style value {style}\n")
        process.stdin.write("position startpos moves " + " ".join(moves) + "\n")
        process.stdin.write(f"go depth {depth} movetime {movetime}\n")
        process.stdin.flush()
        best = next((line.split()[1] for line in process.stdout if line.startswith("bestmove ")), "0000")
        if best == "0000":
            break
        moves.append(best)
    process.stdin.write("quit\n")
    process.stdin.flush()
    process.wait(timeout=5)
    return " ".join(moves)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/crazy-chess"))
    parser.add_argument("--depth", type=int, default=1)
    parser.add_argument("--movetime", type=int, default=100)
    parser.add_argument("--plies", type=int, default=20)
    args = parser.parse_args()
    game = play_game(args.binary, "Chaos", "Classical", args.depth, args.movetime, args.plies)
    print(f"chaos-vs-classical plies={len(game.split())} moves={game}")


if __name__ == "__main__":
    main()
