"""Iterative-deepening negamax with TT, PVS, quiescence, and practical pruning."""

from __future__ import annotations

import threading
import time
from dataclasses import dataclass, field
from typing import Callable

from .board import Board
from .constants import KING, PIECE_VALUES, color_of, type_of
from .move import FLAG_CAPTURE, FLAG_EP, from_sq, is_capture, promotion, to_sq, uci
from .nnue import NNUE, default_network

INF = 1_000_000
MATE = 100_000

EXACT, LOWER, UPPER = 0, 1, 2


@dataclass(slots=True)
class TTEntry:
    key: int
    depth: int
    score: int
    bound: int
    best_move: int


class TranspositionTable:
    def __init__(self, megabytes: int = 32) -> None:
        self.max_entries = max(1_024, megabytes * 1024 * 1024 // 64)
        self.table: dict[int, TTEntry] = {}

    def clear(self) -> None:
        self.table.clear()

    def probe(self, key: int) -> TTEntry | None:
        return self.table.get(key)

    def store(self, entry: TTEntry) -> None:
        previous = self.table.get(entry.key)
        if previous is None or entry.depth >= previous.depth:
            if len(self.table) >= self.max_entries and entry.key not in self.table:
                self.table.pop(next(iter(self.table)))
            self.table[entry.key] = entry

    def hashfull(self) -> int:
        return min(1000, len(self.table) * 1000 // self.max_entries)


@dataclass(slots=True)
class SearchLimits:
    depth: int = 64
    movetime: int | None = None
    wtime: int | None = None
    btime: int | None = None
    winc: int = 0
    binc: int = 0


@dataclass(slots=True)
class SearchResult:
    best_move: int
    score: int
    depth: int
    seldepth: int
    nodes: int
    time_ms: int
    pv: list[int] = field(default_factory=list)

    @property
    def nps(self) -> int:
        return self.nodes * 1000 // max(1, self.time_ms)


class Search:
    def __init__(
        self,
        network: NNUE | None = None,
        hash_mb: int = 32,
        info_callback: Callable[[SearchResult], None] | None = None,
    ) -> None:
        self.network = network or default_network
        self.tt = TranspositionTable(hash_mb)
        self.info_callback = info_callback
        self.stop_event = threading.Event()
        self.deadline = float("inf")
        self.nodes = 0
        self.seldepth = 0
        self.killers: list[list[int]] = [[0, 0] for _ in range(128)]
        self.history = [[0] * 64 for _ in range(13)]
        self.root_pv: list[int] = []

    def stop(self) -> None:
        self.stop_event.set()

    def search(self, board: Board, limits: SearchLimits) -> SearchResult:
        self.stop_event.clear()
        self.nodes = 0
        self.seldepth = 0
        self.root_pv = []
        board.network = self.network
        board.accumulator = self.network.initial_accumulator(board)
        start = time.monotonic()
        budget = limits.movetime
        if budget is None:
            remaining = limits.wtime if board.side == 0 else limits.btime
            increment = limits.winc if board.side == 0 else limits.binc
            if remaining is not None:
                budget = max(10, remaining // 25 + increment // 2)
        self.deadline = start + budget / 1000 if budget is not None else float("inf")
        root_moves = board.legal_moves()
        if not root_moves:
            return SearchResult(0, 0, 0, 0, self.nodes, 0, [])
        best_move, best_score = root_moves[0], 0
        completed_depth = 0
        best_pv = [best_move]
        for depth in range(1, max(1, limits.depth) + 1):
            if self.should_stop():
                break
            if completed_depth >= 2:
                window = 50
                alpha = max(-INF, best_score - window)
                beta = min(INF, best_score + window)
                score, pv = self._root(board, depth, root_moves, alpha, beta)
                if not self.should_stop() and (score <= alpha or score >= beta):
                    score, pv = self._root(board, depth, root_moves, -INF, INF)
            else:
                score, pv = self._root(board, depth, root_moves, -INF, INF)
            if self.should_stop() and completed_depth:
                break
            if pv:
                best_move, best_score, best_pv = pv[0], score, pv
                root_moves = pv + [move for move in root_moves if move not in pv]
            completed_depth = depth
            result = SearchResult(
                best_move, best_score, completed_depth, self.seldepth, self.nodes,
                int((time.monotonic() - start) * 1000), best_pv,
            )
            if self.info_callback:
                self.info_callback(result)
        elapsed = int((time.monotonic() - start) * 1000)
        return SearchResult(best_move, best_score, completed_depth, self.seldepth, self.nodes, elapsed, best_pv)

    def should_stop(self) -> bool:
        return self.stop_event.is_set() or time.monotonic() >= self.deadline

    def _root(self, board: Board, depth: int, root_moves: list[int], alpha: int, beta: int) -> tuple[int, list[int]]:
        best_score, best_pv = -INF, []
        for index, move in enumerate(root_moves):
            if self.should_stop():
                break
            board.make_move(move)
            score, child_pv = self._negamax(board, depth - 1, -beta, -alpha, 1)
            score = -score
            board.undo_move()
            if score > best_score:
                best_score, best_pv = score, [move] + child_pv
            if score > alpha:
                alpha = score
            if alpha >= beta:
                break
        return best_score, best_pv

    def _negamax(self, board: Board, depth: int, alpha: int, beta: int, ply: int) -> tuple[int, list[int]]:
        self.nodes += 1
        self.seldepth = max(self.seldepth, ply)
        if self.should_stop():
            return 0, []
        if board.is_checkmate():
            return -MATE + ply, []
        if board.is_stalemate() or board.halfmove >= 100:
            return 0, []
        if depth <= 0:
            return self._quiescence(board, alpha, beta, ply)

        original_alpha = alpha
        entry = self.tt.probe(board.key)
        tt_move = 0
        if entry:
            tt_move = entry.best_move
            if entry.depth >= depth:
                entry_score = self._score_from_tt(entry.score, ply)
                if entry.bound == EXACT:
                    return entry_score, [entry.best_move] if entry.best_move else []
                if entry.bound == LOWER:
                    alpha = max(alpha, entry_score)
                elif entry.bound == UPPER:
                    beta = min(beta, entry_score)
                if alpha >= beta:
                    return entry_score, [entry.best_move] if entry.best_move else []

        in_check = board.in_check()
        # Null-move pruning is deliberately guarded against endgames and check.
        if depth >= 3 and not in_check and self._has_non_pawn_material(board, board.side):
            board.side = 1 - board.side
            board.key = board.zobrist()
            null_score, _ = self._negamax(board, depth - 3, -beta, -beta + 1, ply + 1)
            board.side = 1 - board.side
            board.key = board.zobrist()
            if -null_score >= beta:
                return beta, []

        moves = board.legal_moves()
        if not moves:
            return (-MATE + ply if in_check else 0), []
        moves.sort(key=lambda move: self._move_score(board, move, tt_move, ply), reverse=True)
        best_score, best_move, best_pv = -INF, 0, []
        quiet_index = 0
        for move_index, move in enumerate(moves):
            if self.should_stop():
                break
            quiet = not is_capture(move) and not promotion(move)
            board.make_move(move)
            extension = 1 if board.in_check() and depth >= 2 else 0
            reduction = 1 if depth >= 3 and move_index >= 4 and quiet and not in_check else 0
            if move_index == 0:
                score, child_pv = self._negamax(board, depth - 1 + extension, -beta, -alpha, ply + 1)
                score = -score
            else:
                score, child_pv = self._negamax(board, depth - 1 - reduction + extension, -alpha - 1, -alpha, ply + 1)
                score = -score
                if score > alpha and score < beta:
                    score, child_pv = self._negamax(board, depth - 1 + extension, -beta, -alpha, ply + 1)
                    score = -score
            board.undo_move()
            if quiet:
                quiet_index += 1
            if score > best_score:
                best_score, best_move, best_pv = score, move, [move] + child_pv
            if score > alpha:
                alpha = score
            if alpha >= beta:
                if quiet:
                    self._record_killer(move, ply)
                    self.history[type_of(board.board[from_sq(move)])][to_sq(move)] += depth * depth
                break
        bound = UPPER if best_score <= original_alpha else LOWER if best_score >= beta else EXACT
        self.tt.store(TTEntry(board.key, depth, self._score_to_tt(best_score, ply), bound, best_move))
        return best_score, best_pv

    def _quiescence(self, board: Board, alpha: int, beta: int, ply: int) -> tuple[int, list[int]]:
        self.nodes += 1
        self.seldepth = max(self.seldepth, ply)
        in_check = board.in_check()
        if ply >= 64:
            return self.network.evaluate(board), []
        if not in_check:
            stand_pat = self.network.evaluate(board)
            if stand_pat >= beta:
                return beta, []
            if stand_pat > alpha:
                alpha = stand_pat
            moves = board.legal_moves(captures_only=True)
        else:
            # A checked position has no legal stand-pat score. Search every
            # evasion, including quiet king moves and interpositions.
            moves = board.legal_moves()
            if not moves:
                return -MATE + ply, []
        best_pv: list[int] = []
        moves.sort(key=lambda move: self._move_score(board, move, 0, ply), reverse=True)
        for move in moves:
            if self.should_stop():
                break
            board.make_move(move)
            score, child_pv = self._quiescence(board, -beta, -alpha, ply + 1)
            score = -score
            board.undo_move()
            if score >= beta:
                return beta, [move] + child_pv
            if score > alpha:
                alpha, best_pv = score, [move] + child_pv
        return alpha, best_pv

    @staticmethod
    def _score_to_tt(score: int, ply: int) -> int:
        if score > MATE - 1_000:
            return score + ply
        if score < -MATE + 1_000:
            return score - ply
        return score

    @staticmethod
    def _score_from_tt(score: int, ply: int) -> int:
        if score > MATE - 1_000:
            return score - ply
        if score < -MATE + 1_000:
            return score + ply
        return score

    def _move_score(self, board: Board, move: int, tt_move: int, ply: int) -> int:
        if move == tt_move:
            return 10_000_000
        source, target = from_sq(move), to_sq(move)
        moving = board.board[source]
        captured = board.board[target]
        if is_capture(move):
            captured_type = type_of(captured) if captured else 1
            return 1_000_000 + PIECE_VALUES[captured_type] * 10 - PIECE_VALUES[type_of(moving)]
        if move == self.killers[ply][0]:
            return 900_000
        if move == self.killers[ply][1]:
            return 800_000
        return self.history[moving][target]

    def _record_killer(self, move: int, ply: int) -> None:
        if self.killers[ply][0] != move:
            self.killers[ply][1] = self.killers[ply][0]
            self.killers[ply][0] = move

    @staticmethod
    def _has_non_pawn_material(board: Board, color: int) -> bool:
        return any(board.bitboards[color * 6 + piece] for piece in (2, 3, 4, 5))


def format_info(result: SearchResult, board: Board, hashfull: int) -> str:
    score = f"mate {(MATE - abs(result.score) + 1) // 2}" if abs(result.score) > MATE - 1000 else f"cp {result.score}"
    pv = " ".join(uci(move) for move in result.pv)
    return (
        f"info depth {result.depth} seldepth {result.seldepth} score {score} "
        f"nodes {result.nodes} nps {result.nps} time {result.time_ms} hashfull {hashfull} pv {pv}"
    )