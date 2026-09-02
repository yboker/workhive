//! Pane kind/state/activity labels, concise activity, message-safety, and AI-activity source labels.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Pane kind labels
    m.insert("pane.kind.agent", "AI agent");
    m.insert("pane.kind.terminal", "terminal");
    m.insert("pane.kind.git_ui", "git UI");
    m.insert("pane.kind.unknown", "unknown");
    // Pane state labels
    m.insert("pane.state.running", "running");
    m.insert("pane.state.idle", "idle");
    m.insert("pane.state.unknown", "unknown");
    // Pane activity labels (user-facing — separate from raw PaneState).
    // Phrasing is deliberately conservative: `foreground process` does
    // not say "running" / "active" / "busy" because we cannot prove a
    // foreground non-shell process is actually doing work.
    m.insert("pane.activity.unknown", "unknown");
    m.insert("pane.activity.shell_idle", "shell idle");
    m.insert("pane.activity.foreground_process", "foreground process");
    // The runtime only confirms the wrapped AI agent process is
    // alive, NOT that it is actively processing. The label must avoid
    // wording such as "running" / "busy" / "active".
    m.insert(
        "pane.activity.agent_foreground",
        "AI assistant open (activity unverified)",
    );
    // Reserved for a future explicit busy/processing protocol
    // handshake. Currently unreachable from the observation pipeline.
    m.insert("pane.activity.agent_busy", "AI assistant busy");
    m.insert("pane.activity.exited", "previous process exited");
    // Pane concise activity labels (pane list only).
    m.insert("pane.concise_activity.unknown", "unknown");
    m.insert("pane.concise_activity.waiting_input", "waiting for input");
    m.insert("pane.concise_activity.running", "running");
    m.insert("pane.concise_activity.completed", "completed");
    m.insert("pane.concise_activity.exited", "exited");
    // Pane message-safety labels.
    m.insert("pane.message_safety.unknown", "unknown");
    m.insert("pane.message_safety.not_applicable", "not applicable (TUI)");
    m.insert("pane.message_safety.shell_input", "shell input only");
    m.insert(
        "pane.message_safety.unverified_agent",
        "unverified agent (no direct activity reading yet)",
    );
    m.insert(
        "pane.message_safety.safe_structured",
        "structured messaging supported",
    );
    m.insert("pane.label.activity", "activity");
    m.insert("pane.label.message_safety", "message safety");
    m.insert("pane.list.header.activity", "Activity");
    // AI assistant activity labels (Claude Code / Codex / OpenCode
    // completion signals) — separate from process-level
    // pane.activity.*.
    m.insert("pane.ai_activity.unknown", "unknown");
    // User-facing wording for `OpenUnverified`. The domain state still
    // expresses "process visible, no completion signal recorded for
    // the current task yet"; the default-output label MUST NOT speak
    // telemetry vocabulary (`activity unverified`) and MUST NOT claim
    // the pane is processing, busy, broken, or unusable. The chosen
    // phrasing answers the user's first question: the pane is open and
    // Workhive is waiting for the next completion signal before it can
    // say more.
    m.insert(
        "pane.ai_activity.open_unverified",
        "open (waiting for signal)",
    );
    m.insert("pane.ai_activity.processing", "processing");
    m.insert("pane.ai_activity.tool_running", "tool running");
    m.insert(
        "pane.ai_activity.waiting_for_permission",
        "waiting for permission",
    );
    m.insert("pane.ai_activity.waiting_for_user", "waiting for user");
    m.insert("pane.ai_activity.error", "error");
    m.insert("pane.ai_provider.claude_code", "Claude Code");
    m.insert("pane.ai_provider.codex", "Codex");
    m.insert("pane.ai_provider.opencode", "OpenCode");
    m.insert("pane.ai_provider.none", "none");
    m.insert("pane.ai_activity.reason_label", "reason");
    m.insert("pane.ai_activity.stale_hint", "(stale)");
    m.insert("pane.ai_activity.no_fact_hint", "(no provider fact yet)");
    m.insert("pane.ai_activity.label", "AI activity");
    m.insert("pane.ai_activity.provider_label", "AI provider");
    m.insert("pane.ai_activity.confidence_label", "AI confidence");
    m.insert("pane.ai_activity.stale_label", "AI stale");
    m.insert("pane.ai_activity.confidence.hook", "hook");
    m.insert("pane.ai_activity.confidence.plugin", "plugin");
    m.insert("pane.ai_activity.confidence.heuristic", "heuristic");
    m.insert("pane.ai_activity.confidence.fallback", "fallback");
    // Productized activity-source / accuracy / guidance. The vocabulary
    // is stable; localized values intentionally avoid the removed
    // `workhive integrations` command name.
    m.insert("pane.ai_activity.source_label", "AI source");
    m.insert(
        "pane.ai_activity.source.claude_code_hook",
        "Claude Code hook",
    );
    m.insert("pane.ai_activity.source.codex_hook", "Codex hook");
    m.insert("pane.ai_activity.source.opencode_plugin", "OpenCode plugin");
    m.insert("pane.ai_activity.source.terminal_status", "terminal status");
    m.insert("pane.ai_activity.source.supervisor", "Workhive observation");
    m.insert(
        "pane.ai_activity.source.process_observer",
        "process observer",
    );
    m.insert("pane.ai_activity.source.tmux_fallback", "tmux fallback");
    m.insert("pane.ai_activity.source.none", "none");
    m.insert("pane.ai_activity.accuracy_label", "AI accuracy");
    m.insert("pane.ai_activity.accuracy.high", "high");
    m.insert("pane.ai_activity.accuracy.medium", "medium");
    m.insert("pane.ai_activity.accuracy.medium_low", "medium-low");
    m.insert("pane.ai_activity.accuracy.low", "low");
    m.insert("pane.ai_activity.accuracy.none", "none");
    m.insert(
        "pane.ai_activity.guidance.fallback",
        "Activity is read from a fallback signal and may be inaccurate.\nTo improve accuracy, launch Claude / Codex / OpenCode through Workhive so it can observe the tool directly.\nIf accuracy stays degraded, run `workhive doctor` to check the general Workhive environment.",
    );
    m.insert(
        "pane.ai_activity.guidance.stale",
        "The last AI activity reading has aged out; the displayed state may be out of date.\nRelaunching the AI tool through Workhive refreshes the activity reading.\nIf the state stays stale, run `workhive doctor` to check the general Workhive environment.",
    );
    m.insert(
        "pane.ai_activity.guidance.unverified",
        "Workhive can see the AI tool in the foreground but has not yet received an activity reading from it.\nLaunch the AI tool through Workhive so it can observe the new session directly.\nIf no activity arrives, run `workhive doctor` to check the general Workhive environment.",
    );
    m.insert(
        "pane.list.activity_source_legend",
        "Source ordering by accuracy: completion signal > terminal-status > process > tmux > none.",
    );
    // Per-row product-facing detection-source tokens rendered in
    // the dedicated `source` column added to `pane list`. Tokens
    // are intentionally short (single English word, ≤2 CJK chars
    // for ZH) so the column survives `print_table_compact`'s
    // narrow-screen drop policy.
    m.insert("pane.ai_activity.source.tag.hook", "hook");
    m.insert("pane.ai_activity.source.tag.plugin", "plugin");
    m.insert(
        "pane.ai_activity.source.tag.terminal_status",
        "terminal-status",
    );
    m.insert("pane.ai_activity.source.tag.supervisor", "observer");
    m.insert("pane.ai_activity.source.tag.process", "process");
    m.insert("pane.ai_activity.source.tag.tmux", "tmux");
    m.insert("pane.ai_activity.source.tag.none", "none");
    m.insert("pane.list.header.source", "source");
    m.insert("pane.list.header.confidence", "confidence");
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // Pane kind labels
    m.insert("pane.kind.agent", "AI助手");
    m.insert("pane.kind.terminal", "终端");
    m.insert("pane.kind.git_ui", "Git界面");
    m.insert("pane.kind.unknown", "未知");
    // Pane state labels
    m.insert("pane.state.running", "运行中");
    m.insert("pane.state.idle", "空闲");
    m.insert("pane.state.unknown", "未知");
    // Pane activity labels (user-facing — separate from raw PaneState).
    // Phrasing is deliberately conservative: `foreground process` does
    // not say "running" / "active" / "busy" because we cannot prove a
    // foreground non-shell process is actually doing work.
    m.insert("pane.activity.unknown", "未知");
    m.insert("pane.activity.shell_idle", "Shell 空闲");
    m.insert("pane.activity.foreground_process", "存在前台进程");
    // The runtime only confirms the wrapped AI agent process is
    // alive, NOT that it is actively processing. The label must avoid
    // wording such as "running" / "busy" / "active".
    m.insert(
        "pane.activity.agent_foreground",
        "AI 助手已打开（活动状态未确认）",
    );
    // Reserved for a future explicit busy/processing protocol
    // handshake. Currently unreachable from the observation pipeline.
    m.insert("pane.activity.agent_busy", "AI 助手处理中");
    m.insert("pane.activity.exited", "前台进程已退出");
    // Pane concise activity labels (pane list only).
    m.insert("pane.concise_activity.unknown", "未知");
    m.insert("pane.concise_activity.waiting_input", "等待输入");
    m.insert("pane.concise_activity.running", "执行中");
    m.insert("pane.concise_activity.completed", "已完成");
    m.insert("pane.concise_activity.exited", "已退出");
    // Pane message-safety labels.
    m.insert("pane.message_safety.unknown", "未知");
    m.insert("pane.message_safety.not_applicable", "不适用（TUI 界面）");
    m.insert("pane.message_safety.shell_input", "仅适合发送 shell 命令");
    m.insert(
        "pane.message_safety.unverified_agent",
        "未纳管的 AI 助手（尚未读取到直接活动信号）",
    );
    m.insert(
        "pane.message_safety.safe_structured",
        "可安全接收结构化消息",
    );
    m.insert("pane.label.activity", "活动状态");
    m.insert("pane.label.message_safety", "消息可达性");
    m.insert("pane.list.header.activity", "活动状态");
    // AI assistant activity labels (Claude Code / Codex / OpenCode
    // completion signals) — separate from process-level
    // pane.activity.*.
    m.insert("pane.ai_activity.unknown", "未知");
    // See EN comment above for product wording authority. The zh-CN
    // copy must match the EN semantics (open + still waiting on a
    // provider event) without telemetry-flavored unverified-activity
    // wording, which reads as confusing rather than reassuring.
    m.insert("pane.ai_activity.open_unverified", "已打开（等待信号）");
    m.insert("pane.ai_activity.processing", "处理中");
    m.insert("pane.ai_activity.tool_running", "工具执行中");
    m.insert("pane.ai_activity.waiting_for_permission", "等待授权");
    m.insert("pane.ai_activity.waiting_for_user", "等待用户");
    m.insert("pane.ai_activity.error", "错误");
    m.insert("pane.ai_provider.claude_code", "Claude Code");
    m.insert("pane.ai_provider.codex", "Codex");
    m.insert("pane.ai_provider.opencode", "OpenCode");
    m.insert("pane.ai_provider.none", "无");
    m.insert("pane.ai_activity.reason_label", "原因");
    m.insert("pane.ai_activity.stale_hint", "（已过期）");
    m.insert("pane.ai_activity.no_fact_hint", "（暂无 provider 事件）");
    m.insert("pane.ai_activity.label", "AI 活动");
    m.insert("pane.ai_activity.provider_label", "AI 提供方");
    m.insert("pane.ai_activity.confidence_label", "AI 置信度");
    m.insert("pane.ai_activity.stale_label", "AI 已过期");
    m.insert("pane.ai_activity.confidence.hook", "hook");
    m.insert("pane.ai_activity.confidence.plugin", "plugin");
    m.insert("pane.ai_activity.confidence.heuristic", "heuristic");
    m.insert("pane.ai_activity.confidence.fallback", "fallback");
    // Productized activity-source / accuracy / guidance. The vocabulary
    // is stable; localized values intentionally avoid the removed
    // `workhive integrations` command name.
    m.insert("pane.ai_activity.source_label", "AI 来源");
    m.insert(
        "pane.ai_activity.source.claude_code_hook",
        "Claude Code hook",
    );
    m.insert("pane.ai_activity.source.codex_hook", "Codex hook");
    m.insert("pane.ai_activity.source.opencode_plugin", "OpenCode 插件");
    m.insert("pane.ai_activity.source.terminal_status", "终端状态");
    m.insert("pane.ai_activity.source.supervisor", "Workhive 监控进程");
    m.insert("pane.ai_activity.source.process_observer", "进程观测器");
    m.insert("pane.ai_activity.source.tmux_fallback", "tmux 回退");
    m.insert("pane.ai_activity.source.none", "无");
    m.insert("pane.ai_activity.accuracy_label", "AI 准确性");
    m.insert("pane.ai_activity.accuracy.high", "高");
    m.insert("pane.ai_activity.accuracy.medium", "中");
    m.insert("pane.ai_activity.accuracy.medium_low", "中低");
    m.insert("pane.ai_activity.accuracy.low", "低");
    m.insert("pane.ai_activity.accuracy.none", "无");
    m.insert(
        "pane.ai_activity.guidance.fallback",
        "当前活动来自回退信号，可能不准确。\n要提高准确性，请通过 Workhive 启动 Claude / Codex / OpenCode，使 Workhive 能直接观察到该工具。\n若仍不准确，可运行 `workhive doctor` 检查 Workhive 通用环境。",
    );
    m.insert(
        "pane.ai_activity.guidance.stale",
        "上一次 AI 活动读数已超时，显示状态可能不再准确。\n通过 Workhive 重新启动 AI 工具会刷新活动读数。\n若仍显示过期，可运行 `workhive doctor` 检查 Workhive 通用环境。",
    );
    m.insert(
        "pane.ai_activity.guidance.unverified",
        "Workhive 已检测到 AI 工具在前台运行，但尚未收到来自该工具的活动读数。\n请通过 Workhive 重新启动 AI 工具，让 Workhive 能直接观察这次会话。\n若仍无活动读数，可运行 `workhive doctor` 检查 Workhive 通用环境。",
    );
    m.insert(
        "pane.list.activity_source_legend",
        "来源按准确性排序：完成信号 > 终端状态 > 进程 > tmux > 无。",
    );
    // Per-row product-facing detection-source tokens rendered in
    // the dedicated `source` column added to `pane list`. Tokens
    // are intentionally short so the column survives the
    // narrow-screen drop policy.
    m.insert("pane.ai_activity.source.tag.hook", "钩子");
    m.insert("pane.ai_activity.source.tag.plugin", "插件");
    m.insert("pane.ai_activity.source.tag.terminal_status", "终端");
    m.insert("pane.ai_activity.source.tag.supervisor", "监控");
    m.insert("pane.ai_activity.source.tag.process", "进程");
    m.insert("pane.ai_activity.source.tag.tmux", "tmux");
    m.insert("pane.ai_activity.source.tag.none", "无");
    m.insert("pane.list.header.source", "来源");
    m.insert("pane.list.header.confidence", "可信度");
}
