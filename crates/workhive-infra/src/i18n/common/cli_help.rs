//! CLI help text, clap templates, command/argument descriptions, and parse-error copy.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // CLI help (clap localization)
    m.insert(
        "cli.about",
        "Terminal workspace manager for project-scoped tmux + worktree workflows.",
    );
    // Note: clap renders `{usage-heading}` as the literal "Usage: ".
    // We embed our own zh-CN heading text via a hand-rolled template
    // when zh-CN is active. The English template keeps `{usage-heading}`
    // so clap renders the default token unchanged.
    m.insert(
        "cli.help.template",
        "{about-with-newline}\n{usage-heading} {usage}\n\nSystem Commands:\n  {bin} init              Bootstrap global / project configuration\n  {bin} config            Inspect and edit configuration\n  {bin} doctor            Check environment and diagnose issues\n  {bin} completion        Install shell completions or print completion scripts\n\nResource Commands:\n  {bin} project           Manage tracked projects\n  {bin} workspace         Manage workspaces (open / list / attach / close)\n  {bin} session           Manage and enter runtime sessions\n  {bin} runtime           Manage backend runtime services\n  {bin} pane              Inspect, split, and message panes inside a session\n\nOptions:\n{options}",
    );
    m.insert(
        "cli.help.nested_template",
        "{about-with-newline}\nUsage: {usage}\n\nCommands:\n{subcommands}\nArguments:\n{positionals}\nOptions:\n{options}{after-help}",
    );
    // Shape-specific nested templates. Selected at command-rewrite time
    // based on whether the clap node exposes visible subcommands and/or
    // visible positional arguments. Headings are intentionally elided
    // for empty sections so leaf commands no longer render an empty
    // `Commands:` / `Arguments:` block.
    m.insert(
        "cli.help.nested_template.subcommands_only",
        "{about-with-newline}\nUsage: {usage}\n\nCommands:\n{subcommands}\nOptions:\n{options}{after-help}",
    );
    m.insert(
        "cli.help.nested_template.positionals_only",
        "{about-with-newline}\nUsage: {usage}\n\nArguments:\n{positionals}\nOptions:\n{options}{after-help}",
    );
    m.insert(
        "cli.help.nested_template.options_only",
        "{about-with-newline}\nUsage: {usage}\n\nOptions:\n{options}{after-help}",
    );
    m.insert(
        "cli.help.help_subcommand",
        "Print this message or the help of the given subcommand(s)",
    );
    m.insert("cli.help.heading.usage", "Usage:");
    m.insert("cli.help.heading.options", "Options:");
    m.insert("cli.help.heading.arguments", "Arguments:");
    m.insert("cli.help.heading.commands", "Commands:");

    m.insert(
        "cli.help.cmd.init",
        "Bootstrap global or project configuration",
    );
    m.insert("cli.help.cmd.config", "Inspect and edit configuration");
    m.insert(
        "cli.help.cmd.doctor",
        "Check environment and diagnose issues",
    );
    m.insert(
        "cli.help.cmd.completion",
        "Install shell completions or print completion scripts",
    );
    m.insert(
        "cli.help.cmd.completion.install",
        "Install shell completions for the current user",
    );
    m.insert(
        "cli.help.cmd.completion.script",
        "Print a shell completion script to stdout",
    );
    m.insert("cli.help.cmd.project", "Manage tracked projects");
    m.insert(
        "cli.help.cmd.workspace",
        "Manage workspaces (open, list, attach, close, remove)",
    );
    m.insert("cli.help.cmd.session", "Inspect and enter runtime sessions");
    m.insert("cli.help.cmd.runtime", "Manage backend runtime services");
    m.insert(
        "cli.help.cmd.runtime.stop",
        "Stop a Workhive-managed backend service",
    );
    m.insert(
        "cli.arg.runtime.force",
        "Stop even if the managed server still has workspaces, panes, or agents",
    );
    m.insert(
        "cli.help.cmd.pane",
        "Inspect, split, and message panes inside a session",
    );

    m.insert("cli.help.cmd.project.list", "List tracked projects");
    m.insert("cli.help.cmd.project.reindex", "Refresh the project index");

    m.insert(
        "cli.help.cmd.workspace.open",
        "Open or resume a workspace and connect to its runtime",
    );
    m.insert(
        "cli.help.cmd.workspace.list",
        "List workspaces in this project",
    );
    m.insert(
        "cli.help.cmd.workspace.attach",
        "Attach to an existing workspace session",
    );
    m.insert(
        "cli.help.cmd.workspace.close",
        "Detach or kill a workspace session",
    );
    m.insert(
        "cli.help.cmd.workspace.remove",
        "Remove a workspace (worktree + metadata)",
    );
    m.insert(
        "cli.help.cmd.workspace.destroy",
        "Destroy ALL workspaces and project metadata (irreversible)",
    );

    m.insert(
        "cli.help.cmd.session.list",
        "List runtime sessions tracked by workhive",
    );
    m.insert(
        "cli.help.cmd.session.attach",
        "Attach to a workhive-managed session globally (no project context required)",
    );
    m.insert(
        "cli.help.cmd.session.resume",
        "Resume a native Herdr session from any directory",
    );

    m.insert(
        "cli.help.cmd.workspace.worktree",
        "Manage git worktrees backing workspaces in the current project",
    );
    m.insert(
        "cli.help.cmd.workspace.worktree.list",
        "List Workhive-managed linked worktrees in the current project",
    );
    m.insert(
        "cli.help.cmd.workspace.worktree.remove",
        "Remove a Workhive-managed linked worktree (interactive when no name is given)",
    );

    m.insert(
        "cli.help.cmd.project.roots",
        "Manage trusted project discovery roots",
    );
    m.insert(
        "cli.help.cmd.project.roots.list",
        "List configured discovery roots",
    );
    m.insert(
        "cli.help.cmd.project.roots.add",
        "Add a discovery root to the global config",
    );
    m.insert(
        "cli.help.cmd.project.roots.remove",
        "Remove a discovery root from the global config",
    );
    m.insert(
        "cli.help.cmd.project.roots.edit",
        "Edit discovery roots in $EDITOR",
    );
    m.insert(
        "cli.help.cmd.project.roots.status",
        "Inspect discovery root state (mount, age, recency)",
    );

    m.insert("cli.help.cmd.pane.split", "Split the focused pane");
    m.insert(
        "cli.help.cmd.pane.close",
        "Close a pane (kill it from the layout)",
    );
    m.insert(
        "cli.help.cmd.pane.list",
        "List panes inside the current session",
    );
    m.insert("cli.help.cmd.pane.show", "Show metadata for a pane");
    m.insert("cli.help.cmd.pane.send", "Send keys / a command to a pane");
    // `approve-hooks` is a hidden compatibility alias, not a
    // regular pane subcommand. Direct `--help` must frame the command
    // as deprecated / compatibility-only and remind the user that
    // Codex owns the trust decision via in-Codex `/hooks` approval.
    m.insert(
        "cli.help.cmd.pane.approve_hooks",
        "Deprecated no-op alias kept for script compatibility. Workhive no longer installs provider-side hooks, so no approval is required and this command performs no action.",
    );
    m.insert(
        "cli.arg.pane.approve_hooks.pane",
        "Target pane (id or role). The argument is accepted for backward compatibility and is otherwise ignored",
    );

    m.insert("cli.help.cmd.config.show", "Show effective configuration");
    m.insert("cli.help.cmd.config.edit", "Open configuration in $EDITOR");
    m.insert("cli.help.cmd.config.set", "Set a configuration value");
    m.insert("cli.help.cmd.config.unset", "Remove a configuration value");
    m.insert(
        "cli.help.cmd.config.env_set",
        "Set an entry in the .env layer",
    );
    m.insert(
        "cli.help.cmd.config.env_unset",
        "Remove an entry from the .env layer",
    );
    m.insert(
        "cli.help.cmd.config.explain",
        "Explain how a configuration key resolves",
    );

    m.insert(
        "cli.parse.help_hint",
        "For full usage, run 'workhive --help' or 'workhive <command> --help'.",
    );
    // Localized parse-error block components. These render through Workhive's
    // own error path instead of clap's hardcoded English block. English keeps
    // its own templates here so the renderer is locale-symmetric.
    m.insert("cli.parse.error_prefix", "error:");
    m.insert("cli.parse.usage_heading", "Usage:");
    m.insert("cli.parse.unknown_subcommand", "unknown subcommand '{0}'");
    m.insert("cli.parse.unknown_subcommand_tip", "did you mean '{0}'?");
    m.insert("cli.parse.unknown_option", "unknown option '{0}'");
    m.insert(
        "cli.parse.missing_required",
        "missing required argument: {0}",
    );
    m.insert(
        "cli.parse.invalid_value",
        "invalid value '{0}' for argument '{1}'",
    );
    m.insert(
        "cli.parse.backend_unavailable",
        "runtime backend '{0}' is unavailable",
    );
    m.insert("cli.parse.generic", "command-line parse error");

    // Per-argument help for public CLI flags. clap renders `#[arg(...)]`
    // descriptions verbatim; we inject these strings into the command tree
    // at parse time when the configured locale is zh-CN. Flag tokens
    // themselves (e.g. `--no-attach`) are never translated.
    m.insert("cli.arg.verbose", "Increase log verbosity (-v, -vv, ...)");
    m.insert("cli.arg.help", "Print help");
    m.insert("cli.arg.version", "Print version");
    m.insert("cli.arg.force", "Skip interactive confirmation");
    m.insert(
        "cli.arg.init.global",
        "Initialize global configuration in ~/.config/workhive",
    );
    m.insert("cli.arg.workspace.name", "Workspace name");
    m.insert(
        "cli.arg.workspace.name_opt",
        "Workspace name (defaults to the current branch)",
    );
    m.insert(
        "cli.arg.open.no_attach",
        "Do not open the runtime client after opening",
    );
    m.insert(
        "cli.arg.open.force",
        "Reapply the layout even if the session already exists",
    );
    m.insert(
        "cli.arg.open.mode",
        "Run mode (manual or auto). Defaults to manual.",
    );
    m.insert(
        "cli.arg.open.init",
        "Auto-initialize the project (.workhive/) if missing",
    );
    m.insert(
        "cli.arg.open.attach_existing_branch",
        "Attach an existing local branch with the same name that Workhive does not yet manage, instead of being prompted",
    );
    m.insert(
        "cli.arg.list.view",
        "Output style: auto, table, compact, or json",
    );
    m.insert(
        "cli.arg.remove.delete_branch",
        "Authorize deleting a user-owned backing branch (pre-existing or unknown provenance). Workhive-created branches are deleted by default and do not need this flag",
    );
    m.insert(
        "cli.arg.remove.keep_branch",
        "Keep the backing branch even if Workhive created it. Takes precedence over --delete-branch",
    );
    m.insert(
        "cli.arg.destroy.delete_branch",
        "Authorize deleting user-owned backing branches (pre-existing or unknown provenance) for the destroyed worktrees. Workhive-created branches are deleted by default and do not need this flag",
    );
    m.insert(
        "cli.arg.destroy.keep_branch",
        "Keep backing branches even if Workhive created them. Takes precedence over --delete-branch",
    );
    m.insert(
        "cli.arg.close.kill",
        "Close the runtime workspace instead of detaching",
    );
    m.insert("cli.arg.pane.target", "Pane target (id or role)");
    m.insert("cli.arg.pane.send.text", "Text to send to the pane");
    m.insert(
        "cli.arg.pane.send.enter",
        "Append a newline (Enter) after the text",
    );
    m.insert(
        "cli.arg.pane.send.no_enter",
        "Do not append a newline after the text",
    );
    m.insert(
        "cli.arg.pane.split.direction",
        "Split direction: right, down, left, up",
    );
    m.insert("cli.arg.pane.split.cmd", "Command to run in the new pane");
    m.insert(
        "cli.arg.completion.script.shell",
        "Shell script type to print: bash, zsh, fish, powershell, elvish",
    );
    m.insert(
        "cli.arg.completion.install.shell",
        "Shell to install completions for. Defaults to the current shell.",
    );
    m.insert(
        "cli.arg.project.reindex.root",
        "Project root paths to (re)index",
    );
    m.insert(
        "cli.arg.project.reindex.config_roots",
        "Reindex all project_roots from configuration",
    );
    m.insert(
        "cli.arg.project.reindex.clear_first",
        "Clear the index before reindexing",
    );
    m.insert(
        "cli.arg.project.reindex.dry_run",
        "Print actions without modifying the index",
    );
    m.insert(
        "cli.arg.worktree.name_opt",
        "Worktree name or branch (optional; interactive selection in TTY when omitted)",
    );
    m.insert(
        "cli.arg.worktree.force",
        "Skip the dirty/unmerged safety block",
    );
    m.insert(
        "cli.arg.worktree.delete_branch",
        "Authorize deleting a user-owned backing branch (pre-existing or unknown provenance) after removing the worktree. Workhive-created branches are deleted by default and do not need this flag",
    );
    m.insert(
        "cli.arg.worktree.keep_branch",
        "Keep the backing branch even if Workhive created it. Takes precedence over --delete-branch",
    );
    m.insert(
        "cli.arg.session.list.view",
        "Output style: auto, table, compact, or json",
    );
    m.insert(
        "cli.arg.session.attach.target",
        "Session: <session>, <project/workspace>, or <workspace>",
    );
    m.insert(
        "cli.arg.session.resume.session",
        "Herdr session name (uses the global default when omitted)",
    );
    m.insert(
        "cli.arg.project.roots.path",
        "Filesystem path to a project discovery root",
    );
    m.insert(
        "cli.arg.config.global",
        "Operate on the global configuration",
    );
    m.insert(
        "cli.arg.config.raw",
        "Print raw configuration without resolution overlays",
    );
    m.insert("cli.arg.config.key", "Configuration key");
    m.insert("cli.arg.config.value", "Configuration value");
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // CLI help (clap zh-CN localization)
    m.insert(
        "cli.about",
        "终端工作区管理器：以项目为单位编排运行时会话与 Git worktree。",
    );
    // zh-CN custom help template — note: clap 4 hardcodes the literal
    // "Usage:"/"Options:" tokens emitted by `{usage-heading}` and
    // `{all-args}`. To deliver Chinese section headings without forking
    // clap, we emit a fully-rendered template here.
    m.insert(
        "cli.help.template",
        "{about-with-newline}\n用法: {usage}\n\n系统命令:\n  {bin} init              引导全局或项目配置\n  {bin} config            查看与编辑配置\n  {bin} doctor            检查环境与诊断问题\n  {bin} completion        安装 shell 自动补全，或输出补全脚本\n\n资源命令:\n  {bin} project           管理已记录的项目\n  {bin} workspace         管理工作区（open / list / attach / close）\n  {bin} session           查看并进入运行时会话\n  {bin} runtime           管理后端运行时服务\n  {bin} pane              查看、切分并向窗格发送命令\n\n选项:\n{options}",
    );
    m.insert(
        "cli.help.nested_template",
        "{about-with-newline}\n用法: {usage}\n\n命令:\n{subcommands}\n参数:\n{positionals}\n选项:\n{options}{after-help}",
    );
    // Shape-specific nested templates. Selected at command-rewrite time
    // based on whether the clap node exposes visible subcommands and/or
    // visible positional arguments. Headings are intentionally elided
    // for empty sections so leaf commands no longer render an empty
    // `Commands:` / `Arguments:` block.
    m.insert(
        "cli.help.nested_template.subcommands_only",
        "{about-with-newline}\n用法: {usage}\n\n命令:\n{subcommands}\n选项:\n{options}{after-help}",
    );
    m.insert(
        "cli.help.nested_template.positionals_only",
        "{about-with-newline}\n用法: {usage}\n\n参数:\n{positionals}\n选项:\n{options}{after-help}",
    );
    m.insert(
        "cli.help.nested_template.options_only",
        "{about-with-newline}\n用法: {usage}\n\n选项:\n{options}{after-help}",
    );
    m.insert(
        "cli.help.help_subcommand",
        "查看帮助信息（或某个子命令的帮助）",
    );
    m.insert("cli.help.heading.usage", "用法:");
    m.insert("cli.help.heading.options", "选项:");
    m.insert("cli.help.heading.arguments", "参数:");
    m.insert("cli.help.heading.commands", "命令:");

    m.insert("cli.help.cmd.init", "引导全局或项目配置");
    m.insert("cli.help.cmd.config", "查看与编辑配置");
    m.insert("cli.help.cmd.doctor", "检查环境与诊断问题");
    m.insert(
        "cli.help.cmd.completion",
        "安装 shell 自动补全，或输出补全脚本",
    );
    m.insert(
        "cli.help.cmd.completion.install",
        "为当前用户安装 shell 自动补全",
    );
    m.insert(
        "cli.help.cmd.completion.script",
        "输出 shell 自动补全脚本到 stdout",
    );
    m.insert("cli.help.cmd.project", "管理已记录的项目");
    m.insert(
        "cli.help.cmd.workspace",
        "管理工作区（open / list / attach / close / remove）",
    );
    m.insert("cli.help.cmd.session", "查看并进入运行时会话");
    m.insert("cli.help.cmd.runtime", "管理后端运行时服务");
    m.insert(
        "cli.help.cmd.runtime.stop",
        "停止由 Workhive 托管的后端服务",
    );
    m.insert(
        "cli.arg.runtime.force",
        "即使托管服务器仍有 workspace、pane 或 agent 也强制停止",
    );
    m.insert("cli.help.cmd.pane", "查看、切分并向窗格发送命令");

    m.insert("cli.help.cmd.project.list", "列出已记录的项目");
    m.insert("cli.help.cmd.project.reindex", "重新扫描并刷新项目索引");

    m.insert(
        "cli.help.cmd.workspace.open",
        "打开或恢复工作区，并连接对应的运行时",
    );
    m.insert("cli.help.cmd.workspace.list", "列出当前项目的工作区");
    m.insert("cli.help.cmd.workspace.attach", "接入已存在的工作区会话");
    m.insert("cli.help.cmd.workspace.close", "断开或终止工作区会话");
    m.insert(
        "cli.help.cmd.workspace.remove",
        "移除某个工作区（含工作树与元数据）",
    );
    m.insert(
        "cli.help.cmd.workspace.destroy",
        "销毁所有工作区及项目元数据（不可逆）",
    );

    m.insert(
        "cli.help.cmd.session.list",
        "列出 Workhive 跟踪的所有运行时会话",
    );
    m.insert(
        "cli.help.cmd.session.attach",
        "全局连接到一个 workhive 管理的会话（无需项目上下文）",
    );
    m.insert(
        "cli.help.cmd.session.resume",
        "从任意目录恢复并进入原生 Herdr 会话",
    );

    m.insert(
        "cli.help.cmd.workspace.worktree",
        "管理当前项目下工作区使用的 git 工作树",
    );
    m.insert(
        "cli.help.cmd.workspace.worktree.list",
        "列出当前项目中由 Workhive 管理的关联工作树",
    );
    m.insert(
        "cli.help.cmd.workspace.worktree.remove",
        "移除一个 Workhive 管理的关联工作树（不传名称时在 TTY 下进入交互选择）",
    );

    m.insert("cli.help.cmd.project.roots", "管理可信的项目发现根目录");
    m.insert("cli.help.cmd.project.roots.list", "列出已配置的发现根目录");
    m.insert(
        "cli.help.cmd.project.roots.add",
        "向全局配置中添加一个发现根目录",
    );
    m.insert(
        "cli.help.cmd.project.roots.remove",
        "从全局配置中移除一个发现根目录",
    );
    m.insert(
        "cli.help.cmd.project.roots.edit",
        "用 $EDITOR 打开发现根目录列表进行编辑",
    );
    m.insert(
        "cli.help.cmd.project.roots.status",
        "查看发现根目录的状态（挂载、可达性、最近活跃）",
    );

    m.insert("cli.help.cmd.pane.split", "切分当前激活的窗格");
    m.insert("cli.help.cmd.pane.close", "关闭某个窗格（从布局中移除）");
    m.insert("cli.help.cmd.pane.list", "列出当前会话内的所有窗格");
    m.insert("cli.help.cmd.pane.show", "显示某个窗格的详细信息");
    m.insert("cli.help.cmd.pane.send", "向某个窗格发送按键或命令");
    // zh-CN direct help for the hidden `approve-hooks` compatibility
    // alias. The wording is intentionally user-visible: the command
    // is for script compatibility only, Codex retains final trust
    // authority through its own `/hooks` approval, and the regular
    // flow lives under `workhive pane list`.
    m.insert(
        "cli.help.cmd.pane.approve_hooks",
        "已废弃的空操作别名（仅为脚本兼容保留）。Workhive 不再安装 provider 端 hook，本命令不执行任何操作，也不需要用户批准。",
    );
    m.insert(
        "cli.arg.pane.approve_hooks.pane",
        "目标窗格（id 或角色名）。仅为向后兼容保留，命令实际忽略此参数",
    );

    m.insert("cli.help.cmd.config.show", "显示当前生效的配置");
    m.insert("cli.help.cmd.config.edit", "用 $EDITOR 打开配置文件");
    m.insert("cli.help.cmd.config.set", "设置某个配置项");
    m.insert("cli.help.cmd.config.unset", "删除某个配置项");
    m.insert("cli.help.cmd.config.env_set", "在 .env 层设置一项");
    m.insert("cli.help.cmd.config.env_unset", "从 .env 层移除一项");
    m.insert("cli.help.cmd.config.explain", "解释某个配置项的解析路径");

    m.insert(
        "cli.parse.help_hint",
        "完整用法请运行 'workhive --help' 或 'workhive <命令> --help'。",
    );
    // Localized parse-error block components. These render through Workhive's
    // own error path instead of clap's hardcoded English block. Command
    // names and flag tokens are never translated; only the error
    // description, tip, and usage heading are localized.
    m.insert("cli.parse.error_prefix", "错误：");
    m.insert("cli.parse.usage_heading", "用法：");
    m.insert("cli.parse.unknown_subcommand", "未知的子命令 '{0}'");
    m.insert("cli.parse.unknown_subcommand_tip", "你是不是想输入 '{0}'？");
    m.insert("cli.parse.unknown_option", "未知的选项 '{0}'");
    m.insert("cli.parse.missing_required", "缺少必填参数：{0}");
    m.insert("cli.parse.invalid_value", "参数 '{1}' 的取值 '{0}' 不合法");
    m.insert("cli.parse.backend_unavailable", "运行时后端 '{0}' 不可用");
    m.insert("cli.parse.generic", "命令行解析失败");

    // Per-argument help for public CLI flags. clap renders `#[arg(...)]`
    // descriptions verbatim; we inject these strings into the command tree
    // at parse time when the configured locale is zh-CN. Flag tokens
    // themselves (e.g. `--no-attach`) are never translated.
    m.insert("cli.arg.verbose", "提高日志输出级别（-v、-vv 依次递增）");
    m.insert("cli.arg.help", "显示帮助");
    m.insert("cli.arg.version", "显示版本号");
    m.insert("cli.arg.force", "跳过交互式确认");
    m.insert(
        "cli.arg.init.global",
        "在 ~/.config/workhive 中初始化全局配置",
    );
    m.insert("cli.arg.workspace.name", "工作区名称");
    m.insert(
        "cli.arg.workspace.name_opt",
        "工作区名称（默认为当前 git 分支）",
    );
    m.insert("cli.arg.open.no_attach", "打开后不要启动运行时客户端");
    m.insert("cli.arg.open.force", "即使会话已存在也重新应用窗格布局");
    m.insert(
        "cli.arg.open.mode",
        "运行模式：manual（手动）或 auto（自动）。默认为 manual。",
    );
    m.insert(
        "cli.arg.open.init",
        "若 .workhive/ 不存在，自动为项目执行初始化",
    );
    m.insert(
        "cli.arg.open.attach_existing_branch",
        "显式挂载同名且尚未被 Workhive 管理的已有本地分支，跳过交互确认",
    );
    m.insert(
        "cli.arg.list.view",
        "输出风格：auto、table、compact 或 json",
    );
    m.insert(
        "cli.arg.remove.delete_branch",
        "授权删除用户自有的支撑分支（pre-existing 或 unknown provenance）。Workhive 创建的分支默认会删除，无需该 flag",
    );
    m.insert(
        "cli.arg.remove.keep_branch",
        "即使分支由 Workhive 创建也予以保留；优先级高于 --delete-branch",
    );
    m.insert(
        "cli.arg.destroy.delete_branch",
        "授权删除被销毁工作树用户自有的支撑分支（pre-existing 或 unknown provenance）。Workhive 创建的分支默认会删除，无需该 flag",
    );
    m.insert(
        "cli.arg.destroy.keep_branch",
        "即使分支由 Workhive 创建也予以保留；优先级高于 --delete-branch",
    );
    m.insert("cli.arg.close.kill", "关闭运行时工作区，而不是 detach");
    m.insert("cli.arg.pane.target", "窗格目标（id 或角色名）");
    m.insert("cli.arg.pane.send.text", "要发送给窗格的文本内容");
    m.insert("cli.arg.pane.send.enter", "在文本结尾追加回车");
    m.insert("cli.arg.pane.send.no_enter", "不要在文本结尾追加回车");
    m.insert(
        "cli.arg.pane.split.direction",
        "分割方向：right、down、left、up",
    );
    m.insert("cli.arg.pane.split.cmd", "新窗格中要执行的命令");
    m.insert(
        "cli.arg.completion.script.shell",
        "要输出的补全脚本类型：bash、zsh、fish、powershell、elvish",
    );
    m.insert(
        "cli.arg.completion.install.shell",
        "要安装补全的 shell；不传时自动识别当前 shell",
    );
    m.insert("cli.arg.project.reindex.root", "需要重建索引的项目根目录");
    m.insert(
        "cli.arg.project.reindex.config_roots",
        "对配置中所有 project_roots 执行重建索引",
    );
    m.insert(
        "cli.arg.project.reindex.clear_first",
        "在重建之前先清空索引",
    );
    m.insert(
        "cli.arg.project.reindex.dry_run",
        "只打印将要执行的动作，不实际修改索引",
    );
    m.insert(
        "cli.arg.worktree.name_opt",
        "工作树名称或分支（可选；不传时在 TTY 下进入交互选择）",
    );
    m.insert("cli.arg.worktree.force", "跳过未提交/未合并的安全检查");
    m.insert(
        "cli.arg.worktree.delete_branch",
        "在移除工作树后授权删除用户自有的支撑分支（pre-existing 或 unknown provenance）。Workhive 创建的分支默认会删除，无需该 flag",
    );
    m.insert(
        "cli.arg.worktree.keep_branch",
        "即使分支由 Workhive 创建也予以保留；优先级高于 --delete-branch",
    );
    m.insert(
        "cli.arg.session.list.view",
        "输出风格：auto、table、compact 或 json",
    );
    m.insert(
        "cli.arg.session.attach.target",
        "会话：完整会话名、project/workspace 或 workspace 名",
    );
    m.insert(
        "cli.arg.session.resume.session",
        "Herdr 会话名称（省略时使用全局默认值）",
    );
    m.insert("cli.arg.project.roots.path", "项目发现根目录的文件系统路径");
    m.insert("cli.arg.config.global", "操作全局配置");
    m.insert("cli.arg.config.raw", "输出未经合并的原始配置");
    m.insert("cli.arg.config.key", "配置项名称");
    m.insert("cli.arg.config.value", "配置项值");
}
