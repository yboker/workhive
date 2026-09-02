//! Command-layer helpers for `workhive pane send`.
//!
//! This module owns every piece of `pane send` logic that lives above
//! the control-plane dispatcher and below the CLI argument parser:
//!
//!   * the typed view-model used by the dispatcher to talk about a
//!     resolved `pane send` target ([`ResolvedPaneTarget`]);
//!   * the localized confirmation and success messages that wrap that
//!     view-model;
//!   * the pure target-resolution routine that maps a raw `--pane`
//!     argument (numeric index or `%target_id`) onto a canonical
//!     [`UnitId`] ([`resolve_pane_target`]);
//!   * the self-target rejection guard that blocks `pane send` from
//!     delivering text into the user's own shell
//!     ([`enforce_not_self_target`]);
//!   * the conservative ACP read helper that converts ACP-store errors
//!     into a typed [`GuardedSendBlockReason`]
//!     ([`read_pane_acp_events_or_block_reason`]);
//!   * the guarded-send plan composer that joins the layered pane
//!     observation stack, the ACP read model, the freshness thresholds,
//!     and the typed core resolver to produce a typed [`PaneSendPlan`]
//!     or a localized refusal/warning ([`plan_pane_send_guarded`]).
//!
//! Responsibility boundary:
//!
//!   * This module owns guarded-send composition and the ACP read that
//!     feeds it. It is the one place command-layer code touches the
//!     ACP store on the `pane send` path.
//!   * It does NOT own runtime delivery: actually pushing keys/text
//!     into a pane goes through `pane_control_plane::dispatch_pane_send`,
//!     and the dispatcher in `commands/pane.rs` chooses when to call
//!     that helper after the guarded plan and the user confirmation
//!     have resolved.
//!   * It depends on the CLI's i18n context, the existing core port
//!     types ([`RuntimeUnitSnapshot`], [`UnitId`], [`ActivityEventStore`]),
//!     the shared view-data helpers ([`load_tool_catalog`]), and the
//!     activity-overlay clock helper ([`now_unix_ms_from`]).
//!
//! The module intentionally uses explicit imports rather than a glob
//! re-export from its parent so the symbol surface it actually depends
//! on is visible at the top of the file and stable across future
//! moves.

use crate::commands::pane_activity_overlay::now_unix_ms_from;
use crate::commands::pane_view_data::load_tool_catalog;
use crate::context::CommandContext;
use crate::error::CliError;
use camino::Utf8Path;
use workhive_core::domain::activity::{ActivityEvent, PaneCorrelationKey, ReadModel};
use workhive_core::domain::{Locale, PaneRuntimeTruth, UnitId};
use workhive_core::i18n_keys as k;
use workhive_core::ports::{ActivityEventStore, I18nPort, RuntimeUnitSnapshot};
use workhive_core::usecases::activity_control_plane::metrics::FreshnessThresholds;
use workhive_core::usecases::orchestration_primitives::GuardedSendBlockReason;
use workhive_core::usecases::pane_observation::gather_pane_truths;
use workhive_core::usecases::pane_send::{
    PaneSendDecision, PaneSendPlan, resolve_pane_send_plan_guarded,
};

/// Resolved view-model for a `pane send --pane <RAW>` argument.
///
/// `input` is preserved verbatim from the CLI so the localized
/// confirmation and success messages can show the user *exactly* what
/// they typed (e.g. the numeric index `2`) alongside the canonical
/// runtime identifier the system resolved it to (e.g. `%2`). The
/// dispatcher must use [`Self::unit_id`] as the actual target for any
/// control-plane delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedPaneTarget {
    pub(crate) input: String,
    pub(crate) unit_id: UnitId,
}

impl ResolvedPaneTarget {
    /// Localized confirmation prompt shown before the send is dispatched.
    ///
    /// When the user typed a value that already matches the canonical
    /// `UnitId` (the `%target_id` form) the message only mentions the
    /// canonical id. When the user typed a numeric index that resolved
    /// to a different canonical id, the message shows both the typed
    /// input and the resolved target so the operator can verify the
    /// mapping before confirming.
    pub(crate) fn confirm_message(&self, ctx: &CommandContext, locale: Locale) -> String {
        if self.input == self.unit_id.as_str() {
            ctx.i18n
                .resolve_with_args(locale, k::PANE_CONFIRM_SEND, &[self.unit_id.as_str()])
        } else {
            ctx.i18n.resolve_with_args(
                locale,
                k::PANE_CONFIRM_SEND_RESOLVED,
                &[self.input.as_str(), self.unit_id.as_str()],
            )
        }
    }

    /// Localized success message printed after a successful dispatch.
    ///
    /// Mirrors the mapping logic of [`Self::confirm_message`]: same
    /// canonical-only / resolved-mapping pair, different i18n keys.
    pub(crate) fn success_message(&self, ctx: &CommandContext, locale: Locale) -> String {
        if self.input == self.unit_id.as_str() {
            ctx.i18n
                .resolve_with_args(locale, k::PANE_SEND_SUCCESS, &[self.unit_id.as_str()])
        } else {
            ctx.i18n.resolve_with_args(
                locale,
                k::PANE_SEND_SUCCESS_RESOLVED,
                &[self.input.as_str(), self.unit_id.as_str()],
            )
        }
    }
}

/// Reject a `pane send` target that resolves to the pane currently
/// running the command. Sending to the current pane would deliver
/// text into the user's own shell and is rejected before confirmation
/// and before any control-plane dispatch.
///
/// The check compares canonical runtime [`UnitId`] values, so a numeric
/// pane index (e.g. `1`) and a tmux target id (e.g. `%2`) are both
/// rejected when they resolve to the current pane.
pub(crate) fn enforce_not_self_target(
    target: &ResolvedPaneTarget,
    current: &UnitId,
    ctx: &CommandContext,
    locale: Locale,
) -> Result<(), CliError> {
    if target.unit_id.as_str() == current.as_str() {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::PANE_TARGET_SELF_REJECTED),
        ));
    }
    Ok(())
}

/// Resolve a raw `--pane <RAW>` argument onto a canonical
/// [`ResolvedPaneTarget`].
///
/// Three input shapes are accepted:
///
///   * a non-empty string starting with `%` — passed through verbatim
///     as the canonical [`UnitId`], because tmux target ids are already
///     canonical runtime identifiers and may name a pane that is not in
///     the current `panes` snapshot;
///   * a numeric index (`0`, `1`, `2`, ...) — looked up in `panes` by
///     [`RuntimeUnitSnapshot::index`] and mapped onto the canonical
///     [`UnitId`] of the matching pane;
///   * anything else — rejected with a typed, localized error.
///
/// Empty / whitespace-only input is rejected with the same invalid-text
/// error so the CLI never delivers a send into an ambiguous target.
pub(crate) fn resolve_pane_target(
    raw: &str,
    panes: &[RuntimeUnitSnapshot],
    ctx: &CommandContext,
) -> Result<ResolvedPaneTarget, CliError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            ctx.locale(),
            k::PANE_TARGET_INVALID,
            &[raw],
        )));
    }
    if trimmed.starts_with('%') {
        return Ok(ResolvedPaneTarget {
            input: raw.to_owned(),
            unit_id: UnitId::new(trimmed.to_owned()),
        });
    }
    if let Ok(index) = trimmed.parse::<u32>() {
        let Some(pane) = panes.iter().find(|pane| pane.index == index) else {
            return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                ctx.locale(),
                k::PANE_TARGET_INDEX_NOT_FOUND,
                &[trimmed],
            )));
        };
        return Ok(ResolvedPaneTarget {
            input: raw.to_owned(),
            unit_id: pane.id.clone(),
        });
    }
    Err(CliError::Runtime(ctx.i18n.resolve_with_args(
        ctx.locale(),
        k::PANE_TARGET_INVALID,
        &[raw],
    )))
}

/// Production mapping for the conservative ACP read used by `pane send`.
///
/// Distinguishes three outcomes:
///
/// 1. `Ok(events)` — the store responded successfully. An empty vector
///    means "store healthy, no facts for this pane" and the caller may
///    legitimately fall through to the legacy path.
/// 2. `Err(GuardedSendBlockReason::AcpUnavailable)` — the store
///    surfaced an `Io` / `Unavailable` / `CorruptEntry` error. The caller
///    MUST refuse the send rather than silently bypassing the activity
///    guard. The error is logged at `warn` here so the production
///    logging policy in the logging policy is satisfied without each caller
///    re-implementing it.
///
/// This is a pure function over the [`ActivityEventStore`] port so the
/// behaviour can be exercised against a real `FsActivityEventStore` with
/// a corrupt JSONL log without spinning up tmux or the supervisor.
pub(crate) fn read_pane_acp_events_or_block_reason<S>(
    store: &S,
    pane: &PaneCorrelationKey,
) -> Result<Vec<ActivityEvent>, GuardedSendBlockReason>
where
    S: ActivityEventStore + ?Sized,
{
    match store.events_for_pane(pane) {
        Ok(events) => Ok(events),
        Err(err) => {
            tracing::warn!(
                target: "workhive.activity_control_plane",
                pane_group = %pane.group,
                pane_workspace = %pane.workspace,
                pane_unit_id = %pane.unit_id,
                error = %err,
                "activity event store read failed; refusing guarded send conservatively"
            );
            Err(GuardedSendBlockReason::AcpUnavailable)
        }
    }
}

/// Compose the guarded `pane send` plan or surface a typed refusal.
///
/// This wraps the full "structured pane-send safety boundary" pipeline
/// previously inlined in `commands/pane.rs`:
///
///   * builds a [`PaneRuntimeTruth`] for the resolved target via the
///     layered observation stack (supervisor → process observer → tmux
///     fallback) using the same shared tool catalog as `pane list` and
///     `pane show`;
///   * reads ACP facts for the target pane through
///     [`read_pane_acp_events_or_block_reason`] so an ACP read error
///     blocks the send conservatively instead of silently downgrading to
///     the legacy path;
///   * derives pane metrics with the conservative freshness thresholds
///     and feeds them, alongside the truth's `message_safety`, into the
///     core resolver [`resolve_pane_send_plan_guarded`];
///   * translates the typed [`PaneSendDecision`] into one of:
///       - `Ok(PaneSendPlan)` — caller should run the existing confirm
///         flow and dispatch;
///       - `Ok(PaneSendPlan)` with a localized stderr warning already
///         printed (`AllowWithWarning`);
///       - `Err(CliError::Runtime(_))` for both ordinary refusals and
///         activity-guard refusals, using the same i18n keys and message
///         shapes the inline implementation used.
///
/// All side effects (stderr warning print, structured `tracing::warn`
/// on ACP read failure) match the previous inline behaviour exactly.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_pane_send_guarded(
    ctx: &CommandContext,
    locale: Locale,
    project_root: &Utf8Path,
    server: &str,
    workspace: &str,
    session: &str,
    panes: &[RuntimeUnitSnapshot],
    target: &ResolvedPaneTarget,
    text: &str,
    no_enter: bool,
    force: bool,
) -> Result<PaneSendPlan, CliError> {
    let target_snapshot = panes
        .iter()
        .find(|p| p.id == target.unit_id)
        .cloned()
        .ok_or_else(|| {
            CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::PANE_TARGET_INVALID,
                &[target.input.as_str()],
            ))
        })?;
    let supervisor = ctx.composition.pane_supervisor(project_root);
    let observer = ctx.composition.process_observer();
    let tools = load_tool_catalog(ctx, project_root);
    let truths = gather_pane_truths(
        std::slice::from_ref(&target_snapshot),
        Some(&supervisor),
        Some(observer.as_ref()),
        server,
        session,
        &tools,
    );
    let truth = truths
        .into_iter()
        .next()
        .unwrap_or_else(PaneRuntimeTruth::unknown);

    // Activity Intelligence v1 (D2): derive ACP metrics for the target
    // pane and route through `resolve_pane_send_plan_guarded` so the
    // activity guard can block sends to busy / stale / uncertain panes.
    // Passing `target_metrics = None` skips the guard (legacy-only
    // panes) per Agent D's design: the guard must not weaken existing
    // safety rules.
    let target_pane_key = PaneCorrelationKey::new(
        server.to_owned(),
        workspace.to_owned(),
        target.unit_id.as_str().to_string(),
    );
    let acp_event_store = ctx.composition.activity_event_store(project_root);
    let clock = workhive_infra::clock::SystemClock;
    let acp_now_ms = now_unix_ms_from(&clock);
    // ACP read failure must NOT silently bypass
    // the activity guard. The branching is centralized in
    // `read_pane_acp_events_or_block_reason` so it can be tested with a
    // real `FsActivityEventStore` against a corrupt on-disk log.
    let acp_events = match read_pane_acp_events_or_block_reason(&acp_event_store, &target_pane_key)
    {
        Ok(events) => events,
        Err(block_reason) => {
            let reason_str = format!("{block_reason:?}");
            return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::PANE_SEND_REFUSE_ACTIVITY_GUARD,
                &[target.input.as_str(), reason_str.as_str()],
            )));
        }
    };
    let acp_read_model = ReadModel::empty(acp_now_ms);
    let acp_thresholds = FreshnessThresholds::default_conservative();
    let target_metrics =
        acp_read_model.pane_metrics(&target_pane_key, &acp_events, acp_now_ms, acp_thresholds);
    // Only pass metrics if they come from the ACP path; legacy panes
    // get `None` so the guard is skipped (preserves legacy fallback
    // behaviour). NOTE: this branch is reached only on a *successful*
    // read — read errors return early above and never fall through to
    // the legacy-skip path.
    let maybe_metrics = if target_metrics.acp_source {
        Some(&target_metrics)
    } else {
        None
    };
    let decision = resolve_pane_send_plan_guarded(
        truth.message_safety,
        no_enter,
        force,
        false, // guard_force: not exposed as a CLI flag in this batch
        text,
        maybe_metrics,
    );
    match decision {
        PaneSendDecision::Refuse {
            reason_key,
            hint_key,
            ..
        } => {
            let mut msg = ctx
                .i18n
                .resolve_with_args(locale, reason_key, &[target.input.as_str()]);
            msg.push('\n');
            msg.push_str(&ctx.i18n.resolve(locale, hint_key));
            Err(CliError::Runtime(msg))
        }
        PaneSendDecision::AllowWithWarning {
            plan, warning_key, ..
        } => {
            eprintln!(
                "{}",
                ctx.i18n
                    .resolve_with_args(locale, warning_key, &[target.input.as_str()])
            );
            Ok(plan)
        }
        PaneSendDecision::Allow { plan, .. } => Ok(plan),
        PaneSendDecision::GuardedSendRefused { block_reason, .. } => {
            let reason_str = format!("{block_reason:?}");
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::PANE_SEND_REFUSE_ACTIVITY_GUARD,
                &[target.input.as_str(), reason_str.as_str()],
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ResolvedPaneTarget, enforce_not_self_target, read_pane_acp_events_or_block_reason,
        resolve_pane_target,
    };
    use crate::context::CommandContext;
    use crate::error::CliError;
    use camino::Utf8PathBuf;
    use workhive_core::domain::{Locale, PaneKind, PaneState, UnitId};
    use workhive_core::ports::RuntimeUnitSnapshot;

    fn test_context() -> CommandContext {
        CommandContext::new()
    }

    /// Minimal [`RuntimeUnitSnapshot`] builder for target-resolution
    /// tests. Only the fields read by [`resolve_pane_target`] (`id`,
    /// `index`) are semantically meaningful; the rest take neutral
    /// defaults so the structure stays valid without coupling the
    /// tests to unrelated pane-observation fields.
    fn unit(id: &str, index: u32) -> RuntimeUnitSnapshot {
        RuntimeUnitSnapshot {
            id: UnitId::new(id),
            index,
            cwd: Utf8PathBuf::from("/Users/test/project"),
            command: "zsh".to_owned(),
            active: false,
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            tty: None,
            process_pid: None,
        }
    }

    #[test]
    fn resolve_target_keeps_tmux_target_id() {
        let ctx = test_context();
        let resolved = resolve_pane_target("%2", &[unit("%2", 2)], &ctx).expect("resolve");
        assert_eq!(resolved.input, "%2");
        assert_eq!(resolved.unit_id.as_str(), "%2");
    }

    #[test]
    fn resolve_target_maps_numeric_index_to_unit_id() {
        let ctx = test_context();
        let panes = [unit("%0", 0), unit("%2", 2)];
        let resolved = resolve_pane_target("2", &panes, &ctx).expect("resolve");
        assert_eq!(resolved.input, "2");
        assert_eq!(resolved.unit_id.as_str(), "%2");
    }

    #[test]
    fn resolve_target_rejects_missing_numeric_index() {
        let ctx = test_context();
        let error = resolve_pane_target("4", &[unit("%2", 2)], &ctx).expect_err("missing index");
        assert!(matches!(error, CliError::Runtime(message) if message.contains('4')));
    }

    #[test]
    fn resolve_target_rejects_invalid_text() {
        let ctx = test_context();
        let error = resolve_pane_target("codex", &[unit("%2", 2)], &ctx).expect_err("invalid");
        assert!(matches!(error, CliError::Runtime(message) if message.contains("codex")));
    }

    #[test]
    fn enforce_not_self_target_rejects_numeric_input_pointing_at_current_en() {
        let ctx = test_context();
        let target = ResolvedPaneTarget {
            input: "2".to_owned(),
            unit_id: UnitId::new("%2"),
        };
        let current = UnitId::new("%2");
        let error = enforce_not_self_target(&target, &current, &ctx, Locale::En)
            .expect_err("self-send by numeric index must be rejected");
        let CliError::Runtime(message) = error else {
            panic!("expected CliError::Runtime, got {error:?}");
        };
        assert!(
            message.contains("current pane"),
            "expected localized self-target message, got {message:?}",
        );
    }

    #[test]
    fn enforce_not_self_target_rejects_target_id_pointing_at_current_zh() {
        let ctx = test_context();
        let target = ResolvedPaneTarget {
            input: "%3".to_owned(),
            unit_id: UnitId::new("%3"),
        };
        let current = UnitId::new("%3");
        let error = enforce_not_self_target(&target, &current, &ctx, Locale::ZhCn)
            .expect_err("self-send by tmux target id must be rejected");
        let CliError::Runtime(message) = error else {
            panic!("expected CliError::Runtime, got {error:?}");
        };
        assert!(
            message.contains("当前窗格"),
            "expected localized self-target message in zh, got {message:?}",
        );
    }

    #[test]
    fn enforce_not_self_target_allows_other_pane() {
        let ctx = test_context();
        let target = ResolvedPaneTarget {
            input: "1".to_owned(),
            unit_id: UnitId::new("%1"),
        };
        let current = UnitId::new("%2");
        enforce_not_self_target(&target, &current, &ctx, Locale::En)
            .expect("non-self target must pass");
    }

    /// When `pane send` is invoked with a numeric index (e.g. `2`) that
    /// resolves to a different canonical id (e.g. `%2`), both the
    /// confirmation prompt and the success message must show the
    /// mapping so the operator can verify they targeted the right pane.
    #[test]
    fn resolved_target_messages_show_mapping_when_input_was_index() {
        let ctx = test_context();
        let target = ResolvedPaneTarget {
            input: "2".to_owned(),
            unit_id: UnitId::new("%2"),
        };
        assert_eq!(
            target.confirm_message(&ctx, Locale::En),
            "Send text to pane 2 (resolved target %2)?"
        );
        assert_eq!(
            target.success_message(&ctx, Locale::En),
            "Text sent to pane 2 (resolved target %2)."
        );
    }

    /// Read-helper safety contract: a corrupt JSONL entry in the ACP
    /// log MUST surface as `Err(GuardedSendBlockReason::AcpUnavailable)`
    /// so the caller refuses the send. Previously the equivalent CLI
    /// path called `events_for_pane(...).unwrap_or_default()`, which
    /// silently downgraded any store error to an empty event list and
    /// let `pane send` bypass the activity guard. This exercises the
    /// production mapping helper (not a re-derivation of the same
    /// branching inside the test body), per the testing policy.
    #[test]
    fn pane_send_guard_refuses_when_acp_log_is_corrupt() {
        use camino::Utf8PathBuf;
        use std::fs;
        use std::io::Write;
        use tempfile::TempDir;
        use workhive_core::domain::activity::PaneCorrelationKey;
        use workhive_core::usecases::orchestration_primitives::GuardedSendBlockReason;
        use workhive_infra::activity_event_store::FsActivityEventStore;

        let tmp = TempDir::new().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf())
            .expect("tempdir path must be UTF-8");
        let store = FsActivityEventStore::new(root.clone());
        let pane = PaneCorrelationKey::new("grp", "ws", "u1");

        // Write a deliberately corrupt JSONL line so the store
        // surfaces `CorruptEntry` rather than `Ok(empty)`.
        let log_path = store.log_path(&pane);
        if let Some(parent) = log_path.as_std_path().parent() {
            fs::create_dir_all(parent).expect("create log parent dir");
        }
        let mut f = fs::File::create(log_path.as_std_path()).expect("create log file");
        writeln!(f, "{{not valid json").expect("write corrupt line");
        drop(f);

        let outcome = read_pane_acp_events_or_block_reason(&store, &pane);
        assert!(
            matches!(outcome, Err(GuardedSendBlockReason::AcpUnavailable)),
            "expected AcpUnavailable refusal on corrupt log, got {outcome:?}"
        );
    }

    /// A healthy empty store legitimately returns `Ok(vec![])` —
    /// "no events yet" is **not** an ACP read failure. The guard
    /// caller will then map empty events through `pane_metrics(...)`
    /// and find `acp_source = false`, which is the correct
    /// legacy-fallback signal (a pane that has not yet emitted any
    /// ACP event is indistinguishable from a legacy pane). This test
    /// pins that semantic so a future "block on empty store too"
    /// change has to be explicit.
    #[test]
    fn pane_send_guard_treats_empty_store_as_no_facts_not_unavailable() {
        use camino::Utf8PathBuf;
        use tempfile::TempDir;
        use workhive_core::domain::activity::PaneCorrelationKey;
        use workhive_infra::activity_event_store::FsActivityEventStore;

        let tmp = TempDir::new().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf())
            .expect("tempdir path must be UTF-8");
        let store = FsActivityEventStore::new(root);
        let pane = PaneCorrelationKey::new("grp", "ws", "u1");

        let outcome = read_pane_acp_events_or_block_reason(&store, &pane);
        match outcome {
            Ok(events) => assert!(
                events.is_empty(),
                "empty store must produce an empty event vec"
            ),
            Err(reason) => panic!("empty store must not produce a guard refusal, got {reason:?}"),
        }
    }
}
