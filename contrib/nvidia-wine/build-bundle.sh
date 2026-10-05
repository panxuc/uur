#!/usr/bin/env bash
# Explicit developer action; no sudo, system Wine changes or client downloads.
set -euo pipefail
contrib_dir=$(cd -- "$(dirname -- "$0")" && pwd)
bundle_dir=${1:?Usage: build-bundle.sh OUTPUT_DIRECTORY}
mkdir -p -- "$bundle_dir"
bundle_dir=$(cd -- "$bundle_dir" && pwd)
if [[ -e "$bundle_dir/source" || -e "$bundle_dir/nvcuda.dll.so" ]]; then
    printf 'Use a new output directory; refusing to overwrite a bridge build.\n' >&2
    exit 1
fi
winegcc_tool=${WINEGCC:-winegcc}
command -v "$winegcc_tool" >/dev/null
source_revision=8982084679efc390d217a7f67553f1e319eefe10
git init -q "$bundle_dir/source"
git -C "$bundle_dir/source" remote add origin https://github.com/SveSop/nvcuda.git
git -C "$bundle_dir/source" fetch -q --depth=1 origin "$source_revision"
git -C "$bundle_dir/source" checkout -q --detach FETCH_HEAD
[[ $(git -C "$bundle_dir/source" rev-parse HEAD) == "$source_revision" ]]
patch --batch --fuzz=0 -d "$bundle_dir/source" -p1 < "$contrib_dir/nvcuda-output-textures.patch"
cp -- "$contrib_dir/uur_nvdec_bridge.c" "$bundle_dir/source/dlls/nvcuda/"
relay_dir="$bundle_dir/source/dlls/nvcuda"
"$winegcc_tool" -O2 -D__WINESRC__ -m64 -shared -o "$bundle_dir/nvcuda.dll" \
    "$relay_dir/nvcuda.spec" "$relay_dir/nvcuda.c" "$relay_dir/internal.c" \
    "$relay_dir/function_mappings.c" "$relay_dir/encryption.c" \
    -I"$bundle_dir/source/include" -I"$relay_dir" -ldl -lpthread -lsetupapi -ldxgi -luuid
archive="$bundle_dir/nvidia-libs-v1.0.2.tar.xz"
curl --fail --location --proto '=https' --tlsv1.2 --output "$archive" \
    https://github.com/SveSop/nvidia-libs/releases/download/v1.0.2/nvidia-libs-v1.0.2.tar.xz
printf '%s  %s\n' 01e8bb6368d088e22d8e8f1d02497214e8db436476021725ef0c0707b7cb1738 "$archive" | sha256sum -c -
tar -xOf "$archive" nvidia-libs-v1.0.2/x64/nvcuvid.dll > "$bundle_dir/nvcuvid.dll"
printf '%s  %s\n' fd51c2f98f8006f097240a1d2cf53d72a6d1b741618fb679226ec563d2ad0944 "$bundle_dir/nvcuvid.dll" | sha256sum -c -
# Preserve the corresponding source and notices for the unchanged CUVID relay.
nvenc_revision=09ec9e1c8b25a351e415a6d0361bfacad0fd710c
git init -q "$bundle_dir/nvenc-source"
git -C "$bundle_dir/nvenc-source" remote add origin https://github.com/SveSop/nvenc.git
git -C "$bundle_dir/nvenc-source" fetch -q --depth=1 origin "$nvenc_revision"
git -C "$bundle_dir/nvenc-source" checkout -q --detach FETCH_HEAD
[[ $(git -C "$bundle_dir/nvenc-source" rev-parse HEAD) == "$nvenc_revision" ]]
cp -- "$contrib_dir/LICENSE.md" "$bundle_dir/LICENSE-nvcuda.md"
printf 'Built opt-in bundle: %s\nRetain source, the pinned CUVID archive and licenses with this build.\n' "$bundle_dir"
