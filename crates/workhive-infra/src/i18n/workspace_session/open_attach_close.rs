//! Open/attach/close/destroy lifecycle, execution summary, workspace remove, and project-init copy.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Open / Attach / Close
    m.insert(
        "open.creating_worktree",
        "Creating worktree for workspace '{0}'...",
    );
    m.insert("open.creating_session", "Creating tmux session '{0}'...");
    m.insert("open.reusing_session", "Reusing existing session '{0}'");
    m.insert("open.success", "Workspace '{0}' is ready.");
    m.insert("open.select_branch", "Select branch / worktree to open");
    m.insert("open.main_branch", "{0}");
    m.insert("open.create_new_wt", "+ New worktree...");
    m.insert("open.group.project_root", "Project root");
    m.insert("open.group.worktree", "Worktree workspaces");
    m.insert("open.group.worktree_empty", "(no worktree workspaces yet)");
    m.insert("open.enter_branch", "Enter branch name for new worktree");
    m.insert(
        "open.confirm_create_worktree",
        "Create a new worktree for workspace '{0}'?",
    );
    m.insert(
        "open.unborn_repo_create_worktree_blocked",
        "Cannot create workspace '{0}' yet: this Git repository has no commits, so there is no base commit to branch from. Create an initial commit first, then run this command again.",
    );
    m.insert(
        "open.unborn_repo_create_initial_commit_prompt",
        "This Git repository has no commits, so workspace '{0}' has no base commit to branch from. Create an empty initial commit and continue opening '{0}'?",
    );
    m.insert(
        "open.unborn_repo_repair_managed_worktree_prompt",
        "A previous failed first-run attempt left a clean managed worktree for '{0}'. Remove it, create an empty initial commit, and continue opening '{0}'?",
    );
    m.insert(
        "open.unborn_repo_managed_worktree_dirty",
        "Cannot repair workspace '{0}' automatically because its existing managed worktree has {1} changed files. Save or remove those changes first, then run this command again.",
    );
    m.insert(
        "open.confirm_attach_existing_branch",
        "Branch '{0}' already exists but is not managed by Workhive. Attach it to a new worktree?",
    );
    m.insert(
        "open.attach_existing_blocked",
        "Branch '{0}' already exists but is not managed by Workhive. Re-run with --attach-existing-branch to attach it, or choose a different workspace name.",
    );
    m.insert(
        "open.branch_attached",
        "Attached existing branch '{0}' to a new Workhive-managed worktree.",
    );
    m.insert("open.worktree_suffix", "(worktree)");
    m.insert("open.invalid_selection", "Invalid selection.");
    m.insert(
        "open.main_dir_mode",
        "Using main branch (project root directory)",
    );
    m.insert("open.applying_layout", "Applying pane layout...");
    m.insert("open.layout_applied", "Layout applied ({0} panes).");
    m.insert(
        "open.initial_pane_not_found",
        "No initial pane found in session. The session may be in an inconsistent state.",
    );

    // Open: execution summary
    m.insert("open.summary.project_init", "- Project: just initialized");
    m.insert(
        "open.summary.project_existing",
        "- Project: already initialized",
    );
    m.insert(
        "open.summary.worktree_created",
        "- Worktree: created (new branch)",
    );
    m.insert(
        "open.summary.worktree_attached",
        "- Worktree: created (existing branch)",
    );
    m.insert(
        "open.summary.worktree_reused",
        "- Worktree: reused existing",
    );
    m.insert(
        "open.summary.worktree_main",
        "- Worktree: project root (main branch)",
    );
    m.insert("open.summary.session_created", "- Session: created ({0})");
    m.insert("open.summary.session_reused", "- Session: reused ({0})");
    m.insert(
        "open.summary.snapshot_restored",
        "- Snapshot: restored ({0} panes)",
    );
    m.insert("open.summary.snapshot_none", "- Snapshot: not found");
    m.insert(
        "open.summary.layout_applied",
        "- Layout: applied ({0} panes)",
    );
    m.insert(
        "open.summary.attach_skipped",
        "- Attach: skipped (--no-attach)",
    );
    m.insert("open.summary.tool_degraded", "  [warn] pane {0}: {1}");
    m.insert(
        "open.warning.runtime_sync_failed",
        "warning: pane registry sync failed: {0}",
    );
    m.insert(
        "open.warning.snapshot_monitor_failed",
        "warning: failed to start snapshot monitor: {0}",
    );
    m.insert(
        "open.summary.degraded_launches_header",
        "Degraded pane launches:",
    );
    m.insert(
        "open.outcome.session_reused",
        "Reused existing live session",
    );
    m.insert(
        "open.outcome.snapshot_missing",
        "No snapshot found; applied layout",
    );
    m.insert(
        "open.outcome.snapshot_failed",
        "Snapshot restore failed: {0}; applied layout",
    );
    m.insert(
        "open.outcome.layout_applied",
        "Applied layout (snapshot skipped)",
    );
    m.insert("open.mode.manual", "Workspace mode: manual");
    m.insert(
        "open.mode.primary_active",
        "Workspace mode: primary-assisted (primary: {0}, workers: {1})",
    );
    m.insert(
        "open.mode.primary_degraded",
        "Workspace mode: primary-assisted requested but unavailable \u{2014} {0}. Operating in manual mode.",
    );
    m.insert(
        "open.mode.primary_failed",
        "Workspace mode: primary-assisted plan failed \u{2014} {0}. Operating in manual mode.",
    );
    m.insert(
        "open.snapshot.fallback_failed",
        "  [fallback] snapshot restore failed ({0}); applying layout instead",
    );
    m.insert(
        "open.snapshot.fallback_not_found",
        "  [fallback] no snapshot found; applying layout",
    );
    m.insert(
        "open.snapshot.force_layout",
        "  [--force-layout] skipping snapshot; applying layout",
    );
    m.insert(
        "open.snapshot.skipped",
        "  [snapshot] snapshot.enabled = false \u{2014} layout applied (snapshot runtime skipped)",
    );

    m.insert(
        "open.outcome.session_reused",
        "  [outcome] existing session reused",
    );
    m.insert(
        "open.outcome.snapshot_missing",
        "  [outcome] no snapshot found; layout applied",
    );
    m.insert(
        "open.outcome.snapshot_failed",
        "  [outcome] snapshot restore failed ({0}); layout applied",
    );
    m.insert(
        "open.outcome.layout_applied",
        "  [outcome] layout applied (snapshot skipped)",
    );

    m.insert("attach.success", "Attached to session '{0}'");
    m.insert(
        "attach.no_sessions",
        "No active sessions found for this project. Use 'workhive workspace open' first.",
    );
    m.insert(
        "attach.select_session",
        "Select a Workhive workspace session to attach",
    );
    m.insert(
        "attach.cannot_extract_workspace",
        "Cannot extract workspace from session '{0}'",
    );
    m.insert(
        "attach.global_select",
        "Select a Workhive workspace session to attach (all projects)",
    );
    m.insert(
        "attach.global_no_index",
        "No project index found. Run 'workhive project reindex --root <path>' to build it.",
    );
    m.insert(
        "attach.global_no_sessions",
        "No active sessions found across registered projects.",
    );
    m.insert("attach.kind.project_root", "Project root");
    m.insert("attach.kind.worktree", "Worktree");
    m.insert("attach.kind.unknown", "Unknown");
    m.insert("attach.group.other", "Other sessions");
    m.insert("attach.marker.attached", "(attached)");
    m.insert("attach.col.session", "Session");
    m.insert("attach.col.workspace", "Workspace");
    m.insert("attach.col.kind", "Kind");
    m.insert("attach.col.path", "Path");
    m.insert("attach.col.attached", "Attached");
    m.insert("attach.col.panes", "Panes");
    m.insert("attach.col.recent", "Last used");
    m.insert("attach.col.project", "Project");
    m.insert("close.select_action", "Close session action");
    m.insert(
        "close.action_detach",
        "Detach — leave session running in background",
    );
    m.insert(
        "close.action_kill",
        "Kill — terminate session and save snapshot",
    );
    m.insert("close.detached", "Detached from session.");
    m.insert("close.killed", "Session killed. Snapshot saved.");
    m.insert("close.killed_no_snapshot", "Session killed.");
    m.insert(
        "close.snapshot_failed",
        "Warning: snapshot failed before kill: {0}",
    );
    m.insert(
        "close.snapshot_skipped",
        "Warning: snapshot skipped (no panes found)",
    );

    // Destroy
    m.insert(
        "destroy.success",
        "All workhive resources for the current project have been destroyed.",
    );
    m.insert(
        "destroy.warning",
        "WARNING: This destroys ALL workhive resources for the current project. This action is irreversible.",
    );
    m.insert(
        "destroy.confirm_delete_branch",
        "Also delete the local branch '{0}'? Workhive did not create it.",
    );
    m.insert("destroy.branch_deleted", "Deleted local branch '{0}'.");
    m.insert("destroy.branch_kept", "Branch '{0}' was kept: {1}");
    m.insert("destroy.safety_report_heading", "Destroy safety report:");
    m.insert(
        "destroy.safety_no_risk",
        "No modified workspace files or unmerged commit risk detected.",
    );
    m.insert(
        "destroy.risky_worktrees_confirm",
        "Workspace data-loss risks detected. Destroy ALL workhive resources for the current project anyway? This is irreversible.",
    );
    m.insert(
        "destroy.force_despite_risk",
        "--force is set, proceeding despite workspace risks.",
    );
    m.insert(
        "destroy.not_initialized",
        "No workhive state found in this project (`.workhive/` is missing). \
         There is nothing to destroy. Run `workhive init` first if you want to manage this project with workhive.",
    );
    m.insert(
        "destroy.partial_init",
        "This project has a partially-initialized `.workhive/` state ({0}). \
         `workspace destroy` will not silently rewrite or repair it. \
         Run `workhive init` to complete initialization, or remove `.workhive/` manually if it is stale.",
    );
    m.insert(
        "safety_report.changed_files",
        "changed files: {0} (staged {1}, unstaged {2}, untracked {3})",
    );
    m.insert(
        "safety_report.more_changed_files",
        "... and {0} more changed file(s)",
    );
    m.insert(
        "safety_report.unmerged_commits",
        "unmerged commits against {0}: {1}",
    );
    m.insert(
        "safety_report.danger_headline",
        "DANGER: workspaces about to be removed contain unsaved or unmerged work.",
    );
    m.insert(
        "safety_report.unmerged_commits_explanation",
        "This branch has {0} commit(s) reachable from it but not from `{1}`. \
         Before removing this workspace, make sure those commits are pushed, \
         merged, or no longer needed.",
    );
    m.insert(
        "safety_report.clean_worktree_diverged_branch",
        "No uncommitted file changes were detected, but this branch still has \
         {0} commit(s) not included in `{1}`. Removing this workspace will \
         delete its local worktree; the commits remain in Git only if the \
         branch is preserved.",
    );
    // Workspace remove (user-facing wording)
    m.insert(
        "workspace.remove.confirm",
        "Remove workspace '{0}' and close its session?",
    );
    m.insert(
        "workspace.remove.success",
        "Workspace '{0}' removed (session removed: {1}, branch deleted: {2}).",
    );
    m.insert(
        "workspace.remove.branch_kept",
        "Workspace '{0}' was removed, but branch '{1}' was kept: {2}",
    );
    m.insert(
        "workspace.remove.confirm_delete_branch",
        "Also delete the local branch '{0}'? Workhive did not create it.",
    );
    m.insert(
        "workspace.remove.main_forbidden",
        "Cannot remove the main project workspace.",
    );
    m.insert(
        "workspace.remove.not_managed",
        "Workspace '{0}' is not managed by Workhive.",
    );
    m.insert(
        "workspace.remove.safety_blocked",
        "Risky workspace removal requires explicit confirmation or --force.",
    );
    m.insert(
        "workspace.remove.safety_report_heading",
        "Workspace remove safety report:",
    );
    m.insert(
        "workspace.remove.risky_confirm",
        "Risky workspace state detected. Continue removal with data-loss risk?",
    );
    m.insert(
        "open.summary.supervisor_degraded",
        "Note: pane runtime telemetry failed to start: {0}. \
         The workspace is usable for navigation, but `workhive pane send` will return a \
         localized error until the workspace is reopened.",
    );

    // Open: project init
    m.insert(
        "open.project_init_prompt",
        "Project is not initialized. Initialize now?",
    );
    m.insert(
        "open.project_init_denied",
        "Project not initialized — use 'workhive init' or pass '--init' to auto-initialize.",
    );
    m.insert(
        "open.project_init_declined",
        "Project initialization declined — run 'workhive init' to initialize manually.",
    );
    m.insert(
        "open.project_partial_init",
        "Project at '{0}' is partially initialized (dir={1}, instance_id={2}, config={3}) — run 'workhive init' to complete setup or remove '.workhive/' to start fresh.",
    );

    // workspace destroy preflight short-circuit
    m.insert(
        "destroy.no_workhive_state",
        "No Workhive project state at '{0}': nothing to destroy. Run `workhive init` to initialize a project.",
    );
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // Open / Attach / Close
    m.insert(
        "open.creating_worktree",
        "正在为工作区 '{0}' 创建 worktree...",
    );
    m.insert("open.creating_session", "正在创建 tmux 会话 '{0}'...");
    m.insert("open.reusing_session", "复用已有会话 '{0}'");
    m.insert("open.success", "工作区 '{0}' 已就绪。");
    m.insert("open.select_branch", "选择要打开的分支 / worktree");
    m.insert("open.main_branch", "{0}");
    m.insert("open.create_new_wt", "+ 新建 worktree...");
    m.insert("open.group.project_root", "项目根目录");
    m.insert("open.group.worktree", "Worktree 工作区");
    m.insert("open.group.worktree_empty", "（暂无 worktree 工作区）");
    m.insert("open.enter_branch", "输入新 worktree 的分支名");
    m.insert(
        "open.confirm_create_worktree",
        "要为工作区 '{0}' 创建新的 worktree 吗？",
    );
    m.insert(
        "open.unborn_repo_create_worktree_blocked",
        "暂时无法创建工作区 '{0}'：当前 Git 仓库还没有任何提交，没有可用于派生分支的基准提交。请先创建一次初始提交，然后重新运行该命令。",
    );
    m.insert(
        "open.unborn_repo_create_initial_commit_prompt",
        "当前 Git 仓库还没有任何提交，工作区 '{0}' 没有可用于派生分支的基准提交。是否创建一次空的初始提交并继续打开 '{0}'？",
    );
    m.insert(
        "open.unborn_repo_repair_managed_worktree_prompt",
        "之前的首次运行尝试留下了工作区 '{0}' 的干净托管 worktree。是否移除它、创建一次空的初始提交，并继续打开 '{0}'？",
    );
    m.insert(
        "open.unborn_repo_managed_worktree_dirty",
        "无法自动修复工作区 '{0}'：现有托管 worktree 中有 {1} 个已改动文件。请先保存或移除这些改动，然后重新运行该命令。",
    );
    m.insert(
        "open.confirm_attach_existing_branch",
        "分支 '{0}' 已存在，但不受 Workhive 管理。是否将其挂载到新的 worktree？",
    );
    m.insert(
        "open.attach_existing_blocked",
        "分支 '{0}' 已存在，但不受 Workhive 管理。请使用 --attach-existing-branch 重新运行以挂载该分支，或改用其他工作区名称。",
    );
    m.insert(
        "open.branch_attached",
        "已将已有分支 '{0}' 挂载到新的 Workhive 管理 worktree。",
    );
    m.insert("open.worktree_suffix", "（worktree）");
    m.insert("open.invalid_selection", "无效的选择。");
    m.insert("open.main_dir_mode", "使用主分支（项目根目录）");
    m.insert("open.applying_layout", "正在应用窗格布局...");
    m.insert("open.layout_applied", "布局已应用（{0} 个窗格）。");
    m.insert(
        "open.initial_pane_not_found",
        "会话中未找到初始窗格。会话可能处于不一致状态。",
    );

    // Open: execution summary
    m.insert("open.summary.project_init", "- 项目: 刚刚初始化");
    m.insert("open.summary.project_existing", "- 项目: 已初始化");
    m.insert(
        "open.summary.worktree_created",
        "- Worktree: 新建（新分支）",
    );
    m.insert(
        "open.summary.worktree_attached",
        "- Worktree: 新建（已有分支）",
    );
    m.insert("open.summary.worktree_reused", "- Worktree: 复用已有");
    m.insert(
        "open.summary.worktree_main",
        "- Worktree: 项目根目录（主分支）",
    );
    m.insert("open.summary.session_created", "- 会话: 新建 ({0})");
    m.insert("open.summary.session_reused", "- 会话: 复用 ({0})");
    m.insert(
        "open.summary.snapshot_restored",
        "- 快照: 已恢复 ({0} 个窗格)",
    );
    m.insert("open.summary.snapshot_none", "- 快照: 未找到");
    m.insert("open.summary.layout_applied", "- 布局: 已应用 ({0} 个窗格)");
    m.insert(
        "open.summary.attach_skipped",
        "- 连接: 已跳过 (--no-attach)",
    );
    m.insert("open.summary.tool_degraded", "  [警告] 窗格 {0}: {1}");
    m.insert(
        "open.warning.runtime_sync_failed",
        "警告：窗格注册表同步失败：{0}",
    );
    m.insert(
        "open.warning.snapshot_monitor_failed",
        "警告：启动快照监控进程失败：{0}",
    );
    m.insert("open.summary.degraded_launches_header", "降级启动的窗格：");
    m.insert("open.outcome.session_reused", "复用了已有的活跃会话");
    m.insert("open.outcome.snapshot_missing", "未找到快照；已应用布局");
    m.insert(
        "open.outcome.snapshot_failed",
        "快照恢复失败：{0}；已应用布局",
    );
    m.insert("open.outcome.layout_applied", "已应用布局（跳过快照）");
    m.insert("open.mode.manual", "工作区模式：手动");
    m.insert(
        "open.mode.primary_active",
        "工作区模式：主窗格协调（主窗格：{0}，工作窗格：{1}）",
    );
    m.insert(
        "open.mode.primary_degraded",
        "工作区模式：主窗格协调已请求但不可用 \u{2014} {0}。已退回手动模式。",
    );
    m.insert(
        "open.mode.primary_failed",
        "工作区模式：主窗格协调计划失败 \u{2014} {0}。已退回手动模式。",
    );
    m.insert(
        "open.snapshot.fallback_failed",
        "  [回退] 快照恢复失败 ({0})；改为应用布局",
    );
    m.insert(
        "open.snapshot.fallback_not_found",
        "  [回退] 未找到快照；应用布局",
    );
    m.insert(
        "open.snapshot.force_layout",
        "  [--force-layout] 跳过快照；应用布局",
    );
    m.insert(
        "open.snapshot.skipped",
        "  [快照] snapshot.enabled = false \u{2014} 已应用布局（跳过快照）",
    );

    m.insert("open.outcome.session_reused", "  [结果] 复用已有会话");
    m.insert(
        "open.outcome.snapshot_missing",
        "  [结果] 未找到快照；已应用布局",
    );
    m.insert(
        "open.outcome.snapshot_failed",
        "  [结果] 快照恢复失败 ({0})；已应用布局",
    );
    m.insert(
        "open.outcome.layout_applied",
        "  [结果] 已应用布局（跳过快照）",
    );

    m.insert("attach.success", "已连接到会话 '{0}'");
    m.insert(
        "attach.no_sessions",
        "当前项目没有活跃会话，请先使用 'workhive workspace open' 创建。",
    );
    m.insert("attach.select_session", "选择要连接的 Workhive 工作区会话");
    m.insert(
        "attach.cannot_extract_workspace",
        "无法从会话 '{0}' 中提取工作区名",
    );
    m.insert(
        "attach.global_select",
        "选择要连接的 Workhive 工作区会话（所有项目）",
    );
    m.insert(
        "attach.global_no_index",
        "未找到项目索引。请先运行 'workhive project reindex --root <路径>' 建立索引。",
    );
    m.insert(
        "attach.global_no_sessions",
        "所有已注册项目中均无活跃会话。",
    );
    m.insert("attach.kind.project_root", "项目根目录");
    m.insert("attach.kind.worktree", "Worktree");
    m.insert("attach.kind.unknown", "未知");
    m.insert("attach.group.other", "其它会话");
    m.insert("attach.marker.attached", "（已连接）");
    m.insert("attach.col.session", "会话");
    m.insert("attach.col.workspace", "工作区名");
    m.insert("attach.col.kind", "类型");
    m.insert("attach.col.path", "路径");
    m.insert("attach.col.attached", "连接");
    m.insert("attach.col.panes", "窗格");
    m.insert("attach.col.recent", "最后使用");
    m.insert("attach.col.project", "项目");
    m.insert("close.select_action", "选择关闭方式");
    m.insert("close.action_detach", "分离 — 会话保留在后台运行");
    m.insert("close.action_kill", "终止 — 关闭会话并保存快照");
    m.insert("close.detached", "已从会话分离。");
    m.insert("close.killed", "会话已终止。快照已保存。");
    m.insert("close.killed_no_snapshot", "会话已终止。");
    m.insert("close.snapshot_failed", "警告：终止前快照失败：{0}");
    m.insert("close.snapshot_skipped", "警告：快照已跳过（未发现窗格）");

    // Destroy
    m.insert("destroy.success", "当前项目下的所有 workhive 资源已清除。");
    m.insert(
        "destroy.warning",
        "警告：此操作将销毁当前项目下的全部 workhive 资源，且不可撤销。",
    );
    m.insert(
        "destroy.confirm_delete_branch",
        "是否同时删除本地分支 '{0}'？该分支不是由 Workhive 创建的。",
    );
    m.insert("destroy.branch_deleted", "已删除本地分支 '{0}'。");
    m.insert("destroy.branch_kept", "分支 '{0}' 已保留：{1}");
    m.insert("destroy.safety_report_heading", "销毁安全报告：");
    m.insert(
        "destroy.safety_no_risk",
        "未检测到工作区文件变更或未合并提交风险。",
    );
    m.insert(
        "destroy.risky_worktrees_confirm",
        "检测到工作区数据丢失风险。是否仍销毁当前项目下的所有 workhive 资源？此操作不可撤销。",
    );
    m.insert(
        "destroy.force_despite_risk",
        "已设置 --force，忽略工作区风险继续执行。",
    );
    m.insert(
        "destroy.not_initialized",
        "当前项目未发现 workhive 状态（缺少 `.workhive/`）。\
         没有可销毁的资源。如需用 workhive 管理本项目，请先执行 `workhive init`。",
    );
    m.insert(
        "destroy.partial_init",
        "当前项目的 `.workhive/` 状态不完整（{0}）。\
         `workspace destroy` 不会自动重写或修复该状态。\
         请运行 `workhive init` 完成初始化，或手动删除已陈旧的 `.workhive/`。",
    );
    m.insert(
        "safety_report.changed_files",
        "变更文件：{0}（已暂存 {1}，未暂存 {2}，未跟踪 {3}）",
    );
    m.insert(
        "safety_report.more_changed_files",
        "... 另有 {0} 个变更文件",
    );
    m.insert(
        "safety_report.unmerged_commits",
        "相对于 {0} 的未合并提交：{1}",
    );
    m.insert(
        "safety_report.danger_headline",
        "危险：即将被删除的工作区中存在未保存或未合并的工作。",
    );
    m.insert(
        "safety_report.unmerged_commits_explanation",
        "该分支包含 {0} 个仅存在于本分支、尚未合并入 `{1}` 的提交。\
         删除该工作区前，请确认这些提交已推送、已合并或不再需要。",
    );
    m.insert(
        "safety_report.clean_worktree_diverged_branch",
        "未检测到未提交的文件变更，但该分支仍有 {0} 个尚未合并入 `{1}` 的提交。\
         删除该工作区将清除其本地工作树；只有当分支被保留时，这些提交才会留在 Git 中。",
    );
    // Workspace remove (user-facing wording)
    m.insert(
        "workspace.remove.confirm",
        "是否移除工作区 '{0}' 并关闭其会话？",
    );
    m.insert(
        "workspace.remove.success",
        "工作区 '{0}' 已移除（会话已移除：{1}，分支已删除：{2}）。",
    );
    m.insert(
        "workspace.remove.branch_kept",
        "工作区 '{0}' 已移除，但分支 '{1}' 已保留：{2}",
    );
    m.insert(
        "workspace.remove.confirm_delete_branch",
        "是否同时删除本地分支 '{0}'？该分支不是由 Workhive 创建的。",
    );
    m.insert("workspace.remove.main_forbidden", "无法移除项目主工作区。");
    m.insert(
        "workspace.remove.not_managed",
        "工作区 '{0}' 不受 Workhive 管理。",
    );
    m.insert(
        "workspace.remove.safety_blocked",
        "存在风险的工作区移除需要显式确认或 --force。",
    );
    m.insert(
        "workspace.remove.safety_report_heading",
        "工作区移除安全报告：",
    );
    m.insert(
        "workspace.remove.risky_confirm",
        "检测到工作区存在风险。是否继续移除（可能导致数据丢失）？",
    );
    m.insert(
        "open.summary.supervisor_degraded",
        "提示：窗格运行时遥测启动失败：{0}。\
         工作区仍可用于浏览，但在工作区被重新打开之前，`workhive pane send` 会返回本地化错误。",
    );

    // Open: project init
    m.insert(
        "open.project_init_prompt",
        "项目还未初始化，是否现在进行初始化？",
    );
    m.insert(
        "open.project_init_denied",
        "项目未初始化 — 请使用 'workhive init' 或传入 '--init' 参数进行自动初始化。",
    );
    m.insert(
        "open.project_init_declined",
        "项目初始化已取消 — 请运行 'workhive init' 手动初始化。",
    );
    m.insert(
        "open.project_partial_init",
        "项目 '{0}' 处于不完整的初始化状态 (dir={1}, instance_id={2}, config={3}) — 请运行 'workhive init' 完成初始化，或删除 '.workhive/' 目录重新开始。",
    );

    // workspace destroy preflight short-circuit
    m.insert(
        "destroy.no_workhive_state",
        "项目 '{0}' 未发现 Workhive 状态：无需销毁。如需创建项目，请运行 `workhive init`。",
    );
}
