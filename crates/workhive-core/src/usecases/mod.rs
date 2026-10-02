//! Application use cases — orchestrate port calls to fulfill CLI commands.
//!
//! Each use case receives port trait references and returns typed results.
//! Use cases contain business logic but no I/O — all I/O goes through ports.

pub mod activity_control_plane;
pub mod ai_activity;
pub mod ai_integrations;
pub mod branch_provenance_policy;
pub mod close;
pub mod codex_hook_approval;
pub mod completion;
pub mod config_mgmt;
pub mod deliver_notification;
pub mod destroy;
pub mod doctor;
pub mod env;
pub mod identity;
pub mod init;
pub mod launch_resolver;
pub mod layout_planner;
pub mod layout_template;
pub mod list;
pub mod naming;
pub mod open;
pub mod orchestration;
pub mod orchestration_primitives;
pub mod pane;
pub mod pane_observation;
pub mod pane_send;
pub mod pane_sync;
pub mod path_completion;
pub mod path_match;
pub mod path_strategy;
pub mod project_state;
pub mod reindex;
pub mod remove_worktree;
pub mod resolve_projects;
pub mod scheduler;
pub mod sessions;
pub mod terminal_status;
pub mod tmux_navigation;
pub mod trusted_roots;
pub mod worktree_safety;
