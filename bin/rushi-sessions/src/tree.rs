//! The tree depth of the event log: the TUI tree model's depth
//! solver (the `rushi-tui` `SessionTree`/`DepthSolver` port, the
//! `tree` floating palette view) for the flat list of the television
//! channel.
//!
//! The depth is derived from the `rewind` markers with the TUI's
//! display rules (its design doc `tree-ui-design-from-human.md`,
//! "Tree indent"):
//!
//! - A rewind branch is rooted at the marker's `target_seq`, the
//!   rewound event. The branch block is the marker row and the events
//!   after it, up to the branch's close.
//! - A marker forks when its abandoned tail (the rows between the
//!   target and the marker) holds no marker row: its branch block
//!   sits one level below the target's row, and the abandoned tail
//!   keeps its own depth.
//! - A marker that abandons a complete nested branch re-enters the
//!   target's branch: its branch block keeps the target's level.
//! - A branch closes at the first later marker that rewinds back to
//!   its target or earlier: the block stops one seq before it.
//! - The depth of a row is the deepest covering branch level, 0 on
//!   the trunk. A marker row renders at the depth of the branch it
//!   heads.
//!
//! Malformed markers are projected out by the kernel's shared parser,
//! the same way the TUI projects them.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use rushi_common::rewind::{parse_rewind_event, RewindRef};

/// One parsed `rewind` marker, with the display-depth facts derived
/// from it (the TUI tree model's `Marker`).
struct Marker {
    /// The marker's own 1-based log seq: the first seq of its span.
    seq: usize,
    /// The rewound event the branch is rooted at.
    target: usize,
    /// A fork when the abandoned tail holds no marker row.
    fork: bool,
    /// The last in-log seq this marker's span covers: the seq before
    /// the first later marker that rewinds back to this marker's
    /// target or earlier, or the log's last seq when none does.
    close: usize,
}

/// The parsed markers of one numbered log, in log order.
fn markers(rows: &[(u64, Value)]) -> Vec<Marker> {
    let refs: Vec<RewindRef> = rows
        .iter()
        .filter_map(|(s, v)| parse_rewind_event(v, *s as usize))
        .collect();
    let marker_seqs: HashSet<usize> = refs.iter().map(|r| r.seq).collect();
    let last = rows.last().map(|(s, _)| *s as usize).unwrap_or(0);
    refs.iter()
        .map(|r| {
            let seq = r.seq;
            let target = r.target;
            let fork = (target + 1..seq).all(|s| !marker_seqs.contains(&s));
            let close = refs
                .iter()
                .find(|m| m.seq > seq && m.target <= target)
                .map(|m| m.seq - 1)
                .unwrap_or(last);
            Marker {
                seq,
                target,
                fork,
                close,
            }
        })
        .collect()
}

/// The depth of the row at `seq`: the deepest level among the marker
/// spans that cover it, 0 on the trunk. The recursion is strictly
/// decreasing in log seq (a span covers its rows only past its
/// target), so every walk ends at the trunk.
fn depth_at(seq: usize, markers: &[Marker], memo: &mut HashMap<usize, usize>) -> usize {
    if let Some(d) = memo.get(&seq) {
        return *d;
    }
    let mut d = 0;
    for m in markers {
        if m.seq > seq || seq > m.close {
            continue;
        }
        let lvl = depth_at(m.target, markers, memo) + usize::from(m.fork);
        d = d.max(lvl);
    }
    memo.insert(seq, d);
    d
}

/// The tree depth (0 = trunk) of every row of `rows`, in row order.
/// Rows are the numbered events of the whole log, so every marker
/// target is in range (the kernel parser rejects a target at or past
/// the marker itself).
pub(crate) fn tree_depths(rows: &[(u64, Value)]) -> Vec<usize> {
    let markers = markers(rows);
    let mut memo: HashMap<usize, usize> = HashMap::new();
    rows.iter()
        .map(|(seq, _)| depth_at(*seq as usize, &markers, &mut memo))
        .collect()
}

/// The box-drawing indent of a depth-`d` row: three spaces per level
/// shallower than the parent, the `└─` branch marker at the parent
/// level, one trailing space. Depth 0 carries no marker (the top of
/// the active branch stays left aligned). This is the TUI spec glyph
/// set: 3-space indent, the `└─` branch, no vertical continuation
/// lines.
pub(crate) fn indent_for(d: usize) -> String {
    if d == 0 {
        String::new()
    } else {
        format!("{}└─ ", "   ".repeat(d - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(t: &str) -> Value {
        serde_json::json!({"type": t})
    }

    fn rewind(target: u64) -> Value {
        serde_json::json!({"type": "rewind", "target_seq": target, "mode": "on"})
    }

    #[test]
    fn no_markers_is_all_trunk() {
        let rows: Vec<(u64, Value)> = vec![(1, v("user_message")), (2, v("assistant_message"))];
        assert_eq!(tree_depths(&rows), vec![0, 0]);
    }

    #[test]
    fn a_fork_marker_indents_its_branch() {
        // Seq 4 rewinds to seq 2: the fork at 4 covers 4..end at
        // level 1 (the tail row 3 keeps the trunk depth).
        let rows: Vec<(u64, Value)> = vec![
            (1, v("user_message")),
            (2, v("assistant_message")),
            (3, v("tool_call")),
            (4, rewind(2)),
            (5, v("user_message")),
            (6, v("assistant_message")),
        ];
        assert_eq!(tree_depths(&rows), vec![0, 0, 0, 1, 1, 1]);
    }

    #[test]
    fn a_reentry_marker_keeps_the_target_level() {
        // Seq 4 rewinds to 2: the fork at 4 covers 4..6 at level 1.
        // Seq 7 rewinds back to 2, and its abandoned tail (3..7)
        // holds the marker at 4: it abandons a complete nested
        // branch, so it re-enters the target's branch and 7..8 keeps
        // the trunk level (the branch's own depth, 0).
        let rows: Vec<(u64, Value)> = vec![
            (1, v("user_message")),
            (2, v("assistant_message")),
            (3, v("tool_call")),
            (4, rewind(2)),
            (5, v("user_message")),
            (6, v("assistant_message")),
            (7, rewind(2)),
            (8, v("user_message")),
        ];
        assert_eq!(tree_depths(&rows), vec![0, 0, 0, 1, 1, 1, 0, 0]);
    }

    #[test]
    fn nested_forks_stack_the_depths() {
        // A three-rewind chain (the kernel's deep-chain test): 3->2
        // forks B at 4, 6->4 forks C at 7, 9->7 forks D at 10.
        // Each marker's tail holds no marker row, so every one
        // forks: B rows 3..5 are level 1, C rows 6..8 are level 2,
        // D rows 9..10 are level 3.
        let rows: Vec<(u64, Value)> = (1..=10)
            .map(|s| {
                if matches!(s, 3 | 6 | 9) {
                    (
                        s,
                        rewind(match s {
                            3 => 2,
                            6 => 4,
                            _ => 7,
                        }),
                    )
                } else {
                    (s, v("user_message"))
                }
            })
            .collect();
        assert_eq!(tree_depths(&rows), vec![0, 0, 1, 1, 1, 2, 2, 2, 3, 3]);
    }

    #[test]
    fn a_branch_closes_at_the_later_earlier_rewind() {
        // Seq 4 rewinds to 2: the fork at 4 is level 1. Seq 8 rewinds
        // to 1, and its tail (2..8) holds the marker at 4: it
        // abandons the whole first branch, so it re-enters the trunk.
        // The first branch closes at 7; the 8..9 block sits at the
        // trunk level (0).
        let rows: Vec<(u64, Value)> = vec![
            (1, v("user_message")),
            (2, v("assistant_message")),
            (3, v("tool_call")),
            (4, rewind(2)),
            (5, v("user_message")),
            (6, v("assistant_message")),
            (7, v("tool_call")),
            (8, rewind(1)),
            (9, v("user_message")),
        ];
        assert_eq!(tree_depths(&rows), vec![0, 0, 0, 1, 1, 1, 1, 0, 0]);
    }

    #[test]
    fn malformed_markers_are_projected_out() {
        // A marker without a target_seq is skipped, like the kernel
        // skips it. The log is all trunk.
        let rows: Vec<(u64, Value)> = vec![
            (1, v("user_message")),
            (2, serde_json::json!({"type": "rewind", "mode": "on"})),
            (3, v("assistant_message")),
        ];
        assert_eq!(tree_depths(&rows), vec![0, 0, 0]);
    }

    #[test]
    fn a_future_target_is_rejected() {
        // target_seq >= the marker's own seq is not a rewind.
        let rows: Vec<(u64, Value)> = vec![
            (1, v("user_message")),
            (
                2,
                serde_json::json!({"type": "rewind", "target_seq": 3, "mode": "on"}),
            ),
        ];
        assert_eq!(tree_depths(&rows), vec![0, 0]);
    }

    #[test]
    fn indent_glyphs_match_the_tui_spec() {
        assert_eq!(indent_for(0), "");
        assert_eq!(indent_for(1), "└─ ");
        assert_eq!(indent_for(2), "   └─ ");
        assert_eq!(indent_for(3), "      └─ ");
    }
}
