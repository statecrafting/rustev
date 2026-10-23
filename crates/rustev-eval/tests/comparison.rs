#[path = "selection_support/mod.rs"]
mod support;
use rustev_contract::Document;
use rustev_eval::comparison::*;
use rustev_eval::repeat::{FigureDoc, Observation};
use serde_json::json;
use support::*;

#[test]
fn comparison_rows_metrics_provenance_and_goldens() {
    let f = Fixture::new();
    let (base, b, c) = f.reports();
    let br = f.runs(false);
    let cr = f.runs(true);
    let doc = compare_candidates(
        &base,
        &RepeatEvidence {
            report: &b,
            inputs: f.inputs(&br),
        },
        &[NamedCandidate {
            name: "candidate",
            evidence: RepeatEvidence {
                report: &c,
                inputs: f.inputs(&cr),
            },
        }],
    )
    .unwrap();
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].name, "base");
    let ComparisonResult::Measured { metrics, .. } = &doc.rows[1].result else {
        panic!("{:?}", doc.rows[1]);
    };
    assert!(metrics.iter().any(|m| m.name == "candidate_outcome_change"));
    for name in [
        "latency_ms/p0.5",
        "latency_ms/p0.95",
        "cost_per_1000/observed",
    ] {
        let m = metrics.iter().find(|m| m.name == name).unwrap();
        assert_eq!(m.observation, Some(Observation::Historical));
        assert!(matches!(m.pooled, FigureDoc::Unknown(_)));
    }
    let bytes = doc.record_canonical().unwrap();
    assert_eq!(Comparison::parse(&bytes).unwrap(), doc);
    golden("comparison.json", &bytes);
    golden("comparison.md", render_markdown(&doc).as_bytes());
    for (field, value) in [
        ("schema", json!("wrong")),
        ("extra", json!(0)),
        (
            "rows",
            json!([{"name":"bad","repeat_report":digest(1),"result":{"unknown":{"reason":"x","extra":0}}}]),
        ),
    ] {
        let mut v = serde_json::to_value(&doc).unwrap();
        v[field] = value;
        assert!(Comparison::parse(v.to_string().as_bytes()).is_err());
    }
    let duplicate = String::from_utf8(bytes).unwrap().replacen(
        "\"schema\":",
        "\"schema\":\"rustev.comparison/1\",\"schema\":",
        1,
    );
    assert!(Comparison::parse(duplicate.as_bytes()).is_err());
    assert_eq!(Comparison::LIMITS, rustev_contract::limits::REPLAY_V1);
}
#[test]
fn candidate_count_name_bounds_collisions_and_mismatches() {
    let f = Fixture::new();
    let (base, b, c) = f.reports();
    let br = f.runs(false);
    let cr = f.runs(true);
    let be = RepeatEvidence {
        report: &b,
        inputs: f.inputs(&br),
    };
    let names: Vec<_> = (0..65).map(|i| format!("c{i}")).collect();
    let named = |n| NamedCandidate {
        name: n,
        evidence: RepeatEvidence {
            report: &c,
            inputs: f.inputs(&cr),
        },
    };
    let candidates: Vec<_> = names.iter().map(|n| named(n.as_str())).collect();
    assert!(compare_candidates(&base, &be, &candidates[..64]).is_ok());
    assert!(compare_candidates(&base, &be, &candidates).is_err());
    assert!(compare_candidates(&base, &be, &[]).is_err());
    let max = "x".repeat(128);
    assert!(compare_candidates(&base, &be, &[named(&max)]).is_ok());
    let over = "x".repeat(129);
    assert!(compare_candidates(&base, &be, &[named(&over)]).is_err());
    for n in ["", "../x", ".hidden", "a/b", "a b", "é"] {
        assert!(compare_candidates(&base, &be, &[named(n)]).is_err());
    }
    assert!(compare_candidates(&base, &be, &[named("x"), named("X")]).is_err());
    // A candidate may not take the baseline's row name, in any case.
    assert!(compare_candidates(&base, &be, &[named(&base.name)]).is_err());
    assert!(compare_candidates(&base, &be, &[named(&base.name.to_ascii_uppercase())]).is_err());
    for field in ["dataset", "repeat_config", "evaluator_config", "plan"] {
        let mut v = serde_json::to_value(&c).unwrap();
        v[field] = json!(digest(99));
        let bad = serde_json::from_value(v).unwrap();
        let d = compare_candidates(
            &base,
            &be,
            &[NamedCandidate {
                name: "bad",
                evidence: RepeatEvidence {
                    report: &bad,
                    inputs: f.inputs(&cr),
                },
            }],
        )
        .unwrap();
        assert!(matches!(d.rows[1].result, ComparisonResult::Unknown { .. }));
    }
}
#[test]
fn rendering_escapes_untrusted_text_and_keeps_reason_counts() {
    let doc = Comparison {
        schema: COMPARISON.into(),
        baseline: serde_json::from_value(json!(digest(1))).unwrap(),
        rows: vec![ComparisonRow {
            name: "<b>|\n".into(),
            repeat_report: serde_json::from_value(json!(digest(2))).unwrap(),
            result: ComparisonResult::Unknown {
                reason: "<script>*".into(),
            },
        }],
    };
    let text = render_markdown(&doc);
    assert!(!text.contains("<script>"));
    assert!(text.contains("&lt;script&gt;\\*"));
    assert!(text.contains("&#124;&#10;"));
}
#[test]
fn abstentions_and_incomparable_reasons_are_counted_and_unknown_rules_are_labeled() {
    let mut f = Fixture::new();
    f.second_outcome(false, false);
    f.second_outcome(true, true);
    let (base, b, c) = f.reports();
    let br = f.runs(false);
    let cr = f.runs(true);
    let doc = compare_candidates(
        &base,
        &RepeatEvidence {
            report: &b,
            inputs: f.inputs(&br),
        },
        &[NamedCandidate {
            name: "candidate",
            evidence: RepeatEvidence {
                report: &c,
                inputs: f.inputs(&cr),
            },
        }],
    )
    .unwrap();
    let ComparisonResult::Measured { abstentions, .. } = &doc.rows[0].result else {
        panic!()
    };
    assert_eq!(abstentions["uncertain"], 1);
    let ComparisonResult::Measured { incomparable, .. } = &doc.rows[1].result else {
        panic!()
    };
    assert_eq!(incomparable["bundle-missing"], 1);
    golden("comparison-reasons.json", &doc.record_canonical().unwrap());
    golden("comparison-reasons.md", render_markdown(&doc).as_bytes());
    let mut f = Fixture::new();
    for (e, c) in [(&mut f.b, &mut f.bc), (&mut f.c, &mut f.cc)] {
        e.config.adapter.rules = Some(rustev_eval::config::AdapterRules::Opaque);
        Fixture::bind(e, c);
    }
    let (base, b, c) = f.reports();
    let br = f.runs(false);
    let cr = f.runs(true);
    let doc = compare_candidates(
        &base,
        &RepeatEvidence {
            report: &b,
            inputs: f.inputs(&br),
        },
        &[NamedCandidate {
            name: "candidate",
            evidence: RepeatEvidence {
                report: &c,
                inputs: f.inputs(&cr),
            },
        }],
    )
    .unwrap();
    assert!(
        doc.rows
            .iter()
            .all(|r| matches!(r.result, ComparisonResult::Unknown { .. }))
    );
}
