//! Static i18n adapter — compiled-in message catalog.
//!
//! English is the built-in default; Chinese (zh-CN) is the second supported locale.
//! Messages are resolved by key and locale at runtime. Missing keys fall back
//! to English, and ultimately to the key itself (ensuring we never panic).

use std::collections::HashMap;

use workhive_core::domain::Locale;
use workhive_core::ports::I18nPort;

/// Static i18n adapter that holds compiled-in message catalogs.
pub struct StaticI18n {
    en: HashMap<&'static str, &'static str>,
    zh_cn: HashMap<&'static str, &'static str>,
}

impl StaticI18n {
    /// Create a new adapter with the built-in message catalogs.
    pub fn new() -> Self {
        Self {
            en: build_en_catalog(),
            zh_cn: build_zh_cn_catalog(),
        }
    }
}

impl Default for StaticI18n {
    fn default() -> Self {
        Self::new()
    }
}

impl I18nPort for StaticI18n {
    fn resolve(&self, locale: Locale, key: &str) -> String {
        let catalog = match locale {
            Locale::ZhCn => &self.zh_cn,
            Locale::En => &self.en,
        };

        catalog
            .get(key)
            .or_else(|| self.en.get(key))
            .map(|s| (*s).to_owned())
            .unwrap_or_else(|| key.to_owned())
    }

    fn resolve_with_args(&self, locale: Locale, key: &str, args: &[&str]) -> String {
        let template = self.resolve(locale, key);
        let mut result = template;
        for (i, arg) in args.iter().enumerate() {
            result = result.replace(&format!("{{{i}}}"), arg);
        }
        result
    }
}

fn build_en_catalog() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    common::register_en(&mut m);
    init_wizard::register_en(&mut m);
    project_config::register_en(&mut m);
    workspace_session::register_en(&mut m);
    pane::register_en(&mut m);
    doctor_completion::register_en(&mut m);
    update::register_en(&mut m);
    m
}

fn build_zh_cn_catalog() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    common::register_zh_cn(&mut m);
    init_wizard::register_zh_cn(&mut m);
    project_config::register_zh_cn(&mut m);
    workspace_session::register_zh_cn(&mut m);
    pane::register_zh_cn(&mut m);
    doctor_completion::register_zh_cn(&mut m);
    update::register_zh_cn(&mut m);
    m
}

mod common;
mod doctor_completion;
mod init_wizard;
mod pane;
mod project_config;
mod update;
mod workspace_session;

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::i18n_keys;

    #[test]
    fn en_key_resolves() {
        let i18n = StaticI18n::new();
        let msg = i18n.resolve(Locale::En, i18n_keys::INIT_GLOBAL_SUCCESS);
        assert!(msg.contains("Global configuration initialized"));
    }

    #[test]
    fn zh_cn_key_resolves() {
        let i18n = StaticI18n::new();
        let msg = i18n.resolve(Locale::ZhCn, i18n_keys::INIT_GLOBAL_SUCCESS);
        assert!(msg.contains("全局配置已初始化"));
    }

    #[test]
    fn fallback_to_en_for_missing_zh_key() {
        let i18n = StaticI18n::new();
        let msg = i18n.resolve(Locale::ZhCn, "nonexistent.key.for.testing");
        assert_eq!(msg, "nonexistent.key.for.testing");
    }

    #[test]
    fn resolve_with_args_replaces_placeholders() {
        let i18n = StaticI18n::new();
        let msg = i18n.resolve_with_args(
            Locale::En,
            i18n_keys::OPEN_CREATING_SESSION,
            &["wh--myproj--feat"],
        );
        assert_eq!(msg, "Creating runtime session 'wh--myproj--feat'...");
    }

    #[test]
    fn open_project_init_prompt_uses_neutral_wording_in_english() {
        let i18n = StaticI18n::new();
        let msg = i18n.resolve(Locale::En, i18n_keys::OPEN_PROJECT_INIT_PROMPT);
        assert_eq!(msg, "Project is not initialized. Initialize now?");
    }

    #[test]
    fn open_project_init_prompt_uses_neutral_wording_in_chinese() {
        let i18n = StaticI18n::new();
        let msg = i18n.resolve(Locale::ZhCn, i18n_keys::OPEN_PROJECT_INIT_PROMPT);
        assert_eq!(msg, "项目还未初始化，是否现在进行初始化？");
    }

    /// The non-interactive `workspace open` selector failure must resolve
    /// to actionable localized guidance in both catalogs — this line is
    /// what an agent/script sees when it forgets the workspace name.
    #[test]
    fn open_selector_non_interactive_has_en_and_zh_cn_guidance() {
        let i18n = StaticI18n::new();
        for locale in [Locale::En, Locale::ZhCn] {
            let msg = i18n.resolve(locale, i18n_keys::OPEN_SELECTOR_NON_INTERACTIVE);
            assert_ne!(
                msg,
                i18n_keys::OPEN_SELECTOR_NON_INTERACTIVE,
                "key must resolve in {locale:?}"
            );
            assert!(
                msg.contains("workhive workspace open <name> --no-attach"),
                "guidance must show the explicit-name invocation ({locale:?}): {msg}"
            );
        }
    }

    #[test]
    fn all_en_keys_have_entries() {
        let i18n = StaticI18n::new();
        let keys = [
            i18n_keys::CONFIRM_INIT_GLOBAL,
            i18n_keys::CONFIRM_INIT_PROJECT,
            i18n_keys::CONFIRM_DESTROY,
            i18n_keys::CONFIRM_GENERIC_MUTATE,
            i18n_keys::CONFIRM_CONTEXT_TRUNCATED,
            i18n_keys::CONFIRM_CONTEXT_TOO_SMALL,
            i18n_keys::INIT_GLOBAL_SUCCESS,
            i18n_keys::INIT_PROJECT_SUCCESS,
            i18n_keys::INIT_ALREADY_EXISTS,
            i18n_keys::INIT_SELECT_LANGUAGE,
            i18n_keys::INIT_LANG_ENGLISH,
            i18n_keys::INIT_LANG_ZH_CN,
            i18n_keys::INIT_GUIDE_GLOBAL,
            i18n_keys::CONFIG_SHOW_HEADER,
            i18n_keys::CONFIG_EDIT_ASK,
            i18n_keys::CONFIG_EDIT_SUCCESS,
            i18n_keys::CONFIG_SET_PREVIEW,
            i18n_keys::CONFIG_SET_SUCCESS,
            i18n_keys::CONFIG_UNSET_SUCCESS,
            i18n_keys::CONFIG_ENV_SET_PREVIEW,
            i18n_keys::CONFIG_ENV_SET_SUCCESS,
            i18n_keys::CONFIG_ENV_UNSET_PREVIEW,
            i18n_keys::CONFIG_ENV_UNSET_SUCCESS,
            i18n_keys::OPEN_CREATING_WORKTREE,
            i18n_keys::OPEN_CREATING_SESSION,
            i18n_keys::OPEN_REUSING_SESSION,
            i18n_keys::OPEN_INITIAL_PANE_NOT_FOUND,
            i18n_keys::OPEN_SUCCESS,
            i18n_keys::OPEN_REPLACEMENT_CLEANUP_STAGED,
            i18n_keys::OPEN_REPLACEMENT_CLEANUP_PROMOTION,
            i18n_keys::OPEN_REPLACEMENT_CLEANUP_CHECKOUT,
            i18n_keys::OPEN_REPLACEMENT_CLEANUP_PREVIOUS,
            i18n_keys::OPEN_RUNTIME_IDENTIFIERS_EMPTY,
            i18n_keys::OPEN_TMUX_STAGED_BINDING_MISSING,
            i18n_keys::ATTACH_GUIDANCE,
            i18n_keys::ATTACH_ALREADY_CURRENT,
            i18n_keys::ATTACH_CROSS_GROUP_REJECTED,
            i18n_keys::ATTACH_PARTIAL_CONTEXT_ERROR,
            i18n_keys::ATTACH_NO_SESSIONS,
            i18n_keys::ATTACH_SELECT_SESSION,
            i18n_keys::ATTACH_CANNOT_EXTRACT_WORKSPACE,
            i18n_keys::CLOSE_DETACHED,
            i18n_keys::CLOSE_KILLED,
            i18n_keys::LIST_HEADER,
            i18n_keys::LIST_BACKEND,
            i18n_keys::LIST_EMPTY,
            i18n_keys::LIST_COL_WORKSPACE,
            i18n_keys::LIST_COL_STATUS,
            i18n_keys::LIST_COL_PATH,
            i18n_keys::LIST_COL_RECENT,
            i18n_keys::LIST_NO_RECORD,
            i18n_keys::LAYOUT_AI_CODING_LEFT,
            i18n_keys::LAYOUT_AI_CODING_DUAL,
            i18n_keys::LAYOUT_HALF_SPLIT,
            i18n_keys::LAYOUT_QUAD,
            i18n_keys::LAYOUT_SINGLE,
            i18n_keys::LAYOUT_CUSTOM,
            i18n_keys::DESTROY_SUCCESS,
            i18n_keys::DESTROY_WARNING,
            i18n_keys::DESTROY_SAFETY_REPORT_HEADING,
            i18n_keys::DESTROY_SAFETY_NO_RISK,
            i18n_keys::DESTROY_RISKY_WORKTREES_CONFIRM,
            i18n_keys::DESTROY_FORCE_DESPITE_RISK,
            i18n_keys::DESTROY_NOT_INITIALIZED,
            i18n_keys::DESTROY_PARTIAL_INIT,
            i18n_keys::SAFETY_REPORT_CHANGED_FILES,
            i18n_keys::SAFETY_REPORT_MORE_CHANGED_FILES,
            i18n_keys::SAFETY_REPORT_UNMERGED_COMMITS,
            i18n_keys::WORKSPACE_REMOVE_CONFIRM,
            i18n_keys::WORKSPACE_REMOVE_SUCCESS,
            i18n_keys::WORKSPACE_REMOVE_BRANCH_KEPT,
            i18n_keys::WORKSPACE_REMOVE_MAIN_FORBIDDEN,
            i18n_keys::WORKSPACE_REMOVE_NOT_MANAGED,
            i18n_keys::WORKSPACE_REMOVE_SAFETY_BLOCKED,
            i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
            i18n_keys::WORKSPACE_REMOVE_RISKY_CONFIRM,
            i18n_keys::PANE_SPLIT_SUCCESS,
            i18n_keys::PANE_CLOSE_SUCCESS,
            i18n_keys::PANE_SEND_SUCCESS,
            i18n_keys::PANE_CONTROL_PLANE_UNAVAILABLE,
            i18n_keys::PANE_CONTROL_PLANE_UNKNOWN_PANE,
            i18n_keys::OPEN_SUMMARY_SUPERVISOR_DEGRADED,
            i18n_keys::PANE_LABEL_ID,
            i18n_keys::PANE_LABEL_CWD,
            i18n_keys::PANE_LABEL_COMMAND,
            i18n_keys::PANE_LABEL_KIND,
            i18n_keys::PANE_LABEL_STATE,
            i18n_keys::STATUS_ACTIVE,
            i18n_keys::STATUS_INACTIVE,
            i18n_keys::ERROR_NOT_IN_SESSION,
            i18n_keys::ERROR_SESSION_NOT_FOUND,
            i18n_keys::ERROR_NOT_INITIALIZED,
            i18n_keys::ERROR_GLOBAL_NOT_INITIALIZED,
            // Dependency checks
            i18n_keys::DEP_CHECKING,
            i18n_keys::DEP_ALL_OK,
            i18n_keys::DEP_SATISFIED,
            i18n_keys::DEP_MISSING,
            i18n_keys::DEP_VERSION_LOW,
            i18n_keys::DEP_BUILTIN,
            i18n_keys::DEP_INSTALL_HINT,
            i18n_keys::DEP_FATAL_MISSING,
            i18n_keys::DEP_GIT_NAME,
            i18n_keys::DEP_TMUX_NAME,
            // Custom layout
            i18n_keys::INIT_LAYOUT_CUSTOM,
            i18n_keys::INIT_CUSTOM_DESC,
            i18n_keys::INIT_CUSTOM_ROWS,
            i18n_keys::INIT_CUSTOM_COLS,
            i18n_keys::INIT_CUSTOM_COLS_DYNAMIC,
            i18n_keys::INIT_CUSTOM_ROWS_DYNAMIC,
            i18n_keys::INIT_CUSTOM_COL_WIDTH,
            i18n_keys::INIT_CUSTOM_COL_ROWS,
            // Intercept
            i18n_keys::INTERCEPT_RERUN_HINT,
            i18n_keys::INTERCEPT_PROJECT_INIT_PROMPT,
            // Open: project init
            i18n_keys::OPEN_PROJECT_INIT_PROMPT,
            i18n_keys::OPEN_PROJECT_INIT_DENIED,
            i18n_keys::OPEN_PROJECT_INIT_DECLINED,
            i18n_keys::OPEN_PROJECT_PARTIAL_INIT,
            // Git init
            i18n_keys::GIT_NOT_INITIALIZED,
            i18n_keys::GIT_INIT_PROMPT,
            i18n_keys::GIT_INIT_SUCCESS,
            i18n_keys::GIT_INIT_ABORT,
            // Validation
            i18n_keys::VALIDATION_INVALID_NUMBER,
            i18n_keys::VALIDATION_COLUMNS_RANGE,
            i18n_keys::VALIDATION_INVALID_WIDTH,
            i18n_keys::VALIDATION_INVALID_ROWS,
            i18n_keys::VALIDATION_ROWS_RANGE,
            // Config diff
            i18n_keys::CONFIG_DIFF_BEFORE,
            i18n_keys::CONFIG_DIFF_AFTER,
            // Pane presets
            i18n_keys::PANE_PRESET_CLAUDE,
            i18n_keys::PANE_PRESET_CODEX,
            i18n_keys::PANE_PRESET_OPENCODE,
            i18n_keys::PANE_PRESET_LAZYGIT,
            i18n_keys::PANE_PRESET_YAZI,
            i18n_keys::PANE_PRESET_HTOP,
            i18n_keys::PANE_PRESET_BOTTOM,
            // Reindex
            i18n_keys::REINDEX_CONFIRM,
            i18n_keys::REINDEX_SCANNING,
            i18n_keys::REINDEX_FOUND,
            i18n_keys::REINDEX_UPDATED,
            i18n_keys::REINDEX_DRY_RUN,
            i18n_keys::REINDEX_NO_ROOTS,
            i18n_keys::REINDEX_DISCOVERED,
            i18n_keys::REINDEX_UPSERTED,
            i18n_keys::REINDEX_RETAINED_EXISTING,
            i18n_keys::REINDEX_FINAL_INDEX_TOTAL,
            i18n_keys::REINDEX_DRY_RUN_HEADING,
            // Projects
            i18n_keys::PROJECTS_HEADER,
            i18n_keys::PROJECTS_EMPTY,
            i18n_keys::PROJECTS_NO_INDEX,
            i18n_keys::PROJECTS_COL_NAME,
            i18n_keys::PROJECTS_COL_ROOT,
            i18n_keys::PROJECTS_COL_INSTANCE_ID,
            // Sessions
            i18n_keys::SESSIONS_HEADER,
            i18n_keys::SESSIONS_EMPTY,
            i18n_keys::SESSIONS_NO_INDEX,
            i18n_keys::SESSIONS_COL_PROJECT,
            i18n_keys::SESSIONS_COL_BACKEND,
            i18n_keys::SESSIONS_COL_WORKSPACE,
            i18n_keys::SESSIONS_COL_SESSION,
            i18n_keys::SESSIONS_COL_ATTACHED,
            i18n_keys::SESSIONS_COL_PATH,
            i18n_keys::SESSIONS_COL_CLIENT,
            i18n_keys::SESSIONS_ATTACHED_YES,
            i18n_keys::SESSIONS_ATTACHED_NO,
            // Session attach (global)
            i18n_keys::SESSION_ATTACH_NO_INDEX,
            i18n_keys::SESSION_ATTACH_NO_SESSIONS,
            i18n_keys::SESSION_ATTACH_AMBIGUOUS,
            i18n_keys::SESSION_ATTACH_NOT_FOUND,
            i18n_keys::SESSION_ATTACH_SELECT,
            i18n_keys::SESSION_ATTACH_NO_TTY,
            i18n_keys::SESSION_ATTACH_HINT_GLOBAL,
            // Worktree command group
            i18n_keys::WORKTREE_LIST_HEADER,
            i18n_keys::WORKTREE_LIST_EMPTY,
            i18n_keys::WORKTREE_COL_NAME,
            i18n_keys::WORKTREE_COL_BRANCH,
            i18n_keys::WORKTREE_COL_PATH,
            i18n_keys::WORKTREE_COL_KIND,
            i18n_keys::WORKTREE_KIND_MAIN,
            i18n_keys::WORKTREE_KIND_LINKED,
            i18n_keys::WORKTREE_REMOVE_NO_TTY,
            i18n_keys::WORKTREE_REMOVE_NONE_AVAILABLE,
            i18n_keys::WORKTREE_REMOVE_SELECT,
            i18n_keys::WORKTREE_REMOVE_CONFIRM,
            i18n_keys::WORKTREE_REMOVE_SUCCESS,
            i18n_keys::WORKTREE_REMOVE_NOT_FOUND,
            i18n_keys::WORKTREE_REMOVE_BLOCKED_RISKY,
            i18n_keys::WORKTREE_REMOVE_NOT_MANAGED,
            i18n_keys::WORKTREE_REMOVE_BRANCH_DELETED,
            i18n_keys::WORKTREE_REMOVE_BRANCH_DELETE_FAILED,
            // CLI help: command groups
            i18n_keys::CLI_HELP_CMD_SESSION_ATTACH,
            i18n_keys::CLI_HELP_CMD_RUNTIME,
            i18n_keys::CLI_HELP_CMD_RUNTIME_STOP,
            i18n_keys::CLI_ARG_RUNTIME_FORCE,
            i18n_keys::CLI_PARSE_BACKEND_UNAVAILABLE,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_LIST,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_ADD,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_REMOVE,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_EDIT,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_STATUS,
            i18n_keys::CLI_ARG_WORKTREE_NAME_OPT,
            i18n_keys::CLI_ARG_WORKTREE_FORCE,
            i18n_keys::CLI_ARG_WORKTREE_DELETE_BRANCH,
            i18n_keys::CLI_ARG_SESSION_LIST_VIEW,
            i18n_keys::CLI_ARG_SESSION_ATTACH_TARGET,
            i18n_keys::CLI_ARG_PROJECT_ROOTS_PATH,
            // Attach global selector
            i18n_keys::ATTACH_GLOBAL_SELECT,
            i18n_keys::ATTACH_GLOBAL_NO_INDEX,
            i18n_keys::ATTACH_GLOBAL_NO_SESSIONS,
            i18n_keys::ATTACH_KIND_PROJECT_ROOT,
            i18n_keys::ATTACH_KIND_WORKTREE,
            i18n_keys::ATTACH_KIND_UNKNOWN,
            i18n_keys::ATTACH_MARKER_ATTACHED,
            // Orchestration mode summary
            i18n_keys::OPEN_MODE_MANUAL,
            i18n_keys::OPEN_MODE_PRIMARY_ACTIVE,
            i18n_keys::OPEN_MODE_PRIMARY_DEGRADED,
            i18n_keys::OPEN_MODE_PRIMARY_FAILED,
            // Doctor
            i18n_keys::DOCTOR_HEADER,
            i18n_keys::DOCTOR_CATEGORY_REMOTE,
            i18n_keys::DOCTOR_STATUS_PASS,
            i18n_keys::DOCTOR_STATUS_WARN,
            i18n_keys::DOCTOR_STATUS_FAIL,
            i18n_keys::DOCTOR_CHECK_RUNTIME_BACKEND,
            i18n_keys::DOCTOR_CHECK_RUNTIME_SCOPE,
            i18n_keys::DOCTOR_CHECK_RUNTIME_BINDING,
            i18n_keys::DOCTOR_CHECK_SESSION_LOOKUP,
            i18n_keys::DOCTOR_CHECK_PTY_AVAILABILITY,
            i18n_keys::DOCTOR_CHECK_ATTACH_READINESS,
            i18n_keys::DOCTOR_ADVICE_RUNTIME_ERROR,
            i18n_keys::DOCTOR_ADVICE_RUNTIME_BINDING_MISMATCH,
            i18n_keys::DOCTOR_ADVICE_LOOKUP_FAILED,
            i18n_keys::DOCTOR_ADVICE_PTY_EXHAUSTED,
            i18n_keys::DOCTOR_ADVICE_PTY_ELEVATED,
            i18n_keys::DOCTOR_ADVICE_NESTED_TMUX,
            i18n_keys::DOCTOR_ALL_PASSED,
            i18n_keys::DOCTOR_HAS_ISSUES,
            // List: session state
            i18n_keys::LIST_SESSION_RUNNING,
            i18n_keys::LIST_SESSION_MISSING,
            i18n_keys::LIST_SESSION_UNREACHABLE,
            i18n_keys::LIST_SESSION_LOOKUP_FAILED,
            i18n_keys::LIST_COL_SESSION_STATE,
            // Error: session lookup
            i18n_keys::ERROR_SESSION_LOOKUP_RUNNING,
            i18n_keys::ERROR_SESSION_LOOKUP_MISSING,
            i18n_keys::ERROR_SESSION_LOOKUP_UNREACHABLE,
            i18n_keys::ERROR_SESSION_LOOKUP_FAILED,
            // Open: recovery path
            i18n_keys::OPEN_RECOVERY_LIVE_SESSION,
            i18n_keys::OPEN_RECOVERY_LAYOUT,
        ];
        for key in keys {
            let msg = i18n.resolve(Locale::En, key);
            assert_ne!(msg, key, "EN catalog missing key: {key}");
        }
    }

    #[test]
    fn all_zh_cn_keys_have_entries() {
        let i18n = StaticI18n::new();
        let keys = [
            i18n_keys::CONFIRM_INIT_GLOBAL,
            i18n_keys::CONFIRM_CONTEXT_TRUNCATED,
            i18n_keys::CONFIRM_CONTEXT_TOO_SMALL,
            i18n_keys::INIT_GLOBAL_SUCCESS,
            i18n_keys::OPEN_CREATING_SESSION,
            i18n_keys::OPEN_REPLACEMENT_CLEANUP_STAGED,
            i18n_keys::OPEN_REPLACEMENT_CLEANUP_PROMOTION,
            i18n_keys::OPEN_REPLACEMENT_CLEANUP_CHECKOUT,
            i18n_keys::OPEN_REPLACEMENT_CLEANUP_PREVIOUS,
            i18n_keys::OPEN_RUNTIME_IDENTIFIERS_EMPTY,
            i18n_keys::OPEN_TMUX_STAGED_BINDING_MISSING,
            i18n_keys::LIST_HEADER,
            i18n_keys::DESTROY_SUCCESS,
            i18n_keys::DESTROY_SAFETY_REPORT_HEADING,
            i18n_keys::DESTROY_SAFETY_NO_RISK,
            i18n_keys::DESTROY_RISKY_WORKTREES_CONFIRM,
            i18n_keys::DESTROY_FORCE_DESPITE_RISK,
            i18n_keys::DESTROY_NOT_INITIALIZED,
            i18n_keys::DESTROY_PARTIAL_INIT,
            i18n_keys::SAFETY_REPORT_CHANGED_FILES,
            i18n_keys::SAFETY_REPORT_MORE_CHANGED_FILES,
            i18n_keys::SAFETY_REPORT_UNMERGED_COMMITS,
            i18n_keys::WORKSPACE_REMOVE_CONFIRM,
            i18n_keys::WORKSPACE_REMOVE_SUCCESS,
            i18n_keys::WORKSPACE_REMOVE_BRANCH_KEPT,
            i18n_keys::WORKSPACE_REMOVE_MAIN_FORBIDDEN,
            i18n_keys::WORKSPACE_REMOVE_NOT_MANAGED,
            i18n_keys::WORKSPACE_REMOVE_SAFETY_BLOCKED,
            i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
            i18n_keys::WORKSPACE_REMOVE_RISKY_CONFIRM,
            i18n_keys::PANE_SPLIT_SUCCESS,
            i18n_keys::ERROR_NOT_IN_SESSION,
            // Orchestration mode summary
            i18n_keys::OPEN_MODE_MANUAL,
            i18n_keys::OPEN_MODE_PRIMARY_ACTIVE,
            i18n_keys::OPEN_MODE_PRIMARY_DEGRADED,
            i18n_keys::OPEN_MODE_PRIMARY_FAILED,
            // Doctor
            i18n_keys::DOCTOR_HEADER,
            i18n_keys::DOCTOR_ALL_PASSED,
            i18n_keys::DOCTOR_HAS_ISSUES,
            // List: session state
            i18n_keys::LIST_SESSION_RUNNING,
            i18n_keys::LIST_SESSION_MISSING,
            i18n_keys::LIST_COL_SESSION_STATE,
            // Error: session lookup
            i18n_keys::ERROR_SESSION_LOOKUP_RUNNING,
            i18n_keys::ERROR_SESSION_LOOKUP_MISSING,
            // Open: recovery path
            i18n_keys::OPEN_RECOVERY_LIVE_SESSION,
            i18n_keys::OPEN_RECOVERY_LAYOUT,
            // Reindex (refined wording)
            i18n_keys::REINDEX_DISCOVERED,
            i18n_keys::REINDEX_UPSERTED,
            i18n_keys::REINDEX_RETAINED_EXISTING,
            i18n_keys::REINDEX_FINAL_INDEX_TOTAL,
            i18n_keys::REINDEX_DRY_RUN_HEADING,
            // Sessions (enhanced view + attach)
            i18n_keys::SESSIONS_COL_PATH,
            i18n_keys::SESSIONS_COL_CLIENT,
            i18n_keys::SESSIONS_ATTACHED_YES,
            i18n_keys::SESSIONS_ATTACHED_NO,
            i18n_keys::SESSION_ATTACH_NO_INDEX,
            i18n_keys::SESSION_ATTACH_NO_SESSIONS,
            i18n_keys::SESSION_ATTACH_AMBIGUOUS,
            i18n_keys::SESSION_ATTACH_NOT_FOUND,
            i18n_keys::SESSION_ATTACH_SELECT,
            i18n_keys::SESSION_ATTACH_NO_TTY,
            i18n_keys::SESSION_ATTACH_HINT_GLOBAL,
            // Worktree command group
            i18n_keys::WORKTREE_LIST_HEADER,
            i18n_keys::WORKTREE_LIST_EMPTY,
            i18n_keys::WORKTREE_COL_NAME,
            i18n_keys::WORKTREE_COL_BRANCH,
            i18n_keys::WORKTREE_COL_PATH,
            i18n_keys::WORKTREE_COL_KIND,
            i18n_keys::WORKTREE_KIND_MAIN,
            i18n_keys::WORKTREE_KIND_LINKED,
            i18n_keys::WORKTREE_REMOVE_NO_TTY,
            i18n_keys::WORKTREE_REMOVE_NONE_AVAILABLE,
            i18n_keys::WORKTREE_REMOVE_SELECT,
            i18n_keys::WORKTREE_REMOVE_CONFIRM,
            i18n_keys::WORKTREE_REMOVE_SUCCESS,
            i18n_keys::WORKTREE_REMOVE_NOT_FOUND,
            i18n_keys::WORKTREE_REMOVE_BLOCKED_RISKY,
            i18n_keys::WORKTREE_REMOVE_NOT_MANAGED,
            i18n_keys::WORKTREE_REMOVE_BRANCH_DELETED,
            i18n_keys::WORKTREE_REMOVE_BRANCH_DELETE_FAILED,
            // CLI help: command groups
            i18n_keys::CLI_HELP_CMD_SESSION_ATTACH,
            i18n_keys::CLI_HELP_CMD_RUNTIME,
            i18n_keys::CLI_HELP_CMD_RUNTIME_STOP,
            i18n_keys::CLI_ARG_RUNTIME_FORCE,
            i18n_keys::CLI_PARSE_BACKEND_UNAVAILABLE,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_LIST,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_ADD,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_REMOVE,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_EDIT,
            i18n_keys::CLI_HELP_CMD_PROJECT_ROOTS_STATUS,
            i18n_keys::CLI_ARG_WORKTREE_NAME_OPT,
            i18n_keys::CLI_ARG_WORKTREE_FORCE,
            i18n_keys::CLI_ARG_WORKTREE_DELETE_BRANCH,
            i18n_keys::CLI_ARG_SESSION_LIST_VIEW,
            i18n_keys::CLI_ARG_SESSION_ATTACH_TARGET,
            i18n_keys::CLI_ARG_PROJECT_ROOTS_PATH,
        ];
        for key in keys {
            let msg = i18n.resolve(Locale::ZhCn, key);
            assert_ne!(msg, key, "ZH-CN catalog missing key: {key}");
        }
    }

    #[test]
    fn no_user_facing_message_references_open_force() {
        let en = build_en_catalog();
        let zh = build_zh_cn_catalog();
        for (key, msg) in en.iter().chain(zh.iter()) {
            assert!(
                !msg.contains("open --force") && !msg.contains("open -f"),
                "i18n key '{key}' still references removed 'open --force': {msg}"
            );
        }
    }

    #[test]
    fn trusted_roots_confirm_keys_render_project_root_in_both_locales() {
        // The first-open/init trust prompt and its post-confirmation line
        // must interpolate the project root so the user always sees the
        // exact directory being trusted.
        let i18n = StaticI18n::new();
        for locale in [Locale::En, Locale::ZhCn] {
            let ask = i18n.resolve_with_args(
                locale,
                i18n_keys::TRUSTED_ROOTS_CONFIRM_ASK,
                &["/Users/me/proj"],
            );
            assert!(
                ask.contains("/Users/me/proj"),
                "{locale:?} trust prompt must show the project root: {ask}"
            );
            let added = i18n.resolve_with_args(
                locale,
                i18n_keys::TRUSTED_ROOTS_CONFIRM_ADDED,
                &["/Users/me/proj"],
            );
            assert!(
                added.contains("/Users/me/proj"),
                "{locale:?} trust-added line must show the persisted root: {added}"
            );
        }
    }

    #[test]
    fn pane_ai_activity_open_unverified_avoids_telemetry_wording_in_both_locales() {
        // The default `pane list` "Activity" cell for an AI pane that is
        // visible in the foreground but has not yet recorded a completion
        // signal MUST NOT use telemetry vocabulary like "activity
        // unverified" in either locale. The chosen wording explains the same
        // state ("AI tool is open in this pane and Workhive is still
        // waiting for the next completion signal") in a way the user can
        // act on. It MUST NOT claim the pane is processing, busy,
        // running, broken, or uninstalled, because PID liveness is not
        // proof of any of those states.
        let i18n = StaticI18n::new();

        let en = i18n.resolve(Locale::En, i18n_keys::PANE_AI_ACTIVITY_OPEN_UNVERIFIED);
        assert!(
            en.contains("open") && en.contains("waiting") && en.contains("signal"),
            "EN OpenUnverified label must read as `open (waiting for signal)`-style copy: {en}"
        );
        for forbidden in [
            "activity unverified",
            "unverified",
            "processing",
            "running",
            "busy",
            "error",
        ] {
            assert!(
                !en.contains(forbidden),
                "EN OpenUnverified label must not regress to telemetry/busy wording `{forbidden}`: {en}"
            );
        }

        let zh = i18n.resolve(Locale::ZhCn, i18n_keys::PANE_AI_ACTIVITY_OPEN_UNVERIFIED);
        assert!(
            zh.contains("打开") && zh.contains("等待") && zh.contains("信号"),
            "ZH OpenUnverified label must read as `已打开（等待信号）`-style copy: {zh}"
        );
        for forbidden in ["活动未验证", "未验证", "处理中", "运行中", "繁忙", "出错"]
        {
            assert!(
                !zh.contains(forbidden),
                "ZH OpenUnverified label must not regress to telemetry/busy wording `{forbidden}`: {zh}"
            );
        }
    }

    #[test]
    fn pane_list_legend_activity_unverified_quotes_the_user_facing_label_in_both_locales() {
        // `pane list` renders only the five concise activity labels.
        // The legend must enumerate them in both locales (so users
        // can search for what they saw in the cell) and must direct
        // them to `pane show` for the underlying source / confidence
        // / reason. The legend must never re-introduce verbose,
        // parenthesized AI-specific wording.
        let i18n = StaticI18n::new();
        let legend_en = i18n.resolve(Locale::En, i18n_keys::PANE_LIST_LEGEND_ACTIVITY_UNVERIFIED);
        let en_waiting = i18n.resolve(Locale::En, i18n_keys::PANE_CONCISE_ACTIVITY_WAITING_INPUT);
        let en_running = i18n.resolve(Locale::En, i18n_keys::PANE_CONCISE_ACTIVITY_RUNNING);
        let en_completed = i18n.resolve(Locale::En, i18n_keys::PANE_CONCISE_ACTIVITY_COMPLETED);
        let en_exited = i18n.resolve(Locale::En, i18n_keys::PANE_CONCISE_ACTIVITY_EXITED);
        let en_unknown = i18n.resolve(Locale::En, i18n_keys::PANE_CONCISE_ACTIVITY_UNKNOWN);
        for token in [
            &en_waiting,
            &en_running,
            &en_completed,
            &en_exited,
            &en_unknown,
        ] {
            assert!(
                legend_en.contains(token.as_str()),
                "EN legend must enumerate concise label `{token}`: {legend_en}"
            );
        }
        assert!(
            legend_en.contains("pane show"),
            "EN legend must point users at `pane show` for detail: {legend_en}"
        );
        assert!(
            !legend_en.contains("(waiting for signal)"),
            "EN legend must not regress to the verbose AI-specific wording: {legend_en}"
        );

        let legend_zh = i18n.resolve(
            Locale::ZhCn,
            i18n_keys::PANE_LIST_LEGEND_ACTIVITY_UNVERIFIED,
        );
        let zh_waiting = i18n.resolve(Locale::ZhCn, i18n_keys::PANE_CONCISE_ACTIVITY_WAITING_INPUT);
        let zh_running = i18n.resolve(Locale::ZhCn, i18n_keys::PANE_CONCISE_ACTIVITY_RUNNING);
        let zh_completed = i18n.resolve(Locale::ZhCn, i18n_keys::PANE_CONCISE_ACTIVITY_COMPLETED);
        let zh_exited = i18n.resolve(Locale::ZhCn, i18n_keys::PANE_CONCISE_ACTIVITY_EXITED);
        let zh_unknown = i18n.resolve(Locale::ZhCn, i18n_keys::PANE_CONCISE_ACTIVITY_UNKNOWN);
        for token in [
            &zh_waiting,
            &zh_running,
            &zh_completed,
            &zh_exited,
            &zh_unknown,
        ] {
            assert!(
                legend_zh.contains(token.as_str()),
                "ZH legend must enumerate concise label `{token}`: {legend_zh}"
            );
        }
        assert!(
            legend_zh.contains("pane show"),
            "ZH legend must point users at `pane show` for detail: {legend_zh}"
        );
        assert!(
            !legend_zh.contains("等待信号"),
            "ZH legend must not regress to the verbose AI-specific wording: {legend_zh}"
        );
    }

    #[test]
    fn open_project_root_wording_en_immediate_is_truthful() {
        let i18n = StaticI18n::new();
        let en = i18n.resolve(Locale::En, i18n_keys::OPEN_MAIN_DIR_MODE);
        assert!(
            !en.contains("main branch"),
            "EN immediate must not claim project root is the main branch: {en}"
        );
        assert_eq!(en, "Using project root (current checked-out branch)",);
    }

    #[test]
    fn open_project_root_wording_zh_immediate_is_truthful() {
        let i18n = StaticI18n::new();
        let zh = i18n.resolve(Locale::ZhCn, i18n_keys::OPEN_MAIN_DIR_MODE);
        assert!(
            !zh.contains("\u{4E3B}\u{5206}\u{652F}"),
            "ZH immediate must not claim project root is the main branch: {zh}"
        );
        assert_eq!(
            zh,
            "\u{4F7F}\u{7528}\u{9879}\u{76EE}\u{6839}\u{76EE}\u{5F55}\u{FF08}\u{5F53}\u{524D}\u{68C0}\u{51FA}\u{5206}\u{652F}\u{FF09}",
        );
    }

    // ── Attach navigation localization tests ──────────────────────────

    #[test]
    fn attach_navigation_messages_exist_in_all_locales() {
        let i18n = StaticI18n::new();
        for locale in [Locale::En, Locale::ZhCn] {
            let already = i18n.resolve(locale, i18n_keys::ATTACH_ALREADY_CURRENT);
            assert!(
                !already.is_empty() && already != i18n_keys::ATTACH_ALREADY_CURRENT,
                "{locale:?}: ATTACH_ALREADY_CURRENT must resolve"
            );
            let cross = i18n.resolve(locale, i18n_keys::ATTACH_CROSS_GROUP_REJECTED);
            assert!(
                !cross.is_empty() && cross != i18n_keys::ATTACH_CROSS_GROUP_REJECTED,
                "{locale:?}: ATTACH_CROSS_GROUP_REJECTED must resolve"
            );
            let partial = i18n.resolve(locale, i18n_keys::ATTACH_PARTIAL_CONTEXT_ERROR);
            assert!(
                !partial.is_empty() && partial != i18n_keys::ATTACH_PARTIAL_CONTEXT_ERROR,
                "{locale:?}: ATTACH_PARTIAL_CONTEXT_ERROR must resolve"
            );
        }
    }

    #[test]
    fn attach_navigation_cross_group_renders_target_then_current_in_all_locales() {
        let i18n = StaticI18n::new();
        let target = "whsrv-TARGET-proj";
        let current = "whsrv-CURRENT-proj";
        for locale in [Locale::En, Locale::ZhCn] {
            let msg = i18n.resolve_with_args(
                locale,
                i18n_keys::ATTACH_CROSS_GROUP_REJECTED,
                &[target, current],
            );
            let target_pos = msg.find(target).unwrap_or_else(|| {
                panic!("{locale:?}: target server must appear in message: {msg}")
            });
            let current_pos = msg.find(current).unwrap_or_else(|| {
                panic!("{locale:?}: current server must appear in message: {msg}")
            });
            assert!(
                target_pos < current_pos,
                "{locale:?}: target server must appear BEFORE current server in cross-group message.\n\
                 target_pos={target_pos}, current_pos={current_pos}\n\
                 message: {msg}"
            );
        }
    }

    #[test]
    fn attach_guidance_does_not_describe_nested_tmux() {
        let i18n = StaticI18n::new();
        let keys = [
            i18n_keys::ATTACH_GUIDANCE,
            i18n_keys::ATTACH_CROSS_GROUP_REJECTED,
            i18n_keys::ATTACH_PARTIAL_CONTEXT_ERROR,
            i18n_keys::ATTACH_ALREADY_CURRENT,
        ];
        let forbidden = ["Ctrl-b Ctrl-b", "unset TMUX", "TMUX_PANE"];
        for locale in [Locale::En, Locale::ZhCn] {
            for key in &keys {
                let msg = i18n.resolve(locale, key);
                for pattern in &forbidden {
                    assert!(
                        !msg.contains(pattern),
                        "{locale:?} key={key} must not contain '{pattern}': {msg}"
                    );
                }
            }
        }
    }
}
