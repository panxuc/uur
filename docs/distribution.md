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
sudo pacman -U ./uur-0.1.1-1-x86_64.pkg.tar.zst
```

## Debian, Ubuntu, and derivatives

```bash
sudo apt install ./uur_0.1.1_amd64.deb
```

The same release artifact covers Debian, Ubuntu, Linux Mint, Pop!_OS, KDE neon,
Zorin OS, elementary OS, Kali Linux, MX Linux, TUXEDO OS, and compatible
derivatives.

## Fedora, RHEL, and openSUSE families

```bash
sudo dnf install ./uur-0.1.1-1.x86_64.rpm

# openSUSE
sudo zypper install ./uur-0.1.1-1.x86_64.rpm
```

This format also covers Fedora derivatives and RPM systems whose repositories
provide the declared Wine, GLib, PipeWire, and portal dependencies.

## Portable archive

The portable archive targets conventional glibc/FHS systems:

```bash
sudo tar --zstd -C / -xf ./uur-0.1.1-linux-x86_64.tar.zst
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
