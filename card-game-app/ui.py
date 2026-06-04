import curses
import locale
import sys
from game import FreeCellGame, Card

# Enable localized Unicode support for suit symbols
try:
    locale.setlocale(locale.LC_ALL, '')
except Exception:
    pass

class FreeCellUI:
    def __init__(self, game: FreeCellGame):
        self.game = game
        self.cursor_row = 1  # 0: Top (Free Cells & Foundations), 1: Bottom (Tableaus)
        self.cursor_col = 0  # 0 to 7
        self.selected_pile = None  # Tuple: (pile_type, index)
        self.status_message = "Welcome to FreeCell! Use arrow keys to navigate, Space/Enter to select."

    def run(self):
        # curses.wrapper automatically handles initialization and cleanup
        curses.wrapper(self._main_loop)

    def _init_colors(self):
        if curses.has_colors():
            try:
                curses.use_default_colors()
                bg = -1
            except Exception:
                bg = curses.COLOR_BLACK

            # Pair 1: Red cards (Red on transparent/black)
            curses.init_pair(1, curses.COLOR_RED, bg)
            # Pair 2: Black cards (White/Default on transparent/black)
            curses.init_pair(2, curses.COLOR_WHITE, bg)
            # Pair 3: Cursor highlight (Black on Cyan)
            curses.init_pair(3, curses.COLOR_BLACK, curses.COLOR_CYAN)
            # Pair 4: Selected highlight (Black on Yellow)
            curses.init_pair(4, curses.COLOR_BLACK, curses.COLOR_YELLOW)
            # Pair 5: Header/Muted label (Blue/Cyan on black)
            curses.init_pair(5, curses.COLOR_CYAN, bg)
            # Pair 6: Key bindings/Help (Green on black)
            curses.init_pair(6, curses.COLOR_GREEN, bg)

    def _draw_card(self, stdscr, y, x, card: Card | None, is_cursor=False, is_selected=False, empty_placeholder="   "):
        """Draws a single card or placeholder with color attributes."""
        # Determine color attributes
        attr = curses.A_NORMAL
        if is_cursor:
            attr = curses.color_pair(3) | curses.A_BOLD
        elif is_selected:
            attr = curses.color_pair(4) | curses.A_BOLD

        if card is None:
            # Draw empty placeholder bracket
            stdscr.addstr(y, x, f"[{empty_placeholder}]", attr)
        else:
            # Format rank name and suit symbol
            # Heart/Diamond (Red), Club/Spade (Black/White)
            card_str = f"{card.rank_name:>2}{card.suit_symbol}"
            
            if is_cursor or is_selected:
                stdscr.addstr(y, x, f"[{card_str}]", attr)
            else:
                stdscr.addstr(y, x, "[", curses.A_NORMAL)
                card_attr = curses.color_pair(1) if card.color == 'red' else curses.color_pair(2)
                stdscr.addstr(y, x + 1, card_str, card_attr | curses.A_BOLD)
                stdscr.addstr(y, x + 4, "]", curses.A_NORMAL)

    def _get_pile_under_cursor(self) -> tuple[str, int]:
        if self.cursor_row == 0:
            if self.cursor_col < 4:
                return ('free_cell', self.cursor_col)
            else:
                return ('foundation', self.cursor_col - 4)
        else:
            return ('tableau', self.cursor_col)

    def _draw_board(self, stdscr):
        stdscr.erase()
        h, w = stdscr.getmaxyx()
        
        # Verify terminal size
        if h < 24 or w < 80:
            stdscr.addstr(0, 0, f"Terminal too small ({w}x{h}). Require at least 80x24.", curses.A_REVERSE)
            return

        # Title
        stdscr.addstr(0, 2, "FREECELL SOLITAIRE", curses.color_pair(5) | curses.A_BOLD)
        stdscr.addstr(0, 30, "Press 'h' for Help / Rules", curses.color_pair(6))

        # ------------------ TOP SECTION (Row 3-5) ------------------
        stdscr.addstr(2, 2, "Free Cells (1-4)", curses.color_pair(5) | curses.A_UNDERLINE)
        stdscr.addstr(2, 42, "Foundations (Hearts, Diamonds, Clubs, Spades)", curses.color_pair(5) | curses.A_UNDERLINE)

        current_pile = self._get_pile_under_cursor()

        # Draw Free Cells (FC)
        for i in range(4):
            x_pos = 2 + i * 8
            card = self.game.free_cells[i]
            is_cur = (current_pile == ('free_cell', i))
            is_sel = (self.selected_pile == ('free_cell', i))
            self._draw_card(stdscr, 4, x_pos, card, is_cursor=is_cur, is_selected=is_sel)

        # Draw Foundations (F)
        suits = ['H', 'D', 'C', 'S']
        for i, suit in enumerate(suits):
            x_pos = 42 + i * 8
            found_list = self.game.foundations[suit]
            top_card = found_list[-1] if found_list else None
            is_cur = (current_pile == ('foundation', i))
            is_sel = (self.selected_pile == ('foundation', i))
            
            # If empty, placeholder contains suit symbol
            empty_symbol = Card.SUIT_SYMBOLS[suit]
            self._draw_card(stdscr, 4, x_pos, top_card, is_cursor=is_cur, is_selected=is_sel, empty_placeholder=f" {empty_symbol} ")

        # Divider
        stdscr.addstr(6, 0, "-" * w, curses.color_pair(5))

        # ------------------ BOTTOM SECTION (Row 7 onwards) ------------------
        stdscr.addstr(7, 2, "Tableaus (1-8)", curses.color_pair(5) | curses.A_UNDERLINE)
        
        # Draw column labels
        for i in range(8):
            stdscr.addstr(8, 2 + i * 8, f" T{i+1} ", curses.color_pair(5))

        # Draw Tableau columns cards
        for col_idx in range(8):
            tab = self.game.tableaus[col_idx]
            x_pos = 2 + col_idx * 8
            
            if not tab:
                # Empty tableau
                is_cur = (current_pile == ('tableau', col_idx))
                is_sel = (self.selected_pile == ('tableau', col_idx))
                self._draw_card(stdscr, 9, x_pos, None, is_cursor=is_cur, is_selected=is_sel)
            else:
                for card_idx, card in enumerate(tab):
                    y_pos = 9 + card_idx
                    # Only the bottom card gets the cursor highlight in Tableau
                    is_bottom = (card_idx == len(tab) - 1)
                    is_cur = (current_pile == ('tableau', col_idx)) and is_bottom
                    is_sel = (self.selected_pile == ('tableau', col_idx)) and is_bottom
                    
                    self._draw_card(stdscr, y_pos, x_pos, card, is_cursor=is_cur, is_selected=is_sel)

        # ------------------ STATUS & CONTROLS ------------------
        # Display selection indicator
        if self.selected_pile:
            p_type, p_idx = self.selected_pile
            if p_type == 'tableau':
                p_name = f"Tableau T{p_idx+1}"
            elif p_type == 'free_cell':
                p_name = f"Free Cell FC{p_idx+1}"
            else:
                p_name = f"Foundation F{p_idx+1}"
            stdscr.addstr(20, 2, f"Selected: {p_name}", curses.color_pair(4) | curses.A_BOLD)
        else:
            stdscr.addstr(20, 2, "Selected: None        ", curses.A_DIM)

        # Draw Status Line
        stdscr.addstr(21, 2, f"Status: {self.status_message[:74]:<74}", curses.A_BOLD)

        # Draw Commands HUD
        stdscr.addstr(22, 2, "Commands: [Arrow keys/WASD] Navigate | [Space/Enter] Select/Move | [U] Undo", curses.color_pair(6))
        stdscr.addstr(23, 2, "          [N] New Game | [H] Help/Rules | [Q/ESC] Quit", curses.color_pair(6))

        stdscr.refresh()

    def _show_help(self, stdscr):
        """Displays a modal instructions overlay."""
        h, w = stdscr.getmaxyx()
        # Create a centered pop-up window
        pop_h, pop_w = 16, 64
        pop_y, pop_x = (h - pop_h) // 2, (w - pop_w) // 2
        
        help_win = curses.newwin(pop_h, pop_w, pop_y, pop_x)
        help_win.box()
        
        help_win.addstr(1, 2, "FREECELL RULES & CONTROLS", curses.color_pair(5) | curses.A_BOLD | curses.A_UNDERLINE)
        
        rules = [
            "Rules:",
            "- Build tableaus down in alternating colors (e.g. Red 9 on Black 10).",
            "- Build foundations up from Ace to King by same suit.",
            "- Use the 4 Free Cells to hold any single card temporarily.",
            "- Moving a group of sorted cards is allowed if there is enough",
            "  empty space in Free Cells and empty Tableaus.",
            "",
            "Keyboard Controls:",
            "- Arrow Keys / WASD / HJKL : Move selection cursor",
            "- Space / Enter            : Pick up card / Drop card",
            "- U                        : Undo previous action",
            "- N                        : Restart / Start a new game",
            "- Q / ESC                  : Exit",
        ]
        
        for idx, line in enumerate(rules):
            help_win.addstr(3 + idx, 3, line)
            
        help_win.addstr(pop_h - 2, 2, "Press any key to return to game...", curses.A_REVERSE)
        help_win.refresh()
        
        # Wait for key
        help_win.getch()

    def _show_restart_confirmation(self, stdscr) -> bool:
        """Prompts the user to confirm starting a new game."""
        h, w = stdscr.getmaxyx()
        pop_h, pop_w = 6, 40
        pop_y, pop_x = (h - pop_h) // 2, (w - pop_w) // 2
        
        conf_win = curses.newwin(pop_h, pop_w, pop_y, pop_x)
        conf_win.box()
        conf_win.addstr(1, 4, "START NEW GAME?", curses.color_pair(1) | curses.A_BOLD)
        conf_win.addstr(3, 4, "Are you sure? (y / n)", curses.A_NORMAL)
        conf_win.refresh()
        
        while True:
            ch = conf_win.getch()
            if ch in [ord('y'), ord('Y')]:
                return True
            if ch in [ord('n'), ord('N'), 27]:
                return False

    def _main_loop(self, stdscr):
        # Setup curses options
        curses.curs_set(0)  # Hide hardware cursor
        stdscr.keypad(True)
        self._init_colors()

        while True:
            # Check win condition
            if self.game.is_won():
                self._draw_board(stdscr)
                h, w = stdscr.getmaxyx()
                win_win = curses.newwin(7, 46, (h - 7) // 2, (w - 46) // 2)
                win_win.box()
                win_win.addstr(2, 6, "CONGRATULATIONS! YOU WON!", curses.color_pair(6) | curses.A_BOLD)
                win_win.addstr(4, 5, "Press [N] for New Game or [Q] to Quit.", curses.A_NORMAL)
                win_win.refresh()
                while True:
                    ch = win_win.getch()
                    if ch in [ord('n'), ord('N')]:
                        self.game.reset()
                        self.selected_pile = None
                        self.status_message = "New game started! Shuffle complete."
                        break
                    elif ch in [ord('q'), ord('Q'), 27]:
                        return

            # Check for move possibilities to warn user
            if not self.game.has_any_valid_moves():
                self.status_message = "NO MOVES LEFT! Press 'u' to Undo or 'n' for New Game."

            self._draw_board(stdscr)

            # Wait for key
            try:
                ch = stdscr.getch()
            except KeyboardInterrupt:
                break

            # Handle global commands
            if ch in [ord('q'), ord('Q'), 27]:  # ESC is 27
                break
            elif ch in [ord('h'), ord('H')]:
                self._show_help(stdscr)
                continue
            elif ch in [ord('n'), ord('N')]:
                if self._show_restart_confirmation(stdscr):
                    self.game.reset()
                    self.selected_pile = None
                    self.status_message = "New game started! Shuffle complete."
                continue
            elif ch in [ord('u'), ord('U')]:
                if self.game.undo():
                    self.selected_pile = None
                    self.status_message = "Move undone successfully."
                else:
                    self.status_message = "Nothing to undo!"
                continue

            # Navigation
            if ch in [curses.KEY_UP, ord('w'), ord('W'), ord('k'), ord('K')]:
                self.cursor_row = 0
            elif ch in [curses.KEY_DOWN, ord('s'), ord('S'), ord('j'), ord('J')]:
                self.cursor_row = 1
            elif ch in [curses.KEY_LEFT, ord('a'), ord('A'), ord('h'), ord('H')]:
                self.cursor_col = max(0, self.cursor_col - 1)
            elif ch in [curses.KEY_RIGHT, ord('d'), ord('D'), ord('l'), ord('L')]:
                self.cursor_col = min(7, self.cursor_col + 1)

            # Selection & Action Execution
            elif ch in [10, 13, ord(' ')]:  # Enter (10/13) or Space (' ')
                target_pile = self._get_pile_under_cursor()
                
                if self.selected_pile is None:
                    # Attempting to select source
                    # Ensure source pile actually has cards
                    src_type, src_idx = target_pile
                    card = self.game._peek_card(src_type, src_idx)
                    if card is None:
                        self.status_message = "Selected pile is empty! Pick another source."
                    elif src_type == 'foundation':
                        self.status_message = "Cannot move cards out of Foundations."
                    else:
                        self.selected_pile = target_pile
                        self.status_message = f"Selected source: {src_type} {src_idx+1}. Choose destination."
                else:
                    # Executing move
                    src_type, src_idx = self.selected_pile
                    dest_type, dest_idx = target_pile
                    
                    if self.selected_pile == target_pile:
                        # Clicking same pile deselects
                        self.selected_pile = None
                        self.status_message = "Selection cleared."
                    else:
                        success = False
                        if dest_type == 'free_cell':
                            success = self.game.move_card_to_free_cell(src_type, src_idx, dest_idx)
                        elif dest_type == 'foundation':
                            success = self.game.move_card_to_foundation(src_type, src_idx, dest_idx)
                        elif dest_type == 'tableau':
                            success = self.game.move_to_tableau(src_type, src_idx, dest_idx)
                        
                        if success:
                            # Auto-solve step (sends safe cards to foundations automatically)
                            auto_moves = self.game.auto_solve_loop()
                            if auto_moves > 0:
                                self.status_message = f"Move executed! {auto_moves} cards auto-solved."
                            else:
                                self.status_message = "Move executed successfully!"
                            self.selected_pile = None
                        else:
                            self.status_message = "Invalid move! Rules violated or lack of empty cells."
                            self.selected_pile = None
