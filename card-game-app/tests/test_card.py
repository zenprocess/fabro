from card_game_tui.engine import Card, Rank, Suit

def test_card_properties():
    card = Card(Rank.ACE, Suit.SPADES)
    assert card.rank == Rank.ACE
    assert card.suit == Suit.SPADES
    assert card.color == "BLACK"
    assert repr(card) == "A♠"

def test_card_opposite_color():
    card_spades = Card(Rank.ACE, Suit.SPADES)
    card_hearts = Card(Rank.TWO, Suit.HEARTS)
    card_clubs = Card(Rank.THREE, Suit.CLUBS)
    
    assert card_spades.is_opposite_color(card_hearts)
    assert not card_spades.is_opposite_color(card_clubs)

def test_can_be_placed_on_tableau():
    card_red_jack = Card(Rank.JACK, Suit.HEARTS)
    card_black_queen = Card(Rank.QUEEN, Suit.SPADES)
    card_red_queen = Card(Rank.QUEEN, Suit.DIAMONDS)
    card_black_ten = Card(Rank.TEN, Suit.CLUBS)
    
    # Red Jack can be placed on Black Queen
    assert card_red_jack.can_be_placed_on_tableau(card_black_queen)
    # Red Jack cannot be placed on Red Queen (same color)
    assert not card_red_jack.can_be_placed_on_tableau(card_red_queen)
    # Red Jack cannot be placed on Black Ten (wrong rank order)
    assert not card_red_jack.can_be_placed_on_tableau(card_black_ten)
