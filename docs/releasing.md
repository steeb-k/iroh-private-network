# Packaging & releasing

> **⚠️ Interim process — pin for later.** Today we ship a **portable zip (Windows)** and a
> **tarball (Linux)** built **locally** (Windows natively; Linux/Android via WSL on the
> maintainer's machine). There is intentionally **no CI**. Proper installers are a TODO — see
> [Planned installers](#planned-installers-todo) at the bottom. When those land, revise this doc.

## Versioning
- The workspace version lives once in the root `Cargo.toml` under `[workspace.package]`; all
  crates inherit it with `version.workspace = true`.
- Pre-1.0 we tag prereleases as `v<version>-test<N>` (e.g. `v0.0.1-test5`) and mark them
  `--prerelease` on GitHub. Bump `<N>` per test build; bump the workspace version for a real
  release.

## Prerequisites
See [building.md](building.md) (Rust + GTK toolchains; Windows uses gvsbuild at `C:\gtk`).

## Build the artifacts
**Windows** (native shell):
```powershell
pwsh -File scripts\bundle-gtk-windows.ps1
# -> dist\ipn-windows-x86_64.zip
```
The script builds `ipn` + `ipn-daemon` + `ipn-cli` in release, then bundles the GTK runtime,
fetches and includes `wintun.dll`, and adds the setup scripts. The zip contains:
`1. Install service (admin).bat`, `2. IPN.bat`, `Uninstall service (admin).bat`, and `bin\`.
It is self-contained — verified to run on a machine with no GTK installed.

**Linux** (run in WSL or on Linux):
```sh
bash scripts/package-linux.sh
# -> dist/ipn-<version>-linux-x86_64.tar.gz
```
Relies on **system GTK** on the target (`sudo apt install libgtk-4-1 libadwaita-1-0`). The
tarball includes `enable-routing.sh` (one-time `setcap` on the daemon) and the `ipn` launcher.

Tip: `scripts/package-linux.sh --skip-build` re-packages without recompiling.

## Cut a release
1. Make sure `cargo test -p ipn-core` and the relevant ignored e2e tests pass.
2. Build **both** artifacts (above).
3. Move `CHANGELOG.md`'s `## [Unreleased]` items under a new version/tag heading.
4. Publish with the GitHub CLI (authenticated as the maintainer):
   ```sh
   gh release create v<version>-test<N> --repo steeb-k/iroh-private-network --prerelease \
     --title "v<version>-test<N> — <summary>" --notes "<what changed + run instructions>" \
     dist/ipn-windows-x86_64.zip dist/ipn-<version>-linux-x86_64.tar.gz
   ```
   To replace an asset on an existing release: `gh release upload <tag> <file> --clobber`.

## Smoke-check before announcing
- Windows: unzip on a clean machine, run the install `.bat` (UAC), then `IPN.bat`; confirm the
  member list and "routing on".
- Linux: extract, `./enable-routing.sh`, `./ipn`; confirm the same.
- Two machines: create on one, join on the other, compare the emoji code, approve, and connect
  RDP/SSH to the peer's `10.99.0.x`.

## Planned installers (TODO)
The portable zip/tarball is interim. Future, per platform:
- **Windows:** signed **MSI** (WiX) that installs to Program Files, registers the service, and
  bundles the GTK runtime + `wintun.dll`; code-signing (e.g. Azure Trusted Signing). Add a
  Start-menu entry and an updater.
- **Linux:** a native package (`.deb`/`.rpm`) and/or **AppImage**/**Flatpak**; a systemd unit
  for the daemon (instead of the `setcap` + wrapper approach).
- **macOS:** a notarized `.app` bundle (or a `curl | sh` bootstrap like seed-sync), with the
  daemon as a launchd service or a Network Extension.
- **Android:** a signed **APK** (Kotlin/Compose + UniFFI + `VpnService`), sideload / F-Droid.

When implementing any of these, update this doc and `docs/building.md`, and keep the portable
build working as a fallback.
