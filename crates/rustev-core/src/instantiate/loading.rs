//! Self-contained validation: profile documents are not needed to load a plan.
use super::*;
use rustev_contract::{
    Document,
    canonical::canonical_bytes,
    parameter::{Lock, ParamDefault},
    plan::{Instantiation, ParameterSource, Plan, ProfileChain},
    schema,
};

pub(crate) fn validate_record(plan: &Plan, record: &Instantiation) -> Result<(), String> {
    if plan.schema != schema::PLAN_V3 {
        return Err("instantiation requires rustev.plan/3".into());
    }
    let bytes = canonical_bytes(&record.template).map_err(|e| e.to_string())?;
    let template = parse_template(&bytes).map_err(|e| e.to_string())?;
    if template.id != record.template_id || template.schema != record.template_schema {
        return Err("instantiation template identity or schema mismatch".into());
    }
    let declarations = check_declarations(&template).map_err(|e| e.to_string())?;
    let variant = check_variant(
        &template,
        match &record.variant {
            Variant::None => None,
            Variant::Named(n) => Some(n),
        },
    )
    .map_err(|e| e.to_string())?;
    if record.parameters.len() != declarations.len() {
        return Err("instantiation parameter count mismatch".into());
    }
    let chain = match &record.profile {
        ProfileChain::None => vec![],
        ProfileChain::Chain(c) if (1..=8).contains(&c.len()) => c.clone(),
        _ => return Err("invalid instantiation profile chain length".into()),
    };
    let mut labels = std::collections::BTreeSet::new();
    for entry in &chain {
        if !rustev_contract::profile::is_profile_name(&entry.profile)
            || entry
                .version
                .parse::<rustev_contract::version::Version>()
                .is_err()
            || !labels.insert(format!("{}@{}", entry.profile, entry.version))
        {
            return Err("invalid instantiation profile chain identity".into());
        }
    }
    let mut parameters = vec![];
    for (d, p) in declarations.iter().zip(&record.parameters) {
        if d.name != p.name {
            return Err("instantiation parameter declaration order mismatch".into());
        }
        let value = check_value(d, &p.value).map_err(|e| format!("parameter:{}: {e}", d.name))?;
        check_bounds(d, &value).map_err(|e| format!("parameter:{}: {e}", d.name))?;
        if value.0 != p.value {
            return Err(format!("parameter:{}: noncanonical value", d.name));
        }
        if d.lock == Lock::Fixed && p.source != ParameterSource::Default {
            return Err(format!("parameter:{}: fixed source is not default", d.name));
        }
        match &p.source {
            ParameterSource::Default => {
                let ParamDefault::Value(default) = &d.default else {
                    return Err(format!("parameter:{}: no default", d.name));
                };
                if check_value(d, default).map_err(|e| e.to_string())?.0 != p.value {
                    return Err(format!("parameter:{}: value differs from default", d.name));
                }
            }
            ParameterSource::Profile(name) if !labels.contains(name) => {
                return Err(format!("parameter:{}: unknown source profile", d.name));
            }
            _ => {}
        }
        parameters.push(ParameterValue {
            name: p.name.clone(),
            value: value.0,
            source: p.source.clone(),
        });
    }
    let result = emit::emit(Analysis {
        selection: Selection {
            template,
            candidate_versions: vec![],
        },
        declarations,
        overlay: Overlay { chain, parameters },
        variant,
    })
    .map_err(|e| e.to_string())?;
    if result.instantiation != *record {
        return Err("instantiation sites or record mismatch".into());
    }
    if result.definition.canonical().map_err(|e| e.to_string())?
        != plan.definition.canonical().map_err(|e| e.to_string())?
    {
        return Err("instantiated definition mismatch".into());
    }
    Ok(())
}
