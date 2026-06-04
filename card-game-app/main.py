#!/usr/bin/env python3
import sys
import os

# Ensure the src/ directory is in the import path
src_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "src")
if src_path not in sys.path:
    sys.path.insert(0, src_path)

from card_game_tui.game import GameState, Card
from card_game_tui.ui import play_game

def print_ascii_state(state):
    print("\n" + "=" * 60)
    print("FREECELLS: ", end="")
    for i, card in enumerate(state.freecells):
        val = repr(card) if card else "[   ]"
        print(f"F{i+1}:{val} ", end="")
    print("\nFOUNDATIONS: ", end="")
    for suit in Card.SUITS:
        symbol = Card.SUIT_SYMBOLS[suit]
        rank = state.foundations[suit]
        val = f"{Card.RANK_SYMBOLS[rank]}{symbol}" if rank > 0 else f"[ {symbol} ]"
        print(f"{suit}:{val} ", end="")
    print("\n" + "-" * 60)
    print("TABLEAUX:")
    max_len = max(len(col) for col in state.tableaux)
    for row in range(max_len):
        row_str = []
        for col_idx in range(8):
            col = state.tableaux[col_idx]
            if row < len(col):
                row_str.append(f"{repr(col[row]):<6}")
            else:
                row_str.append("      ")
        print("  ".join(row_str))
    print("=" * 60)

def run_smoke_test():
    print("Starting non-interactive FreeCell Solitaire smoke test...")
    state = GameState()
    
    # Deal with deterministic seed
    print("\nStep 1: Dealing game with seed 123...")
    state.deal(seed=123)
    print_ascii_state(state)
    
    # Assert initial sizes
    assert len(state.tableaux[0]) == 7, "Tableau 1 should have 7 cards"
    assert len(state.tableaux[7]) == 6, "Tableau 8 should have 6 cards"
    
    # Run auto-collect
    print("\nStep 2: Running auto-collect...")
    collected = state.auto_collect()
    print(f"Auto-collected {collected} card(s).")
    print_ascii_state(state)
    
    # Assert A♥ went to foundation
    assert state.foundations['H'] == 1, "Hearts foundation should be 1 (Ace)"
    assert len(state.tableaux[0]) == 6, "Tableau 1 should now have 6 cards"
    
    # Move bottom card of Tableau 1 (3♠) to FreeCell 1 (index 0)
    print("\nStep 3: Moving bottom card of Tableau 1 (3♠) to FreeCell 1...")
    success, msg = state.move_card_to_freecell('tableau', 0, 0)
    print(f"Result: {success} - {msg}")
    assert success, "Move to FreeCell should succeed"
    assert state.freecells[0] == Card('S', 3), "Freecell 1 should contain 3♠"
    print_ascii_state(state)
    
    # Move bottom card of Tableau 4 (4♣) to Tableau 3 (places on 5♦)
    print("\nStep 4: Moving bottom card of Tableau 4 (4♣) to Tableau 3 (places on 5♦)...")
    success, msg = state.move_card_to_tableau('tableau', 3, 2)
    print(f"Result: {success} - {msg}")
    assert success, "Move to Tableau should succeed"
    assert state.tableaux[2][-1] == Card('C', 4), "Tableau 3 bottom should be 4♣"
    print_ascii_state(state)
    
    # Move bottom card of Tableau 4 (9♥) to FreeCell 2 (index 1)
    print("\nStep 5: Moving bottom card of Tableau 4 (9♥) to FreeCell 2...")
    success, msg = state.move_card_to_freecell('tableau', 3, 1)
    print(f"Result: {success} - {msg}")
    assert success, "Move to FreeCell should succeed"
    print_ascii_state(state)
    
    # Move bottom card of Tableau 4 (4♠) to FreeCell 3 (index 2)
    print("\nStep 6: Moving bottom card of Tableau 4 (4♠) to FreeCell 3...")
    success, msg = state.move_card_to_freecell('tableau', 3, 2)
    print(f"Result: {success} - {msg}")
    assert success, "Move to FreeCell should succeed"
    print_ascii_state(state)
    
    # Move bottom card of Tableau 4 (7♠) to FreeCell 4 (index 3)
    print("\nStep 7: Moving bottom card of Tableau 4 (7♠) to FreeCell 4...")
    success, msg = state.move_card_to_freecell('tableau', 3, 3)
    print(f"Result: {success} - {msg}")
    assert success, "Move to FreeCell should succeed"
    print_ascii_state(state)
    
    # Try to move another card to FreeCell 4 (should fail - fully occupied)
    print("\nStep 8: Trying to move bottom card of Tableau 1 (K♥) to FreeCell 4 (expecting failure)...")
    success, msg = state.move_card_to_freecell('tableau', 0, 3)
    print(f"Result: {success} - {msg}")
    assert not success, "Should fail as FreeCell 4 is occupied"
    
    # Test Undo
    print("\nStep 9: Testing Undo...")
    success = state.undo()
    print(f"Undo result: {success}")
    assert success, "Undo should succeed"
    assert state.freecells[3] is None, "FreeCell 4 should be empty after undo"
    assert state.tableaux[3][-1] == Card('S', 7), "7♠ should be back in Tableau 4"
    print_ascii_state(state)

    print("\nSmoke test PASSED successfully!")
    sys.exit(0)

if __name__ == "__main__":
    if "--smoke" in sys.argv:
        run_smoke_test()
    else:
        # Run normal interactive curses game
        try:
            play_game()
        except Exception as e:
            print(f"An error occurred: {e}", file=sys.stderr)
            sys.exit(1)
