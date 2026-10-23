//! Deterministic Markdown, using only the explanation document.
use super::*;
use serde_json::Value;
use std::fmt::Write;

// HTML entities cannot terminate a code span or a Markdown table cell.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace('`', "&#96;")
        .replace('\n', "&#10;")
        .replace('\r', "&#13;")
}
fn json(v: &Value) -> String {
    let bytes = rustev_contract::canonical::record_canonical_bytes(v).unwrap_or_default();
    let text = String::from_utf8(bytes).unwrap_or_default();
    let mut end = text.len().min(256);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "`{}{}`",
        escape(&text[..end]),
        if end < text.len() { "..." } else { "" }
    )
}
fn encoded<T: serde::Serialize>(v: &T) -> String {
    json(&serde_json::to_value(v).unwrap_or(Value::Null))
}
fn label<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}
fn unavailable(u: &Unavailable) -> String {
    let reason = match u {
        Unavailable::NotRecorded { facet } => format!(
            "not recorded{}",
            facet
                .map(|f| format!(" (facet {})", label(&f)))
                .unwrap_or_default()
        ),
        Unavailable::PlanNotSupplied {} => "plan not supplied".into(),
        Unavailable::Retention { item, availability } => {
            format!("retention {} (item {})", label(availability), item.as_str())
        }
        Unavailable::NotProduced {} => "not produced".into(),
        Unavailable::NotInRecord {} => "not in record".into(),
        Unavailable::Redacted { policy, action, .. } => format!(
            "redacted by policy {policy}{}",
            if *action == WithholdingAction::Digest {
                " (digest not computed)"
            } else {
                ""
            }
        ),
    };
    format!("not available: {}", escape(&reason))
}
fn item(i: &Item) -> String {
    match i {
        Item::String(s) => encoded(s),
        Item::Withheld(w) => unavailable(&w.not_available),
    }
}
fn answer(a: &Answer) -> &'static str {
    match a {
        Answer::WhySelected(a) => match a {
            SelectedAnswer::NotAvailable { .. } => "Selection evidence is not available.",
            SelectedAnswer::NoProposal { .. } => "The recorded outcome is not a proposal.",
            SelectedAnswer::NotSelected { .. } => "The item is not selected; see why_excluded.",
            SelectedAnswer::Selected { .. } => {
                "The item appears in the recorded proposal parameters."
            }
        },
        Answer::WhyExcluded(a) => match a {
            ExcludedAnswer::NotAvailable { .. } => "Exclusion evidence is not available.",
            ExcludedAnswer::NoProposal { .. } => "The recorded outcome is not a proposal.",
            ExcludedAnswer::Excluded { .. } => "The record states reasons for excluding the item.",
            ExcludedAnswer::Cut { .. } => "The item was cut from a recorded shortlist.",
            ExcludedAnswer::Selected { .. } => "The item is selected; see why_selected.",
            ExcludedAnswer::NotInPool { .. } => "The item is not in the recorded pool.",
            ExcludedAnswer::Unknown { .. } => {
                "The evidence does not establish why the item is absent."
            }
        },
        Answer::WhyOrder(a) => match a {
            OrderAnswer::NotAvailable { .. } => "Ordering evidence is not available.",
            OrderAnswer::Ordered { .. } => "The recorded scopes state the item ordering.",
            OrderAnswer::Incomparable(_) => {
                "The items cannot be compared in a shared recorded ranking."
            }
        },
        Answer::WhyOutcome(a) => match a {
            OutcomeAnswer::NotAvailable { .. } => "Outcome evidence is not available.",
            OutcomeAnswer::Proposed(_) => "The recorded outcome is a proposal.",
            OutcomeAnswer::Escalated(_) => "The recorded outcome is an escalation.",
            OutcomeAnswer::Unresolved(_) => "The recorded outcome is unresolved.",
        },
        Answer::TextOrigin(a) => match a {
            TextAnswer::NotAvailable { .. } => "Text evidence is not available.",
            TextAnswer::TextFields { .. } => {
                "The record contains the listed text fields and origin evidence."
            }
            TextAnswer::NoText {} => "The recorded outcome contains no text fields.",
        },
        Answer::Models(a) => match a {
            ModelsAnswer::NotAvailable { .. } => "Model request evidence is not available.",
            ModelsAnswer::Models { .. } => "The record lists model requests, attempts and costs.",
            ModelsAnswer::NoModelCalls { .. } => "The record contains no model calls.",
        },
        Answer::Diff(a) => match a {
            DiffAnswer::Identical { .. } => "The compared decision values are identical.",
            DiffAnswer::Diverged { .. } => "The compared decision values diverged.",
            DiffAnswer::Withheld { .. } => "The comparison is withheld by policy.",
        },
    }
}
/// Render a document without consulting evidence or recomputing an answer.
pub fn render_markdown(doc: &Explanation) -> String {
    let mut out = format!(
        "# Explanation: {}\n\n| Side | Form | Decision | Plan | Capture |\n| --- | --- | --- | --- | --- |\n",
        encoded(&doc.query)
    );
    for s in &doc.sides {
        writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            label(&s.name),
            label(&s.form),
            item(&s.decision_id),
            item(&s.plan_id),
            encoded(&s.capture)
        )
        .unwrap();
    }
    writeln!(
        out,
        "\n{}\n\n| Claim | Value | Derivation | Lineage | Cites |\n| --- | --- | --- | --- | --- |",
        answer(&doc.answer)
    )
    .unwrap();
    for f in &doc.facts {
        let mut value = match &f.availability {
            Availability::Available => json(&f.value),
            Availability::NotAvailable(u) => unavailable(u),
        };
        if f.availability == Availability::Available
            && let Some(t) = &f.truncated
        {
            write!(
                value,
                " (truncated by policy {} to {} bytes)",
                t.policy, t.bytes
            )
            .unwrap();
        }
        writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            label(&f.claim),
            value,
            encoded(&f.derivation),
            encoded(&f.lineage),
            encoded(&f.cites)
        )
        .unwrap();
    }
    if let Some(t) = &doc.truncation {
        writeln!(out, "\ntruncated: {} of {} facts", doc.facts.len(), t.total).unwrap();
    }
    out
}
