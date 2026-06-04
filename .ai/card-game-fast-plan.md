# Klondike Solitaire - Implementation Plan

This document outlines the concise plan for implementing a terminal-based Klondike Solitaire game in Python under `card-game-app/`.

---

## 1. Game Rules & Data Structures

We will implement standard **Klondike Solitaire** (Draw 1 variation).

### Data Models (`card_game_app/game.py` or similar)
- **Suit (Enum)**: `HEARTS` (Red), `DIAMONDS` (Red), `CLUBS` (Black), `SPADES` (Black).
- **Rank (Enum or Int)**: 1 (Ace) to 13 (King).
- **Card**:
  - `suit`: Suit
  - `rank`: int
  - `face_up`: bool
  - Methods: `color()` -> `'red' | 'black'`, `__repr__()` -> e.g. `"10♦"` or `"A♠"`.
- **Pile Types**:
  - `Stock`: Draw pile (face-down cards).
  - `Waste`: Discard/reveal pile (face-up cards).
  - `Foundation`: 4 piles, build up by suit from Ace to King.
  - `Tableau`: 7 piles, build down by alternating color.
- **GameState**:
  - `stock`: List[Card]
  - `waste`: List[Card]
  - `foundations`: Dict[Suit, List[Card]] (or list of 4 piles)
  - `tableaus`: List[List[Card]] (7 piles)
  - `selected_pile`: Optional index/reference
  - `selected_card_idx`: Optional index in that pile for stack moves.

---

## 2. Terminal Rendering with `curses`

The UI will be drawn using the standard Python `curses` module.

### Core UI Components
- **Layout**:
  - **Top Row**: Stock, Waste, spacing, and 4 Foundations.
  - **Bottom Row**: 7 Tableau columns displaying cards stacked vertically.
  - **Footer**: Keybindings help text, move status messages, or win/loss announcements.
- **Rendering Elements**:
  - Face-down cards drawn as `[ █ ]` (blue/cyan background block or custom pattern).
  - Face-up cards drawn with colored suit symbols: red for `♥`/`♦` and white/default for `♠`/`♣`.
  - Selected cards/piles highlighted using reverse-video or blinking attribute (`curses.A_REVERSE`).
  - Active cursor highlighted in yellow/cyan.
- **Color Pairs Setup**:
  - Pair 1: Red text on Black (Hearts/Diamonds)
  - Pair 2: White text on Black (Spades/Clubs)
  - Pair 3: Blue/Cyan on Black (Face-down card back / Board outlines)
  - Pair 4: Yellow/Black (Cursor / Help text)

---

## 3. Input Handling & Navigation

We will implement a standard **Cursor-Based Selection System**:
- **Movement**:
  - Arrow keys or `WASD` to move the cursor between the different piles (Stock, Waste, Foundations 1-4, Tableaus 1-7).
  - When on a Tableau, `Up`/`Down` keys navigate the cursor up/down the face-up cards to select a partial stack to move.
- **Interaction**:
  - `Space` / `Enter`:
    - On **Stock**: Draws a card to Waste. If Stock is empty, recycles Waste back to Stock.
    - On **other piles**:
      - If no pile is currently selected, selects the pile (and the card under the cursor).
      - If a pile *is* selected, attempts to move the selected card/stack to the target pile.
  - `Esc` / `c`: Cancels current selection.
  - `q`: Quits the game.
  - `r`: Restarts/re-shuffles.
- **Move Validation**:
  - Moving to **Foundation**: Only a single card. Must be Ace on empty foundation, or rank+1 of the same suit.
  - Moving to **Tableau**: Can be a stack. Bottom card of the moving stack must be opposite color and rank-1 of the target Tableau's top card. If target Tableau is empty, only a King can be placed.
  - Auto-flip: If a move leaves a Tableau with a face-down card on top, it is automatically flipped face-up.

---

## 4. Win/Loss Detection

- **Win Condition**: All 4 foundations contain 13 cards (total 52 cards).
- **Loss / No Moves Condition**: Game doesn't force end on lock (since cards can be moved back and forth), but can show a hint if no moves are possible.
- When won, show a beautiful success overlay and prompt for restart.

---

## 5. Test Strategy & Smoke Verification

### Unit Tests
- Testing standard moves: Stock draw, Stock recycle, valid/invalid Tableau moves, valid/invalid Foundation moves.
- Testing auto-reveal of top Tableau cards.
- Testing complete win-state detection.

### Smoke Verification (`python3 main.py --smoke`)
- A non-interactive mode that initializes a seed-based game state, executes a valid move sequence, asserts the outcomes, and prints a success message before exiting with `0`. This allows automated verification in environments without a TTY or curses terminal.

---

## 6. Implementation File Structure

```
card-game-app/
├── main.py            # Entrypoint (CLI args, --smoke, starts curses)
├── game.py            # Core game logic engine
└── tests/
    └── test_game.py   # Complete test suite
```
