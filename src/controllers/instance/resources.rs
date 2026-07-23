//! Builders for the k8s children of a GlitchTip instance. All resources are
//! applied via server-side apply and carry an ownerReference to the CR.

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Service;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::core::Resource as _;
use serde_json::json;
use std::collections::BTreeMap;

use crate::crds::GlitchTip;
use crate::crds::instance::WEB_PORT;
use crate::gateway::HTTPRoute;

pub fn owner_ref(gt: &GlitchTip) -> OwnerReference {
    gt.controller_owner_ref(&())
        .expect("GlitchTip has metadata")
}

pub fn labels(gt: &GlitchTip, component: &str) -> BTreeMap<String, String> {
    let name = gt.metadata.name.as_deref().unwrap_or_default();
    BTreeMap::from([
        ("app.kubernetes.io/name".into(), "glitchtip".into()),
        ("app.kubernetes.io/instance".into(), name.into()),
        ("app.kubernetes.io/component".into(), component.into()),
        (
            "app.kubernetes.io/managed-by".into(),
            "glitchtip-operator".into(),
        ),
    ])
}

pub fn config_secret_name(gt: &GlitchTip) -> String {
    format!("{}-config", gt.metadata.name.as_deref().unwrap_or_default())
}

/// Environment shared by web, worker, migrate and bootstrap containers:
/// envFrom the config Secret (SECRET_KEY, DATABASE_URL, optional VALKEY_URL)
/// plus instance-level settings, with user-provided env appended last so it
/// can override anything.
pub fn env_from_json(gt: &GlitchTip) -> serde_json::Value {
    let mut sources = vec![json!({"secretRef": {"name": config_secret_name(gt)}})];
    for extra in &gt.spec.env_from {
        sources.push(serde_json::to_value(extra).unwrap_or_default());
    }
    json!(sources)
}

pub fn env_json(gt: &GlitchTip) -> serde_json::Value {
    let mut vars = vec![
        json!({"name": "GLITCHTIP_DOMAIN", "value": gt.spec.domain}),
        json!({"name": "DEFAULT_FROM_EMAIL", "value": gt.from_email()}),
        json!({"name": "EMAIL_URL", "value": gt.email_url()}),
    ];
    for extra in &gt.spec.env {
        vars.push(serde_json::to_value(extra).unwrap_or_default());
    }
    json!(vars)
}

/// Worker entrypoint: GlitchTip v6+ ships django-vtasks (`run-worker.sh`),
/// older releases (including v5.x) use celery. `spec.worker.command` overrides.
fn worker_command(gt: &GlitchTip) -> Vec<String> {
    if let Some(cmd) = gt.spec.worker.as_ref().and_then(|w| w.command.clone()) {
        return cmd;
    }
    let tag = gt.image();
    let tag = tag.rsplit(':').next().unwrap_or_default();
    let major: u32 = tag
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(5);
    if major >= 6 {
        vec!["./bin/run-worker.sh".into()]
    } else {
        vec!["./bin/run-celery-with-beat.sh".into()]
    }
}

fn deployment(
    gt: &GlitchTip,
    component: &str,
    replicas: i32,
    container: serde_json::Value,
) -> serde_json::Value {
    let name = format!(
        "{}-{component}",
        gt.metadata.name.as_deref().unwrap_or_default()
    );
    let labels = labels(gt, component);
    json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": name,
            "namespace": gt.metadata.namespace,
            "labels": labels,
            "ownerReferences": [owner_ref(gt)],
        },
        "spec": {
            "replicas": replicas,
            "selector": {"matchLabels": labels},
            "template": {
                "metadata": {"labels": labels},
                "spec": {"containers": [container]},
            },
        },
    })
}

pub fn web_deployment(gt: &GlitchTip) -> serde_json::Value {
    let spec = gt.spec.web.clone().unwrap_or_default();
    let container = json!({
        "name": "web",
        "image": gt.image(),
        "ports": [{"containerPort": WEB_PORT, "name": "http"}],
        "env": env_json(gt),
        "envFrom": env_from_json(gt),
        "resources": spec.resources,
        "readinessProbe": {
            "httpGet": {"path": "/_health/", "port": WEB_PORT},
            "initialDelaySeconds": 5,
            "periodSeconds": 10,
        },
        "livenessProbe": {
            "httpGet": {"path": "/_health/", "port": WEB_PORT},
            "initialDelaySeconds": 30,
            "periodSeconds": 30,
        },
    });
    deployment(gt, "web", spec.replicas.unwrap_or(1), container)
}

pub fn worker_deployment(gt: &GlitchTip) -> serde_json::Value {
    let spec = gt.spec.worker.clone().unwrap_or_default();
    let container = json!({
        "name": "worker",
        "image": gt.image(),
        "command": worker_command(gt),
        "env": env_json(gt),
        "envFrom": env_from_json(gt),
        "resources": spec.resources,
    });
    deployment(gt, "worker", spec.replicas.unwrap_or(1), container)
}

pub fn web_service(gt: &GlitchTip) -> serde_json::Value {
    let name = format!("{}-web", gt.metadata.name.as_deref().unwrap_or_default());
    json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": name,
            "namespace": gt.metadata.namespace,
            "labels": labels(gt, "web"),
            "ownerReferences": [owner_ref(gt)],
        },
        "spec": {
            "selector": labels(gt, "web"),
            "ports": [{"name": "http", "port": WEB_PORT, "targetPort": WEB_PORT}],
        },
    })
}

pub fn valkey_deployment(gt: &GlitchTip) -> serde_json::Value {
    let spec = gt.spec.valkey.clone().unwrap_or_default();
    let image = spec.image.unwrap_or_else(|| "valkey/valkey:8".to_string());
    let container = json!({
        "name": "valkey",
        "image": image,
        "ports": [{"containerPort": 6379, "name": "valkey"}],
        "resources": spec.resources,
    });
    deployment(gt, "valkey", 1, container)
}

pub fn valkey_service(gt: &GlitchTip) -> serde_json::Value {
    let name = format!("{}-valkey", gt.metadata.name.as_deref().unwrap_or_default());
    json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": name,
            "namespace": gt.metadata.namespace,
            "labels": labels(gt, "valkey"),
            "ownerReferences": [owner_ref(gt)],
        },
        "spec": {
            "selector": labels(gt, "valkey"),
            "ports": [{"name": "valkey", "port": 6379, "targetPort": 6379}],
        },
    })
}

pub fn valkey_url(gt: &GlitchTip) -> String {
    format!(
        "redis://{}-valkey.{}.svc:6379/0",
        gt.metadata.name.as_deref().unwrap_or_default(),
        gt.metadata.namespace.as_deref().unwrap_or_default(),
    )
}

pub fn http_route(gt: &GlitchTip) -> serde_json::Value {
    let route = gt.spec.route.clone().unwrap_or_default();
    let name = gt.metadata.name.as_deref().unwrap_or_default();
    let hostname = route.hostname.unwrap_or_else(|| gt.domain_host());
    let mut metadata_labels = labels(gt, "web");
    metadata_labels.extend(route.labels);
    json!({
        "apiVersion": "gateway.networking.k8s.io/v1",
        "kind": "HTTPRoute",
        "metadata": {
            "name": name,
            "namespace": gt.metadata.namespace,
            "labels": metadata_labels,
            "annotations": route.annotations,
            "ownerReferences": [owner_ref(gt)],
        },
        "spec": {
            "parentRefs": route.parent_refs,
            "hostnames": [hostname],
            "rules": [{
                "backendRefs": [{"name": format!("{name}-web"), "port": WEB_PORT}],
            }],
        },
    })
}

pub fn is_deployment_available(dep: &Deployment) -> bool {
    dep.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .any(|c| c.type_ == "Available" && c.status == "True")
        })
        .unwrap_or(false)
}

// Typed re-exports used by the reconciler when applying json manifests.
pub type WebService = Service;
pub type Route = HTTPRoute;
