#!/usr/bin/env bash
# End-to-end smoke test against the current kubeconfig context.
# Prereqs: hack/kind-up.sh (or equivalent cluster with pgop + CRDs), and the
# operator running (locally via `cargo run`, or deployed via the Helm chart).
set -euo pipefail
cd "$(dirname "$0")/.."

NS_INSTANCE="${NS_INSTANCE:-glitchtip}"
NS_APP="${NS_APP:-my-app}"
TIMEOUT="${TIMEOUT:-900s}"

kubectl create namespace "$NS_INSTANCE" --dry-run=client -o yaml | kubectl apply -f -
kubectl create namespace "$NS_APP" --dry-run=client -o yaml | kubectl apply -f -

echo ">>> applying example CRs"
# Route disabled for e2e: kind has no Gateway implementation by default.
kubectl apply -n "$NS_INSTANCE" -f - <<'EOF'
apiVersion: glitchtip.ruck.io/v1alpha1
kind: GlitchTip
metadata:
  name: glitchtip
spec:
  domain: http://glitchtip.localtest.me
  database:
    storageSize: 2Gi
    deletionPolicy: Delete
  route:
    enabled: false
EOF
kubectl apply -f examples/organization.yaml
kubectl apply -f examples/project.yaml

echo ">>> waiting for GlitchTip instance Ready (timeout ${TIMEOUT})"
kubectl wait -n "$NS_INSTANCE" glitchtip/glitchtip \
  --for=condition=Ready --timeout="$TIMEOUT"

echo ">>> waiting for organization / team / project Ready"
kubectl wait -n "$NS_INSTANCE" glitchtiporganization/acme --for=condition=Ready --timeout=300s
kubectl wait -n "$NS_INSTANCE" glitchtipteam/backend --for=condition=Ready --timeout=300s
kubectl wait -n "$NS_APP" glitchtipproject/my-app --for=condition=Ready --timeout=300s

echo ">>> asserting DSN secret"
DSN=$(kubectl get secret -n "$NS_APP" my-app-glitchtip-dsn -o jsonpath='{.data.SENTRY_DSN}' | base64 -d)
[[ "$DSN" == http* ]] || { echo "unexpected DSN: $DSN"; exit 1; }
echo "    SENTRY_DSN=$DSN"

echo ">>> verifying org via the GlitchTip API"
TOKEN=$(kubectl get secret -n "$NS_INSTANCE" glitchtip-api-token -o jsonpath='{.data.token}' | base64 -d)
kubectl port-forward -n "$NS_INSTANCE" svc/glitchtip-web 18000:8000 >/dev/null 2>&1 &
PF_PID=$!
trap 'kill $PF_PID 2>/dev/null || true' EXIT
sleep 3
ORGS=$(curl -sf -H "Authorization: Bearer $TOKEN" http://127.0.0.1:18000/api/0/organizations/)
echo "$ORGS" | grep -q '"slug":"acme"' || { echo "org acme not found: $ORGS"; exit 1; }

echo ">>> deleting project and asserting API-side cleanup"
kubectl delete -n "$NS_APP" glitchtipproject/my-app --wait --timeout=120s
sleep 2
if curl -sf -H "Authorization: Bearer $TOKEN" http://127.0.0.1:18000/api/0/projects/acme/my-app/ >/dev/null; then
  echo "project still exists in GlitchTip after CR deletion"; exit 1
fi

echo "e2e OK"
