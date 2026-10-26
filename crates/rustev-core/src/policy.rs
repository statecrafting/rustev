//! Selection policy evaluation (spec 002, 3.11.5): handlers in order, then the
//! first matching rule, then adjustments in order. A pure function of the
//! plan, the validated context, the step values and the evaluation time.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::definition::ProvenanceClass;
use rustev_contract::definition::{
    Cond, HandlerAction, OutcomeDecl, ParamSource, PolicyDecl, ReasonSet,
};
use rustev_contract::judgment::{OutValue, Outcome, Unresolved};

use crate::expr::{Scope, eval_predicate, predicate_refs};
use crate::sem::SemValue;
use crate::value::Value;

/// A policy-visible value.
pub(crate) enum Seen<'a> {
    Exact(&'a Value),
    Projected(Value),
    Semantic(&'a SemValue),
}

/// What the policy can look up.
pub(crate) trait View {
    fn skipped(&self, _r: &str) -> bool {
        false
    }
    /// `Ok(value)`, or the unresolved reason, for `input:`/`step:` refs.
    fn get(&self, r: &str) -> Result<Seen<'_>, Unresolved>;
    fn scope(&self) -> Scope<'_>;
    /// The provenance classes of a resolved `input:`/`step:` reference: an
    /// input's accepted entry's class, or a plan 3 step value's provenance
    /// set (spec 033, 3.4.7).
    fn provenance(&self, r: &str) -> Result<Vec<ProvenanceClass>, Unresolved>;
    /// The input or step a reference reads.
    fn base(&self, r: &str) -> String {
        crate::expr::base_ref(r)
    }
}

pub(crate) struct PolicyResult {
    pub outcome: Outcome,
    /// References read, in reading order, deduplicated.
    pub reads: Vec<String>,
    pub trace: Vec<String>,
    /// The site that produced the outcome (spec 033, 3.5.2): `rule:<id>` or
    /// `handler:<n>`; `None` when no declaration decided it.
    pub site: Option<String>,
    /// The deciding handler's position, when a handler decided.
    pub handler: Option<usize>,
}

struct Run<'v, V: View> {
    view: &'v V,
    unmet: BTreeSet<String>,
    reads: Vec<String>,
}

impl<V: View> Run<'_, V> {
    fn read(&mut self, r: &str) {
        let r = self.view.base(r);
        if !self.reads.contains(&r) {
            self.reads.push(r);
        }
    }

    /// `Ok(None)` when the reference is handled `as_unmet`.
    fn sem(&mut self, step: &str) -> Result<Option<&SemValue>, Unresolved> {
        let r = format!("step:{step}");
        if self.unmet.contains(&r) {
            return Ok(None);
        }
        self.read(&r);
        match self.view.get(&r)? {
            Seen::Semantic(v) => Ok(Some(v)),
            Seen::Exact(_) | Seen::Projected(_) => {
                Err(Unresolved::invalid([], format!("{r} is not semantic")))
            }
        }
    }

    fn cond(&mut self, c: &Cond) -> Result<bool, Unresolved> {
        Ok(match c {
            Cond::Always => true,
            Cond::TopLabel { step, label } => match self.sem(step)? {
                Some(v) => v.top().is_some_and(|(l, _)| &l == label),
                None => false,
            },
            Cond::MassAtLeast {
                step,
                option,
                threshold,
                ..
            } => match self.sem(step)? {
                Some(v) => v
                    .mass(option)
                    .is_some_and(|m| m >= threshold.to_f64_nearest()),
                None => false,
            },
            Cond::TopMassBelow {
                step, threshold, ..
            } => match self.sem(step)? {
                Some(v) => v
                    .top()
                    .and_then(|(_, m)| m)
                    .is_some_and(|m| m < threshold.to_f64_nearest()),
                None => false,
            },
            Cond::ExpectationAtLeast { step, value } => match self.sem(step)? {
                Some(SemValue::Ordinal(o)) => o.expectation() >= value.to_f64_nearest(),
                _ => false,
            },
            Cond::Exact(p) => {
                let mut refs = Vec::new();
                predicate_refs(p, &mut refs);
                let refs: Vec<String> = refs
                    .iter()
                    .filter(|r| r.starts_with("input:") || r.starts_with("step:"))
                    .map(|r| self.view.base(r))
                    .collect();
                if refs.iter().any(|r| self.unmet.contains(r)) {
                    return Ok(false);
                }
                for r in &refs {
                    self.read(r);
                    if !self.view.skipped(r) {
                        self.view.get(r)?;
                    }
                }
                eval_predicate(p, &self.view.scope())
                    .map_err(|f| Unresolved::invalid([], format!("{f:?}")))?
            }
            Cond::Empty(r) | Cond::Nonempty(r) => {
                if self.unmet.contains(&self.view.base(r)) {
                    return Ok(false);
                }
                self.read(r);
                let empty = match self.view.get(r)? {
                    Seen::Exact(Value::List(v)) => v.is_empty(),
                    Seen::Projected(Value::List(v)) => v.is_empty(),
                    Seen::Exact(Value::Filtered(f)) => f.eligible.is_empty(),
                    Seen::Exact(Value::Shortlist(s)) => s.ids.is_empty(),
                    Seen::Exact(Value::Ranking(r)) => r.entries.is_empty(),
                    Seen::Exact(Value::Selection(s)) => s.entries.is_empty(),
                    _ => {
                        return Err(Unresolved::invalid(
                            [],
                            format!("empty() of a non-collection {r}"),
                        ));
                    }
                };
                if matches!(c, Cond::Empty(_)) {
                    empty
                } else {
                    !empty
                }
            }
            Cond::All(cs) => {
                for c in cs {
                    if !self.cond(c)? {
                        return Ok(false);
                    }
                }
                true
            }
            Cond::Any(cs) => {
                for c in cs {
                    if self.cond(c)? {
                        return Ok(true);
                    }
                }
                false
            }
            Cond::Not(c) => !self.cond(c)?,
            // Compilation refuses spec 033's constructs until its
            // evaluation lands (spec 033 increment 1); never a default.
            // Reads its reference like any atomic condition (spec 033, 3.4.7):
            // an input's accepted entry's class, or every class of a step
            // value's provenance set (an empty set is within).
            Cond::ProvenanceWithin { reference, classes } => {
                if self.unmet.contains(&self.view.base(reference)) {
                    return Ok(false);
                }
                self.read(reference);
                self.view.get(reference)?;
                self.view
                    .provenance(reference)?
                    .iter()
                    .all(|c| classes.contains(c))
            }
        })
    }

    fn questions(
        &mut self,
        selection: &rustev_contract::definition::QuestionSelection,
    ) -> Vec<String> {
        use rustev_contract::definition::QuestionSelection;
        let QuestionSelection::Proposed {
            proposed_by,
            allowed,
        } = selection
        else {
            return selection.allowed().to_vec();
        };
        let reference = format!("step:{proposed_by}");
        self.read(&reference);
        match self.view.get(&reference) {
            // 3.13.2: the compiler requires the accepted list's vocabulary to
            // equal `allowed`, so every accepted id is in `allowed` and a
            // non-empty list never filters to nothing.
            Ok(Seen::Exact(Value::List(ids))) if !ids.is_empty() => allowed
                .iter()
                .filter(|id| {
                    ids.iter()
                        .any(|v| matches!(v, Value::Enum(text) if text == *id))
                })
                .cloned()
                .collect(),
            _ => allowed.clone(),
        }
    }

    /// `Ok(None)` when the outcome reads a value handled `as_unmet`.
    fn outcome(&mut self, o: &OutcomeDecl) -> Result<Option<Outcome>, Unresolved> {
        match o {
            OutcomeDecl::Escalate { reason } => Ok(Some(Outcome::Escalate {
                reason: reason.clone(),
            })),
            OutcomeDecl::MissingEvidenceFrom { step } => {
                let r = format!("step:{step}");
                if self.unmet.contains(&r) {
                    return Ok(None);
                }
                self.read(&r);
                match self.view.get(&r)? {
                    Seen::Exact(Value::List(items)) => Ok(Some(Outcome::Unresolved(
                        Unresolved::missing(items.iter().filter_map(|v| match v {
                            Value::Text(s) => Some(s.clone()),
                            _ => None,
                        })),
                    ))),
                    _ => Err(Unresolved::invalid([], format!("{r} lists no fields"))),
                }
            }
            // Its questions, continuation and action are filled in by the
            // evaluation, which knows the plan and snapshot (spec 033, 3.7).
            OutcomeDecl::NeedsInput { fields, questions } => {
                Ok(Some(needs_input(fields, &self.questions(questions))))
            }
            OutcomeDecl::Propose { action, params } => {
                let mut out = BTreeMap::new();
                for p in params {
                    let v = match &p.value {
                        ParamSource::Lit(l) => Value::from_literal(l).to_out(),
                        ParamSource::Ref(r) => {
                            if self.unmet.contains(&self.view.base(r)) {
                                return Ok(None);
                            }
                            self.read(r);
                            match self.view.get(r)? {
                                Seen::Exact(v) => v.to_out(),
                                Seen::Projected(v) => v.to_out(),
                                Seen::Semantic(_) => {
                                    return Err(Unresolved::invalid(
                                        [],
                                        format!("{r} is semantic"),
                                    ));
                                }
                            }
                        }
                    };
                    out.insert(p.name.clone(), v);
                }
                Ok(Some(Outcome::Propose {
                    action: action.clone(),
                    params: out,
                }))
            }
        }
    }
}

pub(crate) fn evaluate<V: View>(policy: &PolicyDecl, view: &V) -> PolicyResult {
    let mut run = Run {
        view,
        unmet: BTreeSet::new(),
        reads: Vec::new(),
    };
    let mut trace = Vec::new();
    let mut handled: BTreeSet<&str> = BTreeSet::new();
    for (i, h) in policy.on_unresolved.iter().enumerate() {
        if handled.contains(h.target.as_str()) {
            continue;
        }
        if view.skipped(&h.target) {
            let matches = match &h.reasons {
                ReasonSet::Any => true,
                ReasonSet::Only(v) => v.contains(&rustev_contract::definition::ReasonKind::Skipped),
            };
            if !matches {
                continue;
            }
            handled.insert(&h.target);
            run.read(&h.target);
            trace.push(format!("on_unresolved[{i}]"));
            match &h.action {
                HandlerAction::AsUnmet => {
                    run.unmet.insert(h.target.clone());
                }
                HandlerAction::Escalate { reason } => {
                    return decided(
                        Outcome::Escalate {
                            reason: reason.clone(),
                        },
                        run,
                        trace,
                        format!("handler:{i}"),
                        Some(i),
                    );
                }
                HandlerAction::Propagate
                | HandlerAction::PropagateWith { .. }
                | HandlerAction::NeedsInput { .. } => {
                    // Unreachable for compiled policies: spec 036, 3.8.3
                    // refuses these actions when they cover a skipped value.
                }
            }
            continue;
        }
        let Err(u) = view.get(&h.target) else {
            continue;
        };
        let matches = match &h.reasons {
            ReasonSet::Any => true,
            ReasonSet::Only(v) => v.contains(&u.kind()),
        };
        if !matches {
            continue;
        }
        handled.insert(&h.target);
        run.read(&h.target);
        trace.push(format!("on_unresolved[{i}]"));
        match &h.action {
            HandlerAction::Propagate => {
                return decided(
                    Outcome::Unresolved(u),
                    run,
                    trace,
                    format!("handler:{i}"),
                    Some(i),
                );
            }
            HandlerAction::Escalate { reason } => {
                return decided(
                    Outcome::Escalate {
                        reason: reason.clone(),
                    },
                    run,
                    trace,
                    format!("handler:{i}"),
                    Some(i),
                );
            }
            HandlerAction::AsUnmet => {
                run.unmet.insert(h.target.clone());
            }
            HandlerAction::PropagateWith { .. } => {
                return decided(
                    Outcome::Unresolved(u),
                    run,
                    trace,
                    format!("handler:{i}"),
                    Some(i),
                );
            }
            HandlerAction::NeedsInput { fields, questions } => {
                return decided(
                    needs_input(fields, &run.questions(questions)),
                    run,
                    trace,
                    format!("handler:{i}"),
                    Some(i),
                );
            }
        }
    }
    let mut outcome = None;
    let mut site = None;
    for r in &policy.rules {
        let matched = match run.cond(&r.when) {
            Ok(m) => m,
            // Unreachable when compile-time coverage holds; propagate rather
            // than invent a value.
            Err(u) => {
                trace.push(format!("rule:{}:unhandled", r.id));
                return done(Outcome::Unresolved(u), run, trace);
            }
        };
        if !matched {
            continue;
        }
        match run.outcome(&r.then) {
            Ok(Some(o)) => {
                trace.push(format!("rule:{}", r.id));
                outcome = Some(o);
                site = Some(format!("rule:{}", r.id));
                break;
            }
            Ok(None) => continue,
            Err(u) => {
                trace.push(format!("rule:{}:unhandled", r.id));
                return done(Outcome::Unresolved(u), run, trace);
            }
        }
    }
    // The last rule is `always` and reads nothing handled as_unmet, so a
    // rule always matched.
    let mut outcome = outcome.unwrap_or_else(|| {
        Outcome::Unresolved(Unresolved::Abstained {
            rule: "no-rule".into(),
        })
    });
    if let Outcome::Propose { params, .. } = &mut outcome {
        for a in &policy.adjustments {
            let Some(OutValue::Enum(cur) | OutValue::Text(cur)) = params.get(&a.param) else {
                continue;
            };
            let Some(pos) = a.scale.iter().position(|s| s == cur) else {
                continue;
            };
            match run.cond(&a.when) {
                Ok(true) => {
                    let next = a.scale[(pos + 1).min(a.scale.len() - 1)].clone();
                    let replaced = match params.get(&a.param) {
                        Some(OutValue::Enum(_)) => OutValue::Enum(next),
                        _ => OutValue::Text(next),
                    };
                    params.insert(a.param.clone(), replaced);
                    trace.push(format!("adjustment:{}", a.id));
                }
                Ok(false) => {}
                Err(_) => trace.push(format!("adjustment:{}:unhandled", a.id)),
            }
        }
    }
    let mut result = done(outcome, run, trace);
    result.site = site;
    result
}

fn done<V: View>(outcome: Outcome, run: Run<'_, V>, trace: Vec<String>) -> PolicyResult {
    PolicyResult {
        outcome,
        reads: run.reads,
        trace,
        site: None,
        handler: None,
    }
}

fn decided<V: View>(
    outcome: Outcome,
    run: Run<'_, V>,
    trace: Vec<String>,
    site: String,
    handler: Option<usize>,
) -> PolicyResult {
    PolicyResult {
        site: Some(site),
        handler,
        ..done(outcome, run, trace)
    }
}

/// A `needs_input` outcome naming its fields and question ids; the
/// evaluation fills in each template, the continuation and the action.
fn needs_input(fields: &[String], questions: &[String]) -> Outcome {
    use rustev_contract::actionability::{
        Action, NeedsInput, Override, Party, QuestionDecl, Remediation,
    };
    let placeholder: rustev_contract::ids::ContentDigest =
        serde_json::from_value(serde_json::json!(format!("sha256:{}", "0".repeat(64))))
            .expect("a well-formed digest");
    Outcome::NeedsInput(Box::new(NeedsInput {
        fields: fields.to_vec(),
        questions: questions
            .iter()
            .map(|id| QuestionDecl {
                id: id.clone(),
                fields: vec![],
                text: String::new(),
            })
            .collect(),
        continuation: placeholder,
        action: Action {
            addressed_to: Party::Caller,
            computed: Party::Caller,
            override_: Override::None,
            remediation: Remediation::None,
            caused_by: vec![],
            transient: false,
        },
    }))
}
