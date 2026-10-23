use super::{
    InstantiationCode as C, InstantiationRefusal, Site, SiteKind as K, Template, refusal, sites,
    values::*,
};
use rustev_contract::{decimal::Decimal, parameter::*};
use serde_json::Value;

/// Stage 2: validate declarations, scopes and every reference before overlay.
pub fn check_declarations(t: &Template) -> Result<Vec<ParameterDecl>, InstantiationRefusal> {
    let invalid = |name: &str, detail: String| {
        refusal(
            2,
            C::InvalidParameterization,
            format!("parameter:{name}"),
            detail,
        )
    };
    let mut declarations: Vec<ParameterDecl> = Vec::new();
    if let Some(raw) = t.json.get("parameters") {
        let a = raw
            .as_array()
            .ok_or_else(|| invalid("?", "parameters must be an array".into()))?;
        if a.len() > MAX_PARAMETERS {
            return Err(invalid("?", "at most 256 parameters".into()));
        }
        for v in a {
            let name = v["name"].as_str().unwrap_or("?");
            let d: ParameterDecl =
                serde_json::from_value(v.clone()).map_err(|e| invalid(name, e.to_string()))?;
            if !is_parameter_name(&d.name)
                || declarations.iter().any(|p| p.name == d.name)
                || !type_valid(&d.ty)
                || !bounds_valid(&d.ty, &d.bounds)
            {
                return Err(invalid(
                    name,
                    "invalid name, duplicate, type or bounds".into(),
                ));
            }
            let lock_ok = match d.lock {
                Lock::Fixed => {
                    matches!(d.default, ParamDefault::Value(_)) && d.bounds == Bounds::None
                }
                Lock::Free => d.bounds == Bounds::None,
                Lock::Bounded => {
                    d.ty != ParamType::Bool && (d.bounds != Bounds::None || enum_leaf(&d.ty))
                }
            };
            if !lock_ok {
                return Err(invalid(
                    name,
                    "invalid lock/default/bounds combination".into(),
                ));
            }
            if let ParamDefault::Value(v) = &d.default {
                let c = check_value(&d, v).map_err(|e| invalid(name, e.to_string()))?;
                check_bounds(&d, &c).map_err(|e| invalid(name, e.to_string()))?;
            }
            declarations.push(d);
        }
    }
    let variants: Vec<String> = t
        .json
        .get("variants")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| invalid("?", format!("variants: {e}")))?
        .unwrap_or_default();
    if variants.len() > MAX_VARIANTS
        || variants
            .iter()
            .enumerate()
            .any(|(i, n)| !is_variant_name(n) || variants[..i].contains(n))
    {
        return Err(invalid(
            "?",
            "invalid or duplicate variants (maximum 16)".into(),
        ));
    }
    for item in sites::items(&t.json) {
        let v = t.json.pointer(&item.pointer).unwrap_or(&Value::Null);
        let err = |s: &str| {
            refusal(
                2,
                C::InvalidParameterization,
                item.subject.clone(),
                format!("{}: {s}", item.pointer),
            )
        };
        if let Some(scope) = v.get("only_in")
            && !scope.as_array().is_some_and(|a| {
                !a.is_empty()
                    && a.iter()
                        .all(|v| v.as_str().is_some_and(|n| variants.iter().any(|s| s == n)))
            })
        {
            return Err(err(
                "only_in must name declared variants in a nonempty list",
            ));
        }
        if let Some(enabled) = v.get("enabled")
            && (item.input || !(enabled.is_boolean() || sites::reference(enabled).is_some()))
        {
            return Err(err("enabled must be bool or a bool parameter reference"));
        }
    }
    if t.sites.len() > MAX_REFERENCE_SITES {
        return Err(invalid("?", "at most 4096 reference sites".into()));
    }
    for site in &t.sites {
        let d = declarations
            .iter()
            .find(|d| d.name == site.parameter)
            .ok_or_else(|| {
                invalid(
                    &site.parameter,
                    format!("{}: undeclared parameter", site.pointer),
                )
            })?;
        if !compatible(d, site) {
            return Err(invalid(
                &d.name,
                format!(
                    "{}: declaration admits values incompatible with {:?}",
                    site.pointer, site.kind
                ),
            ));
        }
    }
    Ok(declarations)
}
fn scalar(t: &ParamType) -> bool {
    !matches!(
        t,
        ParamType::List { .. } | ParamType::Set { .. } | ParamType::Record { .. }
    )
}
fn interval(d: &ParameterDecl) -> Option<(Value, Value)> {
    if d.lock == Lock::Fixed
        && let ParamDefault::Value(v) = &d.default
    {
        return Some((v.clone(), v.clone()));
    }
    if let Bounds::Range([a, b]) = &d.bounds {
        return Some((a.clone(), b.clone()));
    }
    None
}
fn compatible(d: &ParameterDecl, s: &Site) -> bool {
    use ParamType as T;
    match s.kind {
        K::Expr | K::ProposalValue => scalar(&d.ty),
        K::ExactArgJson => true,
        K::Enabled => d.ty == T::Bool,
        K::Artifact => matches!(d.ty, T::Text { .. }),
        K::MinDeterminism => {
            matches!(&d.ty,T::Enum { options } if options.iter().all(|s| ["unspecified","tolerance","bitwise"].contains(&s.as_str())))
        }
        K::AdjustmentScale => {
            matches!(&d.ty,T::List { item, .. } if matches!(item.as_ref(),T::Text { .. } | T::Enum { .. }))
        }
        K::MemberSet => match &d.ty {
            T::Set { item, .. } => scalar(item),
            T::List { item, .. } if scalar(item) => {
                matches!(
                    d.bounds,
                    Bounds::List {
                        unique_items: true,
                        ..
                    }
                ) || (d.lock == Lock::Fixed
                    && matches!(&d.default,ParamDefault::Value(v) if check_value(d, v).is_ok_and(|c| c.0.as_array().is_some_and(|a| unique(a)))))
            }
            _ => false,
        },
        K::LimitInteger | K::DeadlineMs => {
            (d.ty == T::Integer
                && interval(d).is_some_and(|(a, _)| a.as_i64().is_some_and(|n| n >= 0)))
                || (s.kind == K::DeadlineMs && d.ty == T::DurationMs)
        }
        K::DistributionThreshold | K::DistributionTolerance => {
            if d.ty != T::Decimal {
                return false;
            }
            // Expectations have no probability ceiling. Probability thresholds
            // and tolerance must admit only the compiler's valid intervals.
            if s.pointer.ends_with("/expectation_at_least/value") {
                return true;
            }
            let Some((a, b)) = interval(d) else {
                return false;
            };
            let a = a.as_str().and_then(|s| Decimal::parse(s).ok());
            let b = b.as_str().and_then(|s| Decimal::parse(s).ok());
            let max = Decimal::parse(if s.kind == K::DistributionTolerance {
                "0.1"
            } else {
                "1"
            })
            .ok();
            matches!((a,b,max),(Some(a),Some(b),Some(max)) if b <= max && if s.kind == K::DistributionTolerance { a > Decimal::ZERO } else { a >= Decimal::ZERO })
        }
    }
}
