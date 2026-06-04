from enum import Enum
from typing import List, Optional, Dict, Tuple, NamedTuple
import random

class Suit(Enum):
    SPADES = "♠"
    HEARTS = "♥"
    DIAMONDS = "♦"
    CLUBS = "♣"

    @property
    def color(self) -> str:
        if self in (Suit.HEARTS, Suit.DIAMONDS):
            return "RED"
        return "BLACK"

class Rank(Enum):
    ACE = 1
    TWO = 2
    THREE = 3
    FOUR = 4
    FIVE = 5
    SIX = 6
    SEVEN = 7
    EIGHT = 8
    NINE = 9
    TEN = 10
    JACK = 11
    QUEEN = 12
    KING = 13

    @property
    def symbol(self) -> str:
        mapping = {
            Rank.ACE: "A",
            Rank.JACK: "J",
            Rank.QUEEN: "Q",
            Rank.KING: "K"
        }
        return mapping.get(self, str(self.value))

class Card:
    def __init__(self, rank: Rank, suit: Suit):
        self.rank: Rank = rank
        self.suit: Suit = suit

    @property
    def color(self) -> str:
        return self.suit.color

    def is_opposite_color(self, other: "Card") -> bool:
        return self.color != other.color

    def can_be_placed_on_tableau(self, other: "Card") -> bool:
        """Checks if self can be placed on other (which is on top of a Tableau column)."""
        return self.is_opposite_color(other) and self.rank.value == other.rank.value - 1

    def __repr__(self) -> str:
        return f"{self.rank.symbol}{self.suit.value}"

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Card):
            return NotImplemented
        return self.rank == other.rank and self.suit == other.suit

class Move(NamedTuple):
    src_type: str  # 'C' (Tableau), 'F' (Freecell)
    src_idx: int   # 0-indexed
    dst_type: str  # 'C' (Tableau), 'F' (Freecell), 'A' (Foundation)
    dst_idx: int   # 0-indexed
    card_count: int = 1  # For sequence moves

class GameState:
    def __init__(self):
        self.tableau: List[List[Card]] = [[] for _ in range(8)]
        self.free_cells: List[Optional[Card]] = [None] * 4
        self.foundations: Dict[Suit, List[Card]] = {
            Suit.SPADES: [],
            Suit.HEARTS: [],
            Suit.DIAMONDS: [],
            Suit.CLUBS: []
        }
        self.history: List[Tuple[List[List[Card]], List[Optional[Card]], Dict[Suit, List[Card]]]] = []
        self.redo_history: List[Tuple[List[List[Card]], List[Optional[Card]], Dict[Suit, List[Card]]]] = []

    def deal(self, seed: Optional[int] = None) -> None:
        """Generates, shuffles, and distributes a standard 52-card deck."""
        deck = [Card(rank, suit) for suit in Suit for rank in Rank]
        if seed is not None:
            random.seed(seed)
        else:
            random.seed()
        random.shuffle(deck)

        self.tableau = [[] for _ in range(8)]
        self.free_cells = [None] * 4
        self.foundations = {s: [] for s in Suit}
        self.history.clear()
        self.redo_history.clear()

        # Deal cards: 7 to columns 0-3, 6 to columns 4-7
        for idx, card in enumerate(deck):
            col = idx % 8
            self.tableau[col].append(card)

    def save_state(self) -> Tuple[List[List[Card]], List[Optional[Card]], Dict[Suit, List[Card]]]:
        """Creates a deep copy of current piles to push to history."""
        tableau_copy = [col.copy() for col in self.tableau]
        free_cells_copy = list(self.free_cells)
        foundations_copy = {suit: pile.copy() for suit, pile in self.foundations.items()}
        return (tableau_copy, free_cells_copy, foundations_copy)

    def restore_state(self, state_tuple: Tuple[List[List[Card]], List[Optional[Card]], Dict[Suit, List[Card]]]) -> None:
        self.tableau, self.free_cells, self.foundations = state_tuple

    def push_history(self) -> None:
        self.history.append(self.save_state())
        self.redo_history.clear()

    def undo(self) -> bool:
        if not self.history:
            return False
        self.redo_history.append(self.save_state())
        self.restore_state(self.history.pop())
        return True

    def redo(self) -> bool:
        if not self.redo_history:
            return False
        self.history.append(self.save_state())
        self.restore_state(self.redo_history.pop())
        return True

def get_source_cards(state: GameState, src_type: str, src_idx: int, card_count: int) -> List[Card]:
    if src_type == 'C':
        col = state.tableau[src_idx]
        if len(col) < card_count:
            return []
        return col[-card_count:]
    elif src_type == 'F':
        if card_count != 1:
            return []
        card = state.free_cells[src_idx]
        return [card] if card is not None else []
    return []

def is_valid_sequence(cards: List[Card]) -> bool:
    if not cards:
        return False
    for i in range(len(cards) - 1):
        curr = cards[i]
        nxt = cards[i + 1]
        if not nxt.can_be_placed_on_tableau(curr):
            return False
    return True

def get_max_movable_cards(state: GameState, target_is_empty_col: bool) -> int:
    F = sum(1 for fc in state.free_cells if fc is None)
    T = sum(1 for col in state.tableau if not col)
    if target_is_empty_col and T > 0:
        T -= 1
    return (1 + F) * (2 ** T)

def validate_move(state: GameState, move: Move) -> Tuple[bool, str]:
    """
    Returns (True, "") if the move is legal, or (False, "reason") if illegal.
    """
    # 1. Fetch source card(s)
    src_cards = get_source_cards(state, move.src_type, move.src_idx, move.card_count)
    if not src_cards:
        return False, "Source is empty or invalid."

    # 2. If moving multiple cards, verify they form a valid alternating descending sequence
    if len(src_cards) > 1:
        if not is_valid_sequence(src_cards):
            return False, "Selected cards do not form a valid alternating color descending sequence."

    # 3. Validate Destination
    if move.dst_type == 'F':  # Destination is FreeCell
        if move.card_count > 1:
            return False, "Cannot move a sequence to a FreeCell."
        if state.free_cells[move.dst_idx] is not None:
            return False, "Target FreeCell is occupied."

    elif move.dst_type == 'A':  # Destination is Foundation
        if move.card_count > 1:
            return False, "Cannot move a sequence to a Foundation."
        card = src_cards[0]
        f_pile = state.foundations[card.suit]
        if not f_pile:
            if card.rank != Rank.ACE:
                return False, "Foundations must start with an Ace."
        else:
            top_card = f_pile[-1]
            if card.rank.value != top_card.rank.value + 1:
                return False, f"Cannot place {card} on {top_card}. Must be next rank up."

    elif move.dst_type == 'C':  # Destination is Tableau
        dest_col = state.tableau[move.dst_idx]
        first_src_card = src_cards[0]  # The highest rank card in the sequence being moved

        if not dest_col:
            # Moving sequence/card to empty tableau column
            # Verify supermove capacity limit
            max_allowed = get_max_movable_cards(state, target_is_empty_col=True)
            if len(src_cards) > max_allowed:
                return False, f"Insufficient empty FreeCells/Columns to move {len(src_cards)} cards (Max: {max_allowed})."
        else:
            dest_card = dest_col[-1]
            if not first_src_card.can_be_placed_on_tableau(dest_card):
                return False, f"Cannot place {first_src_card} on {dest_card}. Must be alternating color and rank-1."
            # Verify supermove capacity limit
            max_allowed = get_max_movable_cards(state, target_is_empty_col=False)
            if len(src_cards) > max_allowed:
                return False, f"Insufficient empty FreeCells/Columns to move {len(src_cards)} cards (Max: {max_allowed})."

    return True, ""
