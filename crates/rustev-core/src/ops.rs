//! The closed exact-operator registry `rustev.exact/1` (spec 002, 3.8).
//!
//! An operator is named with a version. Changing what an operator means is a
//! new version, never an edit (E-06). Each operator's arguments are decoded
//! strictly against its own schema.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{Expr, Literal, Predicate, ReasonKind};
use rustev_contract::judgment::{Excluded, Filtered, RankEntry, Ranking, Shortlist, Unresolved};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::expr::{
    Fault, Ref, Scope, StepTy, TyEnv, eval_expr, eval_predicate, eval_ref, expr_refs,
    expr_structure, predicate_refs, predicate_structure, ref_ty, type_expr, type_predicate,
};
use crate::sem::{SemInstances, SemKind, SemValue};
use crate::value::{Ty, Value};

/// Every registered `(name, version)`.
pub const REGISTRY: &[(&str, u32)] = &[
    ("count_where", 1),
    ("window_filter", 1),
    ("extremum", 1),
    ("timestamp_diff", 1),
    ("compare", 1),
    ("arith", 1),
    ("fx_convert", 1),
    ("member_of", 1),
    ("field_presence", 1),
    ("filter_with_reasons", 1),
    ("top_k", 1),
    ("weighted_rank", 1),
];

pub fn is_registered(op: &str, version: u32) -> bool {
    REGISTRY.iter().any(|(n, v)| *n == op && *v == version)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CountWhere {
    pub list: String,
    pub predicate: Predicate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowFilter {
    pub list: String,
    pub field: String,
    pub within_ms: u64,
    pub predicate: Predicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Which {
    Min,
    Max,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extremum {
    pub list: String,
    pub field: String,
    pub which: Which,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimestampDiff {
    pub later: Expr,
    pub earlier: Expr,
    pub unit_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compare {
    pub predicate: Predicate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arith {
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FxConvert {
    pub amount: Expr,
    pub from: Expr,
    pub to: Expr,
    pub rates: String,
    pub scale: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberOf {
    pub value: Expr,
    pub set: Vec<Literal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldPresence {
    /// `input:<name>` references, in the order they are reported.
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedRule {
    pub name: String,
    pub predicate: Predicate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterWithReasons {
    pub list: String,
    pub id_field: String,
    pub rules: Vec<NamedRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopK {
    pub list: String,
    pub id_field: String,
    /// `step:<id>` of a filter result; only its eligible ids are ranked.
    pub among: String,
    pub key: Expr,
    pub direction: Direction,
    pub k: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightTable {
    /// `input:<name>`: a list of records.
    pub list: String,
    pub key_field: String,
    pub class_field: String,
    pub table: Vec<ClassWeight>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassWeight {
    pub class: String,
    pub weight: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    /// Weighted mean of a two-index proposition's `true` mass over the other
    /// index, weights by class; zero items give 0.
    PropositionMean {
        step: String,
        candidate_bind: String,
        weights: WeightTable,
    },
    /// A one-index rubric's expectation divided by its highest level index.
    RubricExpectation { step: String },
    /// `1 - i / (n - 1)` over the candidate order; 1 when `n = 1`.
    ShortlistPosition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub name: String,
    pub weight: Decimal,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedRank {
    /// `step:<id>` of a shortlist or filter result, in the order ranked.
    pub candidates: String,
    pub components: Vec<Component>,
}

/// Decoded operator arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum OpArgs {
    CountWhere(CountWhere),
    WindowFilter(WindowFilter),
    Extremum(Extremum),
    TimestampDiff(TimestampDiff),
    Compare(Compare),
    Arith(Arith),
    FxConvert(FxConvert),
    MemberOf(MemberOf),
    FieldPresence(FieldPresence),
    FilterWithReasons(FilterWithReasons),
    TopK(TopK),
    WeightedRank(WeightedRank),
}

fn de<T: DeserializeOwned>(args: &Json) -> Result<T, String> {
    serde_json::from_value(args.clone()).map_err(|e| e.to_string())
}

impl OpArgs {
    /// Registry name and version.
    pub fn op(&self) -> (&'static str, u32) {
        match self {
            OpArgs::CountWhere(_) => ("count_where", 1),
            OpArgs::WindowFilter(_) => ("window_filter", 1),
            OpArgs::Extremum(_) => ("extremum", 1),
            OpArgs::TimestampDiff(_) => ("timestamp_diff", 1),
            OpArgs::Compare(_) => ("compare", 1),
            OpArgs::Arith(_) => ("arith", 1),
            OpArgs::FxConvert(_) => ("fx_convert", 1),
            OpArgs::MemberOf(_) => ("member_of", 1),
            OpArgs::FieldPresence(_) => ("field_presence", 1),
            OpArgs::FilterWithReasons(_) => ("filter_with_reasons", 1),
            OpArgs::TopK(_) => ("top_k", 1),
            OpArgs::WeightedRank(_) => ("weighted_rank", 1),
        }
    }

    /// The arguments as the definition document carries them.
    pub fn to_json(&self) -> Json {
        let v = match self {
            OpArgs::CountWhere(a) => serde_json::to_value(a),
            OpArgs::WindowFilter(a) => serde_json::to_value(a),
            OpArgs::Extremum(a) => serde_json::to_value(a),
            OpArgs::TimestampDiff(a) => serde_json::to_value(a),
            OpArgs::Compare(a) => serde_json::to_value(a),
            OpArgs::Arith(a) => serde_json::to_value(a),
            OpArgs::FxConvert(a) => serde_json::to_value(a),
            OpArgs::MemberOf(a) => serde_json::to_value(a),
            OpArgs::FieldPresence(a) => serde_json::to_value(a),
            OpArgs::FilterWithReasons(a) => serde_json::to_value(a),
            OpArgs::TopK(a) => serde_json::to_value(a),
            OpArgs::WeightedRank(a) => serde_json::to_value(a),
        };
        // Every argument type serializes to a JSON object.
        v.unwrap_or(Json::Null)
    }
}

/// Decode the arguments of a registered operator; `None` when `(op,
/// version)` is not registered (refusal category 2, not a parse failure).
pub fn decode(op: &str, version: u32, args: &Json) -> Option<Result<OpArgs, String>> {
    if !is_registered(op, version) {
        return None;
    }
    Some(match op {
        "count_where" => de(args).map(OpArgs::CountWhere),
        "window_filter" => de(args).map(OpArgs::WindowFilter),
        "extremum" => de(args).map(OpArgs::Extremum),
        "timestamp_diff" => de(args).map(OpArgs::TimestampDiff),
        "compare" => de(args).map(OpArgs::Compare),
        "arith" => de(args).map(OpArgs::Arith),
        "fx_convert" => de(args).map(OpArgs::FxConvert),
        "member_of" => de(args).map(OpArgs::MemberOf),
        "field_presence" => de(args).map(OpArgs::FieldPresence),
        "filter_with_reasons" => de(args).map(OpArgs::FilterWithReasons),
        "top_k" => de(args).map(OpArgs::TopK),
        _ => de(args).map(OpArgs::WeightedRank),
    })
}

fn has_fx_expr(e: &Expr) -> bool {
    match e {
        Expr::Fx { .. } => true,
        Expr::Ref(_) | Expr::Lit(_) => false,
        Expr::Add { left, right }
        | Expr::Sub { left, right }
        | Expr::Mul { left, right }
        | Expr::Div { left, right } => has_fx_expr(left) || has_fx_expr(right),
        Expr::Round { value, .. } | Expr::Len(value) | Expr::ToDecimal(value) => has_fx_expr(value),
    }
}

fn has_fx_pred(p: &Predicate) -> bool {
    match p {
        Predicate::Cmp { left, right, .. } => has_fx_expr(left) || has_fx_expr(right),
        Predicate::All(ps) | Predicate::Any(ps) => ps.iter().any(has_fx_pred),
        Predicate::Not(p) => has_fx_pred(p),
        Predicate::Member { value, .. } => has_fx_expr(value),
        Predicate::Present(_) => false,
    }
}

/// Keep only `input:` and `step:` references (item references and `now` are
/// not dependencies).
fn deps_only(refs: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    refs.into_iter()
        .filter(|r| r.starts_with("input:") || r.starts_with("step:"))
        .map(|r| crate::expr::base_ref(&r))
        .filter(|r| seen.insert(r.clone()))
        .collect()
}

impl OpArgs {
    /// References this step reads, in argument order (spec 002, 3.10.4).
    pub fn refs(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self {
            OpArgs::CountWhere(a) => {
                out.push(a.list.clone());
                predicate_refs(&a.predicate, &mut out);
            }
            OpArgs::WindowFilter(a) => {
                out.push(a.list.clone());
                predicate_refs(&a.predicate, &mut out);
            }
            OpArgs::Extremum(a) => out.push(a.list.clone()),
            OpArgs::TimestampDiff(a) => {
                expr_refs(&a.later, &mut out);
                expr_refs(&a.earlier, &mut out);
            }
            OpArgs::Compare(a) => predicate_refs(&a.predicate, &mut out),
            OpArgs::Arith(a) => expr_refs(&a.expr, &mut out),
            OpArgs::FxConvert(a) => {
                expr_refs(&a.amount, &mut out);
                expr_refs(&a.from, &mut out);
                expr_refs(&a.to, &mut out);
                out.push(a.rates.clone());
            }
            OpArgs::MemberOf(a) => expr_refs(&a.value, &mut out),
            OpArgs::FieldPresence(a) => out.extend(a.inputs.iter().cloned()),
            OpArgs::FilterWithReasons(a) => {
                out.push(a.list.clone());
                a.rules
                    .iter()
                    .for_each(|r| predicate_refs(&r.predicate, &mut out));
            }
            OpArgs::TopK(a) => {
                out.push(a.list.clone());
                out.push(a.among.clone());
                expr_refs(&a.key, &mut out);
            }
            OpArgs::WeightedRank(a) => {
                out.push(a.candidates.clone());
                for c in &a.components {
                    match &c.source {
                        Source::PropositionMean { step, weights, .. } => {
                            out.push(step.clone());
                            out.push(weights.list.clone());
                        }
                        Source::RubricExpectation { step } => out.push(step.clone()),
                        Source::ShortlistPosition => {}
                    }
                }
            }
        }
        deps_only(out)
    }

    /// Every raw reference string, including item references and `now`,
    /// for the structure phase.
    pub fn all_ref_strings(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self {
            OpArgs::CountWhere(a) => {
                out.push(a.list.clone());
                predicate_refs(&a.predicate, &mut out);
            }
            OpArgs::WindowFilter(a) => {
                out.push(a.list.clone());
                predicate_refs(&a.predicate, &mut out);
            }
            OpArgs::FilterWithReasons(a) => {
                out.push(a.list.clone());
                a.rules
                    .iter()
                    .for_each(|r| predicate_refs(&r.predicate, &mut out));
            }
            OpArgs::TopK(a) => {
                out.push(a.list.clone());
                out.push(a.among.clone());
                expr_refs(&a.key, &mut out);
            }
            _ => out = self.refs(),
        }
        out
    }

    /// Structural checks (compile phase b).
    pub fn structure(&self) -> Result<(), String> {
        match self {
            OpArgs::CountWhere(a) => predicate_structure(&a.predicate),
            OpArgs::WindowFilter(a) => predicate_structure(&a.predicate),
            OpArgs::Extremum(_) => Ok(()),
            OpArgs::TimestampDiff(a) => {
                if a.unit_ms == 0 {
                    return Err("unit_ms must be positive".into());
                }
                expr_structure(&a.later)?;
                expr_structure(&a.earlier)
            }
            OpArgs::Compare(a) => predicate_structure(&a.predicate),
            OpArgs::Arith(a) => expr_structure(&a.expr),
            OpArgs::FxConvert(a) => {
                if a.scale > 9 {
                    return Err(format!("scale {} exceeds 9", a.scale));
                }
                expr_structure(&a.amount)
            }
            OpArgs::MemberOf(a) => {
                if a.set.is_empty() {
                    return Err("member set is empty".into());
                }
                expr_structure(&a.value)
            }
            OpArgs::FieldPresence(a) => {
                if a.inputs.is_empty() {
                    return Err("field_presence names no input".into());
                }
                a.inputs.iter().try_for_each(|r| match Ref::parse(r)? {
                    Ref::Input(_, p) if p.is_empty() => Ok(()),
                    _ => Err(format!("field_presence takes inputs, not {r:?}")),
                })
            }
            OpArgs::FilterWithReasons(a) => {
                let mut names = BTreeSet::new();
                for r in &a.rules {
                    if !names.insert(&r.name) {
                        return Err(format!("duplicate rule name {:?}", r.name));
                    }
                    predicate_structure(&r.predicate)?;
                }
                Ok(())
            }
            OpArgs::TopK(a) => {
                if a.k == 0 {
                    return Err("k must be positive".into());
                }
                expr_structure(&a.key)
            }
            OpArgs::WeightedRank(a) => {
                let mut names = BTreeSet::new();
                for c in &a.components {
                    if !names.insert(&c.name) {
                        return Err(format!("duplicate component {:?}", c.name));
                    }
                    if c.weight < Decimal::ZERO {
                        return Err(format!("negative weight for {:?}", c.name));
                    }
                }
                if a.components.is_empty() {
                    return Err("weighted_rank has no component".into());
                }
                Ok(())
            }
        }
    }

    /// Reasons this operator itself can produce, beyond its dependencies'.
    pub fn own_reasons(&self) -> BTreeSet<ReasonKind> {
        let mut r = BTreeSet::new();
        let fx = match self {
            OpArgs::CountWhere(a) => has_fx_pred(&a.predicate),
            OpArgs::WindowFilter(a) => has_fx_pred(&a.predicate),
            OpArgs::Compare(a) => has_fx_pred(&a.predicate),
            OpArgs::Arith(a) => has_fx_expr(&a.expr),
            OpArgs::MemberOf(a) => has_fx_expr(&a.value),
            OpArgs::FxConvert(_) => true,
            _ => false,
        };
        match self {
            OpArgs::Extremum(_) | OpArgs::FieldPresence(_) | OpArgs::WeightedRank(_) => {}
            _ => {
                r.insert(ReasonKind::InvalidInput);
            }
        }
        if fx {
            r.insert(ReasonKind::MissingEvidence);
            r.insert(ReasonKind::Conflict);
        }
        r
    }

    /// The output type, or a kind mismatch (compile phase e).
    pub fn type_check(&self, env: &TyEnv<'_>) -> Result<Ty, String> {
        match self {
            OpArgs::CountWhere(a) => {
                let item = list_item(&a.list, env)?;
                type_predicate(&a.predicate, &with_item(env, &item))?;
                Ok(Ty::Integer)
            }
            OpArgs::WindowFilter(a) => {
                let (item, max) = list_item_max(&a.list, env)?;
                match record_field(&item, &a.field)? {
                    Ty::Timestamp => {}
                    t => {
                        return Err(format!(
                            "window field {:?} is {}, not a timestamp",
                            a.field,
                            t.name()
                        ));
                    }
                }
                type_predicate(&a.predicate, &with_item(env, &item))?;
                Ok(Ty::List {
                    item: Box::new(item),
                    max,
                })
            }
            OpArgs::Extremum(a) => {
                let item = list_item(&a.list, env)?;
                let t = record_field(&item, &a.field)?;
                if !t.is_orderable() {
                    return Err(format!("{} has no order", t.name()));
                }
                Ok(Ty::Maybe(Box::new(t)))
            }
            OpArgs::TimestampDiff(a) => {
                let mut maybe = false;
                for e in [&a.later, &a.earlier] {
                    match type_expr(e, env)? {
                        Ty::Timestamp => {}
                        Ty::Maybe(t) if *t == Ty::Timestamp => maybe = true,
                        t => return Err(format!("timestamp_diff of {}", t.name())),
                    }
                }
                Ok(if maybe {
                    Ty::Maybe(Box::new(Ty::Integer))
                } else {
                    Ty::Integer
                })
            }
            OpArgs::Compare(a) => type_predicate(&a.predicate, env).map(|_| Ty::Bool),
            OpArgs::Arith(a) => match type_expr(&a.expr, env)? {
                t @ (Ty::Integer | Ty::Decimal) => Ok(t),
                t => Err(format!("arith yields {}, not a number", t.name())),
            },
            OpArgs::FxConvert(a) => {
                let e = Expr::Fx {
                    amount: Box::new(a.amount.clone()),
                    from: Box::new(a.from.clone()),
                    to: Box::new(a.to.clone()),
                    rates: a.rates.clone(),
                    scale: a.scale,
                };
                type_expr(&e, env)
            }
            OpArgs::MemberOf(a) => {
                let p = Predicate::Member {
                    value: a.value.clone(),
                    set: a.set.clone(),
                };
                type_predicate(&p, env).map(|_| Ty::Bool)
            }
            OpArgs::FieldPresence(a) => {
                let mut longest = 0u64;
                for r in &a.inputs {
                    let name = r.trim_start_matches("input:");
                    if !env.inputs.contains_key(name) {
                        return Err(format!("unknown input {name:?}"));
                    }
                    longest = longest.max(name.len() as u64);
                }
                Ok(Ty::List {
                    item: Box::new(Ty::Text { max_bytes: longest }),
                    max: a.inputs.len() as u64,
                })
            }
            OpArgs::FilterWithReasons(a) => {
                let (item, max) = list_item_max(&a.list, env)?;
                let id_bytes = id_field_bytes(&item, &a.id_field)?;
                let ienv = with_item(env, &item);
                for r in &a.rules {
                    type_predicate(&r.predicate, &ienv)?;
                }
                Ok(Ty::Filtered { max, id_bytes })
            }
            OpArgs::TopK(a) => {
                let (item, _) = list_item_max(&a.list, env)?;
                let id_bytes = id_field_bytes(&item, &a.id_field)?;
                let among_max = match ref_ty(&a.among, env)? {
                    Ty::Filtered { max, .. } => max,
                    t => return Err(format!("top_k among {}, not a filter result", t.name())),
                };
                let kt = type_expr(&a.key, &with_item(env, &item))?;
                if !kt.is_orderable() {
                    return Err(format!("top_k key is {}, which has no order", kt.name()));
                }
                Ok(Ty::Shortlist {
                    max: a.k.min(among_max),
                    id_bytes,
                })
            }
            OpArgs::WeightedRank(a) => {
                let max = match ref_ty(&a.candidates, env)? {
                    Ty::Shortlist { max, .. } | Ty::Filtered { max, .. } => max,
                    t => {
                        return Err(format!(
                            "weighted_rank over {}, not a shortlist or filter",
                            t.name()
                        ));
                    }
                };
                for c in &a.components {
                    match &c.source {
                        Source::PropositionMean {
                            step,
                            candidate_bind,
                            weights,
                        } => {
                            match semantic(step, env)? {
                                (k, binds)
                                    if k.has_mass()
                                        && binds.len() == 2
                                        && binds.contains(candidate_bind) => {}
                                (k, binds) => {
                                    return Err(format!(
                                        "{step} is {} over {} index(es); proposition_mean needs mass over two, one bound as {candidate_bind:?}",
                                        k.name(),
                                        binds.len()
                                    ));
                                }
                            }
                            let item = list_item(&weights.list, env)?;
                            id_field_bytes(&item, &weights.key_field)?;
                            id_field_bytes(&item, &weights.class_field)?;
                        }
                        Source::RubricExpectation { step } => match semantic(step, env)? {
                            (SemKind::Ordinal(_), binds) if binds.len() == 1 => {}
                            (k, _) => {
                                return Err(format!(
                                    "{step} is {}; rubric_expectation needs an ordinal level over one index",
                                    k.name()
                                ));
                            }
                        },
                        Source::ShortlistPosition => {}
                    }
                }
                Ok(Ty::Ranking { max })
            }
        }
    }
}

fn semantic<'e>(r: &str, env: &'e TyEnv<'_>) -> Result<(SemKind, &'e Vec<String>), String> {
    match Ref::parse(r)? {
        Ref::Step(s) => match env.steps.get(&s) {
            Some(StepTy::Semantic(k, binds)) => Ok((*k, binds)),
            Some(StepTy::Exact(t)) => Err(format!("{r} is exact ({}), not semantic", t.name())),
            None => Err(format!("unknown step {s:?}")),
        },
        _ => Err(format!("{r} is not a step")),
    }
}

fn with_item<'a>(env: &TyEnv<'a>, item: &'a Ty) -> TyEnv<'a> {
    TyEnv {
        inputs: env.inputs,
        steps: env.steps,
        item: Some(item),
    }
}

fn list_item_max(r: &str, env: &TyEnv<'_>) -> Result<(Ty, u64), String> {
    match ref_ty(r, env)? {
        Ty::List { item, max } => Ok((*item, max)),
        t => Err(format!("{r} is {}, not a list", t.name())),
    }
}

fn list_item(r: &str, env: &TyEnv<'_>) -> Result<Ty, String> {
    list_item_max(r, env).map(|(t, _)| t)
}

fn record_field(item: &Ty, name: &str) -> Result<Ty, String> {
    match item {
        Ty::Record(fields) => fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| format!("no field {name:?}")),
        t => Err(format!("{} has no field {name:?}", t.name())),
    }
}

fn id_field_bytes(item: &Ty, name: &str) -> Result<u64, String> {
    match record_field(item, name)? {
        Ty::Text { max_bytes } => Ok(max_bytes),
        Ty::Enum(o) => Ok(o.iter().map(|s| s.len() as u64).max().unwrap_or(0)),
        t => Err(format!("id field {name:?} is {}, not text", t.name())),
    }
}

/// What an operator needs at evaluation time besides expression scope.
pub struct OpContext<'a> {
    pub scope: Scope<'a>,
    pub semantic: &'a BTreeMap<String, SemInstances>,
    /// Status of every declared input.
    pub inputs_status: &'a BTreeMap<String, Result<(), Unresolved>>,
    /// Input fields in this step's lineage, named by `invalid_input`.
    pub lineage_inputs: &'a [String],
    /// Each semantic step's fan-out binds, in instance-key order.
    pub binds: &'a BTreeMap<String, Vec<String>>,
}

fn fault(f: Fault, cx: &OpContext<'_>) -> Unresolved {
    match f {
        Fault::Invalid(d) => Unresolved::invalid(cx.lineage_inputs.iter().cloned(), d),
        Fault::Missing(m) => Unresolved::missing([m]),
        Fault::Conflict(c) => Unresolved::conflict([c]),
    }
}

fn items(r: &str, cx: &OpContext<'_>) -> Result<Vec<Value>, Unresolved> {
    match eval_ref(r, &cx.scope) {
        Ok(Value::List(v)) => Ok(v),
        Ok(_) => Err(Unresolved::invalid(
            cx.lineage_inputs.iter().cloned(),
            format!("{r} is not a list"),
        )),
        Err(f) => Err(fault(f, cx)),
    }
}

fn item_scope<'a>(cx: &'a OpContext<'a>, item: &'a Value) -> Scope<'a> {
    Scope {
        now: cx.scope.now,
        inputs: cx.scope.inputs,
        steps: cx.scope.steps,
        present: cx.scope.present,
        item: Some(item),
    }
}

fn get_field<'v>(item: &'v Value, name: &str) -> Option<&'v Value> {
    match item {
        Value::Record(m) => m.get(name),
        _ => None,
    }
}

fn id_of(item: &Value, id_field: &str) -> Option<String> {
    match get_field(item, id_field) {
        Some(Value::Text(s) | Value::Enum(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Items with unique ids, or `invalid_input` on a duplicate or missing id.
fn keyed<'v>(
    list: &'v [Value],
    id_field: &str,
    cx: &OpContext<'_>,
) -> Result<Vec<(String, &'v Value)>, Unresolved> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(list.len());
    for it in list {
        let id = id_of(it, id_field).ok_or_else(|| {
            Unresolved::invalid(
                cx.lineage_inputs.iter().cloned(),
                format!("item without {id_field:?}"),
            )
        })?;
        if !seen.insert(id.clone()) {
            return Err(Unresolved::invalid(
                cx.lineage_inputs.iter().cloned(),
                format!("duplicate id {id:?}"),
            ));
        }
        out.push((id, it));
    }
    Ok(out)
}

fn order_key(v: &Value) -> Option<(u8, i128)> {
    match v {
        Value::Integer(i) => Some((0, i128::from(*i))),
        Value::Decimal(d) => Some((1, d.units())),
        Value::Timestamp(t) => Some((2, i128::from(t.as_ms()))),
        Value::Duration(d) => Some((3, i128::from(d.0))),
        _ => None,
    }
}

impl OpArgs {
    /// Evaluate with every dependency resolved (the evaluator checks that
    /// first, except for `field_presence`, which reads statuses).
    pub fn eval(&self, cx: &OpContext<'_>) -> Result<Value, Unresolved> {
        let f = |e: Fault| fault(e, cx);
        match self {
            OpArgs::CountWhere(a) => {
                let list = items(&a.list, cx)?;
                let mut n: i64 = 0;
                for it in &list {
                    if eval_predicate(&a.predicate, &item_scope(cx, it)).map_err(f)? {
                        n += 1;
                    }
                }
                Ok(Value::Integer(n))
            }
            OpArgs::WindowFilter(a) => {
                let list = items(&a.list, cx)?;
                let now = cx.scope.now;
                let floor = now
                    .as_ms()
                    .saturating_sub(i64::try_from(a.within_ms).unwrap_or(i64::MAX));
                let mut out = Vec::new();
                for it in list {
                    let Some(Value::Timestamp(t)) = get_field(&it, &a.field) else {
                        return Err(f(Fault::Invalid(format!(
                            "item without timestamp {:?}",
                            a.field
                        ))));
                    };
                    if *t > now {
                        return Err(f(Fault::Invalid(format!(
                            "{:?} is after the evaluation time",
                            a.field
                        ))));
                    }
                    if t.as_ms() >= floor
                        && eval_predicate(&a.predicate, &item_scope(cx, &it)).map_err(f)?
                    {
                        out.push(it);
                    }
                }
                Ok(Value::List(out))
            }
            OpArgs::Extremum(a) => {
                let list = items(&a.list, cx)?;
                let mut best: Option<&Value> = None;
                for it in &list {
                    let Some(v) = get_field(it, &a.field) else {
                        continue;
                    };
                    let better = match best {
                        None => true,
                        Some(b) => match a.which {
                            Which::Min => order_key(v) < order_key(b),
                            Which::Max => order_key(v) > order_key(b),
                        },
                    };
                    if better {
                        best = Some(v);
                    }
                }
                Ok(Value::Maybe(best.cloned().map(Box::new)))
            }
            OpArgs::TimestampDiff(a) => {
                let later = eval_expr(&a.later, &cx.scope).map_err(f)?;
                let earlier = eval_expr(&a.earlier, &cx.scope).map_err(f)?;
                // The result is `maybe` exactly when an operand's type is.
                let is_maybe =
                    matches!(later, Value::Maybe(_)) || matches!(earlier, Value::Maybe(_));
                let unwrap = |v: Value| match v {
                    Value::Timestamp(t) => Ok(Some(t)),
                    Value::Maybe(None) => Ok(None),
                    Value::Maybe(Some(b)) => match *b {
                        Value::Timestamp(t) => Ok(Some(t)),
                        _ => Err(Fault::Invalid("timestamp_diff of a non-timestamp".into())),
                    },
                    _ => Err(Fault::Invalid("timestamp_diff of a non-timestamp".into())),
                };
                match (unwrap(later).map_err(f)?, unwrap(earlier).map_err(f)?) {
                    (Some(l), Some(e)) => {
                        let d = l.since(e).map_err(|_| {
                            f(Fault::Invalid("negative timestamp difference".into()))
                        })?;
                        let q = i64::try_from(d.0 / a.unit_ms)
                            .map_err(|_| f(Fault::Invalid("overflow".into())))?;
                        Ok(if is_maybe {
                            Value::Maybe(Some(Box::new(Value::Integer(q))))
                        } else {
                            Value::Integer(q)
                        })
                    }
                    _ => Ok(Value::Maybe(None)),
                }
            }
            OpArgs::Compare(a) => eval_predicate(&a.predicate, &cx.scope)
                .map(Value::Bool)
                .map_err(f),
            OpArgs::Arith(a) => eval_expr(&a.expr, &cx.scope).map_err(f),
            OpArgs::FxConvert(a) => {
                let e = Expr::Fx {
                    amount: Box::new(a.amount.clone()),
                    from: Box::new(a.from.clone()),
                    to: Box::new(a.to.clone()),
                    rates: a.rates.clone(),
                    scale: a.scale,
                };
                eval_expr(&e, &cx.scope).map_err(f)
            }
            OpArgs::MemberOf(a) => {
                let v = eval_expr(&a.value, &cx.scope).map_err(f)?;
                Ok(Value::Bool(
                    a.set.iter().any(|l| Value::from_literal(l) == v),
                ))
            }
            OpArgs::FieldPresence(a) => {
                let mut missing = Vec::new();
                for r in &a.inputs {
                    let name = r.trim_start_matches("input:");
                    match cx.inputs_status.get(name) {
                        Some(Ok(())) => {}
                        Some(Err(Unresolved::MissingEvidence { .. })) | None => {
                            missing.push(Value::Text(name.to_string()))
                        }
                        Some(Err(other)) => return Err(other.clone()),
                    }
                }
                Ok(Value::List(missing))
            }
            OpArgs::FilterWithReasons(a) => {
                let list = items(&a.list, cx)?;
                let keyed = keyed(&list, &a.id_field, cx)?;
                let mut eligible = Vec::new();
                let mut excluded = Vec::new();
                let mut counts: BTreeMap<String, u64> = BTreeMap::new();
                for (id, it) in keyed {
                    let scope = item_scope(cx, it);
                    let mut reasons = Vec::new();
                    for r in &a.rules {
                        match eval_predicate(&r.predicate, &scope) {
                            Ok(true) => {}
                            Ok(false) => reasons.push(r.name.clone()),
                            Err(_) => reasons.push(format!("unevaluable:{}", r.name)),
                        }
                    }
                    if reasons.is_empty() {
                        eligible.push(id);
                    } else {
                        for r in &reasons {
                            *counts.entry(r.clone()).or_default() += 1;
                        }
                        excluded.push(Excluded { id, reasons });
                    }
                }
                Ok(Value::Filtered(Filtered {
                    eligible,
                    excluded,
                    counts,
                }))
            }
            OpArgs::TopK(a) => {
                let list = items(&a.list, cx)?;
                let keyed = keyed(&list, &a.id_field, cx)?;
                let among: BTreeSet<String> = match eval_ref(&a.among, &cx.scope).map_err(f)? {
                    Value::Filtered(fl) => fl.eligible.into_iter().collect(),
                    _ => return Err(f(Fault::Invalid("top_k among a non-filter".into()))),
                };
                let mut scored = Vec::new();
                let mut excluded = Vec::new();
                for (id, it) in keyed.into_iter().filter(|(id, _)| among.contains(id)) {
                    match eval_expr(&a.key, &item_scope(cx, it))
                        .ok()
                        .as_ref()
                        .and_then(order_key)
                    {
                        Some(k) => scored.push((k, id)),
                        None => excluded.push(Excluded {
                            id,
                            reasons: vec!["unevaluable:key".into()],
                        }),
                    }
                }
                scored.sort_by(|(ka, ia), (kb, ib)| {
                    let by_key = match a.direction {
                        Direction::Ascending => ka.cmp(kb),
                        Direction::Descending => kb.cmp(ka),
                    };
                    by_key.then_with(|| ia.as_bytes().cmp(ib.as_bytes()))
                });
                let k = usize::try_from(a.k).unwrap_or(usize::MAX);
                let truncated = scored.len().saturating_sub(k) as u64;
                let ids = scored.into_iter().take(k).map(|(_, id)| id).collect();
                Ok(Value::Shortlist(Shortlist {
                    ids,
                    truncated,
                    excluded,
                }))
            }
            OpArgs::WeightedRank(a) => weighted_rank(a, cx),
        }
    }
}

fn weights_by_key(
    w: &WeightTable,
    cx: &OpContext<'_>,
) -> Result<BTreeMap<String, Option<f64>>, Unresolved> {
    let list = items(&w.list, cx)?;
    let table: BTreeMap<&str, f64> = w
        .table
        .iter()
        .map(|c| (c.class.as_str(), c.weight.to_f64_nearest()))
        .collect();
    let mut out = BTreeMap::new();
    for it in &list {
        if let (Some(k), Some(c)) = (id_of(it, &w.key_field), id_of(it, &w.class_field)) {
            out.insert(k, table.get(c.as_str()).copied());
        }
    }
    Ok(out)
}

fn weighted_rank(a: &WeightedRank, cx: &OpContext<'_>) -> Result<Value, Unresolved> {
    let candidates: Vec<String> =
        match eval_ref(&a.candidates, &cx.scope).map_err(|e| fault(e, cx))? {
            v @ (Value::Shortlist(_) | Value::Filtered(_)) => v.ids().unwrap_or_default(),
            _ => {
                return Err(fault(
                    Fault::Invalid("weighted_rank over a non-shortlist".into()),
                    cx,
                ));
            }
        };
    let n = candidates.len();
    let mut entries: Vec<RankEntry> = Vec::new();
    let mut excluded: Vec<Excluded> = Vec::new();
    'candidate: for (i, c) in candidates.iter().enumerate() {
        let mut components = BTreeMap::new();
        let mut score = 0.0f64;
        for comp in &a.components {
            let value = match &comp.source {
                Source::ShortlistPosition => {
                    if n <= 1 {
                        1.0
                    } else {
                        1.0 - (i as f64) / ((n - 1) as f64)
                    }
                }
                Source::RubricExpectation { step } => {
                    let inst = semantic_of(step, cx)?;
                    match inst.get(std::slice::from_ref(c)) {
                        Some(Ok(SemValue::Ordinal(o))) => {
                            let top = (o.levels().len().saturating_sub(1)).max(1) as f64;
                            o.expectation() / top
                        }
                        Some(Err(u)) => {
                            excluded.push(Excluded {
                                id: c.clone(),
                                reasons: vec![format!("{}:{u}", comp.name)],
                            });
                            continue 'candidate;
                        }
                        _ => {
                            excluded.push(Excluded {
                                id: c.clone(),
                                reasons: vec![format!("{}:no value", comp.name)],
                            });
                            continue 'candidate;
                        }
                    }
                }
                Source::PropositionMean {
                    step,
                    candidate_bind,
                    weights,
                } => {
                    let inst = semantic_of(step, cx)?;
                    let pos = cx
                        .binds
                        .get(step_name(step))
                        .and_then(|b| b.iter().position(|b| b == candidate_bind))
                        .unwrap_or(0);
                    let other = 1 - pos.min(1);
                    let wmap = weights_by_key(weights, cx)?;
                    let (mut num, mut den) = (0.0f64, 0.0f64);
                    for (key, v) in inst.iter().filter(|(k, _)| k.get(pos) == Some(c)) {
                        let claim = key.get(other).cloned().unwrap_or_default();
                        let w = match wmap.get(&claim) {
                            Some(Some(w)) => *w,
                            _ => {
                                excluded.push(Excluded {
                                    id: c.clone(),
                                    reasons: vec![format!("{}:unweighted {claim}", comp.name)],
                                });
                                continue 'candidate;
                            }
                        };
                        match v {
                            Ok(sv) => match sv.mass("true") {
                                Some(p) => {
                                    num += w * p;
                                    den += w;
                                }
                                None => {
                                    excluded.push(Excluded {
                                        id: c.clone(),
                                        reasons: vec![format!("{}:no mass", comp.name)],
                                    });
                                    continue 'candidate;
                                }
                            },
                            Err(u) => {
                                excluded.push(Excluded {
                                    id: c.clone(),
                                    reasons: vec![format!("{}:{u}", comp.name)],
                                });
                                continue 'candidate;
                            }
                        }
                    }
                    if den > 0.0 { num / den } else { 0.0 }
                }
            };
            components.insert(comp.name.clone(), value);
            score += comp.weight.to_f64_nearest() * value;
        }
        entries.push(RankEntry {
            candidate: c.clone(),
            position: 0,
            score,
            components,
        });
    }
    entries.sort_by(|x, y| {
        y.score
            .total_cmp(&x.score)
            .then_with(|| x.candidate.as_bytes().cmp(y.candidate.as_bytes()))
    });
    for (i, e) in entries.iter_mut().enumerate() {
        e.position = i as u64 + 1;
    }
    Ok(Value::Ranking(Ranking { entries, excluded }))
}

fn step_name(r: &str) -> &str {
    r.strip_prefix("step:").unwrap_or(r)
}

fn semantic_of<'c>(r: &str, cx: &'c OpContext<'_>) -> Result<&'c SemInstances, Unresolved> {
    cx.semantic.get(step_name(r)).ok_or_else(|| {
        Unresolved::invalid(
            cx.lineage_inputs.iter().cloned(),
            format!("{r} has no value"),
        )
    })
}
