import unittest
from game import Card, GameState

class TestSolitaireGame(unittest.TestCase):
    
    def test_card_properties(self):
        card1 = Card('H', 1, face_up=True)  # Ace of Hearts
        card2 = Card('S', 13, face_up=False) # King of Spades
        
        self.assertEqual(card1.color, 'red')
        self.assertEqual(card1.suit_symbol, '♥')
        self.assertEqual(card1.rank_symbol, 'A')
        self.assertEqual(card1.to_string(), "A♥")
        
        self.assertEqual(card2.color, 'black')
        self.assertEqual(card2.suit_symbol, '♠')
        self.assertEqual(card2.rank_symbol, 'K')
        self.assertEqual(card2.to_string(), "[ █ ]")

    def test_initial_state(self):
        state = GameState(seed=42)
        # Total cards = 52
        total_tableau_cards = sum(len(t) for t in state.tableaus)
        self.assertEqual(total_tableau_cards, 28) # 1+2+3+4+5+6+7 = 28
        self.assertEqual(len(state.stock), 24) # 52 - 28 = 24
        self.assertEqual(len(state.waste), 0)
        self.assertTrue(all(len(f) == 0 for f in state.foundations))
        
        # Check that top tableau cards are face-up and lower ones are face-down
        for i in range(7):
            t = state.tableaus[i]
            for j in range(i):
                self.assertFalse(t[j].face_up)
            self.assertTrue(t[i].face_up)

    def test_draw_and_recycle_stock(self):
        state = GameState(seed=42)
        initial_stock_size = len(state.stock)
        
        # Draw all stock cards
        for i in range(initial_stock_size):
            self.assertTrue(state.draw_card())
            
        self.assertEqual(len(state.stock), 0)
        self.assertEqual(len(state.waste), initial_stock_size)
        self.assertTrue(all(c.face_up for c in state.waste))
        
        # Draw when empty should recycle
        self.assertTrue(state.draw_card())
        self.assertEqual(len(state.stock), initial_stock_size - 1)
        self.assertEqual(len(state.waste), 1)
        self.assertTrue(state.waste[-1].face_up)
        self.assertTrue(all(not c.face_up for c in state.stock))

    def test_move_waste_to_tableau_valid_and_invalid(self):
        state = GameState(seed=42)
        
        # Let's set up a predictable waste and tableau top card for testing
        # Waste top: Red Q (Hearts, 12)
        state.waste = [Card('H', 12, face_up=True)]
        
        # Destination Tableau top card is Black K (Spades, 13)
        state.tableaus[0] = [Card('S', 13, face_up=True)]
        
        # Move Q to K is valid (Red 12 on Black 13)
        success = state.move_waste_to_tableau(0)
        self.assertTrue(success)
        self.assertEqual(len(state.waste), 0)
        self.assertEqual(state.tableaus[0][-1].rank, 12)
        
        # Destination Tableau is now Red 12 on top. Let's put Black Jack (11) on waste
        state.waste = [Card('C', 11, face_up=True)]
        success = state.move_waste_to_tableau(0)
        self.assertTrue(success)
        self.assertEqual(len(state.waste), 0)
        self.assertEqual(state.tableaus[0][-1].rank, 11)
        
        # Try to move non-matching card: Black 10 (Spades, 10) on Black Jack (11) (invalid, should be opposite color)
        state.waste = [Card('S', 10, face_up=True)]
        success = state.move_waste_to_tableau(0)
        self.assertFalse(success)
        self.assertEqual(len(state.waste), 1)

    def test_move_to_empty_tableau_only_king(self):
        state = GameState(seed=42)
        state.tableaus[0] = [] # empty
        
        # Try putting a Queen on empty tableau
        state.waste = [Card('H', 12, face_up=True)]
        success = state.move_waste_to_tableau(0)
        self.assertFalse(success)
        
        # Put a King on empty tableau
        state.waste = [Card('D', 13, face_up=True)]
        success = state.move_waste_to_tableau(0)
        self.assertTrue(success)
        self.assertEqual(len(state.tableaus[0]), 1)
        self.assertEqual(state.tableaus[0][0].rank, 13)

    def test_move_tableau_to_tableau_stack(self):
        state = GameState(seed=42)
        # Tableau 0: Black K (Spades, 13)
        state.tableaus[0] = [Card('S', 13, face_up=True)]
        
        # Tableau 1: Red Q (Hearts, 12) -> Black J (Clubs, 11)
        state.tableaus[1] = [Card('H', 12, face_up=True), Card('C', 11, face_up=True)]
        
        # Move the entire stack [Q, J] onto K
        success = state.move_tableau_to_tableau(1, 0, 0)
        self.assertTrue(success)
        self.assertEqual(len(state.tableaus[1]), 0)
        self.assertEqual(len(state.tableaus[0]), 3)
        self.assertEqual([c.rank for c in state.tableaus[0]], [13, 12, 11])

    def test_move_to_foundation(self):
        state = GameState(seed=42)
        # Foundation 0 is 'H' (Hearts)
        # Try putting a 2 of Hearts on empty foundation (should fail, needs Ace)
        state.waste = [Card('H', 2, face_up=True)]
        success = state.move_waste_to_foundation(0)
        self.assertFalse(success)
        
        # Put Ace of Hearts (1)
        state.waste = [Card('H', 1, face_up=True)]
        success = state.move_waste_to_foundation(0)
        self.assertTrue(success)
        self.assertEqual(len(state.foundations[0]), 1)
        
        # Now put 2 of Hearts (2)
        state.waste = [Card('H', 2, face_up=True)]
        success = state.move_waste_to_foundation(0)
        self.assertTrue(success)
        self.assertEqual(len(state.foundations[0]), 2)
        self.assertEqual(state.foundations[0][-1].rank, 2)

    def test_check_win(self):
        state = GameState(seed=42)
        self.assertFalse(state.check_win())
        
        # Fill foundations to win
        for i, suit in enumerate(state.foundation_suits):
            for rank in range(1, 14):
                state.foundations[i].append(Card(suit, rank, face_up=True))
                
        self.assertTrue(state.check_win())

if __name__ == '__main__':
    unittest.main()
