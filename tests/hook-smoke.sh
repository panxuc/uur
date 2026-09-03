#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
prefix=$(mktemp -d "${TMPDIR:-/tmp}/uur-wine-test.XXXXXX")
trap 'WINEPREFIX="$prefix" wineserver -k >/dev/null 2>&1 || true; rm -rf -- "$prefix"' EXIT

for tool in wine wineboot wineserver xvfb-run timeout; do
    command -v "$tool" >/dev/null 2>&1 || {
        printf 'missing hook smoke-test dependency: %s\n' "$tool" >&2
        exit 2
    }
done

WINEPREFIX="$prefix" WINEDEBUG=-all xvfb-run -a wineboot --init
set +e
output=$(
    cd "$project_root/build/hook" &&
        WINEPREFIX="$prefix" WINEDEBUG=-all WINEDLLOVERRIDES=wevtapi=n \
            timeout 30s xvfb-run -a wine ./selftest.exe 2>&1
)
status=$?
set -e

printf '%s\n' "$output"

# Some xvfb-run versions return 1 when their Xvfb child has already exited during
# cleanup, even though the wrapped process completed successfully. Keep checking
# the self-test's terminal success marker so genuine Wine or hook failures remain
# fatal without turning that cleanup race into a CI failure.
if ((status != 0)) && [[ "$output" != *"hook capture and input smoke test passed"* ]]; then
    exit "$status"
fi
