import random
import copy

class Card:
    SUITS = ['C', 'D', 'H', 'S']  # Clubs, Diamonds, Hearts, Spades
    SUIT_SYMBOLS = {
        'C': '♣',
        'D': '♦',
        'H': '♥',
        'S': '♠'
    }
    RANK_SYMBOLS = {
        1: 'A',
        2: '2',
        3: '3',
        4: '4',
        5: '5',
        6: '6',
        7: '7',
        8: '8',
        9: '9',
        10: '10',
        11: 'J',
        12: 'Q',
        13: 'K'
    }

    def __init__(self, suit, rank):
        if suit not in self.SUITS:
            raise ValueError(f"Invalid suit: {suit}")
        if not (1 <= rank <= 13):
            raise ValueError(f"Invalid rank: {rank}")
        self.suit = suit
        self.rank = rank

    @property
    def color(self):
        # Red ('R') for Diamonds/Hearts, Black ('B') for Clubs/Spades
        return 'R' if self.suit in ['D', 'H'] else 'B'

    @property
    def symbol(self):
        return self.SUIT_SYMBOLS[self.suit]

    @property
    def rank_str(self):
        return self.RANK_SYMBOLS[self.rank]

    def __repr__(self):
        return f"{self.rank_str}{self.symbol}"

    def __eq__(self, other):
        if not isinstance(other, Card):
            return False
        return self.suit == other.suit and self.rank == other.rank


class Deck:
    def __init__(self):
        self.cards = [Card(suit, rank) for suit in Card.SUITS for rank in range(1, 14)]

    def shuffle(self, seed=None):
        if seed is not None:
            random.seed(seed)
        random.shuffle(self.cards)


class GameState:
    def __init__(self):
        self.tableaux = [[] for _ in range(8)]
        self.freecells = [None] * 4
        self.foundations = {suit: 0 for suit in Card.SUITS}
        self.history = []

    def deal(self, seed=None):
        deck = Deck()
        deck.shuffle(seed)
        
        self.tableaux = [[] for _ in range(8)]
        self.freecells = [None] * 4
        self.foundations = {suit: 0 for suit in Card.SUITS}
        self.history = []

        # Deal standard 52 cards: 8 columns.
        # Columns 0-3 get 7 cards, columns 4-7 get 6 cards.
        for i, card in enumerate(deck.cards):
            col_idx = i % 8
            self.tableaux[col_idx].append(card)

    def is_won(self):
        # Game is won when all 4 foundations reach King (13)
        return all(rank == 13 for rank in self.foundations.values())

    def push_history(self):
        # Push deep copy of current state to history for undo
        state_snapshot = {
            'tableaux': copy.deepcopy(self.tableaux),
            'freecells': copy.deepcopy(self.freecells),
            'foundations': copy.deepcopy(self.foundations)
        }
        self.history.append(state_snapshot)

    def undo(self):
        if not self.history:
            return False
        snapshot = self.history.pop()
        self.tableaux = snapshot['tableaux']
        self.freecells = snapshot['freecells']
        self.foundations = snapshot['foundations']
        return True

    def get_empty_freecells_count(self):
        return sum(1 for card in self.freecells if card is None)

    def get_empty_tableaux_count(self):
        return sum(1 for col in self.tableaux if len(col) == 0)

    def get_max_movable_size(self, moving_to_empty_tableau=False):
        empty_fc = self.get_empty_freecells_count()
        empty_tab = self.get_empty_tableaux_count()
        
        # If moving to an empty column, that column cannot count as a temporary column
        if moving_to_empty_tableau:
            empty_tab = max(0, empty_tab - 1)
            
        return (1 + empty_fc) * (2 ** empty_tab)

    def get_longest_alternating_sequence_at_bottom(self, col_idx):
        col = self.tableaux[col_idx]
        if not col:
            return []
        
        sequence = [col[-1]]
        for i in range(len(col) - 2, -1, -1):
            curr = col[i]
            prev = sequence[-1]
            if curr.color != prev.color and curr.rank == prev.rank + 1:
                sequence.append(curr)
            else:
                break
        
        sequence.reverse()
        return sequence

    def move_card_to_freecell(self, src_type, src_idx, fc_idx):
        """
        src_type: 'tableau' or 'freecell'
        src_idx: index of source tableau (0-7) or freecell (0-3)
        fc_idx: index of destination freecell (0-3)
        """
        if fc_idx < 0 or fc_idx > 3:
            return False, "Invalid FreeCell index."
        if self.freecells[fc_idx] is not None:
            return False, f"FreeCell {fc_idx+1} is already occupied."

        if src_type == 'tableau':
            if src_idx < 0 or src_idx > 7:
                return False, "Invalid Tableau index."
            col = self.tableaux[src_idx]
            if not col:
                return False, "Source Tableau is empty."
            card = col[-1]
            
            # Valid move, record history
            self.push_history()
            col.pop()
            self.freecells[fc_idx] = card
            return True, f"Moved {card} to FreeCell {fc_idx+1}."

        elif src_type == 'freecell':
            if src_idx < 0 or src_idx > 3:
                return False, "Invalid source FreeCell index."
            if src_idx == fc_idx:
                return False, "Cannot move FreeCell card to itself."
            card = self.freecells[src_idx]
            if card is None:
                return False, f"Source FreeCell {src_idx+1} is empty."
            
            # Valid move, record history
            self.push_history()
            self.freecells[src_idx] = None
            self.freecells[fc_idx] = card
            return True, f"Moved {card} from FreeCell {src_idx+1} to FreeCell {fc_idx+1}."

        return False, "Invalid source type."

    def move_card_to_tableau(self, src_type, src_idx, dest_col_idx):
        """
        src_type: 'freecell' or 'tableau'
        src_idx: index of source freecell (0-3) or tableau (0-7)
        dest_col_idx: index of destination tableau (0-7)
        """
        if dest_col_idx < 0 or dest_col_idx > 7:
            return False, "Invalid destination Tableau index."
        
        dest_col = self.tableaux[dest_col_idx]
        dest_empty = (len(dest_col) == 0)

        if src_type == 'freecell':
            if src_idx < 0 or src_idx > 3:
                return False, "Invalid FreeCell index."
            card = self.freecells[src_idx]
            if card is None:
                return False, f"FreeCell {src_idx+1} is empty."

            # Check destination compatibility
            if not dest_empty:
                target_card = dest_col[-1]
                if card.color == target_card.color:
                    return False, "Must alternate colors."
                if card.rank != target_card.rank - 1:
                    return False, f"Must build descending. Cannot place {card} on {target_card}."

            # Perform single card move
            self.push_history()
            self.freecells[src_idx] = None
            dest_col.append(card)
            return True, f"Moved {card} from FreeCell {src_idx+1} to Tableau {dest_col_idx+1}."

        elif src_type == 'tableau':
            if src_idx < 0 or src_idx > 7:
                return False, "Invalid source Tableau index."
            if src_idx == dest_col_idx:
                return False, "Source and destination Tableau columns are the same."
            
            src_col = self.tableaux[src_idx]
            if not src_col:
                return False, "Source Tableau is empty."

            # Identify valid moving sequences
            longest_seq = self.get_longest_alternating_sequence_at_bottom(src_idx)
            
            # Let's see what sequence length we can move
            if dest_empty:
                # We can move the longest sequence, capped by max movable size
                max_movable = self.get_max_movable_size(moving_to_empty_tableau=True)
                seq_size = min(len(longest_seq), max_movable)
                if seq_size == 0:
                    return False, "No cards available to move."
                
                # Perform sequence move
                self.push_history()
                move_start_idx = len(src_col) - seq_size
                moving_cards = src_col[move_start_idx:]
                self.tableaux[src_idx] = src_col[:move_start_idx]
                dest_col.extend(moving_cards)
                return True, f"Moved sequence of {seq_size} card(s) (bottom: {moving_cards[-1]}) to empty Tableau {dest_col_idx+1}."
            else:
                target_card = dest_col[-1]
                # Find if any card in our longest sequence fits onto target_card
                # To fit on target_card, card must have opposite color and rank target_card.rank - 1
                fit_card_idx = -1
                for i, card in enumerate(longest_seq):
                    if card.color != target_card.color and card.rank == target_card.rank - 1:
                        fit_card_idx = i
                        break
                
                if fit_card_idx == -1:
                    return False, f"No card in source sequence can be placed on {target_card}."
                
                # The sub-sequence to move starts at fit_card_idx in longest_seq
                sub_seq = longest_seq[fit_card_idx:]
                seq_size = len(sub_seq)
                
                max_movable = self.get_max_movable_size(moving_to_empty_tableau=False)
                if seq_size > max_movable:
                    return False, f"Cannot move {seq_size} cards (max movable is {max_movable} with current free space)."
                
                # Perform sequence move
                self.push_history()
                move_start_idx = len(src_col) - seq_size
                moving_cards = src_col[move_start_idx:]
                self.tableaux[src_idx] = src_col[:move_start_idx]
                dest_col.extend(moving_cards)
                return True, f"Moved sequence of {seq_size} card(s) to Tableau {dest_col_idx+1}."

        return False, "Invalid source type."

    def move_card_to_foundation(self, src_type, src_idx):
        """
        src_type: 'freecell' or 'tableau'
        src_idx: index of source freecell (0-3) or tableau (0-7)
        """
        if src_type == 'freecell':
            if src_idx < 0 or src_idx > 3:
                return False, "Invalid FreeCell index."
            card = self.freecells[src_idx]
            if card is None:
                return False, f"FreeCell {src_idx+1} is empty."
            
            # Check foundation
            current_rank = self.foundations[card.suit]
            if card.rank != current_rank + 1:
                return False, f"Cannot move {card} to foundation. Expected rank {current_rank + 1}."
            
            # Perform move
            self.push_history()
            self.freecells[src_idx] = None
            self.foundations[card.suit] = card.rank
            return True, f"Moved {card} to Foundation."

        elif src_type == 'tableau':
            if src_idx < 0 or src_idx > 7:
                return False, "Invalid Tableau index."
            col = self.tableaux[src_idx]
            if not col:
                return False, "Tableau is empty."
            card = col[-1]

            # Check foundation
            current_rank = self.foundations[card.suit]
            if card.rank != current_rank + 1:
                return False, f"Cannot move {card} to foundation. Expected rank {current_rank + 1}."
            
            # Perform move
            self.push_history()
            col.pop()
            self.foundations[card.suit] = card.rank
            return True, f"Moved {card} to Foundation."

        return False, "Invalid source type."

    def is_card_safe_to_auto_collect(self, card):
        # Standard safety rule:
        # A card is safe to auto-collect if its rank is <= 2,
        # OR all cards of opposite color with lower rank are already in foundations.
        if card.rank <= 2:
            return True
        
        opposite_suits = ['S', 'C'] if card.color == 'R' else ['H', 'D']
        # Check if all opposite suits have foundation rank >= card.rank - 1
        return all(self.foundations[suit] >= card.rank - 1 for suit in opposite_suits)

    def auto_collect(self):
        """
        Scans tableaux and freecells to find and automatically move safe cards to foundations.
        Repeats until no more safe cards can be collected.
        Returns the number of cards auto-collected.
        """
        collected_count = 0
        while True:
            moved_any = False
            
            # 1. Check freecells
            for idx in range(4):
                card = self.freecells[idx]
                if card is not None:
                    expected_rank = self.foundations[card.suit] + 1
                    if card.rank == expected_rank and self.is_card_safe_to_auto_collect(card):
                        # Move it!
                        self.move_card_to_foundation('freecell', idx)
                        moved_any = True
                        collected_count += 1
                        break # Break to recheck everything from scratch

            if moved_any:
                continue

            # 2. Check tableaux bottom cards
            for idx in range(8):
                col = self.tableaux[idx]
                if col:
                    card = col[-1]
                    expected_rank = self.foundations[card.suit] + 1
                    if card.rank == expected_rank and self.is_card_safe_to_auto_collect(card):
                        # Move it!
                        self.move_card_to_foundation('tableau', idx)
                        moved_any = True
                        collected_count += 1
                        break # Break to recheck everything from scratch

            if not moved_any:
                break
                
        return collected_count
