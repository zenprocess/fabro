I have successfully verified the terminal UI and completed all validation requirements!

### Summary of Findings & Verification

1. **Smoke Mode Check**: 
   - Running `python3 main.py --smoke` executes flawlessly.
   - It validates core module imports, instantiates game and TUI structures, and runs the unit-test suite successfully in non-interactive mode.

2. **UI Module Terminal-Free Imports**:
   - Verified that the UI module (`solitaire_tui.ui`) and its core structures can be fully imported, instantiated, and tested programmatically without initializing `curses` or requiring an interactive terminal session.

3. **Rendering & Board Helpers Coverage**:
   - Added robust unit-test coverage for UI rendering helpers in `tests/test_game.py`.
   - Used `unittest.mock.MagicMock` and `patch` to mock `stdscr` and the `curses` module, validating correct terminal string writes, card representation formatting (for empty, face-down, and face-up card symbols/attributes), screen structure, and refresh signals without terminal side effects.
   - All tests pass perfectly.

4. **Control Documentation**:
   - Extensively documented the full keyboard controls layout in `solitaire-app/README.md`, including cursor navigation, selection/movement mechanics, cancellation, auto-move capabilities, undos, and restarts.

All findings have been written to `.ai/verify_ui.md`, and the workspace status has been saved to `status.json` with `"outcome": "succeeded"`.