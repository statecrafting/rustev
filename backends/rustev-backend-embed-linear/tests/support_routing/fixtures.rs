use super::*;

const README: &str = r#"# SYNTHETIC support-routing bundle

Mechanics only: these heads and the package's unfitted temperature calibration
are not evidence of routing quality, usefulness or calibration quality.

Hidden size is 8. The nine-token WordPiece vocabulary comes from the existing
`tests/bundle.rs::tokenizer_bytes` generator. Unknown words remain unknown tokens.
There is no checkpoint, network access or ONNX Runtime dependency at execution.

For each head, `synthetic::head_weights(seed, options, 8)` emits option-major
weights followed by biases as little-endian f32. Value k is
`((splitmix64(seed + k) >> 40) as f32) / 8388608 - 1`.
The unsigned 64-bit additions and multiplications wrap. SplitMix64 adds
0x9e3779b97f4a7c15, applies `(z ^ (z >> 30)) * 0xbf58476d1ce4e5b9`, then
`(z ^ (z >> 27)) * 0x94d049bb133111eb`, then `z ^ (z >> 31)`.

| Task | Operation | Options | Seed |
|---|---|---|---|
| support.topic | classify | billing, integration_defect, account_access, other | 11 |
| support.frustration | rubric | calm, frustrated, very_angry | 12 |
| support.deadline | proposition | false, true | 13 |

The generated manifest binds every payload by byte length and SHA-256. Head
questions and options come from the package's reference definition. The only
projected field is ticket.message. The graph file is a synthetic marker.
`identities.json` pins the descriptor and compiled plan with the package's
SYNTHETIC temperature 1.5 calibration rebound to this bundle's ArtifactId.
The existing rules PlanId is checked against the package's committed example
judgment, never re-blessed here.

Normal tests regenerate and compare every file byte and the exact file list;
they never rewrite fixtures. After reviewing an intentional formula change,
re-bless from the workspace root with:

```
cargo test -p rustev-backend-embed-linear --locked --test bundle support_routing::fixtures::bless_support_routing_fixtures -- --ignored --exact
```

Review the resulting diff and run the normal tests. The README itself is
emitted from `tests/support_routing/fixtures.rs::README`.
"#;

pub(super) fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/support-routing")
}

fn generated() -> BTreeMap<String, Vec<u8>> {
    let graph = b"rustev synthetic encoder\n".to_vec();
    let tokenizer = tokenizer_bytes();
    let mut manifest = base_manifest(&graph, &tokenizer, &[], &[]);
    manifest["backend_id"] = json!("embed.support-routing.synthetic");
    manifest["runtime"]["ort"] = json!("2.0.0-rc.15");
    manifest["runtime"]["onnxruntime"] = json!("1.31.0");
    let mut files = BTreeMap::from([
        ("graph.bin".into(), graph),
        ("tokenizer.json".into(), tokenizer),
        ("README.md".into(), README.as_bytes().to_vec()),
    ]);
    let definition = pkg::definition(&pkg::Params::reference().unwrap()).unwrap();
    let mut heads = vec![];
    for (step, seed) in [
        ("topic", 11),
        ("frustration", 12),
        ("explicit_deadline", 13),
    ] {
        let s = definition.steps.iter().find(|s| s.id == step).unwrap();
        let StepBody::Semantic(d) = &s.body else {
            panic!("not semantic")
        };
        let filename = format!("heads/{step}.bin");
        let weights = head_weights(seed, d.options.len(), HIDDEN);
        let options: Vec<_> = d.options.iter().map(String::as_str).collect();
        let mut h = head(
            "classify",
            &d.task,
            &options,
            file_entry(&filename, &weights),
        );
        h["operation"] = serde_json::to_value(d.operation).unwrap();
        h["question"] = json!(d.question);
        h["text"] =
            json!({"fields":[{"label":"", "ref":"input:ticket.message"}], "separator":"\n"});
        h["provenance"]["method"] = json!(format!("splitmix64 seed {seed}; formula in README.md"));
        files.insert(filename, weights);
        heads.push(h);
    }
    manifest["heads"] = json!(heads);
    let parsed = parse(&manifest).unwrap();
    let descriptor = derive_descriptor(&parsed, parsed.artifact_id().unwrap());
    let (compiled, _) = plan(&descriptor);
    files.insert("manifest.json".into(), parsed.canonical().unwrap());
    files.insert(
        "identities.json".into(),
        serde_json::to_vec_pretty(&json!({
            "plan": compiled.id, "descriptor": descriptor.id().unwrap()
        }))
        .unwrap(),
    );
    files
}

fn paths(dir: &Path, prefix: &str) -> Vec<String> {
    let mut out = vec![];
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = format!("{prefix}{}", entry.file_name().to_str().unwrap());
        let kind = entry.file_type().unwrap();
        assert!(!kind.is_symlink());
        if kind.is_dir() {
            out.extend(paths(&entry.path(), &format!("{name}/")));
        } else {
            assert!(kind.is_file());
            out.push(name);
        }
    }
    out.sort();
    out
}

#[test]
fn committed_support_routing_fixtures_are_reproducible() {
    let expected = generated();
    let hint = "re-bless with cargo test -p rustev-backend-embed-linear --locked --test bundle support_routing::fixtures::bless_support_routing_fixtures -- --ignored --exact";
    assert!(directory().is_dir(), "fixture directory missing; {hint}");
    assert_eq!(
        paths(&directory(), ""),
        expected.keys().cloned().collect::<Vec<_>>(),
        "{hint}"
    );
    let mut total = 0;
    for (name, bytes) in expected {
        let actual = fs::read(directory().join(&name)).unwrap();
        assert!(actual == bytes, "{name} differs; {hint}");
        total += actual.len();
    }
    assert!(total < 200_000, "fixture grew to {total} bytes");
    println!("SYNTHETIC support-routing fixture: {total} bytes");
}

#[test]
#[ignore = "explicit fixture maintenance only; never runs in normal verification"]
fn bless_support_routing_fixtures() {
    for (name, bytes) in generated() {
        let path = directory().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

pub(super) fn temporary_copy() -> Bundle {
    // Reuse the existing unique temporary-directory guard and cleanup.
    let b = new_bundle();
    fs::remove_dir_all(&b.dir).unwrap();
    for name in paths(&directory(), "") {
        let path = b.dir.join(&name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(directory().join(name), path).unwrap();
    }
    b
}
