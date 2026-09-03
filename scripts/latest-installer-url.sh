#!/usr/bin/env bash
# Resolve the official NetEase UU Remote installer URL (curl, redirects).
set -euo pipefail

readonly feed_url="https://api.nrd.nie.163.com/api/v1/release/dl/1?channel=gwqd"

curl -fsIL --max-time 20 -o /dev/null -w '%{url_effective}' "$feed_url"
