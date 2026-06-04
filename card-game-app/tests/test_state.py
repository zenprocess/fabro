from card_game_tui.engine import GameState

def test_deal():
    state = GameState()
    state.deal(seed=42)
    assert sum(len(col) for col in state.tableau) == 52
    assert len(state.tableau[0]) == 7
    assert len(state.tableau[7]) == 6
    assert all(fc is None for fc in state.free_cells)

def test_undo_redo():
    state = GameState()
    state.deal(seed=42)
    
    # Push history
    state.push_history()
    
    # Modify state
    card = state.tableau[0].pop()
    state.free_cells[0] = card
    
    # Undo
    assert state.undo()
    assert state.free_cells[0] is None
    assert len(state.tableau[0]) == 7
    
    # Redo
    assert state.redo()
    assert state.free_cells[0] == card
    assert len(state.tableau[0]) == 6
