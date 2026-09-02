//! Trusted-root use cases — admission, setup state, and management primitives.
//!
//! This module wires the typed [`TrustedRootSet`](crate::domain::TrustedRootSet)
//! and [`evaluate_project_root`](crate::domain::evaluate_project_root) policy
//! to the surrounding `AppConfig` storage. It also models the discovery
//! "setup state" surfaced to users by `project list`, `session list`, and
//! `doctor`.

use camino::{Utf8Path, Utf8PathBuf};

use crate::domain::{TrustedRootDecision, TrustedRootSet, evaluate_project_root};
use crate::ports::{AppConfig, PathCompletionFsPort, PathKind};
use crate::usecases::path_strategy::{PathStrategy, UnixPathStrategy};

/// Read trusted roots from the global `AppConfig`, expanding `~` if `home` is
/// supplied. Returns an empty set when no roots are configured at all.
pub fn trusted_roots_from_config(
    global: Option<&AppConfig>,
    home: Option<&Utf8Path>,
) -> TrustedRootSet {
    let raw = global.map(AppConfig::trusted_roots).unwrap_or_default();
    TrustedRootSet::from_raw(&raw, home)
}

/// Composite outcome describing where Workhive thinks the user is in the
/// "set up trusted roots → build the project index → see results" funnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoverySetupState {
    /// No `[projects].trusted_roots` (and no legacy `[discovery].project_roots`)
    /// is configured. The user must add at least one trusted root before
    /// running `project reindex`.
    NoTrustedRootsConfigured,
    /// Trusted roots are configured but the project index does not exist yet.
    /// The user should run `project reindex` to build it.
    TrustedRootsConfiguredIndexMissing,
    /// Trusted roots are configured and the index exists but contains no
    /// projects — typically because no `.workhive/` has been created under
    /// the trusted roots yet.
    TrustedRootsConfiguredIndexEmpty,
    /// Trusted roots are configured and the index has at least one project.
    Ready,
}

/// Inputs needed to compute [`DiscoverySetupState`].
#[derive(Debug, Clone, Copy)]
pub struct SetupStateProbe {
    /// Whether the global config has at least one configured trusted root.
    pub trusted_roots_configured: bool,
    /// Whether the project index file exists at all on disk.
    pub index_present: bool,
    /// Number of projects currently held in the index. Only consulted when
    /// `index_present` is `true`.
    pub indexed_project_count: usize,
}

/// Pure resolution of the `(trusted_roots, index)` setup-state matrix.
///
/// The CLI layer is responsible for collecting the underlying facts via the
/// config and project-index ports, then handing them to this function.
pub fn resolve_setup_state(probe: SetupStateProbe) -> DiscoverySetupState {
    if !probe.trusted_roots_configured {
        return DiscoverySetupState::NoTrustedRootsConfigured;
    }
    if !probe.index_present {
        return DiscoverySetupState::TrustedRootsConfiguredIndexMissing;
    }
    if probe.indexed_project_count == 0 {
        return DiscoverySetupState::TrustedRootsConfiguredIndexEmpty;
    }
    DiscoverySetupState::Ready
}

/// Pre-flight admission check used by every mutating CLI flow that touches
/// `.workhive/`, worktrees, or runtime state.
///
/// Wraps [`evaluate_project_root`] so the CLI never re-implements the policy.
pub fn admit_mutation(trusted: &TrustedRootSet, project_root: &Utf8Path) -> TrustedRootDecision {
    evaluate_project_root(trusted, project_root)
}

/// Outcome of validating a single trusted-root candidate the user typed
/// into the global init wizard or `project roots add`.
///
/// `Accepted` carries both the canonical absolute form (used for storage
/// and duplicate detection) and the user's authored form (preserved so
/// trusted roots typed as `~/dev` keep their portable shape in TOML).
/// All other variants describe exactly *why* the input was rejected so
/// the CLI can render a localized error message without re-deriving
/// the policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedRootValidation {
    /// Path passed every check; safe to add to the trusted-root list.
    Accepted {
        /// Canonical absolute form used for storage and de-duplication.
        canonical: Utf8PathBuf,
        /// User-authored form (e.g. `~/dev` or `/Users/me/dev`) — used
        /// when displaying the path back to the user.
        authored: Utf8PathBuf,
    },
    /// Input did not start with `/` or `~/`. Trusted roots must be
    /// absolute or home-relative; relative paths are rejected because
    /// they would silently depend on the wizard's working directory.
    NotAbsoluteOrTilde,
    /// Input expanded to a path that is too broad to safely admit as a
    /// trusted root — the filesystem root, a top-level system directory,
    /// or the user's `$HOME` itself. The wizard / `project roots add`
    /// must surface this as `INIT_TRUSTED_ROOTS_REJECT_OVERLY_BROAD` /
    /// `TRUSTED_ROOTS_REJECTED_TOO_BROAD` and bounce the user back to
    /// the prompt instead of writing the entry.
    TooBroad {
        /// Canonical form that triggered the heuristic, included for
        /// the error copy.
        canonical: Utf8PathBuf,
    },
    /// Path expanded to a location that does not exist on disk.
    DoesNotExist {
        /// Canonical form that was probed, included for the error copy.
        canonical: Utf8PathBuf,
    },
    /// Path exists but is a regular file. Trusted roots must be
    /// directories — Workhive creates `.workhive/`, worktrees, and
    /// runtime state *under* a trusted root.
    NotADirectory {
        /// Canonical form that was probed, included for the error copy.
        canonical: Utf8PathBuf,
    },
    /// Path is already in the user's pending list — reject silently
    /// rather than surface a confusing duplicate-add success.
    Duplicate {
        /// Canonical form that already exists in the list.
        canonical: Utf8PathBuf,
    },
    /// Candidate is a strict descendant of an already-trusted root.
    /// A trusted root covers its entire descendant subtree for
    /// Workhive's admission policy, so admitting the child would be
    /// redundant noise in the config and could mislead users about
    /// what is actually trusted.
    ///
    /// The covering root is the *first* configured root that contains
    /// the candidate component-wise (after `~` expansion and lexical
    /// normalization), which lets the CLI tell the user exactly *why*
    /// the entry was rejected without re-deriving the policy.
    CoveredByExistingRoot {
        /// Canonical form of the rejected candidate.
        canonical: Utf8PathBuf,
        /// Canonical form of the existing trusted root that already
        /// covers the candidate's subtree.
        covering_root: Utf8PathBuf,
    },
}

/// Banned canonical paths that always count as "overly broad" trusted
/// roots — admitting any of these would let Workhive create
/// `.workhive/`, worktrees, and runtime state inside a system area or
/// the entire user home, which is never the user's intent. This list is
/// intentionally short and explicit so it can be audited at a glance.
///
/// Kept `pub` so CLI surfaces (e.g. interactive prompts that want to
/// describe *why* a path was rejected) can render the exact set if they
/// wish, but the canonical decision lives in
/// [`classify_overly_broad_root`].
pub const OVERLY_BROAD_BANNED_ROOTS: &[&str] = &[
    "/", "/Users", "/home", "/root", "/etc", "/usr", "/var", "/tmp",
];

/// Heuristic guard used by both the global-init wizard and
/// `project roots add` / `roots edit`: refuse paths that are obviously
/// too broad to safely become a trusted root.
///
/// "Too broad" means any of:
/// 1. The filesystem root (`/`).
/// 2. A top-level system directory listed in
///    [`OVERLY_BROAD_BANNED_ROOTS`].
/// 3. The user's `$HOME` directory itself (e.g. expanding `~` to
///    `/Users/me`). This is always rejected because trusting all of
///    `~` would let Workhive create state under any subdirectory the
///    user happens to navigate into.
///
/// `~` and `~/...` inputs are expanded against `home` before the check,
/// so users cannot bypass the heuristic by typing the tilde form.
///
/// Returns `Some(canonical)` when the input must be rejected (and the
/// CLI should render `TRUSTED_ROOTS_REJECTED_TOO_BROAD` /
/// `INIT_TRUSTED_ROOTS_REJECT_OVERLY_BROAD`); returns `None` when the
/// input is admitted past the broad-root guard. Existence and
/// directory-vs-file checks happen separately in
/// [`classify_trusted_root_input`].
pub fn classify_overly_broad_root(
    candidate: &Utf8Path,
    home: Option<&Utf8Path>,
) -> Option<Utf8PathBuf> {
    let expanded = if let Some(home) = home {
        let raw = candidate.as_str();
        if raw == "~" {
            home.to_owned()
        } else if let Some(rest) = raw.strip_prefix("~/") {
            home.join(rest)
        } else {
            candidate.to_owned()
        }
    } else {
        candidate.to_owned()
    };
    let canonical = crate::domain::normalize_lexical(&expanded);
    if OVERLY_BROAD_BANNED_ROOTS
        .iter()
        .any(|b| canonical.as_str() == *b)
    {
        return Some(canonical);
    }
    if let Some(home) = home
        && canonical == crate::domain::normalize_lexical(home)
    {
        return Some(canonical);
    }
    None
}

/// Validate a single trusted-root candidate against the wizard rules.
///
/// `existing` is the list of already-accepted roots (typed paths) so we
/// can detect duplicates. `home` is the user's `$HOME` (used to expand
/// `~` and `~/...`). `fs` classifies the resolved path as
/// directory/file/missing — there is intentionally NO direct
/// `std::fs::metadata` call here, so the use case stays unit-testable
/// without touching the real filesystem.
///
/// Validation order, fail-fast at the first violation:
/// 1. Shape check (absolute under the active path strategy, or `~` /
///    `~/...`) — `NotAbsoluteOrTilde`.
/// 2. Overly-broad guard (`/`, system dirs, `$HOME`) — `TooBroad`.
///    Both `workhive init --global` and
///    `workhive project roots add/edit` route through this same step
///    so the policy cannot drift between surfaces. Callers that want to
///    bypass the guard for trusted automation paths (e.g. test
///    harnesses with `--force`) should call
///    [`classify_overly_broad_root`] directly and skip ahead.
/// 3. Exact duplicate detection against `existing`.
/// 4. Filesystem classification (directory / file / missing). Missing
///    paths surface as `DoesNotExist` and regular files as
///    `NotADirectory` *before* any ancestor-coverage check so the user
///    sees the real path problem rather than a misleading "covered by
///    parent" message.
/// 5. Ancestor-coverage check: only candidates that already classified
///    as `Directory` may surface as `CoveredByExistingRoot`. A missing
///    or file child under an accepted parent is still a path problem,
///    not a redundancy problem.
pub fn classify_trusted_root_input(
    raw: &Utf8Path,
    existing: &[Utf8PathBuf],
    home: Option<&Utf8Path>,
    fs: &dyn PathCompletionFsPort,
) -> TrustedRootValidation {
    classify_trusted_root_input_with_strategy(raw, existing, home, &UnixPathStrategy, fs)
}

/// Strategy-aware trusted-root validation.
///
/// The interactive trusted-root picker and final commit gate must agree on
/// path syntax. Unix builds accept Unix absolute paths plus `~` / `~/...`;
/// Windows builds accept drive-rooted and UNC paths plus the same home alias.
/// The filesystem and broad-root checks remain authoritative after this shape
/// gate, so strategy support never makes arbitrary relative paths valid.
pub fn classify_trusted_root_input_with_strategy(
    raw: &Utf8Path,
    existing: &[Utf8PathBuf],
    home: Option<&Utf8Path>,
    strategy: &dyn PathStrategy,
    fs: &dyn PathCompletionFsPort,
) -> TrustedRootValidation {
    let s = raw.as_str();
    if !(strategy.is_absolute(s) || strategy.is_home_alias(s)) {
        return TrustedRootValidation::NotAbsoluteOrTilde;
    }

    if let Some(canonical) = classify_overly_broad_root(raw, home) {
        return TrustedRootValidation::TooBroad { canonical };
    }

    let canonical = canonicalize_input(raw, home);

    if existing
        .iter()
        .any(|p| canonicalize_input(p.as_ref(), home) == canonical)
    {
        return TrustedRootValidation::Duplicate { canonical };
    }

    // Filesystem classification runs *before* the ancestor-coverage
    // check on purpose. The global-init wizard passes the pending
    // trusted-root list as `existing`, so if we kept the coverage
    // check first the wizard would mask a typo'd or missing child
    // path (`/Users/me/work/missing`, `/Users/me/work/file.txt`)
    // under an already-accepted parent as "covered by parent". The
    // user must instead see the real `DoesNotExist` / `NotADirectory`
    // error so they can correct the path. Only directory candidates
    // are eligible for the redundancy outcome.
    match fs.classify_path(&canonical) {
        PathKind::Missing => TrustedRootValidation::DoesNotExist { canonical },
        PathKind::File => TrustedRootValidation::NotADirectory { canonical },
        PathKind::Directory => {
            // Ancestor-coverage check: a trusted root covers its
            // entire descendant subtree for Workhive's admission
            // policy. Once `/Users/me/work` is trusted, adding
            // `/Users/me/work/proj-a` is redundant noise. We use
            // component-aware containment via `is_path_within` (not
            // raw string prefix) so siblings like
            // `/Users/me/work-extra` do not collide with
            // `/Users/me/work`. The exact-equality case was already
            // drained by the duplicate check above, so an
            // `is_path_within` hit here always describes a strict
            // descendant.
            if let Some(covering_root) = existing
                .iter()
                .map(|p| canonicalize_input(p.as_ref(), home))
                .find(|covering| {
                    covering != &canonical && crate::domain::is_path_within(&canonical, covering)
                })
            {
                return TrustedRootValidation::CoveredByExistingRoot {
                    canonical,
                    covering_root,
                };
            }
            TrustedRootValidation::Accepted {
                canonical,
                authored: raw.to_owned(),
            }
        }
    }
}

/// Result of mutating the global trusted-root list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedRootMutation {
    /// The path was added to the trusted-root list.
    Added { canonical: Utf8PathBuf },
    /// The path was already in the trusted-root list (no-op).
    AlreadyPresent { canonical: Utf8PathBuf },
    /// The candidate is a strict descendant of an already-trusted root
    /// (component-aware containment after `~` expansion and lexical
    /// normalization). Treated as a no-op: the new path is *not*
    /// appended because the covering parent already trusts the entire
    /// subtree, and Workhive must not silently broaden or rewrite the
    /// trusted-root list on the user's behalf.
    CoveredByExistingRoot {
        /// Canonical form of the rejected candidate.
        canonical: Utf8PathBuf,
        /// Canonical form of the existing trusted root that already
        /// covers the candidate's subtree.
        covering_root: Utf8PathBuf,
    },
    /// The path was removed from the trusted-root list.
    Removed { canonical: Utf8PathBuf },
    /// The path was not in the trusted-root list (no-op).
    NotPresent { canonical: Utf8PathBuf },
}

/// Append `path` to the trusted-root list of an `AppConfig`, returning the
/// updated list and a description of what changed.
///
/// The mutation is performed on a typed `Vec<Utf8PathBuf>` and is therefore
/// safe to drive into a `toml_edit` rewrite of the global config.
pub fn add_trusted_root(
    existing: &[Utf8PathBuf],
    path: &Utf8Path,
    home: Option<&Utf8Path>,
) -> (Vec<Utf8PathBuf>, TrustedRootMutation) {
    let canonical = canonicalize_input(path, home);
    let canonicalized = existing.to_vec();
    let already = canonicalized
        .iter()
        .any(|p| canonicalize_input(p.as_ref(), home) == canonical);
    if already {
        return (
            canonicalized,
            TrustedRootMutation::AlreadyPresent { canonical },
        );
    }
    // Ancestor-coverage check: if some existing trusted root already
    // covers the candidate's subtree, do not append the redundant child.
    // The duplicate case above drains exact equality first, so a match
    // here is always a strict descendant. We never silently replace
    // child roots with parents — that broadening decision belongs to
    // the user, not Workhive.
    if let Some(covering_root) = canonicalized
        .iter()
        .map(|p| canonicalize_input(p.as_ref(), home))
        .find(|covering| {
            covering != &canonical && crate::domain::is_path_within(&canonical, covering)
        })
    {
        return (
            canonicalized,
            TrustedRootMutation::CoveredByExistingRoot {
                canonical,
                covering_root,
            },
        );
    }
    let mut canonicalized = canonicalized;
    canonicalized.push(canonical.clone());
    (canonicalized, TrustedRootMutation::Added { canonical })
}

/// Remove `path` from the trusted-root list of an `AppConfig`.
pub fn remove_trusted_root(
    existing: &[Utf8PathBuf],
    path: &Utf8Path,
    home: Option<&Utf8Path>,
) -> (Vec<Utf8PathBuf>, TrustedRootMutation) {
    let canonical = canonicalize_input(path, home);
    let mut next: Vec<Utf8PathBuf> = Vec::with_capacity(existing.len());
    let mut removed = false;
    for entry in existing {
        if canonicalize_input(entry.as_ref(), home) == canonical {
            removed = true;
            continue;
        }
        next.push(entry.clone());
    }
    let mutation = if removed {
        TrustedRootMutation::Removed { canonical }
    } else {
        TrustedRootMutation::NotPresent { canonical }
    };
    (next, mutation)
}

fn canonicalize_input(path: &Utf8Path, home: Option<&Utf8Path>) -> Utf8PathBuf {
    let expanded = expand_tilde(path, home);
    if let Ok(real) = std::fs::canonicalize(expanded.as_std_path())
        && let Ok(utf8) = Utf8PathBuf::try_from(real)
    {
        return utf8;
    }
    crate::domain::normalize_lexical(&expanded)
}

/// Lexical `~` / `~/...` expansion shared by every call site that needs to
/// convert a user-authored trusted-root path into a filesystem path.
///
/// This is intentionally a pure function: no stat, no canonicalization. It
/// is the primitive that [`expand_trusted_root_for_discovery`] and the
/// internal [`canonicalize_input`] share so `~` handling can never silently
/// regress — if `~` expansion breaks here, both the wizard and reindex
/// discovery path go red together.
pub fn expand_tilde(path: &Utf8Path, home: Option<&Utf8Path>) -> Utf8PathBuf {
    match home {
        Some(home) => {
            let raw = path.as_str();
            if raw == "~" {
                home.to_owned()
            } else if let Some(rest) = raw.strip_prefix("~/") {
                home.join(rest)
            } else {
                path.to_owned()
            }
        }
        None => path.to_owned(),
    }
}

/// Convert authored trusted-root entries (which may contain `~`)
/// into lexically-expanded
/// filesystem paths suitable for discovery/reindex. The authored value is
/// left untouched in the caller's config.
pub fn expand_trusted_roots_for_discovery(
    authored: &[Utf8PathBuf],
    home: Option<&Utf8Path>,
) -> Vec<Utf8PathBuf> {
    authored
        .iter()
        .map(|entry| expand_tilde(entry.as_ref(), home))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::ProjectsConfig;

    fn p(s: &str) -> Utf8PathBuf {
        Utf8PathBuf::from(s)
    }

    // Authored `~/...` trusted roots must be expanded before discovery hits
    // the filesystem, and authored paths must be preserved in the caller's
    // config. The helpers below are the only shared place responsible for
    // that conversion; regressions here would reintroduce the original
    // discovery-root-not-found bug.

    #[test]
    fn expand_tilde_resolves_bare_tilde_against_home() {
        let home = p("/Users/me");
        assert_eq!(
            expand_tilde(Utf8Path::new("~"), Some(home.as_ref())),
            home,
            "`~` must expand to the home directory when home is known",
        );
    }

    #[test]
    fn expand_tilde_resolves_tilde_slash_subpath() {
        let home = p("/Users/me");
        assert_eq!(
            expand_tilde(Utf8Path::new("~/development"), Some(home.as_ref())),
            p("/Users/me/development"),
        );
    }

    #[test]
    fn expand_tilde_passes_through_absolute_and_relative_paths() {
        let home = p("/Users/me");
        assert_eq!(
            expand_tilde(Utf8Path::new("/tmp/x"), Some(home.as_ref())),
            p("/tmp/x"),
            "absolute paths must not be rewritten",
        );
        assert_eq!(
            expand_tilde(Utf8Path::new("projects"), Some(home.as_ref())),
            p("projects"),
            "non-tilde relative paths must pass through verbatim",
        );
    }

    #[test]
    fn expand_tilde_without_home_returns_input_unchanged() {
        assert_eq!(
            expand_tilde(Utf8Path::new("~/development"), None),
            p("~/development"),
            "with no known home, callers get back the authored value; the \
             discovery layer surfaces a typed RootNotFound that the CLI \
             then maps to a localized message",
        );
    }

    #[test]
    fn expand_trusted_roots_for_discovery_preserves_authored_values() {
        let authored = vec![p("~/development"), p("/tmp/explicit")];
        let expanded =
            expand_trusted_roots_for_discovery(&authored, Some(Utf8Path::new("/Users/me")));
        assert_eq!(
            expanded,
            vec![p("/Users/me/development"), p("/tmp/explicit")],
            "~ entries must be expanded for discovery; absolute entries must remain intact",
        );
        assert_eq!(
            authored,
            vec![p("~/development"), p("/tmp/explicit")],
            "authored slice must NOT be mutated so the on-disk config keeps the `~/` form",
        );
    }

    #[test]
    fn trusted_roots_from_config_prefers_projects_section() {
        let cfg = AppConfig {
            projects: Some(ProjectsConfig {
                trusted_roots: vec![p("/Users/me/dev")],
            }),
            ..Default::default()
        };
        let set = trusted_roots_from_config(Some(&cfg), None);
        assert_eq!(set.len(), 1);
        assert!(set.admits(&p("/Users/me/dev/proj")));
    }

    #[test]
    fn trusted_roots_from_config_falls_back_to_legacy_discovery() {
        let cfg = AppConfig {
            discovery: Some(crate::ports::DiscoveryConfig {
                project_roots: vec![p("/Users/me/work")],
            }),
            ..Default::default()
        };
        let set = trusted_roots_from_config(Some(&cfg), None);
        assert!(set.admits(&p("/Users/me/work/proj")));
    }

    #[test]
    fn setup_state_no_trusted_roots() {
        let state = resolve_setup_state(SetupStateProbe {
            trusted_roots_configured: false,
            index_present: false,
            indexed_project_count: 0,
        });
        assert_eq!(state, DiscoverySetupState::NoTrustedRootsConfigured);
    }

    #[test]
    fn setup_state_trusted_roots_but_index_missing() {
        let state = resolve_setup_state(SetupStateProbe {
            trusted_roots_configured: true,
            index_present: false,
            indexed_project_count: 0,
        });
        assert_eq!(
            state,
            DiscoverySetupState::TrustedRootsConfiguredIndexMissing
        );
    }

    #[test]
    fn setup_state_trusted_roots_index_empty() {
        let state = resolve_setup_state(SetupStateProbe {
            trusted_roots_configured: true,
            index_present: true,
            indexed_project_count: 0,
        });
        assert_eq!(state, DiscoverySetupState::TrustedRootsConfiguredIndexEmpty);
    }

    #[test]
    fn setup_state_ready_when_indexed() {
        let state = resolve_setup_state(SetupStateProbe {
            trusted_roots_configured: true,
            index_present: true,
            indexed_project_count: 3,
        });
        assert_eq!(state, DiscoverySetupState::Ready);
    }

    #[test]
    fn add_trusted_root_appends_when_missing() {
        let (next, mutation) = add_trusted_root(&[p("/a")], &p("/b"), None);
        assert_eq!(next, vec![p("/a"), p("/b")]);
        assert!(matches!(mutation, TrustedRootMutation::Added { .. }));
    }

    #[test]
    fn add_trusted_root_is_noop_when_present() {
        let (next, mutation) = add_trusted_root(&[p("/a")], &p("/a"), None);
        assert_eq!(next, vec![p("/a")]);
        assert!(matches!(
            mutation,
            TrustedRootMutation::AlreadyPresent { .. }
        ));
    }

    #[test]
    fn remove_trusted_root_drops_existing_entry() {
        let (next, mutation) = remove_trusted_root(&[p("/a"), p("/b")], &p("/a"), None);
        assert_eq!(next, vec![p("/b")]);
        assert!(matches!(mutation, TrustedRootMutation::Removed { .. }));
    }

    #[test]
    fn remove_trusted_root_is_noop_when_missing() {
        let (next, mutation) = remove_trusted_root(&[p("/a")], &p("/missing"), None);
        assert_eq!(next, vec![p("/a")]);
        assert!(matches!(mutation, TrustedRootMutation::NotPresent { .. }));
    }

    // ── classify_trusted_root_input ───────────────────────────────────
    //
    // Empty input MUST be rejected separately by the wizard's "Empty
    // Input" rule (no path at all is never `Accepted` here because we
    // never even call into `classify_trusted_root_input` with empty).

    use crate::ports::{PathCompletionEntry, PathCompletionFsPort, PathKind};
    use crate::usecases::path_strategy::{UnixPathStrategy, WindowsPathStrategy};
    use std::collections::HashMap;

    /// In-memory `PathCompletionFsPort` that returns canned
    /// `classify_path` results. Lets each validation test pin exactly
    /// one rule without touching the real filesystem.
    struct StubFs {
        kinds: HashMap<Utf8PathBuf, PathKind>,
    }
    impl StubFs {
        fn new() -> Self {
            Self {
                kinds: HashMap::new(),
            }
        }
        fn with(mut self, path: &str, kind: PathKind) -> Self {
            self.kinds.insert(Utf8PathBuf::from(path), kind);
            self
        }
    }
    impl PathCompletionFsPort for StubFs {
        fn list_children(
            &self,
            _parent: &Utf8Path,
        ) -> Result<Vec<PathCompletionEntry>, std::io::Error> {
            Ok(Vec::new())
        }
        fn classify_path(&self, path: &Utf8Path) -> PathKind {
            self.kinds.get(path).copied().unwrap_or(PathKind::Missing)
        }
    }

    #[test]
    fn classify_rejects_relative_paths() {
        let fs = StubFs::new();
        let result = classify_trusted_root_input(
            Utf8Path::new("relative/dev"),
            &[],
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        assert_eq!(result, TrustedRootValidation::NotAbsoluteOrTilde);
    }

    #[test]
    fn classify_rejects_nonexistent_paths() {
        let fs = StubFs::new(); // every probe returns Missing
        let result = classify_trusted_root_input(
            Utf8Path::new("/Users/me/missing"),
            &[],
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::DoesNotExist { canonical } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/missing"));
            }
            other => panic!("expected DoesNotExist, got {other:?}"),
        }
    }

    #[test]
    fn classify_rejects_regular_files_so_workhive_dir_creation_cant_target_them() {
        let fs = StubFs::new().with("/Users/me/notes.md", PathKind::File);
        let result = classify_trusted_root_input(
            Utf8Path::new("/Users/me/notes.md"),
            &[],
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::NotADirectory { canonical } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/notes.md"));
            }
            other => panic!("expected NotADirectory, got {other:?}"),
        }
    }

    #[test]
    fn classify_accepts_existing_directory_and_preserves_authored_form() {
        let fs = StubFs::new().with("/Users/me/dev", PathKind::Directory);
        let result = classify_trusted_root_input(
            Utf8Path::new("~/dev"),
            &[],
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::Accepted {
                canonical,
                authored,
            } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/dev"));
                assert_eq!(
                    authored,
                    Utf8PathBuf::from("~/dev"),
                    "the user's authored form MUST survive validation so the \
                     wizard can write `~/dev` (portable across machines) \
                     into the global config"
                );
            }
            other => panic!("expected Accepted, got {other:?}"),
        }
    }

    #[test]
    fn classify_detects_duplicates_against_existing_typed_paths() {
        let fs = StubFs::new().with("/Users/me/dev", PathKind::Directory);
        let result = classify_trusted_root_input(
            Utf8Path::new("~/dev"),
            &[Utf8PathBuf::from("/Users/me/dev")],
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::Duplicate { canonical } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/dev"));
            }
            other => panic!("expected Duplicate, got {other:?}"),
        }
    }

    #[test]
    fn classify_with_windows_strategy_accepts_drive_rooted_existing_directory() {
        let fs = StubFs::new().with(r"C:\Users\me\dev", PathKind::Directory);
        let result = classify_trusted_root_input_with_strategy(
            Utf8Path::new(r"C:\Users\me\dev"),
            &[],
            None,
            &WindowsPathStrategy,
            &fs,
        );
        match result {
            TrustedRootValidation::Accepted {
                canonical,
                authored,
            } => {
                assert_eq!(canonical, Utf8PathBuf::from(r"C:\Users\me\dev"));
                assert_eq!(authored, Utf8PathBuf::from(r"C:\Users\me\dev"));
            }
            other => panic!("expected Accepted, got {other:?}"),
        }
    }

    #[test]
    fn classify_with_windows_strategy_accepts_unc_existing_directory() {
        let fs = StubFs::new().with(r"\\server\share\projects", PathKind::Directory);
        let result = classify_trusted_root_input_with_strategy(
            Utf8Path::new(r"\\server\share\projects"),
            &[],
            None,
            &WindowsPathStrategy,
            &fs,
        );
        match result {
            TrustedRootValidation::Accepted {
                canonical,
                authored,
            } => {
                assert_eq!(canonical, Utf8PathBuf::from(r"\\server\share\projects"));
                assert_eq!(authored, Utf8PathBuf::from(r"\\server\share\projects"));
            }
            other => panic!("expected Accepted, got {other:?}"),
        }
    }

    #[test]
    fn classify_with_windows_strategy_rejects_posix_absolute_shape() {
        let fs = StubFs::new().with("/Users/me/dev", PathKind::Directory);
        let result = classify_trusted_root_input_with_strategy(
            Utf8Path::new("/Users/me/dev"),
            &[],
            Some(Utf8Path::new("/Users/me")),
            &WindowsPathStrategy,
            &fs,
        );
        assert_eq!(result, TrustedRootValidation::NotAbsoluteOrTilde);
    }

    #[test]
    fn classify_with_unix_strategy_rejects_windows_drive_shape() {
        let fs = StubFs::new().with(r"C:\Users\me\dev", PathKind::Directory);
        let result = classify_trusted_root_input_with_strategy(
            Utf8Path::new(r"C:\Users\me\dev"),
            &[],
            None,
            &UnixPathStrategy,
            &fs,
        );
        assert_eq!(result, TrustedRootValidation::NotAbsoluteOrTilde);
    }

    // ── classify_overly_broad_root + TooBroad routing ─────────────────
    //
    // The broad-root policy lives in `workhive-core` and
    // that `workhive init --global`, `project roots add`, and
    // `project roots edit` all route through the SAME function. These
    // tests pin the shared decision so a future drift between surfaces
    // shows up here instead of via a user-visible bug.

    #[test]
    fn classify_overly_broad_root_rejects_filesystem_roots() {
        for raw in OVERLY_BROAD_BANNED_ROOTS {
            let candidate = Utf8PathBuf::from(*raw);
            let result = classify_overly_broad_root(candidate.as_ref(), None);
            assert!(
                result.is_some(),
                "must reject system root {raw} as overly broad",
            );
        }
    }

    #[test]
    fn classify_overly_broad_root_rejects_home_itself() {
        let home = Utf8PathBuf::from("/Users/me");
        let result = classify_overly_broad_root(home.as_ref(), Some(home.as_ref()));
        assert!(
            result.is_some(),
            "trusting `$HOME` directly is too broad and must be rejected",
        );
    }

    #[test]
    fn classify_overly_broad_root_expands_tilde_before_checking() {
        let home = Utf8PathBuf::from("/Users/me");
        assert!(
            classify_overly_broad_root(Utf8Path::new("~"), Some(home.as_ref())).is_some(),
            "`~` MUST expand to home before the broad-root check, not pass through verbatim",
        );
        assert!(
            classify_overly_broad_root(Utf8Path::new("~/"), Some(home.as_ref())).is_some(),
            "`~/` MUST also expand to home before the broad-root check",
        );
    }

    #[test]
    fn classify_overly_broad_root_admits_concrete_dev_dir() {
        let home = Utf8PathBuf::from("/Users/me");
        let candidate = Utf8PathBuf::from("/Users/me/development");
        assert!(
            classify_overly_broad_root(candidate.as_ref(), Some(home.as_ref())).is_none(),
            "a deliberate dev parent directory must pass the broad-root heuristic",
        );
    }

    #[test]
    fn classify_trusted_root_input_routes_filesystem_root_to_too_broad() {
        // `/` is admitted past the shape check (starts_with '/'), so the
        // wizard MUST reach the broad-root guard before any FS probe runs.
        let fs = StubFs::new(); // never consulted
        let result = classify_trusted_root_input(
            Utf8Path::new("/"),
            &[],
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::TooBroad { canonical } => {
                assert_eq!(canonical, Utf8PathBuf::from("/"));
            }
            other => panic!("expected TooBroad for `/`, got {other:?}"),
        }
    }

    #[test]
    fn classify_trusted_root_input_routes_home_tilde_to_too_broad() {
        // The wizard receives `~` literally; it must expand against home
        // and then trip the broad-root guard, NEVER fall through to the
        // FS probe (which would happily admit the home directory).
        let fs = StubFs::new().with("/Users/me", PathKind::Directory);
        let result = classify_trusted_root_input(
            Utf8Path::new("~"),
            &[],
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::TooBroad { canonical } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me"));
            }
            other => panic!("expected TooBroad for `~`, got {other:?}"),
        }
    }

    #[test]
    fn classify_trusted_root_input_routes_top_level_system_dir_to_too_broad() {
        for raw in ["/Users", "/home", "/etc", "/usr", "/var", "/tmp", "/root"] {
            let fs = StubFs::new().with(raw, PathKind::Directory);
            let result = classify_trusted_root_input(
                Utf8Path::new(raw),
                &[],
                Some(Utf8Path::new("/Users/me")),
                &fs,
            );
            match result {
                TrustedRootValidation::TooBroad { canonical } => {
                    assert_eq!(canonical, Utf8PathBuf::from(raw));
                }
                other => panic!("expected TooBroad for {raw}, got {other:?}"),
            }
        }
    }

    // ── covered-by-existing-root: validation + mutation ───────────────
    //
    // A trusted root covers its entire descendant subtree for Workhive
    // admission; adding a child directory under an already-trusted
    // parent must be rejected as a no-op with a distinct outcome so
    // the CLI can explain *why*.

    #[test]
    fn classify_rejects_redundant_child_under_existing_trusted_root() {
        let fs = StubFs::new().with("/Users/me/work/proj-a", PathKind::Directory);
        let existing = vec![Utf8PathBuf::from("/Users/me/work")];
        let result = classify_trusted_root_input(
            Utf8Path::new("/Users/me/work/proj-a"),
            &existing,
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::CoveredByExistingRoot {
                canonical,
                covering_root,
            } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/work/proj-a"));
                assert_eq!(covering_root, Utf8PathBuf::from("/Users/me/work"));
            }
            other => panic!("expected CoveredByExistingRoot, got {other:?}"),
        }
    }

    #[test]
    fn classify_treats_exact_duplicate_as_duplicate_not_covered() {
        // Edge case: a "covering" path equal to the candidate must
        // continue to surface as Duplicate, not CoveredByExistingRoot,
        // so the existing duplicate UX/messages remain unchanged.
        let fs = StubFs::new().with("/Users/me/work", PathKind::Directory);
        let existing = vec![Utf8PathBuf::from("/Users/me/work")];
        let result = classify_trusted_root_input(
            Utf8Path::new("/Users/me/work"),
            &existing,
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::Duplicate { canonical } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/work"));
            }
            other => panic!("expected Duplicate for exact match, got {other:?}"),
        }
    }

    #[test]
    fn classify_does_not_treat_sibling_with_shared_prefix_as_covered() {
        // Component-aware containment: `/Users/me/work-extra` must not
        // be considered "under" `/Users/me/work` just because the raw
        // strings share a prefix. Using string-prefix matching would
        // false-positive here and break valid sibling roots.
        let fs = StubFs::new().with("/Users/me/work-extra", PathKind::Directory);
        let existing = vec![Utf8PathBuf::from("/Users/me/work")];
        let result = classify_trusted_root_input(
            Utf8Path::new("/Users/me/work-extra"),
            &existing,
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::Accepted { canonical, .. } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/work-extra"));
            }
            other => panic!("expected Accepted for sibling root, got {other:?}"),
        }
    }

    // When the user has already accepted a parent trusted root and
    // then types a child path that is *missing on disk* or *points at
    // a regular file*, the validator MUST surface the real filesystem
    // problem (`DoesNotExist` / `NotADirectory`) instead of masking it
    // as `CoveredByExistingRoot`. The wizard prompt reads the actual
    // pending-roots list as `existing`, so before the fix the user
    // got the misleading "already covered by parent" error for an
    // input that did not even exist.

    #[test]
    fn classify_returns_does_not_exist_for_missing_child_under_existing_parent() {
        // StubFs has no entry for `/Users/me/work/missing`, so
        // `classify_path` returns `Missing`. With the parent
        // `/Users/me/work` already accepted, the validator must still
        // report `DoesNotExist` — the path problem outranks the
        // redundancy problem because Workhive cannot trust what does
        // not exist.
        let fs = StubFs::new().with("/Users/me/work", PathKind::Directory);
        let existing = vec![Utf8PathBuf::from("/Users/me/work")];
        let result = classify_trusted_root_input(
            Utf8Path::new("/Users/me/work/missing"),
            &existing,
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::DoesNotExist { canonical } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/work/missing"));
            }
            other => panic!(
                "expected DoesNotExist for missing child under accepted parent, got {other:?}"
            ),
        }
    }

    #[test]
    fn classify_returns_not_a_directory_for_file_child_under_existing_parent() {
        // The candidate is a regular file under an already-accepted
        // trusted root. Workhive creates `.workhive/`, worktrees, and
        // runtime state *inside* a trusted root, so a file path is
        // structurally unusable even though the parent already covers
        // its subtree. The user must see `NotADirectory`, not the
        // redundancy message.
        let fs = StubFs::new()
            .with("/Users/me/work", PathKind::Directory)
            .with("/Users/me/work/notes.md", PathKind::File);
        let existing = vec![Utf8PathBuf::from("/Users/me/work")];
        let result = classify_trusted_root_input(
            Utf8Path::new("/Users/me/work/notes.md"),
            &existing,
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::NotADirectory { canonical } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/work/notes.md"));
            }
            other => {
                panic!("expected NotADirectory for file child under accepted parent, got {other:?}")
            }
        }
    }

    #[test]
    fn classify_expands_tilde_before_running_coverage_check() {
        // `~/work` and `/Users/me/work/proj-a` describe the same
        // logical relationship; the coverage rule must fire regardless
        // of which form the existing root and candidate take.
        let fs = StubFs::new().with("/Users/me/work/proj-a", PathKind::Directory);
        let existing = vec![Utf8PathBuf::from("~/work")];
        let result = classify_trusted_root_input(
            Utf8Path::new("/Users/me/work/proj-a"),
            &existing,
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        match result {
            TrustedRootValidation::CoveredByExistingRoot {
                canonical,
                covering_root,
            } => {
                assert_eq!(canonical, Utf8PathBuf::from("/Users/me/work/proj-a"));
                assert_eq!(covering_root, Utf8PathBuf::from("/Users/me/work"));
            }
            other => panic!("expected CoveredByExistingRoot after tilde expansion, got {other:?}"),
        }
    }

    #[test]
    fn add_trusted_root_skips_descendant_of_existing_root() {
        let (next, mutation) = add_trusted_root(
            &[p("/Users/me/work")],
            &p("/Users/me/work/proj-a"),
            Some(Utf8Path::new("/Users/me")),
        );
        assert_eq!(
            next,
            vec![p("/Users/me/work")],
            "covered child must NOT be appended; the parent already trusts the subtree",
        );
        match mutation {
            TrustedRootMutation::CoveredByExistingRoot {
                canonical,
                covering_root,
            } => {
                assert_eq!(canonical, p("/Users/me/work/proj-a"));
                assert_eq!(covering_root, p("/Users/me/work"));
            }
            other => panic!("expected CoveredByExistingRoot mutation, got {other:?}"),
        }
    }

    #[test]
    fn add_trusted_root_still_appends_independent_sibling_root() {
        // Regression guard: sibling paths whose textual prefix overlaps
        // a trusted root must continue to be accepted. The coverage
        // check is component-aware.
        let (next, mutation) = add_trusted_root(
            &[p("/Users/me/work")],
            &p("/Users/me/work-extra"),
            Some(Utf8Path::new("/Users/me")),
        );
        assert_eq!(next, vec![p("/Users/me/work"), p("/Users/me/work-extra")]);
        assert!(matches!(mutation, TrustedRootMutation::Added { .. }));
    }

    #[test]
    fn add_trusted_root_exact_duplicate_remains_already_present() {
        // The covered-by-parent path must not steal exact-duplicate
        // behavior — the existing AlreadyPresent message and UX stay.
        let (next, mutation) = add_trusted_root(
            &[p("/Users/me/work")],
            &p("/Users/me/work"),
            Some(Utf8Path::new("/Users/me")),
        );
        assert_eq!(next, vec![p("/Users/me/work")]);
        assert!(matches!(
            mutation,
            TrustedRootMutation::AlreadyPresent { .. }
        ));
    }

    #[test]
    fn classify_trusted_root_input_too_broad_takes_precedence_over_duplicate() {
        // If the user re-types `~` after it was already (somehow) accepted
        // earlier, the broad-root guard must still fire — it is the
        // primary safety policy and must NOT be silenced by the
        // duplicate-detection optimization.
        let fs = StubFs::new();
        let existing = vec![Utf8PathBuf::from("/Users/me")];
        let result = classify_trusted_root_input(
            Utf8Path::new("~"),
            &existing,
            Some(Utf8Path::new("/Users/me")),
            &fs,
        );
        assert!(
            matches!(result, TrustedRootValidation::TooBroad { .. }),
            "broad-root guard must fire before duplicate detection: got {result:?}",
        );
    }
}
