//! PostgreSQL provisioning through pgop CRs. All pgop resources live in the
//! instance's namespace (pgop refs are same-namespace only). We deliberately
//! do NOT set ownerReferences on pgop CRs so deletionPolicy=Retain can keep
//! the database alive after the GlitchTip CR is gone; tracking is via labels.

use kube::api::{Api, DeleteParams};
use serde_json::json;

use crate::context::Ctx;
use crate::crds::GlitchTip;
use crate::crds::common::DeletionPolicy;
use crate::error::Result;
use crate::pgop;
use crate::util::{dsn, secrets};

pub struct DatabaseOutcome {
    /// Composed DATABASE_URL once pgop has emitted credentials.
    pub database_url: Option<String>,
    pub message: String,
}

fn instance_name(gt: &GlitchTip) -> &str {
    gt.metadata.name.as_deref().unwrap_or_default()
}

/// Name of the pgop Cluster used by this instance and whether we manage it.
pub fn cluster_name(gt: &GlitchTip) -> (String, bool) {
    match gt
        .spec
        .database
        .as_ref()
        .and_then(|d| d.cluster_ref.as_ref())
    {
        Some(r) => (r.name.clone(), false),
        None => (format!("{}-db", instance_name(gt)), true),
    }
}

pub fn role_name(gt: &GlitchTip) -> String {
    format!("{}-app", instance_name(gt))
}

pub fn database_name(gt: &GlitchTip) -> String {
    instance_name(gt).to_string()
}

pub async fn ensure(gt: &GlitchTip, ctx: &Ctx) -> Result<DatabaseOutcome> {
    let ns = gt.metadata.namespace.as_deref().unwrap_or_default();
    let db_spec = gt.spec.database.clone().unwrap_or_default();
    let (cluster, managed) = cluster_name(gt);
    let role = role_name(gt);
    let database = database_name(gt);
    let labels = pgop_labels(gt);

    if managed {
        let manifest = json!({
            "apiVersion": "pgop.ruck.io/v1alpha1",
            "kind": "Cluster",
            "metadata": {"name": cluster, "namespace": ns, "labels": labels},
            "spec": {
                "image": db_spec.image,
                "replicas": 1,
                "storage": {
                    "size": db_spec.storage_size.clone().unwrap_or_else(|| "10Gi".into()),
                    "storageClassName": db_spec.storage_class_name,
                },
                "resources": db_spec.resources,
            },
        });
        crate::util::apply::apply_json::<pgop::Cluster>(&ctx.client, ns, &cluster, &manifest)
            .await?;
    }

    let role_manifest = json!({
        "apiVersion": "pgop.ruck.io/v1alpha1",
        "kind": "Role",
        "metadata": {"name": role, "namespace": ns, "labels": labels},
        "spec": {"clusterRef": {"name": cluster}, "login": true},
    });
    crate::util::apply::apply_json::<pgop::Role>(&ctx.client, ns, &role, &role_manifest).await?;

    let db_manifest = json!({
        "apiVersion": "pgop.ruck.io/v1alpha1",
        "kind": "Database",
        "metadata": {"name": database, "namespace": ns, "labels": labels},
        "spec": {"clusterRef": {"name": cluster}, "owner": role},
    });
    crate::util::apply::apply_json::<pgop::Database>(&ctx.client, ns, &database, &db_manifest)
        .await?;

    // pgop emits `<database>-<owner>-credentials` once the chain is ready.
    let secret_name = pgop::database_credentials_secret_name(&database, &role);
    let api: Api<k8s_openapi::api::core::v1::Secret> = Api::namespaced(ctx.client.clone(), ns);
    let Some(secret) = api.get_opt(&secret_name).await? else {
        return Ok(DatabaseOutcome {
            database_url: None,
            message: format!("waiting for pgop credentials secret {ns}/{secret_name}"),
        });
    };

    let read = |key: &str| {
        secrets::read_key(&secret, key).ok_or_else(|| {
            crate::error::Error::MissingSecretKey(format!("{ns}/{secret_name}"), key.to_string())
        })
    };
    let url = dsn::postgres_url(
        &read("username")?,
        &read("password")?,
        &read("host")?,
        &read("port")?,
        &read("database")?,
    );
    Ok(DatabaseOutcome {
        database_url: Some(url),
        message: "database credentials available".into(),
    })
}

fn pgop_labels(gt: &GlitchTip) -> serde_json::Value {
    json!({
        "app.kubernetes.io/managed-by": "glitchtip-operator",
        "glitchtip.ruck.io/instance": instance_name(gt),
    })
}

/// Finalizer-time cleanup of pgop CRs, honoring deletionPolicy (default
/// Retain). A user-provided clusterRef Cluster is never deleted.
pub async fn cleanup(gt: &GlitchTip, ctx: &Ctx) -> Result<()> {
    let policy = gt
        .spec
        .database
        .as_ref()
        .and_then(|d| d.deletion_policy)
        .unwrap_or(DeletionPolicy::Retain);
    if policy == DeletionPolicy::Retain {
        return Ok(());
    }
    let ns = gt.metadata.namespace.as_deref().unwrap_or_default();
    let (cluster, managed) = cluster_name(gt);

    let databases: Api<pgop::Database> = Api::namespaced(ctx.client.clone(), ns);
    ignore_not_found(
        databases
            .delete(&database_name(gt), &DeleteParams::default())
            .await,
    )?;
    let roles: Api<pgop::Role> = Api::namespaced(ctx.client.clone(), ns);
    ignore_not_found(roles.delete(&role_name(gt), &DeleteParams::default()).await)?;
    if managed {
        let clusters: Api<pgop::Cluster> = Api::namespaced(ctx.client.clone(), ns);
        ignore_not_found(clusters.delete(&cluster, &DeleteParams::default()).await)?;
    }
    Ok(())
}

fn ignore_not_found<T>(res: kube::Result<T>) -> Result<()> {
    match res {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(e)) if e.code == 404 => Ok(()),
        Err(e) => Err(e.into()),
    }
}
