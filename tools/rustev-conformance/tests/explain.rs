//! Definitions are harvested only with RUSTEV_BLESS=1, never authored JSON.
mod explain_support;
use explain_support::{FIXTURES, cases, names, refusals};
use rustev_conformance::{root, sha256_hex};
use serde_json::{Value, json};
fn pin(path: &str) -> Value {
    json!({"path": path, "sha256": sha256_hex(&std::fs::read(root().join(path)).unwrap())})
}
fn write(path: &str, v: &Value) {
    let mut bytes = serde_json::to_vec_pretty(v).unwrap();
    bytes.push(b'\n');
    let p = root().join(path);
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    } else {
        assert_eq!(
            std::fs::read(p).unwrap(),
            bytes,
            "{path}: run RUSTEV_BLESS=1 cargo test -p rustev-conformance --test explain"
        );
    }
}
#[test]
fn every_explain_golden_and_refusal_is_a_binding_case() {
    let goldens = cases();
    let count = std::fs::read_dir(root().join(FIXTURES))
        .unwrap()
        .filter(|p| {
            p.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".explanation.json")
        })
        .count();
    assert_eq!(
        goldens.len(),
        count,
        "every explain golden has a definition"
    );
    for (name, args, resolve, refusal) in
        goldens.into_iter().map(|(n, a)| (n, a, false, None)).chain(
            refusals()
                .into_iter()
                .map(|(n, a, r, c)| (n, a, r, Some(c))),
        )
    {
        let inputs: Vec<_> = names(&args)
            .into_iter()
            .map(|n| {
                let mut p = pin(&format!("{FIXTURES}/{n}"));
                p["name"] = n.into();
                p
            })
            .collect();
        let outputs: Vec<_> = if let Some(code) = refusal {
            let path = format!("tools/rustev-conformance/expected/explain/{name}.json");
            write(&path, &json!({"class": "InvalidInput", "code": code}));
            let p = pin(&path);
            vec![json!({"file": "error", "golden": path, "sha256": p["sha256"], "normalize": []})]
        } else {
            [("explanation", "json"), ("markdown", "md")]
                .into_iter()
                .map(|(file, ext)| {
                    let path = format!("{FIXTURES}/{name}.explanation.{ext}");
                    let p = pin(&path);
                    json!({"file": file, "golden": path, "sha256": p["sha256"], "normalize": []})
                })
                .collect()
        };
        let case = json!({"id": format!("explain.{name}"), "command": "explain", "surfaces": ["native", "wasm"], "explain": {"args": args, "resolve": resolve, "refusal": refusal, "comparison": "bytes", "probeResolve": name == "support-bundle-models"}, "inputs": inputs, "outputs": outputs});
        write(
            &format!("tools/rustev-conformance/cases/cli/explain.{name}.json"),
            &case,
        );
    }
}
