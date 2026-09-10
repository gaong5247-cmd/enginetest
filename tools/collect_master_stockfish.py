"""Collect public master games and label positions with Stockfish.

The player names are Chess.com public archive usernames, not private APIs
provided by the players. The output uses the engine's existing training JSONL
schema and stores Stockfish scores from White's point of view.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import time
import urllib.request
from urllib.error import HTTPError
from dataclasses import dataclass
from datetime import datetime
from io import StringIO
from pathlib import Path

import chess
import chess.engine
import chess.pgn

# Allow `python tools/collect_master_stockfish.py` from the project root
# without requiring callers to set PYTHONPATH.
PROJECT_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PROJECT_ROOT))

from chess_engine.training import checkpoint_builtin, train_checkpoint

API_ROOT = "https://api.chess.com/pub/player"
USER_AGENT = "Python-NNUE-Alpha/1.0 public-game-dataset"


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
    archive_url = f"{API_ROOT}/{player}/games/archives"
    archives = get_json(archive_url).get("archives", [])
    selected = sorted(archives, key=month_key, reverse=True)[:months]
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
        for game in payload.get("games", []):
            game_url = game.get("url", "")
            pgn = game.get("pgn", "")
            if not game_url or not pgn or game_url in seen:
                continue
            seen.add(game_url)
            games.append(PublicGame(player=player, url=game_url, pgn=pgn))
            if len(games) >= max_games:
                return games
        time.sleep(0.15)
    return games


def parse_score(info: dict, board: chess.Board) -> int:
    score = info["score"].pov(chess.WHITE)
    value = score.score(mate_score=100_000)
    return int(value if value is not None else 0)


def label_games(
    games: list[PublicGame],
    engine_path: str,
    output: Path,
    depth: int,
    every: int,
    limit: int,
    overwrite: bool,
) -> int:
    output.parent.mkdir(parents=True, exist_ok=True)
    mode = "w" if overwrite else "a"
    count = 0
    players = sorted({game.player for game in games})
    per_player_limit = max(1, limit // max(1, len(players)))
    player_counts = {player: 0 for player in players}
    engine = chess.engine.SimpleEngine.popen_uci(engine_path)
    try:
        engine.configure({"Threads": 1, "Hash": 32})
        with output.open(mode, encoding="utf-8") as handle:
            for game_index, public_game in enumerate(games, start=1):
                if player_counts[public_game.player] >= per_player_limit:
                    continue
                try:
                    game = chess.pgn.read_game(StringIO(public_game.pgn))
                    if game is None:
                        continue
                    board = game.board()
                    headers = game.headers
                    white = headers.get("White", "")
                    black = headers.get("Black", "")
                    for ply, move in enumerate(game.mainline_moves()):
                        if (
                            ply % every == 0
                            and count < limit
                            and player_counts[public_game.player] < per_player_limit
                        ):
                            info = engine.analyse(board, chess.engine.Limit(depth=depth))
                            row = {
                                "fen": board.fen(),
                                "target_cp": parse_score(info, board),
                                "source": "chess.com-stockfish",
                                "teacher": "Stockfish",
                                "player": public_game.player,
                                "game_url": public_game.url,
                                "game_index": game_index,
                                "ply": ply,
                                "white": white,
                                "black": black,
                            }
                            handle.write(json.dumps(row, ensure_ascii=False) + "\n")
                            count += 1
                            player_counts[public_game.player] += 1
                            if count % 100 == 0:
                                handle.flush()
                                print(f"labeled {count} positions")
                        board.push(move)
                        if count >= limit:
                            break
                except (ValueError, chess.InvalidMoveError) as error:
                    print(f"skip malformed game {public_game.url}: {error}")
                if count >= limit:
                    break
    finally:
        engine.quit()
    return count


def main() -> None:
    parser = argparse.ArgumentParser(description="Build a Stockfish-labeled dataset from public Chess.com archives.")
    parser.add_argument("--players", nargs="+", default=["magnuscarlsen", "hikaru"])
    parser.add_argument("--months", type=int, default=2, help="latest archive months per player")
    parser.add_argument("--max-games-per-player", type=int, default=25)
    parser.add_argument("--engine", default=shutil.which("stockfish") or "stockfish")
    parser.add_argument("--depth", type=int, default=12)
    parser.add_argument("--every", type=int, default=2, help="label every Nth ply")
    parser.add_argument("--limit-positions", type=int, default=2_000)
    parser.add_argument("--output", type=Path, default=Path("data/master_stockfish.jsonl"))
    parser.add_argument("--overwrite", action="store_true")
    parser.add_argument("--train", action="store_true", help="train a new checkpoint after collection")
    parser.add_argument("--epochs", type=int, default=1)
    parser.add_argument("--learning-rate", type=float, default=0.02)
    parser.add_argument("--model-output", type=Path, default=Path("models/master-stockfish.nnue"))
    args = parser.parse_args()

    if not Path(args.engine).exists() and shutil.which(args.engine) is None:
        parser.error(f"Stockfish executable not found: {args.engine}")

    all_games: list[PublicGame] = []
    for player in args.players:
        games = fetch_games(player, args.months, args.max_games_per_player)
        all_games.extend(games)
        print(f"{player}: fetched {len(games)} public games")
    if not all_games:
        parser.error("No public games were returned by the Chess.com archive API")

    count = label_games(
        all_games,
        args.engine,
        args.output,
        args.depth,
        max(1, args.every),
        max(1, args.limit_positions),
        args.overwrite,
    )
    print(f"saved {count} Stockfish-labeled positions to {args.output}")

    if args.train:
        current, _ = checkpoint_builtin("models")
        trained = train_checkpoint(
            args.output,
            args.model_output,
            base_model=current,
            epochs=max(1, args.epochs),
            learning_rate=args.learning_rate,
        )
        print(f"trained {trained} positions into {args.model_output} at {datetime.now().isoformat(timespec='seconds')}")


if __name__ == "__main__":
    main()