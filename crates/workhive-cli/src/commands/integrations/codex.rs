//! Codex `config.toml` integration: legacy hook cleanup +
//! command-native notify wrapper.
//!
//! ## Surfaces
//!
//! Per the official Codex hook spec
//! (<https://developers.openai.com/codex/hooks>) hook entries live
//! under `[[hooks.<EventName>]]` and `[[hooks.<EventName>.hooks]]`.
//! Earlier Workhive releases registered managed entries tagged with
//! `managed_by = "workhive"`. The current preparation lifecycle does
//! not install hook entries; [`remove`] / [`plan_remove`] exists
//! solely to clean those legacy entries up.
//!
//! The Codex `notify` command (a top-level `notify = "…"` string in
//! `config.toml`) is *not* a hook. Codex invokes it after task
//! completion as a fire-and-forget event. The notify integration
//! ([`install_notify`]) is the only remaining write path in this
//! module: it manages a sentinel-bracketed `# workhive-notify-begin`
//! / `# workhive-notify-end` region so repeated installs are
//! idempotent and the user's pre-existing `notify` command (if any)
//! is forwarded verbatim.
//!
//! ## Idempotency + safety contract
//!
//! * `plan_remove` returns a [`CleanupStep`] describing what `remove`
//!   *would* do; it performs zero writes.
//! * `remove` takes a backup and writes only when the plan reports a
//!   write. A clean file, a file with no `[hooks]` section, or a
//!   file containing only user-authored hook entries produces no
//!   backup and no write.
//! * `install_notify` skips the backup + atomic write when the
//!   serialized body is byte-for-byte identical to the on-disk file.
//! * Malformed TOML surfaces as [`IntegrationError::ParseToml`]; the
//!   user's bytes are never overwritten in that path.

use std::fs;

use camino::Utf8Path;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

use workhive_core::domain::AiActivityProvider;

use super::change::InstallChange;
use super::cleanup::CleanupStep;
use super::error::{IntegrationError, io_err};
use super::sentinel::{atomic_write, backup_file};

/// Sentinel key embedded in each Workhive-owned innermost hook
/// table written by earlier releases. Codex itself ignores unknown
/// keys so this is safe to round-trip via `toml_edit`.
const MANAGED_BY_KEY: &str = "managed_by";
const MANAGED_BY_VALUE: &str = "workhive";

/// Sentinel comment written immediately before the managed notify
/// region so idempotent reinstalls can locate and replace it without
/// stomping on the user's surrounding `config.toml` content.
const NOTIFY_SENTINEL_BEGIN: &str = "# workhive-notify-begin";
const NOTIFY_SENTINEL_END: &str = "# workhive-notify-end";

// ---------------------------------------------------------------------------
// Legacy hook cleanup
// ---------------------------------------------------------------------------

/// Inspect-only: classify the Codex `config.toml` at `config_path`
/// into a [`CleanupStep`]. Performs zero filesystem writes.
///
/// Currently exercised exclusively by the in-process dry-run /
/// inspection path in tests; the production cleanup pass calls
/// [`remove`] which uses the same underlying [`compute_remove_plan`]
/// helper before any destructive write.
#[cfg(test)]
pub(super) fn plan_remove(config_path: &Utf8Path) -> Result<CleanupStep, IntegrationError> {
    let plan = compute_remove_plan(config_path)?;
    Ok(plan.step)
}

/// Plan + execute. Takes a backup and writes only when the plan
/// reports a write. A no-op cleanup leaves the user's `config.toml`
/// byte-for-byte unchanged and creates no backup file.
pub(super) fn remove(config_path: &Utf8Path) -> Result<CleanupStep, IntegrationError> {
    let plan = compute_remove_plan(config_path)?;
    if plan.step.would_write {
        let Some(new_body) = plan.new_body else {
            return Err(io_err(
                "internal_inconsistency",
                config_path.to_path_buf(),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "remove plan reports would_write without computed body",
                ),
            ));
        };
        let _backup = backup_file(config_path)?;
        atomic_write(config_path, &new_body)?;
    }
    Ok(plan.step)
}

/// Bundle a [`CleanupStep`] with the precomputed new file body for
/// the destructive path. Only populated when the step reports a
/// write.
struct RemovePlan {
    step: CleanupStep,
    new_body: Option<String>,
}

fn compute_remove_plan(config_path: &Utf8Path) -> Result<RemovePlan, IntegrationError> {
    let target = config_path.to_path_buf();
    if !config_path.exists() {
        return Ok(RemovePlan {
            step: CleanupStep::missing(AiActivityProvider::Codex, target),
            new_body: None,
        });
    }
    let raw = fs::read_to_string(config_path)
        .map_err(|e| io_err("read", config_path.to_path_buf(), e))?;
    if raw.trim().is_empty() {
        return Ok(RemovePlan {
            step: CleanupStep::already_clean(AiActivityProvider::Codex, target, "empty_config"),
            new_body: None,
        });
    }
    let mut doc: DocumentMut = raw.parse().map_err(|e| IntegrationError::ParseToml {
        path: config_path.to_path_buf(),
        source: e,
    })?;

    let Some(hooks_item) = doc.get_mut("hooks") else {
        return Ok(RemovePlan {
            step: CleanupStep::already_clean(AiActivityProvider::Codex, target, "no_hooks_section"),
            new_body: None,
        });
    };
    let Some(hooks_table) = hooks_item.as_table_mut() else {
        return Ok(RemovePlan {
            step: CleanupStep::already_clean(AiActivityProvider::Codex, target, "non_table_hooks"),
            new_body: None,
        });
    };

    let mut removed = false;
    let event_names: Vec<String> = hooks_table.iter().map(|(k, _)| k.to_owned()).collect();
    for event in event_names {
        let Some(item) = hooks_table.get_mut(&event) else {
            continue;
        };
        let Some(groups) = item.as_array_of_tables_mut() else {
            continue;
        };
        let mut kept = ArrayOfTables::new();
        let drained: Vec<Table> = std::mem::take(groups).into_iter().collect();
        for mut group in drained {
            let mut drop_group = false;
            if let Some(inner_item) = group.get_mut("hooks")
                && let Some(inner) = inner_item.as_array_of_tables_mut()
            {
                let before = inner.len();
                let surviving: Vec<Table> = std::mem::take(inner)
                    .into_iter()
                    .filter(|t| !is_managed_inner_table(t))
                    .collect();
                for t in surviving {
                    inner.push(t);
                }
                if inner.len() != before {
                    removed = true;
                }
                if inner.is_empty() {
                    drop_group = true;
                }
            }
            if !drop_group {
                kept.push(group);
            }
        }
        if kept.is_empty() {
            hooks_table.remove(&event);
        } else {
            *item = Item::ArrayOfTables(kept);
        }
    }
    if hooks_table.is_empty() {
        doc.as_table_mut().remove("hooks");
    }

    if !removed {
        return Ok(RemovePlan {
            step: CleanupStep::already_clean(AiActivityProvider::Codex, target, "no_managed_entry"),
            new_body: None,
        });
    }

    Ok(RemovePlan {
        step: CleanupStep::remove(AiActivityProvider::Codex, target, "sentinel_match"),
        new_body: Some(doc.to_string()),
    })
}

fn is_managed_inner_table(t: &Table) -> bool {
    matches!(
        t.get(MANAGED_BY_KEY)
            .and_then(Item::as_value)
            .and_then(Value::as_str),
        Some(MANAGED_BY_VALUE)
    )
}

// ---------------------------------------------------------------------------
// Command-native notify wrapper
// ---------------------------------------------------------------------------

/// Install or update the Workhive-managed `notify` command in the
/// Codex `config.toml` at `config_path`.
///
/// ## Contract
///
/// * The wrapper region is sentinel-bracketed by
///   [`NOTIFY_SENTINEL_BEGIN`] / [`NOTIFY_SENTINEL_END`].
/// * The user's pre-existing `notify` command (if any), outside the
///   sentinel region, is captured and forwarded so user-authored
///   notify behavior is preserved.
/// * Idempotent: re-running on an already-current file leaves bytes
///   untouched and reports [`InstallChange::AlreadyCurrent`] (no
///   backup is created).
/// * Fail-open: failures inside the wrapper never interrupt Codex —
///   recording calls swallow non-zero exits via `|| true`.
/// * When `notify_callback_path` is `None`, the wrapper still
///   forwards to the user's command (or becomes a `true` no-op); the
///   recording step only fires when `${WORKHIVE_BIN}` is set at
///   runtime.
pub(super) fn install_notify(
    config_path: &Utf8Path,
    notify_callback_path: Option<&Utf8Path>,
) -> Result<InstallChange, IntegrationError> {
    let original_raw = read_raw_optional(config_path)?;
    let existing_raw = original_raw.as_deref().unwrap_or("");

    let user_notify_cmd = extract_user_notify_cmd(existing_raw);
    let notify_script = build_notify_script(notify_callback_path, user_notify_cmd.as_deref());
    let new_body = splice_notify_region(existing_raw, &notify_script);

    if new_body == existing_raw {
        return Ok(InstallChange::AlreadyCurrent);
    }

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| io_err("create_dir_all", parent.to_path_buf(), e))?;
    }
    let backup = backup_file(config_path)?;
    atomic_write(config_path, &new_body)?;
    Ok(InstallChange::Updated { backup })
}

fn read_raw_optional(path: &Utf8Path) -> Result<Option<String>, IntegrationError> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(|e| io_err("read", path.to_path_buf(), e))?;
    Ok(Some(raw))
}

/// Extract the user's existing `notify` command string from `raw`,
/// looking for a `notify = "…"` line outside the sentinel region.
fn extract_user_notify_cmd(raw: &str) -> Option<String> {
    let mut inside_sentinel = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed == NOTIFY_SENTINEL_BEGIN {
            inside_sentinel = true;
            continue;
        }
        if trimmed == NOTIFY_SENTINEL_END {
            inside_sentinel = false;
            continue;
        }
        if inside_sentinel {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("notify") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                let cmd =
                    if let Some(inner) = rest.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                        Some(inner.to_owned())
                    } else {
                        rest.strip_prefix('\'')
                            .and_then(|s| s.strip_suffix('\''))
                            .map(|inner| inner.to_owned())
                    };
                if cmd.as_deref().map(|c| !c.is_empty()).unwrap_or(false) {
                    return cmd;
                }
            }
        }
    }
    None
}

/// Build the shell command body for the managed notify wrapper.
///
/// Layered behavior:
///
/// 1. Prefer `${WORKHIVE_BIN}` (set by the pane supervisor on every
///    managed launch) for the recording step. Fall back to the
///    install-time `notify_callback_path` when supplied so the
///    wrapper still records when invoked outside a Workhive-spawned
///    process tree.
/// 2. The recording call swallows non-zero exits via `|| true` so
///    Codex is never interrupted by Workhive telemetry.
/// 3. Forward to `user_notify_cmd` (if any) via `exec` so the user's
///    own notify command sees the same argv.
/// 4. With neither recorder nor user command wired up, emit `true`
///    so the resulting `notify = "true"` line remains a valid shell
///    command (Codex parses it eagerly).
fn build_notify_script(
    notify_callback_path: Option<&Utf8Path>,
    user_notify_cmd: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(fallback) = notify_callback_path {
        let fallback_quoted = fallback.as_str().replace('"', "\\\"");
        parts.push(format!(
            r#""${{WORKHIVE_BIN:-{fallback}}}" internal activity codex-notify "$@" >/dev/null 2>&1 || true"#,
            fallback = fallback_quoted,
        ));
    } else {
        parts.push(
            r#"if [ -n "${WORKHIVE_BIN:-}" ]; then "$WORKHIVE_BIN" internal activity codex-notify "$@" >/dev/null 2>&1 || true; fi"#
                .to_owned(),
        );
    }

    if let Some(user_cmd) = user_notify_cmd {
        parts.push(format!(
            r#"exec {user_cmd} "$@""#,
            user_cmd = user_cmd.replace('"', "\\\"")
        ));
    }

    if parts.is_empty() {
        "true".to_owned()
    } else {
        parts.join("; ")
    }
}

/// Splice the managed notify region into `raw`. If a sentinel region
/// already exists it is replaced in place; otherwise the block is
/// appended at the end. Any bare `notify = "…"` line outside the
/// sentinel is removed so the sentinel becomes the sole notify
/// entry.
fn splice_notify_region(raw: &str, notify_script: &str) -> String {
    let managed_block = format!(
        "{begin}\nnotify = \"{script}\"\n{end}\n",
        begin = NOTIFY_SENTINEL_BEGIN,
        script = notify_script.replace('"', "\\\""),
        end = NOTIFY_SENTINEL_END,
    );

    if raw.contains(NOTIFY_SENTINEL_BEGIN) {
        let mut result = String::new();
        let mut inside = false;
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed == NOTIFY_SENTINEL_BEGIN {
                inside = true;
                result.push_str(&managed_block);
                continue;
            }
            if trimmed == NOTIFY_SENTINEL_END {
                inside = false;
                continue;
            }
            if inside {
                continue;
            }
            result.push_str(line);
            result.push('\n');
        }
        return result;
    }

    let mut result = String::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        let is_bare_notify = trimmed.starts_with("notify")
            && trimmed
                .trim_start_matches("notify")
                .trim_start()
                .starts_with('=');
        if is_bare_notify {
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    if !result.ends_with('\n') || result.is_empty() {
        result.push('\n');
    }
    result.push_str(&managed_block);
    result
}
