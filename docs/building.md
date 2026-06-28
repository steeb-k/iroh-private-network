# Building from source

A Rust workspace. Desktop builds need GTK4 + libadwaita available to `pkg-config`.

## Prerequisites
- Rust (stable, 1.85+).
- **Linux:** `sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config build-essential`
- **Windows:** the MSVC toolchain and GTK4 + libadwaita via
  [gvsbuild](https://github.com/wingtk/gvsbuild) (the scripts assume it's at `C:\gtk`).
- **macOS:** GTK4 + libadwaita via Homebrew (build supported; packaging not scripted yet).

## Run in development
The daemon (privileged, owns the TUN) and the GUI (unprivileged) run as separate processes.

```sh
# Terminal 1 — the daemon:
#   Linux (once): sudo setcap cap_net_admin,cap_net_raw+ep target/debug/ipn-daemon
#   Windows:      run from an elevated shell
cargo run -p ipn-daemon

# Terminal 2 — the GUI:
cargo run -p ipn-gui
```

Without a running daemon the GUI shows a "daemon not running" page. If the daemon lacks routing
privilege, membership + presence still work and the GUI shows a "routing off" banner.

The headless client is handy for testing without the GUI:
```sh
cargo run -p ipn-cli -- status
cargo run -p ipn-cli -- create home
```

## Tests
```sh
cargo test -p ipn-core                 # unit tests
# end-to-end tests open real iroh endpoints, so they're #[ignore]d by default:
cargo test -p ipn-core --test engine_e2e   -- --ignored   # create / join / verify / connect
cargo test -p ipn-core --test delete_e2e   -- --ignored   # delete boots everyone, no ghosts
cargo test -p ipn-core --test rotate_e2e   -- --ignored   # rotate locks out old-ticket devices
```

## Packaging & releasing
Building the distributable artifacts and cutting a release has its own guide:
**[releasing.md](releasing.md)** (portable zip/tarball today; installers are a documented TODO).
