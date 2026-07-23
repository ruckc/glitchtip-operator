use kube::Client;
use kube::api::{Api, Patch, PatchParams};
use kube::core::{NamespaceResourceScope, Resource};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Debug;

use crate::error::Result;
use crate::util::secrets::FIELD_MANAGER;

/// Server-side apply a namespaced resource (typed).
pub async fn apply<K>(client: &Client, namespace: &str, resource: &K) -> Result<K>
where
    K: Resource<Scope = NamespaceResourceScope, DynamicType = ()>
        + Serialize
        + DeserializeOwned
        + Clone
        + Debug,
{
    let api: Api<K> = Api::namespaced(client.clone(), namespace);
    let name = resource.meta().name.clone().expect("resource has a name");
    Ok(api
        .patch(
            &name,
            &PatchParams::apply(FIELD_MANAGER).force(),
            &Patch::Apply(resource),
        )
        .await?)
}

/// Server-side apply from a raw JSON manifest (for resources built as json!).
pub async fn apply_json<K>(
    client: &Client,
    namespace: &str,
    name: &str,
    manifest: &serde_json::Value,
) -> Result<K>
where
    K: Resource<Scope = NamespaceResourceScope, DynamicType = ()>
        + DeserializeOwned
        + Clone
        + Debug,
{
    let api: Api<K> = Api::namespaced(client.clone(), namespace);
    Ok(api
        .patch(
            name,
            &PatchParams::apply(FIELD_MANAGER).force(),
            &Patch::Apply(manifest),
        )
        .await?)
}
