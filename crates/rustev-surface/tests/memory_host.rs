//! Spec 027, 3.1: a host other than the file system drives the surface and
//! gets the CLI's bytes. Inputs are named byte strings; outputs land in
//! memory. SYNTHETIC reference fixtures.

use std::cell::RefCell;
use std::collections::BTreeMap;

use rustev_surface::deps::{DepArgs, no_rules};
use rustev_surface::host::{Host, IoFail, ReadFail};
use rustev_surface::plan::{CompileArgs, check, compile_cmd, show};

/// Named inputs and the outputs a command published.
#[derive(Default)]
struct Memory {
    inputs: BTreeMap<String, Vec<u8>>,
    outputs: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl Host for Memory {
    // At most `max` bytes, as the CLI's file host reads: a caller that must
    // tell an oversized input asks for its limit plus one byte and compares
    // lengths. This host has no aggregate budget, so it never reports one.
    fn read_classified(&self, label: &str, max: usize) -> Result<Option<Vec<u8>>, ReadFail> {
        Ok(self
            .inputs
            .get(label)
            .map(|b| b[..b.len().min(max)].to_vec()))
    }

    fn ensure_absent(&self, label: &str) -> Result<(), IoFail> {
        if self.outputs.borrow().contains_key(label) {
            return Err(IoFail {
                path: label.into(),
                detail: "already exists; outputs are never overwritten".into(),
            });
        }
        Ok(())
    }

    fn create(&self, label: &str, bytes: &[u8]) -> Result<(), IoFail> {
        self.ensure_absent(label)?;
        self.outputs
            .borrow_mut()
            .insert(label.into(), bytes.to_vec());
        Ok(())
    }

    fn write_all(&self, files: &[(String, Vec<u8>)]) -> Result<(), IoFail> {
        // All or none: a failure removes what this call already wrote.
        for (i, (l, b)) in files.iter().enumerate() {
            if let Err(e) = self.create(l, b) {
                let mut out = self.outputs.borrow_mut();
                for (written, _) in &files[..i] {
                    out.remove(written);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    fn join(&self, dir: &str, name: &str) -> String {
        format!("{dir}/{name}")
    }
}

const DEFINITION: &[u8] = include_bytes!("../../rustev-core/tests/golden/lodging.definition.json");

const RULES: &[u8] =
    include_bytes!("../../../backends/rustev-backend-rules/tests/fixtures/lodging.rules.json");

fn memory() -> Memory {
    let mut m = Memory::default();
    m.inputs
        .insert("definition".into(), DEFINITION.trim_ascii_end().to_vec());
    m.inputs.insert("lodging.rules".into(), RULES.to_vec());
    m
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
    let plan = m.outputs.borrow()["plan"].clone();
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
    let mut m2 = memory();
    m2.inputs.insert("plan".into(), plan);
    let shown = show("plan show", &m2, "plan", deps(), &mut rules_backend).unwrap();
    assert_eq!(
        shown.out.bytes(),
        include_bytes!("../../rustev-cli/tests/golden/lodging.show.json")
    );
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
