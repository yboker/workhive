//! Concrete pane message delivery implementation.
//!
//! `RuntimePaneMessenger` resolves the target pane's routing context
//! from the registry, then delegates actual delivery to the runtime
//! adapter via [`PaneRuntimePort`].

use workhive_core::domain::{PaneCapability, PaneMessage, PaneMessagePayload};
use workhive_core::ports::{
    PaneMessagePort, PaneRegistryPort, PaneRuntimePort, RegistryError, SendInputRequest,
};

/// Delivers messages to panes by resolving routing through the registry
/// and delegating to the runtime port.
///
/// Delivery is capability-aware:
/// - `Text` requires `MessageReceiver` or `CommandExecutor` capability
/// - `Command`/`Signal` are rejected at this phase of the transport contract
pub struct RuntimePaneMessenger<'a> {
    registry: &'a dyn PaneRegistryPort,
    runtime: &'a dyn PaneRuntimePort,
}

impl<'a> RuntimePaneMessenger<'a> {
    pub fn new(registry: &'a dyn PaneRegistryPort, runtime: &'a dyn PaneRuntimePort) -> Self {
        Self { registry, runtime }
    }
}

impl PaneMessagePort for RuntimePaneMessenger<'_> {
    fn send(&self, message: PaneMessage) -> Result<(), RegistryError> {
        let descriptor =
            self.registry
                .get(&message.to)?
                .ok_or_else(|| RegistryError::PaneNotFound {
                    id: message.to.as_str().to_owned(),
                })?;

        let text = match &message.payload {
            PaneMessagePayload::Text { content } => {
                let accepts_text = descriptor
                    .capabilities
                    .contains(&PaneCapability::MessageReceiver)
                    || descriptor
                        .capabilities
                        .contains(&PaneCapability::CommandExecutor);
                if !accepts_text {
                    return Err(RegistryError::Internal {
                        message: format!(
                            "target pane {} cannot accept text input (requires MessageReceiver or CommandExecutor capability)",
                            message.to
                        ),
                    });
                }
                content.clone()
            }
            PaneMessagePayload::Command { .. } => {
                return Err(RegistryError::PayloadNotSupported {
                    payload_type: "Command".to_owned(),
                });
            }
            PaneMessagePayload::Signal { .. } => {
                return Err(RegistryError::PayloadNotSupported {
                    payload_type: "Signal".to_owned(),
                });
            }
        };

        let request = SendInputRequest {
            group: descriptor.group,
            target_unit: descriptor.unit_id,
            text,
            enter: true,
        };

        self.runtime
            .send_input(request)
            .map_err(|e| RegistryError::Internal {
                message: format!("runtime delivery failed: {e}"),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use workhive_core::domain::{
        PaneDescriptor, PaneKind, PaneLaunchOrigin, PaneLifecycleEvent, PaneRuntimeId,
        PaneRuntimeStatus, PaneState, UnitId,
    };
    use workhive_core::ports::{
        RegistryDelta, RuntimeError, RuntimeUnitSnapshot, SpawnUnitRequest,
    };

    struct StubRegistry {
        descriptor: Option<PaneDescriptor>,
    }

    impl PaneRegistryPort for StubRegistry {
        fn upsert(&self, _: PaneDescriptor) -> Result<RegistryDelta, RegistryError> {
            Ok(RegistryDelta {
                changes: Vec::new(),
            })
        }
        fn remove(&self, _: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
            Ok(None)
        }
        fn get(&self, _: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
            Ok(self.descriptor.clone())
        }
        fn list_workspace(&self, _: &str) -> Result<Vec<PaneDescriptor>, RegistryError> {
            Ok(vec![])
        }
        fn list(&self, _: Option<&str>) -> Result<Vec<PaneDescriptor>, RegistryError> {
            Ok(vec![])
        }
        fn drain_events(&self) -> Result<Vec<PaneLifecycleEvent>, RegistryError> {
            Ok(vec![])
        }
        fn update_from_observation(
            &self,
            _: workhive_core::domain::ObservedPaneState,
        ) -> Result<Vec<workhive_core::domain::PaneStateChange>, RegistryError> {
            Ok(vec![])
        }
        fn find_by_capability(
            &self,
            _: PaneCapability,
        ) -> Result<Vec<PaneDescriptor>, RegistryError> {
            Ok(vec![])
        }
    }

    struct RecordingRuntime {
        sent: RefCell<Vec<SendInputRequest>>,
    }

    impl RecordingRuntime {
        fn new() -> Self {
            Self {
                sent: RefCell::new(Vec::new()),
            }
        }
    }

    impl PaneRuntimePort for RecordingRuntime {
        fn spawn_unit(&self, _: SpawnUnitRequest) -> Result<RuntimeUnitSnapshot, RuntimeError> {
            unimplemented!()
        }
        fn close_unit(&self, _: &str, _: &UnitId) -> Result<(), RuntimeError> {
            unimplemented!()
        }
        fn list_units(&self, _: &str, _: &str) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
            unimplemented!()
        }
        fn current_unit(&self, _: &str) -> Result<RuntimeUnitSnapshot, RuntimeError> {
            unimplemented!()
        }
        fn send_input(&self, request: SendInputRequest) -> Result<(), RuntimeError> {
            self.sent.borrow_mut().push(request);
            Ok(())
        }
        fn set_unit_title(
            &self,
            _request: workhive_core::ports::SetUnitTitleRequest,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    fn agent_descriptor() -> PaneDescriptor {
        PaneDescriptor {
            runtime_id: PaneRuntimeId::derive("g", "ws", "%0"),
            group: "g".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: UnitId::new("%0"),
            cwd: "/tmp".to_owned(),
            command: "agent".to_owned(),
            kind: PaneKind::Agent,
            state: PaneState::Running,
            observed_status: PaneRuntimeStatus::Running,
            role: None,
            launch_origin: PaneLaunchOrigin::EmptyShell,
            capabilities: [
                PaneCapability::MessageResponder,
                PaneCapability::MessageReceiver,
            ]
            .into_iter()
            .collect(),
        }
    }

    fn terminal_descriptor() -> PaneDescriptor {
        PaneDescriptor {
            runtime_id: PaneRuntimeId::derive("g", "ws", "%1"),
            group: "g".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: UnitId::new("%1"),
            cwd: "/tmp".to_owned(),
            command: "zsh".to_owned(),
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            observed_status: PaneRuntimeStatus::Idle,
            role: None,
            launch_origin: PaneLaunchOrigin::EmptyShell,
            capabilities: [PaneCapability::CommandExecutor].into_iter().collect(),
        }
    }

    fn git_ui_descriptor() -> PaneDescriptor {
        PaneDescriptor {
            runtime_id: PaneRuntimeId::derive("g", "ws", "%2"),
            group: "g".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: UnitId::new("%2"),
            cwd: "/tmp".to_owned(),
            command: "lazygit".to_owned(),
            kind: PaneKind::GitUi,
            state: PaneState::Running,
            observed_status: PaneRuntimeStatus::Running,
            role: None,
            launch_origin: PaneLaunchOrigin::EmptyShell,
            capabilities: [PaneCapability::Tui].into_iter().collect(),
        }
    }

    #[test]
    fn text_payload_delivers_successfully() {
        let reg = StubRegistry {
            descriptor: Some(agent_descriptor()),
        };
        let rt = RecordingRuntime::new();
        let messenger = RuntimePaneMessenger::new(&reg, &rt);

        let msg = PaneMessage {
            from: None,
            to: PaneRuntimeId::derive("g", "ws", "%0"),
            payload: PaneMessagePayload::Text {
                content: "hello".to_owned(),
            },
        };

        messenger.send(msg).unwrap();
        let sent = rt.sent.borrow();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "hello");
        assert_eq!(sent[0].group, "g");
        assert!(sent[0].enter);
    }

    #[test]
    fn text_payload_delivers_to_shell_pane() {
        let reg = StubRegistry {
            descriptor: Some(terminal_descriptor()),
        };
        let rt = RecordingRuntime::new();
        let messenger = RuntimePaneMessenger::new(&reg, &rt);

        let msg = PaneMessage {
            from: None,
            to: PaneRuntimeId::derive("g", "ws", "%1"),
            payload: PaneMessagePayload::Text {
                content: "pwd".to_owned(),
            },
        };

        messenger.send(msg).unwrap();
        let sent = rt.sent.borrow();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "pwd");
        assert_eq!(sent[0].target_unit, UnitId::new("%1"));
    }

    #[test]
    fn text_payload_rejects_non_receiver_non_shell_pane() {
        let reg = StubRegistry {
            descriptor: Some(git_ui_descriptor()),
        };
        let rt = RecordingRuntime::new();
        let messenger = RuntimePaneMessenger::new(&reg, &rt);

        let msg = PaneMessage {
            from: None,
            to: PaneRuntimeId::derive("g", "ws", "%2"),
            payload: PaneMessagePayload::Text {
                content: "hello".to_owned(),
            },
        };

        let result = messenger.send(msg);
        match result {
            Err(RegistryError::Internal { message }) => {
                assert!(message.contains("MessageReceiver or CommandExecutor"));
            }
            other => panic!("expected Internal, got {:?}", other),
        }

        assert!(
            rt.sent.borrow().is_empty(),
            "runtime must not receive any input for unsupported text target"
        );
    }

    #[test]
    fn command_payload_returns_payload_not_supported() {
        let reg = StubRegistry {
            descriptor: Some(agent_descriptor()),
        };
        let rt = RecordingRuntime::new();
        let messenger = RuntimePaneMessenger::new(&reg, &rt);

        let msg = PaneMessage {
            from: None,
            to: PaneRuntimeId::derive("g", "ws", "%0"),
            payload: PaneMessagePayload::Command {
                name: "run".to_owned(),
                args: vec!["test".to_owned(), "--verbose".to_owned()],
            },
        };

        let result = messenger.send(msg);
        match result {
            Err(RegistryError::PayloadNotSupported { payload_type }) => {
                assert_eq!(payload_type, "Command");
            }
            other => panic!("expected PayloadNotSupported, got {:?}", other),
        }

        assert!(
            rt.sent.borrow().is_empty(),
            "runtime must not receive any input for Command payload"
        );
    }

    #[test]
    fn signal_payload_returns_payload_not_supported() {
        let reg = StubRegistry {
            descriptor: Some(agent_descriptor()),
        };
        let rt = RecordingRuntime::new();
        let messenger = RuntimePaneMessenger::new(&reg, &rt);

        let msg = PaneMessage {
            from: None,
            to: PaneRuntimeId::derive("g", "ws", "%0"),
            payload: PaneMessagePayload::Signal {
                signal: "INT".to_owned(),
            },
        };

        let result = messenger.send(msg);
        match result {
            Err(RegistryError::PayloadNotSupported { payload_type }) => {
                assert_eq!(payload_type, "Signal");
            }
            other => panic!("expected PayloadNotSupported, got {:?}", other),
        }

        assert!(
            rt.sent.borrow().is_empty(),
            "runtime must not receive any input for Signal payload"
        );
    }

    #[test]
    fn command_payload_never_concatenates_args() {
        let reg = StubRegistry {
            descriptor: Some(agent_descriptor()),
        };
        let rt = RecordingRuntime::new();
        let messenger = RuntimePaneMessenger::new(&reg, &rt);

        let msg = PaneMessage {
            from: None,
            to: PaneRuntimeId::derive("g", "ws", "%0"),
            payload: PaneMessagePayload::Command {
                name: "git".to_owned(),
                args: vec![
                    "commit".to_owned(),
                    "-m".to_owned(),
                    "hello world".to_owned(),
                ],
            },
        };

        let result = messenger.send(msg);
        assert!(
            result.is_err(),
            "Command must be rejected, not flattened to string"
        );
        assert!(
            rt.sent.borrow().is_empty(),
            "regression: args must never be joined into a shell-like string"
        );
    }

    #[test]
    fn send_to_non_receiver_non_shell_pane_fails() {
        let reg = StubRegistry {
            descriptor: Some(git_ui_descriptor()),
        };
        let rt = RecordingRuntime::new();
        let messenger = RuntimePaneMessenger::new(&reg, &rt);

        let msg = PaneMessage {
            from: None,
            to: PaneRuntimeId::derive("g", "ws", "%2"),
            payload: PaneMessagePayload::Text {
                content: "hello".to_owned(),
            },
        };

        let result = messenger.send(msg);
        assert!(result.is_err());
        assert!(rt.sent.borrow().is_empty());
    }

    #[test]
    fn send_to_nonexistent_pane_fails() {
        let reg = StubRegistry { descriptor: None };
        let rt = RecordingRuntime::new();
        let messenger = RuntimePaneMessenger::new(&reg, &rt);

        let msg = PaneMessage {
            from: None,
            to: PaneRuntimeId::derive("g", "ws", "%99"),
            payload: PaneMessagePayload::Text {
                content: "hello".to_owned(),
            },
        };

        let result = messenger.send(msg);
        assert!(result.is_err());
    }

    /// Proves that a pane registered during bootstrap (with a real UnitId
    /// like "%0" from the runtime) can be targeted by `RuntimePaneMessenger`.
    /// This is the end-to-end guarantee that synthetic IDs like "col0-row0"
    /// can no longer break message delivery.
    #[test]
    fn bootstrap_registered_pane_can_be_messaged_via_real_unit_id() {
        use crate::pane_registry::InMemoryPaneRegistry;

        let real_unit_id = UnitId::new("%42");
        let runtime_id = PaneRuntimeId::derive("srv", "ws", real_unit_id.as_str());

        let descriptor = PaneDescriptor {
            runtime_id: runtime_id.clone(),
            group: "srv".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: real_unit_id.clone(),
            cwd: "/project".to_owned(),
            command: "claude".to_owned(),
            kind: PaneKind::Agent,
            state: PaneState::Running,
            observed_status: PaneRuntimeStatus::Running,
            role: None,
            launch_origin: PaneLaunchOrigin::EmptyShell,
            capabilities: [
                PaneCapability::MessageReceiver,
                PaneCapability::MessageResponder,
            ]
            .into_iter()
            .collect(),
        };

        let registry = InMemoryPaneRegistry::new();
        registry
            .upsert(descriptor)
            .expect("baseline registration must succeed");

        let rt = RecordingRuntime::new();
        let messenger = RuntimePaneMessenger::new(&registry, &rt);

        let msg = PaneMessage {
            from: None,
            to: runtime_id,
            payload: PaneMessagePayload::Text {
                content: "hello from bootstrap".to_owned(),
            },
        };

        messenger
            .send(msg)
            .expect("message to bootstrap-registered pane must succeed");

        let sent = rt.sent.borrow();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].target_unit, real_unit_id);
        assert_eq!(sent[0].text, "hello from bootstrap");
        assert_eq!(sent[0].group, "srv");
    }
}
