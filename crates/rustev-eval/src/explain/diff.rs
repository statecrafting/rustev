//! Ordered comparisons of evidence, before policy redaction.
use super::*;
use facts::{CORE, J};
use rustev_contract::definition::{Cond, OutcomeDecl, ParamSource};
use rustev_contract::judgment::Derivation;
use rustev_contract::redaction::FieldClass;
use rustev_contract::{Document, limits::REPLAY_V1};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Operand {
    value: Value,
    compared: Value,
    digest: bool,
    state: Value,
    cites: Vec<EvidencePath>,
    derivation: Option<FactDerivation>,
    class: FieldClass,
}
fn prefix(u: &mut Unavailable, side: &str) {
    if let Unavailable::Retention { item, .. } = u {
        *item =
            EvidencePath::parse(&format!("{side}.{}", item.as_str())).expect("valid prefixed path");
    }
}
fn read(set: &evidence::Set, side: &str, role: &str, path: &str, class: FieldClass) -> Operand {
    let value = set.get(role, path);
    let mut cites = vec![];
    let state = if value.is_some() {
        cites.push(EvidencePath::parse(&format!("{side}.{role}:{path}")).expect("evidence path"));
        json!("present")
    } else if let Some(mut u) = set.absent.get(role).cloned().or_else(|| {
        (role == "record" && path.starts_with(CORE))
            .then(|| set.judgment_absent())
            .flatten()
    }) {
        prefix(&mut u, side);
        if let Unavailable::Retention { item, .. } = &u {
            cites.push(item.clone());
        }
        json!({"not_available": u})
    } else {
        json!("absent")
    };
    Operand {
        digest: false,
        compared: value.cloned().unwrap_or(Value::Null),
        value: value.cloned().unwrap_or(Value::Null),
        state,
        cites,
        derivation: None,
        class,
    }
}
fn array<'a>(set: &'a evidence::Set, role: &str, path: &str) -> &'a [Value] {
    set.get(role, path)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn key(v: &Value) -> (String, Vec<String>) {
    (
        v["step"].as_str().unwrap_or_default().into(),
        facts::strings(&v["instance"]),
    )
}
fn subject(k: &(String, Vec<String>)) -> String {
    format!("step:{}#{}", k.0, k.1.join(","))
}
fn builder(set: &evidence::Set) -> facts::Builder<'_> {
    facts::Builder {
        set,
        policy: None,
        facts: vec![],
        total: 0,
        limit: usize::MAX,
        bytes: 0,
        bounds: bounds::Bounds::default(),
    }
}
fn annotate(op: &mut Operand, set: &evidence::Set, input: Option<&Value>, step: Option<&Value>) {
    if op.state != "present" {
        return;
    }
    op.derivation = if let Some(input) = input {
        Some(FactDerivation {
            class: if input
                .pointer("/status/resolved/provenance")
                .and_then(Value::as_str)
                == Some("model-derived")
            {
                Derivation::ModelDerived
            } else {
                Derivation::ExactDerived
            },
            bound: DerivationBound::Exact,
        })
    } else if set.plan.is_some() && step.is_some() {
        step.and_then(|s| s.pointer("/status/resolved/derivation"))
            .and_then(|d| serde_json::from_value(d.clone()).ok())
            .map(|class| FactDerivation {
                class,
                bound: DerivationBound::Exact,
            })
    } else {
        set.get("record", &format!("{J}/derivation"))
            .and_then(|d| serde_json::from_value(d.clone()).ok())
            .map(|class| FactDerivation {
                class,
                bound: DerivationBound::Judgment,
            })
    };
}
struct Walk<'a> {
    sets: [&'a evidence::Set; 2],
    options: &'a ExplainOptions<'a>,
    facts: Vec<Fact>,
    differences: Vec<FactId>,
    observations: Vec<FactId>,
    not_compared: Vec<FactId>,
    earlier: Vec<(String, FactId)>,
    total: u64,
    why: Option<Divergence>,
}
impl Walk<'_> {
    fn emit(
        &mut self,
        name: &str,
        mut ops: [Operand; 2],
        claim: Claim,
        why: Divergence,
        refs: &BTreeSet<String>,
    ) {
        if claim != Claim::OutputNotCompared
            && ops[0].compared == ops[1].compared
            && ((ops[0].digest && ops[1].digest)
                || ops[0].state == ops[1].state
                || (ops[0].state.get("not_available").is_some()
                    && ops[1].state.get("not_available").is_some()
                    && ops[0].state["not_available"]["reason"]
                        == ops[1].state["not_available"]["reason"]
                    && ops[0].state["not_available"]["availability"]
                        == ops[1].state["not_available"]["availability"]))
        {
            return;
        }
        if claim == Claim::Difference {
            self.total += 1;
            if self.total > 10_000 {
                return;
            }
            self.why.get_or_insert(why);
        }
        let derivation = if claim == Claim::Difference
            && (name.starts_with("input:") || name.starts_with("step:") || name == "judgment")
        {
            let ds: Vec<_> = ops.iter().filter_map(|o| o.derivation.as_ref()).collect();
            (!ds.is_empty()).then(|| FactDerivation {
                class: if ds.iter().all(|d| d.class == Derivation::ExactDerived) {
                    Derivation::ExactDerived
                } else {
                    Derivation::MixedDerived
                },
                bound: if ds.iter().all(|d| d.bound == DerivationBound::Exact) {
                    DerivationBound::Exact
                } else {
                    DerivationBound::Judgment
                },
            })
        } else {
            None
        };
        let mut truncated = None;
        for op in &mut ops {
            if op.state == "present" {
                match redact::apply(&mut op.value, op.class, self.options.redaction, true) {
                    Ok(t) => truncated = t.or(truncated),
                    Err(u) => {
                        op.value = Value::Null;
                        op.state = json!({"not_available": u});
                    }
                }
            }
        }
        let mut name_value = json!(name);
        if name.starts_with("input:") || name.starts_with("step:") {
            // Subjects include authored names and instance identifiers.
            let class = if name.starts_with("step:")
                && name.split_once('#').is_some_and(|(_, i)| !i.is_empty())
            {
                FieldClass::Id
            } else {
                FieldClass::PlanMetadata
            };
            if let Err(u) = redact::apply(&mut name_value, class, self.options.redaction, false) {
                name_value = json!({"not_available":u});
            }
            if let Some(u) = redact::withheld(self.options.redaction, FieldClass::PlanMetadata) {
                name_value = json!({"not_available":u});
            }
        }
        let mut value = json!({"subject":name_value, "a":ops[0].value, "b":ops[1].value, "a_state":ops[0].state, "b_state":ops[1].state});
        if claim == Claim::OutputNotCompared {
            value.as_object_mut().unwrap().remove("a");
            value.as_object_mut().unwrap().remove("b");
        }
        if claim == Claim::Difference {
            let (follows, unexplained) = if self.sets.iter().any(|s| s.plan.is_none()) {
                (
                    json!({"not_available":Unavailable::PlanNotSupplied {}}),
                    Value::Null,
                )
            } else if self.sets[0].side.plan_id != self.sets[1].side.plan_id {
                (
                    json!(
                        self.earlier
                            .first()
                            .map(|(_, id)| vec![id])
                            .unwrap_or_default()
                    ),
                    Value::Null,
                )
            } else {
                let ids: Vec<_> = self
                    .earlier
                    .iter()
                    .filter(|(s, _)| refs.contains(s))
                    .map(|(_, id)| id)
                    .collect();
                let unexplained =
                    ids.is_empty() && (name.starts_with("step:") || name == "judgment");
                (json!(ids), json!(unexplained))
            };
            value["follows_from"] = follows;
            value["unexplained"] = unexplained;
        }
        let id = FactId(format!("f{}", self.facts.len() + 1));
        let mut cites: Vec<_> = ops.into_iter().flat_map(|o| o.cites).collect();
        cites.sort();
        cites.dedup();
        self.facts.push(Fact {
            id: id.0.clone(),
            claim,
            value,
            availability: Availability::Available,
            derivation,
            lineage: None,
            truncated,
            cites,
        });
        match claim {
            Claim::Difference => {
                self.earlier.push((name.into(), id.clone()));
                self.differences.push(id);
            }
            Claim::ObservationDifference => self.observations.push(id),
            _ => self.not_compared.push(id),
        }
    }
    fn pair(&self, role: &str, paths: [&str; 2], class: FieldClass) -> [Operand; 2] {
        [
            read(self.sets[0], "a", role, paths[0], class),
            read(self.sets[1], "b", role, paths[1], class),
        ]
    }
    fn compare(&mut self) -> Result<(), ExplainError> {
        for (member, class, why) in [
            ("plan_id", FieldClass::PlanMetadata, Divergence::PlanChanged),
            (
                "snapshot_id",
                FieldClass::ContentDigest,
                Divergence::SnapshotChanged,
            ),
            (
                "evaluation_time_ms",
                FieldClass::Number,
                Divergence::EvaluationTimeChanged,
            ),
        ] {
            let path = if member == "plan_id" {
                format!("/{member}")
            } else {
                format!("{CORE}/{member}")
            };
            self.emit(
                member,
                self.pair("record", [&path, &path], class),
                Claim::Difference,
                why,
                &BTreeSet::new(),
            );
        }
        let inputs = self
            .sets
            .map(|s| array(s, "record", &format!("{CORE}/inputs")));
        let input_indices = inputs.map(|a| {
            let mut indices = BTreeMap::new();
            for (i, v) in a.iter().enumerate() {
                if let Some(field) = v["field"].as_str() {
                    indices.entry(field).or_insert(i);
                }
            }
            indices
        });
        let fields: BTreeSet<_> = input_indices
            .iter()
            .flat_map(|a| a.keys().copied())
            .collect();
        for field in fields {
            let indices = input_indices.each_ref().map(|a| a.get(field).copied());
            for member in ["presence", "status"] {
                let paths = indices.map(|i| {
                    format!(
                        "{CORE}/inputs/{}/{member}",
                        i.map(|i| i.to_string()).unwrap_or_else(|| "missing".into())
                    )
                });
                let mut ops = self.pair("record", [&paths[0], &paths[1]], FieldClass::Enum);
                for i in 0..2 {
                    annotate(
                        &mut ops[i],
                        self.sets[i],
                        indices[i].map(|n| &inputs[i][n]),
                        None,
                    );
                }
                self.emit(
                    &format!("input:{field}"),
                    ops,
                    Claim::Difference,
                    Divergence::InputChanged,
                    &["snapshot_id".into(), "evaluation_time_ms".into()].into(),
                );
            }
        }
        let steps = self
            .sets
            .map(|s| array(s, "record", &format!("{CORE}/steps")));
        let requests = self.sets.map(|s| array(s, "record", "/requests"));
        let mut keys = vec![];
        let mut seen = BTreeSet::new();
        for side in 0..2 {
            for v in steps[side].iter().chain(requests[side]) {
                let k = key(v);
                if seen.insert(k.clone()) {
                    keys.push(k);
                }
            }
        }
        for k in &keys {
            let name = subject(k);
            let refs = self.references(Some(&k.0), &keys)?;
            let indices = steps.map(|a| a.iter().position(|v| key(v) == *k));
            let request_indices = requests.map(|a| a.iter().position(|v| key(v) == *k));
            if request_indices.iter().any(Option::is_some) {
                self.supplied(
                    k,
                    request_indices,
                    [0, 1].map(|i| indices[i].map(|n| &steps[i][n])),
                    &refs,
                )?;
            }
            let paths = indices.map(|i| {
                format!(
                    "{CORE}/steps/{}/status",
                    i.map(|i| i.to_string()).unwrap_or_else(|| "missing".into())
                )
            });
            let mut ops = self.pair("record", [&paths[0], &paths[1]], FieldClass::Enum);
            for i in 0..2 {
                annotate(
                    &mut ops[i],
                    self.sets[i],
                    None,
                    indices[i].map(|n| &steps[i][n]),
                );
            }
            let mut refs = refs;
            refs.insert(name.clone());
            self.emit(
                &name,
                ops,
                Claim::Difference,
                Divergence::StepChanged,
                &refs,
            );
        }
        let refs = self.references(None, &keys)?;
        for member in ["outcome", "derivation", "lineage", "notices", "trace"] {
            let path = format!("{J}/{member}");
            let mut ops = self.pair(
                "record",
                [&path, &path],
                if member == "lineage" || member == "trace" {
                    FieldClass::PlanMetadata
                } else {
                    FieldClass::Enum
                },
            );
            for (i, op) in ops.iter_mut().enumerate() {
                if member == "outcome" {
                    op.class = builder(self.sets[i])
                        .value_class(builder(self.sets[i]).lineage().as_ref(), false);
                }
                annotate(op, self.sets[i], None, None);
            }
            self.emit(
                "judgment",
                ops,
                Claim::Difference,
                Divergence::OutcomeChanged,
                &refs,
            );
        }
        for (member, class) in [
            ("decision_id", FieldClass::Id),
            ("timing", FieldClass::Number),
            ("cost", FieldClass::Number),
        ] {
            let path = format!("/{member}");
            self.emit(
                member,
                self.pair("record", [&path, &path], class),
                Claim::ObservationDifference,
                Divergence::StepChanged,
                &BTreeSet::new(),
            );
        }
        for k in &keys {
            let indices = requests.map(|a| a.iter().position(|v| key(v) == *k));
            let counts = [0, 1].map(|i| {
                indices[i]
                    .and_then(|n| requests[i][n]["attempts"].as_array())
                    .map_or(0, Vec::len)
            });
            for n in 0..counts[0].max(counts[1]) {
                for (member, class) in [
                    ("attempt_id", FieldClass::Id),
                    ("dispatched_ms", FieldClass::Number),
                    ("ended_ms", FieldClass::Number),
                    ("cost", FieldClass::Number),
                ] {
                    let paths = indices.map(|i| {
                        format!(
                            "/requests/{}/attempts/{n}/{member}",
                            i.map(|i| i.to_string()).unwrap_or_else(|| "missing".into())
                        )
                    });
                    self.emit(
                        &format!("{}/{n}/{member}", subject(k)),
                        self.pair("record", [&paths[0], &paths[1]], class),
                        Claim::ObservationDifference,
                        Divergence::StepChanged,
                        &BTreeSet::new(),
                    );
                }
            }
        }
        Ok(())
    }
    fn references(
        &self,
        root: Option<&str>,
        keys: &[(String, Vec<String>)],
    ) -> Result<BTreeSet<String>, ExplainError> {
        let Some(plan) = &self.sets[0].plan else {
            return Ok(BTreeSet::new());
        };
        let mut refs = vec![];
        if let Some(root) = root {
            refs.push(format!("step:{root}"));
        } else {
            let p = &plan.definition.policy;
            for r in &p.rules {
                cond_refs(&r.when, &mut refs);
                match &r.then {
                    OutcomeDecl::Propose { params, .. } => {
                        for p in params {
                            if let ParamSource::Ref(r) = &p.value {
                                refs.push(r.clone());
                            }
                        }
                    }
                    OutcomeDecl::MissingEvidenceFrom { step } => refs.push(format!("step:{step}")),
                    _ => {}
                }
            }
            for a in &p.adjustments {
                cond_refs(&a.when, &mut refs);
            }
            for h in &p.on_unresolved {
                refs.push(h.target.clone());
            }
        }
        let mut result = BTreeSet::new();
        for r in refs {
            let r = rustev_core::expr::base_ref_v2(&r);
            if let Some(s) = r.strip_prefix("step:") {
                let Some(l) = facts::closure(plan, s).or_else(|| builder(self.sets[0]).lineage())
                else {
                    continue;
                };
                for i in l.inputs {
                    result.insert(format!("input:{i}"));
                }
                let mut steps: BTreeSet<_> = l.steps.into_iter().collect();
                if root.is_none() {
                    steps.insert(s.into());
                }
                for k in keys {
                    if steps.contains(&k.0) {
                        result.insert(subject(k));
                    }
                }
            } else {
                result.insert(r.to_string());
            }
        }
        Ok(result)
    }
    fn supplied(
        &mut self,
        k: &(String, Vec<String>),
        indices: [Option<usize>; 2],
        steps: [Option<&Value>; 2],
        refs: &BTreeSet<String>,
    ) -> Result<(), ExplainError> {
        let name = subject(k);
        let requests = self.sets.map(|s| array(s, "record", "/requests"));
        let supplies = self.sets.map(|s| array(s, "bundle", "/supplies"));
        let supply_indices = supplies.map(|s| s.iter().position(|v| key(v) == *k));
        let bundles = self.sets.map(|s| s.side.form == EvidenceForm::Bundle);
        let paths = indices.map(|i| {
            format!(
                "/requests/{}/result",
                i.map(|i| i.to_string()).unwrap_or_else(|| "missing".into())
            )
        });
        let mut ops = self.pair("record", [&paths[0], &paths[1]], FieldClass::Enum);
        let mut identities = [BTreeMap::new(), BTreeMap::new()];
        for i in 0..2 {
            if let Some(n) = indices[i] {
                let attempts = requests[i][n]["attempts"].as_array();
                if let Some(last) = attempts.and_then(|a| a.len().checked_sub(1)) {
                    for m in ["backend_id", "artifact"] {
                        let path = format!("/requests/{n}/attempts/{last}/{m}");
                        let o = read(
                            self.sets[i],
                            ["a", "b"][i],
                            "record",
                            &path,
                            FieldClass::PlanMetadata,
                        );
                        identities[i].insert(m, o);
                    }
                }
            }
        }
        let backend_changed = ["backend_id", "artifact"].iter().any(|m| {
            identities[0].get(m).map(|o| &o.value) != identities[1].get(m).map(|o| &o.value)
        });
        let same_backend = !backend_changed && identities.iter().all(|m| m.len() == 2);
        let outputs = [0, 1].map(|i| ops[i].value.get("output").is_some());
        let mixed = bundles[0] != bundles[1];
        for i in 0..2 {
            if bundles[i]
                && (!mixed || outputs.iter().all(|v| *v))
                && let Some(n) = supply_indices[i]
            {
                let side = ["a", "b"][i];
                let mode = supplies[i][n]["value"]["retention"]
                    .as_object()
                    .and_then(|m| m.keys().next())
                    .map(String::as_str)
                    .unwrap_or("digest_only");
                let digest = read(
                    self.sets[i],
                    side,
                    "bundle",
                    &format!("/supplies/{n}/value/retention/{mode}/digest"),
                    FieldClass::ContentDigest,
                );
                ops[i] = read(
                    self.sets[i],
                    side,
                    &format!("supply/{n}"),
                    "",
                    FieldClass::ThirdParty,
                );
                if !mixed {
                    ops[i].compared = digest.value;
                    ops[i].digest = true;
                }
                ops[i].cites.extend(digest.cites);
            }
            annotate(&mut ops[i], self.sets[i], None, steps[i]);
            let b = builder(self.sets[i]);
            let lineage = self.sets[i]
                .plan
                .as_ref()
                .and_then(|p| facts::closure(p, &k.0));
            ops[i].class = b.value_class(lineage.as_ref().or(b.lineage().as_ref()), false);
        }
        // The current run schema records a target, not the raw output. It must
        // never be compared with a retained document or its digest.
        let missing_mixed_output = mixed && outputs.iter().all(|v| *v);
        if missing_mixed_output {
            for i in 0..2 {
                if !bundles[i] {
                    ops[i].value = Value::Null;
                    ops[i].state =
                        json!({"not_available":Unavailable::NotRecorded { facet: None }});
                }
            }
            if !backend_changed {
                self.emit(
                    &name,
                    ops.clone(),
                    Claim::OutputNotCompared,
                    Divergence::StepChanged,
                    refs,
                );
            }
        } else {
            let why = if backend_changed {
                Divergence::BackendChanged
            } else if outputs.iter().all(|v| *v) && same_backend && bundles.iter().all(|b| *b) {
                Divergence::BackendOutputChanged
            } else {
                Divergence::RequestResultChanged
            };
            // If either document is unread, both digest paths remain citable.
            if !mixed
                && bundles.iter().all(|v| *v)
                && ops.iter().any(|o| o.state.get("not_available").is_some())
            {
                let unavailable = ops
                    .iter()
                    .find(|o| o.state.get("not_available").is_some())
                    .map(|o| o.state.clone())
                    .unwrap();
                for o in &mut ops {
                    o.value = Value::Null;
                    o.derivation = None;
                    if o.state == "present" {
                        // Name the item that prevents showing the value pair;
                        // do not claim the readable side's item has expired.
                        o.state = unavailable.clone();
                    }
                }
            }
            self.emit(&name, ops, Claim::Difference, why, refs);
        }
        if bundles.iter().all(|b| *b) {
            for (m, class) in [
                ("target", FieldClass::Number),
                ("attempt_id", FieldClass::Id),
            ] {
                let paths = supply_indices.map(|n| {
                    format!(
                        "/supplies/{}/{m}",
                        n.map(|n| n.to_string()).unwrap_or_else(|| "missing".into())
                    )
                });
                let mut ops = self.pair("bundle", [&paths[0], &paths[1]], class);
                for i in 0..2 {
                    annotate(&mut ops[i], self.sets[i], None, steps[i]);
                }
                self.emit(
                    &name,
                    ops,
                    Claim::Difference,
                    Divergence::RequestResultChanged,
                    refs,
                );
            }
        }
        // Backend identity is distinct from observation-only attempt ids.
        for m in ["backend_id", "artifact"] {
            let absent = || Operand {
                value: Value::Null,
                compared: Value::Null,
                digest: false,
                state: json!("absent"),
                cites: vec![],
                derivation: None,
                class: FieldClass::PlanMetadata,
            };
            let mut ops = [
                identities[0].remove(m).unwrap_or_else(absent),
                identities[1].remove(m).unwrap_or_else(absent),
            ];
            for i in 0..2 {
                annotate(&mut ops[i], self.sets[i], None, steps[i]);
            }
            self.emit(
                &name,
                ops,
                Claim::Difference,
                Divergence::BackendChanged,
                refs,
            );
        }
        Ok(())
    }
}
fn cond_refs(c: &Cond, out: &mut Vec<String>) {
    match c {
        Cond::Always => {}
        Cond::TopLabel { step, .. }
        | Cond::MassAtLeast { step, .. }
        | Cond::TopMassBelow { step, .. }
        | Cond::ExpectationAtLeast { step, .. } => out.push(format!("step:{step}")),
        Cond::Exact(p) => rustev_core::expr::predicate_refs(p, out),
        Cond::Empty(r) | Cond::Nonempty(r) | Cond::ProvenanceWithin { reference: r, .. } => {
            out.push(r.clone())
        }
        Cond::All(cs) | Cond::Any(cs) => {
            for c in cs {
                cond_refs(c, out);
            }
        }
        Cond::Not(c) => cond_refs(c, out),
    }
}

/// Compare retained evidence without executing either decision.
pub fn explain_diff(
    a: &Evidence<'_>,
    b: &Evidence<'_>,
    options: &ExplainOptions<'_>,
) -> Result<Explanation, ExplainError> {
    if let Some(p) = options.redaction {
        p.validate()
            .map_err(|e| ExplainError::parse("redaction", e))?;
    }
    let (a, b) = (evidence::Set::read(a)?, evidence::Set::read(b)?);
    build(&a, &b, options, REPLAY_V1.max_bytes)
}

fn build(
    a: &evidence::Set,
    b: &evidence::Set,
    options: &ExplainOptions<'_>,
    max_bytes: usize,
) -> Result<Explanation, ExplainError> {
    let mut sides = vec![a.side.clone(), b.side.clone()];
    sides[1].name = SideName::B;
    for (i, side) in sides.iter_mut().enumerate() {
        for item in [&mut side.decision_id, &mut side.plan_id] {
            if let Item::Withheld(w) = item {
                prefix(&mut w.not_available, ["a", "b"][i]);
            }
        }
        redact::side(side, options.redaction);
    }
    let mut w = Walk {
        sets: [a, b],
        options,
        facts: vec![],
        differences: vec![],
        observations: vec![],
        not_compared: vec![],
        earlier: vec![],
        total: 0,
        why: None,
    };
    let withheld = redact::withheld(options.redaction, FieldClass::Enum);
    if withheld.is_none() {
        w.compare()?;
    }
    let base = Explanation {
        schema: rustev_contract::schema::EXPLANATION.into(),
        explainer: EXPLAINER.into(),
        facet_sources: vec![FacetSource {
            name: Facet::ServedIdentity,
            spec: "040-decision-explanation".into(),
            path: "supply/<n>:/served".into(),
        }],
        sides,
        redaction: options.redaction.map(|p| Redaction {
            policy: p.policy_id(),
        }),
        query: Query::Diff,
        answer: Answer::Diff(DiffAnswer::Identical {
            observations: vec![],
            not_compared: vec![],
        }),
        truncation: None,
        facts: vec![],
    };
    let keyed = w.differences.first().map(|id| id.0.as_str());
    let total_lists = w.facts.len() - usize::from(keyed.is_some());
    let construct = |keep: usize| -> Result<Explanation, ExplainError> {
        let mut doc = base.clone();
        let mut lists = 0;
        let mut ids = BTreeMap::new();
        for f in &w.facts {
            if Some(f.id.as_str()) != keyed {
                lists += 1;
                if lists > keep {
                    continue;
                }
            }
            let mut f = f.clone();
            let new_id = format!("f{}", doc.facts.len() + 1);
            ids.insert(f.id.clone(), FactId(new_id.clone()));
            f.id = new_id;
            doc.facts.push(f);
        }
        for f in &mut doc.facts {
            if let Some(refs) = f
                .value
                .get_mut("follows_from")
                .and_then(Value::as_array_mut)
            {
                for id in refs {
                    if let Some(new) = id.as_str().and_then(|id| ids.get(id)) {
                        *id = json!(new);
                    }
                }
            }
        }
        let kept = |a: &[FactId]| {
            a.iter()
                .filter_map(|id| ids.get(&id.0).cloned())
                .collect::<Vec<_>>()
        };
        let observations = kept(&w.observations);
        let not_compared = kept(&w.not_compared);
        doc.answer = Answer::Diff(if let Some(u) = &withheld {
            DiffAnswer::Withheld {
                redacted: u.clone(),
            }
        } else if let Some(first) = w.differences.first() {
            let differences = kept(&w.differences);
            DiffAnswer::Diverged {
                first: ids[&first.0].clone(),
                why: w.why.unwrap(),
                truncated: w.total - differences.len() as u64,
                total: w.total,
                differences,
                observations,
                not_compared,
            }
        } else {
            DiffAnswer::Identical {
                observations,
                not_compared,
            }
        });
        // The first divergence is keyed and is not counted as a list member.
        doc.truncation = (keep < total_lists).then_some(Truncation {
            total: total_lists as u64,
        });
        let bytes = doc.record_canonical().map_err(|_| bounds::too_large())?;
        if bytes.len() > max_bytes
            || doc.facts.len() > MAX_FACTS
            || rustev_contract::bounded::scan(&bytes, &REPLAY_V1).is_err()
        {
            return Err(bounds::too_large());
        }
        doc.validate()
            .map_err(|e| ExplainError::parse("explanation", e))?;
        Ok(doc)
    };
    let mut best = construct(0)?;
    let (mut low, mut high) = (0, total_lists.min(MAX_FACTS - usize::from(keyed.is_some())));
    // Most answers fit at the fact cap. Avoid rebuilding every smaller
    // prefix when the largest possible prefix already satisfies all bounds.
    if high > 0 {
        match construct(high) {
            Ok(doc) => return Ok(doc),
            Err(e) if e.code == RefusalCode::ExplanationTooLarge => high -= 1,
            Err(e) => return Err(e),
        }
    }
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        match construct(mid) {
            Ok(doc) => {
                best = doc;
                low = mid;
            }
            Err(e) if e.code == RefusalCode::ExplanationTooLarge => high = mid - 1,
            Err(e) => return Err(e),
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_diff_byte_limit_counts_omitted_differences() {
        let digest = format!("sha256:{}", "0".repeat(64));
        let set = |presence: &str| {
            let inputs: Vec<_> = (0..8)
                .map(|n| json!({"field":format!("field-{n}"), "presence":presence}))
                .collect();
            evidence::Set {
                docs: [(
                    "record".into(),
                    json!({"core":{"recorded":{"inputs":inputs}}}),
                )]
                .into(),
                absent: Default::default(),
                plan: None,
                side: serde_json::from_value(json!({"name":"a", "form":"record",
                    "record":{"digest":digest}, "plan":null, "bundle":null,
                    "capture":null, "decision_id":"d", "plan_id":digest}))
                .unwrap(),
            }
        };
        let (a, b) = (set("present"), set("missing"));
        let options = ExplainOptions::default();
        let full = build(&a, &b, &options, REPLAY_V1.max_bytes).unwrap();
        assert_eq!(full.facts.len(), 8);
        assert!(full.truncation.is_none());
        // Below both count caps: only the byte budget can omit a difference.
        let max_bytes = full.record_canonical().unwrap().len() - 1;
        let doc = build(&a, &b, &options, max_bytes).unwrap();
        doc.validate().unwrap();
        assert!(doc.record_canonical().unwrap().len() <= max_bytes);
        assert!(doc.truncation.is_some());
        let Answer::Diff(DiffAnswer::Diverged {
            total,
            truncated,
            differences,
            ..
        }) = &doc.answer
        else {
            panic!("expected a divergence");
        };
        assert_eq!(*total, 8);
        assert!(!differences.is_empty());
        assert_eq!(*truncated, *total - differences.len() as u64);
        assert!(*truncated > 0);
    }
}
