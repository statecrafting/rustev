//! Spec 041, 3.1.1, 3.1.2 and 3.4: `rustev.dataset/2` with gold records,
//! and the measure declarations of `rustev.task-adapter/2`.

use std::collections::BTreeMap;

use rustev_contract::canonical::tagged_digest;
use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::SnapshotId;
use rustev_contract::judgment::OutValue;
use rustev_contract::{Document, Identified};
use rustev_eval::config::AdapterRules;
use rustev_eval::dataset::{
    AdapterRef, DATASET, DATASET_V2, DatasetCase, DatasetError, DatasetManifest, Label,
};
use rustev_eval::gold::Gold;
use rustev_eval::measures::{
    Against, ConstraintAggregate, ConstraintTest, MeasuredAdapter, Measures, TASK_ADAPTER_V2,
};
use rustev_eval::metrics::TaskAdapter;
use serde_json::{Value, json};

fn snapshot(i: u8) -> SnapshotId {
    SnapshotId::parse(&format!("sha256:{}", format!("{i:02x}").repeat(32))).unwrap()
}

fn manifest(schema: &str, gold: Option<Gold>) -> DatasetManifest {
    DatasetManifest {
        schema: schema.into(),
        name: "gold synthetic".into(),
        provenance: DatasetProvenance::Synthetic {
            generator: "hand-written".into(),
        },
        adapter: AdapterRef {
            name: "lodging".into(),
            version: "1".into(),
        },
        cases: vec![DatasetCase {
            id: "case-1".into(),
            source: "src-1".into(),
            snapshot: snapshot(1),
            split: Split::FinalTest,
            label: Label::Value("a".into()),
            subgroups: vec![],
            gold,
        }],
    }
}

fn gold(v: Value) -> Gold {
    serde_json::from_value(v).unwrap()
}

#[test]
fn version_1_keeps_its_bytes_and_identity() {
    let v1 = manifest(DATASET, None);
    let bytes = serde_json::to_vec(&v1).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("\"gold\""));
    let parsed = DatasetManifest::parse(&bytes).unwrap();
    assert_eq!(parsed, v1);
    let id = v1.id().unwrap();
    assert_eq!(
        id.to_string(),
        tagged_digest(DATASET, &v1.canonical().unwrap())
    );
    v1.check_structure().unwrap();
    // The same cases under version 2 are another document.
    let v2 = manifest(DATASET_V2, None);
    v2.check_structure().unwrap();
    assert_eq!(
        v2.id().unwrap().to_string(),
        tagged_digest(DATASET_V2, &v2.canonical().unwrap())
    );
    assert_ne!(v2.id().unwrap(), id);
}

#[test]
fn version_2_carries_gold_into_its_identity() {
    let g = gold(json!({"relevance": {"a": 3}, "fields": {"phone": "absent"}}));
    let with = manifest(DATASET_V2, Some(g.clone()));
    with.check_structure().unwrap();
    let bytes = serde_json::to_vec(&with).unwrap();
    assert_eq!(DatasetManifest::parse(&bytes).unwrap(), with);
    let other = manifest(DATASET_V2, Some(gold(json!({"relevance": {"a": 2}}))));
    assert_ne!(with.id().unwrap(), other.id().unwrap());
    assert_ne!(with.id().unwrap(), manifest(DATASET_V2, None).id().unwrap());
}

#[test]
fn gold_is_refused_where_it_does_not_belong() {
    let g = gold(json!({"relevance": {"a": 3}}));
    let Err(DatasetError::Gold { case, .. }) = manifest(DATASET, Some(g)).check_structure() else {
        panic!("gold in a version 1 manifest is refused");
    };
    assert_eq!(case, "case-1");
    // A malformed gold record refuses the manifest, naming the case (3.3.2).
    let bad = gold(json!({"relevance": {"a": 11}}));
    assert!(matches!(
        manifest(DATASET_V2, Some(bad)).check_structure(),
        Err(DatasetError::Gold { case, .. }) if case == "case-1"
    ));
    // Unknown members and unknown schemas are refused at parse.
    let mut v = serde_json::to_value(manifest(DATASET_V2, None)).unwrap();
    v["cases"][0]["gold"] = json!({"labels": {}});
    assert!(DatasetManifest::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    v["cases"][0]["gold"] = json!({});
    v["schema"] = json!("rustev.dataset/3");
    assert!(DatasetManifest::parse(&serde_json::to_vec(&v).unwrap()).is_err());
}

fn measures_json() -> Value {
    json!({
        "rankings": [{
            "name": "offers",
            "param": "shortlist",
            "relevant_at_least": 2,
            "gain": "exponential",
            "metrics": ["precision@5", "ndcg@10", "mrr"],
            "backfill_positions": 3
        }],
        "constraints": [
            {"name": "city", "ranking": "offers", "k": 5, "attribute": "city",
             "test": {"path_prefix": 2}, "against": {"requested": "city"},
             "aggregate": "all_at_k"},
            {"name": "pets", "ranking": "offers", "k": 3, "attribute": "pets",
             "test": "equals", "against": {"value": true}, "aggregate": "share_at_k"},
            {"name": "brand", "ranking": "offers", "k": 3, "attribute": "brand",
             "test": "member_of", "against": {"param": "brands"}, "aggregate": "share_at_k"}
        ],
        "fields": [{"name": "phone", "step": "extract_contact", "field": "phone"}]
    })
}

fn measures(v: Value) -> Measures {
    serde_json::from_value(v).unwrap()
}

#[test]
fn measure_declarations_parse_and_round_trip() {
    let m = measures(measures_json());
    m.check().unwrap();
    assert_eq!(m.constraints[0].test, ConstraintTest::PathPrefix(2));
    assert_eq!(m.constraints[0].against, Against::Requested("city".into()));
    assert_eq!(m.constraints[0].aggregate, ConstraintAggregate::AllAtK);
    assert_eq!(serde_json::to_value(&m).unwrap(), measures_json());
    for bad in [
        json!({"rankings": [], "constraints": [], "fields": [], "extra": []}),
        json!({"rankings": [], "constraints": []}),
    ] {
        assert!(serde_json::from_value::<Measures>(bad).is_err());
    }
    let mut v = measures_json();
    v["rankings"][0]["metrics"] = json!(["precision@0"]);
    assert!(serde_json::from_value::<Measures>(v).is_err());
}

#[test]
fn the_measures_digest_covers_every_declaration() {
    let m = measures(measures_json());
    let d = m.digest().unwrap();
    assert_eq!(
        d.to_string(),
        tagged_digest(
            TASK_ADAPTER_V2,
            &rustev_contract::canonical::canonical_bytes(&m).unwrap()
        )
    );
    assert_eq!(measures(measures_json()).digest().unwrap(), d);
    let mut v = measures_json();
    v["constraints"][1]["k"] = json!(4);
    assert_ne!(measures(v).digest().unwrap(), d);
    let mut v = measures_json();
    v["fields"] = json!([]);
    assert_ne!(measures(v).digest().unwrap(), d);
}

#[test]
fn measure_declarations_are_bounded() {
    let refused = |edit: &dyn Fn(&mut Value)| {
        let mut v = measures_json();
        edit(&mut v);
        measures(v).check().unwrap_err()
    };
    refused(&|v| {
        let r = v["rankings"][0].clone();
        v["rankings"] = Value::Array(
            (0..17)
                .map(|i| {
                    let mut r = r.clone();
                    r["name"] = json!(format!("r{i}"));
                    r
                })
                .collect(),
        );
    });
    refused(&|v| v["fields"][0]["name"] = json!("bad name"));
    refused(&|v| v["fields"][0]["name"] = json!("x".repeat(65)));
    refused(&|v| v["constraints"][1]["name"] = json!("city"));
    refused(&|v| v["rankings"][0]["relevant_at_least"] = json!(0));
    refused(&|v| v["rankings"][0]["relevant_at_least"] = json!(11));
    refused(&|v| v["rankings"][0]["metrics"] = json!(["mrr", "mrr"]));
    refused(&|v| v["rankings"][0]["backfill_positions"] = json!(101));
    refused(&|v| v["rankings"][0]["param"] = json!(""));
    refused(&|v| v["constraints"][0]["ranking"] = json!("other"));
    refused(&|v| v["constraints"][0]["k"] = json!(0));
    refused(&|v| v["constraints"][0]["k"] = json!(101));
    refused(&|v| v["constraints"][0]["test"] = json!({"path_prefix": 0}));
    refused(&|v| v["constraints"][0]["test"] = json!({"path_prefix": 17}));
    refused(&|v| v["constraints"][1]["against"] = json!({"value": []}));
    refused(&|v| v["constraints"][2]["against"] = json!({"param": ""}));
}

struct Lodging;

impl TaskAdapter for Lodging {
    fn adapter(&self) -> AdapterRef {
        AdapterRef {
            name: "lodging".into(),
            version: "1".into(),
        }
    }
    fn rules(&self) -> AdapterRules {
        AdapterRules::Opaque
    }
    fn check_label(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn correct(&self, _: &str, _: &BTreeMap<String, OutValue>, _: &str) -> bool {
        false
    }
}

impl MeasuredAdapter for Lodging {
    fn measures(&self) -> Measures {
        measures(measures_json())
    }
}

#[test]
fn a_library_adapter_supplies_declarations_not_a_digest() {
    let a: &dyn MeasuredAdapter = &Lodging;
    assert_eq!(
        a.measures().digest().unwrap(),
        measures(measures_json()).digest().unwrap()
    );
}
