use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{DeletionPolicy, ObjectRef};

/// A team inside a GlitchTip organization.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "glitchtip.ruck.io",
    version = "v1alpha1",
    kind = "GlitchTipTeam",
    plural = "glitchtipteams",
    namespaced,
    status = "GlitchTipTeamStatus",
    shortname = "gtteam",
    printcolumn = r#"{"name":"Organization","type":"string","jsonPath":".spec.organizationRef.name"}"#,
    printcolumn = r#"{"name":"Slug","type":"string","jsonPath":".status.slug"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type==\"Ready\")].status"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct GlitchTipTeamSpec {
    /// The GlitchTipOrganization CR this team belongs to.
    pub organization_ref: ObjectRef,
    /// Requested team slug; defaults to metadata.name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default)]
    pub deletion_policy: DeletionPolicy,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct GlitchTipTeamStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Org slug captured at creation time so deletion works even if the
    /// organization CR changes afterwards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_slug: Option<String>,
}

impl GlitchTipTeam {
    pub fn desired_slug(&self) -> String {
        self.status
            .as_ref()
            .and_then(|s| s.slug.clone())
            .or_else(|| self.spec.slug.clone())
            .unwrap_or_else(|| self.metadata.name.clone().unwrap_or_default())
    }
}
