# Linux packaging (tarball + system service + auto-updater)

How the Linux release is built and installed. Builds are **local** (the maintainer runs this in
WSL Ubuntu or on a Linux box).

## What ships
`ipn-<version>-linux-x86_64.tar.gz` — the binaries (`ipn`, `ipn-daemon`, `ipn-cli`), the
`ipnctl` install manager, the systemd **system** units, an app-menu `.desktop`, and pre-rendered
hicolor icons. It relies on **system GTK** on the target (not bundled):
`sudo apt install libgtk-4-1 libadwaita-1-0`.

Unlike a pure user app, IPN's daemon needs `CAP_NET_ADMIN`/`CAP_NET_RAW` to create the TUN, so
`ipnctl --install` sets it up as a **root systemd service** (it gets the caps for free), with a
root daily update timer — mirroring the Windows LocalSystem service + SYSTEM task. The GUI runs
as your normal user and talks to the daemon over `/tmp/ipn.sock`.

## Prerequisites
- Build: `cargo`, `tar`, **ImageMagick** (`magick` or `convert`) for icon sizes, and the GTK dev
  packages: `sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config build-essential`.
- Target runtime: `libgtk-4-1 libadwaita-1-0` (GTK 4.10+ / libadwaita 1.4+).

## Build
```sh
scripts/package-linux.sh                 # cargo build --release, then package
scripts/package-linux.sh --skip-build    # repackage existing target/release bins
# -> dist/ipn-<version>-linux-x86_64.tar.gz
```

## Install / manage (on the target)
One-liner (downloads the latest release):
```sh
curl -fsSL https://raw.githubusercontent.com/steeb-k/iroh-private-network/main/install.sh | sh
```
Or from the unpacked tarball: `./ipnctl --install`. Either way `ipnctl` uses `sudo` for the
privileged steps and:
- installs `ipn`/`ipn-daemon`/`ipn-cli`/`ipnctl` to `/usr/local/bin`,
- installs `/etc/systemd/system/ipn-daemon.service` (root; `CAP_NET_ADMIN`) and enables+starts it,
- installs `ipn-update.service` + `ipn-update.timer` (daily auto-update) and enables the timer,
- installs the app-menu entry + hicolor icons, and a tray autostart in `/etc/xdg/autostart`.

Manage: `ipnctl --status`, `ipnctl --update [--check]`, `ipnctl --uninstall [--purge]`.

## Auto-update
`ipn-update.timer` (system, daily, randomized) runs `ipnctl --update` as root: it compares
`ipn-daemon --version` to the latest tag of the public `steeb-k/iroh-private-network` repo,
downloads the new tarball, atomically swaps the binaries, reloads systemd, and restarts the
daemon.

## Gotchas
- The GUI must **not** run as root (it loses your display); privilege lives in the daemon.
- `.gitattributes` keeps the shell scripts/units LF so they survive a Windows checkout.
- Stale socket after an unclean stop: `sudo systemctl restart ipn-daemon`.
