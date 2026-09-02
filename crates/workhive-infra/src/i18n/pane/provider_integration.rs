//! Legacy approve-hooks copy, automatic integration prepare lifecycle, and activity-record entrypoint errors.

use std::collections::HashMap;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    // Legacy `pane approve-hooks` outcome strings. The command is now a
    // deprecated no-op alias and never reaches these branches in
    // production; the entries remain only so the i18n key set stays
    // exhaustive. All copies funnel users to the same deprecation
    // notice and never reference `/hooks` or provider approval.
    m.insert(
        "pane.approve_hooks.disabled",
        "`pane approve-hooks` is deprecated and now performs no action. Workhive no longer installs provider-side hooks, so no approval is required.",
    );
    m.insert(
        "pane.approve_hooks.not_codex",
        "`pane approve-hooks` is deprecated and now performs no action. Workhive no longer installs provider-side hooks, so no approval is required.",
    );
    m.insert(
        "pane.approve_hooks.low_confidence",
        "`pane approve-hooks` is deprecated and now performs no action. Workhive no longer installs provider-side hooks, so no approval is required.",
    );
    m.insert(
        "pane.approve_hooks.already_active",
        "`pane approve-hooks` is deprecated and now performs no action. Workhive no longer installs provider-side hooks, so no approval is required.",
    );
    m.insert(
        "pane.approve_hooks.sent",
        "`pane approve-hooks` is deprecated and now performs no action. Workhive no longer installs provider-side hooks, so no approval is required.",
    );
    m.insert(
        "pane.approve_hooks.deprecated",
        "`pane approve-hooks` is deprecated and now performs no action. Workhive no longer installs provider-side hooks, so no approval is required.",
    );

    // Automatic AI integration prepare lifecycle. There is no
    // user-visible `workhive integrations` CLI; the happy path is
    // silent. Only failure / safety-refusal copy remains localized.
    m.insert(
        "auto_prepare.refused_user_plugin",
        "Refused to overwrite an existing non-Workhive plugin file at {0}. Move it aside; the next Workhive pane launch will retry.",
    );
    m.insert(
        "auto_prepare.failed",
        "Workhive could not prepare the {0} integration: {1}. The next Workhive pane launch will retry.",
    );
    // Internal `activity record` entrypoint errors.
    m.insert(
        "activity_record.invalid_json",
        "invalid activity record payload (not JSON): {0}",
    );
    m.insert(
        "activity_record.invalid_fields",
        "invalid activity record fields: {0}",
    );
    m.insert(
        "activity_record.provider_mismatch",
        "activity record provider mismatch: expected {0}, received {1}",
    );
    m.insert(
        "activity_record.write_failed",
        "failed to write activity fact: {0}",
    );
    m.insert(
        "activity_record.error.missing_runtime_root",
        "activity record: WORKHIVE_RUNTIME_ROOT must be set by the supervisor",
    );
    m.insert(
        "activity_record.error.missing_provider",
        "activity record: WORKHIVE_TOOL_PROVIDER must be set by the supervisor",
    );
    m.insert(
        "activity_record.error.unknown_provider",
        "activity record: WORKHIVE_TOOL_PROVIDER is not a recognised provider",
    );
    m.insert(
        "activity_record.error.no_provider_active",
        "activity record: this pane has no active AI provider integration; refusing to record",
    );
    m.insert(
        "activity_record.error.read_stdin",
        "activity record: failed to read JSON payload from stdin",
    );
    m.insert(
        "activity_record.error.empty_stdin",
        "activity record: stdin is empty; expected a JSON payload",
    );
    m.insert(
        "activity_record.error.invalid_json",
        "activity record: stdin payload is not valid JSON",
    );
    m.insert(
        "activity_record.error.provider_mismatch",
        "activity record: payload provider does not match the pane's tool provider",
    );
    m.insert(
        "activity_record.error.missing_group",
        "activity record: WORKHIVE_GROUP env var is not set; cannot route the fact to a pane",
    );
    m.insert(
        "activity_record.error.missing_workspace",
        "activity record: WORKHIVE_WORKSPACE env var is not set; cannot route the fact to a pane",
    );
    m.insert(
        "activity_record.error.missing_unit_id",
        "activity record: WORKHIVE_UNIT_ID env var is not set; cannot route the fact to a pane",
    );
    m.insert(
        "activity_record.error.identity_mismatch",
        "activity record: payload group/workspace/unit_id disagree with the supervisor-pinned pane identity",
    );
    m.insert(
        "activity_record.error.missing_event_field",
        "activity record: hook payload is missing the required event-name field",
    );
    m.insert(
        "activity_record.error.unhandled_event",
        "activity record: hook event is not in the supported set for this provider",
    );
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    // Legacy `pane approve-hooks` outcome strings. The command is now a
    // deprecated no-op alias and never reaches these branches in
    // production; the entries remain only so the i18n key set stays
    // exhaustive. All copies funnel users to the same deprecation
    // notice and never reference `/hooks` or provider approval.
    m.insert(
        "pane.approve_hooks.disabled",
        "`pane approve-hooks` 已废弃，本命令现在不执行任何操作。Workhive 不再安装 provider 端 hook，无需用户批准。",
    );
    m.insert(
        "pane.approve_hooks.not_codex",
        "`pane approve-hooks` 已废弃，本命令现在不执行任何操作。Workhive 不再安装 provider 端 hook，无需用户批准。",
    );
    m.insert(
        "pane.approve_hooks.low_confidence",
        "`pane approve-hooks` 已废弃，本命令现在不执行任何操作。Workhive 不再安装 provider 端 hook，无需用户批准。",
    );
    m.insert(
        "pane.approve_hooks.already_active",
        "`pane approve-hooks` 已废弃，本命令现在不执行任何操作。Workhive 不再安装 provider 端 hook，无需用户批准。",
    );
    m.insert(
        "pane.approve_hooks.sent",
        "`pane approve-hooks` 已废弃，本命令现在不执行任何操作。Workhive 不再安装 provider 端 hook，无需用户批准。",
    );
    m.insert(
        "pane.approve_hooks.deprecated",
        "`pane approve-hooks` 已废弃，本命令现在不执行任何操作。Workhive 不再安装 provider 端 hook，无需用户批准。",
    );

    // Automatic AI integration prepare lifecycle. There is no
    // user-visible `workhive integrations` CLI; the happy path is
    // silent. Only failure / safety-refusal copy remains localized.
    m.insert(
        "auto_prepare.refused_user_plugin",
        "拒绝覆盖位于 {0} 的非 Workhive 插件文件。请将其移走，Workhive 会在下一次启动相关 pane 时自动重试。",
    );
    m.insert(
        "auto_prepare.failed",
        "Workhive 未能完成 {0} 集成的准备：{1}。Workhive 会在下一次启动相关 pane 时自动重试。",
    );
    // Internal `activity record` entrypoint errors.
    m.insert(
        "activity_record.invalid_json",
        "activity record 载荷不是有效的 JSON：{0}",
    );
    m.insert(
        "activity_record.invalid_fields",
        "activity record 字段无效：{0}",
    );
    m.insert(
        "activity_record.provider_mismatch",
        "activity record provider 不匹配：期望 {0}，收到 {1}",
    );
    m.insert(
        "activity_record.write_failed",
        "写入 activity fact 失败：{0}",
    );
    m.insert(
        "activity_record.error.missing_runtime_root",
        "activity record：监管器必须设置 WORKHIVE_RUNTIME_ROOT",
    );
    m.insert(
        "activity_record.error.missing_provider",
        "activity record：监管器必须设置 WORKHIVE_TOOL_PROVIDER",
    );
    m.insert(
        "activity_record.error.unknown_provider",
        "activity record：WORKHIVE_TOOL_PROVIDER 不是受支持的 provider",
    );
    m.insert(
        "activity_record.error.no_provider_active",
        "activity record：当前 pane 未启用 AI provider 集成，拒绝记录",
    );
    m.insert(
        "activity_record.error.read_stdin",
        "activity record：从 stdin 读取 JSON 载荷失败",
    );
    m.insert(
        "activity_record.error.empty_stdin",
        "activity record：stdin 为空；需要 JSON 载荷",
    );
    m.insert(
        "activity_record.error.invalid_json",
        "activity record：stdin 载荷不是有效的 JSON",
    );
    m.insert(
        "activity_record.error.provider_mismatch",
        "activity record：载荷中的 provider 与当前 pane 的工具 provider 不一致",
    );
    m.insert(
        "activity_record.error.missing_group",
        "activity record：环境变量 WORKHIVE_GROUP 未设置，无法将事实路由到具体 pane",
    );
    m.insert(
        "activity_record.error.missing_workspace",
        "activity record：环境变量 WORKHIVE_WORKSPACE 未设置，无法将事实路由到具体 pane",
    );
    m.insert(
        "activity_record.error.missing_unit_id",
        "activity record：环境变量 WORKHIVE_UNIT_ID 未设置，无法将事实路由到具体 pane",
    );
    m.insert(
        "activity_record.error.identity_mismatch",
        "activity record：载荷中的 group/workspace/unit_id 与 supervisor 固定的 pane 身份不一致",
    );
    m.insert(
        "activity_record.error.missing_event_field",
        "activity record：hook 载荷缺少必需的事件名字段",
    );
    m.insert(
        "activity_record.error.unhandled_event",
        "activity record：该 provider 未支持此 hook 事件",
    );
}
