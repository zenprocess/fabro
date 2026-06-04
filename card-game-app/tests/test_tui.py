from card_game_tui.tui import TUIApp
from card_game_tui.domain import Position, LocationType, Card, Suit, Rank

def test_tui_initialization():
    app = TUIApp()
    assert app.cursor_row == 1
    assert app.cursor_col == 0
    assert app.selected_pos is None
    assert app.selected_count == 1
    assert app.quit_confirm is False

def test_tui_reset_game():
    app = TUIApp()
    app.reset_game(12345)
    assert app.current_seed == 12345
    assert app.selected_pos is None
    assert app.cursor_row == 1
    assert app.cursor_col == 0

def test_tui_get_current_pos():
    app = TUIApp()
    # Row 1, Col 3 should be Tableau index 3
    app.cursor_row = 1
    app.cursor_col = 3
    pos = app.get_current_pos()
    assert pos.type == LocationType.TABLEAU
    assert pos.index == 3

    # Row 0, Col 2 should be FreeCell 2
    app.cursor_row = 0
    app.cursor_col = 2
    pos = app.get_current_pos()
    assert pos.type == LocationType.FREECELL
    assert pos.index == 2

    # Row 0, Col 5 should be Foundation 1 (5 - 4)
    app.cursor_row = 0
    app.cursor_col = 5
    pos = app.get_current_pos()
    assert pos.type == LocationType.FOUNDATION
    assert pos.index == 1

def test_tui_find_longest_sequence():
    app = TUIApp()
    app.state.tableaus = [[] for _ in range(8)]
    
    # Empty column
    assert app.find_longest_sequence(0) == 0

    # Non-empty column
    # Red 10, Black 9, Red 8 (descending and alternating)
    c10 = Card(Suit.HEARTS, Rank.TEN)
    c9 = Card(Suit.SPADES, Rank.NINE)
    c8 = Card(Suit.DIAMONDS, Rank.EIGHT)
    app.state.tableaus[0] = [Card(Suit.CLUBS, Rank.KING), c10, c9, c8]
    assert app.find_longest_sequence(0) == 3

def test_tui_find_max_valid_count():
    app = TUIApp()
    app.state.tableaus = [[] for _ in range(8)]
    app.state.freecells = [None] * 4
    
    # From Empty Tableau
    from_pos = Position(LocationType.TABLEAU, 0)
    to_pos = Position(LocationType.TABLEAU, 1)
    assert app.find_max_valid_count(from_pos, to_pos) == 0

    # With some sequence
    c10 = Card(Suit.HEARTS, Rank.TEN)
    c9 = Card(Suit.SPADES, Rank.NINE)
    app.state.tableaus[0] = [c10, c9]
    # Destination is empty column, max possible sequence to move is 2
    assert app.find_max_valid_count(from_pos, to_pos) == 2
