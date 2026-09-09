"""Tkinter self-play data generator using isolated CPU worker processes."""

from __future__ import annotations

import multiprocessing as mp
import queue
import threading
import time
from dataclasses import dataclass
from pathlib import Path

try:
    import tkinter as tk
    from tkinter import filedialog, ttk
except ModuleNotFoundError:  # Headless Linux development environments
    tk = None
    filedialog = None
    ttk = None

from .board import Board
from .move import uci
from .nnue import load_network
from .search import Search, SearchLimits
from .training import append_training_position, checkpoint_builtin


@dataclass
class GameSummary:
    game: int
    result: str
    plies: int
    nodes: int
    nps: int
    pgn: str


def play_game(model_path: str, depth: int, movetime: int, max_plies: int, data_dir: str, game_id: int) -> GameSummary:
    network = load_network(model_path)
    board = Board()
    board.network = network
    board.accumulator = network.initial_accumulator(board)
    moves: list[str] = []
    total_nodes = 0
    started = time.monotonic()
    position_path = Path(data_dir) / "training.jsonl"
    for _ in range(max_plies):
        if board.is_checkmate() or board.is_stalemate() or board.halfmove >= 100:
            break
        board.network = network
        search = Search(network=network)
        result = search.search(board, SearchLimits(depth=depth, movetime=movetime or None))
        if not result.best_move:
            break
        append_training_position(position_path, board.fen(), result.score, source="selfplay")
        moves.append(uci(result.best_move))
        total_nodes += result.nodes
        board.make_move(result.best_move)
    if board.is_checkmate():
        result_text = "0-1" if board.side == 0 else "1-0"
    else:
        result_text = "1/2-1/2"
    pgn = f'[Event "Selfplay"]\n[Game "{game_id}"]\n[Result "{result_text}"]\n\n'
    pgn_tokens: list[str] = []
    for index, move in enumerate(moves):
        pgn_tokens.extend([f"{(index // 2) + 1}{'.' if index % 2 == 0 else '...'}", move])
    pgn += " ".join(pgn_tokens)
    pgn += f" {result_text}\n"
    pgn_path = Path(data_dir) / "games.pgn"
    pgn_path.parent.mkdir(parents=True, exist_ok=True)
    with pgn_path.open("a", encoding="utf8") as handle:
        handle.write(pgn + "\n")
    elapsed = max(0.001, time.monotonic() - started)
    return GameSummary(game_id, result_text, len(moves), total_nodes, int(total_nodes / elapsed), pgn)


def _worker(worker_id: int, workers: int, games: int, model: str, depth: int, movetime: int, data_dir: str, max_plies: int, events, stop) -> None:
    for game_id in range(worker_id, games, workers):
        if stop.is_set():
            return
        summary = play_game(model, depth, movetime, max_plies, data_dir, game_id + 1)
        events.put(summary.__dict__)


class SelfplayApp:
    def __init__(self, root: tk.Tk) -> None:
        self.root = root
        root.title("Chess Engine — Self Play")
        root.geometry("760x560")
        root.minsize(680, 480)
        self.processes: list[mp.Process] = []
        self.stop_event = None
        self.events = None
        self.total_games = tk.IntVar(value=100)
        self.workers = tk.IntVar(value=max(1, (mp.cpu_count() or 2) - 1))
        self.depth = tk.IntVar(value=8)
        self.movetime = tk.IntVar(value=0)
        self.model = tk.StringVar(value="models/current.nnue")
        self.completed = 0
        self.wins = self.draws = self.losses = 0
        self._build()

    def _build(self) -> None:
        main = ttk.Frame(self.root, padding=24)
        main.pack(fill="both", expand=True)
        ttk.Label(main, text="SELF PLAY", font=("Segoe UI", 22, "bold")).pack(anchor="w")
        ttk.Label(main, text="Generate training positions and evaluate the current model on the local CPU.").pack(anchor="w", pady=(2, 18))
        settings = ttk.LabelFrame(main, text="Run settings", padding=14)
        settings.pack(fill="x")
        fields = [
            ("Games", self.total_games), ("CPU workers", self.workers), ("Depth", self.depth), ("Time / move (ms, 0 = depth)", self.movetime)
        ]
        for column, (label, variable) in enumerate(fields):
            ttk.Label(settings, text=label).grid(row=0, column=column, sticky="w", padx=5)
            ttk.Spinbox(settings, from_=0, to=10000, textvariable=variable, width=12).grid(row=1, column=column, padx=5, pady=(4, 8))
        ttk.Label(settings, text="Model").grid(row=2, column=0, sticky="w", padx=5)
        ttk.Entry(settings, textvariable=self.model, width=46).grid(row=3, column=0, columnspan=3, sticky="ew", padx=5, pady=4)
        ttk.Button(settings, text="Browse", command=self._browse_model).grid(row=3, column=3, padx=5)
        actions = ttk.Frame(main)
        actions.pack(fill="x", pady=16)
        self.start_button = ttk.Button(actions, text="Start self-play", command=self.start)
        self.start_button.pack(side="left")
        self.stop_button = ttk.Button(actions, text="Stop", command=self.stop, state="disabled")
        self.stop_button.pack(side="left", padx=8)
        self.status = ttk.Label(main, text="Ready")
        self.status.pack(anchor="w")
        self.progress = ttk.Progressbar(main, maximum=100, mode="determinate")
        self.progress.pack(fill="x", pady=(8, 18))
        self.stats = ttk.Label(main, text="Game: 0 / 0\nWhite wins: 0    Draws: 0    Black wins: 0\nNPS: —")
        self.stats.pack(anchor="w")
        self.log = tk.Text(main, height=13, state="disabled", background="#10151c", foreground="#d8e4ef")
        self.log.pack(fill="both", expand=True, pady=(18, 0))

    def _browse_model(self) -> None:
        path = filedialog.askopenfilename(filetypes=[("NNUE model", "*.nnue"), ("All files", "*.*")])
        if path:
            self.model.set(path)

    def start(self) -> None:
        self.stop()
        games = max(1, self.total_games.get())
        workers = max(1, min(self.workers.get(), games))
        checkpoint_builtin(Path(self.model.get()).parent)
        self.events = mp.Queue()
        self.stop_event = mp.Event()
        self.processes = [
            mp.Process(target=_worker, args=(worker, workers, games, self.model.get(), self.depth.get(), self.movetime.get(), "data", 300, self.events, self.stop_event))
            for worker in range(workers)
        ]
        for process in self.processes:
            process.start()
        self.completed = self.wins = self.draws = self.losses = 0
        self.progress["value"] = 0
        self.start_button["state"] = "disabled"
        self.stop_button["state"] = "normal"
        self._poll(games)

    def _poll(self, games: int) -> None:
        if not self.events:
            return
        try:
            while True:
                data = self.events.get_nowait()
                self.completed += 1
                result = data["result"]
                if result == "1-0":
                    self.wins += 1
                elif result == "0-1":
                    self.losses += 1
                else:
                    self.draws += 1
                self.progress["value"] = self.completed * 100 / games
                self.stats["text"] = f"Game: {self.completed} / {games}\nWhite wins: {self.wins}    Draws: {self.draws}    Black wins: {self.losses}\nNPS: {data['nps']:,}"
                self._append_log(f"Game {data['game']}: {result} · {data['plies']} plies · {data['nps']:,} NPS")
        except queue.Empty:
            pass
        if self.completed < games and any(p.is_alive() for p in self.processes):
            self.root.after(150, lambda: self._poll(games))
        else:
            self._finish()

    def _append_log(self, text: str) -> None:
        self.log["state"] = "normal"
        self.log.insert("end", text + "\n")
        self.log.see("end")
        self.log["state"] = "disabled"

    def stop(self) -> None:
        if self.stop_event:
            self.stop_event.set()
        for process in self.processes:
            if process.is_alive():
                process.terminate()
        self.processes.clear()
        if hasattr(self, "start_button"):
            self.start_button["state"] = "normal"
            self.stop_button["state"] = "disabled"

    def _finish(self) -> None:
        for process in self.processes:
            process.join(timeout=0.1)
        self.processes.clear()
        self.start_button["state"] = "normal"
        self.stop_button["state"] = "disabled"
        self.status["text"] = "Self-play complete"


def main() -> None:
    if tk is None:
        raise RuntimeError("Tkinter is required for Selfplay GUI; use Windows Python or install Tk support.")
    root = tk.Tk()
    app = SelfplayApp(root)
    root.protocol("WM_DELETE_WINDOW", lambda: (app.stop(), root.destroy()))
    root.mainloop()