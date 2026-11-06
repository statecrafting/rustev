//! The moved rendering over memory, compared with the existing CLI goldens.
#[path = "../../rustev-eval/tests/host_support/mod.rs"]
mod fixture;

use rustev_contract::Document;
use rustev_surface::host::{Host, Memory};
use rustev_surface::repeat::{BaselineArgs, ImportArgs, baseline, import};
use std::collections::BTreeMap;

fn inputs() -> (BTreeMap<String, Vec<u8>>, Vec<String>) {
    let config = fixture::config();
    let mut inputs = BTreeMap::from([
        (
            "repeat-config".into(),
            fixture::repeat_config().canonical().unwrap(),
        ),
        ("config".into(), config.canonical().unwrap()),
    ]);
    let mut runs = vec![];
    for (i, run) in fixture::set_docs(&config, &fixture::three_runs())
        .iter()
        .enumerate()
    {
        let dir = format!("in/run-{i}");
        inputs.insert(
            format!("{dir}/host-run.json"),
            run.record.record_canonical().unwrap(),
        );
        inputs.insert(
            format!("{dir}/views.json"),
            run.views.record_canonical().unwrap(),
        );
        for scores in &run.scores {
            inputs.insert(
                format!("{dir}/scores/{}", scores.scorer.id),
                scores.record_canonical().unwrap(),
            );
        }
        runs.push(dir);
    }
    (inputs, runs)
}

fn args(runs: &[String]) -> ImportArgs<'_> {
    ImportArgs {
        runs,
        repeat_config: "repeat-config",
        config: "config",
        out: "out",
    }
}

#[test]
fn import_in_memory_keeps_cli_bytes_and_publishes_a_readable_host_set() {
    let (inputs, runs) = inputs();
    let host = Memory::new(inputs.clone());
    let result = import("eval repeat import", &host, args(&runs)).unwrap();
    assert_eq!(result.code, 0);
    assert_eq!(
        result.out.bytes(),
        include_bytes!("../../rustev-cli/tests/golden/host/import.stdout")
    );
    assert_eq!(
        host.output("out/repeat.json").unwrap(),
        include_bytes!("../../rustev-cli/tests/golden/host/import.repeat.json")
    );
    assert_eq!(
        host.output("out/run-1.cases.json").unwrap(),
        include_bytes!("../../rustev-cli/tests/golden/host/import.run-1.cases.json")
    );
    assert_eq!(host.inputs(), &inputs);
    assert_eq!(
        host.output("out/run-0.views.json").unwrap(),
        inputs["in/run-0/views.json"]
    );
    let host = Memory::new(host.into_outputs());
    let recorded = baseline(
        "baseline record",
        &host,
        BaselineArgs {
            name: "memory",
            eval: "out",
            out: "baseline",
            now_ms: 17,
        },
    )
    .unwrap();
    assert_eq!(recorded.code, 0);
    assert!(host.output("baseline").is_some());
}

#[test]
fn import_collision_publishes_nothing_and_memory_rolls_back_a_set() {
    let (mut inputs, runs) = inputs();
    inputs.insert("out/run-1.cases.json".into(), b"keep".to_vec());
    let host = Memory::new(inputs);
    let error = import("eval repeat import", &host, args(&runs)).unwrap_err();
    assert_eq!(error.code, 3);
    assert!(host.into_outputs().is_empty());

    let host = Memory::default();
    host.create("keep", b"original").unwrap();
    assert!(
        host.write_all(&[("first".into(), vec![]), ("keep".into(), vec![])])
            .is_err()
    );
    assert_eq!(host.output("keep").unwrap(), b"original");
    assert!(host.output("first").is_none());
}
