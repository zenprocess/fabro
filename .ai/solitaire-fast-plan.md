# Implementation Plan: Python Klondike Solitaire Curses TUI

This document outlines the concise plan to implement a terminal-based Klondike Solitaire game in Python using the standard library `curses` module, with full game logic, undo history, win detection, a non-interactive smoke test suite, and a unit test suite using `pytest`.

All files will reside under the directory `solitaire-app/`.

---

## 1. Directory Structure

```text
solitaire-app/
├── requirements.txt      # Project dependencies (pytest)
├── main.py               # Application entry point (handles --smoke and launches TUI)
├── game_logic.py         # Complete pure Python engine for card and game state management
├── tui.py                # Curses-based terminal interface layout, input handler, and renderer
└── tests/
    └── test_game_logic.py # Unit tests for game rules, moves, and state transitions
```

---

## 2. Technical Stack & Requirements

- **Language**: Python 3.11+
- **UI Library**: Standard library `curses` (fully playable TUI with color support, keyboard controls, and layout adaptability)
- **Testing**: `pytest` for rules engine verification
- **E2E / Demo Verification**: `python3 main.py --smoke` runs a non-interactive automated smoke test of the game logic and exits 0 on success.

---

## 3. Game Engine (game_logic.py)

The game engine will be completely decoupled from the UI layer to ensure reliable testing.

### Key Models

- **`Card`**:
  - `suit`: One of `♠`, `♥`, `♦`, `♣` (or string representation)
  - `rank`: Integer from 1 (Ace) to 13 (King)
  - `face_up`: Boolean
  - `color`: Derived property (Red for ♥/♦, Black for ♠/♣)

- **`GameState`**:
  - `stock`: List of face-down cards
  - `waste`: List of drawn cards (only the top card is visible and playable)
  - `tableau`: List of 7 columns, each being a list of `Card`s
  - `foundations`: Dict with 4 keys (suits/indices) pointing to lists of card sequences (A to K)
  - `undo_stack`: Stack of serialized or deep-copied previous states

### Core Operations & Rules (Draw-One Klondike)

1. **Initialization**:
   - Shuffle a standard 52-card deck.
   - Deal cards to the 7 tableau columns (Column $i$ gets $i$ cards; top card is face_up, others face_down).
   - Remaining cards go to the `stock` pile.
2. **Draw / Recycle**:
   - `draw_card()`: Move 1 card from `stock` to `waste` (face-up).
   - If `stock` is empty, recycle `waste` back to `stock` by reversing and flipping them face-down.
3. **Move Validation & Execution**:
   - **Tableau to Tableau**: A card (or face-up stack) can move to another column if the bottom-most card of the moving stack is 1 rank lower and of the opposite color of the target column's top card. An empty tableau column can only accept a King (rank 13).
   - **Waste to Tableau**: Top of waste can move to a tableau column following the same color/rank rules.
   - **Waste/Tableau to Foundation**: Cards can move to foundations. Foundations build up from Ace (1) to King (13) by same suit.
   - **Auto-Reveal**: If a move exposes a face-down card at the top of a tableau column, it is automatically flipped face-up.
4. **Undo**:
   - Push the full state to `undo_stack` before any mutating game action.
   - `undo()` pops from `undo_stack` and restores the game state.
5. **Win Detection**:
   - `check_win()` returns `True` when all 4 foundations contain 13 cards (or foundations total 52 cards).

---

## 4. TUI Layout & Interactions (tui.py)

The Curses TUI will draw a clean grid layout of the game board.

### Visual Representation

```text
  [Stock]    [Waste]                [F1]   [F2]   [F3]   [F4]
   [#]        [ ♦Q ]                 [ ]    [ ]    [ ]    [ ]

  [Col 1]   [Col 2]   [Col 3]   [Col 4]   [Col 5]   [Col 6]   [Col 7]
   ♠K        [#]       [#]       [#]       [#]       [#]       [#]
             ♥J        [#]       [#]       [#]       [#]       [#]
                       ♣10       [#]       [#]       [#]       [#]
                                 ♦9        [#]       [#]       [#]
                                           ♠8        [#]       [#]
                                                     ♥7        [#]
                                                               ♣6
```

### Color Setup
- Red cards (♥, ♦) drawn with red text foreground.
- Black cards (♠, ♣) drawn with black or white/blue text.
- Focus highlights (selected cards/piles) drawn with distinct background inversion or terminal style.

### Control Scheme

To keep the implementation simple, intuitive, and responsive, the TUI will support a cursor/selection-based movement system:
- **Arrow Keys** or **WASD**: Move cursor between Stock, Waste, Foundations (1-4), and Tableau Piles (1-7).
- **Space / Enter**: 
  - If cursor is on Stock: Draw card.
  - If cursor is on a valid card source (Tableau, Waste): Select card/stack.
  - If cursor is on a valid card destination (Tableau, Foundation): Move selected card/stack to this location.
- **'u' Key**: Undo last action.
- **'r' Key**: Restart a new game.
- **'q' Key**: Quit game.

---

## 5. Non-Interactive Demo Mode (`--smoke`)

In `main.py`, if `--smoke` argument is passed, the script will execute a headless simulation:
1. Initialize a deterministic or random game state.
2. Verify stock drawing and recycling logic.
3. Programmatically execute a valid move (e.g., set up a mock board with a valid move, run it, and assert the state change).
4. Verify `undo` reverts the mock state change.
5. Create a nearly complete foundation set, execute the final winning move, and assert that `check_win()` detects the win.
6. Print a clean summary of actions taken and exit with code 0 on success.

---

## 6. Testing (tests/test_game_logic.py)

We will use `pytest` to test rules and operations. Tests will include:
- `test_game_initialization`: Proper card counts, correct dealing distribution.
- `test_draw_and_recycle`: Correct handling of stock depletion and recycling.
- `test_legal_tableau_moves`: Rules on opposite color and rank subtraction.
- `test_foundation_moves`: Building same-suit foundation piles from A to K.
- `test_invalid_moves`: Rejection of illegal moves (e.g., King on non-empty, wrong colors, wrong order).
- `test_undo`: Multi-step undo stability.
- `test_win_detection`: Assuring win status updates correctly on foundation completion.

---

## 7. Plan Execution Steps

1. **Step 1**: Create folder `solitaire-app/` and establish `requirements.txt`.
2. **Step 2**: Implement the engine in `solitaire-app/game_logic.py`.
3. **Step 3**: Implement unit tests in `solitaire-app/tests/test_game_logic.py` and run them with `pytest` to verify absolute correctness of the core game engine.
4. **Step 4**: Implement the non-interactive demo code in `solitaire-app/main.py` controlled by the `--smoke` flag.
5. **Step 5**: Implement the Curses UI layout and loop in `solitaire-app/tui.py` and hook it into `main.py`.
6. **Step 6**: Validate the entire application interactive interface and verify `--smoke` demo exits successfully.
