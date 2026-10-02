#[allow(unused_imports)]
use super::*;
use crate::commands::reconciliation_retry::{
    PublicationLeaseError, acquire_scheduling_lease_bounded,
};
#[allow(unused_imports)]
use crate::commands::*;

pub(crate) fn persist_trusted_root(
    project_root: &camino::Utf8Path,
    global_cfg: Option<&workhive_core::ports::AppConfig>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::ports::AppConfig;
    use workhive_core::usecases::trusted_roots::add_trusted_root;
    let existing: Vec<camino::Utf8PathBuf> =
        global_cfg.map(AppConfig::trusted_roots).unwrap_or_default();
    let (next_roots, _mutation) = add_trusted_root(&existing, project_root, home_path().as_deref());
    persist_trusted_roots_and_schedule(&next_roots, ctx)
}

/// Persist roots and request reconciliation under a bounded lease retry.
///
/// The production command path retries only the lease acquisition on
/// transient contention before any config write, revision read/write, or
/// worker launch. Once the lease is held, the config write and revision
/// allocation remain atomic. If revision persistence fails, the exact
/// pre-edit global TOML is restored. If the restore also fails, both
/// errors are preserved in one contextual error. The worker launches only
/// after the lease is dropped.
pub(crate) fn persist_trusted_roots_and_schedule(
    next_roots: &[camino::Utf8PathBuf],
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let config_dir = ctx
        .config
        .global_config_dir()
        .map_err(|e| CliError::Runtime(format!("reconciliation scheduling failed: {e}")))?;

    let reconciliation = ctx.composition.index_reconciliation();

    // Normalize roots before fingerprint computation to ensure scheduler and
    // worker use equivalent inputs. This prevents spurious revisions when
    // equivalent paths (e.g. ~/dev and /home/user/dev) would otherwise produce
    // different fingerprints.
    let home = home_path();
    let normalized_roots =
        workhive_core::domain::TrustedRootSet::from_raw(next_roots, home.as_deref())
            .roots()
            .to_vec();

    // Capture original TOML for rollback if revision persistence fails.
    // This must succeed before any config write; failure aborts with typed error.
    let original_toml = ctx.config.load_raw_toml(None, true).map_err(|e| {
        CliError::Runtime(format!("failed to capture original TOML for rollback: {e}"))
    })?;

    persist_trusted_roots_and_schedule_inner(
        &normalized_roots,
        &reconciliation,
        config_dir.as_ref(),
        &|d| std::thread::sleep(d),
        || super::roots::write_trusted_roots_into_global_toml(next_roots, ctx),
        |dir| {
            ctx.process_launcher
                .launch_reconciliation(dir)
                .map_err(|e| CliError::Runtime(format!("reconciliation spawn failed: {e}")))
        },
        || {
            // Rollback: restore original TOML while still holding the lease.
            if let Err(restore_err) = ctx.config.save_raw_toml(None, true, &original_toml) {
                return Err(CliError::Runtime(format!(
                    "config restore failed: {restore_err}"
                )));
            }
            Ok(())
        },
    )
}

/// Bounded-lease coordination: acquire → config write → revision persist →
/// release → conditional launch. When revision persistence fails,
/// rollback is called BEFORE releasing the lease. When launch fails after a
/// scheduled revision, the Failed state is persisted with the revision and
/// fingerprint so the failure is durable.
fn persist_trusted_roots_and_schedule_inner(
    next_roots: &[camino::Utf8PathBuf],
    reconciliation: &dyn workhive_core::ports::IndexReconciliationPort,
    config_dir: &camino::Utf8Path,
    wait: &dyn Fn(std::time::Duration),
    write_config: impl FnOnce() -> Result<(), CliError>,
    launch: impl FnOnce(&camino::Utf8Path) -> Result<(), CliError>,
    rollback: impl FnOnce() -> Result<(), CliError>,
) -> Result<(), CliError> {
    let lease = acquire_scheduling_lease_bounded(reconciliation, config_dir, wait).map_err(
        |e| match e {
            PublicationLeaseError::ContentionExhausted => {
                CliError::Runtime("reconciliation scheduling is already in progress".to_owned())
            }
            PublicationLeaseError::Io(err) => {
                CliError::Runtime(format!("failed to acquire scheduling lock: {err}"))
            }
        },
    )?;
    if let Err(write_err) = write_config() {
        // Config write failed; rollback before releasing lease.
        // Preserve both errors if rollback also fails.
        return match rollback() {
            Ok(()) => Err(write_err),
            Err(rollback_err) => Err(CliError::Runtime(format!("{write_err}; {rollback_err}"))),
        };
    }
    let scheduled =
        match persist_reconciliation_request_locked(reconciliation, config_dir, next_roots) {
            Ok(s) => s,
            Err(revision_err) => {
                // Revision persistence failed; rollback before releasing lease.
                let rollback_result = rollback();
                return match rollback_result {
                    Ok(()) => Err(revision_err),
                    Err(rollback_err) => {
                        Err(CliError::Runtime(format!("{revision_err}; {rollback_err}")))
                    }
                };
            }
        };
    drop(lease);
    match scheduled {
        Some((revision, fingerprint)) => match launch(config_dir) {
            Ok(()) => Ok(()),
            Err(launch_err) => {
                // Launch failed. Before persisting Failed state, reacquire
                // scheduling coordination to ensure we don't overwrite a
                // newer revision (N+1) that another scheduler may have
                // written while we were launching.
                let _coord_lease = match acquire_scheduling_lease_bounded(
                    reconciliation,
                    config_dir,
                    wait,
                )
                .map_err(|e| match e {
                    PublicationLeaseError::ContentionExhausted => CliError::Runtime(
                        "reconciliation scheduling is already in progress".to_owned(),
                    ),
                    PublicationLeaseError::Io(err) => {
                        CliError::Runtime(format!("failed to acquire scheduling lock: {err}"))
                    }
                }) {
                    Ok(l) => l,
                    Err(coord_err) => {
                        return Err(CliError::Runtime(format!(
                            "{launch_err}; failed to reacquire scheduling coordination: {coord_err}"
                        )));
                    }
                };
                // Read latest revision to check if it's still ours.
                let latest_revision = match reconciliation.read_latest_revision(config_dir) {
                    Ok(r) => r,
                    Err(read_err) => {
                        return Err(CliError::Runtime(format!(
                            "{launch_err}; failed to read latest revision: {read_err}"
                        )));
                    }
                };
                // Only write Failed state if latest revision matches our scheduled revision.
                // If another scheduler has written a newer revision, don't overwrite.
                // Keep coord_lease alive through the write to prevent race.
                let should_write_failed = latest_revision == Some(revision);
                if should_write_failed {
                    let failed_state =
                        workhive_core::domain::index_reconciliation::ReconciliationState::Failed {
                            revision,
                            fingerprint,
                            failed_at: std::time::SystemTime::now(),
                            error_detail: launch_err.to_string(),
                        };
                    match reconciliation.write_state(config_dir, &failed_state) {
                        Ok(()) => Err(launch_err),
                        Err(state_err) => Err(CliError::Runtime(format!(
                            "{launch_err}; failed to persist Failed state: {state_err}"
                        ))),
                    }
                } else {
                    // Newer revision exists; don't overwrite. Return original launch error.
                    Err(launch_err)
                }
                // coord_lease drops here, after the conditional write completes.
            }
        },
        None => Ok(()),
    }
}

#[cfg(test)]
fn acquire_scheduling_lease(
    reconciliation: &dyn workhive_core::ports::IndexReconciliationPort,
    config_dir: &camino::Utf8Path,
) -> Result<Box<dyn workhive_core::ports::ReconciliationLease>, CliError> {
    match reconciliation.try_acquire_scheduling_lease(config_dir) {
        Ok(workhive_core::ports::ReconciliationLeaseOutcome::Acquired(lease)) => Ok(lease),
        Ok(workhive_core::ports::ReconciliationLeaseOutcome::Contended) => Err(CliError::Runtime(
            "reconciliation scheduling is already in progress".to_owned(),
        )),
        Err(e) => Err(CliError::Runtime(format!(
            "failed to acquire scheduling lock: {e}"
        ))),
    }
}

/// Persist a durable reconciliation revision if the roots fingerprint changed.
/// Returns `Ok(true)` when a revision was persisted (worker launch warranted).
/// Returns `Ok(false)` only when the fingerprint is unchanged (legitimate no-op).
/// Schedule reconciliation atomically using a lock around read-decide-write.
#[cfg(test)]
pub(crate) fn try_schedule_reconciliation(
    config_dir: &camino::Utf8Path,
    roots: &[camino::Utf8PathBuf],
) -> Result<bool, CliError> {
    let reconciliation = workhive_infra::reconciliation::FsIndexReconciliation::new();
    let _lease = acquire_scheduling_lease(&reconciliation, config_dir)?;
    persist_reconciliation_request_locked(&reconciliation, config_dir, roots).map(|o| o.is_some())
}

fn persist_reconciliation_request_locked(
    reconciliation: &dyn workhive_core::ports::IndexReconciliationPort,
    config_dir: &camino::Utf8Path,
    roots: &[camino::Utf8PathBuf],
) -> Result<
    Option<(
        workhive_core::domain::index_reconciliation::ReconciliationRevision,
        workhive_core::domain::index_reconciliation::RootsFingerprint,
    )>,
    CliError,
> {
    use workhive_core::domain::index_reconciliation::decide_reconciliation;

    let last_rev = reconciliation
        .read_latest_revision(config_dir)
        .map_err(|e| CliError::Runtime(format!("failed to read reconciliation revision: {e}")))?;

    let last_fp = match reconciliation.read_state(config_dir) {
        Ok(workhive_core::domain::index_reconciliation::ReconciliationState::Completed {
            fingerprint,
            ..
        }) => Some(fingerprint),
        Ok(_) => None,
        Err(e) => {
            return Err(CliError::Runtime(format!(
                "failed to read reconciliation state: {e}"
            )));
        }
    };
    let last_fp_ref = last_fp.as_ref();

    // Normalize roots through TrustedRootSet to ensure scheduler and worker
    // compute equivalent fingerprints for the same logical root configuration.
    let home = home_path();
    let trusted_roots = workhive_core::domain::TrustedRootSet::from_raw(roots, home.as_deref());

    let decision = decide_reconciliation(&trusted_roots, last_rev, last_fp_ref).map_err(|e| {
        CliError::Runtime(format!("failed to allocate reconciliation revision: {e}"))
    })?;
    match decision {
        workhive_core::domain::index_reconciliation::ReconciliationDecision::Schedule {
            revision,
            fingerprint,
        } => {
            reconciliation
                .write_latest_revision(config_dir, revision)
                .map_err(|e| {
                    CliError::Runtime(format!("failed to persist reconciliation revision: {e}"))
                })?;
            Ok(Some((revision, fingerprint)))
        }
        workhive_core::domain::index_reconciliation::ReconciliationDecision::NoOp { .. } => {
            Ok(None)
        }
    }
}

#[cfg(test)]
#[path = "reconciliation_scheduling_tests.rs"]
mod tests;
