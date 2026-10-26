//! The projection of a delivered run record into one decision's span tree
//! (spec 028, 3.5): `rustev.decision`, a `rustev.step` per step evidence
//! entry, a `rustev.attempt` per attempt, and `rustev.policy` last.
//!
//! Every value leaves through [`SpanBuilder::put`], which applies the
//! redaction policy and the per-attribute value bound; there is no other
//! path from a record value to an exported one (3.4.7). The projection
//! reads the record and never writes it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use rustev_contract::definition::{Operation, ProvenanceClass, StepBody};
use rustev_contract::evidence::{EvidenceRecord, InputStatus, StepEvidence, StepStatus};
use rustev_contract::judgment::{OutValue, Outcome, Unresolved};
use rustev_contract::plan::{Plan, PlanStepDetail};
use rustev_contract::redaction::{Applied, FieldClass, Redactor, lineage_class, truncate_value};
use rustev_contract::remote::{Disclosure, ExchangeRecord, MemberOutcome};
use rustev_contract::run::{
    AttemptEnd, AttemptRecord, Cancellation, Charge, CoreEvidence, CostBound, RecordedExecution,
    RequestRecord, RequestResult, RunRecord, Transition,
};
use rustev_contract::schema;
use rustev_contract::trace::{
    Mark, SpanKind, SpanStatus, TRACE, TraceAttr, TraceEvent, TraceLine, TraceLineError,
    TraceResource, TraceScope, TraceValue,
};
use rustev_contract::trace_fields::{
    PARAM_PREFIX, SCHEMA_URL, SPAN_SCHEMA_VERSION, event, fixed, span, withheld_name,
};
use serde::Serialize;

use crate::ids::{IdSource, hex};

/// Events kept per span (3.5.5); the excess is counted in `rustev.events_cut`.
pub const MAX_EVENTS_PER_SPAN: usize = 128;
/// The instrumentation scope name (3.2.4).
pub const SCOPE_NAME: &str = "rustev-otel";

/// Where the decision's wall-clock anchor was read (3.5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    /// At `begin`, when the host registered the decision.
    Begin,
    /// At delivery minus `elapsed_ms`.
    Delivery,
}

impl AnchorKind {
    fn name(self) -> &'static str {
        match self {
            AnchorKind::Begin => "begin",
            AnchorKind::Delivery => "delivery",
        }
    }
}

/// The wall-clock time of submission, in Unix nanoseconds, and how it was
/// read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub unix_nanos: u64,
    pub kind: AnchorKind,
}

/// The decision's trace and root ids, and the host span it is a child of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootIds {
    pub trace_id: [u8; 16],
    pub span_id: [u8; 8],
    pub parent_span_id: Option<[u8; 8]>,
}

/// Per backend id, which exchange `usage` keys are input and output tokens
/// (3.7.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageKeys {
    pub input: Option<String>,
    pub output: Option<String>,
}

/// Host configuration the projection reads (3.7.2). Rustev names no
/// vendor: provider names and usage keys come only from here or from the
/// exchange record.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    /// Backend id to `gen_ai.provider.name`.
    pub providers: BTreeMap<String, String>,
    /// Backend id to its usage keys.
    pub usage_keys: BTreeMap<String, UsageKeys>,
}

/// Everything one projection reads.
pub struct ProjectionInput<'a> {
    pub record: &'a RunRecord,
    /// The registered plan, when the host registered it (3.7.2).
    pub plan: Option<&'a Plan>,
    /// Joined exchange records, by attempt id (3.7.3).
    pub exchanges: &'a BTreeMap<String, Arc<ExchangeRecord>>,
    pub bindings: &'a Bindings,
    pub anchor: Anchor,
    pub root: RootIds,
    /// The per-attribute value bound in bytes (3.8.5).
    pub value_bound: usize,
}

/// One decision's span lines and what was cut on the way.
#[derive(Debug, Clone, PartialEq)]
pub struct Projected {
    /// Root first, then each step followed by its attempts, policy last.
    pub lines: Vec<TraceLine>,
    /// Attribute values cut to the value bound (3.8.5).
    pub attributes_cut: u64,
    /// Events beyond 128 on a span, or removed to fit a line (3.5.5).
    pub events_cut: u64,
    /// Facts the sink's metrics and sampling read, in their closed
    /// vocabularies: never caller text.
    pub facts: Facts,
}

/// What the metrics, logs and sampling of 3.6 and 3.9.4 need from a
/// decision, already reduced to enum and number values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    pub outcome: String,
    pub termination: String,
    pub plan_id: String,
    pub outcome_unresolved_reason: Option<String>,
    /// `unresolved_reason` of every unresolved step.
    pub step_unresolved_reasons: Vec<String>,
    /// `fallback` transitions.
    pub fallbacks: u64,
    pub elapsed_ms: u64,
    pub final_cost: String,
    /// Observed plus estimated cost units.
    pub cost_units: u64,
    pub attempts: Vec<AttemptFacts>,
    /// Entry and closed verdict name, never screened content or its digest.
    pub screening: Vec<(String, String)>,
    /// Total charged units, present for decisions with screening.
    pub screening_units: Option<u64>,
}

/// One attempt, for metrics and logs.
#[derive(Debug, Clone, PartialEq)]
pub struct AttemptFacts {
    /// The attempt span's index in `lines`.
    pub line: usize,
    pub step_kind: String,
    pub end: String,
    pub backend_id: String,
    pub duration_ms: u64,
    /// GenAI attributes, when the attempt has them.
    pub genai: Option<GenAiFacts>,
}

/// GenAI facts of one attempt (3.6.1).
#[derive(Debug, Clone, PartialEq)]
pub struct GenAiFacts {
    pub operation: Option<String>,
    pub provider: String,
    pub request_model: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// A record the projection does not know (3.5.6): never a guessed span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionUnknown {
    pub what: String,
}

impl fmt::Display for ProjectionUnknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "projection_unknown: {}", self.what)
    }
}

impl std::error::Error for ProjectionUnknown {}

fn unknown(what: impl Into<String>) -> ProjectionUnknown {
    ProjectionUnknown { what: what.into() }
}

/// The wire name of a unit enum variant.
fn name_of<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => "unknown".to_string(),
    }
}

/// A count or duration as an attribute value: an integer, or, beyond
/// `i64`, its exact decimal string (never converted to a float).
fn num(n: u64) -> TraceValue {
    match i64::try_from(n) {
        Ok(i) => TraceValue::Int(i),
        Err(_) => TraceValue::Str(n.to_string()),
    }
}

fn strs(items: &[String]) -> TraceValue {
    TraceValue::StrArray(items.to_vec())
}

fn ms_to_nanos(ms: u64) -> u64 {
    ms.saturating_mul(1_000_000)
}

/// Collects one span's attributes and events through the policy.
struct SpanBuilder<'r> {
    redactor: &'r Redactor,
    bound: usize,
    attributes: BTreeMap<String, TraceAttr>,
    events: Vec<TraceEvent>,
    withheld: BTreeSet<String>,
    attributes_cut: u64,
    events_cut: u64,
}

impl<'r> SpanBuilder<'r> {
    fn new(redactor: &'r Redactor, bound: usize) -> Self {
        SpanBuilder {
            redactor,
            bound,
            attributes: BTreeMap::new(),
            events: Vec::new(),
            withheld: BTreeSet::new(),
            attributes_cut: 0,
            events_cut: 0,
        }
    }

    /// Apply the policy and the value bound to one value of `field`.
    fn redact(&mut self, field: &str, class: FieldClass, value: &TraceValue) -> Option<TraceAttr> {
        match self.redactor.apply(field, class, value) {
            Applied::Dropped => {
                self.withheld.insert(withheld_name(field).to_string());
                None
            }
            Applied::Kept(mut a) => {
                let (v, cut) = truncate_value(&a.v, self.bound);
                if cut {
                    self.attributes_cut += 1;
                    a.v = v;
                    if a.r == Mark::Allow {
                        a.r = Mark::Truncated;
                    }
                }
                Some(a)
            }
        }
    }

    fn put(&mut self, field: &str, class: FieldClass, value: TraceValue) {
        if let Some(a) = self.redact(field, class, &value) {
            self.attributes.insert(field.to_string(), a);
        }
    }

    /// A fixed field (3.4.4): always exported as written.
    fn put_fixed(&mut self, field: &str, value: TraceValue) {
        self.attributes.insert(
            field.to_string(),
            TraceAttr {
                v: value,
                r: Mark::Allow,
            },
        );
    }

    /// One event; its attributes are catalogued as `<event>.<key>`.
    fn event(&mut self, name: &str, time: u64, attrs: Vec<(&str, FieldClass, TraceValue)>) {
        if self.events.len() >= MAX_EVENTS_PER_SPAN {
            self.events_cut += 1;
            return;
        }
        let mut out = BTreeMap::new();
        for (key, class, value) in attrs {
            let field = format!("{name}.{key}");
            if let Some(a) = self.redact(&field, class, &value) {
                out.insert(key.to_string(), a);
            }
        }
        self.events.push(TraceEvent {
            name: name.to_string(),
            time_unix_nanos: time,
            attributes: out,
        });
    }
}

/// The span being built, before it becomes a line.
struct Draft<'r> {
    name: &'static str,
    span_id: [u8; 8],
    parent: Option<[u8; 8]>,
    kind: SpanKind,
    start: u64,
    end: u64,
    status: SpanStatus,
    b: SpanBuilder<'r>,
}

/// Decision-level context the steps read.
struct Ctx<'a> {
    evidence: Option<&'a EvidenceRecord>,
    plan: Option<&'a Plan>,
    anchor_kind: AnchorKind,
}

impl Ctx<'_> {
    fn step_body(&self, step: &str) -> Option<&StepBody> {
        self.plan?
            .definition
            .steps
            .iter()
            .find(|s| s.id == step)
            .map(|s| &s.body)
    }

    fn step_kind(&self, step: &str) -> Result<String, ProjectionUnknown> {
        if self.plan.is_none() {
            return Ok("unknown".into());
        }
        match self.step_body(step) {
            Some(StepBody::Acquire(_)) => Err(unknown(format!("step kind acquire ({step})"))),
            Some(StepBody::Widen(_)) => Err(unknown(format!("step kind widen ({step})"))),
            Some(StepBody::Generate(_)) => Err(unknown(format!("step kind generate ({step})"))),
            Some(StepBody::Extract(_)) => Err(unknown(format!("step kind extract ({step})"))),
            Some(StepBody::AcceptModelDerived(_)) => {
                Err(unknown(format!("step kind accept_model_derived ({step})")))
            }
            Some(StepBody::Exact(_)) => Ok("exact".into()),
            Some(StepBody::Semantic(s)) => Ok(operation_name(s.operation).into()),
            Some(StepBody::Corroborate(_)) => {
                Err(unknown(format!("step kind corroborate ({step})")))
            }
            None => Err(unknown(format!(
                "step {step} is not in the registered plan"
            ))),
        }
    }

    fn operation(&self, step: &str) -> Option<&'static str> {
        match self.step_body(step)? {
            StepBody::Semantic(s) => Some(operation_name(s.operation)),
            _ => None,
        }
    }

    /// The provenance classes of the judgment lineage's inputs; an input
    /// with no recorded provenance counts as the most restrictive class.
    fn lineage_inputs(&self) -> Vec<ProvenanceClass> {
        let Some(ev) = self.evidence else {
            return vec![];
        };
        ev.judgment
            .lineage
            .inputs
            .iter()
            .map(|field| {
                ev.inputs
                    .iter()
                    .find(|i| &i.field == field)
                    .and_then(|i| match &i.status {
                        InputStatus::Resolved { provenance, .. } => Some(*provenance),
                        InputStatus::Unresolved(_) => None,
                    })
                    .unwrap_or(ProvenanceClass::AttributedClaim)
            })
            .collect()
    }
}

fn operation_name(op: Operation) -> &'static str {
    match op {
        Operation::Classify => "classify",
        Operation::Proposition => "proposition",
        Operation::Rubric => "rubric",
        Operation::Rank => "rank",
        Operation::Generate => "generate",
    }
}

/// The variant name, its fields or facts, and its detail (3.5.4).
fn unresolved_parts(u: &Unresolved) -> (&'static str, Option<&[String]>, Option<&str>) {
    let fields = match u {
        Unresolved::MissingEvidence { fields }
        | Unresolved::StaleEvidence { fields }
        | Unresolved::InvalidInput { fields, .. }
        | Unresolved::InsufficientGrade { fields } => Some(fields.as_slice()),
        Unresolved::Conflict { facts } => Some(facts.as_slice()),
        _ => None,
    };
    let detail = match u {
        Unresolved::InvalidInput { detail, .. }
        | Unresolved::BackendUnavailable { detail }
        | Unresolved::InvalidBackendOutput { detail } => Some(detail.as_str()),
        _ => None,
    };
    (u.kind().name(), fields, detail)
}

/// An outcome parameter as an attribute value, and whether it is a boolean
/// or enum (3.3.2). Scalars keep their kind; a composite value is its
/// record canonical JSON, never converted.
fn param_value(v: &OutValue) -> (TraceValue, bool) {
    match v {
        OutValue::Bool(b) => (TraceValue::Bool(*b), true),
        OutValue::Enum(s) => (TraceValue::Str(s.clone()), true),
        OutValue::Integer(i) => (TraceValue::Int(*i), false),
        OutValue::Text(s) => (TraceValue::Str(s.clone()), false),
        OutValue::Decimal(d) => (TraceValue::Str(d.to_string()), false),
        OutValue::Timestamp(t) => (TraceValue::Int(t.as_ms()), false),
        OutValue::Duration(d) => (num(d.0), false),
        other => (
            TraceValue::Str(
                rustev_contract::canonical::record_canonical_bytes(other)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_else(|| "null".into()),
            ),
            false,
        ),
    }
}

/// Project one delivered run record (3.5). A record schema, judgment
/// outcome or step kind this schema version does not know is
/// [`ProjectionUnknown`].
pub fn project(
    input: &ProjectionInput<'_>,
    redactor: &Redactor,
    ids: &mut dyn IdSource,
) -> Result<Projected, ProjectionUnknown> {
    let rec = input.record;
    if ![schema::RUN, schema::RUN_V2, schema::RUN_V3].contains(&rec.schema.as_str()) {
        return Err(unknown(format!("run record {}", rec.schema)));
    }
    let evidence = match &rec.core {
        CoreEvidence::Recorded(ev) => {
            if ![schema::EVIDENCE, schema::EVIDENCE_V2].contains(&ev.schema.as_str())
                || ![schema::JUDGMENT, schema::JUDGMENT_V2].contains(&ev.judgment.schema.as_str())
            {
                return Err(unknown(format!(
                    "{} with {}",
                    ev.schema, ev.judgment.schema
                )));
            }
            Some(ev.as_ref())
        }
        CoreEvidence::NotProduced => None,
    };
    if let Some(ev) = evidence
        && matches!(
            ev.judgment.outcome,
            Outcome::NeedsInput(_) | Outcome::UnresolvedAction { .. }
        )
    {
        return Err(unknown("judgment outcome of a later spec"));
    }
    if let Some(p) = input.plan
        && ![schema::PLAN, schema::PLAN_V3].contains(&p.schema.as_str())
    {
        return Err(unknown(format!("plan {}", p.schema)));
    }

    let a = input.anchor.unix_nanos;
    let root_start = a.saturating_add(ms_to_nanos(rec.timing.queued_ms));
    let root_end = a
        .saturating_add(ms_to_nanos(rec.timing.elapsed_ms))
        .max(root_start);
    let ctx = Ctx {
        evidence,
        plan: input.plan,

        anchor_kind: input.anchor.kind,
    };
    let mut facts = Facts {
        termination: name_of(&rec.termination),
        plan_id: rec.plan_id.to_string(),
        elapsed_ms: rec.timing.elapsed_ms,
        final_cost: name_of(&rec.cost.final_cost),
        cost_units: rec.cost.observed.saturating_add(rec.cost.estimated),
        ..Facts::default()
    };

    if let Some(entries) = evidence.and_then(|e| e.screening.as_ref()) {
        use rustev_contract::screening::ScreenVerdict;
        facts.screening = entries
            .iter()
            .map(|e| {
                let verdict = match e.verdict {
                    ScreenVerdict::Pass => "pass",
                    ScreenVerdict::Blocked => "blocked",
                    ScreenVerdict::Unavailable { .. } => "unavailable",
                    ScreenVerdict::Empty => "empty",
                };
                (e.entry.clone(), verdict.into())
            })
            .collect();
    }
    if evidence.is_some_and(|e| e.screening.is_some())
        || input.plan.is_some_and(|p| p.screening.is_some())
        || rec.requests.iter().any(|r| r.screen.is_some())
    {
        facts.screening_units = Some(
            rec.requests
                .iter()
                .filter_map(|r| r.screen.as_ref())
                .fold(0u64, |sum, r| sum.saturating_add(r.units.charged)),
        );
    }

    // The root.
    let mut root = Draft {
        name: span::DECISION,
        span_id: input.root.span_id,
        parent: input.root.parent_span_id,
        kind: SpanKind::Internal,
        start: root_start,
        end: root_end,
        status: SpanStatus::Unset,
        b: SpanBuilder::new(redactor, input.value_bound),
    };
    root_attributes(&mut root.b, rec, evidence, &ctx, &mut facts, root_start);

    // Steps: one per evidence entry, then any request with no entry (a
    // decision whose evidence was not produced still ran its requests).
    let mut drafts: Vec<Draft<'_>> = vec![root];
    let mut used = vec![false; rec.requests.len()];
    let mut order: Vec<(Option<&StepEvidence>, Option<&RequestRecord>)> = vec![];
    for se in evidence.map(|e| e.steps.as_slice()).unwrap_or(&[]) {
        let req = rec
            .requests
            .iter()
            .enumerate()
            .find(|(i, r)| !used[*i] && r.step == se.step && r.instance == se.instance);
        if let Some((i, _)) = req {
            used[i] = true;
        }
        order.push((Some(se), req.map(|(_, r)| r)));
    }
    for (i, r) in rec.requests.iter().enumerate() {
        if !used[i] {
            order.push((None, Some(r)));
        }
    }
    for (se, req) in order {
        let step_id = se.map(|s| s.step.as_str()).or(req.map(|r| r.step.as_str()));
        let step_id = step_id.unwrap_or_default();
        let kind = ctx.step_kind(step_id)?;
        let step_span = ids.span_id();
        let step_index = drafts.len();
        let mut step = Draft {
            name: span::STEP,
            span_id: step_span,
            parent: Some(input.root.span_id),
            kind: SpanKind::Internal,
            start: root_end,
            end: root_end,
            status: SpanStatus::Unset,
            b: SpanBuilder::new(redactor, input.value_bound),
        };
        step_attributes(&mut step.b, se, req, &kind, &ctx, step_id, &mut facts);
        drafts.push(step);
        let mut window: Option<(u64, u64)> = None;
        if let Some(r) = req {
            for (n, att) in r.attempts.iter().enumerate() {
                let start = a.saturating_add(ms_to_nanos(att.dispatched_ms));
                let end = a.saturating_add(ms_to_nanos(att.ended_ms)).max(start);
                window = Some(match window {
                    None => (start, end),
                    Some((s, e)) => (s.min(start), e.max(end)),
                });
                let retry_index = r.attempts[..n]
                    .iter()
                    .filter(|p| p.target == att.target)
                    .count() as u64;
                let mut d = Draft {
                    name: span::ATTEMPT,
                    span_id: ids.span_id(),
                    parent: Some(step_span),
                    kind: SpanKind::Internal,
                    start,
                    end,
                    status: SpanStatus::Unset,
                    b: SpanBuilder::new(redactor, input.value_bound),
                };
                let line = drafts.len();
                let genai =
                    attempt_attributes(&mut d, att, r, retry_index, ctx.operation(step_id), input);
                facts.attempts.push(AttemptFacts {
                    line,
                    step_kind: kind.clone(),
                    end: attempt_end_name(&att.end).to_string(),
                    backend_id: att.backend_id.clone(),
                    duration_ms: att.ended_ms.saturating_sub(att.dispatched_ms),
                    genai,
                });
                drafts.push(d);
            }
            // Transitions on the step, at its start.
            let at = window.map(|w| w.0).unwrap_or(root_end);
            for t in &r.transitions {
                transition_event(&mut drafts[step_index].b, t, at);
                if matches!(t, Transition::Fallback { .. }) {
                    facts.fallbacks += 1;
                }
            }
        }
        let s = &mut drafts[step_index];
        match window {
            Some((start, end)) => {
                s.start = start;
                s.end = end;
            }
            None => {
                s.b.put("rustev.timing", FieldClass::Enum, "not_recorded".into())
            }
        }
    }

    // The policy span, last.
    let mut policy = Draft {
        name: span::POLICY,
        span_id: ids.span_id(),
        parent: Some(input.root.span_id),
        kind: SpanKind::Internal,
        start: root_end,
        end: root_end,
        status: SpanStatus::Unset,
        b: SpanBuilder::new(redactor, input.value_bound),
    };
    policy
        .b
        .put("rustev.timing", FieldClass::Enum, "not_recorded".into());
    if let Some(ev) = evidence {
        policy_attributes(&mut policy.b, ev, &ctx, root_end);
    }
    drafts.push(policy);

    // Lines.
    let trace_id = hex(&input.root.trace_id);
    let mut out = Projected {
        lines: Vec::with_capacity(drafts.len()),
        attributes_cut: 0,
        events_cut: 0,
        facts,
    };
    for (i, mut d) in drafts.into_iter().enumerate() {
        if i == 0 {
            d.b.put_fixed(
                fixed::POLICY_ID,
                TraceValue::Str(redactor.policy_id().to_string()),
            );
            d.b.put_fixed(
                fixed::KEY_ID,
                TraceValue::Str(redactor.key_id().to_string()),
            );
        }
        let line = finish(d, &trace_id, i == 0, &mut out)?;
        out.lines.push(line);
    }
    Ok(out)
}

/// Turn a draft into a line, cutting events (then long values) until the
/// line fits 64 KiB; every cut is counted.
fn finish(
    mut d: Draft<'_>,
    trace_id: &str,
    is_root: bool,
    out: &mut Projected,
) -> Result<TraceLine, ProjectionUnknown> {
    loop {
        if d.b.events_cut > 0 {
            d.b.put("rustev.events_cut", FieldClass::Number, num(d.b.events_cut));
        }
        let mut attributes = d.b.attributes.clone();
        attributes.insert(
            fixed::WITHHELD.to_string(),
            TraceAttr {
                v: TraceValue::StrArray(d.b.withheld.iter().cloned().collect()),
                r: Mark::Allow,
            },
        );
        let line = TraceLine {
            schema: TRACE.to_string(),
            schema_version: SPAN_SCHEMA_VERSION,
            trace_id: trace_id.to_string(),
            span_id: hex(&d.span_id),
            parent_span_id: d.parent.map(|p| hex(&p)),
            name: d.name.to_string(),
            kind: d.kind,
            start_unix_nanos: d.start,
            end_unix_nanos: d.end,
            status: d.status,
            attributes,
            events: d.b.events.clone(),
            resource: is_root.then(resource),
        };
        match line.to_line() {
            Ok(_) => {
                out.attributes_cut += d.b.attributes_cut;
                out.events_cut += d.b.events_cut;
                return Ok(line);
            }
            Err(TraceLineError::TooLong { .. }) if !d.b.events.is_empty() => {
                let drop = d.b.events.len().div_ceil(2);
                d.b.events.truncate(d.b.events.len() - drop);
                d.b.events_cut += drop as u64;
            }
            Err(TraceLineError::TooLong { .. }) => {
                // Only attributes are left: cut every long string to 256
                // bytes. Values are already bounded, so this terminates.
                let mut changed = false;
                for a in d.b.attributes.values_mut() {
                    let (v, cut) = truncate_value(&a.v, 256);
                    if cut {
                        a.v = v;
                        if a.r == Mark::Allow {
                            a.r = Mark::Truncated;
                        }
                        d.b.attributes_cut += 1;
                        changed = true;
                    }
                }
                if !changed {
                    return Err(unknown("a span that cannot fit one line"));
                }
            }
            Err(e) => return Err(unknown(format!("span line: {e}"))),
        }
    }
}

fn resource() -> TraceResource {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        fixed::SCHEMA_VERSION.to_string(),
        TraceAttr {
            v: TraceValue::Int(i64::from(SPAN_SCHEMA_VERSION)),
            r: Mark::Allow,
        },
    );
    TraceResource {
        attributes,
        schema_url: SCHEMA_URL.to_string(),
        scope: TraceScope {
            name: SCOPE_NAME.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            schema_url: SCHEMA_URL.to_string(),
        },
    }
}

fn root_attributes(
    b: &mut SpanBuilder<'_>,
    rec: &RunRecord,
    evidence: Option<&EvidenceRecord>,
    ctx: &Ctx<'_>,
    facts: &mut Facts,
    root_start: u64,
) {
    use FieldClass::*;
    b.put("rustev.decision.id", Id, rec.decision_id.as_str().into());
    b.put("rustev.plan.id", PlanMetadata, rec.plan_id.as_str().into());
    b.put(
        "rustev.execution.policy_id",
        PlanMetadata,
        match &rec.execution {
            RecordedExecution::None => "none".into(),
            RecordedExecution::Declared(id) => id.as_str().into(),
        },
    );
    b.put("rustev.termination", Enum, facts.termination.clone().into());
    match evidence {
        None => {
            b.put("rustev.outcome", Enum, "not_produced".into());
            facts.outcome = "not_produced".into();
        }
        Some(ev) => {
            b.put(
                "rustev.definition.id",
                PlanMetadata,
                ev.definition_id.as_str().into(),
            );
            b.put(
                "rustev.snapshot.id",
                ContentDigest,
                ev.snapshot_id.as_str().into(),
            );
            let j = &ev.judgment;
            let outcome = match &j.outcome {
                Outcome::Propose { action, params } => {
                    b.put(
                        "rustev.outcome.action",
                        PlanMetadata,
                        action.as_str().into(),
                    );
                    let inputs = ctx.lineage_inputs();
                    for (name, v) in params {
                        let (value, bool_or_enum) = param_value(v);
                        let class = lineage_class(bool_or_enum, inputs.iter().copied());
                        b.put(&format!("{PARAM_PREFIX}{name}"), class, value);
                    }
                    "propose"
                }
                Outcome::Escalate { reason } => {
                    b.put(
                        "rustev.outcome.escalate_reason",
                        PlanMetadata,
                        reason.as_str().into(),
                    );
                    "escalate"
                }
                Outcome::Unresolved(u) => {
                    let (kind, fields, detail) = unresolved_parts(u);
                    b.put("rustev.outcome.unresolved_reason", Enum, kind.into());
                    facts.outcome_unresolved_reason = Some(kind.into());
                    if let Some(f) = fields {
                        b.put("rustev.outcome.unresolved_fields", PlanMetadata, strs(f));
                    }
                    if let Some(d) = detail {
                        b.put("rustev.outcome.unresolved_detail", FreeTextDetail, d.into());
                    }
                    "unresolved"
                }
                Outcome::Refused { .. } => "refused",
                // `project` returns before any span is built for
                // `NeedsInput` and `UnresolvedAction` (the early check above),
                // so this arm is never reached; `Refused` has its own arm.
                Outcome::NeedsInput(_) | Outcome::UnresolvedAction { .. } => "unknown",
            };
            facts.outcome = outcome.into();
            b.put("rustev.outcome", Enum, outcome.into());
            b.put("rustev.derivation", Enum, name_of(&j.derivation).into());
            b.put(
                "rustev.lineage.inputs",
                PlanMetadata,
                strs(&j.lineage.inputs),
            );
            b.put("rustev.lineage.steps", PlanMetadata, strs(&j.lineage.steps));
            for i in &ev.inputs {
                let mut attrs = vec![("field", PlanMetadata, i.field.as_str().into())];
                match &i.status {
                    InputStatus::Resolved {
                        provenance,
                        as_of_ms,
                        source,
                    } => {
                        attrs.push(("status", Enum, "resolved".into()));
                        attrs.push(("provenance", Enum, name_of(provenance).into()));
                        attrs.push(("as_of_ms", Number, TraceValue::Int(as_of_ms.as_ms())));
                        attrs.push(("source", Id, source.as_str().into()));
                    }
                    InputStatus::Unresolved(u) => {
                        attrs.push(("status", Enum, "unresolved".into()));
                        attrs.push(("unresolved_reason", Enum, u.kind().name().into()));
                    }
                }
                b.event(event::INPUT, root_start, attrs);
            }
            for n in &j.notices {
                b.event(
                    event::NOTICE,
                    root_start,
                    vec![
                        ("kind", Enum, name_of(&n.kind).into()),
                        ("subject", PlanMetadata, n.subject.as_str().into()),
                        ("reason", PlanMetadata, n.reason.as_str().into()),
                    ],
                );
            }
        }
    }
    let c = &rec.cost;
    b.put("rustev.cost.mode", Enum, name_of(&c.mode).into());
    b.put("rustev.cost.final", Enum, name_of(&c.final_cost).into());
    b.put("rustev.cost.limit", Number, num(c.limit));
    b.put("rustev.cost.observed", Number, num(c.observed));
    b.put("rustev.cost.estimated", Number, num(c.estimated));
    b.put("rustev.cost.liability", Number, num(c.liability));
    b.put(
        "rustev.cost.bound_violations",
        Number,
        num(c.bound_violations),
    );
    b.put(
        "rustev.cost.within_guaranteed_cap",
        Enum,
        c.within_guaranteed_cap.into(),
    );
    let t = &rec.timing;
    b.put("rustev.timing.queued_ms", Number, num(t.queued_ms));
    b.put("rustev.timing.elapsed_ms", Number, num(t.elapsed_ms));
    b.put("rustev.timing.deadline_ms", Number, num(t.deadline_ms));
    b.put(
        "rustev.timing.deadline_expired",
        Enum,
        t.deadline_expired.into(),
    );
    b.put(
        "rustev.deadline.remaining_ms",
        Number,
        num(t.deadline_ms.saturating_sub(t.elapsed_ms)),
    );
    b.put("rustev.time_anchor", Enum, ctx.anchor_kind.name().into());
}

fn step_attributes(
    b: &mut SpanBuilder<'_>,
    se: Option<&StepEvidence>,
    req: Option<&RequestRecord>,
    kind: &str,
    ctx: &Ctx<'_>,
    step_id: &str,
    facts: &mut Facts,
) {
    use FieldClass::*;
    b.put("rustev.step.id", PlanMetadata, step_id.into());
    let instance = se.map(|s| &s.instance).or(req.map(|r| &r.instance));
    if let Some(inst) = instance.filter(|i| !i.is_empty()) {
        b.put("rustev.step.instance", Id, strs(inst));
    }
    b.put("rustev.step.kind", Enum, kind.into());
    if let Some(p) = ctx.plan
        && let Some(ps) = p.steps.iter().find(|s| s.id == step_id)
    {
        match &ps.detail {
            PlanStepDetail::Exact { op, version } => b.put(
                "rustev.step.operator",
                PlanMetadata,
                format!("{op}@{version}").into(),
            ),
            PlanStepDetail::Semantic(s) => b.put(
                "rustev.step.backend_id",
                PlanMetadata,
                s.backend_id.as_str().into(),
            ),
            PlanStepDetail::Unsupported { .. }
            | PlanStepDetail::Corroborate
            | PlanStepDetail::Acquire
            | PlanStepDetail::Widen
            | PlanStepDetail::Generate(_) => {}
        }
    }
    if let Some(s) = se {
        match &s.status {
            StepStatus::Skipped { .. } => b.put("rustev.step.status", Enum, "skipped".into()),
            StepStatus::Resolved { kind, derivation } => {
                b.put("rustev.step.status", Enum, "resolved".into());
                b.put("rustev.step.value_kind", Enum, kind.as_str().into());
                b.put("rustev.step.derivation", Enum, name_of(derivation).into());
            }
            StepStatus::Unresolved(u) => {
                b.put("rustev.step.status", Enum, "unresolved".into());
                let k = u.kind().name();
                b.put("rustev.step.unresolved_reason", Enum, k.into());
                facts.step_unresolved_reasons.push(k.into());
            }
        }
    }
    if let Some(r) = req {
        match &r.result {
            RequestResult::Output { target } => {
                b.put("rustev.step.result", Enum, "output".into());
                b.put("rustev.step.target", Number, num(u64::from(*target)));
            }
            RequestResult::Failed(_) => b.put("rustev.step.result", Enum, "failed".into()),
            RequestResult::Withdrawn => {}
            RequestResult::NotSupplied => b.put("rustev.step.result", Enum, "not_supplied".into()),
        }
        if let Some(o) = &r.optimization {
            b.put(
                "rustev.step.optimization.outcome",
                Enum,
                name_of(&o.outcome).into(),
            );
            b.put(
                "rustev.step.optimization.mechanism",
                Enum,
                name_of(&o.mechanism).into(),
            );
        }
        if let Some(from) = r.supplied_from()
            && let Some(id) = from.reused_from
        {
            b.put("rustev.step.reused_from", Id, id.into());
        }
    }
}

fn transition_event(b: &mut SpanBuilder<'_>, t: &Transition, at: u64) {
    use FieldClass::*;
    let attrs = match t {
        Transition::Retry {
            after,
            class,
            delay_ms,
        } => vec![
            ("kind", Enum, "retry".into()),
            ("class", Enum, class.name().into()),
            ("after", Number, num(u64::from(*after))),
            ("delay_ms", Number, num(*delay_ms)),
        ],
        Transition::Fallback { after, class, to } => vec![
            ("kind", Enum, "fallback".into()),
            ("class", Enum, class.name().into()),
            ("after", Number, num(u64::from(*after))),
            ("to", Number, num(u64::from(*to))),
        ],
        Transition::Stop { after, reason } => vec![
            ("kind", Enum, "stop".into()),
            ("after", Number, num(u64::from(*after))),
            ("reason", FreeTextDetail, reason.as_str().into()),
        ],
    };
    b.event(event::TRANSITION, at, attrs);
}

fn attempt_end_name(e: &AttemptEnd) -> &'static str {
    match e {
        AttemptEnd::Output => "output",
        AttemptEnd::Failed { .. } => "failed",
        AttemptEnd::Deadline => "deadline",
        AttemptEnd::Cancelled => "cancelled",
    }
}

/// The attempt's attributes; returns its GenAI facts when it has them.
fn attempt_attributes(
    d: &mut Draft<'_>,
    att: &AttemptRecord,
    req: &RequestRecord,
    retry_index: u64,
    operation: Option<&'static str>,
    input: &ProjectionInput<'_>,
) -> Option<GenAiFacts> {
    use FieldClass::*;
    let b = &mut d.b;
    b.put("rustev.attempt.id", Id, att.attempt_id.as_str().into());
    if let Some(n) = att
        .attempt_id
        .rsplit('/')
        .next()
        .and_then(|n| n.parse::<u64>().ok())
    {
        b.put("rustev.attempt.ordinal", Number, num(n));
    }
    b.put("rustev.attempt.retry_index", Number, num(retry_index));
    b.put("rustev.attempt.target", Number, num(u64::from(att.target)));
    b.put(
        "rustev.attempt.backend_id",
        PlanMetadata,
        att.backend_id.as_str().into(),
    );
    b.put(
        "rustev.attempt.artifact",
        PlanMetadata,
        att.artifact.as_str().into(),
    );
    b.put(
        "rustev.attempt.end",
        Enum,
        attempt_end_name(&att.end).into(),
    );
    match &att.end {
        AttemptEnd::Failed { class, detail } => {
            b.put("rustev.attempt.failure_class", Enum, class.name().into());
            b.put("error.type", Enum, class.name().into());
            b.put(
                "rustev.attempt.failure_detail",
                FreeTextDetail,
                detail.as_str().into(),
            );
            d.status = SpanStatus::Error;
        }
        AttemptEnd::Deadline => {
            b.put("error.type", Enum, "deadline".into());
            d.status = SpanStatus::Error;
        }
        AttemptEnd::Output | AttemptEnd::Cancelled => {}
    }
    b.put(
        "rustev.attempt.cancel",
        Enum,
        match att.cancellation {
            Cancellation::NotRequested => "not_requested".into(),
            Cancellation::Requested { answer } => name_of(&answer).into(),
        },
    );
    b.put(
        "rustev.attempt.remote_state",
        Enum,
        name_of(&att.remote).into(),
    );
    let (bound, _) = match att.cost.bound {
        CostBound::Bounded { max_units } => ("bounded", Some(max_units)),
        CostBound::Estimated { units } => ("estimated", Some(units)),
        CostBound::Unknown => ("unknown", None),
    };
    b.put("rustev.attempt.cost.bound", Enum, bound.into());
    b.put(
        "rustev.attempt.cost.reserved",
        Number,
        num(att.cost.reserved),
    );
    let (charge, units) = match att.cost.charge {
        Charge::Observed { units } => ("observed", Some(units)),
        Charge::Estimated { units } => ("estimated", Some(units)),
        Charge::Unknown => ("unknown", None),
    };
    b.put("rustev.attempt.charge_kind", Enum, charge.into());
    if let Some(u) = units {
        b.put("rustev.attempt.charge_units", Number, num(u));
    }
    if let Some(o) = &req.optimization {
        if let Some(id) = &o.batch_id {
            b.put("rustev.batch.id", Id, id.as_str().into());
        }
        if let Some(m) = o.batch_member {
            b.put("rustev.batch.member", Number, num(u64::from(m)));
        }
    }

    // GenAI attributes (3.5.4): only for an attempt that joined an exchange
    // record or whose backend id the host mapped to a provider name.
    let exchange = input.exchanges.get(&att.attempt_id);
    let provider = input.bindings.providers.get(&att.backend_id);
    if exchange.is_none() && provider.is_none() {
        return None;
    }
    d.kind = SpanKind::Client;
    let provider = provider.cloned().unwrap_or_else(|| "unknown".into());
    b.put(
        "gen_ai.provider.name",
        ModelIdentity,
        provider.as_str().into(),
    );
    if let Some(op) = operation {
        b.put("gen_ai.operation.name", Enum, op.into());
    }
    let request_model = exchange
        .and_then(|x| x.requested_model.clone())
        .unwrap_or_else(|| att.artifact.to_string());
    b.put(
        "gen_ai.request.model",
        ModelIdentity,
        request_model.as_str().into(),
    );
    let mut facts = GenAiFacts {
        operation: operation.map(str::to_string),
        provider,
        request_model,
        input_tokens: None,
        output_tokens: None,
    };
    let Some(x) = exchange else {
        b.put("gen_ai.response.model", ModelIdentity, "unknown".into());
        b.put("rustev.served.disclosure", Enum, "unknown".into());
        return Some(facts);
    };
    let response_model = match (&x.served.disclosure, &x.served.identity) {
        (Disclosure::Pinned | Disclosure::Reported, Some(id)) => id.clone(),
        _ => "unknown".into(),
    };
    b.put(
        "gen_ai.response.model",
        ModelIdentity,
        response_model.into(),
    );
    b.put(
        "rustev.served.disclosure",
        Enum,
        name_of(&x.served.disclosure).into(),
    );
    if let Some(g) = &x.generation_id {
        b.put("gen_ai.response.id", Id, g.as_str().into());
    }
    if let Some(keys) = input.bindings.usage_keys.get(&att.backend_id) {
        // Absent, never 0, when not reported.
        if let Some(n) = keys.input.as_ref().and_then(|k| x.usage.get(k)) {
            b.put("gen_ai.usage.input_tokens", Number, num(*n));
            facts.input_tokens = Some(*n);
        }
        if let Some(n) = keys.output.as_ref().and_then(|k| x.usage.get(k)) {
            b.put("gen_ai.usage.output_tokens", Number, num(*n));
            facts.output_tokens = Some(*n);
        }
    }
    if let Some(dg) = &x.request_digest {
        b.put(
            "rustev.exchange.request_digest",
            ContentDigest,
            dg.as_str().into(),
        );
    }
    if let Some(dg) = &x.response_digest {
        b.put(
            "rustev.exchange.response_digest",
            ContentDigest,
            dg.as_str().into(),
        );
    }
    if let Some(s) = x.status {
        b.put(
            "rustev.exchange.status",
            Number,
            TraceValue::Int(i64::from(s)),
        );
    }
    b.put("rustev.exchange.bytes_sent", Enum, x.bytes_sent.into());
    b.put("rustev.exchange.late", Enum, x.late.into());
    b.put("rustev.exchange.truncated", Enum, x.truncated.into());
    if !x.route.is_empty() {
        b.put("rustev.exchange.route", ModelIdentity, strs(&x.route));
    }
    if let Some(c) = &x.reported_cost {
        b.put(
            "rustev.exchange.reported_cost",
            Number,
            c.amount.as_str().into(),
        );
        b.put(
            "rustev.exchange.reported_currency",
            Enum,
            c.currency.as_str().into(),
        );
    }
    if let Some(r) = &x.retry_after {
        b.put("rustev.exchange.retry_after", ThirdParty, r.as_str().into());
    }
    for (k, v) in &x.provider_extras {
        b.put(
            &format!("rustev.exchange.extra.{k}"),
            ThirdParty,
            v.as_str().into(),
        );
    }
    if let Some(m) = x.members.iter().find(|m| m.attempt_id == att.attempt_id)
        && let MemberOutcome::Failure { detail, .. } = &m.outcome
    {
        b.put(
            "rustev.exchange.member_detail",
            FreeTextDetail,
            detail.as_str().into(),
        );
    }
    Some(facts)
}

fn policy_attributes(b: &mut SpanBuilder<'_>, ev: &EvidenceRecord, ctx: &Ctx<'_>, at: u64) {
    use FieldClass::*;
    let j = &ev.judgment;
    b.put("rustev.policy.rules_fired", PlanMetadata, strs(&j.trace));
    let Outcome::Propose { params, .. } = &j.outcome else {
        return;
    };
    let inputs = ctx.lineage_inputs();
    let mut excluded = None::<u64>;
    let mut truncated = None::<u64>;
    let mut excl = |b: &mut SpanBuilder<'_>, list: &[rustev_contract::judgment::Excluded]| {
        *excluded.get_or_insert(0) += list.len() as u64;
        for e in list {
            b.event(
                event::EXCLUDED,
                at,
                vec![
                    ("candidate", Id, e.id.as_str().into()),
                    ("reasons", PlanMetadata, strs(&e.reasons)),
                ],
            );
        }
    };
    for v in params.values() {
        match v {
            OutValue::Filtered(f) => excl(b, &f.excluded),
            OutValue::Shortlist(s) => {
                *truncated.get_or_insert(0) += s.truncated;
                excl(b, &s.excluded);
            }
            OutValue::Ranking(r) => {
                excl(b, &r.excluded);
                let class = lineage_class(false, inputs.iter().copied());
                for e in &r.entries {
                    b.event(
                        event::POSITION,
                        at,
                        vec![
                            ("position", Number, num(e.position)),
                            ("candidate", Id, e.candidate.as_str().into()),
                            ("score", class, TraceValue::Double(e.score)),
                        ],
                    );
                }
            }
            _ => {}
        }
    }
    if let Some(n) = excluded {
        b.put("rustev.policy.excluded_count", Number, num(n));
    }
    if let Some(n) = truncated {
        b.put("rustev.policy.truncated", Number, num(n));
    }
}
