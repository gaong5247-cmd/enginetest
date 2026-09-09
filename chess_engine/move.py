"""Compact move representation used by the board and search."""

from dataclasses import dataclass

from .constants import (
    BISHOP,
    KNIGHT,
    QUEEN,
    ROOK,
    file_of,
    rank_of,
)

FLAG_CAPTURE = 1
FLAG_EP = 2
FLAG_CASTLE = 4
FLAG_DOUBLE_PAWN = 8
FLAG_PROMOTION = 16

PROMOTION_TO_CHAR = {KNIGHT: "n", BISHOP: "b", ROOK: "r", QUEEN: "q"}
CHAR_TO_PROMOTION = {v: k for k, v in PROMOTION_TO_CHAR.items()}


def encode(from_sq: int, to_sq: int, promotion: int = 0, flags: int = 0) -> int:
    return from_sq | (to_sq << 6) | (promotion << 12) | (flags << 16)


def from_sq(move: int) -> int:
    return move & 63


def to_sq(move: int) -> int:
    return (move >> 6) & 63


def promotion(move: int) -> int:
    return (move >> 12) & 7


def flags(move: int) -> int:
    return move >> 16


def is_capture(move: int) -> bool:
    return bool(flags(move) & FLAG_CAPTURE)


def is_quiet(move: int) -> bool:
    return not flags(move) & (FLAG_CAPTURE | FLAG_EP | FLAG_CASTLE | FLAG_PROMOTION)


def uci(move: int) -> str:
    text = f"{chr(97 + file_of(from_sq(move)))}{rank_of(from_sq(move)) + 1}"
    text += f"{chr(97 + file_of(to_sq(move)))}{rank_of(to_sq(move)) + 1}"
    if promotion(move):
        text += PROMOTION_TO_CHAR[promotion(move)]
    return text


@dataclass(frozen=True, slots=True)
class Move:
    """Public move object for tools and training code."""

    from_square: int
    to_square: int
    promotion: int = 0
    flags: int = 0

    @classmethod
    def from_int(cls, move: int) -> "Move":
        return cls(from_sq(move), to_sq(move), promotion(move), flags(move))

    def to_int(self) -> int:
        return encode(self.from_square, self.to_square, self.promotion, self.flags)

    def uci(self) -> str:
        return uci(self.to_int())

    def __str__(self) -> str:
        return self.uci()