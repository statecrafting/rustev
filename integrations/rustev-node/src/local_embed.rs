//! The optional `local-embed` feature (spec 011, 3.17.1): the local
//! semantic backend of spec 011 opened from a host configuration and
//! registered like the rules backend, a Rust backend and not a host seam.
//! Without the feature `openLocalEmbed` rejects `feature_disabled` and
//! `features().localEmbed` is false; the prebuilt platform packages are
//! built without it (Decision 10 of spec 011).
//!
//! Nothing here is authorization (011 3.1): the backend supplies
//! `model-derived` logits or a classified failure, and a setup verdict is
//! configuration, never a grant.

use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use rustev_core::seams::DecisionBackend;

use crate::errors::fail;

/// What this build of the addon contains.
#[napi(object)]
pub struct Features {
    /// Built with `local-embed` (spec 011, 3.17.1).
    pub local_embed: bool,
}

/// The features this addon was built with.
#[napi]
pub fn features() -> Features {
    Features {
        local_embed: cfg!(feature = "local-embed"),
    }
}

/// An opened local backend: its derived descriptor and artifact identity,
/// ready to register on an engine.
#[napi]
pub struct LocalEmbedHandle {
    pub(crate) backend: Arc<dyn DecisionBackend>,
    descriptor: Vec<u8>,
    artifact: String,
}

#[napi]
impl LocalEmbedHandle {
    /// The descriptor derived from the bundle's manifest (011 3.10), as its
    /// canonical bytes: what a plan is compiled against.
    #[napi(getter)]
    pub fn descriptor(&self) -> Uint8Array {
        Uint8Array::new(self.descriptor.clone())
    }

    /// The bundle's `ArtifactId` (011 3.5).
    #[napi(getter)]
    pub fn artifact(&self) -> String {
        self.artifact.clone()
    }
}

/// The local backends to register on one engine, with their concurrency.
#[napi]
pub struct LocalEmbedSet {
    pub(crate) entries: Vec<(Arc<dyn DecisionBackend>, usize)>,
}

#[napi]
impl LocalEmbedSet {
    #[napi(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        LocalEmbedSet { entries: vec![] }
    }

    #[napi]
    pub fn add(&mut self, backend: &LocalEmbedHandle, max_concurrency: u32) {
        self.entries
            .push((backend.backend.clone(), max_concurrency as usize));
    }
}

/// `LocalEmbedBackend.open` (011 3.17.1): setup reads and verifies the
/// bundle (011 3.7.1), so it runs off the JavaScript thread. A refusal is
/// `ConfigRefused` whose code is the 3.7.2 setup code and whose detail names
/// the item.
#[napi]
pub async fn open_local_embed(config: serde_json::Value) -> Result<LocalEmbedHandle> {
    open(config).await
}

#[cfg(not(feature = "local-embed"))]
async fn open(config: serde_json::Value) -> Result<LocalEmbedHandle> {
    let _ = config;
    Err(fail(
        "ConfigRefused",
        "feature_disabled",
        serde_json::json!({
            "item": "local-embed",
            "detail": "this build of the binding has no local-embed feature; build it from source with the feature",
        }),
    ))
}

#[cfg(feature = "local-embed")]
async fn open(config: serde_json::Value) -> Result<LocalEmbedHandle> {
    use rustev_backend_embed_linear::{EmbedConfig, EmbedLinearBackend};
    use rustev_contract::Document;
    use rustev_contract::ids::ArtifactId;

    /// The 3.7.1 configuration as the host gives it. The embedding cache
    /// (3.14) is not taken until the backend accepts it; naming it is
    /// refused like any other unknown member, never ignored.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    struct Config {
        bundle_dir: String,
        runtime_path: Option<String>,
        expected_artifact: String,
        workers: u32,
        max_queued: u32,
    }

    let invalid = |detail: String| {
        fail(
            "ConfigRefused",
            "out_of_range",
            serde_json::json!({ "field": "localEmbed", "detail": detail }),
        )
    };
    let c: Config = serde_json::from_value(config).map_err(|e| invalid(e.to_string()))?;
    let expected_artifact =
        ArtifactId::parse(&c.expected_artifact).map_err(|e| invalid(e.to_string()))?;
    let config = EmbedConfig {
        bundle_dir: c.bundle_dir.into(),
        runtime_path: c.runtime_path.map(Into::into),
        expected_artifact,
        workers: c.workers as usize,
        max_queued: c.max_queued as usize,
        embedding_cache: None,
    };
    tokio::task::spawn_blocking(move || {
        crate::errors::guard(|| {
            let backend = EmbedLinearBackend::open(config).map_err(|e| {
                fail(
                    "ConfigRefused",
                    e.code.as_str(),
                    serde_json::json!({ "item": e.item, "detail": e.detail }),
                )
            })?;
            let descriptor = backend
                .descriptor()
                .canonical()
                .map_err(|e| fail("InternalError", "internal", e.to_string()))?;
            let artifact = backend.descriptor().artifact.to_string();
            Ok(LocalEmbedHandle {
                backend: Arc::new(backend),
                descriptor,
                artifact,
            })
        })
    })
    .await
    .map_err(|e| fail("InternalError", "internal", e.to_string()))?
}
