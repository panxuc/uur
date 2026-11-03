#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/uur-check.XXXXXX")
trap 'rm -rf -- "$temporary"' EXIT

cargo fmt --manifest-path "$project_root/Cargo.toml" --check
cargo clippy --manifest-path "$project_root/Cargo.toml" \
    --all-targets -- -D warnings
cargo test --manifest-path "$project_root/Cargo.toml" --all-targets
cc -std=c11 -O2 -Wall -Wextra -Werror \
    "$project_root/tests/frame-protocol.c" -o "$temporary/frame-protocol"
"$temporary/frame-protocol"
cc -std=c11 -O2 -Wall -Wextra -Werror \
    "$project_root/tests/cursor-overlay.c" -o "$temporary/cursor-overlay"
"$temporary/cursor-overlay"
"$project_root/hook/build.sh"
"$project_root/capture/build.sh"
"$project_root/compat/build.sh"
# The shim self test needs an X display and exercises XCheckIfEvent, the entry
# point winex11.drv actually pumps events through.  Both directions matter:
# without the shim the bits survive, with it they are stripped.
if [[ -n ${DISPLAY:-} ]]; then
    selftest="$project_root/build/compat/uur-lock-mods-selftest"
    if "$selftest"; then
        printf 'lock-mod shim self test: bits survived without the shim\n' >&2
        exit 1
    fi
    if ! UUR_LOCKSTRIP_FORCE=1 \
        LD_PRELOAD="$project_root/build/compat/libuur-lock-mods.so" \
        "$selftest"; then
        printf 'lock-mod shim self test: bits survived with the shim\n' >&2
        exit 1
    fi
else
    printf 'skipping the lock-mod shim self test: no DISPLAY\n' >&2
fi
