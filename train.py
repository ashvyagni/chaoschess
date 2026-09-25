import random
import numpy as np
from collections import deque
from board import *
from moves import *
from mcts import *
from neural_net import *
from config import *


class ReplayBuffer:
    def __init__(self, maxlen=100000):
        self.buffer = deque(maxlen=maxlen)

    def push(self, state, policy, value):
        self.buffer.append((state, policy, value))

    def sample(self, batch_size):
        batch = random.sample(self.buffer, min(batch_size, len(self.buffer)))
        states, policies, values = zip(*batch)
        return list(states), list(policies), list(values)

    def __len__(self):
        return len(self.buffer)


def play_one_game(net, game_num=0, verbose=False):
    board = Board()
    board.set_fen(FEN_START)
    mcts = MCTS(net, sims=MCTS_SIMS)

    states = []
    policies = []
    current_player = []
    move_count = 0

    while not is_game_over(board) and move_count < 200:
        moves, probs, root = mcts.get_move_probs(board, temperature=TEMPERATURE)
        state = encode_board(board)
        policy = np.zeros(POLICY_SIZE)
        for move, p in zip(moves, probs):
            policy[move.to_index()] = p

        states.append(state)
        policies.append(policy)
        current_player.append(board.color)

        move_idx = np.random.choice(len(moves), p=probs)
        chosen_move = moves[move_idx]

        if verbose and move_count % 10 == 0:
            print(f"  Move {move_count + 1}: {chosen_move}")

        board = apply_move(board, chosen_move)
        move_count += 1

    result = get_result(board)
    if verbose:
        if result == 1:
            print(f"  Result: White wins in {move_count} moves")
        elif result == -1:
            print(f"  Result: Black wins in {move_count} moves")
        else:
            print(f"  Result: Draw in {move_count} moves")

    training_data = []
    for state, policy, player in zip(states, policies, current_player):
        if player == WHITE:
            value = result
        else:
            value = -result
        training_data.append((state, policy, value))

    return training_data


def self_play(net, num_games=10, verbose=True):
    replay_buffer = ReplayBuffer(REPLAY_SIZE)

    for i in range(num_games):
        if verbose:
            print(f"\nSelf-play game {i + 1}/{num_games}...")

        data = play_one_game(net, game_num=i, verbose=verbose)
        for state, policy, value in data:
            replay_buffer.push(state, policy, value)

        if verbose:
            print(f"  Added {len(data)} positions to replay buffer")

    return replay_buffer


def train_on_buffer(net, replay_buffer, epochs=5, batch_size=64):
    if len(replay_buffer) < batch_size:
        return 0.0

    total_loss = 0.0
    num_batches = 0

    for _ in range(epochs):
        states, policies, values = replay_buffer.sample(batch_size)
        loss = net.train_step(states, policies, values)
        total_loss += loss
        num_batches += 1

    return total_loss / num_batches if num_batches > 0 else 0.0


def train_loop(num_iterations=50, games_per_iter=10, save_path='model.pkl', log_file='training_log.txt'):
    net = ChessNet()

    if not net.load(save_path):
        print("Starting from scratch")
    else:
        print(f"Loaded model (step {net.t})")

    log = open(log_file, 'a')

    for iteration in range(1, num_iterations + 1):
        print(f"\n{'='*50}")
        print(f"Iteration {iteration}/{num_iterations}")
        print(f"{'='*50}")

        replay_buffer = self_play(net, num_games=games_per_iter, verbose=True)

        print(f"\nTraining on {len(replay_buffer)} positions...")
        loss = train_on_buffer(net, replay_buffer, epochs=EPOCHS_PER_TRAIN, batch_size=BATCH_SIZE)
        print(f"Average loss: {loss:.4f}")

        if iteration % SAVE_EVERY == 0:
            net.save(save_path)
            print(f"Saved model to {save_path}")

        log.write(f"Iter {iteration} | Loss: {loss:.4f} | Buffer: {len(replay_buffer)}\n")
        log.flush()

    log.close()
    net.save(save_path)
    print("\nTraining complete!")
    return net
