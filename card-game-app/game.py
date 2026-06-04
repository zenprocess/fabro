import random
from typing import List, Dict, Optional, Tuple

class Card:
    SUITS = {
        'H': '♥',  # Hearts (Red)
        'D': '♦',  # Diamonds (Red)
        'C': '♣',  # Clubs (Black)
        'S': '♠'   # Spades (Black)
    }
    
    RANKS = {
        1: 'A', 2: '2', 3: '3', 4: '4', 5: '5', 6: '6', 7: '7',
        8: '8', 9: '9', 10: '10', 11: 'J', 12: 'Q', 13: 'K'
    }

    def __init__(self, suit: str, rank: int, face_up: bool = False):
        if suit not in self.SUITS:
            raise ValueError(f"Invalid suit: {suit}")
        if rank not in self.RANKS:
            raise ValueError(f"Invalid rank: {rank}")
        self.suit = suit
        self.rank = rank
        self.face_up = face_up

    @property
    def color(self) -> str:
        return 'red' if self.suit in ('H', 'D') else 'black'

    @property
    def suit_symbol(self) -> str:
        return self.SUITS[self.suit]

    @property
    def rank_symbol(self) -> str:
        return self.RANKS[self.rank]

    def __repr__(self) -> str:
        status = "↑" if self.face_up else "↓"
        return f"{self.rank_symbol}{self.suit_symbol}{status}"

    def to_string(self) -> str:
        if self.face_up:
            return f"{self.rank_symbol}{self.suit_symbol}"
        return "[ █ ]"


class GameState:
    def __init__(self, seed: Optional[int] = None):
        self.seed = seed
        self.stock: List[Card] = []
        self.waste: List[Card] = []
        # Foundations: 4 piles, indexed 0 to 3. Each starts empty.
        # Associated with H, D, C, S respectively.
        self.foundations: List[List[Card]] = [[], [], [], []]
        self.foundation_suits = ['H', 'D', 'C', 'S']
        # Tableaus: 7 piles.
        self.tableaus: List[List[Card]] = [[] for _ in range(7)]
        
        # Cursor and Selection State
        # row: 0 (top row: Stock, Waste, spacer, F0-F3), 1 (bottom row: T0-T6)
        # col: 0 to 6
        self.cursor_row = 1
        self.cursor_col = 0
        # For tableau columns, we can select a specific card index
        self.cursor_card_idx = 0
        
        # Selection: tuple of (row, col, card_idx) or None
        self.selected: Optional[Tuple[int, int, int]] = None
        
        self.message = "Welcome to Solitaire! Use arrows to navigate, Space/Enter to select/move."
        self.reset()

    def reset(self):
        # Create deck
        deck = []
        for suit in ['H', 'D', 'C', 'S']:
            for rank in range(1, 14):
                deck.append(Card(suit, rank, face_up=False))
        
        if self.seed is not None:
            random.seed(self.seed)
        else:
            random.seed()
        random.shuffle(deck)
        
        # Deal Tableaus
        # T0 gets 1 card, T1 gets 2, ..., T6 gets 7
        self.tableaus = [[] for _ in range(7)]
        for i in range(7):
            for j in range(i + 1):
                card = deck.pop()
                if j == i:
                    card.face_up = True
                self.tableaus[i].append(card)
        
        # Remaining cards go to stock
        self.stock = deck
        self.waste = []
        self.foundations = [[], [], [], []]
        
        # Reset selection and cursor
        self.cursor_row = 1
        self.cursor_col = 0
        self.cursor_card_idx = len(self.tableaus[0]) - 1 if self.tableaus[0] else 0
        self.selected = None
        self.message = "Game reset. New shuffle!"

    def get_first_face_up_idx(self, tableau_idx: int) -> int:
        tableau = self.tableaus[tableau_idx]
        for idx, card in enumerate(tableau):
            if card.face_up:
                return idx
        return -1

    def move_cursor(self, direction: str):
        """
        Moves the cursor around the 2x7 grid.
        Row 0: 0=Stock, 1=Waste, 2=Spacer, 3=F0, 4=F1, 5=F2, 6=F3
        Row 1: 0=Tableau 0, 1=Tableau 1, ..., 6=Tableau 6
        """
        if direction == 'left':
            new_col = self.cursor_col - 1
            if self.cursor_row == 0 and new_col == 2:
                new_col = 1  # Skip spacer going left
            if new_col < 0:
                new_col = 0
            self.cursor_col = new_col
            # Update card index if entering Tableau
            if self.cursor_row == 1:
                t_len = len(self.tableaus[self.cursor_col])
                self.cursor_card_idx = max(0, t_len - 1)
                
        elif direction == 'right':
            new_col = self.cursor_col + 1
            if self.cursor_row == 0 and new_col == 2:
                new_col = 3  # Skip spacer going right
            if new_col > 6:
                new_col = 6
            self.cursor_col = new_col
            # Update card index if entering Tableau
            if self.cursor_row == 1:
                t_len = len(self.tableaus[self.cursor_col])
                self.cursor_card_idx = max(0, t_len - 1)
                
        elif direction == 'up':
            if self.cursor_row == 1:
                # If on a tableau, try to move UP the face-up stack first
                first_face_up = self.get_first_face_up_idx(self.cursor_col)
                if first_face_up != -1 and self.cursor_card_idx > first_face_up:
                    self.cursor_card_idx -= 1
                else:
                    # Move to top row
                    self.cursor_row = 0
                    if self.cursor_col == 2:
                        self.cursor_col = 1  # Don't land on spacer
            else:
                # Already on row 0, do nothing
                pass
                
        elif direction == 'down':
            if self.cursor_row == 0:
                self.cursor_row = 1
                t_len = len(self.tableaus[self.cursor_col])
                self.cursor_card_idx = max(0, t_len - 1)
            else:
                # Already on row 1, try to move DOWN the face-up stack
                t_len = len(self.tableaus[self.cursor_col])
                if self.cursor_card_idx < t_len - 1:
                    self.cursor_card_idx += 1

    def select_or_move(self):
        """
        Handles Space/Enter selection logic.
        """
        row = self.cursor_row
        col = self.cursor_col
        
        # 1. Clicking on Stock (Row 0, Col 0)
        if row == 0 and col == 0:
            self.selected = None  # Clear any active selection
            self.draw_card()
            return
            
        # 2. Clicking on Spacer (Row 0, Col 2) -> Do nothing
        if row == 0 and col == 2:
            return

        # 3. If no active selection, try to select
        if self.selected is None:
            if row == 0:
                if col == 1: # Waste
                    if self.waste:
                        self.selected = (row, col, len(self.waste) - 1)
                        self.message = "Selected Waste card. Choose destination."
                    else:
                        self.message = "Waste is empty!"
                elif col >= 3: # Foundations
                    f_idx = col - 3
                    if self.foundations[f_idx]:
                        self.selected = (row, col, len(self.foundations[f_idx]) - 1)
                        self.message = f"Selected top of Foundation {self.foundation_suits[f_idx]}. Choose destination."
                    else:
                        self.message = "Foundation is empty!"
            elif row == 1: # Tableaus
                t_idx = col
                t_len = len(self.tableaus[t_idx])
                if t_len > 0:
                    # Ensure cursor card index is valid and face-up
                    first_face_up = self.get_first_face_up_idx(t_idx)
                    if first_face_up != -1 and self.cursor_card_idx >= first_face_up:
                        self.selected = (row, col, self.cursor_card_idx)
                        card = self.tableaus[t_idx][self.cursor_card_idx]
                        self.message = f"Selected {card.rank_symbol}{card.suit_symbol} from Tableau {t_idx + 1}."
                    else:
                        # Fallback to top card if cursor card index was somehow out of sync or face-down
                        self.selected = (row, col, t_len - 1)
                        card = self.tableaus[t_idx][-1]
                        self.message = f"Selected {card.rank_symbol}{card.suit_symbol} from Tableau {t_idx + 1}."
                else:
                    self.message = "Tableau is empty!"
                    
        # 4. If there is an active selection, try to perform the move
        else:
            src_row, src_col, src_card_idx = self.selected
            
            # If selecting the same pile/card, deselect
            if src_row == row and src_col == col:
                self.selected = None
                self.message = "Selection canceled."
                return
                
            success = False
            
            # Case A: Source is Waste
            if src_row == 0 and src_col == 1:
                if row == 1:  # To Tableau
                    success = self.move_waste_to_tableau(col)
                elif row == 0 and col >= 3:  # To Foundation
                    success = self.move_waste_to_foundation(col - 3)
                    
            # Case B: Source is Foundation
            elif src_row == 0 and src_col >= 3:
                src_f_idx = src_col - 3
                if row == 1:  # To Tableau
                    success = self.move_foundation_to_tableau(src_f_idx, col)
                    
            # Case C: Source is Tableau
            elif src_row == 1:
                if row == 1:  # To Tableau
                    success = self.move_tableau_to_tableau(src_col, src_card_idx, col)
                elif row == 0 and col >= 3:  # To Foundation
                    # Ensure we are only moving the top single card to Foundation
                    if src_card_idx == len(self.tableaus[src_col]) - 1:
                        success = self.move_tableau_to_foundation(src_col, col - 3)
                    else:
                        self.message = "Can only move the single top card of a Tableau to Foundation!"
                        self.selected = None
                        return
            
            if success:
                self.message = "Valid move completed!"
            else:
                self.message = "Invalid move!"
                
            self.selected = None
            # Update cursor card index for tableaus to the top card
            if self.cursor_row == 1:
                t_len = len(self.tableaus[self.cursor_col])
                self.cursor_card_idx = max(0, t_len - 1)

    # --- Core Move Operations ---
    
    def draw_card(self) -> bool:
        """Draws a card from stock to waste, or recycles waste if stock is empty."""
        if self.stock:
            card = self.stock.pop()
            card.face_up = True
            self.waste.append(card)
            self.message = f"Drew {card.rank_symbol}{card.suit_symbol}."
            return True
        elif self.waste:
            # Recycle waste back to stock
            # Reverse waste, turn face_down
            self.stock = list(reversed(self.waste))
            for card in self.stock:
                card.face_up = False
            self.waste = []
            # and immediately draw the first card
            card = self.stock.pop()
            card.face_up = True
            self.waste.append(card)
            self.message = "Recycled waste pile back to stock and drew first card."
            return True
        else:
            self.message = "No cards left in Stock or Waste!"
            return False

    def move_waste_to_tableau(self, dest_idx: int) -> bool:
        if not self.waste:
            return False
        card = self.waste[-1]
        dest_pile = self.tableaus[dest_idx]
        
        if not dest_pile:
            # Empty tableau accepts only Kings
            if card.rank == 13:
                dest_pile.append(self.waste.pop())
                return True
        else:
            dest_card = dest_pile[-1]
            if dest_card.face_up and card.color != dest_card.color and card.rank == dest_card.rank - 1:
                dest_pile.append(self.waste.pop())
                return True
        return False

    def move_waste_to_foundation(self, f_idx: int) -> bool:
        if not self.waste:
            return False
        card = self.waste[-1]
        f_suit = self.foundation_suits[f_idx]
        dest_pile = self.foundations[f_idx]
        
        if card.suit != f_suit:
            return False
            
        if not dest_pile:
            # Empty foundation accepts only Ace
            if card.rank == 1:
                dest_pile.append(self.waste.pop())
                return True
        else:
            top_card = dest_pile[-1]
            if card.rank == top_card.rank + 1:
                dest_pile.append(self.waste.pop())
                return True
        return False

    def move_foundation_to_tableau(self, f_idx: int, dest_idx: int) -> bool:
        f_pile = self.foundations[f_idx]
        if not f_pile:
            return False
        card = f_pile[-1]
        dest_pile = self.tableaus[dest_idx]
        
        if not dest_pile:
            # Empty tableau accepts only Kings
            if card.rank == 13:
                dest_pile.append(f_pile.pop())
                return True
        else:
            dest_card = dest_pile[-1]
            if dest_card.face_up and card.color != dest_card.color and card.rank == dest_card.rank - 1:
                dest_pile.append(f_pile.pop())
                return True
        return False

    def move_tableau_to_tableau(self, src_idx: int, card_idx: int, dest_idx: int) -> bool:
        src_pile = self.tableaus[src_idx]
        dest_pile = self.tableaus[dest_idx]
        
        if not src_pile or card_idx >= len(src_pile):
            return False
            
        moving_stack = src_pile[card_idx:]
        # Ensure all moving cards are face up
        if not all(c.face_up for c in moving_stack):
            return False
            
        first_moving_card = moving_stack[0]
        
        if not dest_pile:
            # Empty tableau accepts only Kings
            if first_moving_card.rank == 13:
                self.tableaus[dest_idx].extend(moving_stack)
                del src_pile[card_idx:]
                self.auto_flip_top(src_idx)
                return True
        else:
            dest_card = dest_pile[-1]
            if dest_card.face_up and first_moving_card.color != dest_card.color and first_moving_card.rank == dest_card.rank - 1:
                self.tableaus[dest_idx].extend(moving_stack)
                del src_pile[card_idx:]
                self.auto_flip_top(src_idx)
                return True
        return False

    def move_tableau_to_foundation(self, src_idx: int, f_idx: int) -> bool:
        src_pile = self.tableaus[src_idx]
        if not src_pile:
            return False
        card = src_pile[-1]
        f_suit = self.foundation_suits[f_idx]
        dest_pile = self.foundations[f_idx]
        
        if card.suit != f_suit:
            return False
            
        if not dest_pile:
            # Empty foundation accepts only Ace
            if card.rank == 1:
                dest_pile.append(src_pile.pop())
                self.auto_flip_top(src_idx)
                return True
        else:
            top_card = dest_pile[-1]
            if card.rank == top_card.rank + 1:
                dest_pile.append(src_pile.pop())
                self.auto_flip_top(src_idx)
                return True
        return False

    def auto_flip_top(self, tableau_idx: int):
        """If a tableau top card is face down, flip it face up."""
        pile = self.tableaus[tableau_idx]
        if pile and not pile[-1].face_up:
            pile[-1].face_up = True

    def check_win(self) -> bool:
        """The game is won if all four foundations have 13 cards."""
        return all(len(f) == 13 for f in self.foundations)
