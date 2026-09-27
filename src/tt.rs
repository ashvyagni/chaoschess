//! Transposition table: key-verified slots, mate-score conversion, generation aging.

use super::*;

#[derive(Clone, Copy)]
pub(crate) struct Entry {
    pub(crate) key: u64,
    pub(crate) depth: u8,
    /// Search generation this entry was written in, used to prefer replacing stale data.
    pub(crate) generation: u8,
    /// Stored root-relative for mates; see [`score_to_tt`].
    pub(crate) score: i32,
    pub(crate) flag: Bound,
    pub(crate) best: Option<ChessMove>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bound {
    Exact,
    Lower,
    Upper,
}

/// Mate scores are "mate in N plies *from here*". The same position reached at a
/// different ply has a different distance to mate from the root, so a mate score has to
/// be converted to "distance from this node" before storing and back on retrieval.
/// Storing it raw makes a mate found at ply 7 look like a mate at ply 3 when probed there.
pub(crate) fn score_to_tt(score: i32, ply: u8) -> i32 {
    if score > MATE_THRESHOLD {
        score + i32::from(ply)
    } else if score < -MATE_THRESHOLD {
        score - i32::from(ply)
    } else {
        score
    }
}

pub(crate) fn score_from_tt(score: i32, ply: u8) -> i32 {
    if score > MATE_THRESHOLD {
        score - i32::from(ply)
    } else if score < -MATE_THRESHOLD {
        score + i32::from(ply)
    } else {
        score
    }
}

pub(crate) struct Table {
    pub(crate) entries: Vec<Option<Entry>>,
    pub(crate) generation: u8,
}

impl Table {
    pub(crate) fn new(mb: usize) -> Self {
        let count = ((mb.max(1) * 1024 * 1024) / std::mem::size_of::<Option<Entry>>()).max(1);
        Self {
            entries: vec![None; count],
            generation: 0,
        }
    }

    pub(crate) fn slot(&self, key: u64) -> usize {
        (key as usize) % self.entries.len()
    }

    /// The entry for exactly this position, if one is stored.
    ///
    /// The key comparison is the whole point: many positions share a slot, and returning
    /// a neighbour's bound as if it were this position's is silent search corruption.
    /// The audited baseline omitted it (MASTER_ENGINE_AUDIT.md §G.2).
    pub(crate) fn get(&self, key: u64) -> Option<Entry> {
        self.entries[self.slot(key)].filter(|entry| entry.key == key)
    }

    /// Replacement policy: always replace an empty slot, a slot holding the same
    /// position, or a slot left over from an earlier search; otherwise keep whichever
    /// entry was searched deeper.
    pub(crate) fn put(&mut self, entry: Entry) {
        let slot = self.slot(entry.key);
        let replace = match self.entries[slot] {
            None => true,
            Some(old) => {
                old.key == entry.key || old.generation != self.generation || entry.depth >= old.depth
            }
        };
        if replace {
            self.entries[slot] = Some(Entry {
                generation: self.generation,
                ..entry
            });
        }
    }

    /// Start a new search. Entries from earlier searches stay usable but become the
    /// first candidates for replacement.
    pub(crate) fn new_search(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// A zero-capacity stand-in, used only while the real table is lent to a search.
    /// Never probed: `slot` would divide by zero.
    pub(crate) fn placeholder() -> Self {
        Self {
            entries: Vec::new(),
            generation: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.iter_mut().for_each(|entry| *entry = None);
        self.generation = 0;
    }

    /// Permille of a fixed sample of slots written by the current search, the usual UCI
    /// `hashfull` estimate.
    pub(crate) fn hashfull(&self) -> u32 {
        let sample = self.entries.len().min(1_000);
        if sample == 0 {
            return 0;
        }
        let used = self.entries[..sample]
            .iter()
            .filter(|e| e.is_some_and(|e| e.generation == self.generation))
            .count();
        (used * 1_000 / sample) as u32
    }
}
