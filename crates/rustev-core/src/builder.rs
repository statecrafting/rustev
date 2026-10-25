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
            evidence: None,
            on_disagreement: None,
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
            actionability: None,
            screening: None,
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
            present_when: None,
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

/// Adds spec 034 declarations and reference sites to a typed definition builder.
/// `build` validates stages 1 and 2 using the same path as parsed JSON.
#[cfg(feature = "parameterized")]
#[derive(Debug, Clone)]
pub struct TemplateBuilder {
    document: serde_json::Value,
    error: Option<String>,
}
#[cfg(feature = "parameterized")]
impl TemplateBuilder {
    pub fn new(builder: DefinitionBuilder) -> Result<Self, MissingLimits> {
        Ok(Self::from_definition(builder.build()?))
    }
    pub fn from_definition(definition: Definition) -> Self {
        let mut document = serde_json::to_value(definition).expect("definition is serializable");
        document["schema"] = serde_json::json!(schema::DEFINITION_V2);
        Self {
            document,
            error: None,
        }
    }
    pub fn parameter(mut self, declaration: rustev_contract::parameter::ParameterDecl) -> Self {
        let array = self
            .document
            .as_object_mut()
            .expect("definition object")
            .entry("parameters")
            .or_insert_with(|| serde_json::json!([]));
        array
            .as_array_mut()
            .expect("parameter array")
            .push(serde_json::json!(declaration));
        self
    }
    pub fn variants(mut self, names: &[&str]) -> Self {
        self.document["variants"] = serde_json::json!(names);
        self
    }
    pub fn only_in(self, item_pointer: &str, names: &[&str]) -> Self {
        self.item_member(item_pointer, "only_in", serde_json::json!(names))
    }
    pub fn enabled(self, item_pointer: &str, enabled: rustev_contract::parameter::Enabled) -> Self {
        self.item_member(item_pointer, "enabled", serde_json::json!(enabled))
    }
    fn item_member(mut self, pointer: &str, key: &str, value: serde_json::Value) -> Self {
        if let Some(item) = self
            .document
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
        {
            item.insert(key.into(), value);
        } else if self.error.is_none() {
            self.error = Some(format!("no item at {pointer}"));
        }
        self
    }
    /// Replace an existing JSON Pointer site with a parameter reference.
    pub fn reference(mut self, pointer: &str, parameter: &str) -> Self {
        if let Some(site) = self.document.pointer_mut(pointer) {
            *site = serde_json::json!({"param": parameter});
        } else if self.error.is_none() {
            self.error = Some(format!("no reference site at {pointer}"));
        }
        self
    }
    pub fn build(self) -> Result<Vec<u8>, crate::instantiate::InstantiationRefusal> {
        use crate::instantiate::{
            InstantiationCode, InstantiationRefusal, check_declarations, parse_template,
        };
        let invalid = |detail: String| InstantiationRefusal {
            stage: 1,
            code: InstantiationCode::InvalidDefinitionDocument,
            subject: format!(
                "definition:{}@{}",
                self.document["name"].as_str().unwrap_or("?"),
                self.document["version"].as_str().unwrap_or("?")
            ),
            detail,
        };
        if let Some(e) = self.error {
            return Err(invalid(e));
        }
        let bytes = rustev_contract::canonical::canonical_bytes(&self.document)
            .map_err(|e| invalid(e.to_string()))?;
        let template = parse_template(&bytes)?;
        check_declarations(&template)?;
        Ok(template.canonical)
    }
}
