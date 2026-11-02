//! Spec 034 bundles (spec 042, 3.3.1, 3.3.3, 3.3.6 and 3.3.8): a bundle
//! carrying a template, its profile, variant and base profiles is
//! instantiated with exactly those inputs, the registration records the
//! leaf's `ProfileId`, `deny_unsigned_for_profiles` covers every profile of
//! the resolved chain, and section 5's bundle rows are refused with their
//! codes.
use rustev_contract::{
    Document, Identified, ids::ProfileId, plan_bundle::*, profile::Profile, serve::*,
};
use rustev_core::seams::{BoxFuture, EvidenceSink};
use rustev_serve::{Host, Reply, Service};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

const TEMPLATE: &str =
    include_str!("../../../crates/rustev-core/tests/golden/parameterized.template.json");

struct Sink;
impl EvidenceSink for Sink {
    fn accepts(&self, _: &str) -> bool {
        true
    }
    fn deliver<'a>(
        &'a self,
        _: &'a rustev_contract::run::RunRecord,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("kept".into()) })
    }
}

/// The committed spec 034 template without its semantic step, so no
/// backend is bound: `cutoff`, `options`, `unit`, `enabled` and `requests`
/// remain, with variants `interactive` and `batch`.
fn template() -> Value {
    let mut t: Value = serde_json::from_str(TEMPLATE).unwrap();
    t["inputs"] = json!([]);
    t["steps"]
        .as_array_mut()
        .unwrap()
        .retain(|s| s["id"] != "score");
    t["policy"]["on_unresolved"]
        .as_array_mut()
        .unwrap()
        .retain(|h| h["target"] != "step:score");
    t["policy"]["rules"][0]["when"]["all"]
        .as_array_mut()
        .unwrap()
        .truncate(2);
    t["parameters"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p["name"] != "threshold");
    t
}

fn profile(name: &str, extends: &str, values: Value) -> Value {
    json!({"schema": "rustev.profile/1", "profile": name, "version": "1.0.0",
        "definition": {"name": "parameterized", "version": ">=2.0.0 <3.0.0"},
        "extends": extends, "values": values})
}

fn profile_id(p: &Value) -> ProfileId {
    Profile::parse(&serde_json::to_vec(p).unwrap())
        .unwrap()
        .id()
        .unwrap()
}

/// The inputs of a bundle: profile, variant and base profiles.
#[derive(Clone, Default)]
struct Inputs {
    profile: Option<Value>,
    variant: Option<&'static str>,
    bases: Vec<Value>,
}

/// The plan id the instantiation computes, or a dummy when it refuses.
fn plan_id(i: &Inputs) -> String {
    let definition = serde_json::to_vec(&template()).unwrap();
    let profile = i.profile.as_ref().map(|p| serde_json::to_vec(p).unwrap());
    let bases: Vec<_> = i
        .bases
        .iter()
        .map(|b| serde_json::to_vec(b).unwrap())
        .collect();
    let bases: Vec<&[u8]> = bases.iter().map(Vec::as_slice).collect();
    rustev_core::instantiate::compile_profiled_with(
        &[&definition],
        profile.as_deref(),
        &bases,
        i.variant,
        None,
        &[],
        &[],
        &[],
        None,
    )
    .map(|c| c.id.as_str().to_string())
    .unwrap_or_else(|_| format!("sha256:{}", "0".repeat(64)))
}

fn register_body(i: &Inputs) -> (String, Value) {
    let id = plan_id(i);
    let mut bundle = json!({"schema": "rustev.plan-bundle/1", "definition": template(),
        "execution_policy": "none", "calibrations": [], "bindings": {},
        "compile_report": {"command": "plan compile", "status": "compiled",
            "plan_id": id, "out": "plan.json"}});
    if let Some(p) = &i.profile {
        bundle["profile"] = p.clone();
    }
    if let Some(v) = i.variant {
        bundle["variant"] = json!({"named": v});
    }
    if !i.bases.is_empty() {
        bundle["profile_bases"] = json!(i.bases);
    }
    (
        id,
        json!({"schema": "rustev.serve-register/1", "bundle": bundle, "signature": "none"}),
    )
}

fn service(signing_mode: Value) -> Arc<Service> {
    let mut config: Value = serde_json::from_str(include_str!("fixtures/config.json")).unwrap();
    config["signing_mode"] = signing_mode;
    Service::new(
        ServeConfig::parse(&serde_json::to_vec(&config).unwrap()).unwrap(),
        Host {
            backends: BTreeMap::new(),
            contexts: BTreeMap::new(),
            acquisitions: BTreeMap::new(),
            sink: Arc::new(Sink),
            replay_scopes: BTreeMap::new(),
        },
    )
    .unwrap()
}

fn register(s: &Service, id: &str, body: &Value) -> Reply {
    s.register(id, &serde_json::to_vec(body).unwrap())
}

fn refusal(reply: &Reply) -> ServeRefusal {
    ServeRefusal::parse(&reply.body)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&reply.body)))
}

/// The instantiation code of a `compile_refused` answer.
fn instantiation_code(reply: &Reply) -> String {
    assert_eq!(
        reply.status,
        422,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let r = refusal(reply);
    assert_eq!(r.code, RefusalCode::CompileRefused);
    let Some(CompileRefusal::Document(d)) = &r.compile_refusal else {
        panic!("a compile refusal document")
    };
    let v: Value = serde_json::to_value(d).unwrap();
    assert_eq!(v["stage"], "instantiate", "{v}");
    v["code"].as_str().unwrap().to_string()
}

fn on() -> Value {
    profile("on", "base@1.0.0", json!({"cutoff": 2}))
}
fn base() -> Value {
    profile("base", "none", json!({"enabled": true}))
}

#[test]
fn a_profiled_bundle_registers_with_its_leaf_profile_id() {
    let s = service(json!("allow_unsigned"));
    let inputs = Inputs {
        profile: Some(on()),
        variant: Some("interactive"),
        bases: vec![base()],
    };
    let (id, body) = register_body(&inputs);
    let reply = register(&s, &id, &body);
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let record = PlanRegistration::parse(&reply.body).unwrap();
    assert_eq!(record.plan_id.as_str(), id);
    assert_eq!(record.profile_id, Some(profile_id(&on())));
    // A bundle without a profile records none, and keeps its bytes: the
    // member is omitted.
    let plain = Inputs {
        variant: Some("batch"),
        ..Inputs::default()
    };
    let (id, body) = register_body(&plain);
    let reply = register(&s, &id, &body);
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert!(!String::from_utf8_lossy(&reply.body).contains("profile_id"));
}

#[test]
fn section_5_bundle_rows_are_refused_with_their_codes() {
    let s = service(json!("allow_unsigned"));
    // A definition that declares variants, with variant none.
    let (id, body) = register_body(&Inputs::default());
    assert_eq!(
        instantiation_code(&register(&s, &id, &body)),
        "variant-required"
    );
    // A profile extending a profile not in profile_bases.
    let (id, body) = register_body(&Inputs {
        profile: Some(on()),
        variant: Some("interactive"),
        bases: vec![],
    });
    assert_eq!(
        instantiation_code(&register(&s, &id, &body)),
        "profile-missing"
    );
    // An unknown variant is an instantiation refusal too.
    let (id, body) = register_body(&Inputs {
        variant: Some("nightly"),
        ..Inputs::default()
    });
    assert_eq!(
        instantiation_code(&register(&s, &id, &body)),
        "variant-unknown"
    );
    // profile_bases without a profile; "variant": "none"; an empty list.
    let (id, mut body) = register_body(&Inputs {
        variant: Some("interactive"),
        ..Inputs::default()
    });
    for (member, value) in [
        ("profile_bases", json!([base()])),
        ("variant", json!("none")),
        ("profile_bases", json!([])),
        ("variant", json!({"named": "Interactive"})),
        ("profile", json!("none")),
    ] {
        let saved = body["bundle"].clone();
        body["bundle"][member] = value;
        let reply = register(&s, &id, &body);
        assert_eq!(reply.status, 400, "{member}");
        assert_eq!(
            refusal(&reply).code,
            RefusalCode::InvalidDocument,
            "{member}"
        );
        body["bundle"] = saved;
    }
}

#[test]
fn deny_unsigned_for_profiles_covers_every_profile_of_the_chain() {
    let inputs = Inputs {
        profile: Some(on()),
        variant: Some("interactive"),
        bases: vec![base()],
    };
    let (id, body) = register_body(&inputs);
    // The leaf listed, then a base of its chain listed.
    for listed in [profile_id(&on()), profile_id(&base())] {
        let s = service(json!({"deny_unsigned_for_profiles": [listed]}));
        let reply = register(&s, &id, &body);
        assert_eq!(reply.status, 422);
        assert_eq!(refusal(&reply).code, RefusalCode::UnsignedRefused);
    }
    // Listed only elsewhere: registered unsigned, the mode recorded.
    let other = profile_id(&profile("other", "none", json!({})));
    let s = service(json!({"deny_unsigned_for_profiles": [other]}));
    let reply = register(&s, &id, &body);
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let record = PlanRegistration::parse(&reply.body).unwrap();
    assert_eq!(record.signer, Signer::Unsigned);
    assert!(matches!(
        record.signing_mode,
        SigningMode::DenyUnsignedForProfiles(_)
    ));
    // A bundle without a profile is admitted unsigned.
    let (id, body) = register_body(&Inputs {
        variant: Some("batch"),
        ..Inputs::default()
    });
    assert_eq!(register(&s, &id, &body).status, 200);
}

#[test]
fn an_unreached_base_profile_keeps_the_plan_id_but_conflicts() {
    let s = service(json!("allow_unsigned"));
    let leaf = profile("solo", "none", json!({"cutoff": 3}));
    let first = Inputs {
        profile: Some(leaf.clone()),
        variant: Some("interactive"),
        bases: vec![base()],
    };
    let second = Inputs {
        bases: vec![profile("unused", "none", json!({}))],
        ..first.clone()
    };
    let (id, a) = register_body(&first);
    let (same, b) = register_body(&second);
    assert_eq!(
        id, same,
        "a base the chain never reaches is not in the PlanId"
    );
    assert_eq!(register(&s, &id, &a).status, 200);
    let reply = register(&s, &id, &b);
    assert_eq!(reply.status, 409);
    assert_eq!(refusal(&reply).code, RefusalCode::RegistrationConflict);
    // The same bytes again are idempotent.
    assert_eq!(register(&s, &id, &a).status, 200);
}
