import sys
import os
import argparse
import unittest

# Ensure the "src" directory and current directory are on sys.path so imports work seamlessly
current_dir = os.path.dirname(os.path.abspath(__file__))
src_dir = os.path.join(current_dir, "src")
if src_dir not in sys.path:
    sys.path.insert(0, src_dir)
if current_dir not in sys.path:
    sys.path.insert(1, current_dir)

def run_smoke_test() -> None:
    """Proves imports work, instantiates core classes, and runs self-tests."""
    print("Running Solitaire smoke tests...")
    
    # 1. Test Imports
    try:
        from solitaire_tui.game import Card, GameState
        from solitaire_tui.ui import SolitaireTUI
        print("✓ Successfully imported core modules (game, ui).")
    except Exception as e:
        print(f"✗ Failed to import core modules: {e}")
        sys.exit(1)

    # 2. Test instantiation of core logic
    try:
        game = GameState(seed=123)
        print(f"✓ GameState instantiated. Stock size: {len(game.stock)} cards.")
        
        tui = SolitaireTUI(game)
        print("✓ SolitaireTUI instantiated.")
    except Exception as e:
        print(f"✗ Failed to instantiate game components: {e}")
        sys.exit(1)

    # 3. Run full unit tests to confirm rule-engine validity
    print("Running automated unit tests...")
    loader = unittest.TestLoader()
    # Discover and run tests
    try:
        import tests.test_game as test_game
        suite = loader.loadTestsFromModule(test_game)
    except Exception as e:
        print(f"✗ Failed to import tests: {e}")
        sys.exit(1)
        
    runner = unittest.TextTestRunner(verbosity=1)
    result = runner.run(suite)
    
    if result.wasSuccessful():
        print("✓ All automated rules unit tests passed successfully!")
    else:
        print("✗ Automated unit tests failed!")
        sys.exit(1)

    print("Smoke mode passed successfully.")
    sys.exit(0)

def main() -> None:
    parser = argparse.ArgumentParser(description="Terminal Klondike Solitaire")
    parser.add_argument(
        "--smoke",
        action="store_true",
        help="Run non-interactive smoke tests to verify imports, setup, and game rules."
    )
    args = parser.parse_args()

    if args.smoke:
        run_smoke_test()

    # Normal execution starts curses-based TUI
    import curses
    from solitaire_tui.game import GameState
    from solitaire_tui.ui import SolitaireTUI

    game = GameState()
    tui = SolitaireTUI(game)
    
    try:
        curses.wrapper(tui.run)
    except Exception as e:
        print(f"Error running solitaire TUI: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
