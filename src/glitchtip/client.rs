use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use super::models::*;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("unauthorized ({0}): check the operator API token")]
    Unauthorized(StatusCode),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("glitchtip api returned {0}: {1}")]
    Http(StatusCode, String),
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
}

/// Typed client for GlitchTip's Sentry-compatible REST API (`/api/0/`).
#[derive(Clone)]
pub struct GlitchTipClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl GlitchTipClient {
    pub fn new(http: reqwest::Client, base_url: &str, token: &str) -> Self {
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
            http,
        }
    }

    async fn request<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<reqwest::Response, ApiError> {
        let mut req = self
            .http
            .request(method, format!("{}{}", self.base, path))
            .bearer_auth(&self.token);
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let text = resp.text().await.unwrap_or_default();
        Err(match status {
            StatusCode::NOT_FOUND => ApiError::NotFound,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ApiError::Unauthorized(status),
            StatusCode::CONFLICT => ApiError::Conflict(text),
            StatusCode::BAD_REQUEST if text.contains("already exists") => ApiError::Conflict(text),
            _ => ApiError::Http(status, truncate(&text)),
        })
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        Ok(self
            .request::<()>(Method::GET, path, None)
            .await?
            .json()
            .await?)
    }

    async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        Ok(self
            .request(Method::POST, path, Some(body))
            .await?
            .json()
            .await?)
    }

    /// Delete that treats 404 as success (idempotent cleanup).
    async fn delete(&self, path: &str) -> Result<(), ApiError> {
        match self.request::<()>(Method::DELETE, path, None).await {
            Ok(_) | Err(ApiError::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Cheap liveness + token validation probe.
    pub async fn ping(&self) -> Result<(), ApiError> {
        self.request::<()>(Method::GET, "/api/0/organizations/?limit=1", None)
            .await?;
        Ok(())
    }

    // Organizations

    pub async fn get_organization(&self, slug: &str) -> Result<Organization, ApiError> {
        self.get_json(&format!("/api/0/organizations/{slug}/"))
            .await
    }

    pub async fn create_organization(
        &self,
        name: &str,
        slug: Option<&str>,
    ) -> Result<Organization, ApiError> {
        self.post_json("/api/0/organizations/", &CreateOrganization { name, slug })
            .await
    }

    pub async fn update_organization(
        &self,
        slug: &str,
        name: &str,
    ) -> Result<Organization, ApiError> {
        let body = serde_json::json!({ "name": name });
        Ok(self
            .request(
                Method::PUT,
                &format!("/api/0/organizations/{slug}/"),
                Some(&body),
            )
            .await?
            .json()
            .await?)
    }

    pub async fn delete_organization(&self, slug: &str) -> Result<(), ApiError> {
        self.delete(&format!("/api/0/organizations/{slug}/")).await
    }

    // Teams

    pub async fn get_team(&self, org: &str, team: &str) -> Result<Team, ApiError> {
        self.get_json(&format!("/api/0/teams/{org}/{team}/")).await
    }

    pub async fn create_team(&self, org: &str, slug: &str) -> Result<Team, ApiError> {
        self.post_json(
            &format!("/api/0/organizations/{org}/teams/"),
            &CreateTeam { slug },
        )
        .await
    }

    pub async fn delete_team(&self, org: &str, team: &str) -> Result<(), ApiError> {
        self.delete(&format!("/api/0/teams/{org}/{team}/")).await
    }

    // Projects

    pub async fn get_project(&self, org: &str, project: &str) -> Result<Project, ApiError> {
        self.get_json(&format!("/api/0/projects/{org}/{project}/"))
            .await
    }

    pub async fn create_project(
        &self,
        org: &str,
        team: &str,
        name: &str,
        slug: Option<&str>,
        platform: Option<&str>,
    ) -> Result<Project, ApiError> {
        self.post_json(
            &format!("/api/0/teams/{org}/{team}/projects/"),
            &CreateProject {
                name,
                slug,
                platform,
            },
        )
        .await
    }

    pub async fn update_project(
        &self,
        org: &str,
        project: &str,
        name: &str,
        platform: Option<&str>,
    ) -> Result<Project, ApiError> {
        let body = serde_json::json!({ "name": name, "platform": platform });
        Ok(self
            .request(
                Method::PUT,
                &format!("/api/0/projects/{org}/{project}/"),
                Some(&body),
            )
            .await?
            .json()
            .await?)
    }

    pub async fn delete_project(&self, org: &str, project: &str) -> Result<(), ApiError> {
        self.delete(&format!("/api/0/projects/{org}/{project}/"))
            .await
    }

    // Keys

    pub async fn list_keys(&self, org: &str, project: &str) -> Result<Vec<ProjectKey>, ApiError> {
        self.get_json(&format!("/api/0/projects/{org}/{project}/keys/"))
            .await
    }

    pub async fn create_key(
        &self,
        org: &str,
        project: &str,
        label: &str,
    ) -> Result<ProjectKey, ApiError> {
        let body = serde_json::json!({ "name": label });
        self.post_json(&format!("/api/0/projects/{org}/{project}/keys/"), &body)
            .await
    }
}

fn truncate(s: &str) -> String {
    const MAX: usize = 500;
    if s.len() <= MAX {
        s.to_string()
    } else {
        let mut end = MAX;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}
