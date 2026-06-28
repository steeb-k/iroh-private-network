//! The data-plane routing primitives: a forwarding table (virtual IP → member
//! NodeId) derived from the roster, minimal IPv4 header parsing, and the
//! cross-platform [`TunDevice`] abstraction.
//!
//! The actual pump (TUN read → lookup dst → send over that peer's iroh datagram;
//! inbound datagram → TUN write) lives in the engine, which owns the live
//! NodeId→Connection map. These pieces are kept pure so they're unit-testable
//! without a real network interface (which needs elevated privileges).

use std::collections::HashMap;
use std::future::Future;
use std::net::Ipv4Addr;

use crate::roster::{Id, Roster};

/// Virtual IP → member NodeId forwarding table, rebuilt whenever the roster
/// changes.
#[derive(Default, Clone, Debug)]
pub struct RouteTable {
    by_ip: HashMap<Ipv4Addr, Id>,
}

impl RouteTable {
    /// Build the table from the current roster's IP assignments.
    pub fn from_roster(roster: &Roster) -> Self {
        let mut by_ip = HashMap::new();
        for (id, member) in roster.members() {
            by_ip.insert(member.virtual_ip, *id);
        }
        Self { by_ip }
    }

    /// Which member owns `ip`, if any.
    pub fn lookup(&self, ip: &Ipv4Addr) -> Option<Id> {
        self.by_ip.get(ip).copied()
    }

    pub fn len(&self) -> usize {
        self.by_ip.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_ip.is_empty()
    }
}

/// Destination IPv4 address of a raw IP packet, or `None` if it isn't IPv4 or is
/// too short. (We only route IPv4 in the virtual /24; IPv6 packets are dropped.)
pub fn dst_ipv4(pkt: &[u8]) -> Option<Ipv4Addr> {
    if pkt.len() < 20 || (pkt[0] >> 4) != 4 {
        return None;
    }
    Some(Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19]))
}

/// Source IPv4 address of a raw IP packet (used to sanity-check inbound packets).
pub fn src_ipv4(pkt: &[u8]) -> Option<Ipv4Addr> {
    if pkt.len() < 20 || (pkt[0] >> 4) != 4 {
        return None;
    }
    Some(Ipv4Addr::new(pkt[12], pkt[13], pkt[14], pkt[15]))
}

/// A cross-platform TUN interface. Implemented over `tun-rs` on desktop and over
/// the `VpnService` fd on Android; a channel-backed mock is used in tests.
///
/// Uses `async fn in trait` (RPITIT), so it's consumed generically rather than as
/// a `dyn` object — each platform provides one concrete type.
pub trait TunDevice: Send + Sync + 'static {
    /// Read one IP packet from the OS into `buf`, returning its length.
    fn recv(&self, buf: &mut [u8]) -> impl Future<Output = std::io::Result<usize>> + Send;
    /// Write one IP packet to the OS.
    fn send(&self, pkt: &[u8]) -> impl Future<Output = std::io::Result<()>> + Send;
    /// The interface MTU (clamped below the iroh datagram limit).
    fn mtu(&self) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roster::{sign, Config, Op};
    use ed25519_dalek::SigningKey;

    fn ipv4_packet(src: Ipv4Addr, dst: Ipv4Addr) -> Vec<u8> {
        let mut p = vec![0u8; 20];
        p[0] = 0x45; // version 4, IHL 5
        p[12..16].copy_from_slice(&src.octets());
        p[16..20].copy_from_slice(&dst.octets());
        p
    }

    #[test]
    fn parses_ipv4_addresses() {
        let p = ipv4_packet(Ipv4Addr::new(10, 99, 0, 2), Ipv4Addr::new(10, 99, 0, 7));
        assert_eq!(dst_ipv4(&p), Some(Ipv4Addr::new(10, 99, 0, 7)));
        assert_eq!(src_ipv4(&p), Some(Ipv4Addr::new(10, 99, 0, 2)));
    }

    #[test]
    fn rejects_non_ipv4_and_short() {
        let mut p = ipv4_packet(Ipv4Addr::UNSPECIFIED, Ipv4Addr::UNSPECIFIED);
        p[0] = 0x60; // IPv6
        assert_eq!(dst_ipv4(&p), None);
        assert_eq!(dst_ipv4(&[0u8; 4]), None);
    }

    #[test]
    fn route_table_maps_members_to_node_ids() {
        let om = SigningKey::from_bytes(&[1u8; 32]);
        let devo = SigningKey::from_bytes(&[2u8; 32]);
        let net = [9u8; 32];
        let cfg = Config {
            network_id: net,
            originator_id: om.verifying_key().to_bytes(),
        };
        let entries = vec![sign(
            net,
            &om,
            Op::Add {
                node_id: devo.verifying_key().to_bytes(),
                hostname: "o".into(),
                virtual_ip: Ipv4Addr::new(10, 99, 0, 2),
                ts: 1,
            },
        )];
        let roster = Roster::build(&cfg, &entries);
        let table = RouteTable::from_roster(&roster);
        assert_eq!(
            table.lookup(&Ipv4Addr::new(10, 99, 0, 2)),
            Some(devo.verifying_key().to_bytes())
        );
        assert_eq!(table.lookup(&Ipv4Addr::new(10, 99, 0, 9)), None);
    }
}
