# glitchtip-operator

A Kubernetes operator (Rust, [kube-rs](https://kube.rs)) that deploys
[GlitchTip](https://glitchtip.com) instances and manages organizations, teams
and projects declaratively — delivering each project's DSN as a Secret in the
consuming application's namespace.

## What it manages

| CRD | Short name | Purpose |
|---|---|---|
| `GlitchTip` | `gt` | Full instance: web + worker Deployments, migration & bootstrap Jobs, PostgreSQL via [pgop](https://pgop.ruck.io), optional Valkey, Service, optional Gateway API `HTTPRoute` |
| `GlitchTipOrganization` | `gtorg` | Organization inside an instance (via the GlitchTip REST API) |
| `GlitchTipTeam` | `gtteam` | Team inside an organization |
| `GlitchTipProject` | `gtproj` | Project + client key; writes the DSN Secret next to the CR |

The chain is: `GlitchTipProject` → `GlitchTipOrganization` → `GlitchTip`,
with teams referenced by `teamRef` or a raw `teamSlug`. See
[`examples/`](examples/) for complete manifests.

## How it works

- **PostgreSQL** is provisioned by creating pgop `Cluster`/`Role`/`Database`
  CRs in the instance's namespace (or an existing pgop Cluster via
  `spec.database.clusterRef`). The operator composes `DATABASE_URL` from
  pgop's `<database>-<owner>-credentials` Secret.
  `spec.database.deletionPolicy` defaults to `Retain`, so deleting a
  `GlitchTip` CR keeps your data.
- **Migrations** run as a per-revision Job (`./bin/run-migrations.sh`) and
  gate web/worker rollout.
- **API token bootstrap**: the operator pre-generates a token into
  `<name>-api-token`, then a one-shot Job creates a superuser service account
  and inserts the token via `manage.py shell`. Pre-existing instances can
  supply `spec.apiTokenSecretRef` instead.
- **Valkey** is optional (`spec.valkey.enabled`) — GlitchTip v5+ can use
  PostgreSQL as its task broker.
- **HTTPRoute** (`spec.route`) attaches the web Service to your Gateway; if
  the Gateway API CRDs are absent the instance reports
  `Ready=False/GatewayAPIUnavailable` instead of crashing.
- **Drift healing**: every CR re-reconciles on a 5-minute cadence; orgs,
  teams, projects, keys and Secrets deleted or mutated out-of-band are
  recreated/rewritten (Secrets via server-side apply ownership).
- **Deletion**: `deletionPolicy: Delete` (default for org/team/project)
  removes the object via the GlitchTip API through a finalizer; `Retain`
  releases the CR without touching GlitchTip. Finalizers never wedge when the
  parent instance is already gone.

## Install

```sh
helm install glitchtip-operator charts/glitchtip-operator \
  --namespace glitchtip-system --create-namespace
```

Prerequisites: [pgop](https://pgop.ruck.io) installed; Gateway API CRDs if
you use `spec.route`.

## Quick start

```sh
kubectl create ns glitchtip
kubectl apply -f examples/instance.yaml       # the instance
kubectl apply -f examples/organization.yaml   # org + team
kubectl apply -f examples/project.yaml        # project in the app namespace
kubectl get secret -n my-app my-app-glitchtip-dsn -o jsonpath='{.data.SENTRY_DSN}' | base64 -d
```

## Development

```sh
mise install                  # rust toolchain
cargo test                    # unit + wiremock API-client tests
cargo run --bin crdgen        # print CRDs
hack/kind-up.sh               # kind + Gateway API CRDs + pgop + CRDs
RUST_LOG=info cargo run       # run the operator against your kubeconfig
hack/e2e.sh                   # end-to-end smoke test (real glitchtip image)
```

Regenerate the chart's CRDs after changing spec types:

```sh
cargo run --bin crdgen > charts/glitchtip-operator/crds/glitchtip.ruck.io.yaml
```

## Notes / verification points

- Worker command defaults to `./bin/run-worker.sh` for v5+ images and
  `./bin/run-celery-with-beat.sh` for older tags; override with
  `spec.worker.command`.
- The bootstrap Job imports `apps.api_tokens.models.APIToken`; if a future
  GlitchTip release moves that model, the Job logs will say so — override via
  `spec.apiTokenSecretRef` as an escape hatch.
