# Security model

This describes how IPN decides who is in a network and how access is taken away. It assumes the
networking background in [architecture.md](architecture.md).

## Identities
- A **device** is identified by its NodeId (an ed25519 public key). iroh authenticates this at
  the transport layer, so the peer on the other end of a connection is provably that key.
- A **network** is identified by its **secret** (in the join ticket). From it are derived the
  discovery rendezvous, the admission PSK, and the roster's document namespace.
- The **originator** holds a separate, exportable **master key** — the authority for removing
  members, freezing the roster, and rotating the secret.

## Admission (joining)
1. A joiner connects to an existing member and proves it holds the network PSK (an HMAC
   challenge bound to both NodeIds and fresh nonces — knowing the rendezvous alone isn't enough).
2. Both sides derive an identical **emoji short-authentication-string (SAS)** from the session.
   The two humans compare it. This catches a wrong/MITM'd identity and is a friendly stand-in
   for eyeballing a 64-character key.
3. An existing member approves, which writes a signed `Add` for the joiner (web of trust). The
   originator's device is the genesis member, bootstrapped by the master key.

## The roster and "role rules"
Membership is a set of **signed entries** in a replicated document:
- `Add` counts only if signed by a **current member** (or the originator).
- `Remove` / `Freeze` count only if signed by the **originator master key**.

Why it's done this way: the document's write capability is the network secret, which **every
member holds and which can't be un-shared** — you can't claw a secret back. So security doesn't
come from gatekeeping *who can write*; it comes from gatekeeping *which writes count*. A removed
device can still scribble entries into the shared document, but its signature is no longer that
of a current member, so every node — including its own — ignores them. (The
`removed_member_cannot_forge` test proves this even over real replication.)

## Taking access away
- **Remove a member** — the originator signs a `Remove`. It propagates to connected peers; each
  node rebuilds the roster, drops the device from routing, and tears down any live connection to
  it (so no "ghost" connection survives). A device that finds itself removed **self-evicts**:
  it drops its connections and clears the now-dead network.
- **Freeze** — the originator stops any new joins until unfrozen.
- **Rotate** (the hard cutoff) — the originator boots everyone and restarts the network under a
  brand-new secret, then shares a new ticket with the devices to keep. Because the secret drives
  discovery, admission, and the roster namespace, anyone holding the old ticket is locked out
  entirely — including a device that happened to be **offline** when it was removed (the one
  case a single `Remove` can't reach until that device reconnects).

## What this does and doesn't protect
- Traffic between members is end-to-end encrypted by iroh; non-members can't read it, and the
  network's discovery rendezvous is private so outsiders can't even find it.
- Trust is **device-granular**: access is per device key. Losing a device means removing (or
  rotating out) its key.
- This is a personal-scale trust model (a handful of your own devices), not a hardened
  multi-tenant system. Notably, members are mutually trusting once admitted; web-of-trust means
  any member can vouch in another (the originator is the backstop via remove/rotate).
