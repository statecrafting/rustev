//! Conformance case harvest (spec 027, 3.12.2): with `RUSTEV_HARVEST=1`,
//! each invocation whose output a test pins as a CLI golden becomes a case
//! of the shared suite. Its inputs (the files and directories its arguments
//! name, as they were before it ran) are copied under
//! `tools/rustev-conformance/fixtures/cli/<case>/`, and the case is written
//! to `tools/rustev-conformance/cases/cli/<case>.json` with its argument
//! vector, exit code, and each pinned output referencing the golden in
//! place with the normalization that reproduces it; the suite's manifest
//! aggregates the case files. Without the variable nothing happens.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value as Json, json};
// `sha2` is a normal dependency of the CLI, so its tests can use it.
use sha2::{Digest, Sha256};

/// One invocation, as it was seen.
struct Seen {
    dir: PathBuf,
    args: Vec<String>,
    inputs: BTreeMap<String, Vec<u8>>,
    /// Every file in the directory before the invocation.
    present: BTreeMap<String, Vec<u8>>,
    stdout: Vec<u8>,
    code: i32,
    /// The files the invocation wrote, read as soon as it ended, so a file
    /// the test writes later is never taken for an output.
    written: BTreeMap<String, Vec<u8>>,
    case: Option<String>,
}

thread_local! {
    static LAST: RefCell<Option<Seen>> = const { RefCell::new(None) };
}

fn on() -> bool {
    std::env::var_os("RUSTEV_HARVEST").is_some()
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// Every regular file under `path` (or `path` itself), relative to `dir`.
fn files(dir: &Path, path: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .unwrap()
            .map(|e| e.expect("a harvested directory reads"))
            .collect();
        entries.sort_by_key(|e| e.path());
        for e in entries {
            files(dir, &e.path(), out);
        }
    } else if path.is_file() {
        let rel = path
            .strip_prefix(dir)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        out.insert(rel, std::fs::read(path).unwrap());
    }
}

/// The command words of an argument vector.
fn command_of(args: &[String]) -> String {
    args.iter()
        .take_while(|a| !a.starts_with('-'))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Before an invocation: the inputs its arguments name.
pub fn before(dir: &Path, args: &[&str]) {
    if !on() {
        return;
    }
    let mut inputs = BTreeMap::new();
    // The command words (`plan`, `compile`, `run`...) come before the first
    // flag and name no file; only what follows is probed as a path.
    let first_flag = args
        .iter()
        .position(|a| a.starts_with('-'))
        .unwrap_or(args.len());
    for (index, a) in args.iter().enumerate().skip(first_flag) {
        // Variant selectors are names, even when a same-named file exists.
        // Check the flag for this occurrence: a plan path may use that same
        // value elsewhere in the invocation and must still be harvested.
        if index > 0
            && matches!(
                args[index - 1],
                "--variant" | "--left-variant" | "--right-variant"
            )
        {
            continue;
        }
        // Every existing path an argument names is an input, an `--out`
        // that already exists included: the no-clobber cases need it there
        // for the CLI to refuse it.
        let p = dir.join(a);
        if !a.is_empty() && !a.starts_with('-') && p.exists() {
            files(dir, &p, &mut inputs);
        }
    }
    let mut present = BTreeMap::new();
    files(dir, dir, &mut present);
    LAST.with(|l| {
        *l.borrow_mut() = Some(Seen {
            dir: dir.to_path_buf(),
            args: args.iter().map(|s| s.to_string()).collect(),
            inputs,
            present,
            stdout: vec![],
            code: -1,
            written: BTreeMap::new(),
            case: None,
        })
    });
}

/// After it: what it printed and its exit code.
pub fn after(stdout: &[u8], code: i32) {
    if !on() {
        return;
    }
    LAST.with(|l| {
        if let Some(s) = l.borrow_mut().as_mut() {
            s.stdout = stdout.to_vec();
            s.code = code;
            let mut all = BTreeMap::new();
            files(&s.dir, &s.dir, &mut all);
            s.written = all
                .into_iter()
                // Written: new, or changed, against the whole directory as it
                // was before, not only the inputs the arguments name.
                .filter(|(name, bytes)| s.present.get(name) != Some(bytes))
                .collect();
        }
    });
}

/// Which output of the last invocation `golden` pins, and how: stdout or
/// a file it wrote, under no normalization or one the goldens use.
fn locate(seen: &Seen, golden: &[u8]) -> Option<Json> {
    // Each output under exactly the normalizations its shape calls for.
    let matches = |bytes: &[u8]| {
        let names: Vec<String> = serde_json::from_slice::<Json>(bytes)
            .map(|v| super::normalize::applicable(&v))
            .unwrap_or_default()
            .into_iter()
            .map(str::to_string)
            .collect();
        (super::normalize::apply(&names, bytes).as_deref() == Some(golden)).then_some(names)
    };
    if let Some(names) = matches(&seen.stdout) {
        return Some(json!({ "stream": "stdout", "normalize": names }));
    }
    for (name, bytes) in &seen.written {
        if let Some(names) = matches(bytes) {
            return Some(json!({ "file": name, "normalize": names }));
        }
    }
    None
}

/// A golden was pinned: record its invocation as a case.
pub fn golden(name: &str, bytes: &[u8]) {
    if !on() {
        return;
    }
    LAST.with(|l| {
        let mut l = l.borrow_mut();
        let seen = l
            .as_mut()
            .unwrap_or_else(|| panic!("golden {name} pinned without an invocation"));
        let mut output = locate(seen, bytes)
            .unwrap_or_else(|| panic!("golden {name} is no output of {:?}", seen.args));
        output["golden"] = format!("crates/rustev-cli/tests/golden/{name}").into();
        output["sha256"] = hex(bytes).into();
        // The first golden a test pins for an invocation names its case;
        // pinning them in another order renames the case, which the
        // manifest's drift check reports (`make conformance-harvest`).
        let id = seen
            .case
            .get_or_insert_with(|| name.trim_end_matches(".json").to_string())
            .clone();
        let fixtures = format!("tools/rustev-conformance/fixtures/cli/{id}");
        let mut inputs = vec![];
        for (rel, b) in &seen.inputs {
            let p = root().join(&fixtures).join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b).unwrap();
            inputs.push(
                json!({ "path": format!("{fixtures}/{rel}"), "name": rel, "sha256": hex(b) }),
            );
        }
        // Pure commands also run on the WebAssembly surface.
        let pure = matches!(
            command_of(&seen.args).as_str(),
            "plan check" | "plan compile" | "plan show" | "replay"
        );
        // The WebAssembly surface runs no backend (spec 027, 3.10.1): for
        // each rules program a case it runs reads, the descriptor the CLI
        // derives from it, which that surface reads instead. A `run` case
        // records it too: the native runner prepares a compiled plan
        // against it (3.12.3). Other cases record none.
        let is_run = command_of(&seen.args) == "run";
        let mut derived = vec![];
        for (i, a) in seen.args.iter().enumerate() {
            if !(pure || is_run) || a != "--rules" {
                continue;
            }
            let Some(name) = seen.args.get(i + 1) else {
                continue;
            };
            let Some(bytes) = seen.inputs.get(name) else {
                continue;
            };
            // The descriptor is written next to the fixtures under this
            // name: only a plain relative path stays inside them.
            assert!(
                Path::new(name)
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
                "--rules {name:?} is not a plain relative path"
            );
            use rustev_core::seams::DecisionBackend;
            let Ok(b) = rustev_backend_rules::RulesBackend::from_bytes(bytes) else {
                continue;
            };
            // A descriptor that does not canonicalize cannot be a case's
            // input: the harvest fails rather than record a case without it.
            let d = rustev_contract::Document::canonical(b.descriptor())
                .unwrap_or_else(|e| panic!("{name}: its derived descriptor: {e}"));
            let rel = format!("{name}.descriptor.json");
            let p = root().join(&fixtures).join(&rel);
            std::fs::write(&p, &d).unwrap();
            derived.push(json!({ "from": name, "path": format!("{fixtures}/{rel}"),
                                 "sha256": hex(&d) }));
        }
        // Shared surface commands run natively too; pure commands also on
        // WebAssembly (spec 027, 3.12.4). Spec 029 repeat commands are
        // currently implemented by the CLI host.
        let repeat = seen.args.iter().any(|a| a == "--repeat-config")
            || matches!(
                command_of(&seen.args).as_str(),
                "baseline record" | "eval reproducibility" | "eval diff" | "gate select"
            );
        // Spec 034 increment 5 exposes parameterized commands only through
        // the CLI; binding exposure belongs to a later increment.
        let parameterized = name.starts_with("parameterized.");
        let surfaces = if repeat || parameterized {
            json!(["cli"])
        } else if pure {
            json!(["cli", "native", "wasm"])
        } else {
            json!(["cli", "native"])
        };
        let path = root().join(format!("tools/rustev-conformance/cases/cli/{id}.json"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let command = command_of(&seen.args);
        // A second golden of the same invocation joins its case.
        // A case file that exists but does not parse is an error, never
        // silently replaced (it would lose the outputs harvested into it).
        let mut case: Json = std::fs::read(&path)
            .ok()
            .map(|b| {
                serde_json::from_slice(&b).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            })
            .unwrap_or_else(|| {
                json!({
                    "id": id,
                    "command": command,
                    "surfaces": surfaces,
                    "derived": derived,
                    "argv": seen.args,
                    "exit": seen.code,
                    "inputs": inputs,
                    "outputs": [],
                })
            });
        // A case file from an earlier harvest must record the same inputs
        // this one copied: otherwise its digests no longer match the
        // fixtures (`make conformance-harvest` starts from no case files).
        assert_eq!(
            case["inputs"],
            json!(inputs),
            "{}: stale case file; rebuild with `make conformance-harvest`",
            path.display()
        );
        assert_eq!(
            case.get("derived").cloned().unwrap_or_else(|| json!([])),
            json!(derived),
            "{}: stale derived descriptors; rebuild with `make conformance-harvest`",
            path.display()
        );
        assert_eq!(
            case["surfaces"],
            surfaces,
            "{}: stale surfaces; rebuild with `make conformance-harvest`",
            path.display()
        );
        let outs = case["outputs"].as_array_mut().unwrap();
        if !outs.iter().any(|o| o["golden"] == output["golden"]) {
            outs.push(output);
        }
        let mut out = serde_json::to_vec_pretty(&case).unwrap();
        out.push(b'\n');
        std::fs::write(&path, out).unwrap();
    });
}
