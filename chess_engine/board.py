"""Bitboard-backed legal chess board with reversible moves."""

from __future__ import annotations

import random
from dataclasses import dataclass
from typing import Iterator

from .constants import (
    BK,
    BQ,
    BISHOP,
    BLACK,
    FULL_BOARD,
    KING,
    KING_ATTACKS,
    KNIGHT,
    KNIGHT_ATTACKS,
    PAWN,
    PAWN_ATTACKS,
    PIECE_NAMES,
    PIECE_VALUES,
    QUEEN,
    ROOK,
    WK,
    WQ,
    WHITE,
    bishop_attacks,
    bit,
    color_of,
    file_of,
    make_piece,
    pop_lsb,
    queen_attacks,
    rank_of,
    rook_attacks,
    type_of,
)
from .move import (
    FLAG_CAPTURE,
    FLAG_CASTLE,
    FLAG_DOUBLE_PAWN,
    FLAG_EP,
    FLAG_PROMOTION,
    encode,
    flags,
    from_sq,
    promotion,
    to_sq,
    uci,
)

START_FEN = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
ZOBRIST = random.Random(0xC0FFEE).getrandbits
Z_PIECE = [[ZOBRIST(64) for _ in range(64)] for _ in range(13)]
Z_CASTLING = [ZOBRIST(64) for _ in range(16)]
Z_EP = [ZOBRIST(64) for _ in range(65)]
Z_SIDE = ZOBRIST(64)


@dataclass(slots=True)
class _State:
    board: list[int]
    bitboards: list[int]
    occupancies: list[int]
    side: int
    castling: int
    ep_square: int
    halfmove: int
    fullmove: int
    key: int
    accumulator: list[int]


class Board:
    """A complete chess position. Squares are A1=0 through H8=63."""

    def __init__(self, fen: str = START_FEN) -> None:
        from .nnue import default_network

        self.network = default_network
        self.board = [0] * 64
        self.bitboards = [0] * 13
        self.occupancies = [0, 0, 0]
        self.side = WHITE
        self.castling = 0
        self.ep_square = -1
        self.halfmove = 0
        self.fullmove = 1
        self.key = 0
        self._history: list[_State] = []
        self.accumulator: list[int] = []
        self.set_fen(fen)

    def copy(self) -> "Board":
        result = Board()
        result.network = self.network
        result._restore(self._snapshot())
        return result

    def _snapshot(self) -> _State:
        return _State(
            self.board.copy(),
            self.bitboards.copy(),
            self.occupancies.copy(),
            self.side,
            self.castling,
            self.ep_square,
            self.halfmove,
            self.fullmove,
            self.key,
            self.accumulator.copy(),
        )

    def _restore(self, state: _State) -> None:
        self.board = state.board
        self.bitboards = state.bitboards
        self.occupancies = state.occupancies
        self.side = state.side
        self.castling = state.castling
        self.ep_square = state.ep_square
        self.halfmove = state.halfmove
        self.fullmove = state.fullmove
        self.key = state.key
        self.accumulator = state.accumulator

    def set_fen(self, fen: str) -> None:
        fields = fen.split()
        if len(fields) != 6:
            raise ValueError(f"Invalid FEN: {fen}")
        self.board = [0] * 64
        rank = 7
        file = 0
        for char in fields[0]:
            if char == "/":
                rank -= 1
                file = 0
            elif char.isdigit():
                file += int(char)
            else:
                if not 0 <= rank < 8 or not 0 <= file < 8:
                    raise ValueError(f"Invalid placement: {fen}")
                piece = PIECE_NAMES.index(char)
                self.board[rank * 8 + file] = piece
                file += 1
        self.side = WHITE if fields[1] == "w" else BLACK
        self.castling = 0 if fields[2] == "-" else sum(
            { "K": WK, "Q": WQ, "k": BK, "q": BQ }[char] for char in fields[2]
        )
        self.ep_square = -1 if fields[3] == "-" else self.parse_square(fields[3])
        self.halfmove = int(fields[4])
        self.fullmove = int(fields[5])
        self._history.clear()
        self._rebuild()

    def _rebuild(self) -> None:
        self.bitboards = [0] * 13
        for sq, piece in enumerate(self.board):
            if piece:
                self.bitboards[piece] |= bit(sq)
        self.occupancies = [
            sum(self.bitboards[p] for p in range(1, 7)),
            sum(self.bitboards[p] for p in range(7, 13)),
            sum(self.bitboards[p] for p in range(1, 13)),
        ]
        self.key = self.zobrist()
        self.accumulator = [0] * 256
        self._full_accumulator_update()

    def _full_accumulator_update(self) -> None:
        # The NNUE module owns the weights. Import lazily to keep Board lightweight.
        self.accumulator = self.network.initial_accumulator(self)

    @staticmethod
    def parse_square(text: str) -> int:
        if len(text) != 2 or text[0] not in "abcdefgh" or text[1] not in "12345678":
            raise ValueError(f"Invalid square: {text}")
        return (ord(text[1]) - ord("1")) * 8 + ord(text[0]) - ord("a")

    @staticmethod
    def square_name(sq: int) -> str:
        return f"{chr(97 + file_of(sq))}{rank_of(sq) + 1}"

    def fen(self) -> str:
        ranks: list[str] = []
        for rank in range(7, -1, -1):
            empty = 0
            text = ""
            for file in range(8):
                piece = self.board[rank * 8 + file]
                if piece:
                    if empty:
                        text += str(empty)
                        empty = 0
                    text += PIECE_NAMES[piece]
                else:
                    empty += 1
            if empty:
                text += str(empty)
            ranks.append(text)
        castling = "".join(
            char for char, flag in (("K", WK), ("Q", WQ), ("k", BK), ("q", BQ)) if self.castling & flag
        ) or "-"
        ep = "-" if self.ep_square < 0 else self.square_name(self.ep_square)
        return f"{'/'.join(ranks)} {'w' if self.side == WHITE else 'b'} {castling} {ep} {self.halfmove} {self.fullmove}"

    def zobrist(self) -> int:
        key = Z_CASTLING[self.castling] ^ Z_EP[self.ep_square + 1]
        if self.side == BLACK:
            key ^= Z_SIDE
        for sq, piece in enumerate(self.board):
            if piece:
                key ^= Z_PIECE[piece][sq]
        return key

    def piece_at(self, sq: int) -> int:
        return self.board[sq]

    def king_square(self, color: int) -> int:
        kings = self.bitboards[make_piece(color, KING)]
        return (kings & -kings).bit_length() - 1 if kings else -1

    def is_square_attacked(self, sq: int, by_color: int) -> bool:
        pawn_attackers = PAWN_ATTACKS[1 - by_color][sq] & self.bitboards[make_piece(by_color, PAWN)]
        if pawn_attackers:
            return True
        if KNIGHT_ATTACKS[sq] & self.bitboards[make_piece(by_color, KNIGHT)]:
            return True
        if KING_ATTACKS[sq] & self.bitboards[make_piece(by_color, KING)]:
            return True
        occupancy = self.occupancies[2]
        if rook_attacks(sq, occupancy) & (
            self.bitboards[make_piece(by_color, ROOK)] | self.bitboards[make_piece(by_color, QUEEN)]
        ):
            return True
        return bool(
            bishop_attacks(sq, occupancy)
            & (self.bitboards[make_piece(by_color, BISHOP)] | self.bitboards[make_piece(by_color, QUEEN)])
        )

    def in_check(self, color: int | None = None) -> bool:
        color = self.side if color is None else color
        king = self.king_square(color)
        return king < 0 or self.is_square_attacked(king, 1 - color)

    def _add(self, moves: list[int], from_square: int, to_square: int, extra: int = 0) -> None:
        target = self.board[to_square]
        capture = FLAG_CAPTURE if target and color_of(target) != self.side else 0
        moves.append(encode(from_square, to_square, 0, extra | capture))

    def _add_pawn_move(self, moves: list[int], from_square: int, to_square: int, extra: int = 0) -> None:
        rank = rank_of(to_square)
        if rank in (0, 7):
            for piece_type in (QUEEN, ROOK, BISHOP, KNIGHT):
                target = self.board[to_square]
                capture = FLAG_CAPTURE if target else 0
                moves.append(encode(from_square, to_square, piece_type, extra | capture | FLAG_PROMOTION))
        else:
            target = self.board[to_square]
            capture = FLAG_CAPTURE if target else 0
            moves.append(encode(from_square, to_square, 0, extra | capture))

    def pseudo_legal_moves(self, captures_only: bool = False) -> Iterator[int]:
        own = self.occupancies[self.side]
        enemy = self.occupancies[1 - self.side]
        occupancy = self.occupancies[2]
        direction = 8 if self.side == WHITE else -8
        start_rank = 1 if self.side == WHITE else 6
        enemy_pawn = make_piece(1 - self.side, PAWN)
        for from_square in range(64):
            piece = self.board[from_square]
            if not piece or color_of(piece) != self.side:
                continue
            piece_type = type_of(piece)
            from_rank = rank_of(from_square)
            if piece_type == PAWN:
                one = from_square + direction
                if not captures_only and 0 <= one < 64 and not self.board[one]:
                    yield from self._pawn_moves(from_square, one)
                    two = from_square + direction * 2
                    if from_rank == start_rank and not self.board[two]:
                        yield encode(from_square, two, 0, FLAG_DOUBLE_PAWN)
                attacks = PAWN_ATTACKS[self.side][from_square] & enemy
                if self.ep_square >= 0:
                    attacks |= PAWN_ATTACKS[self.side][from_square] & bit(self.ep_square)
                while attacks:
                    to_square, attacks = pop_lsb(attacks)
                    if to_square == self.ep_square:
                        yield encode(from_square, to_square, 0, FLAG_CAPTURE | FLAG_EP)
                    else:
                        yield from self._pawn_moves(from_square, to_square, FLAG_CAPTURE)
            elif piece_type == KNIGHT:
                targets = KNIGHT_ATTACKS[from_square] & (FULL_BOARD ^ own)
                if captures_only:
                    targets &= enemy
                while targets:
                    to_square, targets = pop_lsb(targets)
                    self._add(moves := [], from_square, to_square)
                    yield moves[0]
            elif piece_type == BISHOP:
                targets = bishop_attacks(from_square, occupancy) & (FULL_BOARD ^ own)
                if captures_only:
                    targets &= enemy
                while targets:
                    to_square, targets = pop_lsb(targets)
                    self._add(moves := [], from_square, to_square)
                    yield moves[0]
            elif piece_type == ROOK:
                targets = rook_attacks(from_square, occupancy) & (FULL_BOARD ^ own)
                if captures_only:
                    targets &= enemy
                while targets:
                    to_square, targets = pop_lsb(targets)
                    self._add(moves := [], from_square, to_square)
                    yield moves[0]
            elif piece_type == QUEEN:
                targets = queen_attacks(from_square, occupancy) & (FULL_BOARD ^ own)
                if captures_only:
                    targets &= enemy
                while targets:
                    to_square, targets = pop_lsb(targets)
                    self._add(moves := [], from_square, to_square)
                    yield moves[0]
            elif piece_type == KING:
                targets = KING_ATTACKS[from_square] & (FULL_BOARD ^ own)
                if captures_only:
                    targets &= enemy
                while targets:
                    to_square, targets = pop_lsb(targets)
                    self._add(moves := [], from_square, to_square)
                    yield moves[0]
                if not captures_only:
                    yield from self._castling_moves()

    def _pawn_moves(self, from_square: int, to_square: int, extra: int = 0) -> Iterator[int]:
        rank = rank_of(to_square)
        if rank in (0, 7):
            for piece_type in (QUEEN, ROOK, BISHOP, KNIGHT):
                yield encode(from_square, to_square, piece_type, extra | FLAG_PROMOTION)
        else:
            yield encode(from_square, to_square, 0, extra)

    def _castling_moves(self) -> Iterator[int]:
        if self.side == WHITE:
            if self.castling & WK and self.board[5] == self.board[6] == 0 and self.board[7] == make_piece(WHITE, ROOK):
                if not self.in_check(WHITE) and not self.is_square_attacked(5, BLACK) and not self.is_square_attacked(6, BLACK):
                    yield encode(4, 6, 0, FLAG_CASTLE)
            if self.castling & WQ and self.board[1] == self.board[2] == self.board[3] == 0 and self.board[0] == make_piece(WHITE, ROOK):
                if not self.in_check(WHITE) and not self.is_square_attacked(3, BLACK) and not self.is_square_attacked(2, BLACK):
                    yield encode(4, 2, 0, FLAG_CASTLE)
        else:
            if self.castling & BK and self.board[61] == self.board[62] == 0 and self.board[63] == make_piece(BLACK, ROOK):
                if not self.in_check(BLACK) and not self.is_square_attacked(61, WHITE) and not self.is_square_attacked(62, WHITE):
                    yield encode(60, 62, 0, FLAG_CASTLE)
            if self.castling & BQ and self.board[57] == self.board[58] == self.board[59] == 0 and self.board[56] == make_piece(BLACK, ROOK):
                if not self.in_check(BLACK) and not self.is_square_attacked(59, WHITE) and not self.is_square_attacked(58, WHITE):
                    yield encode(60, 58, 0, FLAG_CASTLE)

    def legal_moves(self, captures_only: bool = False) -> list[int]:
        legal: list[int] = []
        for move in self.pseudo_legal_moves(captures_only):
            self.make_move(move)
            if not self.in_check(1 - self.side):
                legal.append(move)
            self.undo_move()
        return legal

    def find_move(self, text: str) -> int:
        for move in self.legal_moves():
            if uci(move) == text:
                return move
        raise ValueError(f"Illegal move {text} in {self.fen()}")

    def make_move(self, move: int) -> None:
        self._history.append(self._snapshot())
        source, target = from_sq(move), to_sq(move)
        moving = self.board[source]
        captured = self.board[target]
        move_flags = flags(move)
        old_board = self.board.copy()
        self.board[source] = 0
        if move_flags & FLAG_EP:
            captured_square = target - 8 if self.side == WHITE else target + 8
            captured = self.board[captured_square]
            self.board[captured_square] = 0
        self.board[target] = make_piece(self.side, promotion(move)) if promotion(move) else moving
        if move_flags & FLAG_CASTLE:
            if target > source:
                rook_from, rook_to = source + 3, source + 1
            else:
                rook_from, rook_to = source - 4, source - 1
            self.board[rook_to] = self.board[rook_from]
            self.board[rook_from] = 0
        self._update_castling(source, target, moving, captured)
        self.ep_square = source + (target - source) // 2 if move_flags & FLAG_DOUBLE_PAWN else -1
        self.halfmove = 0 if type_of(moving) == PAWN or captured else self.halfmove + 1
        if self.side == BLACK:
            self.fullmove += 1
        self.side = 1 - self.side
        self._rebuild_from_board(old_board)

    def _update_castling(self, source: int, target: int, moving: int, captured: int) -> None:
        if moving == make_piece(WHITE, KING):
            self.castling &= ~(WK | WQ)
        elif moving == make_piece(BLACK, KING):
            self.castling &= ~(BK | BQ)
        elif moving == make_piece(WHITE, ROOK):
            if source == 0:
                self.castling &= ~WQ
            elif source == 7:
                self.castling &= ~WK
        elif moving == make_piece(BLACK, ROOK):
            if source == 56:
                self.castling &= ~BQ
            elif source == 63:
                self.castling &= ~BK
        if captured == make_piece(WHITE, ROOK):
            if target == 0:
                self.castling &= ~WQ
            elif target == 7:
                self.castling &= ~WK
        elif captured == make_piece(BLACK, ROOK):
            if target == 56:
                self.castling &= ~BQ
            elif target == 63:
                self.castling &= ~BK

    def _rebuild_from_board(self, old_board: list[int]) -> None:
        self.bitboards = [0] * 13
        for sq, piece in enumerate(self.board):
            if piece:
                self.bitboards[piece] |= bit(sq)
        self.occupancies = [
            sum(self.bitboards[p] for p in range(1, 7)),
            sum(self.bitboards[p] for p in range(7, 13)),
            sum(self.bitboards[p] for p in range(1, 13)),
        ]
        self.key = self.zobrist()
        accumulator = self.accumulator.copy()
        for sq, (before, after) in enumerate(zip(old_board, self.board)):
            if before != after:
                if before:
                    self.network.apply_feature(accumulator, before, sq, -1)
                if after:
                    self.network.apply_feature(accumulator, after, sq, 1)
        self.accumulator = accumulator

    def undo_move(self) -> None:
        if not self._history:
            raise ValueError("No move to undo")
        self._restore(self._history.pop())

    def is_checkmate(self) -> bool:
        return self.in_check() and not self.legal_moves()

    def is_stalemate(self) -> bool:
        return not self.in_check() and not self.legal_moves()

    def material(self) -> int:
        return sum(PIECE_VALUES[type_of(piece)] * (1 if color_of(piece) == WHITE else -1) for piece in self.board if piece)

    def __str__(self) -> str:
        return self.fen()