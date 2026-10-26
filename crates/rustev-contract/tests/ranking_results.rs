//! Spec 039 increment 2: wire members, bounds, versions and compatibility.
use rustev_contract::canonical::{record_canonical_bytes, tagged_digest};
use rustev_contract::evidence::EvidenceRecord;
use rustev_contract::judgment::{Judgment, OutValue};
use rustev_contract::output::{BackendOutputDoc, RawOutput};
use rustev_contract::plan::{Plan, PlanStepDetail};
use rustev_contract::ranking_result::*;
use rustev_contract::{Document, Identified};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn rt<T: DeserializeOwned + Serialize>(v: Value) {
    let bytes = record_canonical_bytes(&v).unwrap();
    let parsed: T = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        record_canonical_bytes(&serde_json::to_value(parsed).unwrap()).unwrap(),
        bytes
    );
}
fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}
fn score() -> Value {
    json!({"candidate":"a","plan":digest(),"step":"rank","instance":["perm/0"],"value":1.5})
}
fn rank() -> Value {
    json!({"mode":{"listwise":{"scores":"optional"}},"pool":["a","b"],
        "entries":[{"candidate":"a","position":1,"score":score(),"wins":null,"points":null}],
        "unranked":["b"],"dropped":[{"id":"x","reason":"unknown_id"}],
        "partial":true,"score_disagreements":0,"pairwise":null})
}
fn failure() -> Value {
    json!({"backend_unavailable":{"detail":"unavailable"}})
}
fn selection() -> Value {
    json!({"k":2,"entries":[
        {"position":1,"id":"a","origin":"model","source_position":1,"derivation":"model-derived","lineage":{"inputs":[],"steps":["rank"]}},
        {"position":2,"id":"b","origin":"backfill","source_position":3,"derivation":"exact-derived","lineage":{"inputs":[],"steps":["exact"]}}
    ],"shortfall":0,"model":{"ranked":{"entries":1,"partial":true}},
    "backfill":{"used":{"taken":1,"skipped_selected":1,"skipped_outside_pool":1}}})
}
fn binding() -> Value {
    json!({"rank":{"backend_id":"ranker","artifact":digest(),"descriptor":digest(),
        "mode":{"listwise":{"scores":"optional"}},"bias":"none","generator":"rustev.permute/1",
        "tie_break":"id_ascending","pool_bound":2,"max_requests":1}})
}
fn select_detail() -> Value {
    json!({"select":{"from":"step:rank","k":2,"backfill":{"from":"step:exact","exclude_selected":true},"on_backend_failure":"backfill_all"}})
}
fn evidence() -> Value {
    json!({"ranking":rank(),"requests":[{"presented":["b","a"],"scores":[score()]}],"comparisons":[]})
}
fn golden(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rustev-core/tests/golden")
        .join(name);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
#[test]
fn plan_details_roundtrip_and_require_plan_three() {
    for detail in [binding(), select_detail()] {
        rt::<PlanStepDetail>(detail.clone());
        let mut plan = golden("support-routing.plan.json");
        plan["schema"] = json!("rustev.plan/3");
        plan["steps"][0]["detail"] = detail;
        let bytes = record_canonical_bytes(&plan).unwrap();
        let parsed = Plan::parse(&bytes).unwrap();
        assert_eq!(parsed.canonical().unwrap(), bytes);
        plan["schema"] = json!("rustev.plan/2");
        assert!(Plan::parse(&record_canonical_bytes(&plan).unwrap()).is_err());
    }
    for (field, changed) in [
        ("backend_id", json!("other")),
        ("pool_bound", json!(3)),
        ("max_requests", json!(2)),
        ("bias", json!({"shuffle":{"seed":"other"}})),
        ("tie_break", json!({"order_of":"step:exact"})),
        ("mode", json!("pointwise")),
        ("artifact", json!(format!("sha256:{}", "b".repeat(64)))),
        ("descriptor", json!(format!("sha256:{}", "c".repeat(64)))),
    ] {
        let mut p = golden("support-routing.plan.json");
        p["schema"] = json!("rustev.plan/3");
        p["steps"][0]["detail"] = binding();
        let first = Plan::parse(&record_canonical_bytes(&p).unwrap())
            .unwrap()
            .id()
            .unwrap();
        p["steps"][0]["detail"]["rank"][field] = changed;
        assert_ne!(
            Plan::parse(&record_canonical_bytes(&p).unwrap())
                .unwrap()
                .id()
                .unwrap(),
            first
        );
    }
    let mut b = binding();
    b["rank"]["generator"] = json!("rustev.permute/2");
    assert!(serde_json::from_value::<PlanStepDetail>(b).is_err());
}
#[test]
fn backend_output_roundtrip_and_scores_refused_locally() {
    for scores in [json!("none"), json!([0.5, -1.25])] {
        let output = json!({"ranking":{"ids":["b","a"],"scores":scores}});
        rt::<RawOutput>(output.clone());
        rt::<BackendOutputDoc>(
            json!({"schema":"rustev.backend-output/1","step":"rank","instance":["perm/0"],"artifact":digest(),"output":output}),
        );
    }
    for scores in [json!([]), json!([1]), json!([1, 2, 3])] {
        let output = json!({"ranking":{"ids":["b","a"],"scores":scores}});
        assert!(serde_json::from_value::<RawOutput>(output.clone()).is_err());
        let doc = json!({"schema":"rustev.backend-output/1","step":"rank","instance":[],"artifact":digest(),"output":output});
        assert!(BackendOutputDoc::parse(&record_canonical_bytes(&doc).unwrap()).is_err());
    }
    assert!(
        serde_json::from_str::<RawOutput>(r#"{"ranking":{"ids":["a"],"scores":[1e999]}}"#).is_err()
    );
    rt::<RawOutput>(json!({"scores":{"a":0.75}}));
    rt::<RawOutput>(json!({"label":"first"}));
    rt::<RawOutput>(json!({"label":"second"}));
}
#[test]
fn backend_ranking_ids_and_scores_are_bounded() {
    for scores_present in [false, true] {
        for len in [1024, 1025] {
            let scores = if scores_present {
                json!(vec![0.5; len])
            } else {
                json!("none")
            };
            let output = json!({"ranking":{"ids":vec!["a"; len],"scores":scores}});
            assert_eq!(
                serde_json::from_value::<RawOutput>(output.clone()).is_ok(),
                len == 1024
            );
            let doc = json!({"schema":"rustev.backend-output/1","step":"rank",
                "instance":[],"artifact":digest(),"output":output});
            assert_eq!(
                BackendOutputDoc::parse(&record_canonical_bytes(&doc).unwrap()).is_ok(),
                len == 1024
            );
        }
    }
    let output = json!({"ranking":{"ids":vec!["a"; 1024],"scores":vec![0.5; 1025]}});
    let error = serde_json::from_value::<RawOutput>(output).unwrap_err();
    assert!(error.to_string().contains("ranking output bound exceeded"));
}
#[test]
fn result_variants_roundtrip() {
    rt::<ModelRanking>(rank());
    for reason in [
        json!("unknown_id"),
        json!("outside_pool"),
        json!("duplicate"),
        json!({"failed":{"reason":failure()}}),
    ] {
        rt::<DroppedId>(json!({"id":"x","reason":reason}));
    }
    for cycles in [json!(1), json!("not_observable")] {
        let mut r = rank();
        r["mode"] = json!({"pairwise":{"schedule":"round_robin@1","max_comparisons":3}});
        r["entries"][0]["score"] = Value::Null;
        r["entries"][0]["wins"] = json!(1);
        r["pairwise"] =
            json!({"issued":3,"max_comparisons":3,"cyclic_triples":cycles,"inconsistent_pairs":0});
        rt::<ModelRanking>(r);
    }
    let mut r = rank();
    r["entries"][0]["score"] = Value::Null;
    r["entries"][0]["points"] = json!(2);
    rt::<ModelRanking>(r);
    for backfill in [
        json!("none"),
        json!("not_needed"),
        selection()["backfill"].clone(),
        json!({"unresolved":{"reason":failure()}}),
    ] {
        for model in [
            selection()["model"].clone(),
            json!({"failed":{"reason":failure()}}),
        ] {
            let mut s = selection();
            s["model"] = model;
            s["backfill"] = backfill.clone();
            rt::<Selection>(s.clone());
            rt::<OutValue>(json!({"selection":s}));
        }
    }
    for answer in [
        json!("first"),
        json!("second"),
        json!({"failed":{"reason":failure()}}),
    ] {
        rt::<ComparisonAnswer>(answer);
    }
}
#[test]
fn cycle_count_roundtrips_none_and_bare_integer_and_refuses_external_tag() {
    for (wire, expected) in [
        (r#""not_observable""#, CycleCount::NotObservable),
        ("0", CycleCount::Count(0)),
        ("3", CycleCount::Count(3)),
    ] {
        let parsed: CycleCount = serde_json::from_str(wire).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(serde_json::to_vec(&parsed).unwrap(), wire.as_bytes());
    }
    assert!(serde_json::from_str::<CycleCount>(r#"{"count":3}"#).is_err());
}

#[test]
fn evidence_versions_bounds_and_no_item_records() {
    rt::<RankingEvidence>(evidence());
    rt::<RankingEvidence>(
        json!({"ranking":null,"requests":[{"presented":["a","b"],"scores":[]}],"comparisons":[{"failed":{"reason":failure()}}]}),
    );
    let mut doc = golden("actionability.trip-dropped.evidence.json");
    doc["schema"] = json!("rustev.evidence/2");
    doc["steps"] = json!([
        {"step":"rank","instance":[],"status":{"resolved":{"kind":"model_ranking","derivation":"model-derived"}},"ranking":evidence()},
        {"step":"select","instance":[],"status":{"resolved":{"kind":"selection","derivation":"mixed-derived"}}}
    ]);
    doc["steps"][1]["selection"] = selection();
    let bytes = record_canonical_bytes(&doc).unwrap();
    assert_eq!(
        EvidenceRecord::parse(&bytes)
            .unwrap()
            .record_canonical()
            .unwrap(),
        bytes
    );
    doc["schema"] = json!("rustev.evidence/1");
    assert!(EvidenceRecord::parse(&record_canonical_bytes(&doc).unwrap()).is_err());
    for (path, max, item) in [
        ("/ranking/pool", 1024, json!("a")),
        ("/ranking/unranked", 1024, json!("a")),
        ("/ranking/entries", 1024, rank()["entries"][0].clone()),
        ("/ranking/dropped", 8192, rank()["dropped"][0].clone()),
        ("/requests/0/presented", 1024, json!("a")),
        ("/requests/0/scores", 1024, score()),
        ("/comparisons", 4096, json!("first")),
        (
            "/requests",
            4096,
            json!({"presented":["a","b"],"scores":[]}),
        ),
    ] {
        let mut e = evidence();
        *e.pointer_mut(path).unwrap() = Value::Array(vec![item.clone(); max]);
        assert!(
            serde_json::from_value::<RankingEvidence>(e.clone()).is_ok(),
            "{path}"
        );
        *e.pointer_mut(path).unwrap() = Value::Array(vec![item; max + 1]);
        assert!(
            serde_json::from_value::<RankingEvidence>(e.clone()).is_err(),
            "{path}"
        );
        doc["schema"] = json!("rustev.evidence/2");
        doc["steps"][0]["ranking"] = e;
        assert!(
            EvidenceRecord::parse(&record_canonical_bytes(&doc).unwrap()).is_err(),
            "{path}"
        );
    }
    for field in ["question", "items"] {
        let mut e = evidence();
        e[field] = json!("private content");
        assert!(serde_json::from_value::<RankingEvidence>(e).is_err());
    }
    let mut s = selection();
    s["entries"][0]["position"] = json!(0);
    assert!(serde_json::from_value::<Selection>(s).is_err());
}
#[test]
fn existing_goldens_keep_bytes_and_identities() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rustev-core/tests/golden");
    let mut checked = [0; 4];
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let canonical = record_canonical_bytes(&v).unwrap();
        let schema = v["schema"].as_str().unwrap_or("");
        let actual = match schema {
            "rustev.plan/2" | "rustev.plan/3" => {
                let p = Plan::parse(&canonical).unwrap();
                checked[0] += 1;
                assert_eq!(p.id().unwrap().as_str(), tagged_digest(schema, &canonical));
                p.canonical().unwrap()
            }
            "rustev.backend-output/1" => {
                checked[1] += 1;
                BackendOutputDoc::parse(&canonical)
                    .unwrap()
                    .record_canonical()
                    .unwrap()
            }
            "rustev.evidence/1" | "rustev.evidence/2" => {
                checked[2] += 1;
                EvidenceRecord::parse(&canonical)
                    .unwrap()
                    .record_canonical()
                    .unwrap()
            }
            "rustev.judgment/1" | "rustev.judgment/2" => {
                checked[3] += 1;
                Judgment::parse(&canonical)
                    .unwrap()
                    .record_canonical()
                    .unwrap()
            }
            _ => continue,
        };
        assert_eq!(actual, canonical, "{}", path.display());
        assert_eq!(
            actual,
            bytes.strip_suffix(b"\n").unwrap_or(&bytes),
            "{}",
            path.display()
        );
    }
    assert!(
        checked[0] > 0 && checked[2] > 0 && checked[3] > 0,
        "{checked:?}"
    );
    // Backend documents also have stable bytes when no ranking is present.
    rt::<BackendOutputDoc>(
        json!({"schema":"rustev.backend-output/1","step":"old","instance":[],"artifact":digest(),"output":{"scores":{"a":0.5}}}),
    );
}

#[test]
fn judgment_selection_preserves_each_origin_and_lineage() {
    let mut j = golden("actionability.trip-dropped.judgment.json");
    j["schema"] = json!("rustev.judgment/1");
    j["outcome"] =
        json!({"propose":{"action":"shortlist","params":{"chosen":{"selection":selection()}}}});
    j["notices"] = json!([]);
    let bytes = record_canonical_bytes(&j).unwrap();
    let parsed = Judgment::parse(&bytes).unwrap();
    assert_eq!(parsed.record_canonical().unwrap(), bytes);
    let rustev_contract::judgment::Outcome::Propose { params, .. } = parsed.outcome else {
        panic!("proposal")
    };
    let OutValue::Selection(s) = &params["chosen"] else {
        panic!("selection")
    };
    assert_eq!(s.entries[0].origin, SelectionOrigin::Model);
    assert_eq!(s.entries[1].origin, SelectionOrigin::Backfill);
    assert_eq!(s.entries[1].lineage.steps, ["exact"]);
}

#[test]
fn category_28_is_accepted_without_inventing_25_through_27() {
    use rustev_contract::serve::CompilerRefusal;
    for n in [0, 1, 24, 25, 26, 27, 28, 29] {
        let v = json!({"command":"plan compile","status":"refused","category":n,
            "category_name":"exact_order_source","subject":"step:select","detail":"model source","shortfalls":[]});
        let bytes = record_canonical_bytes(&v).unwrap();
        let result = serde_json::from_slice::<CompilerRefusal>(&bytes);
        assert_eq!(result.is_ok(), matches!(n, 1 | 24 | 28), "{n}");
        if let Ok(parsed) = result {
            assert_eq!(record_canonical_bytes(&parsed).unwrap(), bytes);
        }
    }
}

#[test]
fn ranking_scores_require_none_or_a_nonempty_list() {
    for wire in [
        r#"{"ranking":{"ids":["a"],"scores":"none"}}"#,
        r#"{"ranking":{"ids":["a"],"scores":[0.5]}}"#,
    ] {
        let output: RawOutput = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_vec(&output).unwrap(), wire.as_bytes());
    }
    for wire in [
        r#"{"ranking":{"ids":["a"],"scores":null}}"#,
        r#"{"ranking":{"ids":["a"]}}"#,
        r#"{"ranking":{"ids":[],"scores":[]}}"#,
        r#"{"ranking":{"ids":["a"],"scores":{"list":[0.5]}}}"#,
    ] {
        assert!(serde_json::from_str::<RawOutput>(wire).is_err(), "{wire}");
    }
}
