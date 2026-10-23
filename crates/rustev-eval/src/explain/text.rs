use super::facts::{Builder, J, closure, escape, matched};
use super::*;
use serde_json::Value;

pub(super) fn build(b: &mut Builder<'_>) -> Result<Answer, ExplainError> {
    if let Some(u) = b.set.judgment_absent() {
        return Ok(Answer::TextOrigin(TextAnswer::NotAvailable {
            outcome: b.unavailable(Claim::Outcome, u)?,
        }));
    }
    let mut fields = vec![];
    let mut any = false;
    if let Some(params) = b
        .set
        .get("record", &format!("{J}/outcome/propose/params"))
        .and_then(Value::as_object)
    {
        for (name, v) in params {
            let linked = matched(b.set)
                .and_then(|(_, _, path)| {
                    b.set
                        .get("plan", &format!("{path}/then/propose/params"))
                        .and_then(Value::as_array)
                        .and_then(|a| a.iter().find(|p| p["name"].as_str() == Some(name)))
                        .and_then(|p| p.pointer("/value/ref"))
                })
                .and_then(Value::as_str)
                .and_then(|s| s.strip_prefix("step:"))
                .map(str::to_string);
            visit(
                b,
                v,
                &format!("{J}/outcome/propose/params/{}", escape(name)),
                linked.as_deref(),
                &mut fields,
                &mut any,
            )?;
        }
    }
    Ok(Answer::TextOrigin(if any {
        TextAnswer::TextFields { fields }
    } else {
        TextAnswer::NoText {}
    }))
}
fn visit(
    b: &mut Builder<'_>,
    v: &Value,
    path: &str,
    linked: Option<&str>,
    fields: &mut Vec<TextField>,
    any: &mut bool,
) -> Result<(), ExplainError> {
    if v.get("text").is_some() {
        *any = true;
        // Both keyed facts belong to one list entry and are retained together.
        if b.list(2) {
            let lineage = match (linked, &b.set.plan) {
                (Some(s), Some(p)) => closure(p, s).or_else(|| b.lineage()),
                _ => b.lineage(),
            };
            let class = b.value_class(lineage.as_ref(), false);
            let text_field = b.copy(
                Claim::TextField,
                "record",
                &format!("{path}/text"),
                class,
                false,
            )?;
            b.annotate(&text_field, linked, false)?;
            let field_origin = b.facet(Claim::FieldOrigin, Facet::FieldOrigin)?;
            fields.push(TextField {
                text_field,
                field_origin,
            });
        }
    } else if let Some(a) = v.get("list").and_then(Value::as_array) {
        for (i, v) in a.iter().enumerate() {
            visit(b, v, &format!("{path}/list/{i}"), linked, fields, any)?;
        }
    } else if let Some(o) = v.get("record").and_then(Value::as_object) {
        for (k, v) in o {
            visit(
                b,
                v,
                &format!("{path}/record/{}", escape(k)),
                linked,
                fields,
                any,
            )?;
        }
    }
    Ok(())
}
