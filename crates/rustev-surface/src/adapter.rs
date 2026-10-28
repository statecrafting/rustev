//! The declarative task adapter, `rustev.task-adapter/1` (spec 006, 3.8):
//! how evaluation reads labels and decides whether a proposal is correct,
//! without writing Rust. Version 2 (spec 041, 3.1.2) adds the measure
//! declarations a version 3 evaluator configuration binds.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::bounded::{ParseLimits, parse_bounded};
use rustev_contract::canonical::{CanonicalError, tagged_digest};
use rustev_contract::ids::ContentDigest;
use rustev_contract::judgment::OutValue;
use rustev_contract::limits::DESCRIPTOR_V1;
use rustev_contract::{Document, DocumentError};
use rustev_eval::config::AdapterRules;
use rustev_eval::dataset::AdapterRef;
use rustev_eval::measures::{MeasuredAdapter, Measures, TASK_ADAPTER_V2};
use rustev_eval::metrics::TaskAdapter;
use serde::{Deserialize, Serialize};

pub const TASK_ADAPTER: &str = "rustev.task-adapter/1";
const MAX_RULES: usize = 64;
const MAX_LABELS: usize = 1024;
const MAX_TEXT: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskAdapterDoc {
    pub schema: String,
    pub name: String,
    pub version: String,
    pub labels: Labels,
    pub rules: Vec<Rule>,
    /// Required in version 2 and absent in version 1, whose bytes and
    /// rules digest are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measures: Option<Measures>,
}

/// A label is well formed when it equals one of `values` or starts with one
/// of `prefixes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Labels {
    pub values: Vec<String>,
    pub prefixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Rule {
    /// The proposal is correct for exactly this label.
    Label(LabelRule),
    /// The proposal is correct when a selected parameter value, mapped,
    /// equals the label.
    Param(ParamRule),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelRule {
    pub action: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamRule {
    pub action: String,
    pub param: String,
    pub select: Select,
    pub map: BTreeMap<String, String>,
    pub otherwise: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Select {
    Enum,
    Text,
    Integer,
    FirstRanked,
}

/// Either version parses; `SCHEMA` names version 1 only because the trait
/// holds one constant. Digests are tagged with the document's own schema,
/// so a version 1 digest is what it always was.
impl Document for TaskAdapterDoc {
    const SCHEMA: &'static str = TASK_ADAPTER;
    const LIMITS: ParseLimits = DESCRIPTOR_V1;
    fn schema(&self) -> &str {
        &self.schema
    }

    fn parse(bytes: &[u8]) -> Result<Self, DocumentError> {
        let doc: Self = parse_bounded(bytes, &Self::LIMITS).map_err(DocumentError::Parse)?;
        if doc.schema != TASK_ADAPTER && doc.schema != TASK_ADAPTER_V2 {
            return Err(DocumentError::Schema {
                expected: TASK_ADAPTER_V2,
                found: doc.schema.chars().take(80).collect(),
            });
        }
        Ok(doc)
    }

    fn record_digest(&self) -> Result<ContentDigest, CanonicalError> {
        let digest = tagged_digest(&self.schema, &self.record_canonical()?);
        Ok(ContentDigest::parse(&digest).expect("a tagged digest is a content digest"))
    }
}

impl Rule {
    fn action(&self) -> &str {
        match self {
            Rule::Label(r) => &r.action,
            Rule::Param(r) => &r.action,
        }
    }
}

fn text(what: &str, s: &str, max: usize) -> Result<(), String> {
    if s.is_empty() || s.len() > max {
        return Err(format!("{what} is 1 to {max} bytes"));
    }
    Ok(())
}

impl TaskAdapterDoc {
    /// Validate the adapter (spec 006, 3.8.1; spec 041, 3.4.1).
    pub fn check(&self) -> Result<(), String> {
        match (self.schema == TASK_ADAPTER_V2, &self.measures) {
            (true, Some(m)) => m.check().map_err(|e| format!("measures: {e}"))?,
            (true, None) => return Err(format!("{TASK_ADAPTER_V2} requires measures")),
            (false, Some(_)) => return Err(format!("{TASK_ADAPTER} has no measures")),
            (false, None) => {}
        }
        text("name", &self.name, 64)?;
        text("version", &self.version, 64)?;
        let l = &self.labels;
        if l.values.is_empty() && l.prefixes.is_empty() {
            return Err("labels need at least one value or prefix".into());
        }
        if l.values.len() + l.prefixes.len() > MAX_LABELS {
            return Err(format!("at most {MAX_LABELS} label values and prefixes"));
        }
        for v in l.values.iter().chain(&l.prefixes) {
            text("a label value or prefix", v, MAX_TEXT)?;
        }
        if self.rules.len() > MAX_RULES {
            return Err(format!("at most {MAX_RULES} rules"));
        }
        let mut actions = BTreeSet::new();
        for r in &self.rules {
            text("an action", r.action(), MAX_TEXT)?;
            if !actions.insert(r.action()) {
                return Err(format!("two rules for action {:?}", r.action()));
            }
            match r {
                Rule::Label(x) => text("a rule label", &x.label, MAX_TEXT)?,
                Rule::Param(x) => {
                    text("a parameter name", &x.param, MAX_TEXT)?;
                    if x.map.len() > MAX_LABELS {
                        return Err(format!("a map has at most {MAX_LABELS} entries"));
                    }
                    for (k, v) in &x.map {
                        text("a map key", k, MAX_TEXT)?;
                        text("a map value", v, MAX_TEXT)?;
                    }
                    if let Some(o) = &x.otherwise {
                        text("otherwise", o, MAX_TEXT)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// The value a parameter rule selects, or none when the parameter is
/// missing or of another type.
fn selected(select: Select, v: Option<&OutValue>) -> Option<String> {
    match (select, v?) {
        (Select::Enum, OutValue::Enum(s)) | (Select::Text, OutValue::Text(s)) => Some(s.clone()),
        (Select::Integer, OutValue::Integer(i)) => Some(i.to_string()),
        (Select::FirstRanked, OutValue::Ranking(r)) => {
            r.entries.first().map(|e| e.candidate.clone())
        }
        _ => None,
    }
}

impl TaskAdapter for TaskAdapterDoc {
    fn adapter(&self) -> AdapterRef {
        AdapterRef {
            name: self.name.clone(),
            version: self.version.clone(),
        }
    }

    /// Always bound: the document is the whole definition (spec 014,
    /// 3.1.4), and the digest is over the canonical bytes `eval` writes as
    /// `adapter.json`, tagged with the document's schema, so a version 2
    /// digest covers `measures` (spec 041, 3.1.2).
    fn rules(&self) -> AdapterRules {
        let bytes = self
            .canonical()
            .expect("a checked adapter document has a canonical form");
        AdapterRules::Bound(
            ContentDigest::parse(&tagged_digest(&self.schema, &bytes))
                .expect("a tagged digest is a content digest"),
        )
    }

    fn check_label(&self, label: &str) -> Result<(), String> {
        let l = &self.labels;
        if l.values.iter().any(|v| v == label)
            || l.prefixes.iter().any(|p| label.starts_with(p.as_str()))
        {
            Ok(())
        } else {
            Err(format!("{label:?} is not a label of adapter {}", self.name))
        }
    }

    fn correct(&self, action: &str, params: &BTreeMap<String, OutValue>, label: &str) -> bool {
        let Some(rule) = self.rules.iter().find(|r| r.action() == action) else {
            return false;
        };
        match rule {
            Rule::Label(r) => r.label == label,
            Rule::Param(r) => {
                let Some(value) = selected(r.select, params.get(&r.param)) else {
                    return false;
                };
                let mapped = if r.map.is_empty() {
                    Some(value)
                } else {
                    r.map.get(&value).cloned().or_else(|| r.otherwise.clone())
                };
                mapped.is_some_and(|m| m == label)
            }
        }
    }
}

/// A version 1 document declares no measures; `eval` refuses it under a
/// version 3 configuration before evaluation.
impl MeasuredAdapter for TaskAdapterDoc {
    fn measures(&self) -> Measures {
        self.measures.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::judgment::{RankEntry, Ranking};

    fn support() -> TaskAdapterDoc {
        TaskAdapterDoc {
            schema: TASK_ADAPTER.into(),
            name: "support-routing".into(),
            version: "1".into(),
            labels: Labels {
                values: ["billing", "integration_defect", "account_access", "other"]
                    .map(String::from)
                    .to_vec(),
                prefixes: vec![],
            },
            rules: vec![Rule::Param(ParamRule {
                action: "route_ticket".into(),
                param: "queue".into(),
                select: Select::Enum,
                map: [
                    ("billing", "billing"),
                    ("billing-priority", "billing"),
                    ("engineering", "integration_defect"),
                    ("identity", "account_access"),
                ]
                .into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
                otherwise: Some("other".into()),
            })],
            measures: None,
        }
    }

    fn params(v: &[(&str, OutValue)]) -> BTreeMap<String, OutValue> {
        v.iter().map(|(k, x)| (k.to_string(), x.clone())).collect()
    }

    #[test]
    fn a_parameter_rule_maps_and_falls_back_to_otherwise() {
        let a = support();
        a.check().unwrap();
        let q = |s: &str| params(&[("queue", OutValue::Enum(s.into()))]);
        assert!(a.correct("route_ticket", &q("billing-priority"), "billing"));
        assert!(!a.correct("route_ticket", &q("billing-priority"), "other"));
        assert!(a.correct("route_ticket", &q("general"), "other"));
        // A missing parameter, another type or another action is never correct.
        assert!(!a.correct("route_ticket", &params(&[]), "other"));
        let text = params(&[("queue", OutValue::Text("billing".into()))]);
        assert!(!a.correct("route_ticket", &text, "billing"));
        assert!(!a.correct("escalate", &q("billing"), "billing"));
        // Without `otherwise`, an unmapped value is never correct.
        let mut b = support();
        if let Rule::Param(r) = &mut b.rules[0] {
            r.otherwise = None;
        }
        assert!(!b.correct("route_ticket", &q("general"), "other"));
    }

    #[test]
    fn label_rules_first_ranked_and_label_shapes() {
        let a = TaskAdapterDoc {
            schema: TASK_ADAPTER.into(),
            name: "lodging".into(),
            version: "1".into(),
            labels: Labels {
                values: vec!["none".into()],
                prefixes: vec!["c-".into()],
            },
            rules: vec![
                Rule::Param(ParamRule {
                    action: "present_ranked_lodging".into(),
                    param: "ranking".into(),
                    select: Select::FirstRanked,
                    map: BTreeMap::new(),
                    otherwise: None,
                }),
                Rule::Label(LabelRule {
                    action: "no_eligible_lodging".into(),
                    label: "none".into(),
                }),
            ],
            measures: None,
        };
        a.check().unwrap();
        let entry = |c: &str, p| RankEntry {
            candidate: c.into(),
            position: p,
            score: 1.0,
            components: BTreeMap::new(),
        };
        let ranking = |e: Vec<RankEntry>| {
            params(&[(
                "ranking",
                OutValue::Ranking(Ranking {
                    entries: e,
                    excluded: vec![],
                }),
            )])
        };
        let r = ranking(vec![entry("c-1", 1), entry("c-2", 2)]);
        assert!(a.correct("present_ranked_lodging", &r, "c-1"));
        assert!(!a.correct("present_ranked_lodging", &r, "c-2"));
        assert!(!a.correct("present_ranked_lodging", &ranking(vec![]), "c-1"));
        assert!(a.correct("no_eligible_lodging", &params(&[]), "none"));
        assert!(!a.correct("no_eligible_lodging", &params(&[]), "c-1"));
        assert!(a.check_label("c-9").is_ok() && a.check_label("none").is_ok());
        assert!(a.check_label("x-1").is_err());
    }

    #[test]
    fn invalid_adapters_are_refused() {
        let mut a = support();
        a.rules.push(a.rules[0].clone());
        assert!(a.check().unwrap_err().contains("two rules"));
        let mut a = support();
        a.labels.values.clear();
        assert!(a.check().is_err());
        let mut a = support();
        a.name = "x".repeat(65);
        assert!(a.check().is_err());
        // Unknown fields are refused by the parser.
        let bytes = br#"{"schema":"rustev.task-adapter/1","name":"n","version":"1","labels":{"values":["a"],"prefixes":[]},"rules":[{"action":"x","label":"a","extra":1}]}"#;
        assert!(TaskAdapterDoc::parse(bytes).is_err());
    }

    /// Spec 041, 3.1.2: version 2 is version 1 plus required `measures`,
    /// and its rules digest is tagged with its own schema.
    #[test]
    fn version_2_requires_measures_and_tags_its_digest() {
        let v1 = support();
        let mut v2 = support();
        v2.schema = TASK_ADAPTER_V2.into();
        assert!(v2.check().unwrap_err().contains("requires measures"));
        v2.measures = Some(Measures::default());
        v2.check().unwrap();
        let mut v1_with = support();
        v1_with.measures = Some(Measures::default());
        assert!(v1_with.check().unwrap_err().contains("has no measures"));
        // Version 1 bytes carry no `measures` member, so its digests are
        // unchanged; version 2's differ.
        let v1_bytes = v1.canonical().unwrap();
        assert!(!String::from_utf8_lossy(&v1_bytes).contains("measures"));
        let tagged = |schema: &str, bytes: &[u8]| {
            AdapterRules::Bound(ContentDigest::parse(&tagged_digest(schema, bytes)).unwrap())
        };
        assert_eq!(v1.rules(), tagged(TASK_ADAPTER, &v1_bytes));
        assert_eq!(
            v2.rules(),
            tagged(TASK_ADAPTER_V2, &v2.canonical().unwrap())
        );
        assert_eq!(
            v2.record_digest().unwrap().to_string(),
            tagged_digest(TASK_ADAPTER_V2, &v2.record_canonical().unwrap())
        );
        // Both versions parse; another schema does not.
        let round = TaskAdapterDoc::parse(&v2.canonical().unwrap()).unwrap();
        assert_eq!(round, v2);
        assert_eq!(round.measures(), Measures::default());
        assert_eq!(v1.measures(), Measures::default());
        let mut v3 = v2.clone();
        v3.schema = "rustev.task-adapter/3".into();
        assert!(TaskAdapterDoc::parse(&v3.canonical().unwrap()).is_err());
    }
}
