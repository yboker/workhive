//! Trusted project roots — typed model and admission policy.
//!
//! A "trusted project root" is a directory under which Workhive is allowed to:
//! - create `.workhive/` and instance metadata
//! - materialize git worktrees and runtime state
//! - be discovered during `project reindex` / `project list` / `session list`
//!
//! Read-only commands inside an already-initialized project are NOT gated by
//! trusted roots. This module owns the *admission* decision; CLI handlers
//! must funnel mutating operations through [`evaluate_project_root`] instead
//! of writing scattered ad-hoc string-prefix checks.

use camino::{Utf8Path, Utf8PathBuf};

/// A canonicalized list of trusted project roots.
///
/// Constructed from raw config values. All entries are normalized to absolute
/// paths suitable for ancestor-aware comparison; no string prefix matching is
/// performed downstream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedRootSet {
    roots: Vec<Utf8PathBuf>,
}

impl TrustedRootSet {
    /// Build from raw paths, normalizing each entry lexically.
    ///
    /// Normalization rules:
    /// - leading `~` is expanded to the value of `home`, when provided
    /// - paths are normalized lexically (collapsing `.` and `..` segments)
    /// - duplicates and empty entries are dropped
    ///
    /// This method is **purely lexical** — it does NOT resolve symlinks or
    /// probe the filesystem. Filesystem-backed canonicalization (including
    /// symlink resolution) belongs in the `PathCanonicalizationPort` adapter
    /// in `workhive-infra`, wired at the CLI composition root.
    pub fn from_raw(raw: &[Utf8PathBuf], home: Option<&Utf8Path>) -> Self {
        let mut roots = Vec::with_capacity(raw.len());
        for entry in raw {
            if entry.as_str().is_empty() {
                continue;
            }
            let expanded = expand_home(entry, home);
            let canonical = canonicalize_or_normalize(&expanded);
            if !roots.contains(&canonical) {
                roots.push(canonical);
            }
        }
        Self { roots }
    }

    /// Whether the trusted root set is empty.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Number of configured roots.
    pub fn len(&self) -> usize {
        self.roots.len()
    }

    /// Return the canonicalized roots for display / persistence.
    pub fn roots(&self) -> &[Utf8PathBuf] {
        &self.roots
    }

    /// Returns the matching trusted root that admits the given project root,
    /// or `None` when no trusted root contains it.
    ///
    /// Match rule: `project_root` must equal a trusted root, or be a strict
    /// path-component descendant of a trusted root after canonicalization.
    /// String prefix matching is explicitly NOT used.
    pub fn admit<'a>(&'a self, project_root: &Utf8Path) -> Option<&'a Utf8Path> {
        let candidate = canonicalize_or_normalize(project_root);
        self.roots
            .iter()
            .find(|root| is_path_within(&candidate, root.as_ref()))
            .map(|p| p.as_path())
    }

    /// Whether the given project root is admitted by this trusted-root set.
    pub fn admits(&self, project_root: &Utf8Path) -> bool {
        self.admit(project_root).is_some()
    }
}

/// Decision returned by [`evaluate_project_root`] for a mutating operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedRootDecision {
    /// No trusted roots are configured. Caller must direct the user to
    /// add at least one before performing the mutation.
    NoTrustedRootsConfigured,
    /// At least one trusted root is configured but the project root is not
    /// covered by any of them.
    OutsideTrustedRoots {
        /// The project root that was evaluated.
        project_root: Utf8PathBuf,
    },
    /// The project root is admitted by this trusted root.
    Admitted {
        /// The trusted root that admitted the project.
        matched_root: Utf8PathBuf,
    },
}

/// Evaluate a mutating-operation request against the trusted-root policy.
pub fn evaluate_project_root(
    trusted: &TrustedRootSet,
    project_root: &Utf8Path,
) -> TrustedRootDecision {
    if trusted.is_empty() {
        return TrustedRootDecision::NoTrustedRootsConfigured;
    }
    if let Some(matched) = trusted.admit(project_root) {
        return TrustedRootDecision::Admitted {
            matched_root: matched.to_owned(),
        };
    }
    TrustedRootDecision::OutsideTrustedRoots {
        project_root: canonicalize_or_normalize(project_root),
    }
}

/// Lexical normalization (no filesystem access required).
///
/// Collapses redundant separators and resolves `.`/`..` segments without
/// touching the disk. Symbolic links are NOT resolved — this function
/// operates purely on the path string. Filesystem-backed canonicalization
/// (including symlink resolution) belongs in the `PathCanonicalizationPort`
/// adapter in `workhive-infra`, wired at the CLI composition root.
pub fn normalize_lexical(path: &Utf8Path) -> Utf8PathBuf {
    let mut out: Vec<&str> = Vec::new();
    let absolute = path.is_absolute();
    for component in path.as_str().split('/') {
        match component {
            "" => {
                if absolute && out.is_empty() {
                    // Preserve the leading "/" for absolute paths.
                    out.push("");
                }
            }
            "." => {}
            ".." => {
                let pop_root = !absolute || out.len() > 1;
                if pop_root && out.last().is_some_and(|seg| !seg.is_empty()) {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            seg => out.push(seg),
        }
    }
    if absolute && out.len() == 1 {
        // Avoid producing an empty Utf8PathBuf for "/".
        return Utf8PathBuf::from("/");
    }
    let joined = if absolute {
        out.join("/")
    } else if out.is_empty() {
        ".".to_owned()
    } else {
        out.join("/")
    };
    Utf8PathBuf::from(joined)
}

/// Lexical canonicalization of a path (no filesystem access).
///
/// Collapses redundant separators, `.` and `..` segments, and normalizes
/// the result. Domain code MUST call this function (or use
/// [`TrustedRootSet::from_raw`] / [`TrustedRootSet::admit`]) rather than
/// invoking `std::fs::canonicalize` directly — filesystem-backed
/// canonicalization belongs in the `PathCanonicalizationPort` adapter in
/// `workhive-infra`, and the CLI wires it at the composition root.
///
/// Exposed so other modules (CLI handlers, use cases) can reuse the same
/// lexical normalization rule that `TrustedRootSet` applies internally.
pub fn canonicalize_or_normalize(path: &Utf8Path) -> Utf8PathBuf {
    normalize_lexical(path)
}

/// Expand a leading `~` / `~/` reference using the supplied home directory.
///
/// Returns the input unchanged when the entry does not start with a tilde or
/// no home is provided.
pub fn expand_home(entry: &Utf8Path, home: Option<&Utf8Path>) -> Utf8PathBuf {
    let raw = entry.as_str();
    let Some(home) = home else {
        return entry.to_owned();
    };
    if raw == "~" {
        return home.to_owned();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    entry.to_owned()
}

/// Component-aware "is `child` equal to or under `parent`?" check.
///
/// Walks both paths split on `/` and compares them segment-by-segment. This
/// is the canonical containment rule used by `TrustedRootSet::admit`; do not
/// use string-prefix matching anywhere else.
pub fn is_path_within(child: &Utf8Path, parent: &Utf8Path) -> bool {
    if child == parent {
        return true;
    }
    let parent_components: Vec<&str> = parent
        .as_str()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let child_components: Vec<&str> = child
        .as_str()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if parent_components.len() > child_components.len() {
        return false;
    }
    parent_components
        .iter()
        .zip(child_components.iter())
        .all(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Utf8PathBuf {
        Utf8PathBuf::from(s)
    }

    #[test]
    fn empty_set_returns_no_trusted_roots_configured() {
        let set = TrustedRootSet::default();
        let decision = evaluate_project_root(&set, &p("/anywhere/project"));
        assert!(matches!(
            decision,
            TrustedRootDecision::NoTrustedRootsConfigured
        ));
    }

    #[test]
    fn project_root_equal_to_trusted_root_is_admitted() {
        let set = TrustedRootSet::from_raw(&[p("/Users/me/dev/proj")], None);
        let decision = evaluate_project_root(&set, &p("/Users/me/dev/proj"));
        match decision {
            TrustedRootDecision::Admitted { matched_root } => {
                assert_eq!(matched_root, p("/Users/me/dev/proj"));
            }
            other => panic!("expected Admitted, got {other:?}"),
        }
    }

    #[test]
    fn project_under_trusted_root_subdirectory_is_admitted() {
        let set = TrustedRootSet::from_raw(&[p("/Users/me/dev")], None);
        let decision = evaluate_project_root(&set, &p("/Users/me/dev/sub/proj"));
        assert!(matches!(decision, TrustedRootDecision::Admitted { .. }));
    }

    #[test]
    fn sibling_prefix_paths_are_rejected() {
        let set = TrustedRootSet::from_raw(&[p("/Users/me/dev")], None);
        let decision = evaluate_project_root(&set, &p("/Users/me/dev2/proj"));
        match decision {
            TrustedRootDecision::OutsideTrustedRoots { project_root } => {
                assert_eq!(project_root, p("/Users/me/dev2/proj"));
            }
            other => panic!("expected OutsideTrustedRoots, got {other:?}"),
        }
    }

    #[test]
    fn home_prefixed_entries_are_expanded() {
        let home = p("/Users/me");
        let set = TrustedRootSet::from_raw(&[p("~/dev"), p("~")], Some(home.as_ref()));
        assert!(set.roots().contains(&p("/Users/me/dev")));
        assert!(set.roots().contains(&p("/Users/me")));
    }

    #[test]
    fn dot_dot_segments_are_normalized() {
        let normalized = normalize_lexical(&p("/Users/me/dev/../work/proj"));
        assert_eq!(normalized, p("/Users/me/work/proj"));
    }

    #[test]
    fn duplicate_entries_are_collapsed() {
        let set = TrustedRootSet::from_raw(&[p("/Users/me/dev"), p("/Users/me/dev")], None);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn empty_entries_are_dropped() {
        let set = TrustedRootSet::from_raw(&[p(""), p("/Users/me/dev")], None);
        assert_eq!(set.len(), 1);
    }

    /// Domain layer contract: `TrustedRootSet::from_raw` MUST NOT resolve
    /// symlinks. This test pins the architectural decision that domain code
    /// operates purely on lexical paths, and filesystem-backed canonicalization
    /// (including symlink resolution) belongs in the `PathCanonicalizationPort`
    /// adapter in `workhive-infra`.
    ///
    /// If this test fails, it means someone re-introduced `std::fs::canonicalize`
    /// into the domain layer, which would break the clean separation between
    /// domain (lexical) and infrastructure (filesystem-aware) canonicalization.
    #[test]
    fn from_raw_does_not_resolve_symlinks_lexical_only() {
        // Even if /tmp exists on the test host (it almost certainly does),
        // the domain layer must NOT attempt to resolve it via std::fs::canonicalize.
        // The stored root should be the lexically normalized form, not the
        // filesystem-resolved form (which might be /private/tmp on macOS).
        let set = TrustedRootSet::from_raw(&[p("/tmp")], None);
        assert_eq!(
            set.roots(),
            &[p("/tmp")],
            "domain layer must store the lexically normalized path, not the filesystem-resolved form; \
             if this fails on macOS (where /tmp -> /private/tmp), it means std::fs::canonicalize \
             was incorrectly re-introduced into the domain layer"
        );

        // Another example: /var/run on macOS is a symlink to /private/var/run.
        // The domain layer must store /var/run, not /private/var/run.
        let set2 = TrustedRootSet::from_raw(&[p("/var/run")], None);
        assert_eq!(
            set2.roots(),
            &[p("/var/run")],
            "domain layer must NOT resolve symlinks; /var/run should remain as-is, \
             not become /private/var/run"
        );
    }
}
