import os
import sys

# Ensure src directory is in Python path for easy execution
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "src")))

from card_game_tui.engine import GameState, Move, validate_move
from card_game_tui.tui import run_curses_app

def main():
    if "--smoke" in sys.argv:
        print("Running headless smoke tests...")
        try:
            state = GameState()
            state.deal(seed=42)
            
            # Assert initial setup
            assert sum(len(col) for col in state.tableau) == 52, "Tableau must contain 52 cards"
            assert len(state.tableau[0]) == 7, "Column 1 must have 7 cards"
            assert len(state.tableau[7]) == 6, "Column 8 must have 6 cards"
            assert all(fc is None for fc in state.free_cells), "Freecells must start empty"
            
            # Verify move validation logic doesn't crash
            # Attempt an illegal move and ensure it gets rejected
            invalid_move = Move('C', 0, 'C', 1, 1)
            valid, reason = validate_move(state, invalid_move)
            assert not valid, "Expected illegal move to be rejected"
            
            print("Smoke tests passed successfully.")
            sys.exit(0)
        except Exception as e:
            print(f"Smoke test failed: {e}", file=sys.stderr)
            sys.exit(1)
    else:
        # Run standard interactive curses application
        import curses
        curses.wrapper(run_curses_app)

if __name__ == "__main__":
    main()
