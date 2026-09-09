"""Perft and integrity checks for the move generator."""

from __future__ import annotations

from .board import Board
from .move import uci


def perft(board: Board, depth: int) -> int:
    if depth == 0:
        return 1
    total = 0
    for move in board.legal_moves():
        board.make_move(move)
        total += perft(board, depth - 1)
        board.undo_move()
    return total


def divide(board: Board, depth: int) -> dict[str, int]:
    result: dict[str, int] = {}
    for move in board.legal_moves():
        board.make_move(move)
        result[uci(move)] = perft(board, depth - 1)
        board.undo_move()
    return dict(sorted(result.items()))


KNOWN_PERFT = {
    "startpos": [1, 20, 400, 8_902, 197_281, 4_865_609],
    "kiwipete": [1, 48, 2_039, 97_862, 4_085_603],
    "position3": [1, 14, 191, 2_812, 43_238, 674_624],
    "position4": [1, 6, 264, 9_467, 422_333, 15_833_292],
}

PERFT_FENS = {
    "startpos": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "kiwipete": "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "position3": "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "position4": "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
}


def run_known(depth: int = 4) -> list[tuple[str, int, int, bool]]:
    results = []
    for name, fen in PERFT_FENS.items():
        board = Board(fen)
        expected = KNOWN_PERFT[name][depth]
        actual = perft(board, depth)
        results.append((name, expected, actual, expected == actual))
    return results