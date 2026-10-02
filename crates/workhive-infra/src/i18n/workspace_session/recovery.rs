//! Recovery path, reattach outcome, readiness progress, and .env injection copy.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Open: recovery path
    m.insert("open.recovery.live_session", "Reused existing live session");
    m.insert("open.recovery.layout", "Applied layout configuration");

    // Reattach outcome
    m.insert("reattach.reattached", "Session reattached successfully");
    m.insert(
        "reattach.created_fresh",
        "Session was missing; created fresh from recovery chain",
    );
    m.insert(
        "reattach.backend_recovered",
        "Backend was unreachable; new server started and session created",
    );
    m.insert("reattach.unrecoverable", "Session unrecoverable: {0}");

    // Recovery action
    m.insert("recovery.action.none", "No action needed");
    m.insert("recovery.action.recoverable", "Auto-recovered: {0}");
    m.insert(
        "recovery.action.manual",
        "Manual intervention required: {0}",
    );

    // Open: bounded pane-readiness progress
    m.insert(
        "open.progress.waiting_panes",
        "Waiting for panes to be ready before attaching...",
    );
    m.insert(
        "open.progress.panes_ready",
        "{0} pane(s) ready in {1} ms; attaching...",
    );
    m.insert(
        "open.progress.readiness_timeout",
        "Pane readiness timed out after {0} ms (saw {1} of {2} panes); attaching anyway.",
    );
    m.insert(
        "open.progress.readiness_degraded",
        "Could not observe pane readiness ({0}); attaching anyway.",
    );
    m.insert(
        "open.progress.no_attach_summary",
        "--no-attach: workspace '{0}' provisioned without attach; readiness wait skipped.",
    );
    m.insert(
        "open.selector.non_interactive",
        "The interactive workspace selector requires a terminal. In non-interactive mode, pass the workspace name explicitly, e.g.: workhive workspace open <name> --no-attach",
    );

    // .env injection summary. Values are NEVER rendered; only counts
    // and key names are shown.
    m.insert(
        "open.env.injected_global_only",
        "Environment: injected {0} variable(s) (source: global .env)",
    );
    m.insert(
        "open.env.injected_project_only",
        "Environment: injected {0} variable(s) (source: project .env)",
    );
    m.insert(
        "open.env.injected_mixed",
        "Environment: injected {0} variable(s) (global {1}, project override {2})",
    );
    m.insert("open.env.key_item", "  {0}");
    m.insert(
        "open.env.parse_warning",
        "warning: .env at {0} line {1}: {2}",
    );
    m.insert(
        "open.env.parse_reason.looks_like_yaml",
        ".env uses KEY=VALUE syntax; YAML-style 'KEY: VALUE' is not supported",
    );
    m.insert(
        "open.env.parse_reason.missing_equals",
        "missing '=' separator (expected KEY=VALUE)",
    );
    m.insert(
        "open.env.parse_reason.empty_key",
        "empty key before '=' separator",
    );
    m.insert(
        "open.env.parse_reason.invalid_key_chars",
        "key contains characters that are not allowed in environment variable names",
    );
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // Open: recovery path
    m.insert("open.recovery.live_session", "复用了已有的活跃会话");
    m.insert("open.recovery.layout", "已应用布局配置");

    // Reattach outcome
    m.insert("reattach.reattached", "会话重连成功");
    m.insert(
        "reattach.created_fresh",
        "会话不存在，已通过恢复链路重新创建",
    );
    m.insert(
        "reattach.backend_recovered",
        "后端不可达，已启动新服务器并创建会话",
    );
    m.insert("reattach.unrecoverable", "会话不可恢复：{0}");

    // Recovery action
    m.insert("recovery.action.none", "无需操作");
    m.insert("recovery.action.recoverable", "已自动恢复：{0}");
    m.insert("recovery.action.manual", "需要手动介入：{0}");

    // Open: bounded pane-readiness progress (zh-CN)
    m.insert("open.progress.waiting_panes", "等待窗格就绪后再接入会话...");
    m.insert(
        "open.progress.panes_ready",
        "已就绪 {0} 个窗格，用时 {1} 毫秒，正在接入...",
    );
    m.insert(
        "open.progress.readiness_timeout",
        "窗格就绪等待超时（{0} 毫秒，仅观测到 {1} / {2} 个窗格），将继续接入。",
    );
    m.insert(
        "open.progress.readiness_degraded",
        "无法观测窗格就绪状态（{0}），将继续接入。",
    );
    m.insert(
        "open.progress.no_attach_summary",
        "--no-attach：已创建工作区 '{0}'，未自动接入；本次跳过窗格就绪等待。",
    );
    m.insert(
        "open.selector.non_interactive",
        "交互式工作区选择器需要终端。非交互模式下请显式提供工作区名，例如：workhive workspace open <name> --no-attach",
    );

    // .env injection summary. Values are NEVER rendered; only counts
    // and key names are shown.
    m.insert(
        "open.env.injected_global_only",
        "环境变量：已注入 {0} 个（来源：全局 .env）",
    );
    m.insert(
        "open.env.injected_project_only",
        "环境变量：已注入 {0} 个（来源：项目 .env）",
    );
    m.insert(
        "open.env.injected_mixed",
        "环境变量：已注入 {0} 个（全局 {1} 个，项目覆盖 {2} 个）",
    );
    m.insert("open.env.key_item", "  {0}");
    m.insert(
        "open.env.parse_warning",
        "警告：.env 文件 {0} 第 {1} 行：{2}",
    );
    m.insert(
        "open.env.parse_reason.looks_like_yaml",
        ".env 使用 KEY=VALUE 语法；不支持 YAML 风格的 'KEY: VALUE'",
    );
    m.insert(
        "open.env.parse_reason.missing_equals",
        "缺少 '=' 分隔符（期望格式 KEY=VALUE）",
    );
    m.insert("open.env.parse_reason.empty_key", "'=' 之前的键名为空");
    m.insert(
        "open.env.parse_reason.invalid_key_chars",
        "键名包含不允许出现在环境变量名中的字符",
    );
}
