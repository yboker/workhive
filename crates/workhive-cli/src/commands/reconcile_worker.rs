//! Hidden `workhive internal reconcile` worker implementation.
//!
//! Acquires the inter-process reconciliation lock, scans configured roots via
//! `ProjectDiscoveryPort`, checks the publication decision against the latest
//! revision, and atomically publishes the fresh index via `ProjectIndexPort`.

use camino::Utf8PathBuf;
use workhive_core::domain::index_reconciliation::{
    PublicationDecision, ReconciliationRevision, ReconciliationState, RootsFingerprint,
    decide_publication,
};
use workhive_core::ports::{
    IndexReconciliationPort, ProjectDiscoveryPort, ProjectIndexPort, ProjectSummary,
    ReconciliationLease, ReconciliationLeaseOutcome,
};

use super::CommandContext;
use super::reconciliation_retry::{PublicationLeaseError, acquire_scheduling_lease_bounded};
use crate::error::CliError;

pub(crate) fn cmd_internal_reconcile(
    config_dir_str: &str,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let config_dir = Utf8PathBuf::from(config_dir_str);

    // Validate canonical match: the worker must operate on the same directory
    // as the foreground scheduler to prevent split-brain coordination.
    let canonical_dir = ctx.config.global_config_dir().map_err(|e| {
        CliError::Runtime(format!("failed to resolve canonical config directory: {e}"))
    })?;
    if config_dir != canonical_dir {
        return Err(CliError::Runtime(format!(
            "config directory mismatch: worker received '{}', canonical is '{}'",
            config_dir, canonical_dir
        )));
    }

    let discovery = ctx.composition.project_discovery();
    let index = ctx.composition.project_index()?;
    let reconciliation = ctx.composition.index_reconciliation();
    run_reconciliation_worker_with_roots_loader(
        config_dir.as_ref(),
        &|| load_trusted_roots(ctx),
        &discovery,
        &index as &dyn ProjectIndexPort,
        &reconciliation,
    )
}

/// Full worker lifecycle including lock acquisition.
///
/// Returns `Ok(())` only for legitimate contention. All I/O failures
/// (lock, revision, state persistence) propagate as `Err`.
pub(crate) fn run_reconciliation_worker_with_roots_loader(
    config_dir: &camino::Utf8Path,
    load_roots: &dyn Fn() -> Result<workhive_core::domain::TrustedRootSet, CliError>,
    discovery: &dyn ProjectDiscoveryPort,
    index: &dyn ProjectIndexPort,
    reconciliation: &dyn IndexReconciliationPort,
) -> Result<(), CliError> {
    let mut worker_lease = match reconciliation.try_acquire_worker_lease(config_dir) {
        Ok(ReconciliationLeaseOutcome::Acquired(lease)) => Some(lease),
        Ok(ReconciliationLeaseOutcome::Contended) => {
            tracing::debug!("reconciliation lock contended, exiting");
            return Ok(());
        }
        Err(e) => {
            return Err(CliError::Runtime(format!(
                "reconciliation lock I/O failure: {e}"
            )));
        }
    };

    execute_worker_cycle_with_roots_loader_and_reconciliation(
        config_dir,
        load_roots,
        discovery,
        index,
        reconciliation,
        Some(&mut worker_lease),
    )
}

/// Post-lock reconciliation cycle: revision read, scan-loop, publish.
///
/// Separated from lock acquisition to isolate the core convergence logic
/// from platform-specific advisory file locking.
#[cfg(test)]
pub(crate) fn execute_worker_cycle(
    config_dir: &camino::Utf8Path,
    roots: &[Utf8PathBuf],
    discovery: &dyn ProjectDiscoveryPort,
    index: &dyn ProjectIndexPort,
) -> Result<(), CliError> {
    execute_worker_cycle_with_roots_loader(
        config_dir,
        &|| Ok(workhive_core::domain::TrustedRootSet::from_raw(roots, None)),
        discovery,
        index,
    )
}

#[cfg(test)]
pub(crate) fn execute_worker_cycle_with_roots_loader(
    config_dir: &camino::Utf8Path,
    load_roots: &dyn Fn() -> Result<workhive_core::domain::TrustedRootSet, CliError>,
    discovery: &dyn ProjectDiscoveryPort,
    index: &dyn ProjectIndexPort,
) -> Result<(), CliError> {
    let reconciliation = workhive_infra::reconciliation::FsIndexReconciliation::new();
    execute_worker_cycle_with_roots_loader_and_reconciliation(
        config_dir,
        load_roots,
        discovery,
        index,
        &reconciliation,
        None,
    )
}

pub(crate) fn execute_worker_cycle_with_roots_loader_and_reconciliation(
    config_dir: &camino::Utf8Path,
    load_roots: &dyn Fn() -> Result<workhive_core::domain::TrustedRootSet, CliError>,
    discovery: &dyn ProjectDiscoveryPort,
    index: &dyn ProjectIndexPort,
    reconciliation: &dyn IndexReconciliationPort,
    worker_lease: Option<&mut Option<Box<dyn ReconciliationLease>>>,
) -> Result<(), CliError> {
    let target_revision = match reconciliation.read_latest_revision(config_dir) {
        Ok(Some(rev)) => rev,
        Ok(None) => {
            tracing::debug!("no reconciliation revision requested, exiting");
            return Ok(());
        }
        Err(e) => {
            return Err(CliError::Runtime(format!(
                "failed to read reconciliation revision: {e}"
            )));
        }
    };

    scan_and_publish_with_reconciliation(
        config_dir,
        load_roots,
        discovery,
        index,
        target_revision,
        reconciliation,
        worker_lease,
    )
}

/// Scan-publish loop: scans, re-reads the current revision, and publishes
/// only when the scan matches the latest revision. Loops on stale results
/// until convergence or failure.
#[cfg(test)]
pub(crate) fn scan_and_publish(
    config_dir: &camino::Utf8Path,
    load_roots: &dyn Fn() -> Result<workhive_core::domain::TrustedRootSet, CliError>,
    discovery: &dyn ProjectDiscoveryPort,
    index: &dyn ProjectIndexPort,
    target_revision: ReconciliationRevision,
) -> Result<(), CliError> {
    let reconciliation = workhive_infra::reconciliation::FsIndexReconciliation::new();
    scan_and_publish_with_reconciliation(
        config_dir,
        load_roots,
        discovery,
        index,
        target_revision,
        &reconciliation,
        None,
    )
}

fn scan_and_publish_with_reconciliation(
    config_dir: &camino::Utf8Path,
    load_roots: &dyn Fn() -> Result<workhive_core::domain::TrustedRootSet, CliError>,
    discovery: &dyn ProjectDiscoveryPort,
    index: &dyn ProjectIndexPort,
    target_revision: ReconciliationRevision,
    reconciliation: &dyn IndexReconciliationPort,
    worker_lease: Option<&mut Option<Box<dyn ReconciliationLease>>>,
) -> Result<(), CliError> {
    scan_and_publish_with_reconciliation_kernel(
        ScanAndPublishInput {
            config_dir,
            load_roots,
            discovery,
            index,
            target_revision,
            reconciliation,
            worker_lease,
        },
        &std::thread::sleep,
    )
}

/// Dependencies and request state for one scan-and-publish operation.
pub(crate) struct ScanAndPublishInput<'a> {
    config_dir: &'a camino::Utf8Path,
    load_roots: &'a dyn Fn() -> Result<workhive_core::domain::TrustedRootSet, CliError>,
    discovery: &'a dyn ProjectDiscoveryPort,
    index: &'a dyn ProjectIndexPort,
    target_revision: ReconciliationRevision,
    reconciliation: &'a dyn IndexReconciliationPort,
    worker_lease: Option<&'a mut Option<Box<dyn ReconciliationLease>>>,
}

/// Execute one scan-and-publish operation with bounded coordination waits.
///
/// Lease contention retries only coordination acquisition. Roots loading,
/// `Updating` persistence, and discovery each run once per operation.
pub(crate) fn scan_and_publish_with_reconciliation_kernel(
    mut input: ScanAndPublishInput<'_>,
    wait: &dyn Fn(std::time::Duration),
) -> Result<(), CliError> {
    let mut last_fingerprint = None;
    loop {
        // Capture the snapshot token before discovery for optimistic concurrency
        let baseline_token = match input.index.capture_snapshot_token() {
            Ok(token) => token,
            Err(e) => {
                let fingerprint = last_fingerprint.unwrap_or_else(|| {
                    RootsFingerprint::from_trusted_roots(
                        &workhive_core::domain::TrustedRootSet::default(),
                    )
                });
                let failed_state = ReconciliationState::Failed {
                    revision: input.target_revision,
                    fingerprint,
                    failed_at: std::time::SystemTime::now(),
                    error_detail: format!("failed to capture snapshot token: {e}"),
                };
                persist_state(input.reconciliation, input.config_dir, &failed_state)?;
                return Err(CliError::Runtime(format!(
                    "failed to capture snapshot token: {e}"
                )));
            }
        };

        let roots = match (input.load_roots)() {
            Ok(roots) => roots,
            Err(error) => {
                let fingerprint = last_fingerprint.unwrap_or_else(|| {
                    RootsFingerprint::from_trusted_roots(
                        &workhive_core::domain::TrustedRootSet::default(),
                    )
                });
                let failed_state = ReconciliationState::Failed {
                    revision: input.target_revision,
                    fingerprint,
                    failed_at: std::time::SystemTime::now(),
                    error_detail: format!("trusted roots reload failed: {error}"),
                };
                persist_state(input.reconciliation, input.config_dir, &failed_state)?;
                return Err(error);
            }
        };
        let fingerprint = RootsFingerprint::from_trusted_roots(&roots);
        last_fingerprint = Some(fingerprint.clone());

        let updating_state = ReconciliationState::Updating {
            revision: input.target_revision,
            fingerprint: fingerprint.clone(),
            started_at: std::time::SystemTime::now(),
        };
        persist_state(input.reconciliation, input.config_dir, &updating_state)?;

        let discovered = run_discovery(
            input.reconciliation,
            input.config_dir,
            &roots,
            input.discovery,
            input.target_revision,
            &fingerprint,
        )?;

        // Acquire the scheduling lease in a bounded inner loop. Contention
        // retries only the lease acquisition and do not reload roots, rewrite
        // Updating, or re-run discovery. Once acquired, revision and
        // authoritative roots are revalidated atomically before publication.
        // If either changed, the lease is released and the outer scan loop
        // resumes with the new target.
        let scheduling_lease =
            match acquire_scheduling_lease_bounded(input.reconciliation, input.config_dir, wait) {
                Ok(lease) => lease,
                Err(PublicationLeaseError::ContentionExhausted) => {
                    let detail = "publication coordination lease remained contended";
                    let failed_state = ReconciliationState::Failed {
                        revision: input.target_revision,
                        fingerprint,
                        failed_at: std::time::SystemTime::now(),
                        error_detail: detail.to_owned(),
                    };
                    persist_state(input.reconciliation, input.config_dir, &failed_state)?;
                    return Err(CliError::Runtime(detail.to_owned()));
                }
                Err(PublicationLeaseError::Io(err)) => {
                    let detail = format!("failed to acquire publication lock: {err}");
                    let failed_state = ReconciliationState::Failed {
                        revision: input.target_revision,
                        fingerprint,
                        failed_at: std::time::SystemTime::now(),
                        error_detail: detail.clone(),
                    };
                    if let Err(persist_err) =
                        persist_state(input.reconciliation, input.config_dir, &failed_state)
                    {
                        return Err(CliError::Runtime(format!(
                            "{detail}; failed to persist Failed state: {persist_err}"
                        )));
                    }
                    return Err(CliError::Runtime(detail));
                }
            };

        let current_revision = match input.reconciliation.read_latest_revision(input.config_dir) {
            Ok(Some(rev)) => rev,
            Ok(None) => {
                let detail = "reconciliation revision disappeared during scan";
                let failed_state = ReconciliationState::Failed {
                    revision: input.target_revision,
                    fingerprint,
                    failed_at: std::time::SystemTime::now(),
                    error_detail: detail.to_owned(),
                };
                persist_state(input.reconciliation, input.config_dir, &failed_state)?;
                return Err(CliError::Runtime(detail.to_owned()));
            }
            Err(e) => {
                let failed_state = ReconciliationState::Failed {
                    revision: input.target_revision,
                    fingerprint,
                    failed_at: std::time::SystemTime::now(),
                    error_detail: format!("revision re-read failed: {e}"),
                };
                persist_state(input.reconciliation, input.config_dir, &failed_state).map_err(
                    |se| {
                        CliError::Runtime(format!(
                            "revision re-read failed ({e}) and state persistence also failed: {se}"
                        ))
                    },
                )?;
                return Err(CliError::Runtime(format!("revision re-read failed: {e}")));
            }
        };

        match decide_publication(input.target_revision, current_revision) {
            PublicationDecision::Publish => {
                let fresh_roots = match (input.load_roots)() {
                    Ok(roots) => roots,
                    Err(error) => {
                        let failed_state = ReconciliationState::Failed {
                            revision: input.target_revision,
                            fingerprint,
                            failed_at: std::time::SystemTime::now(),
                            error_detail: format!("trusted roots reload failed: {error}"),
                        };
                        persist_state(input.reconciliation, input.config_dir, &failed_state)?;
                        return Err(error);
                    }
                };
                let fresh_fingerprint = RootsFingerprint::from_trusted_roots(&fresh_roots);
                if fresh_fingerprint != fingerprint {
                    tracing::debug!("roots changed during scan, discarding stale result");
                    drop(scheduling_lease);
                    input.target_revision = current_revision;
                    continue;
                }
                match publish_results(
                    input.reconciliation,
                    input.config_dir,
                    input.index,
                    &discovered,
                    input.target_revision,
                    fingerprint,
                    baseline_token,
                )? {
                    PublicationResult::Published => {
                        if let Some(lease_slot) = input.worker_lease.as_deref_mut() {
                            drop(lease_slot.take());
                        }
                        drop(scheduling_lease);
                        return Ok(());
                    }
                    PublicationResult::Stale => {
                        // Another mutation occurred after we captured the snapshot token.
                        // Discard the snapshot and rescan with a new token.
                        tracing::debug!(
                            "snapshot token changed during scan, discarding stale result"
                        );
                        drop(scheduling_lease);
                        // Use the latest revision for the next iteration
                        input.target_revision = current_revision;
                        // Continue the loop to rescan with a fresh token
                        continue;
                    }
                }
            }
            PublicationDecision::Discard { reason } => {
                tracing::debug!(reason, "discarding stale scan, looping");
                drop(scheduling_lease);
                input.target_revision = current_revision;
            }
        }
    }
}

/// Run discovery and convert projects, handling failure with state persistence.
fn run_discovery(
    reconciliation: &dyn IndexReconciliationPort,
    config_dir: &camino::Utf8Path,
    roots: &workhive_core::domain::TrustedRootSet,
    discovery: &dyn ProjectDiscoveryPort,
    revision: ReconciliationRevision,
    fingerprint: &RootsFingerprint,
) -> Result<Vec<ProjectSummary>, CliError> {
    match discovery.discover_projects_report(roots.roots()) {
        Ok(report) => Ok(report
            .projects
            .into_iter()
            .map(|d| ProjectSummary {
                project_name: d.project_name,
                project_root: d.project_root.clone(),
                config_path: d.project_root.join(".workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            })
            .collect()),
        Err(e) => {
            let failed_state = ReconciliationState::Failed {
                revision,
                fingerprint: fingerprint.clone(),
                failed_at: std::time::SystemTime::now(),
                error_detail: e.to_string(),
            };
            persist_state(reconciliation, config_dir, &failed_state).map_err(|se| {
                CliError::Runtime(format!(
                    "discovery failed ({e}) and state persistence also failed: {se}"
                ))
            })?;
            Err(CliError::Runtime(format!("discovery scan failed: {e}")))
        }
    }
}

/// Outcome of a publication attempt.
enum PublicationResult {
    /// The snapshot was published successfully.
    Published,
    /// The snapshot was stale — another mutation occurred. Caller should rescan.
    Stale,
}

fn publish_results(
    reconciliation: &dyn IndexReconciliationPort,
    config_dir: &camino::Utf8Path,
    index: &dyn ProjectIndexPort,
    discovered: &[ProjectSummary],
    revision: ReconciliationRevision,
    fingerprint: RootsFingerprint,
    baseline_token: workhive_core::ports::IndexSnapshotToken,
) -> Result<PublicationResult, CliError> {
    use workhive_core::ports::SnapshotPublishOutcome;

    match index.publish_snapshot_conditional(discovered, baseline_token) {
        Ok(SnapshotPublishOutcome::Published) => {
            let completed_state = ReconciliationState::Completed {
                revision,
                fingerprint,
                completed_at: std::time::SystemTime::now(),
                discovered_count: discovered.len(),
            };
            persist_state(reconciliation, config_dir, &completed_state)?;
            tracing::debug!(
                count = discovered.len(),
                revision = revision.value(),
                "reconciliation completed"
            );
            Ok(PublicationResult::Published)
        }
        Ok(SnapshotPublishOutcome::Stale) => {
            // Another mutation occurred after we captured the snapshot token.
            // Signal the caller to rescan.
            Ok(PublicationResult::Stale)
        }
        Err(e) => {
            let failed_state = ReconciliationState::Failed {
                revision,
                fingerprint,
                failed_at: std::time::SystemTime::now(),
                error_detail: e.to_string(),
            };
            persist_state(reconciliation, config_dir, &failed_state).map_err(|se| {
                CliError::Runtime(format!(
                    "publication failed ({e}) and state persistence also failed: {se}"
                ))
            })?;
            Err(CliError::Runtime(format!("index publication failed: {e}")))
        }
    }
}

/// Persist reconciliation state, mapping I/O failure to `CliError`.
fn persist_state(
    reconciliation: &dyn IndexReconciliationPort,
    config_dir: &camino::Utf8Path,
    state: &ReconciliationState,
) -> Result<(), CliError> {
    reconciliation
        .write_state(config_dir, state)
        .map_err(|e| CliError::Runtime(format!("failed to persist reconciliation state: {e}")))
}

fn load_trusted_roots(
    ctx: &CommandContext,
) -> Result<workhive_core::domain::TrustedRootSet, CliError> {
    use workhive_core::usecases::trusted_roots::trusted_roots_from_config;

    let global_cfg = ctx.config.load_global().map_err(|e| {
        CliError::Runtime(format!(
            "failed to load global config for reconciliation: {e}"
        ))
    })?;
    let home = std::env::var("HOME")
        .ok()
        .and_then(|h| Utf8PathBuf::try_from(std::path::PathBuf::from(h)).ok());
    let trusted = trusted_roots_from_config(global_cfg.as_ref(), home.as_deref());
    Ok(trusted)
}

#[cfg(test)]
#[path = "reconcile_worker_tests.rs"]
mod tests;
