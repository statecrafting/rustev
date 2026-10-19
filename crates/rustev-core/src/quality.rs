//! Evidence quality at compile time (spec 032): the structure of version 2
//! definitions (phase b) and source binding (phase c, category 15). Static
//! grade ceilings and the grade phase (e2) follow in the next change.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::definition::{
    Cond, CorroborateDecl, Definition, Disagreement, EvidenceDecl, Expr, InputDecl, Predicate,
    ProvenanceClass, StepBody, TypeDecl,
};
use rustev_contract::grade::{Grade, PresentWhen};
use rustev_contract::plan::BoundSource;
use rustev_contract::source::{FactGrade, SourceDescriptor, valid_source_name};
use rustev_contract::{Identified, schema};

use crate::compile::{Category, Refusal};
use crate::expr::Ref;
use crate::ops::{FilterWithReasons, OpArgs};

/// Bounds of a presence predicate (spec 032, 3.4.2).
pub const PRESENT_WHEN_MAX_DEPTH: usize = 4;
pub const PRESENT_WHEN_MAX_LEAVES: usize = 32;
/// The largest map a version 2 type may declare (spec 032, 3.4.1).
pub const MAP_MAX_ENTRIES: u64 = 10_000;

fn refusal(category: Category, subject: impl Into<String>, detail: impl Into<String>) -> Refusal {
    Refusal {
        category,
        subject: subject.into(),
        detail: detail.into(),
        shortfalls: vec![],
    }
}

fn invalid(subject: impl Into<String>, detail: impl Into<String>) -> Refusal {
    refusal(Category::InvalidDefinition, subject, detail)
}

/// Non-empty lists, at most 4 deep, at most 32 leaves.
pub(crate) fn present_when_structure(p: &PresentWhen) -> Result<(), String> {
    fn lists(p: &PresentWhen) -> Result<(), String> {
        match p {
            PresentWhen::AnyNonNull(f) | PresentWhen::AllNonNull(f) if f.is_empty() => {
                Err("a presence predicate names no field".into())
            }
            PresentWhen::All(ps) | PresentWhen::Any(ps) if ps.is_empty() => {
                Err("a presence predicate combines nothing".into())
            }
            PresentWhen::All(ps) | PresentWhen::Any(ps) => ps.iter().try_for_each(lists),
            _ => Ok(()),
        }
    }
    lists(p)?;
    let (depth, leaves) = p.shape();
    if depth > PRESENT_WHEN_MAX_DEPTH || leaves > PRESENT_WHEN_MAX_LEAVES {
        return Err(format!(
            "a presence predicate is {depth} deep with {leaves} leaves; at most \
             {PRESENT_WHEN_MAX_DEPTH} and {PRESENT_WHEN_MAX_LEAVES}"
        ));
    }
    Ok(())
}

/// Whether a version 2 definition uses any construct of the #76 track.
pub(crate) fn uses_track_construct(def: &Definition, decoded: &BTreeMap<String, OpArgs>) -> bool {
    fn type_v2(t: &TypeDecl) -> bool {
        match t {
            TypeDecl::Nullable { .. } | TypeDecl::Map { .. } => true,
            TypeDecl::Record {
                present_when: Some(_),
                ..
            } => true,
            TypeDecl::Record { fields, .. } => fields.iter().any(|f| type_v2(&f.ty)),
            TypeDecl::List { item, .. } => type_v2(item),
            _ => false,
        }
    }
    def.inputs
        .iter()
        .any(|i| i.evidence.is_some() || i.on_disagreement.is_some() || type_v2(&i.ty))
        || def.steps.iter().any(|s| match &s.body {
            StepBody::Corroborate(_) => true,
            StepBody::Exact(_) => decoded.get(&s.id).is_some_and(OpArgs::uses_v2),
            StepBody::Semantic(_) => false,
        })
        || policy_conds(def).any(|(_, c)| cond_uses_v2(c))
}

fn cond_uses_v2(c: &Cond) -> bool {
    match c {
        Cond::Exact(p) => crate::expr::predicate_uses_v2(p),
        Cond::All(cs) | Cond::Any(cs) => cs.iter().any(cond_uses_v2),
        Cond::Not(c) => cond_uses_v2(c),
        _ => false,
    }
}

/// Every policy condition with its subject: rules, then adjustments.
pub(crate) fn policy_conds(def: &Definition) -> impl Iterator<Item = (String, &Cond)> {
    def.policy
        .rules
        .iter()
        .map(|r| (format!("policy:rule:{}", r.id), &r.when))
        .chain(
            def.policy
                .adjustments
                .iter()
                .map(|a| (format!("policy:adjustment:{}", a.id), &a.when)),
        )
}

fn path_name_ok(n: &str) -> bool {
    !n.contains('/') && !n.contains('.') && n != "*"
}

/// Version 2 type structure: `nullable` only as a record field type, maps
/// within bounds, presence predicates within bounds and over nullable fields
/// of their record, field names usable in paths.
fn check_type(t: &TypeDecl, subject: &str, field_level: bool) -> Result<(), Refusal> {
    match t {
        TypeDecl::Nullable { ty } => {
            if !field_level {
                return Err(invalid(
                    subject,
                    "nullable is allowed only as a record field type",
                ));
            }
            check_type(ty, subject, false)
        }
        TypeDecl::Map {
            key_max_bytes,
            value,
            max_entries,
        } => {
            if *key_max_bytes == 0 || *max_entries == 0 || *max_entries > MAP_MAX_ENTRIES {
                return Err(invalid(
                    subject,
                    format!("a map has positive key_max_bytes and 1 to {MAP_MAX_ENTRIES} entries"),
                ));
            }
            check_type(value, subject, false)
        }
        TypeDecl::List { item, .. } => check_type(item, subject, false),
        TypeDecl::Record {
            fields,
            present_when,
        } => {
            for f in fields {
                if !path_name_ok(&f.name) {
                    return Err(invalid(
                        subject,
                        format!(
                            "field {:?}: version 2 field names hold no '/', '.' or '*'",
                            f.name
                        ),
                    ));
                }
                check_type(&f.ty, subject, true)?;
            }
            if let Some(p) = present_when {
                present_when_structure(p).map_err(|e| invalid(subject, e))?;
                let mut named = Vec::new();
                p.fields(&mut named);
                for n in named {
                    let nullable = fields
                        .iter()
                        .any(|f| f.name == n && matches!(f.ty, TypeDecl::Nullable { .. }));
                    if !nullable {
                        return Err(refusal(
                            Category::KindMismatch,
                            subject,
                            format!("present_when names {n:?}, which is not a nullable field"),
                        ));
                    }
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_grading(i: &InputDecl) -> Result<(), Refusal> {
    let subject = format!("input:{}", i.name);
    let sources: Option<&Vec<String>> = match &i.evidence {
        Some(EvidenceDecl::Graded { sources, caps }) => {
            let mut sorted = sources.clone();
            sorted.sort();
            sorted.dedup();
            if sources.is_empty() || &sorted != sources {
                return Err(invalid(
                    &subject,
                    "graded sources are non-empty, sorted and duplicate-free",
                ));
            }
            if let Some(s) = sources.iter().find(|s| !valid_source_name(s)) {
                return Err(invalid(&subject, format!("{s:?} is not a source name")));
            }
            if caps.iter().any(|c| parse_path(&c.path).is_none()) {
                return Err(invalid(&subject, "a cap path has an empty segment"));
            }
            Some(sources)
        }
        None => None,
    };
    match (&i.on_disagreement, sources) {
        (Some(_), None) => Err(invalid(
            &subject,
            "disagreement_on_ungraded: only a graded input declares on_disagreement",
        )),
        (Some(Disagreement::Prefer { sources: listed }), Some(named))
        | (Some(Disagreement::TreatAsHint { sources: listed }), Some(named)) => {
            let mut seen = BTreeSet::new();
            for s in listed {
                if !seen.insert(s) {
                    return Err(invalid(
                        &subject,
                        format!("disagreement_sources: {s:?} is listed twice"),
                    ));
                }
                if !named.contains(s) {
                    return Err(invalid(
                        &subject,
                        format!("disagreement_sources: the input does not name {s:?}"),
                    ));
                }
            }
            if let Some(s) = named.iter().find(|s| !seen.contains(s)) {
                return Err(invalid(
                    &subject,
                    format!("disagreement_sources: {s:?} is left out"),
                ));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// A source path: `""` is the whole value; otherwise segments joined by `/`.
pub(crate) fn parse_path(p: &str) -> Option<Vec<String>> {
    if p.is_empty() {
        return Some(vec![]);
    }
    let segs: Vec<String> = p.split('/').map(String::from).collect();
    (!segs.iter().any(String::is_empty)).then_some(segs)
}

fn requirement_mins(e: &Expr, out: &mut Vec<Grade>) {
    match e {
        Expr::Graded { require, .. } => out.push(require.min),
        Expr::Ref(_) | Expr::Lit(_) => {}
        Expr::Add { left, right }
        | Expr::Sub { left, right }
        | Expr::Mul { left, right }
        | Expr::Div { left, right } => {
            requirement_mins(left, out);
            requirement_mins(right, out);
        }
        Expr::Round { value, .. } | Expr::Len(value) | Expr::ToDecimal(value) => {
            requirement_mins(value, out)
        }
        Expr::Fx {
            amount, from, to, ..
        } => {
            requirement_mins(amount, out);
            requirement_mins(from, out);
            requirement_mins(to, out);
        }
    }
}

fn predicate_mins(p: &Predicate, out: &mut Vec<Grade>) {
    match p {
        Predicate::Cmp { left, right, .. } => {
            requirement_mins(left, out);
            requirement_mins(right, out);
        }
        Predicate::All(ps) | Predicate::Any(ps) => ps.iter().for_each(|p| predicate_mins(p, out)),
        Predicate::Not(p) => predicate_mins(p, out),
        Predicate::Member { value, .. } | Predicate::MemberRef { value, .. } => {
            requirement_mins(value, out)
        }
        Predicate::Present(_) => {}
    }
}

fn cond_predicates<'c>(c: &'c Cond, out: &mut Vec<&'c Predicate>) {
    match c {
        Cond::Exact(p) => out.push(p),
        Cond::All(cs) | Cond::Any(cs) => cs.iter().for_each(|c| cond_predicates(c, out)),
        Cond::Not(c) => cond_predicates(c, out),
        _ => {}
    }
}

/// The hard-gate predicates of an exact step (spec 032, 3.9.2).
pub(crate) fn gate_predicates(args: &OpArgs) -> Vec<&Predicate> {
    match args {
        OpArgs::CountWhere(a) => vec![&a.predicate],
        OpArgs::WindowFilter(a) => vec![&a.predicate],
        OpArgs::FilterWithReasons(a) => a.rules.iter().map(|r| &r.predicate).collect(),
        OpArgs::Compare(a) => vec![&a.predicate],
        _ => vec![],
    }
}

/// Phase (b) for a version 2 definition: everything spec 032 adds to the
/// structure checks, in input, step and policy order.
pub(crate) fn structure(
    def: &Definition,
    decoded: &BTreeMap<String, OpArgs>,
) -> Result<(), Refusal> {
    if !uses_track_construct(def, decoded) {
        return Err(invalid(
            "definition",
            format!(
                "use_definition_1: a {} definition uses no construct of the #76 track",
                schema::DEFINITION_V2
            ),
        ));
    }
    for i in &def.inputs {
        let subject = format!("input:{}", i.name);
        check_type(&i.ty, &subject, false)?;
        check_grading(i)?;
    }
    for s in &def.steps {
        let subject = format!("step:{}", s.id);
        if s.id.contains('/') {
            return Err(invalid(&subject, "version 2 step ids hold no '/'"));
        }
        match &s.body {
            StepBody::Exact(_) => {
                let Some(args) = decoded.get(&s.id) else {
                    continue;
                };
                if let OpArgs::FilterWithReasons(FilterWithReasons { rules, .. }) = args {
                    if let Some(r) = rules.iter().find(|r| {
                        r.name == "unmatched"
                            || r.name == "predicate_failed"
                            || r.name.contains(':')
                    }) {
                        return Err(invalid(
                            &subject,
                            format!("reserved_reason: a rule cannot be named {:?}", r.name),
                        ));
                    }
                }
                let mut mins = Vec::new();
                gate_predicates(args)
                    .into_iter()
                    .for_each(|p| predicate_mins(p, &mut mins));
                if let OpArgs::MemberOf(m) = args {
                    requirement_mins(&m.value, &mut mins);
                }
                if mins.contains(&Grade::Hint) {
                    return Err(invalid(
                        &subject,
                        "a hard gate never accepts hint: an explicit min of hint is refused",
                    ));
                }
            }
            StepBody::Corroborate(c) => corroborate_structure(c, &subject)?,
            StepBody::Semantic(_) => {}
        }
    }
    for (subject, c) in policy_conds(def) {
        let mut ps = Vec::new();
        cond_predicates(c, &mut ps);
        let mut mins = Vec::new();
        ps.into_iter().for_each(|p| predicate_mins(p, &mut mins));
        if mins.contains(&Grade::Hint) {
            return Err(invalid(
                subject,
                "a hard gate never accepts hint: an explicit min of hint is refused",
            ));
        }
    }
    Ok(())
}

fn corroborate_structure(c: &CorroborateDecl, subject: &str) -> Result<(), Refusal> {
    if c.upgrade_to <= Grade::Hint {
        return Err(invalid(
            subject,
            "a corroboration upgrades to a grade above hint",
        ));
    }
    if c.by.is_empty() {
        return Err(invalid(subject, "a corroboration has at least one check"));
    }
    match Ref::parse(&c.claim) {
        Ok(Ref::Input(..) | Ref::Step(_)) => {}
        _ => {
            return Err(invalid(
                subject,
                format!("the claim {:?} is not an input path or step", c.claim),
            ));
        }
    }
    for check in &c.by {
        crate::expr::predicate_structure(&check.predicate).map_err(|e| invalid(subject, e))?;
    }
    Ok(())
}

/// A source's or a definition's grade declarations: path and limit.
pub(crate) type Declarations = Vec<(Vec<String>, Grade)>;

/// How one graded input is graded: its bound sources' declarations, its
/// caps and its disagreement rule.
#[derive(Debug, Clone)]
pub(crate) struct Grading {
    /// Per source name, in the input's source order: the fact provenance
    /// and declarations.
    pub sources: Vec<(String, ProvenanceClass, Declarations)>,
    pub caps: Declarations,
    #[allow(dead_code)] // read by plan 3 evaluation, spec 032 increment 3
    pub disagreement: Disagreement,
}

/// Bound sources (spec 032, 3.3.4).
#[derive(Debug, Clone, Default)]
pub(crate) struct Bound {
    pub sources: Vec<BoundSource>,
    pub inputs: BTreeMap<String, Grading>,
}

/// Whether `path` exists in `t`: field names, `*` for list items and map
/// values; `nullable` is transparent.
fn path_exists(t: &TypeDecl, path: &[String]) -> bool {
    let Some((head, rest)) = path.split_first() else {
        return true;
    };
    match t {
        TypeDecl::Nullable { ty } => path_exists(ty, path),
        TypeDecl::Record { fields, .. } => fields
            .iter()
            .any(|f| &f.name == head && path_exists(&f.ty, rest)),
        TypeDecl::List { item, .. } => head == "*" && path_exists(item, rest),
        TypeDecl::Map { value, .. } => head == "*" && path_exists(value, rest),
        _ => false,
    }
}

/// Phase (c): bind every source a graded input names (category 15).
pub(crate) fn bind_sources(
    def: &Definition,
    supplied: &[SourceDescriptor],
) -> Result<Bound, Refusal> {
    let mut by_name: BTreeMap<&str, &SourceDescriptor> = BTreeMap::new();
    for d in supplied {
        if d.schema != schema::SOURCE {
            return Err(refusal(
                Category::Parse,
                format!("source:{}", d.name),
                format!("descriptor schema is not {}", schema::SOURCE),
            ));
        }
        if by_name.insert(d.name.as_str(), d).is_some() {
            return Err(refusal(
                Category::SourceBinding,
                format!("source:{}", d.name),
                "two supplied source descriptors share a name",
            ));
        }
    }
    let mut out = Bound::default();
    let mut bound: BTreeMap<String, BoundSource> = BTreeMap::new();
    for i in &def.inputs {
        let Some(EvidenceDecl::Graded { sources, caps }) = &i.evidence else {
            continue;
        };
        let subject = format!("input:{}", i.name);
        let sb = |detail: String| refusal(Category::SourceBinding, &subject, detail);
        let mut grading = Grading {
            sources: vec![],
            caps: vec![],
            disagreement: i
                .on_disagreement
                .clone()
                .unwrap_or(Disagreement::RequireAgreement),
        };
        for name in sources {
            let d = by_name
                .get(name.as_str())
                .ok_or_else(|| sb(format!("source {name:?} was not supplied")))?;
            let fact = d
                .facts
                .iter()
                .flatten()
                .find(|f| f.name == i.name)
                .ok_or_else(|| sb(format!("source {name:?} has no fact for this input")))?;
            if !i.provenance.contains(&fact.provenance) {
                return Err(sb(format!(
                    "source {name:?} attests provenance {:?}, which the input does not accept",
                    fact.provenance
                )));
            }
            let decls = fact_declarations(&fact.grades, &i.ty).map_err(sb)?;
            grading.sources.push((name.clone(), fact.provenance, decls));
            let id = d
                .id()
                .map_err(|e| refusal(Category::Parse, format!("source:{name}"), e.to_string()))?;
            bound.insert(
                name.clone(),
                BoundSource {
                    name: name.clone(),
                    version: d.version.clone(),
                    id,
                },
            );
        }
        for c in caps {
            // Phase (b) refuses a malformed path first; this keeps a parse
            // failure from reading as the root path if called alone.
            let Some(path) = parse_path(&c.path) else {
                return Err(sb(format!("capped path {:?} has an empty segment", c.path)));
            };
            if !path_exists(&i.ty, &path) {
                return Err(sb(format!(
                    "capped path {:?} does not exist in the input's type",
                    c.path
                )));
            }
            grading.caps.push((path, c.max_grade));
        }
        out.inputs.insert(i.name.clone(), grading);
    }
    out.sources = bound.into_values().collect();
    Ok(out)
}

fn fact_declarations(grades: &[FactGrade], ty: &TypeDecl) -> Result<Declarations, String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for g in grades {
        if !seen.insert(g.path.as_str()) {
            return Err(format!("the fact declares path {:?} twice", g.path));
        }
        let path = parse_path(&g.path)
            .ok_or_else(|| format!("declared path {:?} has an empty segment", g.path))?;
        if !path_exists(ty, &path) {
            return Err(format!(
                "declared path {:?} does not exist in the input's type",
                g.path
            ));
        }
        out.push((path, g.limit()));
    }
    if !seen.contains("") {
        return Err("the fact does not declare the whole value (path \"\")".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::Document;
    use serde_json::{Value, json};

    fn source(provenance: &str) -> SourceDescriptor {
        let v = json!({"schema": "rustev.source/1", "name": "live-quote", "version": "1.0.0",
            "facts": [{"name": "quotes", "provenance": provenance, "grades": [
                {"path": "", "max_grade": "authoritative", "reliability": "reliable"}]}]});
        SourceDescriptor::parse(v.to_string().as_bytes()).unwrap()
    }

    /// A version 2 definition whose one input is a graded map of records
    /// with a presence predicate. SYNTHETIC.
    fn base() -> Value {
        json!({
            "schema": "rustev.definition/2", "name": "eq-unit", "version": "0.1.0",
            "package": "synthetic",
            "inputs": [
                {"name": "quotes", "type": {"map": {"key_max_bytes": 16, "max_entries": 20,
                    "value": {"record": {"fields": [
                        {"name": "price", "type": {"nullable": {"type": "decimal"}}},
                        {"name": "unit_price", "type": {"nullable": {"type": "decimal"}}}],
                        "present_when": {"any_non_null": ["price", "unit_price"]}}}}},
                 "provenance": ["third-party"], "freshness": "not_required",
                 "evidence": {"graded": {"sources": ["live-quote"], "caps": []}}}
            ],
            "steps": [],
            "policy": {"on_unresolved": [], "rules": [
                {"id": "fallback", "when": "always", "then": {"escalate": {"reason": "review"}}}],
                "adjustments": []},
            "limits": {"max_semantic_requests": 0, "on_excess": "refuse",
                "max_projection_bytes": 0, "distribution_tolerance": "0.000001",
                "deadline_ms": 1000}
        })
    }

    fn def(v: &Value) -> Definition {
        Definition::parse(v.to_string().as_bytes()).unwrap()
    }

    fn structure_of(v: &Value) -> Result<(), Refusal> {
        structure(&def(v), &BTreeMap::new())
    }

    fn assert_invalid(r: Result<(), Refusal>, category: Category, detail: &str) {
        let e = r.expect_err("refused");
        assert_eq!(e.category, category, "{}", e.detail);
        assert!(e.detail.contains(detail), "{:?} lacks {detail:?}", e.detail);
    }

    #[test]
    fn version_2_structure() {
        assert!(structure_of(&base()).is_ok());

        let mut plain = base();
        plain["inputs"][0]["evidence"] = Value::Null;
        plain["inputs"][0]["type"] = json!({"text": {"max_bytes": 8}});
        assert_invalid(
            structure_of(&plain),
            Category::InvalidDefinition,
            "use_definition_1",
        );

        let mut top = base();
        top["inputs"][0]["type"] = json!({"nullable": {"type": "decimal"}});
        assert_invalid(
            structure_of(&top),
            Category::InvalidDefinition,
            "only as a record field",
        );

        let mut empty_map = base();
        empty_map["inputs"][0]["type"]["map"]["max_entries"] = json!(0);
        assert_invalid(
            structure_of(&empty_map),
            Category::InvalidDefinition,
            "1 to 10000",
        );

        let mut not_nullable = base();
        not_nullable["inputs"][0]["type"]["map"]["value"]["record"]["fields"][0]["type"] =
            json!("decimal");
        assert_invalid(
            structure_of(&not_nullable),
            Category::KindMismatch,
            "not a nullable field",
        );

        let mut slash = base();
        slash["inputs"][0]["type"]["map"]["value"]["record"]["fields"][1]["name"] =
            json!("unit/price");
        slash["inputs"][0]["type"]["map"]["value"]["record"]["present_when"] =
            json!({"any_non_null": ["price"]});
        assert_invalid(
            structure_of(&slash),
            Category::InvalidDefinition,
            "field names",
        );

        let mut unsorted = base();
        unsorted["inputs"][0]["evidence"]["graded"]["sources"] = json!(["live-quote", "a"]);
        assert_invalid(
            structure_of(&unsorted),
            Category::InvalidDefinition,
            "sorted",
        );

        let mut ungraded = base();
        ungraded["inputs"][0]["evidence"] = Value::Null;
        ungraded["inputs"][0]["on_disagreement"] = json!("abstain");
        assert_invalid(
            structure_of(&ungraded),
            Category::InvalidDefinition,
            "disagreement_on_ungraded",
        );
    }

    #[test]
    fn graded_inputs_bind_their_sources_or_refuse() {
        let ok = bind_sources(&def(&base()), &[source("third-party")]).unwrap();
        assert_eq!(ok.sources.len(), 1);
        assert_eq!(ok.sources[0].name, "live-quote");
        let g = &ok.inputs["quotes"];
        assert_eq!(g.sources[0].2, vec![(vec![], Grade::Authoritative)]);

        let category = |r: Result<Bound, Refusal>| r.expect_err("refused").category;
        assert_eq!(
            category(bind_sources(&def(&base()), &[])),
            Category::SourceBinding,
            "not supplied"
        );
        assert_eq!(
            category(bind_sources(&def(&base()), &[source("system-of-record")])),
            Category::SourceBinding,
            "provenance not accepted"
        );
        assert_eq!(
            category(bind_sources(
                &def(&base()),
                &[source("third-party"), source("third-party")]
            )),
            Category::SourceBinding,
            "two descriptors share a name"
        );
        let mut capped = base();
        capped["inputs"][0]["evidence"]["graded"]["caps"] =
            json!([{"path": "*/price", "max_grade": "cached"}]);
        assert!(bind_sources(&def(&capped), &[source("third-party")]).is_ok());
        capped["inputs"][0]["evidence"]["graded"]["caps"] =
            json!([{"path": "*/total", "max_grade": "cached"}]);
        assert_eq!(
            category(bind_sources(&def(&capped), &[source("third-party")])),
            Category::SourceBinding,
            "capped path does not exist"
        );
        capped["inputs"][0]["evidence"]["graded"]["caps"] =
            json!([{"path": "*//price", "max_grade": "cached"}]);
        assert_eq!(
            category(bind_sources(&def(&capped), &[source("third-party")])),
            Category::SourceBinding,
            "a malformed cap path is not the root"
        );
    }
}
