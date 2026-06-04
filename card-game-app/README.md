# Terminal FreeCell Solitaire

A terminal-based FreeCell solitaire game in Python using the standard library `curses` module.

## Features

- **Intuitive Two-Step Controls**: Select a source tableau column or FreeCell, then select your destination.
- **Auto-Collect (Auto-Home)**: Automatically moves safe cards to foundations on every move or manually via the `s` key.
- **Smart Sequence Moves**: Move alternating, descending card sequences across columns automatically based on the number of empty FreeCells and columns.
- **Undo Support**: Full undo history using the `u` key.
- **Color Coded UI**: Highlighting and color schemes for easy readability and status messages.
- **Deterministic Seed Support**: Supports standard game layouts with seeding for testing or reproduction.
- **Headless Smoke Mode**: Run headless tests and simulation with `--smoke`.

## Controls

### Source/Destination Selection
1. **Source Selection**:
   - `1` - `8`: Select Tableau column 1 to 8.
   - `q`, `w`, `e`, `r`: Select FreeCell 1, 2, 3, or 4.
2. **Destination Selection** (after selecting a source):
   - `1` - `8`: Move to destination Tableau column.
   - `q`, `w`, `e`, `r`: Move to destination FreeCell.
   - `f`: Move to Foundation (or press space).

### General Commands
- `u`: Undo the last move.
- `s`: Force auto-collect safe cards to foundations.
- `n`: Start a new game (asks for confirmation).
- `Esc` or `x`: Exit the game (asks for confirmation).

## Directory Structure

```text
card-game-app/
├── main.py               # CLI entry point (handles --smoke vs curses UI)
├── pyproject.toml        # Build and Pytest configuration
├── README.md             # This documentation
├── src/
│   └── card_game_tui/
│       ├── __init__.py   # Exports core API
│       ├── game.py       # Core rules, Card, Deck, and GameState logic
│       └── ui.py         # Curses interface rendering and loop
└── tests/
    ├── __init__.py
    └── test_game.py      # pytest test suite for game logic
```

## Setup & Running

Requires Python 3.8+.

### Running the game
```bash
python3 main.py
```

### Running the Non-Interactive Smoke Test
```bash
python3 main.py --smoke
```

### Running Unit Tests
```bash
python3 -m pytest tests/ -v
```
