use kube::Client;
use std::sync::Arc;

use glitchtip_operator::context::Ctx;
use glitchtip_operator::controllers;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,kube=warn".into()),
        )
        .init();

    let client = Client::try_default().await?;
    let gateway_api_available = gateway_api_available(&client).await;
    if !gateway_api_available {
        tracing::warn!(
            "gateway.networking.k8s.io not found; HTTPRoute creation will be skipped and \
             instances with spec.route.enabled will report GatewayAPIUnavailable"
        );
    }

    let ctx = Arc::new(Ctx {
        client,
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?,
        gateway_api_available,
    });

    tracing::info!("starting glitchtip-operator controllers");
    controllers::run_all(ctx).await;
    tracing::info!("controllers shut down");
    Ok(())
}

async fn gateway_api_available(client: &Client) -> bool {
    match client.list_api_groups().await {
        Ok(groups) => groups
            .groups
            .iter()
            .any(|g| g.name == "gateway.networking.k8s.io"),
        Err(e) => {
            tracing::warn!(%e, "api group discovery failed; assuming Gateway API is absent");
            false
        }
    }
}
