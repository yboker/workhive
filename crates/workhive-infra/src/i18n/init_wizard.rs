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
        "Step 2: Configure pane startup commands",
    );
    m.insert("init.step.snapshot", "Step 3: Snapshot settings");
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
    m.insert("init.pane_preview_title", "Layout preview");
    m.insert("init.pane_preview_custom", "custom…");
    m.insert("init.tool.not_found", "[not found]");
    m.insert("init.snapshot_ask", "Enable workspace snapshots?");
    m.insert(
        "init.snapshot_desc",
        "When enabled, pane layout is saved automatically and restored by 'workhive workspace open'.",
    );
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
    m.insert("init.step.trusted_roots", "Trusted project directories");
    m.insert(
        "init.trusted_roots_desc",
        "These directories define where Workhive may discover, initialize, and manage projects. Subdirectories under each trusted root are treated as candidate project roots.",
    );
    m.insert(
        "init.trusted_roots_prompt",
        "Trusted project directory (absolute path or ~/...; empty Enter finishes after at least one valid directory):",
    );
    m.insert(
        "init.trusted_roots_add_more",
        "Add another trusted project directory?",
    );
    m.insert(
        "init.trusted_roots_reindex_prompt",
        "Build the initial project index from these trusted directories now?",
    );
    m.insert(
        "init.trusted_roots_invalid",
        "Path '{0}' is not valid (must be absolute, e.g. /Users/me/dev, or start with ~/).",
    );
    m.insert(
        "init.trusted_roots_summary_none",
        "Trusted project directories: (none configured \u{2014} you can add some later via 'workhive project roots add')",
    );
    m.insert(
        "init.trusted_roots_summary_list",
        "Trusted project directories ({0}):",
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
        "init.trusted_roots_entry_ask",
        "Add a trusted project directory now? Workhive only creates .workhive/, worktrees, and runtime state under trusted roots.",
    );
    m.insert(
        "init.trusted_roots_decline_reprompt",
        "At least one trusted project directory is required before global init can complete. Returning to the trusted-root prompt.",
    );
    m.insert(
        "init.trusted_roots_require_at_least_one",
        "Please add at least one trusted project directory. Workhive will only create project state under trusted directories.",
    );
    m.insert(
        "init.trusted_roots_footer_hint",
        "Tab: complete   \u{2191}/\u{2193}: switch suggestion   Enter: add input   Esc: cancel",
    );
    // Variant footer hint used once at least one trusted root has been
    // accepted in this wizard run. `Esc` now means "back to
    // Add another trusted project directory?" (preserving accepted
    // roots) and the empty-input Enter finish shortcut is surfaced
    // explicitly so it stops being the only discoverable exit. The
    // finish shortcut is phrased as "empty Enter: done" so both
    // finish and add hint reference the same `Enter` key.
    m.insert(
        "init.trusted_roots_footer_hint_with_roots",
        "Tab: complete   \u{2191}/\u{2193}: switch suggestion   Enter: add input   empty Enter: done   Esc: back",
    );
    m.insert(
        "init.trusted_roots_no_match",
        "No matching directories found.",
    );
    m.insert("init.trusted_roots_added", "Added trusted directory: {0}");
    m.insert(
        "init.trusted_roots_path_missing",
        "Path '{0}' does not exist. Provide an existing directory.",
    );
    m.insert(
        "init.trusted_roots_path_not_directory",
        "Path '{0}' is not a directory. Trusted project roots must be directories.",
    );
    m.insert(
        "init.trusted_roots_duplicate",
        "Path '{0}' is already in the trusted-root list.",
    );
    m.insert(
        "init.trusted_roots_finish_prompt",
        "Add another trusted project directory?",
    );

    // Trusted-roots wizard: additional rejection reasons surfaced on
    // the input field. Existing trust-root strings
    // (`init.trusted_roots_*` and the older `trusted_roots.*` family)
    // are unchanged.
    m.insert(
        "init.trusted_roots.reject_relative",
        "Path must be absolute or start with '~/': {0}",
    );
    m.insert(
        "init.trusted_roots.reject_overly_broad",
        "Refusing overly-broad trusted root: {0}. \
         Choose a concrete project parent directory like `~/development` or \
         `/Users/<you>/projects`.",
    );
    m.insert(
        "init.trusted_roots.covered_by_existing_root",
        "Path {0} is already covered by trusted root {1}. \
         Workhive trusts the entire subtree, so a redundant child entry is not added.",
    );
    m.insert(
        "init.trusted_roots.examples_hint",
        "Enter a concrete project parent directory, e.g. `~/development`, \
         `~/work`, or `/Users/<you>/projects`. Home (`~`), system roots, \
         and `/` are not allowed. Press Tab on an empty input to start from \
         `~/` or common existing project directories.",
    );
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
    m.insert("init.guide_global", "未找到全局配置。正在引导全局初始化...");
    m.insert(
        "init.global_complete_continue",
        "全局配置完成。继续执行您的命令...",
    );
    m.insert("init.wizard_title", "workhive 配置向导");
    m.insert("init.step.layout", "步骤 1：选择窗格布局模板");
    m.insert("init.step.pane_commands", "步骤 2：配置窗格启动命令");
    m.insert("init.step.snapshot", "步骤 3：快照设置");
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
    m.insert("init.pane_preview_title", "布局预览");
    m.insert("init.pane_preview_custom", "自定义…");
    m.insert("init.tool.not_found", "[未安装]");
    m.insert("init.snapshot_ask", "是否启用工作区快照？");
    m.insert(
        "init.snapshot_desc",
        "开启后将自动保存窗格布局，执行 workhive workspace open 时自动恢复。",
    );
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
    m.insert("init.step.trusted_roots", "可信项目目录");
    m.insert(
        "init.trusted_roots_desc",
        "这些目录决定了 Workhive 在哪些路径下可以发现、初始化和管理项目。每个可信目录下的子目录会被视作候选项目根。",
    );
    m.insert(
        "init.trusted_roots_prompt",
        "可信项目目录（绝对路径或以 ~/ 开头；至少添加一个有效目录后，空行回车结束）：",
    );
    m.insert(
        "init.trusted_roots_add_more",
        "继续添加另一个可信项目目录？",
    );
    m.insert(
        "init.trusted_roots_reindex_prompt",
        "立即基于这些可信项目目录构建初始项目索引？",
    );
    m.insert(
        "init.trusted_roots_invalid",
        "路径 '{0}' 不可用：必须是绝对路径（如 /Users/me/dev）或以 ~/ 开头。",
    );
    m.insert(
        "init.trusted_roots_summary_none",
        "可信项目目录：（未配置 \u{2014} 之后可使用 'workhive project roots add' 添加）",
    );
    m.insert(
        "init.trusted_roots_summary_list",
        "可信项目目录（共 {0} 个）：",
    );
    m.insert(
        "init.trusted_roots_reindex_skipped",
        "已跳过初始项目索引构建。准备好后运行：workhive project reindex",
    );
    m.insert(
        "init.trusted_roots_reindex_done",
        "项目索引已构建，共发现 {0} 个项目。",
    );
    m.insert(
        "init.trusted_roots_entry_ask",
        "现在添加可信项目目录？Workhive 仅在可信目录下创建 .workhive/、worktree 与运行时状态。",
    );
    m.insert(
        "init.trusted_roots_decline_reprompt",
        "全局初始化必须至少添加一个可信项目目录，已返回到目录输入。",
    );
    m.insert(
        "init.trusted_roots_require_at_least_one",
        "请至少添加一个可信项目目录。Workhive 只会在可信目录下创建项目状态。",
    );
    m.insert(
        "init.trusted_roots_footer_hint",
        "Tab 补全   \u{2191}/\u{2193} 切换建议   Enter 添加输入   Esc 取消",
    );
    // Variant footer hint used once at least one trusted root has been
    // accepted in this wizard run. `Esc` now means "back to
    // Add another trusted project directory?" (preserving accepted
    // roots) and the empty-input Enter finish shortcut is surfaced
    // explicitly so it stops being the only discoverable exit. The
    // finish shortcut is phrased so both finish and add hint reference
    // the same `Enter` key.
    m.insert(
        "init.trusted_roots_footer_hint_with_roots",
        "Tab 补全   \u{2191}/\u{2193} 切换建议   Enter 添加输入   空输入 Enter 完成   Esc 返回",
    );
    m.insert("init.trusted_roots_no_match", "未找到匹配目录。");
    m.insert("init.trusted_roots_added", "已添加可信目录：{0}");
    m.insert(
        "init.trusted_roots_path_missing",
        "路径 '{0}' 不存在，请提供已存在的目录。",
    );
    m.insert(
        "init.trusted_roots_path_not_directory",
        "路径 '{0}' 不是目录。可信项目目录必须是目录。",
    );
    m.insert(
        "init.trusted_roots_duplicate",
        "路径 '{0}' 已存在于可信目录列表。",
    );
    m.insert(
        "init.trusted_roots_finish_prompt",
        "继续添加另一个可信项目目录？",
    );

    // Trusted-roots wizard: additional rejection reasons surfaced on
    // the input field. Existing trust-root strings
    // (`init.trusted_roots_*` and the older `trusted_roots.*` family)
    // are unchanged.
    m.insert(
        "init.trusted_roots.reject_relative",
        "路径必须是绝对路径或以 '~/' 开头：{0}",
    );
    m.insert(
        "init.trusted_roots.reject_overly_broad",
        "拒绝过于宽泛的可信目录：{0}。请选择更具体的项目父目录，例如 `~/development` 或 `/Users/<you>/projects`。",
    );
    m.insert(
        "init.trusted_roots.covered_by_existing_root",
        "路径 {0} 已被可信目录 {1} 覆盖。Workhive 已信任该子树下的所有内容，无需再添加冗余子目录。",
    );
    m.insert(
        "init.trusted_roots.examples_hint",
        "请输入具体项目父目录，例如 `~/development`、`~/work` 或 `/Users/<你>/projects`。不允许直接信任主目录（`~`）、系统目录或根目录（`/`）。空输入按 Tab 会从 `~/` 和常见已存在项目目录开始提示。",
    );
    m.insert(
        "init.trusted_roots.discovery_root_missing",
        "配置中的可信目录不存在：{0}。请先创建该目录，或使用 `workhive project roots remove` 移除后再执行 `workhive project reindex`。",
    );
}
