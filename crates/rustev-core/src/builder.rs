//! The typed Rust builder (R-02). It produces the same
//! [`Definition`] value the JSON parser produces, and both go through one
//! compiler, so there is one source of behavior. Operator arguments are the
//! typed [`OpArgs`] structs; nothing is defaulted: every step, input and
//! policy item is stated in full.

use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{
    Adjustment, Cond, Definition, Expr, FieldDecl, Freshness, Handler, HandlerAction, InputDecl,
    LimitsDecl, Literal, OutcomeDecl, ParamDecl, ParamSource, PolicyDecl, Predicate,
    ProvenanceClass, ReasonSet, Rule, SemanticDecl, StepBody, StepDecl, TypeDecl,
};
use rustev_contract::definition::{CmpOp, ExactDecl};
use rustev_contract::schema;

use crate::ops::OpArgs;

/// Builds a definition. `build` requires limits and at least one rule.
#[derive(Debug, Clone)]
pub struct DefinitionBuilder {
    name: String,
    version: String,
    package: String,
    inputs: Vec<InputDecl>,
    steps: Vec<StepDecl>,
    policy: PolicyDecl,
    limits: Option<LimitsDecl>,
}

/// `build` was called without limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingLimits;

impl DefinitionBuilder {
    pub fn new(name: &str, version: &str, package: &str) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            package: package.into(),
            inputs: vec![],
            steps: vec![],
            policy: PolicyDecl {
                on_unresolved: vec![],
                rules: vec![],
                adjustments: vec![],
            },
            limits: None,
        }
    }

    pub fn input(
        mut self,
        name: &str,
        ty: TypeDecl,
        provenance: &[ProvenanceClass],
        freshness: Freshness,
    ) -> Self {
        self.inputs.push(InputDecl {
            name: name.into(),
            ty,
            provenance: provenance.to_vec(),
            freshness,
        });
        self
    }

    pub fn exact(mut self, id: &str, args: OpArgs) -> Self {
        let (op, version) = args.op();
        self.steps.push(StepDecl {
            id: id.into(),
            body: StepBody::Exact(ExactDecl {
                op: op.into(),
                version,
                args: args.to_json(),
            }),
        });
        self
    }

    pub fn semantic(mut self, id: &str, decl: SemanticDecl) -> Self {
        self.steps.push(StepDecl {
            id: id.into(),
            body: StepBody::Semantic(decl),
        });
        self
    }

    pub fn on_unresolved(
        mut self,
        target: &str,
        reasons: ReasonSet,
        action: HandlerAction,
    ) -> Self {
        self.policy.on_unresolved.push(Handler {
            target: target.into(),
            reasons,
            action,
        });
        self
    }

    pub fn rule(mut self, id: &str, when: Cond, then: OutcomeDecl) -> Self {
        self.policy.rules.push(Rule {
            id: id.into(),
            when,
            then,
        });
        self
    }

    pub fn adjustment(mut self, id: &str, when: Cond, param: &str, scale: &[&str]) -> Self {
        self.policy.adjustments.push(Adjustment {
            id: id.into(),
            when,
            param: param.into(),
            scale: scale.iter().map(|s| s.to_string()).collect(),
        });
        self
    }

    pub fn limits(mut self, limits: LimitsDecl) -> Self {
        self.limits = Some(limits);
        self
    }

    pub fn build(self) -> Result<Definition, MissingLimits> {
        Ok(Definition {
            schema: schema::DEFINITION.into(),
            name: self.name,
            version: self.version,
            package: self.package,
            inputs: self.inputs,
            steps: self.steps,
            policy: self.policy,
            limits: self.limits.ok_or(MissingLimits)?,
        })
    }
}

/// Type helpers.
pub mod ty {
    use super::*;

    pub fn text(max_bytes: u64) -> TypeDecl {
        TypeDecl::Text { max_bytes }
    }
    pub fn enumeration(options: &[&str]) -> TypeDecl {
        TypeDecl::Enum {
            options: options.iter().map(|s| s.to_string()).collect(),
        }
    }
    pub fn list(item: TypeDecl, max_items: u64) -> TypeDecl {
        TypeDecl::List {
            item: Box::new(item),
            max_items,
        }
    }
    pub fn record(fields: &[(&str, TypeDecl)]) -> TypeDecl {
        TypeDecl::Record {
            fields: fields
                .iter()
                .map(|(n, t)| FieldDecl {
                    name: n.to_string(),
                    ty: t.clone(),
                })
                .collect(),
        }
    }
}

/// Expression and predicate helpers.
pub mod e {
    use super::*;

    pub fn r(reference: &str) -> Expr {
        Expr::Ref(reference.into())
    }
    pub fn int(v: i64) -> Expr {
        Expr::Lit(Literal::Integer(v))
    }
    /// A decimal literal; panics on an invalid literal, so use it with
    /// constants only.
    pub fn dec(v: &str) -> Expr {
        Expr::Lit(Literal::Decimal(decimal(v)))
    }
    pub fn enum_lit(v: &str) -> Expr {
        Expr::Lit(Literal::Enum(v.into()))
    }
    pub fn text(v: &str) -> Expr {
        Expr::Lit(Literal::Text(v.into()))
    }
    pub fn boolean(v: bool) -> Expr {
        Expr::Lit(Literal::Bool(v))
    }
    pub fn duration_ms(v: u64) -> Expr {
        Expr::Lit(Literal::DurationMs(v))
    }
    pub fn sub(a: Expr, b: Expr) -> Expr {
        Expr::Sub {
            left: Box::new(a),
            right: Box::new(b),
        }
    }
    pub fn fx(amount: Expr, from: Expr, to: Expr, rates: &str, scale: u32) -> Expr {
        Expr::Fx {
            amount: Box::new(amount),
            from: Box::new(from),
            to: Box::new(to),
            rates: rates.into(),
            scale,
        }
    }
    pub fn cmp(a: Expr, op: CmpOp, b: Expr) -> Predicate {
        Predicate::Cmp {
            left: a,
            op,
            right: b,
        }
    }
    pub fn all(ps: Vec<Predicate>) -> Predicate {
        Predicate::All(ps)
    }
    pub fn any(ps: Vec<Predicate>) -> Predicate {
        Predicate::Any(ps)
    }
    pub fn not(p: Predicate) -> Predicate {
        Predicate::Not(Box::new(p))
    }
    /// A decimal constant; panics on an invalid literal.
    pub fn decimal(v: &str) -> Decimal {
        Decimal::parse(v).expect("decimal literal")
    }
}

/// Outcome helpers.
pub mod out {
    use super::*;

    pub fn propose(action: &str, params: &[(&str, ParamSource)]) -> OutcomeDecl {
        OutcomeDecl::Propose {
            action: action.into(),
            params: params
                .iter()
                .map(|(n, v)| ParamDecl {
                    name: n.to_string(),
                    value: v.clone(),
                })
                .collect(),
        }
    }
    pub fn lit_enum(v: &str) -> ParamSource {
        ParamSource::Lit(Literal::Enum(v.into()))
    }
    pub fn from(reference: &str) -> ParamSource {
        ParamSource::Ref(reference.into())
    }
    pub fn escalate(reason: &str) -> OutcomeDecl {
        OutcomeDecl::Escalate {
            reason: reason.into(),
        }
    }
}
