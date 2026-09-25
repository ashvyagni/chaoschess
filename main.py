import sys
import argparse
from board import *
from moves import *
from neural_net import *
from train import *
from gui import run_gui


def cmd_play():
    run_gui()


def cmd_train(args):
    net = ChessNet()
    net.load('model.pkl')
    train_loop(
        num_iterations=args.iterations,
        games_per_iter=args.games,
        save_path='model.pkl'
    )


def cmd_bench(args):
    net = ChessNet()
    net.load('model.pkl')
    print(f"Running {args.games} benchmark games...")
    replay = self_play(net, num_games=args.games, verbose=True)
    print(f"\nTotal positions: {len(replay)}")


def cmd_selfplay_visual():
    from PySide6.QtWidgets import QApplication
    from PySide6.QtCore import QTimer
    import time

    from gui import qt_platform_plugin_problem
    problem = qt_platform_plugin_problem()
    if problem:
        print(problem, file=sys.stderr)
        sys.exit(1)
    app = QApplication.instance() or QApplication(sys.argv)
    app.setStyle('Fusion')

    net = ChessNet()
    net.load('model.pkl')

    from gui import ChessGUI
    window = ChessGUI()
    window.player_color = WHITE
    window.play_engine_vs_engine()
    sys.exit(app.exec())


def main():
    parser = argparse.ArgumentParser(description="Chess Engine")
    sub = parser.add_subparsers(dest='command')

    sub.add_parser('play', help='Play against the engine')
    sub.add_parser('watch', help='Watch engine vs engine')

    train_p = sub.add_parser('train', help='Train the engine')
    train_p.add_argument('-i', '--iterations', type=int, default=50)
    train_p.add_argument('-g', '--games', type=int, default=10)

    bench_p = sub.add_parser('bench', help='Benchmark games')
    bench_p.add_argument('-g', '--games', type=int, default=5)

    args = parser.parse_args()

    if args.command == 'play':
        cmd_play()
    elif args.command == 'watch':
        cmd_selfplay_visual()
    elif args.command == 'train':
        cmd_train(args)
    elif args.command == 'bench':
        cmd_bench(args)
    else:
        cmd_play()


if __name__ == '__main__':
    main()
