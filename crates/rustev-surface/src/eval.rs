//! `eval` and `gate` (spec 006, 3.9), and the dataset and bundle handling
//! `calibrate fit` shares, for every host.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::Document;
use rustev_contract::Identified;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::eval_report::{EvalReport, Split};
use rustev_contract::limits::{DESCRIPTOR_V1, RECORD_V1, REPLAY_V1};
use rustev_contract::replay::ReplayBundle;
use rustev_core::Compiled;
use rustev_eval::bundle::{BundleInput, BundleLoad, LoadFailure};
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::dataset::{DatasetCase, DatasetManifest};
use rustev_eval::judge::{JudgeError, judge_scores};
use rustev_eval::measured::Measured;
use rustev_eval::measures::TASK_ADAPTER_V2;
use rustev_eval::metrics::TaskAdapter;
use rustev_eval::report::{
    Candidate, EvalDetail, EvalError, Evaluated, GateResult, Inputs, evaluate, evaluate_gate,
    evaluate_measured_with_views, evaluate_views,
};
use rustev_eval::scores::{ScoreOutcome, ScoreViews, Scores};

use rustev_eval::replay::ReplayConfig;
use rustev_eval::resolve::Resolver;

use crate::adapter::TaskAdapterDoc;
use crate::deps::{self, DepArgs, RulesHook};
use crate::host::{Host, IoFail, ReadFail};
use crate::out::{Done, Out, Res, code};
use crate::plan::{dep_failed, load_plan, read_plan};

/// A split by its name (spec 006, 3.9).
pub fn split_named(name: &str) -> Option<Split> {
    match name {
        "training" => Some(Split::Training),
        "model_selection" => Some(Split::ModelSelection),
        "calibration" => Some(Split::Calibration),
        "final_test" => Some(Split::FinalTest),
        _ => None,
    }
}

/// Read and parse one document of type `T` under `limit`, as
/// `invalid_input` when it is refused.
pub fn read_doc<T: Document>(
    command: &str,
    host: &dyn Host,
    path: &str,
    limit: usize,
) -> Result<T, Done> {
    let bytes = host.read(path, limit).map_err(|e| Done::io(command, e))?;
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
    check_ids(command, dataset, cases.map(|c| c.id.as_str()))
}

/// [`check_case_ids`] over bare ids, as a views document lists them.
fn check_ids<'a>(
    command: &str,
    input: &str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<(), Done> {
    let mut seen = BTreeSet::new();
    for id in ids {
        let ok = !id.is_empty()
            && id.len() <= 128
            && id.as_bytes()[0].is_ascii_alphanumeric()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if !ok {
            return Err(Done::invalid(
                command,
                input,
                format!("case id {id:?} is not a bundle file name"),
            ));
        }
        if !seen.insert(id.to_ascii_lowercase()) {
            return Err(Done::invalid(
                command,
                input,
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
    host: &dyn Host,
    dir: &str,
    cases: impl Iterator<Item = &'a DatasetCase>,
) -> Result<BTreeMap<String, BundleInput>, Done> {
    read_bundles_of(command, host, dir, cases.map(|c| c.id.as_str()))
}

/// [`read_bundles`] over bare case ids, already checked as file names.
fn read_bundles_of<'a>(
    command: &str,
    host: &dyn Host,
    dir: &str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<String, BundleInput>, Done> {
    let mut bundles = BTreeMap::new();
    for id in ids {
        let path = host.join(dir, &format!("{id}.json"));
        let input = match host.read_classified(&path, REPLAY_V1.max_bytes.saturating_add(1)) {
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
        bundles.insert(id.to_string(), input);
    }
    Ok(bundles)
}

/// Write every `(name, bytes)` into `dir`, all or none. The bytes move
/// into the host's list; nothing is copied.
pub fn write_all(
    command: &str,
    host: &dyn Host,
    dir: &str,
    files: Vec<(&str, Vec<u8>)>,
) -> Result<(), Done> {
    let files: Vec<(String, Vec<u8>)> = files
        .into_iter()
        .map(|(n, b)| (host.join(dir, n), b))
        .collect();
    for (p, _) in &files {
        host.ensure_absent(p).map_err(|e| Done::io(command, e))?;
    }
    host.write_all(&files).map_err(|e| Done::io(command, e))
}

pub const OUTPUTS: [&str; 4] = ["report.json", "detail.json", "config.json", "adapter.json"];

/// What `eval` reads and writes.
#[derive(Debug, Clone, Copy)]
pub struct EvalArgs<'a> {
    pub split: Split,
    pub replay: &'a ReplayConfig,
    pub out: &'a str,
    pub dataset: &'a str,
    pub config: &'a str,
    pub adapter: &'a str,
    pub bundles: &'a str,
    pub candidate_plan: Option<&'a str>,
    pub deps: DepArgs<'a>,
}

/// The most `--scores` documents one `eval` reads (spec 041, 3.10.2).
pub const MAX_SCORES: usize = 16;
/// The score views `eval` adds to `out` when any scorer is declared.
pub const VIEWS: &str = "views.json";

/// What `eval` and `eval views` read, checked.
struct Loaded {
    manifest: DatasetManifest,
    config: EvaluatorConfig,
    adapter: TaskAdapterDoc,
    bundles: BTreeMap<String, BundleInput>,
    candidate: Option<(Compiled, Vec<CalibrationArtifact>)>,
}

impl Loaded {
    fn inputs<'a>(&'a self, a: &EvalArgs<'a>, resolver: &'a dyn Resolver) -> Inputs<'a> {
        Inputs {
            manifest: &self.manifest,
            split: a.split,
            config: &self.config,
            adapter: &self.adapter,
            bundles: &self.bundles,
            resolver,
            replay: a.replay,
            candidate: self
                .candidate
                .as_ref()
                .map(|(plan, calibrations)| Candidate { plan, calibrations }),
        }
    }

    fn measured<'a>(&'a self, scores: &'a [Scores]) -> Measured<'a> {
        Measured {
            adapter: &self.adapter,
            scores,
        }
    }
}

fn load(
    command: &str,
    host: &dyn Host,
    a: &EvalArgs<'_>,
    rules: &mut RulesHook<'_>,
) -> Result<Loaded, Done> {
    let dataset_path = a.dataset;
    let manifest: DatasetManifest = read_doc(command, host, dataset_path, REPLAY_V1.max_bytes)?;
    let config_path = a.config;
    let config: EvaluatorConfig = read_doc(command, host, config_path, REPLAY_V1.max_bytes)?;
    config
        .check()
        .map_err(|e| Done::invalid(command, config_path, e.to_string()))?;
    let adapter_path = a.adapter;
    let adapter: TaskAdapterDoc = read_doc(command, host, adapter_path, DESCRIPTOR_V1.max_bytes)?;
    adapter
        .check()
        .map_err(|e| Done::invalid(command, adapter_path, e))?;
    if config.adapter.reference() != adapter.adapter() {
        return Err(Done::invalid(
            command,
            adapter_path,
            "the configuration names another adapter",
        ));
    }
    // Spec 041, 3.10.1: a version 3 configuration's measures are declared
    // by a version 2 adapter.
    if config.declarations().is_some() && adapter.schema != TASK_ADAPTER_V2 {
        return Err(Done::invalid(
            command,
            adapter_path,
            format!("a version 3 configuration needs a {TASK_ADAPTER_V2} adapter"),
        ));
    }
    if config
        .adapter
        .rules
        .as_ref()
        .is_some_and(|r| *r != adapter.rules())
    {
        return Err(Done::invalid(
            command,
            adapter_path,
            "the configuration binds other adapter rules",
        ));
    }
    manifest
        .check(&adapter.adapter(), |l| adapter.check_label(l))
        .map_err(|e| Done::invalid(command, dataset_path, e.to_string()))?;
    check_case_ids(command, dataset_path, manifest.cases.iter())?;
    let bundles = read_bundles(command, host, a.bundles, manifest.split(a.split))?;
    let candidate = match a.candidate_plan {
        None => None,
        Some(path) => {
            let deps = deps::load(host, a.deps, rules).map_err(|e| dep_failed(command, e))?;
            let plan = read_plan(command, host, path)?;
            Some((load_plan(command, &plan, &deps)?, deps.calibrations))
        }
    };
    Ok(Loaded {
        manifest,
        config,
        adapter,
        bundles,
        candidate,
    })
}

/// An evaluation's refusal: a scores document that does not belong to it
/// names `--scores` (spec 041, 3.7.5), anything else the dataset.
fn eval_failed(command: &str, dataset: &str, e: EvalError) -> Done {
    match e {
        EvalError::Scores(code) => Done::invalid(command, "--scores", code),
        e => Done::invalid(command, dataset, e.to_string()),
    }
}

fn declares_scorers(config: &EvaluatorConfig) -> bool {
    config.scorers.as_ref().is_some_and(|s| !s.is_empty())
}

/// `eval`: a report, its detail, the configuration and the adapter,
/// published in `out`.
pub fn eval(
    command: &str,
    host: &dyn Host,
    a: EvalArgs<'_>,
    resolver: &dyn Resolver,
    rules: &mut RulesHook<'_>,
) -> Res {
    eval_scored(command, host, a, &[], resolver, rules)
}

/// [`eval`] with the scores documents of a version 3 configuration's
/// scorers (spec 041, 3.10.2), and its score views added to `out` when any
/// scorer is declared. With no scores and another version this is [`eval`],
/// byte for byte.
pub fn eval_scored(
    command: &str,
    host: &dyn Host,
    a: EvalArgs<'_>,
    scores: &[String],
    resolver: &dyn Resolver,
    rules: &mut RulesHook<'_>,
) -> Res {
    let split = a.split;
    (|| {
        let out_dir = a.out;
        for name in OUTPUTS {
            host.ensure_absent(&host.join(out_dir, name))
                .map_err(|e| Done::io(command, e))?;
        }
        let l = load(command, host, &a, rules)?;
        let measured = l.config.declarations().is_some();
        if !measured && !scores.is_empty() {
            return Err(Done::invalid(
                command,
                a.config,
                "--scores needs a version 3 configuration",
            ));
        }
        if scores.len() > MAX_SCORES {
            return Err(Done::invalid(
                command,
                "--scores",
                format!("at most {MAX_SCORES} scores documents"),
            ));
        }
        let with_views = declares_scorers(&l.config);
        if with_views {
            host.ensure_absent(&host.join(out_dir, VIEWS))
                .map_err(|e| Done::io(command, e))?;
        }
        let scores = scores
            .iter()
            .map(|path| read_doc::<Scores>(command, host, path, REPLAY_V1.max_bytes))
            .collect::<Result<Vec<_>, _>>()?;
        let inputs = l.inputs(&a, resolver);
        let (evaluated, views) = if measured {
            evaluate_measured_with_views(&inputs, l.measured(&scores))
        } else {
            evaluate(&inputs).map(|e| (e, None))
        }
        .map_err(|e| eval_failed(command, a.dataset, e))?;
        let bytes = |r: Result<Vec<u8>, rustev_contract::canonical::CanonicalError>| {
            r.map_err(|e| Done::invalid(command, out_dir, e.to_string()))
        };
        let report_bytes = bytes(evaluated.report.record_canonical())?;
        let detail_bytes = bytes(evaluated.detail.record_canonical())?;
        let config_bytes = bytes(evaluated.config.canonical())?;
        let adapter_bytes = bytes(l.adapter.canonical())?;
        let metrics = bytes(record_canonical_bytes(&evaluated.report.metrics))?;
        let mut files = vec![
            (OUTPUTS[0], report_bytes),
            (OUTPUTS[1], detail_bytes),
            (OUTPUTS[2], config_bytes),
            (OUTPUTS[3], adapter_bytes),
        ];
        if with_views {
            let views = views
                .ok_or_else(|| Done::invalid(command, a.config, "score views need version 3"))?;
            files.push((VIEWS, bytes(views.record_canonical())?));
        }
        write_all(command, host, out_dir, files)?;
        let digest = |r: Result<rustev_contract::ids::ContentDigest, _>| {
            r.map(|d| d.to_string()).unwrap_or_default()
        };
        Ok(Done::ok(
            Out::new(command, "reported")
                .set("report_digest", &digest(evaluated.report.record_digest()))
                .set("dataset", &evaluated.report.dataset.id)
                .set("split", rustev_eval::report::split_name(split))
                .set("config", &evaluated.report.evaluator_config)
                .set("adapter_digest", &digest(l.adapter.record_digest()))
                .set("candidate", &l.candidate.is_some())
                .set("out", out_dir)
                .raw("metrics", metrics),
        ))
    })()
}

/// `eval views` (spec 041, 3.10.3): the score views of the evaluation
/// `eval` would run over the same inputs, written to the file `out`.
pub fn views(
    command: &str,
    host: &dyn Host,
    a: EvalArgs<'_>,
    resolver: &dyn Resolver,
    rules: &mut RulesHook<'_>,
) -> Res {
    (|| {
        host.ensure_absent(a.out)
            .map_err(|e| Done::io(command, e))?;
        let l = load(command, host, &a, rules)?;
        if l.config.declarations().is_none() {
            return Err(Done::invalid(
                command,
                a.config,
                "score views need a version 3 configuration",
            ));
        }
        let views = evaluate_views(&l.inputs(&a, resolver), l.measured(&[]))
            .map_err(|e| eval_failed(command, a.dataset, e))?;
        let bytes = views
            .record_canonical()
            .map_err(|e| Done::invalid(command, a.out, e.to_string()))?;
        host.write_all(&[(a.out.to_string(), bytes)])
            .map_err(|e| Done::io(command, e))?;
        let digest = views
            .record_digest()
            .map(|d| d.to_string())
            .unwrap_or_default();
        Ok(Done::ok(
            Out::new(command, "viewed")
                .set("views_digest", &digest)
                .set("cases", &views.cases.len())
                .set("out", a.out),
        ))
    })()
}

/// What `eval judge` reads and writes (spec 041, 3.10.4).
#[derive(Debug, Clone, Copy)]
pub struct JudgeArgs<'a> {
    pub config: &'a str,
    pub views: &'a str,
    pub judge_plan: &'a str,
    pub deps: DepArgs<'a>,
    pub bundles: &'a str,
    pub scorer: &'a str,
    pub replay: &'a ReplayConfig,
    pub out: &'a str,
}

/// `eval judge` (spec 041, 3.10.4): reproduce the judge's bundles for each
/// view entry and write the judge scorer's `rustev.scores/1` to `out`.
pub fn judge(
    command: &str,
    host: &dyn Host,
    a: JudgeArgs<'_>,
    resolver: &dyn Resolver,
    rules: &mut RulesHook<'_>,
) -> Res {
    (|| {
        host.ensure_absent(a.out)
            .map_err(|e| Done::io(command, e))?;
        let config: EvaluatorConfig = read_doc(command, host, a.config, REPLAY_V1.max_bytes)?;
        config
            .check()
            .map_err(|e| Done::invalid(command, a.config, e.to_string()))?;
        if config.declarations().is_none() {
            return Err(Done::invalid(
                command,
                a.config,
                "eval judge needs a version 3 configuration",
            ));
        }
        let Some(descriptor) = config.scorers.iter().flatten().find(|s| s.id == a.scorer) else {
            return Err(Done::invalid(
                command,
                a.config,
                format!("the configuration declares no scorer {:?}", a.scorer),
            ));
        };
        let views: ScoreViews = read_doc(command, host, a.views, REPLAY_V1.max_bytes)?;
        let config_id = config
            .id()
            .map_err(|e| Done::invalid(command, a.config, e.to_string()))?;
        if views.config != config_id {
            return Err(Done::invalid(
                command,
                a.views,
                "the views were computed under another configuration",
            ));
        }
        let deps = deps::load(host, a.deps, rules).map_err(|e| dep_failed(command, e))?;
        let plan = read_plan(command, host, a.judge_plan)?;
        let plan = load_plan(command, &plan, &deps)?;
        let ids = views.cases.iter().map(|e| e.case.as_str());
        check_ids(command, a.views, ids.clone())?;
        let bundles = read_bundles_of(command, host, a.bundles, ids)?;
        let scores = judge_scores(&views, descriptor, &plan.id, &bundles, resolver, a.replay)
            .map_err(|e| {
                let input = match e {
                    JudgeError::NotAJudge(_) => a.config,
                    _ => a.bundles,
                };
                Done::invalid(command, input, e.to_string())
            })?;
        let bytes = scores
            .record_canonical()
            .map_err(|e| Done::invalid(command, a.out, e.to_string()))?;
        host.write_all(&[(a.out.to_string(), bytes)])
            .map_err(|e| Done::io(command, e))?;
        let mut outcomes: BTreeMap<&str, u64> = BTreeMap::new();
        for e in &scores.entries {
            let key = match &e.outcome {
                ScoreOutcome::Score(_) => "scored",
                ScoreOutcome::Unscored(reason) => reason.as_str(),
            };
            *outcomes.entry(key).or_default() += 1;
        }
        let digest = scores
            .record_digest()
            .map(|d| d.to_string())
            .unwrap_or_default();
        Ok(Done::ok(
            Out::new(command, "scored")
                .set("scores_digest", &digest)
                .set("scorer", a.scorer)
                .set("outcomes", &outcomes)
                .set("out", a.out),
        ))
    })()
}

/// One side of a gate: the four documents `eval` wrote.
fn read_side(command: &str, host: &dyn Host, dir: &str) -> Result<(Evaluated, Vec<u8>), Done> {
    let report: EvalReport = read_doc(
        command,
        host,
        &host.join(dir, OUTPUTS[0]),
        RECORD_V1.max_bytes,
    )?;
    let detail: EvalDetail = read_doc(
        command,
        host,
        &host.join(dir, OUTPUTS[1]),
        REPLAY_V1.max_bytes,
    )?;
    let config: EvaluatorConfig = read_doc(
        command,
        host,
        &host.join(dir, OUTPUTS[2]),
        REPLAY_V1.max_bytes,
    )?;
    let adapter_path = host.join(dir, OUTPUTS[3]);
    let adapter = host
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

/// `gate`: one declared gate of the candidate's configuration over the two
/// sides `eval` wrote.
pub fn gate(command: &str, host: &dyn Host, baseline: &str, candidate: &str, name: &str) -> Res {
    (|| {
        let (baseline, badapter) = read_side(command, host, baseline)?;
        let (candidate, cadapter) = read_side(command, host, candidate)?;
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
    })()
}

/// Identity of a document, as text, for output.
pub fn id_of<T: Identified>(doc: &T) -> String
where
    T::Id: std::fmt::Display,
{
    doc.id().map(|i| i.to_string()).unwrap_or_default()
}
