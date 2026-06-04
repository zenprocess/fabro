# Terminal FreeCell Solitaire

A terminal-based FreeCell Solitaire game implemented in Python 3 using `curses`.

## Installation and Setup

Ensure you have Python 3.8+ installed.

To install dependencies for testing:
```bash
pip install -e ".[dev]"
```

## Running the Game

To run the interactive TUI game:
```bash
python3 main.py
```

To run headless smoke tests:
```bash
python3 main.py --smoke
```

## Running Tests

To run the automated tests:
```bash
pytest
```
