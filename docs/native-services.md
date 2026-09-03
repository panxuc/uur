# Native service adapters

UU Remote exposes features beyond screen control. On Linux, those features
must operate on the host user account and network namespace, not on Wine's
emulated Windows environment.

## Design boundary

```text
UU feature protocol under Wine
        │
        ▼
small Win64 ABI proxy
        │  authenticated loopback, bounded frames
        ▼
Rust native service owned by `uur run`
        │
        ├─ PTY and login shell
        ├─ XDG user directories and file portals
        └─ host TCP/UDP sockets
```

Proxies translate only the Windows process or stdio boundary expected by UU.
They do not implement Linux policy. Native services are children of the uur
supervisor and cannot outlive the managed session.

## Terminal

Status: implemented.

UU's server uses `conpty_bridge.exe`, `uuyc-mux.exe`, and WTS session queries.
At session startup, uur installs narrow Win64 adapters at those documented
process and API boundaries. The ConPTY adapter forwards stdio and resize
frames; the mux adapter preserves UU's named-session lifecycle; the WTS shim
supplies an active unlocked Wine session. UU's private visible-attach IPC is
delegated to the retained original helper while shell I/O remains native.

The Rust service:

- binds an ephemeral IPv4 loopback port;
- generates a new 256-bit token for every `uur run`;
- accepts at most four concurrent terminal sessions;
- validates a fixed-size handshake and caps frames at 64 KiB;
- opens a real PTY and starts `$SHELL -l` in `$HOME`;
- forwards terminal resize events with `TIOCSWINSZ`;
- sends a hangup and reaps the shell when the UU terminal closes;
- never logs commands, input, output, or environment contents.

The mode-0600 runtime configuration is removed during session shutdown. The
listener, proxy configuration, PTYs, and shells share the main uur lifecycle.

Implementation files:

- `src/terminal.rs`
- `hook/terminal-proxy.c`
- `hook/terminal-protocol.h`
- `tests/terminal-smoke.sh`

The smoke test starts the Rust service, runs the Win64 proxy under Wine, sends a
command through it, and requires a marker emitted by the native Linux shell.

## Quick Launch

Status: implemented.

UU discovers launchable Windows applications through the standard Uninstall
registry. Before starting the server, uur exports the current XDG desktop
application catalog into a private registry namespace. Each item targets a
small Win64 launcher stored in the managed prefix.

The launcher cannot execute an arbitrary command. It reads its immutable
desktop ID, authenticates to a per-run loopback endpoint, and asks the Rust
service to launch that ID only if it is still present in the freshly scanned
catalog. The host uses `gio launch` with `gtk-launch` as a fallback, so normal
desktop activation also covers applications installed through integration
layers that publish `.desktop` entries.

The runtime token is mode 0600 and removed with the main session. A bounded
catalog prevents an untrusted application directory from causing unbounded
registry or prefix growth.

## Desktop integration services

The same lifecycle owns wallpaper synchronization and suspend/idle inhibition.
Proxy settings are imported before Wine starts. Login autostart is explicitly
controlled with `uur autostart`; no permanent root daemon is used.

## File transfer

Status: UU's existing Wine-visible filesystem remains available; native path
integration is not implemented yet.

This means transfers can use paths under the managed prefix or Wine's `Z:`
mapping. A future native adapter should preserve UU's existing transfer
protocol while changing only filesystem access:

### Receiving

1. Receive into a mode-0700 staging directory under the user's XDG data or
   runtime directory.
2. Reject absolute paths, parent traversal, device names, symlinks, sparse-file
   amplification, and declared-size mismatches.
3. Keep partial files hidden and enforce per-file and per-session quotas.
4. After the final size and digest are known, atomically publish into the XDG
   Downloads directory with collision-safe names.
5. Optionally notify the desktop through the standard notification portal.

### Sending

1. Obtain files from an explicit CLI argument, drag-and-drop request, or the
   FileChooser portal.
2. Open and retain file descriptors before acknowledging the request, avoiding
   path replacement races.
3. Expose only selected files through a private staging view; never expose the
   entire home directory by default.
4. Stream from descriptors with bounded memory and cancellation support.

The native adapter should not attempt to translate every Windows path into a
POSIX path. Explicitly selected files and well-defined staging directories are
safer and work in sandboxed environments.

## Port mapping

Status: design complete; protocol attachment to UU is not implemented yet.

The data plane should use host sockets directly so mapped services refer to the
Linux network namespace. The native broker will:

- support TCP first, then UDP after session and timeout semantics are proven;
- bind loopback by default;
- require explicit configuration before binding a LAN or wildcard address;
- reject privileged ports unless a separately configured capability permits
  them;
- enforce destination allowlists, connection limits, idle timeouts, and byte
  quotas;
- resolve names through the host resolver without exposing arbitrary proxy
  environment variables to Wine;
- close every listener and connection when `uur run` ends.

The remaining reverse-engineering task is the stable control-plane boundary
between UU's `PortMappingModule` and its tunnel implementation. The adapter
attaches at an API or helper-process boundary.

## Clipboard and phone text

Clipboard access belongs to the active desktop session. The preferred Wayland
route is the Clipboard interface attached to the existing RemoteDesktop portal
session. X11 uses normal selection ownership. Text payloads require strict size
limits and must not be written to logs.

## Security invariants

- Every service authenticates its Wine-side proxy with a per-run random token.
- Runtime endpoints bind only to loopback or use private Unix descriptors.
- Payload lengths and concurrent sessions are bounded before allocation.
- Native services run as the logged-in user; no root daemon handles content.
- User content is absent from diagnostics.
- Session shutdown revokes tokens, removes runtime files, closes listeners, and
  reaps child processes.
