use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::domain::{
    RuntimeBackendId, RuntimeBinding, SessionAvailability, WorkspaceId, WorkspaceIdentityRecord,
    WorkspaceProvenance,
};
use workhive_core::ports::{
    CreateWorkspaceRuntimeRequest, PaneLaunchIdentityPort, WorkspaceIdentityPort,
};
use workhive_core::usecases;

use crate::commands::{
    apply_layout_and_register, resolve_interaction_policy, resolve_open_effective_env,
};
use crate::context::CommandContext;
use crate::error::CliError;

fn workspace_cwd(
    root: &Utf8Path,
    record: &workhive_core::domain::WorkspaceIdentityRecord,
) -> Utf8PathBuf {
    let mut cwd = workspace_root(root, record);
    if let Some(relative) = &record.relative_project_path {
        cwd.push(relative.as_str());
    }
    cwd
}

fn workspace_root(
    root: &Utf8Path,
    record: &workhive_core::domain::WorkspaceIdentityRecord,
) -> Utf8PathBuf {
    if record.relative_display_path.is_none() {
        root.to_path_buf()
    } else {
        root.join(".workhive")
            .join("worktrees")
            .join(&record.workspace)
    }
}

struct PreparedProjectBackend {
    root: Utf8PathBuf,
    identity: workhive_infra::workspace_identity::FileIdentityAdapter,
    records: Vec<WorkspaceIdentityRecord>,
    targets: Vec<(
        workhive_core::domain::WorkspaceIdentityKey,
        RuntimeBinding,
        bool,
    )>,
}

fn cleanup_binding(
    ctx: &CommandContext,
    launch_identity: &dyn PaneLaunchIdentityPort,
    binding: &RuntimeBinding,
) -> Result<(), CliError> {
    match ctx
        .composition
        .runtime_for_binding(binding)?
        .destroy_workspace_binding(binding)
    {
        Ok(()) | Err(workhive_core::ports::RuntimeError::WorkspaceNotFound { .. }) => {}
        Err(error) => return Err(error.into()),
    }
    launch_identity
        .forget_workspace(&binding.scope_id, &binding.workspace_id)
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    Ok(())
}

fn prepare_project_targets(
    ctx: &CommandContext,
    root: &Utf8Path,
    target_backend: &RuntimeBackendId,
) -> Result<Option<PreparedProjectBackend>, CliError> {
    let identity = ctx.composition.workspace_identity(root);
    let records = identity.list_workspace_identities()?;
    if records.is_empty()
        || records
            .iter()
            .all(|record| &record.runtime_binding.backend == target_backend)
    {
        return Ok(None);
    }
    if records.iter().any(|record| {
        record.runtime_replacement.is_some()
            || record.runtime_binding.backend != records[0].runtime_binding.backend
    }) {
        return Err(CliError::Runtime(
            "project runtime replacement is pending or has mixed backends; recover it before changing backend"
                .to_owned(),
        ));
    }

    let project = identity.load_project_identity()?.ok_or_else(|| {
        CliError::Runtime("project identity is missing; run project init first".to_owned())
    })?;
    let naming_policy = ctx.naming_policy_for(Some(root))?;
    let target_runtime = ctx.composition.runtime_for_backend(target_backend)?;
    let interaction = resolve_interaction_policy(&ctx.config, Some(root))?;
    let env = resolve_open_effective_env(ctx, root)
        .0
        .entries
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect::<Vec<_>>();
    let launch_identity = ctx.composition.pane_launch_identity_store(root);
    let mut targets = Vec::with_capacity(records.len());

    for record in &records {
        let result = (|| {
            let cwd = workspace_cwd(root, record);
            let workspace_root = workspace_root(root, record);
            if !cwd.is_dir() {
                return Err(CliError::Runtime(format!(
                    "workspace replacement path is not a directory: {cwd}"
                )));
            }
            let naming = usecases::naming::assemble_naming_context(
                &project.project_display,
                &record.workspace,
                &project.project_root,
                None,
                Some(project.instance_id.clone()),
                record.relative_project_path.clone(),
            );
            let created = target_runtime.create_workspace_binding_with_label(
                CreateWorkspaceRuntimeRequest {
                    workspace_name: naming_policy.resolve_session(&naming)?,
                    group: naming_policy.resolve_server(&naming)?,
                    working_dir: cwd.clone(),
                    env: env.clone(),
                },
                &project.project_display,
            )?;
            let binding = match target_runtime.register_workspace_checkout(
                workhive_core::ports::RegisterWorkspaceCheckoutRequest {
                    binding: created.clone(),
                    project_root: root.to_path_buf(),
                    workspace_root: workspace_root.clone(),
                },
            ) {
                Ok(binding) => binding,
                Err(error) => {
                    let _ = cleanup_binding(ctx, &launch_identity, &created);
                    return Err(error.into());
                }
            };
            if binding != created {
                cleanup_binding(ctx, &launch_identity, &created)?;
            }
            let binding_owned = binding == created;
            let validation = (|| {
                target_runtime.configure_workspace_binding(&binding, &interaction)?;
                let plan = usecases::open::OpenPlan {
                    workspace: WorkspaceId::new(record.workspace.clone())?,
                    provenance: WorkspaceProvenance::ProjectRoot,
                    server: binding.scope_id.clone(),
                    session: binding.workspace_id.clone(),
                    session_availability: SessionAvailability::Missing,
                    project_root: root.to_path_buf(),
                    workspace_root,
                    working_dir: cwd,
                    defer_checkout_registration: false,
                    relative_project_path: record.relative_project_path.clone(),
                    open_mode: usecases::open::OpenMode::ForceRecreateFromLayout,
                    env: env.clone(),
                    interaction_policy: interaction,
                };
                let layout = apply_layout_and_register(ctx, target_runtime.as_ref(), &plan, root)?;
                let units = target_runtime.list_units(&binding.scope_id, &binding.workspace_id)?;
                if !layout.degraded_launches.is_empty()
                    || layout.layout_pane_count != Some(units.len() as u32)
                {
                    return Err(CliError::Runtime(format!(
                        "backend replacement target validation failed for '{}'",
                        record.workspace
                    )));
                }
                Ok(())
            })();
            if let Err(error) = validation {
                if binding_owned {
                    let _ = cleanup_binding(ctx, &launch_identity, &binding);
                }
                return Err(error);
            }
            Ok((record.identity_key(), binding, binding_owned))
        })();
        match result {
            Ok(target) => targets.push(target),
            Err(error) => {
                for (_, binding, owned) in &targets {
                    if *owned {
                        let _ = cleanup_binding(ctx, &launch_identity, binding);
                    }
                }
                return Err(error);
            }
        }
    }

    Ok(Some(PreparedProjectBackend {
        root: root.to_path_buf(),
        identity,
        records,
        targets,
    }))
}

fn cleanup_project_targets(ctx: &CommandContext, projects: &[PreparedProjectBackend]) {
    for project in projects {
        let launch_identity = ctx.composition.pane_launch_identity_store(&project.root);
        for (_, binding, owned) in &project.targets {
            if *owned {
                let _ = cleanup_binding(ctx, &launch_identity, binding);
            }
        }
    }
}

pub(crate) fn replace_project_backend(
    ctx: &CommandContext,
    root: &Utf8Path,
    target_backend: &RuntimeBackendId,
    updated_config: &str,
    previous_config: &str,
) -> Result<(), CliError> {
    let Some(mut project) = prepare_project_targets(ctx, root, target_backend)? else {
        ctx.config
            .save_raw_toml(Some(root), false, updated_config)?;
        return Ok(());
    };
    let launch_identity = ctx.composition.pane_launch_identity_store(root);
    usecases::identity::replace_project_runtime_bindings(
        &project.identity,
        &project.records,
        |record| {
            let index = project
                .targets
                .iter()
                .position(|(key, _, _)| key == &record.identity_key())
                .expect("prepared target must match every workspace");
            Ok(project.targets.remove(index).1)
        },
        || {
            ctx.config
                .save_raw_toml(Some(root), false, updated_config)?;
            Ok(())
        },
        || {
            ctx.config
                .save_raw_toml(Some(root), false, previous_config)?;
            Ok(())
        },
        |binding| cleanup_binding(ctx, &launch_identity, binding),
    )?;
    Ok(())
}

pub(crate) fn replace_global_backend(
    ctx: &CommandContext,
    roots: &[Utf8PathBuf],
    target_backend: &RuntimeBackendId,
    updated_config: &str,
    previous_config: &str,
) -> Result<(), CliError> {
    let mut projects = Vec::new();
    for root in roots {
        match prepare_project_targets(ctx, root, target_backend) {
            Ok(Some(project)) => projects.push(project),
            Ok(None) => {}
            Err(error) => {
                cleanup_project_targets(ctx, &projects);
                return Err(error);
            }
        }
    }

    let mut staged = Vec::with_capacity(projects.len());
    for project in &projects {
        let targets = project
            .targets
            .iter()
            .map(|(key, binding, _)| (key.clone(), binding.clone()))
            .collect::<Vec<_>>();
        match usecases::identity::prepare_project_runtime_replacements(
            &project.identity,
            &project.records,
            &targets,
        ) {
            Ok(records) => staged.push(records),
            Err(error) => {
                for (prepared, project) in staged.iter().zip(&projects) {
                    let _ = usecases::identity::rollback_project_runtime_replacements(
                        &project.identity,
                        prepared,
                    );
                }
                cleanup_project_targets(ctx, &projects);
                return Err(error.into());
            }
        }
    }
    if let Err(error) = ctx.config.save_raw_toml(None, true, updated_config) {
        for (prepared, project) in staged.iter().zip(&projects) {
            let _ = usecases::identity::rollback_project_runtime_replacements(
                &project.identity,
                prepared,
            );
        }
        cleanup_project_targets(ctx, &projects);
        return Err(error.into());
    }

    let mut committed = Vec::with_capacity(projects.len());
    for (prepared, project) in staged.iter().zip(&projects) {
        match usecases::identity::commit_project_runtime_replacements(&project.identity, prepared) {
            Ok(records) => committed.push(records),
            Err(error) => {
                let _ = ctx.config.save_raw_toml(None, true, previous_config);
                for (state, project) in staged.iter().zip(&projects) {
                    let _ = usecases::identity::rollback_project_runtime_replacements(
                        &project.identity,
                        state,
                    );
                }
                cleanup_project_targets(ctx, &projects);
                return Err(error.into());
            }
        }
    }

    for ((records, project), committed) in projects
        .iter()
        .map(|p| &p.records)
        .zip(&projects)
        .zip(&committed)
    {
        let launch_identity = ctx.composition.pane_launch_identity_store(&project.root);
        for record in records {
            cleanup_binding(ctx, &launch_identity, &record.runtime_binding)?;
        }
        usecases::identity::finish_project_runtime_replacements(&project.identity, committed)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::WorkspaceIdentityRecord;

    #[test]
    fn replacement_cwd_uses_root_or_managed_worktree_and_subdir() {
        let mut record = WorkspaceIdentityRecord {
            workspace: "feature-x".to_owned(),
            relative_project_path: None,
            relative_display_path: None,
            canonical_workspace_component: "feature-x".to_owned(),
            runtime_binding: RuntimeBinding::tmux("server", "session"),
            runtime_replacement: None,
            canonical_session_name: "session".to_owned(),
            canonical_server_name: "server".to_owned(),
        };
        assert_eq!(
            workspace_cwd(Utf8Path::new("/repo"), &record),
            Utf8Path::new("/repo")
        );

        record.relative_display_path = Some("repo/api/feature-x".to_owned());
        record.relative_project_path =
            Some(workhive_core::domain::RelativeProjectPath::new("api").unwrap());
        assert_eq!(
            workspace_cwd(Utf8Path::new("/repo"), &record),
            Utf8Path::new("/repo/.workhive/worktrees/feature-x/api")
        );
    }
}
