use super::{InstantiationCode as C, InstantiationRefusal, Site, refusal, sites};
use rustev_contract::{
    Document, Identified,
    bounded::parse_bounded,
    canonical::{canonical_bytes, tagged_digest},
    definition::Definition,
    ids::DefinitionId,
    limits::{DEFINITION_V1, DESCRIPTOR_V1},
    profile::{Profile, ProfileDefinition},
    version::{Version, VersionRange},
};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Template {
    pub json: Value,
    pub canonical: Vec<u8>,
    pub id: DefinitionId,
    pub schema: String,
    pub name: String,
    pub version: Version,
    pub sites: Vec<Site>,
}
impl Template {
    pub fn subject(&self) -> String {
        format!("definition:{}@{}", self.name, self.version)
    }
}
#[derive(Debug, Clone)]
pub struct Selection {
    pub template: Template,
    pub candidate_versions: Vec<Version>,
}
/// Stage 1 parsing proves the non-template document grammar with typed
/// placeholders. Exact args remain JSON, just as in Definition::parse.
pub fn parse_template(bytes: &[u8]) -> Result<Template, InstantiationRefusal> {
    let v: Value = parse_bounded(bytes, &DEFINITION_V1)
        .map_err(|e| refusal(1, C::InvalidDefinitionDocument, "definition:?@?", e))?;
    let subject = format!(
        "definition:{}@{}",
        v["name"].as_str().unwrap_or("?"),
        v["version"].as_str().unwrap_or("?")
    );
    let err = |e: String| refusal(1, C::InvalidDefinitionDocument, subject.clone(), e);
    let sites = sites::reference_sites(&v)?;
    let mut plain = v.clone();
    let mut has_members = !sites.is_empty();
    for key in ["parameters", "variants"] {
        if let Some(o) = plain.as_object_mut() {
            has_members |= o.remove(key).is_some();
        }
    }
    for site in &sites {
        if let Some(at) = plain.pointer_mut(&site.pointer) {
            *at = sites::placeholder(site.kind);
        }
    }
    for item in sites::items(&v) {
        if let Some(o) = plain
            .pointer_mut(&item.pointer)
            .and_then(Value::as_object_mut)
        {
            has_members |= o.remove("only_in").is_some();
            if !item.input {
                has_members |= o.remove("enabled").is_some();
            }
        }
    }
    if v["schema"] == "rustev.definition/1" && has_members {
        return Err(err(
            "category 1: spec 034 members require rustev.definition/2".into(),
        ));
    }
    let encoded = canonical_bytes(&plain).map_err(|e| err(e.to_string()))?;
    let def = Definition::parse(&encoded).map_err(|e| err(e.to_string()))?;
    let version = def
        .version
        .parse::<Version>()
        .map_err(|e| err(e.to_string()))?;
    let canonical = canonical_bytes(&v).map_err(|e| err(e.to_string()))?;
    Ok(Template {
        id: <Definition as Identified>::wrap(tagged_digest(&def.schema, &canonical)),
        json: v,
        canonical,
        schema: def.schema,
        name: def.name,
        version,
        sites,
    })
}
pub(super) fn parse_profile(bytes: &[u8]) -> Result<Profile, InstantiationRefusal> {
    Profile::parse(bytes).map_err(|e| refusal(3, C::InvalidProfile, "profile:?@?", e))
}
/// Stage 1 selection. The leaf's binding must be parsed to select; malformed
/// profiles retain their stage 3 refusal. Every definition is parsed first.
pub fn select_template(
    definitions: &[&[u8]],
    profile: Option<&[u8]>,
    pin: Option<&str>,
) -> Result<Selection, InstantiationRefusal> {
    let all = definitions
        .iter()
        .map(|b| parse_template(b))
        .collect::<Result<Vec<_>, _>>()?;
    if !(1..=64).contains(&all.len()) || (profile.is_none() && all.len() != 1) {
        let mut supplied: Vec<_> = all.iter().map(Template::subject).collect();
        supplied.sort();
        return Err(refusal(
            1,
            C::InvalidDefinitionDocument,
            "definition:?@?",
            format!(
                "expected 1..=64 definitions, exactly one without a profile; {} supplied: [{}]",
                all.len(),
                supplied.join(", ")
            ),
        ));
    }
    // Read only the selection binding here. Full profile validation belongs
    // after declaration validation, so bad values cannot mask stage 2 errors.
    let profile = profile
        .map(|bytes| {
            let v: Value = parse_bounded(bytes, &DESCRIPTOR_V1)
                .map_err(|e| refusal(3, C::InvalidProfile, "profile:?@?", e))?;
            let subject = format!(
                "profile:{}@{}",
                v["profile"].as_str().unwrap_or("?"),
                v["version"].as_str().unwrap_or("?")
            );
            let binding: ProfileDefinition = serde_json::from_value(v["definition"].clone())
                .map_err(|e| refusal(3, C::InvalidProfile, subject.clone(), e))?;
            Ok::<_, InstantiationRefusal>((binding, subject))
        })
        .transpose()?;
    let candidates: Vec<_> = all
        .into_iter()
        .filter(|t| profile.as_ref().is_none_or(|p| p.0.name == t.name))
        .collect();
    let mut versions: Vec<_> = candidates.iter().map(|t| t.version.clone()).collect();
    versions.sort();
    versions.dedup();
    let list = versions
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let subject = profile
        .as_ref()
        .map(|p| p.1.clone())
        .unwrap_or_else(|| candidates[0].subject());
    let err = |code, detail: &str| {
        refusal(
            1,
            code,
            subject.clone(),
            format!("{detail}; candidates: [{list}]"),
        )
    };
    for (i, t) in candidates.iter().enumerate() {
        if candidates[..i]
            .iter()
            .any(|p| p.version == t.version && p.id != t.id)
        {
            return Err(err(
                C::DefinitionAmbiguous,
                "same version has different DefinitionIds",
            ));
        }
    }
    let range = profile
        .as_ref()
        .map(|p| p.0.version.parse::<VersionRange>())
        .transpose()
        .map_err(|e| refusal(3, C::InvalidProfile, subject.clone(), e))?;
    let pin = pin
        .map(str::parse::<Version>)
        .transpose()
        .map_err(|e| err(C::DefinitionUnsatisfied, &format!("invalid pin: {e}")))?;
    if pin
        .as_ref()
        .is_some_and(|p| range.as_ref().is_some_and(|r| !r.satisfies(p)))
    {
        return Err(err(
            C::DefinitionPinOutsideRange,
            "pin does not satisfy range",
        ));
    }
    let template = candidates
        .into_iter()
        .filter(|t| {
            range.as_ref().is_none_or(|r| r.satisfies(&t.version))
                && pin.as_ref().is_none_or(|p| p == &t.version)
        })
        .max_by(|a, b| a.version.cmp(&b.version))
        .ok_or_else(|| {
            err(
                C::DefinitionUnsatisfied,
                "no supplied candidate satisfies binding",
            )
        })?;
    Ok(Selection {
        template,
        candidate_versions: versions,
    })
}
