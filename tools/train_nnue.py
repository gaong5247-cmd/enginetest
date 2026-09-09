"""Train a new versioned NNUE checkpoint from self-play JSONL."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from chess_engine.training import checkpoint_builtin, train_checkpoint


def main() -> None:
    parser = argparse.ArgumentParser(description="Create a new NNUE checkpoint")
    parser.add_argument("--data", default="data/training.jsonl")
    parser.add_argument("--base", default="models/current.nnue")
    parser.add_argument("--output", default="models/model_001.nnue")
    parser.add_argument("--epochs", type=int, default=1)
    parser.add_argument("--learning-rate", type=float, default=0.02)
    args = parser.parse_args()
    checkpoint_builtin()
    count = train_checkpoint(
        args.data,
        args.output,
        base_model=args.base if Path(args.base).exists() else None,
        epochs=args.epochs,
        learning_rate=args.learning_rate,
    )
    print(f"Trained {count} positions -> {args.output}")


if __name__ == "__main__":
    main()