"""Create the initial current/previous checkpoints for a fresh checkout."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from chess_engine.training import checkpoint_builtin


if __name__ == "__main__":
    current, previous = checkpoint_builtin(Path(__file__).resolve().parents[1] / "models")
    print(f"Current model:  {current}")
    print(f"Previous model: {previous}")