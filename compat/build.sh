#!/usr/bin/env bash
# Build the Wine lock-modifier shim (LD_PRELOAD, links libX11).
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
readonly root
out="$root/build/compat"
cc="${CC:-gcc}"
flags=(-std=c11 -O2 -Wall -Wextra -Werror -fPIC)

command -v "$cc" >/dev/null 2>&1 || { printf 'missing tool: %s\n' "$cc" >&2; exit 1; }
command -v pkg-config >/dev/null 2>&1 || { printf 'missing tool: pkg-config\n' >&2; exit 1; }

mkdir -p "$out"

"$cc" "${flags[@]}" -shared -o "$out/libuur-lock-mods.so" \
    "$root/compat/uur-lock-mods.c" \
    $(pkg-config --cflags --libs x11) -ldl

printf 'built: %s\n' "$out/libuur-lock-mods.so"

# Self test: delivers a synthetic key event carrying Mod2|Mod4 through
# XCheckIfEvent and asserts the bits are gone.
"$cc" "${flags[@]}" -o "$out/uur-lock-mods-selftest" \
    "$root/compat/selftest.c" \
    $(pkg-config --cflags --libs x11)

printf 'built: %s\n' "$out/uur-lock-mods-selftest"
