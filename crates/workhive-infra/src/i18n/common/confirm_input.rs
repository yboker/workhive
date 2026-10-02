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
    m.insert("units.unknown", "?");
    m.insert("bool.yes", "是");
    m.insert("bool.no", "否");
    m.insert("common.value.yes", "是");
    m.insert("common.value.no", "否");
    m.insert("status.active", "活跃");
    m.insert("status.inactive", "非活跃");
}
