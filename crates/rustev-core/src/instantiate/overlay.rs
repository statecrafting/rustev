use super::{
    InstantiationCode as C, InstantiationRefusal, Selection, refusal,
    template::parse_profile,
    values::{ValueError, check_bounds, check_value},
};
use rustev_contract::{
    Identified,
    parameter::{Lock, ParamDefault, ParameterDecl},
    plan::{ParameterSource, ProfileEntry},
    profile::Profile,
    version::VersionRange,
};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ParameterValue {
    pub name: String,
    pub value: Value,
    pub source: ParameterSource,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Overlay {
    pub chain: Vec<ProfileEntry>,
    pub parameters: Vec<ParameterValue>,
}
fn label(p: &Profile) -> String {
    format!("{}@{}", p.profile, p.version)
}
/// Stage 3. Unknown names are checked first in byte order within each
/// profile; declared names then follow declaration order. Chain order is
/// root first. Defaults and profile entries are values, never references.
pub fn resolve_profiles(
    selection: &Selection,
    declarations: &[ParameterDecl],
    leaf: Option<&[u8]>,
    bases: &[&[u8]],
) -> Result<Overlay, InstantiationRefusal> {
    let has_leaf = leaf.is_some();
    let leaf = leaf.map(parse_profile).transpose()?;
    let bases = bases
        .iter()
        .map(|b| parse_profile(b))
        .collect::<Result<Vec<_>, _>>()?;
    let subject = leaf
        .as_ref()
        .map(|p| format!("profile:{}", label(p)))
        .unwrap_or_else(|| "profile:?@?".into());
    if bases.len() > 64 {
        return Err(refusal(
            3,
            C::InvalidProfile,
            subject,
            "at most 64 base profiles",
        ));
    }
    let mut supplied = Vec::new();
    if let Some(p) = leaf {
        supplied.push(p);
    }
    supplied.extend(bases);
    let ids = supplied
        .iter()
        .map(|p| {
            p.id()
                .map_err(|e| refusal(3, C::InvalidProfile, format!("profile:{}", label(p)), e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (i, p) in supplied.iter().enumerate() {
        if supplied[..i]
            .iter()
            .enumerate()
            .any(|(j, q)| label(p) == label(q) && ids[i] != ids[j])
        {
            return Err(refusal(
                3,
                C::ProfileDuplicate,
                format!("profile:{}", label(p)),
                "same name and version have different ProfileIds",
            ));
        }
    }
    let mut chain = Vec::new();
    if has_leaf {
        let mut i = 0;
        loop {
            let p = &supplied[i];
            if chain.contains(&i) {
                let names = chain
                    .iter()
                    .chain(std::iter::once(&i))
                    .map(|&j| label(&supplied[j]))
                    .collect::<Vec<_>>()
                    .join(" -> ");
                return Err(refusal(
                    3,
                    C::ProfileCycle,
                    format!("profile:{}", label(p)),
                    names,
                ));
            }
            chain.push(i);
            if chain.len() > 8 {
                return Err(refusal(
                    3,
                    C::ProfileChainTooDeep,
                    format!("profile:{}", label(p)),
                    "profile chain exceeds 8",
                ));
            }
            if p.extends == "none" {
                break;
            }
            i = supplied
                .iter()
                .position(|q| label(q) == p.extends)
                .ok_or_else(|| {
                    refusal(
                        3,
                        C::ProfileMissing,
                        format!("profile:{}", p.extends),
                        "extends target not supplied",
                    )
                })?;
        }
    }
    chain.reverse();
    let t = &selection.template;
    let candidates = selection
        .candidate_versions
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut values = declarations
        .iter()
        .map(|d| match &d.default {
            ParamDefault::None => Ok(None),
            ParamDefault::Value(v) => check_value(d, v)
                .map(|v| Some((v.0, ParameterSource::Default)))
                .map_err(|e| {
                    refusal(
                        2,
                        C::InvalidParameterization,
                        format!("parameter:{}", d.name),
                        e,
                    )
                }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut entries = Vec::new();
    for &i in &chain {
        let p = &supplied[i];
        let name = label(p);
        let range = p
            .definition
            .version
            .parse::<VersionRange>()
            .map_err(|e| refusal(3, C::InvalidProfile, format!("profile:{name}"), e))?;
        if p.definition.name != t.name || !range.satisfies(&t.version) {
            return Err(refusal(
                3,
                C::DefinitionUnsatisfied,
                format!("profile:{name}"),
                format!(
                    "does not admit {}@{}; candidates: [{candidates}]",
                    t.name, t.version
                ),
            ));
        }
        for key in p.values.keys() {
            if !declarations.iter().any(|d| &d.name == key) {
                return Err(refusal(
                    3,
                    C::ParameterUnknown,
                    format!("parameter:{key}"),
                    format!("profile {name}: undeclared parameter"),
                ));
            }
        }
        for (j, d) in declarations.iter().enumerate() {
            let Some(v) = p.values.get(&d.name) else {
                continue;
            };
            let err = |code, detail: String| {
                refusal(
                    3,
                    code,
                    format!("parameter:{}", d.name),
                    format!("profile {name}, value {v}: {detail}"),
                )
            };
            if d.lock == Lock::Fixed {
                return Err(err(C::ParameterLocked, "fixed parameter".into()));
            }
            let checked = check_value(d, v).map_err(|e| err(C::ParameterType, e.to_string()))?;
            check_bounds(d, &checked).map_err(|e| {
                err(
                    match e {
                        ValueError::IllTyped(_) => C::ParameterType,
                        ValueError::OutOfBounds(_) => C::ParameterOutOfBounds,
                    },
                    e.to_string(),
                )
            })?;
            values[j] = Some((checked.0, ParameterSource::Profile(name.clone())));
        }
        entries.push(ProfileEntry {
            profile: p.profile.clone(),
            version: p.version.clone(),
            id: ids[i].clone(),
        });
    }
    let parameters = declarations
        .iter()
        .zip(values)
        .map(|(d, v)| {
            let (value, source) = v.ok_or_else(|| {
                refusal(
                    3,
                    C::ParameterUnbound,
                    format!("parameter:{}", d.name),
                    "no default or profile value",
                )
            })?;
            Ok(ParameterValue {
                name: d.name.clone(),
                value,
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Overlay {
        chain: entries,
        parameters,
    })
}
