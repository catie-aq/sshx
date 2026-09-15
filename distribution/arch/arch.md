# Arch Linux Package — sshx

Installs the `sshx` binary from a local pre-built distribution server using `makepkg`.

## Prerequisites

- Arch Linux (x86_64)
- `base-devel` package group (`sudo pacman -S base-devel`)
- Network access to `https://vps.burro-piranha.ts.net:5176/`

## Build & Install

```bash
cd distribution/arch
makepkg -si
```

`-s` installs missing dependencies via pacman; `-i` installs the built package when done.

## What it does

1. Downloads `sshx-0.5.0-linux-x86_64.tar.gz` from the distribution server.
2. Extracts the archive and installs the `sshx` binary to `/usr/bin/sshx`.

## Updating the checksum

`sha256sums` is currently set to `SKIP`. To pin it:

```bash
makepkg -g   # prints the computed sha256sum
```

Paste the output hash into `PKGBUILD`:

```
sha256sums=('<paste-hash-here>')
```

## Bumping the version

Edit `PKGBUILD` and update:

```
pkgver=<new-version>
pkgrel=1
```

Then rebuild with `makepkg -si`.

## Uninstall

```bash
sudo pacman -R sshx
```
