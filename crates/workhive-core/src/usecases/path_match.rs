//! Narrow fuzzy matcher for the trusted-root path picker.
//!
//! ## Why this is a narrow wrapper, not `fuzzy-matcher`
//!
//! Two equivalent options were considered:
//!
//!   * pull in a mature Rust matcher crate (e.g. `nucleo-matcher`,
//!     `fuzzy-matcher`); or
//!   * keep the matcher logic in **one** isolated, well-tested
//!     module so the rest of the codebase never grows ad-hoc fuzzy
//!     loops.
//!
//! Workhive picks option 2 here for two reasons:
//!
//!   1. The matching surface is tiny — directory names from a single
//!      level, never source tokens. We do not need fzf-grade scoring
//!      heuristics, just deterministic ordering with prefix priority.
//!   2. We MUST guarantee stable, locale-free ordering across CI
//!      runs and test snapshots. A small wrapper keeps that contract
//!      visible in this file rather than buried behind a dependency.
//!
//! Match tiers, in order of priority:
//!
//!   1. **CaseSensitivePrefix** — `"Doc"` matches `"Documents"` first.
//!   2. **CaseInsensitivePrefix** — `"doc"` matches `"Documents"`.
//!   3. **CaseInsensitiveSubstring** — `"cum"` matches `"Documents"`.
//!   4. **Subsequence** — every query char appears in order, case
//!      insensitive (`"dt"` matches `"Documents"`).
//!
//! Within a tier, ties break by ASCII-lowercase candidate name.

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchKind {
    CaseSensitivePrefix,
    CaseInsensitivePrefix,
    CaseInsensitiveSubstring,
    Subsequence,
}

/// Match outcome for a single candidate. Higher is better, but the
/// comparator here is `Ord` based on tier (smaller tier wins) and
/// then alphabetical tie-breaking is left to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchOutcome {
    pub kind: MatchKind,
}

/// Score `query` against `name`. Returns `None` when `name` does not
/// match at all. An empty `query` always matches with the
/// case-insensitive prefix tier so that the empty filter shows the
/// browse scope as-is.
pub fn score_match(query: &str, name: &str) -> Option<MatchOutcome> {
    if query.is_empty() {
        return Some(MatchOutcome {
            kind: MatchKind::CaseInsensitivePrefix,
        });
    }
    if name.starts_with(query) {
        return Some(MatchOutcome {
            kind: MatchKind::CaseSensitivePrefix,
        });
    }
    let name_lc = name.to_ascii_lowercase();
    let query_lc = query.to_ascii_lowercase();
    if name_lc.starts_with(&query_lc) {
        return Some(MatchOutcome {
            kind: MatchKind::CaseInsensitivePrefix,
        });
    }
    if name_lc.contains(&query_lc) {
        return Some(MatchOutcome {
            kind: MatchKind::CaseInsensitiveSubstring,
        });
    }
    if is_subsequence(&query_lc, &name_lc) {
        return Some(MatchOutcome {
            kind: MatchKind::Subsequence,
        });
    }
    None
}

/// `true` when every character of `query` appears in `haystack` in
/// the same order. Both sides are expected to already be lowercase
/// — callers MUST not pass mixed case here.
fn is_subsequence(query: &str, haystack: &str) -> bool {
    let mut chars = query.chars();
    let mut next = chars.next();
    for c in haystack.chars() {
        if let Some(target) = next
            && target == c
        {
            next = chars.next();
            if next.is_none() {
                return true;
            }
        }
    }
    next.is_none()
}

/// Rank `candidates` against `query` and return the surviving names
/// in display order. Non-matching candidates are filtered out.
///
/// Sort order is:
///   1. Match tier ascending (smaller tier = better).
///   2. Candidate name, ASCII-lowercase, ascending — gives stable,
///      locale-free output so CI runs never reorder rows.
pub fn rank_candidates<I, S>(query: &str, candidates: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut scored: Vec<(MatchKind, String, String)> = candidates
        .into_iter()
        .filter_map(|item| {
            let name = item.as_ref().to_owned();
            score_match(query, &name).map(|m| (m.kind, name.to_ascii_lowercase(), name))
        })
        .collect();

    scored.sort_by(|a, b| match a.0.cmp(&b.0) {
        Ordering::Equal => a.1.cmp(&b.1),
        ord => ord,
    });

    scored.into_iter().map(|(_, _, name)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Empty query returns all candidates and is stable-sorted.
    #[test]
    fn empty_query_keeps_all_candidates_in_stable_alphabetical_order() {
        let out = rank_candidates("", ["Pictures", "Documents", "Downloads", "Applications"]);
        assert_eq!(
            out,
            vec!["Applications", "Documents", "Downloads", "Pictures"]
        );
    }

    /// Prefix match must beat substring match — "doc" must put
    /// "Documents" before "indocs" even though both contain "doc".
    #[test]
    fn prefix_match_outranks_substring_match() {
        let out = rank_candidates("doc", ["indocs", "Documents", "Pictures"]);
        assert_eq!(out, vec!["Documents", "indocs"]);
    }

    /// Case-insensitive: typing "DOC" must still match "Documents".
    #[test]
    fn matching_is_case_insensitive_for_both_sides() {
        let out = rank_candidates("DOC", ["Documents", "DocumentsBak", "data"]);
        assert_eq!(out, vec!["Documents", "DocumentsBak"]);
    }

    /// Subsequence is the lowest tier — only consulted when no
    /// substring matches; "Pictures" must NOT fall under "dc" but
    /// "Documents" must (d → o → c).
    #[test]
    fn subsequence_match_is_used_only_when_substring_fails() {
        let out = rank_candidates("dc", ["Documents", "Pictures", "Music"]);
        assert_eq!(out, vec!["Documents"]);
    }

    /// Stable alphabetical tie-break: two candidates in the same
    /// tier MUST come out alphabetically sorted regardless of input
    /// order.
    #[test]
    fn ties_break_alphabetically_for_stable_output() {
        let out = rank_candidates("ap", ["Apps", "apex", "App-data"]);
        assert_eq!(out, vec!["apex", "App-data", "Apps"]);
    }

    /// Non-matching candidates are filtered out entirely.
    #[test]
    fn non_matching_candidates_are_dropped() {
        let out = rank_candidates("xyz", ["Documents", "Downloads"]);
        assert!(out.is_empty(), "unexpected matches: {out:?}");
    }

    /// Case-sensitive prefix outranks case-insensitive prefix when
    /// both apply — exercises tier ordering.
    #[test]
    fn case_sensitive_prefix_beats_case_insensitive_prefix() {
        let out = rank_candidates("Doc", ["doctor", "Documents", "documents"]);
        assert_eq!(out, vec!["Documents", "doctor", "documents"]);
    }
}
