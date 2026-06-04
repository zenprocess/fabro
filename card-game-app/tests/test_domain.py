from card_game_tui.domain import GameState, Suit, Rank, Card, Deck, LocationType, Position

def test_deck_deals_52_cards():
    deck = Deck()
    assert len(deck.cards) == 52

def test_game_state_initialization():
    state = GameState()
    assert len(state.tableaus) == 8
    # 4 columns should have 7 cards, 4 should have 6 cards
    lengths = [len(col) for col in state.tableaus]
    assert sorted(lengths) == [6, 6, 6, 6, 7, 7, 7, 7]

def test_get_card_at():
    state = GameState(seed=123)
    # Check if we can get cards correctly
    pos = Position(LocationType.TABLEAU, 0)
    card = state.get_card_at(pos)
    assert card is not None
    assert isinstance(card, Card)
