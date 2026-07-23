use k8s_openapi::api::core::v1::{EnvFromSource, EnvVar, ResourceRequirements};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{DeletionPolicy, SecretKeyRef};

pub const DEFAULT_IMAGE_REPO: &str = "glitchtip/glitchtip";
pub const DEFAULT_VERSION: &str = "v5.1";
pub const WEB_PORT: i32 = 8000;

/// A full GlitchTip deployment: web + worker Deployments, migration and
/// bootstrap Jobs, PostgreSQL via pgop CRs, optional Valkey, Service and
/// optional Gateway API HTTPRoute.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "glitchtip.ruck.io",
    version = "v1alpha1",
    kind = "GlitchTip",
    plural = "glitchtips",
    namespaced,
    status = "GlitchTipStatus",
    shortname = "gt",
    printcolumn = r#"{"name":"Domain","type":"string","jsonPath":".spec.domain"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type==\"Ready\")].status"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct GlitchTipSpec {
    /// GlitchTip image tag, e.g. "v5.1". Ignored when `image` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Full image override, e.g. "glitchtip/glitchtip:v5.1.5".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Public URL of the instance including scheme (GLITCHTIP_DOMAIN).
    pub domain: String,
    /// DEFAULT_FROM_EMAIL. Defaults to glitchtip@<domain host>.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_email: Option<String>,
    /// EMAIL_URL (smtp://... etc). Defaults to consolemail://.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<WorkloadSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<WorkloadSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valkey: Option<ValkeySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<RouteSpec>,
    /// Extra environment variables appended to web and worker containers
    /// (S3, OIDC, registration flags, ...). Applied last, so they can
    /// override operator-provided defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_from: Vec<EnvFromSource>,
    /// Use an existing Secret key for SECRET_KEY instead of generating one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_key_secret_ref: Option<SecretKeyRef>,
    /// Use an existing API token instead of running the bootstrap Job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token_secret_ref: Option<SecretKeyRef>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replicas: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    /// Container command override (escape hatch, e.g. for older worker
    /// entrypoints).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseSpec {
    /// Use an existing pgop Cluster (same namespace) instead of creating
    /// one per instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_ref: Option<ClusterRef>,
    /// PVC size for the operator-created pgop Cluster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_class_name: Option<String>,
    /// PostgreSQL image passthrough for the pgop Cluster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    /// Whether operator-created pgop CRs are deleted with this instance.
    /// Defaults to Retain for data safety.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_policy: Option<DeletionPolicy>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClusterRef {
    pub name: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ValkeySpec {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct RouteSpec {
    #[serde(default)]
    pub enabled: bool,
    /// Defaults to the host parsed from spec.domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parent_refs: Vec<RouteParentRef>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub labels: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub annotations: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RouteParentRef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_name: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct GlitchTipStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Name of the Secret holding the operator's API token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token_secret: Option<String>,
    /// Name of the Secret holding the composed application config
    /// (SECRET_KEY, DATABASE_URL, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_secret: Option<String>,
    /// Revision (image + database identity hash) the migration Job last
    /// completed for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrated_revision: Option<String>,
}

impl GlitchTip {
    pub fn image(&self) -> String {
        if let Some(image) = &self.spec.image {
            return image.clone();
        }
        let version = self.spec.version.as_deref().unwrap_or(DEFAULT_VERSION);
        format!("{DEFAULT_IMAGE_REPO}:{version}")
    }

    /// Host portion of spec.domain, e.g. "gt.example.com".
    pub fn domain_host(&self) -> String {
        url::Url::parse(&self.spec.domain)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| self.spec.domain.clone())
    }

    pub fn from_email(&self) -> String {
        self.spec
            .from_email
            .clone()
            .unwrap_or_else(|| format!("glitchtip@{}", self.domain_host()))
    }

    pub fn email_url(&self) -> String {
        self.spec
            .email_url
            .clone()
            .unwrap_or_else(|| "consolemail://".to_string())
    }
}
