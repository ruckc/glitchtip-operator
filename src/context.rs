use kube::Client;
use std::sync::Arc;

use crate::crds::GlitchTip;
use crate::crds::instance::WEB_PORT;
use crate::error::{Error, Result};
use crate::glitchtip::GlitchTipClient;
use crate::util::secrets;

/// Shared state handed to every reconciler.
pub struct Ctx {
    pub client: Client,
    pub http: reqwest::Client,
    /// Result of the startup discovery check for gateway.networking.k8s.io.
    pub gateway_api_available: bool,
}

impl Ctx {
    /// Name of the Secret holding the operator's API token for an instance,
    /// plus the key within it.
    pub fn api_token_source(gt: &GlitchTip) -> (String, String) {
        match &gt.spec.api_token_secret_ref {
            Some(r) => (r.name.clone(), r.key.clone()),
            None => (
                format!(
                    "{}-api-token",
                    gt.metadata.name.as_deref().unwrap_or_default()
                ),
                "token".to_string(),
            ),
        }
    }

    /// In-cluster base URL of an instance's web service. Used instead of the
    /// public domain so the operator works before any Gateway is wired up.
    pub fn instance_base_url(gt: &GlitchTip) -> Result<String> {
        let name = gt
            .metadata
            .name
            .as_deref()
            .ok_or_else(|| Error::Config("instance has no name".into()))?;
        let ns = gt
            .metadata
            .namespace
            .as_deref()
            .ok_or_else(|| Error::Config("instance has no namespace".into()))?;
        Ok(format!("http://{name}-web.{ns}.svc:{WEB_PORT}"))
    }

    /// Build an API client for a GlitchTip instance from its token Secret.
    pub async fn glitchtip_client(self: &Arc<Self>, gt: &GlitchTip) -> Result<GlitchTipClient> {
        let ns = gt
            .metadata
            .namespace
            .as_deref()
            .ok_or_else(|| Error::Config("instance has no namespace".into()))?;
        let (secret_name, key) = Self::api_token_source(gt);
        let token = secrets::get_secret_key(&self.client, ns, &secret_name, &key).await?;
        Ok(GlitchTipClient::new(
            self.http.clone(),
            &Self::instance_base_url(gt)?,
            &token,
        ))
    }
}
