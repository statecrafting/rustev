//! Package-local synthetic generation fixtures.
use rustev_contract::definition::{Definition, Determinism, Operation};
use rustev_contract::descriptor::{
    BackendDescriptor, InputExcess, InputLimit, OperationSupport, OutputKind,
};
use rustev_contract::execution::{CostPolicy, ExecutionPolicy, StepExecution};
use rustev_contract::ids::ArtifactId;
use rustev_contract::schema;
use rustev_contract::snapshot::{Entry, Snapshot};
use rustev_contract::time::Timestamp;
use serde_json::json;

pub fn generation_definition() -> Definition {
    serde_json::from_value(json!({
        "schema": "rustev.definition/2",
        "name": "support-routing", "version": "1.0.0", "package": "rustev-reference",
        "inputs": [], "steps": [],
        "policy": {"on_unresolved": [], "rules": [], "adjustments": []},
        "limits": {"max_semantic_requests": 3, "on_excess": "refuse",
            "max_projection_bytes": 65536, "distribution_tolerance": "0.000001",
            "deadline_ms": 2000}
    }))
    .unwrap()
}

pub fn artifact(c: char) -> ArtifactId {
    ArtifactId::parse(&format!("sha256:{}", c.to_string().repeat(64))).unwrap()
}

pub fn ts(ms: i64) -> Timestamp {
    Timestamp::from_ms(ms).unwrap()
}

pub fn support(op: Operation, output: OutputKind, max: u64) -> OperationSupport {
    OperationSupport {
        isolation: None,
        screen: None,
        operation: op,
        output,
        max_options: max,
    }
}

pub fn linear_head() -> BackendDescriptor {
    BackendDescriptor {
        ranking: None,
        schema: schema::BACKEND.into(),
        backend_id: "synthetic-linear-head".into(),
        artifact: artifact('a'),
        operations: vec![
            support(Operation::Classify, OutputKind::Logits, 16),
            support(Operation::Proposition, OutputKind::Logits, 2),
            support(Operation::Rubric, OutputKind::Logits, 8),
        ],
        input_limit: InputLimit {
            max_bytes: 65_536,
            on_excess: InputExcess::Refuse,
        },
        determinism: Determinism::Tolerance,
    }
}

pub fn execution(cost: CostPolicy, steps: Vec<StepExecution>) -> ExecutionPolicy {
    ExecutionPolicy {
        schema: schema::EXECUTION.into(),
        max_attempts_per_decision: 1_000,
        cost,
        optimization: None,
        steps,
    }
}

pub fn snapshot(entries: Vec<Entry>) -> Snapshot {
    Snapshot {
        schema: schema::SNAPSHOT.into(),
        entries,
    }
}
