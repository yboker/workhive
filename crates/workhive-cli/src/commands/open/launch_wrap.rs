#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

/// Compute the stable Workhive-owned pane title for a freshly launched pane.
///
/// This title is written to tmux via `set_unit_title` / `select-pane -T`,
/// making Workhive the authoritative owner of `#{pane_title}`. Degraded panes
/// include a `(degraded)` suffix so users can distinguish them visually.
pub(crate) fn pane_title_for_launch(
    launch: &workhive_core::domain::PaneLaunchRef,
    resolved: &ResolvedPaneLaunch,
) -> String {
    if resolved.is_degraded() {
        format!("{} (degraded)", launch.display_name())
    } else {
        launch.display_name().to_owned()
    }
}

/// Wrap a tool launch command with the pane supervisor invocation when
/// the launch targets a catalog `Tool`, otherwise pass the command
/// through unchanged.
///
/// ## Fail-closed contract for `PaneLaunchRef::Tool`
///
/// Catalog tools MUST run under the supervisor — that is the only
/// mechanism that produces `Source::Supervisor` observations, the
/// highest-confidence truth source consumed by the merge policy. If
/// the instrumentation port is missing or returns an error for a
/// `Tool` launch, this function fails closed: it returns
/// `Err(CliError::Runtime(_))`, the caller must propagate the error,
/// and `apply_layout` MUST NOT silently fall back to an un-wrapped
/// tool command — an un-wrapped tool would never write supervisor
/// metadata, permanently degrading pane observation for that pane.
///
/// ## Pass-through for shells and inline commands
///
/// `PaneLaunchRef::EmptyShell` and `PaneLaunchRef::InlineCommand`
/// bypass the wrapper because they are not first-class catalog tools.
/// Workhive cannot meaningfully label supervisor metadata for an
/// arbitrary user shell snippet, and the merge policy already
/// accommodates a missing supervisor signal for these panes by
/// falling through to the OS observer / tmux current command.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wrap_with_supervisor_or_passthrough(
    instrumentation: Option<&dyn workhive_core::ports::ToolLaunchInstrumentationPort>,
    server: &str,
    session: &str,
    unit_id: &workhive_core::domain::UnitId,
    launch_ref: &workhive_core::domain::PaneLaunchRef,
    program: &str,
    args: &[String],
    program_path: Option<&str>,
    fallback_command: &str,
) -> Result<String, CliError> {
    use workhive_core::domain::PaneLaunchRef;
    use workhive_core::ports::WrapToolCommandRequest;

    let tool_name = match launch_ref {
        PaneLaunchRef::Tool { name } => name.as_str().to_owned(),
        PaneLaunchRef::InlineCommand { .. } | PaneLaunchRef::EmptyShell => {
            return Ok(fallback_command.to_owned());
        }
    };

    let Some(instrumentation) = instrumentation else {
        return Err(CliError::Runtime(format!(
            "tool launch instrumentation unavailable; refusing to launch \
             unsupervised tool '{tool_name}' (unit {unit_id})"
        )));
    };

    let request = WrapToolCommandRequest {
        group: server.to_owned(),
        workspace: session.to_owned(),
        unit_id: unit_id.clone(),
        tool_name: Some(tool_name.clone()),
        program_path: program_path.map(str::to_owned),
        program: program.to_owned(),
        args: args.to_vec(),
    };
    match instrumentation.wrap_tool_command(request) {
        Ok(wrapped) => Ok(wrapped),
        Err(err) => {
            tracing::error!(
                error = %err,
                unit = %unit_id,
                tool = %tool_name,
                "tool launch instrumentation failed; refusing to launch unsupervised tool"
            );
            Err(CliError::Runtime(format!(
                "tool launch instrumentation failed for '{tool_name}' (unit {unit_id}): {err}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::super::test_support::*;
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::domain::{CommandSpec, PaneLaunchRef, ToolCatalog, ToolName};
    #[allow(unused_imports)]
    use workhive_core::ports::{
        ColumnConfig, CreateWorkspaceRuntimeRequest, LayoutConfig, LayoutTemplate, PaneConfig,
        WorkspaceRuntimePort,
    };
    #[allow(unused_imports)]
    use workhive_testkit::fakes::{FakeI18n, FakeRuntime};

    /// `apply_layout` must call `ToolLaunchInstrumentationPort::wrap_tool_command`
    /// for every catalog tool pane and install the wrapped string through
    /// the typed `respawn_unit_with_command` snapshot-runtime verb (NOT
    /// via the visible `send_input` keystroke path), while skipping
    /// wrapping for empty shells and inline commands. This is the entry
    /// point that ensures the supervisor metadata store actually gets
    /// populated during fresh `workspace open` flows AND that the internal
    /// `workhive internal pane-supervisor ... -- <tool>` wrapper never
    /// shows up at the top of the user's pane.
    #[test]
    fn apply_layout_wraps_tool_panes_via_instrumentation() {
        use workhive_testkit::fakes::FakeToolLaunchInstrumentation;

        let runtime = base_runtime();
        let i18n = FakeI18n;
        let instrumentation = FakeToolLaunchInstrumentation::new();
        let layout = LayoutConfig {
            template: LayoutTemplate::Custom,
            columns: vec![
                ColumnConfig {
                    width: 50,
                    panes: vec![PaneConfig {
                        height: 100,
                        launch: PaneLaunchRef::Tool {
                            name: ToolName::new("claude").expect("valid tool name"),
                        },
                    }],
                },
                ColumnConfig {
                    width: 50,
                    panes: vec![PaneConfig {
                        height: 100,
                        launch: PaneLaunchRef::EmptyShell,
                    }],
                },
            ],
        };

        apply_layout(
            &runtime,
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/worktree"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &i18n,
            Locale::En,
            Some(&instrumentation),
        )
        .expect("layout should apply");

        let captured = instrumentation.captured();
        assert_eq!(
            captured.len(),
            1,
            "instrumentation must run exactly once for the single tool pane (empty shell skipped), got {captured:?}"
        );
        let req = &captured[0];
        assert_eq!(req.tool_name.as_deref(), Some("claude"));
        assert_eq!(req.program, "claude");

        // Contract: the wrapped supervisor command must be installed
        // via the typed snapshot-runtime verb, never via a visible
        // keystroke.
        let respawn_cmd_calls = runtime.respawn_command_calls();
        assert!(
            respawn_cmd_calls
                .iter()
                .any(|(_, _, cmd)| cmd.contains("internal pane-supervisor")),
            "wrapped command must reach respawn_unit_with_command; got {:?}",
            respawn_cmd_calls
        );
        assert!(
            runtime
                .sent_inputs()
                .iter()
                .all(|(_, text)| !text.contains("internal pane-supervisor")),
            "supervisor wrapper MUST NOT be typed into the pane shell; \
             saw {:?}",
            runtime.sent_inputs()
        );
    }

    /// Regression guard: `apply_layout` must keep
    /// user-visible inline commands on the keyboard `send_input` path.
    /// Re-routing them through the typed `respawn_unit_with_command`
    /// verb would replace the pane's shell with the inline command as
    /// the new primary process, closing the pane on exit and changing
    /// user-facing lifecycle. Only the internal supervisor wrapper is
    /// hidden; inline-command semantics stay unchanged.
    #[test]
    fn apply_layout_keeps_inline_command_on_send_input_not_typed_launch() {
        let runtime = base_runtime();
        let i18n = FakeI18n;
        let layout = LayoutConfig {
            template: LayoutTemplate::HalfSplit,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::InlineCommand {
                        spec: CommandSpec::new("less", ["README.md"]),
                    },
                }],
            }],
        };

        apply_layout(
            &runtime,
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/worktree"),
            &layout,
            &ToolCatalog::default(),
            &i18n,
            Locale::En,
            None,
        )
        .expect("inline-command layout should apply");

        let respawn_cmd_calls = runtime.respawn_command_calls();
        assert!(
            respawn_cmd_calls.is_empty(),
            "inline command MUST NOT install via respawn_unit_with_command; \
             saw {:?}",
            respawn_cmd_calls
        );
        let sent = runtime.sent_inputs();
        assert!(
            sent.iter().any(|(_, text)| text.contains("less")),
            "inline command must reach the pane via send_input; got {:?}",
            sent
        );
    }

    /// `apply_layout` MUST fail closed when instrumentation reports an
    /// error for a catalog `Tool` pane. Silently sending the unwrapped
    /// `claude ...` command would mean the supervisor metadata is never
    /// written and `pane list` permanently degrades to OS observer
    /// truth for that pane. Empty shells/inline commands are pass-through
    /// and not affected.
    #[test]
    fn apply_layout_fails_closed_when_tool_instrumentation_errors() {
        use workhive_core::ports::ToolLaunchInstrumentationError;
        use workhive_testkit::fakes::FakeToolLaunchInstrumentation;

        let runtime = base_runtime();
        let i18n = FakeI18n;
        let instrumentation = FakeToolLaunchInstrumentation::new();
        instrumentation.set_force_error(
            ToolLaunchInstrumentationError::SupervisorBinaryUnavailable {
                detail: "test failure: supervisor binary missing".to_owned(),
            },
        );
        let layout = LayoutConfig {
            template: LayoutTemplate::HalfSplit,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::Tool {
                        name: ToolName::new("claude").expect("valid tool name"),
                    },
                }],
            }],
        };

        let result = apply_layout(
            &runtime,
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/worktree"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &i18n,
            Locale::En,
            Some(&instrumentation),
        );

        let err = result.expect_err("apply_layout must fail closed when wrap_tool_command errors");
        let msg = err.to_string();
        assert!(
            msg.contains("claude") && msg.contains("instrumentation"),
            "fail-closed error must name the tool and instrumentation failure: {msg}"
        );

        let send_inputs: Vec<String> = runtime
            .sent_inputs()
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert!(
            send_inputs.is_empty(),
            "fail-closed apply_layout must NOT send any input to the pane; got {send_inputs:?}"
        );
    }

    /// `apply_layout` MUST fail closed when no instrumentation port is
    /// available but the layout includes a catalog `Tool` pane. Without
    /// a supervisor wrapper, `Source::Supervisor` would never appear
    /// for that pane.
    #[test]
    fn apply_layout_fails_closed_when_tool_lacks_instrumentation_port() {
        let runtime = base_runtime();
        let i18n = FakeI18n;
        let layout = LayoutConfig {
            template: LayoutTemplate::HalfSplit,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::Tool {
                        name: ToolName::new("claude").expect("valid tool name"),
                    },
                }],
            }],
        };

        let result = apply_layout(
            &runtime,
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/worktree"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &i18n,
            Locale::En,
            None,
        );

        let err = result.expect_err(
            "apply_layout must fail closed when no instrumentation is wired in for a Tool pane",
        );
        let msg = err.to_string();
        assert!(
            msg.contains("claude") && msg.contains("instrumentation"),
            "fail-closed error must name the tool and instrumentation availability: {msg}"
        );

        let send_inputs: Vec<String> = runtime
            .sent_inputs()
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert!(
            send_inputs.is_empty(),
            "fail-closed apply_layout must NOT send any input to the pane; got {send_inputs:?}"
        );
    }

    /// Pins the deterministic `--no-attach` summary contract: under both
    /// locales, the message must include the workspace name placeholder
    /// substituted with the session, and must mention the `--no-attach`
    /// mode so users get the same lifecycle visibility they'd get if
    /// they had attached. Review item: deterministic --no-attach progress.

    #[test]
    fn pane_title_uses_workhive_owned_identity_not_tmux_format() {
        let normal = pane_title_for_launch(
            &PaneLaunchRef::Tool {
                name: ToolName::new("codex").unwrap(),
            },
            &ResolvedPaneLaunch::Executable {
                command: "codex".to_owned(),
                program: "codex".to_owned(),
                args: Vec::new(),
                program_path: Some("codex".to_owned()),
                baseline_kind: workhive_core::domain::PaneKind::Agent,
            },
        );
        assert_eq!(normal, "codex", "title must come from config identity");
        assert!(
            !normal.contains("#{"),
            "title must not contain tmux format variables"
        );

        let degraded = pane_title_for_launch(
            &PaneLaunchRef::Tool {
                name: ToolName::new("claude").unwrap(),
            },
            &ResolvedPaneLaunch::CatalogMissing {
                name: ToolName::new("claude").unwrap(),
            },
        );
        assert_eq!(degraded, "claude (degraded)");
    }

    // ── Pane title reassertion (title stability before attach) ──────────
}
