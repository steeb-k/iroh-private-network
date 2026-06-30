//! Stateful connection tracking for the one-way **"Disable remote access"**
//! block.
//!
//! When a device turns the switch on, it should still be able to reach other
//! members (RDP/SSH *out*), but no member should be able to initiate to it
//! (*in*). A stateless "drop all inbound" filter can't do that — it would also
//! drop the return traffic of connections this device started. So we track the
//! flows we initiate (on the outbound TUN→mesh path) and, while the block is on,
//! admit an inbound packet only if it matches the reverse of a tracked flow.
//!
//! The table is keyed by [`FlowKey`] and stores a coarse last-seen timestamp;
//! entries idle past [`FLOW_TTL_MS`] are swept on the periodic engine tick. It
//! lives behind a plain `RwLock` (the same lock discipline as the route/conn
//! tables) so the per-packet pump never touches the async state mutex.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::router::{flow_key, is_tcp_initiation, FlowKey};

/// Idle lifetime of a tracked flow. Long enough to cover a quiet RDP/SSH session
/// between keepalives; short enough that the table self-trims.
const FLOW_TTL_MS: u64 = 120_000;

/// Tracks the flows this device initiated, so return traffic is allowed back in
/// while unsolicited inbound is dropped.
#[derive(Default)]
pub struct Conntrack {
    flows: RwLock<HashMap<FlowKey, u64>>,
}

impl Conntrack {
    /// Record/refresh a flow **we initiated** (called on every outbound TUN→mesh
    /// packet). `now` is the engine's coarse clock (ms).
    ///
    /// Direction matters: a flow is *created* only when we open it — a TCP SYN
    /// (client opening a connection), or the first packet of a UDP/other flow.
    /// Server-side responses (TCP packets with ACK, no SYN) only **refresh** a
    /// flow that already exists; they never create one. So an inbound connection
    /// to one of our services never becomes "established" in the table, and
    /// enabling the block cuts it — while our own outbound sessions, refreshed by
    /// their ongoing traffic, stay alive.
    pub fn record_outbound(&self, pkt: &[u8], now: u64) {
        let Some(k) = flow_key(pkt) else { return };
        let mut flows = self.flows.write().unwrap();
        if k.proto == 6 {
            // TCP: create only on a client SYN; otherwise just refresh if present.
            if is_tcp_initiation(pkt) {
                flows.insert(k, now);
            } else if let Some(t) = flows.get_mut(&k) {
                *t = now;
            }
        } else {
            // UDP/ICMP/etc.: no handshake to read direction from — the first
            // outbound packet opens the flow.
            flows.insert(k, now);
        }
    }

    /// Whether an inbound packet is return traffic for a flow we initiated.
    pub fn allows_inbound(&self, pkt: &[u8], now: u64) -> bool {
        let Some(k) = flow_key(pkt) else {
            return false;
        };
        match self.flows.read().unwrap().get(&k.reversed()) {
            Some(&seen) => now.saturating_sub(seen) <= FLOW_TTL_MS,
            None => false,
        }
    }

    /// Drop flows idle past the TTL (called from the periodic tick).
    pub fn sweep(&self, now: u64) {
        self.flows
            .write()
            .unwrap()
            .retain(|_, &mut seen| now.saturating_sub(seen) <= FLOW_TTL_MS);
    }

    /// Forget all tracked flows (on disconnect / teardown).
    pub fn clear(&self) {
        self.flows.write().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const SYN: u8 = 0x02;
    const ACK: u8 = 0x10;

    /// IPv4 TCP packet (full 20-byte TCP header so the flags byte is present).
    fn tcp(src: Ipv4Addr, dst: Ipv4Addr, sport: u16, dport: u16, flags: u8) -> Vec<u8> {
        let mut p = vec![0u8; 40]; // 20 IP + 20 TCP
        p[0] = 0x45; // v4, IHL 5
        p[9] = 6; // TCP
        p[12..16].copy_from_slice(&src.octets());
        p[16..20].copy_from_slice(&dst.octets());
        p[20..22].copy_from_slice(&sport.to_be_bytes());
        p[22..24].copy_from_slice(&dport.to_be_bytes());
        p[33] = flags; // TCP flags at ihl(20) + 13
        p
    }

    #[test]
    fn return_traffic_allowed_unsolicited_dropped() {
        let me = Ipv4Addr::new(10, 99, 0, 2);
        let peer = Ipv4Addr::new(10, 99, 0, 5);
        let ct = Conntrack::default();

        // We initiate me:51000 -> peer:22 (client SYN) -> flow created.
        ct.record_outbound(&tcp(me, peer, 51000, 22, SYN), 1000);

        // Peer's reply (peer:22 -> me:51000) is allowed.
        assert!(ct.allows_inbound(&tcp(peer, me, 22, 51000, ACK), 1001));

        // An unsolicited inbound (peer:40000 -> me:3389) is dropped.
        assert!(!ct.allows_inbound(&tcp(peer, me, 40000, 3389, SYN), 1001));
    }

    #[test]
    fn server_response_does_not_open_a_flow() {
        // The core one-way property: a connection INBOUND to one of our services
        // never becomes "established" in the table, because our server's replies
        // (SYN-ACK / ACK, no client SYN) don't create a flow. So enabling the
        // block cuts an already-open inbound session.
        let me = Ipv4Addr::new(10, 99, 0, 2);
        let peer = Ipv4Addr::new(10, 99, 0, 5);
        let ct = Conntrack::default();
        ct.record_outbound(&tcp(me, peer, 3389, 50000, SYN | ACK), 1000);
        ct.record_outbound(&tcp(me, peer, 3389, 50000, ACK), 1001);
        assert!(!ct.allows_inbound(&tcp(peer, me, 50000, 3389, ACK), 1002));
    }

    #[test]
    fn our_outbound_session_stays_alive() {
        // A session WE initiated keeps working across the TTL because our ongoing
        // (non-SYN) outbound traffic refreshes the flow.
        let me = Ipv4Addr::new(10, 99, 0, 2);
        let peer = Ipv4Addr::new(10, 99, 0, 5);
        let ct = Conntrack::default();
        ct.record_outbound(&tcp(me, peer, 51000, 22, SYN), 1000);
        ct.record_outbound(&tcp(me, peer, 51000, 22, ACK), 1000 + 100_000); // refresh
        assert!(ct.allows_inbound(&tcp(peer, me, 22, 51000, ACK), 1000 + 150_000));
    }

    #[test]
    fn expired_flow_is_not_matched() {
        let me = Ipv4Addr::new(10, 99, 0, 2);
        let peer = Ipv4Addr::new(10, 99, 0, 5);
        let ct = Conntrack::default();
        ct.record_outbound(&tcp(me, peer, 51000, 22, SYN), 1000);
        // Past the TTL with no refresh, the reply no longer matches.
        assert!(!ct.allows_inbound(&tcp(peer, me, 22, 51000, ACK), 1000 + FLOW_TTL_MS + 1));
    }
}
