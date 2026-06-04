Goal: Build a terminal-based Klondike solitaire game in Python with a curses TUI

## Completed stages
- **expand_spec**: succeeded
  - Model: gemini-3.5-flash, 140.8k tokens in / 24.8k out
  - Files: /home/daytona/workspace/fabro/.ai/solitaire-spec.md, /home/daytona/workspace/fabro/solitaire-app/game.py, /home/daytona/workspace/fabro/solitaire-app/main.py, /home/daytona/workspace/fabro/solitaire-app/test_game.py, /home/daytona/workspace/fabro/solitaire-app/ui.py, /home/daytona/workspace/fabro/status.json
- **impl_setup**: succeeded
  - Model: gemini-3.5-flash, 148.0k tokens in / 18.3k out
  - Files: /home/daytona/workspace/fabro/solitaire-app/README.md, /home/daytona/workspace/fabro/solitaire-app/main.py, /home/daytona/workspace/fabro/solitaire-app/pyproject.toml, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/__init__.py, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/game.py, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/ui.py, /home/daytona/workspace/fabro/solitaire-app/tests/__init__.py, /home/daytona/workspace/fabro/solitaire-app/tests/test_game.py, /home/daytona/workspace/fabro/status.json
- **verify_setup**: succeeded
  - Model: gemini-3.5-flash, 154.6k tokens in / 2.2k out
  - Files: /home/daytona/workspace/fabro/.ai/verify_setup.md, /home/daytona/workspace/fabro/status.json
- **check_setup**: succeeded
- **impl_data**: succeeded
  - Model: gemini-3.5-flash, 218.9k tokens in / 18.1k out
  - Files: /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/game.py, /home/daytona/workspace/fabro/solitaire-app/tests/test_game.py, /home/daytona/workspace/fabro/status.json
- **verify_data**: succeeded
  - Model: gemini-3.5-flash, 92.2k tokens in / 3.7k out
  - Files: /home/daytona/workspace/fabro/.ai/verify_data.md, /home/daytona/workspace/fabro/status.json
- **check_data**: succeeded


Read .ai/solitaire-spec.md and the current solitaire-app implementation.

Implement complete draw-one Klondike game logic:
- Legal moves from waste/tableau to tableau
- Legal moves to foundation
- Moving face-up tableau runs
- Flipping newly exposed tableau cards
- Drawing from stock to waste
- Recycling waste into stock when stock is empty
- Undo for player actions
- Win detection when all foundations are complete

Add tests for legal moves, illegal moves, deal invariants, stock/waste behavior, undo, and win detection.

Run:
cd solitaire-app && python3 -m pytest tests/ -v

Write status.json at workspace root: outcome=succeeded if all tests pass and rules are implemented, outcome=failed with failure_reason otherwise.