from typing import Union
from card_game_tui.domain import Card, Suit

def get_card_inner(card: Card) -> str:
    """
    Returns the exact 3-character representation of a card's rank and suit symbol.
    """
    label = card.rank.label
    symbol = card.suit.symbol
    return f"{label:>2}{symbol}"

def get_empty_foundation_inner(suit_idx: int) -> str:
    """
    Returns the suit symbol with padding for an empty foundation slot.
    """
    suits = list(Suit)
    if 0 <= suit_idx < len(suits):
        return f" {suits[suit_idx].symbol} "
    return "   "

def get_card_lines(card: Union[Card, None], is_empty: bool = False, empty_suit_idx: int = -1) -> list[str]:
    """
    Returns 3 lines representing a card or empty cell.
    """
    if is_empty:
        if empty_suit_idx >= 0:
            inner = get_empty_foundation_inner(empty_suit_idx)
        else:
            inner = "   "
        return [
            "+---+",
            f"|{inner}|",
            "+---+"
        ]
    else:
        if card is None:
            return [
                "+---+",
                "|   |",
                "+---+"
            ]
        inner = get_card_inner(card)
        return [
            "+---+",
            f"|{inner}|",
            "+---+"
        ]
