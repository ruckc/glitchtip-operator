use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Time};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Reference to another custom resource; `namespace` defaults to the
/// referencing object's namespace when unset.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObjectRef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

impl ObjectRef {
    pub fn namespace_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.namespace.as_deref().unwrap_or(fallback)
    }
}

/// Reference to a key within a Secret in the same namespace.
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SecretKeyRef {
    pub name: String,
    pub key: String,
}

/// What happens to the GlitchTip-side object when the CR is deleted.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, JsonSchema, PartialEq, Default)]
pub enum DeletionPolicy {
    #[default]
    Delete,
    Retain,
}

/// Upsert a metav1 Condition, preserving lastTransitionTime when the
/// status value did not flip.
pub fn set_condition(
    conditions: &mut Vec<Condition>,
    type_: &str,
    status: bool,
    reason: &str,
    message: &str,
    observed_generation: Option<i64>,
) {
    let status = if status { "True" } else { "False" };
    let now = Time(k8s_openapi::jiff::Timestamp::now());
    match conditions.iter_mut().find(|c| c.type_ == type_) {
        Some(existing) => {
            if existing.status != status {
                existing.last_transition_time = now;
            }
            existing.status = status.to_string();
            existing.reason = reason.to_string();
            existing.message = message.to_string();
            existing.observed_generation = observed_generation;
        }
        None => conditions.push(Condition {
            type_: type_.to_string(),
            status: status.to_string(),
            reason: reason.to_string(),
            message: message.to_string(),
            last_transition_time: now,
            observed_generation,
        }),
    }
}

pub fn is_condition_true(conditions: &[Condition], type_: &str) -> bool {
    conditions
        .iter()
        .any(|c| c.type_ == type_ && c.status == "True")
}
