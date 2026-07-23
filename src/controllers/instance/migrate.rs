//! Migration Job lifecycle. Jobs are immutable, so each revision (image +
//! database identity) gets its own Job name; old Jobs are cleaned up by
//! ttlSecondsAfterFinished and the CR ownerReference.

use k8s_openapi::api::batch::v1::Job;
use kube::api::Api;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::context::Ctx;
use crate::crds::GlitchTip;
use crate::error::Result;

use super::resources::{env_from_json, env_json, labels, owner_ref};

pub fn revision(gt: &GlitchTip, database_url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(gt.image());
    hasher.update(database_url);
    hex::encode(hasher.finalize())[..12].to_string()
}

pub fn job_name(gt: &GlitchTip, revision: &str) -> String {
    format!(
        "{}-migrate-{revision}",
        gt.metadata.name.as_deref().unwrap_or_default()
    )
}

pub enum JobState {
    Complete,
    Running,
    Failed(String),
}

/// Apply the migration Job for this revision and report its state.
pub async fn ensure(gt: &GlitchTip, ctx: &Ctx, revision: &str) -> Result<JobState> {
    let ns = gt.metadata.namespace.as_deref().unwrap_or_default();
    let name = job_name(gt, revision);
    let manifest = json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": name,
            "namespace": ns,
            "labels": labels(gt, "migrate"),
            "ownerReferences": [owner_ref(gt)],
        },
        "spec": {
            "backoffLimit": 6,
            "ttlSecondsAfterFinished": 86400,
            "template": {
                "metadata": {"labels": labels(gt, "migrate")},
                "spec": {
                    "restartPolicy": "OnFailure",
                    "containers": [{
                        "name": "migrate",
                        "image": gt.image(),
                        "command": ["./bin/run-migrations.sh"],
                        "env": env_json(gt),
                        "envFrom": env_from_json(gt),
                    }],
                },
            },
        },
    });

    let api: Api<Job> = Api::namespaced(ctx.client.clone(), ns);
    // Jobs are immutable; only create when absent.
    let job = match api.get_opt(&name).await? {
        Some(job) => job,
        None => {
            api.create(&Default::default(), &serde_json::from_value(manifest)?)
                .await?
        }
    };
    Ok(job_state(&job))
}

pub fn job_state(job: &Job) -> JobState {
    let status = job.status.clone().unwrap_or_default();
    if status.succeeded.unwrap_or(0) > 0 {
        return JobState::Complete;
    }
    if let Some(failed) = status
        .conditions
        .iter()
        .flatten()
        .find(|c| c.type_ == "Failed" && c.status == "True")
    {
        return JobState::Failed(
            failed
                .message
                .clone()
                .unwrap_or_else(|| "migration job failed".into()),
        );
    }
    JobState::Running
}
