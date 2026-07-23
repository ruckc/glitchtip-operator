use kube::ResourceExt;
use kube::api::{Api, Patch, PatchParams};
use kube::core::Resource as _;
use kube::runtime::controller::Action;
use kube::runtime::finalizer::{Event, finalizer};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use crate::context::Ctx;
use crate::crds::common::{DeletionPolicy, is_condition_true, set_condition};
use crate::crds::project::GlitchTipProjectStatus;
use crate::crds::{GlitchTipProject, GlitchTipTeam};
use crate::error::{Error, Result};
use crate::glitchtip::{ApiError, GlitchTipClient, ProjectKey};
use crate::util::secrets;

use super::team::org_chain;

pub const FINALIZER: &str = "glitchtip.ruck.io/project-cleanup";

pub async fn reconcile(project: Arc<GlitchTipProject>, ctx: Arc<Ctx>) -> Result<Action> {
    let ns = project.namespace().unwrap_or_default();
    let api: Api<GlitchTipProject> = Api::namespaced(ctx.client.clone(), &ns);
    finalizer(&api, FINALIZER, project, |event| async {
        match event {
            Event::Apply(project) => apply(project, ctx.clone()).await,
            Event::Cleanup(project) => cleanup(project, ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::Finalizer(Box::new(e)))
}

pub fn error_policy(_p: Arc<GlitchTipProject>, error: &Error, _ctx: Arc<Ctx>) -> Action {
    tracing::warn!(%error, "project reconcile failed");
    Action::requeue(Duration::from_secs(30))
}

enum TeamResolution {
    Slug(String),
    Waiting(String),
    Mismatch(String),
    Unspecified,
}

async fn resolve_team_slug(
    ctx: &Ctx,
    project: &GlitchTipProject,
    org_slug: &str,
) -> Result<TeamResolution> {
    let ns = project.namespace().unwrap_or_default();
    if let Some(team_ref) = &project.spec.team_ref {
        let team_ns = team_ref.namespace_or(&ns).to_string();
        let teams: Api<GlitchTipTeam> = Api::namespaced(ctx.client.clone(), &team_ns);
        let Some(team) = teams.get_opt(&team_ref.name).await? else {
            return Ok(TeamResolution::Waiting(format!(
                "GlitchTipTeam {team_ns}/{} not found",
                team_ref.name
            )));
        };
        let status = team.status.clone().unwrap_or_default();
        let ready = is_condition_true(&status.conditions, "Ready");
        let (Some(slug), true) = (status.slug, ready) else {
            return Ok(TeamResolution::Waiting(format!(
                "GlitchTipTeam {team_ns}/{} is not ready",
                team_ref.name
            )));
        };
        if status.organization_slug.as_deref() != Some(org_slug) {
            return Ok(TeamResolution::Mismatch(format!(
                "team {team_ns}/{} belongs to organization {:?}, not {org_slug}",
                team_ref.name, status.organization_slug
            )));
        }
        return Ok(TeamResolution::Slug(slug));
    }
    if let Some(slug) = &project.spec.team_slug {
        return Ok(TeamResolution::Slug(slug.clone()));
    }
    Ok(TeamResolution::Unspecified)
}

async fn ensure_key(
    client: &GlitchTipClient,
    org_slug: &str,
    project_slug: &str,
) -> Result<ProjectKey> {
    let keys = client.list_keys(org_slug, project_slug).await?;
    if let Some(key) = keys
        .into_iter()
        .find(|k| k.dsn.as_ref().and_then(|d| d.public.as_ref()).is_some())
    {
        return Ok(key);
    }
    Ok(client
        .create_key(org_slug, project_slug, "glitchtip-operator")
        .await?)
}

async fn apply(project: Arc<GlitchTipProject>, ctx: Arc<Ctx>) -> Result<Action> {
    let ns = project.namespace().unwrap_or_default();
    let name = project.name_any();
    let generation = project.metadata.generation;
    let mut status = project.status.clone().unwrap_or_default();
    status.observed_generation = generation;
    let conds = &mut status.conditions;

    let org_ns = project.spec.organization_ref.namespace_or(&ns).to_string();
    let chain = org_chain(&ctx, &org_ns, &project.spec.organization_ref.name).await?;
    let Some((_org, org_slug, client)) = chain else {
        set_condition(
            conds,
            "Ready",
            false,
            "WaitingForOrganization",
            &format!(
                "organization {org_ns}/{} is not ready",
                project.spec.organization_ref.name
            ),
            generation,
        );
        write_status(&ctx, &ns, &name, &status).await?;
        return Ok(Action::requeue(Duration::from_secs(20)));
    };

    let team_slug = match resolve_team_slug(&ctx, &project, &org_slug).await? {
        TeamResolution::Slug(slug) => slug,
        TeamResolution::Waiting(msg) => {
            set_condition(conds, "Ready", false, "WaitingForTeam", &msg, generation);
            write_status(&ctx, &ns, &name, &status).await?;
            return Ok(Action::requeue(Duration::from_secs(20)));
        }
        TeamResolution::Mismatch(msg) => {
            set_condition(conds, "Ready", false, "TeamOrgMismatch", &msg, generation);
            write_status(&ctx, &ns, &name, &status).await?;
            return Ok(Action::requeue(Duration::from_secs(60)));
        }
        TeamResolution::Unspecified => {
            set_condition(
                conds,
                "Ready",
                false,
                "InvalidSpec",
                "one of spec.teamRef or spec.teamSlug is required",
                generation,
            );
            write_status(&ctx, &ns, &name, &status).await?;
            return Ok(Action::requeue(Duration::from_secs(300)));
        }
    };

    let slug = project.desired_slug();
    let remote = match client.get_project(&org_slug, &slug).await {
        Ok(remote) => remote,
        Err(ApiError::NotFound) => {
            client
                .create_project(
                    &org_slug,
                    &team_slug,
                    &project.display_name(),
                    Some(&slug),
                    project.spec.platform.as_deref(),
                )
                .await?
        }
        Err(e) => return Err(e.into()),
    };

    let key = ensure_key(&client, &org_slug, &remote.slug).await?;
    let dsn = key
        .dsn
        .as_ref()
        .and_then(|d| d.public.clone())
        .ok_or_else(|| Error::Config("project key has no public DSN".into()))?;

    // DSN Secret in the project's own namespace; ownerRef gives GC for free
    // and SSA reverts manual edits on the next reconcile.
    let secret_name = project.dsn_secret_name();
    let mut data = BTreeMap::from([(project.dsn_key(), dsn.clone())]);
    for extra in &project.spec.secret.extra_dsn_keys {
        data.insert(extra.clone(), dsn.clone());
    }
    if project
        .spec
        .secret
        .include_security_endpoint
        .unwrap_or(true)
        && let Some(security) = key.dsn.as_ref().and_then(|d| d.security.clone())
    {
        data.insert("SENTRY_SECURITY_ENDPOINT".to_string(), security);
    }
    let mut labels = BTreeMap::from([
        (
            "app.kubernetes.io/name".to_string(),
            "glitchtip".to_string(),
        ),
        (
            "app.kubernetes.io/managed-by".to_string(),
            "glitchtip-operator".to_string(),
        ),
    ]);
    labels.extend(project.spec.secret.labels.clone());
    secrets::apply_secret(
        &ctx.client,
        &ns,
        &secret_name,
        data,
        labels,
        project.spec.secret.annotations.clone(),
        project.controller_owner_ref(&()),
    )
    .await?;

    status.slug = Some(remote.slug.clone());
    status.id = remote.id.clone();
    status.organization_slug = Some(org_slug);
    status.team_slug = Some(team_slug);
    status.key_id = key.id.clone();
    status.secret_name = Some(secret_name);
    set_condition(
        conds,
        "Ready",
        true,
        "Reconciled",
        "project and DSN secret exist",
        generation,
    );
    write_status(&ctx, &ns, &name, &status).await?;
    Ok(Action::requeue(Duration::from_secs(300)))
}

async fn cleanup(project: Arc<GlitchTipProject>, ctx: Arc<Ctx>) -> Result<Action> {
    if project.spec.deletion_policy == DeletionPolicy::Retain {
        return Ok(Action::await_change());
    }
    let ns = project.namespace().unwrap_or_default();
    let org_ns = project.spec.organization_ref.namespace_or(&ns).to_string();
    let Some(org_slug) = project
        .status
        .as_ref()
        .and_then(|s| s.organization_slug.clone())
    else {
        return Ok(Action::await_change());
    };
    match org_chain(&ctx, &org_ns, &project.spec.organization_ref.name).await {
        Ok(Some((_org, _slug, client))) => {
            client
                .delete_project(&org_slug, &project.desired_slug())
                .await?;
        }
        Ok(None) => {
            tracing::warn!(project = %project.name_any(), "organization chain unavailable; skipping API-side project deletion");
        }
        Err(e) => {
            tracing::warn!(%e, "cannot resolve organization; skipping API-side project deletion");
        }
    }
    Ok(Action::await_change())
}

async fn write_status(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    status: &GlitchTipProjectStatus,
) -> Result<()> {
    let api: Api<GlitchTipProject> = Api::namespaced(ctx.client.clone(), ns);
    api.patch_status(
        name,
        &PatchParams::default(),
        &Patch::Merge(json!({ "status": status })),
    )
    .await?;
    Ok(())
}
