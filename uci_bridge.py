"""Small UCI client used by the legacy PySide6 front end."""

from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class SearchInfo:
    move: str
    depth: int
    nodes: int
    score_cp: int


MATE_CP = 100_000


def parse_info(line: str, depth: int, nodes: int, score_cp: int) -> tuple[int, int, int]:
    """Read depth, nodes and score from a UCI `info` line by keyword, not by position.

    UCI allows fields in any order and engines add fields over time, so fixed indices
    break -- the previous version read `fields[7]` and would crash on the current engine's
    `info depth D seldepth S multipv 1 score ...` format. A `score mate N` is mapped to a
    large centipawn value with the right sign so callers that only understand centipawns
    still rank it correctly.
    """
    fields = line.split()
    for i, field in enumerate(fields[:-1]):
        nxt = fields[i + 1]
        if field == "depth" and nxt.isdigit():
            depth = int(nxt)
        elif field == "nodes" and nxt.isdigit():
            nodes = int(nxt)
        elif field == "score" and i + 2 < len(fields):
            kind, value = nxt, fields[i + 2]
            try:
                n = int(value)
            except ValueError:
                continue
            if kind == "cp":
                score_cp = n
            elif kind == "mate":
                score_cp = MATE_CP - abs(n) if n > 0 else -MATE_CP + abs(n)
    return depth, nodes, score_cp


class UCIEngine:
    def __init__(self, root: Path | None = None, depth: int = 4, style: str = "Chaos"):
        root = root or Path(__file__).resolve().parent
        configured = os.environ.get("CRAZY_CHESS_ENGINE")
        binary = Path(configured) if configured else root / "target" / "release" / "crazy-chess"
        command = [str(binary)] if binary.exists() else ["cargo", "run", "--quiet", "--release"]
        self.process = subprocess.Popen(
            command,
            cwd=root,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.depth = max(1, min(depth, 12))
        self.last_info = SearchInfo("", 0, 0, 0)
        self._send("uci")
        self._wait_for("uciok")
        self._send("setoption", "name", "Style", "value", style)
        self._send("isready")
        self._wait_for("readyok")

    def _send(self, *parts: object) -> None:
        if self.process.stdin is None:
            raise RuntimeError("engine stdin is unavailable")
        self.process.stdin.write(" ".join(map(str, parts)) + "\n")
        self.process.stdin.flush()

    def _wait_for(self, expected: str) -> str:
        if self.process.stdout is None:
            raise RuntimeError("engine stdout is unavailable")
        for line in self.process.stdout:
            line = line.strip()
            if line == expected:
                return line
        raise RuntimeError(f"engine exited before replying with {expected}")

    def best_move(self, fen: str, moves: list[str] | None = None) -> str:
        # The FEN already describes the current board. Replaying the move list
        # after it would apply every move twice.
        self._send("position", "fen", *fen.split())
        self._send("go", "depth", self.depth)
        if self.process.stdout is None:
            raise RuntimeError("engine stdout is unavailable")
        depth = nodes = score_cp = 0
        for line in self.process.stdout:
            line = line.strip()
            if line.startswith("info ") and " score " in line:
                depth, nodes, score_cp = parse_info(line, depth, nodes, score_cp)
            if line.startswith("bestmove "):
                move = line.split(maxsplit=1)[1]
                if move == "0000":
                    raise RuntimeError("engine reported no legal move")
                self.last_info = SearchInfo(move, depth, nodes, score_cp)
                return move
        raise RuntimeError("engine exited before returning bestmove")

    def set_style(self, style: str) -> None:
        if style not in {"Classical", "Chaos"}:
            raise ValueError(f"unsupported engine style: {style}")
        self._send("setoption", "name", "Style", "value", style)
        self._send("isready")
        self._wait_for("readyok")

    def close(self) -> None:
        if self.process.poll() is None:
            try:
                self._send("quit")
                self.process.wait(timeout=2)
            except (BrokenPipeError, subprocess.TimeoutExpired):
                self.process.kill()
