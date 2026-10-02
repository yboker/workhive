//! i18n strings for the `update` command.

use std::collections::HashMap;

use workhive_core::i18n_keys as k;

pub fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    m.insert(k::UPDATE_CHECKING_FOR_UPDATES, "Checking for updates...");
    m.insert(
        k::UPDATE_AVAILABLE,
        "Update available: {0} → {1}\nRelease: {2}",
    );
    m.insert(
        k::UPDATE_ALREADY_UP_TO_DATE,
        "Already up to date (version {0})",
    );
    m.insert(
        k::UPDATE_DRY_RUN_MESSAGE,
        "Dry run mode — no changes will be made",
    );
    m.insert(k::UPDATE_WOULD_INSTALL, "Would install version:");
    m.insert(
        k::UPDATE_WOULD_INSTALL_LATEST,
        "Would install the latest version",
    );
    m.insert(k::UPDATE_CONFIRM_PROMPT, "Proceed with update? [y/N]");
    m.insert(k::UPDATE_CANCELLED, "Update cancelled");
    m.insert(
        k::UPDATE_DOWNLOADING_INSTALLING,
        "Downloading and installing...",
    );
    m.insert(k::UPDATE_SUCCESS, "Successfully updated to version {0}");
}

pub fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    m.insert(k::UPDATE_CHECKING_FOR_UPDATES, "正在检查更新...");
    m.insert(
        k::UPDATE_AVAILABLE,
        "有新版本可用：{0} → {1}\n发布页面：{2}",
    );
    m.insert(k::UPDATE_ALREADY_UP_TO_DATE, "已是最新版本（版本 {0}）");
    m.insert(k::UPDATE_DRY_RUN_MESSAGE, "试运行模式 — 不会进行任何更改");
    m.insert(k::UPDATE_WOULD_INSTALL, "将安装版本：");
    m.insert(k::UPDATE_WOULD_INSTALL_LATEST, "将安装最新版本");
    m.insert(k::UPDATE_CONFIRM_PROMPT, "是否继续更新？[y/N]");
    m.insert(k::UPDATE_CANCELLED, "更新已取消");
    m.insert(k::UPDATE_DOWNLOADING_INSTALLING, "正在下载并安装...");
    m.insert(k::UPDATE_SUCCESS, "已成功更新到版本 {0}");
}
