#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Terminal Adapter Interface (Abstract Base)
# Terminal multiplexer adapter interface
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_TERMINAL_BASE_SH_LOADED:-}" ]] && return 0
readonly _TERMINAL_BASE_SH_LOADED=1

# Abstract methods - subclasses must implement

terminal_adapter_name() {
    die "Method not implemented: terminal_adapter_name"
}

terminal_adapter_check_available() {
    die "Method not implemented: terminal_adapter_check_available"
}

terminal_adapter_create_session() {
    die "Method not implemented: terminal_adapter_create_session"
}

terminal_adapter_has_session() {
    die "Method not implemented: terminal_adapter_has_session"
}

terminal_adapter_attach_session() {
    die "Method not implemented: terminal_adapter_attach_session"
}

# Return last attach outcome metadata for diagnostics.
# Expected values: attached | detached | ended | error | unknown
terminal_adapter_get_last_attach_status() {
    echo "unknown"
}

terminal_adapter_kill_session() {
    die "Method not implemented: terminal_adapter_kill_session"
}

terminal_adapter_detach_session() {
    die "Method not implemented: terminal_adapter_detach_session"
}

terminal_adapter_split_vertical() {
    die "Method not implemented: terminal_adapter_split_vertical"
}

terminal_adapter_split_horizontal() {
    die "Method not implemented: terminal_adapter_split_horizontal"
}

terminal_adapter_send_keys() {
    die "Method not implemented: terminal_adapter_send_keys"
}

# Send literal text to pane; optionally append Enter
# Args: $1=session_name, $2=pane_index, $3=text, $4=no_enter(true|false)
terminal_adapter_send_text() {
    die "Method not implemented: terminal_adapter_send_text"
}

# Return last send strategy metadata string for diagnostics/logging.
# Output format is adapter-defined key=value pairs.
terminal_adapter_get_last_send_meta() {
    echo ""
}

terminal_adapter_apply_config() {
    die "Method not implemented: terminal_adapter_apply_config"
}

terminal_adapter_list_sessions() {
    die "Method not implemented: terminal_adapter_list_sessions"
}

terminal_adapter_count_panes() {
    die "Method not implemented: terminal_adapter_count_panes"
}

terminal_adapter_get_current_session() {
    die "Method not implemented: terminal_adapter_get_current_session"
}

terminal_adapter_get_current_pane_index() {
    die "Method not implemented: terminal_adapter_get_current_pane_index"
}

terminal_adapter_get_pane_command() {
    die "Method not implemented: terminal_adapter_get_pane_command"
}

terminal_adapter_get_pane_cwd() {
    die "Method not implemented: terminal_adapter_get_pane_cwd"
}

terminal_adapter_attach_session_readonly() {
    die "Method not implemented: terminal_adapter_attach_session_readonly"
}

terminal_adapter_select_pane() {
    die "Method not implemented: terminal_adapter_select_pane"
}

terminal_adapter_resize_pane() {
    die "Method not implemented: terminal_adapter_resize_pane"
}

terminal_adapter_get_session_info() {
    die "Method not implemented: terminal_adapter_get_session_info"
}

terminal_adapter_kill_pane() {
    die "Method not implemented: terminal_adapter_kill_pane"
}

terminal_adapter_kill_pane_target() {
    die "Method not implemented: terminal_adapter_kill_pane_target"
}

terminal_adapter_resolve_pane_target() {
    die "Method not implemented: terminal_adapter_resolve_pane_target"
}

terminal_adapter_split_current_pane() {
    die "Method not implemented: terminal_adapter_split_current_pane"
}

terminal_adapter_get_layout_string() {
    die "Method not implemented: terminal_adapter_get_layout_string"
}


terminal_adapter_get_window_count() {
    die "Method not implemented: terminal_adapter_get_window_count"
}

terminal_adapter_is_session_attached() {
    die "Method not implemented: terminal_adapter_is_session_attached"
}

terminal_adapter_get_window_size() {
    die "Method not implemented: terminal_adapter_get_window_size"
}

terminal_adapter_resize_window() {
    die "Method not implemented: terminal_adapter_resize_window"
}

terminal_adapter_list_panes_format() {
    die "Method not implemented: terminal_adapter_list_panes_format"
}

terminal_adapter_set_after_kill_pane_hook() {
    die "Method not implemented: terminal_adapter_set_after_kill_pane_hook"
}

terminal_adapter_run_shell() {
    die "Method not implemented: terminal_adapter_run_shell"
}

terminal_adapter_display_message() {
    die "Method not implemented: terminal_adapter_display_message"
}

terminal_adapter_set_option() {
    die "Method not implemented: terminal_adapter_set_option"
}

# Pane-level option read/write for declarative metadata.
# Args: $1=session_name, $2=pane_index, $3=option, $4=value
terminal_adapter_set_pane_option() {
    die "Method not implemented: terminal_adapter_set_pane_option"
}

# Args: $1=session_name, $2=pane_index, $3=option
terminal_adapter_get_pane_option() {
    return 1
}

# Args: $1=session_name, $2=pane_index, $3=option
terminal_adapter_unset_pane_option() {
    return 0
}

terminal_adapter_rename_session() {
    die "Method not implemented: terminal_adapter_rename_session"
}

terminal_adapter_select_layout() {
    die "Method not implemented: terminal_adapter_select_layout"
}

# ──────────────────────────────────────────────────
# Window operations (for multi-window session support)
# ──────────────────────────────────────────────────

# Create a new window in an existing session.
# Args: $1 = session_name, $2 = window_name, $3 = working_dir
terminal_adapter_new_window() {
    die "Method not implemented: terminal_adapter_new_window"
}

# Rename a window.
# Args: $1 = session_name, $2 = window_index, $3 = new_name
terminal_adapter_rename_window() {
    die "Method not implemented: terminal_adapter_rename_window"
}

# Select (focus) a window.
# Args: $1 = session_name, $2 = window_index
terminal_adapter_select_window() {
    die "Method not implemented: terminal_adapter_select_window"
}

# Send keys to a specific window's pane.
# Args: $1 = session_name, $2 = window_index, $3 = pane_index, $4 = keys
terminal_adapter_send_keys_to_window() {
    die "Method not implemented: terminal_adapter_send_keys_to_window"
}

# Spawn command in a target pane without simulated typing echo.
# Args: $1 = pane target (e.g. session:win.pane), $2 = command, $3 = workdir(optional)
terminal_adapter_spawn_command_target() {
    die "Method not implemented: terminal_adapter_spawn_command_target"
}

# Spawn a clean login shell in a target pane without simulating a command.
# Args: $1 = pane target (e.g. session:win.pane), $2 = workdir(optional)
terminal_adapter_spawn_login_shell_target() {
    die "Method not implemented: terminal_adapter_spawn_login_shell_target"
}

# Reflow pane geometry so full-screen/TUI apps receive final dimensions.
# Args: $1 = session_name
terminal_adapter_reflow_session_panes() {
    return 0
}

# List window names for a session.
# Args: $1 = session_name
# Returns (stdout): newline-separated list of window names
terminal_adapter_list_windows() {
    die "Method not implemented: terminal_adapter_list_windows"
}

# Returns "major.minor" version string (e.g. "3.3")
terminal_adapter_get_version() {
    echo "0.0"
}
