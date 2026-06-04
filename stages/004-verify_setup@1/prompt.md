Goal: Build a terminal-based Klondike solitaire game in Python with a curses TUI

## Completed stages
- **expand_spec**: succeeded
  - Model: gemini-3.5-flash, 140.8k tokens in / 24.8k out
  - Files: /home/daytona/workspace/fabro/.ai/solitaire-spec.md, /home/daytona/workspace/fabro/solitaire-app/game.py, /home/daytona/workspace/fabro/solitaire-app/main.py, /home/daytona/workspace/fabro/solitaire-app/test_game.py, /home/daytona/workspace/fabro/solitaire-app/ui.py, /home/daytona/workspace/fabro/status.json
- **impl_setup**: succeeded
  - Model: gemini-3.5-flash, 148.0k tokens in / 18.3k out
  - Files: /home/daytona/workspace/fabro/solitaire-app/README.md, /home/daytona/workspace/fabro/solitaire-app/main.py, /home/daytona/workspace/fabro/solitaire-app/pyproject.toml, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/__init__.py, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/game.py, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/ui.py, /home/daytona/workspace/fabro/solitaire-app/tests/__init__.py, /home/daytona/workspace/fabro/solitaire-app/tests/test_game.py, /home/daytona/workspace/fabro/status.json


Verify setup for the solitaire app.

Check:
1. solitaire-app/pyproject.toml exists
2. solitaire-app/main.py exists
3. solitaire-app/src/solitaire_tui exists
4. Python files compile

Run:
cd solitaire-app && python3 -m py_compile main.py src/solitaire_tui/*.py

Write findings to .ai/verify_setup.md.
Write status.json at workspace root: outcome=succeeded if all checks pass, outcome=failed with failure_reason otherwise.