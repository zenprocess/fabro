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
- **impl_logic**: succeeded
  - Model: gemini-3.5-flash, 220.9k tokens in / 21.3k out
  - Files: /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/game.py, /home/daytona/workspace/fabro/solitaire-app/tests/test_game.py, /home/daytona/workspace/fabro/status.json
- **verify_logic**: succeeded
  - Model: gemini-3.5-flash, 185.6k tokens in / 5.7k out
  - Files: /home/daytona/workspace/fabro/.ai/verify_logic.md, /home/daytona/workspace/fabro/status.json
- **check_logic**: succeeded
- **impl_ui**: succeeded
  - Model: gemini-3.5-flash, 294.6k tokens in / 18.1k out
  - Files: /home/daytona/workspace/fabro/solitaire-app/main.py, /home/daytona/workspace/fabro/solitaire-app/src/solitaire_tui/ui.py, /home/daytona/workspace/fabro/solitaire-app/tests/test_game.py, /home/daytona/workspace/fabro/status.json
- **verify_ui**: succeeded
  - Model: gemini-3.5-flash, 233.4k tokens in / 8.3k out
  - Files: /home/daytona/workspace/fabro/.ai/verify_ui.md, /home/daytona/workspace/fabro/solitaire-app/README.md, /home/daytona/workspace/fabro/solitaire-app/tests/test_game.py, /home/daytona/workspace/fabro/status.json
- **check_ui**: succeeded


Finish the solitaire app.

Do the integration work:
- Wire main.py to start the curses game loop normally
- Keep --smoke non-interactive
- Add README.md run instructions and controls
- Add any missing tests needed for confidence
- Ensure no generated files are outside solitaire-app/ except .ai/ reports and root status.json

Run:
cd solitaire-app && python3 -m pytest tests/ -v && python3 main.py --smoke

Write status.json at workspace root: outcome=succeeded if the app is playable and tests pass, outcome=failed with failure_reason otherwise.