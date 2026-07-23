pub mod bootstrap;
pub mod database;
pub mod migrate;
pub mod resources;

use k8s_openapi::api::apps::v1::Deployment;
use kube::ResourceExt;
use kube::api::{Api, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::runtime::finalizer::{Event, finalizer};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use crate::context::Ctx;
use crate::crds::GlitchTip;
use crate::crds::common::set_condition;
use crate::crds::instance::GlitchTipStatus;
use crate::error::{Error, Result};
use crate::glitchtip::ApiError;
use crate::util::secrets;

pub const FINALIZER: &str = "glitchtip.ruck.io/instance-cleanup";

const READY: &str = "Ready";
const DATABASE_READY: &str = "DatabaseReady";
const MIGRATIONS_COMPLETE: &str = "MigrationsComplete";
const BOOTSTRAP_COMPLETE: &str = "BootstrapComplete";

pub async fn reconcile(gt: Arc<GlitchTip>, ctx: Arc<Ctx>) -> Result<Action> {
    let ns = gt.namespace().unwrap_or_default();
    let api: Api<GlitchTip> = Api::namespaced(ctx.client.clone(), &ns);
    finalizer(&api, FINALIZER, gt, |event| async {
        match event {
            Event::Apply(gt) => apply(gt, ctx.clone()).await,
            Event::Cleanup(gt) => cleanup(gt, ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::Finalizer(Box::new(e)))
}

pub fn error_policy(_gt: Arc<GlitchTip>, error: &Error, _ctx: Arc<Ctx>) -> Action {
    tracing::warn!(%error, "instance reconcile failed");
    Action::requeue(Duration::from_secs(30))
}

async fn cleanup(gt: Arc<GlitchTip>, ctx: Arc<Ctx>) -> Result<Action> {
    // Deployments/Jobs/Secrets/Service/HTTPRoute are garbage-collected via
    // ownerReferences; only the (deliberately un-owned) pgop CRs need help.
    database::cleanup(&gt, &ctx).await?;
    Ok(Action::await_change())
}

async fn apply(gt: Arc<GlitchTip>, ctx: Arc<Ctx>) -> Result<Action> {
    let ns = gt.namespace().unwrap_or_default();
    let name = gt.name_any();
    let generation = gt.metadata.generation;
    let mut status = gt.status.clone().unwrap_or_default();
    status.observed_generation = generation;
    status.url = Some(gt.spec.domain.clone());
    let conds = &mut status.conditions;

    // 1. SECRET_KEY: generated exactly once, or taken from the referenced
    //    Secret. Never regenerated — that would invalidate sessions and
    //    stored encrypted fields.
    let config_secret_name = resources::config_secret_name(&gt);
    let secret_key = match &gt.spec.secret_key_secret_ref {
        Some(r) => secrets::get_secret_key(&ctx.client, &ns, &r.name, &r.key).await?,
        None => {
            match secrets::get_secret_key(&ctx.client, &ns, &config_secret_name, "SECRET_KEY").await
            {
                Ok(existing) => existing,
                Err(_) => secrets::random_alphanumeric(50),
            }
        }
    };

    // 2. API token Secret (pre-generated value the bootstrap Job inserts).
    let (token_secret_name, token_key) = Ctx::api_token_source(&gt);
    if gt.spec.api_token_secret_ref.is_none()
        && secrets::get_secret_key(&ctx.client, &ns, &token_secret_name, &token_key)
            .await
            .is_err()
    {
        secrets::apply_secret(
            &ctx.client,
            &ns,
            &token_secret_name,
            BTreeMap::from([("token".to_string(), secrets::random_hex(32))]),
            resources::labels(&gt, "api-token"),
            BTreeMap::new(),
            Some(resources::owner_ref(&gt)),
        )
        .await?;
    }
    status.api_token_secret = Some(token_secret_name.clone());

    // 3. PostgreSQL via pgop.
    let db = database::ensure(&gt, &ctx).await?;
    let Some(database_url) = db.database_url else {
        set_condition(
            conds,
            DATABASE_READY,
            false,
            "WaitingForPgop",
            &db.message,
            generation,
        );
        set_condition(
            conds,
            READY,
            false,
            "WaitingForDatabase",
            &db.message,
            generation,
        );
        write_status(&ctx, &ns, &name, &status).await?;
        return Ok(Action::requeue(Duration::from_secs(15)));
    };
    set_condition(
        conds,
        DATABASE_READY,
        true,
        "CredentialsAvailable",
        &db.message,
        generation,
    );

    // 4. Config Secret consumed by all GlitchTip containers.
    let mut config = BTreeMap::from([
        ("SECRET_KEY".to_string(), secret_key),
        ("DATABASE_URL".to_string(), database_url.clone()),
    ]);
    let valkey_enabled = gt.spec.valkey.as_ref().map(|v| v.enabled).unwrap_or(false);
    if valkey_enabled {
        crate::util::apply::apply_json::<Deployment>(
            &ctx.client,
            &ns,
            &format!("{name}-valkey"),
            &resources::valkey_deployment(&gt),
        )
        .await?;
        crate::util::apply::apply_json::<k8s_openapi::api::core::v1::Service>(
            &ctx.client,
            &ns,
            &format!("{name}-valkey"),
            &resources::valkey_service(&gt),
        )
        .await?;
        config.insert("VALKEY_URL".to_string(), resources::valkey_url(&gt));
    } else {
        // An explicitly empty VALKEY_URL (not merely unset, which falls back
        // to redis://redis:6379/0) is what activates GlitchTip's Postgres
        // cache/celery/session backend, available since v5.2.
        config.insert("VALKEY_URL".to_string(), String::new());
    }
    secrets::apply_secret(
        &ctx.client,
        &ns,
        &config_secret_name,
        config,
        resources::labels(&gt, "config"),
        BTreeMap::new(),
        Some(resources::owner_ref(&gt)),
    )
    .await?;
    status.config_secret = Some(config_secret_name.clone());

    // 5. Migrations gate everything downstream.
    let revision = migrate::revision(&gt, &database_url);
    match migrate::ensure(&gt, &ctx, &revision).await? {
        migrate::JobState::Complete => {
            status.migrated_revision = Some(revision);
            set_condition(
                conds,
                MIGRATIONS_COMPLETE,
                true,
                "Migrated",
                "migration job succeeded",
                generation,
            );
        }
        migrate::JobState::Running => {
            set_condition(
                conds,
                MIGRATIONS_COMPLETE,
                false,
                "Migrating",
                "migration job is running",
                generation,
            );
            set_condition(
                conds,
                READY,
                false,
                "Migrating",
                "waiting for database migrations",
                generation,
            );
            write_status(&ctx, &ns, &name, &status).await?;
            return Ok(Action::requeue(Duration::from_secs(15)));
        }
        migrate::JobState::Failed(msg) => {
            set_condition(
                conds,
                MIGRATIONS_COMPLETE,
                false,
                "MigrationFailed",
                &msg,
                generation,
            );
            set_condition(conds, READY, false, "MigrationFailed", &msg, generation);
            write_status(&ctx, &ns, &name, &status).await?;
            return Ok(Action::requeue(Duration::from_secs(120)));
        }
    }

    // 6. Web + worker Deployments and the web Service.
    crate::util::apply::apply_json::<Deployment>(
        &ctx.client,
        &ns,
        &format!("{name}-web"),
        &resources::web_deployment(&gt),
    )
    .await?;
    crate::util::apply::apply_json::<Deployment>(
        &ctx.client,
        &ns,
        &format!("{name}-worker"),
        &resources::worker_deployment(&gt),
    )
    .await?;
    crate::util::apply::apply_json::<k8s_openapi::api::core::v1::Service>(
        &ctx.client,
        &ns,
        &format!("{name}-web"),
        &resources::web_service(&gt),
    )
    .await?;

    // 7. Optional HTTPRoute.
    let route_enabled = gt.spec.route.as_ref().map(|r| r.enabled).unwrap_or(false);
    let mut route_ok = true;
    if route_enabled {
        if ctx.gateway_api_available {
            crate::util::apply::apply_json::<crate::gateway::HTTPRoute>(
                &ctx.client,
                &ns,
                &name,
                &resources::http_route(&gt),
            )
            .await?;
        } else {
            route_ok = false;
            set_condition(
                conds,
                READY,
                false,
                "GatewayAPIUnavailable",
                "spec.route.enabled is set but the gateway.networking.k8s.io API group is not installed",
                generation,
            );
        }
    }

    // 8. Bootstrap Job + live token validation.
    let deployments: Api<Deployment> = Api::namespaced(ctx.client.clone(), &ns);
    let web_available = deployments
        .get_opt(&format!("{name}-web"))
        .await?
        .map(|d| resources::is_deployment_available(&d))
        .unwrap_or(false);
    let worker_available = deployments
        .get_opt(&format!("{name}-worker"))
        .await?
        .map(|d| resources::is_deployment_available(&d))
        .unwrap_or(false);

    let auto_bootstrap = gt.spec.api_token_secret_ref.is_none();
    let mut bootstrap_failed = false;
    let mut bootstrap_job_complete = !auto_bootstrap;
    if auto_bootstrap {
        match bootstrap::ensure(&gt, &ctx, &token_secret_name).await? {
            migrate::JobState::Complete => bootstrap_job_complete = true,
            migrate::JobState::Running => {
                set_condition(
                    conds,
                    BOOTSTRAP_COMPLETE,
                    false,
                    "Bootstrapping",
                    "bootstrap job is running",
                    generation,
                );
            }
            migrate::JobState::Failed(msg) => {
                bootstrap_failed = true;
                set_condition(
                    conds,
                    BOOTSTRAP_COMPLETE,
                    false,
                    "BootstrapJobFailed",
                    &msg,
                    generation,
                );
            }
        }
    }

    let mut token_valid = false;
    if web_available && bootstrap_job_complete && !bootstrap_failed {
        match ctx.glitchtip_client(&gt).await?.ping().await {
            Ok(()) => {
                token_valid = true;
                set_condition(
                    conds,
                    BOOTSTRAP_COMPLETE,
                    true,
                    "TokenValidated",
                    "API token accepted",
                    generation,
                );
            }
            Err(ApiError::Unauthorized(_)) if auto_bootstrap => {
                // Job succeeded but the token is rejected — the database was
                // likely rebuilt after bootstrap. Re-run the Job.
                bootstrap::recreate(&gt, &ctx).await?;
                set_condition(
                    conds,
                    BOOTSTRAP_COMPLETE,
                    false,
                    "TokenInvalid",
                    "re-running bootstrap job",
                    generation,
                );
            }
            Err(e) => {
                set_condition(
                    conds,
                    BOOTSTRAP_COMPLETE,
                    false,
                    "ApiUnreachable",
                    &e.to_string(),
                    generation,
                );
            }
        }
    }

    let ready = web_available && worker_available && token_valid && route_ok;
    if ready {
        set_condition(
            conds,
            READY,
            true,
            "Reconciled",
            "instance is ready",
            generation,
        );
    } else if route_ok {
        let reason = if !web_available || !worker_available {
            "DeploymentsNotAvailable"
        } else {
            "WaitingForBootstrap"
        };
        set_condition(
            conds,
            READY,
            false,
            reason,
            "instance is not ready yet",
            generation,
        );
    }
    write_status(&ctx, &ns, &name, &status).await?;
    Ok(Action::requeue(Duration::from_secs(if ready {
        300
    } else {
        20
    })))
}

async fn write_status(ctx: &Ctx, ns: &str, name: &str, status: &GlitchTipStatus) -> Result<()> {
    let api: Api<GlitchTip> = Api::namespaced(ctx.client.clone(), ns);
    api.patch_status(
        name,
        &PatchParams::default(),
        &Patch::Merge(json!({ "status": status })),
    )
    .await?;
    Ok(())
}
