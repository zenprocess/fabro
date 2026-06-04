Goal: Build a terminal-based Klondike solitaire game in Python with a curses TUI

## Completed stages
- **expand_spec**: succeeded
  - Model: gemini-3.5-flash, 140.8k tokens in / 24.8k out
  - Files: /home/daytona/workspace/fabro/.ai/solitaire-spec.md, /home/daytona/workspace/fabro/solitaire-app/game.py, /home/daytona/workspace/fabro/solitaire-app/main.py, /home/daytona/workspace/fabro/solitaire-app/test_game.py, /home/daytona/workspace/fabro/solitaire-app/ui.py, /home/daytona/workspace/fabro/status.json


Read .ai/solitaire-spec.md.

Create the Python project skeleton under solitaire-app/:
- pyproject.toml with pytest configured
- main.py entrypoint
- src/solitaire_tui/ package
- tests/ directory
- README.md stub

Add minimal importable modules so the project compiles.

Run:
cd solitaire-app && python3 -m py_compile main.py src/solitaire_tui/*.py

Write status.json at workspace root: outcome=succeeded if the project skeleton exists and compiles, outcome=failed with failure_reason otherwise.