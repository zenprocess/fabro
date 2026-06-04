I have successfully implemented the terminal-based Klondike solitaire game in Python with a curses TUI inside the `solitaire-app/` directory.

### Summary of Completed Work

1. **Pure Python Game Engine (`solitaire-app/src/solitaire_tui/game_logic.py`)**:
   - Implemented standard Solitaire rule-checking, deck generation, and shuffling.
   - Handled drawing cards from stock to waste and recycling when stock is exhausted.
   - Managed state snapshots to support multi-step **Undo**.
   - Built robust move validation for tableau-to-tableau, waste-to-tableau, waste-to-foundation, tableau-to-foundation, and foundation-to-tableau moves.
   - Programmed win-condition checking (all foundations containing 13 cards).

2. **Visual Terminal Interface (`solitaire-app/src/solitaire_tui/tui.py`)**:
   - Configured robust curses initialization with custom colors (including a custom highlight pair to preserve card color when under cursor/selected).
   - Designed a clear, compact terminal grid representation for Stock, Waste, Foundations, and overlapping Tableau columns.
   - Programmed grid navigation controls using Arrow Keys or WASD.
   - Implemented selection and visual dragging (highlighting multi-card stacks when moving columns).

3. **Application Entry Point (`solitaire-app/main.py`)**:
   - Accepts `--smoke` mode, executing a non-interactive simulation of stock drawing, recycling, valid move validations, undos, and win checks, printing a text snapshot and exiting cleanly.
   - Otherwise starts the interactive game via curses.

4. **Unit Tests & Integration Testing (`solitaire-app/tests/test_game_logic.py`)**:
   - Fully covered game rules, dealing, draw & recycle behavior, tableau-to-tableau, King-to-empty, foundations building, and multi-step undo.
   - Run via `pytest`.

5. **`status.json`**:
   - Recorded `{"outcome": "succeeded"}` at the workspace root as requested.