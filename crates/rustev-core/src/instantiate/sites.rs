use super::{InstantiationCode as C, InstantiationRefusal, refusal};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteKind {
    Expr,
    ProposalValue,
    MemberSet,
    DistributionThreshold,
    MinDeterminism,
    Artifact,
    LimitInteger,
    DeadlineMs,
    DistributionTolerance,
    AdjustmentScale,
    Enabled,
    ExactArgJson,
    PromptLiteral,
    Vocabulary,
}
/// Item identity and location survive pruning and do not depend on array renumbering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub pointer: String,
    pub subject: String,
    pub input: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub pointer: String,
    pub parameter: String,
    pub kind: SiteKind,
    pub owner: Option<Item>,
}
pub(super) fn reference(v: &Value) -> Option<&str> {
    v.as_object()
        .filter(|o| o.len() == 1)?
        .get("param")?
        .as_str()
}
fn child(p: &str, k: &str) -> String {
    format!("{p}/{}", k.replace('~', "~0").replace('/', "~1"))
}
pub(super) fn items(v: &Value) -> Vec<Item> {
    let mut out = Vec::new();
    for (p, kind, key) in [
        ("/inputs", "input", "name"),
        ("/policy/adjustments", "adjustment", "id"),
        ("/policy/on_unresolved", "handler", ""),
        ("/policy/rules", "rule", "id"),
        ("/steps", "step", "id"),
    ] {
        if let Some(a) = v.pointer(p).and_then(Value::as_array) {
            for (i, v) in a.iter().enumerate() {
                out.push(Item {
                    pointer: format!("{p}/{i}"),
                    subject: format!(
                        "{kind}:{}",
                        if key.is_empty() {
                            i.to_string()
                        } else {
                            v[key].as_str().unwrap_or("?").into()
                        }
                    ),
                    input: kind == "input",
                });
            }
        }
    }
    out
}
#[derive(Clone, Copy)]
enum Grammar {
    Expr,
    Predicate,
    Condition,
}
fn grammar(v: &Value, p: &str, g: Grammar, m: &mut BTreeMap<String, SiteKind>) {
    use Grammar::*;
    if matches!(g, Expr) {
        m.insert(p.into(), SiteKind::Expr);
    }
    let Some(o) = v.as_object() else {
        return;
    };
    for (tag, body) in o {
        let q = child(p, tag);
        let fields: &[&str] = match (g, tag.as_str()) {
            (Expr, "add" | "sub" | "mul" | "div") | (Predicate, "cmp") => &["left", "right"],
            (Expr, "round") | (Predicate, "member") => &["value"],
            (Expr, "fx") => &["amount", "from", "to"],
            _ => &[],
        };
        for f in fields {
            grammar(&body[*f], &child(&q, f), Expr, m);
        }
        match (g, tag.as_str()) {
            (Expr, "len" | "to_decimal") => grammar(body, &q, Expr, m),
            (Predicate | Condition, "all" | "any") => {
                if let Some(a) = body.as_array() {
                    for (i, v) in a.iter().enumerate() {
                        grammar(v, &format!("{q}/{i}"), g, m);
                    }
                }
            }
            (Predicate | Condition, "not") => grammar(body, &q, g, m),
            (Condition, "exact") => grammar(body, &q, Predicate, m),
            (Predicate, "member") => {
                m.insert(child(&q, "set"), SiteKind::MemberSet);
            }
            (Condition, "mass_at_least" | "top_mass_below") => {
                m.insert(child(&q, "threshold"), SiteKind::DistributionThreshold);
            }
            (Condition, "expectation_at_least") => {
                m.insert(child(&q, "value"), SiteKind::DistributionThreshold);
            }
            _ => {}
        }
    }
}
/// The exact/1 argument grammar is enumerated, rather than probed with a
/// partially substituted document: simultaneous references cannot change a
/// site's classification. These are precisely the Expr fields of ops::decode.
/// Other exact args are raw JSON; new spec 032 positions are excluded below.
fn exact(v: &Value, p: &str, m: &mut BTreeMap<String, SiteKind>) {
    use Grammar::*;
    if v["version"].as_u64() != Some(1) {
        return;
    }
    let args = &v["args"];
    let p = child(p, "args");
    let fields: &[&str] = match v["op"].as_str().unwrap_or("") {
        "timestamp_diff" => &["later", "earlier"],
        "arith" => &["expr"],
        "fx_convert" => &["amount", "from", "to"],
        "member_of" => &["value"],
        "top_k" => &["key"],
        _ => &[],
    };
    for f in fields {
        grammar(&args[*f], &child(&p, f), Expr, m);
    }
    match v["op"].as_str().unwrap_or("") {
        "count_where" | "window_filter" | "compare" => {
            grammar(&args["predicate"], &child(&p, "predicate"), Predicate, m)
        }
        "filter_with_reasons" => {
            if let Some(a) = args["rules"].as_array() {
                for (i, r) in a.iter().enumerate() {
                    grammar(
                        &r["predicate"],
                        &format!("{p}/rules/{i}/predicate"),
                        Predicate,
                        m,
                    );
                }
            }
        }
        _ => {}
    }
}
/// One table for validation and later substitution, in canonical document
/// order (object keys in byte order, arrays in authored order).
pub fn reference_sites(v: &Value) -> Result<Vec<Site>, InstantiationRefusal> {
    let owners = items(v);
    let mut m = BTreeMap::new();
    let mut exact_roots = Vec::new();
    for item in &owners {
        let p = &item.pointer;
        let v = v.pointer(p).unwrap_or(&Value::Null);
        if !item.input {
            m.insert(child(p, "enabled"), SiteKind::Enabled);
        }
        if p.starts_with("/steps/") {
            let e = &v["body"]["exact"];
            if e.is_object() && !matches!(e["op"].as_str(), Some("join_keyed" | "merge_fields")) {
                let q = format!("{p}/body/exact");
                exact(e, &q, &mut m);
                exact_roots.push(child(&q, "args"));
            }
            for (member, kind) in [
                ("template/params", SiteKind::PromptLiteral),
                ("vocabularies", SiteKind::Vocabulary),
            ] {
                let q = format!("{p}/body/extract/{member}");
                if let Some(o) = v
                    .pointer(&format!("/body/extract/{member}"))
                    .and_then(Value::as_object)
                {
                    for name in o.keys() {
                        m.insert(child(&q, name), kind);
                    }
                }
            }
            for (f, k) in [
                ("min_determinism", SiteKind::MinDeterminism),
                ("artifact", SiteKind::Artifact),
            ] {
                m.insert(format!("{p}/body/semantic/backend/{f}"), k);
            }
        }
        if p.starts_with("/policy/rules/") || p.starts_with("/policy/adjustments/") {
            grammar(&v["when"], &child(p, "when"), Grammar::Condition, &mut m);
        }
        if p.starts_with("/policy/rules/")
            && let Some(a) = v.pointer("/then/propose/params").and_then(Value::as_array)
        {
            for i in 0..a.len() {
                m.insert(
                    format!("{p}/then/propose/params/{i}/value"),
                    SiteKind::ProposalValue,
                );
            }
        }
        if p.starts_with("/policy/adjustments/") {
            m.insert(child(p, "scale"), SiteKind::AdjustmentScale);
        }
    }
    for (f, k) in [
        ("max_semantic_requests", SiteKind::LimitInteger),
        ("max_projection_bytes", SiteKind::LimitInteger),
        ("deadline_ms", SiteKind::DeadlineMs),
        ("distribution_tolerance", SiteKind::DistributionTolerance),
    ] {
        m.insert(format!("/limits/{f}"), k);
    }
    let mut refs = Vec::new();
    collect(v, "", &mut refs);
    refs.into_iter()
        .map(|(p, n)| {
            let raw = exact_roots
                .iter()
                .any(|q| p == *q || p.starts_with(&format!("{q}/")));
            // New graded/member_ref positions are never sites, including in args.
            let excluded = p.split('/').any(|s| matches!(s, "graded" | "member_ref"));
            let kind = if excluded {
                None
            } else {
                m.get(&p).copied().or(if raw {
                    Some(SiteKind::ExactArgJson)
                } else {
                    None
                })
            };
            let owner = owners
                .iter()
                .find(|i| p.starts_with(&format!("{}/", i.pointer)))
                .cloned();
            kind.map(|kind| Site {
                pointer: p.clone(),
                parameter: n,
                kind,
                owner,
            })
            .ok_or_else(|| {
                refusal(
                    1,
                    C::InvalidDefinitionDocument,
                    format!(
                        "definition:{}@{}",
                        v["name"].as_str().unwrap_or("?"),
                        v["version"].as_str().unwrap_or("?")
                    ),
                    format!("category 1: parameter reference outside a declared site at {p}"),
                )
            })
        })
        .collect()
}
fn collect(v: &Value, p: &str, out: &mut Vec<(String, String)>) {
    if let Some(n) = reference(v) {
        out.push((p.into(), n.into()));
        return;
    }
    match v {
        Value::Object(o) => {
            let mut keys: Vec<_> = o.keys().collect();
            keys.sort();
            for k in keys {
                let v = &o[k];
                if p.is_empty() && k == "parameters" {
                    continue;
                }
                collect(v, &child(p, k), out);
            }
        }
        Value::Array(a) => {
            for (i, v) in a.iter().enumerate() {
                collect(v, &format!("{p}/{i}"), out);
            }
        }
        _ => {}
    }
}
pub(super) fn placeholder(k: SiteKind) -> Value {
    match k {
        SiteKind::Expr | SiteKind::ProposalValue => json!({"lit":{"integer":0}}),
        SiteKind::MemberSet | SiteKind::AdjustmentScale => json!([]),
        SiteKind::DistributionThreshold | SiteKind::DistributionTolerance => json!("0.01"),
        SiteKind::MinDeterminism => json!("unspecified"),
        SiteKind::Artifact => json!("any"),
        SiteKind::PromptLiteral => json!({"literal":{"integer":0}}),
        SiteKind::Vocabulary => json!(["placeholder"]),
        SiteKind::Enabled => json!(true),
        _ => json!(0),
    }
}
