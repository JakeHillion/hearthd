#!/usr/bin/env nix-shell
#! nix-shell -i bash -p curl jq
# Example script to control a light via the hearthd API
#
# This script demonstrates:
# - Listing all entities
# - Getting specific entity state
# - Turning a light on
# - Turning a light off

set -e

# Configuration
API_BASE="${HEARTHD_API:-http://127.0.0.1:8565}"
LIGHT_ID="${1:-light.1221051039810110150109113116116_2}"

echo "=== hearthd API Control Example ==="
echo "API Base: $API_BASE"
echo "Light ID: $LIGHT_ID"
echo

# Check API is alive
echo "1. Checking API health..."
if ! response=$(curl -s -f "${API_BASE}/v1/ping" 2>&1); then
  echo "ERROR: Failed to connect to hearthd API at ${API_BASE}"
  echo "Make sure hearthd is running with: cargo run"
  echo "Response: $response"
  exit 1
fi
echo "$response" | jq .
echo

# List all entities
echo "2. Listing all entities..."
curl -s "${API_BASE}/v1/entities" | jq .
echo

# Get specific light state
echo "3. Getting light state for ${LIGHT_ID}..."
curl -s "${API_BASE}/v1/entities/${LIGHT_ID}" | jq .
echo

# Turn light ON
echo "4. Turning light ON..."
curl -s -X POST "${API_BASE}/v1/entities/${LIGHT_ID}/command" \
  -H "Content-Type: application/json" \
  -d '{"command": "light", "on": true, "brightness": 254}' | jq .
echo

echo "Waiting 5 seconds..."
sleep 5

# Turn light OFF
echo "5. Turning light OFF..."
curl -s -X POST "${API_BASE}/v1/entities/${LIGHT_ID}/command" \
  -H "Content-Type: application/json" \
  -d '{"command": "light", "on": false}' | jq .
echo

# Dump all state
echo "6. Dumping all entity states..."
curl -s "${API_BASE}/v1/dump_state" | jq .
echo

echo "=== Done ==="
