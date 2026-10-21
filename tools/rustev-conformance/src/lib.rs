//! The shared conformance suite (spec 027, 3.12): the case manifest, its
//! fixtures, and the Rust runner that drives the CLI through its library
//! entry point. Every host surface runs these cases and must produce each
//! case's expected bytes under only the comparison the case declares.
#![forbid(unsafe_code)]

pub mod normalize;
pub mod runtime;

use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The manifest's schema.
pub const MANIFEST_SCHEMA: &str = "rustev.conformance-manifest/1";

/// `cases/manifest.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub cases: Vec<Case>,
}

/// One case: a command, its inputs and its expected outputs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub command: String,
    /// The surfaces the case applies to (`cli`; the bindings add theirs).
    pub surfaces: Vec<String>,
    /// The CLI's argument vector; inputs and outputs are named relative to
    /// the directory the case runs in. Absent for a runtime case.
    #[serde(default)]
    pub argv: Vec<String>,
    /// The CLI's exit code, presentation the bindings report as their own
    /// result kind (3.1.3). Absent for a runtime case.
    #[serde(default)]
    pub exit: Option<i32>,
    /// A runtime case: the test under `crates/rustev-runtime/tests` it
    /// mirrors, and its decision expressed as data (3.12.4).
    #[serde(default)]
    pub mirrors: Option<String>,
    #[serde(default)]
    pub runtime: Option<serde_json::Value>,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
}

/// An input: its repository path, the name the case reads it by, and the
/// SHA-256 of its bytes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub path: String,
    pub name: String,
    pub sha256: String,
}

/// An expected output: printed (`stream`) or written (`file`), the golden
/// it must equal (repository path and SHA-256), and the normalizations of
/// [`normalize`] applied first (empty: the bytes themselves).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    #[serde(default)]
    pub stream: Option<String>,
    #[serde(default)]
    pub file: Option<String>,
    pub golden: String,
    pub sha256: String,
    pub normalize: Vec<String>,
}

/// The repository root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// The manifest the case files make: every `cases/cli/*.json` (harvested)
/// and `cases/runtime/*.json` (authored), sorted by id, as canonical
/// pretty JSON.
pub fn aggregate() -> Result<Vec<u8>, String> {
    let mut cases = vec![];
    for dir in ["cli", "runtime"] {
        let d = root().join("tools/rustev-conformance/cases").join(dir);
        // Each case directory exists whenever the suite does; a missing one
        // is an error naming it, never a silently smaller manifest.
        let entries = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for e in entries {
            let p = e.map_err(|e| format!("{}: {e}", d.display()))?.path();
            if p.extension().is_some_and(|x| x == "json") {
                let v: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?,
                )
                .map_err(|e| format!("{}: {e}", p.display()))?;
                cases.push(v);
            }
        }
    }
    cases.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let m = serde_json::json!({ "schema": MANIFEST_SCHEMA, "cases": cases });
    let mut out = serde_json::to_vec_pretty(&m).map_err(|e| e.to_string())?;
    out.push(b'\n');
    Ok(out)
}

/// The committed manifest, parsed and checked against its schema.
pub fn manifest() -> Result<Manifest, String> {
    let path = root().join("tools/rustev-conformance/cases/manifest.json");
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let m: Manifest = serde_json::from_slice(&bytes).map_err(|e| format!("manifest: {e}"))?;
    if m.schema != MANIFEST_SCHEMA {
        return Err(format!("manifest schema {:?}", m.schema));
    }
    Ok(m)
}

/// Read a repository file whose digest the manifest pins.
/// `path` as a relative path made only of normal components: a case's
/// paths and input names stay inside the directory they are joined to.
pub fn contained(path: &str) -> Result<&std::path::Path, String> {
    let p = std::path::Path::new(path);
    let normal = p
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    if path.is_empty() || !normal {
        return Err(format!(
            "{path:?}: not a relative path inside its directory"
        ));
    }
    Ok(p)
}

pub fn pinned(path: &str, sha256: &str) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(root().join(contained(path)?)).map_err(|e| format!("{path}: {e}"))?;
    let got = sha256_hex(&bytes);
    if got != sha256 {
        return Err(format!("{path}: sha256 {got}, the manifest pins {sha256}"));
    }
    Ok(bytes)
}

/// Every difference between what a surface produced and what the case
/// expects. Empty is the passing verdict.
pub fn compare(case: &Case, exit: i32, stdout: &[u8], dir: &Path) -> Vec<String> {
    let mut v = vec![];
    if case.exit.is_some_and(|e| e != exit) {
        v.push(format!(
            "{}: exit {exit}, expected {:?}",
            case.id, case.exit
        ));
    }
    for o in &case.outputs {
        let golden = match pinned(&o.golden, &o.sha256) {
            Ok(b) => b,
            Err(e) => {
                v.push(format!("{}: {e}", case.id));
                continue;
            }
        };
        let produced = match (&o.stream, &o.file) {
            (Some(s), None) if s == "stdout" => Some(stdout.to_vec()),
            (None, Some(f)) => {
                // An output path stays inside the case's directory.
                let path = match contained(f) {
                    Ok(p) => dir.join(p),
                    Err(e) => {
                        v.push(format!("{}: {e}", case.id));
                        continue;
                    }
                };
                match std::fs::read(path) {
                    Ok(b) => Some(b),
                    // Not written is the "no output" report below; any
                    // other failure is reported as itself.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => {
                        v.push(format!("{}: {f}: {e}", case.id));
                        continue;
                    }
                }
            }
            (Some(s), None) => {
                v.push(format!(
                    "{}: stream {s:?} is not one a case can pin (stdout)",
                    case.id
                ));
                continue;
            }
            (Some(_), Some(_)) => {
                v.push(format!(
                    "{}: an output names a stream and a file; one only",
                    case.id
                ));
                continue;
            }
            (None, None) => {
                v.push(format!(
                    "{}: an output names neither a stream nor a file",
                    case.id
                ));
                continue;
            }
        };
        let Some(produced) = produced else {
            v.push(format!("{}: no output for {}", case.id, o.golden));
            continue;
        };
        match normalize::apply(&o.normalize, &produced) {
            None => v.push(format!(
                "{}: normalizations {:?} do not apply to the output for {}",
                case.id, o.normalize, o.golden
            )),
            Some(n) if n != golden => v.push(format!("{}: {} differs", case.id, o.golden)),
            Some(_) => {}
        }
    }
    v
}
