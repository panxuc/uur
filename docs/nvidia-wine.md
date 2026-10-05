# Experimental NVIDIA/Wine controller decode bridge

Related: [proposal #19](https://github.com/panxuc/uur/issues/19).

This default-off experiment lets the **official Windows UU client running under
Wine**, when controlling another computer, use NVIDIA's native Linux NVDEC
through CUDA/CUVID relay libraries. It is separate from the native capture and
future native video backends in `src/video.rs`. No proprietary executable is
modified, and no decoder capabilities are manufactured.

## Enable explicitly

Supported initial environment: Linux x86_64, one working CUDA GPU, NVIDIA driver
with `libcuda.so.1` and `libnvcuvid.so.1`, Wine with a **matching `wineserver` on
PATH**, and the pinned relays below. Tested development environment: Ubuntu
26.04.1, GNOME Wayland, Wine 10.0, RTX 2080 Ti, NVIDIA 595.91.07.

Build an external bundle in a new directory. Requires git, patch, curl, tar,
sha256sum, GCC and Wine development tools/headers. `WINEGCC` can select a private
Wine development runtime; that does not change the runtime used to launch UU.
The recorded build used Wine 11's winegcc and the system compiler, with execution
under Wine 10. Do not overwrite libraries in a running prefix.

```sh
./contrib/nvidia-wine/build-bundle.sh "$HOME/.local/share/uur/nvdec-experiment"
```

Add an **absolute path** to `${XDG_CONFIG_HOME:-$HOME/.config}/uur/config.toml`:

```toml
nvidia_wine_bridge = "/absolute/path/to/nvdec-experiment"
```

Then start `uur run`. The option is absent by default; no libraries are fetched
or installed on normal launches. Missing CUDA/NVDEC, more than one CUDA GPU, a
missing wineserver, or an invalid/missing bundle leaves the default path active
and prints the reason. The host probe does not promise that every GPU supports
every codec. Only the official detector and a real connection establish that.

Remove the configuration line and restart to disable. Keep the bundle and its
source in place until the session has stopped. Session shutdown stops the
managed Wine processes before restoring DLLs. It restores prior DLL files or
symlinks and the original capability cache, or removes newly generated cache
when none existed. Per-process DLL overrides are not written to the registry.
An interrupted session is recovered on the next `uur run`, including when the
option has since been removed. External DLL edits cause recovery to stop instead
of overwriting them. The journal lives in `wine/uur-nvdec-session`.

## Relay scope and provenance

The CUDA relay is based on
[SveSop/nvcuda at 8982084](https://github.com/SveSop/nvcuda/tree/8982084679efc390d217a7f67553f1e319eefe10).
`contrib/nvidia-wine/nvcuda-output-textures.patch` and `uur_nvdec_bridge.c` add a
small output-texture path to that source. They retain LGPL-2.1-or-later; the full
upstream license and notices are included separately from uur's MIT code.

The unchanged CUVID relay comes from
[SveSop/nvidia-libs v1.0.2](https://github.com/SveSop/nvidia-libs/releases/tag/v1.0.2).
Its corresponding source is
[SveSop/nvenc at 09ec9e1](https://github.com/SveSop/nvenc/tree/09ec9e1c8b25a351e415a6d0361bfacad0fd710c).
The script verifies the archive and extracted relay SHA-256 and retains the
corresponding source, license files and archive. No UU installer, account data,
client binaries, or compiled relay binaries are committed here.

The bridge supports **decoder output** in 2D, one mip, one array layer, no MSAA:
R8, RG8, R16, RG16, RGBA8 and BGRA8. R8/RG8/R16/RG16 have changing-frame pixel
coverage. Other layouts, HDR presentation, read-only texture mapping,
arbitrary bidirectional CUDA/D3D interop and multiple GPUs are outside this
experiment. Planar NV12/P010 D3D resources are rejected; the tested UU decoder
uses separate plane textures. Unsupported/mixed resource calls return errors;
non-bridge resources retain the relay's native forwarding.

A real CUDA array receives decoded output. On unmap, the producer stream is
synchronized, `cuMemcpy2D_v2` copies to a writable D3D11 staging texture using
its actual `RowPitch`, then `CopyResource` transfers to the display texture.
This still copies GPU → host → GPU. It does **not** claim zero-copy, zero CPU
cost, or a measured reduction of full input-to-display latency.

## Reproduce validation

Keep all testing in a separate managed prefix. Never copy a live production
prefix or share the same bridge port/runtime files between test supervisors.

```sh
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
x86_64-w64-mingw32-gcc -O2 contrib/nvidia-wine/pixel-test.c \
    -o /tmp/uur-nvdec-pixels.exe -ld3d11 -ldxgi -luuid
WINEPREFIX=/absolute/path/to/test/wine wine /tmp/uur-nvdec-pixels.exe
```

Run the pixel test while the opt-in session has installed its relay links. It
checks 120 changing frames, four formats, a nonblocking CUDA stream, unaligned
row pitches and rejection of duplicate/mixed resources and unsupported flags.

In the same prefix, the official client has a batch detector:

```sh
WINEPREFIX=/absolute/path/to/test/wine wine \
    '/path/to/GameViewer/bin/StreamerCodecDetector.exe' --batch 33 DEVICE_ID ADAPTER_LUID
```

Obtain the actual adapter/device identifiers for that Wine runtime; these are
not portable constants. This invocation must really decode its test frames,
not be replaced by manually edited capability JSON. Compare enabled and disabled
results. To inspect the real connection's decoder texture dimensions, use
`WINEDEBUG=warn+nvcuda uur run` temporarily. Registration diagnostics contain
only texture dimensions and DXGI format; turn tracing off for performance runs.

## Validation status

See [recorded results](nvidia-wine-validation.md). The final source executable
passed cold-start, official detector and pixel checks, established a real H.265
hardware-decode connection with user-confirmed clarity, and restored the default
path after normal stop and a disabled restart in the same prefix. The user also
reported improved latency during subsequent high-frame-rate use; matched
before/after values have not been recorded. Exact visible crop, quantitative
moving-content high-FPS latency comparisons, real interrupted-session recovery, a
physical non-NVIDIA environment and a broader Wine/GPU matrix remain outstanding
before a release or default-on decision. A request for 144 fps and a static
one-fps desktop are not evidence of sustained 144-fps decoding.
