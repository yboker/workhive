//! Blocking Unix-socket client for the pane runtime control plane.
//!
//! The client speaks the JSON framing defined in `wire.rs`: one UTF-8 JSON
//! document per request/response, terminated by a single `\n`. A future
//! transport may switch to a length-prefixed or streaming framing; callers
//! are isolated from that choice by the typed protocol.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use camino::Utf8PathBuf;

use workhive_core::control_plane::{
    CONTROL_PLANE_PROTOCOL_VERSION, ControlPlaneRequest, ControlPlaneResponse,
    PaneControlPlaneClient, PaneControlPlaneError, PaneSnapshot, ProtocolVersion, RequestEnvelope,
    RequestId, ResponseEnvelope, SendInputAction, SendInputRequestPayload, SendPaneInputOutcome,
    protocol::{ControlPlaneResponseError, SendInputOutcome},
};
use workhive_core::domain::UnitId;

/// Unix-domain-socket client for the local pane control plane supervisor.
///
/// One `LocalPaneControlPlaneClient` = one socket path. Each method call
/// opens a fresh connection, sends a single request, reads a single
/// response, then closes. Multiplexing multiple requests on one connection
/// is a future protocol extension.
pub struct LocalPaneControlPlaneClient {
    socket_path: Utf8PathBuf,
    connect_timeout: Duration,
    io_timeout: Duration,
}

impl LocalPaneControlPlaneClient {
    /// Construct a client for the given socket path. The socket is not
    /// probed eagerly; each method call attempts to connect lazily so
    /// "supervisor not running" is a per-call condition and not a
    /// constructor error.
    pub fn new(socket_path: Utf8PathBuf) -> Self {
        Self {
            socket_path,
            connect_timeout: Duration::from_millis(200),
            io_timeout: Duration::from_millis(1_500),
        }
    }

    fn next_request_id() -> RequestId {
        // A UUID v4 keeps correlation ids unique per request; the wire
        // representation is just a string so callers never see `uuid`.
        RequestId::new(uuid::Uuid::new_v4().to_string())
    }

    fn round_trip(
        &self,
        request: ControlPlaneRequest,
    ) -> Result<ControlPlaneResponse, PaneControlPlaneError> {
        let envelope = RequestEnvelope {
            version: CONTROL_PLANE_PROTOCOL_VERSION,
            request_id: Self::next_request_id(),
            request,
        };
        let expected_request_id = envelope.request_id.clone();

        let mut stream = connect_with_timeout(&self.socket_path, self.connect_timeout)?;
        stream
            .set_read_timeout(Some(self.io_timeout))
            .map_err(|e| PaneControlPlaneError::Transport {
                detail: format!("set_read_timeout failed: {e}"),
            })?;
        stream
            .set_write_timeout(Some(self.io_timeout))
            .map_err(|e| PaneControlPlaneError::Transport {
                detail: format!("set_write_timeout failed: {e}"),
            })?;

        let mut payload =
            serde_json::to_vec(&envelope).map_err(|e| PaneControlPlaneError::Transport {
                detail: format!("request encode failed: {e}"),
            })?;
        payload.push(b'\n');
        stream
            .write_all(&payload)
            .map_err(|e| PaneControlPlaneError::Transport {
                detail: format!("write failed: {e}"),
            })?;
        stream
            .flush()
            .map_err(|e| PaneControlPlaneError::Transport {
                detail: format!("flush failed: {e}"),
            })?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| PaneControlPlaneError::Transport {
                detail: format!("read failed: {e}"),
            })?;
        if read == 0 {
            return Err(PaneControlPlaneError::Transport {
                detail: "supervisor closed connection without response".to_owned(),
            });
        }

        let response: ResponseEnvelope = serde_json::from_str(line.trim_end_matches('\n'))
            .map_err(|e| PaneControlPlaneError::Transport {
                detail: format!("response decode failed: {e}"),
            })?;

        if !CONTROL_PLANE_PROTOCOL_VERSION.is_compatible_with(response.version) {
            return Err(PaneControlPlaneError::ProtocolVersionMismatch {
                client: CONTROL_PLANE_PROTOCOL_VERSION,
                server: response.version,
            });
        }
        if response.request_id != expected_request_id {
            return Err(PaneControlPlaneError::Transport {
                detail: format!(
                    "response request id mismatch: expected {}, got {}",
                    expected_request_id.as_str(),
                    response.request_id.as_str()
                ),
            });
        }
        Ok(response.response)
    }
}

fn connect_with_timeout(
    socket_path: &camino::Utf8Path,
    timeout: Duration,
) -> Result<UnixStream, PaneControlPlaneError> {
    // `UnixStream::connect` itself does not take a timeout. We emulate one
    // by setting the socket non-blocking for the connect attempt is overkill
    // for a local UDS; instead we spawn a quick blocking probe and rely on
    // the OS returning ENOENT / ECONNREFUSED immediately when the socket
    // isn't live.
    //
    // The `timeout` value is reserved for future async transports and is
    // documented as an upper bound; today we surface any connect error as
    // `SupervisorUnavailable` so the CLI can fall back without blocking.
    let _ = timeout;
    UnixStream::connect(socket_path.as_std_path()).map_err(|e| {
        PaneControlPlaneError::SupervisorUnavailable {
            detail: format!("connect {socket_path}: {e}"),
        }
    })
}

impl PaneControlPlaneClient for LocalPaneControlPlaneClient {
    fn send_pane_input(
        &self,
        unit_id: &UnitId,
        text: &str,
        submit: bool,
    ) -> Result<SendPaneInputOutcome, PaneControlPlaneError> {
        let mut actions = Vec::with_capacity(2);
        if !text.is_empty() {
            actions.push(SendInputAction::LiteralText {
                text: text.to_owned(),
            });
        }
        if submit {
            actions.push(SendInputAction::Key {
                name: "Enter".to_owned(),
            });
        }
        let response =
            self.round_trip(ControlPlaneRequest::SendInput(SendInputRequestPayload {
                unit_id: unit_id.clone(),
                actions,
            }))?;
        match response {
            ControlPlaneResponse::SendInput {
                outcome: SendInputOutcome::Delivered,
            } => Ok(SendPaneInputOutcome::Delivered),
            ControlPlaneResponse::SendInput {
                outcome: SendInputOutcome::RejectedByPolicy { reason },
            } => Ok(SendPaneInputOutcome::RejectedByPolicy { reason }),
            ControlPlaneResponse::Error { error } => Err(map_response_error(error)),
            other => Err(PaneControlPlaneError::Transport {
                detail: format!("unexpected response kind for SendInput: {:?}", other.kind()),
            }),
        }
    }

    fn ping(&self) -> Result<ProtocolVersion, PaneControlPlaneError> {
        let response = self.round_trip(ControlPlaneRequest::Ping)?;
        match response {
            ControlPlaneResponse::Pong { server } => Ok(server),
            ControlPlaneResponse::Error { error } => Err(map_response_error(error)),
            other => Err(PaneControlPlaneError::Transport {
                detail: format!("unexpected response kind for Ping: {:?}", other.kind()),
            }),
        }
    }

    fn query_pane(&self, unit_id: &UnitId) -> Result<PaneSnapshot, PaneControlPlaneError> {
        let response = self.round_trip(ControlPlaneRequest::QueryPane {
            unit_id: unit_id.clone(),
        })?;
        match response {
            ControlPlaneResponse::QueryPane { snapshot } => Ok(snapshot),
            ControlPlaneResponse::Error { error } => Err(map_response_error(error)),
            other => Err(PaneControlPlaneError::Transport {
                detail: format!("unexpected response kind for QueryPane: {:?}", other.kind()),
            }),
        }
    }
}

fn map_response_error(error: ControlPlaneResponseError) -> PaneControlPlaneError {
    match error {
        ControlPlaneResponseError::Backend { detail } => PaneControlPlaneError::Backend { detail },
        ControlPlaneResponseError::Policy { reason } => PaneControlPlaneError::Policy { reason },
        ControlPlaneResponseError::UnknownPane { unit_id } => {
            PaneControlPlaneError::UnknownPane { unit_id }
        }
        ControlPlaneResponseError::ProtocolVersionMismatch { client, server } => {
            PaneControlPlaneError::ProtocolVersionMismatch { client, server }
        }
    }
}
