# FreeCell Solitaire TUI

A terminal-based FreeCell Solitaire game built in Python using the `curses` standard library.

## Project Structure

- `main.py`: CLI entrypoint.
- `src/card_game_tui/`: Core package.
  - `domain.py`: Game rules, logic, state, and card representations (fully decoupled from UI).
  - `tui.py`: Curses-based TUI and interface logic.
- `tests/`: Automated unit and integration tests.

## Installation & Running

This project requires Python 3.10+ and no external dependencies for core play.

To run:
```bash
python3 main.py
```

For smoke-test mode:
```bash
python3 main.py --smoke
```

## Controls & Keyboard Shortcuts

- **Navigation**: Use **Arrow Keys**, **WASD**, or **HJKL** keys to move the cursor around the board.
- **Select / Drop Card**: Press **Space** or **Enter** to select a card from a Tableau or Freecell, and press **Space** or **Enter** again on the destination location to drop/move the card.
- **Adjust Sequence Move Count**: When a Tableau column is selected, use **-** / **+** (or **[** / **]**) to decrease/increase the number of cards to move in a multi-card sequence.
- **Cancel Selection**: Press **Escape** to cancel any active card selection.
- **Undo**: Press **u** or **U** to undo the last move.
- **Redo**: Press **y** or **Y** to redo an undone move.
- **Restart Game**: Press **r** or **R** to restart the current game using the same seed.
- **New Game**: Press **n** or **N** to generate a new random game seed and start a fresh game.
- **Quit**: Press **q** or **Q** to exit the game (press **y** to confirm or any other key to cancel).

## Running Tests

To run tests:
```bash
pytest
```
