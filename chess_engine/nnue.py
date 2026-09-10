"""Small, deterministic NNUE-style evaluator with incremental accumulators.

The network keeps the requested 768 -> 256 -> 64 -> 1 shape.  The default
weights are intentionally compact and deterministic so the engine runs with
no model dependency.  Trained checkpoints use the same binary format and can
be swapped without touching search.
"""

from __future__ import annotations

import math
import os
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

from .constants import (
    BLACK,
    BISHOP,
    KING,
    KNIGHT,
    PAWN,
    PIECE_VALUES,
    QUEEN,
    ROOK,
    WHITE,
    color_of,
    file_of,
    rank_of,
    type_of,
)

INPUTS = 12 * 64
HIDDEN = 256
OUTPUT_HIDDEN = 64
MAGIC = b"PYNNUE01"
MODEL_VERSION = 1


def _seeded_values(count: int, seed: int, scale: int) -> list[int]:
    value = seed & 0xFFFFFFFF
    result: list[int] = []
    for _ in range(count):
        value = (1664525 * value + 1013904223) & 0xFFFFFFFF
        result.append(((value >> 24) % (2 * scale + 1)) - scale)
    return result


@dataclass
class NNUE:
    """A CPU-friendly sparse-feature network.

    Only input->hidden weights are needed during incremental updates.  The
    later layers are evaluated after clipped ReLU, matching the usual NNUE
    accumulator workflow.
    """

    input_weights: list[int]
    hidden_weights: list[int]
    hidden_bias: list[int]
    output_weights: list[int]
    output_bias: int = 0
    name: str = "builtin"

    @classmethod
    def builtin(cls) -> "NNUE":
        return cls(
            _seeded_values(INPUTS * HIDDEN, 0x9E3779B9, 4),
            _seeded_values(HIDDEN * OUTPUT_HIDDEN, 0x243F6A88, 3),
            [0] * HIDDEN,
            _seeded_values(OUTPUT_HIDDEN, 0xB7E15162, 12),
            0,
            "builtin",
        )

    @classmethod
    def load(cls, path: str | os.PathLike[str]) -> "NNUE":
        with open(path, "rb") as handle:
            magic, version, name_len = struct.unpack("<8sII", handle.read(16))
            if magic != MAGIC or version != MODEL_VERSION:
                raise ValueError(f"Unsupported NNUE model: {path}")
            name = handle.read(name_len).decode("utf8")
            lengths = struct.unpack("<5I", handle.read(20))
            expected = (INPUTS * HIDDEN, HIDDEN * OUTPUT_HIDDEN, HIDDEN, OUTPUT_HIDDEN, 1)
            if lengths != expected:
                raise ValueError("NNUE dimensions do not match this engine")

            def read_i16(count: int) -> list[int]:
                return list(struct.unpack(f"<{count}h", handle.read(count * 2)))

            input_weights = read_i16(lengths[0])
            hidden_weights = read_i16(lengths[1])
            hidden_bias = read_i16(lengths[2])
            output_weights = read_i16(lengths[3])
            output_bias = struct.unpack("<i", handle.read(4))[0]
        return cls(input_weights, hidden_weights, hidden_bias, output_weights, output_bias, name)

    def save(self, path: str | os.PathLike[str], name: str | None = None) -> None:
        target = Path(path)
        target.parent.mkdir(parents=True, exist_ok=True)
        model_name = (name or self.name).encode("utf8")
        with target.open("wb") as handle:
            handle.write(struct.pack("<8sII", MAGIC, MODEL_VERSION, len(model_name)))
            handle.write(model_name)
            lengths = (len(self.input_weights), len(self.hidden_weights), len(self.hidden_bias), len(self.output_weights), 1)
            handle.write(struct.pack("<5I", *lengths))
            handle.write(struct.pack(f"<{len(self.input_weights)}h", *self.input_weights))
            handle.write(struct.pack(f"<{len(self.hidden_weights)}h", *self.hidden_weights))
            handle.write(struct.pack(f"<{len(self.hidden_bias)}h", *self.hidden_bias))
            handle.write(struct.pack(f"<{len(self.output_weights)}h", *self.output_weights))
            handle.write(struct.pack("<i", self.output_bias))

    def feature_index(self, piece: int, sq: int) -> int:
        return (piece - 1) * 64 + sq

    def initial_accumulator(self, board: object) -> list[int]:
        accumulator = self.hidden_bias.copy()
        for sq, piece in enumerate(board.board):
            if piece:
                self.apply_feature(accumulator, piece, sq, 1)
        return accumulator

    def apply_feature(self, accumulator: list[int], piece: int, sq: int, sign: int) -> None:
        offset = self.feature_index(piece, sq) * HIDDEN
        for hidden in range(HIDDEN):
            accumulator[hidden] += sign * self.input_weights[offset + hidden]

    @staticmethod
    def _crelu(value: int) -> int:
        return max(0, min(127, value))

    def evaluate_white(self, board: object) -> int:
        # Accumulator is incrementally maintained by Board.make_move/undo_move.
        hidden = [self._crelu(value) for value in board.accumulator]
        result = self.output_bias
        for out in range(OUTPUT_HIDDEN):
            base = out * HIDDEN
            total = 0
            for index, value in enumerate(hidden):
                total += value * self.hidden_weights[base + index]
            result += self._crelu(total // 32 + self.output_weights[out])
        # A transparent classical prior keeps an untrained checkpoint useful.
        # It is deliberately outside the accumulator so trained weights and
        # make/undo verification retain the same binary format.
        material = board.material()
        positional = 0
        pawn_files = [[0] * 8 for _ in (WHITE, BLACK)]
        for sq, piece in enumerate(board.board):
            if not piece:
                continue
            color = color_of(piece)
            piece_type = type_of(piece)
            sign = 1 if color == WHITE else -1
            relative_rank = rank_of(sq) if color == WHITE else 7 - rank_of(sq)
            center_distance = abs(3.5 - file_of(sq)) + abs(3.5 - rank_of(sq))
            center = max(0, int(7 - center_distance))
            if piece_type == PAWN:
                positional += sign * (relative_rank * 7 + center * 2)
                pawn_files[color][file_of(sq)] += 1
            elif piece_type == KNIGHT:
                positional += sign * (center * 9 + relative_rank * 2)
            elif piece_type == BISHOP:
                positional += sign * (center * 4 + relative_rank * 2)
            elif piece_type == ROOK:
                positional += sign * (relative_rank * 3 + center * 2)
            elif piece_type == QUEEN:
                positional += sign * center * 2
            elif piece_type == KING:
                # Prefer a sheltered king early and activity in the endgame
                # without allowing the small term to outweigh material.
                positional += sign * (-center * 3 if relative_rank < 2 else center * 3)

        for color, sign in ((WHITE, 1), (BLACK, -1)):
            for file_count in pawn_files[color]:
                if file_count > 1:
                    positional -= sign * (file_count - 1) * 10
            if sum(1 for count in pawn_files[color] if count) >= 2:
                positional += sign * 30  # bishop-pair-like pawn-chain stability prior
            bishops = board.bitboards[3 + 6 * color].bit_count()
            if bishops >= 2:
                positional += sign * 30
        return int(result // 8 + material + positional)

    def evaluate(self, board: object) -> int:
        score = self.evaluate_white(board)
        return score if board.side == WHITE else -score

    def full_evaluate_white(self, board: object) -> int:
        original = board.accumulator
        board.accumulator = self.initial_accumulator(board)
        try:
            return self.evaluate_white(board)
        finally:
            board.accumulator = original

    def verify_accumulator(self, board: object) -> bool:
        return self.evaluate_white(board) == self.full_evaluate_white(board)


default_network = NNUE.builtin()


def load_network(path: str | None) -> NNUE:
    return NNUE.load(path) if path else default_network