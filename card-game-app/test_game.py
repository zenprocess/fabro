import unittest
from game import Card, Deck, FreeCellGame

class TestCard(unittest.TestCase):
    def test_card_properties(self):
        c1 = Card('H', 1)  # Ace of Hearts
        self.assertEqual(c1.suit, 'H')
        self.assertEqual(c1.rank, 1)
        self.assertEqual(c1.color, 'red')
        self.assertEqual(c1.rank_name, 'A')
        self.assertEqual(str(c1), 'AH')

        c2 = Card('S', 13) # King of Spades
        self.assertEqual(c2.suit, 'S')
        self.assertEqual(c2.rank, 13)
        self.assertEqual(c2.color, 'black')
        self.assertEqual(c2.rank_name, 'K')
        self.assertEqual(str(c2), 'KS')

class TestDeck(unittest.TestCase):
    def test_deck_creation(self):
        deck = Deck(seed=123)
        self.assertEqual(len(deck.cards), 52)
        # All unique
        self.assertEqual(len(set((c.suit, c.rank) for c in deck.cards)), 52)

    def test_deal(self):
        deck = Deck(seed=123)
        tableaus = deck.deal()
        self.assertEqual(len(tableaus), 8)
        # First 4 tableaus should have 7 cards, last 4 should have 6 cards
        self.assertEqual([len(t) for t in tableaus], [7, 7, 7, 7, 6, 6, 6, 6])
        # Total 52 cards
        self.assertEqual(sum(len(t) for t in tableaus), 52)

class TestFreeCellGame(unittest.TestCase):
    def test_initial_state(self):
        game = FreeCellGame(seed=42)
        self.assertEqual(len(game.tableaus), 8)
        self.assertEqual(game.free_cells, [None] * 4)
        self.assertEqual(game.foundations, {'H': [], 'D': [], 'C': [], 'S': []})
        self.assertFalse(game.is_won())
        self.assertTrue(game.has_any_valid_moves())

    def test_move_to_free_cell(self):
        game = FreeCellGame(seed=42)
        # Check initial top card of tableau 0
        card = game.tableaus[0][-1]
        
        # Move to free cell 0
        success = game.move_card_to_free_cell('tableau', 0, 0)
        self.assertTrue(success)
        self.assertEqual(game.free_cells[0], card)
        self.assertEqual(len(game.tableaus[0]), 6)  # Reduced from 7 to 6

        # Try to move to the same busy free cell - should fail
        success = game.move_card_to_free_cell('tableau', 0, 0)
        self.assertFalse(success)

    def test_move_to_foundation(self):
        game = FreeCellGame(seed=42)
        # Force an Ace of Hearts onto the top of tableau 0 for testing
        ace_h = Card('H', 1)
        game.tableaus[0].append(ace_h)

        # Move Ace to Foundation 0 (Hearts)
        success = game.move_card_to_foundation('tableau', 0, 0)
        self.assertTrue(success)
        self.assertEqual(game.foundations['H'], [ace_h])

        # Force a 2 of Hearts onto tableau 0
        two_h = Card('H', 2)
        game.tableaus[0].append(two_h)
        # Move 2 of Hearts to foundation
        success = game.move_card_to_foundation('tableau', 0, 0)
        self.assertTrue(success)
        self.assertEqual(game.foundations['H'], [ace_h, two_h])

        # Try moving something invalid to Hearts foundation
        wrong_card = Card('D', 3)
        game.tableaus[0].append(wrong_card)
        success = game.move_card_to_foundation('tableau', 0, 0)
        self.assertFalse(success)

    def test_move_to_tableau_single(self):
        game = FreeCellGame(seed=42)
        # Setup: Ensure tableau 0 top card is black 10 (10 of Spades) and tableau 1 has red 9 (9 of Hearts)
        game.tableaus[0][-1] = Card('S', 10)
        game.tableaus[1][-1] = Card('H', 9)

        # Move 9 of Hearts to 10 of Spades
        success = game.move_to_tableau('tableau', 1, 0)
        self.assertTrue(success)
        self.assertEqual(game.tableaus[0][-1], Card('H', 9))
        self.assertEqual(game.tableaus[0][-2], Card('S', 10))

    def test_undo(self):
        game = FreeCellGame(seed=42)
        init_state = game.get_state()
        
        # Make a move
        game.move_card_to_free_cell('tableau', 0, 0)
        self.assertNotEqual(game.get_state(), init_state)

        # Undo
        success = game.undo()
        self.assertTrue(success)
        self.assertEqual(game.tableaus, init_state['tableaus'])
        self.assertEqual(game.free_cells, init_state['free_cells'])

        # Undo on empty history
        success = game.undo()
        self.assertFalse(success)

    def test_win_condition(self):
        game = FreeCellGame(seed=42)
        self.assertFalse(game.is_won())
        
        # Fill foundations to win
        for suit in ['H', 'D', 'C', 'S']:
            game.foundations[suit] = [Card(suit, r) for r in range(1, 14)]
            
        self.assertTrue(game.is_won())

if __name__ == '__main__':
    unittest.main()
