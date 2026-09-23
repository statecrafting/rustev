//! Selection policy evaluation (spec 002, 3.11.5): handlers in order, then the
//! first matching rule, then adjustments in order. A pure function of the
//! plan, the validated context, the step values and the evaluation time.

use std::collections::{BTreeMap, BTreeSet};

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
    Semantic(&'a SemValue),
}

/// What the policy can look up.
pub(crate) trait View {
    /// `Ok(value)`, or the unresolved reason, for `input:`/`step:` refs.
    fn get(&self, r: &str) -> Result<Seen<'_>, Unresolved>;
    fn scope(&self) -> Scope<'_>;
}

pub(crate) struct PolicyResult {
    pub outcome: Outcome,
    /// References read, in reading order, deduplicated.
    pub reads: Vec<String>,
    pub trace: Vec<String>,
}

struct Run<'v, V: View> {
    view: &'v V,
    unmet: BTreeSet<String>,
    reads: Vec<String>,
}

impl<V: View> Run<'_, V> {
    fn read(&mut self, r: &str) {
        let r = crate::expr::base_ref(r);
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
            Seen::Exact(_) => Err(Unresolved::invalid([], format!("{r} is not semantic"))),
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
                    .map(|r| crate::expr::base_ref(r))
                    .collect();
                if refs.iter().any(|r| self.unmet.contains(r)) {
                    return Ok(false);
                }
                for r in &refs {
                    self.read(r);
                    self.view.get(r)?;
                }
                eval_predicate(p, &self.view.scope())
                    .map_err(|f| Unresolved::invalid([], format!("{f:?}")))?
            }
            Cond::Empty(r) | Cond::Nonempty(r) => {
                if self.unmet.contains(&crate::expr::base_ref(r)) {
                    return Ok(false);
                }
                self.read(r);
                let empty = match self.view.get(r)? {
                    Seen::Exact(Value::List(v)) => v.is_empty(),
                    Seen::Exact(Value::Filtered(f)) => f.eligible.is_empty(),
                    Seen::Exact(Value::Shortlist(s)) => s.ids.is_empty(),
                    Seen::Exact(Value::Ranking(r)) => r.entries.is_empty(),
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
        })
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
            OutcomeDecl::Propose { action, params } => {
                let mut out = BTreeMap::new();
                for p in params {
                    let v = match &p.value {
                        ParamSource::Lit(l) => Value::from_literal(l).to_out(),
                        ParamSource::Ref(r) => {
                            if self.unmet.contains(&crate::expr::base_ref(r)) {
                                return Ok(None);
                            }
                            self.read(r);
                            match self.view.get(r)? {
                                Seen::Exact(v) => v.to_out(),
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
            HandlerAction::Propagate => return done(Outcome::Unresolved(u), run, trace),
            HandlerAction::Escalate { reason } => {
                return done(
                    Outcome::Escalate {
                        reason: reason.clone(),
                    },
                    run,
                    trace,
                );
            }
            HandlerAction::AsUnmet => {
                run.unmet.insert(h.target.clone());
            }
        }
    }
    let mut outcome = None;
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
    done(outcome, run, trace)
}

fn done<V: View>(outcome: Outcome, run: Run<'_, V>, trace: Vec<String>) -> PolicyResult {
    PolicyResult {
        outcome,
        reads: run.reads,
        trace,
    }
}
