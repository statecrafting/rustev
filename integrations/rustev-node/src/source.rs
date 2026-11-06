//! Spec 063 3.5: the acquisition source seam. Increment 3a binds plans to
//! source descriptors on compile and load (3.5.2); the runtime adapter, its
//! callbacks and gauges join with the rest of increment 3.

use napi::bindgen_prelude::*;
use rustev_contract::Document;
use rustev_contract::source::SourceDescriptor;

use crate::errors::fail;

/// Parse the `sources` a compile or load names (3.5.2), each with the
/// bounded `rustev.source/1` parser. Sources are never taken from the
/// engine's installed set, so a `PlanId` never depends on what an engine
/// holds; an absent field is an empty list.
pub fn parse_sources(sources: Option<&[Uint8Array]>) -> Result<Vec<SourceDescriptor>> {
    sources
        .unwrap_or_default()
        .iter()
        .map(|s| SourceDescriptor::parse(s))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| fail("InvalidDocument", "invalid_document", e.to_string()))
}
