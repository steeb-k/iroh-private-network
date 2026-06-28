//! Phase-0 smoke test: bootstrap the iroh node, print this device's NodeId and
//! dialable address, then wait until a relay is reached. Proves the whole iroh
//! 1.0 stack (endpoint + blobs + gossip + docs + mDNS) resolves, builds, and
//! runs on this platform.
//!
//! Run with:  cargo run -p ipn-core --example node_id

use std::time::Duration;

use ipn_core::IrohNode;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,iroh=warn".into()),
        )
        .init();

    let dir = std::env::temp_dir().join("ipn-node-id-smoke");
    println!("data dir: {}", dir.display());

    let node = IrohNode::spawn(&dir).await?;
    let id = data_encoding::HEXLOWER.encode(&node.node_id_bytes());
    println!("NodeId (hex): {id}");
    println!("addr: {:?}", node.addr());

    println!("waiting for relay/online (up to 10s)...");
    match tokio::time::timeout(Duration::from_secs(10), node.wait_online()).await {
        Ok(()) => println!("online. dialable addr: {:?}", node.addr()),
        Err(_) => println!("timed out waiting for relay (offline or blocked)"),
    }

    node.shutdown().await?;
    Ok(())
}
