//! Spec 031 3.11.2: the signed manifest bound into the crate publisher's
//! journal before any registry contact.
use crate::{
    lifecycle::{Entry, Journal},
    manifest::ReleaseManifest,
};

/// Spec 031 3.11.2: call only after signature verification (fakes supply fixtures).
pub fn bind(m: &ReleaseManifest, signature: &str, j: &mut Journal) -> Result<(), String> {
    if signature.is_empty() {
        return Err("missing manifest signature".into());
    }
    if j.entries
        .iter()
        .any(|e| matches!(e, Entry::SignedManifest { .. }))
    {
        return bound(m, j);
    }
    // 3.11: the signed manifest is journaled before the first upload of any
    // kind, so a journal with attempts but no signed manifest is refused, not
    // resumed (no release was published under the unsigned `/1` form).
    if j.entries
        .iter()
        .any(|e| matches!(e, Entry::Attempt { .. } | Entry::Result { .. }))
    {
        return Err("upload predates signed manifest".into());
    }
    j.record(Entry::SignedManifest {
        manifest: String::from_utf8(m.canonical()).map_err(|e| e.to_string())?,
        signature: signature.into(),
    });
    bound(m, j)
}

pub fn bound(m: &ReleaseManifest, j: &Journal) -> Result<(), String> {
    if j.broken() {
        return Err("journal cannot be written".into());
    }
    let mut found = false;
    for e in &j.entries {
        match e {
            Entry::SignedManifest {
                manifest,
                signature,
            } => {
                if signature.is_empty() {
                    return Err("journaled manifest carries no signature".into());
                }
                if manifest.as_bytes() != m.canonical() {
                    return Err(
                        "journal binds another manifest; version or artifact set cannot change"
                            .into(),
                    );
                }
                found = true;
            }
            // Queries count too: any registry contact under the version
            // belongs to the bound manifest's run, never before it.
            Entry::Attempt { .. } | Entry::Result { .. } | Entry::Query { .. } if !found => {
                return Err("registry activity predates signed manifest".into());
            }
            _ => {}
        }
    }
    if found {
        Ok(())
    } else {
        Err("signed manifest must be journaled before upload".into())
    }
}
