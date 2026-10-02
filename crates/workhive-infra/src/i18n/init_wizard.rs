//! Init wizard surface: bootstrap prompts, pane-position labels, overwrite/scope, and diagram text.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Init
    m.insert(
        "init.global_success",
        "Global configuration initialized at ~/.config/workhive/",
    );
    m.insert(
        "init.project_success",
        "Project initialized. Created .workhive/ directory.",
    );
    m.insert(
        "init.global_next_steps",
        "\nNext:\n  1. cd <your-project>\n  2. workhive init\n  3. workhive workspace open <workspace>",
    );
    // Shell-completion install copy: explains the install, asks for
    // explicit consent on a TTY, and never silently modifies shell
    // startup files.
    m.insert(
        "init.global_completion.intro",
        "Workhive can install shell auto-completion so your terminal will Tab-suggest \
         Workhive commands, subcommands, arguments, and your workspace / pane names. \
         You can always run `workhive completion install` later.",
    );
    m.insert(
        "init.global_completion.prompt",
        "Install shell completion now?",
    );
    m.insert(
        "init.global_completion.manual_hint",
        "Run `workhive completion install` later to enable Tab completion.",
    );
    m.insert(
        "init.global_completion.manual_hint_with_shell",
        "Detected shell: {0}. Run `workhive completion install` later to enable Tab completion.",
    );
    m.insert(
        "init.global_completion.shell_not_detected",
        "Workhive could not detect your shell automatically. Run \
         `workhive completion install <shell>` (one of: bash, zsh, fish, elvish, powershell).",
    );
    m.insert(
        "init.global_completion.declined",
        "Skipped. Run `workhive completion install` later when you want shell completion.",
    );
    m.insert(
        "init.global_completion.installed",
        "Shell completion installed for {0}.",
    );
    m.insert(
        "init.project_next_steps",
        "\nNext:\n  1. workhive workspace list\n  2. workhive workspace open main\n  3. workhive workspace open <feature-name>",
    );
    m.insert("init.already_exists", "Configuration already exists at {0}");
    m.insert("init.select_language", "Select language");
    m.insert("init.lang.english", "English");
    m.insert("init.lang.zh_cn", "Chinese (简体中文)");
    m.insert("init.select_backend", "Runtime backend");
    m.insert("init.backend.inherit", "Inherit from global config");
    m.insert("init.backend.tmux", "tmux");
    m.insert("init.backend.herdr", "herdr");
    m.insert("init.select_herdr_session", "Herdr session");
    m.insert("init.herdr_session.inherit", "Inherit global session ({0})");
    m.insert(
        "init.herdr_session.override",
        "Use a project-specific session",
    );
    m.insert("init.herdr_session.input", "Herdr session name");
    m.insert(
        "init.guide_global",
        "Global config not found. Setting up global configuration first...",
    );
    m.insert(
        "init.global_complete_continue",
        "Global configuration complete. Continuing with your command...",
    );
    m.insert("init.wizard_title", "workhive Setup Wizard");
    m.insert("init.step.layout", "Step 1: Select pane layout template");
    m.insert(
        "init.step.pane_commands",
        "Step 3: Configure pane startup commands",
    );
    m.insert("init.step.confirm", "Configuration summary");
    m.insert("init.layout_template", "Select a layout template");
    m.insert(
        "init.layout.ai_left",
        "AI Coding (Left) — AI agent on left + lazygit + shell",
    );
    m.insert(
        "init.layout.ai_dual",
        "AI Coding (Dual) — Claude Code (left) + Codex (right) + shell",
    );
    m.insert("init.layout.half", "Half Split — two empty shells (50/50)");
    m.insert("init.layout.quad", "Quad — four empty shells (2×2)");
    m.insert("init.layout.single", "Single Pane — one pane only");
    m.insert(
        "init.layout.custom",
        "Custom — define columns and panes freely",
    );
    m.insert("init.layout.custom_dsl", "custom columns × rows layout");
    m.insert("init.layout.preview", "Preview");
    m.insert("init.custom_desc", "define your own columns × rows layout");
    m.insert("init.custom_rows", "Number of rows (1-{0})");
    m.insert("init.custom_cols", "Number of columns (1-{0})");
    m.insert(
        "init.custom_cols_dynamic",
        "Number of columns (1-{0}, total panes: {1}×columns ≤ 10)",
    );
    m.insert(
        "init.custom_rows_dynamic",
        "Number of rows (1-{0}, total panes: {1}×rows ≤ 10)",
    );
    m.insert("init.custom_col_width", "Column {0} width (%)");
    m.insert("init.custom_col_rows", "Column {0} pane count (1-{1})");
    m.insert(
        "init.custom_progress",
        "Columns configured: {0}/{1} ({2}) · panes so far: {3}",
    );
    m.insert("init.custom_progress_pending", "pending");
    m.insert("init.pane_context_header", "Layout: {0} · {1} pane(s)");
    m.insert("init.pane_cmd_modify_ask", "Modify default pane commands?");
    m.insert("init.pane_cmd_prompt", "{0} startup command");
    m.insert("init.pane_cmd_selector_title", "{0} startup command");
    m.insert("init.pane_cmd_selector_panes", "Panes: {0}");
    m.insert("init.pane_cmd_empty", "(empty shell)");
    m.insert("init.pane_cmd_empty_option", "Empty shell (no command)");
    m.insert(
        "init.pane_cmd_custom_option",
        "Custom command (manual input)",
    );
    m.insert("init.pane_cmd_custom_input", "{0} custom command");
    m.insert(
        "init.pane_cmd_not_found_warning",
        "Command '{0}' not found on PATH",
    );
    m.insert("init.pane_cmd_retry_prompt", "Try again?");
    m.insert("init.pane_preview_title", "Layout preview");
    m.insert("init.pane_preview_custom", "custom…");
    m.insert("init.tool.not_found", "[not found]");
    m.insert("init.confirm_summary", "Configuration to apply:");
    m.insert("init.confirm_apply", "Apply this configuration?");

    // Init wizard: pane position labels (row/col based)
    m.insert("init.pane.position", "Row {0}, Col {1}");
    m.insert("init.pane.ai_left.0", "Row 1, Col 1 (AI Agent)");
    m.insert("init.pane.ai_left.1", "Row 2, Col 1 (Shell)");
    m.insert("init.pane.ai_left.2", "Row 1, Col 2 (Lazygit)");
    m.insert("init.pane.ai_dual.0", "Row 1, Col 1 (Claude Code)");
    m.insert("init.pane.ai_dual.1", "Row 2, Col 1 (Shell)");
    m.insert("init.pane.ai_dual.2", "Row 1, Col 2 (Codex)");
    m.insert("init.pane.half.0", "Row 1, Col 1 (Shell)");
    m.insert("init.pane.half.1", "Row 1, Col 2 (Shell)");
    m.insert("init.pane.quad.0", "Row 1, Col 1 (Shell)");
    m.insert("init.pane.quad.1", "Row 2, Col 1 (Shell)");
    m.insert("init.pane.quad.2", "Row 1, Col 2 (Shell)");
    m.insert("init.pane.quad.3", "Row 2, Col 2 (Shell)");
    m.insert("init.pane.single.0", "Row 1, Col 1 (Main)");

    // Init wizard: overwrite & scope
    m.insert(
        "init.config_create_confirm",
        "No configuration found at {0}. Create it now?",
    );
    m.insert(
        "init.config_exists_overwrite",
        "Configuration already exists at {0}. Overwrite?",
    );
    m.insert("init.config_abort", "Aborted. Configuration unchanged.");
    m.insert("init.scope.global", "global");
    m.insert("init.scope.project", "project");
    m.insert("init.step_complete", "  ✓ {0}");

    // Init wizard: diagram text labels
    m.insert("init.diagram.ai_agent", "AI Agent");
    m.insert("init.diagram.shell", "Shell");
    m.insert("init.diagram.lazygit", "Lazygit");
    m.insert("init.diagram.files", "Files");
    m.insert("init.diagram.main", "Main Pane");

    // Init wizard: trusted roots step (English).
    m.insert(
        "init.trusted_roots_reindex_prompt",
        "Build the initial project index from these trusted directories now?",
    );
    m.insert(
        "init.trusted_roots_invalid",
        "Path '{0}' is not valid (must be absolute, e.g. /Users/me/dev, or start with ~/).",
    );
    m.insert(
        "init.trusted_roots_reindex_skipped",
        "Skipped initial project index build. Run 'workhive project reindex' when ready.",
    );
    m.insert(
        "init.trusted_roots_reindex_done",
        "Project index built. Found {0} project(s).",
    );
    m.insert(
        "init.trusted_roots_configure_now",
        "Configure trusted project directories now?",
    );
    m.insert(
        "init.trusted_roots_view_modify",
        "{0} trusted directory(ies) configured. View or modify?",
    );

    // Admission menu (English).
    m.insert("admission.menu.trust_project", "Trust this project");
    m.insert(
        "admission.menu.manage_directories",
        "Manage trusted directories...",
    );
    m.insert("admission.menu.cancel", "Cancel");
    m.insert(
        "admission.menu.prompt",
        "This project is outside trusted directories:",
    );
    m.insert(
        "admission.still_outside",
        "Project is still outside trusted directories after editor changes.",
    );

    // Reconciliation status (English).
    m.insert(
        "reconciliation.status.idle",
        "No reconciliation performed yet.",
    );
    m.insert(
        "reconciliation.status.updating",
        "Index reconciliation in progress (revision {0})...",
    );
    m.insert(
        "reconciliation.status.completed",
        "Index reconciliation completed (revision {0}, {1} project(s) discovered).",
    );
    m.insert(
        "reconciliation.status.failed",
        "Index reconciliation failed (revision {0}): {1}",
    );
    m.insert(
        "reconciliation.recovery_hint",
        "Run 'workhive project reindex' to rebuild the index manually.",
    );

    // Variant footer hint used once at least one trusted root has been
    // accepted in this wizard run. `Esc` now means "back to
    // Add another trusted project directory?" (preserving accepted
    // roots) and the empty-input Enter finish shortcut is surfaced
    // explicitly so it stops being the only discoverable exit. The
    // finish shortcut is phrased as "empty Enter: done" so both
    // finish and add hint reference the same `Enter` key.

    // Trusted-roots wizard: additional rejection reasons surfaced on
    // the input field. Existing trust-root strings
    // (`init.trusted_roots_*` and the older `trusted_roots.*` family)
    // are unchanged.
    m.insert(
        "init.trusted_roots.discovery_root_missing",
        "Configured trusted root does not exist: {0}. \
         Create the directory first, or remove it with `workhive project roots remove`, \
         then rerun `workhive project reindex`.",
    );
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // Init
    m.insert(
        "init.global_success",
        "全局配置已初始化于 ~/.config/workhive/",
    );
    m.insert(
        "init.project_success",
        "项目已初始化。已创建 .workhive/ 目录。",
    );
    m.insert(
        "init.global_next_steps",
        "\n下一步：\n  1. cd <你的项目目录>\n  2. workhive init\n  3. workhive workspace open <工作区名>",
    );
    // Shell-completion install copy: explains the install, asks for
    // explicit consent on a TTY, and never silently modifies shell
    // startup files.
    m.insert(
        "init.global_completion.intro",
        "Workhive 可以为你安装 shell 自动补全：终端将能够 Tab 补全 Workhive \
         的命令、子命令、参数，以及当前工作区/Pane 名称。你随时可以运行 \
         `workhive completion install` 完成这一步。",
    );
    m.insert(
        "init.global_completion.prompt",
        "现在安装 shell completion 吗？",
    );
    m.insert(
        "init.global_completion.manual_hint",
        "稍后运行 `workhive completion install` 即可启用 Tab 补全。",
    );
    m.insert(
        "init.global_completion.manual_hint_with_shell",
        "已检测到 shell：{0}。稍后运行 `workhive completion install` 即可启用 Tab 补全。",
    );
    m.insert(
        "init.global_completion.shell_not_detected",
        "Workhive 无法自动识别当前 shell。请运行 \
         `workhive completion install <shell>`（可选：bash、zsh、fish、elvish、powershell）。",
    );
    m.insert(
        "init.global_completion.declined",
        "已跳过。等需要时再运行 `workhive completion install` 启用 shell 补全。",
    );
    m.insert(
        "init.global_completion.installed",
        "已为 {0} 安装 shell 补全。",
    );
    m.insert(
        "init.project_next_steps",
        "\n下一步：\n  1. workhive workspace list\n  2. workhive workspace open main\n  3. workhive workspace open <功能分支名>",
    );
    m.insert("init.already_exists", "配置已存在于 {0}");
    m.insert("init.select_language", "选择语言");
    m.insert("init.lang.english", "English");
    m.insert("init.lang.zh_cn", "Chinese (简体中文)");
    m.insert("init.select_backend", "运行时后端");
    m.insert("init.backend.inherit", "继承全局配置");
    m.insert("init.backend.tmux", "tmux");
    m.insert("init.backend.herdr", "herdr");
    m.insert("init.select_herdr_session", "Herdr 会话");
    m.insert("init.herdr_session.inherit", "继承全局会话（{0}）");
    m.insert("init.herdr_session.override", "使用项目专属会话");
    m.insert("init.herdr_session.input", "Herdr 会话名称");
    m.insert("init.guide_global", "未找到全局配置。正在引导全局初始化...");
    m.insert(
        "init.global_complete_continue",
        "全局配置完成。继续执行您的命令...",
    );
    m.insert("init.wizard_title", "workhive 配置向导");
    m.insert("init.step.layout", "步骤 1：选择窗格布局模板");
    m.insert("init.step.pane_commands", "步骤 3：配置窗格启动命令");
    m.insert("init.step.confirm", "配置概要");
    m.insert("init.layout_template", "选择布局模板");
    m.insert(
        "init.layout.ai_left",
        "AI 编程（左侧）— AI 助手在左 + lazygit + 终端",
    );
    m.insert(
        "init.layout.ai_dual",
        "AI 编程（左右）— Claude Code（左）+ Codex（右）+ 终端",
    );
    m.insert("init.layout.half", "对半分割 — 两个空终端（50/50）");
    m.insert("init.layout.quad", "四格布局 — 四个空终端（2×2）");
    m.insert("init.layout.single", "单窗格 — 仅一个窗格");
    m.insert("init.layout.custom", "自定义 — 自由定义列数和窗格");
    m.insert("init.layout.custom_dsl", "自定义列 × 行布局");
    m.insert("init.layout.preview", "预览");
    m.insert("init.custom_desc", "自定义列 × 行布局");
    m.insert("init.custom_rows", "行数（1-{0}）");
    m.insert("init.custom_cols", "列数（1-{0}）");
    m.insert(
        "init.custom_cols_dynamic",
        "列数（1-{0}，总窗格数: {1}×列 ≤ 10）",
    );
    m.insert(
        "init.custom_rows_dynamic",
        "行数（1-{0}，总窗格数: {1}×行 ≤ 10）",
    );
    m.insert("init.custom_col_width", "第 {0} 列宽度（%）");
    m.insert("init.custom_col_rows", "第 {0} 列窗格数 (1-{1})");
    m.insert(
        "init.custom_progress",
        "已配置列：{0}/{1}（{2}）· 当前窗格总数：{3}",
    );
    m.insert("init.custom_progress_pending", "待配置");
    m.insert("init.pane_context_header", "布局：{0} · 共 {1} 个窗格");
    m.insert("init.pane_cmd_modify_ask", "是否修改默认窗格命令？");
    m.insert("init.pane_cmd_prompt", "{0} 启动命令");
    m.insert("init.pane_cmd_selector_title", "{0} 启动命令");
    m.insert("init.pane_cmd_selector_panes", "窗格：{0}");
    m.insert("init.pane_cmd_empty", "（空终端）");
    m.insert("init.pane_cmd_empty_option", "空终端（无启动命令）");
    m.insert("init.pane_cmd_custom_option", "自定义命令（手动输入）");
    m.insert("init.pane_cmd_custom_input", "{0} 自定义命令");
    m.insert(
        "init.pane_cmd_not_found_warning",
        "命令 '{0}' 未在系统 PATH 中找到",
    );
    m.insert("init.pane_cmd_retry_prompt", "是否重新输入？");
    m.insert("init.pane_preview_title", "布局预览");
    m.insert("init.pane_preview_custom", "自定义…");
    m.insert("init.tool.not_found", "[未安装]");
    m.insert("init.confirm_summary", "即将应用的配置：");
    m.insert("init.confirm_apply", "是否应用此配置？");

    // Init wizard: pane position labels (row/col based)
    m.insert("init.pane.position", "第 {0} 行，第 {1} 列");
    m.insert("init.pane.ai_left.0", "第 1 行，第 1 列（AI 助手）");
    m.insert("init.pane.ai_left.1", "第 2 行，第 1 列（终端）");
    m.insert("init.pane.ai_left.2", "第 1 行，第 2 列（Lazygit）");
    m.insert("init.pane.ai_dual.0", "第 1 行，第 1 列（Claude Code）");
    m.insert("init.pane.ai_dual.1", "第 2 行，第 1 列（终端）");
    m.insert("init.pane.ai_dual.2", "第 1 行，第 2 列（Codex）");
    m.insert("init.pane.half.0", "第 1 行，第 1 列（终端）");
    m.insert("init.pane.half.1", "第 1 行，第 2 列（终端）");
    m.insert("init.pane.quad.0", "第 1 行，第 1 列（终端）");
    m.insert("init.pane.quad.1", "第 2 行，第 1 列（终端）");
    m.insert("init.pane.quad.2", "第 1 行，第 2 列（终端）");
    m.insert("init.pane.quad.3", "第 2 行，第 2 列（终端）");
    m.insert("init.pane.single.0", "第 1 行，第 1 列（主窗格）");

    // Init wizard: overwrite & scope
    m.insert(
        "init.config_create_confirm",
        "未找到配置文件 {0}。是否立即创建？",
    );
    m.insert(
        "init.config_exists_overwrite",
        "配置已存在于 {0}，是否覆盖？",
    );
    m.insert("init.config_abort", "已取消。配置未更改。");
    m.insert("init.scope.global", "全局");
    m.insert("init.scope.project", "项目");
    m.insert("init.step_complete", "  ✓ {0}");

    // Init wizard: diagram text labels
    m.insert("init.diagram.ai_agent", "AI 助手");
    m.insert("init.diagram.shell", "终端");
    m.insert("init.diagram.lazygit", "Lazygit");
    m.insert("init.diagram.files", "文件");
    m.insert("init.diagram.main", "主窗格");

    // Init wizard: trusted roots step (zh-CN).
    m.insert(
        "init.trusted_roots_reindex_prompt",
        "立即基于这些可信项目目录构建初始项目索引？",
    );
    m.insert(
        "init.trusted_roots_invalid",
        "路径 '{0}' 不可用：必须是绝对路径（如 /Users/me/dev）或以 ~/ 开头。",
    );
    m.insert(
        "init.trusted_roots_reindex_skipped",
        "已跳过初始项目索引构建。准备好后运行：workhive project reindex",
    );
    m.insert(
        "init.trusted_roots_reindex_done",
        "项目索引已构建，共发现 {0} 个项目。",
    );
    m.insert("init.trusted_roots_configure_now", "现在配置可信项目目录？");
    m.insert(
        "init.trusted_roots_view_modify",
        "已配置 {0} 个可信目录。查看或修改？",
    );

    // Admission menu (zh-CN).
    m.insert("admission.menu.trust_project", "信任此项目");
    m.insert("admission.menu.manage_directories", "管理可信目录...");
    m.insert("admission.menu.cancel", "取消");
    m.insert("admission.menu.prompt", "此项目位于可信目录之外：");
    m.insert("admission.still_outside", "编辑后项目仍在可信目录之外。");

    // Reconciliation status (zh-CN).
    m.insert("reconciliation.status.idle", "尚未执行索引调和。");
    m.insert(
        "reconciliation.status.updating",
        "索引调和进行中（修订版 {0}）...",
    );
    m.insert(
        "reconciliation.status.completed",
        "索引调和已完成（修订版 {0}，发现 {1} 个项目）。",
    );
    m.insert(
        "reconciliation.status.failed",
        "索引调和失败（修订版 {0}）：{1}",
    );
    m.insert(
        "reconciliation.recovery_hint",
        "运行 'workhive project reindex' 以手动重建索引。",
    );

    // Variant footer hint used once at least one trusted root has been
    // accepted in this wizard run. `Esc` now means "back to
    // Add another trusted project directory?" (preserving accepted
    // roots) and the empty-input Enter finish shortcut is surfaced
    // explicitly so it stops being the only discoverable exit. The
    // finish shortcut is phrased so both finish and add hint reference
    // the same `Enter` key.

    // Trusted-roots wizard: additional rejection reasons surfaced on
    // the input field. Existing trust-root strings
    // (`init.trusted_roots_*` and the older `trusted_roots.*` family)
    // are unchanged.
    m.insert(
        "init.trusted_roots.discovery_root_missing",
        "配置中的可信目录不存在：{0}。请先创建该目录，或使用 `workhive project roots remove` 移除后再执行 `workhive project reindex`。",
    );
}
