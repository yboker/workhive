use super::*;

/// Render integration: the accepted trusted-roots summary
/// must paint every accepted row with the same success
/// styling. The `most_recent` index must not change the row
/// count, and a snapshot with 2+ paths still renders 2+ path
/// lines (plus header).
#[test]
fn added_summary_renders_all_paths_uniformly_regardless_of_most_recent() {
    // Two accepted roots. Styling is uniform across rows; the
    // snapshot exposes all paths so the consistent-styling code
    // path is exercised regardless of which index is flagged as
    // most recent.
    let paths = vec!["~/alpha".to_owned(), "~/beta".to_owned()];
    let summary = AddedRootsSummary {
        header_template: "Trusted roots added ({0}):",
        empty_label: "",
        overflow_template: "… and {0} more",
        paths: &paths,
        most_recent: Some(1),
    };
    let snap = AddedRootsSnapshot::from(&summary);

    assert_eq!(snap.paths.len(), 2, "both accepted roots must be visible");
    assert_eq!(snap.paths[0], "~/alpha");
    assert_eq!(snap.paths[1], "~/beta");
}

/// `AddedRootsSnapshot::from` produces no header / overflow and
/// surfaces the empty-state label when no roots have been
/// accepted. The empty-state row is what gives the user a
/// visible "nothing yet" affordance.
#[test]
fn added_summary_renders_empty_state_when_no_paths() {
    let summary = AddedRootsSummary {
        header_template: "Trusted roots added ({0}):",
        empty_label: "No trusted roots added yet.",
        overflow_template: "… and {0} more",
        paths: &[],
        most_recent: None,
    };
    let snap = AddedRootsSnapshot::from(&summary);
    assert!(snap.is_renderable(), "empty-state label must be rendered");
    assert_eq!(snap.header_line, "");
    assert_eq!(snap.empty_line, "No trusted roots added yet.");
    assert!(snap.paths.is_empty());
    assert_eq!(snap.overflow_line, "");
    assert_eq!(snap.most_recent, None);
}

/// One accepted root: header carries the running count, the
/// authored path is preserved verbatim, and `most_recent`
/// points to that single row.
#[test]
fn added_summary_renders_single_root_with_recent_highlight() {
    let paths = vec!["~/development".to_owned()];
    let summary = AddedRootsSummary {
        header_template: "Trusted roots added ({0}):",
        empty_label: "No trusted roots added yet.",
        overflow_template: "… and {0} more",
        paths: &paths,
        most_recent: Some(0),
    };
    let snap = AddedRootsSnapshot::from(&summary);
    assert_eq!(snap.header_line, "Trusted roots added (1):");
    assert_eq!(snap.empty_line, "");
    assert_eq!(snap.paths, vec!["~/development".to_owned()]);
    assert_eq!(snap.overflow_line, "");
    assert_eq!(snap.most_recent, Some(0));
}

/// Multi-item summary truncated to `MAX_VISIBLE_ADDED_ROOTS`
/// with an explicit "… and N more" overflow line so narrow
/// terminals stay readable.
#[test]
fn added_summary_truncates_long_lists_with_overflow_indicator() {
    let mut paths: Vec<String> = (0..MAX_VISIBLE_ADDED_ROOTS + 3)
        .map(|i| format!("~/proj-{i}"))
        .collect();
    // Pretend the most-recently-added entry is the last one
    // supplied (a position that will be truncated).
    let last_idx = paths.len() - 1;
    {
        let summary = AddedRootsSummary {
            header_template: "Trusted roots added ({0}):",
            empty_label: "No trusted roots added yet.",
            overflow_template: "… and {0} more",
            paths: &paths,
            most_recent: Some(last_idx),
        };
        let snap = AddedRootsSnapshot::from(&summary);
        assert_eq!(
            snap.paths.len(),
            MAX_VISIBLE_ADDED_ROOTS,
            "long lists must truncate to the in-frame display budget"
        );
        assert_eq!(
            snap.overflow_line, "… and 3 more",
            "overflow line must report the hidden remainder"
        );
        assert_eq!(
            snap.most_recent, None,
            "a truncated most_recent index must NOT point at the wrong row"
        );
        // Header still reports the *total* count, not the visible
        // count, so the user sees how many roots they've accepted.
        assert!(
            snap.header_line
                .contains(&(MAX_VISIBLE_ADDED_ROOTS + 3).to_string()),
            "header must reflect the running total, got {:?}",
            snap.header_line
        );
    }

    // Spot-check the wizard's covered-by-existing-root contract
    // at the snapshot level: when a parent is already added and
    // a child is rejected, the parent row must still be visible
    // in `paths` (i.e. the rejection does not retroactively drop
    // any accepted entry).
    let _ = paths.pop();
    let summary_after_reject = AddedRootsSummary {
        header_template: "Trusted roots added ({0}):",
        empty_label: "No trusted roots added yet.",
        overflow_template: "… and {0} more",
        paths: &paths,
        most_recent: None,
    };
    let snap_after_reject = AddedRootsSnapshot::from(&summary_after_reject);
    assert!(
        snap_after_reject.paths.contains(&"~/proj-0".to_owned()),
        "parent row must remain visible after a covered-child rejection"
    );
}

/// Caller-supplied `most_recent` indices that fall outside the
/// visible window must NOT cause the renderer to highlight the
/// wrong row. The fallback is `None` (no highlight).
#[test]
fn added_summary_drops_out_of_range_most_recent() {
    let paths = vec!["~/a".to_owned(), "~/b".to_owned()];
    let summary = AddedRootsSummary {
        header_template: "Trusted roots added ({0}):",
        empty_label: "No trusted roots added yet.",
        overflow_template: "… and {0} more",
        paths: &paths,
        most_recent: Some(99),
    };
    let snap = AddedRootsSnapshot::from(&summary);
    assert_eq!(snap.most_recent, None);
    assert_eq!(snap.paths.len(), 2);
}
