//! The signed membership roster — the security crux of IPN.
//!
//! The roster is an append-only set of **signed entries** that fold into the
//! current membership. It is designed to ride on a multi-writer store
//! (iroh-docs) where the write capability *cannot be un-shared* — so a removed
//! member physically retains the ability to append entries. Security therefore
//! does **not** come from controlling who can write; it comes from these
//! application-layer role rules, enforced every time the roster is rebuilt:
//!
//!   * **`Add`** — a member may vouch for a joiner (web-of-trust). Valid iff the
//!     signer is a *current member* (or the originator) **and** the roster is not
//!     frozen at that point in time.
//!   * **`Remove`** — valid iff signed by the **originator master key**.
//!   * **`Freeze`** — valid iff signed by the **originator master key**.
//!
//! Consequences that the tests below pin down:
//!   * A non-member cannot inject members.
//!   * A *removed* member's later `Add`s are rejected (they're no longer a
//!     current member), and they can never sign `Remove`/`Freeze`.
//!   * Freezing the roster blocks all further adds until it is unfrozen.
//!
//! The hard mass-cutoff ("block everyone who ever had access") is **rotate** —
//! minting a fresh network secret + originator key + docs namespace — handled a
//! layer up; this module only enforces the rules of a single network.
//!
//! Identity note: a member's signing key **is** their iroh device key — a NodeId
//! is an ed25519 public key, so the 32-byte NodeId doubles as the verifying key
//! for that member's signatures. The originator master key is a *separate*,
//! exportable ed25519 keypair (so super-admin authority survives device loss).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

/// An ed25519 public key: a device's NodeId, or the originator master key.
pub type Id = [u8; 32];

const DOMAIN: &str = "ipn-roster-v1";

/// A membership operation. Every variant carries a logical timestamp (`ts`,
/// milliseconds since the Unix epoch) used only to order a concurrent set of
/// entries deterministically; exact wall-clock accuracy is not required.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum Op {
    /// Admit `node_id` as a member (after out-of-band SAS verification).
    Add {
        node_id: Id,
        hostname: String,
        virtual_ip: Ipv4Addr,
        ts: u64,
    },
    /// Revoke a single member. Originator-only.
    Remove { node_id: Id, ts: u64 },
    /// Freeze (or unfreeze) the membership roll. Originator-only.
    Freeze { frozen: bool, ts: u64 },
}

impl Op {
    fn ts(&self) -> u64 {
        match self {
            Op::Add { ts, .. } | Op::Remove { ts, .. } | Op::Freeze { ts, .. } => *ts,
        }
    }
}

/// A signed roster entry. `signature` is over the canonical bytes of
/// `(DOMAIN, network_id, signer, op)`, so it binds the op to the claimed signer
/// and to this specific network.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    pub network_id: Id,
    pub signer: Id,
    pub op: Op,
    pub signature: Vec<u8>,
}

impl Entry {
    /// Content address of this entry (used for dedup + deterministic tie-break).
    pub fn id(&self) -> [u8; 32] {
        let mut buf = Vec::new();
        ciborium::into_writer(&(&self.network_id, &self.signer, &self.op, &self.signature), &mut buf)
            .expect("serialize entry");
        *blake3::hash(&buf).as_bytes()
    }

    /// Verify the entry's signature against its claimed signer. This checks
    /// authenticity only — *authorization* (role rules) is applied in
    /// [`Roster::build`].
    pub fn verify_signature(&self) -> bool {
        let Ok(sig_arr): Result<[u8; 64], _> = self.signature.as_slice().try_into() else {
            return false;
        };
        let sig = Signature::from_bytes(&sig_arr);
        let Ok(vk) = VerifyingKey::from_bytes(&self.signer) else {
            return false;
        };
        vk.verify_strict(&signing_bytes(&self.network_id, &self.signer, &self.op), &sig)
            .is_ok()
    }
}

/// Canonical bytes that get signed for an entry.
fn signing_bytes(network_id: &Id, signer: &Id, op: &Op) -> Vec<u8> {
    #[derive(Serialize)]
    struct View<'a> {
        domain: &'static str,
        network_id: &'a Id,
        signer: &'a Id,
        op: &'a Op,
    }
    let view = View {
        domain: DOMAIN,
        network_id,
        signer,
        op,
    };
    let mut buf = Vec::new();
    ciborium::into_writer(&view, &mut buf).expect("serialize signing view");
    buf
}

/// Sign an op, producing a transmittable [`Entry`]. `signing_key` is the
/// member's device key (for `Add`) or the originator master key (for
/// `Remove`/`Freeze`).
pub fn sign(network_id: Id, signing_key: &SigningKey, op: Op) -> Entry {
    let signer = signing_key.verifying_key().to_bytes();
    let sig = signing_key.sign(&signing_bytes(&network_id, &signer, &op));
    Entry {
        network_id,
        signer,
        op,
        signature: sig.to_bytes().to_vec(),
    }
}

/// Network parameters needed to evaluate the roster.
#[derive(Clone, Debug)]
pub struct Config {
    /// Stable identifier for this network (domain separation across networks).
    pub network_id: Id,
    /// The originator master public key — the sole authority for removals/freeze
    /// and the bootstrap signer of the first member.
    pub originator_id: Id,
}

/// A current member of the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub hostname: String,
    pub virtual_ip: Ipv4Addr,
    /// Which key vouched this member in (a current member, or the originator).
    pub added_by: Id,
}

/// The folded current state of the roster.
#[derive(Clone, Debug, Default)]
pub struct Roster {
    members: BTreeMap<Id, Member>,
    frozen: bool,
}

impl Roster {
    /// Fold a set of entries into the current membership, enforcing all role
    /// rules. Entries with bad signatures, the wrong network, or insufficient
    /// authority are silently dropped — a hostile writer cannot corrupt the
    /// outcome, only waste space.
    pub fn build(cfg: &Config, entries: &[Entry]) -> Roster {
        // 1. Keep only authentic entries for this network. Dedup by content id.
        let mut valid: BTreeMap<[u8; 32], &Entry> = BTreeMap::new();
        for e in entries {
            if e.network_id == cfg.network_id && e.verify_signature() {
                valid.insert(e.id(), e);
            }
        }

        // 2. Deterministic order: by logical timestamp, then content id.
        let mut ordered: Vec<&Entry> = valid.values().copied().collect();
        ordered.sort_by(|a, b| a.op.ts().cmp(&b.op.ts()).then_with(|| a.id().cmp(&b.id())));

        // 3. Fold, applying authorization at each step against the state so far.
        let mut roster = Roster::default();
        for e in ordered {
            match &e.op {
                Op::Freeze { frozen, .. } => {
                    if e.signer == cfg.originator_id {
                        roster.frozen = *frozen;
                    }
                }
                Op::Remove { node_id, .. } => {
                    if e.signer == cfg.originator_id {
                        roster.members.remove(node_id);
                    }
                }
                Op::Add {
                    node_id,
                    hostname,
                    virtual_ip,
                    ..
                } => {
                    // No adds while frozen — including by the originator; the
                    // switch must be flipped back first.
                    if roster.frozen {
                        continue;
                    }
                    let authorized = e.signer == cfg.originator_id
                        || roster.members.contains_key(&e.signer);
                    if authorized {
                        roster.members.insert(
                            *node_id,
                            Member {
                                hostname: hostname.clone(),
                                virtual_ip: *virtual_ip,
                                added_by: e.signer,
                            },
                        );
                    }
                }
            }
        }
        roster
    }

    pub fn is_member(&self, id: &Id) -> bool {
        self.members.contains_key(id)
    }

    pub fn member(&self, id: &Id) -> Option<&Member> {
        self.members.get(id)
    }

    pub fn members(&self) -> impl Iterator<Item = (&Id, &Member)> {
        self.members.iter()
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// Lowest unused host address in `subnet` (a /24), for assigning a joiner's
    /// virtual IP at add-time. Returns `None` if the /24 is full.
    pub fn next_free_ip(&self, subnet: Ipv4Addr) -> Option<Ipv4Addr> {
        let base = subnet.octets();
        let taken: std::collections::BTreeSet<u8> = self
            .members
            .values()
            .filter(|m| m.virtual_ip.octets()[..3] == base[..3])
            .map(|m| m.virtual_ip.octets()[3])
            .collect();
        (2u8..=254).find(|h| !taken.contains(h)).map(|h| {
            Ipv4Addr::new(base[0], base[1], base[2], h)
        })
    }
}

/// Current time in milliseconds since the Unix epoch (for real entry creation).
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }
    fn id(k: &SigningKey) -> Id {
        k.verifying_key().to_bytes()
    }
    fn ip(last: u8) -> Ipv4Addr {
        Ipv4Addr::new(10, 99, 0, last)
    }

    /// Standard setup: originator master key `om`, originator device `devo`
    /// (bootstrapped by the master key as the genesis member).
    fn setup() -> (Config, SigningKey, SigningKey, Vec<Entry>) {
        let om = key(1); // originator master (exportable authority)
        let devo = key(2); // originator's device (a normal member)
        let net = [9u8; 32];
        let cfg = Config {
            network_id: net,
            originator_id: id(&om),
        };
        let genesis = sign(
            net,
            &om,
            Op::Add {
                node_id: id(&devo),
                hostname: "originator-pc".into(),
                virtual_ip: ip(2),
                ts: 1,
            },
        );
        (cfg, om, devo, vec![genesis])
    }

    #[test]
    fn genesis_member_is_admitted() {
        let (cfg, _om, devo, entries) = setup();
        let r = Roster::build(&cfg, &entries);
        assert!(r.is_member(&id(&devo)));
        assert_eq!(r.len(), 1);
        assert_eq!(r.member(&id(&devo)).unwrap().hostname, "originator-pc");
    }

    #[test]
    fn web_of_trust_member_can_admit_member() {
        let (cfg, _om, devo, mut entries) = setup();
        let laptop = key(3);
        // The originator's device (a member) vouches for the laptop.
        entries.push(sign(
            cfg.network_id,
            &devo,
            Op::Add {
                node_id: id(&laptop),
                hostname: "laptop".into(),
                virtual_ip: ip(3),
                ts: 2,
            },
        ));
        let r = Roster::build(&cfg, &entries);
        assert!(r.is_member(&id(&laptop)));
        assert_eq!(r.member(&id(&laptop)).unwrap().added_by, id(&devo));
    }

    #[test]
    fn non_member_cannot_admit() {
        let (cfg, _om, _devo, mut entries) = setup();
        let stranger = key(50); // not a member, not the originator
        let victim = key(51);
        entries.push(sign(
            cfg.network_id,
            &stranger,
            Op::Add {
                node_id: id(&victim),
                hostname: "evil".into(),
                virtual_ip: ip(9),
                ts: 2,
            },
        ));
        let r = Roster::build(&cfg, &entries);
        assert!(!r.is_member(&id(&victim)));
        assert!(!r.is_member(&id(&stranger)));
    }

    #[test]
    fn only_originator_removes() {
        let (cfg, om, devo, mut entries) = setup();
        let laptop = key(3);
        entries.push(sign(
            cfg.network_id,
            &devo,
            Op::Add {
                node_id: id(&laptop),
                hostname: "laptop".into(),
                virtual_ip: ip(3),
                ts: 2,
            },
        ));
        // A non-originator member tries to remove the laptop -> ignored.
        entries.push(sign(
            cfg.network_id,
            &devo,
            Op::Remove {
                node_id: id(&laptop),
                ts: 3,
            },
        ));
        assert!(Roster::build(&cfg, &entries).is_member(&id(&laptop)));

        // The originator master key removes it -> gone.
        entries.push(sign(
            cfg.network_id,
            &om,
            Op::Remove {
                node_id: id(&laptop),
                ts: 4,
            },
        ));
        assert!(!Roster::build(&cfg, &entries).is_member(&id(&laptop)));
    }

    #[test]
    fn removed_member_cannot_forge() {
        // The crux: even though a removed member still holds the docs write-cap,
        // their later Adds are rejected and they can't sign Remove/Freeze.
        let (cfg, om, devo, mut entries) = setup();
        let laptop = key(3);
        let attacker_target = key(60);
        entries.push(sign(
            cfg.network_id,
            &devo,
            Op::Add {
                node_id: id(&laptop),
                hostname: "laptop".into(),
                virtual_ip: ip(3),
                ts: 2,
            },
        ));
        // Originator removes the laptop at ts=3.
        entries.push(sign(
            cfg.network_id,
            &om,
            Op::Remove {
                node_id: id(&laptop),
                ts: 3,
            },
        ));
        // Removed laptop tries to (a) admit a new member and (b) freeze + remove
        // the originator's device, all at later timestamps.
        entries.push(sign(
            cfg.network_id,
            &laptop,
            Op::Add {
                node_id: id(&attacker_target),
                hostname: "backdoor".into(),
                virtual_ip: ip(7),
                ts: 4,
            },
        ));
        entries.push(sign(
            cfg.network_id,
            &laptop,
            Op::Remove {
                node_id: id(&devo),
                ts: 5,
            },
        ));
        entries.push(sign(
            cfg.network_id,
            &laptop,
            Op::Freeze {
                frozen: true,
                ts: 6,
            },
        ));
        let r = Roster::build(&cfg, &entries);
        assert!(!r.is_member(&id(&laptop)), "removed member stays out");
        assert!(!r.is_member(&id(&attacker_target)), "forged add rejected");
        assert!(r.is_member(&id(&devo)), "forged remove ignored");
        assert!(!r.frozen(), "forged freeze ignored");
    }

    #[test]
    fn freeze_blocks_adds_until_unfrozen() {
        let (cfg, om, devo, base) = setup();
        let q = key(4);

        // Frozen at ts=3, then an add at ts=4 -> rejected.
        let mut frozen = base.clone();
        frozen.push(sign(
            cfg.network_id,
            &om,
            Op::Freeze {
                frozen: true,
                ts: 3,
            },
        ));
        frozen.push(sign(
            cfg.network_id,
            &devo,
            Op::Add {
                node_id: id(&q),
                hostname: "q".into(),
                virtual_ip: ip(4),
                ts: 4,
            },
        ));
        let r = Roster::build(&cfg, &frozen);
        assert!(r.frozen());
        assert!(!r.is_member(&id(&q)), "add blocked while frozen");

        // Unfreeze at ts=5, re-add at ts=6 -> accepted.
        let mut thawed = frozen.clone();
        thawed.push(sign(
            cfg.network_id,
            &om,
            Op::Freeze {
                frozen: false,
                ts: 5,
            },
        ));
        thawed.push(sign(
            cfg.network_id,
            &devo,
            Op::Add {
                node_id: id(&q),
                hostname: "q".into(),
                virtual_ip: ip(4),
                ts: 6,
            },
        ));
        let r = Roster::build(&cfg, &thawed);
        assert!(!r.frozen());
        assert!(r.is_member(&id(&q)), "add allowed after unfreeze");
    }

    #[test]
    fn tampered_signature_is_dropped() {
        let (cfg, _om, devo, _entries) = setup();
        let mut bad = sign(
            cfg.network_id,
            &devo,
            Op::Add {
                node_id: id(&key(3)),
                hostname: "x".into(),
                virtual_ip: ip(3),
                ts: 2,
            },
        );
        bad.signature[0] ^= 0xff; // corrupt
        assert!(!bad.verify_signature());
        let r = Roster::build(&cfg, std::slice::from_ref(&bad));
        assert!(r.is_empty());
    }

    #[test]
    fn wrong_network_id_is_dropped() {
        let (cfg, om, devo, _e) = setup();
        // Genesis signed for a DIFFERENT network must not count here.
        let foreign = sign(
            [7u8; 32],
            &om,
            Op::Add {
                node_id: id(&devo),
                hostname: "x".into(),
                virtual_ip: ip(2),
                ts: 1,
            },
        );
        let r = Roster::build(&cfg, std::slice::from_ref(&foreign));
        assert!(r.is_empty());
    }

    #[test]
    fn next_free_ip_skips_taken() {
        let (cfg, _om, devo, mut entries) = setup(); // devo at .2
        let laptop = key(3);
        entries.push(sign(
            cfg.network_id,
            &devo,
            Op::Add {
                node_id: id(&laptop),
                hostname: "laptop".into(),
                virtual_ip: ip(3),
                ts: 2,
            },
        ));
        let r = Roster::build(&cfg, &entries);
        assert_eq!(r.next_free_ip(Ipv4Addr::new(10, 99, 0, 0)), Some(ip(4)));
    }
}
