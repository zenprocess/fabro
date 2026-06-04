from card_game_tui.domain import Card, Suit, Rank
from card_game_tui.render import get_card_inner, get_empty_foundation_inner, get_card_lines

def test_get_card_inner():
    # Test single-char rank label
    card_ah = Card(Suit.HEARTS, Rank.ACE)
    assert get_card_inner(card_ah) == " A♥"

    # Test double-char rank label
    card_10s = Card(Suit.SPADES, Rank.TEN)
    assert get_card_inner(card_10s) == "10♠"

    # Test face card
    card_kd = Card(Suit.DIAMONDS, Rank.KING)
    assert get_card_inner(card_kd) == " K♦"

def test_get_empty_foundation_inner():
    assert get_empty_foundation_inner(0) == " ♥ "
    assert get_empty_foundation_inner(1) == " ♦ "
    assert get_empty_foundation_inner(2) == " ♣ "
    assert get_empty_foundation_inner(3) == " ♠ "
    assert get_empty_foundation_inner(4) == "   "

def test_get_card_lines():
    card_ah = Card(Suit.HEARTS, Rank.ACE)
    assert get_card_lines(card_ah) == ["+---+", "| A♥|", "+---+"]
    assert get_card_lines(None) == ["+---+", "|   |", "+---+"]
    assert get_card_lines(None, is_empty=True, empty_suit_idx=0) == ["+---+", "| ♥ |", "+---+"]
    assert get_card_lines(None, is_empty=True, empty_suit_idx=-1) == ["+---+", "|   |", "+---+"]
