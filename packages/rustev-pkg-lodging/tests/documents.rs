//! Canonical document and identity checks for spec 008 sections 3.5 and 4.1.

mod common;

use common::*;
use rustev_contract::Document;
use rustev_contract::definition::Definition;
use rustev_contract::eval_report::DatasetProvenance;
use rustev_contract::snapshot::Snapshot;
use rustev_eval::dataset::DatasetManifest;
use rustev_pkg_lodging as pkg;

#[test]
fn data_documents_are_generated_canonical_documents() {
    let docs = data_documents();
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        for (path, bytes) in docs {
            let target = data_dir().join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, bytes).unwrap();
        }
        return;
    }
    for (path, bytes) in &docs {
        assert_eq!(
            std::fs::read(data_dir().join(path)).unwrap(),
            *bytes,
            "data/{path} differs from its generator"
        );
    }
    for case in &CASES {
        assert_eq!(
            Snapshot::parse(pkg::snapshot(case.id).unwrap()).unwrap(),
            snapshot_of(case)
        );
    }
    assert!(pkg::snapshot("unknown").is_none());
}

#[test]
fn definition_matches_the_existing_core_golden_byte_for_byte() {
    assert_eq!(pkg::DEFINITION, core_golden_definition());
}

#[test]
fn builder_and_document_are_one_representation() {
    let built = pkg::definition(&pkg::Params::reference()).unwrap();
    let parsed = Definition::parse(pkg::DEFINITION).unwrap();
    assert_eq!(built, parsed);
    assert_eq!(built.canonical().unwrap(), pkg::DEFINITION);
}

#[test]
fn represented_parameter_changes_change_definition_identity() {
    let reference = pkg::Params::reference();
    let baseline = pkg::definition(&reference).unwrap().canonical().unwrap();
    let mut variants = vec![];

    let mut p = reference.clone();
    p.shortlist_size = 4;
    variants.push(p);
    let mut p = reference.clone();
    p.max_semantic_requests = 127;
    variants.push(p);
    let mut p = reference;
    p.preference_weight = dec("0.4");
    p.price_weight = dec("0.3");
    variants.push(p);

    for variant in variants {
        assert_ne!(
            pkg::definition(&variant).unwrap().canonical().unwrap(),
            baseline
        );
    }
}

#[test]
fn evaluation_documents_bind_synthetic_mechanics() {
    let manifest = DatasetManifest::parse(pkg::DATASET).unwrap();
    assert!(matches!(
        manifest.provenance,
        DatasetProvenance::Synthetic { .. }
    ));
    assert_eq!(manifest.cases.len(), pkg::CASES.len());
    assert_eq!(pkg::ADAPTER, &data_documents()[1].1);
    assert_eq!(pkg::CONFIG, &data_documents()[2].1);
}
