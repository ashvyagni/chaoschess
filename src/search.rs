//! Search: iterative deepening, root PVS with aspiration windows, negamax with PVS,
//! null-move pruning, reverse futility pruning, LMR, check extension, quiescence,
//! move ordering, draw detection, and the long-lived `Engine`.

use super::*;

/// Deepest iterative-deepening depth the engine will attempt.
pub const MAX_DEPTH: u8 = 64;

/// Rows in the triangular principal-variation table; main-search ply never exceeds MAX_DEPTH.
pub(crate) const PV_ROWS: usize = MAX_DEPTH as usize + 2;

/// Reverse futility pruning applies at this remaining depth or less...
pub(crate) const RFP_MAX_DEPTH: u8 = 6;

/// ...when the static eval beats beta by this many centipawns per ply of depth.
pub(crate) const RFP_MARGIN: i32 = 90;

/// Move-ordering bands; see `Searcher::ordered`.
pub(crate) const ORDER_TT: i64 = 4_000_000;

pub(crate) const ORDER_GOOD_CAPTURE: i64 = 3_000_000;

pub(crate) const ORDER_KILLER: i64 = 2_000_000;

pub(crate) const ORDER_BAD_CAPTURE: i64 = -3_000_000;

/// Null-move pruning is tried only with at least this much depth left.
pub(crate) const NULL_MOVE_MIN_DEPTH: u8 = 3;

/// Null-move search depth is `depth - 1 - (NULL_MOVE_BASE_REDUCTION + depth / 6)`.
pub(crate) const NULL_MOVE_BASE_REDUCTION: u8 = 3;

/// Late move reductions apply from this remaining depth...
pub(crate) const LMR_MIN_DEPTH: u8 = 3;

/// ...to moves at this index or later in the ordered list (0-based).
pub(crate) const LMR_MIN_INDEX: usize = 3;

/// How many plies to reduce a late quiet move: grows with both depth and move index
/// (the usual logarithmic shape), one ply less at PV nodes, and never so much that the
/// reduced search would skip straight to quiescence.
pub(crate) fn lmr_reduction(depth: u8, index: usize, pv_node: bool) -> u8 {
    let r = 0.75 + (f64::from(depth)).ln() * (index as f64).ln() / 2.25;
    let r = (r as u8).saturating_sub(u8::from(pv_node)).max(1);
    r.min(depth.saturating_sub(2))
}

/// Ceiling on history-heuristic values. History persists across iterative-deepening
/// iterations, so without a bound it grows until it outranks the TT move and captures.
pub(crate) const HISTORY_MAX: i32 = 16_384;

pub const MAX_QUIESCENCE_PLY: u8 = 32;

/// How many plies into the quiescence search non-capturing checks are still
/// searched. Searching quiet checks is valuable -- it finds short forced mates that a
/// captures-only quiescence walks straight past -- but it must be bounded, because quiet
/// checks generate further quiet checks. Leaving it unbounded is what made the audited
/// baseline unable to finish a one-ply search in a middlegame position; see
/// `experiments/E1-quiescence-quiet-checks.md`.
pub const QS_CHECK_PLIES: u8 = 2;

#[derive(Debug, Clone, Copy)]
pub struct SearchLimits {
    pub depth: u8,
    pub nodes: Option<u64>,
    /// Hard limit: the search is abandoned mid-iteration once this much time has passed.
    pub time: Option<Duration>,
    /// Soft limit: no new iteration is started once half of this has passed, because the
    /// next iteration would very likely overrun it. Used for clock-based time control;
    /// `None` means "use the hard limit only" (e.g. `go movetime`).
    pub soft_time: Option<Duration>,
    pub hash_mb: usize,
    pub style: Style,
    pub threads: usize,
    /// Plies into quiescence for which non-capturing checks are still searched.
    ///
    /// Exposed rather than hard-coded so the tradeoff can be *measured* instead of
    /// assumed: raising it buys tactical sight and costs nodes exponentially. Setting it
    /// to [`MAX_QUIESCENCE_PLY`] reproduces the unbounded behaviour of the audited
    /// baseline, which is how the two are compared.
    pub qs_check_plies: u8,
    /// Prune captures that static exchange evaluation scores as losing material.
    pub qs_see_pruning: bool,
    /// Enable the deliberately non-score-preserving techniques: null-move pruning,
    /// reverse futility pruning, late move reductions and the check extension. They are
    /// what make the engine strong, and also what make its scores differ from plain
    /// minimax. Turning them off leaves only score-preserving machinery (alpha-beta,
    /// PVS, aspiration windows, TT, quiescence), which the exactness tests verify, and
    /// gives a full-width baseline for research comparisons.
    pub selective: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub best_move: ChessMove,
    /// Deepest *completed* iteration. An iteration interrupted by a limit is discarded,
    /// never reported as completed.
    pub depth: u8,
    pub seldepth: u8,
    pub score: i32,
    pub nodes: u64,
    /// Principal variation of the deepest completed iteration, starting with `best_move`.
    pub pv: Vec<ChessMove>,
}

/// Progress report emitted after every completed iterative-deepening iteration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchInfo {
    pub depth: u8,
    /// Deepest ply reached, including quiescence.
    pub seldepth: u8,
    /// Centipawns from the side to move's point of view, or a mate score; see
    /// [`mate_in_moves`].
    pub score: i32,
    pub nodes: u64,
    pub elapsed: Duration,
    pub pv: Vec<ChessMove>,
    /// Transposition-table occupancy by the current search, in permille.
    pub hashfull: u32,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            depth: 6,
            nodes: None,
            time: None,
            soft_time: None,
            hash_mb: 16,
            style: Style::Classical,
            threads: 1,
            qs_check_plies: QS_CHECK_PLIES,
            qs_see_pruning: true,
            selective: true,
        }
    }
}

pub(crate) struct Searcher {
    pub(crate) table: Table,
    pub(crate) limits: SearchLimits,
    pub(crate) start: Instant,
    pub(crate) nodes: u64,
    pub(crate) stopped: bool,
    pub(crate) history: [[i32; 64]; 64],
    /// Set by another thread (UCI `stop`, `quit`, a new `go`) to end the search.
    pub(crate) stop_flag: Arc<AtomicBool>,
    /// Time and external stops are honoured only once depth 1 is complete, so there is
    /// always a searched move to return rather than an arbitrary legal one.
    pub(crate) can_abort: bool,
    /// Triangular PV table: `pv[ply]` is the best line found from `ply` in the current node.
    pub(crate) pv: Vec<Vec<ChessMove>>,
    pub(crate) seldepth: u8,
    /// Hashes of every position from the oldest reversible game position to the current
    /// node, inclusive. Used for repetition detection.
    pub(crate) path: Vec<u64>,
    /// Halfmove clock of each node from the root to the current node.
    pub(crate) clocks: Vec<u32>,
    /// Whether each node from the root was reached by a null move, so two null moves are
    /// never made in a row (that would just hand the move back).
    pub(crate) null_moves: Vec<bool>,
    /// Two killer moves per ply.
    pub(crate) killers: Vec<[Option<ChessMove>; 2]>,
    /// Depth of the current iterative-deepening iteration; bounds check extensions.
    pub(crate) root_depth: u8,
    /// Static evaluation. The search only ever calls `evaluate` through this.
    pub(crate) evaluator: Arc<dyn Evaluator>,
}

impl Searcher {
    #[cfg(test)]
    pub(crate) fn new(limits: SearchLimits) -> Self {
        Self::with_table(
            limits,
            Table::new(limits.hash_mb),
            Arc::new(AtomicBool::new(false)),
            Arc::new(StyleEvaluator(limits.style)),
        )
    }

    pub(crate) fn with_table(
        limits: SearchLimits,
        table: Table,
        stop_flag: Arc<AtomicBool>,
        evaluator: Arc<dyn Evaluator>,
    ) -> Self {
        Self {
            table,
            limits,
            start: Instant::now(),
            nodes: 0,
            stopped: false,
            history: [[0; 64]; 64],
            stop_flag,
            can_abort: false,
            pv: vec![Vec::new(); PV_ROWS],
            seldepth: 0,
            path: Vec::new(),
            clocks: vec![0],
            null_moves: vec![false],
            killers: vec![[None; 2]; PV_ROWS],
            root_depth: MAX_DEPTH,
            evaluator,
        }
    }

    /// Reset the path to a root position and its game history.
    pub(crate) fn set_root(&mut self, root: &Position) {
        self.path.clear();
        self.path.extend_from_slice(&root.prior);
        self.path.push(root.board.get_hash());
        self.clocks.clear();
        self.clocks.push(root.halfmove_clock);
        self.null_moves.clear();
        self.null_moves.push(false);
    }

    /// Search `child` (the result of `m` played in `parent`) and return its score from
    /// the parent's point of view. This is the only way the main search descends a ply,
    /// so the repetition path and the halfmove clocks cannot drift out of step.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn search_child(
        &mut self,
        parent: &Board,
        m: ChessMove,
        child: &Board,
        depth: u8,
        alpha: i32,
        beta: i32,
        ply: u8,
    ) -> i32 {
        let clock = if resets_clock(parent, m) {
            0
        } else {
            self.clocks.last().copied().unwrap_or(0) + 1
        };
        self.path.push(child.get_hash());
        self.clocks.push(clock);
        self.null_moves.push(false);
        let value = -self.negamax(child, depth, -beta, -alpha, ply);
        self.path.pop();
        self.clocks.pop();
        self.null_moves.pop();
        value
    }

    /// Null-move pruning. If the side to move could skip its turn and a reduced search
    /// still fails high, a real move almost certainly fails high too, so the node is cut
    /// off. Returns the cutoff score, or `None` to search normally.
    ///
    /// The guards cover the method's known failure modes:
    /// - not at PV nodes, where an exact score is wanted;
    /// - not in check, where passing is illegal and the idea is meaningless;
    /// - not without non-pawn material: in pawn endings zugzwang is common, and there the
    ///   right to move is a disadvantage, so "passing is fine" proves nothing;
    /// - not twice in a row;
    /// - only when the static evaluation already reaches beta;
    /// - a mate score from the reduced search is not trusted: `beta` is returned instead.
    pub(crate) fn try_null_move(&mut self, board: &Board, static_eval: i32, depth: u8, beta: i32, ply: u8) -> Option<i32> {
        if depth < NULL_MOVE_MIN_DEPTH || self.null_moves.last() == Some(&true) {
            return None;
        }
        let side = *board.color_combined(board.side_to_move());
        let pawns_and_king = *board.pieces(Piece::Pawn) | *board.pieces(Piece::King);
        if side & !pawns_and_king == chess::EMPTY {
            return None;
        }
        if static_eval < beta {
            return None;
        }
        let passed = board.null_move()?;
        let reduction = NULL_MOVE_BASE_REDUCTION + depth / 6;
        // The null move resets the repetition window (clock 0): positions on either side
        // of a pass must never be counted as repetitions of each other.
        self.path.push(passed.get_hash());
        self.clocks.push(0);
        self.null_moves.push(true);
        let score = -self.negamax(&passed, depth.saturating_sub(1 + reduction), -beta, -beta + 1, ply + 1);
        self.path.pop();
        self.clocks.pop();
        self.null_moves.pop();
        if self.stopped || score < beta {
            return None;
        }
        Some(if score >= MATE_THRESHOLD { beta } else { score })
    }

    /// Whether the current node (the last entry of `path`) is a draw by repetition.
    ///
    /// A position that recurs *inside the search* counts as a draw on its first
    /// repetition: if a side can repeat once, it can repeat again, and treating it as a
    /// draw sooner saves search. A position whose earlier occurrences are all in the game
    /// history before the root needs two of them, since that is what makes a real
    /// threefold repetition.
    pub(crate) fn is_repetition(&self, ply: u8) -> bool {
        let n = self.path.len();
        let Some(&current) = self.path.last() else {
            return false;
        };
        let clock = self.clocks.last().copied().unwrap_or(0) as usize;
        let reach = clock.min(n - 1);
        let mut earlier = 0;
        // The same side is to move only every other ply, and a position cannot recur
        // after just two plies, so the scan starts four plies back.
        let mut back = 4;
        while back <= reach {
            if self.path[n - 1 - back] == current {
                if back <= usize::from(ply) {
                    return true;
                }
                earlier += 1;
                if earlier >= 2 {
                    return true;
                }
            }
            back += 2;
        }
        false
    }

    pub(crate) fn stop(&mut self) {
        if self.stopped {
            return;
        }
        // A node budget is exact and always honoured, so fixed-node benchmarks stay
        // reproducible.
        if self.limits.nodes.is_some_and(|n| self.nodes >= n) {
            self.stopped = true;
            return;
        }
        // The clock and the external flag are polled every 1024 nodes: that is ~2 ms at
        // current speeds, and reading them at every node is overhead for nothing.
        if self.can_abort && self.nodes & 1023 == 0 {
            let external = self.stop_flag.load(Ordering::Relaxed);
            let timed_out = self.limits.time.is_some_and(|t| self.start.elapsed() >= t);
            self.stopped = external || timed_out;
        }
    }

    /// `pv[ply] = m` followed by the child's line.
    pub(crate) fn update_pv(&mut self, ply: u8, m: ChessMove) {
        let ply = usize::from(ply);
        if ply + 1 >= self.pv.len() {
            return;
        }
        let (head, tail) = self.pv.split_at_mut(ply + 1);
        let line = &mut head[ply];
        line.clear();
        line.push(m);
        line.extend_from_slice(&tail[0]);
    }
    /// Legal moves, best-first, in the standard bands:
    ///
    /// 1. the transposition-table move;
    /// 2. captures and promotions that static exchange evaluation says don't lose
    ///    material, by MVV-LVA;
    /// 3. the two killer moves for this ply (quiet moves that caused a cutoff at a
    ///    sibling node);
    /// 4. other quiet moves, by history score;
    /// 5. captures that SEE says lose material, last.
    ///
    /// `ply` is `None` in quiescence, which needs only MVV-LVA among captures: it prunes
    /// losing captures itself, so running SEE here too would be paid for twice.
    ///
    /// This replaced an ordering where history (up to 16,384) could outrank captures and
    /// every quiet check was promoted above most captures by playing each move on a board
    /// copy. Experiment E7 traced a failed LMR attempt to that ordering.
    pub(crate) fn ordered(&self, board: &Board, tt: Option<ChessMove>, ply: Option<u8>) -> Vec<ChessMove> {
        let killers = ply.and_then(|p| self.killers.get(usize::from(p))).copied().unwrap_or([None; 2]);
        let mut moves: Vec<_> = MoveGen::new_legal(board).collect();
        // Cached keys: each key is computed once, not once per comparison (see 74fcfb6).
        moves.sort_by_cached_key(|m| {
            let m = *m;
            let score: i64 = if Some(m) == tt {
                ORDER_TT
            } else if is_capture(board, m) || m.get_promotion().is_some() {
                let victim = if is_en_passant(board, m) {
                    PIECE_VALUES[Piece::Pawn.to_index()]
                } else {
                    board.piece_on(m.get_dest()).map_or(0, |p| PIECE_VALUES[p.to_index()])
                };
                let attacker = board.piece_on(m.get_source()).map_or(0, |p| PIECE_VALUES[p.to_index()]);
                let promotion = m.get_promotion().map_or(0, |p| PIECE_VALUES[p.to_index()]);
                let mvv_lva = i64::from(victim * 10 + promotion - attacker);
                if ply.is_none() || see(board, m) >= 0 {
                    ORDER_GOOD_CAPTURE + mvv_lva
                } else {
                    ORDER_BAD_CAPTURE + mvv_lva
                }
            } else if Some(m) == killers[0] {
                ORDER_KILLER
            } else if Some(m) == killers[1] {
                ORDER_KILLER - 1
            } else {
                i64::from(self.history[m.get_source().to_index()][m.get_dest().to_index()])
            };
            std::cmp::Reverse(score)
        });
        moves
    }

    /// Remember a quiet move that caused a cutoff at this ply, most recent first.
    pub(crate) fn store_killer(&mut self, ply: u8, m: ChessMove) {
        if let Some(slot) = self.killers.get_mut(usize::from(ply)) {
            if slot[0] != Some(m) {
                slot[1] = slot[0];
                slot[0] = Some(m);
            }
        }
    }

    /// Quiescence search: resolve the position until nothing forcing is left, so the
    /// evaluation is not read in the middle of an exchange.
    ///
    /// `ply` is the absolute distance from the root and is only used for mate scoring.
    /// `qs_ply` counts plies inside quiescence and bounds it.
    pub(crate) fn quiescence(
        &mut self,
        board: &Board,
        mut alpha: i32,
        beta: i32,
        ply: u8,
        qs_ply: u8,
    ) -> i32 {
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);
        self.stop();
        if self.stopped {
            return 0;
        }

        let in_check = board.checkers() != &chess::EMPTY;

        // The move list doubles as terminal detection, so no separate status() call --
        // which would cost a second full move generation -- is needed.
        let moves = self.ordered(board, None, None);
        if moves.is_empty() {
            return if in_check { -MATE + i32::from(ply) } else { 0 };
        }

        // A capture sequence can strip the board down to a dead position; its static
        // evaluation would still show a material edge that can never become a win.
        if insufficient_material(board) {
            return 0;
        }

        if qs_ply >= MAX_QUIESCENCE_PLY {
            return self.evaluator.evaluate(board);
        }

        // Stand pat: the side to move is not obliged to capture, so the static score is a
        // lower bound -- except in check, where every move must address the check.
        let mut best = if in_check {
            -INF
        } else {
            let stand = self.evaluator.evaluate(board);
            if stand >= beta {
                return stand;
            }
            alpha = alpha.max(stand);
            stand
        };

        let allow_checks = qs_ply < self.limits.qs_check_plies;

        for m in moves {
            if !in_check {
                if is_capture(board, m) || m.get_promotion().is_some() {
                    // Skip captures that static exchange evaluation says lose material.
                    // These are the bulk of quiescence nodes and almost never change the
                    // score, because the opponent simply recaptures.
                    if self.limits.qs_see_pruning && see(board, m) < 0 {
                        continue;
                    }
                } else if allow_checks {
                    if board.make_move_new(m).checkers() == &chess::EMPTY {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            let score = -self.quiescence(&board.make_move_new(m), -beta, -alpha, ply + 1, qs_ply + 1);
            if self.stopped {
                return 0;
            }
            best = best.max(score);
            alpha = alpha.max(best);
            if alpha >= beta {
                break;
            }
        }

        // In check with every evasion pruned away cannot happen (evasions are never
        // pruned), so a -INF best here would be a bug rather than a mate.
        if best == -INF {
            return self.evaluator.evaluate(board);
        }
        best
    }

    pub(crate) fn negamax(&mut self, board: &Board, mut depth: u8, mut alpha: i32, beta: i32, ply: u8) -> i32 {
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);
        if let Some(line) = self.pv.get_mut(usize::from(ply)) {
            line.clear();
        }
        self.stop();
        if self.stopped {
            return 0;
        }
        match board.status() {
            BoardStatus::Checkmate => return -MATE + i32::from(ply),
            BoardStatus::Stalemate => return 0,
            BoardStatus::Ongoing => {}
        }
        // Draw rules come after the mate test: a checkmate delivered on the hundredth
        // halfmove is still checkmate. They also come before the transposition table,
        // because a draw depends on how the position was reached, not only on the position.
        if self.clocks.last().is_some_and(|&c| c >= 100)
            || self.is_repetition(ply)
            || insufficient_material(board)
        {
            return 0;
        }
        // Check extension: a side in check has few legal replies and forcing sequences
        // are where the horizon hides tactics, so search one ply deeper. Capped at twice
        // the iteration depth, so a long run of checks can't extend without bound.
        let in_check = board.checkers() != &chess::EMPTY;
        if self.limits.selective && in_check && ply < self.root_depth.saturating_mul(2) && ply < MAX_DEPTH {
            depth = depth.saturating_add(1);
        }
        if depth == 0 {
            return self.quiescence(board, alpha, beta, ply, 0);
        }
        let key = board.get_hash();
        let tt = self.table.get(key);
        if let Some(entry) = tt.filter(|e| e.depth >= depth) {
            let tt_score = score_from_tt(entry.score, ply);
            match entry.flag {
                Bound::Exact => return tt_score,
                Bound::Lower if tt_score >= beta => return tt_score,
                Bound::Upper if tt_score <= alpha => return tt_score,
                _ => {}
            }
        }
        let pv_node = beta - alpha > 1;
        if self.limits.selective && !pv_node && !in_check {
            let static_eval = self.evaluator.evaluate(board);
            // Reverse futility pruning: this close to the horizon, a position whose static
            // score beats beta by a margin that grows with the remaining depth is very
            // unlikely to drop below beta within that depth. Return without searching.
            // Mate-range betas are excluded, because a static score can't prove a mate.
            if depth <= RFP_MAX_DEPTH
                && beta.abs() < MATE_THRESHOLD
                && static_eval - RFP_MARGIN * i32::from(depth) >= beta
            {
                return static_eval;
            }
            if let Some(cutoff) = self.try_null_move(board, static_eval, depth, beta, ply) {
                return cutoff;
            }
        }

        let original_alpha = alpha;
        let mut best = None;
        let mut score = -INF;
        for (index, m) in self.ordered(board, tt.and_then(|e| e.best), Some(ply)).into_iter().enumerate() {
            let child = board.make_move_new(m);
            // Principal variation search: with good ordering the first move is usually
            // best, so later moves only need to be *refuted*. A null-window scout at
            // (alpha, alpha + 1) is enough to show a move is no better, and only a move
            // that beats alpha is re-searched with the full window to get its true
            // value. Score-preserving: see `shallow_search_score_equals_plain_minimax`.
            let value = if index == 0 {
                self.search_child(board, m, &child, depth - 1, alpha, beta, ply + 1)
            } else {
                // Late move reductions: with good ordering, quiet moves far down the list
                // rarely matter, so they are first scouted at reduced depth. Only a move
                // that beats alpha there earns a full-depth scout. Captures, promotions,
                // checking moves, moves made while in check, and the first few moves are
                // never reduced.
                let reduction = if self.limits.selective
                    && depth >= LMR_MIN_DEPTH
                    && index >= LMR_MIN_INDEX
                    && !in_check
                    && !is_capture(board, m)
                    && m.get_promotion().is_none()
                    && child.checkers() == &chess::EMPTY
                {
                    lmr_reduction(depth, index, pv_node)
                } else {
                    0
                };
                let mut scout = self.search_child(board, m, &child, depth - 1 - reduction, alpha, alpha + 1, ply + 1);
                if reduction > 0 && scout > alpha && !self.stopped {
                    scout = self.search_child(board, m, &child, depth - 1, alpha, alpha + 1, ply + 1);
                }
                if scout > alpha && scout < beta && !self.stopped {
                    self.search_child(board, m, &child, depth - 1, alpha, beta, ply + 1)
                } else {
                    scout
                }
            };
            if self.stopped {
                return 0;
            }
            if value > score {
                score = value;
                best = Some(m);
                if value > alpha {
                    self.update_pv(ply, m);
                }
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                if !is_capture(board, m) && m.get_promotion().is_none() {
                    self.reward_history(m, depth);
                    self.store_killer(ply, m);
                }
                break;
            }
        }
        let flag = if score <= original_alpha {
            Bound::Upper
        } else if score >= beta {
            Bound::Lower
        } else {
            Bound::Exact
        };
        self.table.put(Entry {
            key,
            depth,
            generation: 0, // stamped by Table::put
            score: score_to_tt(score, ply),
            flag,
            best,
        });
        score
    }

    /// Credit a quiet move that caused a beta cutoff.
    ///
    /// Uses the "gravity" update `h += b - h*b/MAX`, which saturates smoothly at
    /// [`HISTORY_MAX`] instead of growing without bound. Only quiet moves are credited:
    /// captures are already ordered by MVV-LVA, and crediting them too lets history
    /// swamp that ordering.
    pub(crate) fn reward_history(&mut self, m: ChessMove, depth: u8) {
        let bonus = (i32::from(depth) * i32::from(depth)).min(HISTORY_MAX);
        let entry = &mut self.history[m.get_source().to_index()][m.get_dest().to_index()];
        *entry += bonus - *entry * bonus / HISTORY_MAX;
    }
}

pub fn best_move(board: &Board, limits: SearchLimits) -> Option<ChessMove> {
    search(board, limits).map(|result| result.best_move)
}

/// Search the root position with principal variation search.
///
/// The first move -- the previous iteration's best move, when there is one -- is searched
/// with the full window, because its score defines the PV. Every later move gets a null
/// window scout and is only re-searched in full if it might beat the current best. The
/// audited baseline scouted the first move too and never re-searched it (§G.3), so on
/// iteration one the PV move's score was a bound against a window of (31999, 32000).
pub(crate) fn search_root(
    board: &Board,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    searcher: &mut Searcher,
    previous_best: Option<ChessMove>,
) -> Option<(ChessMove, i32)> {
    let moves = searcher.ordered(board, previous_best, Some(0));
    let child_depth = depth.saturating_sub(1);
    let mut best = None;
    let mut best_score = -INF;
    for (index, m) in moves.into_iter().enumerate() {
        let child = board.make_move_new(m);
        let value = if index == 0 {
            searcher.search_child(board, m, &child, child_depth, alpha, beta, 1)
        } else {
            let scout = searcher.search_child(board, m, &child, child_depth, alpha, alpha + 1, 1);
            if scout > alpha && scout < beta && !searcher.stopped {
                searcher.search_child(board, m, &child, child_depth, alpha, beta, 1)
            } else {
                scout
            }
        };
        if searcher.stopped {
            break;
        }
        if value > best_score {
            best_score = value;
            best = Some(m);
            searcher.update_pv(0, m);
        }
        alpha = alpha.max(value);
        if alpha >= beta {
            // Fail high against an aspiration window: the caller re-searches with a
            // wider window, so finishing the remaining moves here is wasted work.
            break;
        }
    }
    best.map(|m| (m, best_score))
}

/// Search with a throwaway engine: fresh table, no external stop, no progress reports.
/// Deterministic for fixed depth/nodes, which is what tests and benchmarks need.
pub fn search(board: &Board, limits: SearchLimits) -> Option<SearchResult> {
    Engine::new(limits.hash_mb).search(board, limits, Arc::new(AtomicBool::new(false)), &mut |_| {})
}

/// A long-lived engine. It owns the transposition table across searches, so what it
/// learned thinking about one move is still there for the next. That is how it is used in
/// a real game, via UCI.
pub struct Engine {
    table: Table,
    /// `None`: use the handcrafted evaluation in the style given by `SearchLimits`.
    evaluator: Option<Arc<dyn Evaluator>>,
}

impl Engine {
    pub fn new(hash_mb: usize) -> Self {
        Self {
            table: Table::new(hash_mb),
            evaluator: None,
        }
    }

    /// Search with a custom evaluator instead of the style-selected handcrafted one. This
    /// is the plug-in point for neural or hybrid evaluation.
    pub fn set_evaluator(&mut self, evaluator: Arc<dyn Evaluator>) {
        self.evaluator = Some(evaluator);
    }

    /// Go back to the style-selected handcrafted evaluation.
    pub fn clear_evaluator(&mut self) {
        self.evaluator = None;
    }

    /// Reallocate the table at a new size. Its contents are lost.
    pub fn resize(&mut self, hash_mb: usize) {
        self.table = Table::new(hash_mb);
    }

    /// Forget everything, as for UCI `ucinewgame`.
    pub fn clear(&mut self) {
        self.table.clear();
    }

    /// Iterative-deepening search. `stop` ends it from another thread; `on_info` is called
    /// after every completed iteration. The table size is the engine's own, and
    /// `limits.hash_mb` is ignored here.
    pub fn search(
        &mut self,
        board: &Board,
        limits: SearchLimits,
        stop: Arc<AtomicBool>,
        on_info: &mut dyn FnMut(&SearchInfo),
    ) -> Option<SearchResult> {
        self.search_position(&Position::new(*board), limits, stop, on_info)
    }

    /// Search a position together with its game history, so repetitions and the
    /// fifty-move rule are seen. This is what UCI uses.
    pub fn search_position(
        &mut self,
        root: &Position,
        limits: SearchLimits,
        stop: Arc<AtomicBool>,
        on_info: &mut dyn FnMut(&SearchInfo),
    ) -> Option<SearchResult> {
        let board = &root.board;
        let table = std::mem::replace(&mut self.table, Table::placeholder());
        let evaluator = self
            .evaluator
            .clone()
            .unwrap_or_else(|| Arc::new(StyleEvaluator(limits.style)));
        let mut searcher = Searcher::with_table(limits, table, stop, evaluator);
        searcher.set_root(root);
        let result = iterate(&mut searcher, board, limits, on_info);
        self.table = std::mem::replace(&mut searcher.table, Table::placeholder());
        result
    }
}

pub(crate) fn iterate(
    searcher: &mut Searcher,
    board: &Board,
    limits: SearchLimits,
    on_info: &mut dyn FnMut(&SearchInfo),
) -> Option<SearchResult> {
    let fallback = MoveGen::new_legal(board).next()?;
    let mut result = SearchResult {
        best_move: fallback,
        depth: 0,
        seldepth: 0,
        score: 0,
        nodes: 0,
        pv: vec![fallback],
    };
    searcher.table.new_search();

    // `limits.threads` is accepted but not yet used: the root-splitting parallel search it
    // used to select was measured to be strictly harmful (experiments/E3) and was removed.
    // Lazy SMP over a shared table is the replacement (roadmap item 9).
    for depth in 1..=limits.depth.clamp(1, MAX_DEPTH) {
        let previous_best = (result.depth > 0).then_some(result.best_move);
        let (alpha, beta) = if result.depth > 0 && result.score.abs() < MATE_THRESHOLD {
            (result.score - 40, result.score + 40)
        } else {
            (-INF, INF)
        };
        searcher.root_depth = depth;
        let mut candidate = search_root(board, depth, alpha, beta, searcher, previous_best);
        if !searcher.stopped
            && (alpha, beta) != (-INF, INF)
            && candidate
                .as_ref()
                .is_some_and(|(_, s)| *s <= alpha || *s >= beta)
        {
            candidate = search_root(board, depth, -INF, INF, searcher, previous_best);
        }

        // An interrupted iteration is discarded: its score may be a bound and it has not
        // looked at every move. Depth 1 always runs to completion (see Searcher::can_abort)
        // unless a node budget cuts it, so a searched move is almost always available.
        if searcher.stopped && result.depth > 0 {
            break;
        }
        if let Some((m, score)) = candidate {
            let mut pv = searcher.pv[0].clone();
            if pv.first() != Some(&m) {
                pv = vec![m];
            }
            result = SearchResult {
                best_move: m,
                depth,
                seldepth: searcher.seldepth,
                score,
                nodes: searcher.nodes,
                pv,
            };
            on_info(&SearchInfo {
                depth,
                seldepth: searcher.seldepth,
                score,
                nodes: searcher.nodes,
                elapsed: searcher.start.elapsed(),
                pv: result.pv.clone(),
                hashfull: searcher.table.hashfull(),
            });
        }
        searcher.can_abort = true;
        if searcher.stopped || searcher.stop_flag.load(Ordering::Relaxed) {
            break;
        }
        if let Some(soft) = limits.soft_time {
            // Each iteration costs several times the one before, so once half the soft
            // budget is spent the next iteration would very likely overrun; starting it
            // only burns time that the hard limit then throws away.
            if searcher.start.elapsed() >= soft / 2 {
                break;
            }
        }
    }
    result.nodes = searcher.nodes;
    Some(result)
}
