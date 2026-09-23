//! Seam traits (spec 002, 3.1.2; design section 8), declared as signatures
//! over `std::future` only. Nothing in increment 1 calls them: the runtime
//! (spec 003) composes them. They are transport-neutral; network
//! implementations live under `integrations/` (spec 001, 3.4.3).

use std::future::Future;
use std::pin::Pin;

use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::eval_report::EvalReport;
use rustev_contract::evidence::EvidenceRecord;
use rustev_contract::judgment::Unresolved;
use rustev_contract::output::RawOutput;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;

/// A boxed future, so the traits stay object-safe without an executor.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Passed to every seam call. The deadline is a value, not a clock read.
#[derive(Debug, Clone)]
pub struct CallContext {
    pub deadline: Timestamp,
    pub trace_id: String,
    /// Opaque to Rustev: passed through for the host's authorization and
    /// cache isolation, never interpreted as authority (spec 001, 3.2).
    pub principal_handle: Vec<u8>,
}

/// Supplies an authorized context snapshot. Returns only what the caller may
/// read; a missing field is reported missing, never filled.
pub trait ContextSource: Send + Sync {
    fn fetch<'a>(
        &'a self,
        fields: &'a [String],
        cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<Snapshot, String>>;
}

/// Answers semantic requests. States its capabilities; never manufactures a
/// distribution or a calibration claim.
pub trait DecisionBackend: Send + Sync {
    fn descriptor(&self) -> &BackendDescriptor;
    fn infer<'a>(
        &'a self,
        projection: &'a [u8],
        cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<RawOutput, Unresolved>>;
}

/// A task adapter producing an evaluation report envelope (R-06).
pub trait Evaluator: Send + Sync {
    fn evaluate<'a>(&'a self, cx: &'a CallContext) -> BoxFuture<'a, EvalReport>;
}

/// Records evidence under a declared durability policy; loss is counted,
/// never silent (principle X).
pub trait EvidenceSink: Send + Sync {
    fn record<'a>(
        &'a self,
        record: EvidenceRecord,
        cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<String, String>>;
}
