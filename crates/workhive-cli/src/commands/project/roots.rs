#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::ExecutableAvailabilityPort;

// ── Typed editor outcome ──────────────────────────────────────────────────────

/// Outcome of the shared trusted-root editor interaction.
///
/// All callers (global init, `project roots edit`, admission menu) receive
/// this typed result from the reusable editor flow. No completion output or
/// persistence decisions are made inside the editor — callers compose the
/// next step from this outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditorOutcome {
    /// The user saved changes. Contains the state before and after.
    Saved {
        before: Vec<camino::Utf8PathBuf>,
        after: Vec<camino::Utf8PathBuf>,
    },
    /// The editor exited without changes (user chose Save with no dirty state).
    Unchanged,
    /// The user explicitly cancelled (Cancel action or Ctrl+C).
    Cancelled,
}

impl EditorOutcome {
    /// Whether the outcome represents a successful save with actual changes.
    pub(crate) fn is_changed_save(&self) -> bool {
        matches!(self, Self::Saved { .. })
    }
}

/// Determine the editor outcome from the edit session state.
///
/// Pure decision: given the original roots, the working set, and a dirty flag,
/// returns the typed outcome without performing any I/O.
pub(crate) fn resolve_editor_outcome(
    original: &[camino::Utf8PathBuf],
    working: &[camino::Utf8PathBuf],
    dirty: bool,
    user_action: EditorUserAction,
) -> EditorOutcome {
    match user_action {
        EditorUserAction::Save => {
            if dirty {
                EditorOutcome::Saved {
                    before: original.to_vec(),
                    after: working.to_vec(),
                }
            } else {
                EditorOutcome::Unchanged
            }
        }
        EditorUserAction::Cancel => EditorOutcome::Cancelled,
    }
}

/// The terminal action the user chose to exit the editor loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorUserAction {
    Save,
    Cancel,
}

// ── `workhive project roots` ──────────────────────────────────────────────────

/// Heuristic guard re-export for `project roots add` / `roots edit`.
///
/// The broad-root policy lives in
/// [`workhive_core::usecases::trusted_roots::classify_overly_broad_root`]
/// so `workhive init --global` (the wizard) and `workhive project
/// roots add/edit` (this module) share a single implementation. This
/// thin alias preserves the existing CLI call-sites without duplicating
/// the policy.
pub(crate) fn classify_overly_broad_root(
    candidate: &camino::Utf8Path,
    home: Option<&camino::Utf8Path>,
) -> Option<camino::Utf8PathBuf> {
    workhive_core::usecases::trusted_roots::classify_overly_broad_root(candidate, home)
}

pub(crate) fn load_global_or_default(
    ctx: &CommandContext,
) -> Result<workhive_core::ports::AppConfig, CliError> {
    Ok(ctx.config.load_global()?.unwrap_or_default())
}

/// Apply a `[projects].trusted_roots` mutation to the on-disk global TOML
/// using `toml_edit`, preserving comments, formatting, and unrelated keys.
///
/// `roots add/remove/edit`
/// MUST NOT round-trip through `AppConfig` and `toml::to_string_pretty`,
/// because that loses user comments and reformats unrelated tables. Instead
/// we load the raw document, mutate the single array we care about, and
/// write the document back. Legacy `[discovery].project_roots` is removed
/// only after we have committed the value into the canonical
/// `[projects].trusted_roots` array.
pub(crate) fn write_trusted_roots_into_global_toml(
    next_roots: &[camino::Utf8PathBuf],
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use toml_edit::{Array, DocumentMut, Item, Table, Value};

    let raw = ctx.config.load_raw_toml(None, true)?;
    let mut doc: DocumentMut = if raw.trim().is_empty() {
        DocumentMut::new()
    } else {
        raw.parse::<DocumentMut>().map_err(|e| {
            CliError::Runtime(format!(
                "global config TOML is malformed; refusing to mutate trusted roots: {e}"
            ))
        })?
    };

    // 1) Ensure `[projects]` exists, then replace `trusted_roots` in place.
    if !doc.contains_table("projects") {
        let mut table = Table::new();
        table.set_implicit(false);
        doc.insert("projects", Item::Table(table));
    }
    let projects = doc["projects"]
        .as_table_mut()
        .expect("projects table just inserted");
    let mut array = Array::new();
    for root in next_roots {
        array.push(Value::from(root.as_str()));
    }
    projects.insert("trusted_roots", Item::Value(Value::Array(array)));

    // 2) Drop the legacy `[discovery].project_roots` alias if present so the
    //    canonical key wins. We only remove that one field — any *other*
    //    user keys under `[discovery]` survive untouched, and an empty
    //    discovery table is removed entirely to keep the file tidy.
    if let Some(discovery) = doc.get_mut("discovery").and_then(|i| i.as_table_mut()) {
        discovery.remove("project_roots");
        if discovery.is_empty() {
            doc.remove("discovery");
        }
    }

    let serialized = doc.to_string();
    ctx.config.save_raw_toml(None, true, &serialized)?;
    Ok(())
}

/// Validate that `path` (after `~` expansion) refers to an existing
/// directory. Missing paths and
/// regular files must fail with a localized error before any mutation.
pub(crate) fn validate_trusted_root_path(
    path: &camino::Utf8Path,
    allow_overly_broad: bool,
    ctx: &CommandContext,
) -> Result<camino::Utf8PathBuf, CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::{PathCompletionFsPort, PathKind};
    use workhive_core::usecases::trusted_roots::{
        TrustedRootValidation, classify_trusted_root_input_with_strategy,
    };
    use workhive_infra::path_completion::FsPathCompletion;

    let locale = ctx.locale();
    let home = home_path();
    let strategy = workhive_core::usecases::path_strategy::current_target_strategy();
    let fs = FsPathCompletion::new();

    match classify_trusted_root_input_with_strategy(
        path,
        &[],
        home.as_deref(),
        strategy.as_ref(),
        &fs,
    ) {
        TrustedRootValidation::Accepted { canonical, .. }
        | TrustedRootValidation::Duplicate { canonical } => Ok(canonical),
        TrustedRootValidation::NotAbsoluteOrTilde => Err(CliError::Runtime(
            ctx.i18n
                .resolve_with_args(locale, k::INIT_TRUSTED_ROOTS_INVALID, &[path.as_str()]),
        )),
        // `validate_trusted_root_path` is called *before* mutating the
        // trusted-root list, with `existing` always empty here. So
        // `CoveredByExistingRoot` is structurally unreachable in this
        // call path — the mutation surface
        // (`cmd_project_roots_add` / `cmd_project_roots_edit`) is what
        // actually enforces the redundant-child rule against the live
        // list. We treat any unexpected occurrence as a runtime error
        // rather than `unreachable!()` so the CLI never panics on an
        // unexpected validation state.
        TrustedRootValidation::CoveredByExistingRoot {
            canonical,
            covering_root,
        } => Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::TRUSTED_ROOTS_COVERED_BY_EXISTING_ROOT,
            &[canonical.as_str(), covering_root.as_str()],
        ))),
        TrustedRootValidation::TooBroad { canonical } if allow_overly_broad => {
            match fs.classify_path(canonical.as_ref()) {
                PathKind::Directory => Ok(canonical),
                PathKind::File => Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                    locale,
                    k::TRUSTED_ROOTS_PATH_NOT_DIRECTORY,
                    &[canonical.as_str()],
                ))),
                PathKind::Missing => Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                    locale,
                    k::TRUSTED_ROOTS_PATH_MISSING,
                    &[canonical.as_str()],
                ))),
            }
        }
        TrustedRootValidation::TooBroad { canonical } => {
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::TRUSTED_ROOTS_REJECTED_TOO_BROAD,
                &[canonical.as_str()],
            )))
        }
        TrustedRootValidation::DoesNotExist { canonical } => {
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::TRUSTED_ROOTS_PATH_MISSING,
                &[canonical.as_str()],
            )))
        }
        TrustedRootValidation::NotADirectory { canonical } => {
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::TRUSTED_ROOTS_PATH_NOT_DIRECTORY,
                &[canonical.as_str()],
            )))
        }
    }
}

/// Count indexed projects whose root is identical to, or contained
/// component-wise within, `removed`. Uses
/// `workhive_core::domain::is_path_within` (component-level check) instead of
/// any string-prefix shortcut.
///
/// # Canonicalization
///
/// Uses filesystem-aware canonicalization via `PathCanonicalizationPort` to
/// resolve symlinks and normalize paths. This is intentional: the CLI layer
/// needs to match what the discovery layer produces, which also uses the
/// filesystem-aware adapter.
///
/// TODO: Consider threading `PathCanonicalizationPort` through this function
/// to enable testing with fakes. Currently constructs `RealPathCanonicalization`
/// inline, which is acceptable for a zero-sized type but bypasses DI.
pub(crate) fn count_affected_projects_under_root(
    removed: &camino::Utf8Path,
    projects: &[workhive_core::ports::ProjectSummary],
) -> usize {
    use workhive_core::ports::PathCanonicalizationPort;
    let canonicalizer = workhive_infra::path_canonicalization::RealPathCanonicalization::new();
    let canonical_removed = canonicalizer.canonicalize(removed);
    projects
        .iter()
        .filter(|p| {
            let canonical_project = canonicalizer.canonicalize(p.project_root.as_ref());
            workhive_core::domain::is_path_within(&canonical_project, &canonical_removed)
        })
        .count()
}

/// One entry in the `roots edit` consolidated warning surface: a single
/// staged removal plus the count of indexed projects it affects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AffectedRootSummary {
    /// Canonical path that will be removed from `[projects].trusted_roots`.
    pub canonical_root: camino::Utf8PathBuf,
    /// Number of indexed projects whose root is contained component-wise
    /// within `canonical_root`.
    pub count: usize,
}

/// Compute the set of canonical trusted roots that exist in `original` but
/// not in `working`. Uses filesystem-aware canonicalization for equality so that
/// `~/dev`, `/Users/me/dev`, and `/Users/me/dev/.` collapse together — never
/// a raw string compare.
///
/// # Canonicalization
///
/// Uses filesystem-aware canonicalization via `PathCanonicalizationPort` to
/// resolve symlinks and normalize paths. This is intentional: the CLI layer
/// needs to match what the discovery layer produces, which also uses the
/// filesystem-aware adapter.
pub(crate) fn diff_removed_roots_canonical(
    original: &[camino::Utf8PathBuf],
    working: &[camino::Utf8PathBuf],
) -> Vec<camino::Utf8PathBuf> {
    use workhive_core::ports::PathCanonicalizationPort;
    let canonicalizer = workhive_infra::path_canonicalization::RealPathCanonicalization::new();
    let working_canonical: Vec<camino::Utf8PathBuf> = working
        .iter()
        .map(|p| canonicalizer.canonicalize(p.as_ref()))
        .collect();
    original
        .iter()
        .filter_map(|orig| {
            let canonical = canonicalizer.canonicalize(orig.as_ref());
            if working_canonical.iter().any(|w| w == &canonical) {
                None
            } else {
                Some(canonical)
            }
        })
        .collect()
}

/// For each staged removal, count the indexed projects it affects. Removals
/// that affect zero projects are filtered out — callers only render a
/// warning surface when the result is non-empty. Reuses
/// `count_affected_projects_under_root` so `roots remove` and `roots edit`
/// stay aligned on the component-level path containment rule.
pub(crate) fn aggregate_affected_projects_for_removed_roots(
    removed: &[camino::Utf8PathBuf],
    projects: &[workhive_core::ports::ProjectSummary],
) -> Vec<AffectedRootSummary> {
    removed
        .iter()
        .filter_map(|root| {
            let count = count_affected_projects_under_root(root.as_ref(), projects);
            if count > 0 {
                Some(AffectedRootSummary {
                    canonical_root: root.clone(),
                    count,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Load the global project index for trusted-root removal safety accounting.
///
/// `roots remove` and `roots edit` (Save branch) both depend on the indexed
/// project list to count how many projects sit under a root the user is
/// about to revoke. This helper centralizes the **fail-closed** behavior:
///
/// - `IndexState::Loaded(v)` → `Ok(v)`. Normal path.
/// - `IndexState::NotFound`  → `Ok(vec![])`. The empty-index case is a
///   recognized setup state; nothing has been registered, so a removal
///   cannot affect indexed projects.
/// - `Err(IndexError)`       → `Err(CliError::Index(_))`. A read or parse
///   failure means the safety evidence is *unknown*, not zero. The caller
///   must abort before any TOML is rewritten — never relabel the failure
///   as "no affected projects".
///
/// The previous behavior logged a warning ("project index unreadable;
/// proceeding without affected-project warning") and continued with an
/// empty list. That converted exactly the diagnostic we needed (the index
/// cannot be trusted) into the strongest possible permission (zero
/// affected projects). This helper deliberately removes that bypass.
///
/// The function takes `&dyn ProjectIndexPort` rather than `&CommandContext`
/// so it can be exercised directly through `FakeProjectIndex` without
/// spinning up a real on-disk index.
pub(crate) fn load_indexed_projects_for_root_removal(
    index: &dyn workhive_core::ports::ProjectIndexPort,
) -> Result<Vec<workhive_core::ports::ProjectSummary>, CliError> {
    use workhive_core::ports::IndexState;
    match index.list_projects()? {
        IndexState::Loaded(projects) => Ok(projects),
        IndexState::NotFound => Ok(Vec::new()),
    }
}

/// Validate the non-UI portion of `project roots edit` Save.
///
/// The interactive command stages changes in memory and only writes TOML after
/// this helper returns successfully. Keeping the removed-root diff, fail-closed
/// index load, and affected-project aggregation here gives the Save path a
/// default-running unit-test interface without automating the TUI.
pub(crate) fn validate_roots_edit_save(
    original: &[camino::Utf8PathBuf],
    working: &[camino::Utf8PathBuf],
    index: &dyn workhive_core::ports::ProjectIndexPort,
) -> Result<Vec<AffectedRootSummary>, CliError> {
    let removed = diff_removed_roots_canonical(original, working);
    if removed.is_empty() {
        return Ok(Vec::new());
    }
    let projects = load_indexed_projects_for_root_removal(index)?;
    Ok(aggregate_affected_projects_for_removed_roots(
        &removed, &projects,
    ))
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::ports::{ChangeKind, ChangedFile, WorktreeInfo};
    #[allow(unused_imports)]
    use workhive_core::usecases::worktree_safety::{
        UnmergedCommitSummary, WorktreeRisk, WorktreeSafetyReport,
    };

    /// Smoke check that the CLI alias forwards to
    /// `workhive_core::usecases::trusted_roots::classify_overly_broad_root`.
    /// The exhaustive policy tests live in
    /// `workhive_core::usecases::trusted_roots::tests::*` so global init
    /// and `project roots add/edit` cannot drift.
    #[test]
    fn classify_overly_broad_root_rejects_filesystem_roots() {
        for raw in [
            "/", "/Users", "/home", "/root", "/etc", "/usr", "/var", "/tmp",
        ] {
            let candidate = Utf8PathBuf::from(raw);
            assert!(
                classify_overly_broad_root(candidate.as_ref(), None).is_some(),
                "must reject system root {raw}",
            );
        }
    }

    #[test]
    fn classify_overly_broad_root_rejects_home_itself() {
        let home = Utf8PathBuf::from("/Users/me");
        let result = classify_overly_broad_root(home.as_ref(), Some(home.as_ref()));
        assert!(
            result.is_some(),
            "trusting `$HOME` directly is too broad and must be rejected by the heuristic",
        );
    }

    #[test]
    fn classify_overly_broad_root_admits_realistic_dev_dir() {
        let home = Utf8PathBuf::from("/Users/me");
        let candidate = Utf8PathBuf::from("/Users/me/development");
        assert!(
            classify_overly_broad_root(candidate.as_ref(), Some(home.as_ref())).is_none(),
            "a deliberate dev parent directory must pass the heuristic",
        );
    }

    /// `count_affected_projects_under_root` must
    /// use component-level path containment, not string-prefix matching, so
    /// sibling directories that share a textual prefix are NOT counted.
    /// `/dev` and `/dev2` are siblings, not parent/child.
    #[test]
    fn affected_projects_uses_component_level_containment_not_prefix() {
        use workhive_core::ports::ProjectSummary;
        let removed_root = camino::Utf8PathBuf::from("/dev");
        let projects = vec![
            ProjectSummary {
                project_name: "inside".to_owned(),
                project_root: Utf8PathBuf::from("/dev/inside"),
                config_path: Utf8PathBuf::from("/dev/inside/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            },
            ProjectSummary {
                project_name: "sibling".to_owned(),
                project_root: Utf8PathBuf::from("/dev2/sibling"),
                config_path: Utf8PathBuf::from("/dev2/sibling/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            },
            ProjectSummary {
                project_name: "exact".to_owned(),
                project_root: Utf8PathBuf::from("/dev"),
                config_path: Utf8PathBuf::from("/dev/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            },
        ];
        // /dev is removed → /dev/inside (descendant) and /dev (exact match)
        // count, but /dev2/sibling does NOT, despite sharing the "/dev"
        // string prefix.
        assert_eq!(
            count_affected_projects_under_root(removed_root.as_ref(), &projects),
            2,
            "component-level containment must include /dev/inside and /dev itself, but exclude /dev2/sibling",
        );
    }

    /// Empty project lists trivially produce zero affected projects.
    #[test]
    fn affected_projects_zero_when_index_empty() {
        let removed_root = camino::Utf8PathBuf::from("/dev");
        assert_eq!(
            count_affected_projects_under_root(removed_root.as_ref(), &[]),
            0,
        );
    }

    // -- Shared affected-project guard for `roots edit`
    //    Save path. The helpers below are pure functions over canonical
    //    paths and an in-memory project list, so they exercise the exact
    //    control flow the interactive Save path uses without needing a TTY
    //    fixture. --

    #[test]
    fn diff_removed_roots_canonical_uses_canonical_equality_not_string_compare() {
        // Original contains a path with a trailing `/.`, which canonicalizes
        // to the same physical directory as the bare path. The diff must
        // collapse them, leaving zero removals despite the textual mismatch.
        let original = vec![Utf8PathBuf::from("/dev/projects/.")];
        let working = vec![Utf8PathBuf::from("/dev/projects")];
        let removed = diff_removed_roots_canonical(&original, &working);
        assert!(
            removed.is_empty(),
            "canonical equality must collapse `/dev/projects/.` and `/dev/projects` (got removed={removed:?})",
        );
    }

    #[test]
    fn diff_removed_roots_canonical_returns_canonical_paths_for_disappearances() {
        let original = vec![Utf8PathBuf::from("/dev/foo"), Utf8PathBuf::from("/dev/bar")];
        let working = vec![Utf8PathBuf::from("/dev/foo")];
        let removed = diff_removed_roots_canonical(&original, &working);
        assert_eq!(removed.len(), 1);
        // We can't predict whether `/dev/bar` exists on the test host, so
        // we accept either the canonicalized real path or the lexically
        // normalized path — both equal `/dev/bar` in component form.
        assert!(
            removed[0].as_str().ends_with("dev/bar") || removed[0].as_str() == "/dev/bar",
            "expected /dev/bar (canonical or normalized), got {removed:?}",
        );
    }

    #[test]
    fn diff_removed_roots_canonical_zero_when_no_change() {
        let original = vec![Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("/dev/b")];
        let working = original.clone();
        assert!(diff_removed_roots_canonical(&original, &working).is_empty());
    }

    /// Aggregation across multiple staged removals: each removal contributes
    /// its own count via component-level containment, and rows with zero
    /// affected projects are filtered out so the warning surface stays
    /// focused.
    #[test]
    fn aggregate_affected_projects_filters_zero_count_removals() {
        use workhive_core::ports::ProjectSummary;
        let projects = vec![
            ProjectSummary {
                project_name: "a-1".to_owned(),
                project_root: Utf8PathBuf::from("/dev/a/proj1"),
                config_path: Utf8PathBuf::from("/dev/a/proj1/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            },
            ProjectSummary {
                project_name: "a-2".to_owned(),
                project_root: Utf8PathBuf::from("/dev/a/proj2"),
                config_path: Utf8PathBuf::from("/dev/a/proj2/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            },
            ProjectSummary {
                project_name: "b-only".to_owned(),
                project_root: Utf8PathBuf::from("/dev/b/proj1"),
                config_path: Utf8PathBuf::from("/dev/b/proj1/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            },
        ];
        // Stage 3 removals: /dev/a (2 affected), /dev/b (1 affected),
        // /dev/c (0 — must be filtered out of the result).
        let removed = vec![
            Utf8PathBuf::from("/dev/a"),
            Utf8PathBuf::from("/dev/b"),
            Utf8PathBuf::from("/dev/c"),
        ];
        let summaries = aggregate_affected_projects_for_removed_roots(&removed, &projects);
        assert_eq!(
            summaries.len(),
            2,
            "/dev/c affects zero projects and must be filtered out of the consolidated warning",
        );
        let by_root: std::collections::BTreeMap<&str, usize> = summaries
            .iter()
            .map(|s| (s.canonical_root.as_str(), s.count))
            .collect();
        assert_eq!(by_root.get("/dev/a").copied(), Some(2));
        assert_eq!(by_root.get("/dev/b").copied(), Some(1));
        assert!(!by_root.contains_key("/dev/c"));
    }

    #[test]
    fn aggregate_affected_projects_uses_component_level_containment_not_prefix() {
        use workhive_core::ports::ProjectSummary;
        // /dev2/sibling shares a textual prefix with /dev but lives in a
        // different parent directory. It must NOT be counted as affected
        // by removing /dev — string-prefix matching is forbidden
        // anywhere in the affected-project pipeline.
        let projects = vec![ProjectSummary {
            project_name: "sibling".to_owned(),
            project_root: Utf8PathBuf::from("/dev2/sibling"),
            config_path: Utf8PathBuf::from("/dev2/sibling/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        }];
        let removed = vec![Utf8PathBuf::from("/dev")];
        let summaries = aggregate_affected_projects_for_removed_roots(&removed, &projects);
        assert!(
            summaries.is_empty(),
            "removing /dev must not flag /dev2/sibling as affected; got {summaries:?}",
        );
    }

    #[test]
    fn aggregate_affected_projects_empty_when_no_removals_or_no_projects() {
        // Both empty inputs produce an empty consolidated warning surface.
        assert!(aggregate_affected_projects_for_removed_roots(&[], &[]).is_empty());

        let removed = vec![Utf8PathBuf::from("/dev/anything")];
        assert!(aggregate_affected_projects_for_removed_roots(&removed, &[]).is_empty());

        use workhive_core::ports::ProjectSummary;
        let projects = vec![ProjectSummary {
            project_name: "x".to_owned(),
            project_root: Utf8PathBuf::from("/dev/x"),
            config_path: Utf8PathBuf::from("/dev/x/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        }];
        assert!(aggregate_affected_projects_for_removed_roots(&[], &projects).is_empty());
    }

    // -- Shared fail-closed helper for trusted-root
    //    removal safety. Both `cmd_project_roots_remove` and the Save
    //    branch of `cmd_project_roots_edit` route through
    //    `load_indexed_projects_for_root_removal`, so the contract is
    //    audited once here and the call sites only need to forward the
    //    `?` propagation.

    /// Loaded index → returned verbatim. The helper must not invent or
    /// re-shape projects, only normalize the `IndexState`/`IndexError`
    /// envelope into the shape the affected-project counter expects.
    #[test]
    fn load_indexed_projects_for_root_removal_returns_loaded_projects() {
        use workhive_core::ports::{ProjectIndexPort, ProjectSummary};
        use workhive_testkit::fakes::FakeProjectIndex;
        let index = FakeProjectIndex::new();
        let summary = ProjectSummary {
            project_name: "alpha".to_owned(),
            project_root: Utf8PathBuf::from("/dev/alpha"),
            config_path: Utf8PathBuf::from("/dev/alpha/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        };
        index
            .upsert_project(&summary)
            .expect("seeding fake index must succeed");

        let projects = load_indexed_projects_for_root_removal(&index)
            .expect("loaded index must be returned as-is");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_root, summary.project_root);
    }

    /// `IndexState::NotFound` is a known setup state — there is simply
    /// nothing to count yet. The helper resolves it to an empty list so
    /// `roots remove` / `roots edit` can proceed without an
    /// affected-project warning.
    #[test]
    fn load_indexed_projects_for_root_removal_treats_not_found_as_zero_projects() {
        use workhive_testkit::fakes::FakeProjectIndex;
        let index = FakeProjectIndex::with_not_found();
        let projects = load_indexed_projects_for_root_removal(&index)
            .expect("missing index file is a recognized setup state, not an error");
        assert!(
            projects.is_empty(),
            "NotFound must resolve to an empty project list, got {projects:?}",
        );
    }

    /// `IndexError::ReadFailed` MUST propagate through the helper as
    /// `CliError::Index`. A read failure means the safety evidence is
    /// unknown — the call site must abort the trusted-root removal
    /// before any TOML write, not silently coerce it to "zero affected
    /// projects".
    #[test]
    fn load_indexed_projects_for_root_removal_propagates_read_failed_as_cli_error() {
        use workhive_core::errors::IndexError;
        use workhive_testkit::fakes::FakeProjectIndex;

        let index = FakeProjectIndex::new();
        index.set_list_error(IndexError::ReadFailed {
            path: Utf8PathBuf::from("/tmp/index.json"),
            source: std::io::Error::other("boom"),
        });

        let err = load_indexed_projects_for_root_removal(&index)
            .expect_err("ReadFailed must NOT be silently treated as zero affected projects");
        assert!(
            matches!(err, CliError::Index(IndexError::ReadFailed { .. })),
            "expected CliError::Index(ReadFailed), got {err:?}",
        );
    }

    /// `IndexError::Corrupted` MUST propagate as `CliError::Index`. A
    /// corrupt index is even more dangerous than a missing file — we
    /// know data exists but cannot trust it, so failing closed is
    /// mandatory.
    #[test]
    fn load_indexed_projects_for_root_removal_propagates_corrupted_as_cli_error() {
        use workhive_core::errors::IndexError;
        use workhive_testkit::fakes::FakeProjectIndex;

        let index = FakeProjectIndex::new();
        index.set_list_error(IndexError::Corrupted {
            path: Utf8PathBuf::from("/tmp/index.json"),
        });

        let err = load_indexed_projects_for_root_removal(&index)
            .expect_err("Corrupted must NOT be silently treated as zero affected projects");
        assert!(
            matches!(err, CliError::Index(IndexError::Corrupted { .. })),
            "expected CliError::Index(Corrupted), got {err:?}",
        );
    }

    #[test]
    fn validate_roots_edit_save_propagates_corrupted_index_before_write() {
        use workhive_core::errors::IndexError;
        use workhive_testkit::fakes::FakeProjectIndex;

        let original = vec![Utf8PathBuf::from("/dev")];
        let working = Vec::new();
        let index = FakeProjectIndex::new();
        index.set_list_error(IndexError::Corrupted {
            path: Utf8PathBuf::from("/tmp/index.json"),
        });

        let err = validate_roots_edit_save(&original, &working, &index)
            .expect_err("edit Save must fail before write when index is corrupt");
        assert!(
            matches!(err, CliError::Index(IndexError::Corrupted { .. })),
            "expected CliError::Index(Corrupted), got {err:?}",
        );
    }

    #[test]
    fn validate_roots_edit_save_propagates_read_failed_before_write() {
        use workhive_core::errors::IndexError;
        use workhive_testkit::fakes::FakeProjectIndex;

        let original = vec![Utf8PathBuf::from("/dev")];
        let working = Vec::new();
        let index = FakeProjectIndex::new();
        index.set_list_error(IndexError::ReadFailed {
            path: Utf8PathBuf::from("/tmp/index.json"),
            source: std::io::Error::other("read failed"),
        });

        let err = validate_roots_edit_save(&original, &working, &index)
            .expect_err("edit Save must fail before write when index cannot be read");
        assert!(
            matches!(err, CliError::Index(IndexError::ReadFailed { .. })),
            "expected CliError::Index(ReadFailed), got {err:?}",
        );
    }

    #[test]
    fn validate_roots_edit_save_not_found_index_allows_empty_affected_summary() {
        use workhive_testkit::fakes::FakeProjectIndex;

        let original = vec![Utf8PathBuf::from("/dev")];
        let working = Vec::new();
        let index = FakeProjectIndex::with_not_found();

        let summaries = validate_roots_edit_save(&original, &working, &index)
            .expect("missing index is a recognized setup state");
        assert!(
            summaries.is_empty(),
            "missing index should yield no affected-project warning, got {summaries:?}",
        );
    }

    #[test]
    fn validate_roots_edit_save_returns_affected_summary_for_removed_roots() {
        use workhive_core::ports::{ProjectIndexPort, ProjectSummary};
        use workhive_testkit::fakes::FakeProjectIndex;

        let original = vec![Utf8PathBuf::from("/dev")];
        let working = Vec::new();
        let index = FakeProjectIndex::new();
        index
            .upsert_project(&ProjectSummary {
                project_name: "alpha".to_owned(),
                project_root: Utf8PathBuf::from("/dev/alpha"),
                config_path: Utf8PathBuf::from("/dev/alpha/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            })
            .expect("seeding fake index must succeed");

        let summaries = validate_roots_edit_save(&original, &working, &index)
            .expect("loaded index should produce affected summary");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].canonical_root, Utf8PathBuf::from("/dev"));
        assert_eq!(summaries[0].count, 1);
    }

    /// Tilde-prefixed entries are expanded by the heuristic so users typing
    /// `~` or `~/` aren't accidentally allowed past the guard.
    #[test]
    fn classify_overly_broad_root_expands_tilde_before_check() {
        let home = Utf8PathBuf::from("/Users/me");
        assert!(
            classify_overly_broad_root(camino::Utf8Path::new("~"), Some(home.as_ref())).is_some(),
            "`~` must expand to home and be rejected",
        );
    }

    // ── trusted_roots_editor_outcome_ — typed outcome contract ────────

    #[test]
    fn trusted_roots_editor_outcome_save_with_additions_returns_saved() {
        let original = vec![Utf8PathBuf::from("/dev/a")];
        let working = vec![Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("/dev/b")];
        let outcome = resolve_editor_outcome(&original, &working, true, EditorUserAction::Save);
        assert_eq!(
            outcome,
            EditorOutcome::Saved {
                before: original,
                after: working,
            }
        );
    }

    #[test]
    fn trusted_roots_editor_outcome_save_with_removals_returns_saved() {
        let original = vec![Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("/dev/b")];
        let working = vec![Utf8PathBuf::from("/dev/a")];
        let outcome = resolve_editor_outcome(&original, &working, true, EditorUserAction::Save);
        assert_eq!(
            outcome,
            EditorOutcome::Saved {
                before: original,
                after: working,
            }
        );
    }

    #[test]
    fn trusted_roots_editor_outcome_save_not_dirty_returns_unchanged() {
        let original = vec![Utf8PathBuf::from("/dev/a")];
        let working = original.clone();
        let outcome = resolve_editor_outcome(&original, &working, false, EditorUserAction::Save);
        assert_eq!(outcome, EditorOutcome::Unchanged);
    }

    #[test]
    fn trusted_roots_editor_outcome_cancel_returns_cancelled() {
        let original = vec![Utf8PathBuf::from("/dev/a")];
        let working = vec![Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("/dev/b")];
        let outcome = resolve_editor_outcome(&original, &working, true, EditorUserAction::Cancel);
        assert_eq!(outcome, EditorOutcome::Cancelled);
    }

    #[test]
    fn trusted_roots_editor_outcome_cancel_with_no_changes_returns_cancelled() {
        let original = vec![Utf8PathBuf::from("/dev/a")];
        let working = original.clone();
        let outcome = resolve_editor_outcome(&original, &working, false, EditorUserAction::Cancel);
        assert_eq!(outcome, EditorOutcome::Cancelled);
    }

    #[test]
    fn trusted_roots_editor_outcome_is_changed_save_true_only_for_saved() {
        let saved = EditorOutcome::Saved {
            before: vec![],
            after: vec![Utf8PathBuf::from("/dev/a")],
        };
        assert!(saved.is_changed_save());
        assert!(!EditorOutcome::Unchanged.is_changed_save());
        assert!(!EditorOutcome::Cancelled.is_changed_save());
    }

    #[test]
    fn trusted_roots_editor_outcome_save_empty_working_returns_saved() {
        let original = vec![Utf8PathBuf::from("/dev/a")];
        let working: Vec<Utf8PathBuf> = vec![];
        let outcome = resolve_editor_outcome(&original, &working, true, EditorUserAction::Save);
        assert_eq!(
            outcome,
            EditorOutcome::Saved {
                before: original,
                after: vec![],
            },
            "saving an empty list is a valid user intent (remove all roots)"
        );
    }

    // ── trusted_index_filter_ — read-time trust filtering ─────────────

    use workhive_core::domain::TrustedRootSet;
    use workhive_core::ports::ProjectSummary;
    use workhive_core::usecases::trusted_roots::filter_projects_by_trusted_roots;

    fn make_project(name: &str, root: &str) -> ProjectSummary {
        ProjectSummary {
            project_name: name.to_owned(),
            project_root: Utf8PathBuf::from(root),
            config_path: Utf8PathBuf::from(format!("{root}/.workhive/config.toml")),
            last_opened_at: None,
            last_updated_at: None,
        }
    }

    #[test]
    fn trusted_index_filter_admits_project_under_trusted_root() {
        let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/projects")], None);
        let projects = vec![make_project("alpha", "/dev/projects/alpha")];
        let filtered = filter_projects_by_trusted_roots(projects, &roots);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].project_name, "alpha");
    }

    #[test]
    fn trusted_index_filter_rejects_project_outside_trusted_root() {
        let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/projects")], None);
        let projects = vec![make_project("rogue", "/other/rogue")];
        let filtered = filter_projects_by_trusted_roots(projects, &roots);
        assert!(
            filtered.is_empty(),
            "project outside trusted roots must be filtered out"
        );
    }

    #[test]
    fn trusted_index_filter_removed_root_disappears_before_cleanup() {
        let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/a")], None);
        let projects = vec![
            make_project("inside", "/dev/a/proj"),
            make_project("removed-root", "/dev/b/proj"),
        ];
        let filtered = filter_projects_by_trusted_roots(projects, &roots);
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].project_name, "inside",
            "only projects under current roots survive filtering"
        );
    }

    #[test]
    fn trusted_index_filter_empty_roots_returns_empty() {
        let roots = TrustedRootSet::from_raw(&[], None);
        let projects = vec![make_project("alpha", "/dev/alpha")];
        let filtered = filter_projects_by_trusted_roots(projects, &roots);
        assert!(
            filtered.is_empty(),
            "empty trust set must filter everything"
        );
    }

    #[test]
    fn trusted_index_filter_no_string_prefix_matching() {
        let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev")], None);
        let projects = vec![
            make_project("inside", "/dev/proj"),
            make_project("sibling", "/dev2/proj"),
        ];
        let filtered = filter_projects_by_trusted_roots(projects, &roots);
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].project_name, "inside",
            "/dev2 is NOT under /dev (component-level check)"
        );
    }

    #[test]
    fn trusted_index_filter_multiple_roots_union() {
        let roots = TrustedRootSet::from_raw(
            &[Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("/dev/b")],
            None,
        );
        let projects = vec![
            make_project("a-proj", "/dev/a/proj"),
            make_project("b-proj", "/dev/b/proj"),
            make_project("c-proj", "/dev/c/proj"),
        ];
        let filtered = filter_projects_by_trusted_roots(projects, &roots);
        assert_eq!(filtered.len(), 2);
        let names: Vec<&str> = filtered.iter().map(|p| p.project_name.as_str()).collect();
        assert!(names.contains(&"a-proj"));
        assert!(names.contains(&"b-proj"));
    }

    #[test]
    fn trusted_index_filter_exact_root_match_admitted() {
        let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/projects")], None);
        let projects = vec![make_project("exact", "/dev/projects")];
        let filtered = filter_projects_by_trusted_roots(projects, &roots);
        assert_eq!(
            filtered.len(),
            1,
            "project root exactly matching a trusted root must be admitted"
        );
    }

    #[test]
    fn trusted_index_filter_deeply_nested_project_admitted() {
        let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev")], None);
        let projects = vec![make_project("deep", "/dev/a/b/c/d/project")];
        let filtered = filter_projects_by_trusted_roots(projects, &roots);
        assert_eq!(filtered.len(), 1, "deeply nested projects must be admitted");
    }
}
