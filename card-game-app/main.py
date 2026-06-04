#!/usr/bin/env python3
import sys
from game import FreeCellGame
from ui import FreeCellUI

def run_smoke_test() -> int:
    """Runs a non-interactive smoke test/verification of the FreeCell game logic."""
    print("Initializing FreeCell Game smoke test...")
    
    # 1. Initialize deterministic game with fixed seed
    game = FreeCellGame(seed=42)
    
    # 2. Verify initial deals
    print("Verifying initial card distribution...")
    if len(game.tableaus) != 8:
        print(f"Error: Expected 8 tableaus, got {len(game.tableaus)}", file=sys.stderr)
        return 1
        
    lengths = [len(t) for t in game.tableaus]
    expected_lengths = [7, 7, 7, 7, 6, 6, 6, 6]
    if lengths != expected_lengths:
        print(f"Error: Expected tableau sizes {expected_lengths}, got {lengths}", file=sys.stderr)
        return 1
        
    print(f"Initial tableau card counts match perfectly: {lengths}")
    
    # 3. Verify single card move to Free Cell
    top_card_t0 = game.tableaus[0][-1]
    print(f"Moving card {top_card_t0} from Tableau 0 to Free Cell 0...")
    
    success = game.move_card_to_free_cell('tableau', 0, 0)
    if not success:
        print("Error: Failed to move card to empty free cell", file=sys.stderr)
        return 1
        
    if game.free_cells[0] != top_card_t0:
        print(f"Error: Card at Free Cell 0 should be {top_card_t0}, got {game.free_cells[0]}", file=sys.stderr)
        return 1
        
    if len(game.tableaus[0]) != 6:
        print(f"Error: Expected Tableau 0 size to be 6, got {len(game.tableaus[0])}", file=sys.stderr)
        return 1
        
    print("Single card move to Free Cell validated successfully.")

    # 4. Verify invalid move is rejected
    next_card_t0 = game.tableaus[0][-1]
    print(f"Attempting illegal move: placing {next_card_t0} into already occupied Free Cell 0...")
    success = game.move_card_to_free_cell('tableau', 0, 0)
    if success:
        print("Error: Moving to an occupied Free Cell was incorrectly allowed!", file=sys.stderr)
        return 1
    print("Illegal move correctly rejected.")

    # 5. Verify undo functionality works
    print("Testing Undo functionality...")
    undo_success = game.undo()
    if not undo_success:
        print("Error: Undo failed", file=sys.stderr)
        return 1
        
    if game.free_cells[0] is not None:
        print(f"Error: Expected Free Cell 0 to be empty after undo, got {game.free_cells[0]}", file=sys.stderr)
        return 1
        
    if len(game.tableaus[0]) != 7 or game.tableaus[0][-1] != top_card_t0:
        print("Error: Tableau 0 was not properly restored after undo", file=sys.stderr)
        return 1
        
    print("Undo functionality validated successfully.")

    # 6. Verify valid moves check works
    print("Verifying if state check for available moves is active...")
    if not game.has_any_valid_moves():
        print("Error: Game has valid moves but has_any_valid_moves() returned False", file=sys.stderr)
        return 1
    print("Valid moves check is working.")

    print("\nAll Smoke tests completed successfully! Outcome: Succeeded.")
    return 0

def main():
    if len(sys.argv) > 1 and sys.argv[1] == '--smoke':
        sys.exit(run_smoke_test())
    
    # Otherwise run the interactive curses UI game
    game = FreeCellGame()
    ui = FreeCellUI(game)
    ui.run()

if __name__ == '__main__':
    main()
