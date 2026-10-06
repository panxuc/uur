# Final-source upload A/B benchmark

Measured on 2026-10-06: RTX 2080 Ti / NVIDIA 595.91.07, Ryzen 7 5800X,
Ubuntu 26.04.1 GNOME Wayland/Xwayland, system Wine 10.0 and its builtin
D3D11/DXGI backend. Both ELF relays were rebuilt with the same Wine 11.0
winegcc and GCC `-O2` arguments and executed with Wine 10.0.

The final variant uses the exact helper from PR source commit
`e52de18f7a54e6b85bbc160ab9b94ee37d27df38`.
The reference is **that same source with the earlier host-buffer plus
`UpdateSubresource` upload strategy reconstructed**, using
[`upload-reference.patch`](../contrib/nvidia-wine/upload-reference.patch).
It is not unmodified upstream uur: upstream has no working NVDEC output bridge
against which to time this hardware path. Neither result is copied from the
earlier Guo/DXVK experiment.

Both variants use the same real CUDA arrays, stream synchronization, resource
guards and texture formats. The reference adds a persistent host buffer; both
retain the staging-texture allocation so that the rest of resource creation
remains identical. Only the actual copyback/upload path and associated host
buffer lifetime differ. No profiling was added to the release helper.

## Method

The same fresh, disposable Wine prefix and executable were used for all runs,
stopping its Wine server before switching relay links. `/proc` mappings
confirmed the expected relay in every run. There were no DXVK overrides.

Each workload alternated **reference, final, final, reference, reference, final**:
three runs per variant, 60 warmup frames and 360 measured frames per run.
The three workloads produced 18 runs, 6,480 measured frames, plus warmup.
All rows and bytes of every frame, including warmup, passed readback validation.

R8 uses one 3456 × 1440 texture. NV12 here means two separate output textures:
R8 3456 × 1440 and RG8 1728 × 720, matching the final real-session allocations.
It is not a planar DXGI NV12 resource. Row values change every frame.

A synthetic producer writes pinned host data to real CUDA arrays on a
nonblocking stream and completes **before** timing. Bridge time measures
`cuGraphicsUnmapResources`. GPU-ready time measures from that same start until
a D3D11 EVENT query completes on the producer context. It includes upload and
the completion fence, and excludes consumer readback and pixel verification.
This is offscreen texture upload, not a decoded video or remote session.

## Results

Values below are averages of the three run means per variant.

| Workload / target cadence | Reference bridge ms | Final bridge ms | Reduction | Reference GPU-ready ms | Final GPU-ready ms | Reduction |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| R8 / 144 fps | 2.587 | 0.443 | 82.9% | 3.626 | 1.208 | 66.7% |
| NV12 two planes / 60 fps | 4.413 | 1.055 | 76.1% | 5.217 | 1.812 | 65.3% |
| NV12 two planes / 144 fps | 4.475 | 1.034 | 76.9% | 5.263 | 1.758 | 66.6% |

In the 144-fps NV12 workload, reference bridge run means ranged from
4.318–4.569 ms; final run means from 1.018–1.063 ms. GPU-ready run means ranged
from 5.110–5.355 ms and 1.746–1.782 ms respectively. Full per-run means,
medians, p95, maxima, binary/source hashes, telemetry and cadence diagnostics
are in [the machine-readable record](nvidia-wine-benchmark-results.json).

The reference's 144-fps NV12 fixture achieved 137.51, 137.96 and 143.31 fps;
the final achieved approximately 144.22 fps. **These are fixture throughput
values including untimed correctness readback, not official-client FPS.**
The recorded `late_frames` counts starts more than one period behind the
absolute configured schedule; it is not a packet-loss or dropped-video count.
Both variants met the R8/144 and NV12/60 schedules. The NV12/60 comparison is
therefore also evidence at a cadence both implementations sustained.

GPU clocks and normal desktop activity were not locked. Per-run telemetry is
included, but there are no confidence intervals or statistical-significance
claims, and no cross-GPU, cross-Wine, zero-copy or input-to-display latency claim.
The user's separate report of improved high-frame-rate remote latency is
consistent with cheaper copyback; this benchmark does not measure its magnitude.

## Reproduce

Keep the official-client prefix separate. `run-benchmark.py` stops its selected
Wine server and refuses an unmarked prefix or an unrelated existing CUDA DLL.
Use absolute paths for the relay and fixture arguments.

First build the final bundle with `build-bundle.sh`, then copy its patched
`source` tree to a new reference directory and apply `upload-reference.patch`
with zero fuzz. Build **both** relays from their respective source trees with
the same compiler and arguments as `build-bundle.sh`:

```sh
cp -a /absolute/path/to/bundle/source /absolute/path/to/reference-source
patch --batch --fuzz=0 -d /absolute/path/to/reference-source -p1 \
    < contrib/nvidia-wine/upload-reference.patch

# Repeat once for each source tree and distinct output path.
SOURCE=/absolute/path/to/source-tree
OUTPUT=/absolute/path/to/final-or-reference-nvcuda.dll
WINEGCC=/absolute/path/to/winegcc
"$WINEGCC" -O2 -D__WINESRC__ -m64 -shared -o "$OUTPUT" \
    "$SOURCE/dlls/nvcuda/nvcuda.spec" "$SOURCE/dlls/nvcuda/nvcuda.c" \
    "$SOURCE/dlls/nvcuda/internal.c" "$SOURCE/dlls/nvcuda/function_mappings.c" \
    "$SOURCE/dlls/nvcuda/encryption.c" -I"$SOURCE/include" \
    -I"$SOURCE/dlls/nvcuda" -ldl -lpthread -lsetupapi -ldxgi -luuid

x86_64-w64-mingw32-gcc -O2 -Wall -Wextra -Werror \
    contrib/nvidia-wine/benchmark.c -o /absolute/path/to/benchmark.exe \
    -ld3d11 -ldxgi -luuid

WINEPREFIX=/absolute/path/to/new-benchmark-prefix \
    WINEARCH=win64 WINEDEBUG=-all \
    WINEDLLOVERRIDES='mscoree,mshtml=;winemenubuilder.exe=d' wine wineboot -u
touch /absolute/path/to/new-benchmark-prefix/uur-benchmark-prefix

python3 contrib/nvidia-wine/run-benchmark.py \
    --wine /absolute/path/to/wine \
    --wineserver /absolute/path/to/matching/wineserver \
    --prefix /absolute/path/to/new-benchmark-prefix \
    --fixture /absolute/path/to/benchmark.exe \
    --reference /absolute/path/to/reference-nvcuda.dll.so \
    --final /absolute/path/to/final-nvcuda.dll.so \
    --output /absolute/path/to/new-results-directory
```

The runner removes its test relay link on completion. It never rewrites normal
uur configuration or modifies the installed/official-client prefix.
