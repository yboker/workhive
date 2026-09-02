//! Validation messages, generic/session/runtime errors, and CLI error rendering.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Layout executor runtime messages
    m.insert(
        "error.layout.column_not_created",
        "Layout plan references column {0} before it was created. This is an internal error.",
    );

    // Validation
    m.insert("validation.invalid_number", "Invalid number.");
    m.insert("validation.columns_range", "Column count must be 1-5.");
    m.insert("validation.invalid_width", "Invalid width.");
    m.insert("validation.invalid_rows", "Invalid row count.");
    m.insert("validation.rows_range", "Row count must be 1-{0}.");
    // Index warnings
    m.insert(
        "index.stale_entry",
        "warning: project '{0}' has no instance_id (stale index entry). Run 'workhive project reindex' to refresh.",
    );

    // Error: session lookup
    m.insert("error.session.running", "Session is already running.");
    m.insert("error.session.missing", "Session does not exist.");
    m.insert("error.session.unreachable", "tmux server is unreachable.");
    m.insert("error.session.lookup_failed", "Session lookup failed.");
    m.insert(
        "error.session_lookup_failed_msg",
        "Session lookup failed for '{0}': {1}",
    );

    // Errors
    m.insert(
        "error.not_in_session",
        "This command must run inside a pane of a workhive-managed workspace, but the current shell is not in one.\nOpen or attach to a workspace first:\n  workhive workspace open <name>\n  workhive workspace attach <name>",
    );
    m.insert("error.session_not_found", "Session '{0}' not found.");
    m.insert(
        "error.not_initialized",
        "Project not initialized. Run 'workhive init' first.",
    );
    m.insert(
        "error.global_not_initialized",
        "Global config not initialized. Run 'workhive init --global' first.",
    );
    m.insert(
        "error.project_not_initialized",
        "Project not initialized. Run 'workhive init' first, then 'workhive workspace open'.",
    );
    m.insert(
        "error.project_not_initialized_hint",
        "Project not initialized. Run 'workhive init' or 'workhive workspace open <workspace> --init'.",
    );

    // CLI error rendering
    m.insert("cli.error.user_cancelled", "Operation cancelled by user");
    m.insert("cli.error.user_aborted", "Operation aborted");
    m.insert(
        "cli.error.confirmation_required.help",
        "Re-run with `--yes` (or `-f`/`--force` where applicable) to confirm.",
    );
    m.insert(
        "cli.error.session_lookup_failed",
        "Session lookup failed: {0}",
    );
    m.insert("cli.error.runtime", "Runtime error: {0}");

    // Localized renderer for typed RuntimeError variants. The CLI
    // boundary renders these instead of leaking the raw `Display`
    // text. `{0}` placeholders carry context (server name, socket
    // path, etc.) supplied by the typed reason enum.
    m.insert("cli.runtime_error.prefix", "Runtime error");
    m.insert(
        "cli.runtime_error.backend_unavailable.generic",
        "Runtime backend is not available.",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.binary_not_found",
        "Runtime backend `{0}` was not found in PATH. Please install or expose it before running this command.",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.server_not_running",
        "Runtime backend server `{0}` is not running.",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.socket_not_found",
        "Runtime backend socket `{0}` was not found.",
    );
    m.insert(
        "cli.runtime_error.workspace_not_found",
        "Workspace `{0}` was not found in the runtime.",
    );
    m.insert(
        "cli.runtime_error.workspace_already_exists",
        "Workspace `{0}` already exists in the runtime.",
    );
    m.insert(
        "cli.runtime_error.unit_not_found",
        "Runtime unit `{0}` was not found.",
    );
    m.insert(
        "cli.runtime_error.last_unit_close_rejected",
        "Refusing to close the last unit in this workspace.",
    );
    m.insert(
        "cli.runtime_error.operation_failed",
        "Runtime operation failed: {0}",
    );
    m.insert(
        "cli.runtime_error.permission_denied",
        "Runtime permission denied: {0}",
    );
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // Layout executor runtime messages
    m.insert(
        "error.layout.column_not_created",
        "布局计划引用了尚未创建的第 {0} 列。这是一个内部错误。",
    );

    // Validation
    m.insert("validation.invalid_number", "无效数字。");
    m.insert("validation.columns_range", "列数必须为 1-5。");
    m.insert("validation.invalid_width", "无效宽度。");
    m.insert("validation.invalid_rows", "无效行数。");
    m.insert("validation.rows_range", "行数必须为 1-{0}。");
    // Index warnings
    m.insert(
        "index.stale_entry",
        "警告：项目 '{0}' 缺少 instance_id（索引条目已过期）。请运行 'workhive project reindex' 刷新索引。",
    );

    // Error: session lookup
    m.insert("error.session.running", "会话已在运行中。");
    m.insert("error.session.missing", "会话不存在。");
    m.insert("error.session.unreachable", "tmux 服务器不可达。");
    m.insert("error.session.lookup_failed", "会话查询失败。");
    m.insert(
        "error.session_lookup_failed_msg",
        "会话 '{0}' 查询失败：{1}",
    );

    // Errors
    m.insert(
        "error.not_in_session",
        "该命令必须在 workhive 管理的工作区窗格内执行，但当前终端不在其中。\n请先打开或接入一个工作区：\n  workhive workspace open <名称>\n  workhive workspace attach <名称>",
    );
    m.insert("error.session_not_found", "未找到会话 '{0}'。");
    m.insert(
        "error.not_initialized",
        "项目未初始化。请先运行 'workhive init'。",
    );
    m.insert(
        "error.global_not_initialized",
        "全局配置未初始化。请先运行 'workhive init --global'。",
    );
    m.insert(
        "error.project_not_initialized",
        "项目未初始化。请先运行 'workhive init'，然后再执行 'workhive workspace open'。",
    );
    m.insert(
        "error.project_not_initialized_hint",
        "项目未初始化。请先运行 'workhive init' 或 'workhive workspace open <工作区名> --init'。",
    );

    // CLI error rendering
    m.insert("cli.error.user_cancelled", "用户取消了当前操作");
    m.insert("cli.error.user_aborted", "操作已中止");
    m.insert(
        "cli.error.confirmation_required.help",
        "请使用 `--yes`（或在适用命令上加 `-f`/`--force`）后重新执行以确认。",
    );
    m.insert("cli.error.session_lookup_failed", "查询会话失败：{0}");
    m.insert("cli.error.runtime", "运行时错误：{0}");

    // Localized renderer for typed RuntimeError variants. The CLI
    // boundary renders these instead of leaking the raw `Display`
    // text. `{0}` placeholders carry context (server name, socket
    // path, etc.) supplied by the typed reason enum.
    m.insert("cli.runtime_error.prefix", "运行时错误");
    m.insert(
        "cli.runtime_error.backend_unavailable.generic",
        "运行时后端当前不可用。",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.binary_not_found",
        "未在 PATH 中找到运行时后端 `{0}`，请先安装并确保可执行。",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.server_not_running",
        "运行时后端服务器 `{0}` 当前未运行。",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.socket_not_found",
        "未找到运行时后端套接字：`{0}`。",
    );
    m.insert(
        "cli.runtime_error.workspace_not_found",
        "未在运行时中找到工作区 `{0}`。",
    );
    m.insert(
        "cli.runtime_error.workspace_already_exists",
        "运行时中已存在同名工作区 `{0}`。",
    );
    m.insert(
        "cli.runtime_error.unit_not_found",
        "未找到运行时单元 `{0}`。",
    );
    m.insert(
        "cli.runtime_error.last_unit_close_rejected",
        "拒绝关闭工作区中的最后一个单元。",
    );
    m.insert("cli.runtime_error.operation_failed", "运行时操作失败：{0}");
    m.insert("cli.runtime_error.permission_denied", "运行时权限被拒：{0}");
}
