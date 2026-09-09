"""Small command-line search benchmark."""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from chess_engine.board import Board
from chess_engine.move import uci
from chess_engine.search import Search, SearchLimits


def main() -> None:
    parser = argparse.ArgumentParser(description="Benchmark the Python NNUE Alpha engine")
    parser.add_argument("--depth", type=int, default=5)
    parser.add_argument("--model", default="")
    args = parser.parse_args()
    positions = [
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ]
    total_nodes = 0
    started = time.monotonic()
    for fen in positions:
        board = Board(fen)
        search = Search()
        result = search.search(board, SearchLimits(depth=args.depth))
        total_nodes += result.nodes
        print(f"depth {result.depth} score {result.score} nodes {result.nodes} nps {result.nps} pv {' '.join(uci(m) for m in result.pv)}")
    elapsed = max(0.001, time.monotonic() - started)
    print(f"benchmark nodes {total_nodes} nps {int(total_nodes / elapsed)} time_ms {int(elapsed * 1000)}")


if __name__ == "__main__":
    main()