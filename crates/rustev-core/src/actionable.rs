//! Provenance-conditional checks and actionable outcomes (spec 033).
//!
//! Compilation (3.8): the structural rules of phase (b), the types of phase
//! (e), the possible reasons checks add for phase (h), and phase (h2), which
//! checks exhaustiveness and caller addressing and builds plan 3's static
//! table of causes and parties. Evaluation follows in later increments; a
//! definition with `actionability` is compiled but not yet evaluated.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::actionability::{
    Address, BUILTIN_REMEDIATIONS, CALLER_CONTROLLABLE, Cause, CauseProvenance, Consequence,
    DefaultConsequence, Origin, Party, PlanActionability, RemediationUse, StaticBranch,
    StaticCheck, StaticSite, is_code,
};
use rustev_contract::definition::{
    Cond, Definition, HandlerAction, InputDecl, OutcomeDecl, ParamSource, ProvenanceClass,
    ReasonKind, ReasonSet, TypeDecl,
};
use rustev_contract::grade::Grade;

use crate::compile::{Category as C, Refusal};
use crate::expr::{Ref, TyEnv, predicate_refs, type_predicate};
use crate::value::Ty;

fn refusal(category: C, subject: impl Into<String>, detail: impl Into<String>) -> Refusal {
    Refusal {
        category,
        subject: subject.into(),
        detail: detail.into(),
        shortfalls: vec![],
    }
}

fn invalid(subject: impl Into<String>, detail: impl Into<String>) -> Refusal {
    refusal(C::InvalidDefinition, subject, detail)
}

/// The first construct of spec 033 in `def`, with its subject, in the order
/// the definition declares them: the `actionability` member, then policy
/// handlers, rules and adjustments.
pub(crate) fn first_construct(def: &Definition) -> Option<(String, &'static str)> {
    if def.actionability.is_some() {
        return Some(("definition".into(), "an actionability declaration"));
    }
    let policy = &def.policy;
    for (n, h) in policy.on_unresolved.iter().enumerate() {
        match h.action {
            HandlerAction::PropagateWith { .. } => {
                return Some((handler_subject(n), "a propagate_with handler"));
            }
            HandlerAction::NeedsInput { .. } => {
                return Some((handler_subject(n), "a needs_input handler"));
            }
            _ => {}
        }
    }
    for r in &policy.rules {
        let subject = format!("policy:rule:{}", r.id);
        if cond_uses(&r.when) {
            return Some((subject, "a provenance_within condition"));
        }
        if matches!(r.then, OutcomeDecl::NeedsInput { .. }) {
            return Some((subject, "a needs_input outcome"));
        }
    }
    // An adjustment has a condition and no outcome of its own (spec 002,
    // 3.11), so only its condition can hold a construct of this spec.
    for a in &policy.adjustments {
        if cond_uses(&a.when) {
            return Some((
                format!("policy:adjustment:{}", a.id),
                "a provenance_within condition",
            ));
        }
    }
    None
}

/// A handler's refusal subject, as spec 002's policy structure names it.
pub(crate) fn handler_subject(n: usize) -> String {
    format!("policy:on_unresolved[{n}]")
}

/// True when the condition contains `provenance_within`.
pub(crate) fn cond_uses(c: &Cond) -> bool {
    match c {
        Cond::ProvenanceWithin { .. } => true,
        Cond::All(cs) | Cond::Any(cs) => cs.iter().any(cond_uses),
        Cond::Not(c) => cond_uses(c),
        _ => false,
    }
}

/// The party of one provenance class (3.5.2).
pub(crate) fn class_party(class: ProvenanceClass, caller_controlled: &[ProvenanceClass]) -> Party {
    match class {
        c if caller_controlled.contains(&c) => Party::Caller,
        ProvenanceClass::ThirdParty | ProvenanceClass::SystemOfRecord => Party::Upstream,
        _ => Party::Host,
    }
}

/// The party of one cause (3.5.2).
pub(crate) fn cause_party(cause: &Cause, caller_controlled: &[ProvenanceClass]) -> Party {
    match cause {
        Cause::Input { provenance, .. } => match provenance {
            CauseProvenance::Observed { class } => class_party(*class, caller_controlled),
            CauseProvenance::Accepted { classes } => {
                Party::combine(classes.iter().map(|c| class_party(*c, caller_controlled)))
            }
        },
        Cause::Backend { origin, .. } => match origin {
            Origin::Upstream => Party::Upstream,
            Origin::Host => Party::Host,
        },
        Cause::Budget { .. } | Cause::Deadline | Cause::Capability { .. } => Party::Host,
        Cause::Policy { .. } => Party::None,
    }
}

/// The computed party of a list of causes under the declared
/// caller-controlled classes (3.5.2): each cause's party, combined.
pub fn computed_party(causes: &[Cause], caller_controlled: &[ProvenanceClass]) -> Party {
    Party::combine(causes.iter().map(|c| cause_party(c, caller_controlled)))
}

/// Duplicate-free causes sorted by their canonical JSON bytes (3.5.2).
pub(crate) fn sort_causes(causes: Vec<Cause>) -> Vec<Cause> {
    let mut keyed: Vec<(Vec<u8>, Cause)> = causes
        .into_iter()
        .map(|c| {
            let bytes = rustev_contract::canonical::record_canonical_bytes(&c).unwrap_or_default();
            (bytes, c)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.dedup_by(|a, b| a.0 == b.0);
    keyed.into_iter().map(|(_, c)| c).collect()
}

fn accepted(field: &str, classes: Vec<ProvenanceClass>) -> Cause {
    let mut classes = classes;
    classes.sort();
    classes.dedup();
    Cause::Input {
        field: field.into(),
        provenance: CauseProvenance::Accepted { classes },
    }
}

/// Parameter names of the built-in remediations an author may use (3.6.2).
fn builtin_params(code: &str) -> Option<&'static [&'static str]> {
    Some(match code {
        "provide_fields" | "refresh_fields" | "correct_fields" | "improve_evidence" => &["fields"],
        "resolve_conflict" => &["facts"],
        "retry" => &[],
        "report_backend" => &["step"],
        "raise_budget" => &["resource"],
        "provide_capability" => &["name"],
        "retry_after" => &["ms"],
        _ => return None,
    })
}

fn scalar(t: &TypeDecl) -> bool {
    matches!(
        t,
        TypeDecl::Bool
            | TypeDecl::Integer
            | TypeDecl::Decimal
            | TypeDecl::Text { .. }
            | TypeDecl::Timestamp
            | TypeDecl::Enum { .. }
    )
}

struct Names<'a> {
    def: &'a Definition,
    inputs: &'a BTreeMap<String, (Ty, InputDecl)>,
}

impl Names<'_> {
    fn remediation(&self, subject: &str, r: &RemediationUse) -> Result<(), Refusal> {
        let given: BTreeSet<&str> = match r {
            RemediationUse::Code(_) => BTreeSet::new(),
            RemediationUse::WithParams(c) => c.params.keys().map(String::as_str).collect(),
        };
        let code = r.code();
        let wanted: BTreeSet<&str> = match builtin_params(code) {
            Some(p) => p.iter().copied().collect(),
            None => match self
                .def
                .actionability
                .as_ref()
                .and_then(|a| a.remediations.iter().find(|d| d.code == code))
            {
                Some(d) => d.params.iter().map(|p| p.name.as_str()).collect(),
                None => {
                    return Err(invalid(subject, format!("unknown remediation {code:?}")));
                }
            },
        };
        if given != wanted {
            return Err(invalid(
                subject,
                format!("remediation {code:?} takes exactly the parameters {wanted:?}"),
            ));
        }
        if let RemediationUse::WithParams(c) = r {
            for (name, p) in &c.params {
                if let ParamSource::Ref(x) = p {
                    match Ref::parse(x) {
                        Ok(Ref::Input(n, _)) if self.inputs.contains_key(&n) => {}
                        _ => {
                            return Err(invalid(
                                subject,
                                format!("parameter {name:?} reads {x:?}, not a declared input"),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn address(&self, subject: &str, a: &Address) -> Result<(), Refusal> {
        if let Address::Override(o) = a {
            if o.to == Party::Caller {
                return Err(refusal(
                    C::CallerCannotAct,
                    subject,
                    "override-to-caller: an override moves an outcome away from the caller",
                ));
            }
            if !is_code(&o.reason) {
                return Err(invalid(
                    subject,
                    format!("override reason {:?} is not a code", o.reason),
                ));
            }
        }
        Ok(())
    }

    fn code(&self, subject: &str, what: &str, c: &str) -> Result<(), Refusal> {
        if is_code(c) {
            Ok(())
        } else {
            Err(invalid(subject, format!("{what} {c:?} is not a code")))
        }
    }

    fn consequence(&self, subject: &str, check: &str, k: &Consequence) -> Result<(), Refusal> {
        let a = self.def.actionability.as_ref();
        match k {
            Consequence::Refuse {
                remediation,
                address,
            } => {
                self.remediation(subject, remediation)?;
                self.address(subject, address)
            }
            Consequence::DropField { notice, address }
            | Consequence::ContinueWithNotice { notice, address } => {
                self.code(subject, "notice", notice)?;
                self.address(subject, address)
            }
            Consequence::Relax {
                group,
                notice,
                address,
            } => {
                if !a.is_some_and(|a| a.constraint_groups.iter().any(|g| &g.name == group)) {
                    return Err(invalid(
                        subject,
                        format!("unknown constraint group {group:?}"),
                    ));
                }
                self.code(subject, "notice", notice)?;
                self.address(subject, address)
            }
            Consequence::DowngradeGrade {
                to,
                notice,
                address,
            } => {
                let subject_input = a
                    .and_then(|a| a.checks.iter().find(|c| c.id == check))
                    .and_then(|c| self.inputs.get(&c.subject));
                if !subject_input.is_some_and(|(_, d)| d.evidence.is_some()) {
                    return Err(invalid(
                        subject,
                        "ungraded-subject: downgrade_grade needs a graded subject",
                    ));
                }
                if *to >= Grade::Authoritative {
                    return Err(invalid(
                        subject,
                        "downgrade_grade must name a grade below authoritative",
                    ));
                }
                self.code(subject, "notice", notice)?;
                self.address(subject, address)
            }
            Consequence::Abstain => Ok(()),
            Consequence::Escalate { reason } => self.code(subject, "escalation reason", reason),
        }
    }

    fn needs_input(
        &self,
        subject: &str,
        fields: &[String],
        questions: &[String],
    ) -> Result<(), Refusal> {
        let set: BTreeSet<&String> = fields.iter().collect();
        if fields.is_empty() || set.len() != fields.len() {
            return Err(invalid(subject, "needs_input names unique fields"));
        }
        for f in fields {
            if !self.inputs.contains_key(f) {
                return Err(invalid(subject, format!("unknown input {f:?}")));
            }
        }
        let declared = self
            .def
            .actionability
            .as_ref()
            .map(|a| a.questions.as_slice())
            .unwrap_or_default();
        let qset: BTreeSet<&String> = questions.iter().collect();
        if qset.len() != questions.len() {
            return Err(invalid(subject, "needs_input names each question once"));
        }
        for q in questions {
            let Some(t) = declared.iter().find(|t| &t.id == q) else {
                return Err(invalid(subject, format!("unknown question {q:?}")));
            };
            if !t.fields.iter().all(|f| set.contains(f)) {
                return Err(invalid(
                    subject,
                    format!("question {q:?} asks for a field the outcome does not name"),
                ));
            }
        }
        Ok(())
    }
}

/// Phase (b): every structural rule of 3.1 to 3.7, and category 17's phase
/// (b) cases (3.8.2), in declaration order after the existing structure.
pub(crate) fn structure(
    def: &Definition,
    inputs: &BTreeMap<String, (Ty, InputDecl)>,
    taken_ids: &BTreeSet<String>,
) -> Result<(), Refusal> {
    let names = Names { def, inputs };
    if let Some(screening) = &def.screening {
        for entry in &screening.entries {
            names.address(&format!("screening:{}", entry.id), &entry.address)?;
        }
    }
    let policy = &def.policy;
    for (n, h) in policy.on_unresolved.iter().enumerate() {
        let subject = handler_subject(n);
        match &h.action {
            HandlerAction::PropagateWith {
                remediation,
                address,
            } => {
                if let Some(r) = remediation {
                    names.remediation(&subject, r)?;
                }
                names.address(&subject, address)?;
            }
            HandlerAction::NeedsInput { fields, questions } => {
                names.needs_input(&subject, fields, questions)?
            }
            _ => {}
        }
    }
    for r in &policy.rules {
        let subject = format!("policy:rule:{}", r.id);
        provenance_conds(&r.when, &subject, inputs)?;
        if let OutcomeDecl::NeedsInput { fields, questions } = &r.then {
            names.needs_input(&subject, fields, questions)?;
        }
    }
    for a in &policy.adjustments {
        provenance_conds(&a.when, &format!("policy:adjustment:{}", a.id), inputs)?;
    }
    let Some(a) = &def.actionability else {
        return Ok(());
    };
    for c in &a.caller_controlled {
        if !CALLER_CONTROLLABLE.contains(c) {
            return Err(refusal(
                C::CallerCannotAct,
                "definition",
                format!(
                    "not-caller-controllable: {} cannot be caller-controlled",
                    class_name(*c)
                ),
            ));
        }
    }
    if a.caller_controlled.windows(2).any(|w| w[0] >= w[1]) {
        return Err(invalid(
            "definition",
            "caller_controlled must be sorted and duplicate-free",
        ));
    }
    let mut codes = BTreeSet::new();
    for r in &a.remediations {
        let subject = format!("remediation:{}", r.code);
        if !is_code(&r.code)
            || BUILTIN_REMEDIATIONS.contains(&r.code.as_str())
            || !codes.insert(&r.code)
        {
            return Err(invalid(
                subject,
                "a declared remediation code is a unique code that is not built in",
            ));
        }
        if r.for_party == Party::None {
            return Err(invalid(
                subject,
                "a remediation is for caller, host or upstream",
            ));
        }
        let mut params = BTreeSet::new();
        for p in &r.params {
            if !is_code(&p.name) || !params.insert(&p.name) || !scalar(&p.ty) {
                return Err(invalid(
                    subject,
                    format!(
                        "parameter {:?} needs a unique name and a scalar type",
                        p.name
                    ),
                ));
            }
        }
    }
    let mut qids = BTreeSet::new();
    for q in &a.questions {
        let subject = format!("question:{}", q.id);
        if !valid_name(&q.id) || !qids.insert(&q.id) {
            return Err(invalid(subject, "question ids are unique and valid"));
        }
        if q.fields.is_empty() || q.fields.iter().any(|f| !inputs.contains_key(f)) {
            return Err(invalid(subject, "a question asks for declared inputs"));
        }
        if q.text.is_empty() || q.text.len() > 1024 {
            return Err(invalid(subject, "a question's text is 1 to 1024 bytes"));
        }
    }
    let check_ids: Vec<&String> = a.checks.iter().map(|c| &c.id).collect();
    let mut groups = BTreeSet::new();
    for g in &a.constraint_groups {
        let subject = format!("group:{}", g.name);
        if !valid_name(&g.name) || !groups.insert(&g.name) {
            return Err(invalid(subject, "group names are unique and valid"));
        }
        if g.checks.is_empty() || g.checks.iter().any(|c| !check_ids.contains(&c)) {
            return Err(invalid(
                subject,
                "a group names one or more declared checks",
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for (i, c) in a.checks.iter().enumerate() {
        let subject = format!("check:{}", c.id);
        if !valid_name(&c.id) || taken_ids.contains(&c.id) || !seen.insert(&c.id) {
            return Err(invalid(
                subject,
                "check ids are unique among checks, steps and policy items",
            ));
        }
        let Some((_, decl)) = inputs.get(&c.subject) else {
            return Err(invalid(subject, format!("unknown subject {:?}", c.subject)));
        };
        for class in c.on_fail.by_provenance.keys() {
            if !decl.provenance.contains(class) {
                return Err(invalid(
                    subject,
                    format!(
                        "unreachable-branch: {} is not accepted by {}",
                        class_name(*class),
                        c.subject
                    ),
                ));
            }
        }
        let branches = c
            .on_fail
            .by_provenance
            .values()
            .chain(match &c.on_fail.default {
                DefaultConsequence::None => None,
                DefaultConsequence::Consequence(k) => Some(k),
            });
        for k in branches {
            names.consequence(&subject, &c.id, k)?;
            if let Consequence::Relax { group, .. } = k {
                let g = a.constraint_groups.iter().find(|g| &g.name == group);
                let after = |id: &String| {
                    a.checks
                        .iter()
                        .position(|x| &x.id == id)
                        .is_some_and(|p| p > i)
                };
                if !g.is_some_and(|g| g.checks.iter().all(after)) {
                    return Err(invalid(
                        subject,
                        format!(
                            "relax-order: group {group:?} names a check not declared after {}",
                            c.id
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn provenance_conds(
    c: &Cond,
    subject: &str,
    inputs: &BTreeMap<String, (Ty, InputDecl)>,
) -> Result<(), Refusal> {
    match c {
        Cond::ProvenanceWithin { reference, classes } => {
            if classes.is_empty() || classes.windows(2).any(|w| w[0] >= w[1]) {
                return Err(invalid(
                    subject,
                    "provenance_within classes are non-empty, sorted and duplicate-free",
                ));
            }
            match Ref::parse(reference) {
                Ok(Ref::Input(n, p)) if p.is_empty() && inputs.contains_key(&n) => Ok(()),
                Ok(Ref::Step(_)) => Ok(()),
                _ => Err(invalid(
                    subject,
                    format!("provenance_within reads {reference:?}, not an input or step"),
                )),
            }
        }
        Cond::All(cs) | Cond::Any(cs) => cs
            .iter()
            .try_for_each(|c| provenance_conds(c, subject, inputs)),
        Cond::Not(c) => provenance_conds(c, subject, inputs),
        _ => Ok(()),
    }
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && !s.contains(':') && !s.chars().any(char::is_whitespace)
}

pub(crate) fn class_name(c: ProvenanceClass) -> String {
    serde_json::to_value(c)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Phase (e): each check's test is a `bool` predicate over inputs, `now` and
/// literals (it reads no step), and each remediation parameter has its
/// declared type (category 3).
pub(crate) fn types(
    def: &Definition,
    input_tys: &BTreeMap<String, Ty>,
    v2: bool,
) -> Result<(), Refusal> {
    let Some(a) = &def.actionability else {
        return Ok(());
    };
    let no_steps = BTreeMap::new();
    let env = TyEnv {
        inputs: input_tys,
        steps: &no_steps,
        item: None,
        v2,
    };
    for c in &a.checks {
        let subject = format!("check:{}", c.id);
        // Before typing, whose empty step map would only say "unknown step".
        let mut refs = Vec::new();
        predicate_refs(&c.test, &mut refs);
        if let Some(r) = refs.iter().find(|r| r.starts_with("step:")) {
            return Err(refusal(
                C::KindMismatch,
                subject,
                format!("a check reads inputs, now and literals only, not {r:?}"),
            ));
        }
        type_predicate(&c.test, &env).map_err(|e| refusal(C::KindMismatch, &subject, e))?;
        let branches = c
            .on_fail
            .by_provenance
            .values()
            .chain(match &c.on_fail.default {
                DefaultConsequence::None => None,
                DefaultConsequence::Consequence(k) => Some(k),
            });
        for k in branches {
            if let Consequence::Refuse {
                remediation: RemediationUse::WithParams(call),
                ..
            } = k
            {
                remediation_types(&subject, &call.code, &call.params, a, &env)?;
            }
        }
    }
    for (n, h) in def.policy.on_unresolved.iter().enumerate() {
        if let HandlerAction::PropagateWith {
            remediation: Some(RemediationUse::WithParams(call)),
            ..
        } = &h.action
        {
            remediation_types(&handler_subject(n), &call.code, &call.params, a, &env)?;
        }
    }
    Ok(())
}

fn remediation_types(
    subject: &str,
    code: &str,
    params: &BTreeMap<String, ParamSource>,
    a: &rustev_contract::actionability::Actionability,
    env: &TyEnv<'_>,
) -> Result<(), Refusal> {
    let declared: BTreeMap<&str, Ty> = match a.remediations.iter().find(|d| d.code == code) {
        Some(d) => d
            .params
            .iter()
            .map(|p| (p.name.as_str(), Ty::from_decl(&p.ty)))
            .collect(),
        None => builtin_params(code)
            .unwrap_or_default()
            .iter()
            .map(|n| {
                let ty = match *n {
                    "ms" => Ty::Integer,
                    "fields" | "facts" => Ty::List {
                        item: Box::new(Ty::Text {
                            max_bytes: u64::MAX,
                        }),
                        max: u64::MAX,
                    },
                    _ => Ty::Text {
                        max_bytes: u64::MAX,
                    },
                };
                (*n, ty)
            })
            .collect(),
    };
    for (name, p) in params {
        let Some(want) = declared.get(name.as_str()) else {
            continue;
        };
        let got = match p {
            ParamSource::Lit(l) => crate::value::Value::literal_ty(l),
            ParamSource::Ref(r) => {
                crate::expr::ref_ty(r, env).map_err(|e| refusal(C::KindMismatch, subject, e))?
            }
        };
        if !same_kind(&got, want) {
            return Err(refusal(
                C::KindMismatch,
                subject,
                format!("parameter {name:?} is {got:?}, not {want:?}"),
            ));
        }
    }
    Ok(())
}

/// Types agree in kind; text and list bounds do not matter for a parameter.
fn same_kind(a: &Ty, b: &Ty) -> bool {
    match (a, b) {
        (Ty::Text { .. }, Ty::Text { .. }) => true,
        (Ty::List { item: x, .. }, Ty::List { item: y, .. }) => same_kind(x, y),
        (Ty::Enum(_), Ty::Enum(_)) => true,
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

/// Phase (h): what checks add to the reasons a subject can have, applied to
/// the input reasons before any step's reasons are derived from them. A check
/// whose test cannot be evaluated makes its subject unresolved with the
/// reason of the read that failed (3.4.3); a downgrade can leave a graded
/// read below its requirement (spec 032's `insufficient_grade`).
pub(crate) fn extend_reasons(
    def: &Definition,
    reasons: &mut BTreeMap<String, BTreeSet<ReasonKind>>,
) {
    let Some(a) = &def.actionability else {
        return;
    };
    // To a fixpoint: a check's subject can feed another check's test in any
    // declaration order, and each pass only adds reasons.
    loop {
        let before = reasons.clone();
        extend_once(a, reasons);
        if *reasons == before {
            break;
        }
    }
}

fn extend_once(
    a: &rustev_contract::actionability::Actionability,
    reasons: &mut BTreeMap<String, BTreeSet<ReasonKind>>,
) {
    for c in &a.checks {
        let subject = format!("input:{}", c.subject);
        let mut refs = Vec::new();
        predicate_refs(&c.test, &mut refs);
        let mut add: BTreeSet<ReasonKind> = BTreeSet::new();
        for r in refs.iter().filter(|r| r.starts_with("input:")) {
            add.extend(
                reasons
                    .get(&crate::expr::base_ref(r))
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        // A test that faults (overflow, invalid comparison) is invalid input.
        add.insert(ReasonKind::InvalidInput);
        let downgrades = c
            .on_fail
            .by_provenance
            .values()
            .chain(match &c.on_fail.default {
                DefaultConsequence::None => None,
                DefaultConsequence::Consequence(k) => Some(k),
            })
            .any(|k| matches!(k, Consequence::DowngradeGrade { .. }));
        if downgrades {
            add.insert(ReasonKind::InsufficientGrade);
        }
        reasons.entry(subject).or_default().extend(add);
    }
}

/// Every input a step reads, transitively, in name order.
fn lineage(step: &str, deps: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![step.to_string()];
    let mut seen = BTreeSet::new();
    while let Some(s) = stack.pop() {
        if !seen.insert(s.clone()) {
            continue;
        }
        for d in deps.get(&s).into_iter().flatten() {
            if let Some(n) = d.strip_prefix("input:") {
                out.insert(n.split('/').next().unwrap_or(n).to_string());
            } else if let Some(x) = d.strip_prefix("step:") {
                stack.push(crate::expr::split_step(x).0);
            }
        }
    }
    out.into_iter().collect()
}

/// The static causes of `reason` on `target` (3.5.6, with accepted classes):
/// a backend's origin is not known before registration, so its static
/// cause is the stronger, `upstream`.
fn static_reason_causes(
    reason: ReasonKind,
    target: &str,
    inputs: &BTreeMap<String, (Ty, InputDecl)>,
    deps: &BTreeMap<String, Vec<String>>,
    site: &str,
) -> Vec<Cause> {
    use ReasonKind as K;
    let fields = || -> Vec<String> {
        match target.strip_prefix("input:") {
            Some(n) => vec![n.to_string()],
            None => lineage(target.strip_prefix("step:").unwrap_or(target), deps),
        }
    };
    let input_causes = || {
        fields()
            .into_iter()
            .filter_map(|f| {
                inputs
                    .get(&f)
                    .map(|(_, d)| accepted(&f, d.provenance.clone()))
            })
            .collect()
    };
    match reason {
        K::MissingEvidence
        | K::StaleEvidence
        | K::InvalidInput
        | K::Conflict
        | K::InsufficientGrade => input_causes(),
        K::BackendUnavailable | K::InvalidBackendOutput => vec![Cause::Backend {
            step: target.strip_prefix("step:").unwrap_or(target).to_string(),
            origin: Origin::Upstream,
        }],
        K::BudgetExhausted => vec![Cause::Budget {
            resource: "semantic_requests".into(),
        }],
        K::DeadlineExceeded => vec![Cause::Deadline],
        K::Unsupported => vec![Cause::Capability {
            name: target.strip_prefix("step:").unwrap_or(target).to_string(),
        }],
        K::ScreenUnavailable => Vec::new(),
        K::Abstained | K::Skipped => vec![Cause::Policy { site: site.into() }],
    }
}

/// Phase (h2): exhaustiveness (category 16) and caller addressing
/// (category 17), then the static table of plan 3 (3.8.3).
pub(crate) fn phase_h2(
    def: &Definition,
    inputs: &BTreeMap<String, (Ty, InputDecl)>,
    deps: &BTreeMap<String, Vec<String>>,
    reasons: &BTreeMap<String, BTreeSet<ReasonKind>>,
) -> Result<Option<PlanActionability>, Refusal> {
    let Some(a) = &def.actionability else {
        return Ok(None);
    };
    let cc = &a.caller_controlled;
    let party = |causes: &[Cause]| Party::combine(causes.iter().map(|c| cause_party(c, cc)));
    let for_caller = |r: &RemediationUse| {
        a.remediations
            .iter()
            .any(|d| d.code == r.code() && d.for_party == Party::Caller)
    };
    let mut checks = Vec::new();
    for c in &a.checks {
        let subject = format!("check:{}", c.id);
        let decl = &inputs[&c.subject].1;
        let uncovered: Vec<ProvenanceClass> = sorted(
            decl.provenance
                .iter()
                .filter(|p| !c.on_fail.by_provenance.contains_key(p))
                .copied(),
        );
        if matches!(c.on_fail.default, DefaultConsequence::None) && !uncovered.is_empty() {
            return Err(refusal(
                C::UncoveredProvenance,
                subject,
                format!(
                    "uncovered_provenance: {} accepts {} without a branch or default",
                    c.subject,
                    uncovered
                        .iter()
                        .map(|c| class_name(*c))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
        let mut reads = Vec::new();
        predicate_refs(&c.test, &mut reads);
        let others: Vec<Cause> = reads
            .iter()
            .filter_map(|r| r.strip_prefix("input:"))
            .map(|r| r.split('/').next().unwrap_or(r).to_string())
            .filter(|n| n != &c.subject)
            .filter_map(|n| {
                inputs
                    .get(&n)
                    .map(|(_, d)| accepted(&n, d.provenance.clone()))
            })
            .collect();
        let mut branches = Vec::new();
        let mut branch = |classes: Vec<ProvenanceClass>, k: &Consequence| -> Result<(), Refusal> {
            let mut causes = others.clone();
            causes.push(accepted(&c.subject, classes.clone()));
            let causes = sort_causes(causes);
            let p = party(&causes);
            if let Consequence::Refuse { remediation, .. } = k
                && for_caller(remediation)
                && p != Party::Caller
            {
                return Err(refusal(
                    C::CallerCannotAct,
                    &subject,
                    format!(
                        "remediation {:?} is for the caller, but this branch's static party is {}: {}",
                        remediation.code(),
                        party_name(p),
                        not_caller(&causes, cc)
                    ),
                ));
            }
            branches.push(StaticBranch {
                classes,
                consequence: k.kind().into(),
                causes,
                party: p,
            });
            Ok(())
        };
        for (class, k) in &c.on_fail.by_provenance {
            branch(vec![*class], k)?;
        }
        if let DefaultConsequence::Consequence(k) = &c.on_fail.default
            && !uncovered.is_empty()
        {
            branch(uncovered, k)?;
        }
        checks.push(StaticCheck {
            check: c.id.clone(),
            subject: c.subject.clone(),
            branches,
        });
    }
    let mut sites = Vec::new();
    for (n, h) in def.policy.on_unresolved.iter().enumerate() {
        let site = format!("handler:{n}");
        // Only the reasons the target can produce (spec 002, 3.11.2): a
        // listed reason it never produces (a backend reason on an input)
        // contributes no cause.
        let possible = reasons.get(&h.target);
        let covered: Vec<ReasonKind> = match &h.reasons {
            ReasonSet::Any => possible.into_iter().flatten().copied().collect(),
            ReasonSet::Only(v) => v
                .iter()
                .filter(|r| possible.is_some_and(|p| p.contains(r)))
                .copied()
                .collect(),
        };
        let causes = sort_causes(
            covered
                .iter()
                .flat_map(|r| static_reason_causes(*r, &h.target, inputs, deps, &site))
                .collect(),
        );
        let p = party(&causes);
        match &h.action {
            HandlerAction::PropagateWith {
                remediation: Some(r),
                ..
            } if for_caller(r) && p != Party::Caller => {
                return Err(refusal(
                    C::CallerCannotAct,
                    handler_subject(n),
                    format!(
                        "remediation {:?} is for the caller, but this handler's static party is {}",
                        r.code(),
                        party_name(p)
                    ),
                ));
            }
            HandlerAction::NeedsInput { fields, .. } => {
                needs_caller_fields(&handler_subject(n), fields, inputs, cc)?;
                sites.push(needs_site(site, fields, inputs, cc));
                continue;
            }
            _ => {}
        }
        sites.push(StaticSite {
            site,
            causes,
            party: p,
        });
    }
    for r in &def.policy.rules {
        if let OutcomeDecl::NeedsInput { fields, .. } = &r.then {
            needs_caller_fields(&format!("policy:rule:{}", r.id), fields, inputs, cc)?;
            sites.push(needs_site(format!("rule:{}", r.id), fields, inputs, cc));
        }
    }
    Ok(Some(PlanActionability { checks, sites }))
}

fn needs_caller_fields(
    subject: &str,
    fields: &[String],
    inputs: &BTreeMap<String, (Ty, InputDecl)>,
    cc: &[ProvenanceClass],
) -> Result<(), Refusal> {
    for f in fields {
        // `structure` refused unknown fields first; refuse again rather
        // than index if that order ever changes.
        let Some((_, d)) = inputs.get(f) else {
            return Err(invalid(subject, format!("unknown input {f:?}")));
        };
        if !d.provenance.iter().any(|p| cc.contains(p)) {
            return Err(refusal(
                C::CallerCannotAct,
                subject,
                format!("field-not-caller-supplied: {f} accepts no caller-controlled class"),
            ));
        }
    }
    Ok(())
}

/// A `needs_input` site: addressed to the caller by construction (3.7.2).
fn needs_site(
    site: String,
    fields: &[String],
    inputs: &BTreeMap<String, (Ty, InputDecl)>,
    cc: &[ProvenanceClass],
) -> StaticSite {
    let causes = sort_causes(
        fields
            .iter()
            .map(|f| {
                // Checked by `needs_caller_fields` before any site is built.
                let classes = inputs
                    .get(f)
                    .map(|(_, d)| d.provenance.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .filter(|p| cc.contains(p))
                    .copied()
                    .collect();
                accepted(f, classes)
            })
            .collect(),
    );
    StaticSite {
        site,
        causes,
        party: Party::Caller,
    }
}

fn sorted<I: IntoIterator<Item = ProvenanceClass>>(it: I) -> Vec<ProvenanceClass> {
    let mut v: Vec<_> = it.into_iter().collect();
    v.sort();
    v.dedup();
    v
}

/// The causes the caller cannot act on, named with their classes.
fn not_caller(causes: &[Cause], cc: &[ProvenanceClass]) -> String {
    causes
        .iter()
        .filter(|c| cause_party(c, cc) != Party::Caller)
        .map(|c| match c {
            Cause::Input { field, provenance } => {
                let classes = match provenance {
                    CauseProvenance::Observed { class } => vec![*class],
                    CauseProvenance::Accepted { classes } => classes.clone(),
                };
                let named: Vec<String> = classes
                    .iter()
                    .filter(|x| class_party(**x, cc) != Party::Caller)
                    .map(|x| class_name(*x))
                    .collect();
                format!("{field} ({})", named.join(", "))
            }
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn party_name(p: Party) -> &'static str {
    match p {
        Party::None => "none",
        Party::Caller => "caller",
        Party::Host => "host",
        Party::Upstream => "upstream",
    }
}

// ---------------------------------------------------------------------------
// Evaluation (3.4 to 3.7).

/// A notice a check emitted, before its action is computed.
#[derive(Debug, Clone)]
pub(crate) struct PendingNotice {
    pub kind: rustev_contract::judgment::NoticeKind,
    pub subject: String,
    pub reason: String,
    pub causes: Vec<Cause>,
    pub address: Address,
}

/// A refusing check, before its action is computed.
#[derive(Debug, Clone)]
pub(crate) struct PendingRefusal {
    pub check: String,
    pub subject: String,
    pub causes: Vec<Cause>,
    pub remediation: RemediationUse,
    pub address: Address,
}

/// The terminal outcome checks decided (3.4.6).
#[derive(Debug, Clone)]
pub(crate) enum Terminal {
    Refused,
    Abstain { check: String },
    Escalate { check: String, reason: String },
}

/// What the checks of one evaluation did.
#[derive(Debug, Clone, Default)]
pub(crate) struct CheckRun {
    pub records: Vec<rustev_contract::actionability::CheckRecord>,
    /// Dropped input -> the check that dropped it.
    pub dropped: BTreeMap<String, String>,
    pub notices: Vec<PendingNotice>,
    pub refusals: Vec<PendingRefusal>,
    pub terminal: Option<Terminal>,
    /// The input references the deciding checks read (3.4.6).
    pub deciding_reads: Vec<String>,
}

/// What an action is computed from, beyond its causes.
pub(crate) struct ActionInputs<'a> {
    pub caller_controlled: &'a [ProvenanceClass],
    /// An explicit address of the site, if any (3.5.3).
    pub address: &'a Address,
    /// A declared or built-in remediation already resolved to its value,
    /// with the party a declared one is `for` (a built-in has none); `None`
    /// uses the computed remediation.
    pub declared: Option<(rustev_contract::actionability::Remediation, Option<Party>)>,
    pub computed: rustev_contract::actionability::Remediation,
    pub transient: bool,
}

/// Build an action from its causes (3.5.2, 3.5.3).
pub(crate) fn action(
    causes: Vec<Cause>,
    a: ActionInputs<'_>,
) -> rustev_contract::actionability::Action {
    use rustev_contract::actionability::{Action, Override, OverrideBasis, Remediation};
    let caused_by = sort_causes(causes);
    let computed = Party::combine(
        caused_by
            .iter()
            .map(|c| cause_party(c, a.caller_controlled)),
    );
    let (remediation, remediation_for) = match a.declared {
        Some((r, p)) => (r, p),
        None => (a.computed, None),
    };
    let override_ = match (a.address, remediation_for, &remediation) {
        (Address::Override(o), _, _) => Override::Applied {
            to: o.to,
            basis: OverrideBasis::Declared,
            reason: o.reason.clone(),
        },
        (Address::Computed, Some(p), Remediation::Some { code, .. }) if p != computed => {
            Override::Applied {
                to: p,
                basis: OverrideBasis::Remediation,
                reason: code.clone(),
            }
        }
        _ => Override::None,
    };
    let addressed_to = match &override_ {
        Override::Applied { to, .. } => *to,
        Override::None => computed,
    };
    Action {
        addressed_to,
        computed,
        override_,
        remediation,
        caused_by,
        transient: a.transient,
    }
}

/// The computed remediation of a reason (3.6.2).
pub(crate) fn computed_remediation(
    u: &rustev_contract::judgment::Unresolved,
    transient: bool,
    failed_step: Option<&str>,
) -> rustev_contract::actionability::Remediation {
    use rustev_contract::actionability::{Remediation, RemediationValue};
    use rustev_contract::judgment::Unresolved as U;
    let texts = |key: &str, v: &[String]| {
        Remediation::with(key, [(key_for(key), Remediation::texts(v.iter().cloned()))])
    };
    fn key_for(code: &str) -> String {
        if code == "resolve_conflict" {
            "facts".into()
        } else {
            "fields".into()
        }
    }
    match u {
        U::MissingEvidence { fields } => texts("provide_fields", fields),
        U::StaleEvidence { fields } => texts("refresh_fields", fields),
        U::InvalidInput { fields, .. } => texts("correct_fields", fields),
        U::Conflict { facts } => texts("resolve_conflict", facts),
        U::InsufficientGrade { fields } => texts("improve_evidence", fields),
        U::BackendUnavailable { .. } | U::DeadlineExceeded => Remediation::code("retry"),
        U::InvalidBackendOutput { .. } if transient => Remediation::code("retry"),
        U::InvalidBackendOutput { .. } => Remediation::with(
            "report_backend",
            [(
                "step".to_string(),
                RemediationValue::Text(failed_step.unwrap_or_default().to_string()),
            )],
        ),
        U::BudgetExhausted { resource } => Remediation::with(
            "raise_budget",
            [(
                "resource".to_string(),
                RemediationValue::Text(resource.clone()),
            )],
        ),
        U::Unsupported { capability } => Remediation::with(
            "provide_capability",
            [(
                "name".to_string(),
                RemediationValue::Text(capability.clone()),
            )],
        ),
        U::ScreenUnavailable { .. } | U::Abstained { .. } => Remediation::None,
    }
}

/// Whether retrying the same request unchanged can succeed (3.6.4): only for
/// `backend_unavailable`, `deadline_exceeded`, and `invalid_backend_output`
/// from a backend whose determinism is not `bitwise`.
pub fn transient(
    u: &rustev_contract::judgment::Unresolved,
    determinism: Option<rustev_contract::definition::Determinism>,
) -> bool {
    use rustev_contract::definition::Determinism;
    use rustev_contract::judgment::Unresolved as U;
    match u {
        U::BackendUnavailable { .. } | U::DeadlineExceeded => true,
        U::InvalidBackendOutput { .. } => determinism != Some(Determinism::Bitwise),
        _ => false,
    }
}

/// The input fields a reason names, each reduced to its input (3.5.6).
pub(crate) fn reason_fields(u: &rustev_contract::judgment::Unresolved) -> Vec<String> {
    use rustev_contract::judgment::Unresolved as U;
    let fields: &[String] = match u {
        U::MissingEvidence { fields }
        | U::StaleEvidence { fields }
        | U::InvalidInput { fields, .. }
        | U::InsufficientGrade { fields } => fields,
        U::Conflict { facts } => facts,
        _ => &[],
    };
    let mut out: Vec<String> = fields
        .iter()
        .map(|f| f.split('/').next().unwrap_or(f).to_string())
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::Document;

    /// Check `b` (declared first) reads `a.x`, whose check `a` downgrades it:
    /// `b`'s subject still inherits `insufficient_grade`.
    #[test]
    fn check_reasons_do_not_depend_on_declaration_order() {
        let check = |id: &str, subject: &str, reads: &str, k: serde_json::Value| {
            serde_json::json!({ "id": id, "subject": subject,
                "test": { "present": format!("input:{reads}") },
                "on_fail": { "by_provenance": {}, "default": k } })
        };
        let def = serde_json::json!({
            "schema": "rustev.definition/2", "name": "n", "version": "1.0.0", "package": "p",
            "inputs": [], "steps": [],
            "policy": { "on_unresolved": [], "rules": [], "adjustments": [] },
            "limits": { "max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 1,
                        "distribution_tolerance": "0.01", "deadline_ms": 1 },
            "actionability": { "caller_controlled": [], "remediations": [], "questions": [],
                "constraint_groups": [],
                "checks": [
                    check("b", "b.y", "a.x", serde_json::json!("abstain")),
                    check("a", "a.x", "a.x", serde_json::json!(
                        { "downgrade_grade": { "to": "estimated", "notice": "n" } })),
                ] } });
        let def = Definition::parse(&serde_json::to_vec(&def).unwrap()).unwrap();
        let mut reasons = BTreeMap::new();
        extend_reasons(&def, &mut reasons);
        assert!(reasons["input:b.y"].contains(&ReasonKind::InsufficientGrade));
    }
}
