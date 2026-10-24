//! Frozen synthetic evidence tests CLI mechanics, never semantic quality.
// Both existing harnesses include the core builders; keep their fixture types
// scoped to their harness instead of copying or changing shared helpers.
#![allow(clippy::duplicate_mod)]
#[path = "common/mod.rs"]
mod cli;
#[path = "../../rustev-eval/tests/common/mod.rs"]
mod common;
#[path = "../../rustev-eval/tests/explain_support/mod.rs"]
mod explain_support;
use cli::Scratch;
use rustev_contract::{Document, canonical::record_canonical_bytes, explanation::Explanation};
use serde_json::{Value, json};
use std::path::PathBuf;

const PARAMS: &str = "/core/recorded/judgment/outcome/propose/params";
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/explain")
}
fn frozen(name: &str) -> Vec<u8> {
    std::fs::read(root().join(name)).unwrap()
}
fn value(name: &str) -> Value {
    serde_json::from_slice(&frozen(name)).unwrap()
}
fn save(name: &str, bytes: &[u8]) {
    std::fs::create_dir_all(root()).unwrap();
    std::fs::write(root().join(name), bytes).unwrap();
}
fn golden(name: &str, bytes: &[u8]) {
    if std::env::var("RUSTEV_BLESS").as_deref() == Ok("1") {
        save(name, bytes);
    }
    assert_eq!(bytes, frozen(name), "golden {name}");
}
fn setup() -> Scratch {
    let s = Scratch::new("explain");
    for entry in std::fs::read_dir(root()).unwrap() {
        let p = entry.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap();
        if name.ends_with(".json") && !name.ends_with(".explanation.json") {
            s.write(name, &std::fs::read(&p).unwrap());
        }
    }
    s
}
fn scope() -> Vec<String> {
    [
        "--now-ms",
        "1800000000001",
        "--tenant",
        "tenant-a",
        "--context-revision",
        "rev-1",
        "--principal-scope",
        "scope-p",
    ]
    .map(str::to_string)
    .to_vec()
}
fn args(query: &[&str], form: &str, fixture: &str) -> Vec<String> {
    let mut a = vec!["explain".into()];
    a.extend(query.iter().map(|s| s.to_string()));
    if form == "bundle" {
        a.extend(["--bundle".into(), format!("{fixture}.bundle.json")]);
        a.extend(scope());
    } else {
        a.extend(["--record".into(), format!("{fixture}.record.json")]);
        if form == "plan" {
            a.extend(["--plan".into(), format!("{fixture}.plan.json")]);
        }
    }
    a
}
fn invoke(s: &Scratch, a: &[String]) -> (Value, Vec<u8>, Vec<u8>) {
    let mut a = a.to_vec();
    a.extend(["--out", "out.json", "--markdown", "out.md"].map(str::to_string));
    let out = s.rustev_owned(&a).expect(0, "explained");
    let bytes = s.read("out.json");
    let md = s.read("out.md");
    let e = Explanation::parse(&bytes).unwrap();
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        out,
        json!({"command":format!("explain {}", a[1]),"status":"explained","explanation":e.record_digest().unwrap(),"answer":doc["answer"]["kind"],"not_available":doc["facts"].as_array().unwrap().iter().filter(|f| f["availability"] != "available").count()})
    );
    std::fs::remove_file(s.path("out.json")).unwrap();
    std::fs::remove_file(s.path("out.md")).unwrap();
    (doc, bytes, md)
}
fn check_golden(s: &Scratch, name: &str, a: &[String]) -> Value {
    let first = invoke(s, a);
    let second = invoke(s, a);
    assert_eq!(first, second);
    golden(&format!("{name}.explanation.json"), &first.1);
    golden(&format!("{name}.explanation.md"), &first.2);
    first.0
}

#[test]
fn every_query_every_reference_form() {
    let s = setup();
    for fixture in [
        "support",
        "lodging",
        "lodging-tie",
        "lodging-text",
        "escalated",
        "unresolved",
    ] {
        for form in ["record", "plan", "bundle"] {
            for query in [
                vec!["why-selected", "--item", "c-1"],
                vec!["why-excluded", "--item", "c-excluded"],
                vec!["why-order", "--a", "c-1", "--b", "c-2"],
                vec!["why-outcome"],
                vec!["text-origin"],
                vec!["models"],
                vec!["diff"],
            ] {
                let mut a = args(&query, form, fixture);
                if query[0] == "diff" {
                    a.extend([
                        if form == "bundle" {
                            "--other-bundle"
                        } else {
                            "--other-record"
                        }
                        .into(),
                        format!(
                            "{fixture}.{}.json",
                            if form == "bundle" { "bundle" } else { "record" }
                        ),
                    ]);
                    if form == "plan" {
                        a.extend(["--other-plan".into(), format!("{fixture}.plan.json")]);
                    }
                }
                check_golden(&s, &format!("{fixture}-{form}-{}", query[0]), &a);
            }
        }
    }
    // No input byte changes, including plans, bundles and policies.
    for entry in std::fs::read_dir(root()).unwrap() {
        let p = entry.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap();
        if s.exists(name) {
            assert_eq!(s.read(name), frozen(name));
        }
    }
    let tie = invoke(
        &s,
        &args(
            &["why-order", "--a", "c-1", "--b", "c-2"],
            "plan",
            "lodging-tie",
        ),
    )
    .0;
    assert_eq!(tie["answer"]["scopes"][0]["by"], "tie_break");
    assert_eq!(tie["answer"]["scopes"][0]["consistent"], true);
    let text = invoke(&s, &args(&["text-origin"], "plan", "lodging-text")).0;
    let origins = text["facts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["claim"] == "field_origin")
        .collect::<Vec<_>>();
    assert!(!origins.is_empty());
    for fact in origins {
        assert_eq!(
            fact["availability"]["not_available"]["reason"],
            "not-recorded"
        );
    }
    let cut = check_golden(
        &s,
        "lodging-cut",
        &args(&["why-excluded", "--item", "c-6"], "plan", "lodging"),
    );
    assert_eq!(cut["answer"]["kind"], "cut");
    for (fixture, kind) in [("escalated", "escalated"), ("unresolved", "unresolved")] {
        assert_eq!(
            invoke(&s, &args(&["why-outcome"], "plan", fixture)).0["answer"]["kind"],
            kind
        );
    }
}

#[test]
fn usage_errors_precede_reads_and_writes() {
    let s = Scratch::new("explain-usage");
    let cases = [
        vec!["explain"],
        vec!["explain", "unknown"],
        vec!["explain", "models", "--record", "missing"],
        vec!["explain", "models", "--out", "out"],
        vec![
            "explain", "models", "--record", "missing", "--bundle", "missing",
        ],
        vec![
            "explain", "models", "--bundle", "missing", "--plan", "missing",
        ],
        vec!["explain", "models", "--bundle", "missing"],
        vec!["explain", "why-selected", "--record", "missing"],
        vec![
            "explain",
            "why-excluded",
            "--record",
            "missing",
            "--item",
            "",
        ],
        vec![
            "explain",
            "why-order",
            "--record",
            "missing",
            "--a",
            "x",
            "--b",
            "x",
        ],
        vec!["explain", "why-order", "--record", "missing", "--a", "x"],
        vec!["explain", "models", "--record", "missing", "--item", "x"],
        vec![
            "explain",
            "why-selected",
            "--record",
            "missing",
            "--item",
            "x",
            "--a",
            "y",
        ],
        vec!["explain", "diff", "--record", "missing"],
        vec![
            "explain",
            "diff",
            "--record",
            "missing",
            "--other-bundle",
            "missing",
            "--other-plan",
            "missing",
        ],
        vec![
            "explain",
            "diff",
            "--record",
            "missing",
            "--other-record",
            "missing",
            "--other-bundle",
            "missing",
        ],
    ];
    for (i, case) in cases.into_iter().enumerate() {
        let mut a = case;
        if i != 2 {
            a.extend(["--out", "out"]);
        }
        let r = s.rustev(&a);
        assert_eq!(r.code, 2, "{a:?}: {r:?}");
        assert!(r.stdout.is_empty());
    }
    for flag in [
        "scope",
        "tenant",
        "context-revision",
        "principal-scope",
        "now-ms",
        "store",
        "other-record",
        "other-bundle",
        "other-plan",
        "a",
        "b",
    ] {
        let a = [
            "explain",
            "models",
            "--record",
            "missing",
            "--out",
            "out",
            &format!("--{flag}"),
            "x",
        ];
        assert_eq!(s.rustev(&a).code, 2, "{flag}");
    }
    for other in [false, true] {
        let mut a = args(
            &[if other { "diff" } else { "models" }],
            "bundle",
            "missing",
        );
        if other {
            a.extend(["--other-bundle", "missing", "--other-plan", "missing"].map(str::to_string));
        } else {
            a.extend(["--plan", "missing"].map(str::to_string));
        }
        a.extend(["--out", "out"].map(str::to_string));
        assert_eq!(s.rustev_owned(&a).code, 2);
    }
    for item in ["x".repeat(257), "é".repeat(129)] {
        for query in ["why-selected", "why-excluded", "why-order"] {
            let mut a = vec!["explain", query, "--record", "missing", "--out", "out"];
            if query == "why-order" {
                a.extend(["--a", &item, "--b", "z"]);
            } else {
                a.extend(["--item", &item]);
            }
            assert_eq!(s.rustev(&a).code, 2);
        }
    }
    assert_eq!(std::fs::read_dir(&s.0).unwrap().count(), 0);
}

#[test]
fn refusals_name_the_file_or_item_and_leave_no_output() {
    let s = setup();
    let mut a = args(&["why-outcome"], "record", "support");
    a.extend(
        [
            "--plan",
            "lodging.plan.json",
            "--out",
            "out",
            "--markdown",
            "md",
        ]
        .map(str::to_string),
    );
    let r = s.rustev_owned(&a).expect(4, "invalid_input");
    assert_eq!(r["reason"], "plan-mismatch");
    assert_eq!(r["input"], "lodging.plan.json");
    assert!(!s.exists("out") && !s.exists("md"));
    let mut a = args(&["diff"], "record", "support");
    a.extend(
        [
            "--other-record",
            "support.record.json",
            "--other-plan",
            "lodging.plan.json",
            "--out",
            "out",
        ]
        .map(str::to_string),
    );
    assert_eq!(
        s.rustev_owned(&a).expect(4, "invalid_input")["input"],
        "lodging.plan.json"
    );
    let mut a = args(&["models"], "bundle", "support");
    let tenant = a.iter().position(|v| v == "tenant-a").unwrap();
    a[tenant] = "other".into();
    a.extend(["--out", "out"].map(str::to_string));
    assert_eq!(
        s.rustev_owned(&a).expect(4, "invalid_input")["reason"],
        "scope-mismatch"
    );
    let mut b: rustev_contract::replay::ReplayBundle =
        rustev_contract::replay::ReplayBundle::parse(&s.read("support.bundle.json")).unwrap();
    if let rustev_contract::retention::Retention::Retained { bytes, .. } =
        &mut b.supplies[0].value.retention
    {
        bytes.push(' ');
    }
    s.write("support.bundle.json", &b.record_canonical().unwrap());
    a[tenant] = "tenant-a".into();
    let r = s.rustev_owned(&a).expect(4, "invalid_input");
    assert_eq!(r["reason"], "evidence-integrity");
    assert_eq!(r["item"], "supply/0");
    assert!(!s.exists("out"));
    for (flag, file) in [("--record", "bad"), ("--bundle", "bad")] {
        s.write(file, b"{}");
        let mut a = vec![
            "explain".into(),
            "models".into(),
            flag.into(),
            file.into(),
            "--out".into(),
            "out".into(),
        ];
        if flag == "--bundle" {
            a.extend(scope());
        }
        assert_eq!(
            s.rustev_owned(&a).expect(4, "invalid_input")["reason"],
            "parse"
        );
    }
}

#[test]
fn outputs_are_no_clobber_and_all_or_none() {
    let s = setup();
    for (out, md) in [
        ("support.record.json", "new.md"),
        ("new.json", "support.record.json"),
        ("new.json", "missing/dir/md"),
        ("same", "same"),
    ] {
        let mut a = args(&["models"], "record", "support");
        a.extend(["--out", out, "--markdown", md].map(str::to_string));
        s.rustev_owned(&a).expect(3, "io_error");
        assert_eq!(s.read("support.record.json"), frozen("support.record.json"));
        for name in ["new.json", "new.md", "same"] {
            assert!(!s.exists(name));
        }
    }
    let mut a = args(&["models"], "record", "support");
    a.extend(["--out", "alone"].map(str::to_string));
    s.rustev_owned(&a).expect(0, "explained");
    Explanation::parse(&s.read("alone")).unwrap();
    s.rustev(&["explain", "models", "--record", "missing", "--out", "out"])
        .expect(3, "io_error");
}

// Explicit one-time fixture generation. Blessing explanations never regenerates
// their input evidence. Runtime timing is normalized before bundle assembly.
#[test]
#[ignore = "explicitly regenerate frozen synthetic inputs"]
fn generate_inputs() {
    use common::*;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            async fn support_case(fail: bool, propagate: bool) -> Retained {
                let p = Probe::new(support_rules(), None, move |_, task| {
                    if fail && task == "support.topic" {
                        Behavior::Fail(rustev_core::seams::AdapterFailure::Permanent)
                    } else {
                        Behavior::Pass
                    }
                });
                let mut f = support_fixture(&[&p], &unlimited(), "1.5");
                if propagate {
                    let mut def = f.plan().definition;
                    for h in &mut def.policy.on_unresolved {
                        h.action = rustev_contract::definition::HandlerAction::Propagate;
                    }
                    f.compiled = rustev_core::compile_with(
                        &def,
                        &f.descriptors,
                        &f.calibrations,
                        &unlimited(),
                    )
                    .unwrap();
                }
                run_and_capture(
                    &[&p],
                    f,
                    decision("explain-support", support_snapshot("pro", &[2, 5], 0)),
                    scope(),
                )
                .await
            }
            fn freeze(name: &str, mut r: Retained) {
                let mut v = serde_json::to_value(&r.run).unwrap();
                cli::normalize_timing(&mut v);
                r.run =
                    rustev_contract::run::RunRecord::parse(&record_canonical_bytes(&v).unwrap())
                        .unwrap();
                save(
                    &format!("{name}.record.json"),
                    &r.run.record_canonical().unwrap(),
                );
                save(
                    &format!("{name}.plan.json"),
                    &r.fixture.plan().canonical().unwrap(),
                );
                save(
                    &format!("{name}.bundle.json"),
                    &embedded(&r).record_canonical().unwrap(),
                );
            }
            freeze("support", explain_support::support_shifted(false).await);
            freeze("shifted", explain_support::support_shifted(true).await);
            freeze("escalated", support_case(true, false).await);
            freeze("unresolved", support_case(true, true).await);
            let p = Probe::passing(lodging_rules());
            let mut candidates = (1..=7)
                .map(|n| candidate(&format!("c-{n}"), "120", "USD", 2, true))
                .collect::<Vec<_>>();
            candidates.push(candidate("c-excluded", "120", "USD", 2, false));
            let snapshot = lodging_snapshot(
                true,
                candidates,
                vec![claim("k-1", "verified", "preference", NOW + DAY)],
            );
            freeze(
                "lodging",
                run_and_capture(
                    &[&p],
                    lodging_fixture(&p, &unlimited()),
                    decision("explain-lodging", snapshot.clone()),
                    scope(),
                )
                .await,
            );
            // Reference-plan variants: remove the positional price contribution
            // to produce a genuine runtime tie; expose the recorded input text.
            for text in [false, true] {
                let mut f = lodging_fixture(&p, &unlimited());
                let mut def = f.plan().definition;
                if text {
                    use rustev_contract::definition::*;
                    for rule in &mut def.policy.rules {
                        if let OutcomeDecl::Propose { params, .. } = &mut rule.then {
                            params.push(ParamDecl {
                                name: "description".into(),
                                value: ParamSource::Ref("input:trip.text".into()),
                            });
                        }
                    }
                    def.policy.on_unresolved.push(Handler {
                        target: "input:trip.text".into(),
                        reasons: ReasonSet::Any,
                        action: HandlerAction::Propagate,
                    });
                } else {
                    for step in &mut def.steps {
                        if step.id == "ranking" {
                            let rustev_contract::definition::StepBody::Exact(exact) =
                                &mut step.body
                            else {
                                panic!()
                            };
                            for c in exact.args["components"].as_array_mut().unwrap() {
                                if c["name"] == "price" {
                                    c["weight"] = json!("0");
                                }
                                if c["name"] == "preference" {
                                    c["weight"] = json!("0.7");
                                }
                            }
                        }
                    }
                }
                f.compiled =
                    rustev_core::compile_with(&def, &f.descriptors, &f.calibrations, &unlimited())
                        .unwrap();
                freeze(
                    if text { "lodging-text" } else { "lodging-tie" },
                    run_and_capture(
                        &[&p],
                        f,
                        decision("explain-lodging", snapshot.clone()),
                        scope(),
                    )
                    .await,
                );
            }
            let mut v = value("support.record.json");
            let core = &mut v["core"]["recorded"];
            let input = core["inputs"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|i| i["field"] == "ticket.message")
                .unwrap();
            input["status"] =
                json!({"unresolved":{"missing_evidence":{"fields":["ticket.message"]}}});
            let step = core["steps"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|i| i["step"] == "topic")
                .unwrap();
            step["status"] =
                json!({"unresolved":{"missing_evidence":{"fields":["ticket.message"]}}});
            save("changed.record.json", &record_canonical_bytes(&v).unwrap());
            save("changed.plan.json", &frozen("support.plan.json"));
            for (name, class, action) in [
                (
                    "id",
                    rustev_contract::redaction::FieldClass::Id,
                    rustev_contract::redaction::Action::Digest,
                ),
                (
                    "third-party",
                    rustev_contract::redaction::FieldClass::ThirdParty,
                    rustev_contract::redaction::Action::Drop,
                ),
                (
                    "enum",
                    rustev_contract::redaction::FieldClass::Enum,
                    rustev_contract::redaction::Action::Drop,
                ),
            ] {
                save(
                    &format!("{name}.policy.json"),
                    &explain_support::policy(class, action).canonical().unwrap(),
                );
            }
        });
}

fn put(s: &Scratch, name: &str, v: &Value) {
    s.write(name, &record_canonical_bytes(v).unwrap());
}
fn claim<'a>(doc: &'a Value, name: &str) -> &'a Value {
    doc["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["claim"] == name)
        .unwrap()
}
fn diff_args(form: &str, a: &str, b: &str) -> Vec<String> {
    let mut args = args(&["diff"], form, a);
    args.extend([
        if form == "bundle" {
            "--other-bundle"
        } else {
            "--other-record"
        }
        .into(),
        format!(
            "{b}.{}.json",
            if form == "bundle" { "bundle" } else { "record" }
        ),
    ]);
    if form == "plan" {
        args.extend(["--other-plan".into(), format!("{b}.plan.json")]);
    }
    args
}

#[test]
fn diff_changes_retention_and_redaction() {
    use rustev_contract::{replay::ReplayBundle, retention::Retention};
    let s = setup();
    let d = check_golden(
        &s,
        "backend-changed",
        &diff_args("bundle", "support", "shifted"),
    );
    assert_eq!(d["answer"]["why"], "backend-output-changed");
    let first = d["answer"]["first"].clone();
    let f = d["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == first)
        .unwrap();
    assert!(
        f["value"]["subject"]
            .as_str()
            .unwrap()
            .starts_with("step:topic#")
    );
    for name in ["support", "shifted"] {
        let mut b = ReplayBundle::parse(&s.read(&format!("{name}.bundle.json"))).unwrap();
        if let Retention::Retained { expires_at_ms, .. } = &mut b.supplies[0].value.retention {
            *expires_at_ms = common::wall(common::WALL + 1);
        }
        s.write(
            &format!("{name}.bundle.json"),
            &b.record_canonical().unwrap(),
        );
    }
    let d = invoke(&s, &diff_args("bundle", "support", "shifted")).0;
    let f = claim(&d, "difference");
    assert_eq!(
        f["value"]["a_state"]["not_available"]["availability"],
        "expired"
    );
    assert_eq!(
        f["value"]["b_state"]["not_available"]["availability"],
        "expired"
    );
    assert!(
        f["cites"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v.as_str().unwrap().ends_with("/digest"))
            .count()
            >= 2
    );
    let d = check_golden(
        &s,
        "input-changed",
        &diff_args("plan", "support", "changed"),
    );
    assert_eq!(d["answer"]["why"], "input-changed");
    let first = &d["answer"]["first"];
    assert!(d["facts"].as_array().unwrap().iter().any(|f| {
        f["value"]["follows_from"]
            .as_array()
            .is_some_and(|v| v.contains(first))
    }));
    let d = invoke(&s, &diff_args("record", "support", "lodging")).0;
    assert_eq!(d["answer"]["why"], "plan-changed");
    assert!(
        claim(&d, "difference")["value"]["follows_from"]
            .get("not_available")
            .is_some()
    );
    for b in ["support", "shifted"] {
        let mut a = diff_args("record", "support", b);
        a.extend(["--redaction", "enum.policy.json"].map(str::to_string));
        let d = invoke(&s, &a).0;
        assert_eq!(d["answer"]["kind"], "withheld");
        assert!(
            !d["facts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["claim"] == "difference")
        );
    }
    for class in ["id", "third-party"] {
        let mut a = args(
            &["why-order", "--a", "c-1", "--b", "c-2"],
            "plan",
            "lodging",
        );
        a.extend(["--redaction".into(), format!("{class}.policy.json")]);
        let d = check_golden(&s, &format!("redacted-{class}"), &a);
        assert_eq!(d["answer"]["kind"], "ordered");
        if class == "third-party" {
            assert_eq!(
                claim(&d, "score_a")["availability"]["not_available"]["reason"],
                "redacted"
            );
            assert_eq!(claim(&d, "order_by")["availability"], "available");
        } else {
            let (_, bytes, md) = invoke(&s, &a);
            for withheld in ["c-1", "c-2", "explain-lodging"] {
                for b in [&bytes, &md] {
                    assert!(!String::from_utf8_lossy(b).contains(withheld));
                }
            }
            assert!(String::from_utf8_lossy(&md).contains("digest not computed"));
        }
    }
    // A record on one side and a bundle on the other shares one scope.
    let mut a = args(&["diff"], "record", "support");
    a.extend(["--other-bundle", "shifted.bundle.json"].map(str::to_string));
    a.extend(scope());
    invoke(&s, &a);
}

#[test]
fn unavailable_cancelled_unknown_and_malformed_recorded_order() {
    use rustev_contract::{replay::ReplayBundle, retention::Retention};
    let s = setup();
    let mut b = ReplayBundle::parse(&s.read("support.bundle.json")).unwrap();
    b.run.retention = Retention::DigestOnly {
        digest: b.run.retention.digest().clone(),
        expires_at_ms: b.run.retention.expires_at(),
    };
    s.write("support.bundle.json", &b.record_canonical().unwrap());
    let d = invoke(&s, &args(&["why-outcome"], "bundle", "support")).0;
    assert_eq!(d["answer"]["kind"], "not_available");
    assert!(
        d["facts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["availability"]["not_available"]["availability"] == "digest_only")
    );
    let mut v = value("support.record.json");
    v["core"] = json!("not_produced");
    v["termination"] = json!("cancelled");
    put(&s, "support.record.json", &v);
    let d = invoke(&s, &args(&["why-outcome"], "record", "support")).0;
    assert_eq!(
        claim(&d, "outcome")["availability"]["not_available"]["reason"],
        "not-produced"
    );
    assert_eq!(
        invoke(&s, &args(&["models"], "record", "support")).0["answer"]["kind"],
        "models"
    );
    let d = invoke(
        &s,
        &args(&["why-excluded", "--item", "absent"], "record", "lodging"),
    )
    .0;
    assert_eq!(d["answer"]["kind"], "unknown");
    assert_eq!(
        claim(&d, "pool")["availability"]["not_available"]["reason"],
        "not-recorded"
    );
    let d = invoke(
        &s,
        &args(&["why-selected", "--item", "c-1"], "record", "lodging"),
    )
    .0;
    assert_eq!(
        claim(&d, "filters_passed")["availability"]["not_available"]["reason"],
        "plan-not-supplied"
    );
    assert_eq!(claim(&d, "score")["availability"], "available");
    let mut v = value("lodging.record.json");
    let params = v.pointer_mut(PARAMS).unwrap();
    params["ranking"]["ranking"]["entries"][1]["position"] =
        params["ranking"]["ranking"]["entries"][0]["position"].clone();
    put(&s, "lodging.record.json", &v);
    let d = invoke(
        &s,
        &args(
            &["why-order", "--a", "c-1", "--b", "c-2"],
            "plan",
            "lodging",
        ),
    )
    .0;
    assert!(d["answer"]["scopes"][0]["first"].is_null());
    assert_eq!(d["answer"]["scopes"][0]["consistent"], false);
    let params = v.pointer_mut(PARAMS).unwrap();
    let entries = params["ranking"]["ranking"]["entries"]
        .as_array()
        .unwrap()
        .clone();
    params["ranking"]["ranking"]["entries"] = json!([entries[0]]);
    params["second"] = params["ranking"].clone();
    params["second"]["ranking"]["entries"] = json!([entries[1]]);
    params.as_object_mut().unwrap().remove("shortlist");
    put(&s, "lodging.record.json", &v);
    let d = invoke(
        &s,
        &args(
            &["why-order", "--a", "c-1", "--b", "c-2"],
            "record",
            "lodging",
        ),
    )
    .0;
    assert_eq!(d["answer"]["reason"], "different-scope");
    let mut v = value("support.record.json");
    for r in v["requests"].as_array_mut().unwrap() {
        for a in r["attempts"].as_array_mut().unwrap() {
            a["cost"]["charge"] = json!("unknown");
        }
    }
    put(&s, "support.record.json", &v);
    let d = invoke(&s, &args(&["models"], "record", "support")).0;
    assert!(
        claim(&d, "backend_cost")["value"]["unknown"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(claim(&d, "backend_cost")["value"]["observed"], 0);
}

#[test]
fn bundle_store_resolves_only_confined_regular_files() {
    use rustev_contract::{replay::ReplayBundle, retention::Retention};
    let s = setup();
    let mut b = ReplayBundle::parse(&frozen("support.bundle.json")).unwrap();
    let Retention::Retained {
        bytes,
        digest,
        expires_at_ms,
    } = b.run.retention.clone()
    else {
        panic!()
    };
    let reference = "a".repeat(64);
    std::fs::create_dir(s.path("store")).unwrap();
    s.write(&format!("store/{reference}"), bytes.as_bytes());
    b.run.retention = Retention::External {
        reference: reference.clone(),
        digest,
        expires_at_ms,
    };
    s.write("support.bundle.json", &b.record_canonical().unwrap());
    let mut a = args(&["why-outcome"], "bundle", "support");
    assert_eq!(invoke(&s, &a).0["answer"]["kind"], "not_available");
    a.extend(["--store", "store"].map(str::to_string));
    assert_eq!(invoke(&s, &a).0["answer"]["kind"], "proposed");
    std::fs::remove_file(s.path(&format!("store/{reference}"))).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        s.path("support.record.json"),
        s.path(&format!("store/{reference}")),
    )
    .unwrap();
    assert_eq!(invoke(&s, &a).0["answer"]["kind"], "not_available");
    if let Retention::External { reference, .. } = &mut b.run.retention {
        *reference = "../support.record.json".into();
    }
    s.write("support.bundle.json", &b.record_canonical().unwrap());
    assert_eq!(invoke(&s, &a).0["answer"]["kind"], "not_available");
}
