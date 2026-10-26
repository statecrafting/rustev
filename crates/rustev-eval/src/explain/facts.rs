use super::*;
use rustev_contract::definition::{Candidates, ItemSource, StepBody};
use rustev_contract::judgment::Derivation;
use rustev_contract::redaction::{FieldClass, lineage_class};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) const CORE: &str = "/core/recorded";
pub(super) const J: &str = "/core/recorded/judgment";
pub(super) struct Builder<'a> {
    pub set: &'a evidence::Set,
    pub policy: Option<&'a RedactionPolicy>,
    pub facts: Vec<Fact>,
    pub total: usize,
    pub limit: usize,
    pub bytes: usize,
    pub bounds: bounds::Bounds,
}
impl Builder<'_> {
    pub fn list(&mut self, count: usize) -> bool {
        self.total = self.total.saturating_add(count);
        self.total <= self.limit
    }
    pub fn put(&mut self, mut f: Fact) -> Result<FactId, ExplainError> {
        f.id = format!("f{}", self.facts.len() + 1);
        for cite in &f.cites {
            let role = cite.as_str().split_once(':').map(|x| x.0).unwrap_or("");
            debug_assert!(
                self.set
                    .docs
                    .get(role)
                    .and_then(|v| cite.resolve(v))
                    .is_some()
            );
        }
        self.bytes += serde_json::to_vec(&f)
            .map_err(|e| ExplainError::parse("fact", e))?
            .len();
        if self.facts.len() >= self.bounds.facts || self.bytes > self.bounds.bytes {
            return Err(bounds::too_large());
        }
        let id = FactId(f.id.clone());
        self.facts.push(f);
        Ok(id)
    }
    pub fn unavailable(&mut self, claim: Claim, u: Unavailable) -> Result<FactId, ExplainError> {
        let cites = if let Unavailable::Retention { item, .. } = &u {
            vec![item.clone()]
        } else {
            vec![]
        };
        self.put(Fact {
            id: String::new(),
            claim,
            value: Value::Null,
            availability: Availability::NotAvailable(u),
            derivation: None,
            lineage: None,
            truncated: None,
            cites,
        })
    }
    pub fn facet(&mut self, claim: Claim, facet: Facet) -> Result<FactId, ExplainError> {
        self.unavailable(claim, Unavailable::NotRecorded { facet: Some(facet) })
    }
    pub fn copy(
        &mut self,
        claim: Claim,
        role: &str,
        pointer: &str,
        class: FieldClass,
        composite: bool,
    ) -> Result<FactId, ExplainError> {
        let Some(v) = self.set.get(role, pointer) else {
            let u = self.set.absent.get(role).cloned().unwrap_or_else(|| {
                if role == "record" && pointer.starts_with(J) {
                    self.set
                        .judgment_absent()
                        .unwrap_or(Unavailable::NotInRecord {})
                } else {
                    Unavailable::NotInRecord {}
                }
            });
            return self.unavailable(claim, u);
        };
        let path = EvidencePath::parse(&format!("{role}:{pointer}"))
            .map_err(|e| ExplainError::parse(role, e))?;
        let mut f = Fact {
            id: String::new(),
            claim,
            value: v.clone(),
            availability: Availability::Available,
            derivation: None,
            lineage: None,
            truncated: None,
            cites: vec![path],
        };
        if role == "record"
            && pointer.starts_with(&format!("{CORE}/inputs/"))
            && pointer.ends_with("/status")
            && let Some(provenance) = v.pointer("/resolved/provenance").and_then(Value::as_str)
            && redact::withheld(self.policy, FieldClass::Enum).is_none()
        {
            f.derivation = Some(FactDerivation {
                class: if provenance == "model-derived" {
                    Derivation::ModelDerived
                } else {
                    Derivation::ExactDerived
                },
                bound: DerivationBound::Exact,
            });
        }
        match redact::apply(&mut f.value, class, self.policy, composite) {
            Ok(t) => f.truncated = t,
            Err(u) => {
                f.value = Value::Null;
                f.availability = Availability::NotAvailable(u);
            }
        }
        self.put(f)
    }
    // Projections and integer aggregates cite each recorded source they read.
    pub fn derived(
        &mut self,
        claim: Claim,
        mut value: Value,
        mut cites: Vec<EvidencePath>,
        class: FieldClass,
        composite: bool,
    ) -> Result<FactId, ExplainError> {
        cites.sort();
        cites.dedup();
        let (availability, truncated) =
            match redact::apply_query(&mut value, class, self.policy, composite) {
                Ok(t) => (Availability::Available, t),
                Err(u) => {
                    value = Value::Null;
                    (Availability::NotAvailable(u), None)
                }
            };
        self.put(Fact {
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
    pub fn judgment(
        &mut self,
        claim: Claim,
        member: &str,
        class: FieldClass,
        composite: bool,
    ) -> Result<FactId, ExplainError> {
        self.copy(claim, "record", &format!("{J}/{member}"), class, composite)
    }
    pub fn lineage(&self) -> Option<FactLineage> {
        let v = self.set.get("record", &format!("{J}/lineage"))?;
        let mut inputs = strings(&v["inputs"]);
        let mut steps = strings(&v["steps"]);
        inputs.sort();
        inputs.dedup();
        steps.sort();
        steps.dedup();
        Some(FactLineage {
            inputs,
            steps,
            bound: LineageBound::Judgment,
        })
    }
    pub fn value_class(&self, lineage: Option<&FactLineage>, boolean: bool) -> FieldClass {
        let inputs = lineage
            .into_iter()
            .flat_map(|l| &l.inputs)
            .filter_map(|name| {
                let a = self
                    .set
                    .get("record", &format!("{CORE}/inputs"))?
                    .as_array()?;
                let input = a.iter().find(|v| v["field"].as_str() == Some(name))?;
                serde_json::from_value(input.pointer("/status/resolved/provenance")?.clone()).ok()
            });
        lineage_class(boolean, inputs)
    }
    pub fn annotate(
        &mut self,
        id: &FactId,
        linked: Option<&str>,
        exact_lineage: bool,
    ) -> Result<(), ExplainError> {
        let mut lineage = self.lineage();
        let mut derivation = self
            .set
            .get("record", &format!("{J}/derivation"))
            .and_then(|v| serde_json::from_value::<Derivation>(v.clone()).ok())
            .map(|class| FactDerivation {
                class,
                bound: DerivationBound::Judgment,
            });
        let mut cites = Vec::new();
        for member in ["derivation", "lineage"] {
            let pointer = format!("{J}/{member}");
            if self.set.get("record", &pointer).is_some() {
                cites.push(
                    EvidencePath::parse(&format!("record:{pointer}"))
                        .map_err(|e| ExplainError::parse("record", e))?,
                );
            }
        }
        if let Some(step) = linked
            && let Some(plan) = &self.set.plan
        {
            lineage = closure(plan, step).or(lineage);
            if let Some((i, v)) = self
                .set
                .get("record", &format!("{CORE}/steps"))
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter().enumerate().find(|(_, v)| {
                        v["step"].as_str() == Some(step)
                            && v["instance"].as_array().is_some_and(Vec::is_empty)
                    })
                })
            {
                derivation = v
                    .pointer("/status/resolved/derivation")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .map(|class| FactDerivation {
                        class,
                        bound: DerivationBound::Exact,
                    });
                cites.push(
                    EvidencePath::parse(&format!("record:{CORE}/steps/{i}/status"))
                        .map_err(|e| ExplainError::parse("record", e))?,
                );
            }
        }

        if exact_lineage && let Some(l) = &mut lineage {
            l.bound = LineageBound::Exact;
        }

        if redact::withheld(self.policy, FieldClass::PlanMetadata).is_some() {
            lineage = None;
        }
        if redact::withheld(self.policy, FieldClass::Enum).is_some() {
            derivation = None;
        }
        if let Some(f) = self.facts.iter_mut().find(|f| f.id == id.0) {
            let before = serde_json::to_vec(&f)
                .map_err(|e| ExplainError::parse("fact", e))?
                .len();
            f.derivation = derivation;
            f.lineage = lineage;
            f.cites.extend(cites);
            f.cites.sort();
            f.cites.dedup();
            let after = serde_json::to_vec(&f)
                .map_err(|e| ExplainError::parse("fact", e))?
                .len();
            self.bytes = self.bytes.saturating_sub(before).saturating_add(after);
            if self.bytes > self.bounds.bytes {
                return Err(bounds::too_large());
            }
        }
        Ok(())
    }
}
pub(super) fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}
pub(super) fn escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}
pub(super) fn matched(set: &evidence::Set) -> Option<(usize, String, String)> {
    set.get("record", &format!("{J}/trace"))?
        .as_array()?
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, v)| {
            let s = v.as_str()?;
            if let Some(n) = s
                .strip_prefix("on_unresolved[")
                .and_then(|s| s.strip_suffix(']'))
                .and_then(|s| s.parse::<usize>().ok())
            {
                Some((i, s.into(), format!("/definition/policy/on_unresolved/{n}")))
            } else {
                let id = s
                    .strip_prefix("rule:")?
                    .strip_suffix(":unhandled")
                    .unwrap_or(s.strip_prefix("rule:")?);
                let n = set
                    .get("plan", "/definition/policy/rules")
                    .and_then(Value::as_array)
                    .and_then(|a| a.iter().position(|v| v["id"].as_str() == Some(id)));
                Some((
                    i,
                    s.into(),
                    n.map(|n| format!("/definition/policy/rules/{n}"))
                        .unwrap_or_else(|| "/definition/policy/rules/missing".into()),
                ))
            }
        })
}
pub(super) fn closure(plan: &rustev_contract::plan::Plan, root: &str) -> Option<FactLineage> {
    let mut steps = BTreeSet::new();
    let mut inputs = BTreeSet::new();
    let mut pending = vec![root.to_string()];
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(decl) = plan.definition.steps.iter().find(|s| s.id == id) else {
            continue;
        };
        let refs = match &decl.body {
            StepBody::Acquire(_)
            | StepBody::Widen(_)
            | StepBody::Generate(_)
            | StepBody::Extract(_)
            | StepBody::AcceptModelDerived(_) => return None,
            // Spec 040, 3.5 item 5: undecodable closure uses judgment lineage, never a partial closure.
            StepBody::Exact(e) => rustev_core::ops::decode_v2(&e.op, e.version, &e.args)?
                .ok()?
                .refs(),
            StepBody::Semantic(s) => {
                let mut refs = s.project.clone();
                if let Candidates::From(r) = &s.candidates {
                    refs.push(r.clone());
                }
                for f in &s.for_each {
                    refs.push(f.over.clone());
                    if let ItemSource::List { list, .. } = &f.items {
                        refs.push(list.clone());
                    }
                }
                refs
            }
            StepBody::Corroborate(c) => {
                let mut refs = vec![c.claim.clone()];
                for check in &c.by {
                    rustev_core::expr::predicate_refs(&check.predicate, &mut refs);
                }
                refs
            }
        };
        for r in refs {
            let base = rustev_core::expr::base_ref_v2(&r);
            if let Some(i) = base.strip_prefix("input:") {
                inputs.insert(i.to_string());
            }
            if let Some(s) = base.strip_prefix("step:") {
                steps.insert(s.to_string());
                pending.push(s.to_string());
            }
        }
    }
    Some(FactLineage {
        inputs: inputs.into_iter().collect(),
        steps: steps.into_iter().collect(),
        bound: LineageBound::Plan,
    })
}
