---
name: Engine search correctness
description: Search invariants that materially affect tactical accuracy and large teacher-data training.
---

Quiescence search must not use a stand-pat score while the side to move is in check; it must search all legal evasions, including quiet moves, with a bounded check-evasion depth.

**Why:** A checked node that searches captures only can declare a legal evasion unavailable and miss forced mates or defensive interpositions.

**How to apply:** Preserve this invariant in every engine port, and normalize mate scores by ply when storing and probing transposition-table entries.

Large NNUE teacher datasets must be trained as streamed JSONL rather than loaded into a list of Board objects and accumulators.

**Why:** Two million positions can exceed memory when each record owns a board and a 256-entry accumulator.

**How to apply:** Reopen the dataset once per epoch and update the checkpoint one record at a time.