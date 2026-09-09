"""Tkinter model-vs-model arena with optional promotion."""

from __future__ import annotations

import shutil
import threading
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
from .training import checkpoint_builtin


def match(current_path: str, previous_path: str, games: int, depth: int, movetime: int, report) -> dict[str, float]:
    current = load_network(current_path)
    previous = load_network(previous_path)
    current_wins = previous_wins = draws = 0
    lengths: list[int] = []
    nodes: list[int] = []
    for game in range(games):
        board = Board()
        current_color = 0 if game % 2 == 0 else 1
        board.network = current if current_color == 0 else previous
        board.accumulator = board.network.initial_accumulator(board)
        total_nodes = 0
        moves: list[str] = []
        for _ in range(250):
            if board.is_checkmate() or board.is_stalemate() or board.halfmove >= 100:
                break
            network = current if board.side == current_color else previous
            board.network = network
            board.accumulator = network.initial_accumulator(board)
            result = Search(network=network).search(board, SearchLimits(depth=depth, movetime=movetime or None))
            if not result.best_move:
                break
            total_nodes += result.nodes
            moves.append(uci(result.best_move))
            board.make_move(result.best_move)
        if board.is_checkmate():
            winner = 1 - board.side
            if winner == current_color:
                current_wins += 1
            else:
                previous_wins += 1
        else:
            draws += 1
        lengths.append(len(moves))
        nodes.append(total_nodes)
        report(game + 1, games, current_wins, previous_wins, draws, lengths[-1], total_nodes)
    score = (current_wins + draws / 2) / max(1, games) * 100
    return {
        "current_wins": current_wins,
        "previous_wins": previous_wins,
        "draws": draws,
        "current_score": score,
        "previous_score": 100 - score,
        "average_length": sum(lengths) / max(1, len(lengths)),
        "average_nodes": sum(nodes) / max(1, len(nodes)),
    }


class ArenaApp:
    def __init__(self, root: tk.Tk) -> None:
        self.root = root
        root.title("Chess Engine — Model Arena")
        root.geometry("760x540")
        self.games = tk.IntVar(value=50)
        self.depth = tk.IntVar(value=8)
        self.movetime = tk.IntVar(value=0)
        self.minimum_games = tk.IntVar(value=100)
        self.threshold = tk.DoubleVar(value=55.0)
        self.current = tk.StringVar(value="models/current.nnue")
        self.previous = tk.StringVar(value="models/previous.nnue")
        self.running = False
        self._build()

    def _build(self) -> None:
        main = ttk.Frame(self.root, padding=24)
        main.pack(fill="both", expand=True)
        ttk.Label(main, text="ENGINE ARENA", font=("Segoe UI", 22, "bold")).pack(anchor="w")
        ttk.Label(main, text="Compare the current NNUE checkpoint with the previous one before promotion.").pack(anchor="w", pady=(2, 18))
        settings = ttk.LabelFrame(main, text="Match settings", padding=14)
        settings.pack(fill="x")
        for column, (label, variable, max_value) in enumerate([
            ("Games", self.games, 10000), ("Depth", self.depth, 64), ("Time / move (ms)", self.movetime, 600000),
            ("Promote after games", self.minimum_games, 10000), ("Pass score %", self.threshold, 100),
        ]):
            ttk.Label(settings, text=label).grid(row=0, column=column, sticky="w", padx=5)
            ttk.Spinbox(settings, from_=0, to=max_value, textvariable=variable, width=12).grid(row=1, column=column, padx=5, pady=(4, 12))
        ttk.Label(settings, text="Current model").grid(row=2, column=0, sticky="w", padx=5)
        ttk.Entry(settings, textvariable=self.current, width=48).grid(row=3, column=0, columnspan=3, sticky="ew", padx=5)
        ttk.Label(settings, text="Previous model").grid(row=4, column=0, sticky="w", padx=5, pady=(8, 0))
        ttk.Entry(settings, textvariable=self.previous, width=48).grid(row=5, column=0, columnspan=3, sticky="ew", padx=5)
        actions = ttk.Frame(main)
        actions.pack(fill="x", pady=16)
        self.run_button = ttk.Button(actions, text="Run arena", command=self.run)
        self.run_button.pack(side="left")
        self.promote_button = ttk.Button(actions, text="Promote current", command=self.promote, state="disabled")
        self.promote_button.pack(side="left", padx=8)
        self.status = ttk.Label(main, text="Ready")
        self.status.pack(anchor="w")
        self.result = ttk.Label(main, text="CURRENT MODEL\nWins: —    Score: —\n\nPREVIOUS MODEL\nWins: —    Score: —", font=("Segoe UI", 14, "bold"))
        self.result.pack(anchor="w", pady=18)
        self.log = tk.Text(main, height=11, state="disabled", background="#10151c", foreground="#d8e4ef")
        self.log.pack(fill="both", expand=True)

    def browse(self, variable: tk.StringVar) -> None:
        path = filedialog.askopenfilename(filetypes=[("NNUE model", "*.nnue"), ("All files", "*.*")])
        if path:
            variable.set(path)

    def run(self) -> None:
        if self.running:
            return
        self.running = True
        self.run_button["state"] = "disabled"
        self.promote_button["state"] = "disabled"
        thread = threading.Thread(target=self._run_match, daemon=True)
        thread.start()

    def _run_match(self) -> None:
        def report(game, games, current_wins, previous_wins, draws, length, nodes):
            self.root.after(0, lambda: self._report(game, games, current_wins, previous_wins, draws, length, nodes))

        result = match(self.current.get(), self.previous.get(), max(1, self.games.get()), self.depth.get(), self.movetime.get(), report)
        self.root.after(0, lambda: self._complete(result))

    def _report(self, game, games, current_wins, previous_wins, draws, length, nodes) -> None:
        self.status["text"] = f"Game {game} / {games}"
        current_score = (current_wins + draws / 2) / max(1, game) * 100
        self.result["text"] = f"CURRENT MODEL\nWins: {current_wins}    Draws: {draws}    Score: {current_score:.1f}%\n\nPREVIOUS MODEL\nWins: {previous_wins}    Score: {100-current_score:.1f}%"
        self._append_log(f"Game {game}: {length} plies · {nodes:,} nodes")

    def _complete(self, result: dict[str, float]) -> None:
        self.running = False
        self.run_button["state"] = "normal"
        passed = result["current_score"] >= self.threshold.get() and self.games.get() >= self.minimum_games.get()
        self.promote_button["state"] = "normal" if passed else "disabled"
        verdict = "CURRENT MODEL IS STRONGER" if passed else "CURRENT MODEL DID NOT PASS"
        self.status["text"] = f"RESULT: {verdict}"
        self._append_log(f"Average game length: {result['average_length']:.1f} · Average nodes: {result['average_nodes']:,.0f}")

    def promote(self) -> None:
        current, previous = Path(self.current.get()), Path(self.previous.get())
        if current.exists():
            shutil.copy2(current, previous)
            self._append_log(f"Promoted {current} to {previous}")
            self.promote_button["state"] = "disabled"

    def _append_log(self, text: str) -> None:
        self.log["state"] = "normal"
        self.log.insert("end", text + "\n")
        self.log.see("end")
        self.log["state"] = "disabled"


def main() -> None:
    if tk is None:
        raise RuntimeError("Tkinter is required for Arena GUI; use Windows Python or install Tk support.")
    checkpoint_builtin()
    root = tk.Tk()
    ArenaApp(root)
    root.mainloop()