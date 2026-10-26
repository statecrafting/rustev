//! Spec 038 increment 2a: contract documents only, with synthetic inputs.
use rustev_contract::canonical::{canonical_value_bytes, tagged_digest};
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::extraction_documents::{
    ExtractRequest, ExtractRules, extraction_schema_digest,
};
use rustev_contract::output::{BackendOutputDoc, FieldLogprobs, LogprobItem};
use rustev_contract::request::{ExtractionRequestOperation, RequestDoc};
use rustev_contract::{Document, Identified};
use serde_json::{Value, json};

fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}
fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}
fn rules() -> Value {
    json!({"schema":"rustev.extract-rules/1", "name":"filters", "version":"1",
        "sources":[{"name":"text", "ref":"input:request/text"}],
        "fields":[{"field":"party", "rules":[{"name":"party", "when":"always",
            "then":{"integer_after":{"source":"text","terms":["party "],"max_digits":18}}}]}]})
}
fn request() -> Value {
    let output = json!({"fields":[{"name":"area", "type":{"enum":{"vocabulary":"areas"}}}]});
    let schema = serde_json::from_value(output.clone()).unwrap();
    json!({"schema":"rustev.extract-request/1", "step":"filters", "template":"filters",
        "template_id":digest(), "instructions":"Extract filters", "projection":{"input:text":"quiet"},
        "output":output, "schema_digest":extraction_schema_digest(&schema).unwrap(),
        "vocabularies":{"areas":["quiet","busy"]}})
}
fn descriptor() -> Value {
    json!({"schema":"rustev.backend/1", "backend_id":"extractor", "artifact":digest(),
        "operations":[{"operation":"extract", "max_options":32,
            "output":{"typed_record":{"structure":"json_schema", "max_output_bytes":8192,
                "max_options":32, "option_logprobs":true}}}],
        "input_limit":{"max_bytes":4096,"on_excess":"refuse"}, "determinism":"unspecified"})
}
fn output() -> Value {
    json!({"schema":"rustev.backend-output/1", "step":"filters", "instance":[], "artifact":digest(),
        "output":{"typed_record":{"fields":{"area":"quiet", "raw":[null,4,true]},
            "logprobs":[{"field":"area", "item":"none", "values":{"quiet":-0.25,"busy":-1.5}},
                {"field":"features", "item":0, "values":{"pool":-2.0}}],
            "provider_reported":{"area":"probably"}}}})
}
fn roundtrip<T: Document + PartialEq + std::fmt::Debug>(v: &Value) -> T {
    let doc = T::parse(&bytes(v)).unwrap();
    let canonical = doc.record_canonical().unwrap();
    let again = T::parse(&canonical).unwrap();
    assert_eq!(doc, again);
    assert_eq!(canonical, again.record_canonical().unwrap());
    assert_eq!(doc.record_digest().unwrap(), again.record_digest().unwrap());
    doc
}

#[test]
fn new_documents_roundtrip_and_have_stable_tagged_identities() {
    let r = roundtrip::<ExtractRules>(&rules());
    assert_eq!(
        r.id().unwrap().as_str(),
        tagged_digest(ExtractRules::SCHEMA, &r.canonical().unwrap())
    );
    let req = roundtrip::<ExtractRequest>(&request());
    assert_eq!(req.vocabularies["areas"], ["quiet", "busy"]);
    let d = roundtrip::<BackendDescriptor>(&descriptor());
    assert_eq!(
        d.id().unwrap().as_str(),
        tagged_digest(BackendDescriptor::SCHEMA, &d.canonical().unwrap())
    );
    roundtrip::<BackendOutputDoc>(&output());
    let base = r.id().unwrap();
    for (path, change) in [
        ("/name", json!("other")),
        ("/version", json!("2")),
        ("/sources/0/ref", json!("input:other")),
        ("/fields/0/rules/0/name", json!("other")),
        ("/fields/0/rules/0/when", json!({"not":"always"})),
        ("/fields/0/rules/0/then", json!({"literal":4})),
    ] {
        let mut v = rules();
        *v.pointer_mut(path).unwrap() = change;
        assert_ne!(
            ExtractRules::parse(&bytes(&v)).unwrap().id().unwrap(),
            base,
            "{path}"
        );
    }
}

#[test]
fn fallback_accepts_empty_terms_in_all_three_forms() {
    let mut v = rules();
    v["fields"][0]["rules"][0]["when"] = json!({"all":[
        {"contains_any":{"source":"text","terms":[]}},
        {"contains_all":{"source":"text","terms":[]}}
    ]});
    v["fields"][0]["rules"][0]["then"]["integer_after"]["terms"] = json!([]);
    ExtractRules::parse(&bytes(&v)).expect("document-local validation accepts empty terms");
}

#[test]
fn fallback_refuses_each_local_bound_and_bad_source_syntax() {
    let r = rules();
    let field = r["fields"][0].clone();
    let rule = field["rules"][0].clone();
    for (path, bad) in [
        ("/fields", json!(vec![field.clone(); 33])),
        ("/fields/0/rules", json!(vec![rule.clone(); 129])),
        (
            "/fields/0/rules/0/when",
            json!({"contains_any":{"source":"text","terms":vec!["x";65]}}),
        ),
        (
            "/fields/0/rules/0/when",
            json!({"contains_all":{"source":"text","terms":[""]}}),
        ),
        (
            "/fields/0/rules/0/when",
            json!({"contains_any":{"source":"text","terms":["é".repeat(33)]}}),
        ),
        ("/fields/0/rules/0/then/integer_after/terms", json!([""])),
        (
            "/fields/0/rules/0/then/integer_after/terms",
            json!(vec!["x"; 65]),
        ),
        ("/fields/0/rules/0/then/integer_after/max_digits", json!(0)),
        ("/fields/0/rules/0/then/integer_after/max_digits", json!(19)),
        (
            "/fields/0/rules/0/then",
            json!({"date_iso":{"source":"text","occurrence":0}}),
        ),
        (
            "/fields/0/rules/0/then",
            json!({"record":{"date":{"date_iso":{"source":"text","occurrence":3}}}}),
        ),
    ] {
        let mut v = r.clone();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(ExtractRules::parse(&bytes(&v)).is_err(), "{path}: {v}");
    }
    for bad in [
        "step:text",
        "input:",
        "input:x/",
        "input:x/y/z",
        "input:x//y",
        "input:x:y",
        "input:a b",
    ] {
        let mut v = r.clone();
        v["sources"][0]["ref"] = json!(bad);
        assert!(ExtractRules::parse(&bytes(&v)).is_err(), "{bad}");
    }
    let mut condition = json!("always");
    for _ in 0..8 {
        condition = json!({"not":condition});
    }
    let mut v = r.clone();
    v["fields"][0]["rules"][0]["when"] = condition;
    let error = ExtractRules::parse(&bytes(&v)).unwrap_err().to_string();
    assert!(error.contains("condition depth exceeds 8"), "{error}");
    // Local maxima are accepted. Definition-dependent duplicate list literals
    // and type compatibility deliberately remain compiler checks.
    let mut v = r.clone();
    v["fields"] = json!(vec![field; 32]);
    assert!(ExtractRules::parse(&bytes(&v)).is_ok());
    let mut v = r.clone();
    v["fields"][0]["rules"] = json!(vec![rule; 128]);
    assert!(ExtractRules::parse(&bytes(&v)).is_ok());
    for condition in [
        json!({"contains_all":{"source":"text","terms":vec!["é".repeat(32);64]}}),
        json!({"all":["always",{"any":["always"]}]}),
        (0..7).fold(json!("always"), |c, _| json!({"not":c})),
    ] {
        let mut v = r.clone();
        v["fields"][0]["rules"][0]["when"] = condition;
        assert!(ExtractRules::parse(&bytes(&v)).is_ok());
    }
    for value in [
        json!({"literal":["pool"]}),
        json!({"date_iso":{"source":"text","occurrence":2}}),
        json!({"record":{"party":{"literal":4}}}),
    ] {
        let mut v = r.clone();
        v["fields"][0]["rules"][0]["then"] = value;
        roundtrip::<ExtractRules>(&v);
    }
}

#[test]
fn record_value_refuses_record_subfields() {
    let mut v = rules();
    v["fields"][0]["rules"][0]["then"] =
        json!({"record":{"outer":{"record":{"inner":{"literal":4}}}}});
    let error = ExtractRules::parse(&bytes(&v)).unwrap_err().to_string();
    assert!(
        error.contains("a record value's subfields are scalar"),
        "{error}"
    );
}

#[test]
fn deeply_nested_record_document_is_refused_without_stack_overflow() {
    // Build bytes directly: recursive JSON values would also recurse on drop.
    let mut wire = String::from(
        r#"{"schema":"rustev.extract-rules/1","name":"deep","version":"1","sources":[],"fields":[{"field":"deep","rules":[{"name":"deep","when":"always","then":"#,
    );
    for _ in 0..10_000 {
        wire.push_str(r#"{"record":{"child":"#);
    }
    wire.push_str(r#"{"literal":4}"#);
    for _ in 0..10_000 {
        wire.push_str("}}");
    }
    wire.push_str("}]}]}");
    // The bounded parser rejects this before recursive deserialization.
    assert!(ExtractRules::parse(wire.as_bytes()).is_err());
}

#[test]
fn flat_record_value_is_accepted() {
    let mut v = rules();
    v["fields"][0]["rules"][0]["then"] = json!({"record":{
        "enabled":{"literal":true},
        "party":{"integer_after":{"source":"text","terms":["party "],"max_digits":18}},
        "date":{"date_iso":{"source":"text","occurrence":2}},
        "area":{"literal":"quiet"}
    }});
    roundtrip::<ExtractRules>(&v);
}

#[test]
fn descriptor_bounds_and_operation_pairing() {
    for (path, bad) in [
        ("/operations/0/output/typed_record/max_options", json!(0)),
        ("/operations/0/output/typed_record/max_options", json!(33)),
        (
            "/operations/0/output/typed_record/max_output_bytes",
            json!(0),
        ),
        ("/operations/0/output/typed_record/structure", json!("text")),
        (
            "/operations/0/output/typed_record/option_logprobs",
            json!("true"),
        ),
        ("/operations/0/operation", json!("classify")),
        ("/operations/0/output", json!("label")),
    ] {
        let mut v = descriptor();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(BackendDescriptor::parse(&bytes(&v)).is_err(), "{path}");
    }
    for structure in ["json_schema", "tool_call", "grammar"] {
        let mut v = descriptor();
        v["operations"][0]["output"]["typed_record"]["structure"] = json!(structure);
        v["operations"][0]["output"]["typed_record"]["max_options"] = json!(1);
        roundtrip::<BackendDescriptor>(&v);
    }
}

#[test]
fn logprob_item_roundtrips_scalar_bytes_and_refuses_external_tag() {
    for (wire, expected) in [
        (
            r#"{"field":"f","item":"none","values":{"a":-1.0}}"#,
            LogprobItem::None,
        ),
        (
            r#"{"field":"f","item":3,"values":{"a":-1.0}}"#,
            LogprobItem::Index(3),
        ),
    ] {
        let logprobs: FieldLogprobs = serde_json::from_slice(wire.as_bytes()).unwrap();
        assert_eq!(logprobs.item, expected);
        assert_eq!(serde_json::to_vec(&logprobs).unwrap(), wire.as_bytes());
    }
    let tagged = r#"{"field":"f","item":{"index":3},"values":{"a":-1.0}}"#;
    assert!(serde_json::from_slice::<FieldLogprobs>(tagged.as_bytes()).is_err());
}

#[test]
fn output_refuses_record_local_faults_but_keeps_raw_values() {
    for (path, bad) in [
        ("/output/typed_record/fields", json!([])),
        ("/output/typed_record/logprobs/0/item", json!(null)),
        ("/output/typed_record/logprobs/0/item", json!(-1)),
        ("/output/typed_record/logprobs/0/item", json!(0.5)),
        ("/output/typed_record/logprobs/0/values/quiet", json!(null)),
        (
            "/output/typed_record/provider_reported/area",
            json!("é".repeat(129)),
        ),
    ] {
        let mut v = output();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(BackendOutputDoc::parse(&bytes(&v)).is_err(), "{path}");
    }
    let mut v = output();
    v["output"]["typed_record"]["provider_reported"]["area"] = json!("é".repeat(128));
    roundtrip::<BackendOutputDoc>(&v);
    for nonfinite in ["1e400", "NaN", "Infinity", "-Infinity"] {
        let wire = String::from_utf8(bytes(&output()))
            .unwrap()
            .replace("-0.25", nonfinite);
        assert!(
            BackendOutputDoc::parse(wire.as_bytes()).is_err(),
            "{nonfinite}"
        );
    }
}

fn envelope(v: &Value) -> RequestDoc {
    let op = ExtractionRequestOperation::Extract {
        plan_id: rustev_contract::ids::PlanId::parse(&digest()).unwrap(),
        request: ExtractRequest::parse(&bytes(v)).unwrap(),
    };
    serde_json::from_value(json!({"schema":"rustev.request/1",
        "scope":{"tenant":"tenant","context_revision":"rev","mode":"tenant_only"},
        "backend_id":"extractor","artifact":digest(),"descriptor":digest(),
        "projection":op.projection().unwrap(),"output":descriptor()["operations"][0]["output"],
        "requires":"typed_record","normalization":"none"}))
    .unwrap()
}

#[test]
fn extract_request_id_covers_every_rendered_member_and_option_order() {
    let mut base = envelope(&request());
    let original = base.id().unwrap();
    roundtrip::<RequestDoc>(&serde_json::to_value(&base).unwrap());
    for (path, change) in [
        ("/step", json!("other")),
        ("/template", json!("other")),
        ("/template_id", json!(format!("sha256:{}", "b".repeat(64)))),
        ("/instructions", json!("Other instruction")),
        ("/projection/input:text", json!("busy")),
        ("/vocabularies/areas", json!(["busy", "quiet"])),
        ("/output/fields/0/name", json!("other")),
    ] {
        let mut v = request();
        *v.pointer_mut(path).unwrap() = change;
        let schema = serde_json::from_value(v["output"].clone()).unwrap();
        v["schema_digest"] = json!(extraction_schema_digest(&schema).unwrap());
        assert_ne!(envelope(&v).id().unwrap(), original, "{path}");
    }
    let mut v = request();
    v["schema_digest"] = json!(digest());
    assert!(ExtractRequest::parse(&bytes(&v)).is_err());
    let projection: Value = serde_json::from_str(&base.projection).unwrap();
    assert_eq!(projection["operation"], "extract");
    assert_eq!(
        bytes(&serde_json::from_str::<Value>(&base.projection).unwrap()),
        bytes(&projection)
    );
    base.projection = canonical_value_bytes(&projection)
        .map(|b| String::from_utf8(b).unwrap())
        .unwrap();
    assert_eq!(base.id().unwrap(), original);
}

#[test]
fn documents_refuse_unknown_members_duplicate_keys_and_wrong_versions() {
    fn check<T: Document>(v: Value) {
        let mut bad = v.clone();
        bad["extra"] = json!(true);
        assert!(T::parse(&bytes(&bad)).is_err());
        let wire = String::from_utf8(bytes(&v)).unwrap();
        let duplicate = format!("{{\"schema\":\"{}\",{}", T::SCHEMA, &wire[1..]);
        assert!(T::parse(duplicate.as_bytes()).is_err());
        let mut bad = v;
        bad["schema"] = json!("unknown/1");
        assert!(T::parse(&bytes(&bad)).is_err());
    }
    check::<ExtractRules>(rules());
    check::<ExtractRequest>(request());
    check::<BackendDescriptor>(descriptor());
    check::<BackendOutputDoc>(output());
    let mut v = rules();
    v["fields"][0]["rules"][0]["then"]["integer_after"]["extra"] = json!(true);
    assert!(ExtractRules::parse(&bytes(&v)).is_err());
    let wire = String::from_utf8(bytes(&rules()))
        .unwrap()
        .replace("\"max_digits\":18", "\"max_digits\":18,\"max_digits\":18");
    assert!(ExtractRules::parse(wire.as_bytes()).is_err());
    let wire = String::from_utf8(bytes(&output()))
        .unwrap()
        .replace("\"quiet\":-0.25", "\"quiet\":-0.25,\"quiet\":-0.25");
    assert!(BackendOutputDoc::parse(wire.as_bytes()).is_err());
}

#[test]
fn legacy_request_descriptor_and_output_keep_pinned_bytes_and_ids() {
    // Literal goldens use only the pre-038 vocabulary. Hashes are pinned
    // independently of the document serializers under test.
    let request = r#"{"artifact":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","backend_id":"old","descriptor":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","normalization":"softmax/1","output":"logits","projection":"{\"operation\":\"classify\",\"options\":[\"x\",\"y\"]}","requires":"distribution","schema":"rustev.request/1","scope":{"context_revision":"rev","mode":"tenant_only","tenant":"tenant"}}"#;
    let descriptor = r#"{"artifact":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","backend_id":"old","determinism":"unspecified","input_limit":{"max_bytes":4096,"on_excess":"refuse"},"operations":[{"max_options":32,"operation":"generate","output":{"text_record":{"languages":["en"],"max_output_bytes":8192,"streaming":false,"structure":"json_schema"}}}],"schema":"rustev.backend/1"}"#;
    let output = r#"{"artifact":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","instance":[],"output":{"text_record":{"title":{"text":"legacy"}}},"schema":"rustev.backend-output/1","step":"text"}"#;
    let r = RequestDoc::parse(request.as_bytes()).unwrap();
    let d = BackendDescriptor::parse(descriptor.as_bytes()).unwrap();
    let o = BackendOutputDoc::parse(output.as_bytes()).unwrap();
    assert_eq!(r.canonical().unwrap(), request.as_bytes());
    assert_eq!(d.canonical().unwrap(), descriptor.as_bytes());
    assert_eq!(o.record_canonical().unwrap(), output.as_bytes());
    assert_eq!(
        r.id().unwrap().as_str(),
        "sha256:f639c515a2b72cf933af5bda8495e4ff9d8709cbbb401bcb1fa616b7c3fbf019"
    );
    assert_eq!(
        d.id().unwrap().as_str(),
        "sha256:dc5893200b0b37678827eeb7cdf2f156841e58ae546b59f8b4d2c708027e394d"
    );
    assert_eq!(
        o.record_digest().unwrap().as_str(),
        "sha256:9d9544cad292a9f67eeed4d9e23d815a57b0a7148e059c627b536128a45e9691"
    );
}
