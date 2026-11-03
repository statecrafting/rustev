use super::*;
use rustev_contract::canonical::{canonical_value_bytes, tagged_digest};
use rustev_contract::definition::PolicyDecl;
use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::ContentDigest;
use rustev_contract::judgment::OutValue;
use rustev_eval::bundle::BundleInput;
use rustev_eval::config::{AdapterRules, EvaluatorConfig};
use rustev_eval::dataset::{AdapterRef, DatasetManifest};
use rustev_eval::metrics::TaskAdapter;
use rustev_eval::report::{Inputs, evaluate, evaluate_gate};
use serde::Deserialize;

/// Only the enum-parameter adapter subset used by the package. Parse the
/// package's actual documents and bind their canonical digest; refuse added
/// fields or a different selection mode instead of inventing adapter rules.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Adapter {
    schema: String,
    name: String,
    version: String,
    labels: Labels,
    rules: Vec<Rule>,
    #[serde(skip)]
    digest: Option<ContentDigest>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    prefixes: Vec<String>,
    values: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    action: String,
    map: BTreeMap<String, String>,
    otherwise: Option<String>,
    param: String,
    select: String,
}
impl Adapter {
    fn parse(bytes: &[u8]) -> Self {
        let value: Value = serde_json::from_slice(bytes).unwrap();
        let mut a: Self = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(a.schema, "rustev.task-adapter/1");
        assert_eq!(a.rules.len(), 1);
        assert_eq!(a.rules[0].select, "enum");
        a.digest = Some(
            ContentDigest::parse(&tagged_digest(
                &a.schema,
                &canonical_value_bytes(&value).unwrap(),
            ))
            .unwrap(),
        );
        a
    }
}
impl TaskAdapter for Adapter {
    fn adapter(&self) -> AdapterRef {
        AdapterRef {
            name: self.name.clone(),
            version: self.version.clone(),
        }
    }
    fn rules(&self) -> AdapterRules {
        AdapterRules::Bound(self.digest.clone().unwrap())
    }
    fn check_label(&self, label: &str) -> Result<(), String> {
        if self.labels.values.iter().any(|v| v == label)
            || self.labels.prefixes.iter().any(|p| label.starts_with(p))
        {
            Ok(())
        } else {
            Err(format!("unknown package label: {label}"))
        }
    }
    fn correct(&self, action: &str, params: &BTreeMap<String, OutValue>, label: &str) -> bool {
        let r = &self.rules[0];
        if r.action != action {
            return false;
        }
        let Some(OutValue::Enum(value)) = params.get(&r.param) else {
            return false;
        };
        r.map
            .get(value)
            .or(r.otherwise.as_ref())
            .is_some_and(|v| v == label)
    }
}

fn policy_bytes(policy: &PolicyDecl) -> Vec<u8> {
    canonical_value_bytes(&serde_json::to_value(policy).unwrap()).unwrap()
}

#[test]
fn support_routing_swap_preserves_policy_and_evaluates_both_captures() {
    let rules = RulesBackend::from_bytes(pkg::EXAMPLE_RULES).unwrap();
    let (rules_plan, rules_cal) = plan(rules.descriptor());
    let backend = embed(&fixtures::directory());
    let (embed_plan, embed_cal) = plan(backend.descriptor());
    backend.check_plan(&embed_plan.plan).unwrap();
    assert_ne!(rules_cal.id().unwrap(), embed_cal.id().unwrap());
    let (rp, ep) = (
        &rules_plan.plan.definition.policy,
        &embed_plan.plan.definition.policy,
    );
    // Byte equality includes every threshold and handler, with no normalization.
    assert_eq!(
        policy_bytes(rp),
        policy_bytes(ep),
        "compiled policy members differ"
    );
    let outputs = |p: &PolicyDecl| {
        p.rules
            .iter()
            .map(|r| serde_json::to_value(&r.then).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(outputs(rp), outputs(ep), "output declarations differ");
    let rules_cases = run_cases(Arc::new(rules), &rules_plan, &rules_cal);
    let embed_cases = run_cases(Arc::new(backend), &embed_plan, &embed_cal);
    let bundles = |cases: &BTreeMap<String, CapturedCase>| {
        cases
            .iter()
            .map(|(id, c)| (id.clone(), BundleInput::from(c.bundle.clone())))
            .collect::<BTreeMap<_, _>>()
    };
    let (rules_bundles, embed_bundles) = (bundles(&rules_cases), bundles(&embed_cases));
    for set in [pkg::QUEUE, pkg::PRIORITY] {
        let adapter = Adapter::parse(set.adapter);
        let config = EvaluatorConfig::parse(set.config).unwrap();
        let dataset = DatasetManifest::parse(set.dataset).unwrap();
        for split in [
            Split::Training,
            Split::ModelSelection,
            Split::Calibration,
            Split::FinalTest,
        ] {
            let report = |bundles| {
                evaluate(&Inputs {
                    manifest: &dataset,
                    split,
                    config: &config,
                    adapter: &adapter,
                    bundles,
                    resolver: &NoExternal,
                    replay: &replay_config(),
                    candidate: None,
                })
                .unwrap()
            };
            let baseline = report(&rules_bundles);
            let candidate = report(&embed_bundles);
            assert_eq!(baseline.report.dataset, candidate.report.dataset);
            assert_eq!(
                baseline.report.evaluator_config,
                candidate.report.evaluator_config
            );
            for r in [&baseline, &candidate] {
                assert!(matches!(
                    r.report.dataset.provenance,
                    DatasetProvenance::Synthetic { .. }
                ));
                assert_eq!(r.detail.cases.len(), 4);
                assert!(
                    r.detail
                        .cases
                        .iter()
                        .all(|c| c.outcome.starts_with("proposal:")
                            || c.outcome.starts_with("abstention:")),
                    "{} {split:?}: {:?}",
                    set.name,
                    r.detail.cases
                );
            }
            for gate in &config.gates {
                // Separate runtime captures are baseline-role reports. The gate
                // can say unknown; record its actual verdict, never assert a winner.
                let verdict = evaluate_gate(gate, &baseline, &candidate);
                println!(
                    "SYNTHETIC mechanics only: {} {split:?} gate {}: {verdict:?}",
                    set.name, gate.name
                );
            }
        }
    }
}
