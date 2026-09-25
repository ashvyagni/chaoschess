#!/usr/bin/env python3
"""Reproducible engine benchmark. Writes a timestamped JSON record so that any
later change is a diff against measured data rather than a recollection.

Deliberately uses FIXED DEPTH and FIXED NODE budgets, never wall-clock budgets,
for anything meant to be comparable across machines or runs.

Because the audited baseline needs 182 s for depth 4 from the start position
(see MASTER_ENGINE_AUDIT.md section F.1), --max-depth defaults to 3. Raise it once
the quiescence defect is fixed.
"""

from __future__ import annotations

import argparse
import json
import platform
import resource
import subprocess
import sys
import time
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BIN = ROOT / "target" / "release" / "crazy-chess"

# Fixed benchmark set. Kept small and stable so numbers stay comparable over time.
SUITE: list[tuple[str, str]] = [
    ("startpos", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
    ("kiwipete", "r3k2r/p1ppqpb1/bn2pnp1/2pP4/1p2P3/2N2N2/PPPQBPPP/R3K2R w KQkq - 0 1"),
    ("open-middlegame", "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1"),
    ("rook-endgame", "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"),
    ("pawn-endgame", "8/8/4k3/8/8/4K3/4P3/8 w - - 0 1"),
]


def git_commit() -> str:
    """HEAD, suffixed with '+dirty' when engine sources differ from it.

    A record that says "commit X" must mean the numbers came from commit X's code;
    otherwise the before/after trail is quietly wrong.
    """
    try:
        head = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT,
                              capture_output=True, text=True, check=True).stdout.strip()
        dirty = subprocess.run(["git", "status", "--porcelain", "--", "src", "Cargo.toml"],
                               cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()
        return f"{head}+dirty" if dirty else head
    except Exception:
        return "unknown"


def run_bench(binary: Path, fen: str, depth: int, style: str, threads: int,
              hash_mb: int, timeout: float) -> dict:
    """One `bench` invocation. Returns measured nodes/time, or a timeout marker."""
    script = (f"setoption name Style value {style}\n"
              f"setoption name Threads value {threads}\n"
              f"setoption name Hash value {hash_mb}\n"
              f"position fen {fen}\nbench {depth}\nquit\n")
    before = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    start = time.perf_counter()
    try:
        out = subprocess.run([str(binary)], input=script, capture_output=True,
                             text=True, timeout=timeout).stdout
    except subprocess.TimeoutExpired:
        return {"depth": depth, "timed_out_after_s": timeout}
    elapsed = time.perf_counter() - start
    peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    line = next((l for l in out.splitlines() if l.startswith("bench")), "")
    if not line:
        return {"depth": depth, "error": "no bench line", "raw": out[:200]}
    f = line.split()
    nodes, score, best = int(f[4]), int(f[7]), f[9]
    return {
        "depth": depth, "nodes": nodes, "seconds": round(elapsed, 4),
        "nps": int(nodes / elapsed) if elapsed > 0 else None,
        "score_cp": score, "best_move": best,
        "child_peak_rss_mb": round(max(peak - before, peak) / (1024 * 1024), 1),
    }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--binary", type=Path, default=DEFAULT_BIN)
    p.add_argument("--max-depth", type=int, default=3)
    p.add_argument("--style", default="Classical", choices=["Classical", "Chaos"])
    p.add_argument("--threads", type=int, default=1)
    p.add_argument("--hash", type=int, default=16, dest="hash_mb")
    p.add_argument("--timeout", type=float, default=60.0,
                   help="per-position-per-depth ceiling in seconds")
    p.add_argument("--label", default="baseline")
    p.add_argument("--out", type=Path, default=None)
    args = p.parse_args()

    if not args.binary.exists():
        raise SystemExit(f"engine binary not found: {args.binary}\nrun: cargo build --release")

    record = {
        "label": args.label,
        "date": date.today().isoformat(),
        "commit": git_commit(),
        "config": {"style": args.style, "threads": args.threads,
                   "hash_mb": args.hash_mb, "max_depth": args.max_depth,
                   "timeout_s": args.timeout},
        "machine": {"platform": platform.platform(), "machine": platform.machine(),
                    "python": platform.python_version()},
        "positions": {},
    }

    print(f"benchmark '{args.label}'  commit={record['commit']}  "
          f"style={args.style} threads={args.threads} hash={args.hash_mb}MB\n")
    hdr = f"{'position':18} {'d':>2} {'nodes':>14} {'time':>9} {'nps':>11} {'cp':>7}  best"
    print(hdr); print("-" * len(hdr))

    for name, fen in SUITE:
        record["positions"][name] = {"fen": fen, "depths": []}
        for depth in range(1, args.max_depth + 1):
            r = run_bench(args.binary, fen, depth, args.style,
                          args.threads, args.hash_mb, args.timeout)
            record["positions"][name]["depths"].append(r)
            if "timed_out_after_s" in r:
                print(f"{name:18} {depth:>2} {'TIMEOUT':>14} {r['timed_out_after_s']:>8.0f}s")
                break                                   # deeper will only be slower
            if "error" in r:
                print(f"{name:18} {depth:>2}   ERROR {r['error']}")
                break
            print(f"{name:18} {depth:>2} {r['nodes']:>14,} {r['seconds']:>8.3f}s "
                  f"{r['nps'] or 0:>11,} {r['score_cp']:>7}  {r['best_move']}")

    out = args.out or ROOT / "benchmarks" / f"{args.label}-{record['date']}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(record, indent=2) + "\n")
    print(f"\nwrote {out.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
