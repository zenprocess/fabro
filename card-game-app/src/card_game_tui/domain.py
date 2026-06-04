import random
from dataclasses import dataclass
from enum import Enum, auto
from typing import List, Union, Dict

class Suit(Enum):
    HEARTS = "H"
    DIAMONDS = "D"
    CLUBS = "C"
    SPADES = "S"

    @property
    def color(self) -> str:
        return "RED" if self in (Suit.HEARTS, Suit.DIAMONDS) else "BLACK"

    @property
    def symbol(self) -> str:
        return {
            Suit.HEARTS: "♥",
            Suit.DIAMONDS: "♦",
            Suit.CLUBS: "♣",
            Suit.SPADES: "♠"
        }[self]

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
    def label(self) -> str:
        if self.value == 1: return "A"
        if self.value == 11: return "J"
        if self.value == 12: return "Q"
        if self.value == 13: return "K"
        return str(self.value)

@dataclass(frozen=True)
class Card:
    suit: Suit
    rank: Rank

    def __repr__(self) -> str:
        return f"{self.rank.label}{self.suit.symbol}"

class Deck:
    def __init__(self, seed: int = None):
        self.cards = [Card(suit, rank) for suit in Suit for rank in Rank]
        if seed is not None:
            random.seed(seed)
        random.shuffle(self.cards)

    def deal(self) -> List[List[Card]]:
        """Deals 52 cards into 8 columns."""
        tableaus: List[List[Card]] = [[] for _ in range(8)]
        for i, card in enumerate(self.cards):
            tableaus[i % 8].append(card)
        return tableaus

class LocationType(Enum):
    TABLEAU = auto()
    FREECELL = auto()
    FOUNDATION = auto()

@dataclass(frozen=True)
class Position:
    type: LocationType
    index: int  # 0-7 for Tableau, 0-3 for FreeCell, 0-3 for Foundation

@dataclass
class MoveRecord:
    from_pos: Position
    to_pos: Position
    cards: List[Card]  # Captured for single or sequence moves
    auto_moves: List['MoveRecord'] = None  # Nested moves triggered by auto-homing

class GameState:
    def __init__(self, seed: int = None):
        self.tableaus: List[List[Card]] = Deck(seed).deal()
        self.freecells: List[Union[Card, None]] = [None] * 4
        self.foundations: Dict[Suit, List[Card]] = {suit: [] for suit in Suit}
        self.undo_stack: List[MoveRecord] = []
        self.redo_stack: List[MoveRecord] = []

    def get_card_at(self, position: Position) -> Union[Card, None]:
        if position.type == LocationType.FREECELL:
            return self.freecells[position.index]
        elif position.type == LocationType.FOUNDATION:
            pile = self.foundations[list(Suit)[position.index]]
            return pile[-1] if pile else None
        elif position.type == LocationType.TABLEAU:
            col = self.tableaus[position.index]
            return col[-1] if col else None
        return None

    def validate_move(self, from_pos: Position, to_pos: Position, count: int = 1) -> bool:
        """
        Calculates whether moving 'count' cards from from_pos to to_pos is legal.
        """
        # Placeholder for validation
        return True

    def execute_move(self, from_pos: Position, to_pos: Position, count: int = 1) -> bool:
        """
        Executes a move, saves it to the undo stack, runs auto-homing, and clears the redo stack.
        """
        if not self.validate_move(from_pos, to_pos, count):
            return False
        return True

    def undo(self) -> bool:
        """Reverts the last move, including nested auto-homing steps."""
        if not self.undo_stack:
            return False
        return True

    def check_win(self) -> bool:
        """Returns True if all 52 cards are in the Foundations."""
        return all(len(self.foundations[suit]) == 13 for suit in Suit)
