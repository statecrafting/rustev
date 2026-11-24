//! Spec 073, 3.7: the shared rules-first golden. The input documents under
//! `tests/golden/rules-first/` (a SYNTHETIC, fictional airline confirmation)
//! are the files the CLI test, the Node test and the conformance cases
//! compile; this test pins the plan and `PlanId` the Rust path produces from
//! them, so every surface is compared with the same bytes.
mod common;

use rustev_contract::{
    Document, definition::Definition, descriptor::BackendDescriptor,
    extraction_documents::ExtractRules, generation::PromptTemplate,
};
use rustev_core::compile::compile_with_extraction_documents;

fn read(name: &str) -> Vec<u8> {
    std::fs::read(common::golden_dir().join("rules-first").join(name)).unwrap()
}

struct Inputs {
    definition: Definition,
    descriptor: BackendDescriptor,
    template: PromptTemplate,
    program: ExtractRules,
}

fn inputs() -> Inputs {
    Inputs {
        definition: Definition::parse(&read("definition.json")).unwrap(),
        descriptor: BackendDescriptor::parse(&read("descriptor.json")).unwrap(),
        template: PromptTemplate::parse(&read("template.json")).unwrap(),
        program: ExtractRules::parse(&read("program.json")).unwrap(),
    }
}

#[test]
fn input_documents_are_canonical() {
    // The committed inputs are their own canonical bytes, so a surface that
    // reads them as files compiles exactly what this test compiles.
    let i = inputs();
    assert_eq!(i.definition.canonical().unwrap(), read("definition.json"));
    assert_eq!(i.descriptor.canonical().unwrap(), read("descriptor.json"));
    assert_eq!(i.template.canonical().unwrap(), read("template.json"));
    assert_eq!(i.program.canonical().unwrap(), read("program.json"));
    assert_eq!(i.program.schema, "rustev.extract-rules/2");
}

#[test]
fn rules_first_plan_and_plan_id_are_pinned() {
    let i = inputs();
    let c = compile_with_extraction_documents(
        &i.definition,
        &[i.descriptor],
        &[],
        &[],
        None,
        &[i.template],
        &[i.program],
    )
    .unwrap();
    let text = serde_json::to_string(&c.plan).unwrap();
    assert!(text.contains("\"rules_first\""), "{text}");
    common::golden("rules-first/plan.json", &c.canonical());
    common::golden("rules-first/plan.id", c.id.to_string().as_bytes());
}

#[test]
fn document_order_and_unnamed_documents_do_not_change_the_plan() {
    // Section 5: a program the definition does not name is ignored, and the
    // order documents are supplied in changes nothing.
    let i = inputs();
    let mut other = i.program.clone();
    other.name = "unrelated".into();
    let a = compile_with_extraction_documents(
        &i.definition,
        std::slice::from_ref(&i.descriptor),
        &[],
        &[],
        None,
        std::slice::from_ref(&i.template),
        &[i.program.clone(), other.clone()],
    )
    .unwrap();
    let b = compile_with_extraction_documents(
        &i.definition,
        &[i.descriptor],
        &[],
        &[],
        None,
        &[i.template],
        &[other, i.program],
    )
    .unwrap();
    assert_eq!(a.canonical(), b.canonical());
    assert_eq!(a.id, b.id);
}
