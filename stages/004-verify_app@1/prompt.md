Goal: Quickly build a terminal-based Klondike solitaire game in Python with a curses TUI

## Completed stages
- **plan_app**: succeeded
  - Model: gemini-3.5-flash, 61.9k tokens in / 4.6k out
  - Files: /home/daytona/workspace/fabro/.ai/solitaire-fast-plan.md, /home/daytona/workspace/fabro/status.json
- **implement_app**: succeeded
  - Model: gemini-3.5-flash, 177.7k tokens in / 31.1k out
  - Files: /home/daytona/workspace/fabro/solitaire-app/README.md, /home/daytona/workspace/fabro/solitaire-app/main.py, /home/daytona/workspace/fabro/solitaire-app/pyproject.toml, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/__init__.py, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/game_logic.py, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/tui.py, /home/daytona/workspace/fabro/solitaire-app/tests/__init__.py, /home/daytona/workspace/fabro/solitaire-app/tests/test_game_logic.py, /home/daytona/workspace/fabro/status.json


Verify the completed solitaire app.

Run:
cd solitaire-app && python3 -m pytest tests/ -v && python3 -m py_compile main.py src/solitaire_tui/*.py && python3 main.py --smoke

Check:
- The app is under solitaire-app/
- It uses curses for the interactive TUI
- It implements draw-one Klondike rules
- README.md explains setup, run, tests, and controls
- No generated files are outside solitaire-app/ except .ai/ reports and root status.json

Write findings to .ai/solitaire-fast-verify.md.
Write status.json at workspace root: outcome=succeeded if the app is demo-ready, outcome=failed with specific missing or broken items.