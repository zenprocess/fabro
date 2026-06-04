Goal: Build a terminal-based Spider solitaire game in Python

## Completed stages
- **plan_app**: succeeded
  - Model: gemini-3.5-flash, 173.1k tokens in / 41.4k out
  - Files: /home/daytona/workspace/fabro/.ai/card-game-fast-plan.md, /home/daytona/workspace/fabro/card-game-app/engine.py, /home/daytona/workspace/fabro/card-game-app/main.py, /home/daytona/workspace/fabro/card-game-app/test_engine.py, /home/daytona/workspace/fabro/status.json
- **implement_app**: succeeded
  - Model: gemini-3.5-flash, 200.2k tokens in / 18.6k out
  - Files: /home/daytona/workspace/fabro/card-game-app/README.md, /home/daytona/workspace/fabro/card-game-app/main.py, /home/daytona/workspace/fabro/card-game-app/pyproject.toml, /home/daytona/workspace/fabro/card-game-app/src/card_game_tui/__init__.py, /home/daytona/workspace/fabro/card-game-app/src/card_game_tui/engine.py, /home/daytona/workspace/fabro/card-game-app/src/card_game_tui/main.py, /home/daytona/workspace/fabro/card-game-app/tests/__init__.py, /home/daytona/workspace/fabro/card-game-app/tests/test_engine.py, /home/daytona/workspace/fabro/status.json


Verify the completed card game app.

Run:
cd card-game-app && python3 -m pytest tests/ -v && python3 -m py_compile main.py src/card_game_tui/*.py && python3 main.py --smoke

Check:
- The app is under card-game-app/
- It uses curses for the interactive TUI
- It implements the requested game rules
- README.md explains setup, run, tests, and controls
- No generated files are outside card-game-app/ except .ai/ reports and root status.json

Write findings to .ai/card-game-fast-verify.md.
Write status.json at workspace root: outcome=succeeded if the app is demo-ready, outcome=failed with specific missing or broken items.