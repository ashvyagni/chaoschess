"""Offscreen smoke test of the PySide6 GUI's game loop.

Drives ChessGUI without a display (QT_QPA_PLATFORM=offscreen): human moves, engine
replies through uci_bridge with the full move history, and a threefold repetition
ending. Skipped if PySide6 or the release engine binary is missing.
"""

from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

try:
    from PySide6.QtWidgets import QApplication
except ImportError:  # pragma: no cover
    QApplication = None


def _use_visible_plugin_copy_if_needed():
    """Test-only workaround for iCloud-hidden Qt plugins (see gui.qt_platform_plugin_problem).

    Qt aborts the *whole test process* when it cannot find a platform plugin, which
    would take every other test down with it. If the plugins are hidden, copy the three
    platform plugins to a temporary directory and point Qt there, for this process only.
    The GUI itself refuses to start in that state and says why; it does not do this.
    """
    import shutil
    import tempfile
    from gui import qt_platform_plugin_problem
    import PySide6

    if not qt_platform_plugin_problem():
        return
    source = Path(PySide6.__file__).resolve().parent / "Qt" / "plugins" / "platforms"
    target = Path(tempfile.mkdtemp(prefix="qt-platforms-")) / "platforms"
    shutil.copytree(source, target)
    # copytree preserves BSD file flags on macOS, so the copy arrives hidden too.
    import stat
    for path in [target, *target.iterdir()]:
        os.chflags(path, path.stat().st_flags & ~stat.UF_HIDDEN)
    os.environ["QT_QPA_PLATFORM_PLUGIN_PATH"] = str(target)

ENGINE = ROOT / "target" / "release" / "crazy-chess"


@unittest.skipIf(QApplication is None or not ENGINE.exists(), "needs PySide6 and a release build")
class GuiGameLoop(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        _use_visible_plugin_copy_if_needed()
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        from gui import ChessGUI
        self.gui = ChessGUI()
        self.assertIsNotNone(self.gui.uci_engine, self.gui.status_msg)

    def tearDown(self):
        self.gui.close()

    def human(self, uci):
        from moves import generate_legal_moves
        move = next(m for m in generate_legal_moves(self.gui.board) if str(m) == uci)
        self.gui.make_player_move(move)

    def test_engine_replies_legally_with_full_history(self):
        from moves import generate_legal_moves
        for uci in ["e2e4", "d2d4", "g1f3"]:
            if self.gui.game_over:
                break
            self.assertIn(uci, {str(m) for m in generate_legal_moves(self.gui.board)})
            self.human(uci)
            self.gui.engine_move()  # normally fired by a QTimer
        self.assertEqual(len(self.gui.move_history), 6)
        self.assertEqual(len(self.gui.position_keys), 7)
        self.assertFalse(self.gui.game_over)
        # The engine was handed the move list, so it is at the same position as the GUI.
        self.assertEqual(self.gui.uci_engine.last_info.depth, self.gui.uci_engine.depth)

    def test_threefold_repetition_ends_the_game(self):
        # Replay a knight shuffle through the GUI's own move handling (as if both sides
        # were human), then check the GUI declares the draw.
        self.gui.engine_move = lambda: None
        for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] * 2:
            self.gui.engine_thinking = False
            self.human(uci)
        self.assertTrue(self.gui.game_over)
        self.assertIn("threefold repetition", self.gui.status_label.text())


if __name__ == "__main__":
    unittest.main()
