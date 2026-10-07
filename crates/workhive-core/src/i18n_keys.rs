//! Centralized i18n message keys.
//!
//! All user-facing text in workhive is referenced by a string key.
//! Keys are organized by module prefix:
//! - `init.*` — init command messages
//! - `config.*` — config command messages
//! - `confirm.*` — confirmation prompts
//! - `error.*` — error display messages
//! - `list.*` — list command labels

// -- Confirmation prompts --
pub const CONFIRM_INIT_GLOBAL: &str = "confirm.init_global";
pub const CONFIRM_INIT_PROJECT: &str = "confirm.init_project";
pub const CONFIRM_DESTROY: &str = "confirm.destroy";
pub const CONFIRM_GENERIC_MUTATE: &str = "confirm.generic_mutate";
/// Shown inside a destructive confirmation screen when the terminal is
/// too short to display every safety-report line above the Yes/No
/// choice. Makes truncation explicit so risk detail is never silently
/// dropped from the decision point.
pub const CONFIRM_CONTEXT_TRUNCATED: &str = "confirm.context_truncated";
/// Shown inside a destructive confirmation screen when the terminal is
/// too small to display even the decision-critical safety counts.
/// Signals that the user should enlarge the terminal so the changed-file
/// count breakdown is never hidden behind a normal-looking prompt.
pub const CONFIRM_CONTEXT_TOO_SMALL: &str = "confirm.context_too_small";

// -- Init command --
pub const INIT_GLOBAL_SUCCESS: &str = "init.global_success";
pub const INIT_PROJECT_SUCCESS: &str = "init.project_success";
pub const INIT_ALREADY_EXISTS: &str = "init.already_exists";
pub const INIT_SELECT_LANGUAGE: &str = "init.select_language";
pub const INIT_LANG_ENGLISH: &str = "init.lang.english";
pub const INIT_LANG_ZH_CN: &str = "init.lang.zh_cn";
pub const INIT_SELECT_BACKEND: &str = "init.select_backend";
pub const INIT_BACKEND_INHERIT: &str = "init.backend.inherit";
pub const INIT_BACKEND_TMUX: &str = "init.backend.tmux";
pub const INIT_BACKEND_HERDR: &str = "init.backend.herdr";
pub const INIT_SELECT_HERDR_SESSION: &str = "init.select_herdr_session";
pub const INIT_HERDR_SESSION_INHERIT: &str = "init.herdr_session.inherit";
pub const INIT_HERDR_SESSION_OVERRIDE: &str = "init.herdr_session.override";
pub const INIT_HERDR_SESSION_INPUT: &str = "init.herdr_session.input";
pub const INIT_GUIDE_GLOBAL: &str = "init.guide_global";
pub const INIT_GLOBAL_COMPLETE_CONTINUE: &str = "init.global_complete_continue";
pub const INIT_WIZARD_TITLE: &str = "init.wizard_title";
pub const INIT_STEP_LAYOUT: &str = "init.step.layout";
pub const INIT_STEP_PANE_COMMANDS: &str = "init.step.pane_commands";
pub const INIT_STEP_CONFIRM: &str = "init.step.confirm";
pub const INIT_LAYOUT_TEMPLATE: &str = "init.layout_template";
pub const INIT_LAYOUT_AI_LEFT: &str = "init.layout.ai_left";
pub const INIT_LAYOUT_AI_DUAL: &str = "init.layout.ai_dual";
pub const INIT_LAYOUT_HALF: &str = "init.layout.half";
pub const INIT_LAYOUT_QUAD: &str = "init.layout.quad";
pub const INIT_LAYOUT_SINGLE: &str = "init.layout.single";
pub const INIT_LAYOUT_CUSTOM: &str = "init.layout.custom";
pub const INIT_LAYOUT_CUSTOM_DSL: &str = "init.layout.custom_dsl";
pub const INIT_LAYOUT_PREVIEW: &str = "init.layout.preview";
pub const INIT_CUSTOM_DESC: &str = "init.custom_desc";
pub const INIT_CUSTOM_ROWS: &str = "init.custom_rows";
pub const INIT_CUSTOM_COLS: &str = "init.custom_cols";
pub const INIT_CUSTOM_COLS_DYNAMIC: &str = "init.custom_cols_dynamic";
pub const INIT_CUSTOM_ROWS_DYNAMIC: &str = "init.custom_rows_dynamic";
pub const INIT_CUSTOM_COL_WIDTH: &str = "init.custom_col_width";
pub const INIT_CUSTOM_COL_ROWS: &str = "init.custom_col_rows";
/// Compact running summary emitted in the custom-layout flow after each
/// accepted per-column pane-count answer. Args: configured column
/// count, total column count, per-column pane detail, total panes so far.
pub const INIT_CUSTOM_PROGRESS: &str = "init.custom_progress";
/// Marker rendered in the per-column detail for columns whose pane
/// count has not been answered yet (e.g. `C3=pending`).
pub const INIT_CUSTOM_PROGRESS_PENDING: &str = "init.custom_progress_pending";
/// Compact context header shown before the pane-command step asks its
/// first question. Args: selected layout label, total pane count.
pub const INIT_PANE_CONTEXT_HEADER: &str = "init.pane_context_header";
pub const INIT_PANE_CMD_MODIFY_ASK: &str = "init.pane_cmd_modify_ask";
pub const INIT_PANE_CMD_PROMPT: &str = "init.pane_cmd_prompt";
/// Title for the per-pane command selector. Args: pane position.
pub const INIT_PANE_CMD_SELECTOR_TITLE: &str = "init.pane_cmd_selector_title";
/// Compact pane list shown as non-selectable selector context. Args:
/// pane defaults summary.
pub const INIT_PANE_CMD_SELECTOR_PANES: &str = "init.pane_cmd_selector_panes";
pub const INIT_PANE_CMD_EMPTY: &str = "init.pane_cmd_empty";
pub const INIT_PANE_CMD_EMPTY_OPTION: &str = "init.pane_cmd_empty_option";
pub const INIT_PANE_CMD_CUSTOM_OPTION: &str = "init.pane_cmd_custom_option";
pub const INIT_PANE_CMD_CUSTOM_INPUT: &str = "init.pane_cmd_custom_input";
/// Warning message when a custom command is not found on PATH.
/// Args: {0} = command name
pub const INIT_PANE_CMD_NOT_FOUND_WARNING: &str = "init.pane_cmd_not_found_warning";
/// Prompt asking if user wants to retry entering a command.
pub const INIT_PANE_CMD_RETRY_PROMPT: &str = "init.pane_cmd_retry_prompt";
/// Title for the live layout preview shown beside the pane command
/// selector in wide terminals.
pub const INIT_PANE_PREVIEW_TITLE: &str = "init.pane_preview_title";
/// Placeholder display for the pane being configured when the custom
/// command option is highlighted but no inline command has been entered.
pub const INIT_PANE_PREVIEW_CUSTOM: &str = "init.pane_preview_custom";

// -- Init wizard: pane position labels (row/col based) --

// -- Init wizard: overwrite & scope --
pub const INIT_CONFIG_CREATE_CONFIRM: &str = "init.config_create_confirm";
pub const INIT_CONFIG_EXISTS_OVERWRITE: &str = "init.config_exists_overwrite";
pub const INIT_CONFIG_ABORT: &str = "init.config_abort";
pub const INIT_SCOPE_GLOBAL: &str = "init.scope.global";
pub const INIT_SCOPE_PROJECT: &str = "init.scope.project";
pub const INIT_STEP_COMPLETE: &str = "init.step_complete";

// -- Init wizard: diagram text labels --
pub const INIT_DIAGRAM_SHELL: &str = "init.diagram.shell";

// -- Init: next steps --
pub const INIT_GLOBAL_NEXT_STEPS: &str = "init.global_next_steps";
pub const INIT_PROJECT_NEXT_STEPS: &str = "init.project_next_steps";

// -- Init: shell completion guidance after global init --
//
// `workhive init --global` must explain `workhive completion install`,
// offer interactive installation when running on a TTY, and surface a
// clear manual command for non-TTY runs. Shell startup files
// (`~/.zshrc`, `~/.bashrc`, fish config, …) MUST NOT be modified
// without explicit user confirmation, so the guidance copy here is
// the only entry point that can later trigger the install routine.
/// Localized intro paragraph that explains what
/// `workhive completion install` does (Tab-completes commands,
/// subcommands, args, workspace/pane candidates).
pub const INIT_GLOBAL_COMPLETION_INTRO: &str = "init.global_completion.intro";
/// Yes/No confirm prompt. Asked only on a TTY *after* the intro.
pub const INIT_GLOBAL_COMPLETION_PROMPT: &str = "init.global_completion.prompt";
/// Manual hint copy that ALWAYS includes the literal command
/// `workhive completion install`. One positional argument: a single
/// trailing line summarising next steps (e.g. detected shell name).
pub const INIT_GLOBAL_COMPLETION_MANUAL_HINT: &str = "init.global_completion.manual_hint";
/// Manual hint variant when shell auto-detection succeeded — one
/// positional argument is the detected shell short name.
pub const INIT_GLOBAL_COMPLETION_MANUAL_HINT_WITH_SHELL: &str =
    "init.global_completion.manual_hint_with_shell";
/// Hint shown on a TTY when shell auto-detection failed. Must
/// surface the explicit form `workhive completion install <shell>`
/// with a list of supported shells so the user can pick one.
pub const INIT_GLOBAL_COMPLETION_SHELL_NOT_DETECTED: &str =
    "init.global_completion.shell_not_detected";
/// Acknowledge a "no, not now" answer. Tells the user they can
/// install later with `workhive completion install`.
pub const INIT_GLOBAL_COMPLETION_DECLINED: &str = "init.global_completion.declined";
/// Final "all done" line printed after a successful in-init install.
pub const INIT_GLOBAL_COMPLETION_INSTALLED: &str = "init.global_completion.installed";

// -- Open: execution summary --
pub const OPEN_SUMMARY_PROJECT_INIT: &str = "open.summary.project_init";
pub const OPEN_SUMMARY_PROJECT_EXISTING: &str = "open.summary.project_existing";
pub const OPEN_SUMMARY_WORKTREE_CREATED: &str = "open.summary.worktree_created";
pub const OPEN_SUMMARY_WORKTREE_ATTACHED: &str = "open.summary.worktree_attached";
pub const OPEN_SUMMARY_WORKTREE_REUSED: &str = "open.summary.worktree_reused";
pub const OPEN_SUMMARY_WORKTREE_MAIN: &str = "open.summary.worktree_main";
pub const OPEN_SUMMARY_SESSION_CREATED: &str = "open.summary.session_created";
pub const OPEN_SUMMARY_SESSION_REUSED: &str = "open.summary.session_reused";
pub const OPEN_SUMMARY_LAYOUT_APPLIED: &str = "open.summary.layout_applied";
pub const OPEN_SUMMARY_ATTACH_SKIPPED: &str = "open.summary.attach_skipped";

// -- Init wizard: tool availability --
pub const INIT_TOOL_NOT_FOUND: &str = "init.tool.not_found";

// -- Open: orchestration mode summary --
pub const OPEN_MODE_MANUAL: &str = "open.mode.manual";
pub const OPEN_MODE_PRIMARY_ACTIVE: &str = "open.mode.primary_active";
pub const OPEN_MODE_PRIMARY_DEGRADED: &str = "open.mode.primary_degraded";
pub const OPEN_MODE_PRIMARY_FAILED: &str = "open.mode.primary_failed";

// -- Open: tool degradation --
pub const OPEN_SUMMARY_TOOL_DEGRADED: &str = "open.summary.tool_degraded";

// -- Open: degraded / warning surfaces --
/// Warning printed when post-layout pane registry sync fails. The args are
/// expected to be the underlying error message.
pub const OPEN_WARNING_RUNTIME_SYNC_FAILED: &str = "open.warning.runtime_sync_failed";
/// Header shown before the bullet list of degraded launches in the `open`
/// summary.
pub const OPEN_SUMMARY_DEGRADED_LAUNCHES_HEADER: &str = "open.summary.degraded_launches_header";

// -- Open: outcome summary --
pub const OPEN_OUTCOME_SESSION_REUSED: &str = "open.outcome.session_reused";
pub const OPEN_OUTCOME_LAYOUT_APPLIED: &str = "open.outcome.layout_applied";

// -- List: uninitialized hint --
pub const LIST_UNINITIALIZED_HINT: &str = "list.uninitialized_hint";

// -- Trusted roots / discovery setup state --
pub const TRUSTED_ROOTS_NONE_CONFIGURED: &str = "trusted_roots.none_configured";
pub const TRUSTED_ROOTS_INDEX_MISSING: &str = "trusted_roots.index_missing";
pub const TRUSTED_ROOTS_INDEX_EMPTY: &str = "trusted_roots.index_empty";
pub const TRUSTED_ROOTS_OUTSIDE: &str = "trusted_roots.outside";
/// Empty-list placeholder used inside `TRUSTED_ROOTS_OUTSIDE` when no roots
/// are configured (so the bullet list never collapses to a confusing blank
/// line).
pub const TRUSTED_ROOTS_OUTSIDE_EMPTY_LIST: &str = "trusted_roots.outside.empty_list";
/// Per-line format for each configured trusted root rendered inside the
/// `TRUSTED_ROOTS_OUTSIDE` rejection. Keeps bullet style centralized so the
/// rejection copy stays consistent across locales.
pub const TRUSTED_ROOTS_OUTSIDE_LINE_ITEM: &str = "trusted_roots.outside.line_item";
/// Interactive first-open/init trust prompt. Arg 0 is the project root that
/// would be appended to the global trusted-roots list on confirmation.
pub const TRUSTED_ROOTS_CONFIRM_ASK: &str = "trusted_roots.confirm_ask";
/// Printed after the user confirms the trust prompt and the root has been
/// persisted into the global config. Arg 0 is the persisted root.
pub const TRUSTED_ROOTS_CONFIRM_ADDED: &str = "trusted_roots.confirm_added";
pub const TRUSTED_ROOTS_LIST_HEADER: &str = "trusted_roots.list_header";
pub const TRUSTED_ROOTS_LIST_EMPTY: &str = "trusted_roots.list_empty";
pub const TRUSTED_ROOTS_ADDED: &str = "trusted_roots.added";
pub const TRUSTED_ROOTS_ALREADY_PRESENT: &str = "trusted_roots.already_present";
pub const TRUSTED_ROOTS_REMOVED: &str = "trusted_roots.removed";
pub const TRUSTED_ROOTS_NOT_PRESENT: &str = "trusted_roots.not_present";
pub const TRUSTED_ROOTS_STATUS_CONFIGURED: &str = "trusted_roots.status_configured";
pub const TRUSTED_ROOTS_STATUS_INDEX: &str = "trusted_roots.status_index";
pub const TRUSTED_ROOTS_STATUS_PROJECTS: &str = "trusted_roots.status_projects";
pub const TRUSTED_ROOTS_REJECTED_TOO_BROAD: &str = "trusted_roots.rejected_too_broad";
/// `project roots add` / `roots edit` rejection when the candidate is a
/// strict descendant of an already-trusted root. The first format arg is
/// the candidate (canonical form); the second arg is the existing
/// covering trusted root. Surfaced as
/// `TrustedRootValidation::CoveredByExistingRoot` /
/// `TrustedRootMutation::CoveredByExistingRoot` so the user understands
/// the parent already trusts the entire subtree and the redundant child
/// is intentionally not added.
pub const TRUSTED_ROOTS_COVERED_BY_EXISTING_ROOT: &str = "trusted_roots.covered_by_existing_root";
// -- Read-only setup-hint surfaces: surfaced when global config can't be
//    parsed/loaded; replaces the previously incorrect "no trusted roots
//    configured" mislabeling for unreadable config.
pub const TRUSTED_ROOTS_CONFIG_UNREADABLE: &str = "trusted_roots.config_unreadable";
// -- roots edit: aggregated affected-projects warning shown on Save when
//    staged removals affect indexed projects. --
pub const TRUSTED_ROOTS_EDIT_AFFECTED_HEADER: &str = "trusted_roots.edit_affected_header";
pub const TRUSTED_ROOTS_EDIT_AFFECTED_LINE: &str = "trusted_roots.edit_affected_line";
pub const TRUSTED_ROOTS_EDIT_AFFECTED_PROMPT: &str = "trusted_roots.edit_affected_prompt";
pub const TRUSTED_ROOTS_EDIT_AFFECTED_REQUIRES_FORCE: &str =
    "trusted_roots.edit_affected_requires_force";
// -- roots add: path validation --
pub const TRUSTED_ROOTS_PATH_MISSING: &str = "trusted_roots.path_missing";
pub const TRUSTED_ROOTS_PATH_NOT_DIRECTORY: &str = "trusted_roots.path_not_directory";
// -- roots remove: affected-project guard --
pub const TRUSTED_ROOTS_REMOVE_AFFECTED_PROMPT: &str = "trusted_roots.remove_affected_prompt";
pub const TRUSTED_ROOTS_REMOVE_AFFECTED_REQUIRES_FORCE: &str =
    "trusted_roots.remove_affected_requires_force";
pub const TRUSTED_ROOTS_REMOVE_ABORTED: &str = "trusted_roots.remove_aborted";
// -- roots edit: interactive workflow --
pub const TRUSTED_ROOTS_EDIT_HEADER: &str = "trusted_roots.edit_header";
pub const TRUSTED_ROOTS_EDIT_NO_TTY: &str = "trusted_roots.edit_no_tty";
pub const TRUSTED_ROOTS_EDIT_MENU_PROMPT: &str = "trusted_roots.edit_menu_prompt";
pub const TRUSTED_ROOTS_EDIT_MENU_ADD: &str = "trusted_roots.edit_menu_add";
pub const TRUSTED_ROOTS_EDIT_MENU_REMOVE: &str = "trusted_roots.edit_menu_remove";
pub const TRUSTED_ROOTS_EDIT_MENU_SAVE: &str = "trusted_roots.edit_menu_save";
pub const TRUSTED_ROOTS_EDIT_MENU_CANCEL: &str = "trusted_roots.edit_menu_cancel";
pub const TRUSTED_ROOTS_EDIT_ADD_PROMPT: &str = "trusted_roots.edit_add_prompt";
pub const TRUSTED_ROOTS_EDIT_REMOVE_PROMPT: &str = "trusted_roots.edit_remove_prompt";
pub const TRUSTED_ROOTS_EDIT_NOTHING_TO_REMOVE: &str = "trusted_roots.edit_nothing_to_remove";
pub const TRUSTED_ROOTS_EDIT_NO_CHANGES: &str = "trusted_roots.edit_no_changes";
pub const TRUSTED_ROOTS_EDIT_SAVED: &str = "trusted_roots.edit_saved";
pub const TRUSTED_ROOTS_EDIT_CANCELLED: &str = "trusted_roots.edit_cancelled";
// -- reindex --root: trusted-root admission --
pub const REINDEX_ROOT_NOT_TRUSTED: &str = "reindex.root_not_trusted";
// -- doctor / status: project index state --
pub const PROJECT_INDEX_STATE_PRESENT: &str = "project_index.state_present";
pub const PROJECT_INDEX_STATE_MISSING: &str = "project_index.state_missing";
pub const PROJECT_INDEX_STATE_ERROR: &str = "project_index.state_error";

// -- Init wizard: trusted roots step --
pub const INIT_TRUSTED_ROOTS_INVALID: &str = "init.trusted_roots_invalid";
pub const INIT_TRUSTED_ROOTS_CONFIGURE_NOW: &str = "init.trusted_roots_configure_now";
pub const INIT_TRUSTED_ROOTS_VIEW_MODIFY: &str = "init.trusted_roots_view_modify";

// -- Admission menu --
pub const ADMISSION_MENU_TRUST_PROJECT: &str = "admission.menu.trust_project";
pub const ADMISSION_MENU_MANAGE_DIRECTORIES: &str = "admission.menu.manage_directories";
pub const ADMISSION_MENU_CANCEL: &str = "admission.menu.cancel";
pub const ADMISSION_MENU_PROMPT: &str = "admission.menu.prompt";
pub const ADMISSION_STILL_OUTSIDE: &str = "admission.still_outside";

// -- Reconciliation status --
pub const RECONCILIATION_STATUS_IDLE: &str = "reconciliation.status.idle";
pub const RECONCILIATION_STATUS_UPDATING: &str = "reconciliation.status.updating";
pub const RECONCILIATION_STATUS_COMPLETED: &str = "reconciliation.status.completed";
pub const RECONCILIATION_STATUS_FAILED: &str = "reconciliation.status.failed";
pub const RECONCILIATION_RECOVERY_HINT: &str = "reconciliation.recovery_hint";

/// Localized variant of `DiscoveryError::RootNotFound` surfaced at the CLI
/// boundary. The authored shape (e.g. `~/development`) is rendered back to the
/// user rather than the expanded absolute path.
pub const INIT_TRUSTED_ROOTS_DISCOVERY_ROOT_MISSING: &str =
    "init.trusted_roots.discovery_root_missing";

// -- Config command --
pub const CONFIG_SHOW_HEADER: &str = "config.show_header";
pub const CONFIG_SOURCE_LINE: &str = "config.source_line";
pub const CONFIG_EDIT_ASK: &str = "config.edit_ask";
pub const CONFIG_EDIT_SUCCESS: &str = "config.edit_success";
/// `config edit` preview line that names the file the wizard will
/// actually write to, independent of whatever the effective merged
/// source currently is. Format args: 0 = scope label (Project/Global),
/// 1 = file path.
pub const CONFIG_EDIT_TARGET_LINE: &str = "config.edit_target_line";
/// `config edit` notice rendered for the default (project) flow when
/// the project config file does not yet exist, so the user understands
/// that continuing will create a fresh project override file at the
/// stated path. Format arg 0 = project config path.
pub const CONFIG_EDIT_WILL_CREATE_PROJECT_CONFIG: &str = "config.edit_will_create_project_config";
/// Localized confirmation prompt for `config edit` that explicitly
/// names the target scope and file. Replaces the older
/// `CONFIG_EDIT_ASK` text whenever the edit target is unambiguously
/// known. Format args: 0 = scope label, 1 = file path.
pub const CONFIG_EDIT_CONFIRM_TARGET: &str = "config.edit_confirm_target";
pub const CONFIG_SET_PREVIEW: &str = "config.set_preview";
pub const CONFIG_SET_SUCCESS: &str = "config.set_success";
pub const CONFIG_UNSET_SUCCESS: &str = "config.unset_success";
pub const CONFIG_ENV_SET_PREVIEW: &str = "config.env_set_preview";
pub const CONFIG_ENV_SET_SUCCESS: &str = "config.env_set_success";
pub const CONFIG_ENV_UNSET_PREVIEW: &str = "config.env_unset_preview";
pub const CONFIG_ENV_UNSET_SUCCESS: &str = "config.env_unset_success";

// -- Open / Attach / Close --
pub const OPEN_CREATING_WORKTREE: &str = "open.creating_worktree";
pub const OPEN_CREATING_SESSION: &str = "open.creating_session";
pub const OPEN_REUSING_SESSION: &str = "open.reusing_session";
pub const OPEN_SUCCESS: &str = "open.success";
/// Warning when staged replacement cleanup remains pending. Args: 0 = runtime id, 1 = error.
pub const OPEN_REPLACEMENT_CLEANUP_STAGED: &str = "open.replacement_cleanup.staged";
/// Warning when promotion cleanup remains pending. Args: 0 = runtime id, 1 = error.
pub const OPEN_REPLACEMENT_CLEANUP_PROMOTION: &str = "open.replacement_cleanup.promotion";
/// Warning when checkout-registration cleanup remains pending. Args: 0 = runtime id, 1 = error.
pub const OPEN_REPLACEMENT_CLEANUP_CHECKOUT: &str = "open.replacement_cleanup.checkout";
/// Warning when previous-runtime cleanup remains pending. Args: 0 = runtime id, 1 = error.
pub const OPEN_REPLACEMENT_CLEANUP_PREVIOUS: &str = "open.replacement_cleanup.previous";
/// Error when resolved runtime identifiers are empty.
pub const OPEN_RUNTIME_IDENTIFIERS_EMPTY: &str = "open.runtime_identifiers_empty";
/// Error when a staged tmux replacement is missing its canonical binding.
pub const OPEN_TMUX_STAGED_BINDING_MISSING: &str = "open.tmux_staged_binding_missing";
pub const OPEN_FORCE_REBUILD_REFUSED_TITLE: &str = "open.force_rebuild.refused_title";
pub const OPEN_FORCE_REBUILD_ENTER_EXISTING: &str = "open.force_rebuild.enter_existing";
pub const OPEN_FORCE_REBUILD_KEEP_EXISTING: &str = "open.force_rebuild.keep_existing";
pub const OPEN_FORCE_REBUILD_CANCEL: &str = "open.force_rebuild.cancel";
pub const OPEN_FORCE_REBUILD_NON_INTERACTIVE: &str = "open.force_rebuild.non_interactive";
pub const OPEN_SELECT_BRANCH: &str = "open.select_branch";
pub const OPEN_MAIN_BRANCH: &str = "open.main_branch";
pub const OPEN_CREATE_NEW_WT: &str = "open.create_new_wt";
pub const OPEN_ENTER_BRANCH: &str = "open.enter_branch";
pub const OPEN_CONFIRM_CREATE_WORKTREE: &str = "open.confirm_create_worktree";
/// Failure shown when creating a new branch-backed workspace in an
/// initialized repository with no commits. Format arg: 0 = workspace name.
pub const OPEN_UNBORN_REPO_CREATE_WORKTREE_BLOCKED: &str =
    "open.unborn_repo_create_worktree_blocked";
/// Prompt shown in an interactive terminal when a non-root workspace is
/// requested before the repository has a base commit and Workhive can
/// safely create an empty initial commit before continuing. Format arg:
/// 0 = requested workspace name.
pub const OPEN_UNBORN_REPO_CREATE_INITIAL_COMMIT_PROMPT: &str =
    "open.unborn_repo_create_initial_commit_prompt";
/// Prompt shown when an earlier failed first-run path left a clean managed
/// orphan worktree that must be removed before continuing. Format args:
/// 0 = workspace name.
pub const OPEN_UNBORN_REPO_REPAIR_MANAGED_WORKTREE_PROMPT: &str =
    "open.unborn_repo_repair_managed_worktree_prompt";
/// Failure shown when the managed orphan worktree has local changes and
/// Workhive refuses automatic repair. Format args: 0 = workspace name,
/// 1 = changed file count.
pub const OPEN_UNBORN_REPO_MANAGED_WORKTREE_DIRTY: &str = "open.unborn_repo_managed_worktree_dirty";
/// Prompt shown in an interactive terminal when the requested workspace
/// name matches a pre-existing local branch that Workhive does not manage.
/// Defaults to No: opening must never silently attach a branch the user
/// did not ask Workhive to manage. Format arg: 0 = branch name.
pub const OPEN_CONFIRM_ATTACH_EXISTING_BRANCH: &str = "open.confirm_attach_existing_branch";
/// Non-interactive failure shown when the requested workspace name
/// collides with a pre-existing local branch and `--attach-existing-branch`
/// was not supplied. Format arg: 0 = branch name.
pub const OPEN_ATTACH_EXISTING_BLOCKED: &str = "open.attach_existing_blocked";
/// Info line emitted when the user explicitly attaches a pre-existing
/// branch to a new managed worktree. Format arg: 0 = branch name.
pub const OPEN_BRANCH_ATTACHED: &str = "open.branch_attached";
pub const OPEN_GROUP_PROJECT_ROOT: &str = "open.group.project_root";
pub const OPEN_GROUP_WORKTREE: &str = "open.group.worktree";
pub const OPEN_GROUP_WORKTREE_EMPTY: &str = "open.group.worktree_empty";
pub const OPEN_INVALID_SELECTION: &str = "open.invalid_selection";
pub const OPEN_MAIN_DIR_MODE: &str = "open.main_dir_mode";
pub const OPEN_APPLYING_LAYOUT: &str = "open.applying_layout";
pub const OPEN_LAYOUT_APPLIED: &str = "open.layout_applied";
pub const OPEN_INITIAL_PANE_NOT_FOUND: &str = "open.initial_pane_not_found";
pub const ATTACH_NO_SESSIONS: &str = "attach.no_sessions";
pub const ATTACH_SELECT_SESSION: &str = "attach.select_session";
pub const ATTACH_CANNOT_EXTRACT_WORKSPACE: &str = "attach.cannot_extract_workspace";
pub const ATTACH_GLOBAL_SELECT: &str = "attach.global_select";
pub const ATTACH_GLOBAL_NO_INDEX: &str = "attach.global_no_index";
pub const ATTACH_GLOBAL_NO_SESSIONS: &str = "attach.global_no_sessions";
pub const ATTACH_KIND_PROJECT_ROOT: &str = "attach.kind.project_root";
pub const ATTACH_KIND_WORKTREE: &str = "attach.kind.worktree";
pub const ATTACH_KIND_UNKNOWN: &str = "attach.kind.unknown";
/// Peer-section label for attachable sessions whose name does not parse
/// into a project workspace, so they belong under neither the
/// project-root nor the worktree section.
pub const ATTACH_GROUP_OTHER: &str = "attach.group.other";
pub const ATTACH_MARKER_ATTACHED: &str = "attach.marker.attached";
/// Column header for the leading session id in attach selectors.
pub const ATTACH_COL_SESSION: &str = "attach.col.session";
/// Column header for the paraphrased workspace name.
pub const ATTACH_COL_WORKSPACE: &str = "attach.col.workspace";
/// Column header for the row's workspace kind (project root / worktree).
pub const ATTACH_COL_KIND: &str = "attach.col.kind";
/// Column header for the worktree path or project root path.
pub const ATTACH_COL_PATH: &str = "attach.col.path";
/// Column header for the attached marker.
pub const ATTACH_COL_ATTACHED: &str = "attach.col.attached";
/// Column header for the pane/layout summary.
pub const ATTACH_COL_PANES: &str = "attach.col.panes";
/// Column header for the last time Workhive opened or attached the workspace.
pub const ATTACH_COL_RECENT: &str = "attach.col.recent";
/// Column header for the project name in the global attach selector.
pub const ATTACH_COL_PROJECT: &str = "attach.col.project";
pub const CLOSE_SELECT_ACTION: &str = "close.select_action";
pub const CLOSE_ACTION_DETACH: &str = "close.action_detach";
pub const CLOSE_ACTION_KILL: &str = "close.action_kill";
pub const CLOSE_DETACHED: &str = "close.detached";
pub const CLOSE_KILLED: &str = "close.killed";

// -- List --
pub const LIST_HEADER: &str = "list.header";
pub const LIST_BACKEND: &str = "list.backend";
pub const LIST_EMPTY: &str = "list.empty";
pub const LIST_COL_WORKSPACE: &str = "list.col.workspace";
pub const LIST_COL_STATUS: &str = "list.col.status";
pub const LIST_COL_PATH: &str = "list.col.path";
pub const LIST_COL_SESSION: &str = "list.col.session";
pub const LIST_COL_LAYOUT: &str = "list.col.layout";
pub const LIST_COL_RECENT: &str = "list.col.recent";
pub const LIST_NO_RECORD: &str = "list.no_record";
/// Relative last-used label for a same-day timestamp. Argument `{0}` is the
/// local 24-hour `HH:mm` time.
pub const LIST_LAST_USED_TODAY: &str = "list.last_used.today";
/// Relative last-used label for a previous-day timestamp. Argument `{0}` is
/// the local 24-hour `HH:mm` time.
pub const LIST_LAST_USED_YESTERDAY: &str = "list.last_used.yesterday";
pub const LIST_MAIN_LABEL: &str = "list.main_label";
/// Human-output group header for the project-root workspace row.
pub const LIST_GROUP_PROJECT_ROOT: &str = "list.group.project_root";
/// Human-output group header for the linked-worktree workspace rows.
pub const LIST_GROUP_WORKTREE: &str = "list.group.worktree";
/// Human-output empty-state line shown under the worktree group when the
/// project has no linked worktree workspaces yet.
pub const LIST_GROUP_WORKTREE_EMPTY: &str = "list.group.worktree_empty";
/// Human-only placeholder for the layout column when no session is
/// running and no layout has been configured yet. Replaces the bare
/// `-` glyph in the table/compact human renderer. JSON keeps `-` for
/// backward compatibility.
pub const LIST_LAYOUT_NOT_CONFIGURED: &str = "list.layout.not_configured";
/// Localized "{0} panes" template used in the layout column when a
/// session is running with a known pane count. Replaces the previous
/// hard-coded English fragment "{c} panes". Argument 0 = pane count.
pub const LIST_LAYOUT_PANE_COUNT: &str = "list.layout.pane_count";
/// Layout-column copy when the runtime pane-count probe failed for an
/// active session. Distinct from `LIST_LAYOUT_NOT_CONFIGURED` so
/// operators can tell a degraded probe apart from "no session yet".
pub const LIST_LAYOUT_LOOKUP_FAILED: &str = "list.layout.lookup_failed";
/// Heading above the project-level configured layout diagram rendered by
/// default in human `workspace list` views. Argument 0 = template name.
pub const LIST_CONFIGURED_LAYOUT: &str = "list.configured_layout";
/// Single-line operational overview rendered after the "Workspaces:"
/// header. Arguments: 0 = total, 1 = running.
/// Localized so zh/en operators can scan workspace state without
/// inspecting individual rows.
pub const LIST_OVERVIEW_SUMMARY: &str = "list.overview.summary";
/// Variant of `LIST_OVERVIEW_SUMMARY` used when the project is not
/// initialized: only the total and the "uninitialized" hint are
/// available. Argument 0 = total.
pub const LIST_OVERVIEW_UNINITIALIZED: &str = "list.overview.uninitialized";
pub const BOOL_YES: &str = "bool.yes";
pub const BOOL_NO: &str = "bool.no";

// -- Layout template display names --
pub const LAYOUT_AI_CODING_LEFT: &str = "layout.ai_coding_left";
pub const LAYOUT_AI_CODING_DUAL: &str = "layout.ai_coding_dual";
pub const LAYOUT_HALF_SPLIT: &str = "layout.half_split";
pub const LAYOUT_QUAD: &str = "layout.quad";
pub const LAYOUT_SINGLE: &str = "layout.single";
pub const LAYOUT_CUSTOM: &str = "layout.custom";

// -- Destroy --
pub const DESTROY_SUCCESS: &str = "destroy.success";
pub const DESTROY_WARNING: &str = "destroy.warning";
/// Interactive prompt asking whether to also delete a user-owned backing
/// branch during `workspace destroy`. Defaults to No. Format arg: 0 =
/// branch name.
pub const DESTROY_CONFIRM_DELETE_BRANCH: &str = "destroy.confirm_delete_branch";
/// Info line when a Workhive-created branch is deleted during destroy.
/// Format arg: 0 = branch name.
pub const DESTROY_BRANCH_DELETED: &str = "destroy.branch_deleted";
/// Reported when a branch deletion attempted during destroy failed and the
/// branch was therefore kept. Format args: 0 = branch name, 1 = reason.
pub const DESTROY_BRANCH_KEPT: &str = "destroy.branch_kept";
pub const DESTROY_SAFETY_REPORT_HEADING: &str = "destroy.safety_report_heading";
pub const DESTROY_SAFETY_NO_RISK: &str = "destroy.safety_no_risk";
pub const DESTROY_RISKY_WORKTREES_CONFIRM: &str = "destroy.risky_worktrees_confirm";
pub const DESTROY_FORCE_DESPITE_RISK: &str = "destroy.force_despite_risk";
/// Shown when `workspace destroy` runs in a project that has no
/// `.workhive/` state at all. Destroy must never trigger init or create
/// `.workhive/config.toml`.
pub const DESTROY_NOT_INITIALIZED: &str = "destroy.not_initialized";
/// Shown when `workspace destroy` runs in a project whose `.workhive/`
/// state is partially written (e.g. instance.lock missing). Destroy must
/// not silently rewrite the state; it asks the user to repair via
/// `workhive init` first or remove `.workhive/` manually.
pub const DESTROY_PARTIAL_INIT: &str = "destroy.partial_init";
pub const SAFETY_REPORT_CHANGED_FILES: &str = "safety_report.changed_files";
pub const SAFETY_REPORT_MORE_CHANGED_FILES: &str = "safety_report.more_changed_files";
pub const SAFETY_REPORT_UNMERGED_COMMITS: &str = "safety_report.unmerged_commits";
/// High-urgency banner shown above the per-worktree risk list when any
/// worktree has dirty files or unmerged commits before a destructive
/// confirmation. Rendered in red/bold when color is available; in plain
/// text the wording itself must convey "DANGER" / "危险".
pub const SAFETY_REPORT_DANGER_HEADLINE: &str = "safety_report.danger_headline";
/// Explanation appended below `safety_report.unmerged_commits` when at
/// least one dirty file is also present. Tells the user what
/// "unmerged commits against {base}" means in their language so the
/// number is not mysterious. Format args: 0 = count, 1 = base branch.
pub const SAFETY_REPORT_UNMERGED_COMMITS_EXPLANATION: &str =
    "safety_report.unmerged_commits_explanation";
/// Distinct explanation used when a worktree reports `0` changed files
/// but still has unmerged commits — calls out that there are no
/// uncommitted working-tree changes, but the workspace branch still
/// contains commits that have not been merged into the base branch.
/// Format args: 0 = unmerged commit count, 1 = base branch name.
pub const SAFETY_REPORT_CLEAN_WORKTREE_DIVERGED_BRANCH: &str =
    "safety_report.clean_worktree_diverged_branch";

// -- Workspace remove (user-facing wording for `workspace remove`) --
//
// Workspace is the user-level concept; underlying linked git worktree
// implementation details belong in safety/diagnostic detail strings, not in
// these primary command messages.
pub const WORKSPACE_REMOVE_CONFIRM: &str = "workspace.remove.confirm";
pub const WORKSPACE_REMOVE_SUCCESS: &str = "workspace.remove.success";
pub const WORKSPACE_REMOVE_BRANCH_KEPT: &str = "workspace.remove.branch_kept";
/// Interactive prompt asking whether to also delete a user-owned backing
/// branch (pre-existing/attached/unknown provenance). Defaults to No so a
/// branch Workhive did not create is never deleted without explicit
/// consent. Format arg: 0 = branch name.
pub const WORKSPACE_REMOVE_CONFIRM_DELETE_BRANCH: &str = "workspace.remove.confirm_delete_branch";
pub const WORKSPACE_REMOVE_MAIN_FORBIDDEN: &str = "workspace.remove.main_forbidden";
pub const WORKSPACE_REMOVE_NOT_MANAGED: &str = "workspace.remove.not_managed";
pub const WORKSPACE_REMOVE_SAFETY_BLOCKED: &str = "workspace.remove.safety_blocked";
pub const WORKSPACE_REMOVE_SAFETY_REPORT_HEADING: &str = "workspace.remove.safety_report_heading";
pub const WORKSPACE_REMOVE_RISKY_CONFIRM: &str = "workspace.remove.risky_confirm";

// -- Pane --
pub const PANE_CONFIRM_SPLIT: &str = "pane.confirm_split";
pub const PANE_CONFIRM_CLOSE: &str = "pane.confirm_close";
pub const PANE_CONFIRM_SEND: &str = "pane.confirm_send";
pub const PANE_CONFIRM_SEND_RESOLVED: &str = "pane.confirm_send_resolved";
pub const PANE_SPLIT_SUCCESS: &str = "pane.split_success";
pub const PANE_CLOSE_SUCCESS: &str = "pane.close_success";
pub const PANE_SEND_SUCCESS: &str = "pane.send_success";
pub const PANE_SEND_SUCCESS_RESOLVED: &str = "pane.send_success_resolved";
/// Notice printed by the deprecated `pane approve-hooks` compatibility
/// alias. Workhive no longer installs provider-side hooks, so the
/// command performs no provider input and only explains that approval
/// is unnecessary.
pub const PANE_APPROVE_HOOKS_DEPRECATED: &str = "pane.approve_hooks.deprecated";
pub const PANE_TARGET_INDEX_NOT_FOUND: &str = "pane.target.index_not_found";
pub const PANE_TARGET_INVALID: &str = "pane.target.invalid";
/// Returned when `workhive pane send` resolves the target to the same
/// pane the command is currently running in. Sending to the current
/// pane would deliver text into the user's own shell and is rejected
/// before confirmation/dispatch.
pub const PANE_TARGET_SELF_REJECTED: &str = "pane.target.self_rejected";

// -- Pane send structured semantics --
//
// `pane send` consults `PaneMessageSafety` (already produced by the
// pane observation merge stack) and a typed resolver. The resulting
// decision MUST surface user-facing text through these keys instead of
// hardcoded English/Chinese sentences in the CLI command handler.

/// Refuse `pane send <tui-pane> "msg"`: TUI panes (lazygit, vim,
/// generic GitUi) are not structured message receivers. The CLI
/// renders this localized refusal and never dispatches the keystrokes.
/// Argument 0: the pane identifier the user typed.
pub const PANE_SEND_REFUSE_TUI: &str = "pane.send.refuse.tui";

/// Refuse `pane send` for an unverified or unknown target. Used when
/// no reliable observation can classify the pane as a safe message
/// receiver. Argument 0: the pane identifier the user typed.
pub const PANE_SEND_REFUSE_UNKNOWN: &str = "pane.send.refuse.unknown";

/// Refuse `pane send` for a recognised shell pane. Shell panes default
/// to refuse so chat-shaped text is never silently treated as an
/// arbitrary shell command.
/// Argument 0: the pane identifier the user typed.
pub const PANE_SEND_REFUSE_SHELL: &str = "pane.send.refuse.shell";

/// Localized hint appended to refusals: tells the user how to opt
/// into the explicit raw-mode override. Kept as a separate key so
/// every refusal surface uses identical phrasing.
pub const PANE_SEND_REFUSE_HINT_FORCE: &str = "pane.send.refuse.hint_force";

/// Warning rendered when `--force` overrides a shell-pane refusal.
/// The dispatch happens, but the user is told they intentionally
/// requested raw keystroke injection into a shell prompt. Argument
/// 0: the pane identifier the user typed.
pub const PANE_SEND_WARN_FORCED_SHELL: &str = "pane.send.warn.forced_shell";

/// Warning rendered before dispatching to a foreground process that
/// looks like an AI agent but lacks supervisor handshake confirmation.
/// The send still proceeds; the warning makes the unverified state
/// explicit so users do not assume a managed/protocol guarantee.
pub const PANE_SEND_WARN_UNVERIFIED_AGENT: &str = "pane.send.warn.unverified_agent";

/// Warning rendered when `--force` overrides a TUI refusal. The
/// dispatch happens but the user is told they intentionally bypassed
/// the conservative refusal.
pub const PANE_SEND_WARN_FORCED_TUI: &str = "pane.send.warn.forced_tui";

/// Warning rendered when `--force` overrides an unknown-target
/// refusal.
pub const PANE_SEND_WARN_FORCED_UNKNOWN: &str = "pane.send.warn.forced_unknown";

/// Warning rendered when `guard_force = true` overrides an activity
/// guard (`guarded_send`) block. Indicates the caller explicitly
/// bypassed the ACP-derived target-state safety check.
pub const PANE_SEND_WARN_ACTIVITY_GUARD_FORCED: &str = "pane.send.warn.activity_guard_forced";

/// Error rendered when the activity guard (`guarded_send`) blocks
/// a `pane send` because the target pane is in an unsafe-to-send state
/// (busy, tool-running, waiting for permission, stale, low-confidence,
/// or legacy source).
pub const PANE_SEND_REFUSE_ACTIVITY_GUARD: &str = "pane.send.refuse.activity_guard";

// -- Pane runtime control plane --
//
// `PANE_CONTROL_PLANE_UNAVAILABLE`: shown when `pane send` cannot reach
// the supervisor even after one self-heal retry. The CLI must NOT silently
// fall back to direct tmux in this case.
//
// `OPEN_SUMMARY_SUPERVISOR_DEGRADED`: shown by `workspace open` when the
// open flow finished but the control plane supervisor failed to start.
// Surfacing this keeps users from being surprised by a later `pane send`
// error, while making clear that the workspace itself is usable.
pub const PANE_CONTROL_PLANE_UNAVAILABLE: &str = "pane.control_plane.unavailable";
pub const PANE_CONTROL_PLANE_UNKNOWN_PANE: &str = "pane.control_plane.unknown_pane";
pub const OPEN_SUMMARY_SUPERVISOR_DEGRADED: &str = "open.summary.supervisor_degraded";

// -- Pane labels --
pub const PANE_LABEL_ID: &str = "pane.label.id";
pub const PANE_LABEL_CWD: &str = "pane.label.cwd";
pub const PANE_LABEL_COMMAND: &str = "pane.label.command";
pub const PANE_LABEL_KIND: &str = "pane.label.kind";
pub const PANE_LABEL_STATE: &str = "pane.label.state";
pub const STATUS_ACTIVE: &str = "status.active";
pub const STATUS_INACTIVE: &str = "status.inactive";

// -- Pane list/show display --
// Launch identity is rendered as a dedicated auxiliary column / label
// (`pane.list.header.launch_identity`,
// `pane.label.launch_identity`) so it cannot bleed into kind / state
// classification. The legacy `pane.list.header.title` and
// `pane.label.title` keys are retained for backwards compatibility
// in the catalog but are no longer referenced by the CLI rendering.
pub const PANE_LIST_HEADER_ACTIVE: &str = "pane.list.header.active";
pub const PANE_LIST_HEADER_TARGET_ID: &str = "pane.list.header.target_id";
pub const PANE_LIST_HEADER_NUMBER: &str = "pane.list.header.number";
pub const PANE_LIST_HEADER_KIND: &str = "pane.list.header.kind";
pub const PANE_LIST_HEADER_CWD: &str = "pane.list.header.cwd";
/// Header for the dedicated "detection source" column in `pane list`.
/// The column carries the localized product-facing token from
/// [`crate::domain::AiActivitySource::tag_i18n_key`] so users can
/// scan, per-row, where the displayed activity signal originated
/// (`hook` / `plugin` / `supervisor` / `process` / `tmux` / `none`)
/// without consulting `pane show`. Independent column instead of
/// the prior `[source/accuracy]` suffix on the activity cell so
/// the activity column stays concise on narrow terminals.
pub const PANE_LIST_HEADER_SOURCE: &str = "pane.list.header.source";
pub const PANE_LIST_HEADER_CONFIDENCE: &str = "pane.list.header.confidence";
/// Header for the unified user-recognizable label column in the
/// compact `pane list` output. Prefers Workhive launch identity
/// (e.g. `codex`, `claude`), falls back to the live foreground
/// command, and never surfaces internal `workhive` wrapper noise.
pub const PANE_LIST_HEADER_LABEL: &str = "pane.list.header.label";
/// Placeholder rendered in the label column when neither a launch
/// identity nor a non-internal foreground command is available.
pub const PANE_LIST_LABEL_NONE: &str = "pane.list.label.none";
/// The `pane list` hint block is structured so the alignment stays stable
/// across terminal widths and locales. Each key below
/// becomes a single short line; command examples are rendered
/// verbatim (untranslated) on their own indented lines so they are
/// never split mid-token by display-width-aware wrapping.
pub const PANE_LIST_HINT_HEADING: &str = "pane.list.hint.heading";
/// Single-line explanation of the leading `*` marker in the active
/// column. Lets users self-read the table without having to consult
/// docs.
pub const PANE_LIST_HINT_ACTIVE_MARKER: &str = "pane.list.hint.active_marker";
/// Single-line lead-in for the two `pane send` example commands.
pub const PANE_LIST_HINT_SEND_INTRO: &str = "pane.list.hint.send_intro";

// -- "No hook/plugin event" hint sub-block --
//
// When at least one AI pane (kind = Agent or overlay.provider set)
// is rendering with a detection source other than a fresh
// hook/plugin, `pane list` emits an extra hint sub-block under the
// table. The sub-block is intentionally conservative: it must
// describe the observable fact ("we have not received a
// hook/plugin event for these AI panes") rather than asserting
// that hooks are uninstalled — Workhive does not have proof of
// installation state from inside the pane, and asserting that
// would be wrong for users who have approved hooks but have not
// yet exercised them.
//
// The Codex line is split out because Codex >= 0.130 requires the
// user to run `/hooks` inside Codex itself to approve any hook
// the harness ships. This is a Codex-product step Workhive
// cannot perform on the user's behalf.
//
// None of these strings may mention `workhive integrations`
// (rescinded command); user-facing guidance only references the
// supported `workhive doctor` chain.

/// Heading line of the "no hook/plugin event yet" sub-block.
/// Rendered with no indent under the existing tips block so it
/// announces a separate concern.
pub const PANE_LIST_HINT_NO_HOOK_EVENT_HEADING: &str = "pane.list.hint.no_hook_event.heading";
/// Body line that explains the observable fact and points at the
/// dedicated source column for which rows are affected. Indented
/// to nest visually under the heading.
pub const PANE_LIST_HINT_NO_HOOK_EVENT_BODY: &str = "pane.list.hint.no_hook_event.body";
/// Codex-specific instruction line. Emitted only when at least
/// one affected AI pane appears to be running Codex (overlay
/// provider or current command). Tells the user they may need to
/// run `/hooks` inside Codex itself to approve Workhive-managed
/// hooks before any high-confidence event will arrive.
pub const PANE_LIST_HINT_NO_HOOK_EVENT_CODEX_REVIEW: &str =
    "pane.list.hint.no_hook_event.codex_review";

// -- "Column legend" hint sub-block --
//
// The Tips block covers `*` and the `pane send` examples; the legend
// block documents the remaining table-column semantics in user-facing
// language: the `open (waiting for event)` activity label and the
// `source` / `confidence` columns. Both blocks are emitted by
// `print_pane_list` so the table is understandable from the terminal.
//
// The legend is intentionally vocabulary-only: it does NOT describe
// per-row guidance. Hook-approval guidance lives in the separate
// `no_hook_event` sub-block above so high-confidence rows do not
// surface redundant approval prompts.

/// Heading line of the "column legend" sub-block. Rendered with no
/// indent under the existing `Tips:` block so it announces a
/// separate concern.
pub const PANE_LIST_LEGEND_HEADING: &str = "pane.list.legend.heading";
/// Single-line explanation of the `open (waiting for event)` /
/// `已打开（等待事件）` activity label. Tells the user the AI tool
/// is open but no hook/plugin event has confirmed activity yet.
/// User-facing wording is owned by [`PANE_AI_ACTIVITY_OPEN_UNVERIFIED`].
pub const PANE_LIST_LEGEND_ACTIVITY_UNVERIFIED: &str = "pane.list.legend.activity_unverified";
/// Single-line explanation of the `source` / `来源` column,
/// including the accuracy ordering of the source tags rendered in
/// that column.
pub const PANE_LIST_LEGEND_SOURCE: &str = "pane.list.legend.source";
/// Single-line explanation of the `confidence` / `可信度` column.
pub const PANE_LIST_LEGEND_CONFIDENCE: &str = "pane.list.legend.confidence";

// -- Concise default `pane list` post-table hint --
//
// `workhive pane list` defaults to a status overview, not reference
// documentation. The full Tips / Column legend / hook-event blocks
// above are reserved for `--verbose`. By default we emit a single
// short hint plus, when an AI pane is in a degraded/pre-active
// state, one short actionable line. Both must be localizable.
//
/// One-line default hint immediately below the pane table. Tells
/// users how to target a pane / send a command and points at
/// `--verbose` / `pane show <pane>` for detailed explanations.
/// Must be a single line so default `pane list` stays scannable.
pub const PANE_LIST_DEFAULT_HINT: &str = "pane.list.default_hint";

/// One-line default degraded hint emitted under the table when at
/// least one AI pane is in a pre-active lifecycle (no fresh
/// hook/plugin event yet) but the resolver does **not** specifically
/// require Codex `/hooks` approval. Points users at the `source`
/// column and `pane show <pane>` for details. Single line.
pub const PANE_LIST_DEFAULT_HINT_PENDING_EVENT: &str = "pane.list.default_hint.pending_event";

pub const PANE_LIST_EMPTY: &str = "pane.list.empty";
pub const PANE_SHOW_TITLE: &str = "pane.show.title";
pub const PANE_LABEL_INDEX: &str = "pane.label.index";
pub const PANE_LABEL_LAUNCH_IDENTITY: &str = "pane.label.launch_identity";
/// Placeholder used when no launch identity is available for a pane
/// (e.g. an `EmptyShell` launch or no durable identity recorded).
pub const PANE_LAUNCH_IDENTITY_NONE: &str = "pane.launch_identity.none";

// Pane kind localized names.
pub const PANE_KIND_AGENT: &str = "pane.kind.agent";
pub const PANE_KIND_TERMINAL: &str = "pane.kind.terminal";
pub const PANE_KIND_GIT_UI: &str = "pane.kind.git_ui";
pub const PANE_KIND_UNKNOWN: &str = "pane.kind.unknown";

// Pane state localized names.
pub const PANE_STATE_RUNNING: &str = "pane.state.running";
pub const PANE_STATE_IDLE: &str = "pane.state.idle";
pub const PANE_STATE_UNKNOWN: &str = "pane.state.unknown";

// -- Pane activity (user-facing, separate from raw PaneState) --
//
// These keys surface the typed `PaneActivityState` defined in
// `domain::pane`. They intentionally never re-use the existing
// `pane.state.running` translation: rendering "running" for every
// pane whose foreground is `codex` led users to believe the agent
// was busy.
pub const PANE_ACTIVITY_UNKNOWN: &str = "pane.activity.unknown";
pub const PANE_ACTIVITY_SHELL_IDLE: &str = "pane.activity.shell_idle";
pub const PANE_ACTIVITY_FOREGROUND_PROCESS: &str = "pane.activity.foreground_process";
/// Conservative label for an AI agent pane Workhive's supervisor
/// identifies but cannot prove is actively processing. PID liveness alone is
/// NOT a busy/processing signal, so the visible label must avoid wording such
/// as "running" / "busy" / "active".
pub const PANE_ACTIVITY_AGENT_FOREGROUND: &str = "pane.activity.agent_foreground";
pub const PANE_ACTIVITY_AGENT_BUSY: &str = "pane.activity.agent_busy";
pub const PANE_ACTIVITY_EXITED: &str = "pane.activity.exited";

// -- Pane concise activity (pane list) --
//
// Surfaced by `pane list` only. Five user-facing states designed to
// fit a narrow Activity column without parenthesized qualifiers.
// Detailed reasoning (source, confidence, AI overlay) is intentionally
// reserved for `pane show`.
pub const PANE_CONCISE_ACTIVITY_UNKNOWN: &str = "pane.concise_activity.unknown";
pub const PANE_CONCISE_ACTIVITY_WAITING_INPUT: &str = "pane.concise_activity.waiting_input";
pub const PANE_CONCISE_ACTIVITY_RUNNING: &str = "pane.concise_activity.running";
pub const PANE_CONCISE_ACTIVITY_COMPLETED: &str = "pane.concise_activity.completed";
pub const PANE_CONCISE_ACTIVITY_EXITED: &str = "pane.concise_activity.exited";

// -- Pane message safety classification (typed) --
//
// Surfaced by `pane show` so users can tell whether structured
// inter-pane messaging is allowed for a given pane. Maps to
// `PaneMessageSafety`.
pub const PANE_MESSAGE_SAFETY_UNKNOWN: &str = "pane.message_safety.unknown";
pub const PANE_MESSAGE_SAFETY_NOT_APPLICABLE: &str = "pane.message_safety.not_applicable";
pub const PANE_MESSAGE_SAFETY_SHELL_INPUT: &str = "pane.message_safety.shell_input";
pub const PANE_MESSAGE_SAFETY_UNVERIFIED_AGENT: &str = "pane.message_safety.unverified_agent";
pub const PANE_MESSAGE_SAFETY_SAFE_STRUCTURED: &str = "pane.message_safety.safe_structured";

pub const PANE_LABEL_ACTIVITY: &str = "pane.label.activity";
pub const PANE_LABEL_MESSAGE_SAFETY: &str = "pane.label.message_safety";
pub const PANE_LIST_HEADER_ACTIVITY: &str = "pane.list.header.activity";

// -- Pane runtime observation source/confidence labels --
//
// These keys localize the "source" / "confidence" fields surfaced
// by `workhive pane show` so neither English nor Chinese output
// leaks raw enum names like `Supervisor` or `ProcessObserver`.
pub const PANE_OBSERVATION_SOURCE_SUPERVISOR: &str = "pane.observation.source.supervisor";
pub const PANE_OBSERVATION_SOURCE_PROCESS_OBSERVER: &str =
    "pane.observation.source.process_observer";
pub const PANE_OBSERVATION_SOURCE_TMUX: &str = "pane.observation.source.tmux";
/// The runtime backend's own classification for a pane it hosts.
pub const PANE_OBSERVATION_SOURCE_RUNTIME_BACKEND: &str = "pane.observation.source.runtime_backend";
pub const PANE_OBSERVATION_SOURCE_UNKNOWN: &str = "pane.observation.source.unknown";

pub const PANE_OBSERVATION_CONFIDENCE_AUTHORITATIVE: &str =
    "pane.observation.confidence.authoritative";
pub const PANE_OBSERVATION_CONFIDENCE_HIGH: &str = "pane.observation.confidence.high";
pub const PANE_OBSERVATION_CONFIDENCE_FALLBACK: &str = "pane.observation.confidence.fallback";
pub const PANE_OBSERVATION_CONFIDENCE_NONE: &str = "pane.observation.confidence.none";

pub const PANE_LABEL_OBSERVATION_SOURCE: &str = "pane.label.observation_source";
pub const PANE_LABEL_OBSERVATION_CONFIDENCE: &str = "pane.label.observation_confidence";

// -- Dependency checks --
pub const DEP_CHECKING: &str = "dep.checking";
pub const DEP_ALL_OK: &str = "dep.all_ok";
pub const DEP_SATISFIED: &str = "dep.satisfied";
pub const DEP_MISSING: &str = "dep.missing";
pub const DEP_VERSION_LOW: &str = "dep.version_low";
pub const DEP_BUILTIN: &str = "dep.builtin";
pub const DEP_INSTALL_HINT: &str = "dep.install_hint";
pub const DEP_UNAVAILABLE: &str = "dep.unavailable";
pub const DEP_FATAL_MISSING: &str = "dep.fatal_missing";
pub const DEP_GIT_NAME: &str = "dep.git_name";
pub const DEP_TMUX_NAME: &str = "dep.tmux_name";

// -- Intercept --
pub const INTERCEPT_RERUN_HINT: &str = "intercept.rerun_hint";
pub const INTERCEPT_PROJECT_INIT_PROMPT: &str = "intercept.project_init_prompt";

// -- Open: project initialization --
pub const OPEN_PROJECT_INIT_PROMPT: &str = "open.project_init_prompt";
pub const OPEN_PROJECT_INIT_DENIED: &str = "open.project_init_denied";
pub const OPEN_PROJECT_INIT_DECLINED: &str = "open.project_init_declined";
pub const OPEN_PROJECT_PARTIAL_INIT: &str = "open.project_partial_init";

// -- Git init --
pub const GIT_NOT_INITIALIZED: &str = "git.not_initialized";
pub const GIT_INIT_PROMPT: &str = "git.init_prompt";
pub const GIT_INIT_SUCCESS: &str = "git.init_success";
pub const GIT_INIT_ABORT: &str = "git.init_abort";

// -- Validation messages (wizard inputs) --
pub const VALIDATION_INVALID_NUMBER: &str = "validation.invalid_number";
pub const VALIDATION_COLUMNS_RANGE: &str = "validation.columns_range";
pub const VALIDATION_INVALID_WIDTH: &str = "validation.invalid_width";
pub const VALIDATION_INVALID_ROWS: &str = "validation.invalid_rows";
pub const VALIDATION_ROWS_RANGE: &str = "validation.rows_range";

// -- Config diff --
pub const CONFIG_DIFF_BEFORE: &str = "config.diff.before";
pub const CONFIG_DIFF_AFTER: &str = "config.diff.after";

// -- Pane command preset labels --
pub const PANE_PRESET_CLAUDE: &str = "pane.preset.claude";
pub const PANE_PRESET_CODEX: &str = "pane.preset.codex";
pub const PANE_PRESET_OPENCODE: &str = "pane.preset.opencode";
pub const PANE_PRESET_CURSOR_AGENT: &str = "pane.preset.cursor_agent";
pub const PANE_PRESET_LAZYGIT: &str = "pane.preset.lazygit";
pub const PANE_PRESET_YAZI: &str = "pane.preset.yazi";
pub const PANE_PRESET_HTOP: &str = "pane.preset.htop";
pub const PANE_PRESET_BOTTOM: &str = "pane.preset.bottom";

// -- Reindex --
pub const REINDEX_CONFIRM: &str = "reindex.confirm";
pub const REINDEX_SCANNING: &str = "reindex.scanning";
pub const REINDEX_FOUND: &str = "reindex.found";
pub const REINDEX_UPDATED: &str = "reindex.updated";
pub const REINDEX_DRY_RUN: &str = "reindex.dry_run";
pub const REINDEX_NO_ROOTS: &str = "reindex.no_roots";
/// A configured trusted root was missing and will be ignored during reindex.
pub const REINDEX_MISSING_CONFIG_ROOT: &str = "reindex.missing_config_root";
/// Missing trusted roots were removed from the global configuration.
pub const REINDEX_REMOVED_CONFIG_ROOT: &str = "reindex.removed_config_root";
/// Stale project-index entries removed under missing configured roots.
pub const REINDEX_PRUNED_STALE: &str = "reindex.pruned_stale";
// Reindex result wording distinguishes discovered vs upserted vs retained vs
// final index size.
pub const REINDEX_DISCOVERED: &str = "reindex.discovered";
pub const REINDEX_UPSERTED: &str = "reindex.upserted";
pub const REINDEX_RETAINED_EXISTING: &str = "reindex.retained_existing";
pub const REINDEX_FINAL_INDEX_TOTAL: &str = "reindex.final_index_total";
pub const REINDEX_DRY_RUN_HEADING: &str = "reindex.dry_run_heading";

/// Header line printed once to stderr before a list of skipped
/// descendant subtrees. Hard-failure read errors at the top-level
/// trusted root continue to surface as `DiscoveryError` (no warning).
pub const REINDEX_WARNINGS_HEADER: &str = "reindex.warnings_header";
/// One bullet per skipped descendant subtree, with `{0}` = path and
/// `{1}` = OS-level reason (e.g. "permission denied").
pub const REINDEX_WARNING_UNREADABLE_DESCENDANT: &str = "reindex.warning.unreadable_descendant";

// -- Projects --
pub const PROJECTS_HEADER: &str = "projects.header";
pub const PROJECTS_EMPTY: &str = "projects.empty";
pub const PROJECTS_NO_INDEX: &str = "projects.no_index";
pub const PROJECTS_COL_NAME: &str = "projects.col.name";
pub const PROJECTS_COL_ROOT: &str = "projects.col.root";
pub const PROJECTS_COL_INSTANCE_ID: &str = "projects.col.instance_id";

// -- Sessions --
pub const SESSIONS_HEADER: &str = "sessions.header";
pub const SESSIONS_EMPTY: &str = "sessions.empty";
pub const SESSIONS_NO_INDEX: &str = "sessions.no_index";
// Per-project warning emitted when an indexed project's runtime backend is
// unreachable during a global session sweep. The sweep continues for other
// projects. `{0}` = project name, `{1}` = localized backend-unavailable reason.
pub const SESSIONS_COL_PROJECT: &str = "sessions.col.project";
pub const SESSIONS_COL_BACKEND: &str = "sessions.col.backend";
pub const SESSIONS_COL_WORKSPACE: &str = "sessions.col.workspace";
pub const SESSIONS_COL_SESSION: &str = "sessions.col.session";
pub const SESSIONS_COL_ATTACHED: &str = "sessions.col.attached";
// Session list includes the project path column and refined attached-client
// wording.
pub const SESSIONS_COL_PATH: &str = "sessions.col.path";
pub const SESSIONS_COL_CLIENT: &str = "sessions.col.client";
pub const SESSIONS_ATTACHED_YES: &str = "sessions.attached.yes";
pub const SESSIONS_ATTACHED_NO: &str = "sessions.attached.no";
// Session attach (global) — mirrors workspace attach copy but explicitly global.
pub const SESSION_ATTACH_NO_INDEX: &str = "session.attach.no_index";
pub const SESSION_ATTACH_NO_SESSIONS: &str = "session.attach.no_sessions";
pub const SESSION_ATTACH_AMBIGUOUS: &str = "session.attach.ambiguous";
pub const SESSION_ATTACH_NOT_FOUND: &str = "session.attach.not_found";
pub const SESSION_ATTACH_SELECT: &str = "session.attach.select";
pub const SESSION_ATTACH_COL_TARGET: &str = "session.attach.col.target";
pub const SESSION_ATTACH_COL_DETAILS: &str = "session.attach.col.details";
/// `{0}` = workspace count in the Herdr session.
pub const SESSION_ATTACH_HERDR_DETAILS: &str = "session.attach.herdr_details";
pub const SESSION_ATTACH_NO_TTY: &str = "session.attach.no_tty";
pub const SESSION_ATTACH_HINT_GLOBAL: &str = "session.attach.hint_global";
/// Concise guidance printed before handing terminal control to tmux.
/// Tells users how to detach (return to caller shell) and how to exit/close.
pub const ATTACH_GUIDANCE: &str = "attach.guidance";
/// Informational: user is already in the target workspace session.
pub const ATTACH_ALREADY_CURRENT: &str = "attach.already_current";
/// Error: cross-group (cross-server) navigation is not supported. Args:
/// current server, target server.
pub const ATTACH_CROSS_GROUP_REJECTED: &str = "attach.cross_group_rejected";
/// Error: partial tmux context (server without session or vice versa).
pub const ATTACH_PARTIAL_CONTEXT_ERROR: &str = "attach.partial_context_error";

// -- Worktree --
pub const WORKTREE_LIST_HEADER: &str = "worktree.list_header";
pub const WORKTREE_LIST_EMPTY: &str = "worktree.list_empty";
pub const WORKTREE_COL_NAME: &str = "worktree.col.name";
pub const WORKTREE_COL_BRANCH: &str = "worktree.col.branch";
pub const WORKTREE_COL_PATH: &str = "worktree.col.path";
pub const WORKTREE_COL_KIND: &str = "worktree.col.kind";
pub const WORKTREE_KIND_MAIN: &str = "worktree.kind.main";
pub const WORKTREE_KIND_LINKED: &str = "worktree.kind.linked";
pub const WORKTREE_REMOVE_NO_TTY: &str = "worktree.remove.no_tty";
pub const WORKTREE_REMOVE_NONE_AVAILABLE: &str = "worktree.remove.none_available";
pub const WORKTREE_REMOVE_SELECT: &str = "worktree.remove.select";
pub const WORKTREE_REMOVE_CONFIRM: &str = "worktree.remove.confirm";
pub const WORKTREE_REMOVE_SUCCESS: &str = "worktree.remove.success";
pub const WORKTREE_REMOVE_NOT_FOUND: &str = "worktree.remove.not_found";
pub const WORKTREE_REMOVE_BLOCKED_RISKY: &str = "worktree.remove.blocked_risky";
pub const WORKTREE_REMOVE_NOT_MANAGED: &str = "worktree.remove.not_managed";
pub const WORKTREE_REMOVE_BRANCH_DELETED: &str = "worktree.remove.branch_deleted";
pub const WORKTREE_REMOVE_BRANCH_DELETE_FAILED: &str = "worktree.remove.branch_delete_failed";

/// Help text for the canonical `workhive workspace worktree` group.
pub const CLI_HELP_CMD_WORKSPACE_WORKTREE: &str = "cli.help.cmd.workspace.worktree";
pub const CLI_HELP_CMD_WORKSPACE_WORKTREE_LIST: &str = "cli.help.cmd.workspace.worktree.list";
pub const CLI_HELP_CMD_WORKSPACE_WORKTREE_REMOVE: &str = "cli.help.cmd.workspace.worktree.remove";
pub const CLI_HELP_CMD_SESSION_ATTACH: &str = "cli.help.cmd.session.attach";
pub const CLI_HELP_CMD_SESSION_RESUME: &str = "cli.help.cmd.session.resume";
pub const CLI_HELP_CMD_PROJECT_ROOTS: &str = "cli.help.cmd.project.roots";
pub const CLI_HELP_CMD_PROJECT_ROOTS_LIST: &str = "cli.help.cmd.project.roots.list";
pub const CLI_HELP_CMD_PROJECT_ROOTS_ADD: &str = "cli.help.cmd.project.roots.add";
pub const CLI_HELP_CMD_PROJECT_ROOTS_REMOVE: &str = "cli.help.cmd.project.roots.remove";
pub const CLI_HELP_CMD_PROJECT_ROOTS_EDIT: &str = "cli.help.cmd.project.roots.edit";
pub const CLI_HELP_CMD_PROJECT_ROOTS_STATUS: &str = "cli.help.cmd.project.roots.status";

// -- CLI help argument descriptions for new commands --
pub const CLI_ARG_WORKTREE_NAME_OPT: &str = "cli.arg.worktree.name_opt";
pub const CLI_ARG_WORKTREE_FORCE: &str = "cli.arg.worktree.force";
pub const CLI_ARG_WORKTREE_DELETE_BRANCH: &str = "cli.arg.worktree.delete_branch";
pub const CLI_ARG_WORKTREE_KEEP_BRANCH: &str = "cli.arg.worktree.keep_branch";
pub const CLI_ARG_SESSION_LIST_VIEW: &str = "cli.arg.session.list.view";
pub const CLI_ARG_SESSION_ATTACH_TARGET: &str = "cli.arg.session.attach.target";
pub const CLI_ARG_SESSION_RESUME_SESSION: &str = "cli.arg.session.resume.session";
pub const CLI_ARG_PROJECT_ROOTS_PATH: &str = "cli.arg.project.roots.path";

// -- Index warnings --
pub const INDEX_STALE_ENTRY: &str = "index.stale_entry";
/// Stale index entry whose project root no longer exists on disk.
pub const INDEX_STALE_ENTRY_PROJECT_ROOT_GONE: &str = "index.stale_entry.project_root_gone";

// -- Layout executor runtime messages --
pub const ERROR_LAYOUT_COLUMN_NOT_CREATED: &str = "error.layout.column_not_created";

// -- Doctor --
pub const DOCTOR_HEADER: &str = "doctor.header";
pub const DOCTOR_CATEGORY_REMOTE: &str = "doctor.category.remote";
pub const DOCTOR_STATUS_PASS: &str = "doctor.status.pass";
pub const DOCTOR_STATUS_WARN: &str = "doctor.status.warn";
pub const DOCTOR_STATUS_FAIL: &str = "doctor.status.fail";
pub const DOCTOR_CHECK_RUNTIME_BACKEND: &str = "doctor.check.runtime_backend";
pub const DOCTOR_CHECK_RUNTIME_SCOPE: &str = "doctor.check.runtime_scope";
pub const DOCTOR_CHECK_RUNTIME_BINDING: &str = "doctor.check.runtime_binding";
pub const DOCTOR_CHECK_SESSION_LOOKUP: &str = "doctor.check.session_lookup";
pub const DOCTOR_CHECK_PTY_AVAILABILITY: &str = "doctor.check.pty_availability";
pub const DOCTOR_CHECK_ATTACH_READINESS: &str = "doctor.check.attach_readiness";
pub const DOCTOR_ADVICE_RUNTIME_ERROR: &str = "doctor.advice.runtime_error";
pub const DOCTOR_ADVICE_RUNTIME_BINDING_MISMATCH: &str = "doctor.advice.runtime_binding_mismatch";
pub const DOCTOR_ADVICE_LOOKUP_FAILED: &str = "doctor.advice.lookup_failed";
pub const DOCTOR_ADVICE_SESSION_MISSING: &str = "doctor.advice.session_missing";
pub const DOCTOR_ADVICE_SESSION_SERVER_UNREACHABLE: &str =
    "doctor.advice.session_server_unreachable";
pub const DOCTOR_ADVICE_SESSION_LOOKUP_FAILED: &str = "doctor.advice.session_lookup_failed";
pub const DOCTOR_ADVICE_PTY_EXHAUSTED: &str = "doctor.advice.pty_exhausted";
pub const DOCTOR_ADVICE_PTY_ELEVATED: &str = "doctor.advice.pty_elevated";
pub const DOCTOR_ADVICE_NESTED_TMUX: &str = "doctor.advice.nested_tmux";
pub const DOCTOR_ADVICE_INVALID_WORKSPACE_NAME: &str = "doctor.advice.invalid_workspace_name";
pub const DOCTOR_CHECK_WORKSPACE_IDENTITY: &str = "doctor.check.workspace_identity";
pub const DOCTOR_ALL_PASSED: &str = "doctor.all_passed";
pub const DOCTOR_HAS_ISSUES: &str = "doctor.has_issues";
/// Notice printed when the project has no Workhive init yet.
pub const DOCTOR_PROJECT_NOT_INITIALIZED: &str = "doctor.project_not_initialized";

// -- Completions: runtime messages --
/// Summary line printed after a successful completion install. Args:
/// `{0}` shell name, `{1}` absolute script path.
pub const COMPLETIONS_INSTALLED: &str = "completions.installed";
/// Follow-up line printed beneath `COMPLETIONS_INSTALLED` with a
/// shell-specific activation hint.
pub const COMPLETIONS_ACTIVATE_ZSH_RC: &str = "completions.activate.zsh.rc";
pub const COMPLETIONS_ACTIVATE_ZSH_NO_RC: &str = "completions.activate.zsh.no_rc";
pub const COMPLETIONS_ACTIVATE_BASH: &str = "completions.activate.bash";
pub const COMPLETIONS_ACTIVATE_FISH: &str = "completions.activate.fish";
pub const COMPLETIONS_ACTIVATE_POWERSHELL: &str = "completions.activate.powershell";
pub const COMPLETIONS_ACTIVATE_ELVISH: &str = "completions.activate.elvish";
pub const COMPLETIONS_ACTIVATE_GENERIC: &str = "completions.activate.generic";

// -- List: session state --
pub const LIST_SESSION_RUNNING: &str = "list.session.running";
pub const LIST_SESSION_MISSING: &str = "list.session.missing";
pub const LIST_SESSION_UNREACHABLE: &str = "list.session.unreachable";
pub const LIST_SESSION_LOOKUP_FAILED: &str = "list.session.lookup_failed";
pub const LIST_COL_SESSION_STATE: &str = "list.col.session_state";

// -- Error: session lookup --
pub const ERROR_SESSION_LOOKUP_RUNNING: &str = "error.session.running";
pub const ERROR_SESSION_LOOKUP_MISSING: &str = "error.session.missing";
pub const ERROR_SESSION_LOOKUP_UNREACHABLE: &str = "error.session.unreachable";
pub const ERROR_SESSION_LOOKUP_FAILED: &str = "error.session.lookup_failed";

// -- Doctor: typed errors --
pub const DOCTOR_ERROR_PROJECT_NOT_DETECTED: &str = "doctor.error.project_not_detected";

// -- Error: session lookup (CLI rendering) --
pub const ERROR_SESSION_LOOKUP_FAILED_MSG: &str = "error.session_lookup_failed_msg";

// -- Open: recovery path --
pub const OPEN_RECOVERY_LIVE_SESSION: &str = "open.recovery.live_session";
pub const OPEN_RECOVERY_LAYOUT: &str = "open.recovery.layout";

// -- Reattach outcome --
pub const REATTACH_UNRECOVERABLE: &str = "reattach.unrecoverable";

// -- Recovery action --
pub const RECOVERY_ACTION_RECOVERABLE: &str = "recovery.action.recoverable";
pub const RECOVERY_ACTION_MANUAL: &str = "recovery.action.manual";

// -- Config overview --
pub const CONFIG_OVERVIEW_TITLE: &str = "config.overview.title";
pub const CONFIG_OVERVIEW_SCOPE_GLOBAL: &str = "config.overview.scope_global";
pub const CONFIG_OVERVIEW_SCOPE_PROJECT: &str = "config.overview.scope_project";
pub const CONFIG_OVERVIEW_LANGUAGE: &str = "config.overview.language";
pub const CONFIG_OVERVIEW_DEFAULT: &str = "config.overview.default";
pub const CONFIG_OVERVIEW_TMUX: &str = "config.overview.tmux";
pub const CONFIG_OVERVIEW_LAYOUT: &str = "config.overview.layout";
pub const CONFIG_OVERVIEW_LAYOUT_NONE: &str = "config.overview.layout_none";
pub const CONFIG_OVERVIEW_TOOLS: &str = "config.overview.tools";
pub const CONFIG_OVERVIEW_TOOLS_NONE: &str = "config.overview.tools_none";
pub const CONFIG_OVERVIEW_TOOLS_COUNT: &str = "config.overview.tools_count";
pub const CONFIG_OVERVIEW_LOGGING: &str = "config.overview.logging";
pub const CONFIG_OVERVIEW_LOGGING_DEFAULTS: &str = "config.overview.logging_defaults";
pub const CONFIG_OVERVIEW_ENABLED: &str = "config.overview.enabled";
pub const CONFIG_OVERVIEW_DISABLED: &str = "config.overview.disabled";
pub const CONFIG_OVERVIEW_TIP_EDIT: &str = "config.overview.tip_edit";
pub const CONFIG_OVERVIEW_TIP_RAW: &str = "config.overview.tip_raw";
pub const CONFIG_OVERVIEW_PANE_SHELL: &str = "config.overview.pane_shell";
pub const CONFIG_OVERVIEW_PANE_TOOL: &str = "config.overview.pane_tool";
pub const CONFIG_OVERVIEW_PANE_CMD: &str = "config.overview.pane_cmd";
pub const CONFIG_OVERVIEW_TOOL_MISSING_EXECUTABLE: &str = "config.overview.tool_missing_executable";
pub const CONFIG_OVERVIEW_RETENTION_IGNORED: &str = "config.overview.retention_ignored";
pub const CONFIG_OVERVIEW_GLOBAL_PATH: &str = "config.overview.global_path";
pub const CONFIG_OVERVIEW_PROJECT_PATH: &str = "config.overview.project_path";
pub const CONFIG_OVERVIEW_EFFECTIVE_SOURCE: &str = "config.overview.effective_source";
pub const CONFIG_OVERVIEW_FIELD_SERVER_NAME: &str = "config.overview.field.server_name";
pub const CONFIG_OVERVIEW_FIELD_SERVER_TEMPLATE: &str = "config.overview.field.server_template";
pub const CONFIG_OVERVIEW_FIELD_SESSION_TEMPLATE: &str = "config.overview.field.session_template";
pub const CONFIG_OVERVIEW_FIELD_LAYOUT_TEMPLATE: &str = "config.overview.field.layout_template";
pub const CONFIG_OVERVIEW_FIELD_COLUMNS: &str = "config.overview.field.columns";
pub const CONFIG_OVERVIEW_FIELD_COLUMN: &str = "config.overview.field.column";
pub const CONFIG_OVERVIEW_FIELD_PANE: &str = "config.overview.field.pane";
pub const CONFIG_OVERVIEW_FIELD_TIMED_INTERVAL: &str = "config.overview.field.timed_interval";
pub const CONFIG_OVERVIEW_FIELD_SAVE_ON_EVENT: &str = "config.overview.field.save_on_event";
pub const CONFIG_OVERVIEW_FIELD_SAVE_ON_COMMAND_CHANGE: &str =
    "config.overview.field.save_on_command_change";
pub const CONFIG_OVERVIEW_FIELD_DEBOUNCE_MS: &str = "config.overview.field.debounce_ms";
pub const CONFIG_OVERVIEW_FIELD_RETAIN_REVISIONS: &str = "config.overview.field.retain_revisions";
pub const CONFIG_OVERVIEW_FIELD_RETAIN_EVENT_DAYS: &str = "config.overview.field.retain_event_days";
pub const CONFIG_OVERVIEW_FIELD_CONSOLE: &str = "config.overview.field.console";
pub const CONFIG_OVERVIEW_FIELD_FILE: &str = "config.overview.field.file";
pub const CONFIG_OVERVIEW_FIELD_ENABLED: &str = "config.overview.field.enabled";
pub const CONFIG_OVERVIEW_FIELD_LEVEL: &str = "config.overview.field.level";
pub const CONFIG_OVERVIEW_FIELD_FORMAT: &str = "config.overview.field.format";
pub const CONFIG_OVERVIEW_FIELD_BASE_NAME: &str = "config.overview.field.base_name";
pub const CONFIG_OVERVIEW_FIELD_ROTATION: &str = "config.overview.field.rotation";
pub const CONFIG_OVERVIEW_FIELD_RETENTION: &str = "config.overview.field.retention";
pub const CONFIG_OVERVIEW_FIELD_SCOPE: &str = "config.overview.field.scope";

// -- Errors --
pub const ERROR_NOT_IN_SESSION: &str = "error.not_in_session";
pub const ERROR_SESSION_NOT_FOUND: &str = "error.session_not_found";
pub const ERROR_NOT_INITIALIZED: &str = "error.not_initialized";
pub const ERROR_GLOBAL_NOT_INITIALIZED: &str = "error.global_not_initialized";
pub const ERROR_PROJECT_NOT_INITIALIZED_HINT: &str = "error.project_not_initialized_hint";

// -- Identity errors --
pub const ERROR_IDENTITY_PROJECT_ROOT_MISMATCH: &str = "error.identity.project_root_mismatch";
pub const ERROR_IDENTITY_PROJECT_ROOT_MISMATCH_HELP: &str =
    "error.identity.project_root_mismatch.help";
pub const ERROR_IDENTITY_INSTANCE_ID_CONFLICT: &str = "error.identity.instance_id_conflict";
pub const ERROR_IDENTITY_INSTANCE_ID_CONFLICT_HELP: &str =
    "error.identity.instance_id_conflict.help";
pub const ERROR_IDENTITY_PROJECT_IDENTITY_MISSING: &str = "error.identity.project_identity_missing";
pub const ERROR_IDENTITY_PROJECT_IDENTITY_MISSING_HELP: &str =
    "error.identity.project_identity_missing.help";
pub const ERROR_IDENTITY_CORRUPTED: &str = "error.identity.corrupted";
pub const ERROR_IDENTITY_CORRUPTED_HELP: &str = "error.identity.corrupted.help";
pub const ERROR_IDENTITY_GENERIC: &str = "error.identity.generic";
pub const ERROR_IDENTITY_GENERIC_HELP: &str = "error.identity.generic.help";

// -- Doctor fix prompts --
pub const DOCTOR_FIX_ROOT_MISMATCH_DETECTED: &str = "doctor.fix.root_mismatch_detected";
pub const DOCTOR_FIX_ROOT_MISMATCH_PROMPT: &str = "doctor.fix.root_mismatch_prompt";
pub const DOCTOR_FIX_ROOT_UPDATED: &str = "doctor.fix.root_updated";
pub const DOCTOR_FIX_ROOT_SKIPPED: &str = "doctor.fix.root_skipped";
pub const DOCTOR_FIX_HINT: &str = "doctor.fix.hint";
pub const DOCTOR_FIX_STORED_ROOT_LABEL: &str = "doctor.fix.stored_root_label";
pub const DOCTOR_FIX_CURRENT_ROOT_LABEL: &str = "doctor.fix.current_root_label";

// -- CLI help (clap localization) --
//
// These keys back the zh-CN rewrite of clap-rendered `--help`, usage, and
// command short descriptions. Command names and flags themselves remain
// English (governance: command identifiers are not translated).
pub const CLI_ABOUT: &str = "cli.about";
pub const CLI_HELP_TEMPLATE: &str = "cli.help.template";
/// Default nested help template: command has both visible subcommands
/// AND visible positional arguments. Renders the `Commands:` and
/// `Arguments:` headings.
pub const CLI_HELP_NESTED_TEMPLATE: &str = "cli.help.nested_template";
/// Nested help template variant: command has visible subcommands but
/// no visible positional arguments. Renders the `Commands:` heading
/// only and omits `Arguments:` so leaf categories such as
/// `workhive pane --help` do not show an empty `参数:` section.
pub const CLI_HELP_NESTED_TEMPLATE_SUBCOMMANDS_ONLY: &str =
    "cli.help.nested_template.subcommands_only";
/// Nested help template variant: command has visible positional
/// arguments but no visible subcommands. Renders the `Arguments:`
/// heading only and omits `Commands:` so leaf commands such as
/// `workhive pane send --help` do not show an empty `命令:` section.
pub const CLI_HELP_NESTED_TEMPLATE_POSITIONALS_ONLY: &str =
    "cli.help.nested_template.positionals_only";
/// Nested help template variant: command has neither visible
/// subcommands nor visible positional arguments. Renders only the
/// usage/options chrome so leaf commands such as
/// `workhive pane list --help` do not show empty `命令:` and `参数:`
/// sections.
pub const CLI_HELP_NESTED_TEMPLATE_OPTIONS_ONLY: &str = "cli.help.nested_template.options_only";
pub const CLI_HELP_OPTIONS_HEADING: &str = "cli.help.heading.options";
pub const CLI_HELP_ARGUMENTS_HEADING: &str = "cli.help.heading.arguments";
pub const CLI_HELP_COMMANDS_HEADING: &str = "cli.help.heading.commands";

// -- CLI help: top-level subcommand short descriptions --
pub const CLI_HELP_CMD_INIT: &str = "cli.help.cmd.init";
pub const CLI_HELP_CMD_CONFIG: &str = "cli.help.cmd.config";
pub const CLI_HELP_CMD_DOCTOR: &str = "cli.help.cmd.doctor";
pub const CLI_HELP_CMD_COMPLETION: &str = "cli.help.cmd.completion";
pub const CLI_HELP_CMD_COMPLETION_INSTALL: &str = "cli.help.cmd.completion.install";
pub const CLI_HELP_CMD_COMPLETION_SCRIPT: &str = "cli.help.cmd.completion.script";
pub const CLI_HELP_CMD_PROJECT: &str = "cli.help.cmd.project";
pub const CLI_HELP_CMD_WORKSPACE: &str = "cli.help.cmd.workspace";
pub const CLI_HELP_CMD_SESSION: &str = "cli.help.cmd.session";
pub const CLI_HELP_CMD_RUNTIME: &str = "cli.help.cmd.runtime";
pub const CLI_HELP_CMD_PANE: &str = "cli.help.cmd.pane";

// -- CLI help: project subcommands --
pub const CLI_HELP_CMD_PROJECT_LIST: &str = "cli.help.cmd.project.list";
pub const CLI_HELP_CMD_PROJECT_REINDEX: &str = "cli.help.cmd.project.reindex";

// -- CLI help: workspace subcommands --
pub const CLI_HELP_CMD_WORKSPACE_OPEN: &str = "cli.help.cmd.workspace.open";
pub const CLI_HELP_CMD_WORKSPACE_LIST: &str = "cli.help.cmd.workspace.list";
pub const CLI_HELP_CMD_WORKSPACE_ATTACH: &str = "cli.help.cmd.workspace.attach";
pub const CLI_HELP_CMD_WORKSPACE_CLOSE: &str = "cli.help.cmd.workspace.close";
pub const CLI_HELP_CMD_WORKSPACE_REMOVE: &str = "cli.help.cmd.workspace.remove";
pub const CLI_HELP_CMD_WORKSPACE_DESTROY: &str = "cli.help.cmd.workspace.destroy";

// -- CLI help: session subcommands --
pub const CLI_HELP_CMD_SESSION_LIST: &str = "cli.help.cmd.session.list";
pub const CLI_HELP_CMD_RUNTIME_STOP: &str = "cli.help.cmd.runtime.stop";
pub const CLI_ARG_RUNTIME_FORCE: &str = "cli.arg.runtime.force";

// -- CLI help: pane subcommands --
pub const CLI_HELP_CMD_PANE_SPLIT: &str = "cli.help.cmd.pane.split";
pub const CLI_HELP_CMD_PANE_CLOSE: &str = "cli.help.cmd.pane.close";
pub const CLI_HELP_CMD_PANE_LIST: &str = "cli.help.cmd.pane.list";
pub const CLI_HELP_CMD_PANE_SHOW: &str = "cli.help.cmd.pane.show";
pub const CLI_HELP_CMD_PANE_SEND: &str = "cli.help.cmd.pane.send";
/// About string for the hidden `pane approve-hooks` compatibility
/// alias. The key intentionally exists despite `approve-hooks` being
/// hidden from `pane --help`: direct invocation of
/// `workhive pane approve-hooks --help` is still possible and the
/// direct help must be fully localized in both en and zh-CN. It
/// frames the command as deprecated/compatibility-only, not as a
/// regular user workflow, and includes a Codex-trust reminder so
/// users do not believe Workhive can bypass Codex `/hooks` approval.
pub const CLI_HELP_CMD_PANE_APPROVE_HOOKS: &str = "cli.help.cmd.pane.approve_hooks";
/// Help text for the single positional `<PANE>` argument on the
/// hidden `pane approve-hooks` alias. Localized for the same reason
/// as `CLI_HELP_CMD_PANE_APPROVE_HOOKS` above — direct
/// `--help` invocation must not leak the English derive default.
pub const CLI_ARG_PANE_APPROVE_HOOKS_PANE: &str = "cli.arg.pane.approve_hooks.pane";

// -- CLI help: config subcommands --
pub const CLI_HELP_CMD_CONFIG_SHOW: &str = "cli.help.cmd.config.show";
pub const CLI_HELP_CMD_CONFIG_EDIT: &str = "cli.help.cmd.config.edit";
pub const CLI_HELP_CMD_CONFIG_SET: &str = "cli.help.cmd.config.set";
pub const CLI_HELP_CMD_CONFIG_UNSET: &str = "cli.help.cmd.config.unset";
pub const CLI_HELP_CMD_CONFIG_ENV_SET: &str = "cli.help.cmd.config.env_set";
pub const CLI_HELP_CMD_CONFIG_ENV_UNSET: &str = "cli.help.cmd.config.env_unset";
pub const CLI_HELP_CMD_CONFIG_EXPLAIN: &str = "cli.help.cmd.config.explain";

// -- Open: progress + bounded readiness --
//
// Surfaced before/around `attach` to make `workhive workspace open`
// feel responsive. The wait is bounded by [`OPEN_READINESS_DEFAULT_MS`]
// and never blocks attach indefinitely; on timeout the user sees a
// degraded-mode warning instead of a silent hang.
pub const OPEN_PROGRESS_WAITING_PANES: &str = "open.progress.waiting_panes";
pub const OPEN_PROGRESS_PANES_READY: &str = "open.progress.panes_ready";
pub const OPEN_PROGRESS_READINESS_TIMEOUT: &str = "open.progress.readiness_timeout";
pub const OPEN_PROGRESS_READINESS_DEGRADED: &str = "open.progress.readiness_degraded";
/// Deterministic progress line shown under `--no-attach`. The readiness
/// wait is intentionally skipped (no attach to gate against), but the
/// summary still tells the user what was provisioned and how to attach
/// later.
pub const OPEN_PROGRESS_NO_ATTACH_SUMMARY: &str = "open.progress.no_attach_summary";
/// Deterministic non-interactive failure for `workspace open` without a
/// workspace name: the interactive selector requires a TTY, so scripted
/// and agent callers must pass the workspace name explicitly.
pub const OPEN_SELECTOR_NON_INTERACTIVE: &str = "open.selector.non_interactive";

// -- Workspace open: .env observability --
// All three are emitted at user-visible level. They show counts and key
// names only; raw values are never rendered. The key list rendering
// reuses [`OPEN_ENV_KEY_ITEM`] per key.
pub const OPEN_ENV_INJECTED_GLOBAL_ONLY: &str = "open.env.injected_global_only";
pub const OPEN_ENV_INJECTED_PROJECT_ONLY: &str = "open.env.injected_project_only";
pub const OPEN_ENV_INJECTED_MIXED: &str = "open.env.injected_mixed";
pub const OPEN_ENV_KEY_ITEM: &str = "open.env.key_item";
pub const OPEN_ENV_PARSE_WARNING: &str = "open.env.parse_warning";
// Localized reason text used as the third argument to
// `OPEN_ENV_PARSE_WARNING`. We expose one key per `EnvParseReason`
// variant so user-visible warnings are translated, never the stable
// English `EnvParseReason::description()` text. The warning may include the
// source path, line number, and typed reason but MUST NOT echo the raw line or
// its value.
pub const OPEN_ENV_PARSE_REASON_LOOKS_LIKE_YAML: &str = "open.env.parse_reason.looks_like_yaml";
pub const OPEN_ENV_PARSE_REASON_MISSING_EQUALS: &str = "open.env.parse_reason.missing_equals";
pub const OPEN_ENV_PARSE_REASON_EMPTY_KEY: &str = "open.env.parse_reason.empty_key";
pub const OPEN_ENV_PARSE_REASON_INVALID_KEY_CHARS: &str = "open.env.parse_reason.invalid_key_chars";

// -- Workspace destroy: pre-flight short-circuit when project state is absent.
// `DESTROY_NO_WORKHIVE_STATE` is the localized message printed when the
// project root has no `.workhive/` directory.

// -- Trusted-roots wizard interaction --
//
// The original `init.trusted_roots_*` vocabulary above already covers
// `_PROMPT`, `_ENTRY_ASK`, `_DECLINE_REPROMPT`, `_FOOTER_HINT`,
// `_NO_MATCH`, `_PATH_MISSING`, `_PATH_NOT_DIR`, `_DUPLICATE`,
// `_REQUIRE_AT_LEAST_ONE`, `_INVALID`, and `_FINISH_PROMPT`. Only the
// new "reject relative path" and "reject overly broad path" cases need
// fresh keys — everything else reuses the existing strings.

// -- CLI parse failure hints --
//
// These keys back the Workhive-owned localized renderer for clap parse
// errors. When `language = "zh-CN"`, Workhive intercepts every clap
// `Error` and emits its own short Chinese error block instead of the
// upstream English one. The keys cover unknown subcommand, removed
// command (formerly top-level), unknown option, missing argument,
// invalid value, and a generic catch-all. The "help_hint" key is
// reused as the universal "查看帮助" tail line.
pub const CLI_PARSE_HELP_HINT: &str = "cli.parse.help_hint";
pub const CLI_PARSE_ERROR_PREFIX: &str = "cli.parse.error_prefix";
pub const CLI_PARSE_USAGE_HEADING: &str = "cli.parse.usage_heading";
pub const CLI_PARSE_UNKNOWN_SUBCOMMAND: &str = "cli.parse.unknown_subcommand";
pub const CLI_PARSE_UNKNOWN_SUBCOMMAND_TIP: &str = "cli.parse.unknown_subcommand_tip";
pub const CLI_PARSE_UNKNOWN_OPTION: &str = "cli.parse.unknown_option";
pub const CLI_PARSE_MISSING_REQUIRED: &str = "cli.parse.missing_required";
pub const CLI_PARSE_INVALID_VALUE: &str = "cli.parse.invalid_value";
pub const CLI_PARSE_BACKEND_UNAVAILABLE: &str = "cli.parse.backend_unavailable";
pub const CLI_PARSE_GENERIC: &str = "cli.parse.generic";

// -- CLI help: option/argument descriptions for public commands --
//
// clap renders argument-level descriptions verbatim from the
// `#[arg(...)]` attribute. To keep zh-CN help fully Chinese without
// translating flag tokens, these keys hold the localized description
// strings injected into the command tree at parse time. Command
// names and flag names themselves are NOT translated.
pub const CLI_ARG_VERBOSE: &str = "cli.arg.verbose";
pub const CLI_ARG_HELP: &str = "cli.arg.help";
pub const CLI_ARG_VERSION: &str = "cli.arg.version";
pub const CLI_ARG_FORCE: &str = "cli.arg.force";
pub const CLI_ARG_INIT_GLOBAL: &str = "cli.arg.init.global";
pub const CLI_ARG_WORKSPACE_NAME: &str = "cli.arg.workspace.name";
pub const CLI_ARG_WORKSPACE_NAME_OPT: &str = "cli.arg.workspace.name_opt";
pub const CLI_ARG_OPEN_NO_ATTACH: &str = "cli.arg.open.no_attach";
pub const CLI_ARG_OPEN_FORCE: &str = "cli.arg.open.force";
pub const CLI_ARG_OPEN_MODE: &str = "cli.arg.open.mode";
pub const CLI_ARG_OPEN_INIT: &str = "cli.arg.open.init";
pub const CLI_ARG_OPEN_ATTACH_EXISTING_BRANCH: &str = "cli.arg.open.attach_existing_branch";
pub const CLI_ARG_LIST_VIEW: &str = "cli.arg.list.view";
pub const CLI_ARG_REMOVE_DELETE_BRANCH: &str = "cli.arg.remove.delete_branch";
pub const CLI_ARG_REMOVE_KEEP_BRANCH: &str = "cli.arg.remove.keep_branch";
pub const CLI_ARG_DESTROY_DELETE_BRANCH: &str = "cli.arg.destroy.delete_branch";
pub const CLI_ARG_DESTROY_KEEP_BRANCH: &str = "cli.arg.destroy.keep_branch";
pub const CLI_ARG_CLOSE_KILL: &str = "cli.arg.close.kill";
pub const CLI_ARG_PANE_TARGET: &str = "cli.arg.pane.target";
pub const CLI_ARG_PANE_SEND_TEXT: &str = "cli.arg.pane.send.text";
pub const CLI_ARG_PANE_SEND_NO_ENTER: &str = "cli.arg.pane.send.no_enter";
pub const CLI_ARG_PANE_SPLIT_DIR: &str = "cli.arg.pane.split.direction";
pub const CLI_ARG_PANE_SPLIT_CMD: &str = "cli.arg.pane.split.cmd";
pub const CLI_ARG_COMPLETION_SCRIPT_SHELL: &str = "cli.arg.completion.script.shell";
pub const CLI_ARG_COMPLETION_INSTALL_SHELL: &str = "cli.arg.completion.install.shell";
pub const CLI_ARG_PROJECT_REINDEX_ROOT: &str = "cli.arg.project.reindex.root";
pub const CLI_ARG_PROJECT_REINDEX_CONFIG_ROOTS: &str = "cli.arg.project.reindex.config_roots";
pub const CLI_ARG_PROJECT_REINDEX_CLEAR_FIRST: &str = "cli.arg.project.reindex.clear_first";
pub const CLI_ARG_PROJECT_REINDEX_DRY_RUN: &str = "cli.arg.project.reindex.dry_run";
pub const CLI_ARG_CONFIG_GLOBAL: &str = "cli.arg.config.global";
pub const CLI_ARG_CONFIG_RAW: &str = "cli.arg.config.raw";
pub const CLI_ARG_CONFIG_KEY: &str = "cli.arg.config.key";
pub const CLI_ARG_CONFIG_VALUE: &str = "cli.arg.config.value";

// -- CLI error rendering --
//
// User-facing rendering of [`CliError`] variants that don't already
// resolve to a domain-specific localized string. These keys allow the
// same exit-code mapping to surface in either English or zh-CN at the
// CLI boundary without changing thiserror message templates.

// -- Localized renderer for typed RuntimeError variants --
//
// These keys back the CLI-boundary renderer that translates a typed
// `RuntimeError` (or its embedded `BackendUnavailableReason`) into a
// localized user-facing line. Templates use positional placeholders
// (e.g. `{0}` = path / server name).
pub const CLI_RUNTIME_ERROR_PREFIX: &str = "cli.runtime_error.prefix";
pub const CLI_RUNTIME_BACKEND_UNAVAILABLE_GENERIC: &str =
    "cli.runtime_error.backend_unavailable.generic";
pub const CLI_RUNTIME_BACKEND_BINARY_NOT_FOUND: &str =
    "cli.runtime_error.backend_unavailable.binary_not_found";
pub const CLI_RUNTIME_BACKEND_BINARY_NOT_FOUND_HELP: &str =
    "cli.runtime_error.backend_unavailable.binary_not_found_help";
pub const CLI_RUNTIME_SERVER_NOT_RUNNING: &str =
    "cli.runtime_error.backend_unavailable.server_not_running";
pub const CLI_RUNTIME_SERVER_NOT_RUNNING_HELP: &str =
    "cli.runtime_error.backend_unavailable.server_not_running_help";
pub const CLI_RUNTIME_SOCKET_NOT_FOUND: &str =
    "cli.runtime_error.backend_unavailable.socket_not_found";
pub const CLI_RUNTIME_SOCKET_NOT_FOUND_HELP: &str =
    "cli.runtime_error.backend_unavailable.socket_not_found_help";
pub const CLI_RUNTIME_CAPABILITY_MISSING: &str =
    "cli.runtime_error.backend_unavailable.capability_missing";
pub const CLI_RUNTIME_WORKSPACE_NOT_FOUND: &str = "cli.runtime_error.workspace_not_found";
pub const CLI_RUNTIME_WORKSPACE_GROUP_CLOSE_REQUIRED: &str =
    "cli.runtime_error.workspace_group_close_required";
pub const CLI_RUNTIME_WORKSPACE_ALREADY_EXISTS: &str = "cli.runtime_error.workspace_already_exists";
pub const CLI_RUNTIME_UNIT_NOT_FOUND: &str = "cli.runtime_error.unit_not_found";
pub const CLI_RUNTIME_LAST_UNIT_CLOSE_REJECTED: &str = "cli.runtime_error.last_unit_close_rejected";
pub const CLI_RUNTIME_OPERATION_FAILED: &str = "cli.runtime_error.operation_failed";
pub const CLI_RUNTIME_PERMISSION_DENIED: &str = "cli.runtime_error.permission_denied";

// -- AI assistant activity (Claude / Codex / OpenCode hook & plugin events) --
//
// Surfaces the new tool-provider activity state model defined by
// `domain::ai_activity::AiActivityState` and consumed by
// `pane list` / `pane show`. The labels intentionally avoid the
// stronger "running" / "active" copy because the docs require us to
// distinguish "tool is *currently* doing work via a hook event"
// from "process is alive but we have no fresh provider event"
// (`open_unverified`). All keys go through the catalog so en/zh-CN
// stay aligned.
pub const PANE_AI_ACTIVITY_UNKNOWN: &str = "pane.ai_activity.unknown";
pub const PANE_AI_ACTIVITY_OPEN_UNVERIFIED: &str = "pane.ai_activity.open_unverified";
pub const PANE_AI_ACTIVITY_PROCESSING: &str = "pane.ai_activity.processing";
pub const PANE_AI_ACTIVITY_TOOL_RUNNING: &str = "pane.ai_activity.tool_running";
pub const PANE_AI_ACTIVITY_WAITING_FOR_PERMISSION: &str = "pane.ai_activity.waiting_for_permission";
pub const PANE_AI_ACTIVITY_WAITING_FOR_USER: &str = "pane.ai_activity.waiting_for_user";
pub const PANE_AI_ACTIVITY_ERROR: &str = "pane.ai_activity.error";

/// Provider labels: `claude_code`, `codex`, `opencode`, `none`.
/// Surfaced as the "source" column in pane show.
pub const PANE_AI_PROVIDER_CLAUDE_CODE: &str = "pane.ai_provider.claude_code";
pub const PANE_AI_PROVIDER_CODEX: &str = "pane.ai_provider.codex";
pub const PANE_AI_PROVIDER_OPENCODE: &str = "pane.ai_provider.opencode";
pub const PANE_AI_PROVIDER_NONE: &str = "pane.ai_provider.none";

/// Optional reason line carried by a `DisplayedAiActivity`. Renders
/// in pane show only when the provider attached a display-safe
/// reason (e.g. tool name, error short message).
pub const PANE_AI_ACTIVITY_REASON_LABEL: &str = "pane.ai_activity.reason_label";
/// Compact stale/unverified hint shown when the provider fact has
/// expired or no fact exists for the pane.
pub const PANE_AI_ACTIVITY_STALE_HINT: &str = "pane.ai_activity.stale_hint";
/// Label for the AI activity state line in `pane show`. Distinct
/// from `PANE_LABEL_ACTIVITY` so the supervisor-observed activity
/// and the hook-recorded AI activity render as two unambiguously
/// different fields.
pub const PANE_AI_ACTIVITY_LABEL: &str = "pane.ai_activity.label";
/// Label for the AI provider line in `pane show`.
pub const PANE_AI_ACTIVITY_PROVIDER_LABEL: &str = "pane.ai_activity.provider_label";
/// Label for the recorder confidence line (`hook` / `plugin` /
/// `heuristic` / `fallback`).
pub const PANE_AI_ACTIVITY_CONFIDENCE_LABEL: &str = "pane.ai_activity.confidence_label";
/// Localized values for [`AiActivityConfidence`] variants. Used by
/// `pane show` to label the recorder strength independently from the
/// AI activity state label.
pub const PANE_AI_ACTIVITY_CONFIDENCE_HOOK: &str = "pane.ai_activity.confidence.hook";
pub const PANE_AI_ACTIVITY_CONFIDENCE_PLUGIN: &str = "pane.ai_activity.confidence.plugin";
pub const PANE_AI_ACTIVITY_CONFIDENCE_HEURISTIC: &str = "pane.ai_activity.confidence.heuristic";
pub const PANE_AI_ACTIVITY_CONFIDENCE_FALLBACK: &str = "pane.ai_activity.confidence.fallback";
/// Label for the stale boolean line; the value localizes via
/// [`COMMON_VALUE_YES`] / [`COMMON_VALUE_NO`].
pub const PANE_AI_ACTIVITY_STALE_LABEL: &str = "pane.ai_activity.stale_label";
pub const COMMON_VALUE_YES: &str = "common.value.yes";
pub const COMMON_VALUE_NO: &str = "common.value.no";

// -- Productized activity source / accuracy / guidance --
//
// When hooks/plugins are missing, failed, expired, or there is no
// provider event, the UI must degrade gracefully without claiming the
// activity came from a hook. The `pane show` view therefore renders a
// typed "activity source" (`Claude Code hook` / `Codex hook` /
// `OpenCode plugin` / `Workhive supervisor` / `process observer` /
// `tmux fallback` / `none`) plus a typed "accuracy" tier (`high` /
// `medium` / `low`), and emits a localized guidance block when the
// source is anything other than a non-stale hook/plugin. `pane list`
// carries a compact legend that points at the same vocabulary.
//
// User guidance never instructs running `workhive integrations
// install`; that command is gone. The recommended recovery chain is:
//   1. Launch the AI tool through Workhive (the supervisor performs
//      the automatic preparation lifecycle).
//   2. If accuracy is still degraded, run `workhive doctor` for a
//      typed diagnostic.

/// Label for the rendered "activity source" line in `pane show`.
pub const PANE_AI_ACTIVITY_SOURCE_LABEL: &str = "pane.ai_activity.source_label";
/// Localized values for the activity source. Names mirror
/// [`workhive_core::domain::AiActivitySource`] variants.
pub const PANE_AI_ACTIVITY_SOURCE_CLAUDE_CODE_HOOK: &str =
    "pane.ai_activity.source.claude_code_hook";
pub const PANE_AI_ACTIVITY_SOURCE_CODEX_HOOK: &str = "pane.ai_activity.source.codex_hook";
pub const PANE_AI_ACTIVITY_SOURCE_OPENCODE_PLUGIN: &str = "pane.ai_activity.source.opencode_plugin";
pub const PANE_AI_ACTIVITY_SOURCE_TERMINAL_STATUS: &str = "pane.ai_activity.source.terminal_status";
pub const PANE_AI_ACTIVITY_SOURCE_SUPERVISOR: &str = "pane.ai_activity.source.supervisor";
pub const PANE_AI_ACTIVITY_SOURCE_PROCESS_OBSERVER: &str =
    "pane.ai_activity.source.process_observer";
pub const PANE_AI_ACTIVITY_SOURCE_TMUX_FALLBACK: &str = "pane.ai_activity.source.tmux_fallback";
pub const PANE_AI_ACTIVITY_SOURCE_RUNTIME_BACKEND: &str = "pane.ai_activity.source.runtime_backend";
pub const PANE_AI_ACTIVITY_SOURCE_NONE: &str = "pane.ai_activity.source.none";

/// Label for the rendered "accuracy" line in `pane show`.
pub const PANE_AI_ACTIVITY_ACCURACY_LABEL: &str = "pane.ai_activity.accuracy_label";
/// Localized values for the accuracy tier of the displayed activity
/// source.
pub const PANE_AI_ACTIVITY_ACCURACY_HIGH: &str = "pane.ai_activity.accuracy.high";
pub const PANE_AI_ACTIVITY_ACCURACY_MEDIUM: &str = "pane.ai_activity.accuracy.medium";
pub const PANE_AI_ACTIVITY_ACCURACY_MEDIUM_LOW: &str = "pane.ai_activity.accuracy.medium_low";
pub const PANE_AI_ACTIVITY_ACCURACY_LOW: &str = "pane.ai_activity.accuracy.low";
pub const PANE_AI_ACTIVITY_ACCURACY_NONE: &str = "pane.ai_activity.accuracy.none";

/// Multi-line localized guidance shown under `pane show` when the
/// activity source is anything other than a non-stale Claude/Codex
/// hook or OpenCode plugin. The single key holds the entire block —
/// the `\n` separator survives the i18n catalog roundtrip — so the
/// translator owns line breaks and emphasis. The block tells the
/// user:
///   1. The current source is a fallback / may not be accurate.
///   2. To improve accuracy, launch the AI tool through Workhive.
///   3. If accuracy is still degraded, run `workhive doctor`.
///
/// The block never mentions `workhive integrations` (that command
/// no longer exists).
pub const PANE_AI_ACTIVITY_GUIDANCE_FALLBACK: &str = "pane.ai_activity.guidance.fallback";
/// Same vocabulary, surfaced under `pane show` when the provider
/// fact exists but its TTL has elapsed (`stale = true`).
pub const PANE_AI_ACTIVITY_GUIDANCE_STALE: &str = "pane.ai_activity.guidance.stale";
/// Same vocabulary, surfaced under `pane show` when the supervisor
/// reports an AI tool in the foreground but no provider event was
/// ever recorded.
pub const PANE_AI_ACTIVITY_GUIDANCE_UNVERIFIED: &str = "pane.ai_activity.guidance.unverified";

// -- Per-row product-facing source tags --
//
// Rendered in the dedicated `source` column added to `pane list`
// output. The token vocabulary is product-facing
// (`hook`, `plugin`, `supervisor`, `process`, `tmux`, `none`) so
// users can scan, per row, where the displayed activity signal
// originated without consulting `pane show`. Independent column
// instead of the prior `[source/accuracy]` suffix on the activity
// cell so the activity column stays concise on narrow terminals.

/// Per-row source tag for sources backed by a Claude/Codex hook
/// event. Renders as `hook` / `钩子` in the source column.
pub const PANE_AI_ACTIVITY_SOURCE_TAG_HOOK: &str = "pane.ai_activity.source.tag.hook";
/// Per-row source tag for an OpenCode plugin event. Distinct from
/// `tag.hook` because the user-visible API surface OpenCode exposes
/// is a plugin (per `https://opencode.ai/docs/plugins/`), not a hook.
pub const PANE_AI_ACTIVITY_SOURCE_TAG_PLUGIN: &str = "pane.ai_activity.source.tag.plugin";
/// Per-row source tag for terminal UI status probing.
pub const PANE_AI_ACTIVITY_SOURCE_TAG_TERMINAL_STATUS: &str =
    "pane.ai_activity.source.tag.terminal_status";
/// Per-row source tag for Workhive supervisor as the source.
pub const PANE_AI_ACTIVITY_SOURCE_TAG_SUPERVISOR: &str = "pane.ai_activity.source.tag.supervisor";
/// Per-row source tag for OS process observer as the source.
pub const PANE_AI_ACTIVITY_SOURCE_TAG_PROCESS: &str = "pane.ai_activity.source.tag.process";
/// Per-row source tag for tmux `pane_current_command` as the only
/// available signal (lowest accuracy non-empty source).
pub const PANE_AI_ACTIVITY_SOURCE_TAG_TMUX: &str = "pane.ai_activity.source.tag.tmux";
pub const PANE_AI_ACTIVITY_SOURCE_TAG_BACKEND: &str = "pane.ai_activity.source.tag.backend";
/// Per-row source tag for the `None` sentinel.
pub const PANE_AI_ACTIVITY_SOURCE_TAG_NONE: &str = "pane.ai_activity.source.tag.none";

// -- Automatic AI integration prepare lifecycle --
//
// The integration is an automatic Workhive capability, not a user
// command. The happy path is contractually silent. Only failure /
// safety-refusal / recovery messages remain localized.
//
// `AUTO_PREPARE_REFUSED_USER_PLUGIN`: surfaced when Workhive refuses
// to overwrite a non-Workhive-authored OpenCode plugin sitting at
// the Workhive plugin path. Used by `pane_supervisor` (warn-level
// log) and `init --global` (stderr fail-open hint).
/// Generic prepare failure (provider-specific path is logged at
/// `warn`; user-visible copy stays high-level so we never leak
/// internal paths or third-party config schema details).
pub const AUTO_PREPARE_FAILED: &str = "auto_prepare.failed";

// -- Activity record (internal) error keys --
//
// The hidden `internal activity record` command reads a hook payload
// from stdin and writes a typed fact to the runtime store. Errors are
// localized so the calling hook script can surface them.

// -- Activity record CLI handler error keys --
pub const ACTIVITY_RECORD_ERROR_MISSING_RUNTIME_ROOT: &str =
    "activity_record.error.missing_runtime_root";
pub const ACTIVITY_RECORD_ERROR_MISSING_PROVIDER: &str = "activity_record.error.missing_provider";
pub const ACTIVITY_RECORD_ERROR_UNKNOWN_PROVIDER: &str = "activity_record.error.unknown_provider";
pub const ACTIVITY_RECORD_ERROR_NO_PROVIDER_ACTIVE: &str =
    "activity_record.error.no_provider_active";
pub const ACTIVITY_RECORD_ERROR_READ_STDIN: &str = "activity_record.error.read_stdin";
pub const ACTIVITY_RECORD_ERROR_EMPTY_STDIN: &str = "activity_record.error.empty_stdin";
pub const ACTIVITY_RECORD_ERROR_INVALID_JSON: &str = "activity_record.error.invalid_json";
pub const ACTIVITY_RECORD_ERROR_PROVIDER_MISMATCH: &str = "activity_record.error.provider_mismatch";
// Trusted pane-identity is supplied by the supervisor via env vars.
// Missing or mismatched values are hard rejections at the CLI
// boundary so a hostile/buggy hook payload can never re-route a
// fact to a different pane (review High 1).
pub const ACTIVITY_RECORD_ERROR_MISSING_GROUP: &str = "activity_record.error.missing_group";
pub const ACTIVITY_RECORD_ERROR_MISSING_WORKSPACE: &str = "activity_record.error.missing_workspace";
pub const ACTIVITY_RECORD_ERROR_MISSING_UNIT_ID: &str = "activity_record.error.missing_unit_id";
pub const ACTIVITY_RECORD_ERROR_IDENTITY_MISMATCH: &str = "activity_record.error.identity_mismatch";
// Hook subcommand-specific errors. The hook adapter only knows
// about a small set of documented event names per provider; the
// JSON event-name field also has to be present and well-formed.
pub const ACTIVITY_RECORD_ERROR_MISSING_EVENT_FIELD: &str =
    "activity_record.error.missing_event_field";
pub const ACTIVITY_RECORD_ERROR_UNHANDLED_EVENT: &str = "activity_record.error.unhandled_event";

// -- ACP metrics render keys (Activity Intelligence v1) --
//
// These keys are used by `pane_metrics_view.rs` to render
// `PaneActivityMetrics` values in `pane list` and `pane show`.
// All keys follow the `pane.metrics.*` namespace to avoid
// collisions with the existing `pane.ai_activity.*` namespace.

/// Placeholder shown when a metric value is `Partial` but not stale
/// (e.g. in-progress, low-confidence, or corrupt).
pub const PANE_METRICS_PARTIAL: &str = "pane.metrics.partial";
/// Placeholder shown when a metric value is `Unavailable`.
pub const PANE_METRICS_UNAVAILABLE: &str = "pane.metrics.unavailable";

// Source labels (distinct from the existing source *tag* keys).
/// Human label for `source = "acp"` in `pane show` detail lines.
pub const PANE_METRICS_SOURCE_ACP: &str = "pane.metrics.source.acp";
/// Human label for `source = "legacy"` in `pane list` and `pane show`.
pub const PANE_METRICS_SOURCE_LEGACY: &str = "pane.metrics.source.legacy";
/// Human label for `source = "none"` (no data at all).
pub const PANE_METRICS_SOURCE_NONE: &str = "pane.metrics.source.none";

// Column/field labels for `pane show` detail block.
/// Label for the AI provider field in the metrics block.
pub const PANE_METRICS_LABEL_PROVIDER: &str = "pane.metrics.label.provider";
/// Label for the ACP-derived state field.
pub const PANE_METRICS_LABEL_STATE: &str = "pane.metrics.label.state";
/// Label for the data source field.
pub const PANE_METRICS_LABEL_SOURCE: &str = "pane.metrics.label.source";
/// Label for the confidence tier field.
pub const PANE_METRICS_LABEL_CONFIDENCE: &str = "pane.metrics.label.confidence";
/// Label for the freshness (event age) field.
pub const PANE_METRICS_LABEL_FRESHNESS: &str = "pane.metrics.label.freshness";
/// Label for the elapsed-in-current-state field.
pub const PANE_METRICS_LABEL_ELAPSED: &str = "pane.metrics.label.elapsed";
/// Label for the current in-progress agent turn duration.
pub const PANE_METRICS_LABEL_CURRENT_TURN: &str = "pane.metrics.label.current_turn";
/// Label for the last completed agent turn duration.
pub const PANE_METRICS_LABEL_LAST_TURN: &str = "pane.metrics.label.last_turn";
/// Label for the last completed tool invocation duration.
pub const PANE_METRICS_LABEL_LAST_TOOL: &str = "pane.metrics.label.last_tool";
/// Label for the aggregate tool duration over the event window.
pub const PANE_METRICS_LABEL_AGGREGATE_TOOL: &str = "pane.metrics.label.aggregate_tool";
/// Label for the permission-wait duration field.
pub const PANE_METRICS_LABEL_PERMISSION_WAIT: &str = "pane.metrics.label.permission_wait";
/// Label for the idle duration field.
pub const PANE_METRICS_LABEL_IDLE: &str = "pane.metrics.label.idle";
/// Label for the next-action hint field.
pub const PANE_METRICS_LABEL_HINT: &str = "pane.metrics.label.hint";
/// Heading printed above the `pane list` per-row AI metrics summary
/// block. The block appears immediately after the table when at
/// least one row produced a non-empty metrics line, and is rendered
/// indented underneath this heading so narrow terminals stay
/// readable.
pub const PANE_METRICS_BLOCK_HEADING: &str = "pane.metrics.block.heading";

// Hint human-readable values (shown in `pane show`; JSON uses the typed enum).
/// Human message shown when a permission request is pending.
pub const PANE_METRICS_HINT_PERMISSION_NEEDED: &str = "pane.metrics.hint.permission_needed";
/// Human message shown when the last turn completed successfully.
pub const PANE_METRICS_HINT_COMPLETED: &str = "pane.metrics.hint.completed";
/// Human message shown when the last turn failed.
pub const PANE_METRICS_HINT_FAILED: &str = "pane.metrics.hint.failed";

// -- Pane supervisor exit-policy diagnostics --
//
// Used by `workhive internal pane-supervisor` to surface a single
// stderr line when a recoverable runtime failure happens on the
// exit path:
//   1) spawning the managed tool itself failed before any output
//      could come from it (e.g. `program` not on PATH); the pane
//      would otherwise show only the cryptic Rust error printed by
//      the spawn fallback;
//   2) the configured `respawn_shell` policy could not execute a
//      replacement shell, so the pane is going to close. We tell
//      the user *why* their pane closed instead of silently
//      degrading.
//
// Both messages are intentionally short and English-fallback-safe;
// the supervisor's stdout/stderr is the user's actual pane.
pub const PANE_SUPERVISOR_SPAWN_FAILED: &str = "pane.supervisor.spawn_failed";
pub const PANE_SUPERVISOR_REPLACEMENT_SHELL_FAILED: &str =
    "pane.supervisor.replacement_shell_failed";
/// Pane-supervisor stderr line emitted when the configured
/// `pane.exit.tool_exit_behavior` cannot be trusted because the
/// underlying global or project config failed to load/parse, or
/// because the persisted value is not one of the accepted variants.
///
/// In that case the supervisor **must not** silently apply
/// `respawn_shell`: doing so would honor a behavior the user
/// neither configured nor expressed. The supervisor therefore
/// degrades to `close_pane` and uses this localized line to tell
/// the user *why* their pane closed and where to look. The `{0}`
/// placeholder is filled with the structured reason from
/// [`ConfigError`] (or an equivalent diagnostic).
pub const PANE_SUPERVISOR_EXIT_BEHAVIOR_UNTRUSTED: &str = "pane.supervisor.exit_behavior_untrusted";

// -- Update command messages --
//
// Used by `workhive update` to provide feedback during the self-update process.
// These messages are shown at various stages: checking for updates, downloading,
// installing, and reporting success or failure.
pub const UPDATE_CHECKING_FOR_UPDATES: &str = "update.checking_for_updates";
pub const UPDATE_AVAILABLE: &str = "update.available";
pub const UPDATE_ALREADY_UP_TO_DATE: &str = "update.already_up_to_date";
pub const UPDATE_DRY_RUN_MESSAGE: &str = "update.dry_run_message";
pub const UPDATE_WOULD_INSTALL: &str = "update.would_install";
pub const UPDATE_WOULD_INSTALL_LATEST: &str = "update.would_install_latest";
pub const UPDATE_CONFIRM_PROMPT: &str = "update.confirm_prompt";
pub const UPDATE_CANCELLED: &str = "update.cancelled";
pub const UPDATE_DOWNLOADING_INSTALLING: &str = "update.downloading_installing";
pub const UPDATE_SUCCESS: &str = "update.success";
