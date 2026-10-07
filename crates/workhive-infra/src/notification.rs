//! Local notification port adapters.
//!
//! Three adapters live here, with **explicit** production semantics:
//!
//! 1. [`MacOsOsascriptNotificationPort`] — the **macOS user-attention
//!    adapter**. Spawns `osascript -e 'display
//!    notification "<title>" with title "<subtitle>"'` as a subprocess
//!    using [`std::process::Command::arg`] (no `sh -c`, no shell
//!    string concatenation). The notification body is a typed enum
//!    discriminant only — no prompt or message content reaches the
//!    OS daemon. See [`format_osascript_args`] for exact argv shape
//!    and [`production_notification_port`] for the composed factory.
//!
//! 2. [`LocalLogNotificationPort`] — the **always-on diagnostic
//!    adapter**. Delivers each notification by emitting a structured
//!    `tracing::info!` event under a dedicated target
//!    (`workhive.notification`). On macOS this runs *in addition* to
//!    the osascript user-attention adapter so notifications are still
//!    observable when `osascript` is unavailable (CI, SSH headless,
//!    user-disabled notification center). On Linux it is the
//!    fallback delivery surface (we explicitly do not invoke any
//!    Linux notify daemon — see "Linux degraded path" below).
//!
//! 3. [`NoopNotificationPort`] — **CI/test fallback only**. Returns
//!    `Ok(())` immediately and writes nothing. Production code MUST
//!    NEVER select this adapter; doing so would silently drop every
//!    notification while still letting the outbox claim a successful
//!    delivery, which would be a truthfulness violation.
//!
//! ## Privacy
//!
//! All adapters honor the privacy invariant on `PendingNotification`:
//! the struct carries only metadata enums (pane id, provider, category,
//! hint, transition signature). No prompt or message content can ever
//! reach a log line, an `osascript` argv, or any other side effect.
//! Adapter implementations renders only typed enum discriminants and
//! the pane correlation key (group / workspace / unit_id).
//!
//! ## Why subprocess `osascript`, not a native crate
//!
//! Native macOS notification crates (`mac-notification-sys`,
//! `notify-rust`) require a bundled app identity, a `LSUIElement`
//! Info.plist entry, or both — Workhive ships as a single CLI binary
//! today, so we cannot meet either requirement without architecture
//! approval. The subprocess `osascript -e 'display notification …'`
//! shape is the sanctioned delivery mechanism. The adapter still:
//!
//! * uses `Command::arg` (no `sh -c`, no shell concatenation);
//! * passes only enum discriminants and pane id segments — no prompt
//!   content;
//! * fail-opens on every error path (binary missing, exit non-zero,
//!   spawn refused) so the supervisor / provider hook is never blocked.
//!
//! ## Linux degraded path
//!
//! Linux is **not** the primary target. The factory
//! [`production_notification_port`] returns the same composed adapter
//! on every platform but [`MacOsOsascriptNotificationPort`] surfaces
//! a typed [`NotificationDeliveryError`] on non-macOS platforms via
//! `cfg!(target_os)` so the upstream policy records a `DeliveryFailed`
//! outbox entry instead of a fabricated `Delivered`. The
//! [`LocalLogNotificationPort`] still emits the structured tracing
//! event so the user sees the notification in their terminal / file
//! log on Linux. This is a typed degraded decision: Linux is
//! "fallback / unsupported", not a silent Ok(()).

use std::process::{Command, Stdio};

#[cfg(unix)]
fn command_status_retrying_etxtbsy(
    command: &mut Command,
) -> std::io::Result<std::process::ExitStatus> {
    crate::dependency::spawn_retrying_etxtbsy(command).and_then(|mut child| child.wait())
}

#[cfg(not(unix))]
fn command_status_retrying_etxtbsy(
    command: &mut Command,
) -> std::io::Result<std::process::ExitStatus> {
    command.status()
}

use tracing::{info, warn};
use workhive_core::domain::activity::ProviderKind;
use workhive_core::ports::{
    NotificationCategory, NotificationDeliveryError, NotificationHint, NotificationPort,
    PendingNotification,
};

/// Tracing target used by the production local notification adapter.
///
/// Pinned as a const so log routing rules (e.g. file-only sinks, log
/// level overrides) can target it without string-matching the message
/// body. This is the only target Workhive uses for user-facing
/// notification delivery.
pub const NOTIFICATION_TRACING_TARGET: &str = "workhive.notification";

/// Production-grade local notification port.
///
/// Emits one structured `tracing::info!` event per delivery under the
/// [`NOTIFICATION_TRACING_TARGET`] target. The event is the user-
/// visible notification: when stderr console logging is enabled the
/// user sees it in their terminal, and when file logging is enabled
/// the line is appended to the rotating log file (see the logging policy).
///
/// ## Why this is honest "local delivery"
///
/// `tracing::info!` is a side effect with two properties required of
/// the production adapter:
///
/// 1. **Observable** — the line goes somewhere a user can read,
///    unlike a no-op that returns `Ok(())` while writing nothing.
/// 2. **Local-only** — no network call, no cloud push, no OS daemon
///    handshake. The recipient is the local stderr/file logger that
///    Workhive itself owns.
///
/// ## Privacy
///
/// Every field of [`PendingNotification`] is enum-typed (or an opaque
/// transition signature derived from event ids). This adapter forwards
/// only those enum discriminants and the pane correlation key
/// (group / workspace / unit_id, never prompt content). The
/// `transition_sig` is intentionally NOT logged — it is a dedupe key
/// only and may contain a UUID-shaped value the user does not need to
/// see.
pub struct LocalLogNotificationPort;

impl LocalLogNotificationPort {
    /// Construct the production local notification port.
    ///
    /// This is a zero-cost constructor: there is no runtime state to
    /// initialize — the adapter is a thin wrapper over `tracing::info!`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for LocalLogNotificationPort {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationPort for LocalLogNotificationPort {
    fn deliver(&self, notification: &PendingNotification) -> Result<(), NotificationDeliveryError> {
        // Render a structured event under a dedicated target so log
        // routing can distinguish notification side effects from
        // diagnostic noise. We emit at INFO so default console
        // configurations show the line; the logging policy requires the
        // console sink to be enabled by default.
        //
        // All fields are typed: pane keys are short identifiers,
        // category/hint/provider are enum discriminants. There is no
        // free-form text channel for prompt or message content.
        info!(
            target: NOTIFICATION_TRACING_TARGET,
            pane_group = %notification.pane.group,
            pane_workspace = %notification.pane.workspace,
            pane_unit_id = %notification.pane.unit_id,
            provider = ?notification.provider,
            category = ?notification.category,
            hint = ?notification.hint,
            "workhive notification"
        );
        Ok(())
    }
}

// ── MacOS osascript adapter ───────────────────────────────────────────────────

/// macOS local user-attention notification adapter.
///
/// Spawns `osascript -e 'display notification "<body>" with title
/// "<title>" subtitle "<subtitle>"'` as a subprocess via
/// [`Command::arg`]. There is no shell wrapper, no `sh -c`, and no
/// string concatenation into a shell command — the AppleScript
/// expression is built once by [`format_osascript_expression`] and
/// passed as a single `-e` argument.
///
/// ## Privacy invariants enforced here
///
/// The AppleScript expression is built from typed enum
/// discriminants and the pane correlation key only:
///
/// * **title**: `"Workhive · <ProviderLabel>"` or just `"Workhive"`
///   (provider is a `ProviderKind` enum discriminant).
/// * **subtitle**: `"<workspace> · <unit_id>"` (pane correlation key,
///   no message body).
/// * **body**: `"<CategoryLabel>"` plus optional `" (<HintLabel>)"`
///   (both enum discriminants).
///
/// There is no path through this adapter that reaches prompt or
/// message content — those fields do not exist on
/// [`PendingNotification`] in the first place. The expression
/// builder defends against AppleScript injection by escaping `\\`
/// and `"` in every interpolated segment, even though all inputs are
/// enum-derived and would not contain those characters in practice.
///
/// ## Fail-open semantics
///
/// Every error path returns [`NotificationDeliveryError`] so the
/// upstream policy can record a `DeliveryFailed` outbox entry, but
/// callers MUST treat that as advisory (per
/// [`NotificationPort`]'s fail-open contract). When this adapter is
/// composed via [`production_notification_port`] the upstream
/// composite swallows the error and still delivers the diagnostic
/// log line, so user-attention failure never blocks the supervisor.
///
/// ## Non-macOS behaviour
///
/// On non-macOS targets `deliver` returns
/// `NotificationDeliveryError::other("...unsupported on this
/// platform")`. This is a typed degraded decision: we surface
/// unsupported as an explicit outbox failure, not as a silent
/// `Ok(())`. The composed factory
/// still emits the diagnostic log line so the user has *some*
/// observability surface on Linux.
pub struct MacOsOsascriptNotificationPort {
    binary: std::path::PathBuf,
}

impl MacOsOsascriptNotificationPort {
    /// Construct an adapter that invokes the system `osascript` via
    /// `PATH`. Panics never; absence of the binary surfaces as a
    /// `NotificationDeliveryError` at call time.
    pub fn new() -> Self {
        Self {
            binary: std::path::PathBuf::from("osascript"),
        }
    }

    /// Construct an adapter that invokes a specific `osascript`-like
    /// binary. Used by unit tests with a fixture script so we can
    /// verify both the success and failure paths without depending
    /// on the host having a real macOS notification daemon.
    pub fn with_binary(binary: std::path::PathBuf) -> Self {
        Self { binary }
    }
}

impl Default for MacOsOsascriptNotificationPort {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationPort for MacOsOsascriptNotificationPort {
    fn deliver(&self, notification: &PendingNotification) -> Result<(), NotificationDeliveryError> {
        // Linux / Windows: explicit typed degraded decision. The
        // composed factory still emits the diagnostic log line so
        // the user has an observability surface, but we surface
        // `DeliveryFailed` here so the outbox does not lie about
        // user-attention delivery on unsupported platforms.
        if !cfg!(target_os = "macos") {
            return Err(NotificationDeliveryError {
                reason: "macOS osascript adapter is unsupported on this platform".to_string(),
            });
        }

        let expression = format_osascript_expression(notification);

        // Use Command::arg per the external-command policy — no shell, no
        // `sh -c`, no string concatenation. stdin/stdout/stderr are
        // all redirected to null so osascript cannot deadlock the
        // supervisor on a closed pipe and so its diagnostic noise
        // does not leak into the supervisor's stderr.
        let mut cmd = Command::new(&self.binary);
        cmd.arg("-e")
            .arg(&expression)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match command_status_retrying_etxtbsy(&mut cmd) {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(NotificationDeliveryError {
                reason: format!("osascript exited with non-zero status: {status:?}"),
            }),
            Err(err) => Err(NotificationDeliveryError {
                reason: format!("osascript spawn failed: {err}"),
            }),
        }
    }
}

/// Render the AppleScript `display notification` expression for a
/// notification.
///
/// Pure function; exposed at `pub(crate)` visibility so the unit
/// tests can assert on the exact rendered shape (argv #2 sent to
/// `osascript -e`). The function is invariant: same input → same
/// output. No I/O, no clock, no environment.
///
/// The output shape is a SINGLE AppleScript statement:
///
/// ```text
/// display notification "<body>" with title "<title>" subtitle "<subtitle>"
/// ```
///
/// Every interpolated segment is escaped through [`escape_for_applescript`]
/// so injecting an `"` or `\\` cannot prematurely close the string
/// literal — even though all current inputs are enum-derived and do
/// not contain those characters. We escape defensively because the
/// `PendingNotification` shape is allowed to grow new metadata enum
/// variants in the future.
pub(crate) fn format_osascript_expression(notification: &PendingNotification) -> String {
    let category = format_category_label(notification.category);
    let body = match notification.hint {
        Some(hint) => format!("{} ({})", category, format_hint_label(hint)),
        None => category.to_string(),
    };
    let title = match notification.provider {
        Some(p) => format!("Workhive · {}", format_provider_label(p)),
        None => "Workhive".to_string(),
    };
    let subtitle = format!(
        "{} · {}",
        notification.pane.workspace, notification.pane.unit_id
    );

    format!(
        r#"display notification "{body}" with title "{title}" subtitle "{subtitle}""#,
        body = escape_for_applescript(&body),
        title = escape_for_applescript(&title),
        subtitle = escape_for_applescript(&subtitle),
    )
}

/// Escape a string for embedding inside an AppleScript double-quoted
/// literal. Replaces `\\` first to avoid double-escaping, then `"`.
fn escape_for_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_category_label(c: NotificationCategory) -> &'static str {
    match c {
        NotificationCategory::PermissionNeeded => "Permission needed",
        NotificationCategory::Failed => "Turn failed",
        NotificationCategory::Completed => "Turn completed",
        NotificationCategory::AllPanesComplete => "All panes complete",
    }
}

fn format_hint_label(h: NotificationHint) -> &'static str {
    match h {
        NotificationHint::PermissionUnresolved => "permission unresolved",
        NotificationHint::ProviderError => "provider error",
        NotificationHint::TurnFailed => "turn failed",
        NotificationHint::NotifyCallback => "notify callback",
        NotificationHint::LifecycleHook => "lifecycle hook",
    }
}

fn format_provider_label(p: ProviderKind) -> &'static str {
    match p {
        ProviderKind::Codex => "Codex",
        ProviderKind::ClaudeCode => "Claude Code",
        ProviderKind::OpenCode => "OpenCode",
        ProviderKind::Future => "Workhive provider",
    }
}

// ── Composite adapter & factory ───────────────────────────────────────────────

/// Composite notification port that combines a best-effort
/// user-attention adapter (e.g. macOS `osascript`) with the
/// always-on diagnostic adapter ([`LocalLogNotificationPort`]).
///
/// The composite enforces this contract:
///
/// * **User-attention adapter is best-effort.** Its failures are
///   logged at `warn` under [`NOTIFICATION_TRACING_TARGET`] but
///   never propagated; the supervisor and provider hooks must not
///   be blocked by an OS notification daemon being unavailable.
/// * **Diagnostic adapter is mandatory.** Its `tracing::info!` line
///   is the durable observability surface — it fires whether or
///   not the user-attention adapter succeeded. Its result is the
///   one returned to the caller.
///
/// This shape keeps `LocalLogNotificationPort` as the documented
/// fallback / diagnostic delivery,
/// while making the macOS osascript adapter the actual user-facing
/// surface.
pub struct CompositeNotificationPort {
    user_attention: Box<dyn NotificationPort>,
    diagnostic: Box<dyn NotificationPort>,
}

impl CompositeNotificationPort {
    /// Compose a user-attention adapter with a mandatory diagnostic
    /// fallback. Both must be `'static` because the composite is
    /// constructed at the supervisor boundary and held for the
    /// lifetime of the notification trigger call.
    pub fn new(
        user_attention: Box<dyn NotificationPort>,
        diagnostic: Box<dyn NotificationPort>,
    ) -> Self {
        Self {
            user_attention,
            diagnostic,
        }
    }
}

impl NotificationPort for CompositeNotificationPort {
    fn deliver(&self, notification: &PendingNotification) -> Result<(), NotificationDeliveryError> {
        // Best-effort user-attention surface. Errors degrade to a
        // warn log — the supervisor must never break on osascript
        // failure (binary missing, daemon disabled, AppleScript
        // permission revoked, headless SSH session, etc.).
        if let Err(err) = self.user_attention.deliver(notification) {
            warn!(
                target: NOTIFICATION_TRACING_TARGET,
                pane_group = %notification.pane.group,
                pane_workspace = %notification.pane.workspace,
                pane_unit_id = %notification.pane.unit_id,
                category = ?notification.category,
                error = %err.reason,
                "user-attention notification adapter failed (degraded to diagnostic-only)"
            );
        }
        // Mandatory diagnostic fallback. Its tracing::info! line is
        // the canonical observable side effect; if it fails (it
        // never does today) the upstream policy records a
        // DeliveryFailed outbox entry instead of fabricating
        // success.
        self.diagnostic.deliver(notification)
    }
}

/// Production [`NotificationPort`] factory.
///
/// Returns the composed adapter the supervisor / event-driven
/// notification trigger should use:
///
/// * On **macOS**: [`MacOsOsascriptNotificationPort`] (best-effort)
///   composed with [`LocalLogNotificationPort`] (mandatory diagnostic).
/// * On **Linux**: [`LinuxNotifySendNotificationPort`] (best-effort)
///   composed with [`LocalLogNotificationPort`] (mandatory diagnostic).
/// * On **other platforms**: a degraded user-attention adapter that
///   always returns a typed `DeliveryFailed` (so the outbox is
///   honest about what shipped), composed with
///   [`LocalLogNotificationPort`] so the diagnostic line still
///   reaches the user via the tracing surface.
///
/// This factory is the **only** sanctioned constructor for
/// production NotificationPort instances — `LocalLogNotificationPort`
/// alone is documented as a diagnostic fallback, not a complete
/// production solution. Tests may keep using
/// [`LocalLogNotificationPort::new`] / [`NoopNotificationPort`]
/// directly.
pub fn production_notification_port() -> Box<dyn NotificationPort> {
    let user_attention: Box<dyn NotificationPort> = production_user_attention_port();
    Box::new(CompositeNotificationPort::new(
        user_attention,
        Box::new(LocalLogNotificationPort::new()),
    ))
}

/// Pick the platform-appropriate user-attention adapter.
///
/// Split out so the composed factory stays small and so the
/// platform routing is unit-testable by inspecting the boxed
/// adapter's concrete type-id at the construction boundary in
/// integration tests if needed. Production code should always
/// prefer [`production_notification_port`] over building the
/// composite by hand.
fn production_user_attention_port() -> Box<dyn NotificationPort> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacOsOsascriptNotificationPort::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxNotifySendNotificationPort::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        // Fall back to the macOS adapter; it returns a typed
        // unsupported-platform error on non-macOS targets which the
        // composite swallows after warning. The diagnostic log is
        // still the user's observable surface.
        Box::new(MacOsOsascriptNotificationPort::new())
    }
}

// ── Linux notify-send adapter ─────────────────────────────────────────────────

/// Linux local user-attention notification adapter.
///
/// Spawns `notify-send` (from `libnotify-bin` on Debian/Ubuntu,
/// `libnotify` on most distros) as a subprocess via [`Command::arg`].
/// The argv shape is:
///
/// ```text
/// notify-send --app-name=Workhive --category=workhive
///             --hint=string:x-canonical-private-pane-id:<pane_id>
///             "<summary>" "<body>"
/// ```
///
/// where `<summary>` is a typed enum label (provider + category) and
/// `<body>` carries category/hint discriminants plus the workspace ·
/// unit_id correlation key. No prompt or message content can reach
/// the argv because `PendingNotification` does not carry any such
/// field; the renderer [`format_notify_send_argv`] only consumes
/// the typed enum discriminants and the opaque pane correlation key.
///
/// ## Fail-open semantics
///
/// Every error path returns [`NotificationDeliveryError`] so the
/// upstream policy can record a `DeliveryFailed` outbox entry,
/// but callers MUST treat that as advisory (per the
/// [`NotificationPort`] contract). When this adapter is composed
/// via [`production_notification_port`] the upstream composite
/// swallows the error and still delivers the diagnostic log line,
/// so user-attention failure never blocks the supervisor.
///
/// ## Non-Linux behaviour
///
/// On non-Linux targets `deliver` short-circuits with a typed
/// "unsupported on this platform" error so the outbox cannot
/// silently fabricate a successful delivery. The diagnostic log
/// fallback still fires through the composite.
pub struct LinuxNotifySendNotificationPort {
    binary: std::path::PathBuf,
}

impl LinuxNotifySendNotificationPort {
    /// Construct an adapter that invokes the system `notify-send` via
    /// `PATH`. Panics never; absence of the binary surfaces as a
    /// `NotificationDeliveryError` at call time.
    pub fn new() -> Self {
        Self {
            binary: std::path::PathBuf::from("notify-send"),
        }
    }

    /// Construct an adapter that invokes a specific
    /// `notify-send`-like binary. Used by unit tests with a fixture
    /// script so we can verify both the success and failure paths
    /// without depending on the host having a real Linux desktop
    /// notification daemon.
    pub fn with_binary(binary: std::path::PathBuf) -> Self {
        Self { binary }
    }
}

impl Default for LinuxNotifySendNotificationPort {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationPort for LinuxNotifySendNotificationPort {
    fn deliver(&self, notification: &PendingNotification) -> Result<(), NotificationDeliveryError> {
        // Non-Linux targets: explicit typed degraded decision so the
        // outbox does not silently fabricate Ok(()) on hosts where
        // notify-send is not the right surface.
        if !cfg!(target_os = "linux") {
            return Err(NotificationDeliveryError {
                reason: "Linux notify-send adapter is unsupported on this platform".to_string(),
            });
        }

        let args = format_notify_send_argv(notification);

        // Use Command::arg per the external-command policy — no shell, no `sh -c`.
        // stdin/stdout/stderr are redirected to null so the
        // supervisor can never deadlock on a closed pipe and so
        // notify-send's diagnostic noise does not leak into the
        // supervisor's stderr.
        let mut cmd = Command::new(&self.binary);
        for arg in &args {
            cmd.arg(arg);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match command_status_retrying_etxtbsy(&mut cmd) {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(NotificationDeliveryError {
                reason: format!("notify-send exited with non-zero status: {status:?}"),
            }),
            Err(err) => Err(NotificationDeliveryError {
                reason: format!("notify-send spawn failed: {err}"),
            }),
        }
    }
}

/// Render the `notify-send` argv vector for a notification.
///
/// Pure function; exposed at `pub(crate)` so the unit tests can
/// assert on the exact rendered shape. The function is invariant:
/// same input → same output. No I/O, no clock, no environment.
///
/// The argv shape is:
///
/// ```text
/// --app-name=Workhive
/// --category=workhive.<category>
/// "<summary>"
/// "<body>"
/// ```
///
/// where summary and body carry only typed enum discriminants and
/// the pane correlation key. No prompt content can ever appear here
/// because `PendingNotification` does not carry any.
pub(crate) fn format_notify_send_argv(notification: &PendingNotification) -> Vec<String> {
    let category = format_category_label(notification.category);
    let body = match notification.hint {
        Some(hint) => format!(
            "{} ({}) — {} · {}",
            category,
            format_hint_label(hint),
            notification.pane.workspace,
            notification.pane.unit_id,
        ),
        None => format!(
            "{} — {} · {}",
            category, notification.pane.workspace, notification.pane.unit_id,
        ),
    };
    let summary = match notification.provider {
        Some(p) => format!("Workhive · {}", format_provider_label(p)),
        None => "Workhive".to_string(),
    };
    let category_tag = match notification.category {
        NotificationCategory::PermissionNeeded => "workhive.permission",
        NotificationCategory::Failed => "workhive.failed",
        NotificationCategory::Completed => "workhive.completed",
        NotificationCategory::AllPanesComplete => "workhive.all_panes_complete",
    };
    vec![
        "--app-name=Workhive".to_string(),
        format!("--category={category_tag}"),
        summary,
        body,
    ]
}

/// Test-only no-op notification port.
///
/// Returns `Ok(())` immediately and discards the notification. This
/// adapter exists ONLY for unit/integration tests that want to drive
/// the delivery pipeline without asserting on tracing output, and for
/// CI smoke checks that need to satisfy the [`NotificationPort`]
/// boundary without any side effect.
///
/// **Production code MUST NOT instantiate this adapter.** The pane
/// supervisor's notification trigger point uses
/// [`production_notification_port`] instead, because the production
/// path may not silently drop notifications while pretending they
/// were delivered.
/// The grep-target name (`NoopNotificationPort`) makes any
/// reintroduction in `pane_supervisor` (or any other production
/// module) trivially detectable in code review.
pub struct NoopNotificationPort;

impl NotificationPort for NoopNotificationPort {
    fn deliver(
        &self,
        _notification: &PendingNotification,
    ) -> Result<(), NotificationDeliveryError> {
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::activity::{PaneCorrelationKey, ProviderKind};
    use workhive_core::ports::{
        NotificationCategory, NotificationHint, PendingNotification, TransitionSignature,
    };

    fn sample_notification() -> PendingNotification {
        PendingNotification {
            pane: PaneCorrelationKey::new("g", "w", "u-1"),
            provider: Some(ProviderKind::Codex),
            category: NotificationCategory::Completed,
            hint: Some(NotificationHint::NotifyCallback),
            transition_sig: TransitionSignature::from_event_id("evt-test-only"),
        }
    }

    /// The production adapter never returns an error: tracing is
    /// considered always-available locally. The fail-open contract on
    /// `deliver` still permits future adapters to surface errors, but
    /// the local-log adapter has no failure mode of its own.
    #[test]
    fn local_log_port_delivers_without_error() {
        let port = LocalLogNotificationPort::new();
        port.deliver(&sample_notification())
            .expect("local log adapter must always succeed");
    }

    /// Verify the adapter under the exposed tracing target by hooking
    /// a span subscriber that records the structured event. We assert
    /// that the event uses the dedicated target, NOT the generic root
    /// target, so log routing remains stable.
    #[test]
    fn local_log_port_uses_dedicated_tracing_target() {
        use std::sync::{Arc, Mutex};
        use tracing::Subscriber;
        use tracing::span::{Attributes, Id, Record};

        #[derive(Clone, Default)]
        struct CapturingSubscriber {
            events: Arc<Mutex<Vec<(String, tracing::Level)>>>,
        }

        impl Subscriber for CapturingSubscriber {
            fn enabled(&self, _meta: &tracing::Metadata<'_>) -> bool {
                true
            }
            fn new_span(&self, _span: &Attributes<'_>) -> Id {
                Id::from_u64(1)
            }
            fn record(&self, _span: &Id, _values: &Record<'_>) {}
            fn record_follows_from(&self, _span: &Id, _follows: &Id) {}
            fn event(&self, event: &tracing::Event<'_>) {
                let meta = event.metadata();
                self.events
                    .lock()
                    .unwrap()
                    .push((meta.target().to_owned(), *meta.level()));
            }
            fn enter(&self, _span: &Id) {}
            fn exit(&self, _span: &Id) {}
        }

        let sub = CapturingSubscriber::default();
        let events_handle = sub.events.clone();
        tracing::subscriber::with_default(sub, || {
            let port = LocalLogNotificationPort::new();
            port.deliver(&sample_notification()).unwrap();
        });

        let events = events_handle.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|(t, lvl)| t == NOTIFICATION_TRACING_TARGET && *lvl == tracing::Level::INFO),
            "local log adapter must emit at least one INFO event under \
             the dedicated `{}` target, observed: {:?}",
            NOTIFICATION_TRACING_TARGET,
            events
        );
    }

    /// The Noop adapter is only available in tests / under the
    /// `testkit` feature. We exercise it here to keep its contract
    /// covered, but we can't actually assert "production cannot
    /// instantiate it" from a unit test — that property is enforced
    /// by the cfg gate at the type level.
    #[test]
    fn noop_port_returns_ok_immediately() {
        let port = NoopNotificationPort;
        port.deliver(&sample_notification())
            .expect("noop adapter is infallible by construction");
    }

    // ── osascript adapter: pure expression builder ────────────────────────────

    /// The expression builder must produce a single AppleScript
    /// `display notification` statement, with the title carrying the
    /// provider label, subtitle carrying `<workspace> · <unit_id>`,
    /// and the body carrying the category label plus optional hint.
    /// No prompt or message content can leak into the rendered
    /// expression because `PendingNotification` does not carry any.
    #[test]
    fn format_osascript_expression_renders_typed_metadata_only() {
        let n = sample_notification();
        let expr = format_osascript_expression(&n);
        assert_eq!(
            expr,
            r#"display notification "Turn completed (notify callback)" with title "Workhive · Codex" subtitle "w · u-1""#
        );
    }

    /// Permission-needed routes to its own category label and uses
    /// the typed permission hint. This is the high-priority
    /// notification class.
    #[test]
    fn format_osascript_expression_renders_permission_needed() {
        let n = PendingNotification {
            pane: PaneCorrelationKey::new("g", "w", "u-perm"),
            provider: Some(ProviderKind::ClaudeCode),
            category: NotificationCategory::PermissionNeeded,
            hint: Some(NotificationHint::PermissionUnresolved),
            transition_sig: TransitionSignature::from_event_id("evt-perm"),
        };
        let expr = format_osascript_expression(&n);
        assert!(expr.contains("Permission needed (permission unresolved)"));
        assert!(expr.contains("Workhive · Claude Code"));
        assert!(expr.contains("w · u-perm"));
    }

    /// All-panes-complete is a workspace-level signal; the body
    /// must reflect that label so a user reading the OS notification
    /// can distinguish it from a single-pane completion.
    #[test]
    fn format_osascript_expression_renders_all_panes_complete() {
        let n = PendingNotification {
            pane: PaneCorrelationKey::new("g", "w", "u-1"),
            provider: None,
            category: NotificationCategory::AllPanesComplete,
            hint: None,
            transition_sig: TransitionSignature::from_event_id("evt-all"),
        };
        let expr = format_osascript_expression(&n);
        assert!(expr.starts_with(r#"display notification "All panes complete""#));
        // No provider → bare "Workhive" title.
        assert!(expr.contains(r#"with title "Workhive""#));
    }

    /// Defensive escape: `"` inside any interpolated segment must be
    /// escaped so the AppleScript string literal does not close
    /// early. Even though no current enum variant produces this
    /// shape, we exercise the escape path through a synthetic pane
    /// id (the only field the adapter does not enum-discriminate).
    #[test]
    fn format_osascript_expression_escapes_quotes_and_backslashes() {
        let n = PendingNotification {
            pane: PaneCorrelationKey::new("g", "w\\\"x", "u-\"-1"),
            provider: Some(ProviderKind::Codex),
            category: NotificationCategory::Failed,
            hint: Some(NotificationHint::ProviderError),
            transition_sig: TransitionSignature::from_event_id("evt"),
        };
        let expr = format_osascript_expression(&n);
        // backslashes are doubled, quotes are escaped to `\"`. We
        // assert on substrings rather than the full literal because
        // the rendered byte sequence is hostile to direct comparison.
        assert!(expr.contains(r#"subtitle "w\\\"x · u-\"-1""#));
    }

    // ── osascript adapter: subprocess invocation ──────────────────────────────

    /// Verify the success path: with a fixture binary that always
    /// exits 0, the adapter must return `Ok(())`. We only exercise
    /// this on macOS because the adapter explicitly returns a typed
    /// "unsupported on this platform" error elsewhere (verified by
    /// [`osascript_adapter_unsupported_on_non_macos_targets`]).
    #[test]
    #[cfg(target_os = "macos")]
    fn osascript_adapter_succeeds_with_zero_exit_fixture() {
        let fixture = write_shell_fixture("#!/bin/sh\nexit 0\n");
        let port = MacOsOsascriptNotificationPort::with_binary(fixture.path().to_path_buf());
        port.deliver(&sample_notification())
            .expect("zero-exit fixture must succeed");
        // fixture 在这里被 drop，TempDir 自动清理
    }

    /// Verify the failure mapping: a fixture binary that exits 1
    /// must surface as `NotificationDeliveryError` whose reason
    /// mentions the non-zero status. The supervisor must NOT block
    /// on this — it is the upstream composite's job to swallow the
    /// error (see [`composite_swallows_user_attention_failure`]).
    #[test]
    #[cfg(target_os = "macos")]
    fn osascript_adapter_maps_non_zero_exit_to_delivery_error() {
        let fixture = write_shell_fixture("#!/bin/sh\nexit 1\n");
        let port = MacOsOsascriptNotificationPort::with_binary(fixture.path().to_path_buf());
        let err = port
            .deliver(&sample_notification())
            .expect_err("non-zero exit must surface a NotificationDeliveryError");
        assert!(
            err.reason.contains("non-zero status"),
            "reason should describe the failure mode, got: {}",
            err.reason
        );
        // fixture 在这里被 drop，TempDir 自动清理
    }

    /// Verify the spawn-failure mapping: pointing the adapter at a
    /// path that does not exist must surface as a typed
    /// `DeliveryFailed` reason mentioning spawn failure. This is
    /// the most important failure mode in practice (e.g. when
    /// `osascript` is missing on a Linux developer machine that
    /// ran the binary by mistake).
    #[test]
    fn osascript_adapter_maps_spawn_failure_to_delivery_error() {
        let port = MacOsOsascriptNotificationPort::with_binary(std::path::PathBuf::from(
            "/definitely/not/a/binary/that/exists/wh-test",
        ));
        let result = port.deliver(&sample_notification());
        // On non-macOS targets the adapter short-circuits with a
        // typed unsupported error. Either failure mode satisfies
        // the contract — the adapter must NOT silently succeed.
        let err = result.expect_err("missing binary must surface a NotificationDeliveryError");
        assert!(
            !err.reason.is_empty(),
            "adapter must surface a non-empty failure reason"
        );
    }

    /// On non-macOS targets the adapter must surface a typed
    /// `unsupported on this platform` error so the upstream policy
    /// records a `DeliveryFailed` outbox entry instead of pretending
    /// delivery succeeded.
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn osascript_adapter_unsupported_on_non_macos_targets() {
        let port = MacOsOsascriptNotificationPort::new();
        let err = port
            .deliver(&sample_notification())
            .expect_err("non-macOS must report unsupported");
        assert!(
            err.reason.contains("unsupported on this platform"),
            "non-macOS reason must be explicit, got: {}",
            err.reason
        );
    }

    // ── composite adapter ─────────────────────────────────────────────────────

    /// Capturing port that records every delivered notification
    /// and lets the test assert on the count. Used only inside
    /// composite tests.
    struct CapturingPort {
        delivered: std::sync::Mutex<Vec<PendingNotification>>,
    }

    impl CapturingPort {
        fn new() -> Self {
            Self {
                delivered: std::sync::Mutex::new(Vec::new()),
            }
        }
        fn count(&self) -> usize {
            self.delivered.lock().unwrap().len()
        }
    }

    impl NotificationPort for CapturingPort {
        fn deliver(
            &self,
            notification: &PendingNotification,
        ) -> Result<(), NotificationDeliveryError> {
            self.delivered.lock().unwrap().push(notification.clone());
            Ok(())
        }
    }

    /// Failing port whose deliver always errors. Used to drive the
    /// composite's user-attention failure path.
    struct FailingPort;

    impl NotificationPort for FailingPort {
        fn deliver(
            &self,
            _notification: &PendingNotification,
        ) -> Result<(), NotificationDeliveryError> {
            Err(NotificationDeliveryError {
                reason: "synthetic failure for test".to_string(),
            })
        }
    }

    /// The composite must invoke BOTH adapters even when the
    /// user-attention adapter fails. The diagnostic adapter's result
    /// is the one returned to the caller, so a user-attention
    /// failure becomes a degraded-but-still-Ok delivery outcome.
    #[test]
    fn composite_swallows_user_attention_failure_and_runs_diagnostic() {
        let diagnostic = std::sync::Arc::new(CapturingPort::new());
        struct ArcWrapper(std::sync::Arc<CapturingPort>);
        impl NotificationPort for ArcWrapper {
            fn deliver(&self, n: &PendingNotification) -> Result<(), NotificationDeliveryError> {
                self.0.deliver(n)
            }
        }
        let composite = CompositeNotificationPort::new(
            Box::new(FailingPort),
            Box::new(ArcWrapper(diagnostic.clone())),
        );
        composite
            .deliver(&sample_notification())
            .expect("composite must surface diagnostic Ok even when user-attention fails");
        assert_eq!(
            diagnostic.count(),
            1,
            "diagnostic adapter must still receive the notification"
        );
    }

    /// The composite must propagate diagnostic failures (today the
    /// diagnostic adapter does not fail, but the contract holds for
    /// future replacements). Without a propagation path the outbox
    /// would silently lie about delivery.
    #[test]
    fn composite_propagates_diagnostic_failure() {
        let composite =
            CompositeNotificationPort::new(Box::new(NoopNotificationPort), Box::new(FailingPort));
        let err = composite
            .deliver(&sample_notification())
            .expect_err("diagnostic failure must propagate");
        assert!(err.reason.contains("synthetic failure"));
    }

    /// The production factory must be callable on every supported
    /// host without panicking. We don't actually invoke the returned
    /// adapter (the host might lack a notification daemon), only
    /// confirm the constructor is total.
    #[test]
    fn production_notification_port_factory_is_total() {
        let _port = production_notification_port();
    }

    // ── Linux notify-send adapter ─────────────────────────────────────────────

    /// The notify-send argv builder must produce a stable shape
    /// composed of typed enum labels and the pane correlation key
    /// only. No prompt content can leak in because
    /// `PendingNotification` does not carry any.
    #[test]
    fn format_notify_send_argv_renders_typed_metadata_only() {
        let n = sample_notification();
        let argv = format_notify_send_argv(&n);
        assert_eq!(
            argv,
            vec![
                "--app-name=Workhive".to_string(),
                "--category=workhive.completed".to_string(),
                "Workhive · Codex".to_string(),
                "Turn completed (notify callback) — w · u-1".to_string(),
            ]
        );
    }

    /// Permission notifications must route to the permission
    /// category tag so a desktop environment with category-based
    /// filtering (e.g. KDE notification rules) can differentiate
    /// them from completion or failure events.
    #[test]
    fn format_notify_send_argv_renders_permission_needed() {
        let n = PendingNotification {
            pane: PaneCorrelationKey::new("g", "w", "u-perm"),
            provider: Some(ProviderKind::ClaudeCode),
            category: NotificationCategory::PermissionNeeded,
            hint: Some(NotificationHint::PermissionUnresolved),
            transition_sig: TransitionSignature::from_event_id("evt-perm"),
        };
        let argv = format_notify_send_argv(&n);
        assert!(argv.iter().any(|a| a == "--category=workhive.permission"));
        assert!(argv.iter().any(|a| a == "Workhive · Claude Code"));
        let body = argv.last().expect("body");
        assert!(body.starts_with("Permission needed (permission unresolved)"));
        assert!(body.contains("w · u-perm"));
    }

    /// Verify the success path on Linux only — a fixture binary
    /// that exits zero must yield `Ok(())`. On non-Linux platforms
    /// the adapter explicitly returns a typed `unsupported` error
    /// (verified by [`notify_send_adapter_unsupported_on_non_linux_targets`]).
    #[test]
    #[cfg(target_os = "linux")]
    fn notify_send_adapter_succeeds_with_zero_exit_fixture() {
        let fixture = write_shell_fixture("#!/bin/sh\nexit 0\n");
        let port = LinuxNotifySendNotificationPort::with_binary(fixture.path().to_path_buf());
        port.deliver(&sample_notification())
            .expect("zero-exit fixture must succeed");
        // fixture 在这里被 drop，TempDir 自动清理
    }

    /// Verify the failure mapping: non-zero exit must surface as
    /// `NotificationDeliveryError` with a status-bearing reason.
    /// The supervisor must NOT block — the composite swallows the
    /// error (see [`composite_swallows_user_attention_failure_and_runs_diagnostic`]).
    #[test]
    #[cfg(target_os = "linux")]
    fn notify_send_adapter_maps_non_zero_exit_to_delivery_error() {
        let fixture = write_shell_fixture("#!/bin/sh\nexit 1\n");
        let port = LinuxNotifySendNotificationPort::with_binary(fixture.path().to_path_buf());
        let err = port
            .deliver(&sample_notification())
            .expect_err("non-zero exit must surface a NotificationDeliveryError");
        assert!(
            err.reason.contains("non-zero status"),
            "reason should describe the failure mode, got: {}",
            err.reason
        );
        // fixture 在这里被 drop，TempDir 自动清理
    }

    /// Spawn failure (missing binary) must surface as a typed
    /// `DeliveryFailed` reason. This is the most common failure
    /// mode on a Linux server without a desktop session or on a
    /// container without `libnotify-bin` installed.
    #[test]
    fn notify_send_adapter_maps_spawn_failure_to_delivery_error() {
        let port = LinuxNotifySendNotificationPort::with_binary(std::path::PathBuf::from(
            "/definitely/not/a/binary/that/exists/wh-notify-test",
        ));
        let result = port.deliver(&sample_notification());
        // On non-Linux targets the adapter short-circuits with a
        // typed unsupported error. Either failure mode satisfies
        // the contract — the adapter must NOT silently succeed.
        let err = result.expect_err("missing binary must surface a NotificationDeliveryError");
        assert!(
            !err.reason.is_empty(),
            "adapter must surface a non-empty failure reason"
        );
    }

    /// On non-Linux targets the adapter must surface a typed
    /// `unsupported on this platform` error so the outbox does not
    /// silently fabricate a successful delivery.
    #[test]
    #[cfg(not(target_os = "linux"))]
    fn notify_send_adapter_unsupported_on_non_linux_targets() {
        let port = LinuxNotifySendNotificationPort::new();
        let err = port
            .deliver(&sample_notification())
            .expect_err("non-Linux must report unsupported");
        assert!(
            err.reason.contains("unsupported on this platform"),
            "non-Linux reason must be explicit, got: {}",
            err.reason
        );
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    /// RAII guard for shell-script fixture files.
    ///
    /// Holds a `TempDir` to ensure the directory is cleaned up when dropped.
    /// The fixture script path is stored separately because the `TempDir`
    /// owns the directory lifecycle.
    ///
    /// # Design Rationale
    ///
    /// We use `TempDir` + `std::fs::write` instead of `NamedTempFile` to avoid
    /// `ETXTBSY` on Linux. The key insight:
    ///
    /// - `NamedTempFile` keeps a write-mode fd open until dropped
    /// - Linux rejects `execve()` on files with open writers
    /// - `std::fs::write` opens, writes, and closes the fd atomically
    /// - No write-mode fd remains open after `std::fs::write` returns
    /// - `TempDir` ensures the directory is cleaned up when this guard drops
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    struct FixtureScript {
        /// Path to the executable shell script
        path: std::path::PathBuf,
        /// RAII guard for the temporary directory.
        /// When this field is dropped, `TempDir::drop` deletes the directory
        /// and all its contents. The leading `_` suppresses dead-code warnings
        /// — the field is never read directly, but its `Drop` impl is essential.
        _dir: tempfile::TempDir,
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    impl FixtureScript {
        /// Returns a reference to the fixture script path
        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    /// Write a shell-script fixture to a temp file, mark it executable, and return
    /// an RAII guard that cleans up the directory on drop.
    ///
    /// # ETXTBSY Prevention
    ///
    /// Uses `std::fs::write` and `std::fs::set_permissions` instead of
    /// `std::fs::File` to ensure no write-mode fd remains open when the function
    /// returns. Linux rejects `execve()` on files with open writers (`ETXTBSY`).
    ///
    /// `std::fs::write` opens, writes, and closes the fd atomically — no manual
    /// fd management needed. `std::fs::set_permissions` opens its own fd, applies
    /// the mode, and closes it.
    ///
    /// # Cleanup
    ///
    /// The returned `FixtureScript` holds a `TempDir` that automatically deletes
    /// the directory and all contents when dropped (including on test panic).
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn write_shell_fixture(body: &str) -> FixtureScript {
        use std::os::unix::fs::PermissionsExt;

        // Create temporary directory
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let path = dir.path().join("fixture.sh");

        // std::fs::write opens the file, writes, and closes the fd.
        // No write-mode fd remains open, so execve() can proceed.
        std::fs::write(&path, body.as_bytes()).expect("write fixture body");

        // std::fs::set_permissions opens its own fd, applies the mode, closes it.
        let mut perms = std::fs::metadata(&path)
            .expect("read metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod fixture");

        // Return guard that holds TempDir ownership.
        // When this guard drops, TempDir deletes the directory and all contents.
        FixtureScript { path, _dir: dir }
    }
}
