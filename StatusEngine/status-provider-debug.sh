#!/usr/bin/env bash
# Debug StatusEngine provider script
# - Reads JSON payload from stdin
# - Saves it to status-payload.json alongside this script
# - Prints a single status line for the TUI line3

set -euo pipefail

# Resolve script directory (works when invoked via absolute or relative path)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_FILE="$SCRIPT_DIR/status-payload.json"

# Optional: also keep a rolling log in the workspace if desired
LOG_DIR="${STATUSENGINE_LOG_DIR:-.statusengine-debug}"
mkdir -p "$LOG_DIR"
TS="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

# Read the full payload from stdin
PAYLOAD="$(cat)"

# Save the payload exactly as received next to the script
printf "%s" "$PAYLOAD" >"$OUT_FILE"

# Also append a colorized (if jq available) copy to a rolling log for convenience
printf "%s" "$PAYLOAD" >"$LOG_DIR/last.json"
if command -v jq >/dev/null 2>&1; then
  { echo "[$TS] payload:"; echo "$PAYLOAD" | jq -C .; } >>"$LOG_DIR/payload.log"
else
  echo "[$TS] payload: $PAYLOAD" >>"$LOG_DIR/payload.log"
fi

# Extract a couple of fields for a human-friendly line (optional)
MODEL=""
MS="0"
if command -v jq >/dev/null 2>&1; then
  MODEL="$(jq -r '.model.id // empty' <<<"$PAYLOAD")"
  MS="$(jq -r '.timing.since_session_ms // 0' <<<"$PAYLOAD")"
fi
SECS=$(( MS / 1000 ))

# Emit a single-line status for the TUI (first stdout line only is used)
printf "debug: model=%s, up %ss (saved payload to %s)\n" "${MODEL:-unknown}" "$SECS" "$OUT_FILE"

