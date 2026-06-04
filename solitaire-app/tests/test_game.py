import unittest
from solitaire_tui.game import GameState, Card

class TestSolitaireGame(unittest.TestCase):
    def setUp(self):
        # Use a fixed seed for reproducible tests
        self.game = GameState(seed=42)

    def test_initial_deal(self):
        # Verify 7 tableau columns have correct card counts (1 to 7)
        for i in range(7):
            self.assertEqual(len(self.game.tableau[i]), i + 1)
            # Top card of each column must be face-up
            self.assertTrue(self.game.tableau[i][-1].is_face_up)
            # Other cards in column must be face-down
            for j in range(i):
                self.assertFalse(self.game.tableau[i][j].is_face_up)

        # Foundations must be empty initially
        for f in self.game.foundations:
            self.assertEqual(len(f), 0)

        # Stock must contain the remaining cards (52 - 28 = 24)
        self.assertEqual(len(self.game.stock), 24)
        # Waste should be empty initially
        self.assertEqual(len(self.game.waste), 0)

    def test_draw_and_recycle(self):
        initial_stock_len = len(self.game.stock)
        
        # Draw all cards
        for _ in range(initial_stock_len):
            success = self.game.draw()
            self.assertTrue(success)

        self.assertEqual(len(self.game.stock), 0)
        self.assertEqual(len(self.game.waste), initial_stock_len)

        # Draw again to recycle
        success = self.game.draw()
        self.assertTrue(success)
        self.assertEqual(len(self.game.stock), initial_stock_len)
        self.assertEqual(len(self.game.waste), 0)

    def test_cannot_move_invalid_tableau_to_tableau(self):
        # Try to move a card to an empty slot or invalid card
        # Setup specific tableau configuration manually to test rules
        self.game.tableau[0] = [Card(suit='H', rank=5, is_face_up=True)]
        self.game.tableau[1] = [Card(suit='S', rank=7, is_face_up=True)]
        
        # Moving 5 of Hearts onto 7 of Spades is invalid (rank diff != 1)
        self.assertFalse(self.game.can_move_tableau_to_tableau(src_col=0, dest_col=1, card_idx=0))

        # Moving 5 of Hearts onto an empty column is invalid (must be King)
        self.game.tableau[2] = []
        self.assertFalse(self.game.can_move_tableau_to_tableau(src_col=0, dest_col=2, card_idx=0))

    def test_valid_tableau_to_tableau_move(self):
        self.game.tableau[0] = [Card(suit='H', rank=6, is_face_up=True)]
        self.game.tableau[1] = [
            Card(suit='C', rank=8, is_face_up=False),
            Card(suit='S', rank=7, is_face_up=True)
        ]

        # 6 of Hearts onto 7 of Spades: valid! (opposite color, rank = 7 - 1)
        self.assertTrue(self.game.can_move_tableau_to_tableau(src_col=0, dest_col=1, card_idx=0))

        # Perform the move
        success = self.game.move_tableau_to_tableau(src_col=0, dest_col=1, card_idx=0)
        self.assertTrue(success)
        self.assertEqual(len(self.game.tableau[0]), 0)
        self.assertEqual(len(self.game.tableau[1]), 3)
        self.assertEqual(self.game.tableau[1][-1].rank, 6)

    def test_auto_reveal(self):
        self.game.tableau[0] = [Card(suit='H', rank=6, is_face_up=True)]
        self.game.tableau[1] = [
            Card(suit='C', rank=8, is_face_up=False),
            Card(suit='S', rank=7, is_face_up=True)
        ]

        # Move 7 of Spades from tableau[1] to tableau[0] is invalid due to colors/ranks,
        # let's set up a valid case where a face-down card gets exposed and revealed.
        self.game.tableau[0] = [Card(suit='D', rank=8, is_face_up=True)]
        self.game.tableau[1] = [
            Card(suit='C', rank=10, is_face_up=False),
            Card(suit='S', rank=7, is_face_up=True)
        ]

        # Let's change tableau[0] top card to 8 of Diamonds (Red) and tableau[1] to 7 of Spades (Black)
        success = self.game.move_tableau_to_tableau(src_col=1, dest_col=0, card_idx=1)
        self.assertTrue(success)
        # Check that the face-down card left in tableau[1] (10 of Clubs) is now face-up!
        self.assertTrue(self.game.tableau[1][0].is_face_up)

    def test_undo_functionality(self):
        self.game.tableau[0] = [Card(suit='D', rank=8, is_face_up=True)]
        self.game.tableau[1] = [
            Card(suit='C', rank=10, is_face_up=False),
            Card(suit='S', rank=7, is_face_up=True)
        ]

        initial_state = self.game.serialize_state()

        success = self.game.move_tableau_to_tableau(src_col=1, dest_col=0, card_idx=1)
        self.assertTrue(success)

        # Undo the move
        undo_success = self.game.undo()
        self.assertTrue(undo_success)

        # Verify state is restored
        self.assertEqual(len(self.game.tableau[1]), 2)
        self.assertFalse(self.game.tableau[1][0].is_face_up)
        self.assertTrue(self.game.tableau[1][1].is_face_up)
        self.assertEqual(len(self.game.tableau[0]), 1)

    def test_win_condition(self):
        # Empty game state check_win should be False
        self.assertFalse(self.game.check_win())

        # Set up a winning board state
        suits = ['H', 'D', 'C', 'S']
        for i, suit in enumerate(suits):
            self.game.foundations[i] = [Card(suit=suit, rank=r, is_face_up=True) for r in range(1, 14)]

        self.assertTrue(self.game.check_win())

    def test_card_color_and_helpers(self):
        # Card should have color property and display_str helper
        red_card = Card(suit='H', rank=1, is_face_up=True)
        black_card = Card(suit='S', rank=13, is_face_up=True)
        face_down_card = Card(suit='D', rank=10, is_face_up=False)

        self.assertEqual(red_card.color, "red")
        self.assertEqual(black_card.color, "black")
        self.assertTrue(red_card.is_red)
        self.assertFalse(black_card.is_red)

        self.assertEqual(red_card.display_str(), "A♥")
        self.assertEqual(black_card.display_str(), "K♠")
        self.assertEqual(face_down_card.display_str(), "##")

        # Repr check
        self.assertEqual(repr(red_card), "[A♥]")
        self.assertEqual(repr(face_down_card), "[##]")

    def test_deck_creation_and_shuffling(self):
        from solitaire_tui.game import Deck
        deck = Deck(seed=42)
        self.assertEqual(len(deck), 52)

        # Deck cards should be face-down
        for card in deck.cards:
            self.assertFalse(card.is_face_up)

        # Drawing a card
        top_card = deck.cards[-1]
        drawn = deck.draw()
        self.assertEqual(drawn, top_card)
        self.assertEqual(len(deck), 51)

        # Shuffle
        deck2 = Deck(seed=42)
        deck2.shuffle()
        # Verify that the deck order has changed from unshuffled
        unshuffled_deck = Deck()
        self.assertNotEqual([c.suit + str(c.rank) for c in deck2.cards], [c.suit + str(c.rank) for c in unshuffled_deck.cards])

    def test_pile_abstractions(self):
        from solitaire_tui.game import StockPile, WastePile, FoundationPile, TableauPile
        stock = StockPile([Card('H', 1), Card('D', 2)])
        self.assertIsInstance(stock, list)
        self.assertEqual(stock.top_card.rank, 2)

        waste = WastePile()
        self.assertIsNone(waste.top_card)

        # Check types in GameState
        self.assertIsInstance(self.game.stock, StockPile)
        self.assertIsInstance(self.game.waste, WastePile)
        self.assertIsInstance(self.game.foundations[0], FoundationPile)
        self.assertIsInstance(self.game.tableau[0], TableauPile)

    def test_waste_to_tableau_moves(self):
        # 1. Clear waste and test cannot move when waste is empty
        self.game.waste.clear()
        self.assertFalse(self.game.can_move_waste_to_tableau(dest_col=0))

        # 2. Setup waste top card with a Red 5
        self.game.waste.append(Card(suit='H', rank=5, is_face_up=True))

        # 3. Setup tableau target with Black 6 (face up) -> Legal move
        self.game.tableau[0] = [Card(suit='S', rank=6, is_face_up=True)]
        self.assertTrue(self.game.can_move_waste_to_tableau(dest_col=0))

        # Perform the move
        success = self.game.move_waste_to_tableau(dest_col=0)
        self.assertTrue(success)
        self.assertEqual(len(self.game.waste), 0)
        self.assertEqual(len(self.game.tableau[0]), 2)
        self.assertEqual(self.game.tableau[0][-1], Card(suit='H', rank=5, is_face_up=True))

        # 4. Setup waste with Red 5 again, but tableau has Black 5 -> Illegal move (same rank)
        self.game.waste.append(Card(suit='D', rank=5, is_face_up=True))
        self.game.tableau[1] = [Card(suit='C', rank=5, is_face_up=True)]
        self.assertFalse(self.game.can_move_waste_to_tableau(dest_col=1))

        # 5. Setup tableau with Red 6 -> Illegal move (same color)
        self.game.tableau[2] = [Card(suit='H', rank=6, is_face_up=True)]
        self.assertFalse(self.game.can_move_waste_to_tableau(dest_col=2))

        # 6. Target is empty -> Illegal move because Red 5 is not King (rank 13)
        self.game.tableau[3] = []
        self.assertFalse(self.game.can_move_waste_to_tableau(dest_col=3))

        # 7. Setup waste with King -> Legal to move to empty tableau
        self.game.waste.clear()
        self.game.waste.append(Card(suit='S', rank=13, is_face_up=True))
        self.assertTrue(self.game.can_move_waste_to_tableau(dest_col=3))
        success = self.game.move_waste_to_tableau(dest_col=3)
        self.assertTrue(success)
        self.assertEqual(len(self.game.tableau[3]), 1)
        self.assertEqual(self.game.tableau[3][0].rank, 13)

    def test_waste_to_foundation_moves(self):
        # 1. Setup empty foundation, test cannot move waste when empty
        self.game.foundations[0] = []
        self.game.waste.clear()
        self.assertFalse(self.game.can_move_waste_to_foundation(dest_found=0))

        # 2. Setup waste with Ace of Hearts -> Legal
        self.game.waste.append(Card(suit='H', rank=1, is_face_up=True))
        self.assertTrue(self.game.can_move_waste_to_foundation(dest_found=0))
        
        # Perform move
        success = self.game.move_waste_to_foundation(dest_found=0)
        self.assertTrue(success)
        self.assertEqual(len(self.game.foundations[0]), 1)
        self.assertEqual(self.game.foundations[0][-1], Card(suit='H', rank=1, is_face_up=True))

        # 3. Setup waste with 2 of Hearts -> Legal (same suit, rank+1)
        self.game.waste.append(Card(suit='H', rank=2, is_face_up=True))
        self.assertTrue(self.game.can_move_waste_to_foundation(dest_found=0))
        success = self.game.move_waste_to_foundation(dest_found=0)
        self.assertTrue(success)

        # 4. Setup waste with 4 of Hearts -> Illegal (rank not +1)
        self.game.waste.append(Card(suit='H', rank=4, is_face_up=True))
        self.assertFalse(self.game.can_move_waste_to_foundation(dest_found=0))

        # 5. Setup waste with 3 of Diamonds -> Illegal (wrong suit)
        self.game.waste.clear()
        self.game.waste.append(Card(suit='D', rank=3, is_face_up=True))
        self.assertFalse(self.game.can_move_waste_to_foundation(dest_found=0))

    def test_tableau_to_foundation_moves(self):
        # 1. Setup tableau with Ace of Clubs, foundation empty -> Legal
        self.game.tableau[0] = [Card(suit='C', rank=1, is_face_up=True)]
        self.game.foundations[1] = []
        self.assertTrue(self.game.can_move_tableau_to_foundation(src_col=0, dest_found=1))

        # Perform move
        success = self.game.move_tableau_to_foundation(src_col=0, dest_found=1)
        self.assertTrue(success)
        self.assertEqual(len(self.game.tableau[0]), 0)
        self.assertEqual(len(self.game.foundations[1]), 1)
        self.assertEqual(self.game.foundations[1][-1].rank, 1)

        # 2. Test tableau top card is face-down -> Illegal
        self.game.tableau[1] = [Card(suit='C', rank=2, is_face_up=False)]
        self.assertFalse(self.game.can_move_tableau_to_foundation(src_col=1, dest_found=1))

    def test_foundation_to_tableau_moves(self):
        # 1. Setup foundation with Ace and 2 of Spades, tableau with Red 3 -> Legal
        self.game.foundations[0] = [
            Card(suit='S', rank=1, is_face_up=True),
            Card(suit='S', rank=2, is_face_up=True)
        ]
        self.game.tableau[0] = [Card(suit='H', rank=3, is_face_up=True)]
        self.assertTrue(self.game.can_move_foundation_to_tableau(src_found=0, dest_col=0))

        # Perform move
        success = self.game.move_foundation_to_tableau(src_found=0, dest_col=0)
        self.assertTrue(success)
        self.assertEqual(len(self.game.foundations[0]), 1)
        self.assertEqual(len(self.game.tableau[0]), 2)
        self.assertEqual(self.game.tableau[0][-1], Card(suit='S', rank=2, is_face_up=True))

        # 2. Setup foundation with 2 of Spades, tableau with Black 3 -> Illegal (same color)
        self.game.foundations[0].append(Card(suit='S', rank=2, is_face_up=True))
        self.game.tableau[1] = [Card(suit='C', rank=3, is_face_up=True)]
        self.assertFalse(self.game.can_move_foundation_to_tableau(src_found=0, dest_col=1))

    def test_moving_face_up_tableau_runs(self):
        # Setup tableau with face-down card, then a run of Red 10, Black 9, Red 8 (all face-up)
        self.game.tableau[0] = [
            Card(suit='C', rank=12, is_face_up=False), # Face down King
            Card(suit='D', rank=10, is_face_up=True),
            Card(suit='S', rank=9, is_face_up=True),
            Card(suit='H', rank=8, is_face_up=True)
        ]
        # Target has Black Jack (11)
        self.game.tableau[1] = [Card(suit='C', rank=11, is_face_up=True)]

        # Move the entire run starting at Red 10 (card_idx = 1) -> Legal
        self.assertTrue(self.game.can_move_tableau_to_tableau(src_col=0, dest_col=1, card_idx=1))
        
        # Perform move
        success = self.game.move_tableau_to_tableau(src_col=0, dest_col=1, card_idx=1)
        self.assertTrue(success)

        # Check source pile has only 1 card left and it got automatically flipped face up!
        self.assertEqual(len(self.game.tableau[0]), 1)
        self.assertTrue(self.game.tableau[0][0].is_face_up)

        # Check dest pile has 4 cards (Jack, 10, 9, 8) and their order and attributes are correct
        self.assertEqual(len(self.game.tableau[1]), 4)
        self.assertEqual(self.game.tableau[1][1], Card(suit='D', rank=10, is_face_up=True))
        self.assertEqual(self.game.tableau[1][2], Card(suit='S', rank=9, is_face_up=True))
        self.assertEqual(self.game.tableau[1][3], Card(suit='H', rank=8, is_face_up=True))

        # Check moving an invalid run (e.g. non-alternating or face-down card) is prevented
        self.game.tableau[2] = [
            Card(suit='D', rank=5, is_face_up=True),
            Card(suit='H', rank=4, is_face_up=True)  # same color! invalid run
        ]
        self.game.tableau[3] = [Card(suit='C', rank=6, is_face_up=True)]
        self.assertFalse(self.game.can_move_tableau_to_tableau(src_col=2, dest_col=3, card_idx=0))


class TestSolitaireUI(unittest.TestCase):
    def setUp(self):
        from solitaire_tui.game import GameState
        from solitaire_tui.ui import SolitaireTUI
        self.game = GameState(seed=42)
        self.tui = SolitaireTUI(self.game)

    def test_cursor_navigation(self):
        # Starts at 'top', col 0
        self.assertEqual(self.tui.cursor_zone, 'top')
        self.assertEqual(self.tui.cursor_col, 0)

        # Move right
        self.tui.move_cursor('right')
        self.assertEqual(self.tui.cursor_col, 1)

        # Move right again (should skip col 2 and land on col 3)
        self.tui.move_cursor('right')
        self.assertEqual(self.tui.cursor_col, 3)

        # Move down to tableau
        self.tui.move_cursor('down')
        self.assertEqual(self.tui.cursor_zone, 'bottom')
        self.assertEqual(self.tui.cursor_col, 3)
        # Tableau 3 has 4 cards, so card index should be 3 (0-based)
        self.assertEqual(self.tui.cursor_card_idx, 3)

        # Move up from bottom of col 3
        # First face-up card of col 3 is at index 3
        self.tui.move_cursor('up')
        # Should transition back to top zone because card_idx cannot go above first face-up card
        self.assertEqual(self.tui.cursor_zone, 'top')

    def test_ui_action_draw(self):
        # Cursor is at top, col 0 (Stock)
        self.tui.cursor_zone = 'top'
        self.tui.cursor_col = 0
        
        self.assertEqual(len(self.game.stock), 24)
        self.assertEqual(len(self.game.waste), 0)
        
        self.tui.handle_action()
        self.assertEqual(len(self.game.stock), 23)
        self.assertEqual(len(self.game.waste), 1)
        self.assertEqual(self.tui.status_msg, "Drawn card.")

    def test_draw_card_representation(self):
        from unittest.mock import MagicMock, patch
        stdscr = MagicMock()
        
        with patch('solitaire_tui.ui.curses.color_pair', return_value=0):
            # 1. Test None (empty slot) card representation
            self.tui.draw_card_representation(stdscr, y=5, x=10, card=None, is_cursor=False, is_selected=False)
            stdscr.addstr.assert_any_call(5, 10, "[  -]", unittest.mock.ANY)
            
            # 2. Test face-down card representation
            face_down_card = Card('H', 1, is_face_up=False)
            stdscr.reset_mock()
            self.tui.draw_card_representation(stdscr, y=5, x=10, card=face_down_card, is_cursor=False, is_selected=False)
            stdscr.addstr.assert_any_call(5, 10, "[###]", unittest.mock.ANY)

            # 3. Test face-up card representation
            face_up_card = Card('H', 10, is_face_up=True)
            stdscr.reset_mock()
            self.tui.draw_card_representation(stdscr, y=5, x=10, card=face_up_card, is_cursor=False, is_selected=False)
            stdscr.addstr.assert_any_call(5, 10, "[", unittest.mock.ANY)
            try:
                stdscr.addstr.assert_any_call(5, 11, "10♥", unittest.mock.ANY)
            except AssertionError:
                stdscr.addstr.assert_any_call(5, 11, "10H", unittest.mock.ANY)
            stdscr.addstr.assert_any_call(5, 14, "]", unittest.mock.ANY)

    def test_draw_screen(self):
        from unittest.mock import MagicMock, patch
        stdscr = MagicMock()
        stdscr.getmaxyx.return_value = (24, 80)
        
        with patch('solitaire_tui.ui.curses.color_pair', return_value=0):
            self.tui.draw_screen(stdscr)
            
            stdscr.erase.assert_called_once()
            stdscr.refresh.assert_called_once()
            stdscr.addstr.assert_any_call(0, 2, "KLONDIKE SOLITAIRE", unittest.mock.ANY)
            stdscr.addstr.assert_any_call(3, 2, "STOCK", unittest.mock.ANY)


if __name__ == '__main__':
    unittest.main()
