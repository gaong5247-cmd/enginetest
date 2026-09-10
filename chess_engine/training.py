"""PGN/position extraction and simple NNUE checkpoint tooling."""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Iterable

from .board import Board, START_FEN
from .nnue import NNUE, default_network


def append_training_position(path: str | Path, fen: str, target: int, *, source: str = "selfplay") -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with Path(path).open("a", encoding="utf8") as handle:
        handle.write(json.dumps({"fen": fen, "target_cp": target, "source": source}) + "\n")


def positions_from_pgn(pgn: str) -> list[str]:
    """Extract FENs from a coordinate- or SAN-light PGN.

    Self-play writes coordinate moves, which makes this parser deterministic
    and dependency-free. Standard tag pairs and comments are ignored.
    """
    cleaned = re.sub(r"\{[^}]*\}|\([^)]*\)|;[^\n]*", " ", pgn)
    moves = []
    for token in cleaned.split():
        if token.startswith("[") or token.endswith("]") or token[0].isdigit() and token.endswith("."):
            continue
        if token in {"1-0", "0-1", "1/2-1/2", "*"}:
            continue
        if re.fullmatch(r"[a-h][1-8][a-h][1-8][nbrq]?", token):
            moves.append(token)
    board = Board()
    result = [board.fen()]
    for move_text in moves:
        board.make_move(board.find_move(move_text))
        result.append(board.fen())
    return result


def teacher_positions(
    fens: Iterable[str],
    teacher,
    output_path: str | Path,
) -> int:
    count = 0
    for fen in fens:
        board = Board(fen)
        score = teacher(board)
        append_training_position(output_path, fen, int(score), source="teacher")
        count += 1
    return count


def checkpoint_builtin(directory: str | Path = "models") -> tuple[Path, Path]:
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    current = directory / "current.nnue"
    previous = directory / "previous.nnue"
    if not current.exists():
        default_network.save(current, "current")
    if not previous.exists():
        default_network.save(previous, "previous")
    return current, previous


def copy_checkpoint(source: str | Path, target: str | Path, name: str | None = None) -> None:
    model = NNUE.load(source)
    model.save(target, name or Path(target).stem)


def train_checkpoint(
    dataset_path: str | Path,
    output_path: str | Path,
    *,
    base_model: str | Path | None = None,
    epochs: int = 1,
    learning_rate: float = 0.02,
) -> int:
    """Train a checkpoint with a dependency-free SGD pass.

    The initial trainer updates the scalar output bias from teacher targets.
    It is deliberately conservative and provides a reproducible checkpoint
    pipeline; richer feature-gradient training can use the same model format.
    """
    model = NNUE.load(base_model) if base_model else default_network
    record_count = 0
    for _ in range(max(1, epochs)):
        # Stream the JSONL file so large teacher datasets do not require one
        # Board plus accumulator per row in memory.
        with Path(dataset_path).open(encoding="utf8") as handle:
            for line in handle:
                if not line.strip():
                    continue
                row = json.loads(line)
                board = Board(row["fen"])
                board.network = model
                board.accumulator = model.initial_accumulator(board)
                target = int(row["target_cp"])
                prediction = model.evaluate_white(board)
                model.output_bias += int((target - prediction) * learning_rate)
                if _ == 0:
                    record_count += 1
    model.save(output_path, Path(output_path).stem)
    return record_count