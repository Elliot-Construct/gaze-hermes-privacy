#!/usr/bin/env bash
set -euo pipefail

# Docker health/auth smoke test for Gaze sidecar

cleanup() {
    echo "Cleaning up..."
    docker compose -f ../docker-compose.yml down -v
}
trap cleanup EXIT

# Create temporary synthetic secrets
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

GAZE_TEST_TOKEN=$(openssl rand -base64 32 | tr -d '\n')
SNAPSHOT_KEY=$(openssl rand -base64 32 | tr -d '\n')

echo "$GAZE_TEST_TOKEN" > "$TMPDIR/api-token"
echo "$SNAPSHOT_KEY" > "$TMPDIR/snapshot-key"

# Create minimal no-NER test policy
cat > "$TMPDIR/global.toml" <<'EOF'
schema_version = "0.1.0"

[session]
scope = "conversation"

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "class"
class = "name"
action = "tokenize"
EOF

# Start compose service
echo "Starting docker compose..."
docker compose -f ../docker-compose.yml up -d

# Wait for service to be ready
echo "Waiting for sidecar to be ready..."
for i in {1..30}; do
    if curl --fail --silent http://127.0.0.1:65113/healthz >/dev/null 2>&1; then
        echo "Sidecar is ready"
        break
    fi
    sleep 1
done

# Test healthz (unauthenticated)
echo "Testing /healthz..."
curl --fail http://127.0.0.1:65113/healthz

# Test unauthenticated /v1/status (should return 401)
echo "Testing unauthenticated /v1/status (expect 401)..."
if curl --fail-with-body http://127.0.0.1:65113/v1/status; then
    echo "ERROR: /v1/status should require authentication"
    exit 1
elif [ "$?" -eq 22 ]; then
    echo "OK: /v1/status returned 401 as expected"
else
    echo "ERROR: Unexpected exit code from curl"
    exit 1
fi

# Test authenticated /v1/status
echo "Testing authenticated /v1/status..."
curl --fail -H "Authorization: Bearer $GAZE_TEST_TOKEN" http://127.0.0.1:65113/v1/status

echo "All smoke tests passed!"