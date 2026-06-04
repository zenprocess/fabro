import unittest
import copy
from card_game_tui.game import Card, Deck, GameState

class TestFreeCellGame(unittest.TestCase):
    def test_card_properties(self):
        c1 = Card('H', 1) # Ace of Hearts
        self.assertEqual(c1.color, 'R')
        self.assertEqual(c1.symbol, '♥')
        self.assertEqual(c1.rank_str, 'A')
        self.assertEqual(repr(c1), 'A♥')

        c2 = Card('S', 10) # 10 of Spades
        self.assertEqual(c2.color, 'B')
        self.assertEqual(c2.symbol, '♠')
        self.assertEqual(c2.rank_str, '10')
        self.assertEqual(repr(c2), '10♠')

    def test_deck_and_deal(self):
        state = GameState()
        state.deal(seed=42)

        # Count total cards dealt
        total_cards = sum(len(col) for col in state.tableaux)
        self.assertEqual(total_cards, 52)

        # Verify sizes: first 4 must have 7 cards, last 4 have 6 cards
        for i in range(4):
            self.assertEqual(len(state.tableaux[i]), 7)
        for i in range(4, 8):
            self.assertEqual(len(state.tableaux[i]), 6)

    def test_move_to_freecell(self):
        state = GameState()
        state.deal(seed=42)

        # Move bottom card of Tableau 0 to FreeCell 0
        card_to_move = state.tableaux[0][-1]
        success, msg = state.move_card_to_freecell('tableau', 0, 0)
        self.assertTrue(success, msg)
        self.assertEqual(state.freecells[0], card_to_move)
        self.assertEqual(len(state.tableaux[0]), 6)

        # Try to move another card to the same FreeCell (should fail)
        success, msg = state.move_card_to_freecell('tableau', 1, 0)
        self.assertFalse(success)
        self.assertIn("already occupied", msg)

    def test_move_to_foundation(self):
        state = GameState()
        # Create a manually controlled state for testing
        state.tableaux = [[] for _ in range(8)]
        state.freecells = [None] * 4
        state.foundations = {suit: 0 for suit in Card.SUITS}

        # Put an Ace in Tableau 0
        ace_h = Card('H', 1)
        state.tableaux[0].append(ace_h)

        # Moving Ace to foundation should succeed
        success, msg = state.move_card_to_foundation('tableau', 0)
        self.assertTrue(success, msg)
        self.assertEqual(state.foundations['H'], 1)
        self.assertEqual(len(state.tableaux[0]), 0)

        # Putting Hearts 2 in freecell 0
        two_h = Card('H', 2)
        state.freecells[0] = two_h
        
        # Moving Hearts 2 to foundation should succeed
        success, msg = state.move_card_to_foundation('freecell', 0)
        self.assertTrue(success, msg)
        self.assertEqual(state.foundations['H'], 2)
        self.assertIsNone(state.freecells[0])

        # Trying to move Hearts 4 (skip 3) should fail
        four_h = Card('H', 4)
        state.freecells[1] = four_h
        success, msg = state.move_card_to_foundation('freecell', 1)
        self.assertFalse(success)
        self.assertIn("Expected rank 3", msg)

    def test_tableau_single_card_move(self):
        state = GameState()
        state.tableaux = [[] for _ in range(8)]
        state.freecells = [None] * 4
        
        # Place red 10 on Tableau 0 and black 9 on Freecell 0
        ten_h = Card('H', 10)
        state.tableaux[0].append(ten_h)

        nine_s = Card('S', 9)
        state.freecells[0] = nine_s

        # Move black 9 to Tableau 0 (valid: alternating colors and descending)
        success, msg = state.move_card_to_tableau('freecell', 0, 0)
        self.assertTrue(success, msg)
        self.assertEqual(len(state.tableaux[0]), 2)
        self.assertEqual(state.tableaux[0][-1], nine_s)
        self.assertIsNone(state.freecells[0])

        # Try to move a red 8 to Tableau 0 (valid: alternating color, descending)
        eight_d = Card('D', 8)
        state.freecells[1] = eight_d
        success, msg = state.move_card_to_tableau('freecell', 1, 0)
        self.assertTrue(success, msg)

        # Try to move black 8 to Tableau 0 (invalid: same rank, invalid sequence)
        eight_c = Card('C', 8)
        state.freecells[2] = eight_c
        success, msg = state.move_card_to_tableau('freecell', 2, 0)
        self.assertFalse(success)
        self.assertIn("Must build descending", msg or "")

    def test_tableau_sequence_move_and_limits(self):
        state = GameState()
        state.tableaux = [[] for _ in range(8)]
        state.freecells = [None] * 4

        # Tableau 0: Jack♥, 10♠, 9♦, 8♣ (valid sequence of size 4)
        state.tableaux[0] = [
            Card('H', 11),
            Card('S', 10),
            Card('D', 9),
            Card('C', 8)
        ]

        # Tableau 1: Queen♠
        state.tableaux[1] = [Card('S', 12)]

        # Max movable size:
        # FreeCells empty: 4. Empty Tableau columns: 6.
        # Moving to Tableau 1 (not empty):
        # max size = (1 + 4) * 2^6 = 320.
        # We can move all 4 cards of the sequence!
        success, msg = state.move_card_to_tableau('tableau', 0, 1)
        self.assertTrue(success, msg)
        self.assertEqual(len(state.tableaux[0]), 0)
        self.assertEqual(len(state.tableaux[1]), 5)

        # Fill up freecells
        state.freecells = [Card('S', 2), Card('D', 2), Card('H', 2), Card('C', 2)] # 0 free cells empty
        
        # Now reset
        state.tableaux[0] = [
            Card('H', 11),
            Card('S', 10),
            Card('D', 9),
            Card('C', 8)
        ]
        state.tableaux[1] = [Card('S', 12)]

        # Empty tableaux count: 6 (columns 2, 3, 4, 5, 6, 7 are empty).
        # Max movable = (1 + 0) * 2^6 = 64.
        # So we can still move it.
        # Now let's fill up columns to eliminate empty columns.
        for i in range(2, 8):
            state.tableaux[i] = [Card('S', 13)] # Now 0 empty columns, 0 empty free cells
        
        # Max movable = (1 + 0) * 2^0 = 1.
        # Trying to move sequence from 0 to 1 should fail because sequence size needed is 4 (Jack to Queen), and limit is 1.
        success, msg = state.move_card_to_tableau('tableau', 0, 1)
        self.assertFalse(success, msg)
        self.assertIn("Cannot move 4 cards", msg)

    def test_undo(self):
        state = GameState()
        state.deal(seed=42)

        # Save initial state
        initial_tableaux = copy.deepcopy(state.tableaux)

        # Make a move
        state.move_card_to_freecell('tableau', 0, 0)
        self.assertNotEqual(state.tableaux, initial_tableaux)

        # Undo
        self.assertTrue(state.undo())
        self.assertEqual(state.tableaux, initial_tableaux)

        # Undo empty history
        self.assertFalse(state.undo())

    def test_auto_collect(self):
        state = GameState()
        state.tableaux = [[] for _ in range(8)]
        state.freecells = [None] * 4
        state.foundations = {suit: 0 for suit in Card.SUITS}

        # Place Ace of Hearts and 2 of Hearts in Tableau 0
        state.tableaux[0] = [Card('H', 2), Card('H', 1)]

        # Run auto collect
        collected = state.auto_collect()
        self.assertEqual(collected, 2)
        self.assertEqual(state.foundations['H'], 2)
        self.assertEqual(len(state.tableaux[0]), 0)

if __name__ == '__main__':
    unittest.main()
