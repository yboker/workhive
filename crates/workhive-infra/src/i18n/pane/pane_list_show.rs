//! Pane commands, targeting/send guards, labels, list/show rendering, metrics, supervisor diagnostics, observation, intercept, and presets.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Pane
    m.insert("pane.confirm_split", "Split this pane?");
    m.insert("pane.confirm_close", "Close this pane?");
    m.insert("pane.confirm_send", "Send text to pane {0}?");
    m.insert(
        "pane.confirm_send_resolved",
        "Send text to pane {0} (resolved target {1})?",
    );
    m.insert("pane.split_success", "Pane split successfully.");
    m.insert("pane.close_success", "Pane closed.");
    m.insert("pane.send_success", "Text sent to pane {0}.");
    m.insert(
        "pane.send_success_resolved",
        "Text sent to pane {0} (resolved target {1}).",
    );

    m.insert(
        "pane.target.index_not_found",
        "Pane number {0} was not found. Run `workhive pane list` to see pane numbers and target IDs.",
    );
    m.insert(
        "pane.target.invalid",
        "Invalid pane target '{0}'. Use a pane number such as `2` or a target ID such as `%2`.",
    );
    m.insert(
        "pane.target.self_rejected",
        "Cannot send text to the current pane. Choose another pane target; \
         run `workhive pane list` to see available pane numbers.",
    );
    m.insert(
        "pane.send.refuse.tui",
        "Refused to send chat text to pane {0}: target is a TUI (such as lazygit) \
         and is not a structured message receiver. Use `workhive pane list` to \
         see candidate AI panes.",
    );
    m.insert(
        "pane.send.refuse.unknown",
        "Refused to send chat text to pane {0}: target's runtime is unknown or \
         unverified. Workhive will not silently inject keystrokes into a pane \
         it cannot classify.",
    );
    m.insert(
        "pane.send.refuse.shell",
        "Refused to send chat text to pane {0}: target is a shell prompt, \
         not a structured message receiver. Workhive will not silently treat \
         chat text as a shell command. Use `workhive pane list` to find an \
         AI pane.",
    );
    m.insert(
        "pane.send.refuse.hint_force",
        "  Hint: pass --force to send raw keystrokes anyway (advanced; this \
         types the message directly into the target as keyboard input and \
         may disrupt shell or TUI state).",
    );
    m.insert(
        "pane.send.warn.forced_shell",
        "Warning: --force is sending raw keystrokes to shell pane {0}. The \
         text will be typed at the shell prompt as keyboard input — treat it \
         as a shell command.",
    );
    m.insert(
        "pane.send.warn.unverified_agent",
        "Note: pane {0} looks like an AI agent but Workhive has not yet \
         received a direct activity reading from it. The message will be \
         inserted, but submit behavior is best-effort.",
    );
    m.insert(
        "pane.send.warn.forced_tui",
        "Warning: --force is sending raw keystrokes to TUI pane {0}. This may \
         corrupt the TUI's screen state.",
    );
    m.insert(
        "pane.send.warn.forced_unknown",
        "Warning: --force is sending raw keystrokes to unverified pane {0}.",
    );
    m.insert(
        "pane.send.warn.activity_guard_forced",
        "Warning: --force is overriding the activity guard for pane {0}. \
         The pane may be busy, waiting for permission, or in an uncertain \
         state. Proceed with caution.",
    );
    m.insert(
        "pane.send.refuse.activity_guard",
        "Cannot send to pane {0}: activity guard blocked the send ({1}). \
         The target pane may be busy, running a tool, waiting for permission, \
         or in an uncertain state. Use --force to override.",
    );
    m.insert(
        "pane.control_plane.unavailable",
        "Pane runtime telemetry is unavailable; failed to deliver text to pane {0}. \
         Workhive attempted one retry and gave up. Inspect Workhive logs for details.",
    );
    m.insert(
        "pane.control_plane.unknown_pane",
        "Pane runtime telemetry does not know about pane {0}. \
         The pane may have been closed or never registered.",
    );

    // Pane labels
    m.insert("pane.label.id", "id");
    m.insert("pane.label.cwd", "cwd");
    m.insert("pane.label.command", "command");
    m.insert("pane.label.kind", "kind");
    m.insert("pane.label.state", "observed state");
    m.insert("pane.label.index", "index");
    m.insert("pane.label.title", "title");
    m.insert("pane.label.launch_identity", "launch identity");
    m.insert("pane.launch_identity.none", "—");
    // Pane list/show display
    m.insert("pane.list.title", "Panes");
    m.insert("pane.list.header.active", "active");
    m.insert("pane.list.header.target_id", "target id");
    m.insert("pane.list.header.index", "idx");
    m.insert("pane.list.header.number", "No.");
    m.insert("pane.list.header.title", "title");
    m.insert("pane.list.header.command", "command");
    m.insert("pane.list.header.kind", "type");
    m.insert("pane.list.header.state", "observed state");
    m.insert("pane.list.header.cwd", "cwd");
    m.insert("pane.list.header.launch_identity", "launch identity");
    m.insert("pane.list.header.label", "label");
    m.insert("pane.list.label.none", "—");
    m.insert(
        "pane.list.target_id_hint",
        "Hint: use the pane number or target ID, e.g. `workhive pane send 2 \"...\"` or `workhive pane send %2 \"...\"`.",
    );
    // Structured hint block for `pane list`. Each key is a short
    // single-line tip so terminal wrapping stays stable across locales
    // and command examples are never split mid-token.
    m.insert("pane.list.hint.heading", "Tips:");
    m.insert(
        "pane.list.hint.active_marker",
        "* in the active column marks the currently focused pane.",
    );
    m.insert(
        "pane.list.hint.send_intro",
        "Send to a pane by its number (No.) or target ID, e.g.:",
    );
    m.insert(
        "pane.list.empty",
        "No panes are currently registered for this workspace.",
    );
    m.insert("pane.show.title", "Current pane");

    // ACP metrics render keys (Activity Intelligence v1).
    // These keys are used by `pane_metrics_view.rs` for `pane list` and
    // `pane show` to surface typed `PaneActivityMetrics` values.
    m.insert("pane.metrics.partial", "~");
    m.insert("pane.metrics.unavailable", "—");
    m.insert("pane.metrics.source.acp", "acp");
    m.insert("pane.metrics.source.legacy", "legacy");
    m.insert("pane.metrics.source.none", "none");
    m.insert("pane.metrics.label.provider", "AI provider");
    m.insert("pane.metrics.label.state", "AI state");
    m.insert("pane.metrics.label.source", "AI data source");
    m.insert("pane.metrics.label.confidence", "AI confidence");
    m.insert("pane.metrics.label.freshness", "AI freshness");
    m.insert("pane.metrics.label.elapsed", "time in state");
    m.insert("pane.metrics.label.current_turn", "current turn");
    m.insert("pane.metrics.label.last_turn", "last turn");
    m.insert("pane.metrics.label.last_tool", "last tool");
    m.insert("pane.metrics.label.aggregate_tool", "total tool time");
    m.insert("pane.metrics.label.permission_wait", "permission wait");
    m.insert("pane.metrics.label.idle", "idle for");
    m.insert("pane.metrics.label.hint", "next action");
    m.insert("pane.metrics.block.heading", "AI metrics:");
    m.insert(
        "pane.metrics.hint.permission_needed",
        "waiting for permission — approve or deny in the pane",
    );
    m.insert("pane.metrics.hint.completed", "last turn completed");
    m.insert("pane.metrics.hint.failed", "last turn failed");
    // Pane supervisor exit-policy diagnostics. Surfaced on stderr by
    // `workhive internal pane-supervisor` so the pane never closes
    // silently on a recoverable failure. Kept short because the pane
    // itself *is* the surface.
    m.insert(
        "pane.supervisor.spawn_failed",
        "workhive: failed to launch '{0}': {1}",
    );
    m.insert(
        "pane.supervisor.replacement_shell_failed",
        "workhive: could not respawn shell in pane: {0}",
    );
    m.insert(
        "pane.supervisor.exit_behavior_untrusted",
        "workhive: pane.exit.tool_exit_behavior config could not be trusted ({0}); pane will close so the exit code stays visible",
    );
    // No-hook-event hint sub-block. The body and Codex review line
    // are conservative: they describe the observable fact and a
    // Codex product step; they never assert hooks are uninstalled
    // and never reference a nonexistent command.
    m.insert(
        "pane.list.hint.no_hook_event.heading",
        "Completion signals:",
    );
    m.insert(
        "pane.list.hint.no_hook_event.body",
        "  No completion signal has been observed yet for some AI panes (see the `source` column). Workhive records a signal each time an AI tool finishes a task in the pane.",
    );
    m.insert("pane.list.hint.no_hook_event.codex_review", "");
    // Column-legend sub-block. Each line is a complete sentence so
    // terminal wrapping never splits an explanation mid-clause. The
    // legend documents column meaning only; per-row hook approval
    // guidance lives in the separate `no_hook_event` block above.
    m.insert("pane.list.legend.heading", "Column legend:");
    m.insert(
        "pane.list.legend.activity_unverified",
        "  activity column shows one of: waiting for input, running, completed, exited, unknown. Run `workhive pane show <id>` for the underlying source, confidence, and reason.",
    );
    m.insert(
        "pane.list.legend.source",
        "  source: where the activity came from. Ordering by accuracy: completion signal > terminal-status > process > tmux > none.",
    );
    m.insert(
        "pane.list.legend.confidence",
        "  confidence: how trustworthy the source is for activity assertions (recorded completion signals are highest; fallback inference is lower).",
    );
    // Concise default-mode hints. Single-line each. Verbose mode is
    // gated separately and re-emits the full Tips / Column legend /
    // hook-event blocks above.
    m.insert(
        "pane.list.default_hint",
        "Send to a pane: `workhive pane send <number-or-id> \"...\"`. For column legend and details run with `--verbose`, or `workhive pane show <pane>`.",
    );
    m.insert(
        "pane.list.default_hint.pending_event",
        "Some AI panes have not recorded a completion signal yet — see `source` column; run `workhive pane show <pane>` or `--verbose` for details.",
    );
    m.insert("pane.list.default_hint.codex_review", "");

    // Pane observation source / confidence labels surfaced by `pane show`.
    m.insert("pane.observation.source.supervisor", "workhive supervisor");
    m.insert(
        "pane.observation.source.process_observer",
        "OS process observer",
    );
    m.insert("pane.observation.source.tmux", "tmux fallback");
    m.insert("pane.observation.source.unknown", "no observation");
    m.insert("pane.observation.confidence.authoritative", "authoritative");
    m.insert("pane.observation.confidence.high", "high");
    m.insert("pane.observation.confidence.fallback", "fallback");
    m.insert("pane.observation.confidence.none", "none");
    m.insert("pane.label.observation_source", "observation source");
    m.insert(
        "pane.label.observation_confidence",
        "observation confidence",
    );

    // Intercept
    m.insert(
        "intercept.rerun_hint",
        "Global configuration complete. Please re-run your command:\n\n  {0}\n",
    );
    m.insert(
        "intercept.project_init_prompt",
        "Project not initialized. Run 'workhive init' now?",
    );

    // Pane preset labels
    m.insert("pane.preset.claude", "Claude Code (AI Agent)");
    m.insert("pane.preset.codex", "Codex (AI Agent)");
    m.insert("pane.preset.opencode", "OpenCode (AI Agent)");
    // Cursor Agent label normalization: matches the other built-in AI
    // agents so the init wizard does not visually segregate it. The
    // canonical command identity stays `cursor-agent`; this string is
    // display-only.
    m.insert("pane.preset.cursor_agent", "Cursor Agent (AI Agent)");
    m.insert("pane.preset.lazygit", "Lazygit (Git TUI)");
    m.insert("pane.preset.yazi", "Yazi (File Manager)");
    m.insert("pane.preset.htop", "htop (System Monitor)");
    m.insert("pane.preset.bottom", "bottom (System Monitor)");
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // Pane
    m.insert("pane.confirm_split", "是否分割窗格？");
    m.insert("pane.confirm_close", "是否关闭此窗格？");
    m.insert("pane.confirm_send", "是否发送文本到窗格 {0}？");
    m.insert(
        "pane.confirm_send_resolved",
        "是否发送文本到窗格 {0}（解析为目标 {1}）？",
    );
    m.insert("pane.split_success", "窗格已分割。");
    m.insert("pane.close_success", "窗格已关闭。");
    m.insert("pane.send_success", "文本已发送到窗格 {0}。");
    m.insert(
        "pane.send_success_resolved",
        "文本已发送到窗格 {0}（解析为目标 {1}）。",
    );

    m.insert(
        "pane.target.index_not_found",
        "未找到编号为 {0} 的窗格。请运行 `workhive pane list` 查看窗格编号和目标ID。",
    );
    m.insert(
        "pane.target.invalid",
        "无效的窗格目标 '{0}'。请使用窗格编号（例如 `2`）或目标ID（例如 `%2`）。",
    );
    m.insert(
        "pane.target.self_rejected",
        "不能发送文本到当前窗格。请选择另一个窗格目标，例如运行 `workhive pane list` 查看可发送的窗格编号。",
    );
    m.insert(
        "pane.send.refuse.tui",
        "拒绝向窗格 {0} 发送聊天文本：目标是 TUI（如 lazygit），不是结构化消息接收方。\
         请运行 `workhive pane list` 选择可接收消息的 AI 窗格。",
    );
    m.insert(
        "pane.send.refuse.unknown",
        "拒绝向窗格 {0} 发送聊天文本：目标运行时未知或未验证。\
         Workhive 不会向无法识别的窗格静默注入按键。",
    );
    m.insert(
        "pane.send.refuse.shell",
        "拒绝向窗格 {0} 发送聊天文本：目标是 shell 提示符，不是结构化消息接收方。\
         Workhive 不会把聊天文本静默当作 shell 命令执行。\
         请运行 `workhive pane list` 选择 AI 窗格。",
    );
    m.insert(
        "pane.send.refuse.hint_force",
        "  提示：使用 --force 可以强制发送原始按键（高级选项，文本会作为键盘输入直接注入目标，\
         可能破坏 shell 或 TUI 显示状态）。",
    );
    m.insert(
        "pane.send.warn.forced_shell",
        "警告：--force 正在向 shell 窗格 {0} 发送原始按键。文本会作为键盘输入注入到 shell 提示符，\
         请按 shell 命令对待。",
    );
    m.insert(
        "pane.send.warn.unverified_agent",
        "提示：窗格 {0} 看起来是 AI 助手，但 Workhive 尚未直接读取到该窗格的活动信号。\
         消息会被插入，但提交行为属于尽力而为。",
    );
    m.insert(
        "pane.send.warn.forced_tui",
        "警告：--force 正在向 TUI 窗格 {0} 发送原始按键，可能破坏 TUI 显示状态。",
    );
    m.insert(
        "pane.send.warn.forced_unknown",
        "警告：--force 正在向未验证的窗格 {0} 发送原始按键。",
    );
    m.insert(
        "pane.send.warn.activity_guard_forced",
        "警告：--force 已覆盖窗格 {0} 的活动守卫。该窗格可能正忙、\
         等待权限或处于不确定状态，请谨慎操作。",
    );
    m.insert(
        "pane.send.refuse.activity_guard",
        "无法向窗格 {0} 发送消息：活动守卫阻止了发送（{1}）。\
         目标窗格可能正忙、正在执行工具、等待权限或处于不确定状态。可使用 --force 强制覆盖。",
    );
    m.insert(
        "pane.control_plane.unavailable",
        "窗格运行时遥测不可用，无法将文本发送到窗格 {0}。\
         Workhive 已尝试一次重试仍然失败，请检查 Workhive 日志。",
    );
    m.insert(
        "pane.control_plane.unknown_pane",
        "窗格运行时遥测无法识别窗格 {0}（可能已关闭或从未注册）。",
    );

    // Pane labels
    m.insert("pane.label.id", "目标ID");
    m.insert("pane.label.cwd", "工作目录");
    m.insert("pane.label.command", "命令");
    m.insert("pane.label.kind", "类型");
    m.insert("pane.label.state", "观测状态");
    m.insert("pane.label.index", "序号");
    m.insert("pane.label.title", "标题");
    m.insert("pane.label.launch_identity", "启动身份");
    m.insert("pane.launch_identity.none", "—");
    // Pane list/show display
    m.insert("pane.list.title", "窗格列表");
    m.insert("pane.list.header.active", "当前");
    m.insert("pane.list.header.target_id", "目标ID");
    m.insert("pane.list.header.index", "序号");
    m.insert("pane.list.header.number", "编号");
    m.insert("pane.list.header.title", "标题");
    m.insert("pane.list.header.command", "当前命令");
    m.insert("pane.list.header.kind", "类型");
    m.insert("pane.list.header.state", "观测状态");
    m.insert("pane.list.header.cwd", "工作目录");
    m.insert("pane.list.header.launch_identity", "启动身份");
    m.insert("pane.list.header.label", "标签");
    m.insert("pane.list.label.none", "—");
    m.insert(
        "pane.list.target_id_hint",
        "提示：可使用窗格编号或目标ID，例如 `workhive pane send 2 \"...\"` 或 `workhive pane send %2 \"...\"`。",
    );
    // Structured hint block for `pane list`. Each line is a complete
    // sentence with a stable width, and command examples stay on
    // their own line so they are never split mid-token by terminal
    // wrapping. Localized message values stay in Chinese; only the
    // surrounding code comments are English per the language policy.
    m.insert("pane.list.hint.heading", "提示：");
    m.insert(
        "pane.list.hint.active_marker",
        "“当前”列中的 * 表示当前聚焦的窗格。",
    );
    m.insert(
        "pane.list.hint.send_intro",
        "可使用窗格编号（编号列）或目标ID发送命令，例如：",
    );
    m.insert("pane.list.empty", "当前工作区暂无已登记的窗格。");
    m.insert("pane.show.title", "当前窗格");

    // ACP metrics render keys (Activity Intelligence v1).
    // These keys are used by `pane_metrics_view.rs` for `pane list` and
    // `pane show` to surface typed `PaneActivityMetrics` values.
    m.insert("pane.metrics.partial", "~");
    m.insert("pane.metrics.unavailable", "—");
    m.insert("pane.metrics.source.acp", "ACP");
    m.insert("pane.metrics.source.legacy", "兜底");
    m.insert("pane.metrics.source.none", "无");
    m.insert("pane.metrics.label.provider", "AI 提供方");
    m.insert("pane.metrics.label.state", "AI 状态");
    m.insert("pane.metrics.label.source", "AI 数据来源");
    m.insert("pane.metrics.label.confidence", "AI 置信度");
    m.insert("pane.metrics.label.freshness", "AI 数据新鲜度");
    m.insert("pane.metrics.label.elapsed", "当前状态时长");
    m.insert("pane.metrics.label.current_turn", "当前轮次时长");
    m.insert("pane.metrics.label.last_turn", "上次轮次时长");
    m.insert("pane.metrics.label.last_tool", "上次工具时长");
    m.insert("pane.metrics.label.aggregate_tool", "工具总时长");
    m.insert("pane.metrics.label.permission_wait", "等待授权时长");
    m.insert("pane.metrics.label.idle", "空闲时长");
    m.insert("pane.metrics.label.hint", "建议操作");
    m.insert("pane.metrics.block.heading", "AI 指标：");
    m.insert(
        "pane.metrics.hint.permission_needed",
        "等待授权 — 请在 pane 中批准或拒绝",
    );
    m.insert("pane.metrics.hint.completed", "上次轮次已完成");
    m.insert("pane.metrics.hint.failed", "上次轮次失败");
    // Pane supervisor exit-policy diagnostics. Surfaced on stderr
    // when the managed tool cannot start or the replacement shell
    // cannot be executed, so the pane never closes silently.
    m.insert(
        "pane.supervisor.spawn_failed",
        "workhive：启动 '{0}' 失败：{1}",
    );
    m.insert(
        "pane.supervisor.replacement_shell_failed",
        "workhive：无法在 pane 中重新启动 shell：{0}",
    );
    m.insert(
        "pane.supervisor.exit_behavior_untrusted",
        "workhive：pane.exit.tool_exit_behavior 配置不可信（{0}），pane 将关闭以保留退出码可见性",
    );
    // No-hook-event hint sub-block. The body and Codex review line are
    // conservative: they describe the observable fact and a Codex
    // product step; they never assert hooks are uninstalled and never
    // reference the removed `workhive integrations` command.
    m.insert("pane.list.hint.no_hook_event.heading", "完成信号：");
    m.insert(
        "pane.list.hint.no_hook_event.body",
        "  以下 AI pane 暂未记录到完成信号（见 `来源` 列）。AI 工具在面板内完成一次任务后，Workhive 会记录一次信号。",
    );
    m.insert("pane.list.hint.no_hook_event.codex_review", "");
    // Column-legend sub-block. Each line is a complete sentence so
    // terminal wrapping never splits an explanation mid-clause. The
    // legend documents column meaning only; per-row hook approval
    // guidance lives in the separate `no_hook_event` block above.
    m.insert("pane.list.legend.heading", "列说明：");
    m.insert(
        "pane.list.legend.activity_unverified",
        "  “活动状态”列只显示五种状态之一：等待输入、执行中、已完成、已退出、未知。使用 `workhive pane show <编号>` 查看具体来源、可信度与原因。",
    );
    m.insert(
        "pane.list.legend.source",
        "  “来源”列显示活动判断的依据。按准确性排序：完成信号 > 终端状态 > 进程 > tmux > 无。",
    );
    m.insert(
        "pane.list.legend.confidence",
        "  “可信度”列汇总该来源对活动判断的可信程度（已记录的完成信号最高，回退推断较低）。",
    );
    // Concise default-mode hints. Single-line each. Verbose mode is
    // gated separately and re-emits the full Tips / Column legend /
    // hook-event blocks above.
    m.insert(
        "pane.list.default_hint",
        "向窗格发送命令：`workhive pane send <编号或ID> \"...\"`。需查看列说明与详细信息，请加 `--verbose` 或运行 `workhive pane show <pane>`。",
    );
    m.insert(
        "pane.list.default_hint.pending_event",
        "部分 AI pane 暂未记录到完成信号——参见 `来源` 列；可运行 `workhive pane show <pane>` 或加 `--verbose` 查看详情。",
    );
    m.insert("pane.list.default_hint.codex_review", "");

    // Pane observation source / confidence labels surfaced by `pane show`.
    m.insert("pane.observation.source.supervisor", "Workhive 进程监管器");
    m.insert(
        "pane.observation.source.process_observer",
        "操作系统进程观测",
    );
    m.insert("pane.observation.source.tmux", "tmux 兜底");
    m.insert("pane.observation.source.unknown", "无观测");
    m.insert("pane.observation.confidence.authoritative", "权威");
    m.insert("pane.observation.confidence.high", "高");
    m.insert("pane.observation.confidence.fallback", "兜底");
    m.insert("pane.observation.confidence.none", "无");
    m.insert("pane.label.observation_source", "观测来源");
    m.insert("pane.label.observation_confidence", "观测可信度");

    // Intercept
    m.insert(
        "intercept.rerun_hint",
        "全局配置已完成。请重新执行您的命令：\n\n  {0}\n",
    );
    m.insert(
        "intercept.project_init_prompt",
        "项目未初始化。是否现在执行 'workhive init'？",
    );

    // Pane preset labels
    m.insert("pane.preset.claude", "Claude Code (AI 助手)");
    m.insert("pane.preset.codex", "Codex (AI 助手)");
    m.insert("pane.preset.opencode", "OpenCode (AI 助手)");
    // Cursor Agent label normalization: matches the other built-in AI
    // agents so the init wizard does not visually segregate it. The
    // canonical command identity stays `cursor-agent`; this string is
    // display-only.
    m.insert("pane.preset.cursor_agent", "Cursor Agent (AI 助手)");
    m.insert("pane.preset.lazygit", "Lazygit (Git TUI)");
    m.insert("pane.preset.yazi", "Yazi (文件管理器)");
    m.insert("pane.preset.htop", "htop (系统监控)");
    m.insert("pane.preset.bottom", "bottom (系统监控)");
}
