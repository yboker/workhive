#[allow(unused_imports)]
use std::collections::{BTreeMap, HashMap};

#[allow(unused_imports)]
use camino::Utf8PathBuf;

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{
    CommandSpec, MaterializationKind, PaneDirection, PaneLaunchIntent, PaneLaunchRef,
    ReattachOutcome, RuntimeBackendId, SessionAvailability, ToolCatalog, ToolName, WorkspaceId,
    WorkspaceOpenIntent, WorkspaceProvenance, WorkspaceProvider, classify_reattach_outcome,
};
#[allow(unused_imports)]
use crate::errors::SnapshotError;
#[allow(unused_imports)]
use crate::ports::{
    CreateWorkspaceRuntimeRequest, ExecutableAvailabilityPort, PaneRuntimePort, ProvisionError,
    ProvisionOutcome, ProvisionWorkspaceRequest, RuntimeError, SendInputRequest,
    SnapshotPaneRuntimePort, SnapshotRuntimePort, SnapshotStorePort, WorkspaceFsPort,
    WorkspaceProvisionPort, WorkspaceRuntimePort,
};

/// Cwd correction policy for a single restore pane.
///
/// Newly created panes (`create_window` / `split_pane`) already start in the
/// captured cwd through the backend's native `-c <dir>` facility, so they
/// pass `Skip`. Reused panes (the existing tmux session's residual panes)
/// must go through the infra-only respawn path; passing `Repair` instructs
/// the restore step to call [`crate::ports::SnapshotRuntimePort::respawn_unit_with_cwd`]
/// before any tool relaunch — never a visible `cd -- ...` keystroke.
#[derive(Clone, Copy)]
pub(crate) enum ReusedPaneCwdRepair<'a> {
    /// Pane was created during this restore; its cwd is already correct.
    Skip,
    /// Pane is being reused. If the observed cwd differs from the captured
    /// cwd, respawn the pane in `cwd` via `respawn_unit_with_cwd` BEFORE any
    /// launch command is installed.
    ///
    /// When a `Tool` launch follows, the typed
    /// `respawn_unit_with_command` carries its own `-c <cwd>` and the prior
    /// shell-cwd repair is harmless (it only realigns the residual shell
    /// before the tool replaces it). When a `Raw` (user inline) launch
    /// follows, the cwd repair is what keeps the original
    /// shell-retained semantics intact: the inline command is then typed
    /// into the (now correctly placed) shell via `send_input`, leaving the
    /// pane alive after the command exits.
    Repair {
        session: &'a str,
        observed_cwd: Option<&'a camino::Utf8Path>,
    },
}

/// Typed request bundle for `restore_runtime_pane` to satisfy the
/// `clippy::too_many_arguments` policy. The bundle is a transient,
/// crate-internal value type that groups the runtime context, target
/// identity, launch description, and observation seams required to
/// restore a single pane. It does not change call semantics or
/// behavior; every field corresponds 1:1 to the prior positional
/// argument of the same name and type.
pub(crate) struct RestoreRuntimePaneRequest<'a> {
    pub(crate) pane_runtime: &'a dyn PaneRuntimePort,
    pub(crate) snapshot_runtime: &'a dyn crate::ports::SnapshotRuntimePort,
    pub(crate) server: &'a str,
    pub(crate) session: &'a str,
    pub(crate) unit_id: crate::domain::UnitId,
    pub(crate) pane_index: u32,
    pub(crate) cwd: &'a camino::Utf8PathBuf,
    pub(crate) launch_intent: &'a PaneLaunchIntent,
    pub(crate) title: Option<&'a str>,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) exec_checker: &'a dyn ExecutableAvailabilityPort,
    pub(crate) cwd_repair: ReusedPaneCwdRepair<'a>,
    pub(crate) instrumentation: Option<&'a dyn crate::ports::ToolLaunchInstrumentationPort>,
}

pub(crate) fn restore_runtime_pane(request: RestoreRuntimePaneRequest<'_>) -> Result<(), String> {
    let RestoreRuntimePaneRequest {
        pane_runtime,
        snapshot_runtime,
        server,
        session,
        unit_id,
        pane_index,
        cwd,
        launch_intent,
        title,
        catalog,
        exec_checker,
        cwd_repair,
        instrumentation,
    } = request;
    // Keep Workhive launch identity stable across restores. Runtime-captured
    // pane titles are metadata and must not override config-derived labels.
    let pane_title = preferred_restore_title(title, launch_intent);
    if let Some(pane_title) = pane_title
        && let Err(e) = pane_runtime.set_unit_title(crate::ports::SetUnitTitleRequest {
            group: server.to_owned(),
            target_unit: unit_id.clone(),
            title: pane_title,
        })
    {
        tracing::warn!(error = %e, unit_id = %unit_id, "failed to restore pane title");
    }

    let launch_cmd = restore_command_from_launch_intent(launch_intent, catalog, exec_checker)?;

    // Narrow visible-keystroke policy:
    //
    // * `ToolCommand` is wrapped by the supervisor instrumentation port
    //   and installed via the typed snapshot-runtime verb
    //   (`respawn_unit_with_command` → `tmux respawn-pane -k -c <cwd>
    //   <command>`). The internal `workhive internal pane-supervisor ...`
    //   wrapper MUST NOT be typed into the pane shell — it would be
    //   visible in pane scrollback.
    //   Tools become the pane's primary process under the supervisor;
    //   that is the supervisor's documented fail-closed contract.
    //
    // * `Raw` (user inline command, captured from a snapshot's
    //   `LaunchCapture::Exec`) is INTENTIONALLY preserved at the
    //   `send_input` path. It is the user's own command, the ECHO is
    //   normal, and most importantly the pane shell remains alive after
    //   the command exits — re-routing inline commands through
    //   `respawn-pane -k` would silently switch the pane to a primary
    //   process that closes the pane on exit, changing user-facing
    //   lifecycle semantics. See
    //   `restore_runtime_pane_installs_inline_command_via_send_input_preserving_user_visibility`.
    //
    // * `None` (`EmptyShell`) installs nothing.
    match launch_cmd {
        RestoreLaunchCommand::None => {
            // No command to install. Fall through to the optional
            // keystroke-free cwd repair below for reused panes.
        }
        RestoreLaunchCommand::Raw(cmd) => {
            // Reused pane: realign cwd FIRST via the keystroke-free
            // respawn so the subsequent `send_input` types into the
            // correct directory. Newly created panes pass `Skip` and
            // already start in the captured cwd.
            apply_cwd_repair_if_needed(snapshot_runtime, server, &unit_id, cwd, cwd_repair)?;
            pane_runtime
                .send_input(SendInputRequest {
                    group: server.to_owned(),
                    target_unit: unit_id.clone(),
                    text: cmd,
                    enter: true,
                })
                .map_err(|e| format!("send_input failed for pane {pane_index}: {e}"))?;
            return Ok(());
        }
        RestoreLaunchCommand::ToolCommand {
            tool_name,
            program,
            args,
            program_path,
        } => {
            // Catalog tools MUST run under the supervisor — that is the
            // only mechanism that keeps `Source::Supervisor` flowing as
            // the highest-confidence observation source after restore.
            // If the instrumentation port is missing or returns an
            // error, restore fails closed: we do NOT silently launch an
            // unwrapped tool.
            let Some(instrumentation) = instrumentation else {
                return Err(format!(
                    "restore pane {pane_index}: tool launch instrumentation \
                     unavailable; refusing to launch unsupervised tool '{tool_name}'"
                ));
            };
            let request = crate::ports::WrapToolCommandRequest {
                group: server.to_owned(),
                workspace: session.to_owned(),
                unit_id: unit_id.clone(),
                tool_name: Some(tool_name.clone()),
                program_path,
                program,
                args,
            };
            let wrapped = match instrumentation.wrap_tool_command(request) {
                Ok(wrapped) => wrapped,
                Err(err) => {
                    tracing::error!(
                        error = %err,
                        unit_id = %unit_id,
                        "tool launch instrumentation failed during restore; \
                         refusing to launch unsupervised tool"
                    );
                    return Err(format!(
                        "restore pane {pane_index}: tool launch instrumentation \
                         failed for '{tool_name}': {err}"
                    ));
                }
            };
            // The wrapped supervisor command is installed via the typed
            // verb so the wrapper text never appears in pane scrollback.
            // `respawn_unit_with_command` carries its own `-c <cwd>`
            // which subsumes any pending cwd repair.
            snapshot_runtime
                .respawn_unit_with_command(server, session, &unit_id, cwd.as_path(), &wrapped)
                .map_err(|e| {
                    format!(
                        "respawn_unit_with_command failed for pane {pane_index} \
                         (cwd={cwd}): {e}"
                    )
                })?;
            return Ok(());
        }
    }

    // No launch command was installed (EmptyShell). For reused panes
    // whose live cwd drifted from the captured snapshot, realign via the
    // keystroke-free `respawn_unit_with_cwd` path.
    apply_cwd_repair_if_needed(snapshot_runtime, server, &unit_id, cwd, cwd_repair)?;

    Ok(())
}

/// Apply the reused-pane cwd repair when it is required.
///
/// Returns `Ok(())` on `Skip`, on `Repair` with matching observed cwd,
/// and on a successful `respawn_unit_with_cwd`. Any backend error is
/// surfaced as a typed string error to the caller.
pub(crate) fn apply_cwd_repair_if_needed(
    snapshot_runtime: &dyn crate::ports::SnapshotRuntimePort,
    server: &str,
    unit_id: &crate::domain::UnitId,
    cwd: &camino::Utf8PathBuf,
    cwd_repair: ReusedPaneCwdRepair<'_>,
) -> Result<(), String> {
    if let ReusedPaneCwdRepair::Repair {
        session,
        observed_cwd,
    } = cwd_repair
    {
        let needs_repair = match observed_cwd {
            Some(observed) => observed != cwd.as_path(),
            None => true,
        };
        if needs_repair {
            snapshot_runtime
                .respawn_unit_with_cwd(server, session, unit_id, cwd.as_path())
                .map_err(|e| {
                    format!(
                        "respawn_unit_with_cwd failed for reused pane {unit}: {e}",
                        unit = unit_id.as_str(),
                    )
                })?;
        }
    }
    Ok(())
}

/// Outcome of resolving a `PaneLaunchIntent` into something restorable.
///
/// `ToolCommand` carries the typed metadata callers MUST forward to
/// [`crate::ports::ToolLaunchInstrumentationPort::wrap_tool_command`].
/// Tool launches always run wrapped — the supervisor binding is the
/// only mechanism that produces `Source::Supervisor` observations,
/// and `restore_runtime_pane` fails closed if wrapping is
/// unavailable. `Raw` is a pre-formatted shell string for non-tool
/// paths (inline commands today) where supervisor wrapping does not
/// apply. `None` means the pane has no launch command.
pub(crate) enum RestoreLaunchCommand {
    None,
    Raw(String),
    ToolCommand {
        tool_name: String,
        program: String,
        args: Vec<String>,
        program_path: Option<String>,
    },
}

pub(crate) fn restore_command_from_launch_intent(
    launch: &PaneLaunchIntent,
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> Result<RestoreLaunchCommand, String> {
    match launch {
        PaneLaunchIntent::EmptyShell => Ok(RestoreLaunchCommand::None),
        PaneLaunchIntent::InlineCommand { program, args } => {
            // Heal a quote-polluted known-tool inline command before it
            // reaches the visible `send_input` path. A configured tool
            // launch persisted as `"\"cursor-agent"` + `["--yolo\""]`
            // (matching outer double quotes scattered across the slots)
            // is tokenization pollution, not a user-authored inline
            // command; replaying it raw makes the shell look for an
            // executable literally named `"cursor-agent`. The shared
            // classifier recovers the canonical tool with high
            // confidence, and we restore it through the typed tool
            // launch path so it never appears as raw keystrokes.
            if let crate::usecases::runtime_start_command::StartCommandClassification::Tool {
                name,
            } = crate::usecases::runtime_start_command::classify_inline_command(program, args)
            {
                return restore_tool_command(name.as_str(), catalog, exec_checker);
            }
            Ok(RestoreLaunchCommand::Raw(
                CommandSpec::new(program, args).to_string(),
            ))
        }
        PaneLaunchIntent::ToolRef { tool_name } => {
            restore_tool_command(tool_name, catalog, exec_checker)
        }
        PaneLaunchIntent::NonRestorable { display, reason } => {
            Err(format!("non-restorable process '{display}': {reason}"))
        }
    }
}

/// Resolve a catalog tool name into a typed [`RestoreLaunchCommand::ToolCommand`].
///
/// Tool launches always run wrapped through the supervisor (see
/// [`RestoreLaunchCommand`]); a degraded resolution is a restore error
/// rather than an unsupervised fallback.
pub(crate) fn restore_tool_command(
    tool_name: &str,
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> Result<RestoreLaunchCommand, String> {
    let name = ToolName::new(tool_name.to_owned())
        .map_err(|e| format!("invalid tool reference '{tool_name}': {e}"))?;
    let resolved = crate::usecases::launch_resolver::resolve_launch(
        &PaneLaunchRef::Tool { name },
        catalog,
        exec_checker,
    );
    match resolved {
        crate::domain::ResolvedPaneLaunch::Executable {
            program,
            args,
            program_path,
            ..
        } => Ok(RestoreLaunchCommand::ToolCommand {
            tool_name: tool_name.to_owned(),
            program,
            args,
            program_path,
        }),
        degraded => Err(format!(
            "tool restore degraded for '{}': {}",
            tool_name,
            degraded
                .degradation_reason()
                .unwrap_or_else(|| "unknown degrade".to_owned())
        )),
    }
}

pub(crate) fn snapshot_title_from_launch_intent(launch: &PaneLaunchIntent) -> Option<String> {
    match launch {
        PaneLaunchIntent::ToolRef { tool_name } => Some(tool_name.clone()),
        PaneLaunchIntent::InlineCommand { program, .. } => {
            Some(program.rsplit('/').next().unwrap_or(program).to_owned())
        }
        PaneLaunchIntent::EmptyShell => Some("shell".to_owned()),
        PaneLaunchIntent::NonRestorable { display, .. } => Some(display.clone()),
    }
}

/// Determine the stable Workhive-owned pane title for a restored pane.
///
/// Priority: config/launch identity always wins over snapshot-captured runtime
/// title. This prevents programs that set `pane_title` via ANSI escapes from
/// polluting the next snapshot restore's display identity. The returned title
/// is written to tmux via `set_unit_title` / `select-pane -T`, making Workhive
/// the sole owner of `#{pane_title}` in tmux format strings.
pub(crate) fn preferred_restore_title(
    title: Option<&str>,
    launch: &PaneLaunchIntent,
) -> Option<String> {
    snapshot_title_from_launch_intent(launch).or_else(|| title.map(str::to_owned))
}
