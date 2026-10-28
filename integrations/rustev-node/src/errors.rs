//! The error mapping of spec 027, 3.4.1: every refusal and failure crosses
//! as a JavaScript `Error` whose message is `{ class, code, detail }` JSON;
//! the JavaScript layer throws the exported `RustevError` subclass. A panic
//! never crosses (3.4.3).

use napi::{Error, Result, Status};
use rustev_core::Refusal;
use rustev_runtime::{ConfigError, PrepareError};

/// A typed failure for the JavaScript layer.
pub fn fail(class: &str, code: &str, detail: impl Into<serde_json::Value>) -> Error {
    let msg = serde_json::json!({ "class": class, "code": code, "detail": detail.into() });
    Error::new(Status::GenericFailure, msg.to_string())
}

/// Run `f`, turning a panic into `InternalError` (3.4.3).
pub fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            Err(fail("InternalError", "internal", msg))
        }
    }
}

/// A compile refusal: its category name as the code (3.4.1).
pub fn refusal(r: Refusal) -> Error {
    fail(
        "CompileRefusal",
        rustev_surface::plan::category_name(r.category),
        serde_json::json!({
            "category": r.category as u8,
            "subject": r.subject,
            "detail": r.detail,
        }),
    )
}

pub fn config_error(e: ConfigError) -> Error {
    match e {
        ConfigError::InvalidSource(source) => fail(
            "ConfigRefused",
            "invalid_source",
            serde_json::json!({ "source": source }),
        ),
        ConfigError::Zero(field) => fail(
            "ConfigRefused",
            "zero_bound",
            serde_json::json!({ "field": field }),
        ),
        ConfigError::DuplicateBackend(id) => fail(
            "ConfigRefused",
            "duplicate_backend",
            serde_json::json!({ "backendId": id }),
        ),
    }
}

pub fn prepare_error(e: PrepareError) -> Error {
    match e {
        PrepareError::SourceBinding(source) => fail(
            "PlanPrepareRefused",
            "source_binding",
            serde_json::json!({ "source": source }),
        ),
        PrepareError::MissingBackend(id) => fail(
            "PlanPrepareRefused",
            "missing_backend",
            serde_json::json!({ "backendId": id }),
        ),
        PrepareError::DescriptorMismatch { backend_id } => fail(
            "PlanPrepareRefused",
            "descriptor_mismatch",
            serde_json::json!({ "backendId": backend_id }),
        ),
        PrepareError::HardBudgetUnenforceable { backend_id, model } => fail(
            "PlanPrepareRefused",
            "hard_budget_unenforceable",
            serde_json::json!({ "backendId": backend_id, "model": model }),
        ),
        PrepareError::SinkVersionUnsupported { schema } => fail(
            "PlanPrepareRefused",
            "sink_version_unsupported",
            serde_json::json!({ "schema": schema }),
        ),
        // Spec 044, 3.4.1: the binding installs no store (its exposure is
        // out of that spec's scope).
        PrepareError::StoreMissing => {
            fail("PlanPrepareRefused", "store_missing", serde_json::json!({}))
        }
        PrepareError::StoreContractMismatch => fail(
            "PlanPrepareRefused",
            "store_contract_mismatch",
            serde_json::json!({}),
        ),
    }
}
