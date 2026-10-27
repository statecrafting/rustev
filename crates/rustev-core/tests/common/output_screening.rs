//! Synthetic output-screening capability shared by the three generation suites.
use rustev_contract::definition::{BindingChoice, Definition, Operation, StepBody};
use rustev_contract::descriptor::{BackendDescriptor, OperationSupport, OutputKind, ScreenProfile};
use rustev_contract::generation::ScreenOutput;

pub fn configure(d: &mut Definition, b: &mut BackendDescriptor) {
    let StepBody::Generate(g) = &mut d.steps[0].body else {
        panic!()
    };
    let mut backend = g.backend.clone();
    backend.binding = BindingChoice::Backend(b.backend_id.clone());
    g.screen.output = ScreenOutput::Screener { backend };
    d.limits.max_semantic_requests = 4;
    b.operations.push(OperationSupport {
        operation: Operation::Proposition,
        output: OutputKind::Screen,
        max_options: 2,
        isolation: None,
        screen: Some(ScreenProfile {
            policy_version: "synthetic/1".into(),
            categories: vec!["injection".into()],
            max_text_bytes: 400,
            text_unit_bytes: 16,
        }),
    });
}
