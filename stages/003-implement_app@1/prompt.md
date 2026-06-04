Goal: Build a terminal-based Klondike solitaire game in Python

## Completed stages
- **plan_app**: succeeded
  - Model: gemini-3.5-flash, 113.2k tokens in / 23.4k out
  - Files: /home/daytona/workspace/fabro/.ai/card-game-fast-plan.md, /home/daytona/workspace/fabro/card-game-app/game.py, /home/daytona/workspace/fabro/card-game-app/main.py, /home/daytona/workspace/fabro/card-game-app/tests/test_game.py, /home/daytona/workspace/fabro/status.json


Read .ai/card-game-fast-plan.md.

Build the complete app under card-game-app/ in one focused pass:
- pyproject.toml
- main.py
- src/card_game_tui/ package
- tests/ package
- README.md

Implement:
- Card, Deck, Pile, or equivalent game-state types
- Requested game rules: initial setup/deal where applicable, move/action validation, auto-complete or helper actions where applicable, win/loss condition, undo
- Curses UI with card rendering, board layout, keyboard input, move/action selection, and help text
- --smoke mode that imports the app, creates a game, renders a text snapshot or summary, and exits without curses interaction

Run:
cd card-game-app && python3 -m pytest tests/ -v && python3 -m py_compile main.py src/card_game_tui/*.py && python3 main.py --smoke

Write status.json at workspace root: outcome=succeeded if the app builds, tests pass, and smoke mode works, outcome=failed with failure_reason otherwise.