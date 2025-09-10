#!/bin/bash

# StatusEngine Command Provider Script
# Extracts timing and session information from stdin JSON payload
# Usage: Configure in ~/.codex/config.toml with command = "/path/to/status-provider.sh"

# Read JSON payload from stdin
payload=$(cat)

# Check if jq is available for JSON parsing
if command -v jq >/dev/null 2>&1; then
    # Use jq for robust JSON parsing
    timing=$(echo "$payload" | jq -r '.timing.since_session_ms // 0')
    session_id=$(echo "$payload" | jq -r '.session_id // "no-session"')
    
    # Convert milliseconds to human-readable format
    if [ "$timing" -gt 0 ]; then
        if [ "$timing" -ge 60000 ]; then
            # More than 1 minute
            minutes=$((timing / 60000))
            seconds=$(((timing % 60000) / 1000))
            timing_display="${minutes}m${seconds}s"
        elif [ "$timing" -ge 1000 ]; then
            # More than 1 second
            seconds=$((timing / 1000))
            timing_display="${seconds}s"
        else
            # Less than 1 second
            timing_display="${timing}ms"
        fi
    else
        timing_display="0s"
    fi
    
else
    # Fallback: Basic text parsing without jq
    timing=$(echo "$payload" | grep -o '"since_session_ms":[0-9]*' | grep -o '[0-9]*')
    session_id=$(echo "$payload" | grep -o '"session_id":"[^"]*"' | cut -d'"' -f4)
    
    # Default values if extraction failed
    timing=${timing:-0}
    session_id=${session_id:-"no-session"}
    
    # Simple timing display
    if [ "$timing" -ge 1000 ]; then
        timing_display="$((timing / 1000))s"
    else
        timing_display="${timing}ms"
    fi
fi

# Generate Line 3 output (first line only will be used)
# Format: [timing] [session_id_short]
session_short=$(echo "$session_id" | cut -c1-8)
echo "⏱️ ${timing_display} • 🔑 ${session_short}"

# Debug output to stderr (won't appear in Line 3)
# echo "DEBUG: Full payload received" >&2
# echo "$payload" >&2