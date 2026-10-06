//! Line diff for wiki revisions.
//!
//! Deliberately bounded: a wiki body can be 200k characters, and the classic LCS
//! table is quadratic. We trim the common prefix/suffix first — which is where most
//! of a typical edit lives, unchanged — and only then run the DP, over the middle
//! and only up to [`MAX_DP_CELLS`]. Anything larger degrades to "these lines went
//! away, those lines arrived", which is still a truthful diff, just coarser.

use serde::Serialize;

/// Upper bound on the LCS table (cells). 1M cells × 4 bytes = 4MB, the largest
/// allocation we are willing to make while serving a request.
const MAX_DP_CELLS: usize = 1_000_000;

/// Refuse to diff documents bigger than this, rather than returning a response
/// measured in megabytes.
pub const MAX_LINES: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Same,
    Add,
    Remove,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    pub kind: Kind,
    pub text: String,
    /// 1-based line number in the "from" revision, when the line exists there.
    pub old_no: Option<usize>,
    /// 1-based line number in the "to" revision, when the line exists there.
    pub new_no: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diff {
    pub from: i64,
    pub to: i64,
    pub lines: Vec<DiffLine>,
    /// True when the quadratic pass was skipped, so a caller can say "diff too
    /// large, showing wholesale replacement" instead of implying precision.
    pub coarse: bool,
    pub added: usize,
    pub removed: usize,
}

/// Compute the diff between two bodies. `Err` carries a message for the client.
pub fn diff(from: &str, to: &str, from_rev: i64, to_rev: i64) -> Result<Diff, String> {
    let old: Vec<&str> = from.lines().collect();
    let new: Vec<&str> = to.lines().collect();
    if old.len() > MAX_LINES || new.len() > MAX_LINES {
        return Err(format!(
            "revision is too large to diff (limit {MAX_LINES} lines per side)"
        ));
    }

    // Common prefix.
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    // Common suffix, without overlapping the prefix.
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];

    let mut lines: Vec<DiffLine> =
        Vec::with_capacity(prefix + suffix + old_mid.len() + new_mid.len());
    for (i, text) in old[..prefix].iter().enumerate() {
        lines.push(DiffLine {
            kind: Kind::Same,
            text: (*text).to_string(),
            old_no: Some(i + 1),
            new_no: Some(i + 1),
        });
    }

    let mid_cells = old_mid.len().saturating_mul(new_mid.len());
    let coarse = mid_cells > MAX_DP_CELLS;
    let mut old_no = prefix + 1;
    let mut new_no = prefix + 1;

    if coarse {
        // Honest fallback: report the whole middle as replaced.
        for text in old_mid {
            lines.push(DiffLine {
                kind: Kind::Remove,
                text: (*text).to_string(),
                old_no: Some(old_no),
                new_no: None,
            });
            old_no += 1;
        }
        for text in new_mid {
            lines.push(DiffLine {
                kind: Kind::Add,
                text: (*text).to_string(),
                old_no: None,
                new_no: Some(new_no),
            });
            new_no += 1;
        }
    } else {
        let mut mid = lcs_diff(old_mid, new_mid);
        for line in &mut mid {
            match line.kind {
                Kind::Remove => {
                    line.old_no = Some(old_no);
                    old_no += 1;
                }
                Kind::Add => {
                    line.new_no = Some(new_no);
                    new_no += 1;
                }
                Kind::Same => {
                    line.old_no = Some(old_no);
                    line.new_no = Some(new_no);
                    old_no += 1;
                    new_no += 1;
                }
            }
        }
        lines.append(&mut mid);
    }

    let tail_old_start = old.len() - suffix;
    for (offset, text) in old[tail_old_start..].iter().enumerate() {
        lines.push(DiffLine {
            kind: Kind::Same,
            text: (*text).to_string(),
            old_no: Some(tail_old_start + offset + 1),
            new_no: Some(new.len() - suffix + offset + 1),
        });
    }

    let added = lines.iter().filter(|l| l.kind == Kind::Add).count();
    let removed = lines.iter().filter(|l| l.kind == Kind::Remove).count();

    Ok(Diff {
        from: from_rev,
        to: to_rev,
        lines,
        coarse,
        added,
        removed,
    })
}

/// Classic LCS-driven diff over the two middles.
fn lcs_diff(old: &[&str], new: &[&str]) -> Vec<DiffLine> {
    let n = old.len();
    let m = new.len();
    // table[i][j] = LCS length of old[i..] and new[j..]
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if old[i] == new[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }

    let mut out = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old[i] == new[j] {
            out.push(DiffLine {
                kind: Kind::Same,
                text: old[i].to_string(),
                old_no: None,
                new_no: None,
            });
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            out.push(DiffLine {
                kind: Kind::Remove,
                text: old[i].to_string(),
                old_no: None,
                new_no: None,
            });
            i += 1;
        } else {
            out.push(DiffLine {
                kind: Kind::Add,
                text: new[j].to_string(),
                old_no: None,
                new_no: None,
            });
            j += 1;
        }
    }
    while i < n {
        out.push(DiffLine {
            kind: Kind::Remove,
            text: old[i].to_string(),
            old_no: None,
            new_no: None,
        });
        i += 1;
    }
    while j < m {
        out.push(DiffLine {
            kind: Kind::Add,
            text: new[j].to_string(),
            old_no: None,
            new_no: None,
        });
        j += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property that makes a diff trustworthy: applying it to `from` yields
    /// `to`, and dropping the additions yields `from` again.
    fn assert_round_trip(from: &str, to: &str) {
        let d = diff(from, to, 1, 2).expect("diff");
        let rebuilt_to: Vec<&str> = d
            .lines
            .iter()
            .filter(|l| l.kind != Kind::Remove)
            .map(|l| l.text.as_str())
            .collect();
        let rebuilt_from: Vec<&str> = d
            .lines
            .iter()
            .filter(|l| l.kind != Kind::Add)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(
            rebuilt_to.join("\n"),
            to.lines().collect::<Vec<_>>().join("\n"),
            "applying the diff did not reproduce `to` for {from:?} -> {to:?}"
        );
        assert_eq!(
            rebuilt_from.join("\n"),
            from.lines().collect::<Vec<_>>().join("\n"),
            "dropping additions did not reproduce `from` for {from:?} -> {to:?}"
        );
    }

    #[test]
    fn identical_bodies_produce_only_context() {
        let d = diff("a\nb\nc", "a\nb\nc", 1, 2).unwrap();
        assert!(d.lines.iter().all(|l| l.kind == Kind::Same));
        assert_eq!((d.added, d.removed), (0, 0));
        assert!(!d.coarse);
        assert_eq!(d.lines[1].old_no, Some(2));
        assert_eq!(d.lines[1].new_no, Some(2));
    }

    #[test]
    fn insertions_deletions_and_changes_are_classified() {
        assert_round_trip("a\nb", "a\nnew\nb");
        assert_round_trip("a\nb\nc", "a\nc");
        assert_round_trip("a\nb\nc", "a\nB\nc");
        assert_round_trip("", "a\nb");
        assert_round_trip("a\nb", "");
        assert_round_trip("one\ntwo\nthree\nfour", "one\n2\nthree\nfour\nfive");

        let d = diff("a\nb", "a\nnew\nb", 1, 2).unwrap();
        assert_eq!(d.added, 1);
        assert_eq!(d.removed, 0);
        let added = d.lines.iter().find(|l| l.kind == Kind::Add).unwrap();
        assert_eq!(added.text, "new");
        assert_eq!(added.old_no, None);
        assert_eq!(added.new_no, Some(2));
        // The line after the insertion keeps its old number.
        let tail = d.lines.last().unwrap();
        assert_eq!((tail.old_no, tail.new_no), (Some(2), Some(3)));
    }

    #[test]
    fn numbering_stays_consistent_across_a_whole_document() {
        let from = "1\n2\n3\n4\n5\n6";
        let to = "1\n2\nX\n4\n5\n6\n7";
        let d = diff(from, to, 3, 4).unwrap();
        assert_eq!((d.from, d.to), (3, 4));
        // Every old line number appears at most once, and in order.
        let old_nos: Vec<usize> = d.lines.iter().filter_map(|l| l.old_no).collect();
        assert_eq!(old_nos, (1..=6).collect::<Vec<_>>());
        let new_nos: Vec<usize> = d.lines.iter().filter_map(|l| l.new_no).collect();
        assert_eq!(new_nos, (1..=7).collect::<Vec<_>>());
    }

    #[test]
    fn a_huge_middle_degrades_to_a_coarse_but_truthful_diff() {
        // Two bodies with nothing in common and more cells than the DP allows.
        let from: String = (0..1200).map(|i| format!("old {i}\n")).collect();
        let to: String = (0..1200).map(|i| format!("new {i}\n")).collect();
        let d = diff(&from, &to, 1, 2).unwrap();
        assert!(d.coarse, "expected the quadratic pass to be skipped");
        assert_eq!((d.added, d.removed), (1200, 1200));
        assert_round_trip(&from, &to);
    }

    #[test]
    fn documents_beyond_the_line_limit_are_refused() {
        let big: String = (0..MAX_LINES + 1).map(|i| format!("{i}\n")).collect();
        assert!(diff(&big, "a", 1, 2).is_err());
    }
}
