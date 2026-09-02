//! Blocking, single-connection-at-a-time supervisor for the pane runtime
//! control plane. The supervisor coordinates pane registry, pane runtime
//! status, and typed query/delivery outcomes for a single (group, workspace)
//! pair, routing each typed request to the host-supplied
//! [`PaneRuntimePort`] and refreshing the optional [`PaneRegistryPort`]
//! cache from runtime listing on every `QueryPane`.
//!
//! Threading model: `serve_blocking` accepts connections on the calling
//! thread. Callers that want concurrency spawn a thread pool externally; we
//! deliberately do not pull in `tokio` / `rayon` to stay within the
//! project's synchronous runtime baseline.
//!
//! `QueryPane` must return the known pane's
//! `title/kind/status/cwd`; for unknown panes it must return a typed
//! `UnknownPane` error sourced from the registry-backed lookup, never a
//! fabricated `display_title: None`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};

use camino::{Utf8Path, Utf8PathBuf};

use workhive_core::control_plane::{
    CONTROL_PLANE_PROTOCOL_VERSION, ControlPlaneRequest, ControlPlaneResponse, RequestEnvelope,
    ResponseEnvelope,
    protocol::{
        ControlPlaneResponseError, PaneSnapshot, SendInputAction, SendInputOutcome,
        SendInputRequestPayload,
    },
};
use workhive_core::domain::{
    ObservedPaneState, PaneLaunchOrigin, PaneObservationConfidence, PaneObservationSource,
    PaneRuntimeId, PaneRuntimeStatus, UnitId, runtime_status_from_state,
};
use workhive_core::ports::{PaneRegistryPort, PaneRuntimePort, SendInputRequest};

/// Fatal supervisor errors. Per-connection IO failures log + continue; only
/// socket-lifecycle failures propagate here.
#[derive(Debug, thiserror::Error)]
pub enum ControlPlaneSupervisorError {
    #[error("failed to bind pane control plane socket at {path}: {source}")]
    Bind {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to accept pane control plane connection: {source}")]
    Accept {
        #[source]
        source: std::io::Error,
    },
}

/// Supervisor entry point.
///
/// This is intentionally a free function + struct rather than a trait: the
/// supervisor is singular per project/group/workspace and its collaborators
/// are the already-abstract `PaneRuntimePort` plus the optional
/// `PaneRegistryPort`. A richer variant (multiple workers, orchestration)
/// can add that layer on top later without changing the wire protocol.
pub struct ControlPlaneSupervisor<'a> {
    runtime: &'a dyn PaneRuntimePort,
    /// Optional registry the supervisor keeps in sync with runtime listing.
    /// When wired, every `QueryPane` performs a write-through observation
    /// pass so the registry and the wire response agree on cwd / status
    /// / kind / command.
    registry: Option<&'a dyn PaneRegistryPort>,
    /// The tmux server / runtime group this supervisor owns. Forwarded
    /// verbatim into every [`SendInputRequest`] so the runtime adapter can
    /// target the right backend.
    group: String,
    /// Workspace name this supervisor coordinates. Required so `QueryPane`
    /// can list units through the runtime port (which keys on
    /// (group, workspace)) without inventing a parallel state model.
    workspace_name: String,
}

impl<'a> ControlPlaneSupervisor<'a> {
    /// Construct a supervisor that does not maintain a registry cache.
    /// `QueryPane` still resolves real pane data via the runtime listing.
    pub fn new(
        runtime: &'a dyn PaneRuntimePort,
        group: impl Into<String>,
        workspace_name: impl Into<String>,
    ) -> Self {
        Self {
            runtime,
            registry: None,
            group: group.into(),
            workspace_name: workspace_name.into(),
        }
    }

    /// Construct a supervisor that keeps a [`PaneRegistryPort`] in sync.
    /// On every `QueryPane`, the supervisor performs a write-through
    /// observation update so the registry and the wire response stay in
    /// agreement. Production composition (`workhive workspace open`)
    /// should always use this path.
    pub fn with_registry(
        runtime: &'a dyn PaneRuntimePort,
        registry: &'a dyn PaneRegistryPort,
        group: impl Into<String>,
        workspace_name: impl Into<String>,
    ) -> Self {
        Self {
            runtime,
            registry: Some(registry),
            group: group.into(),
            workspace_name: workspace_name.into(),
        }
    }

    /// Bind to `socket_path` and serve requests sequentially until the
    /// listener closes or `should_stop` returns true between connections.
    ///
    /// The caller is responsible for creating the socket's parent directory
    /// and for removing a stale socket file before calling this — see
    /// [`prepare_socket_parent`] and [`remove_stale_socket`] for helpers.
    pub fn serve_blocking(
        &self,
        socket_path: &Utf8Path,
        mut should_stop: impl FnMut() -> bool,
    ) -> Result<(), ControlPlaneSupervisorError> {
        let listener = UnixListener::bind(socket_path.as_std_path()).map_err(|source| {
            ControlPlaneSupervisorError::Bind {
                path: socket_path.to_owned(),
                source,
            }
        })?;
        tracing::debug!(path = %socket_path, "pane control plane supervisor listening");
        for conn in listener.incoming() {
            if should_stop() {
                break;
            }
            match conn {
                Ok(stream) => {
                    if let Err(error) = self.handle_connection(stream) {
                        tracing::warn!(
                            error = %error,
                            "pane control plane connection handler failed"
                        );
                    }
                }
                Err(source) => {
                    return Err(ControlPlaneSupervisorError::Accept { source });
                }
            }
        }
        Ok(())
    }

    fn handle_connection(&self, stream: UnixStream) -> std::io::Result<()> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut writer = stream;
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(());
        }
        let trimmed = line.trim_end_matches('\n');
        let envelope: RequestEnvelope = match serde_json::from_str(trimmed) {
            Ok(envelope) => envelope,
            Err(error) => {
                tracing::warn!(error = %error, "pane control plane request decode failed");
                return write_error_envelope(
                    &mut writer,
                    None,
                    ControlPlaneResponseError::Backend {
                        detail: format!("request decode failed: {error}"),
                    },
                );
            }
        };
        let request_id = envelope.request_id.clone();

        if !envelope
            .version
            .is_compatible_with(CONTROL_PLANE_PROTOCOL_VERSION)
        {
            return write_error_envelope(
                &mut writer,
                Some(request_id),
                ControlPlaneResponseError::ProtocolVersionMismatch {
                    client: envelope.version,
                    server: CONTROL_PLANE_PROTOCOL_VERSION,
                },
            );
        }

        let response = self.dispatch(envelope.request);
        let reply = ResponseEnvelope {
            version: CONTROL_PLANE_PROTOCOL_VERSION,
            request_id,
            response,
        };
        write_envelope(&mut writer, &reply)
    }

    fn dispatch(&self, request: ControlPlaneRequest) -> ControlPlaneResponse {
        match request {
            ControlPlaneRequest::Ping => ControlPlaneResponse::Pong {
                server: CONTROL_PLANE_PROTOCOL_VERSION,
            },
            ControlPlaneRequest::SendInput(payload) => self.dispatch_send_input(payload),
            ControlPlaneRequest::QueryPane { unit_id } => self.dispatch_query_pane(unit_id),
        }
    }

    /// Resolve `QueryPane` against the runtime listing for the supervised
    /// (group, workspace) pair. The supervisor refreshes the optional
    /// registry through `update_from_observation` so subsequent reads
    /// observe the same fields the wire response carries. Returns a typed
    /// `UnknownPane` error when no unit matches — never a fabricated
    /// snapshot.
    fn dispatch_query_pane(&self, unit_id: UnitId) -> ControlPlaneResponse {
        let units = match self.runtime.list_units(&self.group, &self.workspace_name) {
            Ok(units) => units,
            Err(error) => {
                return ControlPlaneResponse::Error {
                    error: ControlPlaneResponseError::Backend {
                        detail: format!(
                            "list_units failed for group={} workspace={}: {error}",
                            self.group, self.workspace_name
                        ),
                    },
                };
            }
        };

        let Some(matched) = units.iter().find(|u| u.id == unit_id) else {
            return ControlPlaneResponse::Error {
                error: ControlPlaneResponseError::UnknownPane { unit_id },
            };
        };

        // Write-through into the registry (when wired) so QueryPane is
        // both a query and a refresh. Errors are intentionally non-fatal:
        // the wire response below is the authoritative truth, and a
        // registry hiccup must not turn a successful query into a backend
        // failure.
        //
        // `display_title` MUST
        // come from Workhive-managed launch identity (the registry's
        // `PaneDescriptor::launch_origin`), NOT from the foreground
        // command. Returning `command` here would reproduce the original
        // bug where a pane that launched `codex` but is now showing
        // `shell` reports `shell` as its display title. When no
        // authoritative launch identity is available, we leave
        // `display_title = None` and let the foreground command live in
        // the dedicated `command` field only.
        let mut authoritative_title: Option<String> = None;
        if let Some(registry) = self.registry {
            // The control-plane server only observes the tmux runtime
            // layer (`runtime.list_units`) here; there is no
            // supervisor handshake or OS process observer enrichment
            // at this seam. Pin source/confidence to the tmux fallback
            // so the registry's `derive_runtime_capabilities` does not
            // promote unverified agents to MessageReceiver.
            let observed = ObservedPaneState {
                group: self.group.clone(),
                workspace: self.workspace_name.clone(),
                unit_id: matched.id.clone(),
                cwd: matched.cwd.as_str().to_owned(),
                command: matched.command.clone(),
                kind: matched.kind,
                state: matched.state,
                source: PaneObservationSource::TmuxCurrentCommand,
                confidence: PaneObservationConfidence::Fallback,
            };
            if let Err(error) = registry.update_from_observation(observed) {
                tracing::warn!(error = %error, "registry observation update failed");
            }
            // Sole authoritative source of `display_title`: the
            // registry's launch identity for this runtime id. We treat
            // `EmptyShell` as "no Workhive title to surface" because
            // returning the literal string `"shell"` here would
            // reproduce the same drift the product bug was about.
            let runtime_id =
                PaneRuntimeId::derive(&self.group, &self.workspace_name, matched.id.as_str());
            if let Ok(Some(descriptor)) = registry.get(&runtime_id) {
                authoritative_title = workhive_launch_title(&descriptor.launch_origin);
            }
        }

        let status = runtime_status_from_state(matched.state);
        ControlPlaneResponse::QueryPane {
            snapshot: PaneSnapshot {
                unit_id: matched.id.clone(),
                // Sole source: Workhive-managed launch identity from
                // the registry. When the registry has no authoritative
                // title (no descriptor, or `EmptyShell` launch), this
                // stays `None` — the foreground command lives in the
                // dedicated `command` field below and is never used
                // here.
                display_title: authoritative_title,
                kind: matched.kind,
                status: if matches!(status, PaneRuntimeStatus::Unknown) {
                    PaneRuntimeStatus::Unknown
                } else {
                    status
                },
                cwd: Some(matched.cwd.as_str().to_owned()),
                command: if matched.command.is_empty() {
                    None
                } else {
                    Some(matched.command.clone())
                },
            },
        }
    }

    /// Deliver a `SendInput` request through the supervisor-owned pane
    /// state model.
    ///
    /// The
    /// supervisor validates the target through `list_units` before
    /// touching the runtime adapter, so unknown panes are rejected as a
    /// typed `UnknownPane` error rather than leaking the adapter's raw
    /// "no such pane" message. The supervisor also refreshes the
    /// optional registry from observation before delivery so registry
    /// callers always see a (cwd / kind / status / command) tuple that
    /// matches the runtime view.
    ///
    /// Action handling preserves item 7 semantics: literal text and the
    /// Enter key are sent together via the runtime adapter's
    /// `send_input(..., enter=true)` path which, in the tmux adapter,
    /// already uses `send-keys -l` + a separate Enter so the two are
    /// never fused into a single tmux invocation.
    fn dispatch_send_input(&self, payload: SendInputRequestPayload) -> ControlPlaneResponse {
        // ── Pane-existence validation ─────────────────────────────────
        let units = match self.runtime.list_units(&self.group, &self.workspace_name) {
            Ok(units) => units,
            Err(error) => {
                return ControlPlaneResponse::Error {
                    error: ControlPlaneResponseError::Backend {
                        detail: format!(
                            "list_units failed for group={} workspace={}: {error}",
                            self.group, self.workspace_name
                        ),
                    },
                };
            }
        };
        let Some(matched) = units.iter().find(|u| u.id == payload.unit_id) else {
            // Unknown pane — return the typed error and DO NOT call
            // `runtime.send_input`, so adapter-level "no such pane"
            // text never bleeds through to clients.
            return ControlPlaneResponse::Error {
                error: ControlPlaneResponseError::UnknownPane {
                    unit_id: payload.unit_id,
                },
            };
        };

        // ── Registry refresh-before-delivery ──────────────────────────
        if let Some(registry) = self.registry {
            // Same rationale as in `dispatch_query_pane`: tmux-only
            // signal at this seam, so source/confidence stay at the
            // fallback floor. The supervisor handshake (if any) is
            // recorded by upstream layers, not synthesised here.
            let observed = ObservedPaneState {
                group: self.group.clone(),
                workspace: self.workspace_name.clone(),
                unit_id: matched.id.clone(),
                cwd: matched.cwd.as_str().to_owned(),
                command: matched.command.clone(),
                kind: matched.kind,
                state: matched.state,
                source: PaneObservationSource::TmuxCurrentCommand,
                confidence: PaneObservationConfidence::Fallback,
            };
            if let Err(error) = registry.update_from_observation(observed) {
                tracing::warn!(
                    error = %error,
                    "registry observation refresh before send_input failed; \
                     delivery still proceeds via runtime port"
                );
            }
        }

        // ── Action interpretation (item 7 semantics) ──────────────────
        let mut literal: String = String::new();
        let mut wants_enter = false;
        for action in payload.actions {
            match action {
                SendInputAction::LiteralText { text } => {
                    literal.push_str(&text);
                }
                SendInputAction::Key { name } => {
                    if name.eq_ignore_ascii_case("Enter") || name == "C-m" {
                        wants_enter = true;
                    } else {
                        return ControlPlaneResponse::Error {
                            error: ControlPlaneResponseError::Policy {
                                reason: format!(
                                    "control plane currently supports Enter only; got key {name:?}"
                                ),
                            },
                        };
                    }
                }
            }
        }

        // ── Delivery ─────────────────────────────────────────────────
        if let Err(error) = self.runtime.send_input(SendInputRequest {
            group: self.group.clone(),
            target_unit: payload.unit_id.clone(),
            text: literal,
            enter: wants_enter,
        }) {
            return ControlPlaneResponse::Error {
                error: ControlPlaneResponseError::Backend {
                    detail: error.to_string(),
                },
            };
        }
        ControlPlaneResponse::SendInput {
            outcome: SendInputOutcome::Delivered,
        }
    }
}

fn write_envelope(writer: &mut UnixStream, envelope: &ResponseEnvelope) -> std::io::Result<()> {
    let mut payload = serde_json::to_vec(envelope).map_err(std::io::Error::other)?;
    payload.push(b'\n');
    writer.write_all(&payload)?;
    writer.flush()
}

fn write_error_envelope(
    writer: &mut UnixStream,
    request_id: Option<workhive_core::control_plane::RequestId>,
    error: ControlPlaneResponseError,
) -> std::io::Result<()> {
    let request_id =
        request_id.unwrap_or_else(|| workhive_core::control_plane::RequestId::new(String::new()));
    let envelope = ResponseEnvelope {
        version: CONTROL_PLANE_PROTOCOL_VERSION,
        request_id,
        response: ControlPlaneResponse::Error { error },
    };
    write_envelope(writer, &envelope)
}

/// Map a Workhive-managed launch identity to its authoritative pane
/// display title.
///
/// The `display_title` returned
/// in `QueryPane` MUST come from the registry's `PaneLaunchOrigin`, not
/// from the foreground command observed in the runtime listing. When
/// the launch origin is `EmptyShell` we deliberately return `None`
/// rather than the literal `"shell"`, because `EmptyShell` is a
/// "no Workhive title" sentinel — the foreground command is allowed to
/// be anything in that case (e.g. the user ran `codex` by hand) and
/// cooking up a title from it would reproduce the original bug where
/// `QueryPane` impersonates the foreground command.
fn workhive_launch_title(origin: &PaneLaunchOrigin) -> Option<String> {
    match origin {
        PaneLaunchOrigin::EmptyShell => None,
        PaneLaunchOrigin::Tool { .. } | PaneLaunchOrigin::InlineCommand { .. } => {
            Some(origin.display_name().to_owned())
        }
    }
}

/// Ensure the parent directory for `socket_path` exists. Returns
/// [`ControlPlaneSupervisorError::Bind`] when creation fails.
///
/// On Unix the directory is created with `0o700` permissions when it does
/// not already exist so that the user-scoped runtime root cannot be hijacked
/// by another user that happens to share the temp directory. We do not
/// chmod existing directories — `$XDG_RUNTIME_DIR` already enforces user
/// isolation, and racing a chmod against an attacker-owned directory would
/// be its own bug class.
pub fn prepare_socket_parent(socket_path: &Utf8Path) -> Result<(), ControlPlaneSupervisorError> {
    if let Some(parent) = socket_path.parent()
        && !parent.as_str().is_empty()
    {
        match std::fs::metadata(parent.as_std_path()) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(ControlPlaneSupervisorError::Bind {
                    path: parent.to_owned(),
                    source: std::io::Error::other(
                        "parent runtime path exists but is not a directory",
                    ),
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                create_runtime_dir_secure(parent).map_err(|source| {
                    ControlPlaneSupervisorError::Bind {
                        path: parent.to_owned(),
                        source,
                    }
                })?;
            }
            Err(err) => {
                return Err(ControlPlaneSupervisorError::Bind {
                    path: parent.to_owned(),
                    source: err,
                });
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_runtime_dir_secure(parent: &Utf8Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent.as_std_path())
}

#[cfg(not(unix))]
fn create_runtime_dir_secure(parent: &Utf8Path) -> std::io::Result<()> {
    std::fs::create_dir_all(parent.as_std_path())
}

/// Remove a stale socket file before bind. Silently ignores
/// "not-a-file"/ENOENT conditions.
pub fn remove_stale_socket(socket_path: &Utf8Path) -> std::io::Result<()> {
    match std::fs::remove_file(socket_path.as_std_path()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}
