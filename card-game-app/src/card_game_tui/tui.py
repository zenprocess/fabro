import curses
import random
import sys
from typing import Union, List, Optional
from card_game_tui.domain import GameState, Position, LocationType, Suit, Card
from card_game_tui.render import get_card_inner, get_empty_foundation_inner

class TUIApp:
    def __init__(self):
        # We generate a random seed for the game to start with, e.g. 1 to 99999
        self.current_seed = random.randint(1, 99999)
        self.state = GameState(seed=self.current_seed)
        
        # Cursor and Selection State
        self.cursor_row = 1  # 0: Top row (Freecells/Foundations), 1: Tableau Columns
        self.cursor_col = 0  # 0 to 7
        
        self.selected_pos: Optional[Position] = None
        self.selected_count = 1
        
        self.message = "Welcome to FreeCell! Select a card to begin."
        self.message_attr = 0  # Will be set to color pair
        
        self.quit_confirm = False

    def reset_game(self, seed: int):
        self.current_seed = seed
        self.state = GameState(seed=seed)
        self.selected_pos = None
        self.selected_count = 1
        self.cursor_row = 1
        self.cursor_col = 0
        self.message = f"Game started with seed: {self.current_seed}"
        self.message_attr = 0
        self.quit_confirm = False

    def get_current_pos(self) -> Position:
        if self.cursor_row == 0:
            if self.cursor_col < 4:
                return Position(LocationType.FREECELL, self.cursor_col)
            else:
                return Position(LocationType.FOUNDATION, self.cursor_col - 4)
        else:
            return Position(LocationType.TABLEAU, self.cursor_col)

    def find_max_valid_count(self, from_pos: Position, to_pos: Position) -> int:
        if from_pos.type != LocationType.TABLEAU:
            return 1
        col = self.state.tableaus[from_pos.index]
        if not col:
            return 0
        
        # Find maximum length of alternating, descending sequence at bottom
        max_seq = 1
        for i in range(len(col) - 2, -1, -1):
            c1 = col[i]
            c2 = col[i+1]
            if c2.rank.value == c1.rank.value - 1 and c2.suit.color != c1.suit.color:
                max_seq += 1
            else:
                break
                
        # Find the maximum count that is legally movable to destination
        for count in range(max_seq, 0, -1):
            if self.state.validate_move(from_pos, to_pos, count):
                return count
        return 0

    def find_longest_sequence(self, col_idx: int) -> int:
        col = self.state.tableaus[col_idx]
        if not col:
            return 0
        max_seq = 1
        for i in range(len(col) - 2, -1, -1):
            c1 = col[i]
            c2 = col[i+1]
            if c2.rank.value == c1.rank.value - 1 and c2.suit.color != c1.suit.color:
                max_seq += 1
            else:
                break
        return max_seq

    def safe_addstr(self, stdscr, y: int, x: int, text: str, attr: int = 0):
        try:
            if y < self.max_y and x < self.max_x:
                available = self.max_x - x
                if len(text) > available:
                    text = text[:available]
                stdscr.addstr(y, x, text, attr)
        except curses.error:
            pass

    def draw_board(self, stdscr):
        # Clear screen
        stdscr.erase()
        
        # Draw Title
        title_attr = curses.color_pair(3) | curses.A_BOLD
        self.safe_addstr(stdscr, 1, 2, "FREECELL SOLITAIRE", title_attr)
        self.safe_addstr(stdscr, 1, 55, f"[SEED: {self.current_seed}]", curses.color_pair(4))
        
        # Labels
        label_attr = curses.color_pair(4) | curses.A_BOLD
        self.safe_addstr(stdscr, 3, 2, "Free Cells", label_attr)
        self.safe_addstr(stdscr, 3, 34, "Foundations", label_attr)
        
        # Free Cells (Cols 0-3 on row 0)
        for i in range(4):
            x = 2 + i * 8
            y = 4
            is_cursor = (self.cursor_row == 0 and self.cursor_col == i)
            
            # Draw header label [A], [B], [C], [D]
            header_attr = (curses.color_pair(3) | curses.A_BOLD) if is_cursor else curses.A_NORMAL
            self.safe_addstr(stdscr, y - 1, x, f" [{chr(65 + i)}] ", header_attr)
            
            card = self.state.freecells[i]
            border_attr = curses.color_pair(3) if is_cursor else curses.A_NORMAL
            is_selected = (self.selected_pos is not None and self.selected_pos.type == LocationType.FREECELL and self.selected_pos.index == i)
            
            if card is None:
                self.safe_addstr(stdscr, y, x, "+---+", border_attr)
                self.safe_addstr(stdscr, y + 1, x, "|   |", border_attr)
                self.safe_addstr(stdscr, y + 2, x, "+---+", border_attr)
            else:
                card_attr = curses.color_pair(1) if card.suit.color == "RED" else curses.color_pair(2)
                if is_selected:
                    card_attr |= curses.A_REVERSE
                self.safe_addstr(stdscr, y, x, "+---+", border_attr)
                self.safe_addstr(stdscr, y + 1, x, f"|{get_card_inner(card)}|", card_attr)
                self.safe_addstr(stdscr, y + 2, x, "+---+", border_attr)

        # Foundations (Cols 4-7 on row 0)
        for i in range(4):
            x = 34 + i * 8
            y = 4
            is_cursor = (self.cursor_row == 0 and self.cursor_col == i + 4)
            
            suit = list(Suit)[i]
            header_attr = (curses.color_pair(3) | curses.A_BOLD) if is_cursor else curses.A_NORMAL
            self.safe_addstr(stdscr, y - 1, x, f" [{suit.symbol}] ", header_attr)
            
            pile = self.state.foundations[suit]
            border_attr = curses.color_pair(3) if is_cursor else curses.A_NORMAL
            
            if not pile:
                inner = get_empty_foundation_inner(i)
                inner_attr = curses.color_pair(1) if suit.color == "RED" else curses.color_pair(2)
                inner_attr |= curses.A_DIM
                self.safe_addstr(stdscr, y, x, "+---+", border_attr)
                self.safe_addstr(stdscr, y + 1, x, f"|{inner}|", inner_attr)
                self.safe_addstr(stdscr, y + 2, x, "+---+", border_attr)
            else:
                card = pile[-1]
                card_attr = curses.color_pair(1) if card.suit.color == "RED" else curses.color_pair(2)
                self.safe_addstr(stdscr, y, x, "+---+", border_attr)
                self.safe_addstr(stdscr, y + 1, x, f"|{get_card_inner(card)}|", card_attr)
                self.safe_addstr(stdscr, y + 2, x, "+---+", border_attr)

        # Tableau columns
        self.safe_addstr(stdscr, 8, 2, "Tableau Columns", label_attr)
        
        for col_idx in range(8):
            x = 2 + col_idx * 8
            y = 10
            is_cursor_col = (self.cursor_row == 1 and self.cursor_col == col_idx)
            
            # Header label [1] to [8]
            header_attr = (curses.color_pair(3) | curses.A_BOLD) if is_cursor_col else curses.A_NORMAL
            self.safe_addstr(stdscr, y - 1, x, f" [{col_idx + 1}] ", header_attr)
            
            col = self.state.tableaus[col_idx]
            
            if not col:
                empty_attr = curses.color_pair(3) if is_cursor_col else curses.A_DIM
                self.safe_addstr(stdscr, y, x, "+---+", empty_attr)
                self.safe_addstr(stdscr, y + 1, x, "|   |", empty_attr)
                self.safe_addstr(stdscr, y + 2, x, "+---+", empty_attr)
            else:
                # Draw stacked cards
                top_border_attr = curses.color_pair(3) if is_cursor_col else curses.A_NORMAL
                self.safe_addstr(stdscr, y, x, "+---+", top_border_attr)
                
                for card_idx, card in enumerate(col):
                    is_selected_card = False
                    if (self.selected_pos is not None and 
                            self.selected_pos.type == LocationType.TABLEAU and 
                            self.selected_pos.index == col_idx):
                        if card_idx >= len(col) - self.selected_count:
                            is_selected_card = True
                    
                    card_attr = curses.color_pair(1) if card.suit.color == "RED" else curses.color_pair(2)
                    if is_selected_card:
                        card_attr |= curses.A_REVERSE
                        
                    card_y = y + 1 + card_idx
                    self.safe_addstr(stdscr, card_y, x, f"|{get_card_inner(card)}|", card_attr)
                
                bottom_y = y + 1 + len(col)
                bottom_border_attr = curses.color_pair(3) if is_cursor_col else curses.A_NORMAL
                self.safe_addstr(stdscr, bottom_y, x, "+---+", bottom_border_attr)

        # Status & Message bar (fixed near bottom of standard screen size or max_y)
        msg_y = max(20, self.max_y - 4)
        sep_y = msg_y - 1
        keys_y = msg_y + 1
        
        # Horizontal separator
        self.safe_addstr(stdscr, sep_y, 0, "-" * self.max_x, curses.A_DIM)
        
        # Message string
        self.safe_addstr(stdscr, msg_y, 2, f"MSG: {self.message}", self.message_attr)
        
        # Keys bar
        keys_text = "KEYS: [Arrows/WASD] Navigate  [Space/Enter] Select/Drop  [U] Undo  [Y] Redo  [R] Restart  [N] New  [Q] Quit"
        if self.selected_pos is not None:
            longest_seq = self.find_longest_sequence(self.selected_pos.index) if self.selected_pos.type == LocationType.TABLEAU else 1
            if longest_seq > 1:
                keys_text = f"KEYS: [-/+] Move Count: {self.selected_count}/{longest_seq}  [Arrows/WASD] Navigate  [Space] Drop  [Esc] Cancel"
        
        self.safe_addstr(stdscr, keys_y, 2, keys_text, curses.color_pair(4) | curses.A_BOLD)

    def handle_input(self, key: int) -> bool:
        """
        Processes key inputs. Returns True if we should keep running, False if we should exit.
        """
        # If in Quit Confirmation state
        if self.quit_confirm:
            if key in (ord('y'), ord('Y')):
                return False
            else:
                self.quit_confirm = False
                self.message = "Quit canceled. Game resumed."
                self.message_attr = curses.color_pair(4)
                return True

        # Standard navigation keys
        # UP: Arrow UP, w, W, k, K
        if key in (curses.KEY_UP, ord('w'), ord('W'), ord('k'), ord('K')):
            self.cursor_row = 0
            self.message_attr = 0
        # DOWN: Arrow DOWN, s, S, j, J
        elif key in (curses.KEY_DOWN, ord('s'), ord('S'), ord('j'), ord('J')):
            self.cursor_row = 1
            self.message_attr = 0
        # LEFT: Arrow LEFT, a, A, h, H
        elif key in (curses.KEY_LEFT, ord('a'), ord('A'), ord('h'), ord('H')):
            self.cursor_col = (self.cursor_col - 1) % 8
            self.message_attr = 0
        # RIGHT: Arrow RIGHT, d, D, l, L
        elif key in (curses.KEY_RIGHT, ord('d'), ord('D'), ord('l'), ord('L')):
            self.cursor_col = (self.cursor_col + 1) % 8
            self.message_attr = 0

        # Undo key: u, U
        elif key in (ord('u'), ord('U')):
            if self.state.undo():
                self.message = "Undo successful."
                self.message_attr = curses.color_pair(5)
                self.selected_pos = None
            else:
                self.message = "Nothing to undo!"
                self.message_attr = curses.color_pair(1)

        # Redo key: y, Y or Ctrl-Y (25) or Ctrl-R (18)
        elif key in (ord('y'), ord('Y'), 25, 18):
            if self.state.redo():
                self.message = "Redo successful."
                self.message_attr = curses.color_pair(5)
                self.selected_pos = None
            else:
                self.message = "Nothing to redo!"
                self.message_attr = curses.color_pair(1)

        # Restart key: r, R
        elif key in (ord('r'), ord('R')):
            self.reset_game(self.current_seed)
            self.message = f"Game restarted. Seed: {self.current_seed}"
            self.message_attr = curses.color_pair(4)

        # New Game key: n, N
        elif key in (ord('n'), ord('N')):
            new_seed = random.randint(1, 99999)
            self.reset_game(new_seed)

        # Quit key: q, Q
        elif key in (ord('q'), ord('Q')):
            self.quit_confirm = True
            self.message = "Are you sure you want to quit? (y/n)"
            self.message_attr = curses.color_pair(1) | curses.A_BOLD

        # Cancel Selection: Escape key (27)
        elif key == 27:
            if self.selected_pos is not None:
                self.selected_pos = None
                self.selected_count = 1
                self.message = "Selection canceled."
                self.message_attr = curses.color_pair(4)

        # Selection Count adjustments (only valid when a Tableau selection is active)
        elif key in (ord('-'), ord('_'), ord('[')) and self.selected_pos is not None and self.selected_pos.type == LocationType.TABLEAU:
            if self.selected_count > 1:
                self.selected_count -= 1
                self.message = f"Adjusted selection count to {self.selected_count}."
                self.message_attr = curses.color_pair(4)

        elif key in (ord('+'), ord('='), ord(']')) and self.selected_pos is not None and self.selected_pos.type == LocationType.TABLEAU:
            longest_seq = self.find_longest_sequence(self.selected_pos.index)
            if self.selected_count < longest_seq:
                self.selected_count += 1
                self.message = f"Adjusted selection count to {self.selected_count}."
                self.message_attr = curses.color_pair(4)

        # Space (32) or Enter (10, 13, curses.KEY_ENTER)
        elif key in (32, 10, 13, curses.KEY_ENTER):
            curr_pos = self.get_current_pos()
            
            if self.selected_pos is None:
                # Attempt to select
                if curr_pos.type == LocationType.FOUNDATION:
                    self.message = "Cannot pick up cards from the foundation piles."
                    self.message_attr = curses.color_pair(1)
                else:
                    card = self.state.get_card_at(curr_pos)
                    if card is None:
                        self.message = "That slot is empty!"
                        self.message_attr = curses.color_pair(1)
                    else:
                        self.selected_pos = curr_pos
                        # Default count to 1
                        self.selected_count = 1
                        
                        # Automatically compute max valid count if destination is a Tableau
                        # We will keep count as 1, but user can adjust or we automatically resolve on execute.
                        self.message = f"Selected {card}. Use [-/+] to change count. Select destination and press Space."
                        self.message_attr = curses.color_pair(4)
            else:
                # Destination select / execute move
                if curr_pos == self.selected_pos:
                    # Clicking source again deselects
                    self.selected_pos = None
                    self.selected_count = 1
                    self.message = "Selection cleared."
                    self.message_attr = curses.color_pair(4)
                else:
                    # Let's perform move execution
                    # First, if moving Tableau-to-Tableau, let's automatically check if a sequence move is valid
                    # and if we should adjust selected_count if user didn't specify one larger than 1.
                    count_to_use = self.selected_count
                    if self.selected_pos.type == LocationType.TABLEAU and curr_pos.type == LocationType.TABLEAU:
                        # If user is moving a sequence and selected_count is 1, let's find if a larger sequence is valid and use it.
                        # This auto-moves the sequence by default if they didn't manually change count, matching standard games.
                        max_valid = self.find_max_valid_count(self.selected_pos, curr_pos)
                        if max_valid > count_to_use:
                            count_to_use = max_valid

                    if self.state.execute_move(self.selected_pos, curr_pos, count=count_to_use):
                        self.message = "Move successful!"
                        self.message_attr = curses.color_pair(5)
                        
                        # Check win condition
                        if self.state.check_win():
                            self.message = "CONGRATULATIONS! YOU HAVE WON THE GAME! Press N for new game, Q to quit."
                            self.message_attr = curses.color_pair(5) | curses.A_BLINK | curses.A_BOLD
                        # Check loss condition
                        elif self.state.check_loss():
                            self.message = "GAME OVER! No legal moves remaining. Press R to restart, N for new game, Q to quit."
                            self.message_attr = curses.color_pair(1) | curses.A_BOLD
                            
                        self.selected_pos = None
                        self.selected_count = 1
                    else:
                        # Show precise validation error
                        self.message = "Invalid move under FreeCell rules!"
                        self.message_attr = curses.color_pair(1)
                        # We do NOT clear selection on invalid move, so user can try another target!

        return True

    def _main_loop(self, stdscr) -> None:
        # Curses configurations
        curses.curs_set(0)  # Hide hardware cursor
        stdscr.keypad(True)  # Enable arrow keys
        stdscr.nodelay(False)  # Wait for keys
        
        # Set up color pairs
        if curses.has_colors():
            curses.start_color()
            curses.use_default_colors()
            # Pair 1: Red cards
            curses.init_pair(1, curses.COLOR_RED, curses.COLOR_BLACK)
            # Pair 2: Black cards
            curses.init_pair(2, curses.COLOR_WHITE, curses.COLOR_BLACK)
            # Pair 3: Headers / Active Selection
            curses.init_pair(3, curses.COLOR_CYAN, curses.COLOR_BLACK)
            # Pair 4: Keys / Info
            curses.init_pair(4, curses.COLOR_YELLOW, curses.COLOR_BLACK)
            # Pair 5: Success / Win
            curses.init_pair(5, curses.COLOR_GREEN, curses.COLOR_BLACK)

        while True:
            # Get current window boundaries
            self.max_y, self.max_x = stdscr.getmaxyx()
            
            # Draw the game board
            self.draw_board(stdscr)
            
            # Perform screen updates
            stdscr.refresh()
            
            # Read next keyboard input
            try:
                key = stdscr.getch()
            except KeyboardInterrupt:
                break
                
            # Handle the keyboard input
            if not self.handle_input(key):
                break

    def run(self) -> None:
        try:
            curses.wrapper(self._main_loop)
        except Exception as e:
            # Restore terminal cleanly on error
            print(f"Error in curses TUI: {e}", file=sys.stderr)
            sys.exit(1)
