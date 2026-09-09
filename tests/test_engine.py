import unittest

from chess_engine.board import Board
from chess_engine.nnue import default_network
from chess_engine.perft import perft
from chess_engine.search import Search, SearchLimits


class BoardTests(unittest.TestCase):
    def test_start_position_perft(self) -> None:
        board = Board()
        self.assertEqual(perft(board, 1), 20)
        self.assertEqual(perft(board, 2), 400)
        self.assertEqual(perft(board, 3), 8902)
        self.assertEqual(perft(board, 4), 197281)

    def test_reference_positions(self) -> None:
        kiwi = Board("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
        pos3 = Board("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1")
        pos4 = Board("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1")
        self.assertEqual(perft(kiwi, 3), 97862)
        self.assertEqual(perft(pos3, 4), 43238)
        self.assertEqual(perft(pos4, 3), 9467)

    def test_make_undo_restores_everything(self) -> None:
        board = Board()
        original = (board.fen(), board.key, board.board[:], board.accumulator[:])
        for move in board.legal_moves():
            board.make_move(move)
            board.undo_move()
            self.assertEqual((board.fen(), board.key, board.board, board.accumulator), original)

    def test_zobrist_and_accumulator(self) -> None:
        board = Board()
        self.assertEqual(board.key, board.zobrist())
        self.assertTrue(default_network.verify_accumulator(board))
        board.make_move(board.find_move("e2e4"))
        self.assertEqual(board.key, board.zobrist())
        self.assertTrue(default_network.verify_accumulator(board))

    def test_special_moves_are_legal(self) -> None:
        castle = Board("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1")
        self.assertIn(castle.find_move("e1g1"), castle.legal_moves())
        castle.make_move(castle.find_move("e1g1"))
        self.assertEqual(castle.fen().split()[2], "kq")
        castle.undo_move()

        ep = Board("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1")
        self.assertIn(ep.find_move("e5d6"), ep.legal_moves())
        ep.make_move(ep.find_move("e5d6"))
        self.assertEqual(ep.board[43], 1)
        self.assertEqual(ep.board[35], 0)
        ep.undo_move()

        promotion = Board("4k3/P7/8/8/8/8/8/4K3 w - - 0 1")
        self.assertIn(promotion.find_move("a7a8q"), promotion.legal_moves())
        promotion.make_move(promotion.find_move("a7a8q"))
        self.assertEqual(promotion.board[56], 5)


class SearchTests(unittest.TestCase):
    def test_search_returns_legal_move(self) -> None:
        board = Board()
        result = Search().search(board, SearchLimits(depth=2))
        self.assertIn(result.best_move, board.legal_moves())
        self.assertGreater(result.nodes, 0)


if __name__ == "__main__":
    unittest.main()