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

## Running Tests

To run tests:
```bash
pytest
```
