from card_game_tui.engine import GameState

def test_integration_setup():
    state = GameState()
    state.deal(seed=123)
    assert sum(len(col) for col in state.tableau) == 52
