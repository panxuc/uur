#!/usr/bin/env bash
# Build the PipeWire capture helper (host C, links libpipewire).
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
readonly root
out="$root/capture"
cc="${CC:-gcc}"
flags=(-std=c11 -O2 -Wall -Wextra -Werror)

command -v "$cc" >/dev/null 2>&1 || { printf 'missing tool: %s\n' "$cc" >&2; exit 1; }

"$cc" "${flags[@]}" -o "$out/uur-pw-capture" \
    "$root/capture/uur-pw-capture.c" \
    $(pkg-config --cflags --libs libpipewire-0.3)

printf 'built: %s\n' "$out/uur-pw-capture"
