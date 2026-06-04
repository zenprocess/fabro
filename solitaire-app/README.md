# Terminal Klondike Solitaire

A terminal-based Klondike Solitaire game in Python 3.11+ using the standard `curses` library.

## Features
- Standard Draw-One Klondike Solitaire rules
- Fully keyboard-driven curses TUI
- Unlimited Undo functionality
- Auto-move to foundations
- Fully testable pure Python core game engine

## Installation & Running

To run the game, use:
```bash
python3 main.py
```

To run smoke tests (non-interactive):
```bash
python3 main.py --smoke
```

## Keyboard Controls

Navigate around the board using the keyboard:

*   **Move Cursor**: Use standard **Arrow Keys** or Vim navigation keys (**`h`** / **`j`** / **`k`** / **`l`**).
*   **Select / Draw / Move**: Press **Space** or **Enter**.
    *   Highlighting the **STOCK** and pressing Space/Enter draws a card to the **WASTE** pile. When empty, it recycles the waste back to stock.
    *   Highlighting a card in the **WASTE** or **TABLEAU** and pressing Space/Enter selects that card (or stack of cards).
    *   With a card selected, moving the cursor to a target pile (**TABLEAU** or **FOUNDATION**) and pressing Space/Enter completes the move if valid.
*   **Cancel Selection**: Press **`c`**, **`C`**, or **Escape**.
*   **Auto-move**: Press **`a`** or **`A`** to automatically move the currently highlighted card (from Waste or Tableau) to a valid Foundation pile, if possible.
*   **Undo**: Press **`u`** or **`U`** to undo your last move. Supports unlimited undos!
*   **Restart**: Press **`r`** or **`R`** to redeal a new game.
*   **Help**: Press **`?`** to show the control help banner.
*   **Quit**: Press **`q`** or **`Q`** to exit the game.

## Testing
To run the automated tests using pytest:
```bash
cd solitaire-app
pytest
```
