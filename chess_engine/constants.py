"""Chess constants and small bitboard helpers."""

WHITE, BLACK = 0, 1
COLORS = (WHITE, BLACK)
EMPTY = 0
PAWN, KNIGHT, BISHOP, ROOK, QUEEN, KING = range(1, 7)
PIECE_NAMES = ".PNBRQKpnbrqk"
PIECE_VALUES = (0, 100, 320, 330, 500, 900, 20_000)

WK = 1
WQ = 2
BK = 4
BQ = 8

FILE_A = 0x0101010101010101
FILE_H = 0x8080808080808080
NOT_FILE_A = 0xFEFEFEFEFEFEFEFE
NOT_FILE_H = 0x7F7F7F7F7F7F7F7F
FULL_BOARD = 0xFFFFFFFFFFFFFFFF

KNIGHT_ATTACKS = [0] * 64
KING_ATTACKS = [0] * 64
PAWN_ATTACKS = [[0] * 64 for _ in COLORS]


def square(file: int, rank: int) -> int:
    return rank * 8 + file


def file_of(sq: int) -> int:
    return sq & 7


def rank_of(sq: int) -> int:
    return sq >> 3


def bit(sq: int) -> int:
    return 1 << sq


def color_of(piece: int) -> int:
    return WHITE if 1 <= piece <= 6 else BLACK


def type_of(piece: int) -> int:
    return piece if piece <= 6 else piece - 6


def make_piece(color: int, piece_type: int) -> int:
    return piece_type + (6 if color == BLACK else 0)


def pop_lsb(bb: int) -> tuple[int, int]:
    lsb = bb & -bb
    return lsb.bit_length() - 1, bb ^ lsb


def _step_attacks(sq: int, deltas: tuple[tuple[int, int], ...]) -> int:
    f, r = file_of(sq), rank_of(sq)
    attacks = 0
    for df, dr in deltas:
        nf, nr = f + df, r + dr
        if 0 <= nf < 8 and 0 <= nr < 8:
            attacks |= bit(square(nf, nr))
    return attacks


for _sq in range(64):
    KNIGHT_ATTACKS[_sq] = _step_attacks(
        _sq, ((1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2))
    )
    KING_ATTACKS[_sq] = _step_attacks(
        _sq, ((1, 1), (1, 0), (1, -1), (0, 1), (0, -1), (-1, 1), (-1, 0), (-1, -1))
    )
    f, r = file_of(_sq), rank_of(_sq)
    if r < 7:
        if f > 0:
            PAWN_ATTACKS[WHITE][_sq] |= bit(square(f - 1, r + 1))
        if f < 7:
            PAWN_ATTACKS[WHITE][_sq] |= bit(square(f + 1, r + 1))
    if r > 0:
        if f > 0:
            PAWN_ATTACKS[BLACK][_sq] |= bit(square(f - 1, r - 1))
        if f < 7:
            PAWN_ATTACKS[BLACK][_sq] |= bit(square(f + 1, r - 1))


def rook_attacks(sq: int, occupancy: int) -> int:
    attacks = 0
    f, r = file_of(sq), rank_of(sq)
    for nf in range(f + 1, 8):
        target = bit(square(nf, r))
        attacks |= target
        if occupancy & target:
            break
    for nf in range(f - 1, -1, -1):
        target = bit(square(nf, r))
        attacks |= target
        if occupancy & target:
            break
    for nr in range(r + 1, 8):
        target = bit(square(f, nr))
        attacks |= target
        if occupancy & target:
            break
    for nr in range(r - 1, -1, -1):
        target = bit(square(f, nr))
        attacks |= target
        if occupancy & target:
            break
    return attacks


def bishop_attacks(sq: int, occupancy: int) -> int:
    attacks = 0
    f, r = file_of(sq), rank_of(sq)
    for df, dr in ((1, 1), (1, -1), (-1, 1), (-1, -1)):
        nf, nr = f + df, r + dr
        while 0 <= nf < 8 and 0 <= nr < 8:
            target = bit(square(nf, nr))
            attacks |= target
            if occupancy & target:
                break
            nf, nr = nf + df, nr + dr
    return attacks


def queen_attacks(sq: int, occupancy: int) -> int:
    return rook_attacks(sq, occupancy) | bishop_attacks(sq, occupancy)