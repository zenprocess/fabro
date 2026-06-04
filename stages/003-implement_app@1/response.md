I have successfully designed, built, and verified the complete terminal-based Spider Solitaire game package in Python. 

### What has been built:
1. **Packaging (`card-game-app/pyproject.toml`)**: Configured a modern standard Python PEP-621 layout featuring setuptools package discovery for the TUI app, along with standard test path configurations and test pythonpath injection so tests resolve cleanly.
2. **Modular Architecture (`card-game-app/src/card_game_tui/`)**:
   - `__init__.py`: Package initialization.
   - `engine.py`: A highly decoupled, pure-Python state machine implementing full Spider Solitaire rules, including initial deal/setup of 2 full decks, move/action validation, automatic exposure of facedown cards, detection and auto-clearing of King-to-Ace single-suit sequences, full deep-state Undo/Redo capability, score computation, and win/loss verification.
   - `main.py`: Fully featured standard-library `curses` TUI framework featuring intuitive grid-based cursor navigation (Arrow keys/WASD), color-coded cards and highlighted states, difficulty selectors (1-Suit/2-Suit/4-Suit), pop-up action confirmation overlays, full-screen resize handling, and a dedicated non-interactive `--smoke` mode.
3. **App Entry Point (`card-game-app/main.py`)**: Thin entry script wrapping package execution and adjusting system lookup paths.
4. **Automated Testing Suite (`card-game-app/tests/`)**: Fully isolated test module validating:
   - Initial layout setup, stock levels, and difficulty configuration splits.
   - Sequence generation and movable indices detection.
   - Valid and invalid move validations.
   - Automated stock-dealing restrictions.
   - Move execution and state updates.
   - Sequence clearing & score updates.
   - Full history-state undo reliability.
5. **Detailed Documentation (`card-game-app/README.md`)**: Complete controls list, feature outline, setup, and execution instructions.

### Test Execution & Smoke Output:
All python compilation checks, unit tests, and automated smoke test validation succeeded flawlessly.

```bash
cd card-game-app && python3 -m pytest tests/ -v && python3 -m py_compile main.py src/card_game_tui/*.py && python3 main.py --smoke
```

- **9 pytest unit tests passed** in `0.01s`.
- Code compiled completely with no syntax issues or warnings.
- Smoke test completed successfully with programmatic JSON report output.
- `/home/daytona/workspace/fabro/status.json` has been updated with `"outcome": "succeeded"`.