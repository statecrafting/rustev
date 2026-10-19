//! Evidence quality at compile time (spec 032): the structure of version 2
//! definitions (phase b), source binding (phase c, category 15), static
//! grade ceilings and the grade phase (e2, categories 13 and 14), and the
//! plan 3 grade table.
//!
//! A ceiling is the highest grade a value can have. It is computed from
//! source descriptors, definition caps, propagation and corroboration only;
//! a snapshot claim can lower a grade at evaluation, never raise it.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::definition::{
    Cond, CorroborateDecl, Definition, Disagreement, EvidenceDecl, Expr, GradeRequirement,
    InputDecl, Predicate, ProvenanceClass, StepBody, TypeDecl,
};
use rustev_contract::grade::{Grade, PresentWhen, ValueGrade};
use rustev_contract::plan::{BoundSource, StaticGrade};
use rustev_contract::source::{FactGrade, SourceDescriptor, valid_source_name};
use rustev_contract::{Identified, schema};

use crate::compile::{Category, Refusal};
use crate::expr::{Ref, split_item, split_step};
use crate::ops::{FilterWithReasons, OpArgs};

/// Bounds of a presence predicate (spec 032, 3.4.2).
pub const PRESENT_WHEN_MAX_DEPTH: usize = 4;
pub const PRESENT_WHEN_MAX_LEAVES: usize = 32;
/// The largest map a version 2 type may declare (spec 032, 3.4.1).
pub const MAP_MAX_ENTRIES: u64 = 10_000;

fn refusal(category: Category, subject: impl Into<String>, detail: impl Into<String>) -> Refusal {
    Refusal {
        category,
        subject: subject.into(),
        detail: detail.into(),
        shortfalls: vec![],
    }
}

fn invalid(subject: impl Into<String>, detail: impl Into<String>) -> Refusal {
    refusal(Category::InvalidDefinition, subject, detail)
}

/// Non-empty lists, at most 4 deep, at most 32 leaves.
pub(crate) fn present_when_structure(p: &PresentWhen) -> Result<(), String> {
    fn lists(p: &PresentWhen) -> Result<(), String> {
        match p {
            PresentWhen::AnyNonNull(f) | PresentWhen::AllNonNull(f) if f.is_empty() => {
                Err("a presence predicate names no field".into())
            }
            PresentWhen::All(ps) | PresentWhen::Any(ps) if ps.is_empty() => {
                Err("a presence predicate combines nothing".into())
            }
            PresentWhen::All(ps) | PresentWhen::Any(ps) => ps.iter().try_for_each(lists),
            _ => Ok(()),
        }
    }
    lists(p)?;
    let (depth, leaves) = p.shape();
    if depth > PRESENT_WHEN_MAX_DEPTH || leaves > PRESENT_WHEN_MAX_LEAVES {
        return Err(format!(
            "a presence predicate is {depth} deep with {leaves} leaves; at most \
             {PRESENT_WHEN_MAX_DEPTH} and {PRESENT_WHEN_MAX_LEAVES}"
        ));
    }
    Ok(())
}

/// Whether a version 2 definition uses any construct of the #76 track.
pub(crate) fn uses_track_construct(def: &Definition, decoded: &BTreeMap<String, OpArgs>) -> bool {
    fn type_v2(t: &TypeDecl) -> bool {
        match t {
            TypeDecl::Nullable { .. } | TypeDecl::Map { .. } => true,
            TypeDecl::Record {
                present_when: Some(_),
                ..
            } => true,
            TypeDecl::Record { fields, .. } => fields.iter().any(|f| type_v2(&f.ty)),
            TypeDecl::List { item, .. } => type_v2(item),
            _ => false,
        }
    }
    def.inputs
        .iter()
        .any(|i| i.evidence.is_some() || i.on_disagreement.is_some() || type_v2(&i.ty))
        || def.steps.iter().any(|s| match &s.body {
            StepBody::Corroborate(_) => true,
            StepBody::Exact(_) => decoded.get(&s.id).is_some_and(OpArgs::uses_v2),
            StepBody::Semantic(_) => false,
        })
        || policy_conds(def).any(|(_, c)| cond_uses_v2(c))
}

fn cond_uses_v2(c: &Cond) -> bool {
    match c {
        Cond::Exact(p) => crate::expr::predicate_uses_v2(p),
        Cond::All(cs) | Cond::Any(cs) => cs.iter().any(cond_uses_v2),
        Cond::Not(c) => cond_uses_v2(c),
        _ => false,
    }
}

/// Every policy condition with its subject: rules, then adjustments.
pub(crate) fn policy_conds(def: &Definition) -> impl Iterator<Item = (String, &Cond)> {
    def.policy
        .rules
        .iter()
        .map(|r| (format!("policy:rule:{}", r.id), &r.when))
        .chain(
            def.policy
                .adjustments
                .iter()
                .map(|a| (format!("policy:adjustment:{}", a.id), &a.when)),
        )
}

fn path_name_ok(n: &str) -> bool {
    !n.contains('/') && !n.contains('.') && n != "*"
}

/// Version 2 type structure: `nullable` only as a record field type, maps
/// within bounds, presence predicates within bounds and over nullable fields
/// of their record, field names usable in paths.
fn check_type(t: &TypeDecl, subject: &str, field_level: bool) -> Result<(), Refusal> {
    match t {
        TypeDecl::Nullable { ty } => {
            if !field_level {
                return Err(invalid(
                    subject,
                    "nullable is allowed only as a record field type",
                ));
            }
            check_type(ty, subject, false)
        }
        TypeDecl::Map {
            key_max_bytes,
            value,
            max_entries,
        } => {
            if *key_max_bytes == 0 || *max_entries == 0 || *max_entries > MAP_MAX_ENTRIES {
                return Err(invalid(
                    subject,
                    format!("a map has positive key_max_bytes and 1 to {MAP_MAX_ENTRIES} entries"),
                ));
            }
            check_type(value, subject, false)
        }
        TypeDecl::List { item, .. } => check_type(item, subject, false),
        TypeDecl::Record {
            fields,
            present_when,
        } => {
            for f in fields {
                if !path_name_ok(&f.name) {
                    return Err(invalid(
                        subject,
                        format!(
                            "field {:?}: version 2 field names hold no '/', '.' or '*'",
                            f.name
                        ),
                    ));
                }
                check_type(&f.ty, subject, true)?;
            }
            if let Some(p) = present_when {
                present_when_structure(p).map_err(|e| invalid(subject, e))?;
                let mut named = Vec::new();
                p.fields(&mut named);
                for n in named {
                    let nullable = fields
                        .iter()
                        .any(|f| f.name == n && matches!(f.ty, TypeDecl::Nullable { .. }));
                    if !nullable {
                        return Err(refusal(
                            Category::KindMismatch,
                            subject,
                            format!("present_when names {n:?}, which is not a nullable field"),
                        ));
                    }
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_grading(i: &InputDecl) -> Result<(), Refusal> {
    let subject = format!("input:{}", i.name);
    let sources: Option<&Vec<String>> = match &i.evidence {
        Some(EvidenceDecl::Graded { sources, caps }) => {
            let mut sorted = sources.clone();
            sorted.sort();
            sorted.dedup();
            if sources.is_empty() || &sorted != sources {
                return Err(invalid(
                    &subject,
                    "graded sources are non-empty, sorted and duplicate-free",
                ));
            }
            if let Some(s) = sources.iter().find(|s| !valid_source_name(s)) {
                return Err(invalid(&subject, format!("{s:?} is not a source name")));
            }
            if caps.iter().any(|c| parse_path(&c.path).is_none()) {
                return Err(invalid(&subject, "a cap path has an empty segment"));
            }
            Some(sources)
        }
        None => None,
    };
    match (&i.on_disagreement, sources) {
        (Some(_), None) => Err(invalid(
            &subject,
            "disagreement_on_ungraded: only a graded input declares on_disagreement",
        )),
        (Some(Disagreement::Prefer { sources: listed }), Some(named))
        | (Some(Disagreement::TreatAsHint { sources: listed }), Some(named)) => {
            let mut seen = BTreeSet::new();
            for s in listed {
                if !seen.insert(s) {
                    return Err(invalid(
                        &subject,
                        format!("disagreement_sources: {s:?} is listed twice"),
                    ));
                }
                if !named.contains(s) {
                    return Err(invalid(
                        &subject,
                        format!("disagreement_sources: the input does not name {s:?}"),
                    ));
                }
            }
            if let Some(s) = named.iter().find(|s| !seen.contains(s)) {
                return Err(invalid(
                    &subject,
                    format!("disagreement_sources: {s:?} is left out"),
                ));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// A source path: `""` is the whole value; otherwise segments joined by `/`.
pub(crate) fn parse_path(p: &str) -> Option<Vec<String>> {
    if p.is_empty() {
        return Some(vec![]);
    }
    let segs: Vec<String> = p.split('/').map(String::from).collect();
    (!segs.iter().any(String::is_empty)).then_some(segs)
}

fn requirement_mins(e: &Expr, out: &mut Vec<Grade>) {
    match e {
        Expr::Graded { require, .. } => out.push(require.min),
        Expr::Ref(_) | Expr::Lit(_) => {}
        Expr::Add { left, right }
        | Expr::Sub { left, right }
        | Expr::Mul { left, right }
        | Expr::Div { left, right } => {
            requirement_mins(left, out);
            requirement_mins(right, out);
        }
        Expr::Round { value, .. } | Expr::Len(value) | Expr::ToDecimal(value) => {
            requirement_mins(value, out)
        }
        Expr::Fx {
            amount, from, to, ..
        } => {
            requirement_mins(amount, out);
            requirement_mins(from, out);
            requirement_mins(to, out);
        }
    }
}

fn predicate_mins(p: &Predicate, out: &mut Vec<Grade>) {
    match p {
        Predicate::Cmp { left, right, .. } => {
            requirement_mins(left, out);
            requirement_mins(right, out);
        }
        Predicate::All(ps) | Predicate::Any(ps) => ps.iter().for_each(|p| predicate_mins(p, out)),
        Predicate::Not(p) => predicate_mins(p, out),
        Predicate::Member { value, .. } | Predicate::MemberRef { value, .. } => {
            requirement_mins(value, out)
        }
        Predicate::Present(_) => {}
    }
}

fn cond_predicates<'c>(c: &'c Cond, out: &mut Vec<&'c Predicate>) {
    match c {
        Cond::Exact(p) => out.push(p),
        Cond::All(cs) | Cond::Any(cs) => cs.iter().for_each(|c| cond_predicates(c, out)),
        Cond::Not(c) => cond_predicates(c, out),
        _ => {}
    }
}

/// The hard-gate predicates of an exact step (spec 032, 3.9.2).
pub(crate) fn gate_predicates(args: &OpArgs) -> Vec<&Predicate> {
    match args {
        OpArgs::CountWhere(a) => vec![&a.predicate],
        OpArgs::WindowFilter(a) => vec![&a.predicate],
        OpArgs::FilterWithReasons(a) => a.rules.iter().map(|r| &r.predicate).collect(),
        OpArgs::Compare(a) => vec![&a.predicate],
        _ => vec![],
    }
}

/// Phase (b) for a version 2 definition: everything spec 032 adds to the
/// structure checks, in input, step and policy order.
pub(crate) fn structure(
    def: &Definition,
    decoded: &BTreeMap<String, OpArgs>,
) -> Result<(), Refusal> {
    if !uses_track_construct(def, decoded) {
        return Err(invalid(
            "definition",
            format!(
                "use_definition_1: a {} definition uses no construct of the #76 track",
                schema::DEFINITION_V2
            ),
        ));
    }
    for i in &def.inputs {
        let subject = format!("input:{}", i.name);
        check_type(&i.ty, &subject, false)?;
        check_grading(i)?;
    }
    for s in &def.steps {
        let subject = format!("step:{}", s.id);
        if s.id.contains('/') {
            return Err(invalid(&subject, "version 2 step ids hold no '/'"));
        }
        match &s.body {
            StepBody::Exact(_) => {
                let Some(args) = decoded.get(&s.id) else {
                    continue;
                };
                if let OpArgs::FilterWithReasons(FilterWithReasons { rules, .. }) = args {
                    if let Some(r) = rules.iter().find(|r| {
                        r.name == "unmatched"
                            || r.name == "predicate_failed"
                            || r.name.contains(':')
                    }) {
                        return Err(invalid(
                            &subject,
                            format!("reserved_reason: a rule cannot be named {:?}", r.name),
                        ));
                    }
                }
                let mut mins = Vec::new();
                gate_predicates(args)
                    .into_iter()
                    .for_each(|p| predicate_mins(p, &mut mins));
                if let OpArgs::MemberOf(m) = args {
                    requirement_mins(&m.value, &mut mins);
                }
                if mins.contains(&Grade::Hint) {
                    return Err(invalid(
                        &subject,
                        "a hard gate never accepts hint: an explicit min of hint is refused",
                    ));
                }
            }
            StepBody::Corroborate(c) => corroborate_structure(c, &subject)?,
            StepBody::Semantic(_) => {}
        }
    }
    for (subject, c) in policy_conds(def) {
        let mut ps = Vec::new();
        cond_predicates(c, &mut ps);
        let mut mins = Vec::new();
        ps.into_iter().for_each(|p| predicate_mins(p, &mut mins));
        if mins.contains(&Grade::Hint) {
            return Err(invalid(
                subject,
                "a hard gate never accepts hint: an explicit min of hint is refused",
            ));
        }
    }
    Ok(())
}

fn corroborate_structure(c: &CorroborateDecl, subject: &str) -> Result<(), Refusal> {
    if c.upgrade_to <= Grade::Hint {
        return Err(invalid(
            subject,
            "a corroboration upgrades to a grade above hint",
        ));
    }
    if c.by.is_empty() {
        return Err(invalid(subject, "a corroboration has at least one check"));
    }
    match Ref::parse(&c.claim) {
        Ok(Ref::Input(..) | Ref::Step(_)) => {}
        _ => {
            return Err(invalid(
                subject,
                format!("the claim {:?} is not an input path or step", c.claim),
            ));
        }
    }
    for check in &c.by {
        crate::expr::predicate_structure(&check.predicate).map_err(|e| invalid(subject, e))?;
    }
    Ok(())
}

/// A source's or a definition's grade declarations: path and limit.
pub(crate) type Declarations = Vec<(Vec<String>, Grade)>;

/// How one graded input is graded: its bound sources' declarations, its
/// caps and its disagreement rule.
#[derive(Debug, Clone)]
pub(crate) struct Grading {
    /// Per source name, in the input's source order: the fact provenance
    /// and declarations.
    pub sources: Vec<(String, ProvenanceClass, Declarations)>,
    pub caps: Declarations,
    #[allow(dead_code)] // read by plan 3 evaluation, spec 032 increment 3
    pub disagreement: Disagreement,
}

/// Bound sources (spec 032, 3.3.4).
#[derive(Debug, Clone, Default)]
pub(crate) struct Bound {
    pub sources: Vec<BoundSource>,
    pub inputs: BTreeMap<String, Grading>,
}

/// Whether `path` exists in `t`: field names, `*` for list items and map
/// values; `nullable` is transparent.
fn path_exists(t: &TypeDecl, path: &[String]) -> bool {
    let Some((head, rest)) = path.split_first() else {
        return true;
    };
    match t {
        TypeDecl::Nullable { ty } => path_exists(ty, path),
        TypeDecl::Record { fields, .. } => fields
            .iter()
            .any(|f| &f.name == head && path_exists(&f.ty, rest)),
        TypeDecl::List { item, .. } => head == "*" && path_exists(item, rest),
        TypeDecl::Map { value, .. } => head == "*" && path_exists(value, rest),
        _ => false,
    }
}

/// Phase (c): bind every source a graded input names (category 15).
pub(crate) fn bind_sources(
    def: &Definition,
    supplied: &[SourceDescriptor],
) -> Result<Bound, Refusal> {
    let mut by_name: BTreeMap<&str, &SourceDescriptor> = BTreeMap::new();
    for d in supplied {
        if d.schema != schema::SOURCE {
            return Err(refusal(
                Category::Parse,
                format!("source:{}", d.name),
                format!("descriptor schema is not {}", schema::SOURCE),
            ));
        }
        if by_name.insert(d.name.as_str(), d).is_some() {
            return Err(refusal(
                Category::SourceBinding,
                format!("source:{}", d.name),
                "two supplied source descriptors share a name",
            ));
        }
    }
    let mut out = Bound::default();
    let mut bound: BTreeMap<String, BoundSource> = BTreeMap::new();
    for i in &def.inputs {
        let Some(EvidenceDecl::Graded { sources, caps }) = &i.evidence else {
            continue;
        };
        let subject = format!("input:{}", i.name);
        let sb = |detail: String| refusal(Category::SourceBinding, &subject, detail);
        let mut grading = Grading {
            sources: vec![],
            caps: vec![],
            disagreement: i
                .on_disagreement
                .clone()
                .unwrap_or(Disagreement::RequireAgreement),
        };
        for name in sources {
            let d = by_name
                .get(name.as_str())
                .ok_or_else(|| sb(format!("source {name:?} was not supplied")))?;
            let fact = d
                .facts
                .iter()
                .flatten()
                .find(|f| f.name == i.name)
                .ok_or_else(|| sb(format!("source {name:?} has no fact for this input")))?;
            if !i.provenance.contains(&fact.provenance) {
                return Err(sb(format!(
                    "source {name:?} attests provenance {:?}, which the input does not accept",
                    fact.provenance
                )));
            }
            let decls = fact_declarations(&fact.grades, &i.ty).map_err(sb)?;
            grading.sources.push((name.clone(), fact.provenance, decls));
            let id = d
                .id()
                .map_err(|e| refusal(Category::Parse, format!("source:{name}"), e.to_string()))?;
            bound.insert(
                name.clone(),
                BoundSource {
                    name: name.clone(),
                    version: d.version.clone(),
                    id,
                },
            );
        }
        for c in caps {
            // Phase (b) refuses a malformed path first; this keeps a parse
            // failure from reading as the root path if called alone.
            let Some(path) = parse_path(&c.path) else {
                return Err(sb(format!("capped path {:?} has an empty segment", c.path)));
            };
            if !path_exists(&i.ty, &path) {
                return Err(sb(format!(
                    "capped path {:?} does not exist in the input's type",
                    c.path
                )));
            }
            grading.caps.push((path, c.max_grade));
        }
        out.inputs.insert(i.name.clone(), grading);
    }
    out.sources = bound.into_values().collect();
    Ok(out)
}

fn fact_declarations(grades: &[FactGrade], ty: &TypeDecl) -> Result<Declarations, String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for g in grades {
        if !seen.insert(g.path.as_str()) {
            return Err(format!("the fact declares path {:?} twice", g.path));
        }
        let path = parse_path(&g.path)
            .ok_or_else(|| format!("declared path {:?} has an empty segment", g.path))?;
        if !path_exists(ty, &path) {
            return Err(format!(
                "declared path {:?} does not exist in the input's type",
                g.path
            ));
        }
        out.push((path, g.limit()));
    }
    if !seen.contains("") {
        return Err("the fact does not declare the whole value (path \"\")".into());
    }
    Ok(out)
}

/// A static ceiling tree: `here` is the ceiling at a node from declarations
/// on it and its ancestors; `own` is the ceiling of the node as a whole
/// value, the combination of itself and its descendants (spec 032, 3.2.2).
/// `corroborated` marks a value a corroboration step lies upstream of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ceil {
    pub here: ValueGrade,
    pub own: ValueGrade,
    pub corroborated: bool,
    pub fields: BTreeMap<String, Ceil>,
    pub item: Option<Box<Ceil>>,
}

impl Ceil {
    pub fn leaf(g: ValueGrade, corroborated: bool) -> Ceil {
        Ceil {
            here: g,
            own: g,
            corroborated,
            fields: BTreeMap::new(),
            item: None,
        }
    }

    /// The node at `path`; past the tree's depth, a leaf carrying the
    /// deepest node's whole ceiling.
    pub fn at(&self, path: &[String]) -> Ceil {
        let Some((head, rest)) = path.split_first() else {
            return self.clone();
        };
        let child = if head == "*" {
            self.item.as_deref()
        } else {
            self.fields.get(head)
        };
        match child {
            Some(c) => c.at(rest),
            None => Ceil::leaf(self.own, self.corroborated),
        }
    }

    fn finish(mut self) -> Ceil {
        let mut own = self.here;
        for c in self.fields.values().chain(self.item.as_deref()) {
            own = own.combine(c.own);
        }
        self.own = own;
        self
    }

    /// Every node with its path, root first.
    pub fn walk(&self, prefix: &str, out: &mut Vec<StaticGrade>) {
        out.push(StaticGrade {
            subject: prefix.to_string(),
            ceiling: self.own,
        });
        for (n, c) in &self.fields {
            c.walk(&format!("{prefix}/{n}"), out);
        }
        if let Some(c) = &self.item {
            c.walk(&format!("{prefix}/*"), out);
        }
    }
}

fn prefix_of(decl: &[String], path: &[String]) -> bool {
    decl.len() <= path.len() && decl.iter().zip(path).all(|(a, b)| a == b)
}

impl Grading {
    /// The ceiling at `path` through one source: the minimum over its
    /// declarations on the path and its ancestors (a deeper one only
    /// lowers).
    fn source_here(decls: &[(Vec<String>, Grade)], path: &[String]) -> Grade {
        decls
            .iter()
            .filter(|(d, _)| prefix_of(d, path))
            .map(|(_, g)| *g)
            .min()
            .unwrap_or(Grade::Hint)
    }

    /// The cap at `path`: the minimum over caps on it and its ancestors.
    fn cap_here(&self, path: &[String]) -> Option<Grade> {
        self.caps
            .iter()
            .filter(|(c, _)| prefix_of(c, path))
            .map(|(_, g)| *g)
            .min()
    }

    /// The effective ceiling of `path` through `source` (None: the highest
    /// over every source), capped.
    pub fn here(&self, path: &[String], source: Option<&str>) -> Grade {
        let g = self
            .sources
            .iter()
            .filter(|(n, _, _)| source.is_none_or(|s| s == n))
            .map(|(_, _, d)| Self::source_here(d, path))
            .max()
            .unwrap_or(Grade::Hint);
        match self.cap_here(path) {
            Some(c) => g.min(c),
            None => g,
        }
    }
}

/// The static ceiling tree of an input (ungraded inputs are ungraded
/// throughout), or with `source` the tree through that one source.
pub(crate) fn input_tree(ty: &TypeDecl, grading: Option<&Grading>, source: Option<&str>) -> Ceil {
    fn build(
        t: &TypeDecl,
        path: &mut Vec<String>,
        g: Option<&Grading>,
        source: Option<&str>,
    ) -> Ceil {
        let here = match g {
            Some(g) => ValueGrade::from(g.here(path, source)),
            None => ValueGrade::Ungraded,
        };
        let mut node = Ceil::leaf(here, false);
        match t {
            TypeDecl::Nullable { ty } => return build(ty, path, g, source),
            TypeDecl::Record { fields, .. } => {
                for f in fields {
                    path.push(f.name.clone());
                    node.fields
                        .insert(f.name.clone(), build(&f.ty, path, g, source));
                    path.pop();
                }
            }
            TypeDecl::List { item, .. } | TypeDecl::Map { value: item, .. } => {
                path.push("*".into());
                node.item = Some(Box::new(build(item, path, g, source)));
                path.pop();
            }
            _ => {}
        }
        node.finish()
    }
    build(ty, &mut vec![], grading, source)
}

/// How a read uses a value: as a whole (the node and everything under it),
/// or only the node itself (a list's items exist, whatever their fields).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Whole,
    Node,
}

/// One read of a step or policy item (spec 032, 3.2.3 and 3.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Read {
    /// `input:<name>` or `step:<id>`.
    pub base: String,
    pub path: Vec<String>,
    pub mode: Mode,
    pub require: Option<GradeRequirement>,
    pub gate: bool,
}

impl Read {
    /// `input:x/a/b` or `step:y/a`, as evidence and refusals name it.
    pub fn name(&self) -> String {
        if self.path.is_empty() {
            self.base.clone()
        } else {
            format!("{}/{}", self.base, self.path.join("/"))
        }
    }
}

/// The base and path a reference names; item references resolve against
/// `item_base` (the list a per-item operator iterates, with `*`).
fn resolve(r: &str, item_base: Option<(&str, &[String])>) -> Option<(String, Vec<String>)> {
    match Ref::parse(r).ok()? {
        Ref::Input(n, p) => Some((format!("input:{n}"), p)),
        Ref::Step(s) => {
            let (id, p) = split_step(&s);
            Some((format!("step:{id}"), p))
        }
        Ref::Item(p) => {
            let (b, lp) = item_base?;
            let mut path = lp.to_vec();
            path.push("*".into());
            path.extend(split_item(p));
            Some((b.to_string(), path))
        }
        Ref::Now | Ref::Bind(_) => None,
    }
}

fn expr_reads(e: &Expr, item: Option<(&str, &[String])>, gate: bool, out: &mut Vec<Read>) {
    let mut push = |r: &str, require: Option<GradeRequirement>| {
        if let Some((base, path)) = resolve(r, item) {
            out.push(Read {
                base,
                path,
                mode: Mode::Whole,
                require,
                gate,
            });
        }
    };
    match e {
        Expr::Ref(r) => push(r, None),
        Expr::Graded { reference, require } => push(reference, Some(require.clone())),
        Expr::Lit(_) => {}
        Expr::Add { left, right }
        | Expr::Sub { left, right }
        | Expr::Mul { left, right }
        | Expr::Div { left, right } => {
            expr_reads(left, item, gate, out);
            expr_reads(right, item, gate, out);
        }
        Expr::Round { value, .. } | Expr::Len(value) | Expr::ToDecimal(value) => {
            expr_reads(value, item, gate, out)
        }
        Expr::Fx {
            amount,
            from,
            to,
            rates,
            ..
        } => {
            expr_reads(amount, item, gate, out);
            expr_reads(from, item, gate, out);
            expr_reads(to, item, gate, out);
            expr_reads(&Expr::Ref(rates.clone()), item, gate, out);
        }
    }
}

pub(crate) fn predicate_reads(
    p: &Predicate,
    item: Option<(&str, &[String])>,
    gate: bool,
    out: &mut Vec<Read>,
) {
    match p {
        Predicate::Cmp { left, right, .. } => {
            expr_reads(left, item, gate, out);
            expr_reads(right, item, gate, out);
        }
        Predicate::All(ps) | Predicate::Any(ps) => {
            ps.iter().for_each(|p| predicate_reads(p, item, gate, out))
        }
        Predicate::Not(p) => predicate_reads(p, item, gate, out),
        Predicate::Member { value, .. } => expr_reads(value, item, gate, out),
        Predicate::MemberRef { value, set } => {
            expr_reads(value, item, gate, out);
            expr_reads(&Expr::Ref(set.clone()), item, gate, out);
        }
        Predicate::Present(r) => {
            if let Some((base, path)) = resolve(r, item) {
                out.push(Read {
                    base,
                    path,
                    mode: Mode::Node,
                    require: None,
                    gate,
                });
            }
        }
    }
}

/// A whole-value read of a reference, outside any per-item operator.
pub(crate) fn whole_read(r: &str) -> Option<Read> {
    whole(r)
}

fn whole(r: &str) -> Option<Read> {
    resolve(r, None).map(|(base, path)| Read {
        base,
        path,
        mode: Mode::Whole,
        require: None,
        gate: false,
    })
}

fn node(r: &str) -> Option<Read> {
    whole(r).map(|r| Read {
        mode: Mode::Node,
        ..r
    })
}

fn item_field(list: &str, field: &str) -> Option<Read> {
    resolve(list, None).map(|(base, mut path)| {
        path.push("*".into());
        path.push(field.into());
        Read {
            base,
            path,
            mode: Mode::Whole,
            require: None,
            gate: false,
        }
    })
}

/// Every value an exact step reads, for propagation and the grade phase.
/// The step's value metadata is the combination of these reads (spec 032,
/// 3.2.3); the gate reads are the hard gates of 3.9.2.
pub(crate) fn op_reads(args: &OpArgs) -> Vec<Read> {
    let mut out: Vec<Read> = Vec::new();
    let list_base = |r: &str| resolve(r, None);
    match args {
        OpArgs::CountWhere(a) => {
            out.extend(node(&a.list));
            if let Some((b, p)) = list_base(&a.list) {
                predicate_reads(&a.predicate, Some((&b, &p)), true, &mut out);
            }
        }
        OpArgs::WindowFilter(a) => {
            out.extend(whole(&a.list));
            if let Some((b, p)) = list_base(&a.list) {
                predicate_reads(&a.predicate, Some((&b, &p)), true, &mut out);
            }
        }
        OpArgs::Extremum(a) => {
            out.extend(node(&a.list));
            out.extend(item_field(&a.list, &a.field));
        }
        OpArgs::TimestampDiff(a) => {
            expr_reads(&a.later, None, false, &mut out);
            expr_reads(&a.earlier, None, false, &mut out);
        }
        OpArgs::Compare(a) => predicate_reads(&a.predicate, None, true, &mut out),
        OpArgs::Arith(a) => expr_reads(&a.expr, None, false, &mut out),
        OpArgs::FxConvert(a) => {
            expr_reads(&a.amount, None, false, &mut out);
            expr_reads(&a.from, None, false, &mut out);
            expr_reads(&a.to, None, false, &mut out);
            out.extend(whole(&a.rates));
        }
        OpArgs::MemberOf(a) => expr_reads(&a.value, None, true, &mut out),
        OpArgs::FieldPresence(a) => out.extend(a.inputs.iter().filter_map(|r| node(r))),
        OpArgs::FilterWithReasons(a) => {
            out.extend(node(&a.list));
            out.extend(item_field(&a.list, &a.id_field));
            if let Some((b, p)) = list_base(&a.list) {
                for r in &a.rules {
                    predicate_reads(&r.predicate, Some((&b, &p)), true, &mut out);
                }
            }
        }
        OpArgs::TopK(a) => {
            out.extend(node(&a.list));
            out.extend(item_field(&a.list, &a.id_field));
            out.extend(whole(&a.among));
            if let Some((b, p)) = list_base(&a.list) {
                expr_reads(&a.key, Some((&b, &p)), false, &mut out);
            }
        }
        OpArgs::WeightedRank(a) => {
            out.extend(whole(&a.candidates));
            for c in &a.components {
                match &c.source {
                    crate::ops::Source::PropositionMean { step, weights, .. } => {
                        out.extend(whole(step));
                        out.extend(whole(&weights.list));
                    }
                    crate::ops::Source::RubricExpectation { step } => out.extend(whole(step)),
                    crate::ops::Source::ShortlistPosition => {}
                }
            }
        }
        OpArgs::JoinKeyed(a) => {
            out.extend(node(&a.left));
            if a.key != "id" {
                out.extend(item_field(&a.left, &a.key));
            } else if let Some((base, mut path)) = list_base(&a.left) {
                path.push("*".into());
                out.push(Read {
                    base,
                    path,
                    mode: Mode::Whole,
                    require: None,
                    gate: false,
                });
            }
            out.extend(node(&a.right));
        }
        OpArgs::MergeFields(a) => {
            out.extend(whole(&a.base));
            out.extend(whole(&a.overlay));
        }
    }
    out
}

/// The reads of a policy condition: every one is a hard gate unless the
/// condition is `always` (spec 032, 3.9.2).
pub(crate) fn cond_reads(c: &Cond, out: &mut Vec<Read>) {
    match c {
        Cond::Always => {}
        Cond::TopLabel { step, .. }
        | Cond::MassAtLeast { step, .. }
        | Cond::TopMassBelow { step, .. }
        | Cond::ExpectationAtLeast { step, .. } => out.push(Read {
            base: format!("step:{step}"),
            path: vec![],
            mode: Mode::Whole,
            require: None,
            gate: true,
        }),
        Cond::Exact(p) => predicate_reads(p, None, true, out),
        Cond::Empty(r) | Cond::Nonempty(r) => {
            if let Some((base, path)) = resolve(r, None) {
                out.push(Read {
                    base,
                    path,
                    mode: Mode::Node,
                    require: None,
                    gate: true,
                });
            }
        }
        Cond::All(cs) | Cond::Any(cs) => cs.iter().for_each(|c| cond_reads(c, out)),
        Cond::Not(c) => cond_reads(c, out),
    }
}

/// The reads of a semantic step's projection, fan-out and candidates.
pub(crate) fn semantic_reads(d: &rustev_contract::definition::SemanticDecl) -> Vec<Read> {
    let mut out: Vec<Read> = d
        .project
        .iter()
        .filter(|r| !r.starts_with("bind:"))
        .filter_map(|r| whole(r))
        .collect();
    for f in &d.for_each {
        out.extend(whole(&f.over));
        if let rustev_contract::definition::ItemSource::List { list, .. } = &f.items {
            out.extend(whole(list));
        }
    }
    if let rustev_contract::definition::Candidates::From(c) = &d.candidates {
        out.extend(whole(c));
    }
    out
}

/// The reads of a corroboration step's checks.
pub(crate) fn check_reads(c: &CorroborateDecl) -> Vec<Vec<Read>> {
    c.by.iter()
        .map(|check| {
            let mut out = Vec::new();
            predicate_reads(&check.predicate, None, false, &mut out);
            out
        })
        .collect()
}

/// The claim's base and path.
pub(crate) fn claim_path(c: &CorroborateDecl) -> (String, Vec<String>) {
    resolve(&c.claim, None).unwrap_or_default()
}

/// Whether a read is of the claim itself (or a part of it): such a read is
/// not evidence for the claim, so its grade does not count against
/// `upgrade_to` (spec 032, 3.7.3).
pub(crate) fn reads_claim(r: &Read, claim: &(String, Vec<String>)) -> bool {
    r.base == claim.0 && prefix_of(&claim.1, &r.path)
}

/// The ceiling a read sees.
pub(crate) fn read_ceiling(r: &Read, trees: &BTreeMap<String, Ceil>) -> Ceil {
    let node = trees
        .get(&r.base)
        .map(|t| t.at(&r.path))
        .unwrap_or_else(|| Ceil::leaf(ValueGrade::Ungraded, false));
    match r.mode {
        Mode::Whole => node,
        Mode::Node => Ceil {
            own: node.here,
            ..node
        },
    }
}

/// The combination of reads' ceilings, as one leaf.
pub(crate) fn combine_reads(reads: &[Read], trees: &BTreeMap<String, Ceil>) -> Ceil {
    let mut g = ValueGrade::Ungraded;
    let mut corroborated = false;
    for r in reads {
        let c = read_ceiling(r, trees);
        g = g.combine(c.own);
        corroborated |= c.corroborated;
    }
    Ceil::leaf(g, corroborated)
}

/// Raise every node of a claim's tree to at least `g` (spec 032, 3.7.5): a
/// corroborated claim has grade `max(claim grade, upgrade_to)`; an ungraded
/// claim takes `upgrade_to`.
pub(crate) fn raised(t: &Ceil, g: Grade) -> Ceil {
    let up = |v: ValueGrade| match v.graded() {
        Some(x) => ValueGrade::from(x.max(g)),
        None => ValueGrade::from(g),
    };
    // An interior node without a declaration of its own (a merge root)
    // takes its content's ceiling, so `own` stays `combine(here, children)`
    // and equals `max(own, upgrade_to)`.
    let interior = !t.fields.is_empty() || t.item.is_some();
    let here = match t.here.graded() {
        None if interior => up(t.own),
        _ => up(t.here),
    };
    Ceil {
        here,
        own: here,
        corroborated: true,
        fields: t
            .fields
            .iter()
            .map(|(k, c)| (k.clone(), raised(c, g)))
            .collect(),
        item: t.item.as_ref().map(|c| Box::new(raised(c, g))),
    }
    .finish()
}

/// The category 13 or 14 refusal a requirement or gate read earns, if any
/// (spec 032, 3.9.3).
pub(crate) fn requirement_refusal(
    subject: &str,
    r: &Read,
    trees: &BTreeMap<String, Ceil>,
) -> Option<Refusal> {
    let c = read_ceiling(r, trees);
    let ceiling = c.own.graded()?;
    let min = match &r.require {
        Some(req) => req.min,
        None if r.gate => Grade::Estimated,
        None => return None,
    };
    if ceiling >= min {
        return None;
    }
    let (category, name) = if ceiling == Grade::Hint && !c.corroborated {
        (Category::UncorroboratedHint, "uncorroborated_hint")
    } else {
        (Category::GradeRequirement, "grade_requirement")
    };
    Some(refusal(
        category,
        subject,
        format!(
            "{name}: {} has ceiling {} where {} is required",
            r.name(),
            ceiling.name(),
            min.name()
        ),
    ))
}

fn max_grade(a: ValueGrade, b: ValueGrade) -> ValueGrade {
    match (a.graded(), b.graded()) {
        (Some(x), Some(y)) => x.max(y).into(),
        (Some(x), None) | (None, Some(x)) => x.into(),
        (None, None) => ValueGrade::Ungraded,
    }
}

fn tree_at(trees: &BTreeMap<String, Ceil>, r: &str, extra: &[&str]) -> Ceil {
    let (base, mut path) = resolve(r, None).unwrap_or_default();
    path.extend(extra.iter().map(|s| s.to_string()));
    trees
        .get(&base)
        .map(|t| t.at(&path))
        .unwrap_or_else(|| Ceil::leaf(ValueGrade::Ungraded, false))
}

/// The static tree of an exact step: one node, the combination of its
/// reads, except a join (whose matched values keep their own ceilings) and a
/// merge (one ceiling per field).
pub(crate) fn step_tree(args: &OpArgs, trees: &BTreeMap<String, Ceil>) -> Ceil {
    let whole = combine_reads(&op_reads(args), trees);
    match args {
        OpArgs::JoinKeyed(a) => {
            let id = if a.key == "id" {
                tree_at(trees, &a.left, &["*"])
            } else {
                tree_at(trees, &a.left, &["*", &a.key])
            };
            let matched = tree_at(trees, &a.right, &["*"]);
            let mut item = Ceil::leaf(id.own, id.corroborated);
            item.fields.insert("id".into(), id);
            item.fields.insert("match".into(), matched);
            let mut node = Ceil::leaf(whole.here, whole.corroborated);
            node.item = Some(Box::new(item.finish()));
            node.finish()
        }
        OpArgs::MergeFields(m) => {
            let per_item = matches!(trees.get(&resolve(&m.overlay, None).unwrap_or_default().0), Some(t) if t.item.as_ref().is_some_and(|i| i.fields.contains_key("match")));
            let mut rec = Ceil::leaf(ValueGrade::Ungraded, whole.corroborated);
            for r in &m.rules {
                let (base, overlay) = if per_item {
                    (
                        tree_at(trees, &m.base, &["*", &r.field]),
                        tree_at(trees, &m.overlay, &["*", "match", &r.field]),
                    )
                } else {
                    (
                        tree_at(trees, &m.base, &[&r.field]),
                        tree_at(trees, &m.overlay, &[&r.field]),
                    )
                };
                let field = match &r.rule {
                    crate::ops::MergeRule::OverlayOnly => overlay,
                    crate::ops::MergeRule::BaseOnly => base,
                    crate::ops::MergeRule::OverlayElseBaseIfGradeAtLeast { grade } => {
                        let base_ok = base.own.graded().is_some_and(|b| b >= *grade);
                        let g = if base_ok {
                            max_grade(overlay.own, base.own)
                        } else {
                            overlay.own
                        };
                        Ceil::leaf(g, overlay.corroborated || base.corroborated)
                    }
                };
                rec.fields.insert(r.field.clone(), field);
            }
            let rec = rec.finish();
            if per_item {
                let mut node = Ceil::leaf(ValueGrade::Ungraded, rec.corroborated);
                node.item = Some(Box::new(rec));
                node.finish()
            } else {
                rec
            }
        }
        _ => whole,
    }
}

/// A semantic step: at most `estimated` when it reads a graded value,
/// otherwise `ungraded` (spec 032, 3.2.3).
pub(crate) fn semantic_tree(
    d: &rustev_contract::definition::SemanticDecl,
    trees: &BTreeMap<String, Ceil>,
) -> Ceil {
    let c = combine_reads(&semantic_reads(d), trees);
    match c.own.graded() {
        Some(g) => Ceil::leaf(g.min(Grade::Estimated).into(), c.corroborated),
        None => Ceil::leaf(ValueGrade::Ungraded, c.corroborated),
    }
}

/// A corroboration step: the claim's tree raised to `upgrade_to`, its
/// highest possible grade.
pub(crate) fn corroborate_tree(c: &CorroborateDecl, trees: &BTreeMap<String, Ceil>) -> Ceil {
    raised(&tree_at(trees, &c.claim, &[]), c.upgrade_to)
}

/// The grade checks of a corroboration step (spec 032, 3.9.3): explicit
/// requirements in its checks, and no check may read a value (other than
/// the claim) whose ceiling is below `upgrade_to`.
pub(crate) fn corroborate_refusal(
    subject: &str,
    c: &CorroborateDecl,
    trees: &BTreeMap<String, Ceil>,
) -> Option<Refusal> {
    let claim = claim_path(c);
    for reads in check_reads(c) {
        for r in &reads {
            if let Some(x) = requirement_refusal(subject, r, trees) {
                return Some(x);
            }
            if reads_claim(r, &claim) {
                continue;
            }
            if let Some(g) = read_ceiling(r, trees).own.graded() {
                if g < c.upgrade_to {
                    return Some(refusal(
                        Category::GradeRequirement,
                        subject,
                        format!(
                            "grade_requirement: a check reads {} with ceiling {}, below upgrade_to {}",
                            r.name(),
                            g.name(),
                            c.upgrade_to.name()
                        ),
                    ));
                }
            }
        }
    }
    None
}

/// Worst-case value-metadata nodes of a version 2 type: one per value,
/// item, entry and field.
fn meta_nodes(t: &TypeDecl) -> u64 {
    match t {
        TypeDecl::Nullable { ty } => meta_nodes(ty),
        TypeDecl::Record { fields, .. } => fields
            .iter()
            .fold(1u64, |n, f| n.saturating_add(meta_nodes(&f.ty))),
        TypeDecl::List { item, max_items } => {
            1u64.saturating_add(max_items.saturating_mul(meta_nodes(item)))
        }
        TypeDecl::Map {
            value, max_entries, ..
        } => 1u64.saturating_add(max_entries.saturating_mul(meta_nodes(value))),
        _ => 1,
    }
}

/// Values one metadata node writes at most, beside its parts: grade,
/// basis and its member, as-of, and the provenance and source sets.
const VALUES_PER_META: u64 = 16;

/// Phase (g) for plan 3 (spec 032, 3.10.4): the per-item and per-field
/// records of evidence 2, bounded by declared list, map and record sizes,
/// fit `RECORD_V1`.
pub(crate) fn evidence_bound(
    def: &Definition,
    infos: &BTreeMap<String, crate::compile::StepInfo>,
) -> Result<(), String> {
    use rustev_contract::limits::RECORD_V1;
    let collection = RECORD_V1.max_collection_len as u64;
    fn widest(t: &TypeDecl) -> u64 {
        match t {
            TypeDecl::Nullable { ty } => widest(ty),
            TypeDecl::Record { fields, .. } => fields
                .iter()
                .map(|f| widest(&f.ty))
                .max()
                .unwrap_or(0)
                .max(fields.len() as u64),
            TypeDecl::List { item, max_items } => (*max_items).max(widest(item)),
            TypeDecl::Map {
                value, max_entries, ..
            } => (*max_entries).max(widest(value)),
            _ => 0,
        }
    }
    let mut nodes: u64 = 0;
    for i in &def.inputs {
        if widest(&i.ty) > collection {
            return Err(format!(
                "input:{} declares a collection over {collection} entries",
                i.name
            ));
        }
        nodes = nodes.saturating_add(meta_nodes(&i.ty));
    }
    for (id, info) in infos {
        if let crate::compile::StepInfo::Exact { ty, .. }
        | crate::compile::StepInfo::Corroborate { ty, .. } = info
        {
            if ty_widest(ty) > collection {
                return Err(format!(
                    "step:{id} produces a collection over {collection} entries"
                ));
            }
        }
        nodes = nodes.saturating_add(match info {
            crate::compile::StepInfo::Exact { ty, .. }
            | crate::compile::StepInfo::Corroborate { ty, .. } => ty_nodes(ty),
            crate::compile::StepInfo::Semantic { .. } => 1,
        });
    }
    let values = nodes.saturating_mul(VALUES_PER_META);
    if values > RECORD_V1.max_total_values as u64 {
        return Err(format!(
            "worst-case value metadata of {nodes} nodes exceeds the record bound of {} values",
            RECORD_V1.max_total_values
        ));
    }
    Ok(())
}

/// The most entries any one collection of a step's type can hold.
fn ty_widest(t: &crate::value::Ty) -> u64 {
    use crate::value::Ty;
    match t {
        Ty::Nullable(t) | Ty::Maybe(t) => ty_widest(t),
        Ty::Record(fields) => fields
            .iter()
            .map(|(_, t)| ty_widest(t))
            .max()
            .unwrap_or(0)
            .max(fields.len() as u64),
        Ty::List { item, max } => (*max).max(ty_widest(item)),
        Ty::Map { value, max, .. } => (*max).max(ty_widest(value)),
        Ty::Joined {
            value,
            max,
            id_bytes,
        } => (*max).max(ty_widest(&Ty::join_item(value, *id_bytes))),
        _ => 0,
    }
}

fn ty_nodes(t: &crate::value::Ty) -> u64 {
    use crate::value::Ty;
    match t {
        Ty::Nullable(t) | Ty::Maybe(t) => ty_nodes(t),
        Ty::Record(fields) => fields
            .iter()
            .fold(1u64, |n, (_, t)| n.saturating_add(ty_nodes(t))),
        Ty::List { item, max } => 1u64.saturating_add(max.saturating_mul(ty_nodes(item))),
        Ty::Map { value, max, .. } => 1u64.saturating_add(max.saturating_mul(ty_nodes(value))),
        Ty::Joined {
            value,
            max,
            id_bytes,
        } => 1u64.saturating_add(max.saturating_mul(ty_nodes(&Ty::join_item(value, *id_bytes)))),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::Document;
    use serde_json::{Value, json};

    fn source(provenance: &str) -> SourceDescriptor {
        let v = json!({"schema": "rustev.source/1", "name": "live-quote", "version": "1.0.0",
            "facts": [{"name": "quotes", "provenance": provenance, "grades": [
                {"path": "", "max_grade": "authoritative", "reliability": "reliable"}]}]});
        SourceDescriptor::parse(v.to_string().as_bytes()).unwrap()
    }

    /// A version 2 definition whose one input is a graded map of records
    /// with a presence predicate. SYNTHETIC.
    fn base() -> Value {
        json!({
            "schema": "rustev.definition/2", "name": "eq-unit", "version": "0.1.0",
            "package": "synthetic",
            "inputs": [
                {"name": "quotes", "type": {"map": {"key_max_bytes": 16, "max_entries": 20,
                    "value": {"record": {"fields": [
                        {"name": "price", "type": {"nullable": {"type": "decimal"}}},
                        {"name": "unit_price", "type": {"nullable": {"type": "decimal"}}}],
                        "present_when": {"any_non_null": ["price", "unit_price"]}}}}},
                 "provenance": ["third-party"], "freshness": "not_required",
                 "evidence": {"graded": {"sources": ["live-quote"], "caps": []}}}
            ],
            "steps": [],
            "policy": {"on_unresolved": [], "rules": [
                {"id": "fallback", "when": "always", "then": {"escalate": {"reason": "review"}}}],
                "adjustments": []},
            "limits": {"max_semantic_requests": 0, "on_excess": "refuse",
                "max_projection_bytes": 0, "distribution_tolerance": "0.000001",
                "deadline_ms": 1000}
        })
    }

    fn def(v: &Value) -> Definition {
        Definition::parse(v.to_string().as_bytes()).unwrap()
    }

    fn structure_of(v: &Value) -> Result<(), Refusal> {
        structure(&def(v), &BTreeMap::new())
    }

    fn assert_invalid(r: Result<(), Refusal>, category: Category, detail: &str) {
        let e = r.expect_err("refused");
        assert_eq!(e.category, category, "{}", e.detail);
        assert!(e.detail.contains(detail), "{:?} lacks {detail:?}", e.detail);
    }

    #[test]
    fn a_step_collection_is_measured_at_its_widest() {
        use crate::value::Ty;
        let list = |max, item| Ty::List {
            item: Box::new(item),
            max,
        };
        assert_eq!(ty_widest(&Ty::Bool), 0);
        assert_eq!(ty_widest(&list(3, list(70, Ty::Bool))), 70);
        let joined = Ty::Joined {
            value: Box::new(Ty::Record([("a".to_string(), list(9, Ty::Bool))].into())),
            max: 4,
            id_bytes: 8,
        };
        assert_eq!(ty_widest(&joined), 9);
        // A join's matched value is optional; its nodes still count.
        let quote = Ty::Record(vec![
            ("a".to_string(), Ty::Bool),
            ("b".to_string(), Ty::Bool),
        ]);
        assert_eq!(ty_nodes(&Ty::Maybe(Box::new(quote.clone()))), 3);
        let join = Ty::Joined {
            value: Box::new(quote),
            max: 20,
            id_bytes: 8,
        };
        // Each item is {id, match: {a, b}}: 1 + 1 + 3 nodes.
        assert_eq!(ty_nodes(&join), 1 + 20 * 5);
    }

    #[test]
    fn version_2_structure() {
        assert!(structure_of(&base()).is_ok());

        let mut plain = base();
        plain["inputs"][0]["evidence"] = Value::Null;
        plain["inputs"][0]["type"] = json!({"text": {"max_bytes": 8}});
        assert_invalid(
            structure_of(&plain),
            Category::InvalidDefinition,
            "use_definition_1",
        );

        let mut top = base();
        top["inputs"][0]["type"] = json!({"nullable": {"type": "decimal"}});
        assert_invalid(
            structure_of(&top),
            Category::InvalidDefinition,
            "only as a record field",
        );

        let mut empty_map = base();
        empty_map["inputs"][0]["type"]["map"]["max_entries"] = json!(0);
        assert_invalid(
            structure_of(&empty_map),
            Category::InvalidDefinition,
            "1 to 10000",
        );

        let mut not_nullable = base();
        not_nullable["inputs"][0]["type"]["map"]["value"]["record"]["fields"][0]["type"] =
            json!("decimal");
        assert_invalid(
            structure_of(&not_nullable),
            Category::KindMismatch,
            "not a nullable field",
        );

        let mut slash = base();
        slash["inputs"][0]["type"]["map"]["value"]["record"]["fields"][1]["name"] =
            json!("unit/price");
        slash["inputs"][0]["type"]["map"]["value"]["record"]["present_when"] =
            json!({"any_non_null": ["price"]});
        assert_invalid(
            structure_of(&slash),
            Category::InvalidDefinition,
            "field names",
        );

        let mut unsorted = base();
        unsorted["inputs"][0]["evidence"]["graded"]["sources"] = json!(["live-quote", "a"]);
        assert_invalid(
            structure_of(&unsorted),
            Category::InvalidDefinition,
            "sorted",
        );

        let mut ungraded = base();
        ungraded["inputs"][0]["evidence"] = Value::Null;
        ungraded["inputs"][0]["on_disagreement"] = json!("abstain");
        assert_invalid(
            structure_of(&ungraded),
            Category::InvalidDefinition,
            "disagreement_on_ungraded",
        );
    }

    #[test]
    fn graded_inputs_bind_their_sources_or_refuse() {
        let ok = bind_sources(&def(&base()), &[source("third-party")]).unwrap();
        assert_eq!(ok.sources.len(), 1);
        assert_eq!(ok.sources[0].name, "live-quote");
        let g = &ok.inputs["quotes"];
        assert_eq!(g.sources[0].2, vec![(vec![], Grade::Authoritative)]);

        let category = |r: Result<Bound, Refusal>| r.expect_err("refused").category;
        assert_eq!(
            category(bind_sources(&def(&base()), &[])),
            Category::SourceBinding,
            "not supplied"
        );
        assert_eq!(
            category(bind_sources(&def(&base()), &[source("system-of-record")])),
            Category::SourceBinding,
            "provenance not accepted"
        );
        assert_eq!(
            category(bind_sources(
                &def(&base()),
                &[source("third-party"), source("third-party")]
            )),
            Category::SourceBinding,
            "two descriptors share a name"
        );
        let mut capped = base();
        capped["inputs"][0]["evidence"]["graded"]["caps"] =
            json!([{"path": "*/price", "max_grade": "cached"}]);
        assert!(bind_sources(&def(&capped), &[source("third-party")]).is_ok());
        capped["inputs"][0]["evidence"]["graded"]["caps"] =
            json!([{"path": "*/total", "max_grade": "cached"}]);
        assert_eq!(
            category(bind_sources(&def(&capped), &[source("third-party")])),
            Category::SourceBinding,
            "capped path does not exist"
        );
        capped["inputs"][0]["evidence"]["graded"]["caps"] =
            json!([{"path": "*//price", "max_grade": "cached"}]);
        assert_eq!(
            category(bind_sources(&def(&capped), &[source("third-party")])),
            Category::SourceBinding,
            "a malformed cap path is not the root"
        );
    }

    fn graded_tree(caps: Value) -> Ceil {
        let v = json!({"schema": "rustev.source/1", "name": "live-quote", "version": "1.0.0",
            "facts": [{"name": "quotes", "provenance": "third-party", "grades": [
                {"path": "", "max_grade": "authoritative", "reliability": "reliable"},
                {"path": "*/price", "max_grade": "cached", "reliability": "reliable"}]}]});
        let d = SourceDescriptor::parse(v.to_string().as_bytes()).unwrap();
        let mut b = base();
        b["inputs"][0]["evidence"]["graded"]["caps"] = caps;
        let def = def(&b);
        let bound = bind_sources(&def, &[d]).unwrap();
        input_tree(&def.inputs[0].ty, bound.inputs.get("quotes"), None)
    }

    fn path(p: &str) -> Vec<String> {
        parse_path(p).unwrap()
    }

    #[test]
    fn raising_a_tree_keeps_each_ceiling_the_combination_of_its_parts() {
        let g = |x: Grade| ValueGrade::from(x);
        let merge_root = |fields: &[(&str, Grade)]| {
            let mut t = Ceil::leaf(ValueGrade::Ungraded, false);
            for (k, x) in fields {
                t.fields.insert(k.to_string(), Ceil::leaf(g(*x), false));
            }
            t.finish()
        };
        let consistent =
            |t: &Ceil| t.fields.values().fold(t.here, |own, c| own.combine(c.own)) == t.own;
        // Every field above `upgrade_to`: the whole keeps its own grade.
        let r = raised(&merge_root(&[("a", Grade::Authoritative)]), Grade::Cached);
        assert_eq!(r.own, g(Grade::Authoritative));
        assert!(consistent(&r));
        // A field below `upgrade_to` is raised to it, and the whole with it.
        let r = raised(
            &merge_root(&[("a", Grade::Authoritative), ("b", Grade::Hint)]),
            Grade::Cached,
        );
        assert_eq!(r.fields["b"].own, g(Grade::Cached));
        assert_eq!(r.own, g(Grade::Cached));
        assert!(consistent(&r));
        // An ungraded leaf takes `upgrade_to`.
        let r = raised(&Ceil::leaf(ValueGrade::Ungraded, false), Grade::Estimated);
        assert_eq!((r.here, r.own), (g(Grade::Estimated), g(Grade::Estimated)));
    }

    #[test]
    fn a_ceiling_is_the_lowest_declaration_on_the_path_capped() {
        let t = graded_tree(json!([{"path": "*/unit_price", "max_grade": "estimated"}]));
        assert_eq!(t.here, ValueGrade::from(Grade::Authoritative));
        assert_eq!(t.at(&path("*/price")).own, ValueGrade::from(Grade::Cached));
        assert_eq!(
            t.at(&path("*/unit_price")).own,
            ValueGrade::from(Grade::Estimated),
            "a cap lowers"
        );
        // A whole value is as weak as its weakest part.
        assert_eq!(t.own, ValueGrade::from(Grade::Estimated));
        // A cap above the source's declaration never raises it.
        let high = graded_tree(json!([{"path": "*/price", "max_grade": "authoritative"}]));
        assert_eq!(
            high.at(&path("*/price")).own,
            ValueGrade::from(Grade::Cached)
        );
        // An ungraded input is ungraded throughout.
        let def = def(&base());
        let ungraded = input_tree(&def.inputs[0].ty, None, None);
        assert_eq!(ungraded.own, ValueGrade::Ungraded);
        assert_eq!(ungraded.at(&path("*/price")).own, ValueGrade::Ungraded);
    }

    #[test]
    fn requirements_and_gates_are_judged_against_the_static_ceiling() {
        let read = |p: &str, require: Option<Grade>, gate: bool, mode: Mode| Read {
            base: "input:quotes".into(),
            path: path(p),
            mode,
            require: require.map(|min| GradeRequirement {
                min,
                max_age_ms: rustev_contract::definition::AgeLimit::NotRequired,
            }),
            gate,
        };
        let hint = graded_tree(json!([{"path": "*/price", "max_grade": "hint"}]));
        let trees = BTreeMap::from([("input:quotes".to_string(), hint.clone())]);
        let gate = read("*/price", None, true, Mode::Whole);
        let r = requirement_refusal("step:s", &gate, &trees).expect("refused");
        assert_eq!(r.category, Category::UncorroboratedHint);
        // Corroboration raises the hint: the gate's default `estimated` holds.
        let raised_trees =
            BTreeMap::from([("input:quotes".to_string(), raised(&hint, Grade::Cached))]);
        assert!(requirement_refusal("step:s", &gate, &raised_trees).is_none());
        // An explicit requirement above the ceiling is category 13.
        let strict = read(
            "*/unit_price",
            Some(Grade::Authoritative),
            false,
            Mode::Whole,
        );
        let trees = BTreeMap::from([("input:quotes".to_string(), graded_tree(json!([])))]);
        assert_eq!(
            requirement_refusal("step:s", &strict, &trees).map(|r| r.category),
            None,
            "unit_price inherits authoritative"
        );
        let strict = read("*/price", Some(Grade::Authoritative), false, Mode::Whole);
        assert_eq!(
            requirement_refusal("step:s", &strict, &trees).map(|r| r.category),
            Some(Category::GradeRequirement)
        );
        // Reading only the list node ignores its items' weaker fields.
        let node = read("", Some(Grade::Authoritative), false, Mode::Node);
        assert!(requirement_refusal("step:s", &node, &trees).is_none());
        let whole = read("", Some(Grade::Authoritative), false, Mode::Whole);
        assert!(requirement_refusal("step:s", &whole, &trees).is_some());
        // A plain read is never judged.
        let plain = read("*/price", None, false, Mode::Whole);
        assert!(requirement_refusal("step:s", &plain, &trees).is_none());
    }
}
