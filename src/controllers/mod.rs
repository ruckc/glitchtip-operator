pub mod instance;
pub mod organization;
pub mod project;
pub mod team;

use futures::StreamExt;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{Secret, Service};
use kube::api::Api;
use kube::runtime::Controller;
use kube::runtime::watcher::Config;
use std::sync::Arc;

use crate::context::Ctx;
use crate::crds::{GlitchTip, GlitchTipOrganization, GlitchTipProject, GlitchTipTeam};

/// Run all four controllers until shutdown. Parent→child propagation is
/// requeue-based (children requeue every ~20s while waiting on a parent, and
/// every 5m when settled for drift healing), which keeps the watch topology
/// simple; `.owns()` covers operator-owned k8s children.
pub async fn run_all(ctx: Arc<Ctx>) {
    let client = &ctx.client;

    let instances = Controller::new(Api::<GlitchTip>::all(client.clone()), Config::default())
        .owns(Api::<Deployment>::all(client.clone()), Config::default())
        .owns(Api::<Service>::all(client.clone()), Config::default())
        .owns(Api::<Job>::all(client.clone()), Config::default())
        .owns(Api::<Secret>::all(client.clone()), Config::default())
        .shutdown_on_signal()
        .run(instance::reconcile, instance::error_policy, ctx.clone())
        .for_each(|res| async move {
            if let Err(e) = res {
                tracing::debug!(error = %e, "instance controller event error");
            }
        });

    let organizations = Controller::new(
        Api::<GlitchTipOrganization>::all(client.clone()),
        Config::default(),
    )
    .shutdown_on_signal()
    .run(
        organization::reconcile,
        organization::error_policy,
        ctx.clone(),
    )
    .for_each(|res| async move {
        if let Err(e) = res {
            tracing::debug!(error = %e, "organization controller event error");
        }
    });

    let teams = Controller::new(Api::<GlitchTipTeam>::all(client.clone()), Config::default())
        .shutdown_on_signal()
        .run(team::reconcile, team::error_policy, ctx.clone())
        .for_each(|res| async move {
            if let Err(e) = res {
                tracing::debug!(error = %e, "team controller event error");
            }
        });

    let projects = Controller::new(
        Api::<GlitchTipProject>::all(client.clone()),
        Config::default(),
    )
    .owns(Api::<Secret>::all(client.clone()), Config::default())
    .shutdown_on_signal()
    .run(project::reconcile, project::error_policy, ctx.clone())
    .for_each(|res| async move {
        if let Err(e) = res {
            tracing::debug!(error = %e, "project controller event error");
        }
    });

    futures::join!(instances, organizations, teams, projects);
}
