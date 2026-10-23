//! Spec 027, 3.1: a host other than the file system drives the surface and
//! gets the CLI's bytes. Inputs are named byte strings; outputs land in
//! memory. SYNTHETIC reference fixtures.

use rustev_surface::deps::{DepArgs, no_rules};
use rustev_surface::host::{Host, Memory};
use rustev_surface::plan::{CompileArgs, check, compile_cmd, show};

const DEFINITION: &[u8] = include_bytes!("../../rustev-core/tests/golden/lodging.definition.json");

const RULES: &[u8] =
    include_bytes!("../../../backends/rustev-backend-rules/tests/fixtures/lodging.rules.json");

fn memory() -> Memory {
    Memory::new(
        [
            (
                "definition".to_string(),
                DEFINITION.trim_ascii_end().to_vec(),
            ),
            ("lodging.rules".to_string(), RULES.to_vec()),
        ]
        .into(),
    )
}

static RULES_LABELS: std::sync::LazyLock<Vec<String>> =
    std::sync::LazyLock::new(|| vec!["lodging.rules".to_string()]);

fn deps() -> DepArgs<'static> {
    DepArgs {
        rules: &RULES_LABELS,
        ..DepArgs::default()
    }
}

/// The host's rules backend, as the CLI's.
fn rules_backend(
    _: &str,
    bytes: &[u8],
) -> Result<rustev_contract::descriptor::BackendDescriptor, String> {
    use rustev_core::seams::DecisionBackend;
    let b = rustev_backend_rules::RulesBackend::from_bytes(bytes).map_err(|e| e.to_string())?;
    Ok(b.descriptor().clone())
}

fn args() -> CompileArgs<'static> {
    CompileArgs {
        definition: "definition",
        deps: deps(),
        execution: None,
    }
}

#[test]
fn check_prints_the_cli_bytes() {
    let m = memory();
    let done = check("plan check", &m, args(), &mut rules_backend).unwrap();
    assert_eq!(done.code, 0);
    assert_eq!(
        done.out.bytes(),
        include_bytes!("../../rustev-cli/tests/golden/lodging.check.json")
    );
}

#[test]
fn compile_publishes_the_plan_and_show_reads_it() {
    let m = memory();
    let done = compile_cmd("plan compile", &m, args(), &mut rules_backend, "plan").unwrap();
    assert_eq!(done.code, 0);
    let plan = m.output("plan").unwrap();
    // The published plan is the one `plan check` names.
    let compiled = String::from_utf8(done.out.bytes()).unwrap();
    let checked = String::from_utf8(
        include_bytes!("../../rustev-cli/tests/golden/lodging.check.json").to_vec(),
    )
    .unwrap();
    let id = |s: &str| s.split("\"plan_id\":\"").nth(1).unwrap()[..71].to_string();
    assert_eq!(id(&compiled), id(&checked));
    // An existing output is refused before any work, as on a file system.
    let again = compile_cmd("plan compile", &m, args(), &mut rules_backend, "plan").unwrap_err();
    assert_eq!(again.code, 3);
    let mut inputs = memory().inputs().clone();
    inputs.insert("plan".into(), plan);
    let m2 = Memory::new(inputs);
    let shown = show("plan show", &m2, "plan", deps(), &mut rules_backend).unwrap();
    assert_eq!(
        shown.out.bytes(),
        include_bytes!("../../rustev-cli/tests/golden/lodging.show.json")
    );
}

#[cfg(feature = "parameterized")]
#[test]
fn show_requires_a_valid_recorded_template_version() {
    use serde_json::{Value, json};

    let original: Value = serde_json::from_slice(include_bytes!(
        "../../rustev-core/tests/golden/parameterized.off.batch.plan.json"
    ))
    .unwrap();
    let show_plan = |plan: &Value| {
        let m = Memory::new([
            ("plan".into(), serde_json::to_vec(plan).unwrap()),
            ("descriptor".into(), include_bytes!("../../../tools/rustev-conformance/fixtures/runtime/synthetic-linear-head.descriptor.json").to_vec()),
        ].into());
        let descriptors = ["descriptor".into()];
        show(
            "plan show",
            &m,
            "plan",
            DepArgs {
                descriptors: &descriptors,
                ..DepArgs::default()
            },
            &mut no_rules,
        )
    };
    let shown = show_plan(&original).unwrap();
    assert_eq!(shown.code, 0);
    let output: Value = serde_json::from_slice(&shown.out.bytes()).unwrap();
    assert_eq!(output["plan_schema"], "rustev.plan/3");
    assert_eq!(
        output["instantiation"]["template_version"],
        original["instantiation"]["template"]["version"]
    );

    for version in [
        None,
        Some(Value::Null),
        Some(json!(3)),
        Some(json!("")),
        Some(json!("1.2")),
        Some(json!("01.2.3")),
        Some(json!("1.2.3-beta")),
    ] {
        let mut plan = original.clone();
        let template = plan["instantiation"]["template"].as_object_mut().unwrap();
        match version {
            None => {
                template.remove("version");
            }
            Some(v) => {
                template.insert("version".into(), v);
            }
        }
        let refused = show_plan(&plan).unwrap_err();
        assert_eq!(refused.code, 5);
        let output: Value = serde_json::from_slice(&refused.out.bytes()).unwrap();
        assert_eq!(output["command"], "plan show");
        assert_eq!(output["status"], "refused");
        assert_eq!(output["category"], 1);
        assert_eq!(output["category_name"], "parse");
        assert_eq!(output["subject"], "plan:plan");
        assert!(
            output["detail"]
                .as_str()
                .unwrap()
                .contains("instantiation.template.version")
        );
        assert!(output.get("instantiation").is_none());
    }
}

#[test]
fn a_rules_program_without_a_rules_backend_is_refused() {
    let m = memory();
    let done = check("plan check", &m, args(), &mut no_rules).unwrap_err();
    assert_eq!(done.code, 4);
    assert!(
        String::from_utf8(done.out.bytes())
            .unwrap()
            .contains("no rules backend")
    );
}

#[test]
fn join_names_an_output_by_its_directory_or_alone() {
    // An empty directory is the binding's top level: the label is the name
    // itself, as the bindings return artifacts keyed by name.
    let m = memory();
    assert_eq!(m.join("", "plan.json"), "plan.json");
    assert_eq!(m.join("out", "plan.json"), "out/plan.json");
}

#[test]
fn an_output_never_replaces_an_input() {
    // As on a file system, where the input is a file at that path.
    let m = memory();
    let e = m.ensure_absent("definition").unwrap_err();
    assert!(e.detail.contains("names an input"), "{}", e.detail);
    assert!(m.ensure_absent("plan").is_ok());
}
