# Capture frame protocol

How frames travel from the compositor (via PipeWire) into the client's
capture calls.  Two producers/consumers, one shared file, no locks.

```text
xdg-desktop-portal ScreenCast
        │ (portal-consented session; per-monitor)
        ▼
uur-pw-capture (C) ── PipeWire stream, BGRx, SHM buffers
        │ writes slots, seqlock per slot
        ▼
$XDG_RUNTIME_DIR/uur/frames.v1  (mode 0600, 64-byte header + 3 slots)
        │ mapped read-only by the PE hook through its configured Wine Z: path
        ▼
uur-hook.dll ── intercepts static/dynamic gdi32!BitBlt/StretchBlt calls;
                scales the newest stable snapshot into the destination DIB
```

## Shared file layout

All integers little-endian.  Total size = 64 + 3 × (8 + stride × height).

| offset | type | field |
| --- | --- | --- |
| 0 | u32 | magic `0x46525555` ("UURF") |
| 4 | u32 | protocol version = 1 |
| 8 | u32 | frame width (px) |
| 12 | u32 | frame height (px) |
| 16 | u32 | stride (bytes per row) |
| 20 | u32 | format: 1 = BGRx (32 bits per pixel) |
| 24 | u64 | total frames written (monotonic counter) |
| 32..64 | — | reserved, zero |

Slot *i* (i = 0, 1, 2) at offset `64 + i × (8 + stride × height)`:

| offset | type | field |
| --- | --- | --- |
| 0 | u64 | sequence: `2×n − 1` while slot n is being written, `2×n` when slot n is stable |
| 8 | bytes | frame, `stride × height`, rows top-to-bottom, pixels BGRx |

Writer picks slots round-robin (`n mod 3`), bumps the counter, publishes
the odd sequence (release), copies the frame, then publishes the even
sequence (release).  The reader is a classic seqlock read: load the
sequence, copy, re-load; retry while the value changed or is odd.  No
mutexes; a torn read is detected, never observed.

The transport path is created per user and passed to the Wine adapter through
the managed prefix configuration. It is never a global shared-memory name.
