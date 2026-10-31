//! Spec 039's swap test (Acceptance, 3.17): one definition with a
//! pointwise `rank` step and a `select` with backfill, run through
//! `rustev-runtime` with spec 005's rules backend (3.14) and with this
//! adapter answering from documented-shape SYNTHETIC fixtures on loopback.
//! Each run is captured and evaluated: the spec 004 report, spec 041's
//! ranking measures (E1) and spec 029's repeat report (E2). Mechanics only
//! (R-04): no report here is a quality claim, and no network is used.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use rustev_backend_rules::RulesBackend;
use rustev_bedrock::{
    BedrockBackend,
    adapter::Config,
    binding::{BedrockBinding, OperationBinding, RANK_MAPPING},
    budget::Budget,
    signing::{CredentialError, CredentialProvider},
};
use rustev_contract::canonical::{canonical_value_bytes, tagged_digest};
use rustev_contract::definition::Definition;
use rustev_contract::judgment::Outcome;
use rustev_contract::run::RunRecord;
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, Identified};
use rustev_core::Compiled;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_runtime::{
    CaptureConfig, CaptureOutcome, Completion, DecisionRequest, MAX_CAPTURE_BYTES, ManualClock,
    Runtime, RuntimeConfig, SinkPolicy,
};
use serde_json::{Value, json};

#[path = "../../../crates/rustev-core/tests/common/mod.rs"]
mod reference;

const TENANT: &str = "SYNTHETIC-tenant";
const REVISION: &str = "rev-1";
const PRINCIPAL: &str = "scope-p";
/// Case id, then each candidate's id and price.
const CASES: [(&str, [(&str, &str); 3]); 3] = [
    ("r-1", [("a-1", "5"), ("a-2", "12"), ("a-3", "8")]),
    ("r-2", [("b-1", "20"), ("b-2", "3"), ("b-3", "10")]),
    ("r-3", [("c-1", "4"), ("c-2", "6"), ("c-3", "1")]),
];

/// Pointwise `rank` over the filtered pool, then `select` k 2 backfilled
/// from the pool's exact order; the policy proposes the selection.
fn definition() -> Definition {
    let (mut d, _) = reference::rank::fixture();
    d.inputs = vec![serde_json::from_value(json!({"name":"items","type":{"list":{"item":{"record":{"fields":[{"name":"id","type":{"text":{"max_bytes":8}}},{"name":"price","type":"decimal"}]}},"max_items":5}},"provenance":["system-of-record"],"freshness":"not_required"})).unwrap()];
    d.steps = serde_json::from_value(json!([
        {"id":"pool","body":{"exact":{"op":"filter_with_reasons","version":1,"args":{"list":"input:items","id_field":"id","rules":[]}}}},
        {"id":"ranked","body":{"rank":{"task":"fit","question":"Which fits?","candidates":"step:pool","items":{"list":{"list":"input:items","id_field":"id"}},"bind":"candidate","project":[],"mode":"pointwise","bias":"none","tie_break":"id_ascending","backend":{"min_determinism":"unspecified","artifact":"any","binding":"auto"},"fallback":"none"}}},
        {"id":"chosen","body":{"select":{"from":"step:ranked","k":2,"backfill":{"from":"step:pool","exclude_selected":true},"on_backend_failure":"backfill_all"}}}
    ])).unwrap();
    d.policy = serde_json::from_value(json!({"on_unresolved":[{"target":"step:chosen","reasons":"any","action":"propagate"}],"rules":[{"id":"present","when":"always","then":{"propose":{"action":"recommend","params":[{"name":"chosen","value":{"ref":"step:chosen"}}]}}}],"adjustments":[]})).unwrap();
    d
}

/// Spec 005's rules backend scoring each candidate 2 - 0.1 x price (3.14).
fn rules_backend() -> RulesBackend {
    RulesBackend::new(serde_json::from_value(json!({
        "schema":"rustev.rules/1","backend_id":"rules-rank","limits":{"max_projection_bytes":4096},"cost":{"units_per_call":0},
        "tasks":[{"operation":"rank","task":"fit","question":"Which fits?","options":["score"],
            "interpretation":"SYNTHETIC swap-test program; authored scores.",
            "fields":[{"name":"price","ref":"bind:candidate/price","type":"decimal"}],"base":{"score":"2"},
            "rules":[{"name":"price","when":"always","then":[{"linear":{"field":"price","coefficients":{"score":"-0.1"}}}],"stop":false}],
            "on_no_match":"base"}]
    })).unwrap()).unwrap()
}

struct Credentials;
impl CredentialProvider for Credentials {
    fn credentials<'a>(
        &'a self,
        _: Budget,
        _: &'a CancelSignal,
    ) -> BoxFuture<'a, Result<aws_credential_types::Credentials, CredentialError>> {
        Box::pin(async {
            Ok(aws_credential_types::Credentials::new(
                "HOST_TEST_KEY",
                "HOST_TEST_SECRET",
                None,
                None,
                "test",
            ))
        })
    }
}

/// The fixture binding with `rank` admitted, its signed calls sent to
/// `endpoint`.
fn bedrock_backend(endpoint: String) -> BedrockBackend {
    let mut binding = BedrockBinding::parse(include_bytes!("../fixtures/binding.json")).unwrap();
    binding.operations.push(OperationBinding {
        operation: "rank".into(),
        mapping_version: RANK_MAPPING.into(),
        max_options: 100,
    });
    BedrockBackend::new(
        "bedrock-rank".into(),
        binding,
        rustev_bedrock::policy::ModelPolicy::parse(include_bytes!("../fixtures/policy.json"))
            .unwrap(),
        serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap(),
        Config {
            endpoint: Some(endpoint),
            credentials: Some(Arc::new(Credentials)),
            ..Default::default()
        },
    )
    .unwrap()
}

/// A loopback Converse endpoint answering each pointwise request with the
/// fixture answer for its presented candidate, in a documented-shape
/// `toolUse` response. Returns its URL and a request count.
async fn fixture_endpoint() -> (String, Arc<AtomicUsize>) {
    let fixture: Value =
        serde_json::from_slice(include_bytes!("../fixtures/rank/swap-answers.json")).unwrap();
    assert!(
        fixture["provenance"]
            .as_str()
            .unwrap()
            .contains("documented-shape")
    );
    let answers = Arc::new(fixture["answers"].clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(addr.ip().is_loopback());
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    tokio::spawn(async move {
        loop {
            let (stream, peer) = listener.accept().await.unwrap();
            assert!(peer.ip().is_loopback());
            let (answers, count) = (answers.clone(), count.clone());
            let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                let (answers, count) = (answers.clone(), count.clone());
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    let body = BodyExt::collect(req.into_body()).await?.to_bytes();
                    let request: Value = serde_json::from_slice(&body).unwrap();
                    let input: Value = serde_json::from_str(
                        request["messages"][0]["content"][0]["text"]
                            .as_str()
                            .unwrap(),
                    )
                    .unwrap();
                    let id = input["presented"][0]["id"].as_str().unwrap();
                    let response = json!({"stopReason":"tool_use","output":{"message":{"role":"assistant","content":[{"toolUse":{"toolUseId":"synthetic-1","name":"rustev_answer","input":answers[id]}}]}},"usage":{"inputTokens":10,"outputTokens":2}});
                    Ok::<_, hyper::Error>(hyper::Response::new(Full::new(Bytes::from(
                        serde_json::to_vec(&response).unwrap(),
                    ))))
                }
            });
            tokio::spawn(async move {
                let io = hyper_util::rt::TokioIo::new(stream);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });
    (format!("http://{addr}"), hits)
}

fn snapshot(items: &[(&str, &str)]) -> Snapshot {
    reference::snapshot(vec![reference::entry(
        "items",
        json!(
            items
                .iter()
                .map(|(id, price)| json!({"id":id,"price":price}))
                .collect::<Vec<_>>()
        ),
        rustev_contract::definition::ProvenanceClass::SystemOfRecord,
        reference::NOW,
    )])
}

struct Discard;
impl EvidenceSink for Discard {
    /// Rank plans record `rustev.run/3` (spec 039 3.13).
    fn accepts(&self, _: &str) -> bool {
        true
    }
    fn deliver<'a>(&'a self, r: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("kept:{}", r.decision_id)) })
    }
}

fn scope() -> Scope {
    let h = |s: &str| Handle::new(s).unwrap();
    Scope::principal(h(TENANT), h(REVISION), h(PRINCIPAL))
}

fn fresh_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("bedrock-rank-swap")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs every case with capture, writes each replay bundle as
/// `<case>.json` and returns each case's outcome and policy trace.
async fn run_cases(
    backend: Arc<dyn DecisionBackend>,
    compiled: &Compiled,
    dir: &Path,
) -> Vec<(Outcome, Vec<String>)> {
    let descriptors = vec![backend.descriptor().clone()];
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Discard),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 8,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    )
    .backend(backend, 8)
    .build()
    .unwrap();
    let prepared = rt.prepare(compiled.clone()).unwrap();
    let evaluation_time = Timestamp::from_ms(reference::NOW).unwrap();
    let mut out = vec![];
    for (case, items) in CASES {
        let snapshot = snapshot(&items);
        let decided = rt
            .decide_with_capture(
                &prepared,
                DecisionRequest {
                    decision_id: format!("rank-swap.{case}"),
                    snapshot: snapshot.clone(),
                    evaluation_time,
                    deadline_ms: None,
                    principal_handle: TENANT.as_bytes().to_vec(),
                },
                &CancelSignal::new(),
                &CaptureConfig {
                    scope: scope(),
                    max_bytes: MAX_CAPTURE_BYTES,
                },
            )
            .await
            .unwrap();
        let result = decided.result.unwrap();
        let Completion::Judged(judgment) = &result.completion else {
            panic!("{case}: cancelled")
        };
        let CaptureOutcome::Captured(capture) = decided.capture else {
            panic!("{case}: not captured")
        };
        let bundle = assemble(
            &AssemblyInput {
                capture: Some(&capture),
                scope: &capture.scope,
                plan: &compiled.plan,
                descriptors: &descriptors,
                calibrations: &[],
                snapshot: &snapshot,
                run: &result.record,
                evaluation_time,
                created_at_ms: evaluation_time,
            },
            &RetentionChoice {
                content: Content::Embedded,
                ..RetentionChoice::default()
            },
        )
        .unwrap();
        std::fs::write(
            dir.join(format!("{case}.json")),
            bundle.canonical().unwrap(),
        )
        .unwrap();
        out.push((judgment.outcome.clone(), judgment.trace.clone()));
    }
    out
}

/// The evaluation inputs: a `rustev.dataset/2` with gold relevance, a
/// version 2 task adapter declaring the selection's ranking measures with
/// backfill positions, a version 3 configuration bound to it, and a
/// repeat configuration.
fn write_inputs(dir: &Path) {
    let adapter = json!({"schema":"rustev.task-adapter/2","name":"rank-swap","version":"1",
        "labels":{"values":["selected"],"prefixes":[]},
        "rules":[{"action":"recommend","label":"selected"}],
        "measures":{"rankings":[{"name":"chosen","param":"chosen","relevant_at_least":2,"gain":"exponential",
            "metrics":["precision@2","ndcg@2","mrr"],"backfill_positions":2}],"constraints":[],"fields":[]}});
    let tagged = |bytes: &[u8]| tagged_digest("rustev.task-adapter/2", bytes);
    let gold = [
        json!({"a-1":3,"a-2":0,"a-3":2}),
        json!({"b-1":0,"b-2":3,"b-3":1}),
        json!({"c-1":1,"c-2":0,"c-3":3}),
    ];
    let cases: Vec<Value> = CASES
        .iter()
        .zip(gold)
        .map(|((case, items), relevance)| json!({"id":case,"label":{"value":"selected"},"gold":{"relevance":relevance},
            "snapshot":snapshot(items).id().unwrap().as_str(),"source":format!("src:{case}"),"split":"calibration","subgroups":[]}))
        .collect();
    let dataset = json!({"schema":"rustev.dataset/2","name":"rank swap gold","adapter":{"name":"rank-swap","version":"1"},
        "provenance":{"synthetic":{"generator":"rustev-bedrock tests, hand-authored"}},"cases":cases});
    let metric =
        |name: &str| json!({"name":name,"formula":format!("SYNTHETIC formula for {name}")});
    let metrics = [
        "coverage",
        "acceptance_coverage",
        "labeled_proposal_coverage",
        "error_among_accepted",
        "candidate_outcome_change",
        "mean_log_loss",
        "brier",
        "reliability",
        "latency_ms",
        "queue_ms",
        "cost_observed_units",
        "cost_estimated_units",
        "cost_unknown_attempts",
        "cost_liability_units",
        "ranking",
    ]
    .map(metric);
    let config = json!({"schema":"rustev.evaluator-config/3",
        "adapter":{"name":"rank-swap","version":"1",
            "rules":{"bound":tagged(&canonical_value_bytes(&adapter).unwrap())},
            "measures":tagged(&canonical_value_bytes(&adapter["measures"]).unwrap())},
        "label_interpretation":"SYNTHETIC authored label","agreement":{"params":[]},"probability_step":null,
        "reliability_bins":["0","0.5","1"],"subgroups":[],"latency_quantiles":["0.5"],"cost_units_comparable":false,
        "metrics":metrics,
        "gates":[],"temperature_grid":[],"scorers":[],"rubric_metrics":[],"human_scales":[],"agreements":[],"origin":[]});
    // `rustev eval --repeat-config` evaluates each run without the
    // adapter's measures, so a version 3 configuration is refused there;
    // the repeat runs under the same configuration at version 2.
    let mut config_v2 = config.clone();
    config_v2["schema"] = json!("rustev.evaluator-config/2");
    let c = config_v2.as_object_mut().unwrap();
    c["adapter"].as_object_mut().unwrap().remove("measures");
    for k in [
        "scorers",
        "rubric_metrics",
        "human_scales",
        "agreements",
        "origin",
    ] {
        c.remove(k);
    }
    c["metrics"]
        .as_array_mut()
        .unwrap()
        .retain(|m| m["name"] != "ranking");
    let repeat = json!({"schema":"rustev.repeat-config/1",
        "bootstrap":{"method":"percentile-cluster/1","prng":"xoshiro256starstar-splitmix64/1","level":"0.95","resamples":1000,"seed":"39"},
        "noise":{"rule":{"k_sd":{"k":"2"}},"floor":"0","overrides":{}},"slices":[],"stability_metrics":["coverage"],"top_k":null});
    for (name, doc) in [
        ("adapter", adapter),
        ("dataset", dataset),
        ("config-v2", config_v2),
        ("config", config),
        ("repeat-config", repeat),
    ] {
        std::fs::write(
            dir.join(format!("{name}.json")),
            canonical_value_bytes(&doc).unwrap(),
        )
        .unwrap();
    }
}

/// `rustev eval` over one bundle directory; with `repeat`, spec 029's
/// replayed repeat of two runs.
fn eval(inputs: &Path, bundles: &Path, out: &Path, repeat: bool) -> (u8, Value) {
    let p = |path: &Path| path.to_str().unwrap().to_owned();
    let mut argv: Vec<String> = vec!["eval".into()];
    for (flag, value) in [
        ("--dataset", p(&inputs.join("dataset.json"))),
        ("--split", "calibration".into()),
        (
            "--config",
            p(&inputs.join(if repeat {
                "config-v2.json"
            } else {
                "config.json"
            })),
        ),
        ("--adapter", p(&inputs.join("adapter.json"))),
        ("--bundles", p(bundles)),
        ("--now-ms", reference::NOW.to_string()),
        ("--tenant", TENANT.into()),
        ("--context-revision", REVISION.into()),
        ("--principal-scope", PRINCIPAL.into()),
        ("--out", p(out)),
    ] {
        argv.extend([flag.to_owned(), value]);
    }
    if repeat {
        argv.extend([
            "--repeat-config".into(),
            p(&inputs.join("repeat-config.json")),
            "--repeat".into(),
            "2".into(),
        ]);
    }
    let exit = rustev_cli::execute(&argv, &rustev_cli::Context::default());
    let json = serde_json::from_slice(&exit.stdout)
        .unwrap_or_else(|_| panic!("eval: {} {}", exit.code, exit.stderr));
    (exit.code, json)
}

fn measured(report: &Value, name: &str) -> Option<(u64, f64)> {
    let m = report["metrics"]
        .as_array()?
        .iter()
        .find(|m| m["name"] == name)?;
    Some((
        m["value"]["measured"]["n"].as_u64()?,
        m["value"]["measured"]["value"].as_f64()?,
    ))
}

#[test]
fn swapping_rules_for_bedrock_fixtures_keeps_one_authority_path_and_comparable_reports() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (rules_dir, bedrock_dir) = (fresh_dir("rules/bundles"), fresh_dir("bedrock/bundles"));
    let (rules_plan, bedrock_plan, rules_out, bedrock_out, hits) = rt.block_on(async {
        let (url, hits) = fixture_endpoint().await;
        let (rules, bedrock) = (rules_backend(), bedrock_backend(url));
        let def = definition();
        let rules_plan = rustev_core::compile(&def, &[rules.descriptor().clone()], &[]).unwrap();
        let bedrock_plan =
            rustev_core::compile(&def, &[bedrock.descriptor().clone()], &[]).unwrap();
        bedrock.check_plan(&bedrock_plan.plan).unwrap();
        let rules_out = run_cases(Arc::new(rules), &rules_plan, &rules_dir).await;
        let bedrock_out = run_cases(Arc::new(bedrock), &bedrock_plan, &bedrock_dir).await;
        (
            rules_plan,
            bedrock_plan,
            rules_out,
            bedrock_out,
            hits.load(Ordering::SeqCst),
        )
    });
    // One pointwise request per candidate, all to the loopback fixture.
    assert_eq!(hits, 9);

    // One plan, one authority path: the same definition and policy, the
    // plans differing only by the binding; every case proposes through
    // the same rule, whichever backend ranked.
    assert_eq!(rules_plan.plan.definition, bedrock_plan.plan.definition);
    assert_ne!(rules_plan.id, bedrock_plan.id);
    let chosen = |o: &Outcome| match o {
        Outcome::Propose { action, params } => {
            assert_eq!(action, "recommend");
            let rustev_contract::judgment::OutValue::Selection(s) = &params["chosen"] else {
                panic!("{params:?}")
            };
            s.entries
                .iter()
                .map(|e| format!("{}:{:?}", e.id, e.origin))
                .collect::<Vec<_>>()
        }
        other => panic!("{other:?}"),
    };
    for ((r, rt), (b, bt)) in rules_out.iter().zip(&bedrock_out) {
        assert_eq!(rt, bt, "the policy trace differs");
        assert_eq!(std::mem::discriminant(r), std::mem::discriminant(b));
    }
    let rules_chosen: Vec<_> = rules_out.iter().map(|(o, _)| chosen(o)).collect();
    let bedrock_chosen: Vec<_> = bedrock_out.iter().map(|(o, _)| chosen(o)).collect();
    assert_eq!(rules_chosen[0], ["a-1:Model", "a-3:Model"]);
    assert_eq!(bedrock_chosen[0], rules_chosen[0]);
    assert_eq!(rules_chosen[1], ["b-2:Model", "b-3:Model"]);
    assert_eq!(bedrock_chosen[1], ["b-2:Model", "b-1:Model"]);
    // Every Bedrock answer for r-3 lacks `score`: the ranking failed and
    // `backfill_all` filled both positions from the exact order.
    assert_eq!(rules_chosen[2], ["c-3:Model", "c-1:Model"]);
    assert_eq!(bedrock_chosen[2], ["c-1:Backfill", "c-2:Backfill"]);

    // Comparable reports under one dataset, split and configuration: spec
    // 004's, spec 041's ranking measures (E1) and spec 029's repeat (E2).
    let inputs = fresh_dir("inputs");
    write_inputs(&inputs);
    let mut reports = vec![];
    for (name, bundles) in [("rules", &rules_dir), ("bedrock", &bedrock_dir)] {
        let out = fresh_dir(&format!("{name}/report"));
        let (code, report) = eval(&inputs, bundles, &out, false);
        assert_eq!(code, 0, "{name}: {report}");
        let repeated = fresh_dir(&format!("{name}/repeat"));
        let (code, summary) = eval(&inputs, bundles, &repeated, true);
        assert_eq!(code, 0, "{name}: {summary}");
        assert_eq!(
            (summary["n"].as_u64(), summary["source"].as_str()),
            (Some(2), Some("replay"))
        );
        let repeat: Value =
            serde_json::from_slice(&std::fs::read(repeated.join("repeat.json")).unwrap()).unwrap();
        reports.push((report, repeat));
    }
    let ((rules, rules_repeat), (bedrock, bedrock_repeat)) = (&reports[0], &reports[1]);
    for key in ["dataset", "config", "adapter_digest"] {
        assert!(!rules[key].is_null(), "{key}: {rules}");
        assert_eq!(rules[key], bedrock[key], "{key} differs");
    }
    for report in [rules, bedrock] {
        assert_eq!(measured(report, "coverage"), Some((3, 1.0)), "{report}");
        for metric in ["precision@2", "ndcg@2", "mrr"] {
            assert!(
                measured(report, &format!("ranking/chosen/{metric}")).is_some(),
                "{metric}: {report}"
            );
        }
    }
    assert_eq!(
        measured(rules, "ranking/chosen/backfill@p1"),
        Some((3, 0.0))
    );
    let (n, share) = measured(bedrock, "ranking/chosen/backfill@p1").unwrap();
    assert_eq!(n, 3);
    assert!((share - 1.0 / 3.0).abs() < 1e-12, "{share}");
    for key in [
        "schema",
        "dataset",
        "evaluator_config",
        "adapter_rules",
        "repeat_config",
        "split",
    ] {
        assert!(!rules_repeat[key].is_null(), "{key}: {rules_repeat}");
        assert_eq!(
            rules_repeat[key], bedrock_repeat[key],
            "repeat {key} differs"
        );
    }
    assert_ne!(rules_repeat["plan"], bedrock_repeat["plan"]);
    assert_eq!(rules_repeat["runs"].as_array().unwrap().len(), 2);
}
