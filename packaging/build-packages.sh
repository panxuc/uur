#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$project_root/Cargo.toml" | head -n1)
output="$project_root/dist"
stage="$output/root"

mkdir -p "$output" "$stage"
find "$stage" -mindepth 1 -delete
find "$output" -maxdepth 1 -type f \
    \( -name 'uur-*.tar.zst' -o -name 'uur-*.rpm' -o -name 'uur_*.deb' \
       -o -name 'uur_*.apk' -o -name 'SHA256SUMS' \) -delete

cargo build --manifest-path "$project_root/Cargo.toml" --release --locked
"$project_root/hook/build.sh"
"$project_root/capture/build.sh"
"$project_root/packaging/stage.sh" "$stage"

tar --zstd -C "$stage" -cf "$output/uur-${version}-linux-x86_64.tar.zst" .

if command -v nfpm >/dev/null 2>&1; then
    nfpm_config="$output/nfpm.yaml"
    sed -e "s|@UUR_VERSION@|$version|g" \
        -e "s|@UUR_STAGE@|$stage|g" \
        -e "s|@UUR_PROJECT_ROOT@|$project_root|g" \
        "$project_root/packaging/nfpm.yaml" >"$nfpm_config"
    for format in deb rpm; do
        nfpm package \
            --config "$nfpm_config" \
            --packager "$format" \
            --target "$output/"
    done
else
    printf 'nfpm not found; created only the portable tar.zst bundle\n' >&2
fi

if command -v makepkg >/dev/null 2>&1; then
    "$project_root/packaging/build-arch-package.sh" "$output"
else
    printf 'makepkg not found; Arch package was not created\n' >&2
fi

artifacts=()
while IFS= read -r -d '' artifact; do
    artifacts+=("$artifact")
done < <(
    find "$output" -maxdepth 1 -type f \
        \( -name "uur-${version}*.tar.zst" -o \
           -name "uur-${version}*.rpm" -o \
           -name "uur_${version}*.deb" -o \
           -name "uur_${version}*.apk" \) \
        -print0 | sort -z
)
((${#artifacts[@]} > 0)) || {
    printf 'no release artifacts were produced\n' >&2
    exit 1
}
artifact_names=()
for artifact in "${artifacts[@]}"; do
    artifact_names+=("${artifact##*/}")
done
(
    cd "$output"
    sha256sum "${artifact_names[@]}" >SHA256SUMS
)
