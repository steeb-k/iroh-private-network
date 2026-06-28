# TODO / ideas

A catch-all for things to build later. Loosely grouped; not prioritized. Move items into a
release once done and add a `CHANGELOG.md` entry (see `docs/development.md`).

## Hardening
- Self-host relay setting (point at your own iroh relay; reduce reliance on n0's relays).
- Reconnect / keepalive tuning (periodic keepalive datagrams to hold hole-punches; faster
  recovery after network blips).
- Rotate the **originator master key** itself (currently only the network secret rotates).
- Rate-limit / back off repeated join attempts at the daemon.
- A `SECURITY.md` / threat-model write-up.

## UX / quality of life
- Editable device name in the UI (instead of just the hostname).
- Tray icon + minimize-to-tray; launch GUI on login.
- App icon + window/desktop integration.
- Friendlier first-run / empty states; surface direct-vs-relay and latency more clearly.
- Show/scan the invite QR more prominently; maybe "copy QR as image".

## Platforms
- macOS: packaging (notarized `.app` or `curl|sh` bootstrap; daemon as launchd / Network
  Extension).
- Android: Kotlin/Compose UI over a UniFFI facade around `ipn-core`, TUN via `VpnService`.

## Packaging / installers
See `docs/releasing.md` "Planned installers". Today we ship a portable zip (Windows) + tarball
(Linux); replace with real installers (Windows MSI + signing, Linux `.deb`/AppImage/Flatpak +
systemd unit, macOS notarized app, Android APK).

## Maybe / open questions
- Support more than one network per device at once.
- Optional: expose a per-peer "last seen" history / connection quality.

---

The original feasibility goals (from the initial planning notes) are **implemented**: a private
virtual LAN over iroh that links your own devices, reachable by stable private IPs with existing
clients, with no full-tunnel VPN chokepoint, and simple access control (add a device key + a
network password, with remove/rotate to block anyone who previously had access). The alternative
of building a full custom RDP client was intentionally **not** pursued — IPN provides the network
and you use the RDP/SSH/etc. clients you already have.
