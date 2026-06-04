#!/usr/bin/env python3
import sys
import os

# Add src to python path to make importing easier
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), 'src')))

from card_game_tui.domain import GameState, Position, LocationType

def run_smoke_test() -> int:
    print("Running non-interactive smoke test probe...")
    try:
        # 1. Initialize GameState with a deterministic seed
        seed = 42
        state = GameState(seed=seed)
        print(f"Initialized GameState with seed {seed}")

        # 2. Basic assertions to verify components compile and behave as expected
        # Let's verify we have 8 tableau columns
        if len(state.tableaus) != 8:
            print(f"Error: Expected 8 tableaus, found {len(state.tableaus)}", file=sys.stderr)
            return 1
        
        # 3. Validate a mock move
        from_pos = Position(LocationType.TABLEAU, 0)
        to_pos = Position(LocationType.FREECELL, 0)
        
        is_valid = state.validate_move(from_pos, to_pos)
        print(f"Validating move from Tableau 0 to FreeCell 0: {is_valid}")
        
        success = state.execute_move(from_pos, to_pos)
        print(f"Executed move from Tableau 0 to FreeCell 0: {success}")
        
        # Check win state
        is_win = state.check_win()
        print(f"Win checked: {is_win}")

        print("SMOKE TEST SUCCESSFUL")
        return 0
    except Exception as e:
        print(f"Smoke test failed with error: {e}", file=sys.stderr)
        return 1

def main():
    if "--smoke" in sys.argv:
        sys.exit(run_smoke_test())
    else:
        from card_game_tui.tui import TUIApp
        app = TUIApp()
        app.run()

if __name__ == "__main__":
    main()
