//! Local embedding backends registered from the host's configuration at
//! startup (spec 011, 3.17.3): the local semantic backend of spec 011, opened
//! from a bundle directory, an optional runtime path and an expected
//! `ArtifactId` the host configures, and bound under a `backend_bindings`
//! entry of `rustev.serve-config/1` whose backend is
//! `{"compiled": {"feature": "local-embed"}}`.
//!
//! Only the host names a bundle, a path or a runtime: the configuration is
//! read once, before any listener starts, and no request document has a
//! member that could name one (every service document refuses unknown
//! members). A registration binds a plan's backend id to a binding name, and
//! a binding is either configured here or refused at startup.
//!
//! Without the `local-embed` feature a configured local backend is refused
//! `feature_disabled`, and the build has no crate of spec 011's local
//! inference family (011 3.20.2). Nothing here is authorization (011 3.1):
//! the backend supplies `model-derived` logits or a classified failure.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustev_contract::serve::{BackendKind, ServeConfig};
use rustev_core::seams::DecisionBackend;
use serde::Deserialize;

/// The compiled feature a `backend_bindings` entry names to bind a local
/// backend.
pub const FEATURE: &str = "local-embed";

/// The largest local backend configuration file read.
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

/// The most local backends one configuration names.
pub const MAX_BACKENDS: usize = 16;

/// Whether this build has the `local-embed` feature.
pub const ENABLED: bool = cfg!(feature = "local-embed");

/// One local backend's configuration: spec 011 3.7.1's members the backend
/// takes today. Paths are absolute (the backend refuses a relative one). The
/// embedding cache (3.14) is not taken; naming it is refused like any other
/// unknown member, never ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEmbedConfig {
    pub bundle_dir: String,
    #[serde(default)]
    pub runtime_path: Option<String>,
    pub expected_artifact: String,
    pub workers: usize,
    pub max_queued: usize,
}

/// Parse a local backend configuration file: a JSON object from binding
/// name to [`LocalEmbedConfig`].
pub fn parse(bytes: &[u8]) -> Result<BTreeMap<String, LocalEmbedConfig>, String> {
    let local: BTreeMap<String, LocalEmbedConfig> =
        serde_json::from_slice(bytes).map_err(|e| format!("local-embed configuration: {e}"))?;
    if local.len() > MAX_BACKENDS {
        return Err(format!(
            "local-embed configuration: more than {MAX_BACKENDS} backends"
        ));
    }
    Ok(local)
}

/// Open every configured local backend, keyed by its binding name, for
/// [`crate::sinks::standalone_with_backends`]. Each name must be a binding of
/// `config` whose backend is the compiled feature [`FEATURE`], and every such
/// binding must be configured; the service then checks each backend's
/// descriptor against the binding's expected `DescriptorId`. Opening reads
/// and verifies each bundle, so a caller on an async runtime runs this on a
/// blocking thread.
pub fn open_all(
    config: &ServeConfig,
    local: &BTreeMap<String, LocalEmbedConfig>,
) -> Result<BTreeMap<String, Arc<dyn DecisionBackend>>, String> {
    for (name, binding) in &config.backend_bindings {
        if matches!(&binding.backend, BackendKind::Compiled { feature } if feature.as_str() == FEATURE)
            && !local.contains_key(name.as_str())
        {
            return Err(format!(
                "backend binding {} names {FEATURE} but no local backend is configured for it",
                name.as_str()
            ));
        }
    }
    let mut out = BTreeMap::new();
    for (name, c) in local {
        let bound = config
            .backend_bindings
            .iter()
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, b)| &b.backend);
        if !matches!(bound, Some(BackendKind::Compiled { feature }) if feature.as_str() == FEATURE)
        {
            return Err(format!(
                "local backend {name} is not a backend binding whose backend is the compiled feature {FEATURE}"
            ));
        }
        out.insert(name.clone(), open(name, c)?);
    }
    Ok(out)
}

/// Open one local backend. A setup refusal names the binding and carries
/// spec 011 3.7.2's code and item.
#[cfg(feature = "local-embed")]
pub fn open(name: &str, c: &LocalEmbedConfig) -> Result<Arc<dyn DecisionBackend>, String> {
    use rustev_backend_embed_linear::{EmbedConfig, EmbedLinearBackend};
    use rustev_contract::ids::ArtifactId;
    let expected_artifact = ArtifactId::parse(&c.expected_artifact)
        .map_err(|e| format!("local backend {name}: expected_artifact: {e}"))?;
    let backend = EmbedLinearBackend::open(EmbedConfig {
        bundle_dir: c.bundle_dir.clone().into(),
        runtime_path: c.runtime_path.clone().map(Into::into),
        expected_artifact,
        workers: c.workers,
        max_queued: c.max_queued,
        embedding_cache: None,
    })
    .map_err(|e| format!("local backend {name}: {e}"))?;
    Ok(Arc::new(backend))
}

/// Without the feature every local backend is refused `feature_disabled`.
#[cfg(not(feature = "local-embed"))]
pub fn open(name: &str, c: &LocalEmbedConfig) -> Result<Arc<dyn DecisionBackend>, String> {
    let _ = c;
    Err(format!(
        "local backend {name}: feature_disabled: local-embed: this build of rustev-serve has no local-embed feature; build it from source with the feature"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configuration_refuses_unknown_members_and_too_many_backends() {
        let one =
            r#"{"bundle_dir": "/b", "expected_artifact": "a", "workers": 1, "max_queued": 0}"#;
        assert_eq!(
            parse(format!(r#"{{"le": {one}}}"#).as_bytes())
                .unwrap()
                .len(),
            1
        );
        for unknown in ["embedding_cache", "url", "model"] {
            let c = one.replacen('{', &format!(r#"{{"{unknown}": 1, "#), 1);
            let e = parse(format!(r#"{{"le": {c}}}"#).as_bytes()).unwrap_err();
            assert!(e.contains("unknown field"), "{unknown}: {e}");
        }
        let many: Vec<String> = (0..=MAX_BACKENDS)
            .map(|n| format!(r#""b{n}": {one}"#))
            .collect();
        let e = parse(format!("{{{}}}", many.join(",")).as_bytes()).unwrap_err();
        assert!(e.contains("more than"), "{e}");
    }

    fn config(bindings: serde_json::Value) -> ServeConfig {
        use rustev_contract::Document;
        let mut c: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/config.json")).unwrap();
        c["backend_bindings"] = bindings;
        ServeConfig::parse(&serde_json::to_vec(&c).unwrap()).unwrap()
    }

    fn binding(feature: &str) -> serde_json::Value {
        serde_json::json!({
            "expected_descriptor_id": format!("sha256:{}", "0".repeat(64)),
            "backend": {"compiled": {"feature": feature}}})
    }

    fn le(bundle_dir: String, expected_artifact: String) -> LocalEmbedConfig {
        LocalEmbedConfig {
            bundle_dir,
            runtime_path: None,
            expected_artifact,
            workers: 1,
            max_queued: 0,
        }
    }

    #[test]
    fn every_local_binding_is_configured_and_every_configured_backend_is_a_local_binding() {
        let c = le("/b".into(), "a".into());
        let bound = config(serde_json::json!({ "le": binding(FEATURE) }));
        let e = open_all(&bound, &BTreeMap::new()).err().unwrap();
        assert!(e.contains("no local backend is configured"), "{e}");
        let unbound = config(serde_json::json!({}));
        let e = open_all(&unbound, &BTreeMap::from([("le".into(), c.clone())]))
            .err()
            .unwrap();
        assert!(e.contains("is not a backend binding"), "{e}");
        let other = config(serde_json::json!({ "le": binding("rules") }));
        let e = open_all(&other, &BTreeMap::from([("le".into(), c)]))
            .err()
            .unwrap();
        assert!(e.contains("is not a backend binding"), "{e}");
        assert!(open_all(&unbound, &BTreeMap::new()).unwrap().is_empty());
    }

    /// The committed SYNTHETIC support-routing bundle (spec 011 increment 5).
    #[cfg(feature = "local-embed")]
    fn bundle() -> (String, String) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../backends/rustev-backend-embed-linear/tests/fixtures/support-routing")
            .canonicalize()
            .unwrap();
        let case: serde_json::Value = serde_json::from_slice(
            &std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                "../../tools/rustev-conformance/cases/local-embed/local-embed.support-routing.json",
            ))
            .unwrap(),
        )
        .unwrap();
        let artifact = case["local_embed"]["expected_artifact"].as_str().unwrap();
        (dir.to_str().unwrap().into(), artifact.into())
    }

    #[cfg(feature = "local-embed")]
    #[test]
    fn a_configured_bundle_opens_and_a_wrong_one_is_refused_with_its_setup_code() {
        let (dir, artifact) = bundle();
        let bound = config(serde_json::json!({ "le": binding(FEATURE) }));
        let opened = open_all(
            &bound,
            &BTreeMap::from([("le".into(), le(dir.clone(), artifact.clone()))]),
        )
        .unwrap();
        assert_eq!(opened["le"].descriptor().artifact.to_string(), artifact);
        let other = format!("sha256:{}", "1".repeat(64));
        let e = open("le", &le(dir, other)).err().unwrap();
        assert!(e.contains("artifact_mismatch"), "{e}");
        let e = open("le", &le("relative/dir".into(), artifact))
            .err()
            .unwrap();
        assert!(e.contains("bundle_missing"), "{e}");
    }

    #[cfg(not(feature = "local-embed"))]
    #[test]
    fn without_the_feature_a_local_backend_is_refused_feature_disabled() {
        let c = LocalEmbedConfig {
            bundle_dir: "/b".into(),
            runtime_path: None,
            expected_artifact: "a".into(),
            workers: 1,
            max_queued: 0,
        };
        let e = open("le", &c).err().unwrap();
        assert!(e.contains("feature_disabled"), "{e}");
    }
}
