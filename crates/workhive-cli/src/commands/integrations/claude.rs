//! Legacy-hook cleanup for `~/.claude/settings.json` (and
//! `$CLAUDE_CONFIG_DIR/settings.json`).
//!
//! The official Claude Code hook schema
//! (<https://code.claude.com/docs/en/hooks>) stores hooks under:
//!
//! ```json
//! {
//!   "hooks": {
//!     "UserPromptSubmit": [
//!       {
//!         "hooks": [
//!           {
//!             "type": "command",
//!             "command": "...",
//!             "_workhive_managed": true
//!           }
//!         ]
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! Earlier Workhive releases registered managed entries tagged with
//! the [`super::sentinel::WORKHIVE_MANAGED_KEY`] sentinel field
//! (`"_workhive_managed": true`). This module removes only those
//! sentinel-tagged entries; user-authored hook entries for the same
//! event are left intact.
//!
//! ## Idempotency + safety contract
//!
//! * `plan_remove` returns a [`CleanupStep`] that describes what the
//!   destructive `remove` *would* do, without any filesystem write.
//! * `remove` runs `plan_remove` and performs a backup + atomic write
//!   *only* when the plan reports `would_write = true`. A cleanup
//!   pass on an already-clean file, on a file containing only
//!   user-authored hook entries, or on a file with no `hooks`
//!   section, creates no backup and leaves the user's `settings.json`
//!   byte-for-byte unchanged.
//! * Malformed JSON surfaces as [`IntegrationError::ParseJson`]; the
//!   user's bytes are never overwritten in that path.

use std::fs;

use camino::Utf8Path;
use serde_json::Value;

use workhive_core::domain::AiActivityProvider;

use super::cleanup::CleanupStep;
use super::error::{IntegrationError, io_err};
use super::sentinel::{WORKHIVE_MANAGED_KEY, atomic_write, backup_file};

/// Inspect-only: classify the Claude `settings.json` at `settings_path`
/// into a [`CleanupStep`]. Performs zero filesystem writes.
///
/// Currently exercised exclusively by the in-process dry-run /
/// inspection path in tests; the production cleanup pass calls
/// [`remove`] which uses the same underlying [`compute_remove_plan`]
/// helper before any destructive write.
#[cfg(test)]
pub(super) fn plan_remove(settings_path: &Utf8Path) -> Result<CleanupStep, IntegrationError> {
    let plan = compute_remove_plan(settings_path)?;
    Ok(plan.step)
}

/// Plan + execute. The destructive path takes a backup and writes
/// the new body atomically *only* when the plan reports a write.
pub(super) fn remove(settings_path: &Utf8Path) -> Result<CleanupStep, IntegrationError> {
    let plan = compute_remove_plan(settings_path)?;
    if plan.step.would_write {
        let Some(new_body) = plan.new_body else {
            // Defensive guard: a `Remove` step must always carry the
            // computed replacement bytes. The plan helper enforces
            // this invariant; a divergence would indicate a logic
            // bug here. We surface it as a typed I/O error rather
            // than panic so the lifecycle caller can degrade.
            return Err(io_err(
                "internal_inconsistency",
                settings_path.to_path_buf(),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "remove plan reports would_write without computed body",
                ),
            ));
        };
        let _backup = backup_file(settings_path)?;
        atomic_write(settings_path, &new_body)?;
    }
    Ok(plan.step)
}

/// Bundle a [`CleanupStep`] with the precomputed new file body for
/// the destructive path. The body is only present when the step
/// reports a write so the execute path never needs to re-derive it.
struct RemovePlan {
    step: CleanupStep,
    new_body: Option<String>,
}

/// Read the on-disk file, locate any Workhive-managed entries, and
/// classify the situation into a [`RemovePlan`]. This function does
/// not write to disk.
///
/// Decision matrix:
///
/// * file missing → `Missing` step, no body.
/// * file present but malformed → propagate
///   [`IntegrationError::ParseJson`].
/// * file parses, no `hooks` object → `AlreadyClean` step, no body.
/// * file parses, `hooks` present, no managed entries → `AlreadyClean`
///   step, no body (hook entries the user wrote themselves remain).
/// * file parses, contains at least one managed entry → `Remove` step
///   carrying a serialized new body, with managed entries stripped
///   and empty matcher groups / event keys collapsed.
fn compute_remove_plan(settings_path: &Utf8Path) -> Result<RemovePlan, IntegrationError> {
    let target = settings_path.to_path_buf();
    if !settings_path.exists() {
        return Ok(RemovePlan {
            step: CleanupStep::missing(AiActivityProvider::ClaudeCode, target),
            new_body: None,
        });
    }
    let raw = fs::read_to_string(settings_path)
        .map_err(|e| io_err("read", settings_path.to_path_buf(), e))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(RemovePlan {
            step: CleanupStep::already_clean(
                AiActivityProvider::ClaudeCode,
                target,
                "empty_settings",
            ),
            new_body: None,
        });
    }
    let mut doc: Value =
        serde_json::from_str(trimmed).map_err(|e| IntegrationError::ParseJson {
            path: settings_path.to_path_buf(),
            source: e,
        })?;

    let Some(root) = doc.as_object_mut() else {
        // The file parsed but is not a JSON object (e.g. the user
        // wrote a top-level array). Workhive's managed entries can
        // only live inside an object; treat as already-clean for
        // cleanup purposes and leave the user's bytes alone.
        return Ok(RemovePlan {
            step: CleanupStep::already_clean(
                AiActivityProvider::ClaudeCode,
                target,
                "non_object_root",
            ),
            new_body: None,
        });
    };
    let Some(hooks_value) = root.get_mut("hooks") else {
        return Ok(RemovePlan {
            step: CleanupStep::already_clean(
                AiActivityProvider::ClaudeCode,
                target,
                "no_hooks_section",
            ),
            new_body: None,
        });
    };
    let Some(hooks) = hooks_value.as_object_mut() else {
        // `hooks` exists but is not an object (e.g. user wrote a
        // string). Workhive's managed sentinel can never live inside
        // a non-object `hooks` shape; preserve user bytes.
        return Ok(RemovePlan {
            step: CleanupStep::already_clean(
                AiActivityProvider::ClaudeCode,
                target,
                "non_object_hooks",
            ),
            new_body: None,
        });
    };

    let mut removed_any = false;
    let event_keys: Vec<String> = hooks.keys().cloned().collect();
    for event in event_keys {
        let Some(groups_value) = hooks.get_mut(&event) else {
            continue;
        };
        let Some(groups) = groups_value.as_array_mut() else {
            continue;
        };
        let mut kept_groups: Vec<Value> = Vec::with_capacity(groups.len());
        for mut group in groups.drain(..) {
            if let Some(inner_array) = group
                .as_object_mut()
                .and_then(|m| m.get_mut("hooks"))
                .and_then(|v| v.as_array_mut())
            {
                let before = inner_array.len();
                inner_array.retain(|h| !is_workhive_managed(h));
                if inner_array.len() != before {
                    removed_any = true;
                }
                if !inner_array.is_empty() {
                    kept_groups.push(group);
                }
            } else {
                kept_groups.push(group);
            }
        }
        if kept_groups.is_empty() {
            hooks.remove(&event);
        } else {
            *groups_value = Value::Array(kept_groups);
        }
    }

    if !removed_any {
        return Ok(RemovePlan {
            step: CleanupStep::already_clean(
                AiActivityProvider::ClaudeCode,
                target,
                "no_managed_entry",
            ),
            new_body: None,
        });
    }

    let body = render_settings_json(&doc, settings_path)?;
    Ok(RemovePlan {
        step: CleanupStep::remove(AiActivityProvider::ClaudeCode, target, "sentinel_match"),
        new_body: Some(body),
    })
}

fn render_settings_json(doc: &Value, path: &Utf8Path) -> Result<String, IntegrationError> {
    let body = serde_json::to_string_pretty(doc).map_err(|e| IntegrationError::ParseJson {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(body + "\n")
}

fn is_workhive_managed(hook: &Value) -> bool {
    matches!(
        hook.get(WORKHIVE_MANAGED_KEY).and_then(|v| v.as_bool()),
        Some(true)
    )
}
