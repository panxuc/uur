# Video and display backends

## Frame sources

- X11 sessions expose the XRandR monitor topology directly to Wine.
- Wayland monitor and window sources use ScreenCast/PipeWire.
- `uur display source virtual` requests the portal's Virtual source type and
  stores its restore token.
- A multi-stream compositor will combine or expose several PipeWire nodes to
  UU's display topology for independent Wayland screen switching.

## Codec capabilities

`uur doctor` inventories the native acceleration APIs independently:

- DRM render nodes are the common device boundary;
- VA-API covers Intel, AMD, and supported third-party drivers;
- Vulkan Video is recognized through `VK_KHR_video_queue`;
- NVENC and NVDEC are recognized through NVIDIA's encode/decode libraries;
- software H.264 remains available through UU's current Windows path.

The native codec data path will consume the same BGRx frame transport used by
the current capture adapter. A backend receives frame descriptors, negotiated
color metadata, dimensions, and target latency; it returns encoded access units
through one common interface. Codec selection can then prefer a verified
hardware route and fall back to software without changing capture, display, or
UU protocol code.

HDR requires a 10-bit frame format, color primaries, transfer function, and
mastering metadata across the portal, PipeWire, encoder, and UU transport. The
current BGRx transport is SDR.
