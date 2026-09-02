//! Project, configuration, trusted-roots discovery, reindex, dependency, and git-init copy.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Config
    m.insert("config.show_header", "Current configuration:");
    m.insert("config.source_line", "Config source: {0} ({1})");
    m.insert("config.edit_ask", "Modify this configuration?");
    m.insert("config.edit_success", "Configuration updated.");
    m.insert("config.edit_target_line", "Edit target ({0}): {1}");
    m.insert(
        "config.edit_will_create_project_config",
        "No project config exists yet. Continuing will create a new project override at {0}.",
    );
    m.insert("config.edit_confirm_target", "Modify {0} config at {1}?");
    m.insert("config.set_preview", "Will set {0} = {1}");
    m.insert("config.set_success", "Configuration updated.");
    m.insert("config.unset_success", "Key removed from configuration.");
    m.insert("config.env_set_preview", "Will set env {0} = {1}");
    m.insert("config.env_set_success", "Environment variable updated.");
    m.insert("config.env_unset_preview", "Will remove env variable {0}");
    m.insert("config.env_unset_success", "Environment variable removed.");

    // Trusted-roots discovery setup-state hints (English).
    m.insert(
        "trusted_roots.none_configured",
        "No trusted project directories configured.\n  - Add one: workhive project roots add <path>\n  - Trusted roots gate where Workhive may create .workhive/, worktrees, and runtime state, and feed project discovery.",
    );
    m.insert(
        "trusted_roots.index_missing",
        "Trusted project directories are configured but the project index has not been built yet.\n  - Run: workhive project reindex",
    );
    m.insert(
        "trusted_roots.index_empty",
        "Project index is empty. No initialized projects were found under the trusted roots.\n  - Initialize one: cd into a trusted directory and run 'workhive init'.",
    );
    m.insert(
        "trusted_roots.outside",
        "Refused to mutate '{0}': not under any trusted project root.\n  - Configured trusted roots:\n{1}\n  - Add it: workhive project roots add {0}\n  - Or work inside an existing trusted root.\n  - Inspect: workhive project roots list",
    );
    // Listed when no trusted roots are configured at all, so the
    // rejection copy can stay self-explanatory without a separate variant.
    m.insert(
        "trusted_roots.outside.empty_list",
        "      (none configured)",
    );
    // Format used to render each configured trusted root inside the
    // `trusted_roots.outside` rejection. Keeps the bullet style centralized
    // so future copy changes don't drift between locales.
    m.insert("trusted_roots.outside.line_item", "      - {0}");
    m.insert("trusted_roots.list_header", "Trusted project roots:");
    m.insert(
        "trusted_roots.list_empty",
        "No trusted project roots configured. Add one with: workhive project roots add <path>",
    );
    m.insert("trusted_roots.added", "Added trusted project root: {0}");
    m.insert(
        "trusted_roots.already_present",
        "Already configured as a trusted project root: {0}",
    );
    m.insert("trusted_roots.removed", "Removed trusted project root: {0}");
    m.insert(
        "trusted_roots.not_present",
        "Not in the trusted project roots list: {0}",
    );
    m.insert(
        "trusted_roots.status_configured",
        "Trusted project roots configured: {0}",
    );
    m.insert("trusted_roots.status_index", "Project index: {0}");
    m.insert("trusted_roots.status_projects", "Indexed projects: {0}");
    m.insert(
        "trusted_roots.rejected_too_broad",
        "Refusing to add overly broad path '{0}'. Use a project parent directory like ~/development or ~/work.",
    );
    m.insert(
        "trusted_roots.covered_by_existing_root",
        "Path '{0}' is already covered by trusted root '{1}'. Workhive trusts the entire subtree, so a redundant child entry is not added.",
    );
    m.insert(
        "trusted_roots.config_unreadable",
        "Global config could not be read ({0}); trusted-roots hints are degraded. Run `workhive doctor` for details.",
    );
    m.insert(
        "trusted_roots.edit_affected_header",
        "These trusted-root removals will affect {0} indexed project(s):",
    );
    m.insert(
        "trusted_roots.edit_affected_line",
        "  - {0} ({1} project(s))",
    );
    m.insert(
        "trusted_roots.edit_affected_prompt",
        "Save changes anyway? Removing the root does not delete the project files or `.workhive/`.",
    );
    m.insert(
        "trusted_roots.edit_affected_requires_force",
        "Refusing to remove trusted roots that affect indexed projects in non-interactive mode. Re-run `workhive project roots edit` interactively (TTY) or use `workhive project roots remove <path> --force` for individual removals.",
    );
    m.insert(
        "trusted_roots.path_missing",
        "Path '{0}' does not exist. Provide an existing directory.",
    );
    m.insert(
        "trusted_roots.path_not_directory",
        "Path '{0}' is not a directory. Trusted project roots must be directories.",
    );
    m.insert(
        "trusted_roots.remove_affected_prompt",
        "Removing '{0}' will exclude {1} indexed project(s) from the trusted-roots safety policy. The projects and their .workhive/ state are NOT deleted. Continue?",
    );
    m.insert(
        "trusted_roots.remove_affected_requires_force",
        "Removing '{0}' affects {1} indexed project(s). Re-run with --force to confirm in non-interactive mode.",
    );
    m.insert(
        "trusted_roots.remove_aborted",
        "Removal cancelled. Trusted project roots unchanged.",
    );
    m.insert(
        "trusted_roots.edit_header",
        "Editing trusted project roots. Use the menu to add/remove entries; nothing is written until you choose Save.",
    );
    m.insert(
        "trusted_roots.edit_no_tty",
        "Interactive edit requires a TTY. Use 'workhive project roots add/remove' for non-interactive changes.",
    );
    m.insert("trusted_roots.edit_menu_prompt", "Choose an action:");
    m.insert("trusted_roots.edit_menu_add", "Add a trusted root");
    m.insert("trusted_roots.edit_menu_remove", "Remove a trusted root");
    m.insert("trusted_roots.edit_menu_save", "Save and exit");
    m.insert("trusted_roots.edit_menu_cancel", "Cancel without saving");
    m.insert(
        "trusted_roots.edit_add_prompt",
        "Path to add (absolute or ~/...):",
    );
    m.insert(
        "trusted_roots.edit_remove_prompt",
        "Select a trusted root to remove:",
    );
    m.insert(
        "trusted_roots.edit_nothing_to_remove",
        "No trusted project roots configured \u{2014} nothing to remove.",
    );
    m.insert(
        "trusted_roots.edit_no_changes",
        "No changes made; trusted project roots unchanged.",
    );
    m.insert(
        "trusted_roots.edit_saved",
        "Trusted project roots saved ({0} entries).",
    );
    m.insert(
        "trusted_roots.edit_cancelled",
        "Edit cancelled; trusted project roots unchanged.",
    );
    m.insert(
        "reindex.root_not_trusted",
        "Refused to reindex '{0}': not under any trusted project root.\n  - Add it first: workhive project roots add {0}\n  - Or omit --root to use the configured trusted roots.",
    );
    m.insert("project_index.state_present", "present");
    m.insert("project_index.state_missing", "missing");
    m.insert("project_index.state_error", "error: {0}");

    // Dependency checks
    m.insert("dep.checking", "Checking system dependencies...");
    m.insert("dep.all_ok", "All dependencies satisfied.");
    m.insert("dep.satisfied", "  ✓ {0} ({1})");
    m.insert("dep.missing", "  ✗ {0} — not found");
    m.insert("dep.version_low", "  ✗ {0} — found {1}, requires {2}+");
    m.insert("dep.builtin", "  ✓ {0} (built-in)");
    m.insert("dep.install_hint", "  Install {0}:");
    m.insert(
        "dep.fatal_missing",
        "Required dependencies are missing. Please install them before continuing.",
    );
    m.insert("dep.git_name", "Git");
    m.insert("dep.tmux_name", "tmux");

    // Git init
    m.insert(
        "git.not_initialized",
        "This directory is not a Git repository.",
    );
    m.insert("git.init_prompt", "Initialize a Git repository here?");
    m.insert("git.init_success", "Git repository initialized.");
    m.insert("git.init_abort", "Aborted. A Git repository is required.");

    // Config diff
    m.insert("config.diff.before", "before");
    m.insert("config.diff.after", "after");

    // Reindex
    m.insert("reindex.confirm", "Rebuild project index?");
    m.insert(
        "reindex.scanning",
        "Scanning {0} root(s) for workhive projects...",
    );
    m.insert(
        "reindex.found",
        "Found {0} workhive project(s) under {1} root(s).",
    );
    m.insert("reindex.updated", "Index updated: {0} active entries.");
    m.insert(
        "reindex.dry_run",
        "(dry-run) Would update index with {0} entries.",
    );
    m.insert(
        "reindex.no_roots",
        "No discovery roots configured. Use --root <path> or set [discovery] project_roots in global config.",
    );
    m.insert(
        "reindex.discovered",
        "Discovered: {0} project(s) under {1} root(s) this run.",
    );
    m.insert(
        "reindex.upserted",
        "Upserted: {0} entry(ies) (new or updated this run).",
    );
    m.insert(
        "reindex.retained_existing",
        "Retained: {0} existing entry(ies) outside scanned roots.",
    );
    m.insert(
        "reindex.final_index_total",
        "Index now contains: {0} project(s) total.",
    );
    m.insert(
        "reindex.dry_run_heading",
        "(dry-run) Reindex preview — no changes were written.",
    );
    m.insert(
        "reindex.warnings_header",
        "warning: some descendant directories were skipped because they could not be read; siblings were still scanned:",
    );
    m.insert("reindex.warning.unreadable_descendant", "  - {0}: {1}");

    // Projects
    m.insert("projects.header", "Registered projects:");
    m.insert(
        "projects.empty",
        "No projects registered. Run 'workhive project reindex' first.",
    );
    m.insert(
        "projects.no_index",
        "No global project index found. Run 'workhive project reindex --root <path>' to build it.",
    );
    m.insert("projects.col.name", "Project");
    m.insert("projects.col.root", "Root");
    m.insert("projects.col.instance_id", "Instance");

    // Config overview
    m.insert("config.overview.title", "Workhive Configuration Overview");
    m.insert("config.overview.scope_global", "Global");
    m.insert("config.overview.scope_project", "Project (merged)");
    m.insert("config.overview.language", "Language");
    m.insert("config.overview.default", "(default)");
    m.insert("config.overview.tmux", "Tmux");
    m.insert("config.overview.layout", "Layout");
    m.insert("config.overview.layout_none", "(none configured)");
    m.insert("config.overview.tools", "Tools");
    m.insert("config.overview.tools_none", "(none configured)");
    m.insert("config.overview.tools_count", "{0} registered");
    m.insert("config.overview.snapshot", "Snapshot");
    m.insert("config.overview.logging", "Logging");
    m.insert("config.overview.logging_defaults", "(defaults)");
    m.insert("config.overview.enabled", "enabled");
    m.insert("config.overview.disabled", "disabled");
    m.insert(
        "config.overview.tip_edit",
        "Tip: use `workhive config edit` to modify settings interactively.",
    );
    m.insert(
        "config.overview.tip_raw",
        "     use `workhive config show --raw` to see raw TOML.",
    );
    m.insert("config.overview.pane_shell", "shell");
    m.insert("config.overview.pane_tool", "tool: {0}");
    m.insert("config.overview.pane_cmd", "cmd: {0}");
    m.insert(
        "config.overview.tool_missing_executable",
        "missing executable",
    );
    m.insert(
        "config.overview.retention_ignored",
        "(retention ignored when rotation=never)",
    );
    m.insert("config.overview.global_path", "Global config: {0}");
    m.insert("config.overview.project_path", "Project config: {0}");
    m.insert(
        "config.overview.effective_source",
        "Effective source: {0} ({1})",
    );
    m.insert(
        "config.overview.field.server_name",
        "Tmux server name (server_name)",
    );
    m.insert(
        "config.overview.field.server_template",
        "Tmux server template (server_template)",
    );
    m.insert(
        "config.overview.field.session_template",
        "Tmux session template (session_template)",
    );
    m.insert(
        "config.overview.field.layout_template",
        "Layout template (template)",
    );
    m.insert("config.overview.field.columns", "Columns (columns)");
    m.insert("config.overview.field.column", "Column {0} (width {1}%)");
    m.insert("config.overview.field.pane", "Pane {0} (height {1}%): {2}");
    m.insert(
        "config.overview.field.timed_interval",
        "Timed save interval (interval_minutes)",
    );
    m.insert(
        "config.overview.field.save_on_event",
        "Save on event (save_on_event)",
    );
    m.insert(
        "config.overview.field.save_on_command_change",
        "Save on command change (save_on_command_change)",
    );
    m.insert(
        "config.overview.field.debounce_ms",
        "Debounce (debounce_ms)",
    );
    m.insert(
        "config.overview.field.retain_revisions",
        "Revision retention (retain_revisions)",
    );
    m.insert(
        "config.overview.field.retain_event_days",
        "Event retention days (retain_event_days)",
    );
    m.insert(
        "config.overview.field.console",
        "Console logging (logging.console)",
    );
    m.insert("config.overview.field.file", "File logging (logging.file)");
    m.insert("config.overview.field.enabled", "Enabled");
    m.insert("config.overview.field.level", "level");
    m.insert("config.overview.field.format", "format");
    m.insert(
        "config.overview.field.base_name",
        "Log file base name (base_name)",
    );
    m.insert(
        "config.overview.field.rotation",
        "Rotation policy (rotation)",
    );
    m.insert(
        "config.overview.field.retention",
        "Retention policy (retention)",
    );
    m.insert("config.overview.field.scope", "Log scope (scope)");
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // Config
    m.insert("config.show_header", "当前配置：");
    m.insert("config.source_line", "配置来源：{0}（{1}）");
    m.insert("config.edit_ask", "是否修改此配置？");
    m.insert("config.edit_success", "配置已更新。");
    m.insert("config.edit_target_line", "编辑目标（{0}）：{1}");
    m.insert(
        "config.edit_will_create_project_config",
        "项目配置文件尚不存在。继续操作将在 {0} 创建新的项目覆盖配置。",
    );
    m.insert("config.edit_confirm_target", "确认修改 {0} 配置（{1}）？");
    m.insert("config.set_preview", "将设置 {0} = {1}");
    m.insert("config.set_success", "配置已更新。");
    m.insert("config.unset_success", "已从配置中移除该键。");
    m.insert("config.env_set_preview", "将设置环境变量 {0} = {1}");
    m.insert("config.env_set_success", "环境变量已更新。");
    m.insert("config.env_unset_preview", "将移除环境变量 {0}");
    m.insert("config.env_unset_success", "环境变量已移除。");

    // Trusted-roots discovery setup-state hints (zh-CN).
    m.insert(
        "trusted_roots.none_configured",
        "尚未配置可信项目目录。\n  - 添加：workhive project roots add <路径>\n  - 可信项目目录决定 Workhive 在哪些路径下可创建 .workhive/、worktree 与 runtime 状态，并为项目发现提供来源。",
    );
    m.insert(
        "trusted_roots.index_missing",
        "已配置可信项目目录，但全局项目索引尚未建立。\n  - 运行：workhive project reindex",
    );
    m.insert(
        "trusted_roots.index_empty",
        "项目索引为空，可信项目目录下未发现已初始化的项目。\n  - 进入某个可信目录后运行：workhive init",
    );
    m.insert(
        "trusted_roots.outside",
        "拒绝在 '{0}' 上执行写操作：该路径不在任何可信项目目录下。\n  - 当前已配置的可信目录：\n{1}\n  - 添加：workhive project roots add {0}\n  - 或在现有可信目录内工作。\n  - 查看可信目录：workhive project roots list",
    );
    m.insert(
        "trusted_roots.outside.empty_list",
        "      （未配置任何可信目录）",
    );
    m.insert("trusted_roots.outside.line_item", "      - {0}");
    m.insert("trusted_roots.list_header", "可信项目目录：");
    m.insert(
        "trusted_roots.list_empty",
        "尚未配置可信项目目录。可使用：workhive project roots add <路径> 添加。",
    );
    m.insert("trusted_roots.added", "已添加可信项目目录：{0}");
    m.insert(
        "trusted_roots.already_present",
        "该路径已是可信项目目录：{0}",
    );
    m.insert("trusted_roots.removed", "已移除可信项目目录：{0}");
    m.insert(
        "trusted_roots.not_present",
        "该路径不在可信项目目录列表中：{0}",
    );
    m.insert(
        "trusted_roots.status_configured",
        "已配置的可信项目目录数量：{0}",
    );
    m.insert("trusted_roots.status_index", "项目索引：{0}");
    m.insert("trusted_roots.status_projects", "索引中项目数：{0}");
    m.insert(
        "trusted_roots.rejected_too_broad",
        "拒绝添加过于宽泛的路径 '{0}'。请使用更具体的项目父目录，例如 ~/development 或 ~/work。",
    );
    m.insert(
        "trusted_roots.covered_by_existing_root",
        "路径 '{0}' 已被可信目录 '{1}' 覆盖。Workhive 已信任该子树下的所有内容，无需再添加冗余子目录。",
    );
    m.insert(
        "trusted_roots.config_unreadable",
        "全局配置无法读取（{0}），可信目录相关提示已降级。请运行 `workhive doctor` 查看详情。",
    );
    m.insert(
        "trusted_roots.edit_affected_header",
        "本次保存将影响 {0} 个已索引的项目（受这些可信目录移除影响）：",
    );
    m.insert("trusted_roots.edit_affected_line", "  - {0}（{1} 个项目）");
    m.insert(
        "trusted_roots.edit_affected_prompt",
        "确认保存吗？移除可信目录不会删除项目文件或 `.workhive/`。",
    );
    m.insert(
        "trusted_roots.edit_affected_requires_force",
        "在非交互模式下拒绝移除会影响已索引项目的可信目录。请在 TTY 中重新运行 `workhive project roots edit`，或对单个目录使用 `workhive project roots remove <path> --force`。",
    );
    m.insert(
        "trusted_roots.path_missing",
        "路径 '{0}' 不存在。请提供一个已存在的目录。",
    );
    m.insert(
        "trusted_roots.path_not_directory",
        "路径 '{0}' 不是目录。可信项目目录必须是目录路径。",
    );
    m.insert(
        "trusted_roots.remove_affected_prompt",
        "移除 '{0}' 将使 {1} 个已索引项目脱离可信目录策略保护。项目本身和 .workhive/ 状态不会被删除。是否继续？",
    );
    m.insert(
        "trusted_roots.remove_affected_requires_force",
        "移除 '{0}' 将影响 {1} 个已索引项目。在非交互模式下需通过 --force 显式确认。",
    );
    m.insert(
        "trusted_roots.remove_aborted",
        "已取消移除，可信项目目录未变更。",
    );
    m.insert(
        "trusted_roots.edit_header",
        "编辑可信项目目录。在菜单中添加/删除条目，选择“保存”后才会写入磁盘。",
    );
    m.insert(
        "trusted_roots.edit_no_tty",
        "交互式编辑需要 TTY。请使用 'workhive project roots add/remove' 在非交互模式下修改。",
    );
    m.insert("trusted_roots.edit_menu_prompt", "选择操作：");
    m.insert("trusted_roots.edit_menu_add", "添加可信目录");
    m.insert("trusted_roots.edit_menu_remove", "删除可信目录");
    m.insert("trusted_roots.edit_menu_save", "保存并退出");
    m.insert("trusted_roots.edit_menu_cancel", "放弃修改并退出");
    m.insert(
        "trusted_roots.edit_add_prompt",
        "要添加的路径（绝对路径或以 ~/ 开头）：",
    );
    m.insert("trusted_roots.edit_remove_prompt", "选择要删除的可信目录：");
    m.insert(
        "trusted_roots.edit_nothing_to_remove",
        "尚未配置可信项目目录，无可删除项。",
    );
    m.insert(
        "trusted_roots.edit_no_changes",
        "未做任何修改，可信项目目录保持不变。",
    );
    m.insert(
        "trusted_roots.edit_saved",
        "可信项目目录已保存（共 {0} 项）。",
    );
    m.insert(
        "trusted_roots.edit_cancelled",
        "已取消编辑，可信项目目录保持不变。",
    );
    m.insert(
        "reindex.root_not_trusted",
        "拒绝对 '{0}' 执行 reindex：该路径不在任何可信项目目录下。\n  - 先添加：workhive project roots add {0}\n  - 或省略 --root 直接使用已配置的可信目录。",
    );
    m.insert("project_index.state_present", "已建立");
    m.insert("project_index.state_missing", "未建立");
    m.insert("project_index.state_error", "读取失败：{0}");

    // Dependency checks
    m.insert("dep.checking", "正在检查系统依赖...");
    m.insert("dep.all_ok", "所有依赖已满足。");
    m.insert("dep.satisfied", "  ✓ {0}（{1}）");
    m.insert("dep.missing", "  ✗ {0} — 未找到");
    m.insert("dep.version_low", "  ✗ {0} — 当前版本 {1}，需要 {2}+");
    m.insert("dep.builtin", "  ✓ {0}（内置）");
    m.insert("dep.install_hint", "  安装 {0}：");
    m.insert("dep.fatal_missing", "缺少必要依赖，请先安装后再继续。");
    m.insert("dep.git_name", "Git");
    m.insert("dep.tmux_name", "tmux");

    // Git init
    m.insert("git.not_initialized", "当前目录不是 Git 仓库。");
    m.insert("git.init_prompt", "是否在此初始化 Git 仓库？");
    m.insert("git.init_success", "Git 仓库已初始化。");
    m.insert("git.init_abort", "已取消。需要 Git 仓库才能继续。");

    // Config diff
    m.insert("config.diff.before", "变更前");
    m.insert("config.diff.after", "变更后");

    // Reindex
    m.insert("reindex.confirm", "是否重建项目索引？");
    m.insert("reindex.scanning", "正在扫描 {0} 个根目录...");
    m.insert(
        "reindex.found",
        "在 {1} 个根目录下找到 {0} 个 workhive 项目。",
    );
    m.insert("reindex.updated", "索引已更新：{0} 个活跃条目。");
    m.insert("reindex.dry_run", "（试运行）将以 {0} 个条目更新索引。");
    m.insert(
        "reindex.no_roots",
        "未配置发现根目录。请使用 --root <path> 或在全局配置中设置 [discovery] project_roots。",
    );
    m.insert(
        "reindex.discovered",
        "本次扫描发现：{0} 个项目，覆盖 {1} 个根目录。",
    );
    m.insert(
        "reindex.upserted",
        "本次写入/更新：{0} 个条目（新增或刷新）。",
    );
    m.insert(
        "reindex.retained_existing",
        "保留既有条目：{0} 个未在本次扫描根目录范围内。",
    );
    m.insert("reindex.final_index_total", "索引当前共有：{0} 个项目。");
    m.insert(
        "reindex.dry_run_heading",
        "（试运行）以下为重建预览，未对索引做任何修改。",
    );
    m.insert(
        "reindex.warnings_header",
        "警告：以下子目录无法读取，已跳过，但同级目录仍被扫描：",
    );
    m.insert("reindex.warning.unreadable_descendant", "  - {0}：{1}");

    // Projects
    m.insert("projects.header", "已注册项目：");
    m.insert(
        "projects.empty",
        "暂无项目。请先运行 'workhive project reindex'。",
    );
    m.insert(
        "projects.no_index",
        "未找到全局项目索引。请运行 'workhive project reindex --root <path>' 来构建索引。",
    );
    m.insert("projects.col.name", "项目");
    m.insert("projects.col.root", "根目录");
    m.insert("projects.col.instance_id", "实例");

    // Config overview
    m.insert("config.overview.title", "Workhive 配置总览");
    m.insert("config.overview.scope_global", "全局");
    m.insert("config.overview.scope_project", "项目（合并后）");
    m.insert("config.overview.language", "语言");
    m.insert("config.overview.default", "（默认）");
    m.insert("config.overview.tmux", "Tmux");
    m.insert("config.overview.layout", "窗格布局");
    m.insert("config.overview.layout_none", "（未配置）");
    m.insert("config.overview.tools", "工具");
    m.insert("config.overview.tools_none", "（未配置）");
    m.insert("config.overview.tools_count", "已注册 {0} 个");
    m.insert("config.overview.snapshot", "快照");
    m.insert("config.overview.logging", "日志");
    m.insert("config.overview.logging_defaults", "（默认值）");
    m.insert("config.overview.enabled", "启用");
    m.insert("config.overview.disabled", "禁用");
    m.insert(
        "config.overview.tip_edit",
        "提示：使用 `workhive config edit` 交互式修改设置。",
    );
    m.insert(
        "config.overview.tip_raw",
        "      使用 `workhive config show --raw` 查看原始 TOML。",
    );
    m.insert("config.overview.pane_shell", "终端");
    m.insert("config.overview.pane_tool", "工具: {0}");
    m.insert("config.overview.pane_cmd", "命令: {0}");
    m.insert("config.overview.tool_missing_executable", "可执行文件缺失");
    m.insert(
        "config.overview.retention_ignored",
        "（rotation=never 时保留策略被忽略）",
    );
    m.insert("config.overview.global_path", "全局配置: {0}");
    m.insert("config.overview.project_path", "项目配置: {0}");
    m.insert(
        "config.overview.effective_source",
        "当前生效来源: {0}（{1}）",
    );
    m.insert(
        "config.overview.field.server_name",
        "Tmux 服务名（server_name）",
    );
    m.insert(
        "config.overview.field.server_template",
        "Tmux 服务模板（server_template）",
    );
    m.insert(
        "config.overview.field.session_template",
        "Tmux 会话模板（session_template）",
    );
    m.insert(
        "config.overview.field.layout_template",
        "布局模板（template）",
    );
    m.insert("config.overview.field.columns", "列配置（columns）");
    m.insert("config.overview.field.column", "第 {0} 列（宽度 {1}%）");
    m.insert("config.overview.field.pane", "Pane {0}（高度 {1}%）: {2}");
    m.insert(
        "config.overview.field.timed_interval",
        "定时保存间隔（interval_minutes）",
    );
    m.insert(
        "config.overview.field.save_on_event",
        "事件触发保存（save_on_event）",
    );
    m.insert(
        "config.overview.field.save_on_command_change",
        "命令变化保存（save_on_command_change）",
    );
    m.insert(
        "config.overview.field.debounce_ms",
        "防抖时间（debounce_ms）",
    );
    m.insert(
        "config.overview.field.retain_revisions",
        "版本保留数（retain_revisions）",
    );
    m.insert(
        "config.overview.field.retain_event_days",
        "事件保留天数（retain_event_days）",
    );
    m.insert(
        "config.overview.field.console",
        "控制台日志（logging.console）",
    );
    m.insert("config.overview.field.file", "文件日志（logging.file）");
    m.insert("config.overview.field.enabled", "启用状态");
    m.insert("config.overview.field.level", "级别");
    m.insert("config.overview.field.format", "格式");
    m.insert("config.overview.field.base_name", "日志文件名（base_name）");
    m.insert("config.overview.field.rotation", "轮转策略（rotation）");
    m.insert("config.overview.field.retention", "保留策略（retention）");
    m.insert("config.overview.field.scope", "日志作用域（scope）");
}
