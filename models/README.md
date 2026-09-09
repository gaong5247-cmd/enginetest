# NNUE model directory

`current.nnue` is the checkpoint used by `Engine.exe` by default when selected
from the GUI. `previous.nnue` is the comparison checkpoint used by Arena.

A fresh checkout can create both deterministic baseline models with:

```bash
python tools/initialize_models.py
```

The binary format is versioned by `chess_engine.nnue.NNUE`. New checkpoints
should be written to a new file first, then tested by Arena before promotion.