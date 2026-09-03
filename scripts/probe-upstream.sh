#!/usr/bin/env bash
set -euo pipefail

input=${1:?usage: scripts/probe-upstream.sh INSTALLER_OR_CLIENT_TREE}
work=$(mktemp -d "${TMPDIR:-/tmp}/uur-probe.XXXXXX")
trap 'rm -rf -- "$work"' EXIT

for tool in 7z objdump find strings; do
    command -v "$tool" >/dev/null 2>&1 || {
        printf 'missing probe dependency: %s\n' "$tool" >&2
        exit 2
    }
done

scan_root=$input
if [[ -f $input ]] && command -v osslsigncode >/dev/null 2>&1; then
    osslsigncode verify -in "$input" >"$work/signature.log" 2>&1 || {
        printf 'the upstream installer has no valid Authenticode signature\n' >&2
        exit 1
    }
fi

if [[ -f $input ]]; then
    7z x -y -o"$work/payload" "$input" >"$work/extract.log" || true
    scan_root="$work/payload"
fi
mapfile -d '' pe_files < <(
    find "$scan_root" -type f \( -iname '*.exe' -o -iname '*.dll' \) -print0
)
if ((${#pe_files[@]} == 0)) && [[ -f $input ]]; then
    for tool in wine wineboot xvfb-run timeout; do
        command -v "$tool" >/dev/null 2>&1 || {
            printf 'installer extraction is opaque and %s is unavailable for the Wine probe\n' \
                "$tool" >&2
            exit 2
        }
    done
    export WINEPREFIX="$work/wine"
    export WINEDEBUG=-all
    xvfb-run -a wineboot --init >"$work/wineboot.log" 2>&1
    timeout 240s xvfb-run -a wine "$input" /S >"$work/install.log" 2>&1 || true
    scan_root="$WINEPREFIX/drive_c"
    mapfile -d '' pe_files < <(
        find "$scan_root" -type f \( -iname '*.exe' -o -iname '*.dll' \) -print0
    )
fi
((${#pe_files[@]} > 0)) || {
    printf 'no PE payload was available after extraction and Wine installation\n' >&2
    exit 1
}

imports="$work/imports.txt"
strings_file="$work/strings.txt"
for file in "${pe_files[@]}"; do
    objdump -p "$file" >>"$imports" 2>/dev/null || true
    strings -a "$file" >>"$strings_file" 2>/dev/null || true
done

require_symbol() {
    local symbol=$1
    if ! grep -Fqi "$symbol" "$imports" && ! grep -Fqi "$symbol" "$strings_file"; then
        printf 'missing upstream API contract: %s\n' "$symbol" >&2
        return 1
    fi
}

require_symbol SendInput
require_symbol EvtOpenPublisherMetadata
if ! grep -Eqi 'BitBlt|StretchBlt|CreateDXGIFactory1|DuplicateOutput' \
    "$imports" "$strings_file"; then
    printf 'missing every supported desktop capture API contract\n' >&2
    exit 1
fi

printf 'upstream static contracts passed (%d PE files)\n' "${#pe_files[@]}"
