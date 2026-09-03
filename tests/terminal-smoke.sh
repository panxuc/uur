#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/uur-terminal-test.XXXXXX")
prefix="$temporary/wine"
proxy="$temporary/powershell.exe"
config="$temporary/uu-terminal-bridge.runtime"
server_pid=""

cleanup() {
    if [[ -n $server_pid ]]; then
        kill "$server_pid" >/dev/null 2>&1 || true
        wait "$server_pid" 2>/dev/null || true
    fi
    WINEPREFIX="$prefix" wineserver -k >/dev/null 2>&1 || true
    rm -rf -- "$temporary"
}
trap cleanup EXIT

for tool in wine wineboot wineserver xvfb-run timeout; do
    command -v "$tool" >/dev/null 2>&1 || {
        printf 'missing terminal smoke-test dependency: %s\n' "$tool" >&2
        exit 2
    }
done

uur="$project_root/target/debug/uur"
[[ -x $uur ]] || uur="$project_root/target/release/uur"
[[ -x $uur ]] || {
    printf 'build uur before running the terminal smoke test\n' >&2
    exit 2
}
[[ -f $project_root/build/hook/uur-terminal-proxy.exe ]] ||
    "$project_root/hook/build.sh"
cp "$project_root/build/hook/uur-terminal-proxy.exe" "$proxy"

SHELL=/bin/sh UUR_TERMINAL_CONFIG="$config" "$uur" __terminal \
    >"$temporary/server.log" 2>&1 &
server_pid=$!
for _ in {1..250}; do
    [[ -s $config ]] && break
    kill -0 "$server_pid" 2>/dev/null || {
        cat "$temporary/server.log" >&2
        exit 1
    }
    sleep 0.02
done
[[ -s $config ]] || {
    printf 'terminal server did not publish its runtime configuration\n' >&2
    exit 1
}

WINEPREFIX="$prefix" WINEDEBUG=-all xvfb-run -a wineboot --init
{
    printf 'echo UUR_NATIVE_TERMINAL_OK\nexit\n'
    sleep 2
} | WINEPREFIX="$prefix" WINEDEBUG=-all \
    timeout 30s xvfb-run -a wine "$proxy" >"$temporary/output" 2>&1

grep -Fq UUR_NATIVE_TERMINAL_OK "$temporary/output" || {
    cat "$temporary/output" >&2
    exit 1
}
printf 'native terminal Wine/PTY round-trip passed\n'
