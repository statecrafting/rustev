//! Capture cases use semantic, generation and (spec 063 increment 3)
//! acquisition steps; a plan's source descriptors reach assembly.
use rustev_contract::{Document, canonical::tagged_digest, scope::Scope, time::Timestamp};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble_with_sources};
use rustev_eval::replay::ReplayConfig;
use rustev_eval::resolve::{Resolution, Resolver};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub scope: Scope,
    pub max_bytes: u64,
    pub created_at_ms: Timestamp,
    pub retain: BTreeMap<String, String>,
}

struct Items(BTreeMap<String, Vec<u8>>);
impl Resolver for Items {
    fn resolve(&self, reference: &str, max: usize) -> Resolution {
        self.0.get(reference).map_or(Resolution::Missing, |b| {
            Resolution::Bytes(b[..b.len().min(max)].to_vec())
        })
    }
}

pub fn outputs(input: &AssemblyInput<'_>, retain: &str) -> Result<super::runtime::Outputs, String> {
    outputs_with_sources(input, retain, &[])
}

/// [`outputs`] for a plan bound to acquisition sources (spec 063 3.8):
/// assembly receives their descriptors, and replay reuses the captured
/// pages without a source call.
pub fn outputs_with_sources(
    input: &AssemblyInput<'_>,
    retain: &str,
    sources: &[rustev_contract::source::SourceDescriptor],
) -> Result<super::runtime::Outputs, String> {
    let items = RefCell::new(BTreeMap::new());
    let external = |_, bytes: &[u8]| {
        let doc: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let digest = tagged_digest(doc["schema"].as_str().unwrap(), bytes);
        items.borrow_mut().insert(digest.clone(), bytes.to_vec());
        digest
    };
    let bundle = assemble_with_sources(
        input,
        &RetentionChoice {
            content: match retain {
                "embedded" => Content::Embedded,
                "external" => Content::External(&external),
                _ => return Err("unknown retention".into()),
            },
            ..Default::default()
        },
        sources,
    )
    .map_err(|e| e.to_string())?;
    let bytes = bundle.record_canonical().map_err(|e| e.to_string())?;
    let host =
        rustev_surface::host::Memory::new(BTreeMap::from([("b.json".into(), bytes.clone())]));
    // Offline replay has no backend or source handle, so cannot call either.
    let done = rustev_surface::replay::replay(
        "replay",
        &host,
        "b.json",
        &Items(items.into_inner()),
        &ReplayConfig {
            scope: input.scope.clone(),
            now_ms: input.created_at_ms,
            host_cap_ms: None,
        },
    )
    .map_err(|e| format!("replay: {:?}", e))?;
    if done.code != 0 || done.out.status() != "reproduced" {
        return Err(format!("replay: {done:?}"));
    }
    Ok(BTreeMap::from([
        ("bundle.json".into(), bytes),
        ("replay.json".into(), done.out.bytes()),
    ]))
}
