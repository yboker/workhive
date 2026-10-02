//! Local notification port and outbox types.
//!
//! All notification delivery goes through this port so the core
//! domain never takes an OS-specific dependency. The notification
//! system is **local-only**: no network calls, no cloud push.
//!
//! ## Fail-open contract
//!
//! A `NotificationPort::deliver` error MUST be logged at `warn` by
//! the caller and MUST NOT propagate to break any provider or pane
//! execution path. Callers must treat notification failure as
//! advisory, not fatal.
//!
//! ## Privacy
//!
//! [`PendingNotification`] deliberately carries NO prompt or message
//! content. The body is metadata only: pane id, provider (if known),
//! state category, and an optional short refusal or hint reason enum.
//! This ensures the notification system never leaks conversation
//! contents to any OS notification daemon.

use std::collections::HashSet;

use thiserror::Error;

use crate::domain::activity::{PaneCorrelationKey, ProviderKind};

// ── Category ─────────────────────────────────────────────────────────────────

/// The category of a local notification.
///
/// Each category has its own default noise level. Categories are
/// feature-gated (see [`NotificationConfig`]): if a category is
/// disabled the notification must be **silently dropped** — no
/// writes, no diagnostics, no user guidance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationCategory {
    /// A pane is waiting for user permission to proceed. This is a
    /// high-priority signal: it means the agent is blocked.
    PermissionNeeded,
    /// A pane turn or tool invocation failed. Enabled by default.
    Failed,
    /// A pane turn completed successfully. Low-noise by default;
    /// configurable to fully disable.
    Completed,
    /// All relevant AI panes have reached completion-like states.
    /// Available but must not spam when panes complete in rapid
    /// succession.
    AllPanesComplete,
}

// ── NotificationRefusal ───────────────────────────────────────────────────────

/// Why a notification was refused before reaching the port.
///
/// Callers log this at `debug` or surface it as a typed metric; they
/// MUST NOT re-throw it as an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationRefusal {
    /// The feature gate for this category is disabled.
    FeatureGateDisabled,
    /// The underlying pane state has low confidence.
    LowConfidence,
    /// The underlying signal is stale or expired.
    Stale,
    /// The pane integration is disabled or unsupported.
    IntegrationDisabled,
    /// The pane integration is degraded or unknown.
    IntegrationDegraded,
    /// The pane data is from the legacy path (`acp_source = false`),
    /// which carries no reliable state for notification decisions.
    LegacySource,
    /// This exact transition has already been notified (dedupe hit).
    AlreadyNotified,
}

// ── PendingNotification ───────────────────────────────────────────────────────

/// A single notification ready for delivery.
///
/// This is deliberately **metadata-only** — it carries no prompt text,
/// no message body, and no conversation content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNotification {
    /// The pane this notification is about.
    pub pane: PaneCorrelationKey,
    /// Provider, if known (may be `None` for legacy-source panes that
    /// do reach delivery — though in practice the policy refuses those).
    pub provider: Option<ProviderKind>,
    /// What kind of notification this is.
    pub category: NotificationCategory,
    /// An optional structured hint about why this category fired.
    /// Must not contain free-form text derived from conversation content.
    pub hint: Option<NotificationHint>,
    /// Deterministic state-transition signature used for dedupe.
    /// See [`NotificationOutbox`] for the derivation rules.
    pub transition_sig: TransitionSignature,
}

/// Short structured hint accompanying a notification.
///
/// All variants are metadata enums — no free-form content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationHint {
    /// The permission request is still unresolved.
    PermissionUnresolved,
    /// The turn failed due to a provider-reported error.
    ProviderError,
    /// The turn failed but no further detail is available.
    TurnFailed,
    /// The completion was signalled via notify callback (medium
    /// confidence).
    NotifyCallback,
    /// The completion was observed via a lifecycle hook (high
    /// confidence).
    LifecycleHook,
}

// ── TransitionSignature ───────────────────────────────────────────────────────

/// Deterministic state-transition identifier used for dedupe.
///
/// ## Design
///
/// The signature is keyed on `(pane, category, source_event_id_or_seq)`.
/// When `source_event_id` is present (ACP path), it IS the signature:
/// a UUID derived from the event that triggered the notification.
/// This makes dedupe exact — the same ACP event never fires twice.
///
/// When only a sequential position is available (fallback path), the
/// signature uses the pane-id + category + the **highest event
/// sequence number** seen in the triggering window. This is still
/// deterministic: if no new events arrive the sequence does not
/// change, so repeated evaluations hit dedupe.
///
/// Importantly: this is **never wall-clock-based**. Two evaluations
/// at different times that see the same events will produce the same
/// signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransitionSignature(pub String);

impl TransitionSignature {
    /// Build from a specific ACP event UUID string (most precise form).
    pub fn from_event_id(event_id: &str) -> Self {
        Self(event_id.to_owned())
    }

    /// Build from pane key + category + latest event sequence number.
    ///
    /// `seq` is the `observed_at_unix_ms` of the newest event in
    /// the triggering window, which is monotonically increasing in the
    /// append-only event store and therefore deterministic.
    pub fn from_seq(pane: &PaneCorrelationKey, category: NotificationCategory, seq: i64) -> Self {
        let cat = format!("{category:?}");
        Self(format!(
            "{}:{}:{}:{}:{}",
            pane.group, pane.workspace, pane.unit_id, cat, seq
        ))
    }
}

// ── NotificationPort ─────────────────────────────────────────────────────────

/// Error returned by [`NotificationPort::deliver`].
///
/// Callers MUST treat this as advisory — log at `warn` and continue.
#[derive(Debug, Error)]
#[error("notification delivery failed: {reason}")]
pub struct NotificationDeliveryError {
    pub reason: String,
}

/// Port for local notification delivery.
///
/// Implementations must NOT make network calls. They may use OS
/// notification APIs, write to a local log, or simply record in
/// memory (for tests). Delivery failures MUST be recoverable by
/// the caller (fail-open contract).
pub trait NotificationPort {
    /// Deliver a notification to the local system.
    ///
    /// Returns `Ok(())` on success or `Err(NotificationDeliveryError)`
    /// when the delivery mechanism is temporarily unavailable. The
    /// caller MUST log the error at `warn` and continue.
    fn deliver(&self, notification: &PendingNotification) -> Result<(), NotificationDeliveryError>;
}

// ── NotificationOutbox ────────────────────────────────────────────────────────

/// Status of a queued notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboxEntryStatus {
    /// Delivered successfully.
    Delivered,
    /// Delivery failed; the outbox records the reason for inspection.
    DeliveryFailed(String),
    /// Silently dropped by policy (disabled gate, low confidence, etc.).
    Refused(NotificationRefusal),
}

/// A single entry in the outbox — used for test inspection and
/// diagnostics.
#[derive(Debug, Clone)]
pub struct OutboxEntry {
    pub notification: PendingNotification,
    pub status: OutboxEntryStatus,
}

/// In-memory notification outbox.
///
/// The outbox tracks which transition signatures have already been
/// notified so that a repeated evaluation with the same events
/// produces no duplicate delivery. It also queues failures for
/// diagnostic inspection without re-throwing them to callers.
///
/// ## Dedupe guarantee
///
/// Once a `TransitionSignature` is added to `delivered_sigs`, it
/// will never be delivered again within the lifetime of this outbox
/// instance. Across restarts, dedupe is not persistent (notifications
/// are local-only; the next process restart can re-fire a completed
/// transition that was never persisted). This is acceptable for the
/// conservative local notification use case.
#[derive(Debug, Default)]
pub struct NotificationOutbox {
    /// Signatures of transitions that have been successfully delivered.
    delivered_sigs: HashSet<TransitionSignature>,
    /// All entries ever queued (delivered or failed), for inspection.
    entries: Vec<OutboxEntry>,
}

impl NotificationOutbox {
    /// Create a fresh empty outbox.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the outbox with previously-delivered signatures.
    ///
    /// Used by the trigger boundary at the start of every
    /// `evaluate_after_acp_append` invocation to restore the dedupe
    /// set from a durable [`NotificationOutboxStore`]. Seeded
    /// signatures count as duplicates immediately; they are not
    /// added to `entries` because they were already delivered in a
    /// prior process / invocation and their outbox record lives in
    /// the durable store.
    pub fn seed_delivered<I: IntoIterator<Item = TransitionSignature>>(&mut self, sigs: I) {
        for sig in sigs {
            self.delivered_sigs.insert(sig);
        }
    }

    /// Check whether this transition has already been notified.
    pub fn is_duplicate(&self, sig: &TransitionSignature) -> bool {
        self.delivered_sigs.contains(sig)
    }

    /// Record a successfully delivered notification in the outbox.
    pub fn record_delivered(&mut self, notification: PendingNotification) {
        self.delivered_sigs
            .insert(notification.transition_sig.clone());
        self.entries.push(OutboxEntry {
            notification,
            status: OutboxEntryStatus::Delivered,
        });
    }

    /// Record a delivery failure. Does NOT mark the signature as
    /// delivered so it can be retried (e.g. after a system restart).
    pub fn record_failed(&mut self, notification: PendingNotification, reason: String) {
        self.entries.push(OutboxEntry {
            notification,
            status: OutboxEntryStatus::DeliveryFailed(reason),
        });
    }

    /// Record a notification that was silently dropped by policy.
    pub fn record_refused(
        &mut self,
        notification: PendingNotification,
        refusal: NotificationRefusal,
    ) {
        self.entries.push(OutboxEntry {
            notification,
            status: OutboxEntryStatus::Refused(refusal),
        });
    }

    /// All entries (delivered, failed, refused), newest last.
    pub fn entries(&self) -> &[OutboxEntry] {
        &self.entries
    }

    /// Count of successfully delivered entries.
    pub fn delivered_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, OutboxEntryStatus::Delivered))
            .count()
    }

    /// Iterate all `Delivered` signatures recorded in this outbox.
    ///
    /// Used by the trigger boundary at the end of an evaluation to
    /// flush newly-delivered signatures into the durable
    /// [`NotificationOutboxStore`]. Refused and failed entries are
    /// not flushed because they did not produce a user-visible
    /// notification.
    pub fn delivered_signatures(&self) -> impl Iterator<Item = &TransitionSignature> {
        self.entries.iter().filter_map(|e| match &e.status {
            OutboxEntryStatus::Delivered => Some(&e.notification.transition_sig),
            _ => None,
        })
    }
}

// ── NotificationOutboxStore ───────────────────────────────────────────────────

/// Error returned by [`NotificationOutboxStore`] operations.
///
/// The dedupe store is best-effort: callers MUST log this error at
/// `warn` and continue with an empty seeded set / a no-op append.
/// Failing closed on a missing dedupe boundary would silence the
/// notification trigger entirely and violate the fail-open contract
/// for notification side effects.
#[derive(Debug, Error)]
#[error("notification outbox store unavailable: {reason}")]
pub struct NotificationOutboxStoreError {
    pub reason: String,
}

/// Durable, local-only dedupe boundary for the notification trigger.
///
/// ## Why this port exists
///
/// [`NotificationOutbox`] only dedupes within one in-memory instance
/// (see its dedupe-guarantee doc above). The event-driven trigger
/// boundary ([`evaluate_after_acp_append`] in `workhive-cli`) is
/// called once per ACP event append, so successive ACP events that
/// re-project the pane into the same notification-worthy state would
/// re-fire delivery if the outbox were not seeded from a persistent
/// source.
///
/// Implementations MUST:
///
/// - Persist signatures locally only (no network calls).
/// - Persist append-only (or otherwise crash-safe). The notification
///   trigger is allowed to crash between `record_delivered` and the
///   next read; on the next read the recovered set MUST include any
///   signature whose append returned `Ok(())`.
/// - Carry no prompt or message content. A [`TransitionSignature`]
///   is a metadata-only hash / id, see its docs.
/// - Fail open: I/O errors return
///   [`NotificationOutboxStoreError`] and the caller MUST continue
///   evaluation without the seeded set (worst case: a single
///   duplicate notification fires once after the corrupted read).
///
/// [`evaluate_after_acp_append`]: crate::ports::NotificationPort
pub trait NotificationOutboxStore: Send + Sync {
    /// Load the set of signatures that have been recorded as
    /// delivered by prior trigger invocations (this process or a
    /// previous one).
    ///
    /// Returns an empty `Vec` when the underlying store is missing
    /// (first run after install). Returns
    /// `Err(NotificationOutboxStoreError)` when the store exists
    /// but is unreadable; the caller treats that as "seed nothing"
    /// per the fail-open contract.
    fn load_delivered_signatures(
        &self,
    ) -> Result<Vec<TransitionSignature>, NotificationOutboxStoreError>;

    /// Record a successfully-delivered signature in the durable
    /// store. Implementations SHOULD use append-only semantics so a
    /// crash mid-write cannot corrupt previously-persisted entries.
    ///
    /// MUST be idempotent: the trigger may flush the same signature
    /// twice (e.g. when two ACP appends in close succession project
    /// the same final state); duplicate entries are harmless and
    /// MUST collapse on `load_delivered_signatures`.
    fn record_delivered(
        &self,
        sig: &TransitionSignature,
    ) -> Result<(), NotificationOutboxStoreError>;
}
