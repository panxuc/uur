#!/usr/bin/env bash
# Print the dotted version inside an installer filename such as
# UURemote_Setup_4.39.1.1375_0902062520_gwqd.exe or uuyc_4.33.0.exe.
set -euo pipefail

[[ $# == 1 ]] || { printf 'usage: %s <filename>\n' "$0" >&2; exit 2; }

grep -oE '[0-9]+(\.[0-9]+)+' <<<"$1" | head -n 1
