//! Environment doctor diagnostics and shell-completion install copy.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Doctor
    m.insert("doctor.header", "workhive doctor — diagnostic report");
    m.insert("doctor.category.remote", "Remote / Session");
    m.insert("doctor.status.pass", "\u{2713}");
    m.insert("doctor.status.warn", "\u{26A0}");
    m.insert("doctor.status.fail", "\u{2717}");
    m.insert("doctor.check.tmux_server", "tmux server reachable");
    m.insert("doctor.check.session_lookup", "Session lookup");
    m.insert("doctor.check.snapshot_integrity", "Snapshot integrity");
    m.insert("doctor.check.pty_availability", "PTY availability");
    m.insert("doctor.check.attach_readiness", "Attach readiness");
    m.insert("doctor.check.workspace_identity", "Workspace identity");
    m.insert(
        "doctor.advice.tmux_not_running",
        "Start the tmux server: tmux new-session -d",
    );
    m.insert(
        "doctor.advice.tmux_error",
        "Check tmux installation and server status: tmux info",
    );
    m.insert(
        "doctor.advice.lookup_failed",
        "Verify the project is initialized: workhive init",
    );
    m.insert(
        "doctor.advice.snapshot_corrupt",
        "Remove corrupt snapshot and re-create with: workhive workspace destroy && workhive workspace open",
    );
    m.insert(
        "doctor.advice.pty_exhausted",
        "Close unused terminal sessions to free PTY devices",
    );
    m.insert(
        "doctor.advice.pty_elevated",
        "PTY usage is high; consider closing idle sessions",
    );
    m.insert(
        "doctor.advice.nested_tmux",
        "Unset $TMUX before attaching: unset TMUX && workhive workspace attach",
    );
    m.insert(
        "doctor.advice.session_missing",
        "Session does not exist; run workhive workspace open to create it",
    );
    m.insert(
        "doctor.advice.session_server_unreachable",
        "tmux server is not reachable; session state cannot be determined",
    );
    m.insert(
        "doctor.advice.session_lookup_failed",
        "Session lookup returned garbled or unexpected output",
    );
    m.insert(
        "doctor.advice.invalid_workspace_name",
        "Workspace directory name is not a valid workspace identifier; skipped from diagnostics",
    );
    m.insert("doctor.all_passed", "All checks passed.");
    m.insert("doctor.has_issues", "{0} issue(s) found.");
    m.insert(
        "doctor.project_not_initialized",
        "project not initialized — run `workhive init` first",
    );
    m.insert("completions.installed", "Installed {0} completions to {1}.");
    m.insert(
        "completions.activate.zsh.rc",
        "Updated {0} so zsh can load them.\nOpen a new terminal or run `source {0}` to use completion now.",
    );
    m.insert(
        "completions.activate.zsh.no_rc",
        "Open a new terminal or run `source ~/.zshrc` to use completion now.",
    );
    m.insert(
        "completions.activate.bash",
        "Open a new terminal, or ensure bash-completion is loaded and run `source ~/.bashrc` (or `source ~/.bash_profile` on systems that use it).",
    );
    m.insert(
        "completions.activate.fish",
        "Open a new fish shell, or run `exec fish` to reload completions in the current terminal.",
    );
    m.insert(
        "completions.activate.powershell",
        "Open a new PowerShell session, or reload your profile with `. $PROFILE` if it imports Workhive completions.",
    );
    m.insert(
        "completions.activate.elvish",
        "Open a new Elvish shell, or load the installed completion script from your rc.elv.",
    );
    m.insert(
        "completions.activate.generic",
        "Open a new terminal or reload your shell completion configuration to use completion now.",
    );
    m.insert(
        "doctor.error.project_not_detected",
        "Cannot detect project — run from a workhive-initialized directory or pass --project.",
    );
    m.insert(
        "doctor.error.workspace_enum_failed",
        "Failed to enumerate workspaces for the project.",
    );
    m.insert(
        "doctor.error.snapshot_root_unknown",
        "Cannot determine snapshot root — project detection failed.",
    );
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // Doctor
    m.insert("doctor.header", "workhive doctor — 诊断报告");
    m.insert("doctor.category.remote", "远程 / 会话");
    m.insert("doctor.status.pass", "\u{2713}");
    m.insert("doctor.status.warn", "\u{26A0}");
    m.insert("doctor.status.fail", "\u{2717}");
    m.insert("doctor.check.tmux_server", "tmux 服务器可达");
    m.insert("doctor.check.session_lookup", "会话查询");
    m.insert("doctor.check.snapshot_integrity", "快照完整性");
    m.insert("doctor.check.pty_availability", "PTY 可用性");
    m.insert("doctor.check.attach_readiness", "连接就绪");
    m.insert("doctor.check.workspace_identity", "工作区标识");
    m.insert(
        "doctor.advice.tmux_not_running",
        "启动 tmux 服务器：tmux new-session -d",
    );
    m.insert(
        "doctor.advice.tmux_error",
        "检查 tmux 安装及服务器状态：tmux info",
    );
    m.insert(
        "doctor.advice.lookup_failed",
        "确认项目已初始化：workhive init",
    );
    m.insert(
        "doctor.advice.snapshot_corrupt",
        "删除损坏的快照并重建：workhive workspace destroy && workhive workspace open",
    );
    m.insert(
        "doctor.advice.pty_exhausted",
        "关闭未使用的终端会话以释放 PTY 设备",
    );
    m.insert(
        "doctor.advice.pty_elevated",
        "PTY 使用率较高；建议关闭闲置会话",
    );
    m.insert(
        "doctor.advice.nested_tmux",
        "在连接前取消 $TMUX 变量：unset TMUX && workhive workspace attach",
    );
    m.insert(
        "doctor.advice.session_missing",
        "会话不存在；运行 workhive workspace open 创建",
    );
    m.insert(
        "doctor.advice.session_server_unreachable",
        "tmux 服务器不可达，无法确定会话状态",
    );
    m.insert(
        "doctor.advice.session_lookup_failed",
        "会话查询返回了异常或无法解析的结果",
    );
    m.insert(
        "doctor.advice.invalid_workspace_name",
        "工作区目录名不是有效的工作区标识，已跳过诊断",
    );
    m.insert("doctor.all_passed", "所有检查均通过。");
    m.insert("doctor.has_issues", "发现 {0} 个问题。");
    m.insert(
        "doctor.project_not_initialized",
        "项目尚未初始化 — 请先运行 `workhive init`",
    );
    m.insert("completions.installed", "已安装 {0} 补全到 {1}。");
    m.insert(
        "completions.activate.zsh.rc",
        "已更新 {0} 以便 zsh 加载补全。\n请打开新终端，或运行 `source {0}` 立即启用补全。",
    );
    m.insert(
        "completions.activate.zsh.no_rc",
        "请打开新终端，或运行 `source ~/.zshrc` 立即启用补全。",
    );
    m.insert(
        "completions.activate.bash",
        "请打开新终端，或确认 bash-completion 已加载，并运行 `source ~/.bashrc`（部分系统使用 `source ~/.bash_profile`）。",
    );
    m.insert(
        "completions.activate.fish",
        "请打开新的 fish shell，或运行 `exec fish` 在当前终端重新加载补全。",
    );
    m.insert(
        "completions.activate.powershell",
        "请打开新的 PowerShell 会话，或在已引入 Workhive 补全的配置文件中运行 `. $PROFILE` 重新加载。",
    );
    m.insert(
        "completions.activate.elvish",
        "请打开新的 Elvish shell，或在 rc.elv 中加载已安装的补全脚本。",
    );
    m.insert(
        "completions.activate.generic",
        "请打开新终端，或重新加载 shell 的补全配置以立即启用补全。",
    );
    m.insert(
        "doctor.error.project_not_detected",
        "无法检测项目 — 请在 workhive 初始化的目录中运行，或使用 --project 指定。",
    );
    m.insert("doctor.error.workspace_enum_failed", "枚举项目工作区失败。");
    m.insert(
        "doctor.error.snapshot_root_unknown",
        "无法确定快照根目录 — 项目检测失败。",
    );
}
