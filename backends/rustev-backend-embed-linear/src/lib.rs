//! Rustev's local semantic backend (spec 011): a frozen sentence encoder
//! and identified, externally produced linear heads answering
//! `classify`, `proposition` and `rubric` requests with logits.
//!
//! This crate holds, so far, the parts that need no real artifact: the
//! `rustev.embed-linear/1` manifest and its checks ([`manifest`]), bundle
//! verification against it ([`verify_bundle`]), the descriptor derived from
//! it ([`derive_descriptor`]), pooling and head arithmetic ([`head`]),
//! the in-process embedding cache ([`cache`]), and,
//! behind the `synthetic` feature, the synthetic encoder of 3.19. The ONNX
//! encoder and its inference dependencies wait on the feasibility gate
//! (3.2.3).
//!
//! Nothing here is authorization or an input fact (3.1): every value the
//! backend will supply is `model-derived` logits over a head's trained
//! options, linear scores and not probabilities.
#![forbid(unsafe_code)]

pub mod bundle;
pub mod cache;
pub mod head;
pub mod manifest;
#[cfg(feature = "synthetic")]
pub mod synthetic;

use rustev_contract::definition::{Determinism, Operation};
use rustev_contract::descriptor::{
    BackendDescriptor, InputExcess, InputLimit, OperationSupport, OutputKind,
};
use rustev_contract::ids::ArtifactId;
use rustev_contract::run::CostBound;
use rustev_contract::schema;

pub use bundle::{EmbedSetupError, SetupCode, VerifiedBundle, verify_bundle};
pub use manifest::{EmbedManifest, ManifestError};

/// The descriptor of a checked manifest (3.10.1): one entry per operation
/// with at least one head, in the order `classify`, `proposition`,
/// `rubric`, each with output `logits` and the largest option count among
/// its heads; the projection bound refused on excess; determinism
/// `tolerance`. No `rank`, scores, labels or distributions (3.10.2).
pub fn derive_descriptor(manifest: &EmbedManifest, artifact: ArtifactId) -> BackendDescriptor {
    let operations = [
        Operation::Classify,
        Operation::Proposition,
        Operation::Rubric,
    ]
    .into_iter()
    .filter_map(|op| {
        manifest
            .heads
            .iter()
            .filter(|h| h.operation == op)
            .map(|h| h.options.len() as u64)
            .max()
            .map(|max_options| OperationSupport {
                isolation: None,
                screen: None,
                operation: op,
                output: OutputKind::Logits,
                max_options,
            })
    })
    .collect();
    BackendDescriptor {
        schema: schema::BACKEND.into(),
        backend_id: manifest.backend_id.clone(),
        artifact,
        operations,
        input_limit: InputLimit {
            max_bytes: manifest.limits.max_projection_bytes,
            on_excess: InputExcess::Refuse,
        },
        determinism: Determinism::Tolerance,
        ranking: None,
    }
}

/// The cost bound of every call (3.10.3):
/// `units_per_call + units_per_token * max_tokens`.
pub fn cost_bound(manifest: &EmbedManifest) -> CostBound {
    CostBound::Bounded {
        max_units: manifest.max_units().unwrap_or(u64::MAX),
    }
}
