"""Large public-master Stockfish teacher pipeline.

This pipeline intentionally uses public Chess.com game archives rather than
claiming access to a private API owned by either player:

    generate -> python-chess FEN pool
    evaluate -> parallel Stockfish labels
    train    -> streaming NNUE checkpoint update

The files are JSONL so a two-million-position run stays restartable and never
requires the entire dataset in RAM.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import time
import urllib.request
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from urllib.error import HTTPError

import chess
import chess.engine
import chess.pgn

PROJECT_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PROJECT_ROOT))

from chess_engine.training import checkpoint_builtin, train_checkpoint

API_ROOT = "https://api.chess.com/pub/player"
USER_AGENT = "Python-NNUE-Alpha/1.0 public-game-dataset"
_ENGINE: chess.engine.SimpleEngine | None = None
_DEPTH = 10


@dataclass(frozen=True)
class PublicGame:
    player: str
    url: str
    pgn: str


def get_json(url: str) -> dict:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def month_key(url: str) -> tuple[int, int]:
    year, month = url.rstrip("/").split("/")[-2:]
    return int(year), int(month)


def fetch_games(player: str, months: int, max_games: int) -> list[PublicGame]:
    archives = get_json(f"{API_ROOT}/{player}/games/archives").get("archives", [])
    selected = sorted(archives, key=month_key, reverse=True)
    if months > 0:
        selected = selected[:months]
    games: list[PublicGame] = []
    seen: set[str] = set()
    for url in selected:
        try:
            payload = get_json(url)
        except HTTPError as error:
            if error.code == 404:
                print(f"{player}: archive not ready, skipping {url}")
                continue
            raise
        for item in payload.get("games", []):
            game_url = item.get("url", "")
            pgn = item.get("pgn", "")
            if not game_url or not pgn or game_url in seen:
                continue
            seen.add(game_url)
            games.append(PublicGame(player, game_url, pgn))
            if max_games > 0 and len(games) >= max_games:
                return games
        time.sleep(0.1)
    return games


def write_position_pool(
    games: list[PublicGame],
    output: Path,
    positions: int,
    every: int,
    overwrite: bool,
) -> int:
    output.parent.mkdir(parents=True, exist_ok=True)
    written = 0
    mode = "w" if overwrite else "a"
    with output.open(mode, encoding="utf-8") as handle:
        for game_index, public_game in enumerate(games, start=1):
            game = chess.pgn.read_game(__import__("io").StringIO(public_game.pgn))
            if game is None:
                continue
            board = game.board()
            headers = game.headers
            for ply, move in enumerate(game.mainline_moves()):
                if ply % every == 0:
                    row = {
                        "fen": board.fen(),
                        "source": "chess.com-public",
                        "player": public_game.player,
                        "game_url": public_game.url,
                        "game_index": game_index,
                        "ply": ply,
                        "white": headers.get("White", ""),
                        "black": headers.get("Black", ""),
                    }
                    handle.write(json.dumps(row, ensure_ascii=False) + "\n")
                    written += 1
                    if written % 10_000 == 0:
                        handle.flush()
                        print(f"generated {written} positions")
                    if written >= positions:
                        return written
                board.push(move)
    return written


def _init_worker(engine_path: str, depth: int) -> None:
    global _ENGINE, _DEPTH
    _DEPTH = depth
    _ENGINE = chess.engine.SimpleEngine.popen_uci(engine_path)
    # python-chess manages Ponder itself; configuring it here raises
    # EngineError on recent python-chess releases.
    _ENGINE.configure({"Threads": 1, "Hash": 16})


def _evaluate_line(line: str) -> str:
    if _ENGINE is None:
        raise RuntimeError("Stockfish worker was not initialized")
    row = json.loads(line)
    board = chess.Board(row["fen"])
    info = _ENGINE.analyse(board, chess.engine.Limit(depth=_DEPTH))
    score = info["score"].pov(chess.WHITE).score(mate_score=100_000)
    row["target_cp"] = int(score if score is not None else 0)
    row["teacher"] = "Stockfish"
    row["source"] = "chess.com-stockfish"
    return json.dumps(row, ensure_ascii=False)


def evaluate_pool(
    input_path: Path,
    output_path: Path,
    engine_path: str,
    depth: int,
    workers: int,
    positions: int,
    overwrite: bool,
) -> int:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    mode = "w" if overwrite else "a"
    count = 0
    with input_path.open(encoding="utf-8") as source, output_path.open(mode, encoding="utf-8") as target:
        with ProcessPoolExecutor(
            max_workers=workers,
            initializer=_init_worker,
            initargs=(engine_path, depth),
        ) as pool:
            lines = (line for line in source if line.strip())
            for evaluated in pool.map(_evaluate_line, lines, chunksize=32):
                target.write(evaluated + "\n")
                count += 1
                if count % 10_000 == 0:
                    target.flush()
                    print(f"evaluated {count} positions")
                if count >= positions:
                    break
    return count


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate and Stockfish-label a large public-master position dataset.")
    parser.add_argument("--mode", choices=("generate", "evaluate", "all", "train"), default="all")
    parser.add_argument("--players", nargs="+", default=["magnuscarlsen", "hikaru"])
    parser.add_argument("--months", type=int, default=0, help="latest months per player; 0 means all available archives")
    parser.add_argument("--max-games-per-player", type=int, default=0, help="0 means all games in selected archives")
    parser.add_argument("--positions", type=int, default=2_000_000)
    parser.add_argument("--every", type=int, default=1)
    parser.add_argument("--workers", type=int, default=max(1, min(8, (os.cpu_count() or 2) // 2)))
    parser.add_argument("--depth", type=int, default=10)
    parser.add_argument("--engine", default=shutil.which("stockfish") or "stockfish")
    parser.add_argument("--pool", type=Path, default=Path("data/master_positions.jsonl"))
    parser.add_argument("--output", type=Path, default=Path("data/master_stockfish.jsonl"))
    parser.add_argument("--overwrite", action="store_true")
    parser.add_argument("--train", action="store_true")
    parser.add_argument("--epochs", type=int, default=1)
    parser.add_argument("--learning-rate", type=float, default=0.02)
    parser.add_argument("--model-output", type=Path, default=Path("models/master-stockfish.nnue"))
    args = parser.parse_args()

    if not Path(args.engine).exists() and shutil.which(args.engine) is None:
        parser.error(f"Stockfish executable not found: {args.engine}")
    positions = max(1, args.positions)

    if args.mode in {"generate", "all"}:
        games: list[PublicGame] = []
        for player in args.players:
            fetched = fetch_games(player, args.months, args.max_games_per_player)
            games.extend(fetched)
            print(f"{player}: fetched {len(fetched)} public games")
        if not games:
            parser.error("No public games were returned by the Chess.com archive API")
        generated = write_position_pool(games, args.pool, positions, max(1, args.every), args.overwrite)
        print(f"generated {generated} raw positions in {args.pool}")

    if args.mode in {"evaluate", "all"}:
        if not args.pool.exists():
            parser.error(f"position pool does not exist: {args.pool}")
        evaluated = evaluate_pool(
            args.pool,
            args.output,
            args.engine,
            max(1, args.depth),
            max(1, args.workers),
            positions,
            args.overwrite,
        )
        print(f"evaluated {evaluated} positions in {args.output}")

    if args.train or args.mode == "train":
        if not args.output.exists():
            parser.error(f"Stockfish dataset does not exist: {args.output}")
        current, _ = checkpoint_builtin("models")
        trained = train_checkpoint(
            args.output,
            args.model_output,
            base_model=current,
            epochs=max(1, args.epochs),
            learning_rate=args.learning_rate,
        )
        print(f"trained {trained} streamed positions into {args.model_output} at {datetime.now().isoformat(timespec='seconds')}")


if __name__ == "__main__":
    main()