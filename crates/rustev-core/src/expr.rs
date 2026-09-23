//! References, expressions and predicates of registry `rustev.exact/1`:
//! typing (compile phase e) and evaluation (spec 002, 3.4 and 3.8).

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::decimal::{ArithError, Decimal, Rounding};
use rustev_contract::definition::{CmpOp, Expr, Literal, Predicate};
use rustev_contract::time::{DurationMs, Timestamp};

use crate::value::{Ty, Value};

/// A parsed reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ref {
    /// An input, optionally followed by a record field path
    /// (`input:trip.dates/check_in`).
    Input(String, Vec<String>),
    Step(String),
    /// A field path inside the current item of a per-item operator.
    Item(Vec<String>),
    /// The item bound to a fan-out instance key (projections only).
    Bind(String),
    Now,
}

impl Ref {
    pub fn parse(s: &str) -> Result<Ref, String> {
        if s == "now" {
            return Ok(Ref::Now);
        }
        let (kind, rest) = s
            .split_once(':')
            .ok_or_else(|| format!("reference {s:?} has no kind"))?;
        if rest.is_empty() {
            return Err(format!("reference {s:?} names nothing"));
        }
        match kind {
            "input" => {
                let mut parts = rest.split('/');
                let name = parts.next().unwrap_or_default().to_string();
                let path: Vec<String> = parts.map(String::from).collect();
                if name.is_empty() || path.iter().any(String::is_empty) {
                    return Err(format!("reference {s:?} has an empty part"));
                }
                Ok(Ref::Input(name, path))
            }
            "step" => Ok(Ref::Step(rest.into())),
            "bind" => Ok(Ref::Bind(rest.into())),
            "item" => {
                let path: Vec<String> = rest.split('.').map(String::from).collect();
                if path.iter().any(String::is_empty) {
                    return Err(format!("reference {s:?} has an empty field"));
                }
                Ok(Ref::Item(path))
            }
            _ => Err(format!("reference {s:?} has unknown kind {kind:?}")),
        }
    }
}

/// The type of a step's result as expressions see it.
#[derive(Debug, Clone, PartialEq)]
pub enum StepTy {
    Exact(Ty),
    /// A semantic value and its fan-out binds; exact expressions cannot read
    /// it, only `weighted_rank@1` components and the policy can.
    Semantic(crate::sem::SemKind, Vec<String>),
}

/// Types visible while typing an expression.
pub struct TyEnv<'a> {
    pub inputs: &'a BTreeMap<String, Ty>,
    pub steps: &'a BTreeMap<String, StepTy>,
    pub item: Option<&'a Ty>,
}

/// The dependency a reference names: `input:<name>` or `step:<id>` without
/// any field path.
pub fn base_ref(r: &str) -> String {
    match r.strip_prefix("input:") {
        Some(rest) => format!("input:{}", rest.split('/').next().unwrap_or_default()),
        None => r.to_string(),
    }
}

/// Every reference an expression or predicate reads, in reading order.
pub fn expr_refs(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Ref(r) => out.push(r.clone()),
        Expr::Lit(_) => {}
        Expr::Add { left, right }
        | Expr::Sub { left, right }
        | Expr::Mul { left, right }
        | Expr::Div { left, right } => {
            expr_refs(left, out);
            expr_refs(right, out);
        }
        Expr::Round { value, .. } | Expr::Len(value) | Expr::ToDecimal(value) => {
            expr_refs(value, out)
        }
        Expr::Fx {
            amount,
            from,
            to,
            rates,
            ..
        } => {
            expr_refs(amount, out);
            expr_refs(from, out);
            expr_refs(to, out);
            out.push(rates.clone());
        }
    }
}

pub fn predicate_refs(p: &Predicate, out: &mut Vec<String>) {
    match p {
        Predicate::Cmp { left, right, .. } => {
            expr_refs(left, out);
            expr_refs(right, out);
        }
        Predicate::All(ps) | Predicate::Any(ps) => ps.iter().for_each(|p| predicate_refs(p, out)),
        Predicate::Not(p) => predicate_refs(p, out),
        Predicate::Member { value, .. } => expr_refs(value, out),
        Predicate::Present(r) => out.push(r.clone()),
    }
}

/// Structural checks of compile phase b: scales in range.
pub fn expr_structure(e: &Expr) -> Result<(), String> {
    match e {
        Expr::Ref(_) | Expr::Lit(_) => Ok(()),
        Expr::Add { left, right }
        | Expr::Sub { left, right }
        | Expr::Mul { left, right }
        | Expr::Div { left, right } => {
            expr_structure(left)?;
            expr_structure(right)
        }
        Expr::Round { value, scale } => {
            if *scale > 9 {
                return Err(format!("scale {scale} exceeds 9"));
            }
            expr_structure(value)
        }
        Expr::Fx {
            amount,
            from,
            to,
            scale,
            ..
        } => {
            if *scale > 9 {
                return Err(format!("scale {scale} exceeds 9"));
            }
            expr_structure(amount)?;
            expr_structure(from)?;
            expr_structure(to)
        }
        Expr::Len(v) | Expr::ToDecimal(v) => expr_structure(v),
    }
}

pub fn predicate_structure(p: &Predicate) -> Result<(), String> {
    match p {
        Predicate::Cmp { left, right, .. } => {
            expr_structure(left)?;
            expr_structure(right)
        }
        Predicate::All(ps) | Predicate::Any(ps) => ps.iter().try_for_each(predicate_structure),
        Predicate::Not(p) => predicate_structure(p),
        Predicate::Member { value, set } => {
            if set.is_empty() {
                return Err("member set is empty".into());
            }
            expr_structure(value)
        }
        Predicate::Present(r) => match Ref::parse(r)? {
            Ref::Input(_, p) if p.is_empty() => Ok(()),
            _ => Err(format!("present() takes an input reference, not {r:?}")),
        },
    }
}

fn field_ty<'t>(ty: &'t Ty, path: &[String]) -> Result<&'t Ty, String> {
    let mut cur = ty;
    for f in path {
        match cur {
            Ty::Record(fields) => {
                cur = fields
                    .iter()
                    .find(|(n, _)| n == f)
                    .map(|(_, t)| t)
                    .ok_or_else(|| format!("no field {f:?}"))?;
            }
            other => return Err(format!("{} has no field {f:?}", other.name())),
        }
    }
    Ok(cur)
}

/// The type of a reference, or a kind mismatch.
pub fn ref_ty(r: &str, env: &TyEnv<'_>) -> Result<Ty, String> {
    match Ref::parse(r)? {
        Ref::Input(n, path) => {
            let t = env
                .inputs
                .get(&n)
                .ok_or_else(|| format!("unknown input {n:?}"))?;
            field_ty(t, &path).cloned()
        }
        Ref::Step(s) => match env.steps.get(&s) {
            Some(StepTy::Exact(t)) => Ok(t.clone()),
            Some(StepTy::Semantic(..)) => Err(format!(
                "step {s:?} is semantic; exact operators cannot read it"
            )),
            None => Err(format!("unknown step {s:?}")),
        },
        Ref::Item(path) => {
            let item = env
                .item
                .ok_or_else(|| format!("{r:?} outside a per-item operator"))?;
            field_ty(item, &path).cloned()
        }
        Ref::Now => Ok(Ty::Timestamp),
        Ref::Bind(_) => Err(format!("{r:?} is only valid in a projection")),
    }
}

fn is_currency(t: &Ty) -> bool {
    matches!(t, Ty::Text { .. } | Ty::Enum(_))
}

/// The static type of an expression (compile phase e). Values never convert
/// silently: integer and decimal mix only through `to_decimal`.
pub fn type_expr(e: &Expr, env: &TyEnv<'_>) -> Result<Ty, String> {
    let bin = |l: &Expr, r: &Expr| -> Result<(Ty, Ty), String> {
        Ok((type_expr(l, env)?, type_expr(r, env)?))
    };
    match e {
        Expr::Ref(r) => ref_ty(r, env),
        Expr::Lit(l) => Ok(Value::literal_ty(l)),
        Expr::Add { left, right } => match bin(left, right)? {
            (Ty::Integer, Ty::Integer) => Ok(Ty::Integer),
            (Ty::Decimal, Ty::Decimal) => Ok(Ty::Decimal),
            (Ty::Timestamp, Ty::Duration) => Ok(Ty::Timestamp),
            (Ty::Duration, Ty::Duration) => Ok(Ty::Duration),
            (a, b) => Err(format!("cannot add {} and {}", a.name(), b.name())),
        },
        Expr::Sub { left, right } => match bin(left, right)? {
            (Ty::Integer, Ty::Integer) => Ok(Ty::Integer),
            (Ty::Decimal, Ty::Decimal) => Ok(Ty::Decimal),
            (Ty::Timestamp, Ty::Duration) => Ok(Ty::Timestamp),
            (Ty::Timestamp, Ty::Timestamp) => Ok(Ty::Duration),
            (Ty::Duration, Ty::Duration) => Ok(Ty::Duration),
            (a, b) => Err(format!("cannot subtract {} from {}", b.name(), a.name())),
        },
        Expr::Mul { left, right } => match bin(left, right)? {
            (Ty::Integer, Ty::Integer) => Ok(Ty::Integer),
            (Ty::Decimal, Ty::Decimal) => Ok(Ty::Decimal),
            (a, b) => Err(format!("cannot multiply {} by {}", a.name(), b.name())),
        },
        Expr::Div { left, right } => match bin(left, right)? {
            (Ty::Decimal, Ty::Decimal) => Ok(Ty::Decimal),
            (a, b) => Err(format!(
                "cannot divide {} by {} (only decimals divide)",
                a.name(),
                b.name()
            )),
        },
        Expr::Round { value, .. } => match type_expr(value, env)? {
            Ty::Decimal => Ok(Ty::Decimal),
            t => Err(format!("cannot round {}", t.name())),
        },
        Expr::Fx {
            amount,
            from,
            to,
            rates,
            ..
        } => {
            if type_expr(amount, env)? != Ty::Decimal {
                return Err("fx amount must be a decimal".into());
            }
            if !is_currency(&type_expr(from, env)?) || !is_currency(&type_expr(to, env)?) {
                return Err("fx currencies must be text or enum".into());
            }
            match ref_ty(rates, env)? {
                Ty::List { item, .. } => match item.as_ref() {
                    Ty::Record(f)
                        if f.iter().any(|(n, t)| n == "currency" && is_currency(t))
                            && f.iter().any(|(n, t)| n == "rate" && *t == Ty::Decimal) =>
                    {
                        Ok(Ty::Decimal)
                    }
                    _ => Err(
                        "fx rates must be a list of records with `currency` and decimal `rate`"
                            .into(),
                    ),
                },
                t => Err(format!("fx rates must be a list, not {}", t.name())),
            }
        }
        Expr::Len(v) => match type_expr(v, env)? {
            Ty::List { .. } => Ok(Ty::Integer),
            t => Err(format!("len() of {}", t.name())),
        },
        Expr::ToDecimal(v) => match type_expr(v, env)? {
            Ty::Integer => Ok(Ty::Decimal),
            t => Err(format!("to_decimal() of {}", t.name())),
        },
    }
}

fn literal_fits(l: &Literal, t: &Ty) -> bool {
    match (l, t) {
        (Literal::Enum(s), Ty::Enum(opts)) => opts.contains(s),
        _ => Value::literal_ty(l).same_shape(t),
    }
}

fn operand_fits(e: &Expr, t_other: &Ty, t_self: &Ty) -> bool {
    match e {
        Expr::Lit(l) => literal_fits(l, t_other),
        _ => t_self.same_shape(t_other),
    }
}

pub fn type_predicate(p: &Predicate, env: &TyEnv<'_>) -> Result<(), String> {
    match p {
        Predicate::Cmp { left, op, right } => {
            let (lt, rt) = (type_expr(left, env)?, type_expr(right, env)?);
            if matches!(lt, Ty::Maybe(_)) || matches!(rt, Ty::Maybe(_)) {
                return Err("a maybe value cannot be compared".into());
            }
            if !(operand_fits(left, &rt, &lt) && operand_fits(right, &lt, &rt)) {
                return Err(format!("cannot compare {} with {}", lt.name(), rt.name()));
            }
            let ordered = !matches!(op, CmpOp::Eq | CmpOp::Ne);
            if ordered && !lt.is_orderable() {
                return Err(format!("{} has no order", lt.name()));
            }
            if !ordered
                && !matches!(
                    lt,
                    Ty::Bool
                        | Ty::Integer
                        | Ty::Decimal
                        | Ty::Text { .. }
                        | Ty::Enum(_)
                        | Ty::Timestamp
                        | Ty::Duration
                )
            {
                return Err(format!("{} cannot be tested for equality", lt.name()));
            }
            Ok(())
        }
        Predicate::All(ps) | Predicate::Any(ps) => {
            ps.iter().try_for_each(|p| type_predicate(p, env))
        }
        Predicate::Not(p) => type_predicate(p, env),
        Predicate::Member { value, set } => {
            let t = type_expr(value, env)?;
            if set.iter().all(|l| literal_fits(l, &t)) {
                Ok(())
            } else {
                Err(format!("member set does not match {}", t.name()))
            }
        }
        Predicate::Present(r) => match Ref::parse(r)? {
            Ref::Input(n, p) if p.is_empty() && env.inputs.contains_key(&n) => Ok(()),
            _ => Err(format!("present() of unknown input {r:?}")),
        },
    }
}

/// Why evaluation produced no value; becomes the step's unresolved reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// Overflow, division by zero, an out-of-range timestamp.
    Invalid(String),
    /// A missing fact, e.g. `fx.rates.EUR`.
    Missing(String),
    /// Two different values for one fact.
    Conflict(String),
}

impl From<ArithError> for Fault {
    fn from(e: ArithError) -> Self {
        Fault::Invalid(e.to_string())
    }
}

/// Values visible while evaluating.
pub struct Scope<'a> {
    pub now: Timestamp,
    pub inputs: &'a BTreeMap<String, Value>,
    pub steps: &'a BTreeMap<String, Value>,
    pub present: &'a BTreeSet<String>,
    pub item: Option<&'a Value>,
}

fn field<'v>(v: &'v Value, path: &[String]) -> Result<&'v Value, Fault> {
    let mut cur = v;
    for f in path {
        match cur {
            Value::Record(m) => {
                cur = m
                    .get(f)
                    .ok_or_else(|| Fault::Invalid(format!("no field {f:?}")))?
            }
            _ => return Err(Fault::Invalid(format!("no field {f:?}"))),
        }
    }
    Ok(cur)
}

pub fn eval_ref(r: &str, s: &Scope<'_>) -> Result<Value, Fault> {
    match Ref::parse(r).map_err(Fault::Invalid)? {
        Ref::Input(n, path) => {
            let v = s.inputs.get(&n).ok_or_else(|| Fault::Missing(n.clone()))?;
            field(v, &path).cloned()
        }
        Ref::Step(n) => s.steps.get(&n).cloned().ok_or(Fault::Missing(n)),
        Ref::Item(path) => {
            let item = s
                .item
                .ok_or_else(|| Fault::Invalid("no current item".into()))?;
            field(item, &path).cloned()
        }
        Ref::Now => Ok(Value::Timestamp(s.now)),
        Ref::Bind(b) => Err(Fault::Invalid(format!("bind:{b} outside a projection"))),
    }
}

fn time_err(_: rustev_contract::time::TimeRangeError) -> Fault {
    Fault::Invalid("timestamp or duration out of range".into())
}

fn rate(rates: &[Value], currency: &str, table: &str) -> Result<Decimal, Fault> {
    let mut found: Option<Decimal> = None;
    for r in rates {
        let (Ok(Value::Text(c) | Value::Enum(c)), Ok(Value::Decimal(v))) =
            (field(r, &["currency".into()]), field(r, &["rate".into()]))
        else {
            continue;
        };
        if c == currency {
            match found {
                Some(prev) if prev != *v => {
                    return Err(Fault::Conflict(format!("{table}.{currency}")));
                }
                _ => found = Some(*v),
            }
        }
    }
    found.ok_or_else(|| Fault::Missing(format!("{table}.{currency}")))
}

/// `amount / rate(from) * rate(to)`, each step half-even at 10^-9, then
/// rescaled half-even to `scale` (`fx_convert@1`).
pub fn fx(
    amount: Decimal,
    from: &str,
    to: &str,
    rates: &[Value],
    table: &str,
    scale: u32,
) -> Result<Decimal, Fault> {
    let rf = rate(rates, from, table)?;
    let rt = rate(rates, to, table)?;
    let base = amount.checked_div(rf, Rounding::HalfEven)?;
    Ok(base
        .checked_mul(rt, Rounding::HalfEven)?
        .round_to_scale(scale, Rounding::HalfEven)?)
}

fn text_of(v: Value) -> Result<String, Fault> {
    match v {
        Value::Text(s) | Value::Enum(s) => Ok(s),
        _ => Err(Fault::Invalid("expected text".into())),
    }
}

pub fn eval_expr(e: &Expr, s: &Scope<'_>) -> Result<Value, Fault> {
    let bin = |l: &Expr, r: &Expr| -> Result<(Value, Value), Fault> {
        Ok((eval_expr(l, s)?, eval_expr(r, s)?))
    };
    let overflow = || Fault::Invalid("integer overflow".into());
    match e {
        Expr::Ref(r) => eval_ref(r, s),
        Expr::Lit(l) => Ok(Value::from_literal(l)),
        Expr::Add { left, right } => match bin(left, right)? {
            (Value::Integer(a), Value::Integer(b)) => {
                a.checked_add(b).map(Value::Integer).ok_or_else(overflow)
            }
            (Value::Decimal(a), Value::Decimal(b)) => Ok(Value::Decimal(a.checked_add(b)?)),
            (Value::Timestamp(a), Value::Duration(b)) => {
                a.checked_add(b).map(Value::Timestamp).map_err(time_err)
            }
            (Value::Duration(a), Value::Duration(b)) => {
                a.0.checked_add(b.0)
                    .map(|d| Value::Duration(DurationMs(d)))
                    .ok_or_else(overflow)
            }
            _ => Err(Fault::Invalid("ill-typed add".into())),
        },
        Expr::Sub { left, right } => match bin(left, right)? {
            (Value::Integer(a), Value::Integer(b)) => {
                a.checked_sub(b).map(Value::Integer).ok_or_else(overflow)
            }
            (Value::Decimal(a), Value::Decimal(b)) => Ok(Value::Decimal(a.checked_sub(b)?)),
            (Value::Timestamp(a), Value::Duration(b)) => {
                a.checked_sub(b).map(Value::Timestamp).map_err(time_err)
            }
            (Value::Timestamp(a), Value::Timestamp(b)) => {
                a.since(b).map(Value::Duration).map_err(time_err)
            }
            (Value::Duration(a), Value::Duration(b)) => {
                a.0.checked_sub(b.0)
                    .map(|d| Value::Duration(DurationMs(d)))
                    .ok_or_else(|| Fault::Invalid("negative duration".into()))
            }
            _ => Err(Fault::Invalid("ill-typed sub".into())),
        },
        Expr::Mul { left, right } => match bin(left, right)? {
            (Value::Integer(a), Value::Integer(b)) => {
                a.checked_mul(b).map(Value::Integer).ok_or_else(overflow)
            }
            (Value::Decimal(a), Value::Decimal(b)) => {
                Ok(Value::Decimal(a.checked_mul(b, Rounding::HalfEven)?))
            }
            _ => Err(Fault::Invalid("ill-typed mul".into())),
        },
        Expr::Div { left, right } => match bin(left, right)? {
            (Value::Decimal(a), Value::Decimal(b)) => {
                Ok(Value::Decimal(a.checked_div(b, Rounding::HalfEven)?))
            }
            _ => Err(Fault::Invalid("ill-typed div".into())),
        },
        Expr::Round { value, scale } => match eval_expr(value, s)? {
            Value::Decimal(d) => Ok(Value::Decimal(
                d.round_to_scale(*scale, Rounding::HalfEven)?,
            )),
            _ => Err(Fault::Invalid("ill-typed round".into())),
        },
        Expr::Fx {
            amount,
            from,
            to,
            rates,
            scale,
        } => {
            let Value::Decimal(a) = eval_expr(amount, s)? else {
                return Err(Fault::Invalid("ill-typed fx".into()));
            };
            let from = text_of(eval_expr(from, s)?)?;
            let to = text_of(eval_expr(to, s)?)?;
            let Value::List(table) = eval_ref(rates, s)? else {
                return Err(Fault::Invalid("ill-typed fx rates".into()));
            };
            let name = rates.strip_prefix("input:").unwrap_or(rates);
            Ok(Value::Decimal(fx(a, &from, &to, &table, name, *scale)?))
        }
        Expr::Len(v) => match eval_expr(v, s)? {
            Value::List(items) => i64::try_from(items.len())
                .map(Value::Integer)
                .map_err(|_| overflow()),
            _ => Err(Fault::Invalid("ill-typed len".into())),
        },
        Expr::ToDecimal(v) => match eval_expr(v, s)? {
            Value::Integer(i) => Ok(Value::Decimal(Decimal::from_i64(i))),
            _ => Err(Fault::Invalid("ill-typed to_decimal".into())),
        },
    }
}

fn cmp_values(a: &Value, op: CmpOp, b: &Value) -> Result<bool, Fault> {
    use std::cmp::Ordering;
    let ord: Option<Ordering> = match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => Some(x.cmp(y)),
        (Value::Decimal(x), Value::Decimal(y)) => Some(x.cmp(y)),
        (Value::Timestamp(x), Value::Timestamp(y)) => Some(x.cmp(y)),
        (Value::Duration(x), Value::Duration(y)) => Some(x.cmp(y)),
        _ => None,
    };
    Ok(match (op, ord) {
        (CmpOp::Eq, _) => {
            a == b
                || matches!((a, b), (Value::Text(x), Value::Text(y)) | (Value::Enum(x), Value::Enum(y)) if x == y)
        }
        (CmpOp::Ne, _) => a != b,
        (CmpOp::Lt, Some(o)) => o == Ordering::Less,
        (CmpOp::Le, Some(o)) => o != Ordering::Greater,
        (CmpOp::Gt, Some(o)) => o == Ordering::Greater,
        (CmpOp::Ge, Some(o)) => o != Ordering::Less,
        _ => return Err(Fault::Invalid("ill-typed comparison".into())),
    })
}

pub fn eval_predicate(p: &Predicate, s: &Scope<'_>) -> Result<bool, Fault> {
    match p {
        Predicate::Cmp { left, op, right } => {
            cmp_values(&eval_expr(left, s)?, *op, &eval_expr(right, s)?)
        }
        Predicate::All(ps) => {
            for p in ps {
                if !eval_predicate(p, s)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Predicate::Any(ps) => {
            for p in ps {
                if eval_predicate(p, s)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Predicate::Not(p) => Ok(!eval_predicate(p, s)?),
        Predicate::Member { value, set } => {
            let v = eval_expr(value, s)?;
            Ok(set.iter().any(|l| Value::from_literal(l) == v))
        }
        Predicate::Present(r) => Ok(r
            .strip_prefix("input:")
            .is_some_and(|n| s.present.contains(n))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::definition::Literal as L;

    fn dec(s: &str) -> Decimal {
        Decimal::parse(s).unwrap()
    }

    fn rates(v: &[(&str, &str)]) -> Vec<Value> {
        v.iter()
            .map(|(c, r)| {
                Value::Record(
                    [
                        ("currency".to_string(), Value::Text(c.to_string())),
                        ("rate".to_string(), Value::Decimal(dec(r))),
                    ]
                    .into(),
                )
            })
            .collect()
    }

    #[test]
    fn fx_converts_through_the_base_and_rounds_half_even() {
        let t = rates(&[("USD", "1"), ("EUR", "0.8"), ("JPY", "150")]);
        assert_eq!(fx(dec("100"), "EUR", "USD", &t, "fx", 2), Ok(dec("125")));
        assert_eq!(fx(dec("1"), "USD", "JPY", &t, "fx", 0), Ok(dec("150")));
        // 0.125 at scale 2 is a tie: half-even gives 0.12; 0.135 gives 0.14.
        let t2 = rates(&[("A", "1"), ("B", "0.125"), ("C", "0.135")]);
        assert_eq!(fx(dec("1"), "A", "B", &t2, "fx", 2), Ok(dec("0.12")));
        assert_eq!(fx(dec("1"), "A", "C", &t2, "fx", 2), Ok(dec("0.14")));
    }

    #[test]
    fn fx_missing_conflicting_and_zero_rates_are_faults() {
        let t = rates(&[("USD", "1"), ("EUR", "0.8"), ("EUR", "0.9"), ("ZZZ", "0")]);
        assert_eq!(
            fx(dec("1"), "GBP", "USD", &t, "fx", 2),
            Err(Fault::Missing("fx.GBP".into()))
        );
        assert_eq!(
            fx(dec("1"), "EUR", "USD", &t, "fx", 2),
            Err(Fault::Conflict("fx.EUR".into()))
        );
        assert!(matches!(
            fx(dec("1"), "ZZZ", "USD", &t, "fx", 2),
            Err(Fault::Invalid(_))
        ));
        // Duplicate but equal rates are not a conflict.
        let same = rates(&[("USD", "1"), ("EUR", "0.8"), ("EUR", "0.8")]);
        assert!(fx(dec("1"), "EUR", "USD", &same, "fx", 2).is_ok());
    }

    #[test]
    fn arithmetic_overflow_is_a_fault_not_a_wrap() {
        let inputs = BTreeMap::new();
        let steps = BTreeMap::new();
        let present = BTreeSet::new();
        let s = Scope {
            now: Timestamp::from_ms(0).unwrap(),
            inputs: &inputs,
            steps: &steps,
            present: &present,
            item: None,
        };
        let big = Expr::Lit(L::Integer(i64::MAX));
        let add = Expr::Add {
            left: Box::new(big.clone()),
            right: Box::new(Expr::Lit(L::Integer(1))),
        };
        assert!(matches!(eval_expr(&add, &s), Err(Fault::Invalid(_))));
        let div0 = Expr::Div {
            left: Box::new(Expr::Lit(L::Decimal(dec("1")))),
            right: Box::new(Expr::Lit(L::Decimal(dec("0")))),
        };
        assert!(matches!(eval_expr(&div0, &s), Err(Fault::Invalid(_))));
        let back = Expr::Sub {
            left: Box::new(Expr::Ref("now".into())),
            right: Box::new(Expr::Lit(L::DurationMs(1))),
        };
        assert!(matches!(eval_expr(&back, &s), Err(Fault::Invalid(_))));
        // Negative control: in-range arithmetic succeeds.
        let ok = Expr::Add {
            left: Box::new(Expr::Lit(L::Integer(1))),
            right: Box::new(Expr::Lit(L::Integer(1))),
        };
        assert_eq!(eval_expr(&ok, &s), Ok(Value::Integer(2)));
    }

    #[test]
    fn integers_and_decimals_do_not_mix_silently() {
        let inputs = BTreeMap::new();
        let steps = BTreeMap::new();
        let env = TyEnv {
            inputs: &inputs,
            steps: &steps,
            item: None,
        };
        let mixed = Expr::Add {
            left: Box::new(Expr::Lit(L::Integer(1))),
            right: Box::new(Expr::Lit(L::Decimal(dec("1")))),
        };
        assert!(type_expr(&mixed, &env).is_err());
        let explicit = Expr::Add {
            left: Box::new(Expr::ToDecimal(Box::new(Expr::Lit(L::Integer(1))))),
            right: Box::new(Expr::Lit(L::Decimal(dec("1")))),
        };
        assert_eq!(type_expr(&explicit, &env), Ok(Ty::Decimal));
        let int_div = Expr::Div {
            left: Box::new(Expr::Lit(L::Integer(1))),
            right: Box::new(Expr::Lit(L::Integer(1))),
        };
        assert!(type_expr(&int_div, &env).is_err());
    }

    #[test]
    fn enum_literals_must_be_declared_options() {
        let inputs: BTreeMap<String, Ty> = [(
            "tier".to_string(),
            Ty::Enum(vec!["free".into(), "enterprise".into()]),
        )]
        .into();
        let steps = BTreeMap::new();
        let env = TyEnv {
            inputs: &inputs,
            steps: &steps,
            item: None,
        };
        let ok = Predicate::Cmp {
            left: Expr::Ref("input:tier".into()),
            op: CmpOp::Eq,
            right: Expr::Lit(L::Enum("enterprise".into())),
        };
        assert!(type_predicate(&ok, &env).is_ok());
        let bad = Predicate::Cmp {
            left: Expr::Ref("input:tier".into()),
            op: CmpOp::Eq,
            right: Expr::Lit(L::Enum("gold".into())),
        };
        assert!(type_predicate(&bad, &env).is_err());
        let text = Predicate::Cmp {
            left: Expr::Ref("input:tier".into()),
            op: CmpOp::Eq,
            right: Expr::Lit(L::Text("enterprise".into())),
        };
        assert!(type_predicate(&text, &env).is_err(), "text is not an enum");
        let ordered = Predicate::Cmp {
            left: Expr::Ref("input:tier".into()),
            op: CmpOp::Lt,
            right: Expr::Lit(L::Enum("free".into())),
        };
        assert!(
            type_predicate(&ordered, &env).is_err(),
            "enums have no order"
        );
    }

    #[test]
    fn references_parse_strictly() {
        assert_eq!(
            Ref::parse("input:a.b"),
            Ok(Ref::Input("a.b".into(), vec![]))
        );
        assert_eq!(
            Ref::parse("input:a.b/c/d"),
            Ok(Ref::Input("a.b".into(), vec!["c".into(), "d".into()]))
        );
        assert_eq!(
            Ref::parse("item:price.amount"),
            Ok(Ref::Item(vec!["price".into(), "amount".into()]))
        );
        assert_eq!(Ref::parse("now"), Ok(Ref::Now));
        for bad in [
            "",
            "input:",
            "x:y",
            "item:a..b",
            "plain",
            "input:a//b",
            "input:/b",
        ] {
            assert!(Ref::parse(bad).is_err(), "{bad:?}");
        }
    }
}
