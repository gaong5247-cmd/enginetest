---
name: Python engine runtime
description: Runtime boundary between the dependency-free chess core and Tkinter Windows GUIs.
---

The engine, self-play game loop, training pipeline, and model tooling must remain importable without Tkinter; GUI modules should load Tk support only when a GUI is launched.

**Why:** The development container may be headless and omit `_tkinter`, while the Windows deliverables include Tkinter through the Python/PyInstaller environment.

**How to apply:** Keep core smoke tests independent of GUI imports and make GUI entry points fail with a clear installation message when Tk support is unavailable.