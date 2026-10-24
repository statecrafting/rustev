#[path = "../../../tools/rustev-conformance/tests/explain_support/mod.rs"]
mod explain_support;
use explain_support::{FIXTURES, cases, names, refusals};
use rustev_eval::resolve::{Resolution, Resolver};
use std::{collections::BTreeMap, path::PathBuf};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn files(args: &serde_json::Value) -> BTreeMap<String, Vec<u8>> {
    names(args)
        .into_iter()
        .map(|n| {
            let b = std::fs::read(root().join(FIXTURES).join(&n)).unwrap();
            (n, b)
        })
        .collect()
}
struct Never;
impl Resolver for Never {
    fn resolve(&self, _: &str, _: usize) -> Resolution {
        panic!("option refusal must precede resolution")
    }
}
#[test]
fn every_cli_explanation_is_byte_identical() {
    for (name, args) in cases() {
        let r = rustev_surface::explain::explain(&files(&args), &args, None).unwrap();
        assert_eq!(
            r.explanation,
            std::fs::read(
                root()
                    .join(FIXTURES)
                    .join(format!("{name}.explanation.json"))
            )
            .unwrap(),
            "{name}"
        );
        assert_eq!(
            r.markdown.as_bytes(),
            std::fs::read(root().join(FIXTURES).join(format!("{name}.explanation.md"))).unwrap(),
            "{name}"
        );
    }
}
#[test]
fn options_and_refusal_codes_match_the_cli_rules() {
    for (name, args, resolve, code) in refusals() {
        let r = rustev_surface::explain::explain(
            &files(&args),
            &args,
            resolve.then_some(&Never as &dyn Resolver),
        );
        assert_eq!(r.err().unwrap().code.as_str(), code, "{name}");
    }
}

#[test]
fn mixed_diff_preserves_a_b_order_and_shared_bundle_options() {
    for bundle_first in [false, true] {
        let a =
            explain_support::evidence("support", if bundle_first { "bundle" } else { "record" });
        let b =
            explain_support::evidence("support", if bundle_first { "record" } else { "bundle" });
        let args = serde_json::json!({"evidence": a, "query": {"diff": {"other": b}}, "options": explain_support::options()});
        let result = rustev_surface::explain::explain(&files(&args), &args, None).unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&result.explanation).unwrap();
        assert_eq!(doc["query"], "diff");
        assert_eq!(doc["sides"][0]["name"], "a");
        assert_eq!(doc["sides"][1]["name"], "b");
        assert_eq!(
            doc["sides"][0]["form"],
            if bundle_first { "bundle" } else { "record" }
        );
        assert_eq!(
            doc["sides"][1]["form"],
            if bundle_first { "record" } else { "bundle" }
        );
    }
}

#[test]
fn bundle_resolution_uses_the_supplied_callback_and_scope_precedes_it() {
    use std::cell::Cell;
    struct Store {
        bytes: Vec<u8>,
        calls: Cell<usize>,
    }
    impl Resolver for Store {
        fn resolve(&self, reference: &str, max_bytes: usize) -> Resolution {
            assert_eq!(reference, "host-record");
            assert!(max_bytes >= self.bytes.len());
            self.calls.set(self.calls.get() + 1);
            Resolution::Bytes(self.bytes.clone())
        }
    }
    let mut args = serde_json::json!({"evidence": explain_support::evidence("support", "bundle"), "query": "models", "options": explain_support::options()});
    let mut inputs = files(&args);
    let mut bundle: serde_json::Value =
        serde_json::from_slice(&inputs["support.bundle.json"]).unwrap();
    let retained = bundle["run"]["retention"]["retained"].clone();
    let store = Store {
        bytes: retained["bytes"].as_str().unwrap().as_bytes().to_vec(),
        calls: Cell::new(0),
    };
    bundle["run"]["retention"] = serde_json::json!({"external": {"reference": "host-record", "digest": retained["digest"], "expires_at_ms": retained["expires_at_ms"]}});
    inputs.insert(
        "support.bundle.json".into(),
        serde_json::to_vec(&bundle).unwrap(),
    );
    rustev_surface::explain::explain(&inputs, &args, Some(&store)).unwrap();
    assert_eq!(store.calls.get(), 1);
    args["options"]["scope"]["tenant"] = "another-tenant".into();
    let error = rustev_surface::explain::explain(&inputs, &args, Some(&Never))
        .err()
        .unwrap();
    assert_eq!(error.code.as_str(), "scope-mismatch");
}
