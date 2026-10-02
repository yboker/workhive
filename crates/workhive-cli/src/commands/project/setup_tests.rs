use super::*;

#[test]
fn infers_project_root_from_managed_worktree_root_path() {
    let inferred = infer_project_root_from_managed_worktree_path(Utf8Path::new(
        "/repo/.workhive/worktrees/feat-new",
    ));

    assert_eq!(inferred, Some(Utf8PathBuf::from("/repo")));
}

#[test]
fn infers_project_root_from_managed_worktree_subdir_path() {
    let inferred = infer_project_root_from_managed_worktree_path(Utf8Path::new(
        "/repo/.workhive/worktrees/feat-new/src/module",
    ));

    assert_eq!(inferred, Some(Utf8PathBuf::from("/repo")));
}

#[test]
fn does_not_infer_project_root_from_unmanaged_path() {
    let inferred =
        infer_project_root_from_managed_worktree_path(Utf8Path::new("/repo/worktrees/feat-new"));

    assert_eq!(inferred, None);
}

// ── trust gate action matrix ──────────────────────────────────────
//
// Full matrix of (admission decision × interactive × overly broad).
// Admitted roots always proceed; unadmitted roots may only be offered
// the trust prompt in an interactive session for a non-broad root.

use workhive_core::domain::TrustedRootDecision;

fn admitted() -> TrustedRootDecision {
    TrustedRootDecision::Admitted {
        matched_root: Utf8PathBuf::from("/trusted"),
    }
}

fn outside() -> TrustedRootDecision {
    TrustedRootDecision::OutsideTrustedRoots {
        project_root: Utf8PathBuf::from("/elsewhere/proj"),
    }
}

#[test]
fn admitted_root_proceeds_regardless_of_interactivity() {
    for interactive in [true, false] {
        for broad in [true, false] {
            assert_eq!(
                resolve_trust_gate_action(&admitted(), interactive, broad),
                TrustGateAction::Proceed,
            );
        }
    }
}

#[test]
fn unadmitted_root_offers_prompt_only_when_interactive_and_not_broad() {
    for decision in [TrustedRootDecision::NoTrustedRootsConfigured, outside()] {
        assert_eq!(
            resolve_trust_gate_action(&decision, true, false),
            TrustGateAction::OfferTrustPrompt,
        );
    }
}

#[test]
fn unadmitted_root_rejects_in_non_interactive_sessions() {
    for decision in [TrustedRootDecision::NoTrustedRootsConfigured, outside()] {
        assert_eq!(
            resolve_trust_gate_action(&decision, false, false),
            TrustGateAction::Reject,
            "non-interactive sessions must fail explicitly, never hang on a prompt",
        );
    }
}

#[test]
fn overly_broad_root_is_never_offered_the_trust_prompt() {
    for interactive in [true, false] {
        assert_eq!(
            resolve_trust_gate_action(
                &TrustedRootDecision::NoTrustedRootsConfigured,
                interactive,
                true,
            ),
            TrustGateAction::Reject,
            "the broad-root guard outranks interactivity",
        );
    }
}

// ── trusted_roots_admission_menu_ — admission action model ────────

#[test]
fn trusted_roots_admission_menu_workspace_open_defaults_to_trust_project() {
    assert_eq!(
        admission_menu_default_index(AdmissionCaller::WorkspaceOpen),
        ADMISSION_INDEX_TRUST_PROJECT,
    );
}

#[test]
fn trusted_roots_admission_menu_project_init_defaults_to_manage_directories() {
    assert_eq!(
        admission_menu_default_index(AdmissionCaller::ProjectInit),
        ADMISSION_INDEX_MANAGE_DIRECTORIES,
    );
}

#[test]
fn trusted_roots_admission_menu_index_0_maps_to_trust_project() {
    assert_eq!(
        admission_action_from_index(ADMISSION_INDEX_TRUST_PROJECT),
        AdmissionAction::TrustProject,
    );
}

#[test]
fn trusted_roots_admission_menu_index_1_maps_to_manage_directories() {
    assert_eq!(
        admission_action_from_index(ADMISSION_INDEX_MANAGE_DIRECTORIES),
        AdmissionAction::ManageTrustedDirectories,
    );
}

#[test]
fn trusted_roots_admission_menu_index_2_maps_to_cancel() {
    assert_eq!(
        admission_action_from_index(ADMISSION_INDEX_CANCEL),
        AdmissionAction::Cancel,
    );
}

#[test]
fn trusted_roots_admission_menu_out_of_range_maps_to_cancel() {
    assert_eq!(
        admission_action_from_index(99),
        AdmissionAction::Cancel,
        "any unknown index must be treated as cancel"
    );
}

#[test]
fn trusted_roots_admission_menu_labels_returns_three_items_in_both_locales() {
    use workhive_core::domain::Locale;
    use workhive_infra::i18n::StaticI18n;
    let i18n = StaticI18n::new();
    for locale in [Locale::En, Locale::ZhCn] {
        let labels = admission_menu_labels(&i18n, locale);
        assert_eq!(
            labels.len(),
            3,
            "{locale:?}: admission menu must have exactly 3 labels"
        );
        for (idx, label) in labels.iter().enumerate() {
            assert!(
                !label.is_empty(),
                "{locale:?}: label at index {idx} must not be empty"
            );
        }
    }
}

#[test]
fn trusted_roots_admission_menu_project_admitted_after_editor_true_when_covered() {
    let project_root = camino::Utf8Path::new("/dev/projects/myapp");
    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    assert!(
        is_project_admitted_after_editor(project_root, &roots, None),
        "project under a trusted root must be admitted"
    );
}

#[test]
fn trusted_roots_admission_menu_project_not_admitted_when_outside() {
    let project_root = camino::Utf8Path::new("/other/myapp");
    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    assert!(
        !is_project_admitted_after_editor(project_root, &roots, None),
        "project outside trusted roots must not be admitted"
    );
}

#[test]
fn trusted_roots_admission_menu_non_interactive_remains_reject() {
    for decision in [TrustedRootDecision::NoTrustedRootsConfigured, outside()] {
        assert_eq!(
            resolve_trust_gate_action(&decision, false, false),
            TrustGateAction::Reject,
            "non-interactive admission must remain fail-closed"
        );
    }
}
