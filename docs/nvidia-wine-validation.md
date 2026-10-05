# NVIDIA/Wine bridge validation record

Recorded on 2026-10-06 against uur master
`d349e6ff6a608e9942b0c84b5752bba46a8b8bf9` and the accompanying experiment.
This is a development record, not a release or performance claim.

## Environment

- Ubuntu 26.04.1, GNOME 50.1 on Wayland, NVIDIA RTX 2080 Ti, driver 595.91.07.
- System Wine 10.0. Wine 11 development tools built the ELF CUDA relay;
  execution used Wine 10.0. A fresh Wine 11 prefix was also used for diagnosis.
- Pinned source and relay revisions/checksums are in `build-bundle.sh`.
- An isolated test prefix, private XDG directories and a separate bridge port
  were used. The installed production prefix was not modified.

## Results

| Check | Result | What it establishes |
| --- | --- | --- |
| `cargo test --locked --all-targets` | 44 passed | Includes seven bridge lifecycle tests: disabled/unsupported-host paths, original files and dangling links, cache recovery, invalid bundle, external edits and interrupted restoration. |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed | Rust checks for the accompanying source. |
| `cargo build --release --locked` | Passed | The integrated executable builds. |
| Pinned bundle build | Passed | The patch applies with zero fuzz and the archive and CUVID relay checksums match. |
| Current helper, changing-frame pixel test after a fresh integrated launch with the display restored | Passed | 120 frames each in R8, RG8, R16 and RG16; odd widths, actual row pitches, a nonblocking CUDA stream and resource/flag rejection checks. RGBA/BGRA are not pixel-validated. |
| Current helper, official `StreamerCodecDetector.exe --batch 33` after that launch | Passed for reported supported profiles | H.264 8-bit 4:2:0 and H.265 8/10-bit 4:2:0/4:4:4 reported a maximum of 3840 × 2160. Unsupported profiles were left unsupported. These are detector limits, not measured live-stream dimensions. |
| Original baseline, official batch detector with the option absent | Completed; all eight profiles unsupported | In the isolated unmodified baseline, with no nvcuda/nvcuvid relay files, every result was `0,0,0`; with the bridge enabled the supported profiles above were `3840,2160,1`. |
| Experimental real-session shutdown and disabled restart in the same prefix | Passed | Normal `uur stop` removed the journal and installed relay links. With the configuration option removed, the final executable restarted, D3D11 initialized at 3440 × 1440 and the official detector completed with all eight hardware profiles unsupported, matching the original baseline. No opt-in journal was recreated. The option was then restored for the user's isolated test launcher. |
| Earlier minimal bridge prototype, real official-client connection | H.265 hardware decoding shown in session statistics | This used the earlier local helper, not the final contributed helper and lifecycle integration. The desktop was mostly static (about 1 fps); it does not validate sustained 144 fps or a latency improvement. |
| Final integration, cold start and real connection | Passed for NVDEC selection and user-reported clarity | D3D11 initialized successfully in both isolated baseline and experimental prefixes after restoring the monitor and stopping stale Wine processes. The final source executable connected to the remote Windows computer. The viewer loaded the contributed CUDA relay, pinned CUVID relay and native Linux CUDA/CUVID libraries; its live output textures were R8 3456 × 1440 and RG8 1728 × 720. The user's control-center screenshot showed H.265, hardware encode/hardware decode, DXGI capture, 3 ms network delay, 10 ms frame delay and 0% packet loss. The user confirmed that the image was clear. The screenshot was captured at only 1 fps and is not a high-FPS latency benchmark. |
| Physical non-NVIDIA machine | Not tested | Unsupported-host fallback has a unit test; a physical-machine check remains outstanding. |

## Independent display initialization failure

After stopping Wine, cold-start D3D11 initialization failed with HRESULT
`0x887a0001`. This also occurred with the bridge disabled in the original test
prefix and in freshly created Wine 10 and Wine 11 prefixes. Fresh prefixes
reported a fallback 1024 × 768 desktop. Wine logged failure to read the display
configuration and to obtain the window's DXGI output.

At the same time, GNOME's `org.gnome.Mutter.DisplayConfig.GetCurrentState`
returned empty physical and logical monitor arrays. Xwayland reported no
monitors or outputs. The available evidence does not establish why the desktop
entered this state, or that the bridge caused it. A Wine virtual desktop did not
restore D3D11 initialization. Temporary virtual-desktop registry values were
removed and the isolated test session was stopped.

On resuming validation, DP-3 was connected again and GNOME/Xwayland reported
3440 × 1440 at approximately 144 Hz. D3D11 initialization then succeeded in
the experimental prefix and, after stopping a stale Wine server, the original
baseline prefix. The final integrated source executable established a real
Windows connection and passed the pixel/detector checks again. This resolves
the display-condition blocker. The real session selected hardware decoding
and the user confirmed clarity; this does not establish an end-to-end latency
improvement.

## Remaining acceptance checks

1. Confirm the exact visible/cropped dimensions against the requested
   3440 × 1440 mode. The registered luma texture is 3456 × 1440, consistent
   with possible width alignment, but texture allocation alone does not
   measure the visible crop. Detector limits alone are also insufficient.
2. Exercise moving content at the requested high frame rate. Compare enabled
   and disabled behavior under equivalent host, network and display conditions.
   Record frame delivery and latency without claiming a zero-copy path.
3. Exercise interrupted-supervisor recovery during a real connection. Unit
   coverage and normal real-session stop/disabled restart are already recorded.
4. Test the fallback on a physical non-NVIDIA system before a release decision.

Private account/device identifiers, connection screenshots, tokens and official
client binaries are deliberately excluded from this record and contribution.
