use serde::{Deserialize, Deserializer, Serialize};

/// GlitchTip/Sentry APIs are inconsistent about numeric vs string ids.
fn id_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(v.map(|v| match v {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }))
}

#[derive(Deserialize, Debug, Clone)]
pub struct Organization {
    #[serde(default, deserialize_with = "id_string")]
    pub id: Option<String>,
    pub slug: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Team {
    #[serde(default, deserialize_with = "id_string")]
    pub id: Option<String>,
    pub slug: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Project {
    #[serde(default, deserialize_with = "id_string")]
    pub id: Option<String>,
    pub slug: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ProjectKey {
    #[serde(default, deserialize_with = "id_string")]
    pub id: Option<String>,
    #[serde(default)]
    pub public: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub dsn: Option<Dsn>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Dsn {
    #[serde(default)]
    pub public: Option<String>,
    #[serde(default)]
    pub security: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct CreateOrganization<'a> {
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<&'a str>,
}

#[derive(Serialize, Debug, Clone)]
pub struct CreateTeam<'a> {
    pub slug: &'a str,
}

#[derive(Serialize, Debug, Clone)]
pub struct CreateProject<'a> {
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<&'a str>,
}
