//! Fake (in-memory) implementations of all port traits.
//!
//! Each fake stores its state in memory and can be pre-loaded with test data.
//! They implement the port traits from `workhive-core` so that use cases
//! can be tested without real git, tmux, or filesystem access.
//!
//! # Runtime fakes
//!
//! `FakeRuntime` implements `WorkspaceRuntimePort` and `PaneRuntimePort` in
//! memory without any knowledge of tmux. It replaces the old `FakeTmux` for
//! use-case–level tests. The infra-level `TmuxPort` trait and `TmuxError` are
//! not exposed here.

mod activity_event_store;
mod clock;
mod config;
mod dependency;
mod discovery;
mod environment;
mod executable;
mod git;
mod i18n;
mod identity;
mod message_bus;
mod notification;
mod orchestration;
mod pane;
mod path_canonicalization;
mod project_index;
mod project_init_lock;
mod scheduler;
mod tmux;
mod tool_launch_instrumentation;
mod workspace_fs;
mod worktree_provenance;

pub use activity_event_store::{ActivityEventStoreErrorInjection, InMemoryActivityEventStore};
pub use clock::FakeClock;
pub use config::FakeConfig;
pub use dependency::FakeDependency;
pub use discovery::FakeDiscovery;
pub use environment::FakeEnvironment;
pub use executable::FakeExecutableChecker;
pub use git::FakeGit;
pub use i18n::FakeI18n;
pub use identity::FakeIdentityPort;
pub use message_bus::{FakeMessageBus, FakeMessageDelivery};
pub use notification::FakeNotificationPort;
pub use orchestration::FakeOrchestration;
pub use pane::{FakePaneObserver, FakePaneRegistry};
pub use path_canonicalization::FakePathCanonicalization;
pub use project_index::FakeProjectIndex;
pub use project_init_lock::FakeProjectInitLock;
pub use scheduler::{FakeScheduler, SchedulerErrorTarget};
pub use tmux::{FakeRuntime, RuntimeErrorInjection};
pub use tool_launch_instrumentation::FakeToolLaunchInstrumentation;
pub use workspace_fs::{FakeDirectoryRegistry, FakeWorkspaceFs};
pub use worktree_provenance::FakeWorktreeProvenance;
