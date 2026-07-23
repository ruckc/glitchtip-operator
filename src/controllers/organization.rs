use kube::ResourceExt;
use kube::api::{Api, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::runtime::finalizer::{Event, finalizer};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use crate::context::Ctx;
use crate::crds::common::{DeletionPolicy, is_condition_true, set_condition};
use crate::crds::organization::GlitchTipOrganizationStatus;
use crate::crds::{GlitchTip, GlitchTipOrganization};
use crate::error::{Error, Result};
use crate::glitchtip::ApiError;

pub const FINALIZER: &str = "glitchtip.ruck.io/organization-cleanup";

pub async fn reconcile(org: Arc<GlitchTipOrganization>, ctx: Arc<Ctx>) -> Result<Action> {
    let ns = org.namespace().unwrap_or_default();
    let api: Api<GlitchTipOrganization> = Api::namespaced(ctx.client.clone(), &ns);
    finalizer(&api, FINALIZER, org, |event| async {
        match event {
            Event::Apply(org) => apply(org, ctx.clone()).await,
            Event::Cleanup(org) => cleanup(org, ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::Finalizer(Box::new(e)))
}

pub fn error_policy(_org: Arc<GlitchTipOrganization>, error: &Error, _ctx: Arc<Ctx>) -> Action {
    tracing::warn!(%error, "organization reconcile failed");
    Action::requeue(Duration::from_secs(30))
}

/// The instance is usable for API management once its token validated;
/// full Ready (which includes e.g. the HTTPRoute) is not required.
fn instance_usable(gt: &GlitchTip) -> bool {
    gt.status
        .as_ref()
        .map(|s| is_condition_true(&s.conditions, "BootstrapComplete"))
        .unwrap_or(false)
}

async fn resolve_instance(ctx: &Ctx, org: &GlitchTipOrganization) -> Result<Option<GlitchTip>> {
    let ns = org.namespace().unwrap_or_default();
    let instance_ns = org.spec.instance_ref.namespace_or(&ns).to_string();
    let api: Api<GlitchTip> = Api::namespaced(ctx.client.clone(), &instance_ns);
    Ok(api.get_opt(&org.spec.instance_ref.name).await?)
}

async fn apply(org: Arc<GlitchTipOrganization>, ctx: Arc<Ctx>) -> Result<Action> {
    let ns = org.namespace().unwrap_or_default();
    let name = org.name_any();
    let generation = org.metadata.generation;
    let mut status = org.status.clone().unwrap_or_default();
    status.observed_generation = generation;
    let conds = &mut status.conditions;

    let Some(instance) = resolve_instance(&ctx, &org).await? else {
        let msg = format!(
            "GlitchTip {}/{} not found",
            org.spec.instance_ref.namespace_or(&ns),
            org.spec.instance_ref.name
        );
        set_condition(conds, "Ready", false, "InstanceNotFound", &msg, generation);
        write_status(&ctx, &ns, &name, &status).await?;
        return Ok(Action::requeue(Duration::from_secs(30)));
    };
    if !instance_usable(&instance) {
        set_condition(
            conds,
            "Ready",
            false,
            "WaitingForInstance",
            "instance API is not ready yet",
            generation,
        );
        write_status(&ctx, &ns, &name, &status).await?;
        return Ok(Action::requeue(Duration::from_secs(20)));
    }

    let client = ctx.glitchtip_client(&instance).await?;
    let slug = org.desired_slug();
    let remote = match client.get_organization(&slug).await {
        Ok(remote) => remote,
        Err(ApiError::NotFound) => {
            client
                .create_organization(&org.display_name(), Some(&slug))
                .await?
        }
        Err(e) => return Err(e.into()),
    };
    // Heal display-name drift (slug is identity and left alone).
    if remote.name.as_deref() != Some(&org.display_name()) {
        client
            .update_organization(&remote.slug, &org.display_name())
            .await?;
    }
    status.slug = Some(remote.slug.clone());
    status.id = remote.id.clone();
    set_condition(
        conds,
        "Ready",
        true,
        "Reconciled",
        "organization exists",
        generation,
    );
    write_status(&ctx, &ns, &name, &status).await?;
    Ok(Action::requeue(Duration::from_secs(300)))
}

async fn cleanup(org: Arc<GlitchTipOrganization>, ctx: Arc<Ctx>) -> Result<Action> {
    if org.spec.deletion_policy == DeletionPolicy::Retain {
        return Ok(Action::await_change());
    }
    let slug = org.desired_slug();
    // Never wedge deletion when the instance or its token are already gone.
    let Some(instance) = resolve_instance(&ctx, &org).await? else {
        tracing::warn!(org = %org.name_any(), "instance gone; skipping API-side organization deletion");
        return Ok(Action::await_change());
    };
    let client = match ctx.glitchtip_client(&instance).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(%e, "cannot build API client; skipping organization deletion");
            return Ok(Action::await_change());
        }
    };
    client.delete_organization(&slug).await?;
    Ok(Action::await_change())
}

async fn write_status(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    status: &GlitchTipOrganizationStatus,
) -> Result<()> {
    let api: Api<GlitchTipOrganization> = Api::namespaced(ctx.client.clone(), ns);
    api.patch_status(
        name,
        &PatchParams::default(),
        &Patch::Merge(json!({ "status": status })),
    )
    .await?;
    Ok(())
}
