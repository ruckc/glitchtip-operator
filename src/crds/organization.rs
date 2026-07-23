use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{DeletionPolicy, ObjectRef};

/// An organization inside a GlitchTip instance, managed via its REST API.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "glitchtip.ruck.io",
    version = "v1alpha1",
    kind = "GlitchTipOrganization",
    plural = "glitchtiporganizations",
    namespaced,
    status = "GlitchTipOrganizationStatus",
    shortname = "gtorg",
    printcolumn = r#"{"name":"Instance","type":"string","jsonPath":".spec.instanceRef.name"}"#,
    printcolumn = r#"{"name":"Slug","type":"string","jsonPath":".status.slug"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type==\"Ready\")].status"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct GlitchTipOrganizationSpec {
    /// The GlitchTip instance CR this organization belongs to.
    pub instance_ref: ObjectRef,
    /// Display name; defaults to metadata.name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Requested slug; when unset GlitchTip derives one from the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default)]
    pub deletion_policy: DeletionPolicy,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct GlitchTipOrganizationStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    /// Authoritative slug once assigned by GlitchTip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl GlitchTipOrganization {
    pub fn display_name(&self) -> String {
        self.spec
            .name
            .clone()
            .unwrap_or_else(|| self.metadata.name.clone().unwrap_or_default())
    }

    /// Slug to look up / request: status takes precedence (authoritative),
    /// then spec.slug, then the CR name.
    pub fn desired_slug(&self) -> String {
        self.status
            .as_ref()
            .and_then(|s| s.slug.clone())
            .or_else(|| self.spec.slug.clone())
            .unwrap_or_else(|| self.metadata.name.clone().unwrap_or_default())
    }
}
