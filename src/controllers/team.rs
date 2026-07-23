use kube::ResourceExt;
use kube::api::{Api, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::runtime::finalizer::{Event, finalizer};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use crate::context::Ctx;
use crate::crds::common::{DeletionPolicy, is_condition_true, set_condition};
use crate::crds::team::GlitchTipTeamStatus;
use crate::crds::{GlitchTip, GlitchTipOrganization, GlitchTipTeam};
use crate::error::{Error, Result};
use crate::glitchtip::{ApiError, GlitchTipClient};

pub const FINALIZER: &str = "glitchtip.ruck.io/team-cleanup";

pub async fn reconcile(team: Arc<GlitchTipTeam>, ctx: Arc<Ctx>) -> Result<Action> {
    let ns = team.namespace().unwrap_or_default();
    let api: Api<GlitchTipTeam> = Api::namespaced(ctx.client.clone(), &ns);
    finalizer(&api, FINALIZER, team, |event| async {
        match event {
            Event::Apply(team) => apply(team, ctx.clone()).await,
            Event::Cleanup(team) => cleanup(team, ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::Finalizer(Box::new(e)))
}

pub fn error_policy(_team: Arc<GlitchTipTeam>, error: &Error, _ctx: Arc<Ctx>) -> Action {
    tracing::warn!(%error, "team reconcile failed");
    Action::requeue(Duration::from_secs(30))
}

/// Resolve the org CR, its Ready slug, and an API client for its instance.
pub async fn org_chain(
    ctx: &Arc<Ctx>,
    org_ref_ns: &str,
    org_name: &str,
) -> Result<Option<(GlitchTipOrganization, String, GlitchTipClient)>> {
    let orgs: Api<GlitchTipOrganization> = Api::namespaced(ctx.client.clone(), org_ref_ns);
    let Some(org) = orgs.get_opt(org_name).await? else {
        return Ok(None);
    };
    let ready = org
        .status
        .as_ref()
        .map(|s| is_condition_true(&s.conditions, "Ready"))
        .unwrap_or(false);
    let Some(slug) = org.status.as_ref().and_then(|s| s.slug.clone()) else {
        return Ok(None);
    };
    if !ready {
        return Ok(None);
    }
    let org_ns = org.namespace().unwrap_or_default();
    let instance_ns = org.spec.instance_ref.namespace_or(&org_ns).to_string();
    let instances: Api<GlitchTip> = Api::namespaced(ctx.client.clone(), &instance_ns);
    let Some(instance) = instances.get_opt(&org.spec.instance_ref.name).await? else {
        return Ok(None);
    };
    let client = ctx.glitchtip_client(&instance).await?;
    Ok(Some((org, slug, client)))
}

async fn apply(team: Arc<GlitchTipTeam>, ctx: Arc<Ctx>) -> Result<Action> {
    let ns = team.namespace().unwrap_or_default();
    let name = team.name_any();
    let generation = team.metadata.generation;
    let mut status = team.status.clone().unwrap_or_default();
    status.observed_generation = generation;
    let conds = &mut status.conditions;

    let org_ns = team.spec.organization_ref.namespace_or(&ns).to_string();
    let chain = org_chain(&ctx, &org_ns, &team.spec.organization_ref.name).await?;
    let Some((_org, org_slug, client)) = chain else {
        set_condition(
            conds,
            "Ready",
            false,
            "WaitingForOrganization",
            &format!(
                "organization {org_ns}/{} is not ready",
                team.spec.organization_ref.name
            ),
            generation,
        );
        write_status(&ctx, &ns, &name, &status).await?;
        return Ok(Action::requeue(Duration::from_secs(20)));
    };

    let slug = team.desired_slug();
    let remote = match client.get_team(&org_slug, &slug).await {
        Ok(remote) => remote,
        Err(ApiError::NotFound) => client.create_team(&org_slug, &slug).await?,
        Err(e) => return Err(e.into()),
    };
    status.slug = Some(remote.slug.clone());
    status.id = remote.id.clone();
    status.organization_slug = Some(org_slug);
    set_condition(
        conds,
        "Ready",
        true,
        "Reconciled",
        "team exists",
        generation,
    );
    write_status(&ctx, &ns, &name, &status).await?;
    Ok(Action::requeue(Duration::from_secs(300)))
}

async fn cleanup(team: Arc<GlitchTipTeam>, ctx: Arc<Ctx>) -> Result<Action> {
    if team.spec.deletion_policy == DeletionPolicy::Retain {
        return Ok(Action::await_change());
    }
    // Prefer the slugs captured in status; the org CR may already be gone
    // (in which case GlitchTip cascade-deleted the team server-side anyway).
    let ns = team.namespace().unwrap_or_default();
    let org_ns = team.spec.organization_ref.namespace_or(&ns).to_string();
    let org_slug = match team
        .status
        .as_ref()
        .and_then(|s| s.organization_slug.clone())
    {
        Some(s) => s,
        None => return Ok(Action::await_change()),
    };
    match org_chain(&ctx, &org_ns, &team.spec.organization_ref.name).await {
        Ok(Some((_org, _slug, client))) => {
            client.delete_team(&org_slug, &team.desired_slug()).await?;
        }
        Ok(None) => {
            tracing::warn!(team = %team.name_any(), "organization chain unavailable; skipping API-side team deletion");
        }
        Err(e) => {
            tracing::warn!(%e, "cannot resolve organization; skipping API-side team deletion");
        }
    }
    Ok(Action::await_change())
}

async fn write_status(ctx: &Ctx, ns: &str, name: &str, status: &GlitchTipTeamStatus) -> Result<()> {
    let api: Api<GlitchTipTeam> = Api::namespaced(ctx.client.clone(), ns);
    api.patch_status(
        name,
        &PatchParams::default(),
        &Patch::Merge(json!({ "status": status })),
    )
    .await?;
    Ok(())
}
