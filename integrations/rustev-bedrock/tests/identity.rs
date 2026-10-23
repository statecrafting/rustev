use rustev_bedrock::binding::*;
use rustev_contract::{Document, Identified};
use serde_json::{Value, json};
fn binding() -> BedrockBinding {
    BedrockBinding::parse(include_bytes!("../fixtures/binding.json")).unwrap()
}
#[test]
fn version_one_binding_digest_is_fixed() {
    assert_eq!(
        binding().artifact().unwrap().as_str(),
        include_str!("../fixtures/binding.digest").trim()
    );
}
#[test]
fn all_output_affecting_members_enter_identity() {
    let b = binding();
    let original = b.artifact().unwrap();
    for change in 0..9 {
        let mut b = b.clone();
        change_binding(&mut b, change);
        assert_ne!(b.artifact().unwrap(), original, "{change}");
    }
}
fn change_binding(b: &mut BedrockBinding, change: usize) {
    match change {
        0 => {
            b.region = "eu-west-1".into();
            b.destination_regions = Destinations::Regions(RegionalDestinations {
                as_of: "2026-10-16".into(),
                regions: vec![b.region.clone()],
            });
            b.residency_policy
                .allowed_source_regions
                .push(b.region.clone());
            b.residency_policy.allowed_destination_regions = AllowedDestinations::Any(Any::Any);
        }
        1 => {
            b.model_id = "eu.anthropic.model".into();
            b.inference_profile = Profile::Geo;
            b.residency_policy.allowed_profile_kinds.push(Profile::Geo);
        }
        2 => {
            b.model_id = "eu.anthropic.model".into();
            b.inference_profile = Profile::Geo;
            b.residency_policy.allowed_profile_kinds.push(Profile::Geo);
            if let Destinations::Regions(d) = &mut b.destination_regions {
                d.regions.push("eu-west-1".into());
                b.residency_policy.allowed_destination_regions = AllowedDestinations::Any(Any::Any);
            }
        }
        3 => b
            .residency_policy
            .allowed_source_regions
            .push("eu-west-1".into()),
        4 => b.parameters.max_tokens += 1,
        5 => b.structured_output = StructuredOutput::None,
        6 => b.pricing.table_id = "changed-price-table".into(),
        7 => {
            b.model_id = "anthropic.other-model".into();
        }
        _ => {
            b.wire = Wire::Responses;
            b.provider_store = json!(false);
        }
    }
}
#[test]
fn existing_reference_plan_bytes_and_ids_remain_identical() {
    let pinned: Value =
        serde_json::from_slice(include_bytes!("../fixtures/reference-plan-ids.json")).unwrap();
    for (name, bytes, id) in [
        (
            "support-routing",
            include_bytes!("../../../crates/rustev-core/tests/golden/support-routing.plan.json")
                .as_slice(),
            include_str!("../../../crates/rustev-core/tests/golden/support-routing.plan-id"),
        ),
        (
            "lodging",
            include_bytes!("../../../crates/rustev-core/tests/golden/lodging.plan.json").as_slice(),
            include_str!("../../../crates/rustev-core/tests/golden/lodging.plan-id"),
        ),
    ] {
        let plan = rustev_contract::plan::Plan::parse(bytes).unwrap();
        assert_eq!(plan.canonical().unwrap(), bytes);
        assert_eq!(plan.id().unwrap().as_str(), id);
        assert_eq!(pinned[name], id);
    }
}

#[path = "../../../crates/rustev-core/tests/common/mod.rs"]
mod reference;
#[test]
fn reference_builders_still_compile_to_original_goldens() {
    let support = rustev_core::compile(
        &reference::support_routing(),
        &reference::descriptors(),
        &[reference::topic_calibration("1.5")],
    )
    .unwrap();
    let lodging =
        rustev_core::compile(&reference::lodging(), &reference::descriptors(), &[]).unwrap();
    assert_eq!(
        support.canonical(),
        include_bytes!("../../../crates/rustev-core/tests/golden/support-routing.plan.json")
    );
    assert_eq!(
        lodging.canonical(),
        include_bytes!("../../../crates/rustev-core/tests/golden/lodging.plan.json")
    );
}
#[test]
fn label_descriptor_never_satisfies_a_distribution_requirement() {
    use rustev_contract::definition::{
        BindingChoice, Determinism, Fallback, RequiredKind, StepBody,
    };
    let mut definition = reference::support_routing();
    for step in &mut definition.steps {
        if step.id == "explicit_deadline"
            && let StepBody::Semantic(s) = &mut step.body
        {
            s.backend.binding = BindingChoice::Backend("synthetic-label-only".into());
            s.backend.min_determinism = Determinism::Unspecified;
            s.requires = RequiredKind::Distribution;
            s.fallback = Fallback::None;
        }
    }
    assert!(
        rustev_core::compile(
            &definition,
            &reference::descriptors(),
            &[reference::topic_calibration("1.5")]
        )
        .is_err()
    );
}

struct NoCredentials;
impl rustev_bedrock::signing::CredentialProvider for NoCredentials {
    fn credentials<'a>(
        &'a self,
        _: rustev_bedrock::budget::Budget,
        _: &'a rustev_core::seams::CancelSignal,
    ) -> rustev_core::seams::BoxFuture<
        'a,
        Result<aws_credential_types::Credentials, rustev_bedrock::signing::CredentialError>,
    > {
        Box::pin(async { Err(rustev_bedrock::signing::CredentialError) })
    }
}
fn backend(binding: BedrockBinding) -> rustev_bedrock::BedrockBackend {
    rustev_bedrock::BedrockBackend::new(
        "identity-test".into(),
        binding,
        rustev_bedrock::policy::ModelPolicy::parse(include_bytes!("../fixtures/policy.json"))
            .unwrap(),
        serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap(),
        rustev_bedrock::adapter::Config {
            credentials: Some(std::sync::Arc::new(NoCredentials)),
            ..Default::default()
        },
    )
    .unwrap()
}
#[test]
fn binding_changes_propagate_to_descriptor_and_plan_but_policy_does_not() {
    use rustev_contract::definition::{Cond, Freshness, Operation, ProvenanceClass, RequiredKind};
    use rustev_core::builder::{DefinitionBuilder, out, ty};
    use rustev_core::seams::DecisionBackend;
    let definition = DefinitionBuilder::new("binding-identity", "1.0.0", "synthetic")
        .input(
            "text",
            ty::text(128),
            &[ProvenanceClass::UserSupplied],
            Freshness::NotRequired,
        )
        .semantic(
            "choice",
            reference::semantic(
                Operation::Classify,
                "synthetic",
                "Choose",
                &["a", "b"],
                RequiredKind::Label,
                &["input:text"],
            ),
        )
        .rule("result", Cond::Always, out::propose("synthetic", &[]))
        .limits(reference::limits(1))
        .build()
        .unwrap();
    let original = backend(binding());
    let plan = rustev_core::compile(&definition, &[original.descriptor().clone()], &[]).unwrap();
    let snapshot = reference::snapshot(vec![reference::entry(
        "text",
        json!("synthetic"),
        ProvenanceClass::UserSupplied,
        reference::NOW,
    )]);
    let evaluation =
        rustev_core::evaluate::Evaluation::start(&plan, &snapshot, reference::ts(reference::NOW))
            .unwrap();
    let pending = evaluation.pending();
    assert_eq!(pending.len(), 1);
    assert!(matches!(
        evaluation
            .check_output(
                &pending[0].step,
                &pending[0].instance,
                &rustev_contract::output::RawOutput::Label("outside".into())
            )
            .unwrap(),
        Err(rustev_contract::judgment::Unresolved::InvalidBackendOutput { .. })
    ));
    for change in 0..9 {
        let mut changed = binding();
        change_binding(&mut changed, change);
        let changed = backend(changed);
        assert_ne!(
            original.descriptor().id().unwrap(),
            changed.descriptor().id().unwrap()
        );
        let other =
            rustev_core::compile(&definition, &[changed.descriptor().clone()], &[]).unwrap();
        assert_ne!(plan.canonical(), other.canonical());
        assert_ne!(plan.id, other.id);
        original.check_plan(&plan.plan).unwrap();
        assert!(changed.check_plan(&plan.plan).is_err());
    }
    // Credentials and journal locations are host state, not descriptor inputs.
    // Use distinct providers and actual journal paths, then compare compiled plans.
    for suffix in ["first", "second"] {
        let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("identity-{suffix}-{}.jsonl", std::process::id()));
        let journal = rustev_bedrock::spend::SpendJournal::open(
            &path,
            rustev_bedrock::spend::BudgetKind::Testing,
            std::sync::Arc::new(rustev_bedrock::spend::SetClock::new(0)),
        )
        .unwrap();
        let other = rustev_bedrock::BedrockBackend::new(
            "identity-test".into(),
            binding(),
            rustev_bedrock::policy::ModelPolicy::parse(include_bytes!("../fixtures/policy.json"))
                .unwrap(),
            serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap(),
            rustev_bedrock::adapter::Config {
                credentials: Some(std::sync::Arc::new(StaticCredentials(suffix))),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(journal.path(), path);
        assert_eq!(
            original.descriptor().id().unwrap(),
            other.descriptor().id().unwrap()
        );
        let same = rustev_core::compile(&definition, &[other.descriptor().clone()], &[]).unwrap();
        assert_eq!(plan.id, same.id);
        assert_eq!(plan.canonical(), same.canonical());
        std::fs::remove_file(path).unwrap();
    }
    let mut policy =
        rustev_bedrock::policy::ModelPolicy::parse(include_bytes!("../fixtures/policy.json"))
            .unwrap();
    policy.allow[0].model = original.binding().model_id.clone();
    original.replace_policy(policy).unwrap();
    let same = rustev_core::compile(&definition, &[original.descriptor().clone()], &[]).unwrap();
    assert_eq!(plan.canonical(), same.canonical());
    let mut distribution = definition;
    if let rustev_contract::definition::StepBody::Semantic(s) = &mut distribution.steps[0].body {
        s.requires = RequiredKind::Distribution;
    }
    assert!(rustev_core::compile(&distribution, &[original.descriptor().clone()], &[]).is_err());
}

struct StaticCredentials(&'static str);
impl rustev_bedrock::signing::CredentialProvider for StaticCredentials {
    fn credentials<'a>(
        &'a self,
        _: rustev_bedrock::budget::Budget,
        _: &'a rustev_core::seams::CancelSignal,
    ) -> rustev_core::seams::BoxFuture<
        'a,
        Result<aws_credential_types::Credentials, rustev_bedrock::signing::CredentialError>,
    > {
        Box::pin(async move {
            Ok(aws_credential_types::Credentials::new(
                self.0,
                self.0,
                Some(self.0.into()),
                None,
                "synthetic",
            ))
        })
    }
}
