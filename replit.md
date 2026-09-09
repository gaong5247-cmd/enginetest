# Python Chess Engine System

Dependency-free Python chess engine system with legal move generation, Alpha-Beta search, NNUE-style evaluation, self-play data generation, model arena, and Windows packaging.

## Run & Operate

- `python tools/initialize_models.py` — create deterministic baseline NNUE checkpoints
- `python Engine.py` — run the UCI engine
- `python Selfplay.py` — open the self-play GUI
- `python Arena.py` — open the model arena GUI
- `python -m unittest discover -s tests -v` — run engine tests
- `python tools/benchmark.py --depth 4` — run the search benchmark
- `.\build_windows.ps1` — build Engine.exe, Selfplay.exe, and Arena.exe on Windows

## Stack

- Python 3.11+ (tested with Python 3.13)
- Standard library runtime; optional PyInstaller only for Windows packaging
- Bitboard-backed Board with reversible state snapshots
- Iterative-deepening Negamax with TT, PVS, quiescence, null move, LMR
- Sparse 768 -> 256 -> 64 -> 1 NNUE-style evaluator

## Where things live

- `chess_engine/board.py` — board state, attacks, legal move generation, FEN, make/undo
- `chess_engine/search.py` — Alpha-Beta, TT, move ordering, pruning
- `chess_engine/nnue.py` — checkpoint format, inference, incremental accumulator
- `chess_engine/uci.py` — UCI protocol loop
- `chess_engine/perft.py` — reference Perft suites
- `chess_engine/selfplay.py` — CPU worker GUI and training data
- `chess_engine/arena.py` — model comparison and promotion UI
- `tests/test_engine.py` — regression and state-integrity tests

## Architecture decisions

- The move generator uses pseudo-legal moves followed by make/check/undo; this favors correctness and keeps pinned/double-check handling explicit.
- Board state snapshots preserve all reversible metadata and the accumulator, making undo exact and easy to debug.
- The built-in NNUE model is deterministic and dependency-free; binary checkpoints use a versioned format.
- Self-play workers are separate processes so CPU worker count is meaningful despite Python's GIL.

## Product

The system provides a UCI engine for GUI/Arena integration, a self-play data generator, and an Arena for testing and promoting current versus previous NNUE checkpoints.

## User preferences

The user requested a staged implementation with correctness and Perft validation before optimization.

## Gotchas

- `python tools/initialize_models.py` should be run before selecting model files in the GUI.
- Perft uses the standard Chessprogramming reference FENs; the Kiwipete label refers to the canonical Position 2 FEN.
- The core does not use `python-chess`; UCI coordinate moves are parsed internally.

