//! Workspace/session listing, sessions, worktree command group, and layout labels.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // List
    m.insert("list.header", "Workspaces:");
    m.insert("list.backend", "Runtime backend: {0}");
    m.insert(
        "list.empty",
        "No workspaces found. Run 'workhive workspace open <name>' to create one.",
    );
    m.insert(
        "list.uninitialized_hint",
        "\nThis project is not yet initialized.\n  - Run: workhive init\n  - Or:  workhive workspace open <workspace> --init",
    );

    m.insert("list.col.workspace", "Workspace");
    m.insert("list.col.status", "Status");
    m.insert("list.col.path", "Path");
    m.insert("list.col.session", "Session");
    m.insert("list.col.worktree_exists", "Worktree");
    m.insert("list.col.updated_at", "Updated");
    m.insert("list.col.units", "Panes");
    m.insert("list.col.layout", "Layout");
    m.insert("list.col.recent", "Last used");
    m.insert("list.no_record", "no record");
    m.insert("list.last_used.today", "Today {0}");
    m.insert("list.last_used.yesterday", "Yesterday {0}");
    m.insert("list.main_label", "(main)");
    m.insert("list.layout.not_configured", "not started");
    m.insert("list.layout.pane_count", "{0} panes");
    m.insert("list.layout.lookup_failed", "lookup failed");
    m.insert("list.configured_layout", "Configured layout: {0}");
    m.insert(
        "list.overview.summary",
        "Total: {0} workspaces \u{b7} {1} running",
    );
    m.insert(
        "list.overview.uninitialized",
        "Project not initialized \u{b7} run `workhive init` to start",
    );
    m.insert("list.projects_header", "Registered projects:");
    m.insert("list.col.project", "Project");
    m.insert("list.col.root", "Root");
    m.insert("layout.ai_coding_left", "AI Coding Left \u{b7} 3 panes");
    m.insert("layout.ai_coding_dual", "Dual AI Coding \u{b7} 3 panes");
    m.insert("layout.half_split", "Half Split \u{b7} 2 panes");
    m.insert("layout.quad", "Quad Grid \u{b7} 4 panes");
    m.insert("layout.single", "Single \u{b7} 1 pane");
    m.insert("layout.custom", "Custom");

    // Sessions
    m.insert("sessions.header", "Active sessions:");
    m.insert("sessions.empty", "No active sessions found.");
    m.insert(
        "sessions.project_unavailable",
        "Project '{0}' runtime backend unavailable, skipped: {1}",
    );
    m.insert(
        "sessions.no_index",
        "No global project index found. Configure trusted roots with 'workhive project roots add <path>' (or 'workhive init'), then run 'workhive project reindex'.",
    );
    m.insert("sessions.col.project", "Project");
    m.insert("sessions.col.backend", "Backend");
    m.insert("sessions.col.workspace", "Workspace");
    m.insert("sessions.col.session", "Session");
    m.insert("sessions.col.attached", "Attached");
    m.insert("sessions.col.path", "Path");
    m.insert("sessions.col.client", "Client");
    m.insert("sessions.attached.yes", "yes");
    m.insert("sessions.attached.no", "no");

    // Session attach (global)
    m.insert(
        "session.attach.no_index",
        "No global project index found. Run 'workhive project reindex --root <path>' first.",
    );
    m.insert(
        "session.attach.no_sessions",
        "No active workhive sessions found across known projects.",
    );
    m.insert(
        "session.attach.ambiguous",
        "Workspace name '{0}' is ambiguous across multiple projects. Specify 'project/workspace' or the full session name.",
    );
    m.insert(
        "session.attach.not_found",
        "No session matched '{0}'. Use 'workhive session list' to see available sessions.",
    );
    m.insert("session.attach.select", "Select a session to attach:");
    m.insert("session.attach.col.target", "Session");
    m.insert("session.attach.col.details", "Details");
    m.insert("session.attach.herdr_details", "{0} workspace(s)");
    m.insert(
        "session.attach.no_tty",
        "No session identifier supplied and stdin is not a terminal. Pass <session>, <project/workspace>, or <workspace>.",
    );
    m.insert(
        "session.attach.hint_global",
        "Tip: 'workhive session attach' resolves sessions globally without a project context.",
    );

    // Worktree command group
    m.insert("worktree.list_header", "Git worktrees:");
    m.insert("worktree.list_empty", "No worktrees found.");
    m.insert("worktree.col.name", "Name");
    m.insert("worktree.col.branch", "Branch");
    m.insert("worktree.col.path", "Path");
    m.insert("worktree.col.kind", "Kind");
    m.insert("worktree.kind.main", "main");
    m.insert("worktree.kind.linked", "linked");
    m.insert(
        "worktree.remove.no_tty",
        "Worktree name required when stdin is not a terminal. Pass `workhive workspace worktree remove <name>`.",
    );
    m.insert(
        "worktree.remove.none_available",
        "No Workhive-managed linked worktrees found under .workhive/worktrees/.",
    );
    m.insert("worktree.remove.select", "Select a worktree to remove:");
    m.insert(
        "worktree.remove.confirm",
        "Remove worktree '{0}' at '{1}'? This cannot be undone.",
    );
    m.insert("worktree.remove.success", "Removed worktree '{0}'.");
    m.insert(
        "worktree.remove.not_found",
        "Worktree '{0}' is not managed by Workhive (must live under .workhive/worktrees/).",
    );
    m.insert(
        "worktree.remove.blocked_risky",
        "Worktree '{0}' has dirty/unmerged work. Pass `--force` to remove anyway.",
    );
    m.insert(
        "worktree.remove.not_managed",
        "Worktree '{0}' at '{1}' is not under Workhive's managed base ('{2}'). Refusing.",
    );
    m.insert(
        "worktree.remove.branch_deleted",
        "Branch '{0}' was deleted along with the worktree.",
    );
    m.insert(
        "worktree.remove.branch_delete_failed",
        "Worktree was removed, but failed to delete branch '{0}': {1}.",
    );

    // List: session availability — the four values the `Session` column
    // distinguishes. These describe session presence only, never pane or
    // doctor/validation status.
    m.insert("list.session.running", "running");
    m.insert("list.session.missing", "no session");
    m.insert("list.session.unreachable", "unreachable");
    m.insert("list.session.lookup_failed", "lookup failed");
    m.insert("list.col.session_state", "Session");
    // List: human-output group headers (project root vs linked worktrees).
    m.insert("list.group.project_root", "Project root");
    m.insert("list.group.worktree", "Worktree workspaces");
    m.insert("list.group.worktree_empty", "(no worktree workspaces yet)");
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // List
    m.insert("list.header", "工作区列表：");
    m.insert("list.backend", "运行时后端：{0}");
    m.insert(
        "list.empty",
        "未找到工作区。运行 'workhive workspace open <名称>' 创建一个。",
    );
    m.insert(
        "list.uninitialized_hint",
        "\n当前项目尚未初始化。\n  - 运行: workhive init\n  - 或者: workhive workspace open <工作区名> --init",
    );

    m.insert("list.col.workspace", "工作区");
    m.insert("list.col.status", "状态");
    m.insert("list.col.path", "路径");
    m.insert("list.col.session", "会话");
    m.insert("list.col.worktree_exists", "Worktree");
    m.insert("list.col.updated_at", "更新时间");
    m.insert("list.col.units", "窗格数");
    m.insert("list.col.layout", "布局");
    m.insert("list.col.recent", "最后使用");
    m.insert("list.no_record", "无记录");
    m.insert("list.last_used.today", "今天 {0}");
    m.insert("list.last_used.yesterday", "昨天 {0}");
    m.insert("list.main_label", "（主分支）");
    m.insert("list.layout.not_configured", "未启动");
    m.insert("list.layout.pane_count", "{0} 个窗格");
    m.insert("list.layout.lookup_failed", "查询失败");
    m.insert("list.configured_layout", "配置布局：{0}");
    m.insert(
        "list.overview.summary",
        "总计：{0} 个工作区 \u{b7} {1} 个运行中",
    );
    m.insert(
        "list.overview.uninitialized",
        "项目未初始化 \u{b7} 请先运行 `workhive init`",
    );
    m.insert("list.projects_header", "已注册项目：");
    m.insert("list.col.project", "项目");
    m.insert("list.col.root", "路径");
    m.insert("layout.ai_coding_left", "AI 编程 左侧 \u{b7} 3 窗格");
    m.insert("layout.ai_coding_dual", "双 AI 编程 \u{b7} 3 窗格");
    m.insert("layout.half_split", "左右分屏 \u{b7} 2 窗格");
    m.insert("layout.quad", "四宫格 \u{b7} 4 窗格");
    m.insert("layout.single", "单窗格 \u{b7} 1 窗格");
    m.insert("layout.custom", "自定义");

    // Sessions
    m.insert("sessions.header", "活跃会话：");
    m.insert("sessions.empty", "暂无活跃会话。");
    m.insert(
        "sessions.project_unavailable",
        "项目 '{0}' 的运行时后端不可用，已跳过：{1}",
    );
    m.insert(
        "sessions.no_index",
        "未找到全局项目索引。请先运行 'workhive project roots add <路径>'（或 'workhive init'）配置可信项目目录，然后运行 'workhive project reindex'。",
    );
    m.insert("sessions.col.project", "项目");
    m.insert("sessions.col.backend", "后端");
    m.insert("sessions.col.workspace", "工作区");
    m.insert("sessions.col.session", "会话");
    m.insert("sessions.col.attached", "客户端");
    m.insert("sessions.col.path", "路径");
    m.insert("sessions.col.client", "客户端");
    m.insert("sessions.attached.yes", "有");
    m.insert("sessions.attached.no", "无");

    // Session attach (global)
    m.insert(
        "session.attach.no_index",
        "未找到全局项目索引。请先运行 'workhive project reindex --root <path>' 构建索引。",
    );
    m.insert(
        "session.attach.no_sessions",
        "在已知项目范围内未找到任何活跃的 workhive 会话。",
    );
    m.insert(
        "session.attach.ambiguous",
        "工作区名 '{0}' 在多个项目中重名。请改用 'project/workspace' 或完整会话名指定会话。",
    );
    m.insert(
        "session.attach.not_found",
        "未匹配到会话 '{0}'。请使用 'workhive session list' 查看可用会话。",
    );
    m.insert("session.attach.select", "请选择要连接的会话：");
    m.insert("session.attach.col.target", "会话");
    m.insert("session.attach.col.details", "详情");
    m.insert("session.attach.herdr_details", "{0} 个工作区");
    m.insert(
        "session.attach.no_tty",
        "未提供会话标识且当前不在终端环境，无法进入交互选择。请显式传入 <session>、<project/workspace> 或 <workspace>。",
    );
    m.insert(
        "session.attach.hint_global",
        "提示：'workhive session attach' 是全局操作，不依赖项目上下文。",
    );

    // Worktree command group
    m.insert("worktree.list_header", "Workhive 关联工作树：");
    m.insert(
        "worktree.list_empty",
        "未发现 Workhive 管理的关联工作树（路径需位于 .workhive/worktrees/ 下）。",
    );
    m.insert("worktree.col.name", "名称");
    m.insert("worktree.col.branch", "分支");
    m.insert("worktree.col.path", "路径");
    m.insert("worktree.col.kind", "类型");
    m.insert("worktree.kind.main", "主仓");
    m.insert("worktree.kind.linked", "关联");
    m.insert(
        "worktree.remove.no_tty",
        "当前不在终端环境，无法交互选择。请显式执行 `workhive workspace worktree remove <名称>`。",
    );
    m.insert(
        "worktree.remove.none_available",
        "未发现 Workhive 管理的关联工作树（路径需位于 .workhive/worktrees/ 下）。",
    );
    m.insert("worktree.remove.select", "请选择要移除的工作树：");
    m.insert(
        "worktree.remove.confirm",
        "确认移除工作树 '{0}'（路径：{1}）？此操作不可撤销。",
    );
    m.insert("worktree.remove.success", "已移除工作树 '{0}'。");
    m.insert(
        "worktree.remove.not_found",
        "工作树 '{0}' 不在 Workhive 管理范围内（必须位于 .workhive/worktrees/ 下）。",
    );
    m.insert(
        "worktree.remove.blocked_risky",
        "工作树 '{0}' 存在未提交或未合并的更改。如需强行移除请加上 `--force`。",
    );
    m.insert(
        "worktree.remove.not_managed",
        "工作树 '{0}' 路径 '{1}' 不在 Workhive 管理基目录 '{2}' 下，已拒绝移除。",
    );
    m.insert(
        "worktree.remove.branch_deleted",
        "已随工作树一并删除分支 '{0}'。",
    );
    m.insert(
        "worktree.remove.branch_delete_failed",
        "工作树已移除，但删除分支 '{0}' 失败：{1}。",
    );

    // List: session availability — the four values the `Session` column
    // distinguishes. These describe session presence only, never pane or
    // doctor/validation status.
    m.insert("list.session.running", "运行中");
    m.insert("list.session.missing", "无会话");
    m.insert("list.session.unreachable", "不可达");
    m.insert("list.session.lookup_failed", "查询失败");
    m.insert("list.col.session_state", "会话状态");
    // List: human-output group headers (project root vs linked worktrees).
    m.insert("list.group.project_root", "项目根目录");
    m.insert("list.group.worktree", "Worktree 工作区");
    m.insert("list.group.worktree_empty", "（暂无 Worktree 工作区）");
}
