//! Phase-0 connectivity probe: establishes an iroh connection between two
//! machines and reports whether the **live path is DIRECT (hole-punched P2P)
//! or via RELAY** — the thing `dumbpipe` doesn't surface. iroh routinely opens
//! on the relay and *upgrades* to a direct path a few seconds later once
//! hole-punching completes, so the connector samples the path repeatedly.
//!
//! This is throwaway-shaped but exercises the exact API the product's member
//! list will use for its direct-vs-relay status badge
//! (`Endpoint::remote_info` → per-address `usage()` + `is_ip()/is_relay()`).
//!
//! On the machine being reached (e.g. the home PC):
//!     cargo run -p ipn-core --example conn_probe -- accept
//! It prints a ticket. On the other machine (e.g. the laptop):
//!     cargo run -p ipn-core --example conn_probe -- connect <ticket>

use std::time::Duration;

use anyhow::{bail, Context};
use iroh::{
    endpoint::{presets, RemoteInfo, TransportAddrUsage},
    Endpoint,
};
use iroh_tickets::endpoint::EndpointTicket;

const ALPN: &[u8] = b"ipn/probe/0";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let mode = std::env::args().nth(1).unwrap_or_default();
    match mode.as_str() {
        "accept" => accept().await,
        "connect" => {
            let ticket = std::env::args()
                .nth(2)
                .context("usage: conn_probe connect <ticket>")?;
            connect(&ticket).await
        }
        _ => bail!("usage: conn_probe [accept | connect <ticket>]"),
    }
}

async fn accept() -> anyhow::Result<()> {
    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("bind endpoint")?;

    // Wait until we have a relay + observed addresses so the ticket is dialable.
    println!("waiting for relay/online...");
    let _ = tokio::time::timeout(Duration::from_secs(10), endpoint.online()).await;

    let ticket = EndpointTicket::new(endpoint.addr());
    println!("\n=== share this ticket with the connecting machine ===\n{ticket}\n");
    println!("waiting for an incoming connection (Ctrl-C to stop)...");

    while let Some(incoming) = endpoint.accept().await {
        let conn = match incoming.await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("incoming failed: {e}");
                continue;
            }
        };
        let peer = conn.remote_id();
        println!("accepted connection from {}", peer.fmt_short());
        // Echo whatever the prober sends, to keep the connection active.
        tokio::spawn(async move {
            while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                let mut buf = [0u8; 64];
                while let Ok(Some(n)) = recv.read(&mut buf).await {
                    let _ = send.write_all(&buf[..n]).await;
                }
            }
        });
    }
    Ok(())
}

async fn connect(ticket: &str) -> anyhow::Result<()> {
    let ticket: EndpointTicket = ticket.parse().context("parse ticket")?;
    let addr = ticket.endpoint_addr().clone();
    let peer = addr.id;

    let endpoint = Endpoint::builder(presets::N0)
        .bind()
        .await
        .context("bind endpoint")?;

    println!("dialing {}...", peer.fmt_short());
    let conn = endpoint.connect(addr, ALPN).await.context("connect")?;
    println!("connected. sampling the live path for ~24s\n");

    // Keep one bi-stream busy so the connection stays active while we sample.
    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?;
    tokio::spawn(async move {
        let mut buf = [0u8; 64];
        loop {
            if recv.read(&mut buf).await.is_err() {
                break;
            }
        }
    });

    let mut last = String::new();
    for i in 0..12 {
        let _ = send.write_all(b"ping").await;
        if let Some(info) = endpoint.remote_info(peer).await {
            let line = classify(&info);
            if line != last {
                println!("[{:>2}s] {line}", i * 2);
                last = line;
            } else {
                println!("[{:>2}s] (unchanged)", i * 2);
            }
        } else {
            println!("[{:>2}s] no remote info yet", i * 2);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    println!("\nverdict: {}", verdict(&last));
    conn.close(0u32.into(), b"done");
    endpoint.close().await;
    Ok(())
}

/// Summarize the live path: which transport addresses are *active*.
fn classify(info: &RemoteInfo) -> String {
    let mut active_ip = Vec::new();
    let mut active_relay = Vec::new();
    let mut inactive = 0usize;
    for a in info.addrs() {
        match a.usage() {
            TransportAddrUsage::Active => {
                if a.addr().is_ip() {
                    active_ip.push(format!("{}", a.addr()));
                } else if a.addr().is_relay() {
                    active_relay.push(format!("{}", a.addr()));
                }
            }
            _ => inactive += 1,
        }
    }
    let kind = match (active_ip.is_empty(), active_relay.is_empty()) {
        (false, true) => "DIRECT (P2P, hole-punched)",
        (true, false) => "RELAY (via n0 relay)",
        (false, false) => "MIXED (relay + direct; likely upgrading to direct)",
        (true, true) => "no active path yet",
    };
    format!(
        "{kind}  active_ip={active_ip:?} active_relay={active_relay:?} inactive={inactive}"
    )
}

fn verdict(last: &str) -> &'static str {
    if last.starts_with("DIRECT") {
        "solid direct P2P connection — traffic does NOT go through any relay."
    } else if last.starts_with("MIXED") {
        "direct path is established alongside relay; iroh prefers direct, so you're effectively P2P."
    } else if last.starts_with("RELAY") {
        "still on relay after 24s — hole-punching didn't succeed on this network pair (NAT too strict). Works, but not direct."
    } else {
        "could not determine the path."
    }
}
