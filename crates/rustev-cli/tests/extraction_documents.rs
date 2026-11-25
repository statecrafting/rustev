//! Spec 073 (amending 006, 3.3 and 3.4): `plan check` and `plan compile`
//! take extraction programs (`--extract-rules`) and prompt templates
//! (`--template`) and compile through `compile_with_extraction_documents`.
//! The inputs are the core's shared rules-first golden (3.7), a SYNTHETIC
//! fictional airline confirmation; the written plan must be the core's
//! golden plan, byte for byte.

mod common;

use std::path::Path;

use common::*;
use serde_json::Value as Json;

const INPUTS: [&str; 4] = [
    "definition.json",
    "descriptor.json",
    "program.json",
    "template.json",
];

fn core_golden(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rustev-core/tests/golden/rules-first")
            .join(name),
    )
    .unwrap()
}

fn scratch(name: &str) -> Scratch {
    let s = Scratch::new(name);
    for f in INPUTS {
        s.write(f, &core_golden(f));
    }
    s
}

fn compile_args<'a>(tail: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec![
        "plan",
        "compile",
        "--definition",
        "definition.json",
        "--descriptor",
        "descriptor.json",
    ];
    v.extend_from_slice(tail);
    v
}

fn check_args<'a>(tail: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec![
        "plan",
        "check",
        "--definition",
        "definition.json",
        "--descriptor",
        "descriptor.json",
    ];
    v.extend_from_slice(tail);
    v
}

const DOCS: [&str; 4] = [
    "--extract-rules",
    "program.json",
    "--template",
    "template.json",
];

#[test]
fn the_shared_rules_first_golden_compiles_to_the_core_plan() {
    // 3.3.3 and I-1: the same bytes and PlanId as the Rust path.
    let s = scratch("xdoc-compile");
    let mut args = compile_args(&DOCS);
    args.extend(["--out", "plan.json"]);
    let r = s.rustev(&args);
    let j = r.expect(0, "compiled");
    cli_golden("extraction-documents.compile.json", &r.stdout);
    let id = String::from_utf8(core_golden("plan.id")).unwrap();
    assert_eq!(j["plan_id"], Json::String(id.clone()));
    assert_eq!(s.read("plan.json"), core_golden("plan.json"));

    let r = s.rustev(&check_args(&DOCS));
    let c = r.expect(0, "ok");
    cli_golden("extraction-documents.check.json", &r.stdout);
    assert_eq!(c["plan_id"], Json::String(id));

    // Section 5: document order changes nothing.
    let r = s.rustev(&check_args(&[
        "--template",
        "template.json",
        "--extract-rules",
        "program.json",
    ]));
    assert_eq!(r.expect(0, "ok")["plan_id"], c["plan_id"]);
}

#[test]
fn a_definition_naming_a_program_not_supplied_is_the_cores_refusal() {
    // Section 5: as the Rust path, `invalid_extraction`.
    let s = scratch("xdoc-no-program");
    let r = s.rustev(&check_args(&["--template", "template.json"]));
    let j = r.expect(5, "refused");
    cli_golden("extraction-documents.no-program.json", &r.stdout);
    assert_eq!(j["category_name"], "invalid_extraction");
    assert_eq!(j["subject"], "step:read_confirmation");
}

#[test]
fn a_missing_template_is_the_cores_refusal() {
    let s = scratch("xdoc-no-template");
    let r = s.rustev(&check_args(&["--extract-rules", "program.json"]));
    let j = r.expect(5, "refused");
    assert_eq!(j["subject"], "step:read_confirmation", "{j}");
}

#[test]
fn malformed_documents_are_category_1_naming_the_file() {
    // 3.4.1: as a malformed descriptor (spec 006, 3.4.1).
    let s = scratch("xdoc-malformed");
    s.write("bad.json", b"not json");
    // A version 1 program holding a version 2 value (spec 070, 3.1.3).
    let mut v1: Json = serde_json::from_slice(&core_golden("program.json")).unwrap();
    v1["schema"] = "rustev.extract-rules/1".into();
    s.write("v1.json", &serde_json::to_vec(&v1).unwrap());
    for (flag, file, subject, golden) in [
        (
            "--extract-rules",
            "bad.json",
            "extract-rules:bad.json",
            Some("extraction-documents.malformed-program.json"),
        ),
        ("--template", "bad.json", "template:bad.json", None),
        ("--extract-rules", "v1.json", "extract-rules:v1.json", None),
    ] {
        let r = s.rustev(&check_args(&[flag, file]));
        let j = r.expect(5, "refused");
        if let Some(g) = golden {
            cli_golden(g, &r.stdout);
        }
        assert_eq!(
            (j["category"].as_u64(), j["subject"].as_str()),
            (Some(1), Some(subject)),
            "{j}"
        );
    }
}

#[test]
fn conflicting_programs_are_the_cores_refusal() {
    // Section 5: two programs with one name and version and different bytes.
    let s = scratch("xdoc-conflict");
    let mut other: Json = serde_json::from_slice(&core_golden("program.json")).unwrap();
    other["fields"][0]["rules"][0]["name"] = "other".into();
    s.write("other.json", &serde_json::to_vec(&other).unwrap());
    let r = s.rustev(&check_args(&[
        "--extract-rules",
        "program.json",
        "--extract-rules",
        "other.json",
        "--template",
        "template.json",
    ]));
    let j = r.expect(5, "refused");
    cli_golden("extraction-documents.conflict.json", &r.stdout);
    assert_eq!(j["category_name"], "invalid_extraction");
}

#[test]
fn an_unnamed_program_is_ignored() {
    let s = scratch("xdoc-unnamed");
    let mut other: Json = serde_json::from_slice(&core_golden("program.json")).unwrap();
    other["name"] = "unrelated".into();
    s.write("other.json", &serde_json::to_vec(&other).unwrap());
    let r = s.rustev(&check_args(&[
        "--extract-rules",
        "other.json",
        "--extract-rules",
        "program.json",
        "--template",
        "template.json",
    ]));
    let id = String::from_utf8(core_golden("plan.id")).unwrap();
    assert_eq!(r.expect(0, "ok")["plan_id"], Json::String(id));
}

#[test]
fn instantiation_by_flag_is_a_usage_error_before_any_read() {
    // 3.3.4: refused before any file is read (none of these exist).
    let s = Scratch::new("xdoc-usage");
    for extra in [
        &["--profile", "p.json"][..],
        &["--variant", "a"][..],
        &["--definition", "second.json"][..],
    ] {
        let mut args = vec![
            "plan",
            "check",
            "--definition",
            "missing.json",
            "--extract-rules",
            "missing-program.json",
        ];
        args.extend_from_slice(extra);
        let r = s.rustev(&args);
        assert_eq!(r.code, 2, "{extra:?}: {}", r.stderr);
        assert!(r.stdout.is_empty());
    }
}

#[test]
fn a_template_definition_is_invalid_input_before_any_dependency_is_read() {
    // 3.3.4: found only in the definition, so after it is read; the
    // dependency named here does not exist and is never read.
    let s = scratch("xdoc-template-definition");
    let mut d: Json = serde_json::from_slice(&core_golden("definition.json")).unwrap();
    d["variants"] = serde_json::json!([]);
    s.write("template-definition.json", &serde_json::to_vec(&d).unwrap());
    let r = s.rustev(&[
        "plan",
        "check",
        "--definition",
        "template-definition.json",
        "--template",
        "missing-template.json",
    ]);
    let j = r.expect(4, "invalid_input");
    assert_eq!(j["input"], "missing-template.json");
}

#[test]
fn more_than_4096_files_per_flag_is_an_io_error_before_any_read() {
    // 3.2.1, amending 006 3.3.2.
    let s = Scratch::new("xdoc-caps");
    for flag in ["--extract-rules", "--template"] {
        let mut args = vec!["plan", "check", "--definition", "missing.json"];
        for _ in 0..4097 {
            args.extend([flag, "missing.json"]);
        }
        let r = s.rustev(&args);
        let j = r.expect(3, "io_error");
        assert_eq!(j["path"], flag, "{j}");
    }
}

#[test]
fn rules_still_takes_a_rules_backend_program() {
    // Section 5: `--rules` is unchanged; an extraction program there is
    // refused by the rules backend as invalid input.
    let s = scratch("xdoc-rules");
    let r = s.rustev(&check_args(&["--rules", "program.json"]));
    let j = r.expect(4, "invalid_input");
    assert_eq!(j["input"], "program.json");
}
