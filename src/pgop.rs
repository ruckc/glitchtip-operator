//! Locally-defined typed structs for pgop's CRDs (https://pgop.ruck.io).
//! These are foreign resources: the operator only creates/reads them, and
//! crdgen must never emit their CRD definitions. Only the fields we set are
//! mirrored; server-side apply keeps unknown fields intact.
//! All pgop references are same-namespace by design.

use k8s_openapi::api::core::v1::ResourceRequirements;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NameRef {
    pub name: String,
}

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "pgop.ruck.io",
    version = "v1alpha1",
    kind = "Cluster",
    plural = "clusters",
    namespaced,
    status = "ClusterStatus"
)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replicas: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    pub storage: StorageSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct StorageSpec {
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_class_name: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClusterStatus {
    #[serde(default)]
    pub ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "pgop.ruck.io",
    version = "v1alpha1",
    kind = "Role",
    plural = "roles",
    namespaced,
    status = "RoleStatus"
)]
#[serde(rename_all = "camelCase")]
pub struct RoleSpec {
    pub cluster_ref: NameRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_limit: Option<i32>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct RoleStatus {
    #[serde(default)]
    pub ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "pgop.ruck.io",
    version = "v1alpha1",
    kind = "Database",
    plural = "databases",
    namespaced,
    status = "DatabaseStatus"
)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseSpec {
    pub cluster_ref: NameRef,
    /// Owner Role CR name (same namespace).
    pub owner: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<ExtensionSpec>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseStatus {
    #[serde(default)]
    pub ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}

/// Secret emitted by pgop for a Database: `<database>-<owner>-credentials`
/// with data keys username/password/host/port/database.
pub fn database_credentials_secret_name(database: &str, owner: &str) -> String {
    format!("{database}-{owner}-credentials")
}
