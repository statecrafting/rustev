mod common;
mod explain_support;
use common::*;
use explain_support::support_shifted as support;
use explain_support::*;
use rustev_contract::redaction::{Action, FieldClass};
use rustev_contract::{Document, explanation::*};
use rustev_eval::explain::{ExplainOptions, render_markdown};
use serde_json::{Value, json};

fn document(query: Value, answer: Value) -> Explanation {
    let digest = id('a');
    let side = json!({"name":"a","form":"record","record":{"digest":digest},"plan":null,"bundle":null,"capture":null,"decision_id":"decision","plan_id":digest});
    let mut sides = vec![side.clone()];
    if query == "diff" {
        let mut side = side;
        side["name"] = json!("b");
        sides.push(side);
    }
    let doc: Explanation = serde_json::from_value(json!({"schema":"rustev.explanation/1","explainer":"rustev-explain/1","facet_sources":[{"name":"served_identity","spec":"040-decision-explanation","path":"supply/<n>:/served"}],"sides":sides,"redaction":null,"query":query,"answer":answer,"truncation":null,"facts":[{"id":"f1","claim":"outcome","value":null,"availability":"available","derivation":null,"lineage":null,"truncated":null,"cites":[]}]})).unwrap();
    doc.validate().unwrap();
    doc
}
#[test]
fn every_answer_kind_has_a_deterministic_sentence() {
    let outcome = json!({"outcome":"f1","matched":"f1","adjustments":"f1","notices":"f1","judgment_derivation":"f1","judgment_lineage":"f1","termination":"f1","causes":[],"facets":[]});
    let mut cases = vec![];
    for (query, answers) in [
        (
            json!({"why_selected":{"item":"x"}}),
            vec![
                json!({"kind":"not_available","outcome":"f1"}),
                json!({"kind":"no_proposal","outcome":"f1"}),
                json!({"kind":"not_selected","see":"why_excluded"}),
                json!({"kind":"selected","scopes":[],"proposing_rule":"f1","adjustments":"f1","judgment_derivation":"f1","judgment_lineage":"f1","facets":[]}),
            ],
        ),
        (
            json!({"why_excluded":{"item":"x"}}),
            vec![
                json!({"kind":"not_available","outcome":"f1"}),
                json!({"kind":"no_proposal","outcome":"f1"}),
                json!({"kind":"excluded","scopes":[],"cuts":[],"selected_in":[],"facets":[]}),
                json!({"kind":"cut","cuts":[],"selected_in":[],"facets":[]}),
                json!({"kind":"selected","see":"why_selected"}),
                json!({"kind":"not_in_pool","pool":"f1","facets":[]}),
                json!({"kind":"unknown","pool":"f1","facets":[]}),
            ],
        ),
        (
            json!({"why_order":{"a":"x","b":"y"}}),
            vec![
                json!({"kind":"not_available","outcome":"f1"}),
                json!({"kind":"ordered","scopes":[]}),
                json!({"kind":"incomparable","reason":"different-scope"}),
            ],
        ),
        (
            json!("text_origin"),
            vec![
                json!({"kind":"not_available","outcome":"f1"}),
                json!({"kind":"text_fields","fields":[]}),
                json!({"kind":"no_text"}),
            ],
        ),
        (
            json!("models"),
            vec![
                json!({"kind":"not_available","record":"f1"}),
                json!({"kind":"no_model_calls","cost_summary":"f1"}),
                json!({"kind":"models","requests":[],"backends":[],"cost_summary":"f1"}),
            ],
        ),
        (
            json!("diff"),
            vec![
                json!({"kind":"identical","observations":[],"not_compared":[]}),
                json!({"kind":"diverged","first":"f1","why":"input-changed","differences":["f1"],"total":1,"truncated":0,"observations":[],"not_compared":[]}),
                json!({"kind":"withheld","redacted":{"reason":"redacted","policy":id('b'),"field_class":"enum","action":"drop"}}),
            ],
        ),
    ] {
        for answer in answers {
            cases.push((query.clone(), answer));
        }
    }
    cases.push((
        json!("why_outcome"),
        json!({"kind":"not_available","outcome":"f1"}),
    ));
    for kind in ["proposed", "escalated", "unresolved"] {
        let mut a = outcome.clone();
        a["kind"] = json!(kind);
        cases.push((json!("why_outcome"), a));
    }
    for (q, a) in cases {
        let doc = document(q, a);
        let text = render_markdown(&doc);
        assert!(text.starts_with("# Explanation:"));
        assert!(text.contains("| Claim | Value | Derivation | Lineage | Cites |"));
        assert!(text.lines().any(|s| s.ends_with('.')));
        assert_eq!(text, render_markdown(&doc));
    }
}
#[test]
fn unavailable_reasons_policy_cut_and_utf8_display_cut() {
    let mut doc = document(json!("text_origin"), json!({"kind":"no_text"}));
    let cases = [
        (
            json!({"reason":"not-recorded","facet":null}),
            "not recorded".into(),
        ),
        (
            json!({"reason":"not-recorded","facet":"field_origin"}),
            "not recorded (facet field_origin)".into(),
        ),
        (
            json!({"reason":"plan-not-supplied"}),
            "plan not supplied".into(),
        ),
        (
            json!({"reason":"retention","availability":"expired","item":"bundle:/run/retention/retained/digest"}),
            "retention expired (item bundle:/run/retention/retained/digest)".into(),
        ),
        (json!({"reason":"not-produced"}), "not produced".into()),
        (json!({"reason":"not-in-record"}), "not in record".into()),
        (
            json!({"reason":"redacted","policy":id('b'),"field_class":"id","action":"drop"}),
            format!("redacted by policy {}", id('b')),
        ),
        (
            json!({"reason":"redacted","policy":id('b'),"field_class":"id","action":"digest"}),
            format!("redacted by policy {} (digest not computed)", id('b')),
        ),
    ];
    for (u, expected) in cases {
        doc.facts[0].availability = Availability::NotAvailable(serde_json::from_value(u).unwrap());
        doc.validate().unwrap();
        assert!(render_markdown(&doc).contains(&format!("not available: {expected}")));
    }
    doc.facts[0].availability = Availability::Available;
    doc.facts[0].value = json!("é".repeat(200));
    doc.validate().unwrap();
    let text = render_markdown(&doc);
    assert!(text.contains(&format!("`\"{}...`", "é".repeat(127))));
    assert_eq!(doc.facts[0].value.as_str().unwrap().len(), 400);
    doc.facts[0].value = json!("a|`b\nc");
    doc.facts[0].truncated =
        Some(serde_json::from_value(json!({"policy":id('b'),"bytes":8})).unwrap());
    doc.truncation = Some(Truncation { total: 2 });
    doc.validate().unwrap();
    let text = render_markdown(&doc);
    assert!(text.contains("a&#124;&#96;b"));
    assert!(text.contains(&format!(" (truncated by policy {} to 8 bytes)", id('b'))));
    assert!(text.contains("truncated: 1 of 2 facts"));
}
#[tokio::test]
async fn newly_implemented_queries_render_real_records() {
    let r = explain_support::lodging().await;
    for (query, sentence) in [
        (
            selected("c-1"),
            "The item appears in the recorded proposal parameters.",
        ),
        (excluded("c-1"), "The item is selected; see why_selected."),
        (
            order("c-1", "c-2"),
            "The recorded scopes state the item ordering.",
        ),
    ] {
        let doc = record(&r, true, query, None);
        let text = render_markdown(&doc);
        assert!(text.contains(sentence));
        assert_eq!(text, render_markdown(&doc));
    }
    let r = explain_support::support(true).await;
    let doc = record(&r, true, Query::Models, None);
    let text = render_markdown(&doc);
    assert!(text.contains("The record lists model requests, attempts and costs."));
    assert_eq!(text, render_markdown(&doc));
}

#[tokio::test]
async fn withheld_bytes_absent_in_document_and_rendering() {
    let a = support(false).await;
    let mut b = a.run.clone();
    b.decision_id = "SECRET-DECISION-THAT-MUST-NOT-APPEAR".into();
    let p = policy(FieldClass::Id, Action::Drop);
    let doc = records(
        &a.run,
        &b,
        None,
        &ExplainOptions {
            redaction: Some(&p),
        },
    );
    let bytes = String::from_utf8(doc.record_canonical().unwrap()).unwrap();
    let text = render_markdown(&doc);
    assert!(!bytes.contains(&b.decision_id));
    assert!(!text.contains(&b.decision_id));
    assert_eq!(text, render_markdown(&doc));
}
