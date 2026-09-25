import math
import random
import numpy as np
from board import *
from moves import *
from config import *


class MCTSNode:
    def __init__(self, board, parent=None, move=None, prior=0.0):
        self.board = board
        self.parent = parent
        self.move = move
        self.prior = prior
        self.children = []
        self.visit_count = 0
        self.total_value = 0.0
        self.is_expanded = False

    def q_value(self):
        if self.visit_count == 0:
            return 0.0
        return self.total_value / self.visit_count

    def uct_score(self, c_puct):
        exploration = c_puct * self.prior * math.sqrt(self.parent.visit_count) / (1 + self.visit_count)
        return self.q_value() + exploration

    def best_child(self, c_puct):
        return max(self.children, key=lambda n: n.uct_score(c_puct))

    def is_leaf(self):
        return not self.is_expanded


class MCTS:
    def __init__(self, net, sims=400, c_puct=1.5):
        self.net = net
        self.sims = sims
        self.c_puct = c_puct

    def search(self, board, temperature=1.0, add_noise=True):
        root = MCTSNode(board.copy())
        self.expand_node(root, add_noise=add_noise)

        for _ in range(self.sims):
            node = root
            while not node.is_leaf():
                node = node.best_child(self.c_puct)

            if is_game_over(node.board):
                if is_checkmate(node.board):
                    value = get_result(node.board)
                    if node.board.color == WHITE:
                        value = -value
                else:
                    value = 0.0
            else:
                value = self.expand_node(node)

            self.backpropagate(node, value)

        visits = np.array([c.visit_count for c in root.children], dtype=np.float32)

        if temperature < 0.01:
            best_idx = np.argmax(visits)
            probs = np.zeros(len(visits))
            probs[best_idx] = 1.0
        else:
            visits_temp = visits ** (1.0 / temperature)
            total = visits_temp.sum()
            if total > 0:
                probs = visits_temp / total
            else:
                probs = np.ones(len(visits)) / len(visits)

        return root, probs

    def expand_node(self, node, add_noise=False):
        legal_moves = generate_legal_moves(node.board)

        if len(legal_moves) == 0:
            node.is_expanded = True
            if node.board.in_check():
                return -1.0
            return 0.0

        encoding = encode_board(node.board)
        policy, value = self.net.predict(encoding)

        node.is_expanded = True

        priors = []
        for move in legal_moves:
            idx = move.to_index()
            priors.append(policy[idx])

        prior_sum = sum(priors)
        if prior_sum > 0:
            priors = [p / prior_sum for p in priors]
        else:
            priors = [1.0 / len(legal_moves)] * len(legal_moves)

        if add_noise:
            noise = np.random.dirichlet([DIRICHLET_ALPHA] * len(legal_moves))
            priors = [(1 - DIRICHLET_EPSILON) * p + DIRICHLET_EPSILON * n
                      for p, n in zip(priors, noise)]

        for move, prior in zip(legal_moves, priors):
            child_board = apply_move(node.board, move)
            child = MCTSNode(child_board, parent=node, move=move, prior=prior)
            node.children.append(child)

        return value

    def backpropagate(self, node, value):
        current = node
        v = value
        while current is not None:
            current.visit_count += 1
            current.total_value += v
            v = -v
            current = current.parent

    def get_move_probs(self, board, temperature=1.0):
        root, probs = self.search(board, temperature=temperature)
        moves = [child.move for child in root.children]
        return moves, probs, root
