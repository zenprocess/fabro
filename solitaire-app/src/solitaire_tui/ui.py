import curses
from .game import GameState, Card
from typing import Optional, Tuple

class SolitaireTUI:
    def __init__(self, game: GameState) -> None:
        self.game = game
        
        # Cursor positioning
        # zone: 'top' or 'bottom'
        self.cursor_zone = 'top'
        # col: 0 to 6 (for 'top' or 'bottom')
        self.cursor_col = 0
        # card_idx: index within the Tableau column (only used when zone == 'bottom')
        self.cursor_card_idx = 0

        # Selection state
        # (zone, col, card_idx) or None
        self.selected_pos: Optional[Tuple[str, int, int]] = None
        self.status_msg = "Welcome to Klondike Solitaire! Arrow/Vim keys to move, Space/Enter to select/draw."

    def get_first_face_up_idx(self, col: int) -> int:
        """Returns the index of the first face-up card in Tableau column col."""
        pile = self.game.tableau[col]
        for idx, card in enumerate(pile):
            if card.is_face_up:
                return idx
        return 0

    def move_cursor(self, direction: str) -> None:
        if self.cursor_zone == 'top':
            if direction == 'left':
                if self.cursor_col > 0:
                    self.cursor_col -= 1
                    if self.cursor_col == 2:  # Skip the gap column
                        self.cursor_col = 1
            elif direction == 'right':
                if self.cursor_col < 6:
                    self.cursor_col += 1
                    if self.cursor_col == 2:  # Skip the gap column
                        self.cursor_col = 3
            elif direction == 'down':
                self.cursor_zone = 'bottom'
                # Transition to tableau column of same index
                pile_len = len(self.game.tableau[self.cursor_col])
                self.cursor_card_idx = max(0, pile_len - 1)
            elif direction == 'up':
                pass  # Already at top row

        elif self.cursor_zone == 'bottom':
            col = self.cursor_col
            pile = self.game.tableau[col]
            
            if direction == 'left':
                if self.cursor_col > 0:
                    self.cursor_col -= 1
                    # Update card index to the top card of the new column
                    new_pile_len = len(self.game.tableau[self.cursor_col])
                    self.cursor_card_idx = max(0, new_pile_len - 1)
            elif direction == 'right':
                if self.cursor_col < 6:
                    self.cursor_col += 1
                    new_pile_len = len(self.game.tableau[self.cursor_col])
                    self.cursor_card_idx = max(0, new_pile_len - 1)
            elif direction == 'up':
                first_face_up = self.get_first_face_up_idx(col)
                if pile and self.cursor_card_idx > first_face_up:
                    self.cursor_card_idx -= 1
                else:
                    # Transition to top row
                    self.cursor_zone = 'top'
                    # Map to closest top-row element (skip gap)
                    if self.cursor_col == 2:
                        self.cursor_col = 1
            elif direction == 'down':
                if pile and self.cursor_card_idx < len(pile) - 1:
                    self.cursor_card_idx += 1

    def handle_action(self) -> None:
        """Handles Space/Enter press based on current cursor position."""
        if self.cursor_zone == 'top':
            if self.cursor_col == 0:
                # Draw from Stock to Waste
                was_empty_stock = not self.game.stock
                success = self.game.draw()
                if success:
                    if was_empty_stock:
                        self.status_msg = "Recycled waste to stock."
                    else:
                        self.status_msg = "Drawn card."
                else:
                    self.status_msg = "Stock and Waste are empty."
                self.selected_pos = None  # Clear any active selection
            elif self.cursor_col == 1:
                # Waste pile selected or moved to
                if not self.game.waste:
                    self.status_msg = "Waste is empty."
                    return
                
                if self.selected_pos is None:
                    self.selected_pos = ('top', 1, len(self.game.waste) - 1)
                    self.status_msg = "Selected card from Waste. Choose target."
                else:
                    # You can't move anything onto the waste pile
                    self.status_msg = "Cannot move cards onto the Waste pile."
                    self.selected_pos = None
            elif self.cursor_col >= 3:
                found_idx = self.cursor_col - 3
                if self.selected_pos is None:
                    # Select from Foundation
                    if not self.game.foundations[found_idx]:
                        self.status_msg = "Foundation is empty."
                        return
                    self.selected_pos = ('top', self.cursor_col, len(self.game.foundations[found_idx]) - 1)
                    self.status_msg = f"Selected from Foundation {found_idx + 1}."
                else:
                    # Move to Foundation
                    src_zone, src_col, src_card_idx = self.selected_pos
                    success = False
                    if src_zone == 'top' and src_col == 1:
                        success = self.game.move_waste_to_foundation(found_idx)
                    elif src_zone == 'bottom':
                        success = self.game.move_tableau_to_foundation(src_col, found_idx)
                    
                    if success:
                        self.status_msg = f"Moved to Foundation {found_idx + 1}."
                        if self.game.check_win():
                            self.status_msg = "CONGRATULATIONS! YOU WON THE GAME!"
                    else:
                        self.status_msg = "Invalid move to Foundation."
                    self.selected_pos = None

        elif self.cursor_zone == 'bottom':
            dest_col = self.cursor_col
            if self.selected_pos is None:
                # Select Tableau pile/card
                pile = self.game.tableau[dest_col]
                if not pile:
                    self.status_msg = "Tableau column is empty."
                    return
                # Verify chosen card is face up
                if not pile[self.cursor_card_idx].is_face_up:
                    self.status_msg = "Cannot select face-down card."
                    return
                self.selected_pos = ('bottom', dest_col, self.cursor_card_idx)
                self.status_msg = f"Selected cards from Column {dest_col + 1}."
            else:
                # Execute move to Tableau column
                src_zone, src_col, src_card_idx = self.selected_pos
                success = False
                if src_zone == 'top' and src_col == 1:
                    success = self.game.move_waste_to_tableau(dest_col)
                elif src_zone == 'top' and src_col >= 3:
                    success = self.game.move_foundation_to_tableau(src_col - 3, dest_col)
                elif src_zone == 'bottom':
                    success = self.game.move_tableau_to_tableau(src_col, dest_col, src_card_idx)
                
                if success:
                    self.status_msg = "Move successful."
                else:
                    self.status_msg = "Invalid move."
                
                # Update cursor_card_idx to the new top card of destination
                new_len = len(self.game.tableau[dest_col])
                self.cursor_card_idx = max(0, new_len - 1)
                self.selected_pos = None

    def handle_auto_move(self) -> None:
        """Tries to automatically move the current highlighted card to any valid foundation."""
        success = False
        target_found = -1

        if self.cursor_zone == 'top' and self.cursor_col == 1:
            # Try to move from waste to any foundation
            for f_idx in range(4):
                if self.game.can_move_waste_to_foundation(f_idx):
                    success = self.game.move_waste_to_foundation(f_idx)
                    target_found = f_idx
                    break
        elif self.cursor_zone == 'bottom':
            # Try to move top card of tableau column to any foundation
            col = self.cursor_col
            if self.game.tableau[col]:
                for f_idx in range(4):
                    if self.game.can_move_tableau_to_foundation(col, f_idx):
                        success = self.game.move_tableau_to_foundation(col, f_idx)
                        target_found = f_idx
                        # Update cursor position
                        new_len = len(self.game.tableau[col])
                        self.cursor_card_idx = max(0, new_len - 1)
                        break

        if success:
            self.status_msg = f"Auto-moved card to Foundation {target_found + 1}."
            if self.game.check_win():
                self.status_msg = "CONGRATULATIONS! YOU WON THE GAME!"
        else:
            self.status_msg = "No valid Foundation move available."

    def draw_card_representation(self, stdscr, y: int, x: int, card: Optional[Card], is_cursor: bool, is_selected: bool) -> None:
        # Determine bracket symbols
        left_br, right_br = "[", "]"
        if is_selected:
            left_br, right_br = "*", "*"

        # Determine attributes
        attr = curses.A_NORMAL
        if is_cursor:
            attr |= curses.A_REVERSE

        if card is None:
            # Empty slot indicator
            color = curses.color_pair(5)  # Cyan
            stdscr.addstr(y, x, f"{left_br}  -{right_br}", color | attr)
        elif not card.is_face_up:
            # Face down card
            color = curses.color_pair(2)  # White/Standard
            stdscr.addstr(y, x, f"{left_br}###{right_br}", color | attr)
        else:
            # Face up card
            color = curses.color_pair(1) if card.is_red else curses.color_pair(2)
            suit_syms = {'H': 'H', 'D': 'D', 'C': 'C', 'S': 'S'}
            # If terminal supports unicode, we can use symbols
            try:
                suit_syms = {'H': '♥', 'D': '♦', 'C': '♣', 'S': '♠'}
            except Exception:
                pass
            
            sym = suit_syms.get(card.suit, card.suit)
            rank_lbl = card.label
            if len(rank_lbl) == 1:
                lbl = f" {rank_lbl}{sym}"
            else:
                lbl = f"{rank_lbl}{sym}"
                
            stdscr.addstr(y, x, left_br, attr)
            stdscr.addstr(y, x + 1, lbl, color | attr)
            stdscr.addstr(y, x + 4, right_br, attr)

    def draw_screen(self, stdscr) -> None:
        try:
            stdscr.erase()
            h, w = stdscr.getmaxyx()
            
            # Check window size
            if h < 20 or w < 60:
                try:
                    stdscr.addstr(0, 0, "Terminal too small. Please enlarge to at least 80x24.", curses.color_pair(1))
                except curses.error:
                    pass
                stdscr.refresh()
                return

            # Title / Help Banner
            stdscr.addstr(0, 2, "KLONDIKE SOLITAIRE", curses.color_pair(4) | curses.A_BOLD)
            help_text = "Arrows/Vim:Move | Space/Enter:Select/Draw | A:Auto-Move | U:Undo | R:Restart | Q:Quit"
            stdscr.addstr(1, 2, help_text[:w-3], curses.color_pair(3))

            # Top row elements positioning
            # Stock (col 0), Waste (col 1), gap (col 2), Foundations 0-3 (cols 3-6)
            x_coords = [2 + i * 8 for i in range(7)]

            # --- Draw STOCK ---
            is_cursor = (self.cursor_zone == 'top' and self.cursor_col == 0)
            is_selected = False  # Stock can never be selected
            stdscr.addstr(3, x_coords[0], "STOCK", curses.color_pair(3))
            
            if self.game.stock:
                # Top card of Stock is face down
                self.draw_card_representation(stdscr, 4, x_coords[0], Card('S', 1, False), is_cursor, is_selected)
            else:
                self.draw_card_representation(stdscr, 4, x_coords[0], None, is_cursor, is_selected)

            # --- Draw WASTE ---
            is_cursor = (self.cursor_zone == 'top' and self.cursor_col == 1)
            is_selected = (self.selected_pos is not None and self.selected_pos[0] == 'top' and self.selected_pos[1] == 1)
            stdscr.addstr(3, x_coords[1], "WASTE", curses.color_pair(3))
            
            if self.game.waste:
                self.draw_card_representation(stdscr, 4, x_coords[1], self.game.waste[-1], is_cursor, is_selected)
            else:
                self.draw_card_representation(stdscr, 4, x_coords[1], None, is_cursor, is_selected)

            # --- Draw FOUNDATIONS ---
            for i in range(4):
                col_idx = 3 + i
                is_cursor = (self.cursor_zone == 'top' and self.cursor_col == col_idx)
                is_selected = (self.selected_pos is not None and self.selected_pos[0] == 'top' and self.selected_pos[1] == col_idx)
                stdscr.addstr(3, x_coords[col_idx], f"FOUND {i+1}", curses.color_pair(3))
                
                pile = self.game.foundations[i]
                if pile:
                    self.draw_card_representation(stdscr, 4, x_coords[col_idx], pile[-1], is_cursor, is_selected)
                else:
                    self.draw_card_representation(stdscr, 4, x_coords[col_idx], None, is_cursor, is_selected)

            # --- Draw TABLEAU ---
            stdscr.addstr(6, 2, "TABLEAU COLUMNS:", curses.color_pair(4))
            for col_idx in range(7):
                pile = self.game.tableau[col_idx]
                x = x_coords[col_idx]
                
                # Label
                stdscr.addstr(7, x, f"COL {col_idx+1}", curses.color_pair(3))
                
                if not pile:
                    is_cursor = (self.cursor_zone == 'bottom' and self.cursor_col == col_idx)
                    is_selected = (self.selected_pos is not None and self.selected_pos[0] == 'bottom' and self.selected_pos[1] == col_idx)
                    self.draw_card_representation(stdscr, 8, x, None, is_cursor, is_selected)
                else:
                    for card_idx, card in enumerate(pile):
                        y = 8 + card_idx
                        # Check cursor highlighting
                        is_cursor = (
                            self.cursor_zone == 'bottom' and 
                            self.cursor_col == col_idx and 
                            self.cursor_card_idx == card_idx
                        )
                        # Check selection highlighting
                        # If this column is selected as source, we highlight the selected card and all cards below it!
                        is_selected = False
                        if self.selected_pos is not None:
                            src_zone, src_col, src_card_idx = self.selected_pos
                            if src_zone == 'bottom' and src_col == col_idx and card_idx >= src_card_idx:
                                is_selected = True

                        self.draw_card_representation(stdscr, y, x, card, is_cursor, is_selected)

            # Draw Status Bar at bottom
            stdscr.addstr(h - 2, 2, f"Status: {self.status_msg}"[:w-3], curses.color_pair(4) | curses.A_BOLD)
            
            # Draw game seed info or undo history count
            info_str = f"Undos available: {len(self.game.history)}"
            if w > len(info_str) + 10:
                stdscr.addstr(h - 2, w - len(info_str) - 3, info_str, curses.color_pair(3))

            stdscr.refresh()
        except curses.error:
            pass

    def run(self, stdscr) -> None:
        # Initialize color scheme
        if curses.has_colors():
            curses.start_color()
            curses.init_pair(1, curses.COLOR_RED, curses.COLOR_BLACK)     # Hearts/Diamonds
            curses.init_pair(2, curses.COLOR_WHITE, curses.COLOR_BLACK)   # Spades/Clubs
            curses.init_pair(3, curses.COLOR_YELLOW, curses.COLOR_BLACK)  # Navigation/Headers
            curses.init_pair(4, curses.COLOR_GREEN, curses.COLOR_BLACK)   # Success/Banners
            curses.init_pair(5, curses.COLOR_CYAN, curses.COLOR_BLACK)    # Empty Slots

        curses.curs_set(0)
        stdscr.keypad(True)

        while True:
            self.draw_screen(stdscr)
            try:
                ch = stdscr.getch()
            except KeyboardInterrupt:
                break

            if ch == -1:
                continue

            # Handle Quit
            if ch in (ord('q'), ord('Q')):
                break

            # Handle Movement
            elif ch == curses.KEY_LEFT or ch == ord('h'):
                self.move_cursor('left')
            elif ch == curses.KEY_RIGHT or ch == ord('l'):
                self.move_cursor('right')
            elif ch == curses.KEY_UP or ch == ord('k'):
                self.move_cursor('up')
            elif ch == curses.KEY_DOWN or ch == ord('j'):
                self.move_cursor('down')

            # Handle Selection / Draw / Move
            elif ch in (ord(' '), 10, 13, curses.KEY_ENTER):  # Space, Enter, Return
                self.handle_action()

            # Handle Cancel selection
            elif ch in (27, ord('c'), ord('C')):  # Escape, 'c', 'C'
                self.selected_pos = None
                self.status_msg = "Selection cancelled."

            # Handle Auto-move
            elif ch in (ord('a'), ord('A')):
                self.handle_auto_move()

            # Handle Undo
            elif ch in (ord('u'), ord('U')):
                if self.game.undo():
                    self.status_msg = "Last move undone."
                    self.selected_pos = None
                    # Make sure cursor positions are validated against new column lens
                    if self.cursor_zone == 'bottom':
                        pile_len = len(self.game.tableau[self.cursor_col])
                        self.cursor_card_idx = min(self.cursor_card_idx, max(0, pile_len - 1))
                else:
                    self.status_msg = "Nothing to undo."

            # Handle Restart
            elif ch in (ord('r'), ord('R')):
                self.game.deal()
                self.cursor_zone = 'top'
                self.cursor_col = 0
                self.cursor_card_idx = 0
                self.selected_pos = None
                self.status_msg = "New game started!"

            # Handle Help
            elif ch in (ord('?'), ord('H')):
                self.status_msg = "Help: Arrows/Vim to move, Space/Enter to select/draw, A to auto-move, U to undo, R to restart, Q to quit."

            # Handle Resize
            elif ch == curses.KEY_RESIZE:
                stdscr.clear()
                stdscr.refresh()
