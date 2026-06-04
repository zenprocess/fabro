# Detailed Implementation Spec: Terminal-Based FreeCell Solitaire in Python

This document defines the complete technical specifications, software architecture, game rules, and interface design for a terminal-based FreeCell Solitaire game implemented in Python 3. The game relies on the standard library `curses` module for interactive rendering and must support a headless `--smoke` mode for CI/CD test automation.

---

## 1. Overview and Game Rules

FreeCell is a solitaire card game played with a single standard 52-card deck. Unlike most other solitaire games, almost all deals are solvable because all cards are dealt face-up from the start.

### The Board Layout
The board consists of three main areas:
1. **The Tableau**: 8 columns.
   - Columns 1–4 are dealt 7 cards each.
   - Columns 5–8 are dealt 6 cards each.
   - All cards are face-up and overlap vertically so that only their values/suits are visible, except for the bottom-most card which is fully exposed.
2. **The FreeCells (Open Cells)**: 4 temporary storage slots.
   - Each slot can hold at most one card of any rank or suit.
   - Cards in FreeCells can be moved back to the Tableau or to the Foundations.
3. **The Foundations**: 4 piles, one for each suit (Spades ♠, Hearts ♥, Diamonds ♦, Clubs ♣).
   - These are built up in ascending order by suit, starting from Ace (A) and ending at King (K).
   - Once a card is placed in a Foundation pile, it is generally kept there (or can optionally be pulled back if required, though typical play is unidirectional).

### Valid Moves
* **To FreeCell**: Any single exposed card (the bottom-most card of a Tableau column or a card in another FreeCell) can be moved to an empty FreeCell.
* **To Tableau (Single Card)**:
  - An exposed card can be placed on top of the bottom-most card of any Tableau column if the target card has a rank exactly 1 higher and is of the **opposite color** (Red vs. Black).
  - Any single exposed card can be placed in an empty Tableau column.
* **To Foundation**:
  - An Ace of any suit can be moved to an empty Foundation pile.
  - A card can be placed on a Foundation pile if it matches the pile's suit and has a rank exactly 1 higher than the top card of that pile (e.g., a 4♦ on a 3♦).
* **Tableau to Tableau (Multi-card Sequence or "Supermove")**:
  - A valid sequence (ordered descending by rank and alternating in color, e.g., 8♥, 7♠, 6♦) can be moved together from one Tableau column to another.
  - The maximum size $M$ of a sequence that can be moved depends on the number of empty FreeCells ($F$) and empty Tableau columns ($T$):
    $$M = (1 + F) \times 2^T$$
    *Note: If the destination Tableau column is empty, it does not count as "empty" in the exponent $T$ of the formula, because it is the target of the move.*

### Win and Loss Conditions
* **Win**: All 52 cards are successfully placed in the 4 Foundation piles (each containing 13 cards from Ace to King).
* **Loss / Stalemate**: No legal moves are possible between the Tableau, FreeCells, and Foundations.

---

## 2. Core Data Structures (The Headless Engine)

To guarantee that the game logic is 100% testable without `curses` or any active terminal context, all game rules, cards, and board state must be encapsulated in a pure Python engine.

### Enums and Basic Types

```python
from enum import Enum, auto
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
```

### The `Card` Class

```python
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
```

### Pile Types and Identifiers

Move commands will refer to specific regions of the board. We define a standard naming schema for sources and destinations:
- **FreeCells**: `F1`, `F2`, `F3`, `F4` (or 0-indexed integer indices `0` to `3` mapped to `F`)
- **Foundations**: `A1`, `A2`, `A3`, `A4` (or `Foundation_Spades`, etc., mapped to `A`)
- **Tableau Columns**: `C1` to `C8` (or 0-indexed `0` to `7` mapped to `C`)

```python
class Move(NamedTuple):
    src_type: str  # 'C' (Tableau), 'F' (Freecell)
    src_idx: int   # 0-indexed
    dst_type: str  # 'C' (Tableau), 'F' (Freecell), 'A' (Foundation)
    dst_idx: int   # 0-indexed
    card_count: int = 1  # For sequence moves
```

### GameState Class

```python
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
```

---

## 3. Terminal UI Layout (using Curses)

The UI will be formatted to fit standard terminal windows (minimum requirement: 80 columns by 24 lines).

### ASCII Layout Blueprint

```text
======================= TERMINAL FREECELL =======================
Moves: 12      Time: 01:45                          [H]elp  [Q]uit

 [ F1 ]  [ F2 ]  [ F3 ]  [ F4 ]        [ ♠ ]   [ ♥ ]   [ ♦ ]   [ ♣ ]
 [ A♠ ]  [ -- ]  [ -- ]  [ -- ]        [ A♠ ]  [ -- ]  [ -- ]  [ -- ]
 
   1       2       3       4             5       6       7       8
=================================================================
  C1      C2      C3      C4      C5      C6      C7      C8
  K♠      10♦     5♣      8♥      A♦      7♣      Q♠      J♥
  Q♦      9♣      4♦      7♠      J♣      6♦      10♥     
  J♣      8♦              6♦              5♠      
  10♥                                             
  
=================================================================
Command: Move from (e.g. C1): _
[Error/Notification Bar: Invalid move! Red Jack cannot go on Red Queen.]
```

### Layout Sections
1. **Header Bar**: Displays current move count, elapsed timer, game state notifications, and quick keys (`H` for Help, `Q` for Quit, `U` for Undo, `R` for Reset, `N` for New Game).
2. **Top Deck Row**:
   - **FreeCells (Left)**: 4 slots showing empty state `[ -- ]` or the card string `[ Q♦ ]`.
   - **Foundations (Right)**: 4 slots showing the suit symbol for empty stacks `[ ♠ ]`, or the top-most card of the stack `[ K♠ ]`.
3. **Tableau Columns Row**:
   - Headers: `C1` to `C8`.
   - Vertical stacks. Overlapping cards are printed downward.
   - Selected columns or selected cards must be highlighted in reverse-video or marked with a cursor indicator (e.g., `>` prefix).
4. **Interactive Command/Status Row**:
   - Prompts the user for action input if using key sequences.
   - Highlights error states with high-contrast text.

---

## 4. Input Handling, Navigation, and Commands

To accommodate varying terminal capabilities and user preferences, the program will support **Command Sequences** as its primary interface.

### Keyboard Command Input

The prompt `Command:` accepts simple character-based coordinates to declare source and destination.

1. **Selecting Source & Destination**:
   - Keys `1` to `8` map directly to Tableau Columns `C1` to `C8`.
   - Keys `q`, `w`, `e`, `r` map directly to FreeCells `F1`, `F2`, `F3`, `F4`.
   - Keys `a`, `s`, `d`, `f` map directly to Foundations `A1`, `A2`, `A3`, `A4` (or users can just use `a` for automatic best-fit foundation routing).
   
   *Example Gameplay Interaction*:
   - Pressing `1` selects Column 1. The top card is highlighted.
   - Pressing `q` immediately moves that card to FreeCell 1 (if empty).
   - Pressing `2` followed by `3` moves the bottom card (or valid sequence) of Column 2 to Column 3.

2. **Command Actions**:
   - `u` / `U`: Undo last move.
   - `r` / `R`: Redo undone move.
   - `n` / `N`: Deal a completely new game.
   - `s` / `S`: Restart current game (using same shuffle seed).
   - `h` / `H`: Toggle help overlay showing game controls and rules.
   - `esc`: Cancel current selection.
   - `q` / `Q`: Exit game.

### Cursor-Based (Optional Secondary Interface)
If arrow keys/WASD navigation is enabled:
- Arrow keys move a highlighted cursor box across Columns 1-8, Freecells, and Foundations.
- `Space` / `Enter` selects the active stack.
- Pressing `Space` / `Enter` on another stack completes the move.

---

## 5. Move and Action Validation

The rules engine must rigidly enforce FreeCell constraints.

### Validation Logic Flow Chart (Engine level)

```python
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
```

### Auto-Home (Quality of Life)
To minimize repetitive actions, the engine automatically checks if any exposed cards can be safely moved to foundations.
A card of rank $R$ and suit $S$ can be safely moved to its foundation if:
1. It is a legal foundation move.
2. All cards of rank $R-1$ of the **opposite color** are already in the foundation piles.
3. All cards of rank $R-2$ of the **same color** are already in the foundation piles.

*Why?* This ensures that no remaining card in the Tableau can possibly require this card as a sequence parent (since any card that could pair with it is already safely homed).

---

## 6. Smoke Mode Implementation

To support automated test execution and code validation in continuous integration (CI) pipelines, the program must implement a **headless smoke mode**.

### Invocation
```bash
python3 main.py --smoke
```

### Headless execution logic
1. Parse CLI arguments. If `--smoke` is present:
   - **Do not** initialize `curses` or modify the terminal buffer.
   - Instantiate the `GameState` engine.
   - Seed the random generator with a fixed value (e.g., `seed=42`) to guarantee a deterministic state.
   - Run `state.deal(seed=42)`.
   - Verify that:
     - 52 cards are distributed.
     - Tableau columns have size `[7, 7, 7, 7, 6, 6, 6, 6]`.
     - FreeCells are empty.
     - Foundation piles are empty.
   - Run a sequence of mock moves (e.g., attempt to move a valid card if possible, perform undos, verify validation logic handles both valid and invalid moves).
   - Cleanly exit with return code `0` on success, or return code `1` (or throw exceptions) if assertions fail.

```python
# Sketch of main entry-point logic
import sys

def main():
    if "--smoke" in sys.argv:
        print("Running headless smoke tests...")
        try:
            state = GameState()
            state.deal(seed=12345)
            
            # Assert initial setup
            assert sum(len(col) for col in state.tableau) == 52, "Tableau must contain 52 cards"
            assert len(state.tableau[0]) == 7, "Column 1 must have 7 cards"
            assert len(state.tableau[7]) == 6, "Column 8 must have 6 cards"
            assert all(fc is None for fc in state.free_cells), "Freecells must start empty"
            
            # Verify move validation logic doesn't crash
            # e.g., attempt an illegal move and ensure it gets rejected
            invalid_move = Move('C', 0, 'C', 1, 1)
            valid, reason = validate_move(state, invalid_move)
            
            print("Smoke tests passed successfully.")
            sys.exit(0)
        except Exception as e:
            print(f"Smoke test failed: {e}", file=sys.stderr)
            sys.exit(1)
    else:
        # Run standard interactive curses application
        import curses
        curses.wrapper(run_curses_app)
```

---

## 7. Test Strategy

Comprehensive automated tests must validate the engine rules independently of rendering.

### Test Architecture Blueprint

```text
tests/
├── __init__.py
├── test_card.py         # Card attributes, comparison, color, and suitability checks
├── test_rules.py        # Single-move checks, sequence checking, supermove limit calculation
├── test_state.py        # Deal correctness, seed-determinism, Undo/Redo stack preservation
└── test_integration.py  # Simulation of a short deterministic game sequence to verification of Win/Loss
```

### Key Test Categories and Mock Scenarios
1. **The Alternating Descending Rule**:
   - Try placing Red Jack on Black Queen (Valid).
   - Try placing Red Jack on Red Queen (Invalid).
   - Try placing Red Jack on Black 10 (Invalid).
2. **The Supermove Formula Verification**:
   - Setup state with $F$ empty Freecells and $T$ empty Tableau columns.
   - Assert `get_max_movable_cards(state)` yields exactly the mathematical output of $(1 + F) \times 2^T$.
   - Assert that trying to move a sequence of length $M+1$ gets rejected with a helpful warning message.
3. **Undo/Redo Integrity**:
   - Execute a series of moves.
   - Store historical snapshots.
   - Assert calling `undo()` restores card values, suits, and counts perfectly.
   - Assert calling `redo()` reapplies changes perfectly.
4. **Win State Execution**:
   - Artificially mock all foundation stacks to contain Aces through Queens.
   - Make the final 4 moves placing Kings.
   - Verify `state.is_won()` is triggered exactly on the final King placement.
