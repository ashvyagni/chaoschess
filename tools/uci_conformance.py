#!/usr/bin/env python3
"""Probe the Rust engine's UCI conformance on the points that decide whether any
standard GUI or tournament manager can drive it.

Each check states what UCI requires, what the engine does, and why it matters. A
failing check is a defect in src/main.rs, not in this script. Exit status is the
number of failed checks, so this is usable as a gate.
"""

from __future__ import annotations

import argparse
import subprocess
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BIN = ROOT / "target" / "release" / "crazy-chess"


class Engine:
    """Drive the engine over real UCI, with a hard timeout on every expectation.

    The engine under audit can block forever (see audit G.6), so every read is
    bounded and the process is killed rather than waited on.
    """

    def __init__(self, binary: Path):
        self.proc = subprocess.Popen(
            [str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1,
        )
        self.lines: list[str] = []
        self._hit = threading.Event()
        self._needle = "\0"
        threading.Thread(target=self._reader, daemon=True).start()

    def _reader(self) -> None:
        assert self.proc.stdout
        for line in self.proc.stdout:
            self.lines.append(line.rstrip())
            if line.startswith(self._needle):
                self._hit.set()

    def send(self, *cmds: str) -> None:
        assert self.proc.stdin
        self.proc.stdin.write("".join(c + "\n" for c in cmds))
        self.proc.stdin.flush()

    def expect(self, prefix: str, timeout: float) -> tuple[bool, float, str | None]:
        """Wait for a line starting with `prefix`. Returns (seen, elapsed, line)."""
        self._needle = prefix
        self._hit.clear()
        for line in self.lines:                     # may already have arrived
            if line.startswith(prefix):
                return True, 0.0, line
        start = time.perf_counter()
        seen = self._hit.wait(timeout)
        elapsed = time.perf_counter() - start
        line = next((l for l in self.lines if l.startswith(prefix)), None)
        return seen, elapsed, line

    def close(self) -> None:
        self.proc.kill()


RESULTS: list[tuple[str, bool, str]] = []


def record(name: str, ok: bool, detail: str) -> None:
    RESULTS.append((name, ok, detail))
    print(f"  [{'PASS' if ok else 'FAIL'}] {name}\n         {detail}")


def check_handshake(binary: Path) -> None:
    e = Engine(binary)
    e.send("uci")
    ok, el, _ = e.expect("uciok", 5)
    record("uci -> uciok", ok, f"replied in {el*1000:.0f} ms" if ok else "no uciok within 5 s")
    e.send("isready")
    ok2, el2, _ = e.expect("readyok", 5)
    record("isready -> readyok", ok2, f"replied in {el2*1000:.0f} ms" if ok2 else "no readyok within 5 s")
    e.close()


def check_movetime(binary: Path, budgets=(200, 1000)) -> None:
    for ms in budgets:
        e = Engine(binary)
        e.send("uci", "isready", "position startpos", f"go movetime {ms}")
        ok, el, line = e.expect("bestmove", ms / 1000 * 4 + 3)
        e.close()
        if not ok:
            record(f"go movetime {ms}", False, f"no bestmove within {ms/1000*4+3:.0f} s")
            continue
        ratio = el / (ms / 1000)
        record(f"go movetime {ms}", ratio <= 2.0,
               f"used {el:.2f} s for a {ms} ms budget ({ratio:.1f}x); reply: {line}")


def check_clock(binary: Path) -> None:
    """UCI requires wtime/btime support; without it no tournament can be played."""
    e = Engine(binary)
    e.send("uci", "isready", "position startpos", "go wtime 5000 btime 5000 winc 0 binc 0")
    ok, el, line = e.expect("bestmove", 10)
    e.close()
    record("go wtime/btime (clock time control)", ok,
           f"replied in {el:.2f} s: {line}" if ok else
           "NO bestmove within 10 s -- the clock is ignored, so the engine forfeits any timed game")


def check_stop(binary: Path) -> None:
    """UCI requires that `stop` yields a bestmove promptly."""
    e = Engine(binary)
    e.send("uci", "isready", "position startpos", "go infinite")
    time.sleep(0.4)
    e.send("stop")
    ok, el, line = e.expect("bestmove", 6)
    e.close()
    record("go infinite + stop", ok,
           f"stopped in {el:.2f} s: {line}" if ok else
           "NO bestmove within 6 s -- search is synchronous, so no GUI can interrupt it")


def check_mate_score(binary: Path) -> None:
    """UCI requires `score mate N` for forced mates, not a large `score cp`."""
    e = Engine(binary)
    e.send("uci", "isready",
           "position fen 6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1", "go depth 3")
    ok, _, _ = e.expect("bestmove", 60)
    info = [l for l in e.lines if l.startswith("info")]
    e.close()
    mate = any("score mate" in l for l in info)
    record("mate reported as 'score mate N'", mate,
           f"info lines: {info or '(none)'}" +
           ("" if mate else "  -- a GUI renders 'score cp 29999' as +299.99 pawns"))


def check_pv(binary: Path) -> None:
    """Every GUI and every debugging workflow needs a principal variation."""
    e = Engine(binary)
    e.send("uci", "isready", "position startpos", "go depth 3")
    e.expect("bestmove", 60)
    info = [l for l in e.lines if l.startswith("info")]
    e.close()
    has_pv = any(" pv " in l for l in info)
    record("info lines carry a 'pv'", has_pv,
           f"{len(info)} info line(s) emitted, none with a pv" if not has_pv else "pv present")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=DEFAULT_BIN)
    args = parser.parse_args()
    if not args.binary.exists():
        raise SystemExit(f"engine binary not found: {args.binary}\nrun: cargo build --release")

    print(f"UCI conformance probe -- {args.binary}\n")
    for fn in (check_handshake, check_movetime, check_clock,
               check_stop, check_mate_score, check_pv):
        fn(args.binary)
        print()

    failed = sum(1 for _, ok, _ in RESULTS if not ok)
    print("=" * 72)
    print(f"{len(RESULTS) - failed}/{len(RESULTS)} checks passed, {failed} failed")
    return failed


if __name__ == "__main__":
    raise SystemExit(main())
