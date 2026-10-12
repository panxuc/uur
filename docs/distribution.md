# Package installation

Choose the package format native to the system. Every format installs the same
Rust supervisor, Wine adapters, PipeWire helper, desktop entry, icon, and
uinput integration files. `sudo`, `doas`, or another privilege frontend may be
needed for system-wide package installation.

## Arch Linux

```bash
# AUR
paru -S uur

# Downloaded release package
sudo pacman -U ./uur-0.1.4-1-x86_64.pkg.tar.zst
```

## Debian, Ubuntu, and derivatives

```bash
sudo apt install ./uur_0.1.4_amd64.deb
```

The same release artifact covers Debian, Ubuntu, Linux Mint, Pop!_OS, KDE neon,
Zorin OS, elementary OS, Kali Linux, MX Linux, TUXEDO OS, and compatible
derivatives.

## Fedora, RHEL, and openSUSE families

```bash
sudo dnf install ./uur-0.1.4-1.x86_64.rpm

# openSUSE
sudo zypper install ./uur-0.1.4-1.x86_64.rpm
```

This format also covers Fedora derivatives and RPM systems whose repositories
provide the declared Wine, GLib, PipeWire, and portal dependencies.

## Portable archive

The portable archive targets conventional glibc/FHS systems:

```bash
sudo tar --zstd -C / -xf ./uur-0.1.4-linux-x86_64.tar.zst
```

## Nix and NixOS

```bash
nix build
nix run . -- doctor
```

The flake includes a package, application, development shell, lock file, and
NixOS module. See [Nix and NixOS](nix.md).

## Alpine Linux and musl

`packaging/alpine/APKBUILD` builds an x86_64 musl package from a tagged source
archive. The container recipe runs the same Rust, MinGW, and PipeWire builds:

```bash
docker build -f packaging/alpine/Dockerfile .
```

Alpine edge provides native x86_64 Wine, PulseAudio/ALSA Wine drivers,
PipeWire, desktop portals, Rust, and the MinGW-w64 toolchain.

```bash
cd packaging/alpine
abuild checksum
abuild -r
sudo apk add ~/packages/*/x86_64/uur-*.apk
```

## Other package ecosystems

Gentoo, Funtoo, Void Linux, Slackware, Solus, Clear Linux, ALT Linux, Exherbo,
Mageia, OpenMandriva, PCLinuxOS, Venom Linux, and other FHS/glibc systems can
install the portable release archive or build from source. Their native recipe
can call `packaging/stage.sh` after the three standard builds; the staged tree
is shared with pacman, deb, rpm, APK, and the portable archive.

GNU Guix can wrap the same source build and staged tree. Flatpak and Snap need
manifests granting Wine execution, session D-Bus, PipeWire, and input access.

## Source installation for one user

```bash
cargo build --release --locked
./hook/build.sh
./capture/build.sh
./scripts/install-user.sh
```

This installs under `~/.local` by default. It requires Rust, a native C
compiler, PipeWire development files, `pkg-config`, and the MinGW-w64 x86_64
toolchain.

## Required first run

Package installation does not implicitly accept NetEase's EULA or download the
official client. Complete the setup explicitly:

```bash
uur doctor
uur setup --accept-eula
uur run
```

See [Platform support and usage](platform-support.md) for desktop-session and
portal requirements.

## Package removal, repair, and per-user state

Package removal leaves the managed Wine prefix, downloaded installer cache,
configuration and logs in the user's XDG directories. This preserves login
and client data across package upgrades and reinstalls. Reinstalling the
package therefore does not create a new Wine prefix. Start with
`uur stop` followed by `uur setup --repair` for an interrupted setup.

For a fresh prefix, stop uur and move the old prefix aside before setup:

```bash
uur stop
uur_prefix="${XDG_DATA_HOME:-$HOME/.local/share}/uur/wine"
mv -- "$uur_prefix" "$uur_prefix.backup-$(date +%Y%m%d-%H%M%S)"
uur setup --accept-eula
```

The fresh prefix needs the official client installed and logged in again.
The backup retains the previous client and account data.

To clear the managed client, configuration and logs, first disable autostart and stop the
session, then archive the three uur directories. Remove the distribution
package separately with its package manager:

```bash
uur autostart disable
uur stop
uur_backup="$HOME/uur-backup-$(date +%Y%m%d-%H%M%S)"
mkdir -m 0700 -- "$uur_backup"
uur_data="${XDG_DATA_HOME:-$HOME/.local/share}/uur"
uur_config="${XDG_CONFIG_HOME:-$HOME/.config}/uur"
uur_state="${XDG_STATE_HOME:-$HOME/.local/state}/uur"
if test -d "$uur_data"; then mv -- "$uur_data" "$uur_backup/data"; fi
if test -d "$uur_config"; then mv -- "$uur_config" "$uur_backup/config"; fi
if test -d "$uur_state"; then mv -- "$uur_state" "$uur_backup/state"; fi
```

Delete that backup when its client data and logs are no longer needed.
Unpin any retained Wine-generated launcher after package removal; its command
is an alias for `uur run`.
