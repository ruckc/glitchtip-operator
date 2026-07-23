#!/usr/bin/env bash
# Bring up a kind cluster with everything the operator needs:
# Gateway API CRDs, pgop, and this repo's CRDs.
set -euo pipefail
cd "$(dirname "$0")/.."

CLUSTER_NAME="${CLUSTER_NAME:-glitchtip-operator}"
# Kept in sync with .github/workflows/ci.yaml so local dev matches CI.
GATEWAY_API_VERSION="${GATEWAY_API_VERSION:-v1.6.1}"
PGOP_VERSION="${PGOP_VERSION:-v0.4.5}"

if ! kind get clusters 2>/dev/null | grep -qx "$CLUSTER_NAME"; then
  kind create cluster --name "$CLUSTER_NAME"
fi

echo ">>> installing Gateway API CRDs ${GATEWAY_API_VERSION}"
kubectl apply -f "https://github.com/kubernetes-sigs/gateway-api/releases/download/${GATEWAY_API_VERSION}/standard-install.yaml"

echo ">>> installing pgop ${PGOP_VERSION}"
helm upgrade --install pgop oci://ghcr.io/ruckc/charts/pgop \
  --version "${PGOP_VERSION#v}" \
  --namespace pgop-system \
  --create-namespace \
  --wait --timeout=180s

echo ">>> installing glitchtip-operator CRDs"
cargo run --bin crdgen | kubectl apply -f -

echo ">>> done. Run the operator locally with:"
echo "    RUST_LOG=info cargo run"
