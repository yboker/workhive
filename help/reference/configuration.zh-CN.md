## 配置文件

配置文件位于 `.workhive/config.yaml`，由 `workhive init` 生成，也可手动编辑。
语言切换：[English](configuration.md) | [中文](configuration.zh-CN.md)

### 配置优先级

1. **项目级** — `.workhive/config.yaml` (最高优先级)
2. **用户级** — `~/.config/workhive/config.yaml`
3. **内置默认值** (最低优先级)

### 完整配置参考

`config.yaml` 中所有可用的配置键如下。所有键均为可选——未指定的键使用内置默认值。
`workhive init` 会写入这套 schema；`workhive config` 仅做严格校验，不会回填旧键。

| 键 | 类型 | 默认值 | 说明 |
|----|------|--------|------|
| `version` | string | (当前版本) | 配置文件版本（由 `workhive init` 自动设置）|
| `language` | string | `"en"` | 界面语言：`"en"`（英文）或 `"zh"`（中文）|
| **布局 (Column-based DSL)** | | | |
| `layout.columns` | array | (2列默认) | 列定义数组 |
| `layout.columns[].width` | int | 50 | 列宽度百分比 |
| `layout.columns[].panes` | array | — | 该列的面板定义数组 |
| `layout.columns[].panes[].height` | int | 50 | 面板高度百分比 |
| `layout.columns[].panes[].command` | string\|null | null | 面板命令 |
| `layout.columns[].panes[].role` | string\|null | null | 可选面板角色（用于角色寻址）|
| **Worktree** | | | |
| `worktree.base_path` | string | `".workhive/worktrees"` | Worktree 存放目录（相对于 Git 根目录）|
| `worktree.auto_create` | bool | `true` | `open` 时自动创建 Worktree |
| `worktree.auto_delete` | bool | `true` | `remove` 时自动删除 Worktree |
| `worktree.sync_files` | array | `[]` | 新 Worktree 同步的文件列表 |
| `worktree.sync_files[].source` | string | — | 源文件/目录路径（相对于 Git 根目录）|
| `worktree.sync_files[].mode` | string | `"copy"` | 同步模式：`"copy"` 或 `"symlink"` |
| **tmux** | | | |
| `tmux.session_prefix` | string\|null | null | 自定义会话名前缀（null = 自动：`wh@<id>@<项目名>`）|
| `tmux.mouse_support` | bool | `true` | 启用 tmux 鼠标支持 |
| `tmux.interaction_profile` | string | `"workspace"` | 鼠标滚轮交互策略。`workspace`：agent pane 按 pane metadata 滚动历史，shell pane 不会隐式进入 copy-mode；`compatible`：更接近 tmux 默认滚轮行为 |
| `tmux.env` | map | `{ CLAUDE_CODE_NO_FLICKER: 1 }` | 注入到 pane 启动命令的环境变量（优先级：项目 > 全局） |
| `tmux.theme` | string | `"catppuccin-mocha"` | 状态栏主题（见下方可选主题）|
| `tmux.attach_delay` | duration | `0s` | 自动进入会话前的可选等待时长；未设置时默认在就绪后立即 attach |
| **快照恢复** | | | |
| `tmux.persistence.enabled` | bool | `false` | 启用快照保存/恢复 |
| `tmux.persistence.generation` | int | `0` | 快照代际隔离标记；重新启用快照恢复时自动递增 |
| `tmux.persistence.auto_save_mode` | string | `"hybrid"` | 自动保存策略：`hybrid`（事件+定时）、`event`（仅事件）、`interval`（仅定时） |
| `tmux.persistence.auto_save_debounce` | duration | `3s` | 事件触发保存的防抖窗口 |
| `tmux.persistence.auto_save_interval` | duration | `10m` | `interval`/`hybrid` 模式的定时保存间隔 |
| `tmux.persistence.auto_save_env` | map | `{ WORKHIVE_SAVE_LOCK_QUIET: 1, WORKHIVE_SAVE_QUALITY_QUIET: 1 }` | 注入到内部自动保存流程的环境变量；仅使用显式配置值 |
| `tmux.persistence.pre_kill_env` | map | `{ WORKHIVE_SAVE_LOCK_QUIET: 1, WORKHIVE_SAVE_QUALITY_QUIET: 1, WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1 }` | 注入到关闭前快照保存流程的环境变量；仅使用显式配置值 |
| `tmux.persistence.restore_processes` | bool | `true` | 恢复时重启面板中的进程 |
| `tmux.persistence.snapshot_history_limit` | int | `8` | 历史归档快照数量上限（不含 `wh_snapshot_latest.jsonl` 及其目标文件） |
| `tmux.persistence.snapshot_max_size_mb` | int | `8` | 快照归档总容量上限（MB，`0`=不限制） |
| `maintenance.temp_cleanup.max_age_days` | int | `7` | 自动清理超过 N 天的陈旧临时配置文件 |
| `maintenance.log.path` | string | `".workhive/logs/workhive.log"` | 运行时文件日志路径（绝对路径，或相对 Git 根目录） |
| `maintenance.test_log.path` | string | `"/dev/null"` | 测试模式文件日志路径（`/dev/null` 表示默认不落测试日志文件） |
| `maintenance.test_log.level` | string | 继承 `maintenance.log.level` | 可选测试日志级别覆盖：`debug`、`info`、`warn`、`error` |
| `maintenance.test_log.format` | string | 继承 `maintenance.log.format` | 可选测试日志格式覆盖：`text` 或单行 `json` |
| `maintenance.test_log.max_size_mb` | int | 继承 `maintenance.log.max_size_mb` | 可选测试日志单文件轮转阈值（MB） |
| `maintenance.test_log.max_backups` | int | 继承 `maintenance.log.max_backups` | 可选测试日志轮转保留份数（`0` = 截断模式） |
| `maintenance.log.level` | string | `"info"` | 文件日志级别阈值：`debug`、`info`、`warn`、`error` |
| `maintenance.log.format` | string | `"text"` | 文件日志格式：`text` 或单行 `json` |
| `maintenance.log.max_size_mb` | int | `10` | 单文件日志轮转阈值（MB） |
| `maintenance.log.max_backups` | int | `5` | 日志轮转保留份数（`0` = 截断模式） |
| **Git 初始化安全守卫（仅全局）** | | | |
| `git_init.whitelist_paths` | string[] | `[]` | 允许自动 `git init` 的目录白名单（含子目录）；非空时仅白名单内目录可触发初始化 |
| `git_init.dangerous_paths_extra` | string[] | *系统相关* | 额外禁止 `git init` 的目录列表（仅精确匹配）；默认根据操作系统自动生成（`/`、`~`、系统目录等） |
| **工具注册表** | | | |
| `tools.default` | string | `"__empty__"` | 默认工具引用（`tool_name` 或特殊值 `__empty__`、`__custom__`） |
| `tools.list` | array | 内置列表 | 全局面板工具注册表列表 |
| `tools.list[].tool_name` | string | — | 工具名称（全局唯一，用于 `set-default`） |
| `tools.list[].label` | string | `tool_name` | `workhive tools list` 中展示的标签 |
| `tools.list[].command` | string | — | 实际执行命令 |
| `tools.list[].is_ai` | bool | `false` | 是否标记为 AI 工具（用于 AI 检测/筛选） |
| `tools.list[].enabled` | bool | `true` | 是否在可选列表中启用 |

> **命令值:** 使用 `null` 表示空 Shell，或使用引号字符串如 `"claude --dangerously-skip-permissions"` 指定工具。命令的第一个词必须是已安装的可执行文件。

> **可选主题:** `catppuccin-mocha` · `tokyo-night` · `dracula` · `nord` · `gruvbox` · `rose-pine` · `everforest` · `onedark`

> `tools` 仅支持全局配置，建议使用 `workhive tools ...` 管理。
> `git_init.*` 与 `tools.*` 仅写入全局配置；项目配置不会持久化这些键。
> 推荐命令：`workhive tools list|add|edit|remove|set-default|validate|manage`。

#### Git 初始化安全守卫

workhive 在非 Git 仓库目录下运行时，可以协助自动执行 `git init`。为防止在危险目录意外初始化 Git 仓库，workhive 提供了多层安全防护：

**评估策略（按优先级执行）：**
1. **内置硬保护** — 根目录 `/` 始终硬阻断，不可覆盖
2. **`whitelist_paths`** — 若非空，白名单中的目录（及其子目录）将立即放行
3. **`dangerous_paths_extra`** — 用户可配置的精确匹配阻断目录（执行 `workhive init --global` 时会自动填充主目录/系统目录默认值）
4. **用户确认** — 通过以上检查的目录，仍需用户显式确认（输入 `y`）才会执行初始化

**配置示例：**

```yaml
# 全局配置 (~/.config/workhive/config.yaml)
git_init:
  whitelist_paths:
    - "/Users/me/projects"
    - "/Users/me/work"
  dangerous_paths_extra:
    - "/tmp"
    - "/var"
    - "/usr"
    # ... 更多系统目录（初始化时自动生成）
```

**使用 CLI 管理白名单：**

```bash
workhive config git-whitelist add ~/projects      # 添加白名单目录
workhive config git-whitelist add ~/work           # 添加另一个目录
workhive config git-whitelist remove ~/projects    # 移除白名单目录
workhive config git-whitelist list                 # 查看当前白名单
```

> **提示：** 根目录 `/` 由内置硬保护处理。`dangerous_paths_extra` 会在执行 `workhive init --global` 时自动填充默认危险目录；macOS 默认包含主目录、`/System`、`/Applications`、`/Library` 及用户文件夹，Linux 默认包含主目录及 `/boot`、`/sbin`、`/proc`、`/sys` 等。你可以在全局配置文件中手动编辑此列表。
> 若同一路径同时出现在白名单与 `dangerous_paths_extra` 中，白名单优先；唯一不可覆盖的是根目录 `/` 的硬保护。

### 布局配置 (Column-based DSL)

workhive 使用统一的 **Column-based DSL** 定义所有布局。每个布局由列数组组成，每列拥有独立的宽度和面板定义。

#### 宽高比例

`width` 和 `height` 的值为**百分比** (0-100)：


| 属性                         | 含义            | 作用范围 |
| -------------------------- | ------------- | ---- |
| `columns[].width`          | 列宽度占总宽度的百分比   | 每列   |
| `columns[].panes[].height` | 面板高度占所在列高度的百分比 | 每面板  |


> **操作步骤**: 修改 `.workhive/config.yaml` 中的 `width`/`height` 值后，使用 `--force-layout` 重建布局：
>
> ```bash
> workhive open <branch> --force-layout    # 按最新配置重建布局
> workhive attach <branch> --force-layout  # 同样效果，通过 attach 命令
> ```
>
> 其他方式：
>
> ```bash
> workhive config --layout
> workhive config --global --layout
> workhive remove <branch>     # 或者：关闭后重新打开
> workhive open <branch>
> ```

#### AI Coding 布局 (2列, 2+1 面板)

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null                    # 空 Shell
    - width: 40
      panes:
        - height: 100
          command: "lazygit"
```

#### Quad 布局 (2列, 2+2 面板)

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 70
          command: "lazygit"
        - height: 30
          command: "yazi"
```

#### Multi-Agent 布局 (2列, 4+1 面板, 带角色)

```yaml
layout:
  columns:
    - width: 50
      panes:
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent1"
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent2"
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent3"
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent4"
    - width: 50
      panes:
        - height: 100
          command: null
          role: "preview"
```

> **面板角色** 支持按名称寻址面板：`workhive pane send --role preview "preview path/to/file"`

#### 3列布局

```yaml
layout:
  columns:
    - width: 33
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null
    - width: 34
      panes:
        - height: 100
          command: "lazygit"
    - width: 33
      panes:
        - height: 50
          command: "npm run dev"
        - height: 50
          command: "npm test -- --watch"
```

#### 单列布局 (1列, 面板垂直堆叠)

```yaml
layout:
  columns:
    - width: 100
      panes:
        - height: 33
          command: "claude --dangerously-skip-permissions"
        - height: 33
          command: null
        - height: 34
          command: "lazygit"
```

### Worktree 配置

```yaml
worktree:
  base_path: ".workhive/worktrees"     # Worktree 存放路径
  auto_create: true                    # open 时自动创建
  auto_delete: true                    # remove 时自动删除
  sync_files:                          # 新建 worktree 时自动同步的文件
    - source: ".env"
      mode: "copy"                     # copy: 复制文件
    - source: "node_modules"
      mode: "symlink"                  # symlink: 创建符号链接
```

通过 `workhive open` 创建新 worktree 时，`sync_files` 中列出的文件会自动从项目根目录复制或符号链接到新目录。确保环境文件 (`.env`)、依赖 (`node_modules`) 等未被 git 跟踪的文件即刻可用。

### tmux 配置

```yaml
tmux:
  # session_prefix: null               # 运行时自动计算: wh@<instance_id>@<项目名>
  mouse_support: true                  # 鼠标支持
  interaction_profile: "workspace"     # 鼠标滚轮交互策略
  env:
    CLAUDE_CODE_NO_FLICKER: 1          # 注入到 pane 启动命令的环境变量
  attach_delay: 0s                      # 自动 attach 前等待秒数
  theme: "catppuccin-mocha"            # 主题
  persistence:
    auto_save_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
    pre_kill_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
      WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1
```

**会话命名:** workhive 会为每个项目自动生成 4 位十六进制 instance ID（存储在 `.workhive/instance_id`）。会话名格式为 `wh@<id>@<项目名>@<分支名>`；专用 tmux server 名称格式为 `workhive-<project>-<instance_id>`。如需覆盖 session 前缀，可取消 `session_prefix` 注释并填入自定义值。

**`tmux.env` / `tmux.persistence.*_env` 键规则：** key 必须匹配 `[A-Za-z_][A-Za-z0-9_]*`；value 必须是标量（字符串/数字/布尔）。合并时项目配置覆盖全局配置。如果配置文件里省略了某个 env map，workhive 运行时不会为该 map 回退注入默认环境变量。

可选主题: `catppuccin-mocha` | `tokyo-night` | `dracula` | `nord` | `gruvbox` | `rose-pine` | `everforest` | `onedark`

### 会话快照恢复

workhive 内置 tmux 快照恢复模块，可自动保存和恢复 tmux 会话结构。快照捕获窗口、面板、布局、工作目录和运行中的命令。支持**项目级**和**全局**两种配置。

> 该功能专为 tmux 设计，因此配置放在 `tmux.persistence` 命名空间下。

#### 启用快照恢复

```bash
# 方式一：交互式配置
workhive config              # 选择 "Session Persistence"（快照恢复）→ Yes

# 方式二：命令行直接设置（项目级）
workhive config set tmux.persistence.enabled true

# 方式三：全局启用（对所有项目生效）
workhive config set tmux.persistence.enabled true --global

# 在初始化时启用
workhive init                # 步骤 4 会询问是否启用
```

#### 配置参考（`config.yaml`）

```yaml
tmux:
  persistence:
    enabled: true               # 启用/禁用快照保存与恢复
    generation: 0               # 快照代际隔离标记（通常无需手动修改）
    auto_save_mode: hybrid      # 自动保存策略: hybrid / event / interval
    auto_save_debounce: 3s  # 事件保存防抖（秒）
    auto_save_interval: 10m       # 定时保存间隔（分钟，hybrid/interval 共用）
    restore_processes: true     # 恢复时是否重启面板中的进程
    snapshot_history_limit: 8   # 历史归档快照数量上限（不含 latest 指针及其目标文件）
    snapshot_max_size_mb: 8     # 归档快照总容量上限（MB）
```

#### 字段取值说明（可填写什么）

| 字段 | 类型 | 可填写值/格式 | 含义 |
|------|------|---------------|------|
| `tmux.persistence.enabled` | bool | `true` / `false` | 是否启用快照保存与恢复 |
| `tmux.persistence.generation` | int | `0` 或更大整数 | 快照代际隔离。`enabled` 从 `false` 切到 `true` 时会自动递增；通常不建议手改 |
| `tmux.persistence.auto_save_mode` | enum | `hybrid` / `event` / `interval` | `hybrid`=事件+定时，`event`=仅事件，`interval`=仅定时 |
| `tmux.persistence.auto_save_debounce` | duration | 如 `3s`、`30s`、`1m`、`2h` | 事件触发保存的防抖窗口，仅事件路径生效 |
| `tmux.persistence.auto_save_interval` | duration | 如 `1m`、`10m`、`1h` | 定时保存间隔，`interval/hybrid` 模式生效 |
| `tmux.persistence.restore_processes` | bool | `true` / `false` | 恢复会话时是否尝试重启 pane 内进程 |
| `tmux.persistence.snapshot_history_limit` | int | `>=1` | 历史归档快照保留数量上限（不含 `wh_snapshot_latest.jsonl` 及其目标文件） |
| `tmux.persistence.snapshot_max_size_mb` | int | `>=0` | 快照归档总容量上限（MB）；`0` 表示不限制大小 |

补充：
- duration 目前支持 `s` / `m` / `h` 单位（秒/分钟/小时）。
- `tmux.attach_delay` 也是 duration，常用值：`0s`、`1s`、`2s`、`5s`。

#### 保存内容

| 项目 | 保存？ | 说明 |
|------|--------|------|
| 窗口和面板 | ✅ 是 | 布局、大小、位置 |
| 工作目录 | ✅ 是 | 每个面板的当前目录 |
| 面板命令 | ✅ 是 | 运行中的命令（lazygit、claude 等）|
| 环境变量 | ❌ 否 | 不保存 shell 环境变量 |
| 面板内容/历史 | ❌ 否 | 终端滚动内容不会保存到快照；对话/输出滚动由 tmux 运行时交互策略处理 |

#### 快照存储

快照按项目存储在 `.workhive/snapshots/` 目录中。每个快照是一个 JSONL 格式文件（每行一个 JSON 对象）：

- `wh_snapshot_latest.jsonl` 指向最新快照
- 历史归档使用 `wh_snapshot_archive_<timestamp>.jsonl`
- `snapshot_history_limit` 仅统计历史归档，不包含 `wh_snapshot_latest.jsonl` 及其目标文件

#### 工作原理

```
workhive open/attach
    │
    ├── persistence.enabled = false?
    │   └── 按 config.yaml 布局创建会话 (标准流程)
    │
    └── persistence.enabled = true?
        ├── 有快照 
        │   └── 从快照恢复：重建窗口/面板/布局/工作目录
        │       └── restore_processes = true? → 重启可恢复进程
        │
        └── 无快照?
            └── 按 config.yaml 布局创建会话 (标准流程)

会话运行中:
    ├── auto_save_mode=event: 仅在 pane/window/layout 等事件变化时保存
    ├── auto_save_mode=interval: 按 auto_save_interval 定时保存
    ├── 事件触发保存受 auto_save_debounce 防抖保护
    ├── 用户 detach 时保存快照
    ├── workhive close 前保存快照
    └── auto_save_mode=hybrid: 事件触发 + 定时保存 (auto_save_interval)

快照清理:
    ├── 按 snapshot_history_limit 裁剪历史数量
    └── 按 snapshot_max_size_mb 裁剪总容量
```

**关键点：**
- 快照保存的是 tmux 会话结构（窗口、面板布局、工作目录、运行命令），不是终端内容
- 恢复时，面板会被重建到相同的工作目录，可恢复的进程会被重新启动
- 默认可恢复进程列表包括：`vim`、`nvim`、`lazygit`、`yazi`、`htop`、`top` 等
- 如果恢复失败，会自动回退到 `config.yaml` 中定义的布局

#### 查看快照恢复状态

```bash
# 在 list 命令中查看
workhive list          # 底部会显示快照恢复状态、最近快照时间

# 在 doctor 命令中检查
workhive doctor        # 显示快照模块健康状态、快照数量、最近快照

# 查看快照配置
workhive config        # 显示 enabled/auto_save_interval
```

#### 如何设置（可直接复制）

```bash
# 1) 启用快照恢复
workhive config set tmux.persistence.enabled true

# 2) 推荐的小体积配置（适合大多数项目）
workhive config set tmux.persistence.snapshot_history_limit 8
workhive config set tmux.persistence.snapshot_max_size_mb 8

# 3) 推荐自动保存策略（高可用 + 低写入）
workhive config set tmux.persistence.auto_save_mode hybrid
workhive config set tmux.persistence.auto_save_debounce 3s
workhive config set tmux.persistence.auto_save_interval 10m
```

> 如果你希望“完全只在变化时保存”，可设为：`tmux.persistence.auto_save_mode=event`。

#### 禁用快照恢复

```bash
workhive config set tmux.persistence.enabled false
```

### 场景配置示例

AI 辅助开发 (Claude + Shell + Lazygit)

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 100
          command: "lazygit"
```

---
