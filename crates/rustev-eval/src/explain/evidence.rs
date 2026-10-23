use super::*;
use crate::resolve::{Budget, ItemFailure, check_bytes, item_bytes};
use rustev_contract::retention::{Availability as Retained, RetentionItem};
use rustev_contract::{Document, Identified};
use rustev_contract::{
    plan::Plan,
    replay::{ItemLocation, ReplayBundle},
    run::RunRecord,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) struct Set {
    pub docs: BTreeMap<String, Value>,
    pub absent: BTreeMap<String, Unavailable>,
    pub side: Side,
    pub plan: Option<Plan>,
}
fn value<T: Document + PartialEq>(doc: &T, name: &str) -> Result<Value, ExplainError> {
    let bytes = doc
        .record_canonical()
        .map_err(|e| ExplainError::parse(name, e))?;
    serde_json::from_slice(&bytes).map_err(|e| ExplainError::parse(name, e))
}
impl Set {
    pub fn read(input: &Evidence<'_>) -> Result<Self, ExplainError> {
        let mut docs = BTreeMap::new();
        let mut absent = BTreeMap::new();
        let (record, plan, mut side) = match input {
            Evidence::Record { record, plan } => {
                let r = RunRecord::parse(record).map_err(|e| ExplainError::parse("record", e))?;
                let p = plan
                    .map(Plan::parse)
                    .transpose()
                    .map_err(|e| ExplainError::parse("plan", e))?;
                let side = Side {
                    name: SideName::A,
                    form: EvidenceForm::Record,
                    record: DigestRef::Digest(DigestValue {
                        digest: r
                            .record_digest()
                            .map_err(|e| ExplainError::parse("record", e))?,
                    }),
                    plan: None,
                    bundle: None,
                    capture: None,
                    decision_id: Item::String(r.decision_id.clone()),
                    plan_id: Item::String(r.plan_id.to_string()),
                };
                (Some(r), p, side)
            }
            Evidence::Bundle {
                bundle,
                scope,
                now_ms,
                resolver,
            } => {
                let b =
                    ReplayBundle::parse(bundle).map_err(|e| ExplainError::parse("bundle", e))?;
                if &b.scope != *scope {
                    return Err(ExplainError::new(
                        RefusalCode::ScopeMismatch,
                        "bundle",
                        "scope differs",
                    ));
                }
                b.check(None)
                    .map_err(|e| ExplainError::parse("bundle", e))?;
                docs.insert("bundle".into(), value(&b, "bundle")?);
                let mut budget = Budget::default();
                let mut record = None;
                let mut plan = None;
                for (loc, item) in b.items() {
                    let (name, pointer) = match loc {
                        ItemLocation::Plan => ("plan".into(), "/plan".into()),
                        ItemLocation::Run => ("record".into(), "/run".into()),
                        ItemLocation::Snapshot => ("snapshot".into(), "/snapshot".into()),
                        ItemLocation::Supply(i) => {
                            (format!("supply/{i}"), format!("/supplies/{i}/value"))
                        }
                        ItemLocation::Descriptor(i) => {
                            (format!("descriptor/{i}"), format!("/descriptors/{i}"))
                        }
                        ItemLocation::Calibration(i) => {
                            (format!("calibration/{i}"), format!("/calibrations/{i}"))
                        }
                        ItemLocation::Bundle => continue,
                    };
                    let result =
                        item_bytes(item, *now_ms, *resolver, &mut budget).and_then(|bytes| {
                            macro_rules! read {
                                ($t:ty) => {
                                    check_bytes::<$t>(item, &bytes).map(|d| (d, bytes))
                                };
                            }
                            // Every item is checked, including items this query does not use.
                            match loc {
                                ItemLocation::Plan => read!(Plan).map(|(p, _)| {
                                    plan = Some(p);
                                    None
                                }),
                                ItemLocation::Run => read!(RunRecord).map(|(r, _)| {
                                    record = Some(r);
                                    None
                                }),
                                ItemLocation::Snapshot => {
                                    read!(rustev_contract::snapshot::Snapshot)
                                        .map(|(_, bytes)| Some(bytes))
                                }
                                ItemLocation::Descriptor(_) => {
                                    read!(rustev_contract::descriptor::BackendDescriptor)
                                        .map(|_| None)
                                }
                                ItemLocation::Calibration(_) => {
                                    read!(rustev_contract::calibration::CalibrationArtifact)
                                        .map(|_| None)
                                }
                                _ if item.document_schema
                                    == rustev_contract::schema::BACKEND_OUTPUT =>
                                {
                                    read!(rustev_contract::output::BackendOutputDoc)
                                        .map(|(_, bytes)| Some(bytes))
                                }
                                _ => read!(rustev_contract::reason::RuntimeReasonDoc)
                                    .map(|(_, bytes)| Some(bytes)),
                            }
                        });
                    match result {
                        Ok(Some(bytes)) => {
                            docs.insert(
                                name.clone(),
                                serde_json::from_slice(&bytes)
                                    .map_err(|e| ExplainError::parse(&name, e))?,
                            );
                        }
                        Ok(None) => {}
                        Err(f) => {
                            absent.insert(name.clone(), unavailable(item, &pointer, &name, f)?);
                        }
                    }
                }
                if let Some(r) = &record
                    && (r.plan_id != b.plan_id || r.decision_id != b.decision_id)
                {
                    return Err(ExplainError::new(
                        RefusalCode::EvidenceIntegrity,
                        "record",
                        "bundle identity differs",
                    ));
                }

                if let Some(p) = &plan
                    && p.id().map_err(|e| ExplainError::parse("plan", e))? != b.plan_id
                {
                    return Err(ExplainError::new(
                        RefusalCode::PlanMismatch,
                        "plan",
                        "bundle plan differs",
                    ));
                }

                let id = |s: String| {
                    absent.get("record").map_or(Item::String(s), |u| {
                        Item::Withheld(Withheld {
                            not_available: u.clone(),
                        })
                    })
                };
                let side = Side {
                    name: SideName::A,
                    form: EvidenceForm::Bundle,
                    record: DigestRef::Digest(DigestValue {
                        digest: b.run.retention.digest().clone(),
                    }),
                    plan: None,
                    bundle: Some(
                        b.record_digest()
                            .map_err(|e| ExplainError::parse("bundle", e))?,
                    ),
                    capture: Some(b.capture),
                    decision_id: id(b.decision_id.clone()),
                    plan_id: id(b.plan_id.to_string()),
                };
                (record, plan, side)
            }
        };
        if let Some(p) = &plan {
            let id = p.id().map_err(|e| ExplainError::parse("plan", e))?;
            if record.as_ref().is_some_and(|r| r.plan_id != id) {
                return Err(ExplainError::new(
                    RefusalCode::PlanMismatch,
                    "plan",
                    "record plan differs",
                ));
            }
            side.plan = Some(id);
            docs.insert("plan".into(), value(p, "plan")?);
        } else {
            absent
                .entry("plan".into())
                .or_insert(Unavailable::PlanNotSupplied {});
        }
        if let Some(r) = record {
            docs.insert("record".into(), value(&r, "record")?);
        }
        Ok(Self {
            docs,
            absent,
            side,
            plan,
        })
    }
    pub fn get(&self, role: &str, pointer: &str) -> Option<&Value> {
        self.docs.get(role)?.pointer(pointer)
    }
    pub fn judgment_absent(&self) -> Option<Unavailable> {
        self.absent.get("record").cloned().or_else(|| {
            (self.get("record", "/core").and_then(Value::as_str) == Some("not_produced"))
                .then_some(Unavailable::NotProduced {})
        })
    }
}
fn unavailable(
    item: &RetentionItem,
    pointer: &str,
    name: &str,
    failure: ItemFailure,
) -> Result<Unavailable, ExplainError> {
    let availability = match failure {
        ItemFailure::Unavailable(Retained::Missing) if item.retention.mode() == "digest_only" => {
            ItemAvailability::DigestOnly
        }
        ItemFailure::Unavailable(Retained::Missing) => ItemAvailability::Missing,
        ItemFailure::Unavailable(Retained::Expired) => ItemAvailability::Expired,
        ItemFailure::Unavailable(Retained::Erased) => ItemAvailability::Erased,
        ItemFailure::Unavailable(Retained::Inaccessible) => ItemAvailability::Inaccessible,
        ItemFailure::Oversized => {
            return Err(ExplainError::parse(
                name,
                "resolved item exceeds document or bundle limits",
            ));
        }
        _ => {
            return Err(ExplainError::new(
                RefusalCode::EvidenceIntegrity,
                name,
                format!("{failure:?}"),
            ));
        }
    };
    let path = format!(
        "bundle:{pointer}/retention/{}/digest",
        item.retention.mode()
    );
    Ok(Unavailable::Retention {
        item: EvidencePath::parse(&path).map_err(|e| ExplainError::parse(name, e))?,
        availability,
    })
}
