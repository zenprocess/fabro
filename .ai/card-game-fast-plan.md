# FreeCell Solitaire - Implementation Plan

An elegant, terminal-based FreeCell Solitaire game built in Python using the standard library `curses` module.

## 1. Game Rules & Logic Representation

FreeCell is a solitaire card game played with a standard 52-card deck.

### Core Components
- **Card**:
  - `suit`: 'H' (Hearts), 'D' (Diamonds), 'C' (Clubs), 'S' (Spades)
  - `rank`: 1 (Ace) to 13 (King)
  - `color`: Red ('H', 'D') or Black ('C', 'S')
  - Methods: string representation (e.g., `10H`, `AS`, `QD`), color check.
- **Deck**:
  - Contains 52 unique cards.
  - Shuffle and deal logic.
- **State Structure**:
  - **Free Cells** (4 slots): Can each hold at most one arbitrary card.
  - **Foundations** (4 slots, one per suit): Built from Ace to King by suit.
  - **Tableaus** (8 columns):
    - Columns 1-4 start with 7 cards.
    - Columns 5-8 start with 6 cards.
    - Cards within tableaus are built down in alternating colors (e.g., Red 9 on Black 10).
    - Empty tableau columns can accept any card or valid sequence.

### Move Rules & Validation
- **Single Card Move**:
  - Any card from the bottom of a Tableau, or any card in a Free Cell, can be moved to:
    - An empty Free Cell.
    - An empty Tableau column.
    - A Tableau column if the destination card is of opposite color and exactly 1 rank higher.
    - A Foundation slot if the suit matches and the rank is exactly 1 higher than the current foundation top card (or Ace on an empty slot).
- **Multi-Card Sequence Move**:
  - A descending, alternating-color sequence of cards can be moved between tableaus.
  - The maximum size of the sequence that can be moved is determined by the formula:
    $$\text{max\_cards} = (N + 1) \times 2^M$$
    where $N$ is the number of empty Free Cells, and $M$ is the number of empty Tableaus (excluding the destination tableau if it was empty).
- **Undo History**:
  - Keep a stack of previous states or individual moves to support unlimited undo operations ('u').

---

## 2. Terminal Rendering with `curses`

The UI will fit within a standard `80x24` terminal.

### Layout Coordinates
- **Top Row (Row 2)**:
  - Free Cells 1-4 on the left (columns 2, 10, 18, 26).
  - Foundations 1-4 on the right (columns 42, 50, 58, 66).
- **Bottom Row (Row 8 onwards)**:
  - Tableau columns 1-8 spaced out evenly across columns 2, 10, 18, 26, 34, 42, 50, 58.
  - Cards stacked downwards with vertical overlap (each card offset by 1 or 2 rows).

### Cursor & Selection Visuals
- A grid-based cursor `(row, col)`:
  - `row = 0`: Top row (col 0-3 = Free Cells 1-4, col 4-7 = Foundations 1-4).
  - `row = 1`: Bottom row (col 0-7 = Tableaus 1-8).
- The active cursor position will be highlighted with a distinctive border or background color.
- If a source card/column is selected, it will be marked with a unique color pair (e.g., highlighted yellow) to indicate "selected" state.

### Colors
- Red Cards: Red foreground, black background (or standard terminal color).
- Black Cards: White/cyan foreground, black background.
- Cursor/Selected: Yellow/cyan background or inverted colors.
- Safe fallback if colors are not supported by the terminal.

---

## 3. Input Handling & Interactive Loop

### Gameplay Flow
1. **Initialize curses**: Setup screen, hide cursor, enable keypad, initialize colors.
2. **Main loop**:
   - Draw board: Free Cells, Foundations, Tableaus, headers, and status messages.
   - Wait for keyboard input.
   - Handle key:
     - **Arrow Keys / WASD / HJKL**: Move cursor between piles.
     - **Space / Enter**:
       - If no source is selected: Set the pile at the current cursor as the source.
       - If a source is selected: Attempt to move from the source pile to the pile at the current cursor. Deselect.
     - **'u'**: Undo previous move.
     - **'n'**: Start a new game (prompt for confirmation).
     - **'h'**: Show rules/help window.
     - **'q' / ESC**: Quit the game.
3. **Safety & Robustness**:
   - Catch window resize events (`curses.KEY_RESIZE`) and adapt.
   - Graceful error recovery if drawing bounds are exceeded due to small terminal.

---

## 4. Win/Loss Detection

- **Win Condition**: All 4 foundations contain 13 cards (total 52 cards, all ending in Kings).
- **Loss Warning**: Check if any valid moves exist across all tableaus, free cells, and foundations. If none exist, display a "No moves left! Press 'n' for a new game or 'u' to undo." notice in the status line.

---

## 5. Non-Interactive Demo & Smoke Test

To support automated test runners and non-interactive environments:
- Include a `--smoke` flag on `main.py`.
- When run as `python3 main.py --smoke`, the game will:
  1. Initialize a deterministic game state using a fixed random seed.
  2. Validate that the deal is correct (52 cards correctly distributed).
  3. Simulate a series of valid moves (e.g., Tableau card to Free Cell).
  4. Verify the state updates correctly.
  5. Attempt an invalid move and assert that it is rejected.
  6. Perform an undo and verify the state rolls back.
  7. Output test diagnostics to stdout and exit with `0` (success) or `1` (failure).
  8. Run completely headless (no curses initialization) to avoid terminal constraints.

---

## 6. Test Strategy

- **Unit Tests (`test_game.py`)**:
  - Test `Card` representation and behavior.
  - Test `Deck` creation and card uniqueness.
  - Test valid single card movements (Tableau -> Free Cell, Tableau -> Foundation, etc.).
  - Test multi-card sequence validation rules.
  - Test state undo/redo accuracy.
  - Test win detection.
- **Run Tests**:
  - Executed via `pytest` or `python3 -m unittest discover`.
