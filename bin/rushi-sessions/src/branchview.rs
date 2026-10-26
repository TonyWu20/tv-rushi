//! The collapsed branch view of one numbered log: the active branch
//! plus the history before the diverging node. Inactive spans fold
//! onto the row that hides them.
//!
//! A view is the pair `(F, X)`: `F` the focus block (the parent
//! branch, or the trunk) and `X` the cutoff seq. The default view
//! (no focus) takes `X` at the log's last seq and `F` at the block
//! that contains it. A focused view (the `branch_view` action) takes
//! `X` at the picked seq and `F` at the block that contains the
//! pick's branch point: a marker pick points at its target, a
//! message pick at itself.
//!
//! The shown rows are:
//!
//! - the trunk rows down to the first block's branch point, or the
//!   cutoff when the trunk is the focus;
//! - every block marker on the chain from the trunk to `F`;
//! - the focus block's interior, up to the cutoff;
//! - the picked marker row, when the pick is a marker;
//! - the picked block's child markers past the cutoff, each with its
//!   subtree folded (the `branched out to` lines).
//!
//! Every other seq is hidden and folds onto one shown row: its
//! deepest shown header. A header's region is: its branch span for a
//! shown marker row; the focus structure's tail past the cutoff for
//! the cutoff row; the union of each marker's abandoned gap and span
//! for a branch-point row. The fold count of a row is the number of
//! hidden seqs that fold onto it. The `--chat` filter keeps the
//! shown user and assistant messages and the markers.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use crate::events::event_summary;
use crate::format::{short_tag, ts_local};
use crate::tree::{indent_for, markers, tree_depths, Marker};

/// One TSV row of the collapsed view, oldest first; the caller
/// prints reversed, newest first, like the flat list.
pub(crate) fn view_rows(rows: &[(u64, Value)], focus: Option<u64>, chat: bool) -> Vec<String> {
    let model = Model::new(rows, focus, chat);
    model.render()
}

/// The view facts of one numbered log, derived once per run.
struct Model {
    rows: Vec<(u64, Value)>,
    markers: Vec<Marker>,
    /// The seq-to-depth map over the whole log (the tree view's
    /// depth solver, per row seq).
    depths: HashMap<usize, usize>,
    last: usize,
    /// The cutoff seq: the picked seq, or the log's last seq.
    x: usize,
    /// The picked row's marker, when the pick is a well-formed
    /// rewind marker.
    x_marker: Option<usize>,
    /// The focus block, `None` for the trunk.
    focus_block: Option<usize>,
    /// The seqs that exist in the log: the unparseable lines of a
    /// torn or mixed log are absent, so the shown and hidden sets
    /// stay inside them.
    present: BTreeSet<usize>,
    /// The shown rows.
    shown: BTreeSet<usize>,
    /// The fold count per shown row: the hidden seqs that fold onto
    /// it.
    fold: HashMap<usize, u64>,
    chat: bool,
}

impl Model {
    fn new(rows: &[(u64, Value)], focus: Option<u64>, chat: bool) -> Self {
        let markers = markers(rows);
        let depths = rows
            .iter()
            .zip(&tree_depths(rows))
            .map(|((s, _), d)| (*s as usize, *d))
            .collect();
        let last = rows.last().map(|(s, _)| *s as usize).unwrap_or(0);
        // The caller validates the focus against the log length.
        let x = focus.map(|f| f as usize).unwrap_or(last);
        let x_marker = markers.iter().find(|m| m.seq == x).map(|m| m.seq);
        // The pick's branch point: a marker pick points at its
        // target, a message pick at itself. The focus block is the
        // deepest block that contains the branch point, the trunk
        // when none does.
        let anchor = match x_marker {
            Some(mx) => markers.iter().find(|m| m.seq == mx).unwrap().target,
            None => x,
        };
        let focus_block = markers
            .iter()
            .filter(|m| m.seq <= anchor && anchor <= m.close)
            .max_by_key(|m| m.seq)
            .map(|m| m.seq);
        let mut m = Model {
            rows: rows.to_vec(),
            markers,
            depths,
            last,
            x,
            x_marker,
            focus_block,
            present: rows.iter().map(|(s, _)| *s as usize).collect(),
            shown: BTreeSet::new(),
            fold: HashMap::new(),
            chat,
        };
        m.compute_shown();
        m.attribute_folds();
        m
    }

    fn marker_at(&self, seq: usize) -> Option<&Marker> {
        self.markers.iter().find(|m| m.seq == seq)
    }

    /// The deepest block that contains seq `s`, `None` on the trunk.
    fn block_containing(&self, s: usize) -> Option<usize> {
        self.markers
            .iter()
            .filter(|m| m.seq <= s && s <= m.close)
            .max_by_key(|m| m.seq)
            .map(|m| m.seq)
    }

    fn compute_shown(&mut self) {
        // The ancestor chain from the trunk to the focus block.
        let mut chain: Vec<usize> = Vec::new();
        let mut cur = self.focus_block;
        while let Some(b) = cur {
            chain.push(b);
            let target = self.markers.iter().find(|m| m.seq == b).unwrap().target;
            cur = self.block_containing(target);
        }
        chain.reverse();
        let chain_head = chain.first().copied();
        // The trunk rows, down to the first block's branch point (or
        // the cutoff when the trunk is the focus).
        let trunk_end = match chain_head {
            Some(b) => {
                let eff = self.markers.iter().find(|m| m.seq == b).unwrap().eff();
                eff.min(self.x)
            }
            None => self.x,
        };
        for s in 1..=trunk_end {
            self.shown.insert(s);
        }
        for b in &chain {
            self.shown.insert(*b);
        }
        // The focus block's interior, up to the cutoff.
        if let Some(f) = self.focus_block {
            let close = self.markers.iter().find(|m| m.seq == f).unwrap().close;
            for s in f + 1..=self.x.min(close) {
                self.shown.insert(s);
            }
        }
        // The picked marker row itself.
        if let Some(xm) = self.x_marker {
            self.shown.insert(xm);
        }
        // The picked block's child markers past the cutoff: the
        // `branched out to` lines, each with its subtree folded.
        let out_block = self.x_marker.or(self.focus_block);
        if let Some(b) = out_block {
            let bm = *self.marker_at(b).unwrap();
            for m in &self.markers {
                if m.seq != bm.seq && self.is_direct_child(m, &bm) && m.seq > self.x {
                    self.shown.insert(m.seq);
                }
            }
        }
        // Keep the shown set inside the log: a seq of an
        // unparseable line was never a row.
        self.shown.retain(|s| self.present.contains(s));
    }

    /// A direct child of block `b`: one level below it, targeting a
    /// row of its span. A re-entry marker (one that abandons a
    /// complete nested branch) keeps the target's level, so it is
    /// not a child, matching the depth solver.
    fn is_direct_child(&self, c: &Marker, b: &Marker) -> bool {
        c.seq > b.seq
            && c.target >= b.seq
            && c.target <= b.close
            && self.depths[&c.seq] == self.depths[&b.seq] + 1
    }

    /// Whether the shown row `r`'s fold region covers the hidden seq
    /// `h`. The regions: the branch span of a shown marker row; the
    /// focus structure's tail past the cutoff for the cutoff row;
    /// the abandoned gap plus span of each marker that branches
    /// from `r` for a branch-point row.
    fn region_covers(&self, r: usize, h: usize) -> bool {
        if let Some(m) = self.marker_at(r) {
            // A marker row heads its whole branch span, the cutoff
            // marker row included: the span covers the tail up to its
            // own close.
            return m.seq <= h && h <= m.close;
        }
        if r == self.x && self.x < self.last {
            return self.tail_covers(h);
        }
        for m in &self.markers {
            if m.eff() == r && m.eff() + 1 <= h && h <= m.close {
                return true;
            }
        }
        false
    }

    /// The cutoff row's tail region: the focus structure's seqs past
    /// the cutoff, the trunk's tail on the trunk. A marker cutoff
    /// row is handled by its own branch span instead.
    fn tail_covers(&self, h: usize) -> bool {
        if self.x_marker.is_some() {
            return false;
        }
        let lo = self.x + 1;
        if lo > h {
            return false;
        }
        let hi = match self.focus_block {
            Some(f) => self.markers.iter().find(|m| m.seq == f).unwrap().close,
            None => self.trunk_end(self.last),
        };
        h <= hi
    }

    fn trunk_end(&self, bound: usize) -> usize {
        self.markers
            .first()
            .map(|m| (m.seq - 1).min(bound))
            .unwrap_or(bound)
    }

    /// Attach every hidden seq to its deepest shown header, and
    /// count the folds per header. A hidden seq with no covering
    /// header attaches to the nearest shown row below it, so the
    /// count stays visible when the `--chat` filter drops the
    /// header row.
    fn attribute_folds(&mut self) {
        let shown: Vec<usize> = self.shown.iter().copied().collect();
        for h in 1..=self.last {
            if shown.contains(&h) || !self.present.contains(&h) {
                continue;
            }
            let header = shown
                .iter()
                .rev()
                .find(|r| self.region_covers(**r, h))
                .or_else(|| shown.iter().rev().find(|r| **r < h));
            if let Some(hdr) = header {
                *self.fold.entry(*hdr).or_insert(0) += 1;
            }
        }
    }

    /// The one-line summary of the shown row: the fold prefix, the
    /// base summary, and the branch annotations. The annotations use
    /// the 0-based item numbers of the list (the user's `#N`): a
    /// 1-based log seq prints as `#(seq - 1)`.
    fn summary_of(&self, r: usize) -> String {
        let ev = self.event_at(r);
        let base = event_summary(ev);
        let mut ann: Vec<String> = Vec::new();
        if let Some(m) = self.marker_at(r) {
            if m.eff() > 0 {
                ann.push(format!("branched from #{}", m.eff() - 1));
            }
            let kids: Vec<String> = self
                .markers
                .iter()
                .filter(|c| c.seq != m.seq && self.is_direct_child(c, m) && self.shown.contains(&c.seq))
                .map(|c| format!("#{}", c.seq - 1))
                .collect();
            if !kids.is_empty() {
                ann.push(format!("branched out to {}", kids.join(" ")));
            }
            let hidden = m.seq - m.eff() - 1;
            if hidden > 0 {
                ann.push(format!("{hidden} hidden"));
            }
        } else {
            let kids: Vec<String> = self
                .markers
                .iter()
                .filter(|c| c.eff() == r && self.shown.contains(&c.seq))
                .map(|c| format!("#{}", c.seq - 1))
                .collect();
            if !kids.is_empty() {
                ann.push(format!("branched out to {}", kids.join(" ")));
            }
        }
        let foldn = self.fold.get(&r).copied().unwrap_or(0);
        let mut s = String::new();
        if foldn > 0 {
            s.push_str(&format!("(Folded {foldn}) "));
        }
        s.push_str(&base);
        if !ann.is_empty() {
            s.push_str(&format!(" ({})", ann.join(", ")));
        }
        s
    }

    /// The event value of the shown row at seq `r`.
    fn event_at(&self, r: usize) -> &Value {
        &self.rows
            .iter()
            .find(|(s, _)| *s as usize == r)
            .unwrap()
            .1
    }

    fn render(&self) -> Vec<String> {
        let shown: Vec<usize> = self.shown.iter().copied().collect();
        shown
            .iter()
            .filter(|r| {
                let ty = self
                    .event_at(**r)
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                matches!(ty, "user_message" | "assistant_message" | "rewind") || !self.chat
            })
            .map(|r| {
                let ty = self
                    .event_at(*r)
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let ev = self.event_at(*r);
                let ts = ev.get("ts").map(ts_local).unwrap_or_default();
                let tag = short_tag(ty);
                let foldn = self.fold.get(r).copied().unwrap_or(0);
                let depth = self.depths.get(r).copied().unwrap_or(0);
                let indent = indent_for(depth, foldn > 0);
                let summary = self.summary_of(*r);
                format!("{r}\t{tag}\t{ts}\t{summary}\t{indent}")
            })
            .map(|row| row)
            .collect()
    }
}
