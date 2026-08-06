//! Diff preview rendering as plain ANSI strings (port of
//! `apps/dimi/src/tui/components/media/diff-preview.ts`).
//!
//! Provides the LCS-based line diff (`compute_diff_lines`), the simple
//! changed-lines renderer (`render_diff_lines`), and the context-clustered
//! renderer used by Edit's call preview / approval panels
//! (`render_diff_lines_clustered`). All colours go through
//! [`crate::theme::Theme`]; the strong header counts use `bold_hex` because the
//! TS source calls `chalk.bold.hex(...)` (bold opens before the colour).

use crate::theme::{ColorToken, Theme, current_theme};

/// Diff line kind (`DiffLineKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Add,
    Delete,
}

/// One diff line (`DiffLine`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    /// Source line number: `newStart + j - 1` for context/add lines,
    /// `oldStart + i - 1` for delete lines.
    pub line_num: usize,
    pub code: String,
}

/// Compute the diff between two texts as a sequence of [`DiffLine`]s using an
/// LCS DP (port of `computeDiffLines`).
///
/// While the text is still streaming (`is_incomplete`), trailing delete lines
/// are suppressed — they are likely artefacts of `newText` not having arrived
/// yet rather than genuine deletions.
pub fn compute_diff_lines(
    old_text: &str,
    new_text: &str,
    old_start: usize,
    new_start: usize,
    is_incomplete: bool,
) -> Vec<DiffLine> {
    // `oldText ? oldText.split('\n') : []` — an empty string yields no lines.
    let old_lines: Vec<&str> = if old_text.is_empty() {
        Vec::new()
    } else {
        old_text.split('\n').collect()
    };
    let new_lines: Vec<&str> = if new_text.is_empty() {
        Vec::new()
    } else {
        new_text.split('\n').collect()
    };
    let m = old_lines.len();
    let n = new_lines.len();

    // LCS DP table, `dp[i][j]` = LCS length of prefixes `old[..i]`/`new[..j]`.
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old_lines[i - 1] == new_lines[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack from `(m, n)` — collects lines in reverse order.
    let mut reversed: Vec<DiffLine> = Vec::new();
    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_lines[i - 1] == new_lines[j - 1] {
            reversed.push(DiffLine {
                kind: DiffLineKind::Context,
                line_num: new_start + j - 1,
                code: new_lines[j - 1].to_owned(),
            });
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            reversed.push(DiffLine {
                kind: DiffLineKind::Add,
                line_num: new_start + j - 1,
                code: new_lines[j - 1].to_owned(),
            });
            j -= 1;
        } else {
            reversed.push(DiffLine {
                kind: DiffLineKind::Delete,
                line_num: old_start + i - 1,
                code: old_lines[i - 1].to_owned(),
            });
            i -= 1;
        }
    }
    let mut result: Vec<DiffLine> = reversed.into_iter().rev().collect();

    // Suppress trailing deletes while streaming.
    if is_incomplete && !result.is_empty() {
        let mut last_non_delete: isize = (result.len() - 1) as isize;
        while last_non_delete >= 0 && result[last_non_delete as usize].kind == DiffLineKind::Delete
        {
            last_non_delete -= 1;
        }
        if last_non_delete >= 0 {
            result.truncate((last_non_delete + 1) as usize);
        } else {
            // Every line would be shown as deleted; suppress them all so the
            // UI doesn't flash a wall of red before newText starts arriving.
            result.clear();
        }
    }

    result
}

/// Styles used by the diff renderers (`DiffStyles`), built from the current
/// theme each call — mirrors `makeDiffStyles()`.
struct DiffStyles {
    theme: Theme,
}

impl DiffStyles {
    fn new() -> Self {
        DiffStyles {
            theme: current_theme(),
        }
    }

    fn add(&self, s: &str) -> String {
        self.theme.fg(ColorToken::DiffAdded, s)
    }
    fn del(&self, s: &str) -> String {
        self.theme.fg(ColorToken::DiffRemoved, s)
    }
    /// `chalk.bold.hex(diffAddedStrong)` — bold opens *before* the colour.
    fn add_bold(&self, s: &str) -> String {
        self.theme.bold_hex(ColorToken::DiffAddedStrong, s)
    }
    /// `chalk.bold.hex(diffRemovedStrong)` — bold opens *before* the colour.
    fn del_bold(&self, s: &str) -> String {
        self.theme.bold_hex(ColorToken::DiffRemovedStrong, s)
    }
    fn gutter(&self, s: &str) -> String {
        self.theme.fg(ColorToken::DiffGutter, s)
    }
    fn meta(&self, s: &str) -> String {
        self.theme.fg(ColorToken::DiffMeta, s)
    }
}

/// Render a compact diff preview: a `+N -M path` header (only the counts that
/// are non-zero) followed by the changed lines, with an optional truncation
/// footer (port of `renderDiffLines`).
///
/// `max_lines` caps the number of changed lines shown (a `… N more changes
/// hidden (ctrl+o to expand)` footer is appended when lines are dropped).
pub fn render_diff_lines(
    old_text: &str,
    new_text: &str,
    path: &str,
    is_incomplete: bool,
    old_start: usize,
    new_start: usize,
    max_lines: Option<usize>,
) -> Vec<String> {
    let s = DiffStyles::new();
    let diff_lines = compute_diff_lines(old_text, new_text, old_start, new_start, is_incomplete);
    let changed_lines: Vec<&DiffLine> = diff_lines
        .iter()
        .filter(|l| l.kind != DiffLineKind::Context)
        .collect();
    let added = changed_lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Add)
        .count();
    let removed = changed_lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Delete)
        .count();

    let mut output: Vec<String> = Vec::new();

    let mut header = String::new();
    if added > 0 {
        header.push_str(&s.add_bold(&format!("+{added} ")));
    }
    if removed > 0 {
        header.push_str(&s.del_bold(&format!("-{removed} ")));
    }
    header.push_str(path);
    output.push(header);

    let changed_total = changed_lines.len();
    let shown: Vec<&DiffLine> = match max_lines {
        Some(max) if changed_total > max => changed_lines[..max].to_vec(),
        _ => changed_lines,
    };

    for line in &shown {
        let marker = if line.kind == DiffLineKind::Add {
            "+"
        } else {
            "-"
        };
        let colored = if line.kind == DiffLineKind::Add {
            s.add(&format!("{marker} {}", line.code))
        } else {
            s.del(&format!("{marker} {}", line.code))
        };
        output.push(format!(
            "{}{}",
            s.gutter(&format!("{:>4} ", line.line_num)),
            colored
        ));
    }

    let hidden = changed_total - shown.len();
    if hidden > 0 {
        let plural = if hidden > 1 { "s" } else { "" };
        output.push(s.meta(&format!(
            "     … {hidden} more change{plural} hidden (ctrl+o to expand)"
        )));
    }

    output
}

/// Options for [`render_diff_lines_clustered`] (`ClusteredDiffOptions`).
#[derive(Debug, Clone, Default)]
pub struct ClusteredDiffOptions {
    /// Context lines around each change cluster; defaults to 3.
    pub context_lines: Option<usize>,
    /// Cap on body rows emitted (at a cluster boundary); `None` = unlimited.
    pub max_lines: Option<usize>,
    pub is_incomplete: Option<bool>,
    /// Key hint shown in the truncation footer; defaults to `ctrl+o`.
    pub expand_key_hint: Option<String>,
    pub old_start: Option<usize>,
    pub new_start: Option<usize>,
}

/// A contiguous run of diff rows to render (context + change lines).
#[derive(Debug, Clone, Copy)]
struct Cluster {
    start: usize,
    end: usize,
}

/// Group change indices into clusters separated by at least `2 * contextLines`
/// context rows (port of `buildClusters`). Returns
/// `(clusters, changed_count, added_count, removed_count)`.
fn build_clusters(
    diff_lines: &[DiffLine],
    context_lines: usize,
) -> (Vec<Cluster>, usize, usize, usize) {
    let mut change_indices: Vec<usize> = Vec::new();
    let mut added = 0usize;
    let mut removed = 0usize;
    for (i, line) in diff_lines.iter().enumerate() {
        match line.kind {
            DiffLineKind::Add => {
                added += 1;
                change_indices.push(i);
            }
            DiffLineKind::Delete => {
                removed += 1;
                change_indices.push(i);
            }
            DiffLineKind::Context => {}
        }
    }

    let mut clusters: Vec<Cluster> = Vec::new();
    if change_indices.is_empty() {
        return (clusters, 0, added, removed);
    }

    let merge_gap = 2 * context_lines;
    let mut group_start = change_indices[0];
    let mut group_end = change_indices[0];
    for idx in change_indices.iter().skip(1).copied() {
        if idx - group_end <= merge_gap {
            group_end = idx;
        } else {
            clusters.push(Cluster {
                start: group_start.saturating_sub(context_lines),
                end: (group_end + context_lines).min(diff_lines.len() - 1),
            });
            group_start = idx;
            group_end = idx;
        }
    }
    clusters.push(Cluster {
        start: group_start.saturating_sub(context_lines),
        end: (group_end + context_lines).min(diff_lines.len() - 1),
    });

    (clusters, change_indices.len(), added, removed)
}

/// Render one diff row with its gutter (port of `formatDiffRow`).
fn format_diff_row(line: &DiffLine, s: &DiffStyles) -> String {
    let gutter = s.gutter(&format!("{:>4} ", line.line_num));
    match line.kind {
        DiffLineKind::Add => format!("{gutter}{}", s.add(&format!("+ {}", line.code))),
        DiffLineKind::Delete => format!("{gutter}{}", s.del(&format!("- {}", line.code))),
        DiffLineKind::Context => format!("{gutter}  {}", line.code),
    }
}

/// Render a diff with surrounding context, eliding unchanged middle regions
/// between change clusters with a `… N unchanged lines …` separator. When
/// `max_lines` is set, the body is capped at a cluster boundary and a
/// `ctrl+o to expand` footer is appended (port of `renderDiffLinesClustered`).
///
/// Used by Edit's call preview where we want to show *what changed* with
/// enough context to read the change, but not the whole file.
pub fn render_diff_lines_clustered(
    old_text: &str,
    new_text: &str,
    path: &str,
    opts: &ClusteredDiffOptions,
) -> Vec<String> {
    let s = DiffStyles::new();
    let context_lines = opts.context_lines.unwrap_or(3);
    let max_lines = opts.max_lines;
    let diff_lines = compute_diff_lines(
        old_text,
        new_text,
        opts.old_start.unwrap_or(1),
        opts.new_start.unwrap_or(1),
        opts.is_incomplete.unwrap_or(false),
    );
    let (clusters, changed_count, added_count, removed_count) =
        build_clusters(&diff_lines, context_lines);

    let mut output: Vec<String> = Vec::new();
    let mut header = String::new();
    if added_count > 0 {
        header.push_str(&s.add_bold(&format!("+{added_count} ")));
    }
    if removed_count > 0 {
        header.push_str(&s.del_bold(&format!("-{removed_count} ")));
    }
    header.push_str(path);
    output.push(header);

    if clusters.is_empty() {
        return output;
    }

    let cap = max_lines.unwrap_or(usize::MAX);
    let mut body = 0usize;
    let mut prev_end: Option<usize> = None;
    let mut truncated = false;
    let mut shown_changes = 0usize;

    'outer: for cluster in &clusters {
        if body >= cap {
            truncated = true;
            break;
        }
        if let Some(pe) = prev_end {
            let gap = cluster.start.saturating_sub(pe + 1);
            if gap > 0 {
                if body + 1 > cap {
                    truncated = true;
                    break;
                }
                let plural = if gap > 1 { "s" } else { "" };
                output.push(s.meta(&format!("     … {gap} unchanged line{plural} …")));
                body += 1;
            }
        }
        // Emit cluster rows one at a time; allow mid-cluster truncation so a
        // single huge cluster (e.g. the whole file replaced inline) still
        // shows the leading lines instead of degenerating to "N changes
        // hidden" with no body at all.
        for (i, line) in diff_lines
            .iter()
            .enumerate()
            .take(cluster.end + 1)
            .skip(cluster.start)
        {
            if body >= cap {
                truncated = true;
                break 'outer;
            }
            output.push(format_diff_row(line, &s));
            body += 1;
            if line.kind != DiffLineKind::Context {
                shown_changes += 1;
            }
            prev_end = Some(i);
        }
    }

    if truncated {
        let hidden = changed_count.saturating_sub(shown_changes);
        if hidden > 0 {
            let hint = opts
                .expand_key_hint
                .clone()
                .unwrap_or_else(|| "ctrl+o".to_owned());
            let plural = if hidden > 1 { "s" } else { "" };
            output.push(s.meta(&format!(
                "     … {hidden} more change{plural} hidden ({hint} to expand)"
            )));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(lines: &[DiffLine]) -> Vec<DiffLineKind> {
        lines.iter().map(|l| l.kind).collect()
    }

    #[test]
    fn complete_diff_context_and_deletes() {
        let lines = compute_diff_lines("A\nB\nC\nD", "A\nB", 1, 1, false);
        assert_eq!(
            kinds(&lines),
            [
                DiffLineKind::Context,
                DiffLineKind::Context,
                DiffLineKind::Delete,
                DiffLineKind::Delete
            ]
        );
        assert_eq!(lines[0].line_num, 1);
        assert_eq!(lines[2].line_num, 3);
        assert_eq!(lines[3].code, "D");
    }

    #[test]
    fn incomplete_suppresses_trailing_deletes() {
        let lines = compute_diff_lines("A\nB\nC\nD", "A\nB", 1, 1, true);
        assert_eq!(
            kinds(&lines),
            [DiffLineKind::Context, DiffLineKind::Context]
        );
    }

    #[test]
    fn incomplete_suppresses_all_when_everything_deleted() {
        let lines = compute_diff_lines("A\nB\nC", "", 1, 1, true);
        assert!(lines.is_empty());
    }

    #[test]
    fn incomplete_keeps_trailing_adds() {
        let lines = compute_diff_lines("A\nB\nC", "A\nB\nX", 1, 1, true);
        assert_eq!(
            kinds(&lines),
            [
                DiffLineKind::Context,
                DiffLineKind::Context,
                DiffLineKind::Delete,
                DiffLineKind::Add
            ]
        );
    }

    #[test]
    fn incomplete_keeps_internal_delete_blocks() {
        let lines = compute_diff_lines("A\nB\nC\nD", "A\nC", 1, 1, true);
        assert_eq!(
            kinds(&lines),
            [
                DiffLineKind::Context,
                DiffLineKind::Delete,
                DiffLineKind::Context
            ]
        );
    }

    #[test]
    fn empty_old_text_means_no_lines() {
        // `'' ? split : []` — an empty string is falsy in the TS source.
        let lines = compute_diff_lines("", "A\nB", 1, 1, false);
        assert_eq!(kinds(&lines), [DiffLineKind::Add, DiffLineKind::Add]);
    }

    #[test]
    fn clustered_respects_old_and_new_start() {
        // Mirror the TS test: strip ANSI before asserting on the row text.
        let out = render_diff_lines_clustered(
            "A\nB\nC",
            "A\nX\nC",
            "f.ts",
            &ClusteredDiffOptions {
                context_lines: Some(1),
                old_start: Some(10),
                new_start: Some(20),
                ..Default::default()
            },
        );
        let joined = crate::ansi::strip_ansi(&out.join("\n"));
        assert!(joined.contains("  20   A"), "{joined}");
        assert!(joined.contains("  11 - B"), "{joined}");
        assert!(joined.contains("  21 + X"), "{joined}");
        assert!(joined.contains("  22   C"), "{joined}");
    }
}
