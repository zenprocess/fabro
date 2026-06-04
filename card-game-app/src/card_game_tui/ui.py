import curses
import os
import sys
from .game import GameState, Card

def init_colors():
    # Only initialize if color is supported
    if curses.has_colors():
        curses.start_color()
        # Pair 1: Red cards on default background
        curses.init_pair(1, curses.COLOR_RED, curses.COLOR_BLACK)
        # Pair 2: Black cards on default background
        # Cyan is highly visible on both dark and light terminal backgrounds
        curses.init_pair(2, curses.COLOR_CYAN, curses.COLOR_BLACK)
        # Pair 3: Labels & headers (Yellow)
        curses.init_pair(3, curses.COLOR_YELLOW, curses.COLOR_BLACK)
        # Pair 4: Win message (Green)
        curses.init_pair(4, curses.COLOR_GREEN, curses.COLOR_BLACK)
        # Pair 5: Selected/highlighted state (Black text on Yellow background)
        curses.init_pair(5, curses.COLOR_BLACK, curses.COLOR_YELLOW)
        # Pair 6: Title bar (White text on Blue background)
        curses.init_pair(6, curses.COLOR_WHITE, curses.COLOR_BLUE)

def draw_card(stdscr, card, y, x, is_selected=False):
    if card is None:
        stdscr.addstr(y, x, "[   ]")
        return
    
    card_str = f"[{card.rank_str:>2}{card.symbol}]"
    
    # Pick color pair
    if is_selected:
        pair = 5
    else:
        pair = 1 if card.color == 'R' else 2
        
    stdscr.addstr(y, x, card_str, curses.color_pair(pair) | (curses.A_BOLD if not is_selected else 0))

def draw_board(stdscr, state, selected_src, message):
    stdscr.clear()
    y_max, x_max = stdscr.getmaxyx()
    
    # Check minimum window size
    if y_max < 24 or x_max < 80:
        stdscr.addstr(0, 0, f"Terminal window is too small ({y_max}x{x_max}).")
        stdscr.addstr(1, 0, "Please enlarge your terminal to at least 24 lines and 80 columns.")
        stdscr.addstr(3, 0, "Press any key to retry or 'x' to exit.")
        stdscr.refresh()
        return False

    # 1. Title bar
    title = " --- FREECELL SOLITAIRE --- "
    stdscr.addstr(0, 0, title.center(80), curses.color_pair(6) | curses.A_BOLD)
    
    # 2. FreeCells and Foundations Headers
    stdscr.addstr(2, 2, "FREECELLS (q, w, e, r)", curses.color_pair(3) | curses.A_BOLD)
    stdscr.addstr(2, 42, "FOUNDATIONS (auto-collected or via 'f')", curses.color_pair(3) | curses.A_BOLD)

    # 3. Draw FreeCells
    fc_keys = ['q', 'w', 'e', 'r']
    for i in range(4):
        x = 2 + (i * 8)
        # Label below FreeCell
        stdscr.addstr(5, x + 2, fc_keys[i], curses.color_pair(3))
        
        # Check if selected
        is_sel = (selected_src is not None and selected_src[0] == 'freecell' and selected_src[1] == i)
        card = state.freecells[i]
        draw_card(stdscr, card, 3, x, is_selected=is_sel)

    # 4. Draw Foundations
    # Let's draw empty foundations with suit symbol placeholders for clarity
    suits = ['C', 'D', 'H', 'S']
    for i, suit in enumerate(suits):
        x = 42 + (i * 8)
        current_rank = state.foundations[suit]
        if current_rank == 0:
            # Empty placeholder
            symbol = Card.SUIT_SYMBOLS[suit]
            color_pair = 1 if suit in ['D', 'H'] else 2
            stdscr.addstr(3, x, f"[ {symbol} ]", curses.color_pair(color_pair))
        else:
            card = Card(suit, current_rank)
            draw_card(stdscr, card, 3, x, is_selected=False)
        stdscr.addstr(5, x + 2, suit, curses.color_pair(3))

    # Divider
    stdscr.addstr(6, 0, "=" * 80, curses.color_pair(3))

    # 5. Draw Tableaux (Columns 1-8)
    stdscr.addstr(7, 2, "TABLEAUX (1-8)", curses.color_pair(3) | curses.A_BOLD)
    
    # Column labels
    for col_idx in range(8):
        x = 2 + (col_idx * 9)
        stdscr.addstr(8, x + 2, str(col_idx + 1), curses.color_pair(3) | curses.A_BOLD)

    # Draw card stacks
    # We display up to 14 cards vertically. If a column is empty, show empty space placeholder.
    for col_idx in range(8):
        col = state.tableaux[col_idx]
        x = 2 + (col_idx * 9)
        if not col:
            stdscr.addstr(9, x, "[   ]", curses.A_DIM)
        else:
            longest_seq = state.get_longest_alternating_sequence_at_bottom(col_idx)
            seq_start_idx = len(col) - len(longest_seq)
            
            for card_idx, card in enumerate(col):
                y = 9 + card_idx
                if y >= y_max - 4:
                    # Prevent drawing past screen boundary
                    stdscr.addstr(y_max - 5, x, "[...]", curses.A_DIM)
                    break
                
                # Check selection status:
                # Is this column selected as source?
                is_sel = False
                if selected_src is not None and selected_src[0] == 'tableau' and selected_src[1] == col_idx:
                    # If this column is the selected source, we highlight the entire movable bottom sequence!
                    if card_idx >= seq_start_idx:
                        is_sel = True
                
                draw_card(stdscr, card, y, x, is_selected=is_sel)

    # 6. Commands and Instructions at the bottom
    divider_y = y_max - 4
    stdscr.addstr(divider_y, 0, "=" * 80, curses.color_pair(3))
    
    cmd_y = y_max - 3
    stdscr.addstr(cmd_y, 2, "Commands: [1-8]/[q-r] to select source, then [1-8]/[q-r]/[f] to move.", curses.A_BOLD)
    stdscr.addstr(cmd_y + 1, 2, "[u] Undo  |  [s] Auto-Collect  |  [n] New Game  |  [x] Exit", curses.color_pair(3))

    # 7. Status / Prompt message
    status_y = y_max - 1
    if state.is_won():
        stdscr.addstr(status_y, 2, "🎉 CONGRATULATIONS! YOU WON! Press 'n' for a new game or 'x' to exit. 🎉", curses.color_pair(4) | curses.A_BOLD)
    else:
        # Standard status bar showing current instruction or outcome
        stdscr.addstr(status_y, 2, f"Status: {message}", curses.A_BOLD)

    stdscr.refresh()
    return True


def play_game():
    def main_loop(stdscr):
        # Set up screen
        try:
            curses.curs_set(0) # Hide cursor
        except curses.error:
            pass # Some terminals don't support hiding cursor
        init_colors()
        
        state = GameState()
        state.deal()
        
        # Initial auto-collect to grab any instant Aces
        state.auto_collect()

        selected_src = None # Stores ('tableau', idx) or ('freecell', idx)
        message = "Game started! Select a source card."

        # Key mappings
        tab_keys = {str(i): i - 1 for i in range(1, 9)} # '1'-'8' -> 0-7
        fc_keys = {'q': 0, 'w': 1, 'e': 2, 'r': 3}     # 'q'-'r' -> 0-3

        while True:
            # Draw board
            if not draw_board(stdscr, state, selected_src, message):
                # Window too small, wait for key and re-evaluate
                ch = stdscr.getch()
                if ch == ord('x') or ch == ord('X'):
                    break
                continue

            # Read keyboard input
            try:
                ch = stdscr.getch()
            except KeyboardInterrupt:
                break

            if ch == -1:
                continue

            key_char = chr(ch).lower() if 0 <= ch < 256 else ""

            # Check general commands first
            if key_char == 'x' or ch == 27: # 'x' or Escape
                # Confirm exit
                stdscr.addstr(stdscr.getmaxyx()[0] - 1, 2, "Are you sure you want to exit? (y/n): ", curses.color_pair(3) | curses.A_BOLD)
                stdscr.refresh()
                confirm = stdscr.getch()
                if chr(confirm).lower() == 'y':
                    break
                else:
                    message = "Exit cancelled."
                    continue

            elif key_char == 'n':
                # Confirm restart
                stdscr.addstr(stdscr.getmaxyx()[0] - 1, 2, "Start a new game? (y/n): ", curses.color_pair(3) | curses.A_BOLD)
                stdscr.refresh()
                confirm = stdscr.getch()
                if chr(confirm).lower() == 'y':
                    state.deal()
                    state.auto_collect()
                    selected_src = None
                    message = "New game started!"
                else:
                    message = "Restart cancelled."
                continue

            elif key_char == 'u':
                if state.undo():
                    selected_src = None
                    message = "Undo successful."
                else:
                    message = "Nothing to undo."
                continue

            elif key_char == 's':
                collected = state.auto_collect()
                if collected > 0:
                    message = f"Auto-collected {collected} card(s) to foundations."
                else:
                    message = "No safe cards to auto-collect."
                continue

            # Check game won state (only allow commands like restart or exit)
            if state.is_won():
                message = "You won! Start a new game with [n] or exit with [x]."
                continue

            # Source/Destination Selection Flow
            if selected_src is None:
                # Selecting source
                if key_char in tab_keys:
                    idx = tab_keys[key_char]
                    if not state.tableaux[idx]:
                        message = f"Tableau {idx+1} is empty! Cannot select as source."
                    else:
                        selected_src = ('tableau', idx)
                        top_card = state.tableaux[idx][-1]
                        message = f"Selected column {idx+1} (bottom card: {top_card}). Choose destination."
                elif key_char in fc_keys:
                    idx = fc_keys[key_char]
                    if state.freecells[idx] is None:
                        message = f"FreeCell {idx+1} is empty! Cannot select as source."
                    else:
                        selected_src = ('freecell', idx)
                        card = state.freecells[idx]
                        message = f"Selected {card} from FreeCell {idx+1}. Choose destination."
                else:
                    message = "Invalid key. Select a source: [1-8] for Tableaux, [q-r] for FreeCells."
            
            else:
                # Selecting destination
                src_type, src_idx = selected_src
                
                if key_char in tab_keys:
                    dest_idx = tab_keys[key_char]
                    success, msg = state.move_card_to_tableau(src_type, src_idx, dest_idx)
                    message = msg
                    if success:
                        # Auto-collect after successful move
                        collected = state.auto_collect()
                        if collected > 0:
                            message += f" (Auto-collected {collected} card(s) to foundations.)"
                    selected_src = None # Clear selection
                    
                elif key_char in fc_keys:
                    dest_idx = fc_keys[key_char]
                    success, msg = state.move_card_to_freecell(src_type, src_idx, dest_idx)
                    message = msg
                    if success:
                        collected = state.auto_collect()
                        if collected > 0:
                            message += f" (Auto-collected {collected} card(s) to foundations.)"
                    selected_src = None
                    
                elif key_char == 'f':
                    # Move to foundation
                    success, msg = state.move_card_to_foundation(src_type, src_idx)
                    message = msg
                    if success:
                        collected = state.auto_collect()
                        if collected > 0:
                            message += f" (Auto-collected {collected} card(s) to foundations.)"
                    selected_src = None
                    
                else:
                    # Cancel selection if any other key pressed
                    selected_src = None
                    message = "Selection cancelled."

    curses.wrapper(main_loop)
