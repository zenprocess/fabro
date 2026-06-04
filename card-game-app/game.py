import random
import copy

class Card:
    SUIT_SYMBOLS = {'H': '♥', 'D': '♦', 'C': '♣', 'S': '♠'}
    SUIT_NAMES = {'H': 'Hearts', 'D': 'Diamonds', 'C': 'Clubs', 'S': 'Spades'}
    RANK_NAMES = {
        1: 'A', 2: '2', 3: '3', 4: '4', 5: '5', 6: '6', 7: '7', 8: '8', 9: '9', 10: '10',
        11: 'J', 12: 'Q', 13: 'K'
    }

    def __init__(self, suit: str, rank: int):
        self.suit = suit  # 'H', 'D', 'C', 'S'
        self.rank = rank  # 1 to 13
        self.color = 'red' if suit in ['H', 'D'] else 'black'

    @property
    def rank_name(self) -> str:
        return self.RANK_NAMES[self.rank]

    @property
    def suit_symbol(self) -> str:
        return self.SUIT_SYMBOLS[self.suit]

    def __repr__(self) -> str:
        return f"{self.rank_name}{self.suit}"

    def __eq__(self, other) -> bool:
        if not isinstance(other, Card):
            return False
        return self.suit == other.suit and self.rank == other.rank

class Deck:
    def __init__(self, seed=None):
        self.cards = [Card(suit, rank) for suit in ['H', 'D', 'C', 'S'] for rank in range(1, 14)]
        if seed is not None:
            random.seed(seed)
        random.shuffle(self.cards)

    def deal(self) -> list[list[Card]]:
        # Deal into 8 tableaus
        tableaus = [[] for _ in range(8)]
        for idx, card in enumerate(self.cards):
            tableaus[idx % 8].append(card)
        return tableaus

class FreeCellGame:
    def __init__(self, seed=None):
        self.seed = seed
        self.reset()

    def reset(self):
        deck = Deck(self.seed)
        self.tableaus = deck.deal()
        self.free_cells = [None] * 4
        self.foundations = {'H': [], 'D': [], 'C': [], 'S': []}
        self.history = []  # Stack of states for undo

    def get_state(self):
        # Return deep copy of the state for undo history
        return {
            'tableaus': copy.deepcopy(self.tableaus),
            'free_cells': copy.deepcopy(self.free_cells),
            'foundations': copy.deepcopy(self.foundations)
        }

    def restore_state(self, state):
        self.tableaus = copy.deepcopy(state['tableaus'])
        self.free_cells = copy.deepcopy(state['free_cells'])
        self.foundations = copy.deepcopy(state['foundations'])

    def save_to_history(self):
        self.history.append(self.get_state())

    def undo(self) -> bool:
        if not self.history:
            return False
        state = self.history.pop()
        self.restore_state(state)
        return True

    def is_won(self) -> bool:
        # All foundations must have 13 cards (ending with King)
        return all(len(self.foundations[suit]) == 13 for suit in ['H', 'D', 'C', 'S'])

    def get_empty_free_cells_count(self) -> int:
        return sum(1 for c in self.free_cells if c is None)

    def get_empty_tableaus_count(self, exclude_idx=None) -> int:
        count = 0
        for idx, tab in enumerate(self.tableaus):
            if idx == exclude_idx:
                continue
            if not tab:
                count += 1
        return count

    def is_valid_sequence(self, cards: list[Card]) -> bool:
        if not cards:
            return False
        for i in range(len(cards) - 1):
            curr = cards[i]
            nxt = cards[i + 1]
            if curr.rank != nxt.rank + 1:
                return False
            if curr.color == nxt.color:
                return False
        return True

    def can_place_on_tableau(self, card: Card, tab_idx: int) -> bool:
        tab = self.tableaus[tab_idx]
        if not tab:
            return True
        top_card = tab[-1]
        return top_card.rank == card.rank + 1 and top_card.color != card.color

    def can_place_on_foundation(self, card: Card, f_idx: int) -> bool:
        # f_idx is 0-3 corresponding to H, D, C, S
        suits = ['H', 'D', 'C', 'S']
        suit = suits[f_idx]
        if card.suit != suit:
            return False
        found_cards = self.foundations[suit]
        if not found_cards:
            return card.rank == 1  # Must be Ace
        return card.rank == found_cards[-1].rank + 1

    def move_card_to_free_cell(self, src_type: str, src_idx: int, fc_idx: int) -> bool:
        # Validate target
        if self.free_cells[fc_idx] is not None:
            return False

        # Get card
        card = self._peek_card(src_type, src_idx)
        if not card:
            return False

        # Save history before change
        self.save_to_history()

        # Remove from src
        self._pop_card(src_type, src_idx)
        # Put in free cell
        self.free_cells[fc_idx] = card
        return True

    def move_card_to_foundation(self, src_type: str, src_idx: int, f_idx: int) -> bool:
        card = self._peek_card(src_type, src_idx)
        if not card:
            return False

        if not self.can_place_on_foundation(card, f_idx):
            return False

        self.save_to_history()
        self._pop_card(src_type, src_idx)
        self.foundations[card.suit].append(card)
        return True

    def move_to_tableau(self, src_type: str, src_idx: int, dest_tab_idx: int) -> bool:
        if src_type == 'tableau':
            if src_idx == dest_tab_idx:
                return False
            # Multi-card sequence movement validation
            src_tab = self.tableaus[src_idx]
            if not src_tab:
                return False

            # We want to find the largest valid sub-sequence we can move
            empty_free_cells = self.get_empty_free_cells_count()
            dest_is_empty = len(self.tableaus[dest_tab_idx]) == 0
            empty_tableaus = self.get_empty_tableaus_count(exclude_idx=dest_tab_idx)

            limit = (empty_free_cells + 1) * (2 ** empty_tableaus)

            # Try to find the largest sub-sequence that is valid and fits
            for L in range(len(src_tab), 0, -1):
                sub_seq = src_tab[-L:]
                if not self.is_valid_sequence(sub_seq):
                    continue
                if L > limit:
                    continue
                if self.can_place_on_tableau(sub_seq[0], dest_tab_idx):
                    # Valid move!
                    self.save_to_history()
                    # Pop L cards
                    self.tableaus[src_idx] = src_tab[:-L]
                    # Append to destination
                    self.tableaus[dest_tab_idx].extend(sub_seq)
                    return True
            return False

        else:
            # Single card from free_cell or foundation
            card = self._peek_card(src_type, src_idx)
            if not card:
                return False

            if not self.can_place_on_tableau(card, dest_tab_idx):
                return False

            self.save_to_history()
            self._pop_card(src_type, src_idx)
            self.tableaus[dest_tab_idx].append(card)
            return True

    def has_any_valid_moves(self) -> bool:
        # Check if any cards from free cells or tableaus can move to foundations or other tableaus or free cells
        suits = ['H', 'D', 'C', 'S']
        
        # Helper to check if a single card can move anywhere
        def check_card_moves(card, src_type, src_idx):
            # To Free Cells
            if src_type != 'free_cell':
                for fc_idx in range(4):
                    if self.free_cells[fc_idx] is None:
                        return True
            # To Foundations
            for f_idx in range(4):
                if self.can_place_on_foundation(card, f_idx):
                    return True
            # To Tableaus
            for t_idx in range(8):
                if src_type == 'tableau' and src_idx == t_idx:
                    continue
                if self.can_place_on_tableau(card, t_idx):
                    return True
            return False

        # 1. Check free cells
        for idx, card in enumerate(self.free_cells):
            if card and check_card_moves(card, 'free_cell', idx):
                return True

        # 2. Check tableaus bottom cards
        for idx, tab in enumerate(self.tableaus):
            if not tab:
                continue
            card = tab[-1]
            if check_card_moves(card, 'tableau', idx):
                return True

        # 3. Check tableau sequences (can we move any valid sequence to another tableau?)
        for src_idx, src_tab in enumerate(self.tableaus):
            if not src_tab:
                continue
            empty_free_cells = self.get_empty_free_cells_count()
            
            for dest_idx, dest_tab in enumerate(self.tableaus):
                if src_idx == dest_idx:
                    continue
                
                dest_is_empty = len(dest_tab) == 0
                empty_tableaus = self.get_empty_tableaus_count(exclude_idx=dest_idx)
                limit = (empty_free_cells + 1) * (2 ** empty_tableaus)

                for L in range(len(src_tab), 1, -1):  # L > 1 since L=1 is checked by step 2
                    if L > limit:
                        continue
                    sub_seq = src_tab[-L:]
                    if not self.is_valid_sequence(sub_seq):
                        continue
                    if self.can_place_on_tableau(sub_seq[0], dest_idx):
                        return True

        return False

    def auto_solve_step(self) -> bool:
        # Automatically move cards to foundations if they are safe to do so
        # A card is safe to move to a foundation if both cards of lower rank of opposite color are already in the foundations
        # (meaning there's no need to build on them in the tableaus).
        # For simplicity and classic play, we can just auto-move any card that can legally go to foundations
        # if it's an Ace or 2, or if cards of lower rank are already at foundations.
        # Let's write a simple auto-foundation-move step and return True if a move was made.
        suits = ['H', 'D', 'C', 'S']
        
        def is_safe_for_foundation(card: Card) -> bool:
            # Aces and 2s are always safe
            if card.rank <= 2:
                return True
            # Otherwise, check the ranks of opposite color suits in foundation.
            # e.g., if card is 5 of Hearts (Red), opposite colors are Clubs (Black) and Spades (Black).
            # If both Black foundations have at least rank 4, then 5 of Hearts is safe because no black cards
            # of rank 4 or lower can ever need to be placed on 5 of Hearts.
            opp_suits = ['C', 'S'] if card.color == 'red' else ['H', 'D']
            opp_rank_limit = card.rank - 1
            for os in opp_suits:
                f_cards = self.foundations[os]
                if not f_cards or f_cards[-1].rank < opp_rank_limit:
                    return False
            return True

        # Check Free Cells
        for idx, card in enumerate(self.free_cells):
            if card:
                for f_idx, suit in enumerate(suits):
                    if self.can_place_on_foundation(card, f_idx) and is_safe_for_foundation(card):
                        self.move_card_to_foundation('free_cell', idx, f_idx)
                        return True

        # Check Tableaus
        for idx, tab in enumerate(self.tableaus):
            if tab:
                card = tab[-1]
                for f_idx, suit in enumerate(suits):
                    if self.can_place_on_foundation(card, f_idx) and is_safe_for_foundation(card):
                        self.move_card_to_foundation('tableau', idx, f_idx)
                        return True

        return False

    def auto_solve_loop(self) -> int:
        moves_made = 0
        while self.auto_solve_step():
            moves_made += 1
        return moves_made

    def _peek_card(self, src_type: str, src_idx: int) -> Card | None:
        if src_type == 'free_cell':
            return self.free_cells[src_idx]
        elif src_type == 'foundation':
            suits = ['H', 'D', 'C', 'S']
            suit = suits[src_idx]
            found = self.foundations[suit]
            return found[-1] if found else None
        elif src_type == 'tableau':
            tab = self.tableaus[src_idx]
            return tab[-1] if tab else None
        return None

    def _pop_card(self, src_type: str, src_idx: int) -> Card | None:
        if src_type == 'free_cell':
            card = self.free_cells[src_idx]
            self.free_cells[src_idx] = None
            return card
        elif src_type == 'foundation':
            suits = ['H', 'D', 'C', 'S']
            suit = suits[src_idx]
            return self.foundations[suit].pop() if self.foundations[suit] else None
        elif src_type == 'tableau':
            return self.tableaus[src_idx].pop() if self.tableaus[src_idx] else None
        return None
