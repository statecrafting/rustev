//! Shared helpers for the CLI tests. SYNTHETIC only (R-04): the reference
//! definitions come from the core's shared builders and the rules programs
//! from the rules backend's fixtures, emitted into a scratch directory at
//! test time, so there is one source of each (R-02).
#![allow(dead_code)]

#[path = "../../../rustev-core/tests/common/mod.rs"]
pub mod core;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rustev_backend_rules::RulesBackend;
use rustev_contract::calibration::{
    CalibrationArtifact, CalibrationBinding, CalibrationMethod, CalibrationParameters,
};
use rustev_contract::definition::Definition;
use rustev_contract::ids::{ArtifactId, DatasetId};
use rustev_contract::{Document, schema};
use serde_json::Value as Json;

pub use self::core::*;

/// A scratch directory, removed when dropped.
pub struct Scratch(pub PathBuf);

static NEXT: AtomicUsize = AtomicUsize::new(0);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "rustev-cli-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> String {
        let p = self.path(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, bytes).unwrap();
        name.to_string()
    }

    pub fn read(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.path(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).exists()
    }

    /// Run the `rustev` binary here.
    pub fn rustev(&self, args: &[&str]) -> Ran {
        let o = Command::new(env!("CARGO_BIN_EXE_rustev"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap();
        Ran {
            code: o.status.code().unwrap_or(-1),
            stdout: o.stdout,
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One finished invocation of the binary.
#[derive(Debug)]
pub struct Ran {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Ran {
    pub fn json(&self) -> Json {
        serde_json::from_slice(&self.stdout)
            .unwrap_or_else(|e| panic!("{e}: {:?} {}", self.stdout_str(), self.stderr))
    }

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn status(&self) -> String {
        self.json()["status"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    #[track_caller]
    pub fn expect(&self, code: i32, status: &str) -> Json {
        assert_eq!(
            (self.code, self.status().as_str()),
            (code, status),
            "stdout {} stderr {}",
            self.stdout_str(),
            self.stderr
        );
        self.json()
    }
}

pub fn rules_fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../backends/rustev-backend-rules/tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn support_rules() -> RulesBackend {
    RulesBackend::from_bytes(&rules_fixture("support-routing.rules.json")).unwrap()
}

pub fn lodging_rules() -> RulesBackend {
    RulesBackend::from_bytes(&rules_fixture("lodging.rules.json")).unwrap()
}

/// A SYNTHETIC, unfitted temperature calibration for `topic`, bound to the
/// rules program's artifact.
pub fn rules_calibration(artifact: &ArtifactId, temperature: &str) -> CalibrationArtifact {
    CalibrationArtifact {
        schema: schema::CALIBRATION.into(),
        binding: CalibrationBinding {
            artifact: artifact.clone(),
            task: "support.topic".into(),
            question: "topic".into(),
            dataset: DatasetId::parse(&id('d')).unwrap(),
            method: CalibrationMethod::Temperature1,
        },
        options: strings(&TOPICS),
        parameters: CalibrationParameters::Temperature {
            temperature: dec(temperature),
        },
    }
}

pub fn canonical<T: Document>(doc: &T) -> Vec<u8> {
    doc.canonical().unwrap()
}

/// The support-routing definition bound to the rules program's topic
/// calibration.
pub fn support_definition() -> (Definition, CalibrationArtifact) {
    let cal = rules_calibration(support_rules().artifact(), "1.5");
    (support_routing_builder(&cal).build().unwrap(), cal)
}

/// The reference inputs of both tasks, emitted into `s`:
/// `support.definition.json`, `support.rules.json`, `topic.calibration.json`,
/// `lodging.definition.json` and `lodging.rules.json`.
pub fn reference_inputs(s: &Scratch) {
    let (def, cal) = support_definition();
    s.write("support.definition.json", &canonical(&def));
    s.write("topic.calibration.json", &canonical(&cal));
    s.write(
        "support.rules.json",
        &rules_fixture("support-routing.rules.json"),
    );
    s.write("lodging.definition.json", &canonical(&lodging()));
    s.write("lodging.rules.json", &rules_fixture("lodging.rules.json"));
}

/// Compare `bytes` with the committed CLI golden `name`; `RUSTEV_BLESS=1`
/// rewrites it (goldens are emitted, R-02).
pub fn cli_golden(name: &str, bytes: &[u8]) {
    golden(name, bytes)
}

/// The support-routing snapshot the rules backend routes to billing.
pub fn support_snapshot_default() -> rustev_contract::snapshot::Snapshot {
    support_snapshot("pro", &[2, 5], 0)
}

pub fn lodging_snapshot_default() -> rustev_contract::snapshot::Snapshot {
    lodging_snapshot(
        true,
        vec![
            candidate("c-1", "120", "USD", 2, true),
            candidate("c-2", "90", "EUR", 4, false),
            candidate("c-3", "250", "USD", 2, true),
        ],
        vec![
            claim("k-1", "verified", "preference", NOW + DAY),
            claim("k-2", "stated", "preference", NOW + DAY),
        ],
    )
}

pub const NOW_ARG: &str = "1790164800000";
/// Host wall-clock time for bundle creation and replay; unrelated to domain
/// time.
pub const WALL: i64 = 1_800_000_000_000;

/// Everything `run` needs for both tasks, compiled through the CLI:
/// `support.plan.json`, `lodging.plan.json` and the snapshots.
pub fn run_inputs(s: &Scratch) {
    reference_inputs(s);
    s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "support.definition.json",
        "--rules",
        "support.rules.json",
        "--calibration",
        "topic.calibration.json",
        "--out",
        "support.plan.json",
    ])
    .expect(0, "compiled");
    s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "lodging.definition.json",
        "--rules",
        "lodging.rules.json",
        "--out",
        "lodging.plan.json",
    ])
    .expect(0, "compiled");
    s.write(
        "support.snapshot.json",
        &canonical(&support_snapshot_default()),
    );
    s.write(
        "lodging.snapshot.json",
        &canonical(&lodging_snapshot_default()),
    );
}

/// The `run` arguments of one task, before any optional flag.
pub fn run_args(task: &str, decision: &str, record: &str) -> Vec<String> {
    let mut v: Vec<String> = [
        "run",
        "--plan",
        &format!("{task}.plan.json"),
        "--rules",
        &format!("{task}.rules.json"),
        "--snapshot",
        &format!("{task}.snapshot.json"),
        "--evaluation-time",
        NOW_ARG,
        "--decision-id",
        decision,
        "--record-out",
        record,
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    if task == "support" {
        v.extend(["--calibration".into(), "topic.calibration.json".into()]);
    }
    v
}

impl Scratch {
    pub fn rustev_owned(&self, args: &[String]) -> Ran {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        self.rustev(&a)
    }
}

/// Zero the runtime's elapsed-time fields of a run record's JSON, the only
/// fields a golden normalizes (with the digests computed over them).
pub fn normalize_timing(record: &mut Json) {
    record["timing"]["elapsed_ms"] = 0.into();
    record["timing"]["queued_ms"] = 0.into();
    for r in record["requests"].as_array_mut().into_iter().flatten() {
        for a in r["attempts"].as_array_mut().into_iter().flatten() {
            a["dispatched_ms"] = 0.into();
            a["ended_ms"] = 0.into();
        }
    }
}
