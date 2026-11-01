//! Spec 011 increment 1 over synthetic bundles only (3.19.3): manifest
//! checks (3.4.3), bundle verification and setup codes (3.7.2), identity
//! (3.5), the descriptor (3.10), pooling and head arithmetic (3.9) and the
//! synthetic encoder (3.19.1). No network, checkpoint or runtime library.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use rustev_backend_embed_linear::bundle::sha256_hex;
use rustev_backend_embed_linear::head::{LinearHead, pool};
use rustev_backend_embed_linear::manifest::{self, Normalize};
use rustev_backend_embed_linear::synthetic::{SyntheticEncoder, head_weights, splitmix64, weight};
use rustev_backend_embed_linear::{
    EmbedManifest, SetupCode, cost_bound, derive_descriptor, verify_bundle,
};
use rustev_contract::Document;
use rustev_contract::definition::{Determinism, Operation};
use rustev_contract::descriptor::OutputKind;
use rustev_contract::document::Identified;
use rustev_contract::ids::ArtifactId;
use rustev_contract::run::CostBound;
use serde_json::{Value, json};

const HIDDEN: usize = 8;

/// A synthetic bundle in its own temporary directory, removed on drop.
struct Bundle {
    dir: PathBuf,
    manifest: Value,
}

impl Drop for Bundle {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn file_entry(name: &str, bytes: &[u8]) -> Value {
    json!({"file": name, "bytes": bytes.len(), "sha256": sha256_hex(bytes)})
}

fn head(op: &str, task: &str, options: &[&str], weights: Value) -> Value {
    json!({
        "operation": op,
        "task": task,
        "question": format!("Which {task}?"),
        "options": options,
        "text": {"fields": [{"label": "Subject", "ref": "input:subject"},
                            {"label": "", "ref": "bind:ticket/body"}],
                 "separator": "\n"},
        "weights": weights,
        "interpretation": "SYNTHETIC linear scores; not probabilities.",
        "provenance": {"producer": "rustev tests", "method": "splitmix64 seed formula",
                       "training_data_description": "none: generated, not fitted",
                       "training_data_sha256": "none", "synthetic": true}
    })
}

fn base_manifest(graph: &[u8], tokenizer: &[u8], route: &[u8], urgent: &[u8]) -> Value {
    let mut g = file_entry("graph.bin", graph);
    g["source"] = json!("synthetic");
    let mut t = file_entry("tokenizer.json", tokenizer);
    t["implementation"] = json!("tokenizers@0.21.1");
    t["max_tokens"] = json!(256);
    t["truncation"] = json!("refuse");
    json!({
        "schema": "rustev.embed-linear/1",
        "backend_id": "embed.synthetic-1",
        "checkpoint": {"repository": "synthetic", "revision": "0".repeat(40), "files": [],
                       "license": "none", "base_model": "synthetic"},
        "encoder": {"kind": "synthetic", "graph": g, "inputs": ["input_ids", "attention_mask"],
                    "output": "last_hidden_state", "hidden_size": HIDDEN, "precision": "f32"},
        "tokenizer": t,
        "pooling": {"mode": "mean_masked", "normalize": "l2"},
        "runtime": {"onnxruntime": "1.22.0", "ort": "2.0.0-rc.10", "execution_provider": "cpu",
                    "graph_optimization": "disabled", "intra_op_threads": 1,
                    "inter_op_threads": 1, "libraries": []},
        "heads": [
            head("classify", "route", &["billing", "technical", "other"],
                 file_entry("heads/route.bin", route)),
            head("proposition", "urgent", &["false", "true"],
                 file_entry("heads/urgent.bin", urgent)),
        ],
        "limits": {"max_projection_bytes": 65536, "max_text_bytes": 4096},
        "cost": {"units_per_call": 1, "units_per_token": 2},
        "tolerance": {"abs_logit": "0.00001", "rel_logit": "0.0001",
                      "decision_margin": "0.001", "feasibility_record": "none"}
    })
}

fn new_bundle() -> Bundle {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rustev-embed-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("heads")).unwrap();
    let graph = b"rustev synthetic encoder\n".to_vec();
    let tokenizer = b"{}".to_vec();
    let route = head_weights(1, 3, HIDDEN);
    let urgent = head_weights(2, 2, HIDDEN);
    for (p, b) in [
        ("graph.bin", &graph),
        ("tokenizer.json", &tokenizer),
        ("heads/route.bin", &route),
        ("heads/urgent.bin", &urgent),
    ] {
        fs::write(dir.join(p), b).unwrap();
    }
    let manifest = base_manifest(&graph, &tokenizer, &route, &urgent);
    let b = Bundle { dir, manifest };
    b.write();
    b
}

impl Bundle {
    fn write(&self) {
        fs::write(
            self.dir.join("manifest.json"),
            serde_json::to_vec(&self.manifest).unwrap(),
        )
        .unwrap();
    }
    fn artifact(&self) -> ArtifactId {
        parse(&self.manifest).unwrap().artifact_id().unwrap()
    }
    fn verify(&self) -> Result<rustev_backend_embed_linear::VerifiedBundle, SetupCode> {
        verify_bundle(&self.dir, &self.artifact()).map_err(|e| e.code)
    }
    /// Replace a named file's bytes and re-declare them in the manifest.
    fn replace(&mut self, ptr: &str, rel: &str, bytes: &[u8]) {
        fs::write(self.dir.join(rel), bytes).unwrap();
        let entry = self.manifest.pointer_mut(ptr).unwrap();
        entry["bytes"] = json!(bytes.len());
        entry["sha256"] = json!(sha256_hex(bytes));
        self.write();
    }
}

fn parse(v: &Value) -> Result<EmbedManifest, manifest::ManifestError> {
    let m = EmbedManifest::parse(&serde_json::to_vec(v).unwrap())?;
    manifest::check(&m)?;
    Ok(m)
}

/// `edit` applied to the base manifest is refused, naming `at`.
fn refused(edit: impl FnOnce(&mut Value), at: &str) {
    let b = new_bundle();
    let mut v = b.manifest.clone();
    edit(&mut v);
    let e = parse(&v).expect_err(at);
    assert!(e.at.starts_with(at), "{at}: {e}");
}

fn accepted(edit: impl FnOnce(&mut Value)) {
    let b = new_bundle();
    let mut v = b.manifest.clone();
    edit(&mut v);
    parse(&v).unwrap();
}

#[test]
fn a_synthetic_bundle_verifies_and_derives_its_descriptor() {
    let b = new_bundle();
    let v = b.verify().unwrap();
    assert_eq!(v.artifact, b.artifact());
    assert_eq!(v.heads.len(), 2);
    assert_eq!(v.graph, b"rustev synthetic encoder\n");
    let d = derive_descriptor(&v.manifest, v.artifact.clone());
    assert_eq!(d.determinism, Determinism::Tolerance);
    assert!(d.ranking.is_none());
    let ops: Vec<_> = d
        .operations
        .iter()
        .map(|o| (o.operation, o.output.clone(), o.max_options))
        .collect();
    assert_eq!(
        ops,
        [
            (Operation::Classify, OutputKind::Logits, 3),
            (Operation::Proposition, OutputKind::Logits, 2)
        ]
    );
    assert_eq!(d.input_limit.max_bytes, 65536);
    assert_eq!(
        cost_bound(&v.manifest),
        CostBound::Bounded { max_units: 513 }
    );
}

#[test]
fn manifest_consistency_refusals() {
    refused(
        |v| v["heads"][0]["options"] = json!(["a", "a"]),
        "heads[0].options",
    );
    refused(
        |v| v["heads"][1]["options"] = json!(["true", "false"]),
        "heads[1].options",
    );
    refused(
        |v| v["runtime"]["intra_op_threads"] = json!(0),
        "runtime.intra_op_threads",
    );
    refused(
        |v| v["tokenizer"]["max_tokens"] = json!(513),
        "tokenizer.max_tokens",
    );
    accepted(|v| v["tokenizer"]["max_tokens"] = json!(512));
    refused(|v| v["backend_id"] = json!("x".repeat(65)), "backend_id");
    accepted(|v| v["backend_id"] = json!("x".repeat(64)));
    refused(|v| v["backend_id"] = json!("Upper"), "backend_id");
    refused(|v| v["heads"] = json!([]), "heads");
    refused(
        |v| {
            let h = v["heads"][0].clone();
            v["heads"] = Value::Array(
                (0..33)
                    .map(|i| {
                        let mut h = h.clone();
                        h["task"] = json!(format!("t{i}"));
                        h
                    })
                    .collect(),
            );
        },
        "heads",
    );
    refused(|v| v["heads"][1] = v["heads"][0].clone(), "heads[1].task");
    refused(
        |v| v["heads"][0]["operation"] = json!("rank"),
        "heads[0].operation",
    );
    refused(
        |v| v["heads"][0]["text"]["fields"] = json!([]),
        "heads[0].text.fields",
    );
    refused(
        |v| v["heads"][0]["text"]["fields"][0]["ref"] = json!("subject"),
        "heads[0].text.fields[0].ref",
    );
    refused(
        |v| v["heads"][0]["interpretation"] = json!("linear scores"),
        "heads[0].interpretation",
    );
    refused(
        |v| v["tolerance"]["abs_logit"] = json!("0.0001"),
        "tolerance",
    );
    refused(
        |v| v["tolerance"]["feasibility_record"] = json!("sha256:00"),
        "tolerance.feasibility_record",
    );
    refused(
        |v| v["checkpoint"]["repository"] = json!("acme/m"),
        "checkpoint.repository",
    );
    refused(
        |v| v["checkpoint"]["revision"] = json!("abc"),
        "checkpoint.revision",
    );
    refused(
        |v| v["tokenizer"]["implementation"] = json!("tokenizers@latest"),
        "tokenizer.implementation",
    );
    refused(
        |v| v["limits"]["max_projection_bytes"] = json!(65537),
        "limits.max_projection_bytes",
    );
    refused(
        |v| v["cost"]["units_per_token"] = json!(u64::MAX / 128),
        "cost",
    );
    refused(
        |v| v["heads"][0]["weights"]["file"] = json!("../w.bin"),
        "../w.bin",
    );
    refused(
        |v| v["heads"][0]["weights"]["file"] = json!("/abs.bin"),
        "/abs.bin",
    );
    refused(
        |v| v["heads"][0]["weights"]["file"] = json!("manifest.json"),
        "manifest.json",
    );
    refused(
        |v| {
            v["heads"][1]["weights"]["file"] = json!("heads/route.bin");
        },
        "heads/route.bin",
    );
    // Unknown members and a wrong schema fail the bounded parse.
    refused(|v| v["extra"] = json!(1), "");
    refused(|v| v["pooling"]["extra"] = json!(1), "");
    refused(|v| v["schema"] = json!("rustev.embed-linear/2"), "");
}

#[test]
fn an_onnx_graph_source_must_be_a_checkpoint_file_with_its_digest() {
    let onnx = |v: &mut Value, listed_sha: &str| {
        let sha = v["encoder"]["graph"]["sha256"].clone();
        v["encoder"]["kind"] = json!("onnx");
        v["encoder"]["graph"]["source"] = json!("onnx/model.onnx");
        v["checkpoint"]["repository"] = json!("sentence-transformers/all-MiniLM-L6-v2");
        v["checkpoint"]["files"] = json!([{"name": "onnx/model.onnx",
            "sha256": if listed_sha.is_empty() { sha } else { json!(listed_sha) }}]);
        v["runtime"]["libraries"] =
            json!([{"platform": "macos-aarch64", "sha256": "a".repeat(64)}]);
        v["tolerance"]["feasibility_record"] = json!(format!("sha256:{}", "b".repeat(64)));
        for h in v["heads"].as_array_mut().unwrap() {
            h["interpretation"] = json!("linear scores");
            h["provenance"]["synthetic"] = json!(false);
        }
    };
    accepted(|v| onnx(v, ""));
    refused(|v| onnx(v, &"c".repeat(64)), "encoder.graph.source");
    refused(
        |v| {
            onnx(v, "");
            v["encoder"]["graph"]["source"] = json!("onnx/model_O4.onnx");
        },
        "encoder.graph.source",
    );
    refused(
        |v| {
            onnx(v, "");
            v["runtime"]["libraries"] = json!([]);
        },
        "runtime.libraries",
    );
}

#[test]
fn setup_codes_for_missing_large_and_mismatched_files() {
    // Absent or relative directory, absent file.
    let b = new_bundle();
    let id = b.artifact();
    let code = |dir: &Path| verify_bundle(dir, &id).unwrap_err().code;
    assert_eq!(code(Path::new("relative/bundle")), SetupCode::BundleMissing);
    assert_eq!(code(&b.dir.join("absent")), SetupCode::BundleMissing);
    fs::remove_file(b.dir.join("heads/urgent.bin")).unwrap();
    assert_eq!(b.verify().unwrap_err(), SetupCode::BundleMissing);

    // One byte changed, one byte short, one byte long.
    let b = new_bundle();
    let mut graph = fs::read(b.dir.join("graph.bin")).unwrap();
    graph[0] ^= 1;
    fs::write(b.dir.join("graph.bin"), &graph).unwrap();
    assert_eq!(b.verify().unwrap_err(), SetupCode::DigestMismatch);
    fs::write(b.dir.join("graph.bin"), &graph[1..]).unwrap();
    assert_eq!(b.verify().unwrap_err(), SetupCode::DigestMismatch);
    graph.push(0);
    fs::write(b.dir.join("graph.bin"), &graph).unwrap();
    assert_eq!(b.verify().unwrap_err(), SetupCode::BundleTooLarge);

    // A declared total over 150 MiB is refused before any file is read.
    let mut b = new_bundle();
    b.manifest["encoder"]["graph"]["bytes"] = json!(150 * 1024 * 1024);
    b.write();
    assert_eq!(b.verify().unwrap_err(), SetupCode::BundleTooLarge);

    // A manifest over the descriptor parse bound.
    let b = new_bundle();
    fs::write(b.dir.join("manifest.json"), vec![b' '; 256 * 1024 + 1]).unwrap();
    assert_eq!(
        verify_bundle(&b.dir, &id).unwrap_err().code,
        SetupCode::BundleTooLarge
    );
    fs::write(b.dir.join("manifest.json"), b"{}").unwrap();
    assert_eq!(
        verify_bundle(&b.dir, &id).unwrap_err().code,
        SetupCode::ManifestInvalid
    );

    // Another bundle's identity.
    let b = new_bundle();
    let other = ArtifactId::parse(&format!("sha256:{}", "0".repeat(64))).unwrap();
    assert_eq!(
        verify_bundle(&b.dir, &other).unwrap_err().code,
        SetupCode::ArtifactMismatch
    );
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_refused() {
    use std::os::unix::fs::symlink;
    let b = new_bundle();
    fs::rename(b.dir.join("graph.bin"), b.dir.join("real.bin")).unwrap();
    symlink(b.dir.join("real.bin"), b.dir.join("graph.bin")).unwrap();
    assert_eq!(b.verify().unwrap_err(), SetupCode::BundleMissing);

    let b = new_bundle();
    fs::rename(b.dir.join("heads"), b.dir.join("real-heads")).unwrap();
    symlink(b.dir.join("real-heads"), b.dir.join("heads")).unwrap();
    assert_eq!(b.verify().unwrap_err(), SetupCode::BundleMissing);

    let b = new_bundle();
    fs::rename(b.dir.join("manifest.json"), b.dir.join("m.json")).unwrap();
    symlink(b.dir.join("m.json"), b.dir.join("manifest.json")).unwrap();
    assert_eq!(b.verify().unwrap_err(), SetupCode::BundleMissing);
}

#[test]
fn head_weights_one_value_short_or_a_nan_bias_are_refused() {
    let mut b = new_bundle();
    let short = &head_weights(1, 3, HIDDEN)[4..];
    b.replace("/heads/0/weights", "heads/route.bin", short);
    assert_eq!(b.verify().unwrap_err(), SetupCode::HeadInvalid);

    let mut b = new_bundle();
    let mut w = head_weights(2, 2, HIDDEN);
    let n = w.len();
    w[n - 4..].copy_from_slice(&f32::NAN.to_le_bytes());
    b.replace("/heads/1/weights", "heads/urgent.bin", &w);
    let e = verify_bundle(&b.dir, &b.artifact()).unwrap_err();
    assert_eq!(
        (e.code, e.item.as_str()),
        (SetupCode::HeadInvalid, "heads[1]")
    );
}

#[test]
fn every_member_and_file_byte_is_identity() {
    let b = new_bundle();
    let base = b.artifact();
    let d0 = derive_descriptor(&parse(&b.manifest).unwrap(), base.clone())
        .id()
        .unwrap();
    let edits: Vec<(&str, Value)> = vec![
        ("/backend_id", json!("embed.other")),
        ("/encoder/hidden_size", json!(16)),
        ("/tokenizer/max_tokens", json!(128)),
        ("/tokenizer/truncation", json!("truncate_end")),
        ("/tokenizer/implementation", json!("tokenizers@0.21.2")),
        ("/pooling/normalize", json!("none")),
        ("/runtime/graph_optimization", json!("all")),
        ("/runtime/intra_op_threads", json!(2)),
        ("/heads/0/options/2", json!("account")),
        ("/heads/0/text/separator", json!(" | ")),
        ("/heads/0/interpretation", json!("SYNTHETIC, other words")),
        ("/limits/max_text_bytes", json!(1024)),
        ("/cost/units_per_call", json!(3)),
        ("/heads/1/weights/sha256", json!(sha256_hex(b"other bytes"))),
    ];
    for (ptr, value) in edits {
        let mut v = b.manifest.clone();
        *v.pointer_mut(ptr).unwrap() = value;
        let m = parse(&v).unwrap();
        let id = m.artifact_id().unwrap();
        assert_ne!(id, base, "{ptr}");
        assert_ne!(derive_descriptor(&m, id).id().unwrap(), d0, "{ptr}");
    }
    // The same members in another key order are the same identity.
    let reordered: Value =
        serde_json::from_str(&serde_json::to_string_pretty(&b.manifest).unwrap()).unwrap();
    assert_eq!(parse(&reordered).unwrap().artifact_id().unwrap(), base);
}

#[test]
fn the_synthetic_encoder_is_bitwise_repeatable_and_pools_to_unit_length() {
    let b = new_bundle();
    let v = b.verify().unwrap();
    let enc = SyntheticEncoder::for_bundle(&v).unwrap();
    assert_eq!(enc.hidden_size(), HIDDEN);
    let ids = [101u32, 7592, 2088, 999, 102, 0, 0];
    let mask = [1u32, 1, 1, 1, 1, 0, 0];
    let run = || {
        let tokens = enc.encode(&ids);
        let e = pool(&tokens, &mask, HIDDEN, Normalize::L2).unwrap();
        let logits: Vec<Vec<u64>> = v
            .heads
            .iter()
            .map(|h| h.logits(&e).unwrap().iter().map(|x| x.to_bits()).collect())
            .collect();
        (e.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), logits)
    };
    let first = run();
    for _ in 0..10 {
        assert_eq!(run(), first);
    }
    let e: Vec<f32> = first.0.iter().map(|b| f32::from_bits(*b)).collect();
    let norm: f32 = e.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-6);
    // Padding positions do not move the embedding.
    let unpadded = pool(&enc.encode(&ids[..5]), &mask[..5], HIDDEN, Normalize::L2).unwrap();
    assert_eq!(unpadded, e);
    // An empty mask has no mean.
    assert!(pool(&enc.encode(&ids), &[0; 7], HIDDEN, Normalize::None).is_none());
    // A synthetic encoder is not offered for an ONNX bundle's kind.
    let mut onnx = v.clone();
    onnx.manifest.encoder.kind = manifest::EncoderKind::Onnx;
    assert!(SyntheticEncoder::for_bundle(&onnx).is_none());
}

#[test]
fn the_encoder_and_heads_follow_the_documented_formula() {
    let enc = SyntheticEncoder::new(HIDDEN).unwrap();
    let tokens = enc.encode(&[42]);
    let h = splitmix64(42);
    let mut expected = vec![0f32; HIDDEN];
    expected[(h % HIDDEN as u64) as usize] = weight(h);
    assert_eq!(tokens, expected);
    assert!((-1.0..1.0).contains(&weight(u64::MAX)));
    assert_eq!(weight(0), -1.0);
    assert!(SyntheticEncoder::new(0).is_none());

    // Logits are b_i + sum_j W_ij e_j in f64, in index order.
    let bytes = head_weights(7, 2, 3);
    let vals: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    assert_eq!(vals[0], weight(splitmix64(7)));
    let head = LinearHead::from_bytes(&bytes, 2, 3).unwrap();
    let e = [0.5f32, -0.25, 0.125];
    let want: Vec<f64> = (0..2)
        .map(|i| {
            vals[i * 3..i * 3 + 3]
                .iter()
                .zip(e)
                .fold(f64::from(vals[6 + i]), |acc, (w, x)| {
                    acc + f64::from(*w) * f64::from(x)
                })
        })
        .collect();
    assert_eq!(head.logits(&e).unwrap(), want);
    assert!(head.logits(&e[..2]).is_none());
    assert!(LinearHead::from_bytes(&bytes[..bytes.len() - 4], 2, 3).is_err());
}
