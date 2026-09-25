//! `rustev eval` and `rustev gate` (spec 006, 3.9), and the dataset and
//! bundle handling `calibrate fit` shares.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::Document;
use rustev_contract::Identified;
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::eval_report::{EvalReport, Split};
use rustev_contract::limits::{DESCRIPTOR_V1, RECORD_V1, REPLAY_V1};
use rustev_contract::replay::ReplayBundle;
use rustev_eval::bundle::{BundleInput, BundleLoad, LoadFailure};
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::dataset::{DatasetCase, DatasetManifest};
use rustev_eval::metrics::TaskAdapter;
use rustev_eval::report::{
    Candidate, EvalDetail, Evaluated, GateResult, Inputs, evaluate, evaluate_gate,
};

use crate::adapter::TaskAdapterDoc;
use crate::args::{Parsed, Usage, usage};
use crate::deps;
use crate::io::{self, IoFail, ReadFail, Reads};
use crate::out::{Done, Out, Res, code};
use crate::plan::{dep_failed, load_plan, read_plan};
use crate::replay::{replay_config, resolver};

pub fn split(p: &Parsed) -> Result<Split, Usage> {
    match p.req("split") {
        "training" => Ok(Split::Training),
        "model_selection" => Ok(Split::ModelSelection),
        "calibration" => Ok(Split::Calibration),
        "final_test" => Ok(Split::FinalTest),
        other => usage(format!(
            "--split is training, model_selection, calibration or final_test, not {other:?}"
        )),
    }
}

/// Read and parse one document of type `T` under `limit`, as
/// `invalid_input` when it is refused.
pub fn read_doc<T: Document>(
    command: &str,
    reads: &Reads,
    path: &str,
    limit: usize,
) -> Result<T, Done> {
    let bytes = reads.read(path, limit).map_err(|e| Done::io(command, e))?;
    T::parse(&bytes).map_err(|e| Done::invalid(command, path, e.to_string()))
}

/// Case ids that map to bundle files (spec 006, 3.9.1): 1 to 128 bytes of
/// `[A-Za-z0-9._-]` starting with an alphanumeric, distinct ignoring ASCII
/// case.
pub fn check_case_ids<'a>(
    command: &str,
    dataset: &str,
    cases: impl Iterator<Item = &'a DatasetCase>,
) -> Result<(), Done> {
    let mut seen = BTreeSet::new();
    for c in cases {
        let id = c.id.as_str();
        let ok = !id.is_empty()
            && id.len() <= 128
            && id.as_bytes()[0].is_ascii_alphanumeric()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if !ok {
            return Err(Done::invalid(
                command,
                dataset,
                format!("case id {id:?} is not a bundle file name"),
            ));
        }
        if !seen.insert(id.to_ascii_lowercase()) {
            return Err(Done::invalid(
                command,
                dataset,
                format!("case id {id:?} differs from another only by case"),
            ));
        }
    }
    Ok(())
}

/// The bundles of `cases` from `dir` (spec 015, 3.3): absent files are left
/// out for the library to judge (`bundle-missing`); a file that exists but
/// cannot be read, is past the document limit or does not parse is
/// supplied as that case's load failure, and the command continues. Only
/// an exhausted read budget stops it, because that depends on read order
/// rather than on the bundle.
pub fn read_bundles<'a>(
    command: &str,
    reads: &Reads,
    dir: &str,
    cases: impl Iterator<Item = &'a DatasetCase>,
) -> Result<BTreeMap<String, BundleInput>, Done> {
    let mut bundles = BTreeMap::new();
    for c in cases {
        let path = io::join(dir, &format!("{}.json", c.id));
        let input = match reads.read_classified(&path, REPLAY_V1.max_bytes.saturating_add(1)) {
            Ok(None) => continue,
            Err(ReadFail::Budget(e)) => return Err(Done::io(command, e)),
            Err(ReadFail::File(e)) => LoadFailure::new(BundleLoad::Inaccessible, &e.detail).into(),
            Ok(Some(bytes)) if bytes.len() > REPLAY_V1.max_bytes => LoadFailure::new(
                BundleLoad::Oversized,
                &format!("more than {} bytes", REPLAY_V1.max_bytes),
            )
            .into(),
            Ok(Some(bytes)) => match ReplayBundle::parse(&bytes) {
                Ok(b) => b.into(),
                Err(e) => LoadFailure::new(BundleLoad::Corrupt, &e.to_string()).into(),
            },
        };
        bundles.insert(c.id.clone(), input);
    }
    Ok(bundles)
}

/// Write every `(name, bytes)` into `dir`, all or none.
pub fn write_all(command: &str, dir: &str, files: &[(&str, Vec<u8>)]) -> Result<(), Done> {
    let paths: Vec<String> = files.iter().map(|(n, _)| io::join(dir, n)).collect();
    for p in &paths {
        io::ensure_absent(p).map_err(|e| Done::io(command, e))?;
    }
    for (i, (p, (_, bytes))) in paths.iter().zip(files).enumerate() {
        if let Err(e) = io::create(p, bytes) {
            for written in &paths[..i] {
                let _ = std::fs::remove_file(written);
            }
            return Err(Done::io(command, e));
        }
    }
    Ok(())
}

pub const OUTPUTS: [&str; 4] = ["report.json", "detail.json", "config.json", "adapter.json"];

pub fn eval(p: &Parsed) -> Result<Res, Usage> {
    let split = split(p)?;
    let replay = replay_config(p)?;
    if !p.has("candidate-plan") {
        p.forbid(
            &["descriptor", "rules", "calibration"],
            "describes a candidate; give --candidate-plan",
        )?;
    }
    let command = &p.name();
    Ok((|| {
        let out_dir = p.req("out");
        for name in OUTPUTS {
            io::ensure_absent(&io::join(out_dir, name)).map_err(|e| Done::io(command, e))?;
        }
        let reads = Reads::default();
        let dataset_path = p.req("dataset");
        let manifest: DatasetManifest =
            read_doc(command, &reads, dataset_path, REPLAY_V1.max_bytes)?;
        let config_path = p.req("config");
        let config: EvaluatorConfig = read_doc(command, &reads, config_path, REPLAY_V1.max_bytes)?;
        config
            .check()
            .map_err(|e| Done::invalid(command, config_path, e.to_string()))?;
        let adapter_path = p.req("adapter");
        let adapter: TaskAdapterDoc =
            read_doc(command, &reads, adapter_path, DESCRIPTOR_V1.max_bytes)?;
        adapter
            .check()
            .map_err(|e| Done::invalid(command, adapter_path, e))?;
        if config.adapter != adapter.adapter() {
            return Err(Done::invalid(
                command,
                adapter_path,
                "the configuration names another adapter",
            ));
        }
        manifest
            .check(&adapter.adapter(), |l| adapter.check_label(l))
            .map_err(|e| Done::invalid(command, dataset_path, e.to_string()))?;
        check_case_ids(command, dataset_path, manifest.cases.iter())?;
        let bundles = read_bundles(command, &reads, p.req("bundles"), manifest.split(split))?;
        let candidate = match p.one("candidate-plan") {
            None => None,
            Some(path) => {
                let deps = deps::load(p, &reads).map_err(|e| dep_failed(command, e))?;
                let plan = read_plan(command, &reads, path)?;
                Some((load_plan(command, &plan, &deps)?, deps.calibrations))
            }
        };
        let resolver = resolver(p);
        let evaluated = evaluate(&Inputs {
            manifest: &manifest,
            split,
            config: &config,
            adapter: &adapter,
            bundles: &bundles,
            resolver: resolver.as_ref(),
            replay: &replay,
            candidate: candidate
                .as_ref()
                .map(|(plan, calibrations)| Candidate { plan, calibrations }),
        })
        .map_err(|e| Done::invalid(command, dataset_path, e.to_string()))?;
        let bytes = |r: Result<Vec<u8>, rustev_contract::canonical::CanonicalError>| {
            r.map_err(|e| Done::invalid(command, out_dir, e.to_string()))
        };
        let report_bytes = bytes(evaluated.report.record_canonical())?;
        let detail_bytes = bytes(evaluated.detail.record_canonical())?;
        let config_bytes = bytes(evaluated.config.canonical())?;
        let adapter_bytes = bytes(adapter.canonical())?;
        let metrics = bytes(record_canonical_bytes(&evaluated.report.metrics))?;
        write_all(
            command,
            out_dir,
            &[
                (OUTPUTS[0], report_bytes),
                (OUTPUTS[1], detail_bytes),
                (OUTPUTS[2], config_bytes),
                (OUTPUTS[3], adapter_bytes),
            ],
        )?;
        let digest = |r: Result<rustev_contract::ids::ContentDigest, _>| {
            r.map(|d| d.to_string()).unwrap_or_default()
        };
        Ok(Done::ok(
            Out::new(command, "reported")
                .set("report_digest", &digest(evaluated.report.record_digest()))
                .set("dataset", &evaluated.report.dataset.id)
                .set("split", rustev_eval::report::split_name(split))
                .set("config", &evaluated.report.evaluator_config)
                .set("adapter_digest", &digest(adapter.record_digest()))
                .set("candidate", &candidate.is_some())
                .set("out", out_dir)
                .raw("metrics", metrics),
        ))
    })())
}

/// One side of a gate: the four documents `eval` wrote.
fn read_side(command: &str, reads: &Reads, dir: &str) -> Result<(Evaluated, Vec<u8>), Done> {
    let report: EvalReport = read_doc(
        command,
        reads,
        &io::join(dir, OUTPUTS[0]),
        RECORD_V1.max_bytes,
    )?;
    let detail: EvalDetail = read_doc(
        command,
        reads,
        &io::join(dir, OUTPUTS[1]),
        REPLAY_V1.max_bytes,
    )?;
    let config: EvaluatorConfig = read_doc(
        command,
        reads,
        &io::join(dir, OUTPUTS[2]),
        REPLAY_V1.max_bytes,
    )?;
    let adapter_path = io::join(dir, OUTPUTS[3]);
    let adapter = reads
        .read(&adapter_path, DESCRIPTOR_V1.max_bytes)
        .map_err(|e: IoFail| Done::io(command, e))?;
    TaskAdapterDoc::parse(&adapter)
        .map_err(|e| Done::invalid(command, &adapter_path, e.to_string()))?;
    Ok((
        Evaluated {
            report,
            detail,
            config,
        },
        adapter,
    ))
}

pub fn gate(p: &Parsed) -> Result<Res, Usage> {
    let command = &p.name();
    Ok((|| {
        let reads = Reads::default();
        let (baseline, badapter) = read_side(command, &reads, p.req("baseline"))?;
        let (candidate, cadapter) = read_side(command, &reads, p.req("candidate"))?;
        let name = p.req("gate");
        let result = if badapter != cadapter {
            GateResult::Unknown {
                reason: "the two sides were evaluated with different task adapters".into(),
            }
        } else {
            match candidate.config.gates.iter().find(|g| g.name == name) {
                None => GateResult::Unknown {
                    reason: format!("the configuration declares no gate {name:?}"),
                },
                Some(g) => evaluate_gate(g, &baseline, &candidate),
            }
        };
        let out = Out::new(command, "").set("gate", name);
        Ok(match result {
            GateResult::Pass => Done::ok(out.set("status", "pass")),
            GateResult::Fail { reason } => {
                Done::new(code::FAIL, out.set("status", "fail").set("reason", &reason))
            }
            GateResult::Unknown { reason } => Done::new(
                code::UNKNOWN,
                out.set("status", "unknown").set("reason", &reason),
            ),
        })
    })())
}

/// Identity of a document, as text, for output.
pub fn id_of<T: Identified>(doc: &T) -> String
where
    T::Id: std::fmt::Display,
{
    doc.id().map(|i| i.to_string()).unwrap_or_default()
}
