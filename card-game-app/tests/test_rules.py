from card_game_tui.engine import GameState, Move, validate_move, Card, Rank, Suit

def test_validate_move_initial_illegal():
    state = GameState()
    state.deal(seed=42)
    # Moving card from empty FreeCell is illegal
    move = Move('F', 0, 'C', 0, 1)
    valid, reason = validate_move(state, move)
    assert not valid
    assert "Source is empty or invalid" in reason
