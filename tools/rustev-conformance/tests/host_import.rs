//! Spec 063, 3.9.1: the inputs of spec 061's host import as files, so the
//! Node binding's `evalRepeatImport` runs on the synthetic host runs the
//! CLI's `tests/host_sets.rs` imports, under the same names, and compares
//! with the same goldens (`crates/rustev-cli/tests/golden/host/`).
//! SYNTHETIC. `RUSTEV_BLESS=1` writes them; otherwise they must be current.

#[path = "../../../crates/rustev-eval/tests/host_support/mod.rs"]
mod host;

use rustev_conformance::root;
use rustev_contract::Document;

#[test]
fn the_host_import_fixtures_are_the_shared_host_runs() {
    let config = host::config();
    let mut files = vec![
        (
            "repeat-config.json".to_string(),
            host::repeat_config().canonical().unwrap(),
        ),
        ("config.json".to_string(), config.canonical().unwrap()),
    ];
    for (r, d) in host::set_docs(&config, &host::three_runs())
        .iter()
        .enumerate()
    {
        let dir = format!("in/run-{r}");
        files.push((
            format!("{dir}/host-run.json"),
            d.record.record_canonical().unwrap(),
        ));
        files.push((
            format!("{dir}/views.json"),
            d.views.record_canonical().unwrap(),
        ));
        for doc in &d.scores {
            files.push((
                format!("{dir}/scores/{}.json", doc.scorer.id),
                doc.record_canonical().unwrap(),
            ));
        }
    }
    let base = root().join("tools/rustev-conformance/fixtures/host-import");
    for (name, bytes) in files {
        let p = base.join(&name);
        if std::env::var_os("RUSTEV_BLESS").is_some() {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, &bytes).unwrap();
        }
        let got = std::fs::read(&p).unwrap_or_else(|e| panic!("{name}: {e}; bless it"));
        assert!(got == bytes, "{name} is stale; run with RUSTEV_BLESS=1");
    }
}
