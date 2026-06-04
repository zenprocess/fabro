Goal: Build a terminal-based FreeCell solitaire game in Python


Goal: $goal

Create a detailed implementation spec for the requested Python terminal card game.

Cover:
- Game rules and data structures (Card, Deck, Pile or equivalent state types)
- Terminal rendering approach using the standard-library curses module
- Input handling and move/action validation
- Win/loss detection
- UI layout
- Test strategy

Keep game rules testable without curses. Include a smoke mode so `python3 main.py --smoke` starts enough of the app to prove imports and setup without requiring an interactive terminal.

Write the spec to .ai/card-game-spec.md.
Write status.json at workspace root: outcome=succeeded if the spec is complete, outcome=failed with failure_reason otherwise.