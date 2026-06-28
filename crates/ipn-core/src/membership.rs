//! Replication of the signed roster ([`crate::roster`]) over an **iroh-docs**
//! multi-writer document.
//!
//! Each signed [`Entry`] is stored under a content-addressed key (`e/<id>`), so
//! entries never overwrite one another — the document is an append-only *set* of
//! entries that every member's node folds into the same [`Roster`] via the role
//! rules. iroh-docs handles the actual multi-writer sync/merge across the mesh;
//! this module is just the (serialize → put) / (get → deserialize → fold) glue.
//!
//! Security still lives entirely in [`Roster::build`]: every member holds the
//! doc's write capability, so anyone *in the doc* can append entries — but only
//! entries that pass the role rules affect membership. See the `removed member`
//! integration test in `tests/membership_docs.rs`.

use anyhow::{Context, Result};
use futures_lite::StreamExt;
use iroh_blobs::api::blobs::Blobs;
use iroh_docs::{api::Doc, store::Query, AuthorId};

use crate::roster::{Config, Entry, Roster};

/// Key prefix for roster entries inside the membership document.
const KEY_PREFIX: &[u8] = b"e/";

/// Publish a signed roster entry into the membership document. Keyed by the
/// entry's content id, so re-publishing the same entry is idempotent and
/// distinct entries coexist.
pub async fn publish_entry(doc: &Doc, author: AuthorId, entry: &Entry) -> Result<()> {
    let mut value = Vec::new();
    ciborium::into_writer(entry, &mut value).context("serialize entry")?;
    let mut key = KEY_PREFIX.to_vec();
    key.extend_from_slice(&entry.id());
    doc.set_bytes(author, key, value)
        .await
        .context("set_bytes")?;
    Ok(())
}

/// Read every roster entry currently in the document whose blob content is
/// available locally. Malformed entries and not-yet-synced blobs are skipped —
/// a hostile or partial writer can't break the read.
pub async fn load_entries(doc: &Doc, blobs: &Blobs) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    let mut stream = std::pin::pin!(doc.get_many(Query::all()).await?);
    while let Some(entry) = stream.next().await {
        let entry = entry?;
        if !entry.key().starts_with(KEY_PREFIX) {
            continue;
        }
        let Ok(bytes) = blobs.get_bytes(entry.content_hash()).await else {
            continue; // blob not synced to this node yet
        };
        if let Ok(parsed) = ciborium::from_reader::<Entry, _>(bytes.as_ref()) {
            out.push(parsed);
        }
    }
    Ok(out)
}

/// Fold the document into the current [`Roster`], applying all role rules.
pub async fn build_roster(cfg: &Config, doc: &Doc, blobs: &Blobs) -> Result<Roster> {
    let entries = load_entries(doc, blobs).await?;
    Ok(Roster::build(cfg, &entries))
}
