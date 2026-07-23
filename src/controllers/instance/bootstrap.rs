//! API-token auto-bootstrap. The operator pre-generates the token value into
//! a Secret, then a one-shot Job (running the GlitchTip image with Django
//! settings from the config Secret) idempotently creates a superuser service
//! account and inserts an APIToken row with that exact value. The token is
//! passed via env-from-secret so it never appears in the pod spec.

use k8s_openapi::api::batch::v1::Job;
use kube::api::{Api, DeleteParams, PropagationPolicy};
use serde_json::json;

use crate::context::Ctx;
use crate::crds::GlitchTip;
use crate::error::Result;

use super::migrate::{JobState, job_state};
use super::resources::{env_from_json, env_json, labels, owner_ref};

/// Idempotent Django shell script. Model layout notes:
/// - GlitchTip's user model is email-based (no username field).
/// - APIToken lives in apps.api_tokens.models with a raw `token` column and
///   a django-bitfield `scopes` BitField; all-bits-set grants every scope.
const BOOTSTRAP_SCRIPT: &str = r#"
import os
from django.contrib.auth import get_user_model

User = get_user_model()
email = os.environ["OPERATOR_EMAIL"]
user = User.objects.filter(email=email).first()
if user is None:
    user = User(email=email)
    user.set_unusable_password()
user.is_staff = True
user.is_superuser = True
user.is_active = True
user.save()

from apps.api_tokens.models import APIToken

token_value = os.environ["OPERATOR_API_TOKEN"]
if not APIToken.objects.filter(token=token_value).exists():
    t = APIToken(user=user, token=token_value)
    field = t._meta.get_field("scopes")
    flags = getattr(field, "flags", None)
    if flags:
        t.scopes = (1 << len(flags)) - 1
    t.save()
print("glitchtip-operator bootstrap complete")
"#;

pub fn job_name(gt: &GlitchTip) -> String {
    format!(
        "{}-bootstrap",
        gt.metadata.name.as_deref().unwrap_or_default()
    )
}

pub fn operator_email(gt: &GlitchTip) -> String {
    format!("glitchtip-operator@{}", gt.domain_host())
}

pub async fn ensure(gt: &GlitchTip, ctx: &Ctx, token_secret: &str) -> Result<JobState> {
    let ns = gt.metadata.namespace.as_deref().unwrap_or_default();
    let name = job_name(gt);

    let mut env = match env_json(gt) {
        serde_json::Value::Array(vars) => vars,
        _ => vec![],
    };
    env.push(json!({"name": "OPERATOR_EMAIL", "value": operator_email(gt)}));
    env.push(json!({
        "name": "OPERATOR_API_TOKEN",
        "valueFrom": {"secretKeyRef": {"name": token_secret, "key": "token"}},
    }));

    let manifest = json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": name,
            "namespace": ns,
            "labels": labels(gt, "bootstrap"),
            "ownerReferences": [owner_ref(gt)],
        },
        "spec": {
            "backoffLimit": 4,
            "template": {
                "metadata": {"labels": labels(gt, "bootstrap")},
                "spec": {
                    "restartPolicy": "OnFailure",
                    "containers": [{
                        "name": "bootstrap",
                        "image": gt.image(),
                        "command": ["python", "manage.py", "shell", "-c", BOOTSTRAP_SCRIPT],
                        "env": env,
                        "envFrom": env_from_json(gt),
                    }],
                },
            },
        },
    });

    let api: Api<Job> = Api::namespaced(ctx.client.clone(), ns);
    let job = match api.get_opt(&name).await? {
        Some(job) => job,
        None => {
            api.create(&Default::default(), &serde_json::from_value(manifest)?)
                .await?
        }
    };
    Ok(job_state(&job))
}

/// Delete the bootstrap Job so the next reconcile recreates it (used when
/// the token fails validation, e.g. the DB was rebuilt underneath us).
pub async fn recreate(gt: &GlitchTip, ctx: &Ctx) -> Result<()> {
    let ns = gt.metadata.namespace.as_deref().unwrap_or_default();
    let api: Api<Job> = Api::namespaced(ctx.client.clone(), ns);
    match api
        .delete(
            &job_name(gt),
            &DeleteParams {
                propagation_policy: Some(PropagationPolicy::Background),
                ..Default::default()
            },
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(e)) if e.code == 404 => Ok(()),
        Err(e) => Err(e.into()),
    }
}
