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
    /// A version 2 definition: step references may carry a field path
    /// (`step:<id>/<field>`) and item paths may use `/` (spec 032, 3.4.4
    /// and 3.5.4). Version 2 step ids and field names hold no `/` or `.`.
    pub v2: bool,
}

/// A version 2 step reference's id and field path.
pub fn split_step(s: &str) -> (String, Vec<String>) {
    let mut parts = s.split('/');
    let id = parts.next().unwrap_or_default().to_string();
    (id, parts.map(String::from).collect())
}

/// A version 2 item path, with `/` as well as `.` separating fields.
pub fn split_item(path: Vec<String>) -> Vec<String> {
    path.iter()
        .flat_map(|p| p.split('/').map(String::from))
        .collect()
}

/// The dependency a reference names: `input:<name>` or `step:<id>` without
/// any field path.
pub fn base_ref(r: &str) -> String {
    match r.strip_prefix("input:") {
        Some(rest) => format!("input:{}", rest.split('/').next().unwrap_or_default()),
        None => r.to_string(),
    }
}

/// [`base_ref`] for a version 2 definition, whose step references may carry
/// a field path.
pub fn base_ref_v2(r: &str) -> String {
    match r.strip_prefix("step:") {
        Some(s) => format!("step:{}", split_step(s).0),
        None => base_ref(r),
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
        Expr::Graded { reference, .. } => out.push(reference.clone()),
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
        Predicate::Present(r) | Predicate::IsConstructed { reference: r } => out.push(r.clone()),
        Predicate::MemberRef { value, set } => {
            expr_refs(value, out);
            out.push(set.clone());
        }
    }
}

/// Whether an expression uses a construct of `rustev.exact/2` that a
/// version 1 definition cannot carry (spec 032, 3.1.3).
pub fn expr_uses_v2(e: &Expr) -> bool {
    match e {
        Expr::Graded { .. } => true,
        Expr::Ref(_) | Expr::Lit(_) => false,
        Expr::Add { left, right }
        | Expr::Sub { left, right }
        | Expr::Mul { left, right }
        | Expr::Div { left, right } => expr_uses_v2(left) || expr_uses_v2(right),
        Expr::Round { value, .. } | Expr::Len(value) | Expr::ToDecimal(value) => {
            expr_uses_v2(value)
        }
        Expr::Fx {
            amount, from, to, ..
        } => expr_uses_v2(amount) || expr_uses_v2(from) || expr_uses_v2(to),
    }
}

/// Whether a predicate uses a construct of `rustev.exact/2`.
pub fn predicate_uses_v2(p: &Predicate) -> bool {
    match p {
        Predicate::MemberRef { .. } | Predicate::IsConstructed { .. } => true,
        Predicate::Cmp { left, right, .. } => expr_uses_v2(left) || expr_uses_v2(right),
        Predicate::All(ps) | Predicate::Any(ps) => ps.iter().any(predicate_uses_v2),
        Predicate::Not(p) => predicate_uses_v2(p),
        Predicate::Member { value, .. } => expr_uses_v2(value),
        Predicate::Present(_) => false,
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
        Expr::Graded { reference, .. } => match Ref::parse(reference)? {
            Ref::Input(..) | Ref::Step(_) | Ref::Item(_) => Ok(()),
            _ => Err(format!("a grade requirement reads {reference:?}")),
        },
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
        Predicate::IsConstructed { reference } => Ref::parse(reference).map(|_| ()),
        Predicate::Present(r) => match Ref::parse(r)? {
            Ref::Step(s) if !s.contains('/') => Ok(()),
            Ref::Input(_, p) if p.is_empty() => Ok(()),
            _ => Err(format!(
                "present() takes an input or a bare step reference, not {r:?}"
            )),
        },
        Predicate::MemberRef { value, set } => {
            expr_structure(value)?;
            match Ref::parse(set)? {
                Ref::Input(..) | Ref::Step(_) => Ok(()),
                _ => Err(format!("member_ref set {set:?} must be an input or step")),
            }
        }
    }
}

fn field_ty<'t>(ty: &'t Ty, path: &[String]) -> Result<&'t Ty, String> {
    let unwrap = |t: &'t Ty| match t {
        // Reading a nullable field yields its type; `null` is missing.
        Ty::Nullable(inner) => inner.as_ref(),
        t => t,
    };
    // A root is never nullable (`nullable` is a record field type only,
    // spec 032 3.4.1), so only a field read is unwrapped.
    let mut cur = ty;
    for f in path {
        if f.is_empty() {
            return Err("a path has an empty field name".into());
        }
        match cur {
            Ty::Record(fields) => {
                cur = fields
                    .iter()
                    .find(|(n, _)| n == f)
                    .map(|(_, t)| t)
                    .ok_or_else(|| format!("no field {f:?}"))?;
                cur = unwrap(cur);
            }
            other => return Err(format!("{} has no field {f:?}", other.name())),
        }
    }
    Ok(cur)
}

/// Plan 3 acquired-key projection (spec 036 acceptance): one list of
/// records becomes a bounded text list. Metadata uses the same `*` path.
fn step_field_ty(ty: &Ty, path: &[String]) -> Result<Ty, String> {
    let Some(index) = path.iter().position(|p| p == "*") else {
        return field_ty(ty, path).cloned();
    };
    let Ty::List { item, max } = field_ty(ty, &path[..index])? else {
        return Err("text projection requires a list of records".into());
    };
    if !matches!(item.as_ref(), Ty::Record(_)) || index + 1 == path.len() {
        return Err("text projection requires a record field".into());
    }
    let projected = field_ty(item, &path[index + 1..])?;
    if !matches!(projected, Ty::Text { .. }) {
        return Err("text projection requires a text field".into());
    }
    Ok(Ty::List {
        item: Box::new(projected.clone()),
        max: *max,
    })
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
        Ref::Step(s) => {
            let (s, path) = if env.v2 { split_step(&s) } else { (s, vec![]) };
            if s.is_empty() {
                return Err(format!("{r:?} names no step"));
            }
            match env.steps.get(&s) {
                Some(StepTy::Exact(t)) => step_field_ty(t, &path),
                Some(StepTy::Semantic(..)) => Err(format!(
                    "step {s:?} is semantic; exact operators cannot read it"
                )),
                None => Err(format!("unknown step {s:?}")),
            }
        }
        Ref::Item(path) => {
            let item = env
                .item
                .ok_or_else(|| format!("{r:?} outside a per-item operator"))?;
            let path = if env.v2 { split_item(path) } else { path };
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
        Expr::Graded { reference, .. } => ref_ty(reference, env),
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
        Predicate::IsConstructed { reference } => ref_ty(reference, env).map(|_| ()),
        Predicate::Present(r) => match Ref::parse(r)? {
            Ref::Step(s) if env.v2 && !s.contains('/') => ref_ty(r, env).map(|_| ()),
            Ref::Input(n, p) if p.is_empty() && env.inputs.contains_key(&n) => Ok(()),
            _ => Err(format!("present() of unknown input {r:?}")),
        },
        Predicate::MemberRef { value, set } => {
            let t = type_expr(value, env)?;
            let textual = matches!(t, Ty::Text { .. } | Ty::Enum(_));
            match ref_ty(set, env)? {
                Ty::List { item, .. } if textual && item.same_shape(&t) => Ok(()),
                // A map's keys are its members, never its values (spec 032,
                // 3.7.2).
                Ty::Map { .. } if textual => Ok(()),
                s => Err(format!(
                    "member_ref of {} in {}: the set is a list of text or enum values, or a map",
                    t.name(),
                    s.name()
                )),
            }
        }
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
    /// A read of an absent value or a `null` field, naming its path
    /// (spec 032, 3.4.4); only plan 3 values can be absent.
    Absent(String),
}

impl From<ArithError> for Fault {
    fn from(e: ArithError) -> Self {
        Fault::Invalid(e.to_string())
    }
}

/// Values visible while evaluating.
pub struct Scope<'a> {
    pub constructed: Option<&'a BTreeSet<String>>,
    pub now: Timestamp,
    pub inputs: &'a BTreeMap<String, Value>,
    pub steps: &'a BTreeMap<String, Value>,
    pub present: &'a BTreeSet<String>,
    pub item: Option<&'a Value>,
    /// See [`TyEnv::v2`].
    pub v2: bool,
}

/// Reads `path` from `v`. A `null` or absent value on the way is missing
/// evidence named by `name`. That includes the root: `field_ty` does not
/// unwrap a root because no root is typed nullable (spec 032 3.4.1), so a
/// null root can only be a fault, never a value.
fn field<'v>(v: &'v Value, path: &[String], name: &dyn Fn() -> String) -> Result<&'v Value, Fault> {
    let present = |v: &'v Value| match v {
        Value::Null | Value::Absent(_) => Err(Fault::Absent(name())),
        v => Ok(v),
    };
    let mut cur = present(v)?;
    for f in path {
        match cur {
            Value::Record(m) => {
                cur = present(
                    m.get(f)
                        .ok_or_else(|| Fault::Invalid(format!("no field {f:?}")))?,
                )?
            }
            _ => return Err(Fault::Invalid(format!("no field {f:?}"))),
        }
    }
    Ok(cur)
}

fn step_field(v: &Value, path: &[String], name: &dyn Fn() -> String) -> Result<Value, Fault> {
    let Some(index) = path.iter().position(|p| p == "*") else {
        return field(v, path, name).cloned();
    };
    let Value::List(items) = field(v, &path[..index], name)? else {
        return Err(Fault::Invalid("text projection requires a list".into()));
    };
    items
        .iter()
        .map(|item| match field(item, &path[index + 1..], name)? {
            Value::Text(text) => Ok(Value::Text(text.clone())),
            _ => Err(Fault::Invalid(
                "text projection requires a text field".into(),
            )),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::List)
}

pub fn eval_ref(r: &str, s: &Scope<'_>) -> Result<Value, Fault> {
    match Ref::parse(r).map_err(Fault::Invalid)? {
        Ref::Input(n, path) => {
            let v = s.inputs.get(&n).ok_or_else(|| Fault::Missing(n.clone()))?;
            field(v, &path, &|| {
                [vec![n.clone()], path.clone()].concat().join("/")
            })
            .cloned()
        }
        Ref::Step(n) if s.v2 => {
            let (id, path) = split_step(&n);
            if id.is_empty() {
                return Err(Fault::Invalid(format!("{r:?} names no step")));
            }
            let v = s.steps.get(&id).ok_or_else(|| Fault::Missing(id.clone()))?;
            step_field(v, &path, &|| format!("step:{n}"))
        }
        Ref::Step(n) => s.steps.get(&n).cloned().ok_or(Fault::Missing(n)),
        Ref::Item(path) => {
            let item = s
                .item
                .ok_or_else(|| Fault::Invalid("no current item".into()))?;
            let path = if s.v2 { split_item(path) } else { path };
            // `Ref::parse` refuses a bare `item:`; the fallback keeps a
            // missing-evidence name non-empty regardless.
            let name = || {
                if path.is_empty() {
                    "item".to_string()
                } else {
                    path.join("/")
                }
            };
            field(item, &path, &name).cloned()
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
        let (Ok(Value::Text(c) | Value::Enum(c)), Ok(Value::Decimal(v))) = (
            field(r, &["currency".into()], &String::new),
            field(r, &["rate".into()], &String::new),
        ) else {
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
        // The grade requirement is checked by the evaluator before the
        // expression is evaluated (spec 032, 3.9.1); the value is the
        // reference's own.
        Expr::Graded { reference, .. } => eval_ref(reference, s),
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
        Predicate::IsConstructed { reference } => Ok(s
            .constructed
            .is_some_and(|set| set.contains(&base_ref_v2(reference)))
            && eval_ref(reference, s).is_ok()),
        Predicate::Present(r) if s.v2 => Ok(eval_ref(r, s).is_ok()),
        Predicate::Present(r) => Ok(r
            .strip_prefix("input:")
            .is_some_and(|n| s.present.contains(n))),
        Predicate::MemberRef { value, set } => {
            let v = match eval_expr(value, s)? {
                Value::Text(x) | Value::Enum(x) => x,
                _ => return Err(Fault::Invalid("member_ref of a non-text value".into())),
            };
            match eval_ref(set, s)? {
                Value::List(items) => Ok(items
                    .iter()
                    .any(|i| matches!(i, Value::Text(x) | Value::Enum(x) if *x == v))),
                Value::Map(m) => Ok(m.contains_key(&v)),
                _ => Err(Fault::Invalid("member_ref of a non-set".into())),
            }
        }
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
    fn plan_three_text_projection_checks_bounds_types_and_absence() {
        let text = Ty::Text { max_bytes: 32 };
        let records = Ty::List {
            item: Box::new(Ty::Record(vec![
                ("key".into(), text.clone()),
                ("count".into(), Ty::Integer),
            ])),
            max: 4,
        };
        let ty = Ty::Record(vec![("items".into(), records)]);
        let path: Vec<String> = ["items", "*", "key"].map(String::from).into();
        assert_eq!(
            step_field_ty(&ty, &path).unwrap(),
            Ty::List {
                item: Box::new(text),
                max: 4
            }
        );
        for p in [
            vec!["items", "*", "count"],
            vec!["items", "*", "unknown"],
            vec!["items", "*"],
            vec!["*", "key"],
            vec!["items", "*", "key", "*", "x"],
        ] {
            assert!(
                step_field_ty(&ty, &p.into_iter().map(String::from).collect::<Vec<_>>()).is_err()
            );
        }
        let inputs = BTreeMap::new();
        let steps = BTreeMap::from([("lookup".into(), StepTy::Exact(ty))]);
        let mut env = TyEnv {
            inputs: &inputs,
            steps: &steps,
            item: None,
            v2: true,
        };
        assert!(ref_ty("step:lookup/items/*/key", &env).is_ok());
        env.v2 = false;
        assert!(ref_ty("step:lookup/items/*/key", &env).is_err());
        let value = |keys: Vec<Value>| {
            Value::Record(BTreeMap::from([(
                "items".into(),
                Value::List(
                    keys.into_iter()
                        .map(|key| Value::Record(BTreeMap::from([("key".into(), key)])))
                        .collect(),
                ),
            )]))
        };
        let keys = vec![
            Value::Text("b".into()),
            Value::Text("a".into()),
            Value::Text("b".into()),
        ];
        assert_eq!(
            step_field(&value(keys.clone()), &path, &|| "lookup".into()).unwrap(),
            Value::List(keys)
        );
        assert_eq!(
            step_field(&value(vec![]), &path, &|| "lookup".into()).unwrap(),
            Value::List(vec![])
        );
        assert!(matches!(
            step_field(&value(vec![Value::Null]), &path, &|| "lookup/key".into()),
            Err(Fault::Absent(_))
        ));
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
            constructed: None,
            now: Timestamp::from_ms(0).unwrap(),
            inputs: &inputs,
            steps: &steps,
            present: &present,
            item: None,
            v2: false,
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
    fn an_empty_field_name_in_a_path_is_refused() {
        let rec = Ty::Record(vec![("a".into(), Ty::Integer)]);
        assert_eq!(field_ty(&rec, &["a".into()]), Ok(&Ty::Integer));
        let e = field_ty(&rec, &["".into(), "a".into()]).unwrap_err();
        assert!(e.contains("empty field name"), "{e}");
    }

    #[test]
    fn version_2_step_paths_split_at_the_step_id() {
        assert_eq!(
            split_step("join/match/total"),
            (
                "join".to_string(),
                vec!["match".to_string(), "total".to_string()]
            )
        );
        assert_eq!(split_step("plain"), ("plain".to_string(), vec![]));
        // `Ref::parse` takes a step reference whole, so a version 2 path
        // with an empty id is refused where it is split.
        assert!(Ref::parse("step:/a").is_ok());
        let inputs = BTreeMap::new();
        let steps = BTreeMap::from([("".to_string(), StepTy::Exact(Ty::Bool))]);
        let env = TyEnv {
            inputs: &inputs,
            steps: &steps,
            item: None,
            v2: true,
        };
        let e = ref_ty("step:/a", &env).unwrap_err();
        assert!(e.contains("names no step"), "{e}");
        let values = BTreeMap::from([("".to_string(), Value::Bool(true))]);
        let present = BTreeSet::new();
        let s = Scope {
            constructed: None,
            now: Timestamp::from_ms(0).unwrap(),
            inputs: &BTreeMap::new(),
            steps: &values,
            present: &present,
            item: None,
            v2: true,
        };
        assert!(
            matches!(eval_ref("step:/a", &s), Err(Fault::Invalid(e)) if e.contains("names no step"))
        );
        assert_eq!(base_ref_v2("step:join/match"), "step:join");
        assert_eq!(base_ref_v2("input:a/b"), "input:a");
        // Version 1 keeps a step reference whole.
        assert_eq!(base_ref("step:join/match"), "step:join/match");
        assert_eq!(
            split_item(vec!["match/total".into(), "unit".into()]),
            vec!["match", "total", "unit"]
        );
    }

    #[test]
    fn member_ref_of_a_map_tests_its_keys() {
        let map_ty = Ty::Map {
            key_max: 8,
            value: Box::new(Ty::Text { max_bytes: 8 }),
            max: 4,
        };
        let tys = BTreeMap::from([
            ("codes".to_string(), map_ty),
            (
                "tier".to_string(),
                Ty::Enum(vec!["gold".into(), "eu".into()]),
            ),
            ("n".to_string(), Ty::Integer),
        ]);
        let step_tys = BTreeMap::new();
        let env = TyEnv {
            inputs: &tys,
            steps: &step_tys,
            item: None,
            v2: true,
        };
        let member = |value: &str| Predicate::MemberRef {
            value: Expr::Ref(value.into()),
            set: "input:codes".into(),
        };
        assert!(type_predicate(&member("input:tier"), &env).is_ok());
        assert!(
            type_predicate(&member("input:n"), &env).is_err(),
            "a non-text value"
        );

        let inputs = BTreeMap::from([
            (
                "codes".to_string(),
                Value::Map(BTreeMap::from([(
                    "eu".to_string(),
                    Value::Text("gold".into()),
                )])),
            ),
            ("tier".to_string(), Value::Enum("eu".into())),
            ("other".to_string(), Value::Enum("gold".into())),
        ]);
        let steps = BTreeMap::new();
        let present = BTreeSet::new();
        let s = Scope {
            constructed: None,
            now: Timestamp::from_ms(0).unwrap(),
            inputs: &inputs,
            steps: &steps,
            present: &present,
            item: None,
            v2: true,
        };
        assert_eq!(eval_predicate(&member("input:tier"), &s), Ok(true));
        assert_eq!(
            eval_predicate(&member("input:other"), &s),
            Ok(false),
            "a map's values are not its members"
        );
    }

    #[test]
    fn a_reference_that_names_nothing_is_refused() {
        for r in ["item:", "input:", "step:", "bind:"] {
            assert!(Ref::parse(r).is_err(), "{r}");
        }
        let item = Value::Null;
        let inputs = BTreeMap::new();
        let steps = BTreeMap::new();
        let present = BTreeSet::new();
        let s = Scope {
            constructed: None,
            now: Timestamp::from_ms(0).unwrap(),
            inputs: &inputs,
            steps: &steps,
            present: &present,
            item: Some(&item),
            v2: true,
        };
        assert_eq!(eval_ref("item:a", &s), Err(Fault::Absent("a".into())));
    }

    #[test]
    fn integers_and_decimals_do_not_mix_silently() {
        let inputs = BTreeMap::new();
        let steps = BTreeMap::new();
        let env = TyEnv {
            inputs: &inputs,
            steps: &steps,
            item: None,
            v2: false,
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
            v2: false,
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
