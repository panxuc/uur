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
"$project_root/hook/build.sh"
"$project_root/capture/build.sh"
