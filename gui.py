import sys
import math
import time
import numpy as np
from PySide6.QtWidgets import *
from PySide6.QtCore import *
from PySide6.QtGui import *
from board import *
from moves import *
from mcts import *
from neural_net import *
from config import *
from uci_bridge import UCIEngine

SQUARE_SIZE = 80
BOARD_PX = SQUARE_SIZE * 8
SIDEBAR_W = 280
WINDOW_W = BOARD_PX + SIDEBAR_W
WINDOW_H = BOARD_PX + 60

LIGHT_SQ = QColor(240, 217, 181)
DARK_SQ = QColor(181, 136, 99)
HIGHLIGHT = QColor(255, 255, 0, 120)
MOVE_DOT = QColor(0, 0, 0, 80)
LAST_MOVE = QColor(205, 210, 106, 150)
CHECK_COLOR = QColor(255, 0, 0, 140)

PIECE_UNICODE = {
    W_PAWN: '♙', W_KNIGHT: '♘', W_BISHOP: '♗', W_ROOK: '♖', W_QUEEN: '♕', W_KING: '♔',
    B_PAWN: '♟', B_KNIGHT: '♞', B_BISHOP: '♝', B_ROOK: '♜', B_QUEEN: '♛', B_KING: '♚',
}

PIECE_NAMES = {
    W_PAWN: 'White Pawn', W_KNIGHT: 'White Knight', W_BISHOP: 'White Bishop',
    W_ROOK: 'White Rook', W_QUEEN: 'White Queen', W_KING: 'White King',
    B_PAWN: 'Black Pawn', B_KNIGHT: 'Black Knight', B_BISHOP: 'Black Bishop',
    B_ROOK: 'Black Rook', B_QUEEN: 'Black Queen', B_KING: 'Black King',
}


class ChessGUI(QMainWindow):
    def __init__(self):
        super().__init__()
        self.board = Board()
        self.board.set_fen(FEN_START)
        self.selected_sq = -1
        self.legal_moves_for_sq = []
        self.last_move = None
        self.game_over = False
        self.player_color = WHITE
        self.engine_thinking = False
        self.move_history = []
        self.status_msg = "Your turn (White)"

        self.uci_engine = None
        try:
            self.uci_engine = UCIEngine(depth=4, style="Chaos")
        except (OSError, RuntimeError) as error:
            self.status_msg = f"Rust engine unavailable: {error}"

        self.setWindowTitle("Chess Engine")
        self.setFixedSize(WINDOW_W, WINDOW_H)

        central = QWidget()
        self.setCentralWidget(central)
        layout = QHBoxLayout(central)
        layout.setContentsMargins(0, 0, 0, 0)

        self.board_widget = QWidget()
        self.board_widget.setFixedSize(BOARD_PX, BOARD_PX)
        self.board_widget.paintEvent = self.paint_board
        self.board_widget.mousePressEvent = self.handle_click
        layout.addWidget(self.board_widget)

        self.build_sidebar(layout)

        self.setStyleSheet("background-color: #2b2b2b; color: white;")
        self.show()

    def build_sidebar(self, parent_layout):
        sidebar = QWidget()
        sidebar.setFixedSize(SIDEBAR_W, WINDOW_H)
        sb_layout = QVBoxLayout(sidebar)
        sb_layout.setContentsMargins(15, 15, 15, 15)

        title = QLabel("CHESS ENGINE")
        title.setStyleSheet("font-size: 20px; font-weight: bold; color: #e0e0e0; padding: 5px;")
        title.setAlignment(Qt.AlignCenter)
        sb_layout.addWidget(title)

        sb_layout.addSpacing(10)

        self.eval_label = QLabel("Eval: 0.00")
        self.eval_label.setStyleSheet("font-size: 16px; color: #aaa;")
        self.eval_label.setAlignment(Qt.AlignCenter)
        sb_layout.addWidget(self.eval_label)

        self.turn_label = QLabel("White to move")
        self.turn_label.setStyleSheet("font-size: 14px; color: #888;")
        self.turn_label.setAlignment(Qt.AlignCenter)
        sb_layout.addWidget(self.turn_label)

        self.status_label = QLabel(self.status_msg)
        self.status_label.setStyleSheet("font-size: 12px; color: #ff9800;")
        self.status_label.setAlignment(Qt.AlignCenter)
        sb_layout.addWidget(self.status_label)

        style_label = QLabel("Playstyle")
        style_label.setStyleSheet("font-size: 12px; color: #888;")
        sb_layout.addWidget(style_label)
        self.style_combo = QComboBox()
        self.style_combo.addItems(["Chaos", "Classical"])
        self.style_combo.setStyleSheet(
            "QComboBox { background: #1e1e1e; color: #ddd; padding: 5px; border: 1px solid #444; }"
        )
        self.style_combo.currentTextChanged.connect(self.set_engine_style)
        sb_layout.addWidget(self.style_combo)

        sb_layout.addSpacing(15)

        sep = QFrame()
        sep.setFrameShape(QFrame.HLine)
        sep.setStyleSheet("color: #555;")
        sb_layout.addWidget(sep)

        sb_layout.addSpacing(5)

        hist_label = QLabel("Moves")
        hist_label.setStyleSheet("font-size: 13px; color: #999; font-weight: bold;")
        sb_layout.addWidget(hist_label)

        self.move_list = QListWidget()
        self.move_list.setStyleSheet("""
            QListWidget { background: #1e1e1e; border: 1px solid #444; font-size: 12px; color: #ccc; }
            QListWidget::item { padding: 2px 5px; }
        """)
        sb_layout.addWidget(self.move_list)

        sb_layout.addSpacing(10)

        self.new_game_btn = QPushButton("New Game")
        self.new_game_btn.setStyleSheet(self.btn_style())
        self.new_game_btn.clicked.connect(self.new_game)
        sb_layout.addWidget(self.new_game_btn)

        self.flip_btn = QPushButton("Flip Board")
        self.flip_btn.setStyleSheet(self.btn_style())
        self.flip_btn.clicked.connect(self.flip_board)
        sb_layout.addWidget(self.flip_btn)

        sb_layout.addSpacing(5)

        btn_row = QHBoxLayout()
        self.play_white_btn = QPushButton("Play White")
        self.play_white_btn.setStyleSheet(self.btn_style())
        self.play_white_btn.clicked.connect(lambda: self.set_player_color(WHITE))
        btn_row.addWidget(self.play_white_btn)

        self.play_black_btn = QPushButton("Play Black")
        self.play_black_btn.setStyleSheet(self.btn_style())
        self.play_black_btn.clicked.connect(lambda: self.set_player_color(BLACK))
        btn_row.addWidget(self.play_black_btn)
        sb_layout.addLayout(btn_row)

        sb_layout.addSpacing(5)

        self.vs_engine_btn = QPushButton("vs Engine")
        self.vs_engine_btn.setStyleSheet(self.btn_style())
        self.vs_engine_btn.clicked.connect(self.play_vs_engine)
        sb_layout.addWidget(self.vs_engine_btn)

        self.engine_v_engine_btn = QPushButton("Engine vs Engine")
        self.engine_v_engine_btn.setStyleSheet(self.btn_style())
        self.engine_v_engine_btn.clicked.connect(self.play_engine_vs_engine)
        sb_layout.addWidget(self.engine_v_engine_btn)

        sb_layout.addStretch()

        parent_layout.addWidget(sidebar)

    def btn_style(self):
        return """
            QPushButton {
                background-color: #3a3a3a; color: #ddd; border: 1px solid #555;
                padding: 8px; border-radius: 4px; font-size: 13px;
            }
            QPushButton:hover { background-color: #4a4a4a; }
            QPushButton:pressed { background-color: #555; }
        """

    def set_player_color(self, color):
        self.player_color = color
        self.new_game()

    def set_engine_style(self, style):
        if self.uci_engine is not None:
            self.uci_engine.set_style(style)
        self.status_label.setText(f"{style} style ready")

    def new_game(self):
        self.board = Board()
        self.board.set_fen(FEN_START)
        self.selected_sq = -1
        self.legal_moves_for_sq = []
        self.last_move = None
        self.game_over = False
        self.move_history = []
        self.move_list.clear()
        self.status_msg = "Your turn" if self.board.color == self.player_color else "Engine thinking..."
        self.status_label.setText(self.status_msg)
        self.turn_label.setText("White to move")
        self.eval_label.setText("Eval: 0.00")
        self.board_widget.update()

    def flip_board(self):
        self.player_color = 1 - self.player_color
        self.board_widget.update()

    def paint_board(self, event):
        painter = QPainter(self.board_widget)
        painter.setRenderHint(QPainter.Antialiasing)

        white_king_sq = bit_scan(self.board.pieces[W_KING])
        black_king_sq = bit_scan(self.board.pieces[B_KING])
        white_in_check = self.board.color == WHITE and self.board.in_check()
        black_in_check = self.board.color == BLACK and self.board.in_check()

        for sq in range(64):
            f = sq_file(sq)
            r = sq_rank(sq)

            if self.player_color == WHITE:
                draw_f, draw_r = f, 7 - r
            else:
                draw_f, draw_r = 7 - f, r

            x = draw_f * SQUARE_SIZE
            y = (7 - draw_r) * SQUARE_SIZE

            is_light = (f + r) % 2 == 0
            color = LIGHT_SQ if is_light else DARK_SQ

            if self.last_move and (sq == self.last_move.from_sq or sq == self.last_move.to_sq):
                color = LAST_MOVE

            painter.fillRect(x, y, SQUARE_SIZE, SQUARE_SIZE, color)

            if sq == white_king_sq and white_in_check:
                painter.fillRect(x, y, SQUARE_SIZE, SQUARE_SIZE, CHECK_COLOR)
            elif sq == black_king_sq and black_in_check:
                painter.fillRect(x, y, SQUARE_SIZE, SQUARE_SIZE, CHECK_COLOR)

            if sq == self.selected_sq:
                painter.fillRect(x, y, SQUARE_SIZE, SQUARE_SIZE, HIGHLIGHT)

            piece = self.board.piece_at(sq)
            if piece != -1:
                char = PIECE_UNICODE[piece]
                painter.setFont(QFont("Segoe UI Symbol", 48))
                if piece >= 6:
                    painter.setPen(QPen(QColor(0, 0, 0), 1))
                    painter.drawText(x, y, SQUARE_SIZE, SQUARE_SIZE, Qt.AlignCenter, char)
                else:
                    painter.setPen(QPen(QColor(255, 255, 255), 1))
                    painter.drawText(x, y, SQUARE_SIZE, SQUARE_SIZE, Qt.AlignCenter, char)

            if sq in [m.to_sq for m in self.legal_moves_for_sq]:
                painter.setBrush(QBrush(MOVE_DOT))
                painter.setPen(Qt.NoPen)
                center = QPoint(x + SQUARE_SIZE // 2, y + SQUARE_SIZE // 2)
                painter.drawEllipse(center, 12, 12)

        painter.setPen(QPen(QColor(80, 80, 80), 1))
        for i in range(9):
            painter.drawLine(i * SQUARE_SIZE, 0, i * SQUARE_SIZE, BOARD_PX)
            painter.drawLine(0, i * SQUARE_SIZE, BOARD_PX, i * SQUARE_SIZE)

        painter.setFont(QFont("Arial", 10))
        painter.setPen(QColor(150, 150, 150))
        for f in range(8):
            if self.player_color == WHITE:
                ch = chr(ord('a') + f)
            else:
                ch = chr(ord('h') - f)
            painter.drawText(f * SQUARE_SIZE + SQUARE_SIZE - 12, BOARD_PX - 4, ch)
        for r in range(8):
            if self.player_color == WHITE:
                num = str(r + 1)
            else:
                num = str(8 - r)
            painter.drawText(3, (7 - r) * SQUARE_SIZE + 14, num)

        painter.end()

    def handle_click(self, event):
        if self.game_over or self.engine_thinking:
            return

        if self.board.color != self.player_color:
            return

        mx = event.pos().x()
        my = event.pos().y()

        if mx < 0 or mx >= BOARD_PX or my < 0 or my >= BOARD_PX:
            return

        click_f = mx // SQUARE_SIZE
        click_r = 7 - my // SQUARE_SIZE

        if self.player_color == WHITE:
            f = click_f
            r = click_r
        else:
            f = 7 - click_f
            r = click_r

        sq = make_sq(f, r)

        if self.selected_sq == -1:
            piece = self.board.piece_at(sq)
            if piece != -1:
                is_white_piece = piece < 6
                if (self.board.color == WHITE and is_white_piece) or \
                   (self.board.color == BLACK and not is_white_piece):
                    self.selected_sq = sq
                    self.legal_moves_for_sq = [m for m in generate_legal_moves(self.board) if m.from_sq == sq]
                    self.board_widget.update()
        else:
            move_to_make = None
            for m in self.legal_moves_for_sq:
                if m.to_sq == sq:
                    if m.promo != -1:
                        promo = self.ask_promotion()
                        if promo == -1:
                            self.selected_sq = -1
                            self.legal_moves_for_sq = []
                            self.board_widget.update()
                            return
                        m = Move(m.from_sq, m.to_sq, promo=promo)
                    move_to_make = m
                    break

            if move_to_make:
                self.make_player_move(move_to_make)
            else:
                piece = self.board.piece_at(sq)
                if piece != -1:
                    is_white_piece = piece < 6
                    if (self.board.color == WHITE and is_white_piece) or \
                       (self.board.color == BLACK and not is_white_piece):
                        self.selected_sq = sq
                        self.legal_moves_for_sq = [m for m in generate_legal_moves(self.board) if m.from_sq == sq]
                    else:
                        self.selected_sq = -1
                        self.legal_moves_for_sq = []
                else:
                    self.selected_sq = -1
                    self.legal_moves_for_sq = []
                self.board_widget.update()

    def ask_promotion(self):
        items = ["Queen", "Rook", "Bishop", "Knight"]
        item, ok = QInputDialog.getItem(self, "Promote Pawn", "Choose piece:", items, 0, False)
        if ok:
            idx = items.index(item)
            return [PROMO_QUEEN, PROMO_ROOK, PROMO_BISHOP, PROMO_KNIGHT][idx]
        return -1

    def make_player_move(self, move):
        self.move_history.append(str(move))
        self.move_list.addItem(f"{len(self.move_history)}. {move}")
        self.last_move = move
        self.board = apply_move(self.board, move)
        self.selected_sq = -1
        self.legal_moves_for_sq = []

        self.turn_label.setText("White to move" if self.board.color == WHITE else "Black to move")
        self.board_widget.update()

        if is_game_over(self.board):
            self.end_game()
        else:
            self.status_label.setText("Engine thinking...")
            self.engine_thinking = True
            QTimer.singleShot(50, self.engine_move)

    def engine_move(self):
        legal_moves = generate_legal_moves(self.board)
        if not legal_moves:
            self.end_game()
            return
        if self.uci_engine is None:
            self.status_label.setText("Rust engine unavailable")
            self.engine_thinking = False
            return
        move_text = self.uci_engine.best_move(self.board.to_fen())
        move = next((candidate for candidate in legal_moves if str(candidate) == move_text), None)
        if move is None:
            raise RuntimeError(f"Rust engine returned unknown legal move: {move_text}")

        self.move_history.append(str(move))
        self.move_list.addItem(f"{len(self.move_history)}. {move}")
        self.last_move = move
        self.board = apply_move(self.board, move)

        info = self.uci_engine.last_info
        self.eval_label.setText(
            f"Eval: {info.score_cp / 100:.2f} | d{info.depth} | {info.nodes:,} nodes"
        )

        self.turn_label.setText("White to move" if self.board.color == WHITE else "Black to move")
        self.engine_thinking = False

        if is_game_over(self.board):
            self.end_game()
        else:
            self.status_label.setText("Your turn")

        self.board_widget.update()

    def play_vs_engine(self):
        self.new_game()
        if self.player_color == BLACK:
            self.engine_thinking = True
            QTimer.singleShot(100, self.engine_move)

    def play_engine_vs_engine(self):
        self.new_game()
        self.game_over = False
        self.status_label.setText("Engine vs Engine...")
        self.engine_thinking = True
        QTimer.singleShot(100, self.engine_vs_engine_step)

    def engine_vs_engine_step(self):
        if self.game_over or is_game_over(self.board):
            self.end_game()
            return

        self.engine_move()
        if self.game_over:
            return
        self.board_widget.update()
        QTimer.singleShot(300, self.engine_vs_engine_step)

    def end_game(self):
        self.game_over = True
        self.engine_thinking = False
        result = get_result(self.board)
        if is_checkmate(self.board):
            winner = "Black" if self.board.color == WHITE else "White"
            self.status_label.setText(f"Checkmate! {winner} wins!")
        else:
            self.status_label.setText("Draw!")
        self.board_widget.update()

    def closeEvent(self, event):
        if self.uci_engine is not None:
            self.uci_engine.close()
        super().closeEvent(event)


def run_gui():
    app = QApplication(sys.argv)
    app.setStyle('Fusion')
    palette = QPalette()
    palette.setColor(QPalette.Window, QColor(43, 43, 43))
    palette.setColor(QPalette.WindowText, QColor(220, 220, 220))
    app.setPalette(palette)
    window = ChessGUI()
    sys.exit(app.exec())


if __name__ == '__main__':
    run_gui()
