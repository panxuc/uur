# Upstream adaptation

UU Remote and uur have independent release lifecycles. The AUR package
contains only uur, so a new proprietary client does not automatically require
a package rebuild.

## Compatibility contract

The adapter depends on named operating-system APIs, not a table of UU versions,
hashes, RVAs, or instruction patches:

- `wevtapi!EvtOpenPublisherMetadata` loads the compatibility shim;
- `USER32!SendInput` is routed to the native input bridge;
- GDI or DXGI desktop-capture entry points are routed to the frame adapter;
- dynamic `GetProcAddress` resolution is intercepted as well as static imports.

`scripts/probe-upstream.sh` verifies those contracts across every PE file in
an installed client tree. If an installer cannot be extracted directly, the
probe installs it in a disposable Wine prefix and examines the result.

## Automation

The daily `upstream-watch` workflow:

1. resolves NetEase's official HTTPS release URL;
2. verifies the Authenticode signature;
3. extracts or installs the client in an ephemeral environment;
4. runs the static API-contract probe;
5. opens or updates a tracking issue with the result.

Static success means the generic adapter still has a viable attachment point;
it is not presented as full runtime proof. A release candidate must also pass
the Wine input round-trip and changing-frame canary before it is promoted.

Project releases build all package formats from the same source and publish
immutable artifacts and checksums. A new proprietary client version alone does
not cause a meaningless uur package rebuild.

## When a contract changes

Compatibility changes add a stable API adapter or transport behind the existing
Rust traits. Capability probes select the route whose prerequisite API is
available.
