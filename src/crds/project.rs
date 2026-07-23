use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::common::{DeletionPolicy, ObjectRef};

/// A GlitchTip project living in the consuming application's namespace.
/// The operator creates the project + client key via the GlitchTip API and
/// writes the DSN into a Secret next to this CR.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "glitchtip.ruck.io",
    version = "v1alpha1",
    kind = "GlitchTipProject",
    plural = "glitchtipprojects",
    namespaced,
    status = "GlitchTipProjectStatus",
    shortname = "gtproj",
    printcolumn = r#"{"name":"Organization","type":"string","jsonPath":".spec.organizationRef.name"}"#,
    printcolumn = r#"{"name":"Slug","type":"string","jsonPath":".status.slug"}"#,
    printcolumn = r#"{"name":"Secret","type":"string","jsonPath":".status.secretName"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type==\"Ready\")].status"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct GlitchTipProjectSpec {
    /// The GlitchTipOrganization CR (usually in another namespace).
    pub organization_ref: ObjectRef,
    /// Reference to a GlitchTipTeam CR. Takes precedence over teamSlug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_ref: Option<ObjectRef>,
    /// Raw team slug alternative to teamRef.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_slug: Option<String>,
    /// Display name; defaults to metadata.name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// GlitchTip platform hint, e.g. "python-django".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default)]
    pub secret: DsnSecretSpec,
    #[serde(default)]
    pub deletion_policy: DeletionPolicy,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct DsnSecretSpec {
    /// Secret name; defaults to "<cr-name>-glitchtip-dsn".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Primary data key holding the DSN. Defaults to SENTRY_DSN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsn_key: Option<String>,
    /// Additional data keys written with the same DSN value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_dsn_keys: Vec<String>,
    /// Also write SENTRY_SECURITY_ENDPOINT when the API provides one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_security_endpoint: Option<bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct GlitchTipProjectStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_name: Option<String>,
}

impl GlitchTipProject {
    pub fn display_name(&self) -> String {
        self.spec
            .name
            .clone()
            .unwrap_or_else(|| self.metadata.name.clone().unwrap_or_default())
    }

    pub fn desired_slug(&self) -> String {
        self.status
            .as_ref()
            .and_then(|s| s.slug.clone())
            .or_else(|| self.spec.slug.clone())
            .unwrap_or_else(|| self.metadata.name.clone().unwrap_or_default())
    }

    pub fn dsn_secret_name(&self) -> String {
        self.spec.secret.name.clone().unwrap_or_else(|| {
            format!(
                "{}-glitchtip-dsn",
                self.metadata.name.clone().unwrap_or_default()
            )
        })
    }

    pub fn dsn_key(&self) -> String {
        self.spec
            .secret
            .dsn_key
            .clone()
            .unwrap_or_else(|| "SENTRY_DSN".to_string())
    }
}
