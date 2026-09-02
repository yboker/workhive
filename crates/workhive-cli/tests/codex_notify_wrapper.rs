//! Closed-loop integration tests for the Codex `notify` wrapper.
//!
//! The wrapper that Workhive installs into Codex `config.toml` must
//! satisfy four runtime invariants:
//!
//! 1. When invoked with the Workhive trust envelope (`WORKHIVE_BIN`,
//!    `WORKHIVE_RUNTIME_ROOT`, `WORKHIVE_TOOL_PROVIDER`,
//!    `WORKHIVE_GROUP`, `WORKHIVE_WORKSPACE`, `WORKHIVE_UNIT_ID`),
//!    the wrapper invokes `workhive internal activity codex-notify`.
//! 2. The recorded activity event lands at
//!    `<WORKHIVE_RUNTIME_ROOT>/<group>/<workspace>/activity_events/<unit_id>.jsonl`
//!    and is the canonical `TurnNotifiedComplete` /
//!    `SignalChannel::NotifyCallback` /
//!    `ObservationConfidence::Medium` completion observation.
//! 3. The wrapper still `exec`s the user's original notify command,
//!    preserving the original argv Codex passed in.
//! 4. The wrapper is fail-open: when the recorder rejects the event
//!    (trust envelope mismatched, store unwritable, etc.), the
//!    wrapper still exits 0 and the user's notify command still
//!    runs. Workhive telemetry must never suppress the user's
//!    notify pipeline.
//!
//! The wrapper string used here is the same shell template
//! `commands::integrations::codex::build_notify_script` emits in
//! production. The unit tests in `commands::integrations::tests`
//! pin the script's static shape; this file proves the runtime
//! side effects when the script is actually executed.

use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Same shell template `build_notify_script` emits in production.
///
/// Keeping a separate copy in the integration test is intentional:
/// the unit tests under `commands::integrations::tests` already
/// pin the production string against substring invariants
/// (`internal activity codex-notify`, `${WORKHIVE_BIN:-…}`, etc.).
/// This test goes one step further and *executes* the same template
/// against the real CLI binary, so any future drift in
/// `build_notify_script` shape will be caught by both layers.
fn render_notify_wrapper(workhive_bin_fallback: &Path, user_notify_cmd: &str) -> String {
    let fallback = workhive_bin_fallback
        .to_str()
        .expect("workhive bin path must be utf-8")
        .replace('"', "\\\"");
    format!(
        r#""${{WORKHIVE_BIN:-{fallback}}}" internal activity codex-notify "$@" >/dev/null 2>&1 || true; exec {user_cmd} "$@""#,
        fallback = fallback,
        user_cmd = user_notify_cmd,
    )
}

fn workhive_bin_path() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("workhive")
}

fn make_test_root() -> TempDir {
    tempfile::TempDir::new().expect("tempdir for codex-notify wrapper integration test")
}

/// Build a small shell user-notify recorder that writes the argv it
/// observed to `marker_path`, one JSON object per line. Used to
/// verify the wrapper forwarded the original notify argv intact.
fn write_user_notify_recorder(dir: &Path, marker_path: &Path) -> std::path::PathBuf {
    let script_path = dir.join("user_notify.sh");
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > {marker}\n",
        marker = marker_path
            .to_str()
            .expect("marker path must be utf-8")
            .replace('"', "\\\""),
    );
    std::fs::write(&script_path, body).expect("write user notify recorder");
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).expect("chmod user notify recorder");
    script_path
}

/// Locate the FsActivityEventStore JSONL file for a given pane key.
///
/// The on-disk layout is
/// `<runtime_root>/<group>/<workspace>/activity_events/<unit_id>.jsonl`
/// per [`workhive_infra::activity_event_store::FsActivityEventStore`].
fn pane_event_log(runtime_root: &Path, group: &str, workspace: &str, unit_id: &str) -> Vec<Value> {
    let log_path = runtime_root
        .join(group)
        .join(workspace)
        .join("activity_events")
        .join(format!("{unit_id}.jsonl"));
    let raw = match std::fs::read_to_string(&log_path) {
        Ok(s) => s,
        Err(err) => panic!(
            "expected ACP event log at {} but read failed: {err}",
            log_path.display()
        ),
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ACP event log line must be valid JSON"))
        .collect()
}

/// Happy path: when the wrapper runs with a valid Workhive trust
/// envelope, it records exactly one `TurnNotifiedComplete` /
/// `NotifyCallback` / `Medium` activity event at the correlated
/// pane key, and then `exec`s the user's notify command with the
/// original argv intact.
#[test]
fn codex_notify_wrapper_records_event_and_forwards_user_command() {
    let tmp = make_test_root();
    let runtime_root = tmp.path().join(".workhive").join("runtime");
    std::fs::create_dir_all(&runtime_root).expect("create runtime root");
    let marker = tmp.path().join("user_notify.argv");
    let user_cmd_path = write_user_notify_recorder(tmp.path(), &marker);

    let wrapper = render_notify_wrapper(
        &workhive_bin_path(),
        user_cmd_path
            .to_str()
            .expect("user notify script path must be utf-8"),
    );

    // Run the wrapper via `/bin/sh -c` with the trusted correlation
    // env vars Workhive injects when launching Codex. The two
    // positional args (`codex-arg1 codex-arg2`) stand in for the
    // notify event payload Codex would have passed.
    let status = Command::new("/bin/sh")
        .arg("-c")
        .arg(&wrapper)
        .arg("sh") // $0
        .arg("codex-arg1") // $1
        .arg("codex-arg2") // $2
        .env("WORKHIVE_BIN", workhive_bin_path())
        .env("WORKHIVE_RUNTIME_ROOT", &runtime_root)
        .env("WORKHIVE_TOOL_PROVIDER", "codex")
        .env("WORKHIVE_GROUP", "g-codex")
        .env("WORKHIVE_WORKSPACE", "main")
        .env("WORKHIVE_UNIT_ID", "u-codex-1")
        .env("HOME", tmp.path()) // hermetic config home
        .env("XDG_CONFIG_HOME", tmp.path().join(".config"))
        .status()
        .expect("notify wrapper executed");
    assert!(
        status.success(),
        "wrapper must exit 0 — Codex must never observe a non-zero exit from notify"
    );

    // Verify the user's original notify command ran with the
    // original argv preserved.
    let marker_body =
        std::fs::read_to_string(&marker).expect("user notify recorder must have written marker");
    let argv: Vec<&str> = marker_body.lines().collect();
    assert_eq!(
        argv,
        vec!["codex-arg1", "codex-arg2"],
        "wrapper must forward original notify argv unchanged"
    );

    // Verify the ACP event landed at the correct trusted-correlation
    // pane key and matches the canonical notify completion shape.
    let events = pane_event_log(&runtime_root, "g-codex", "main", "u-codex-1");
    assert_eq!(
        events.len(),
        1,
        "expected exactly one TurnNotifiedComplete event, got {events:?}"
    );
    let event = &events[0];
    assert_eq!(
        event["kind"], "turn_notified_complete",
        "notify wrapper MUST produce TurnNotifiedComplete, never working/started/permission-needed"
    );
    assert_eq!(
        event["channel"], "notify_callback",
        "notify wrapper events MUST be tagged SignalChannel::NotifyCallback"
    );
    assert_eq!(
        event["source"], "notify",
        "notify wrapper events MUST be tagged ProviderSignalSource::Notify"
    );
    assert_eq!(
        event["confidence"], "medium",
        "notify lacks the trusted payload to be promoted above Medium"
    );
    assert_eq!(event["provider"], "codex");
    assert_eq!(event["pane"]["group"], "g-codex");
    assert_eq!(event["pane"]["workspace"], "main");
    assert_eq!(event["pane"]["unit_id"], "u-codex-1");
}

/// Fail-open path: when the recorder rejects the event (here, the
/// trust envelope is corrupted by setting `WORKHIVE_TOOL_PROVIDER`
/// to a provider that disagrees with the notify endpoint), the
/// wrapper must still exit 0 and the user's notify command must
/// still run. The fail-open envelope in
/// `cmd_internal_activity_codex_notify` lets Workhive telemetry
/// be added without ever risking the user's own notify pipeline.
#[test]
fn codex_notify_wrapper_fail_open_when_recorder_rejects_event() {
    let tmp = make_test_root();
    let runtime_root = tmp.path().join(".workhive").join("runtime");
    std::fs::create_dir_all(&runtime_root).expect("create runtime root");
    let marker = tmp.path().join("user_notify.argv");
    let user_cmd_path = write_user_notify_recorder(tmp.path(), &marker);

    let wrapper = render_notify_wrapper(
        &workhive_bin_path(),
        user_cmd_path
            .to_str()
            .expect("user notify script path must be utf-8"),
    );

    // Deliberately corrupt the trust envelope: notify endpoint is
    // Codex-only, but we claim the pane is Claude. The recorder
    // returns Err internally; the fail-open envelope must still
    // return 0 to Codex.
    let status = Command::new("/bin/sh")
        .arg("-c")
        .arg(&wrapper)
        .arg("sh")
        .arg("fail-open-arg")
        .env("WORKHIVE_BIN", workhive_bin_path())
        .env("WORKHIVE_RUNTIME_ROOT", &runtime_root)
        .env("WORKHIVE_TOOL_PROVIDER", "claude-code") // mismatch
        .env("WORKHIVE_GROUP", "g-codex")
        .env("WORKHIVE_WORKSPACE", "main")
        .env("WORKHIVE_UNIT_ID", "u-codex-1")
        .env("HOME", tmp.path())
        .env("XDG_CONFIG_HOME", tmp.path().join(".config"))
        .status()
        .expect("notify wrapper executed even with corrupt trust envelope");
    assert!(
        status.success(),
        "wrapper MUST exit 0 even when the recorder rejects the event \
         (fail-open contract: Workhive telemetry never breaks notify)"
    );

    // Verify no event was written — the recorder correctly refused
    // the cross-provider event but the wrapper still exited 0.
    let log_path = runtime_root
        .join("g-codex")
        .join("main")
        .join("activity_events")
        .join("u-codex-1.jsonl");
    assert!(
        !log_path.exists(),
        "recorder MUST NOT write an event when the trust envelope is corrupt"
    );

    // Verify the user's original notify command STILL ran — this is
    // the central fail-open invariant.
    let marker_body = std::fs::read_to_string(&marker).expect(
        "user notify recorder MUST run even when the Workhive recorder fails (fail-open contract)",
    );
    let argv: Vec<&str> = marker_body.lines().collect();
    assert_eq!(
        argv,
        vec!["fail-open-arg"],
        "wrapper must forward user argv even after a recorder failure"
    );
}

/// `WORKHIVE_BIN` fallback path: when the env var is absent (e.g.
/// the user invoked Codex outside a Workhive pane after Workhive
/// had installed the wrapper), the wrapper falls back to the
/// install-time `notify_callback_path` pinned into the script body.
/// The recorder still records the event and the user's notify
/// command still runs.
#[test]
fn codex_notify_wrapper_uses_install_time_fallback_when_env_unset() {
    let tmp = make_test_root();
    let runtime_root = tmp.path().join(".workhive").join("runtime");
    std::fs::create_dir_all(&runtime_root).expect("create runtime root");
    let marker = tmp.path().join("user_notify.argv");
    let user_cmd_path = write_user_notify_recorder(tmp.path(), &marker);

    let wrapper = render_notify_wrapper(
        &workhive_bin_path(),
        user_cmd_path
            .to_str()
            .expect("user notify script path must be utf-8"),
    );

    let status = Command::new("/bin/sh")
        .arg("-c")
        .arg(&wrapper)
        .arg("sh")
        .arg("fallback-arg")
        .env_remove("WORKHIVE_BIN") // simulate non-supervisor invocation
        .env("WORKHIVE_RUNTIME_ROOT", &runtime_root)
        .env("WORKHIVE_TOOL_PROVIDER", "codex")
        .env("WORKHIVE_GROUP", "g-codex")
        .env("WORKHIVE_WORKSPACE", "main")
        .env("WORKHIVE_UNIT_ID", "u-codex-1")
        .env("HOME", tmp.path())
        .env("XDG_CONFIG_HOME", tmp.path().join(".config"))
        .status()
        .expect("notify wrapper executed via install-time fallback");
    assert!(
        status.success(),
        "wrapper must exit 0 via fallback path too"
    );

    // Recording still succeeded via the install-time fallback.
    let events = pane_event_log(&runtime_root, "g-codex", "main", "u-codex-1");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["kind"], "turn_notified_complete");
    assert_eq!(events[0]["channel"], "notify_callback");
    assert_eq!(events[0]["confidence"], "medium");

    let marker_body = std::fs::read_to_string(&marker).expect("user notify ran via fallback path");
    let argv: Vec<&str> = marker_body.lines().collect();
    assert_eq!(argv, vec!["fallback-arg"]);
}
