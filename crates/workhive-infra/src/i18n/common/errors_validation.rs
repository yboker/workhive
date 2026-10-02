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
    m.insert(
        "index.stale_entry.project_root_gone",
        "warning: project '{0}' no longer exists at its indexed root (stale index entry). Run 'workhive project reindex' to refresh.",
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

    // Identity errors
    m.insert(
        "error.identity.project_root_mismatch",
        "Project root mismatch: stored path '{0}' does not match current path '{1}'.",
    );
    m.insert(
        "error.identity.project_root_mismatch.help",
        "The project directory may have been moved or renamed. Run `workhive doctor --fix` in the current directory to repair the stored root, then `workhive project reindex` to refresh the global project index, or `cd` back to the original directory.",
    );
    m.insert(
        "error.identity.instance_id_conflict",
        "Project identity conflict: stored instance_id '{0}' does not match current '{1}' (project: {2}).",
    );
    m.insert(
        "error.identity.instance_id_conflict.help",
        "The identity file may be corrupted or manually edited. Run `workhive doctor` to diagnose and repair.",
    );
    m.insert(
        "error.identity.project_identity_missing",
        "No project identity found at '{0}'. The project may not be initialized.",
    );
    m.insert(
        "error.identity.project_identity_missing.help",
        "The project may not be initialized. Run `workhive init` from the project root.",
    );
    m.insert(
        "error.identity.corrupted",
        "Corrupted identity file at '{0}': {1}.",
    );
    m.insert(
        "error.identity.corrupted.help",
        "The identity file is corrupted. Run `workhive doctor` to diagnose, or restore .workhive/identity/project.json from backup.",
    );
    m.insert("error.identity.generic", "Project identity error: {0}");
    m.insert(
        "error.identity.generic.help",
        "Please inspect files under .workhive/identity/ or run `workhive doctor` for diagnosis.",
    );

    // Doctor fix prompts
    m.insert(
        "doctor.fix.root_mismatch_detected",
        "Stored project root does not match current directory",
    );
    m.insert(
        "doctor.fix.root_mismatch_prompt",
        "Update project root from {0} to {1}?",
    );
    m.insert("doctor.fix.root_updated", "Updated project root to {0}");
    m.insert(
        "doctor.fix.root_skipped",
        "Skipped repair, keeping original project root",
    );
    m.insert(
        "doctor.fix.hint",
        "Run `workhive doctor --fix` to automatically repair",
    );
    m.insert("doctor.fix.stored_root_label", "Stored:");
    m.insert("doctor.fix.current_root_label", "Current:");

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
        "cli.runtime_error.backend_unavailable.binary_not_found_help",
        "Install the backend with the command below:",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.server_not_running_help",
        "Start the selected backend server, then retry. Workhive will ensure its own reserved session, but will not start a user-defined session.",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.socket_not_found_help",
        "Start the backend or check the configured socket path, then retry.",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.capability_missing",
        "Runtime backend capability `{0}` is not available. Check the selected backend configuration.",
    );
    m.insert(
        "cli.runtime_error.workspace_not_found",
        "Workspace `{0}` was not found in the runtime.",
    );
    m.insert(
        "cli.runtime_error.workspace_group_close_required",
        "Workspace `{0}` is the parent of open workspaces registered against the same checkout. Close or remove the child workspaces first; Workhive will not close the group implicitly.",
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
    m.insert(
        "index.stale_entry.project_root_gone",
        "警告：项目 '{0}' 的索引根目录已不存在（索引条目已过期）。请运行 'workhive project reindex' 刷新索引。",
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

    // Identity errors
    m.insert(
        "error.identity.project_root_mismatch",
        "项目根目录不匹配：存储的路径 '{0}' 与当前路径 '{1}' 不一致。",
    );
    m.insert(
        "error.identity.project_root_mismatch.help",
        "项目目录可能已被移动或重命名。请在当前目录下运行 `workhive doctor --fix` 修复存储的项目根目录，再运行 `workhive project reindex` 刷新全局项目索引，或 `cd` 回原目录继续工作。",
    );
    m.insert(
        "error.identity.instance_id_conflict",
        "项目 identity 冲突：存储的 instance_id '{0}' 与当前 '{1}' 不一致（项目路径：{2}）。",
    );
    m.insert(
        "error.identity.instance_id_conflict.help",
        "identity 文件可能已损坏或被手动修改。运行 `workhive doctor` 检查并修复。",
    );
    m.insert(
        "error.identity.project_identity_missing",
        "未找到项目 identity（路径：{0}），项目可能尚未初始化。",
    );
    m.insert(
        "error.identity.project_identity_missing.help",
        "项目可能尚未初始化，请在项目根目录运行 `workhive init`。",
    );
    m.insert(
        "error.identity.corrupted",
        "identity 文件已损坏（路径：{0}）：{1}。",
    );
    m.insert(
        "error.identity.corrupted.help",
        "identity 文件已损坏。运行 `workhive doctor` 检查，或从备份恢复 .workhive/identity/project.json。",
    );
    m.insert("error.identity.generic", "项目 identity 错误：{0}");
    m.insert(
        "error.identity.generic.help",
        "请检查 .workhive/identity/ 目录下的文件，或运行 `workhive doctor` 诊断。",
    );

    // Doctor fix prompts
    m.insert(
        "doctor.fix.root_mismatch_detected",
        "存储的项目根目录与当前目录不一致",
    );
    m.insert(
        "doctor.fix.root_mismatch_prompt",
        "将项目根目录从 {0} 更新为 {1}？",
    );
    m.insert("doctor.fix.root_updated", "已更新项目根目录为 {0}");
    m.insert("doctor.fix.root_skipped", "跳过修复，保留原始项目根目录");
    m.insert("doctor.fix.hint", "运行 `workhive doctor --fix` 自动修复");
    m.insert("doctor.fix.stored_root_label", "存储路径：");
    m.insert("doctor.fix.current_root_label", "当前路径：");

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
        "cli.runtime_error.backend_unavailable.binary_not_found_help",
        "请使用下方命令安装后端：",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.server_not_running_help",
        "请启动所选后端服务器后重试。Workhive 会确保自己的保留 session，但不会启动用户自定义 session。",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.socket_not_found_help",
        "请启动后端或检查 socket 配置后重试。",
    );
    m.insert(
        "cli.runtime_error.backend_unavailable.capability_missing",
        "运行时后端不支持所需能力 `{0}`。请检查所选后端配置。",
    );
    m.insert(
        "cli.runtime_error.workspace_not_found",
        "未在运行时中找到工作区 `{0}`。",
    );
    m.insert(
        "cli.runtime_error.workspace_group_close_required",
        "工作区 `{0}` 是仍处于打开状态的、注册到同一 checkout 的子工作区的父级。请先关闭或移除子工作区；Workhive 不会隐式关闭整组。",
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
