//! Minimal Gateway API HTTPRoute types (gateway.networking.k8s.io/v1).
//! Hand-rolled to avoid gateway-api crate version pairing with kube; only
//! the fields the operator sets are mirrored. Foreign CRD — crdgen must not
//! emit it.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "gateway.networking.k8s.io",
    version = "v1",
    kind = "HTTPRoute",
    plural = "httproutes",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct HTTPRouteSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parent_refs: Vec<ParentReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hostnames: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<HTTPRouteRule>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParentReference {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_name: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct HTTPRouteRule {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backend_refs: Vec<BackendRef>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackendRef {
    pub name: String,
    pub port: i32,
}
