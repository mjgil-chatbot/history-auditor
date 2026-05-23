#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
  echo "usage: $0 <smee-url> [target-url]" >&2
  exit 1
fi

smee_url="$1"
target_url="${2:-http://127.0.0.1:8787/webhooks/github}"

exec npx smee-client --url "$smee_url" --target "$target_url"
