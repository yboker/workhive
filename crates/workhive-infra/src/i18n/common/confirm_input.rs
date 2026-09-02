//! Confirmation prompts and shared path-input widget labels.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Confirmation
    m.insert(
        "confirm.init_global",
        "Initialize global workhive configuration?",
    );
    m.insert(
        "confirm.init_project",
        "Initialize workhive for this project?",
    );
    m.insert(
        "confirm.destroy",
        "This will permanently remove ALL workhive resources for this project. Continue?",
    );
    m.insert("confirm.generic_mutate", "Apply this change?");
    m.insert(
        "confirm.context_truncated",
        "… more safety lines hidden; enlarge the terminal to see the full report.",
    );
    m.insert(
        "confirm.context_too_small",
        "Terminal too small to show the changed-file safety counts. Enlarge the terminal before confirming.",
    );

    // The trusted-root prompt must look like a real input field with
    // an explicit scope, child labels, and a commit-preview line.
    m.insert("path_input.placeholder_home", "Browsing under ~/");
    m.insert("path_input.field_label", "Trusted root path");
    m.insert("path_input.scope_label", "Current location: {path}");
    m.insert("path_input.preview_label", "Will add: {path}");
    m.insert(
        "path_input.preview_bare_scope_warning",
        "Pick a child directory below — Workhive will not commit a bare home or root directory.",
    );
    m.insert(
        "path_input.relative_warning",
        "Trusted roots must be absolute (start with `/`) or `~/`-rooted.",
    );
    m.insert(
        "path_input.scope_no_match_warning",
        "no match — `{0}` does not exist under the current browse scope; Enter is blocked until a real path is entered. Type a full `~/...` or absolute path to commit a path that doesn't exist yet.",
    );
    m.insert(
        "path_input.added_roots_header",
        "Trusted roots added ({0}):",
    );
    m.insert(
        "path_input.added_roots_empty",
        "No trusted roots added yet.",
    );
    m.insert("path_input.added_roots_overflow", "… and {0} more");
    m.insert(
        "path_input.dir_no_children",
        "No child directories under this path.",
    );
    m.insert("path_input.validation_prefix", "Error:");
    m.insert("units.unknown", "?");
    m.insert("bool.yes", "yes");
    m.insert("bool.no", "no");
    m.insert("common.value.yes", "yes");
    m.insert("common.value.no", "no");
    m.insert("status.active", "active");
    m.insert("status.inactive", "inactive");
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    m.insert("confirm.init_global", "初始化全局 workhive 配置？");
    m.insert("confirm.init_project", "为当前项目初始化 workhive？");
    m.insert(
        "confirm.destroy",
        "此操作将永久删除该项目的所有 workhive 资源，是否继续？",
    );
    m.insert("confirm.generic_mutate", "是否应用此更改？");
    m.insert(
        "confirm.context_truncated",
        "……更多安全信息已隐藏；请放大终端以查看完整报告。",
    );
    m.insert(
        "confirm.context_too_small",
        "终端过小，无法显示变更文件安全计数。请先放大终端再确认。",
    );

    // The trusted-root prompt must look like a real input field with
    // an explicit scope, child labels, and a commit-preview line.
    m.insert("path_input.placeholder_home", "正在浏览 ~/ 下的目录");
    m.insert("path_input.field_label", "可信目录路径");
    m.insert("path_input.scope_label", "当前位置: {path}");
    m.insert("path_input.preview_label", "将添加: {path}");
    m.insert(
        "path_input.preview_bare_scope_warning",
        "请选择下面的子目录 — Workhive 不会以裸 home 或根目录作为可信目录。",
    );
    m.insert(
        "path_input.relative_warning",
        "可信目录必须是绝对路径（以 `/` 开头）或 `~/` 起始路径。",
    );
    m.insert(
        "path_input.scope_no_match_warning",
        "未找到匹配项：当前浏览范围下不存在 `{0}`；回车已被阻止。请输入完整的 `~/...` 或绝对路径以提交一个尚不存在的路径。",
    );
    m.insert("path_input.added_roots_header", "已添加的可信目录（{0}）：");
    m.insert("path_input.added_roots_empty", "尚未添加任何可信目录。");
    m.insert("path_input.added_roots_overflow", "… 还有 {0} 项");
    m.insert("path_input.dir_no_children", "该目录下没有子目录。");
    m.insert("path_input.validation_prefix", "错误：");
    m.insert("units.unknown", "?");
    m.insert("bool.yes", "是");
    m.insert("bool.no", "否");
    m.insert("common.value.yes", "是");
    m.insert("common.value.no", "否");
    m.insert("status.active", "活跃");
    m.insert("status.inactive", "非活跃");
}
