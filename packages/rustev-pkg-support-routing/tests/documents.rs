//! Spec 007 3.1.3, 3.1.4 and 3.2: the embedded documents are exactly what
//! the builder and the generator emit, and the definition document is
//! byte-identical to spec 002's golden. `RUSTEV_BLESS=1` rewrites `data/`.

mod common;

use common::*;
use rustev_contract::Document;
use rustev_contract::definition::Definition;
use rustev_contract::eval_report::DatasetProvenance;
use rustev_contract::snapshot::Snapshot;
use rustev_eval::dataset::DatasetManifest;
use rustev_pkg_support_routing as pkg;

#[test]
fn the_data_documents_are_what_the_generator_emits() {
    let docs = data_documents();
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        for (path, bytes) in &docs {
            let p = data_dir().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, bytes).unwrap();
        }
        return;
    }
    for (path, bytes) in &docs {
        let committed = std::fs::read(data_dir().join(path))
            .unwrap_or_else(|_| panic!("missing data/{path}; run with RUSTEV_BLESS=1"));
        assert!(
            committed == *bytes,
            "data/{path} differs from the generator"
        );
    }
    let embedded: Vec<(&str, &[u8])> = vec![
        ("support-routing.definition.json", pkg::DEFINITION),
        (
            "support-routing.topic-calibration.json",
            pkg::REFERENCE_TOPIC_CALIBRATION,
        ),
        ("queue.adapter.json", pkg::QUEUE.adapter),
        ("queue.config.json", pkg::QUEUE.config),
        ("queue.dataset.json", pkg::QUEUE.dataset),
        ("priority.adapter.json", pkg::PRIORITY.adapter),
        ("priority.config.json", pkg::PRIORITY.config),
        ("priority.dataset.json", pkg::PRIORITY.dataset),
    ];
    for (path, bytes) in embedded {
        let generated = &docs.iter().find(|(p, _)| p == path).unwrap().1;
        assert!(generated == bytes, "{path} is not the embedded copy");
    }
    let ids: Vec<&str> = CASES.iter().map(|c| c.id).collect();
    assert_eq!(pkg::CASES, &ids[..]);
    for c in &CASES {
        let s = Snapshot::parse(pkg::snapshot(c.id).unwrap()).unwrap();
        assert_eq!(s, snapshot_of(c));
    }
    assert!(pkg::snapshot("sr-unknown").is_none());
}

#[test]
fn the_definition_document_is_spec_002s_golden_byte_for_byte() {
    assert!(
        pkg::DEFINITION == core_golden_definition().as_slice(),
        "the package's definition differs from crates/rustev-core/tests/golden/support-routing.definition.json"
    );
}

#[test]
fn the_builder_and_the_parsed_document_are_one_representation() {
    let built = pkg::definition(&pkg::Params::reference().unwrap()).unwrap();
    let parsed = Definition::parse(pkg::DEFINITION).unwrap();
    assert_eq!(built, parsed);
    assert_eq!(built.canonical().unwrap(), pkg::DEFINITION);
}

#[test]
fn a_changed_threshold_changes_the_definition_bytes() {
    let mut p = pkg::Params::reference().unwrap();
    p.topic_threshold = dec("0.65");
    assert_ne!(
        pkg::definition(&p).unwrap().canonical().unwrap(),
        pkg::DEFINITION
    );
}

#[test]
fn both_datasets_are_synthetic_with_one_split_membership() {
    let q = DatasetManifest::parse(pkg::QUEUE.dataset).unwrap();
    let p = DatasetManifest::parse(pkg::PRIORITY.dataset).unwrap();
    for d in [&q, &p] {
        assert!(matches!(d.provenance, DatasetProvenance::Synthetic { .. }));
    }
    let key = |d: &DatasetManifest| {
        d.cases
            .iter()
            .map(|c| (c.id.clone(), c.snapshot.clone(), c.split))
            .collect::<Vec<_>>()
    };
    assert_eq!(key(&q), key(&p));
}
