#!/usr/bin/env python3
"""Run paired relays in one disposable Wine prefix; never use a live prefix."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--wine", required=True)
parser.add_argument("--wineserver", required=True)
parser.add_argument("--prefix", type=Path, required=True)
parser.add_argument("--fixture", type=Path, required=True)
parser.add_argument("--reference", type=Path, required=True)
parser.add_argument("--final", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
args.prefix = args.prefix.resolve()
if not (args.prefix / "uur-benchmark-prefix").is_file():
    raise SystemExit("Prefix must be explicitly marked disposable with uur-benchmark-prefix")
if args.output.exists() and any(args.output.iterdir()):
    raise SystemExit("Use a new, empty output directory; refusing to overwrite prior measurements")
args.output.mkdir(parents=True, exist_ok=True)
env = os.environ.copy()
for key in ["WINEDLLPATH", "UU_INTEROP_PROFILE", "UU_REMOTE_D3D11_UPLOAD"]:
    env.pop(key, None)
env.update(WINEPREFIX=str(args.prefix), WINEARCH="win64", WINEDEBUG="-all",
           WINEDLLOVERRIDES="nvcuda=n;mscoree,mshtml=;winemenubuilder.exe=d")
libraries = {"reference": args.reference.resolve(), "final": args.final.resolve()}
if not (args.prefix / "drive_c/windows/system32").is_dir():
    raise SystemExit("Initialize a separate disposable prefix before running this script")
link = args.prefix / "drive_c/windows/system32/nvcuda.dll"
if link.exists() or link.is_symlink():
    if not link.is_symlink() or link.resolve() not in libraries.values():
        raise SystemExit("Refusing to replace an unrelated nvcuda.dll")


def stop_prefix():
    result = subprocess.run([args.wineserver, "-k"], env=env, timeout=15)
    if result.returncode not in (0, 1):
        raise RuntimeError("wineserver -k failed")
    subprocess.run([args.wineserver, "-w"], env=env, timeout=15, check=True)


def telemetry():
    query = "clocks.current.graphics,clocks.current.memory,temperature.gpu,power.draw,utilization.gpu"
    result = subprocess.run(["nvidia-smi", "--query-gpu=" + query,
                             "--format=csv,noheader,nounits"],
                            capture_output=True, text=True, timeout=10)
    return result.stdout.strip() if result.returncode == 0 else "unavailable"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


metadata = {
    "wine": subprocess.check_output([args.wine, "--version"], env=env, stderr=subprocess.STDOUT, text=True).strip(),
    "wineserver": subprocess.check_output([args.wineserver, "--version"], env=env, stderr=subprocess.STDOUT, text=True).strip(),
    "graphics_backend": "Wine builtin D3D11/DXGI (no DXVK overrides)",
    "reference_sha256": digest(libraries["reference"]),
    "final_sha256": digest(libraries["final"]),
    "fixture_sha256": digest(args.fixture),
    "gpu_telemetry_columns": ["graphics_clock_MHz", "memory_clock_MHz", "temperature_C", "power_W", "gpu_util_percent"],
    "run_order": ["reference", "final", "final", "reference", "reference", "final"],
    "reference_scope": "Final source with only the upload strategy changed to a host buffer + UpdateSubresource; not upstream uur",
    "timing_scope": "Unmap/bridge and producer D3D11 event fence; excludes synthetic producer writes and consumer readback",
    "limits": ["No remote/network/input-to-display measurement", "No GPU clock lock or desktop-load isolation",
               "Fixture cadence includes untimed pixel verification; it is not official-client FPS"],
}
results = []
try:
    for layout, fps in [("R8", 144), ("NV12", 60), ("NV12", 144)]:
        for mode in metadata["run_order"]:
            stop_prefix()
            if link.is_symlink():
                link.unlink()
            link.symlink_to(libraries[mode])
            number = len(results) + 1
            path = args.output / f"run-{number:02d}-{layout}-{fps}-{mode}.log"
            before = telemetry()
            with path.open("w") as log:
                process = subprocess.Popen([args.wine, str(args.fixture.resolve()), layout, str(fps)],
                                           env=env, stdout=log, stderr=subprocess.STDOUT)
                confirmed = False
                try:
                    for _ in range(100):
                        if process.poll() is not None:
                            break
                        try:
                            maps = Path(f"/proc/{process.pid}/maps").read_text()
                            if str(libraries[mode]) in maps:
                                confirmed = True
                                break
                        except OSError:
                            pass
                        time.sleep(0.03)
                    if process.wait(timeout=25) != 0:
                        raise RuntimeError(f"Benchmark failed; inspect {path.name}")
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.wait()
            lines = [line for line in path.read_text().splitlines() if line.startswith('{"pass":')]
            if len(lines) != 1 or not confirmed:
                raise RuntimeError("Missing unique benchmark result or expected relay mapping")
            result = json.loads(lines[0])
            if result["pass"] is not True:
                raise RuntimeError("Pixel verification failed")
            result.update(run=number, mode=mode, expected_relay_mapping_confirmed=confirmed,
                          gpu_before=before, gpu_after=telemetry())
            results.append(result)
            (args.output / "results.json").write_text(json.dumps({"metadata": metadata, "runs": results}, indent=2) + "\n")
            print(f"{number:02d} {layout} target={fps} {mode}: bridge={result['bridge_mean_ms']:.4f}ms "
                  f"ready={result['ready_mean_ms']:.4f}ms fixture_fps={result['fixture_fps']:.2f} PASS", flush=True)
finally:
    stop_prefix()
    if link.is_symlink() and link.resolve() in libraries.values():
        link.unlink()
