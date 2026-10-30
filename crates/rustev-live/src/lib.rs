//! Persistent accounting for live decision execution (spec 043).

pub mod documents;
pub mod gate;
pub mod journal;
pub mod probe;
pub use gate::JournalGate;
mod reports;
pub mod run;
pub use run::{
    CaseInput, LiveCase, PreparedRun, ReportInputs, RunOptions, RunOutcome, RunSpec, RunStatus,
    prepare,
};

/// Test-only crash injection behind the non-default `fault-injection`
/// feature (spec 043 increment 9). Not supported API: it may change or
/// disappear in any release.
#[cfg(feature = "fault-injection")]
#[doc(hidden)]
pub mod fault_injection;
