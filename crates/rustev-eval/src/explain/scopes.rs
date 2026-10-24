//! Shared ordered parameter walk and evidence helpers for item queries.
use super::facts::{Builder, J, closure, escape, matched};
use super::*;
use rustev_contract::judgment::Derivation;
use rustev_contract::redaction::FieldClass;
use serde_json::{Value, json};

pub(super) struct Scope<'a> {
    pub param: String,
    pub kind: ValueKind,
    pub value: &'a Value,
    pub path: String,
    pub linked: Option<String>,
}
impl Scope<'_> {
    pub fn index(&self, item: &str) -> Option<usize> {
        self.member(item).map(|(index, _)| index)
    }
    pub fn member(&self, item: &str) -> Option<(usize, String)> {
        let key = match self.kind {
            ValueKind::Filtered => "eligible",
            ValueKind::Shortlist => "ids",
            ValueKind::Ranking => "entries",
        };
        self.value[key]
            .as_array()?
            .iter()
            .position(|v| {
                if self.kind == ValueKind::Ranking {
                    v["candidate"].as_str() == Some(item)
                } else {
                    v.as_str() == Some(item)
                }
            })
            .map(|i| (i, format!("{}/{key}/{i}", self.path)))
    }
    pub fn declaration(&self, set: &evidence::Set) -> Option<String> {
        let id = self.linked.as_deref()?;
        let i = set
            .get("plan", "/definition/steps")?
            .as_array()?
            .iter()
            .position(|v| v["id"].as_str() == Some(id))?;
        Some(format!("/definition/steps/{i}/body/exact"))
    }
    pub fn copy(
        &self,
        b: &mut Builder<'_>,
        claim: Claim,
        member: &str,
        number: bool,
    ) -> Result<FactId, ExplainError> {
        let lineage = match (self.linked.as_deref(), &b.set.plan) {
            (Some(s), Some(p)) => closure(p, s).or_else(|| b.lineage()),
            _ => b.lineage(),
        };
        let class = if number {
            FieldClass::Number
        } else {
            b.value_class(lineage.as_ref(), false)
        };
        let id = b.copy(
            claim,
            "record",
            &format!("{}/{member}", self.path),
            class,
            false,
        )?;
        b.annotate(&id, self.linked.as_deref(), false)?;
        Ok(id)
    }
}
pub(super) fn walk(set: &evidence::Set) -> Vec<Scope<'_>> {
    // Recursion is bounded: every value here comes from a document that
    // passed its typed, bounded parse (`Document::parse`, at most
    // `max_depth` nesting; serde_json also stops at 128).
    fn visit<'a>(
        out: &mut Vec<Scope<'a>>,
        param: &str,
        v: &'a Value,
        path: String,
        linked: &Option<String>,
    ) {
        for (name, kind) in [
            ("filtered", ValueKind::Filtered),
            ("shortlist", ValueKind::Shortlist),
            ("ranking", ValueKind::Ranking),
        ] {
            if let Some(value) = v.get(name) {
                out.push(Scope {
                    param: param.into(),
                    kind,
                    value,
                    path: format!("{path}/{name}"),
                    linked: linked.clone(),
                });
                return;
            }
        }
        if let Some(a) = v.get("list").and_then(Value::as_array) {
            for (i, v) in a.iter().enumerate() {
                visit(out, param, v, format!("{path}/list/{i}"), linked);
            }
        } else if let Some(o) = v.get("record").and_then(Value::as_object) {
            for (k, v) in o {
                visit(
                    out,
                    param,
                    v,
                    format!("{path}/record/{}", escape(k)),
                    linked,
                );
            }
        }
    }
    let mut out = vec![];
    if let Some(params) = set
        .get("record", &format!("{J}/outcome/propose/params"))
        .and_then(Value::as_object)
    {
        for (name, v) in params {
            let linked = matched(set)
                .and_then(|(_, _, p)| set.get("plan", &format!("{p}/then/propose/params")))
                .and_then(Value::as_array)
                .and_then(|a| a.iter().find(|p| p["name"].as_str() == Some(name)))
                .and_then(|p| p.pointer("/value/ref"))
                .and_then(Value::as_str)
                .and_then(|s| s.strip_prefix("step:"))
                .map(str::to_string);
            visit(
                &mut out,
                name,
                v,
                format!("{J}/outcome/propose/params/{}", escape(name)),
                &linked,
            );
        }
    }
    out
}
// Derived facts cite the actual operands; no synthetic path is introduced.
pub(super) fn fact(
    b: &mut Builder<'_>,
    claim: Claim,
    mut value: Value,
    cites: Vec<String>,
    class: FieldClass,
) -> Result<FactId, ExplainError> {
    let mut cites = cites
        .into_iter()
        .map(|s| EvidencePath::parse(&s).map_err(|e| ExplainError::parse("fact", e)))
        .collect::<Result<Vec<_>, _>>()?;
    cites.sort();
    cites.dedup();
    let apply = |value: &mut Value| {
        if let Some(fields) = value.as_object_mut() {
            let mut cut = None;
            for (key, value) in fields {
                let field_class = match (claim, key.as_str()) {
                    (Claim::Exclusion, "reason_kind") => FieldClass::Enum,
                    (Claim::CutByK, "k") | (Claim::OrderBy, "rule") => FieldClass::PlanMetadata,
                    _ => class,
                };
                cut = redact::apply(value, field_class, b.policy, false)?.or(cut);
            }
            Ok(cut)
        } else {
            redact::apply(value, class, b.policy, false)
        }
    };
    let (availability, truncated) = match apply(&mut value) {
        Ok(t) => (Availability::Available, t),
        Err(u) => {
            value = Value::Null;
            (Availability::NotAvailable(u), None)
        }
    };
    b.put(Fact {
        id: String::new(),
        claim,
        value,
        availability,
        derivation: None,
        lineage: None,
        truncated,
        cites,
    })
}
pub(super) fn comparison(
    b: &mut Builder<'_>,
    id: &FactId,
    linked: Option<&str>,
) -> Result<(), ExplainError> {
    b.annotate(id, linked, false)?;
    if let Some(d) = b
        .facts
        .iter_mut()
        .find(|f| f.id == id.0)
        .and_then(|f| f.derivation.as_mut())
        && d.class != Derivation::ExactDerived
    {
        d.class = Derivation::MixedDerived;
    }
    Ok(())
}
pub(super) fn item(s: &str, b: &Builder<'_>) -> Item {
    let mut i = Item::String(s.into());
    redact::item(&mut i, b.policy, FieldClass::Id);
    i
}
pub(super) fn declaration(
    b: &mut Builder<'_>,
    claim: Claim,
    path: Option<String>,
) -> Result<FactId, ExplainError> {
    b.copy(
        claim,
        "plan",
        path.as_deref().unwrap_or("/missing"),
        FieldClass::PlanMetadata,
        false,
    )
}
pub(super) fn facets(b: &mut Builder<'_>) -> Result<Vec<FactId>, ExplainError> {
    let mut out = vec![];
    for (c, f) in [
        (Claim::Presence, Facet::Presence),
        (Claim::Tier, Facet::Tier),
    ] {
        if b.list(1) {
            out.push(b.facet(c, f)?);
        }
    }
    Ok(out)
}
pub(super) fn outcome(b: &mut Builder<'_>) -> Result<FactId, ExplainError> {
    b.judgment(Claim::Outcome, "outcome", FieldClass::Enum, true)
}
pub(super) fn position(
    b: &mut Builder<'_>,
    s: &Scope<'_>,
    index: usize,
    claim: Claim,
) -> Result<FactId, ExplainError> {
    if s.kind == ValueKind::Ranking {
        return s.copy(b, claim, &format!("entries/{index}/position"), true);
    }
    let id = fact(
        b,
        claim,
        json!(index + 1),
        vec![format!("record:{}/ids/{index}", s.path)],
        FieldClass::Number,
    )?;
    b.annotate(&id, s.linked.as_deref(), false)?;
    Ok(id)
}
