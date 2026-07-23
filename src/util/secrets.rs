use k8s_openapi::api::core::v1::Secret;
use kube::Client;
use kube::api::{Api, Patch, PatchParams};
use rand::RngExt as _;
use rand::distr::Alphanumeric;
use std::collections::BTreeMap;

use crate::error::{Error, Result};

pub const FIELD_MANAGER: &str = "glitchtip-operator";

pub fn random_alphanumeric(len: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill(&mut buf[..]);
    hex::encode(buf)
}

pub async fn get_secret_key(
    client: &Client,
    namespace: &str,
    name: &str,
    key: &str,
) -> Result<String> {
    let api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let secret = api
        .get_opt(name)
        .await?
        .ok_or_else(|| Error::MissingSecretKey(format!("{namespace}/{name}"), key.to_string()))?;
    read_key(&secret, key)
        .ok_or_else(|| Error::MissingSecretKey(format!("{namespace}/{name}"), key.to_string()))
}

pub fn read_key(secret: &Secret, key: &str) -> Option<String> {
    if let Some(v) = secret.data.as_ref().and_then(|d| d.get(key)) {
        return String::from_utf8(v.0.clone()).ok();
    }
    secret
        .string_data
        .as_ref()
        .and_then(|d| d.get(key).cloned())
}

/// Server-side apply a Secret built from string data, with the given owner.
pub async fn apply_secret(
    client: &Client,
    namespace: &str,
    name: &str,
    string_data: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
    owner: Option<k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference>,
) -> Result<()> {
    let api: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let mut secret = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": labels,
        },
        "type": "Opaque",
        "stringData": string_data,
    });
    if !annotations.is_empty() {
        secret["metadata"]["annotations"] = serde_json::to_value(&annotations)?;
    }
    if let Some(owner) = owner {
        secret["metadata"]["ownerReferences"] = serde_json::to_value(vec![owner])?;
    }
    api.patch(
        name,
        &PatchParams::apply(FIELD_MANAGER).force(),
        &Patch::Apply(&secret),
    )
    .await?;
    Ok(())
}
