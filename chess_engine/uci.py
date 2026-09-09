"""UCI protocol loop for Engine.exe and chess GUIs."""

from __future__ import annotations

import sys
import threading
from typing import TextIO

from .board import Board, START_FEN
from .move import uci
from .nnue import load_network
from .search import Search, SearchLimits, format_info


class UCISession:
    def __init__(self, stdin: TextIO = sys.stdin, stdout: TextIO = sys.stdout) -> None:
        self.stdin, self.stdout = stdin, stdout
        self.board = Board()
        self.search: Search | None = None
        self.search_thread: threading.Thread | None = None
        self.lock = threading.Lock()
        self.network_path: str | None = None
        self.hash_mb = 32

    def write(self, text: str) -> None:
        self.stdout.write(text + "\n")
        self.stdout.flush()

    def run(self) -> None:
        for raw in self.stdin:
            command = raw.strip()
            if not command:
                continue
            if not self.handle(command):
                break
        self.stop_search()

    def handle(self, command: str) -> bool:
        parts = command.split()
        name = parts[0].lower()
        if name == "uci":
            self.write("id name Python NNUE Alpha")
            self.write("id author Replit")
            self.write("option name Hash type spin default 32 min 1 max 1024")
            self.write("option name ModelFile type string default")
            self.write("uciok")
        elif name == "isready":
            self.write("readyok")
        elif name == "setoption":
            if "name" in parts and "value" in parts:
                name_index = parts.index("name") + 1
                value_index = parts.index("value") + 1
                option = " ".join(parts[name_index:parts.index("value")]).lower()
                value = " ".join(parts[value_index:])
                if option == "modelfile":
                    self.network_path = value or None
                elif option == "hash":
                    self.hash_mb = max(1, min(1024, int(value)))
        elif name == "ucinewgame":
            self.stop_search()
            self.board = Board()
        elif name == "position":
            self.stop_search()
            self._position(parts[1:])
        elif name == "go":
            self.stop_search()
            limits = self._limits(parts[1:])
            self.search_thread = threading.Thread(target=self._go, args=(limits,), daemon=True)
            self.search_thread.start()
        elif name == "stop":
            self.stop_search()
        elif name == "quit":
            self.stop_search()
            return False
        elif name == "debug":
            pass
        return True

    def stop_search(self) -> None:
        if self.search:
            self.search.stop()
        if self.search_thread and self.search_thread.is_alive():
            self.search_thread.join(timeout=0.2)
        self.search = None
        self.search_thread = None

    def _position(self, args: list[str]) -> None:
        if not args:
            return
        if args[0] == "startpos":
            self.board = Board(START_FEN)
            move_start = 2 if len(args) > 1 and args[1] == "moves" else len(args)
        elif args[0] == "fen":
            try:
                moves_index = args.index("moves")
                fen = " ".join(args[1:moves_index])
                move_start = moves_index + 1
            except ValueError:
                fen = " ".join(args[1:])
                move_start = len(args)
            self.board = Board(fen)
        else:
            return
        if "moves" in args:
            for move_text in args[move_start:]:
                self.board.make_move(self.board.find_move(move_text))

    def _limits(self, args: list[str]) -> SearchLimits:
        values: dict[str, int] = {}
        index = 0
        while index < len(args):
            if args[index] in {"depth", "movetime", "wtime", "btime", "winc", "binc"} and index + 1 < len(args):
                values[args[index]] = int(args[index + 1])
                index += 2
            else:
                index += 1
        return SearchLimits(
            depth=values.get("depth", 64),
            movetime=values.get("movetime"),
            wtime=values.get("wtime"),
            btime=values.get("btime"),
            winc=values.get("winc", 0),
            binc=values.get("binc", 0),
        )

    def _go(self, limits: SearchLimits) -> None:
        network = load_network(self.network_path)
        board = self.board.copy()
        board.network = network
        board.accumulator = network.initial_accumulator(board)

        def callback(result: object) -> None:
            self.write(format_info(result, board, self.search.tt.hashfull() if self.search else 0))

        self.search = Search(network=network, hash_mb=self.hash_mb, info_callback=callback)
        result = self.search.search(board, limits)
        self.write(f"bestmove {uci(result.best_move)}")


def main() -> None:
    UCISession().run()